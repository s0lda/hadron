//! Semantic Content-Addressable Storage (CAS) & Subtree Artifact Replay (Capability #2).
//!
//! Provides deterministic content-addressed artifact caching across worktrees and build tasks.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SemanticCas {
    root_dir: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CasStats {
    pub entries: usize,
    pub total_bytes: u64,
}

impl SemanticCas {
    /// Initializes CAS storage under `<hadron_dir>/cache/cas`.
    pub fn new(hadron_dir: &Path) -> io::Result<Self> {
        let root_dir = hadron_dir.join("cache").join("cas");
        fs::create_dir_all(&root_dir)?;
        Ok(Self { root_dir })
    }

    /// Returns the root directory path of the CAS storage.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Computes deterministic SHA-256 content hash of input bytes.
    pub fn hash_bytes(input: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input);
        format!("{:x}", hasher.finalize())
    }

    /// Computes deterministic content hash across multiple file paths and contents.
    pub fn hash_files(files: &[(&str, &[u8])]) -> String {
        let mut hasher = Sha256::new();
        let mut sorted = files.to_vec();
        sorted.sort_by_key(|(path, _)| *path);
        for (path, content) in sorted {
            hasher.update(path.as_bytes());
            hasher.update(b":");
            hasher.update(content);
            hasher.update(b"\0");
        }
        format!("{:x}", hasher.finalize())
    }

    /// Stores artifact data keyed by `key_hash`.
    pub fn store(&self, key_hash: &str, artifact_data: &[u8]) -> io::Result<PathBuf> {
        let target_path = self.root_dir.join(key_hash);
        fs::write(&target_path, artifact_data)?;
        Ok(target_path)
    }

    /// Checks if an artifact exists for `key_hash`.
    pub fn contains(&self, key_hash: &str) -> bool {
        self.root_dir.join(key_hash).is_file()
    }

    /// Retrieves artifact data for `key_hash`.
    pub fn retrieve(&self, key_hash: &str) -> io::Result<Option<Vec<u8>>> {
        let path = self.root_dir.join(key_hash);
        if path.is_file() {
            Ok(Some(fs::read(path)?))
        } else {
            Ok(None)
        }
    }

    /// Computes CAS storage metrics (total entries and aggregate bytes).
    pub fn stats(&self) -> io::Result<CasStats> {
        let mut stats = CasStats::default();
        if self.root_dir.is_dir() {
            for entry in fs::read_dir(&self.root_dir)? {
                let entry = entry?;
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        stats.entries += 1;
                        stats.total_bytes += meta.len();
                    }
                }
            }
        }
        Ok(stats)
    }

    /// Purges all cached entries from the CAS.
    pub fn clear(&self) -> io::Result<()> {
        if self.root_dir.is_dir() {
            for entry in fs::read_dir(&self.root_dir)? {
                let entry = entry?;
                if entry.path().is_file() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cas_replay_and_stats() {
        let tmp = tempdir().unwrap();
        let cas = SemanticCas::new(tmp.path()).unwrap();

        let file_tree = vec![
            ("src/lib.rs", b"pub fn add(a: i32, b: i32) -> i32 { a + b }" as &[u8]),
            ("Cargo.toml", b"[package]\nname = \"foo\"" as &[u8]),
        ];

        let hash = SemanticCas::hash_files(&file_tree);
        assert!(!cas.contains(&hash));

        let artifact = b"test result: ok. 1 passed";
        cas.store(&hash, artifact).unwrap();

        assert!(cas.contains(&hash));
        let retrieved = cas.retrieve(&hash).unwrap().unwrap();
        assert_eq!(retrieved, artifact);

        let stats = cas.stats().unwrap();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.total_bytes, artifact.len() as u64);

        cas.clear().unwrap();
        assert!(!cas.contains(&hash));
    }
}
