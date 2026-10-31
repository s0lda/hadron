use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context};
use hadron_lattice::SnapshotRef;
use ulid::Ulid;

const SNAPSHOT_REF_PREFIX: &str = "refs/hadron/snapshots/";

/// Run `git` inside `repo_root` with explicit identity so snapshotting works
/// even when the repo has no configured user. Returns stdout on success.
///
/// `pub(crate)` so `worktree.rs` and `merge.rs` reuse the one git wrapper (with
/// its pinned identity) instead of growing a third `Command::new("git")`.
pub fn git(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    git_with_env(repo_root, args, &[])
}

/// Like [`git`], but returns `Err` only on spawn failure — a nonzero exit is
/// reported as `Ok(None)`. For the many probe-shaped git calls (`does this ref
/// exist?`, `is HEAD detached?`) where "it failed" IS the answer.
pub(crate) fn git_ok(repo_root: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    match git(repo_root, args) {
        Ok(out) => Ok(Some(out)),
        Err(_) => Ok(None),
    }
}

/// How long any single `git` invocation may take before it is killed.
///
/// Deliberately far above a healthy git call and far below
/// [`crate::merge::GATE_TEST_DEADLINE`]: the gate's rebase and its `commits_ahead`
/// probes run BEFORE the tests, so a hang here is additive to a turn AND invisible —
/// it happens after the "gating…" notice and before any other field append.
pub(crate) const GIT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

/// Run `cmd` under a wall-clock `deadline`, killing its process GROUP on expiry.
///
/// A thin adapter over [`hadron_forge::exec::run_bounded`], which owns the spawn,
/// the draining wait and the group kill — `git` is a launcher (hooks, `git
/// rebase`'s own sub-gits), so killing the leader alone would orphan its children
/// exactly the way killing `cargo test` alone once orphaned a test binary for four
/// CPU-hours. What this adds is the *contract every git caller here wants*: a
/// timeout is an `Err`, because a git command that did not finish has no output
/// worth reading.
fn run_bounded(
    cmd: Command,
    deadline: std::time::Duration,
    args: &[&str],
) -> anyhow::Result<hadron_forge::exec::BoundedOutput> {
    let label = format!("git {args:?}");
    let out = hadron_forge::exec::run_bounded(cmd, deadline, &label)
        .with_context(|| format!("failed to spawn git {args:?}"))?;
    if out.timed_out {
        return Err(anyhow!("git {args:?} timed out after {deadline:?} and was killed"));
    }
    Ok(out)
}

/// Guard `git push` invocations so Hadron never publishes internal `archive/*` tags
/// or any non-release tag (only `v<major>.<minor>.<patch>` tags may be pushed).
/// Remote ref deletions (`--delete` / `-d` / `:ref`) are permitted so stale remote tags
/// can still be cleaned up.
fn validate_push_args(args: &[&str]) -> anyhow::Result<()> {
    if args.first().copied() != Some("push") {
        return Ok(());
    }
    let is_delete_flag = args.iter().any(|&a| a == "--delete" || a == "-d");
    for &arg in &args[1..] {
        if matches!(arg, "--tags" | "--follow-tags" | "--all" | "--mirror") {
            return Err(anyhow!(
                "refusing `git push {arg}`: bulk ref/tag pushes are forbidden to prevent publishing internal archive/* tags"
            ));
        }
        if arg.starts_with('-') {
            continue;
        }
        if is_delete_flag || arg.starts_with(':') {
            continue;
        }
        let target_ref = arg
            .rsplit_once(':')
            .map(|(_, dst)| dst)
            .unwrap_or(arg)
            .strip_prefix('+')
            .unwrap_or(arg);
        if target_ref.starts_with("archive/") || target_ref.starts_with("refs/tags/archive/") {
            return Err(anyhow!(
                "refusing to push internal archive tag `{arg}` to remote"
            ));
        }
        if let Some(tag_name) = target_ref.strip_prefix("refs/tags/") {
            if !is_release_version_tag(tag_name) {
                return Err(anyhow!(
                    "refusing to push non-release tag `{arg}` (only vX.Y.Z release tags may be published)"
                ));
            }
        }
    }
    Ok(())
}

