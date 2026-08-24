//! Pure logic for the `tree_checkpoint` tool family.
//! Fast zero-commit atomic worktree snapshots, diffing, and restoration jailed under `.hadron/checkpoints/`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TreeCheckpointAction {
    Save,
    Restore,
    Diff,
    List,
    Prune,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckpointMeta {
    pub id: String,
    pub label: String,
    pub created_at_ms: u64,
    pub files_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointReport {
    pub action: String,
    pub checkpoint: Option<CheckpointMeta>,
    pub list: Vec<CheckpointMeta>,
    pub diff: Option<String>,
    pub restored_files: Vec<String>,
    pub pruned_count: usize,
    pub summary: String,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".hadron/checkpoints",
    ".hadron/cassettes",
    ".hadron/screenshots",
];

fn should_ignore(rel_path: &str) -> bool {
    for ign in IGNORED_DIRS {
        if rel_path == *ign || rel_path.starts_with(&format!("{}/", ign)) {
            return true;
        }
    }
    false
}

fn collect_files(base: &Path, current: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(base) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if should_ignore(&rel_str) {
            continue;
        }
        if path.is_dir() {
            collect_files(base, &path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

pub fn save_checkpoint(root: &Root, label: &str) -> Result<CheckpointMeta, ForgeError> {
    let checkpoints_dir = resolve_jailed_path(root, ".hadron/checkpoints")?;
    fs::create_dir_all(&checkpoints_dir)
        .map_err(|e| ForgeError::Io(format!("Failed to create checkpoints directory: {e}")))?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let id = format!("ckpt_{}", now_ms);
    let ckpt_dir = checkpoints_dir.join(&id);
    let files_dir = ckpt_dir.join("files");
    fs::create_dir_all(&files_dir)
        .map_err(|e| ForgeError::Io(format!("Failed to create checkpoint directory: {e}")))?;

    let root_path = root.path();
    let mut files = Vec::new();
    collect_files(&root_path, &root_path, &mut files);

    let mut files_count = 0;
    let mut total_bytes = 0;

    for file_path in &files {
        let rel = file_path
            .strip_prefix(&root_path)
            .map_err(|e| ForgeError::Io(e.to_string()))?;
        let target_file = files_dir.join(rel);
        if let Some(parent) = target_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ForgeError::Io(format!("Failed to create parent dir: {e}")))?;
        }
        let bytes = fs::copy(file_path, &target_file)
            .map_err(|e| ForgeError::Io(format!("Failed to copy file {rel:?}: {e}")))?;
        files_count += 1;
        total_bytes += bytes;
    }

    let meta = CheckpointMeta {
        id: id.clone(),
        label: label.to_string(),
        created_at_ms: now_ms,
        files_count,
        total_bytes,
    };

    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| ForgeError::Io(format!("Failed to serialize meta: {e}")))?;
    fs::write(ckpt_dir.join("meta.json"), meta_json)
        .map_err(|e| ForgeError::Io(format!("Failed to write meta.json: {e}")))?;

    Ok(meta)
}

pub fn list_checkpoints(root: &Root) -> Result<Vec<CheckpointMeta>, ForgeError> {
    let checkpoints_dir = resolve_jailed_path(root, ".hadron/checkpoints")?;
    if !checkpoints_dir.exists() {
        return Ok(Vec::new());
    }

    let mut list = Vec::new();
    let Ok(entries) = fs::read_dir(&checkpoints_dir) else {
        return Ok(Vec::new());
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let meta_file = path.join("meta.json");
            if meta_file.is_file() {
                if let Ok(content) = fs::read_to_string(&meta_file) {
                    if let Ok(meta) = serde_json::from_str::<CheckpointMeta>(&content) {
                        list.push(meta);
                    }
                }
            }
        }
    }

    list.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    Ok(list)
}

