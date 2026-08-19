//! Quark Cross-Worktree Gossip Bus (Capability #3).
//!
//! Broadcasts file touches, locks, and collision warnings across active quarks.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GossipPayload {
    FileTouch {
        path: String,
        is_edit: bool,
    },
    FileLock {
        path: String,
        reason: String,
    },
    Heartbeat {
        status: String,
    },
    CollisionWarning {
        path: String,
        quarks: Vec<String>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GossipMessage {
    pub quark: String,
    pub timestamp: DateTime<Utc>,
    pub payload: GossipPayload,
}

#[derive(Debug, Clone)]
pub struct GossipBus {
    bus_file: PathBuf,
    touch_history: Arc<Mutex<HashMap<String, Vec<(String, DateTime<Utc>, bool)>>>>,
}

impl GossipBus {
    /// Initializes GossipBus with backing file under `<hadron_dir>/gossip/bus.ndjson`.
    pub fn new(hadron_dir: &Path) -> io::Result<Self> {
        let dir = hadron_dir.join("gossip");
        fs::create_dir_all(&dir)?;
        let bus_file = dir.join("bus.ndjson");
        Ok(Self {
            bus_file,
            touch_history: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Appends a message to the gossip log.
    pub fn publish(&self, msg: &GossipMessage) -> io::Result<()> {
        let serialized = serde_json::to_string(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.bus_file)?;
        writeln!(file, "{serialized}")?;
        Ok(())
    }

    /// Reads all gossip messages from the start or after a given line offset.
    pub fn read_messages(&self, start_offset: usize) -> io::Result<(Vec<GossipMessage>, usize)> {
        if !self.bus_file.is_file() {
            return Ok((Vec::new(), 0));
        }

        let file = fs::File::open(&self.bus_file)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();
        let mut count = 0;

        for (ix, line) in reader.lines().enumerate() {
            count = ix + 1;
            if ix >= start_offset {
                if let Ok(l) = line {
                    if let Ok(msg) = serde_json::from_str::<GossipMessage>(&l) {
                        messages.push(msg);
                    }
                }
            }
        }

        Ok((messages, count))
    }

    /// Broadcasts a file touch and returns a collision warning if multiple quarks are editing concurrently.
    pub fn publish_touch(
        &self,
        quark: &str,
        path: &str,
        is_edit: bool,
        window_secs: i64,
    ) -> io::Result<Option<GossipMessage>> {
        let now = Utc::now();
        let mut hist = self.touch_history.lock().unwrap();
        let entries = hist.entry(path.to_string()).or_default();

        // Prune older than window_secs
        entries.retain(|(_, ts, _)| *ts >= now - Duration::seconds(window_secs));

        let mut warning = None;
        if is_edit {
            let other_quarks: Vec<String> = entries
                .iter()
                .filter(|(q, _, other_edit)| q != quark && *other_edit)
                .map(|(q, _, _)| q.clone())
                .collect();

            if !other_quarks.is_empty() {
                let mut involved = other_quarks;
                involved.push(quark.to_string());
                involved.sort();
                involved.dedup();

                let warn_msg = GossipMessage {
                    quark: "gluon".to_string(),
                    timestamp: now,
                    payload: GossipPayload::CollisionWarning {
                        path: path.to_string(),
                        quarks: involved.clone(),
                        message: format!(
                            "Collision warning: quarks {:?} are concurrently editing '{}'",
                            involved, path
                        ),
                    },
                };
                self.publish(&warn_msg)?;
                warning = Some(warn_msg);
            }
        }

        entries.push((quark.to_string(), now, is_edit));

        let msg = GossipMessage {
            quark: quark.to_string(),
            timestamp: now,
            payload: GossipPayload::FileTouch {
                path: path.to_string(),
                is_edit,
            },
        };
        self.publish(&msg)?;

        Ok(warning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_gossip_bus_touch_and_collision_detection() {
        let tmp = tempdir().unwrap();
        let bus = GossipBus::new(tmp.path()).unwrap();

        // 1. Quark A edits file.rs -> No collision
        let warn1 = bus.publish_touch("quark-a", "src/lib.rs", true, 60).unwrap();
        assert!(warn1.is_none());

        // 2. Quark B reads file.rs -> No collision
        let warn2 = bus.publish_touch("quark-b", "src/lib.rs", false, 60).unwrap();
        assert!(warn2.is_none());

        // 3. Quark B edits file.rs -> Collision triggered!
        let warn3 = bus.publish_touch("quark-b", "src/lib.rs", true, 60).unwrap();
        assert!(warn3.is_some());
        let warn = warn3.unwrap();
        match warn.payload {
            GossipPayload::CollisionWarning { quarks, path, .. } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(quarks, vec!["quark-a", "quark-b"]);
            }
            _ => panic!("Expected collision warning"),
        }

        // 4. Read messages
        let (msgs, count) = bus.read_messages(0).unwrap();
        assert_eq!(msgs.len(), 4); // touch-a, touch-b-read, collision-warn, touch-b-edit
        assert_eq!(count, 4);
    }
}