fn is_release_version_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let mut parts = rest.split('.');
    let (Some(maj), Some(min), Some(pat), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !maj.is_empty()
        && !min.is_empty()
        && !pat.is_empty()
        && maj.bytes().all(|b| b.is_ascii_digit())
        && min.bytes().all(|b| b.is_ascii_digit())
        && pat.bytes().all(|b| b.is_ascii_digit())
}

pub fn git_with_env(repo_root: &Path, args: &[&str], envs: &[(&str, &str)]) -> anyhow::Result<String> {
    validate_push_args(args)?;
    let cmd = if let Some((distro, linux_cwd)) = hadron_lattice::wsl::parse_wsl_unc_path(repo_root) {
        let mut c = Command::new("wsl.exe");
        c.args([
            "-d",
            &distro,
            "--cd",
            &linux_cwd.to_string_lossy().replace('\\', "/"),
            "--exec",
            "env",
            "GIT_AUTHOR_NAME=hadron",
            "GIT_AUTHOR_EMAIL=hadron@localhost",
            "GIT_COMMITTER_NAME=hadron",
            "GIT_COMMITTER_EMAIL=hadron@localhost",
            "GIT_TERMINAL_PROMPT=0",
        ]);
        for (k, v) in envs {
            let translated_v = hadron_lattice::wsl::translate_arg_for_wsl(&distro, v);
            c.arg(format!("{k}={translated_v}"));
        }
        c.arg("git");
        for a in args {
            c.arg(hadron_lattice::wsl::translate_arg_for_wsl(&distro, a));
        }
        c
    } else {
        let clean_root = hadron_lattice::sys::paths::simplified(repo_root);
        let mut c = Command::new("git");
        c.arg("-C").arg(clean_root);
        let clean_args: Vec<String> = args
            .iter()
            .map(|a| hadron_lattice::sys::paths::strip_unc_prefix(a))
            .collect();
        c.args(&clean_args);
        c.env("GIT_AUTHOR_NAME", "hadron")
            .env("GIT_AUTHOR_EMAIL", "hadron@localhost")
            .env("GIT_COMMITTER_NAME", "hadron")
            .env("GIT_COMMITTER_EMAIL", "hadron@localhost")
            // Never block on a human who is not there: the daemon has no terminal, so a
            // credential prompt would wait forever instead of failing.
            .env("GIT_TERMINAL_PROMPT", "0");
        for (k, v) in envs {
            c.env(k, v);
        }
        c
    };
    let out = run_bounded(cmd, GIT_DEADLINE, args)?;
    let stdout = hadron_lattice::wsl::strip_wsl_noise(&String::from_utf8_lossy(&out.stdout));
    let stderr = hadron_lattice::wsl::strip_wsl_noise(&String::from_utf8_lossy(&out.stderr));
    if !out.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            stderr
        ));
    }
    if args.first() == Some(&"push") {
        if stdout.is_empty() {
            Ok(stderr)
        } else if stderr.is_empty() {
            Ok(stdout)
        } else {
            Ok(format!("{stdout}\n{stderr}"))
        }
    } else {
        Ok(stdout)
    }
}

/// True if HEAD resolves to a commit (i.e. the repo has at least one commit).
fn head_commit(repo_root: &Path) -> Option<String> {
    git(repo_root, &["rev-parse", "--verify", "HEAD"]).ok()
}

/// The directory git actually keeps this checkout's private state in — its index,
/// its in-progress rebase, its refs.
///
/// **Never `path.join(".git")`.** That is only true of a *main* checkout. In a linked
/// worktree — which is now every place a quark works — `.git` is a plain FILE holding
/// `gitdir: <repo>/.git/worktrees/<name>`, so joining onto it yields a path under a
/// non-directory and every write there fails with `Not a directory`. Ask git instead:
/// `--absolute-git-dir` answers `<root>/.git` for a main checkout (byte-identical to
/// the old behaviour) and `<root>/.git/worktrees/<name>` inside a worktree. It is also
/// per-worktree, so two concurrent snapshots cannot collide on one index file.
pub(crate) fn git_dir(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let raw = git(path, &["rev-parse", "--absolute-git-dir"])?;
    if let Some((distro, _)) = hadron_lattice::wsl::parse_wsl_unc_path(path) {
        if raw.starts_with('/') {
            return Ok(hadron_lattice::wsl::format_wsl_unc_path(&distro, Path::new(&raw)));
        }
    }
    Ok(std::path::PathBuf::from(raw))
}

