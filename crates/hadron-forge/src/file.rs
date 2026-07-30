use std::path::{Path, PathBuf};
use crate::block::{annotate_lang, short_hash};
use crate::edit::{apply_edit_lang, EditOutcome, HashedEdit};
use crate::lang::{lang_for_path, Lang};

/// What a quark may do inside one **external** root — a directory outside its own
/// worktree that a seat has explicitly been granted.
///
/// There is no `None` rung: "not allowed" is the root not being in the list at all,
/// so a disallowed root is unrepresentable rather than a third variant every match
/// has to remember to handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAccess {
    ReadOnly,
    ReadWrite,
}

/// One directory outside the worktree that this quark may reach.
///
/// The path is canonicalised **once, here**, and the constructor is the only way to
/// build one — so a symlinked or relative entry can never reach the comparison in
/// [`Root::resolve`], and a root that does not exist is refused up front rather than
/// silently never matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRoot {
    canonical: PathBuf,
    access: ExternalAccess,
}

impl ExternalRoot {
    pub fn new(path: impl AsRef<Path>, access: ExternalAccess) -> Result<Self, ForgeError> {
        let canonical = path
            .as_ref()
            .canonicalize()
            .map_err(|e| ForgeError::Io(format!("external root {:?}: {e}", path.as_ref())))?;
        Ok(Self { canonical, access })
    }

    pub fn path(&self) -> &Path {
        &self.canonical
    }

    pub fn access(&self) -> ExternalAccess {
        self.access
    }
}

/// The worktree a quark's tools are jailed to, plus any external roots it was
/// explicitly granted. `external` is **empty by default**: `Root::new` alone is the
/// pre-existing behaviour, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    path: PathBuf,
    external: Vec<ExternalRoot>,
}

impl Root {
    pub fn new(p: impl Into<PathBuf>) -> Self {
        Self { path: p.into(), external: Vec::new() }
    }

