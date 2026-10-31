//! The swarm loop: run the engine as a persistent daemon that sleeps at ~0% CPU
//! and wakes to process new field appends until told to shut down.

use std::time::Duration;

use hadron_lattice::term::{self, Source};
use tokio::sync::watch;

use crate::engine::Engine;
use crate::watch::FieldWatcher;

/// How often the daemon re-checks the field even if notify never fires.
/// notify (via [`FieldWatcher`]) wakes it sooner; this is the liveness floor.
pub const SAFETY_POLL: Duration = Duration::from_millis(500);

/// Safe-point lifecycle maintenance hook executed by [`Engine::serve_with_hooks`]
/// after every quiescence pass while the swarm holds no active turns.
pub trait DaemonHook: Send {
    fn on_safe_point<'a>(
        &'a mut self,
        engine: &'a mut Engine,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>;
}

struct NoopDaemonHook;
impl DaemonHook for NoopDaemonHook {
    fn on_safe_point<'a>(
        &'a mut self,
        _engine: &'a mut Engine,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

impl Engine {
    /// Run as a persistent daemon: process the field to quiescence, then sleep
    /// until the field changes (notify) or `SAFETY_POLL` elapses, then process
    /// again. Returns when `shutdown` observes `true`.
    ///
    /// 0-CPU class: the process is asleep in `select!` between wakes. notify is
    /// a latency optimization; the safety-poll arm is the liveness guarantee, so
    /// the daemon still progresses where inotify never fires (WSL2 `/mnt/c`) and
    /// never busy-spins if the watcher bridge dies.
    pub async fn serve(&mut self, shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        let mut noop = NoopDaemonHook;
        self.serve_with_hooks(shutdown, SAFETY_POLL, &mut noop).await
    }

    /// Run as a persistent daemon with a configurable safety-poll interval and safe-point
    /// maintenance hook executed after each quiescence pass.
    pub async fn serve_with_hooks<H: DaemonHook + ?Sized>(
        &mut self,
        mut shutdown: watch::Receiver<bool>,
        safety_poll: Duration,
        hook: &mut H,
    ) -> anyhow::Result<()> {
        // Bridge the blocking notify watcher into an async change signal. A
        // bounded(1) channel coalesces bursts: one pending "something changed"
        // is all the loop needs.
        let (change_tx, mut change_rx) = tokio::sync::mpsc::channel::<()>(1);
        let field_path = self.field_path().to_path_buf();
        let bridge_shutdown = shutdown.clone();
        let bridge_change_tx = change_tx.clone();
        let bridge = tokio::task::spawn_blocking(move || {
            // If the watcher can't be created (e.g. dir missing), drop the
            // sender and let the daemon fall back to pure safety-polling.
            let watcher = match FieldWatcher::new(&field_path) {
                Ok(w) => w,
                Err(_) => return,
            };
            while !*bridge_shutdown.borrow() {
                if watcher.wait(Duration::from_millis(200)) {
                    let _ = bridge_change_tx.try_send(());
                }
            }
        });

        let hadron_dir = hadron_lattice::hadron_dir_of(self.field_path());
        let bus = crate::actor::bus::ActorBus::new(64);
        let mut _bus_rxs = Vec::new();
        for card in self.roster() {
            if let Ok((_mailbox, rx)) = bus.register_quark(card.id.as_str()).await {
                _bus_rxs.push(rx);
            }
        }
        let dispatcher = std::sync::Arc::new(
            crate::rpc_server::RpcDispatcher::new(bus)
                .with_field_and_wake(self.field_path().to_path_buf(), change_tx.clone()),
        );
        let rpc_server_task = match crate::rpc_server::RpcServer::bind_and_serve(&hadron_dir, dispatcher, shutdown.clone()).await {
            Ok((addr, handle)) => {
                term::info(Source::Gluon, &format!("serving JSON-RPC on {addr} ({})", hadron_dir.display()));
                Some(handle)
            }
            Err(e) => {
                term::warn(Source::Gluon, &format!("unable to bind JSON-RPC server (continuing): {e:#}"));
                None
            }
        };

        // Once the change channel closes (bridge exited / watcher failed) we
        // stop selecting on it so a ready-`None` can't starve the sleep arm and
        // spin the loop. The safety-poll keeps the daemon live regardless.
        let mut change_open = true;
        loop {
            if *shutdown.borrow() {
                break;
            }

            let before = crate::field::read_events(self.field_path()).map(|e| e.len()).unwrap_or(0);
            if let Err(e) = self.run_until_quiesce().await {
                term::error(Source::Gluon, &format!("excite error in daemon (continuing): {e:#}"));
                let events_after = crate::field::read_events(self.field_path()).map(|ev| ev.len()).unwrap_or(0);
                if events_after == before {
                    let orch_exists = self.roster().iter().any(|c| c.flavor == hadron_lattice::Flavor::Orchestrator);
                    let body = if orch_exists {
                        format!("@{} Gluon excite error: {e:#}", crate::router::ORCHESTRATOR_ALIAS)
                    } else {
                        format!("Gluon excite error: {e:#}")
                    };
                    let _ = self
                        .append(
                            hadron_lattice::Event::new(
                                hadron_lattice::Actor::Gluon,
                                None,
                                hadron_lattice::Kind::Message { body },
                            )
                            .with_severity(hadron_lattice::Severity::Error),
                        )
                        .await;
                }
            }
            let after = crate::field::read_events(self.field_path()).map(|e| e.len()).unwrap_or(0);
            if after > before {
                term::info(Source::Gluon, &format!("appended {} event(s) → {after} total", after - before));
            }

            // Safe point maintenance hook
            if let Err(e) = hook.on_safe_point(self).await {
                term::warn(Source::Gluon, &format!("daemon safe-point maintenance error: {e:#}"));
            }

            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
                recv = change_rx.recv(), if change_open => {
                    if recv.is_none() {
                        change_open = false;
                    }
                }
                _ = tokio::time::sleep(safety_poll) => {}
            }
        }

