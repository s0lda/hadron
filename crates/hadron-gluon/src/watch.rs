//! The 0-CPU field watcher: a thin `notify` seam that coalesces raw filesystem
//! events on `field.jsonl` into a single "the field changed" signal. Correctness
//! of *what* changed lives in [`hadron_lattice::io::read_new`]; this only answers
//! *whether* to re-read. Watches the parent directory (not the file) so it
//! survives create/rename/atomic-replace.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use hadron_lattice::io::read_new;
use hadron_lattice::Event;

pub struct FieldWatcher {
    // Kept alive for the lifetime of the watcher; dropping it stops watching.
    _watcher: RecommendedWatcher,
    rx: Receiver<()>,
}

impl FieldWatcher {
    /// Start watching `field_path`'s parent directory. Any raw event touching
    /// `field_path` becomes one `()` on the channel.
    pub fn new(field_path: &Path) -> anyhow::Result<FieldWatcher> {
        let field: PathBuf = field_path.to_path_buf();
        let dir = field
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if ev.paths.iter().any(|p| p == &field) {
                    // Ignore send errors: a closed receiver just means we're shutting down.
                    let _ = tx.send(());
                }
            }
        })?;
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        Ok(FieldWatcher {
            _watcher: watcher,
            rx,
        })
    }

    /// Block until the field changes or `timeout` elapses. Coalesces: a single
    /// call drains all currently-queued signals so one `wait` maps to one
    /// re-read regardless of how many raw fs events a write produced.
    pub fn wait(&self, timeout: Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(()) => {
                // Drain any backlog from the same logical change.
                while self.rx.try_recv().is_ok() {}
                true
            }
            Err(_) => false,
        }
    }

    /// Wait for the field to change (up to `timeout`), then read any newly
    /// appended events via [`read_new`]. Returns `(events, new_offset)`; the
    /// events may be empty if the wait timed out or the change added no complete
    /// line yet. `field_path` is the same path the watcher was created for.
    pub fn next_batch(
        &self,
        field_path: &Path,
        offset: u64,
        timeout: Duration,
    ) -> anyhow::Result<(Vec<Event>, u64)> {
        self.wait(timeout);
        let (events, new_offset) = read_new(field_path, offset)?;
        Ok((events, new_offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    // Liveness only: proves the notify wire is live in this environment. Correctness
    // of incremental reads is covered deterministically by hadron_lattice::io::read_new.
    #[test]
    fn wait_fires_on_append() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        std::fs::write(&field, "seed\n").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        // Give the watch a moment to register before the write.
        std::thread::sleep(Duration::from_millis(150));

        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&field).unwrap();
            writeln!(f, "another line").unwrap();
            f.flush().unwrap();
        }

        // Generous timeout — a liveness check, not a latency benchmark.
        assert!(
            watcher.wait(Duration::from_secs(5)),
            "watcher should fire on append"
        );
    }

    // Negative case: once the watcher has quiesced, an idle field must not wake it.
    //
    // Under parallel-test load the inotify backend can deliver a *residual* event
    // from the pre-watch seed write slightly after the watch registers, so a single
    // `wait` is fragile (it may consume that one leftover signal). We instead drain
    // any startup residue, then assert the *next* idle wait times out — mirroring
    // how the daemon processes the initial field state and only then waits for new
    // writes. A genuinely busy-waking watcher would still fail here, because the
    // post-drain wait would also fire.
    #[test]
    fn wait_times_out_when_no_changes() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Seed write happens BEFORE the watcher exists; any delivery of it is residue.
        std::fs::write(&field, "seed\n").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        std::thread::sleep(Duration::from_millis(150));

        // Absorb any residual startup signal (returns immediately if none).
        watcher.wait(Duration::from_millis(200));

        // Nothing writes to the field now → the next wait must time out.
        assert!(
            !watcher.wait(Duration::from_millis(400)),
            "watcher should be quiet when idle"
        );
    }

    #[test]
    fn next_batch_yields_appended_events() {
        use hadron_lattice::{Actor, Kind};
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        std::fs::write(&field, "").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        std::thread::sleep(Duration::from_millis(150));

        // Append a real event via the lattice writer.
        let ev = Event::new(Actor::Human, None, Kind::Message { body: "hi swarm".into() });
        hadron_lattice::io::append_event(&field, &ev).unwrap();

        let (batch, off) = watcher
            .next_batch(&field, 0, Duration::from_secs(5))
            .unwrap();
        assert_eq!(batch, vec![ev]);
        assert_eq!(off, std::fs::metadata(&field).unwrap().len());
    }
}