pub fn restore_checkpoint(root: &Root, id: &str) -> Result<Vec<String>, ForgeError> {
    let checkpoints_dir = resolve_jailed_path(root, ".hadron/checkpoints")?;
    let ckpt_dir = checkpoints_dir.join(id);
    let files_dir = ckpt_dir.join("files");

    if !files_dir.exists() {
        return Err(ForgeError::Rejected(format!("Checkpoint '{}' does not exist", id)));
    }

    let root_path = root.path();
    let mut files = Vec::new();
    collect_files(&files_dir, &files_dir, &mut files);

    let mut restored = Vec::new();
    for file_path in &files {
        let rel = file_path
            .strip_prefix(&files_dir)
            .map_err(|e| ForgeError::Io(e.to_string()))?;
        let target_path = root_path.join(rel);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ForgeError::Io(format!("Failed to create parent dir: {e}")))?;
        }
        fs::copy(file_path, &target_path)
            .map_err(|e| ForgeError::Io(format!("Failed to restore file {rel:?}: {e}")))?;
        restored.push(rel.to_string_lossy().replace('\\', "/"));
    }

    Ok(restored)
}

pub fn diff_checkpoint(root: &Root, id: &str) -> Result<String, ForgeError> {
    let checkpoints_dir = resolve_jailed_path(root, ".hadron/checkpoints")?;
    let ckpt_dir = checkpoints_dir.join(id);
    let files_dir = ckpt_dir.join("files");

    if !files_dir.exists() {
        return Err(ForgeError::Rejected(format!("Checkpoint '{}' does not exist", id)));
    }

    let root_path = root.path();
    let mut ckpt_files = Vec::new();
    collect_files(&files_dir, &files_dir, &mut ckpt_files);

    let mut current_files = Vec::new();
    collect_files(&root_path, &root_path, &mut current_files);

    let mut diff_output = Vec::new();

    for cf in &ckpt_files {
        let rel = cf.strip_prefix(&files_dir).map_err(|e| ForgeError::Io(e.to_string()))?;
        let curr_path = root_path.join(rel);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if !curr_path.exists() {
            diff_output.push(format!("--- {} (checkpoint)\n+++ /dev/null\n[Deleted in current worktree]", rel_str));
            continue;
        }

        let ckpt_content = fs::read_to_string(cf).unwrap_or_default();
        let curr_content = fs::read_to_string(&curr_path).unwrap_or_default();

        if ckpt_content != curr_content {
            diff_output.push(format!(
                "--- a/{}\n+++ b/{}\n@@ Checkpoint vs Current @@\n- <checkpoint content ({}) bytes>\n+ <current content ({}) bytes>",
                rel_str,
                rel_str,
                ckpt_content.len(),
                curr_content.len()
            ));
        }
    }

    for curr in &current_files {
        let rel = curr.strip_prefix(&root_path).map_err(|e| ForgeError::Io(e.to_string()))?;
        let ckpt_path = files_dir.join(rel);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if !ckpt_path.exists() {
            diff_output.push(format!("--- /dev/null\n+++ b/{}\n[Added since checkpoint]", rel_str));
        }
    }

    if diff_output.is_empty() {
        Ok(format!("No diff found: current worktree exactly matches checkpoint '{}'.", id))
    } else {
        Ok(diff_output.join("\n\n"))
    }
}

pub fn prune_checkpoints(root: &Root, keep: usize) -> Result<usize, ForgeError> {
    let list = list_checkpoints(root)?;
    if list.len() <= keep {
        return Ok(0);
    }

    let checkpoints_dir = resolve_jailed_path(root, ".hadron/checkpoints")?;
    let to_remove = &list[keep..];
    let mut pruned = 0;

    for item in to_remove {
        let path = checkpoints_dir.join(&item.id);
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
            pruned += 1;
        }
    }

    Ok(pruned)
}

