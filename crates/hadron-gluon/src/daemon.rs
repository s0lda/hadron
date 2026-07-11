//! The swarm loop: run the engine as a persistent daemon that sleeps at ~0% CPU
//! and wakes to process new field appends until told to shut down.

use std::time::Duration;

use tokio::sync::watch;

use crate::engine::Engine;
use crate::watch::FieldWatcher;

/// How often the daemon re-checks the field even if notify never fires.
/// notify (via [`FieldWatcher`]) wakes it sooner; this is the liveness floor.
const SAFETY_POLL: Duration = Duration::from_millis(500);

impl Engine {
    /// Run as a persistent daemon: process the field to quiescence, then sleep
    /// until the field changes (notify) or `SAFETY_POLL` elapses, then process
    /// again. Returns when `shutdown` observes `true`.
    ///
    /// 0-CPU class: the process is asleep in `select!` between wakes. notify is
    /// a latency optimization; the safety-poll arm is the liveness guarantee, so
    /// the daemon still progresses where inotify never fires (WSL2 `/mnt/c`) and
    /// never busy-spins if the watcher bridge dies.
    pub async fn serve(&mut self, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        // Bridge the blocking notify watcher into an async change signal. A
        // bounded(1) channel coalesces bursts: one pending "something changed"
        // is all the loop needs.
        let (change_tx, mut change_rx) = tokio::sync::mpsc::channel::<()>(1);
        let field_path = self.field_path().to_path_buf();
        let bridge_shutdown = shutdown.clone();
        let bridge = tokio::task::spawn_blocking(move || {
            // If the watcher can't be created (e.g. dir missing), drop the
            // sender and let the daemon fall back to pure safety-polling.
            let watcher = match FieldWatcher::new(&field_path) {
                Ok(w) => w,
                Err(_) => return,
            };
            while !*bridge_shutdown.borrow() {
                if watcher.wait(Duration::from_millis(200)) {
                    let _ = change_tx.try_send(());
                }
            }
        });

        // Once the change channel closes (bridge exited / watcher failed) we
        // stop selecting on it so a ready-`None` can't starve the sleep arm and
        // spin the loop. The safety-poll keeps the daemon live regardless.
        let mut change_open = true;
        loop {
            self.run_until_quiesce().await?;
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
                _ = tokio::time::sleep(SAFETY_POLL) => {}
            }
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
        Engine::new(field.to_path_buf(), vec![Box::new(quark)], String::new(), 8)
    }

    #[tokio::test]
    async fn daemon_processes_an_appended_message_then_shuts_down() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Seed the file so the watcher's parent dir exists and is watchable.
        append_event(&field, &human_msg("agy", "hello")).unwrap();

        let mut engine = engine_with_quark(&field, "agy", "pong");
        let (tx, rx) = watch::channel(false);

        let handle = tokio::spawn(async move { engine.serve(rx).await });

        // Poll for the quark's reply to land (safety-poll floor is 500ms; give margin).
        let mut saw_reply = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let events = read_events(&field).unwrap();
            if events
                .iter()
                .any(|e| matches!(&e.kind, Kind::Message { body } if body == "pong"))
            {
                saw_reply = true;
                break;
            }
        }
        assert!(
            saw_reply,
            "daemon should have processed the seeded message and appended a reply"
        );

        // Shut down and confirm serve() returns Ok promptly.
        tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(joined.is_ok(), "serve() should return promptly after shutdown");
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