        if let Err(e) = hadron_lattice::wire::remove_rpc_metadata(&hadron_dir) {
            term::warn(Source::Gluon, &format!("unable to clean up rpc metadata: {e:#}"));
        }
        if let Some(task) = rpc_server_task {
            let _ = task.await;
        }
        let _ = bridge.await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::field::{append_event, read_events};
    use crate::mock::MockQuark;
    use hadron_lattice::{Actor, Event, Flavor, Kind, QuarkId};
    use tempfile::tempdir;

    fn human_msg(to: &str, body: &str) -> Event {
        Event::new(
            Actor::Human,
            Some(QuarkId::new(to)),
            Kind::Message { body: body.into() },
        )
    }

    fn engine_with_quark(field: &Path, id: &str, reply: &str) -> Engine {
        let quark = MockQuark::repeating(QuarkId::new(id), Flavor::Orchestrator, reply);
        Engine::new(field.to_path_buf(), vec![Box::new(quark)], 8)
    }

    /// Poll the field for up to ~4s waiting for a `body` message to appear.
    async fn wait_for_message(field: &Path, body: &str) -> bool {
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let events = read_events(field).unwrap();
            if events
                .iter()
                .any(|e| matches!(&e.kind, Kind::Message { body: b } if b == body))
            {
                return true;
            }
        }
        false
    }

    /// Startup pass: a message already in the field is processed by the first
    /// `run_until_quiesce` before the daemon ever idles. (Does NOT exercise the
    /// wake path — see `daemon_wakes_from_idle_...` for that.)
    #[tokio::test]
    async fn daemon_processes_a_preexisting_message_on_startup() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);

        let handle = tokio::spawn(async move { engine.serve(rx).await });

        assert!(
            wait_for_message(&field, "pong").await,
            "daemon should have processed the pre-existing message and appended a reply"
        );

        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(joined.is_ok(), "serve() should return promptly after shutdown");
        joined.unwrap().unwrap().unwrap();
    }

    /// The headline Phase 4 behavior: the daemon idles at ~0% CPU on a quiesced
    /// field, and when an append arrives it wakes (notify, or the safety-poll
    /// floor) and processes it — no interaction, no restart.
    #[tokio::test]
    async fn daemon_wakes_from_idle_when_the_field_grows() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Start on an EMPTY field: the startup pass quiesces immediately, so the
        // daemon is genuinely idle in `select!` when the message lands.
        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Let it reach the idle select, THEN inject the message.
        tokio::time::sleep(Duration::from_millis(150)).await;
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        assert!(
            wait_for_message(&field, "pong").await,
            "daemon should have woken from idle and processed the appended message"
        );

        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(joined.is_ok(), "serve() should return promptly after shutdown");
        joined.unwrap().unwrap().unwrap();
    }

    /// Even if the watcher bridge never starts (its parent dir doesn't exist, so
    /// `FieldWatcher::new` fails), the daemon must not busy-spin: the `change_open`
    /// guard drops the dead channel arm and the safety-poll carries the loop. It
    /// still shuts down promptly.
    #[tokio::test]
    async fn daemon_does_not_spin_when_the_watcher_bridge_dies() {
        // Nonexistent parent dir → FieldWatcher::new errs → bridge returns →
        // change_tx drops. run_until_quiesce reads a missing file as empty → quiesce.
        let field = Path::new("/nonexistent-hadron-dir-xyz/field.jsonl").to_path_buf();
        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Give the dead-channel path time to run several loop turns; if it spun,
        // this window would peg a core but still not hang the test.
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(
            joined.is_ok(),
            "serve() should shut down promptly even with a dead watcher bridge"
        );
        joined.unwrap().unwrap().unwrap();
    }

    #[tokio::test]
    async fn daemon_shuts_down_promptly_when_idle() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Let it reach the idle select, then shut down.
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(
            joined.is_ok(),
            "idle serve() should return promptly after shutdown"
        );
        joined.unwrap().unwrap().unwrap();
    }
}