pub fn run_tree_checkpoint(
    root: &Root,
    action: TreeCheckpointAction,
    label: Option<&str>,
    checkpoint_id: Option<&str>,
    keep: Option<usize>,
) -> Result<CheckpointReport, ForgeError> {
    match action {
        TreeCheckpointAction::Save => {
            let lbl = label.unwrap_or("manual checkpoint");
            let meta = save_checkpoint(root, lbl)?;
            let summary = format!("Saved checkpoint '{}' ({}) with {} files ({} bytes).", meta.id, meta.label, meta.files_count, meta.total_bytes);
            Ok(CheckpointReport {
                action: "save".to_string(),
                checkpoint: Some(meta),
                list: Vec::new(),
                diff: None,
                restored_files: Vec::new(),
                pruned_count: 0,
                summary,
            })
        }
        TreeCheckpointAction::Restore => {
            let id = checkpoint_id.ok_or_else(|| ForgeError::Rejected("checkpoint_id required for restore".to_string()))?;
            let restored = restore_checkpoint(root, id)?;
            let summary = format!("Restored {} files from checkpoint '{}'.", restored.len(), id);
            Ok(CheckpointReport {
                action: "restore".to_string(),
                checkpoint: None,
                list: Vec::new(),
                diff: None,
                restored_files: restored,
                pruned_count: 0,
                summary,
            })
        }
        TreeCheckpointAction::Diff => {
            let id = checkpoint_id.ok_or_else(|| ForgeError::Rejected("checkpoint_id required for diff".to_string()))?;
            let diff = diff_checkpoint(root, id)?;
            let summary = format!("Diff computed for checkpoint '{}'.", id);
            Ok(CheckpointReport {
                action: "diff".to_string(),
                checkpoint: None,
                list: Vec::new(),
                diff: Some(diff),
                restored_files: Vec::new(),
                pruned_count: 0,
                summary,
            })
        }
        TreeCheckpointAction::List => {
            let list = list_checkpoints(root)?;
            let summary = format!("Found {} saved checkpoints.", list.len());
            Ok(CheckpointReport {
                action: "list".to_string(),
                checkpoint: None,
                list,
                diff: None,
                restored_files: Vec::new(),
                pruned_count: 0,
                summary,
            })
        }
        TreeCheckpointAction::Prune => {
            let keep_cnt = keep.unwrap_or(5);
            let pruned = prune_checkpoints(root, keep_cnt)?;
            let summary = format!("Pruned {} older checkpoints (kept {}).", pruned, keep_cnt);
            Ok(CheckpointReport {
                action: "prune".to_string(),
                checkpoint: None,
                list: Vec::new(),
                diff: None,
                restored_files: Vec::new(),
                pruned_count: pruned,
                summary,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_save_restore_diff_and_prune() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path());

        // Create some sample files
        fs::write(temp.path().join("file1.txt"), "hello world").unwrap();
        fs::create_dir_all(temp.path().join("sub")).unwrap();
        fs::write(temp.path().join("sub/file2.txt"), "nested file").unwrap();

        // 1. Save checkpoint
        let meta = save_checkpoint(&root, "initial state").unwrap();
        assert_eq!(meta.files_count, 2);

        // 2. Modify files and add a new one
        fs::write(temp.path().join("file1.txt"), "modified world").unwrap();
        fs::write(temp.path().join("new_file.txt"), "fresh").unwrap();

        // 3. Diff checkpoint
        let diff = diff_checkpoint(&root, &meta.id).unwrap();
        assert!(diff.contains("file1.txt"));
        assert!(diff.contains("new_file.txt"));

        // 4. Restore checkpoint
        let restored = restore_checkpoint(&root, &meta.id).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(fs::read_to_string(temp.path().join("file1.txt")).unwrap(), "hello world");

        // 5. List checkpoints
        let list = list_checkpoints(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, meta.id);

        // 6. Prune checkpoints
        let pruned = prune_checkpoints(&root, 0).unwrap();
        assert_eq!(pruned, 1);
        assert_eq!(list_checkpoints(&root).unwrap().len(), 0);
    }
}
