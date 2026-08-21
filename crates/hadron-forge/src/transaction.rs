//! Atomic multi-file batch edit transactions.
//!
//! Provides optimistic concurrency control and all-or-nothing rollback for multi-file
//! modifications in a single turn.

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// A single atomic file edit operation in a multi-file batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEditOp {
    pub path: PathBuf,
    /// Expected blake3 hash of existing file content (or empty/None for new file).
    pub expected_hash: Option<String>,
    pub new_content: String,
}

/// Execution outcome report for an applied batch edit transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchEditReport {
    pub files_modified: Vec<PathBuf>,
    pub total_bytes_written: usize,
}

/// Rollback error details when any file in a transaction fails validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchEditRollbackError {
    pub failed_path: PathBuf,
    pub reason: String,
    pub restored_paths: Vec<PathBuf>,
}

impl std::fmt::Display for BatchEditRollbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Transaction rolled back: failed on {:?} ({})",
            self.failed_path, self.reason
        )
    }
}

impl std::error::Error for BatchEditRollbackError {}

/// Atomic multi-file transaction engine.
#[derive(Debug, Clone, Default)]
pub struct BatchEditTransaction {
    pub edits: Vec<FileEditOp>,
}

impl BatchEditTransaction {
    pub fn new() -> Self {
        Self { edits: Vec::new() }
    }

    pub fn add_edit(&mut self, edit: FileEditOp) {
        self.edits.push(edit);
    }

    /// Compute hash hex string of content.
    pub fn hash_content(content: &str) -> String {
        blake3::hash(content.as_bytes()).to_hex().to_string()
    }

    /// Validate all file hashes and apply all edits atomically.
    /// If any file fails hash validation or write, all modified files are restored.
    pub fn validate_and_apply(&self) -> Result<BatchEditReport, BatchEditRollbackError> {
        let mut original_backups: Vec<(PathBuf, Option<String>)> = Vec::new();

        // 1. Validation phase
        for op in &self.edits {
            if op.path.exists() {
                let content = match fs::read_to_string(&op.path) {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(BatchEditRollbackError {
                            failed_path: op.path.clone(),
                            reason: format!("Failed to read file: {}", e),
                            restored_paths: Vec::new(),
                        });
                    }
                };

                if let Some(ref exp_hash) = op.expected_hash {
                    let actual_hash = Self::hash_content(&content);
                    if exp_hash != &actual_hash {
                        return Err(BatchEditRollbackError {
                            failed_path: op.path.clone(),
                            reason: format!(
                                "Hash mismatch: expected {}, found {}",
                                exp_hash, actual_hash
                            ),
                            restored_paths: Vec::new(),
                        });
                    }
                }
                original_backups.push((op.path.clone(), Some(content)));
            } else {
                if let Some(ref exp_hash) = op.expected_hash {
                    if !exp_hash.is_empty() {
                        return Err(BatchEditRollbackError {
                            failed_path: op.path.clone(),
                            reason: "File does not exist but expected hash was provided".into(),
                            restored_paths: Vec::new(),
                        });
                    }
                }
                original_backups.push((op.path.clone(), None));
            }
        }

        // 2. Application phase with rollback on error
        let mut applied_paths = Vec::new();
        let mut total_bytes = 0;

        for op in &self.edits {
            if let Some(parent) = op.path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    self.rollback(&applied_paths, &original_backups);
                    return Err(BatchEditRollbackError {
                        failed_path: op.path.clone(),
                        reason: format!("Failed to create parent directory: {}", e),
                        restored_paths: applied_paths,
                    });
                }
            }

            if let Err(e) = fs::write(&op.path, &op.new_content) {
                self.rollback(&applied_paths, &original_backups);
                return Err(BatchEditRollbackError {
                    failed_path: op.path.clone(),
                    reason: format!("Write failed: {}", e),
                    restored_paths: applied_paths,
                });
            }

            total_bytes += op.new_content.len();
            applied_paths.push(op.path.clone());
        }

        Ok(BatchEditReport {
            files_modified: applied_paths,
            total_bytes_written: total_bytes,
        })
    }

    fn rollback(
        &self,
        applied: &[PathBuf],
        backups: &[(PathBuf, Option<String>)],
    ) {
        for path in applied {
            if let Some((_, orig)) = backups.iter().find(|(p, _)| p == path) {
                if let Some(ref content) = orig {
                    let _ = fs::write(path, content);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}
