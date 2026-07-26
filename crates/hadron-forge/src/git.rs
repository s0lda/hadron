//! Pure logic for the `git` tool family: uncommitted diff and file history.
//!
//! Both go through `exec`, which already runs every argument through
//! `arg_is_jailed` — a path argument here is jailed by the same guard `exec`
//! itself relies on, not a second copy of it.

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

/// One commit touching a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

// Record/field separators unlikely to appear in a commit subject, so a log
// entry can be split back apart without ambiguity.
const FIELD_SEP: &str = "\x1f";
const RECORD_SEP: &str = "\x1e";

/// The diff of uncommitted changes, optionally scoped to one path.
pub fn git_diff(root: &Root, path: Option<&str>) -> Result<String, ForgeError> {
    let mut args = vec!["diff".to_string()];
    if let Some(p) = path {
        args.push("--".to_string());
        args.push(p.to_string());
    }
    let out = exec(root, Program::Git, &args, EXEC_DEADLINE)?;
    Ok(out.stdout)
}

fn parse_log(raw: &str) -> Vec<CommitEntry> {
    raw.split(RECORD_SEP)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let mut fields = record.splitn(4, FIELD_SEP);
            Some(CommitEntry {
                hash: fields.next()?.to_string(),
                author: fields.next()?.to_string(),
                date: fields.next()?.to_string(),
                subject: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// Recent commits touching `path` (or the whole tree when absent), newest first.
pub fn git_log(
    root: &Root,
    path: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CommitEntry>, ForgeError> {
    let n = limit.unwrap_or(20).clamp(1, 200);
    let mut args = vec![
        "log".to_string(),
        format!("-n{n}"),
        "--date=short".to_string(),
        format!("--format=%H{FIELD_SEP}%an{FIELD_SEP}%ad{FIELD_SEP}%s{RECORD_SEP}"),
    ];
    if let Some(p) = path {
        args.push("--".to_string());
        args.push(p.to_string());
    }
    let out = exec(root, Program::Git, &args, EXEC_DEADLINE)?;
    Ok(parse_log(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A real one-commit repo with an uncommitted change on top, so both
    /// `git_diff` and `git_log` have something real to see.
    fn fixture_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "add a.txt"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        dir
    }

    #[test]
    fn git_diff_shows_the_uncommitted_change() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let diff = git_diff(&root, None).unwrap();
        assert!(diff.contains("+two"), "diff was: {diff:?}");
    }

    #[test]
    fn git_diff_scopes_to_the_given_path() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        std::fs::write(dir.path().join("untouched.txt"), "x\n").unwrap();
        let diff = git_diff(&root, Some("a.txt")).unwrap();
        assert!(diff.contains("+two"));
        assert!(!diff.contains("untouched.txt"));
    }

    #[test]
    fn git_diff_refuses_a_path_escaping_the_worktree() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let err = git_diff(&root, Some("../../etc/passwd"));
        assert!(matches!(err, Err(ForgeError::Rejected(_))), "got {err:?}");
    }

    #[test]
    fn git_log_lists_hash_author_date_and_subject() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let log = git_log(&root, None, None).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].author, "Test");
        assert_eq!(log[0].subject, "add a.txt");
        assert_eq!(log[0].hash.len(), 40, "a full commit hash: {:?}", log[0].hash);
    }

    #[test]
    fn git_log_respects_the_limit() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let log = git_log(&root, None, Some(0)).unwrap();
        // clamp(1, 200): 0 becomes 1, not "no commits".
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn git_log_refuses_a_path_escaping_the_worktree() {
        let dir = fixture_repo();
        let root = Root::new(dir.path());
        let err = git_log(&root, Some("/etc/passwd"), None);
        assert!(matches!(err, Err(ForgeError::Rejected(_))), "got {err:?}");
    }
}
