//! The chamber's read-only view of git.
//!
//! Deliberately NOT `hadron_gluon::snapshot`. That module lives in the engine crate,
//! which links bundled SQLite, the tokio runtime, the file watcher and the CLI process
//! adapters — none of which the UI has any business carrying just to read a diff. The
//! chamber renders the field; it does not drive the swarm, so it must not depend on the
//! crate that does. Shelling out to git is the whole implementation.

use std::path::Path;
use std::process::Command;

/// `git diff HEAD` for the working tree — what the Changes rail shows.
///
/// A repository with no commits yet has no HEAD to diff against, so it has no changes
/// to show rather than an error to report.
pub fn working_diff(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The project root that owns a field path — `<root>/.hadron/field.jsonl` → `<root>`.
/// A field sitting outside a `.hadron/` directory is taken to be in the root already.
pub fn repo_root_of(field_path: &Path) -> &Path {
    let Some(parent) = field_path.parent() else {
        return field_path;
    };
    if parent.file_name() == Some(std::ffi::OsStr::new(".hadron")) {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_repo_root_is_the_parent_of_the_hadron_dir() {
        let field = PathBuf::from("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/home/jake/dev/hadron"));
    }

    #[test]
    fn a_field_outside_a_hadron_dir_is_already_in_the_root() {
        let field = PathBuf::from("/tmp/scratch/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/tmp/scratch"));
    }
}
