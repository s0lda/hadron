use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context};
use hadron_lattice::SnapshotRef;
use ulid::Ulid;

const SNAPSHOT_REF_PREFIX: &str = "refs/hadron/snapshots/";

/// Run `git` inside `repo_root` with explicit identity so snapshotting works
/// even when the repo has no configured user. Returns stdout on success.
fn git(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    git_with_env(repo_root, args, &[])
}

fn git_with_env(repo_root: &Path, args: &[&str], envs: &[(&str, &str)]) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_root).args(args);
    cmd.env("GIT_AUTHOR_NAME", "hadron")
        .env("GIT_AUTHOR_EMAIL", "hadron@localhost")
        .env("GIT_COMMITTER_NAME", "hadron")
        .env("GIT_COMMITTER_EMAIL", "hadron@localhost");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .with_context(|| format!("failed to spawn git {args:?}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True if HEAD resolves to a commit (i.e. the repo has at least one commit).
fn head_commit(repo_root: &Path) -> Option<String> {
    git(repo_root, &["rev-parse", "--verify", "HEAD"]).ok()
}

/// Snapshot the current worktree into a shadow ref without touching the user's
/// index or HEAD. Uses a throwaway index file, writes a tree, commit-trees it
/// (parented on HEAD when one exists), and points `refs/hadron/snapshots/<id>`
/// at the result.
pub fn create(repo_root: &Path, label: &str) -> anyhow::Result<SnapshotRef> {
    let id = Ulid::new().to_string();

    // Throwaway index so we never disturb the user's staging area.
    let tmp_index = repo_root.join(format!(".git/hadron-index-{id}"));
    let tmp_index_str = tmp_index.to_string_lossy().to_string();
    let env = [("GIT_INDEX_FILE", tmp_index_str.as_str())];

    // Stage everything into the throwaway index, then write its tree.
    git_with_env(repo_root, &["add", "-A"], &env)?;
    let tree = git_with_env(repo_root, &["write-tree"], &env)?;
    let _ = std::fs::remove_file(&tmp_index);

    // Commit the tree, parenting on HEAD if the project has history.
    let commit = match head_commit(repo_root) {
        Some(parent) => git(repo_root, &["commit-tree", &tree, "-p", &parent, "-m", label])?,
        None => git(repo_root, &["commit-tree", &tree, "-m", label])?,
    };

    // Park it under the shadow ref namespace.
    let refname = format!("{SNAPSHOT_REF_PREFIX}{id}");
    git(repo_root, &["update-ref", &refname, &commit])?;

    Ok(SnapshotRef { id, label: label.to_string(), commit })
}

/// List every hadron snapshot. Labels come from each snapshot commit's subject.
pub fn list(repo_root: &Path) -> anyhow::Result<Vec<SnapshotRef>> {
    let out = git(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(contents:subject)",
            "refs/hadron/snapshots/",
        ],
    )?;
    let mut refs = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(refname), Some(commit)) = (parts.next(), parts.next()) else {
            continue;
        };
        let label = parts.next().unwrap_or("").to_string();
        let id = refname
            .strip_prefix(SNAPSHOT_REF_PREFIX)
            .unwrap_or(refname)
            .to_string();
        refs.push(SnapshotRef { id, label, commit: commit.to_string() });
    }
    Ok(refs)
}

/// The current working diff against HEAD — what a quark has changed so far.
/// Feeds `Projection.git_diff`. Empty string when the repo has no commit yet.
pub fn working_diff(repo_root: &Path) -> anyhow::Result<String> {
    if head_commit(repo_root).is_none() {
        return Ok(String::new());
    }
    git(repo_root, &["diff", "HEAD"])
}

/// Restore the worktree to a snapshot (undo). Reverts tracked paths to the
/// snapshot's tree without moving HEAD or the branch. v1 limitation: files
/// created after the snapshot are left in place (documented; a hard clean is a
/// later concern).
pub fn restore(repo_root: &Path, snap: &SnapshotRef) -> anyhow::Result<()> {
    git(repo_root, &["restore", "--source", &snap.commit, "--worktree", "--", "."])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Make a temp git repo with one committed file. Returns the TempDir guard.
    fn repo_with_file(name: &str, contents: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]).unwrap();
        std::fs::write(root.join(name), contents).unwrap();
        git(root, &["add", name]).unwrap();
        git(root, &["commit", "-q", "-m", "initial"]).unwrap();
        dir
    }

    #[test]
    fn create_makes_a_shadow_ref_without_touching_head_or_index() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        let head_before = git(root, &["rev-parse", "HEAD"]).unwrap();

        let snap = create(root, "before edit").unwrap();
        assert!(!snap.commit.is_empty());
        assert_eq!(snap.label, "before edit");

        // HEAD unchanged; snapshot lives only under the shadow ref.
        assert_eq!(git(root, &["rev-parse", "HEAD"]).unwrap(), head_before);
        let refname = format!("{SNAPSHOT_REF_PREFIX}{}", snap.id);
        assert_eq!(git(root, &["rev-parse", &refname]).unwrap(), snap.commit);
        // The user's index/status is clean (nothing staged by snapshotting).
        assert_eq!(git(root, &["status", "--porcelain"]).unwrap(), "");
    }

    #[test]
    fn snapshot_captures_uncommitted_changes() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        // Dirty the worktree, then snapshot.
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        std::fs::write(root.join("new.txt"), "fresh\n").unwrap();
        let snap = create(root, "dirty state").unwrap();

        // The snapshot tree contains the modified + new file contents.
        let a = git(root, &["show", &format!("{}:a.txt", snap.commit)]).unwrap();
        let n = git(root, &["show", &format!("{}:new.txt", snap.commit)]).unwrap();
        assert_eq!(a, "two");
        assert_eq!(n, "fresh");
    }

    #[test]
    fn list_returns_created_snapshots() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        let s1 = create(root, "first").unwrap();
        let s2 = create(root, "second").unwrap();

        let listed = list(root).unwrap();
        let ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&s1.id.as_str()));
        assert!(ids.contains(&s2.id.as_str()));
        let labels: Vec<&str> = listed.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"first"));
        assert!(labels.contains(&"second"));
    }

    #[test]
    fn working_diff_shows_uncommitted_edits() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        assert_eq!(working_diff(root).unwrap(), "");
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        let diff = working_diff(root).unwrap();
        assert!(diff.contains("a.txt"));
        assert!(diff.contains("+changed"));
    }

    #[test]
    fn restore_reverts_worktree_to_snapshot() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        // Snapshot the clean state, then mutate the file.
        let snap = create(root, "clean").unwrap();
        std::fs::write(root.join("a.txt"), "corrupted\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "corrupted\n");

        restore(root, &snap).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
    }
}
