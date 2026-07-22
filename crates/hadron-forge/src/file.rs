use std::path::{Path, PathBuf};
use crate::block::{annotate_lang, short_hash};
use crate::edit::{apply_edit_lang, EditOutcome, HashedEdit};
use crate::lang::{lang_for_path, Lang};

pub struct Root(PathBuf);

impl Root {
    pub fn new(p: impl Into<PathBuf>) -> Self {
        Self(p.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ForgeError {
    OutsideRoot,
    NotFound,
    Io(String),
    Rejected(String),
    NotHashable,
}

impl std::fmt::Display for ForgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForgeError::OutsideRoot => write!(f, "path escapes root directory"),
            ForgeError::NotFound => write!(f, "file not found"),
            ForgeError::Io(s) => write!(f, "IO error: {s}"),
            ForgeError::Rejected(s) => write!(f, "edit rejected: {s}"),
            ForgeError::NotHashable => write!(f, "file type does not support AST block editing"),
        }
    }
}

impl std::error::Error for ForgeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditReport {
    pub blocks: String,
}

pub fn resolve_jailed_path(root: &Root, rel_path: &str) -> Result<PathBuf, ForgeError> {
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return Err(ForgeError::OutsideRoot);
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(ForgeError::OutsideRoot);
        }
    }
    let canonical_root = root.0.canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
    let full_path = root.0.join(rel_path);
    if full_path.exists() {
        let canonical_full = full_path.canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
        if !canonical_full.starts_with(&canonical_root) {
            return Err(ForgeError::OutsideRoot);
        }
        Ok(canonical_full)
    } else {
        if let Some(parent) = full_path.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
                if !canonical_parent.starts_with(&canonical_root) {
                    return Err(ForgeError::OutsideRoot);
                }
            }
        }
        Ok(full_path)
    }
}

fn atomic_write(path: &Path, content: &str) -> Result<(), ForgeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ForgeError::Io(e.to_string()))?;
    }
    let tmp_path = path.with_extension(format!("tmp_{}", std::process::id()));
    std::fs::write(&tmp_path, content).map_err(|e| ForgeError::Io(e.to_string()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| ForgeError::Io(e.to_string()))?;
    Ok(())
}

pub fn apply_block_edit(
    root: &Root,
    rel_path: &str,
    target_hash: &str,
    new_text: &str,
) -> Result<EditReport, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    let lang = lang_for_path(rel_path);
    if lang == Lang::Opaque {
        return Err(ForgeError::NotHashable);
    }
    let source = std::fs::read_to_string(&full_path).map_err(|_| ForgeError::NotFound)?;
    let edit = HashedEdit {
        target_hash: target_hash.to_string(),
        new_text: new_text.to_string(),
    };
    match apply_edit_lang(&source, &edit, lang) {
        EditOutcome::Applied { new_source } => {
            atomic_write(&full_path, &new_source)?;
            let blocks = annotate_lang(&new_source, lang);
            Ok(EditReport { blocks })
        }
        EditOutcome::Rejected { reason } => Err(ForgeError::Rejected(reason)),
    }
}

pub fn create_file(
    root: &Root,
    rel_path: &str,
    content: &str,
) -> Result<EditReport, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    if full_path.exists() {
        return Err(ForgeError::Rejected(format!("file {rel_path} already exists")));
    }
    atomic_write(&full_path, content)?;
    let lang = lang_for_path(rel_path);
    let blocks = if lang != Lang::Opaque {
        annotate_lang(content, lang)
    } else {
        short_hash(content)
    };
    Ok(EditReport { blocks })
}

pub fn write_file_cas(
    root: &Root,
    rel_path: &str,
    content: &str,
    expected_hash: Option<&str>,
) -> Result<EditReport, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    if let Some(expected) = expected_hash {
        let current = std::fs::read_to_string(&full_path).map_err(|_| ForgeError::NotFound)?;
        let cur_hash = short_hash(&current);
        if cur_hash != expected {
            return Err(ForgeError::Rejected(format!(
                "stale file hash {expected}: current file hashes to {cur_hash}"
            )));
        }
    }
    atomic_write(&full_path, content)?;
    let lang = lang_for_path(rel_path);
    let blocks = if lang != Lang::Opaque {
        annotate_lang(content, lang)
    } else {
        short_hash(content)
    };
    Ok(EditReport { blocks })
}

pub fn delete_file_cas(
    root: &Root,
    rel_path: &str,
    expected_hash: Option<&str>,
) -> Result<(), ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    if !full_path.exists() {
        return Err(ForgeError::NotFound);
    }
    if let Some(expected) = expected_hash {
        let current = std::fs::read_to_string(&full_path).map_err(|e| ForgeError::Io(e.to_string()))?;
        let cur_hash = short_hash(&current);
        if cur_hash != expected {
            return Err(ForgeError::Rejected(format!(
                "stale file hash {expected}: current file hashes to {cur_hash}"
            )));
        }
    }
    std::fs::remove_file(&full_path).map_err(|e| ForgeError::Io(e.to_string()))?;
    Ok(())
}

pub fn read_blocks(root: &Root, rel_path: &str) -> Result<EditReport, ForgeError> {
    let full_path = resolve_jailed_path(root, rel_path)?;
    let current = std::fs::read_to_string(&full_path).map_err(|_| ForgeError::NotFound)?;
    let lang = lang_for_path(rel_path);
    let blocks = if lang != Lang::Opaque {
        annotate_lang(&current, lang)
    } else {
        short_hash(&current)
    };
    Ok(EditReport { blocks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::parse_blocks;

    #[test]
    fn edits_a_rust_fn_by_hash_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("a.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let h = parse_blocks(&std::fs::read_to_string(dir.path().join("a.rs")).unwrap())[0]
            .hash
            .clone();
        let rep = apply_block_edit(&root, "a.rs", &h, "pub fn a() -> i32 { 2 }").unwrap();
        assert!(std::fs::read_to_string(dir.path().join("a.rs")).unwrap().contains("2"));
        assert!(rep.blocks.contains("[Hash: "));
    }

    #[test]
    fn rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        assert!(matches!(
            apply_block_edit(&root, "../evil.rs", "abc", "x"),
            Err(ForgeError::OutsideRoot)
        ));
        assert!(matches!(
            apply_block_edit(&root, "/etc/passwd", "abc", "x"),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn cas_rejects_stale_then_applies_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("c.toml"), "a = 1\n").unwrap();
        assert!(matches!(
            write_file_cas(&root, "c.toml", "a = 2\n", Some("000000")),
            Err(ForgeError::Rejected(_))
        ));
        let cur = short_hash(&std::fs::read_to_string(dir.path().join("c.toml")).unwrap());
        assert!(write_file_cas(&root, "c.toml", "a = 2\n", Some(&cur)).is_ok());
    }

    #[test]
    fn create_refuses_existing() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("x.md"), "hi").unwrap();
        assert!(matches!(
            create_file(&root, "x.md", "other"),
            Err(ForgeError::Rejected(_))
        ));
    }

    #[test]
    fn delete_file_cas_and_read_blocks_work() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        create_file(&root, "test.py", "def foo(): pass\n").unwrap();

        let rep = read_blocks(&root, "test.py").unwrap();
        assert!(rep.blocks.contains("fn foo"));

        let cur_hash = short_hash("def foo(): pass\n");
        assert!(matches!(
            delete_file_cas(&root, "test.py", Some("wrong_hash")),
            Err(ForgeError::Rejected(_))
        ));
        assert!(delete_file_cas(&root, "test.py", Some(&cur_hash)).is_ok());
        assert!(!dir.path().join("test.py").exists());
    }
}
