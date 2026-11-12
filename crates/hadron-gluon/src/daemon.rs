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
                .with_field_and_wake(self.field_path().to_path_buf(), change_tx.clone())
                .with_wake_notify(self.wake_notifier()),
        );

        let bridge_shutdown = shutdown.clone();
        let bridge_change_tx = change_tx.clone();
        let bridge_wake_notify = self.wake_notifier();
        let bridge_dispatcher = std::sync::Arc::clone(&dispatcher);
        let bridge_field_path = field_path.clone();
        let bridge = tokio::task::spawn_blocking(move || {
            // If the watcher can't be created (e.g. dir missing), drop the
            // sender and let the daemon fall back to pure safety-polling.
            let watcher = match FieldWatcher::new(&bridge_field_path) {
                Ok(w) => w,
                Err(_) => return,
            };
            let mut last_offset = std::fs::metadata(&bridge_field_path)
                .map(|m| m.len())
                .unwrap_or(0);
            while !*bridge_shutdown.borrow() && bridge_shutdown.has_changed().is_ok() {
                if watcher.wait(Duration::from_millis(200)) {
                    let _ = bridge_change_tx.try_send(());
                    bridge_wake_notify.notify_one();
                    if let Ok((events, next_offset)) =
                        hadron_lattice::io::read_new(&bridge_field_path, last_offset)
                    {
                        if next_offset != last_offset || !events.is_empty() {
                            last_offset = next_offset;
                            bridge_dispatcher.publish(
                                hadron_lattice::wire::RpcPushEvent::FieldAppended {
                                    next_offset,
                                    count: events.len(),
                                },
                            );
                        }
                    }
                }
            }
        });

        let live_dir = hadron_lattice::live::live_dir(&field_path);
        let gates_dir = hadron_lattice::live::gates_dir(&field_path);
        let live_dispatcher = std::sync::Arc::clone(&dispatcher);
        let mut live_shutdown = shutdown.clone();
        let live_watch_task = tokio::spawn(async move {
            let mut seen_live: std::collections::HashMap<String, std::time::SystemTime> =
                std::collections::HashMap::new();
            let mut seen_gates: std::collections::HashMap<String, std::time::SystemTime> =
                std::collections::HashMap::new();
            loop {
                tokio::select! {
                    res = live_shutdown.changed() => {
                        if res.is_err() || *live_shutdown.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(75)) => {
                        let mut cur_live = std::collections::HashMap::new();
                        if let Ok(entries) = std::fs::read_dir(&live_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                                    continue;
                                }
                                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                                    continue;
                                };
                                if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                                    let quark = stem.to_string();
                                    if seen_live.get(&quark) != Some(&mtime) {
                                        live_dispatcher.publish(
                                            hadron_lattice::wire::RpcPushEvent::LiveUpdated {
                                                quark: quark.clone(),
                                            },
                                        );
                                    }
                                    cur_live.insert(quark, mtime);
                                }
                            }
                        }
                        for removed_quark in seen_live.keys() {
                            if !cur_live.contains_key(removed_quark) {
                                live_dispatcher.publish(
                                    hadron_lattice::wire::RpcPushEvent::LiveUpdated {
                                        quark: removed_quark.clone(),
                                    },
                                );
                            }
                        }
                        seen_live = cur_live;

                        let mut cur_gates = std::collections::HashMap::new();
                        if let Ok(entries) = std::fs::read_dir(&gates_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                                    continue;
                                }
                                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                                    continue;
                                };
                                if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                                    let key = stem.to_string();
                                    if seen_gates.get(&key) != Some(&mtime) {
                                        let quark = std::fs::read_to_string(&path)
                                            .ok()
                                            .and_then(|raw| serde_json::from_str::<hadron_lattice::Activity>(&raw).ok())
                                            .map(|a| a.quark.as_str().to_string())
                                            .unwrap_or_else(|| key.clone());
                                        live_dispatcher.publish(
                                            hadron_lattice::wire::RpcPushEvent::GateUpdated {
                                                quark,
                                                passed: None,
                                            },
                                        );
                                    }
                                    cur_gates.insert(key, mtime);
                                }
                            }
                        }
                        for removed_gate in seen_gates.keys() {
                            if !cur_gates.contains_key(removed_gate) {
                                live_dispatcher.publish(
                                    hadron_lattice::wire::RpcPushEvent::GateUpdated {
                                        quark: removed_gate.clone(),
                                        passed: None,
                                    },
                                );
                            }
                        }
                        seen_gates = cur_gates;
                    }
                }
            }
        });

        let rpc_server_task = match crate::rpc_server::RpcServer::bind_and_serve(&hadron_dir, std::sync::Arc::clone(&dispatcher), shutdown.clone()).await {
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
                let next_offset = std::fs::metadata(self.field_path())
                    .map(|m| m.len())
                    .unwrap_or(0);
                dispatcher.publish(hadron_lattice::wire::RpcPushEvent::FieldAppended {
                    next_offset,
                    count: after - before,
                });
            }

            // Safe point maintenance hook
            if let Err(e) = hook.on_safe_point(self).await {
                term::warn(Source::Gluon, &format!("daemon safe-point maintenance error: {e:#}"));
            }

            tokio::select! {
                res = shutdown.changed() => {
                    if res.is_err() || *shutdown.borrow() {
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
        let _ = live_watch_task.await;
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

    #[tokio::test]
    async fn daemon_pushes_field_appended_to_subscribed_rpc_client() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Wait for gluon.rpc metadata to appear in hadron_dir_of(&field)
        let hadron_dir = hadron_lattice::hadron_dir_of(&field);
        let mut rpc_meta = None;
        for _ in 0..30 {
            if let Some(m) = hadron_lattice::wire::read_rpc_metadata(&hadron_dir) {
                rpc_meta = Some(m);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let meta = rpc_meta.expect("daemon should publish gluon.rpc");

        let mut stream = tokio::net::TcpStream::connect(&meta.addr).await.unwrap();
        let (reader, mut writer) = stream.split();
        let mut buf_reader = BufReader::new(reader);

        let sub = hadron_lattice::wire::JsonRpcRequest::new(
            hadron_lattice::wire::DAEMON_SUBSCRIBE_METHOD,
            serde_json::json!({}),
        );
        writer
            .write_all(format!("{}\n", serde_json::to_string(&sub).unwrap()).as_bytes())
            .await
            .unwrap();
        writer.flush().await.unwrap();

        let mut ack_line = String::new();
        tokio::time::timeout(Duration::from_secs(2), buf_reader.read_line(&mut ack_line))
            .await
            .unwrap()
            .unwrap();
        let ack: hadron_lattice::wire::JsonRpcResponse =
            serde_json::from_str(ack_line.trim()).unwrap();
        assert_eq!(ack.result.unwrap()["subscribed"], true);

        // Append a message and verify a FieldAppended push notification arrives over TCP
        append_event(&field, &human_msg("agy", "ping")).unwrap();

        let mut push_line = String::new();
        tokio::time::timeout(Duration::from_secs(3), buf_reader.read_line(&mut push_line))
            .await
            .expect("daemon should push FieldAppended notification")
            .unwrap();
        let push_ev =
            hadron_lattice::wire::JsonRpcCodec::decode_push_event(&push_line).expect("decode push");
        assert!(matches!(
            push_ev,
            hadron_lattice::wire::RpcPushEvent::FieldAppended { next_offset, count }
                if next_offset > 0 && count >= 1
        ));

        drop(buf_reader);
        drop(stream);
        tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