/// The **main** checkout's root, asked from anywhere — including a linked worktree.
///
/// The sibling of [`git_dir`], and the opposite question: `git_dir` answers "where is
/// *this* checkout's private state", this answers "where is the repo every worktree
/// shares". `--git-common-dir` is `<root>/.git` from a main checkout *and* from a
/// linked worktree, so its parent is the root in both cases. `--path-format=absolute`
/// because git otherwise answers a *relative* path from inside a worktree.
pub(crate) fn main_repo_root(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::path::PathBuf>>,
    > = std::sync::OnceLock::new();

    if path.join(".git").exists() {
        let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        if let Ok(guard) = cache.lock() {
            if let Some(cached) = guard.get(path) {
                if cached.join(".git").exists() {
                    return Ok(cached.clone());
                }
            }
        }
    }

    let raw = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = if let Some((distro, _)) = hadron_lattice::wsl::parse_wsl_unc_path(path) {
        if raw.starts_with('/') {
            hadron_lattice::wsl::format_wsl_unc_path(&distro, Path::new(&raw))
        } else {
            std::path::PathBuf::from(raw)
        }
    } else {
        std::path::PathBuf::from(raw)
    };
    let root = common
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("git common dir has no parent: {}", common.display()))?;

    if path.join(".git").exists() && root.join(".git").exists() {
        let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
        if let Ok(mut guard) = cache.lock() {
            guard.insert(path.to_path_buf(), root.clone());
        }
    }

    Ok(root)
}

const ATOMIC_SNAPSHOT_CREATE_SCRIPT: &str = r#"set -e
export GIT_AUTHOR_NAME="hadron"
export GIT_AUTHOR_EMAIL="hadron@localhost"
export GIT_COMMITTER_NAME="hadron"
export GIT_COMMITTER_EMAIL="hadron@localhost"
export GIT_TERMINAL_PROMPT="0"
id="$1"
label="$2"
refname="$3"
gitdir="$(git rev-parse --absolute-git-dir)"
tmp_index="$gitdir/hadron-index-$id"
trap 'rm -f "$tmp_index"' EXIT
GIT_INDEX_FILE="$tmp_index" git add -A
tree="$(GIT_INDEX_FILE="$tmp_index" git write-tree)"
rm -f "$tmp_index"
if parent="$(git rev-parse --verify HEAD 2>/dev/null)"; then
    commit="$(git commit-tree "$tree" -p "$parent" -m "$label")"
else
    commit="$(git commit-tree "$tree" -m "$label")"
fi
git update-ref "$refname" "$commit"
printf '%s\n' "$commit"
"#;

fn create_atomic_sh(
    repo_root: &Path,
    id: &str,
    label: &str,
    refname: &str,
) -> Option<anyhow::Result<String>> {
    let mut cmd = if let Some((distro, linux_cwd)) = hadron_lattice::wsl::parse_wsl_unc_path(repo_root) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut c = Command::new("wsl.exe");
            c.args([
                "-d",
                &distro,
                "--cd",
                &linux_cwd.to_string_lossy().replace('\\', "/"),
                "--exec",
                "sh",
                "-c",
                ATOMIC_SNAPSHOT_CREATE_SCRIPT,
                "sh",
                id,
                label,
                refname,
            ]);
            c.creation_flags(0x08000000);
            c
        }
        #[cfg(not(windows))]
        {
            let _ = distro;
            let mut c = Command::new("sh");
            c.current_dir(linux_cwd).args([
                "-c",
                ATOMIC_SNAPSHOT_CREATE_SCRIPT,
                "sh",
                id,
                label,
                refname,
            ]);
            c
        }
    } else if cfg!(unix) {
        let mut c = Command::new("sh");
        c.current_dir(repo_root).args([
            "-c",
            ATOMIC_SNAPSHOT_CREATE_SCRIPT,
            "sh",
            id,
            label,
            refname,
        ]);
        c
    } else {
        return None;
    };

    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let out = match run_bounded(cmd, GIT_DEADLINE, &["snapshot-create"]) {
        Ok(o) => o,
        Err(e) => return Some(Err(e)),
    };
    let stdout = hadron_lattice::wsl::strip_wsl_noise(&String::from_utf8_lossy(&out.stdout));
    let stderr = hadron_lattice::wsl::strip_wsl_noise(&String::from_utf8_lossy(&out.stderr));
    if !out.success() {
        return Some(Err(anyhow!("snapshot create failed: {stderr}")));
    }
    let commit = stdout.lines().last().unwrap_or("").trim().to_string();
    if commit.is_empty() {
        return None;
    }
    Some(Ok(commit))
}

