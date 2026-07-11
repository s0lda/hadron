//! The 0-CPU field watcher: a thin `notify` seam that coalesces raw filesystem
//! events on `field.jsonl` into a single "the field changed" signal. Correctness
//! of *what* changed lives in [`hadron_lattice::io::read_new`]; this only answers
//! *whether* to re-read. Watches the parent directory (not the file) so it
//! survives create/rename/atomic-replace.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

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

    // Negative case kept separate and write-free so it can't race trailing events
    // from a preceding append (the flaky failure mode).
    #[test]
    fn wait_times_out_when_no_changes() {
        let dir = tempdir().unwrap();
        let field = dir.path().join("field.jsonl");
        // Seed write happens BEFORE the watcher exists, so it produces no signal.
        std::fs::write(&field, "seed\n").unwrap();

        let watcher = FieldWatcher::new(&field).unwrap();
        std::thread::sleep(Duration::from_millis(150));

        // No writes at all after watching → wait must time out.
        assert!(
            !watcher.wait(Duration::from_millis(400)),
            "watcher should be quiet when idle"
        );
    }
}
