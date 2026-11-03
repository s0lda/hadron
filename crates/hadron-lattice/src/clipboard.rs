//! Shared Inter-Quark Artifact Clipboard (`.hadron/clipboard/clips.json`).
//!
//! Provides an out-of-band key-value clipboard buffer shared across all isolated
//! quark worktrees and the Chamber UI so quarks can exchange code snippets,
//! diffs, and structured payloads without polluting `field.jsonl`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub key: String,
    pub content: String,
    pub author: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ClipboardFile {
    #[serde(default)]
    clips: BTreeMap<String, ClipboardEntry>,
}

/// Resolve the shared `<repo>/.hadron` directory from any path (whether `<repo>`,
/// `<repo>/.hadron`, `<repo>/.hadron/field.jsonl`, or an isolated worktree
/// `<repo>/.hadron/trees/<quark>`).
pub fn resolve_shared_hadron_dir(path: &Path) -> PathBuf {
    let mut cur = Some(path);
    while let Some(p) = cur {
        if p.file_name().and_then(|n| n.to_str()) == Some(".hadron") {
            return p.to_path_buf();
        }
        cur = p.parent();
    }
    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
        if let Some(parent) = path.parent() {
            return parent.to_path_buf();
        }
    }
    path.join(".hadron")
}

/// Path to the shared clipboard JSON store (`<repo>/.hadron/clipboard/clips.json`).
pub fn clipboard_file_path(cwd_or_hadron: &Path) -> PathBuf {
    resolve_shared_hadron_dir(cwd_or_hadron)
        .join("clipboard")
        .join("clips.json")
}

fn load_store(path: &Path) -> std::io::Result<ClipboardFile> {
    match fs::read_to_string(path) {
        Ok(raw) if !raw.trim().is_empty() => serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Ok(_) => Ok(ClipboardFile::default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ClipboardFile::default()),
        Err(e) => Err(e),
    }
}

fn save_store(path: &Path, store: &ClipboardFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(store)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Store or overwrite a named clipboard artifact in the shared `.hadron/clipboard/clips.json`.
pub fn put_clip(
    cwd_or_hadron: &Path,
    key: &str,
    content: &str,
    author: &str,
) -> std::io::Result<ClipboardEntry> {
    let clean_key = key.trim();
    if clean_key.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "clipboard key must not be empty",
        ));
    }
    let path = clipboard_file_path(cwd_or_hadron);
    let mut store = load_store(&path)?;
    let entry = ClipboardEntry {
        key: clean_key.to_string(),
        content: content.to_string(),
        author: if author.trim().is_empty() {
            "quark".to_string()
        } else {
            author.trim().to_string()
        },
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    store.clips.insert(clean_key.to_string(), entry.clone());
    save_store(&path, &store)?;
    Ok(entry)
}

/// Retrieve a named clipboard entry from the shared `.hadron/clipboard/clips.json`.
pub fn get_clip(cwd_or_hadron: &Path, key: &str) -> std::io::Result<Option<ClipboardEntry>> {
    let path = clipboard_file_path(cwd_or_hadron);
    let store = load_store(&path)?;
    Ok(store.clips.get(key.trim()).cloned())
}

/// List all entries in the shared `.hadron/clipboard/clips.json` sorted by key.
pub fn list_clips(cwd_or_hadron: &Path) -> std::io::Result<Vec<ClipboardEntry>> {
    let path = clipboard_file_path(cwd_or_hadron);
    let store = load_store(&path)?;
    Ok(store.clips.into_values().collect())
}

/// Clear all entries in the shared `.hadron/clipboard/clips.json`, returning the number removed.
pub fn clear_clips(cwd_or_hadron: &Path) -> std::io::Result<usize> {
    let path = clipboard_file_path(cwd_or_hadron);
    let mut store = load_store(&path)?;
    let count = store.clips.len();
    store.clips.clear();
    save_store(&path, &store)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn shared_clipboard_resolves_across_isolated_worktrees() {
        let repo = tempdir().unwrap();
        let wt_a = repo.path().join(".hadron").join("trees").join("cli-agy");
        let wt_b = repo.path().join(".hadron").join("trees").join("acp-agy");
        fs::create_dir_all(&wt_a).unwrap();
        fs::create_dir_all(&wt_b).unwrap();

        let put = put_clip(&wt_a, "api-schema", "struct User { id: u64 }", "cli-agy").unwrap();
        assert_eq!(put.key, "api-schema");

        // Reading from sibling worktree wt_b sees the clip immediately.
        let got = get_clip(&wt_b, "api-schema").unwrap().expect("clip present");
        assert_eq!(got.content, "struct User { id: u64 }");
        assert_eq!(got.author, "cli-agy");

        let all = list_clips(repo.path()).unwrap();
        assert_eq!(all.len(), 1);

        let cleared = clear_clips(&wt_b).unwrap();
        assert_eq!(cleared, 1);
        assert!(list_clips(&wt_a).unwrap().is_empty());
    }
}