/// Snapshot the current worktree into a shadow ref without touching the user's
/// index or HEAD. Uses a throwaway index file, writes a tree, commit-trees it
/// (parented on HEAD when one exists), and points `refs/hadron/snapshots/<id>`
/// at the result.
pub fn create(repo_root: &Path, label: &str) -> anyhow::Result<SnapshotRef> {
    let id = Ulid::new().to_string();
    let refname = format!("{SNAPSHOT_REF_PREFIX}{id}");

    if let Some(res) = create_atomic_sh(repo_root, &id, label, &refname) {
        let commit = res?;
        return Ok(SnapshotRef {
            id,
            label: label.to_string(),
            commit,
        });
    }

    // Fallback multi-step path for non-WSL Windows hosts without `sh`.
    // Throwaway index so we never disturb the user's staging area. It lives in the
    // checkout's REAL git dir (see `git_dir`) — in a quark's worktree that is not
    // `<path>/.git`, which is a file there, not a directory.
    let tmp_index = git_dir(repo_root)?.join(format!("hadron-index-{id}"));
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

    /// **The dispatch loop's unbounded window.** `merge_gate` runs `sync` (a `git rebase`)
    /// and its `commits_ahead`/`is_dirty` probes BEFORE the deadline-protected test run,
    /// and every one of them went through this module's blocking `output()`. A git that
    /// never returns — a hook waiting on stdin, a credential prompt, a stale lock — wedged
    /// the daemon with the "gating…" notice already posted and nothing after it, which is
    /// exactly what `my-cloud`'s field showed three times on one branch. Nothing in the
    /// engine could save it: `GATE_TEST_DEADLINE` wraps only the tests, and `TURN_DEADLINE`
    /// wraps only `quark.excite`.
    #[test]
    fn a_command_that_outlives_its_deadline_is_killed_and_reported() {
        let started = std::time::Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");

        let err = run_bounded(cmd, std::time::Duration::from_millis(300), &["sleep", "30"])
            .expect_err("a 30s sleep under a 300ms deadline must not succeed");

        assert!(
            err.to_string().contains("timed out"),
            "the error must say it timed out, got: {err}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "it waited {:?} — the deadline did not cut it",
            started.elapsed()
        );
    }

    /// The bound must not change the answer for a command that finishes normally:
    /// stdout still comes back, and a nonzero exit is still an `Err`.
    #[test]
    fn a_command_inside_its_deadline_is_unaffected() {
        let dir = repo_with_file("a.txt", "hi");
        let out = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
        assert!(!out.is_empty(), "a normal git call must still return its stdout");
        assert!(
            git(dir.path(), &["rev-parse", "--verify", "refs/heads/nope"]).is_err(),
            "a nonzero exit must still be an Err"
        );
    }

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

    #[test]
    fn validate_push_args_blocks_archive_and_bulk_tag_pushes() {
        assert!(validate_push_args(&["push", "origin", "HEAD"]).is_ok());
        assert!(validate_push_args(&["push", "origin", "refs/tags/v0.33.9"]).is_ok());
        assert!(validate_push_args(&["push", "origin", "--delete", "archive/cli-agy-01M5"]).is_ok());
        assert!(validate_push_args(&["push", "origin", ":refs/tags/archive/cli-agy-01M5"]).is_ok());

        assert!(validate_push_args(&["push", "origin", "--tags"]).is_err());
        assert!(validate_push_args(&["push", "origin", "--follow-tags"]).is_err());
        assert!(validate_push_args(&["push", "origin", "--mirror"]).is_err());
        assert!(validate_push_args(&["push", "origin", "archive/cli-agy-01M5"]).is_err());
        assert!(validate_push_args(&["push", "origin", "refs/tags/archive/cli-agy-01M5"]).is_err());
        assert!(validate_push_args(&["push", "origin", "refs/tags/some-scratch-tag"]).is_err());
    }
}