    /// Grant one external root. Chained from `new`, so the common case reads as one
    /// expression and the ungranted case stays the default.
    pub fn allowing(mut self, root: ExternalRoot) -> Self {
        self.external.push(root);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn external_roots(&self) -> &[ExternalRoot] {
        &self.external
    }

    /// The one external root `canonical` falls under, if any, given the access needed.
    /// A read-only root answers `None` for a write, which is what makes the ladder a
    /// property of the root rather than of the caller.
    fn external_for(&self, canonical: &Path, need_write: bool) -> Option<&ExternalRoot> {
        self.external.iter().find(|r| {
            canonical.starts_with(&r.canonical)
                && (!need_write || r.access == ExternalAccess::ReadWrite)
        })
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

/// Canonicalise `full_path`, tolerating a tail that does not exist yet.
///
/// Returns the **canonical existing prefix** (every symlink in it resolved) and the
/// full resolved path. Splitting this out is what lets the worktree branch and the
/// external-root branch share one answer to "where does this path really point" —
/// two copies of this walk is exactly how a symlink escape gets reintroduced.
fn canonicalize_allowing_missing(full_path: &Path) -> Result<(PathBuf, PathBuf), ForgeError> {
    if full_path.exists() {
        let canonical = full_path.canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
        return Ok((canonical.clone(), canonical));
    }
    let mut existing_ancestor = full_path;
    let mut missing_suffix: Vec<&std::ffi::OsStr> = Vec::new();
    while !existing_ancestor.exists() {
        missing_suffix.push(existing_ancestor.file_name().ok_or(ForgeError::OutsideRoot)?);
        existing_ancestor = existing_ancestor.parent().ok_or(ForgeError::OutsideRoot)?;
    }
    let canonical_existing =
        existing_ancestor.canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
    let mut resolved = canonical_existing.clone();
    for segment in missing_suffix.into_iter().rev() {
        resolved.push(segment);
    }
    Ok((canonical_existing, resolved))
}

/// Resolve a path for **reading**. An absolute path is refused unless it lands inside
/// an external root this `Root` was granted.
pub fn resolve_jailed_path(root: &Root, rel_path: &str) -> Result<PathBuf, ForgeError> {
    resolve(root, rel_path, false)
}

/// Resolve a path for **writing**. Identical to [`resolve_jailed_path`] inside the
/// worktree; outside it, only a `ReadWrite` external root answers.
pub fn resolve_jailed_path_for_write(root: &Root, rel_path: &str) -> Result<PathBuf, ForgeError> {
    resolve(root, rel_path, true)
}

fn resolve(root: &Root, rel_path: &str, need_write: bool) -> Result<PathBuf, ForgeError> {
    let path = Path::new(rel_path);
    if path.is_absolute() {
        // The ONLY way out of the worktree, and only into a root the seat named.
        // With no external roots granted this is the pre-existing hard refusal.
        //
        // This branch deliberately returns BEFORE the lexical `..` loop below:
        // canonicalisation is what guards it, and it is strictly stronger (it also
        // resolves symlinks, which a component scan cannot). Do not "restore" the
        // loop here — `external_roots::a_traversal_out_of_an_allowed_external_root`
        // and `..._symlink_...` are the guards that this holds.
        let (canonical_existing, resolved) = canonicalize_allowing_missing(path)?;
        return match root.external_for(&canonical_existing, need_write) {
            Some(_) => Ok(resolved),
            None => Err(ForgeError::OutsideRoot),
        };
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(ForgeError::OutsideRoot);
        }
    }
    let canonical_root = root.path().canonicalize().map_err(|e| ForgeError::Io(e.to_string()))?;
    let full_path = root.path().join(rel_path);
    let (canonical_existing, resolved) = canonicalize_allowing_missing(&full_path)?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(ForgeError::OutsideRoot);
    }
    Ok(resolved)
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
    let full_path = resolve_jailed_path_for_write(root, rel_path)?;
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
    let full_path = resolve_jailed_path_for_write(root, rel_path)?;
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
    let full_path = resolve_jailed_path_for_write(root, rel_path)?;
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
    let full_path = resolve_jailed_path_for_write(root, rel_path)?;
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
    fn rejects_a_two_level_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let outside = tempfile::tempdir().unwrap();
        // `vendor` is a real symlink out of root, but the escape only reaches
        // the unchecked branch when a NON-existent path is walked one level
        // further through it (`generated/` does not exist under `outside`).
        std::os::unix::fs::symlink(outside.path(), dir.path().join("vendor")).unwrap();

        let resolved = resolve_jailed_path(&root, "vendor/generated/out.rs");
        assert!(
            matches!(resolved, Err(ForgeError::OutsideRoot)),
            "expected OutsideRoot, got {resolved:?}"
        );
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

    /// A root with one external root allowed, plus the outside dir it names.
    fn root_with_external(access: ExternalAccess) -> (tempfile::TempDir, tempfile::TempDir, Root) {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root =
            Root::new(inside.path()).allowing(ExternalRoot::new(outside.path(), access).unwrap());
        (inside, outside, root)
    }

    #[test]
    fn an_absolute_path_is_still_refused_when_no_external_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s").unwrap();
        let root = Root::new(dir.path());
        let target = outside.path().join("secret");
        assert!(matches!(
            resolve_jailed_path(&root, target.to_str().unwrap()),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn a_read_inside_an_allowed_external_root_resolves() {
        let (_inside, outside, root) = root_with_external(ExternalAccess::ReadOnly);
        std::fs::write(outside.path().join("note.md"), "hi").unwrap();
        let target = outside.path().join("note.md");
        let resolved = resolve_jailed_path(&root, target.to_str().unwrap());
        assert!(resolved.is_ok(), "expected Ok, got {resolved:?}");
    }

    #[test]
    fn a_read_only_external_root_refuses_a_write() {
        let (_inside, outside, root) = root_with_external(ExternalAccess::ReadOnly);
        std::fs::write(outside.path().join("note.md"), "hi").unwrap();
        let target = outside.path().join("note.md");
        assert!(matches!(
            resolve_jailed_path_for_write(&root, target.to_str().unwrap()),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn a_writable_external_root_accepts_a_write() {
        let (_inside, outside, root) = root_with_external(ExternalAccess::ReadWrite);
        let target = outside.path().join("new.md");
        let resolved = resolve_jailed_path_for_write(&root, target.to_str().unwrap());
        assert!(resolved.is_ok(), "expected Ok, got {resolved:?}");
    }

    #[test]
    fn a_traversal_out_of_an_allowed_external_root_is_refused() {
        let (_inside, outside, root) = root_with_external(ExternalAccess::ReadWrite);
        // `<outside>/../` climbs above the allowed root; canonicalisation must catch it.
        let target = outside.path().join("..").join("escaped.md");
        assert!(matches!(
            resolve_jailed_path(&root, target.to_str().unwrap()),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn a_symlink_out_of_an_allowed_external_root_is_refused() {
        let (_inside, outside, root) = root_with_external(ExternalAccess::ReadWrite);
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), outside.path().join("link")).unwrap();
        let target = outside.path().join("link").join("deep").join("out.rs");
        assert!(matches!(
            resolve_jailed_path(&root, target.to_str().unwrap()),
            Err(ForgeError::OutsideRoot)
        ));
    }

    #[test]
    fn an_external_root_that_does_not_exist_is_refused_at_construction() {
        let missing = std::path::Path::new("/nonexistent-hadron-external-root");
        assert!(ExternalRoot::new(missing, ExternalAccess::ReadOnly).is_err());
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
