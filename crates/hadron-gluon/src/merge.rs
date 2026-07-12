//! The merge gate's **effects**: running the workspace tests, and landing a branch.
//!
//! The split is forced by `hadron-gatekeeper`'s own contract — that crate is
//! "intentionally offline and side-effect-free", so it cannot shell out to `cargo
//! test` or `git merge`. It keeps the *decision* ([`hadron_gatekeeper::merge_decision`],
//! a truth table); this module keeps everything that touches the world.
//!
//! Everything here is behind the [`MergeRunner`] trait, and the engine's gate is
//! `None` by default. That is not fastidiousness: the production runner spawns
//! `cargo test --workspace`, and a unit test that reached it would recurse into the
//! very suite it is running from. Tests inject a fake; only the daemon seats
//! [`CargoMergeRunner`].

use std::path::Path;

use async_trait::async_trait;

use crate::snapshot::{git, git_ok};
use crate::worktree::Worktree;

/// How a branch reached the default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// The clean case: the default branch had not moved, so history stays linear.
    FastForward,
    /// The default branch moved while the quark worked (another quark landed
    /// first) — the branch was rebased onto it and then fast-forwarded. Under
    /// concurrency this is the *common* case, not the edge case.
    RebasedThenFastForward,
    /// The rebase hit conflicts a machine must not resolve. Nothing was landed and
    /// nothing was lost: the branch is exactly where the quark left it.
    Conflicted(String),
}

impl Landed {
    pub fn describe(&self, branch: &str, base: &str) -> String {
        match self {
            Landed::FastForward => format!("✅ merged `{branch}` → `{base}` (fast-forward)."),
            Landed::RebasedThenFastForward => format!(
                "✅ merged `{branch}` → `{base}` (`{base}` had moved, so the branch was rebased \
                 onto it and re-tested first)."
            ),
            Landed::Conflicted(err) => format!(
                "⚠️ could not merge `{branch}` → `{base}`: rebasing onto `{base}` conflicts, and \
                 a machine must not guess at a resolution. The branch is untouched.\n\n{err}"
            ),
        }
    }
}

/// The seam. `tests` is async (a real one takes minutes); `land` is not (git is
/// fast, and it must be serialized against the default branch anyway).
#[async_trait]
pub trait MergeRunner: Send + Sync {
    /// Run the workspace tests **in the quark's worktree**, on the branch as it now
    /// stands. Returns (passed, a tail of the output for the human).
    async fn tests(&self, wt: &Worktree) -> anyhow::Result<(bool, String)>;

    /// Land the branch on `base`, in `repo_root`.
    fn land(&self, repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Landed>;
}

/// True when the repo has any remote configured. FALSE for hadron today (`git
/// remote -v` is empty), which is why the local `--ff-only` path is the live one
/// and `git push` + `gh pr create` is the dormant branch.
pub fn has_remote(repo_root: &Path) -> bool {
    git_ok(repo_root, &["remote"])
        .ok()
        .flatten()
        .is_some_and(|out| !out.trim().is_empty())
}

/// The production runner: `cargo test --workspace` and a local `--ff-only` merge.
pub struct CargoMergeRunner;

#[async_trait]
impl MergeRunner for CargoMergeRunner {
    async fn tests(&self, wt: &Worktree) -> anyhow::Result<(bool, String)> {
        run_tests_with(wt, "cargo", &["test", "--workspace"]).await
    }

    fn land(&self, repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Landed> {
        land(repo_root, wt, base)
    }
}

/// How many bytes of test output to carry back to the human. The tail, not the
/// head: a failure's cause is at the end.
const TAIL_BYTES: usize = 4000;

fn tail(s: &str) -> String {
    let start = s.len().saturating_sub(TAIL_BYTES);
    s[start..].to_string()
}

/// Run an arbitrary command as "the tests", in the worktree.
///
/// The command is a parameter so this is testable without spawning a real `cargo
/// test` (which, inside this workspace's own suite, would recurse). The production
/// call site pins `cargo test --workspace`.
pub async fn run_tests_with(
    wt: &Worktree,
    program: &str,
    args: &[&str],
) -> anyhow::Result<(bool, String)> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .current_dir(&wt.path)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run the gate's tests ({program}): {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), tail(&text)))
}

/// Land `wt.branch` on `base` in the parent repo.
///
/// **`--ff-only`, never a merge commit.** The gate's whole value is that the
/// default branch only ever contains tested, approved, linear work; a merge commit
/// would let two individually-green branches combine into something nobody ran. If
/// `base` has moved, the branch is rebased onto it — in the quark's own worktree,
/// so the parent checkout is never touched — and the fast-forward is retried.
///
/// A conflicting rebase is aborted and reported. Nothing is deleted: the branch
/// still points at exactly the commits the quark made.
pub fn land(repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Landed> {
    if let Ok(()) = ff_only(repo_root, &wt.branch) {
        return Ok(Landed::FastForward);
    }

    // `base` moved under us. Rebase the quark's branch onto it, in the quark's tree.
    if let Err(e) = git(&wt.path, &["rebase", base]) {
        // Leave no half-rebase behind for the next turn to trip over.
        let _ = git(&wt.path, &["rebase", "--abort"]);
        return Ok(Landed::Conflicted(format!("{e:#}")));
    }

    ff_only(repo_root, &wt.branch)?;
    Ok(Landed::RebasedThenFastForward)
}

fn ff_only(repo_root: &Path, branch: &str) -> anyhow::Result<()> {
    git(repo_root, &["merge", "--ff-only", branch])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::{self, tests::git_repo};
    use hadron_lattice::QuarkId;

    fn q(id: &str) -> QuarkId {
        QuarkId::new(id)
    }

    #[test]
    fn has_remote_is_false_for_a_bare_local_repo() {
        let repo = git_repo();
        assert!(!has_remote(repo.path()), "a fresh local repo has no remote");
    }

    /// The live path today: no remote, so a completed branch is fast-forwarded onto
    /// the default branch locally, and history stays linear.
    #[test]
    fn land_ff_merges_locally_when_there_is_no_remote() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "work\n").unwrap();
        let sha = worktree::commit_turn(&wt, "opus: did the work").unwrap().unwrap();

        assert_eq!(land(repo.path(), &wt, "main").unwrap(), Landed::FastForward);

        // main now IS the quark's commit — a fast-forward, so history is linear.
        assert_eq!(git(repo.path(), &["rev-parse", "main"]).unwrap(), sha);
        assert!(repo.path().join("a.txt").exists(), "the work reached the human's tree");
        let merges = git(repo.path(), &["log", "--merges", "--oneline"]).unwrap();
        assert!(merges.is_empty(), "no merge commit: {merges}");
    }

    /// The COMMON case under concurrency: a sibling landed first, so `main` moved and
    /// `--ff-only` refuses. The gate rebases onto the new `main` and retries — it does
    /// NOT fall back to a merge commit, and it does not lose the quark's work.
    #[test]
    fn land_rebases_and_retries_when_the_default_branch_moved() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "opus work\n").unwrap();
        worktree::commit_turn(&wt, "opus: work").unwrap().unwrap();

        // A sibling lands on main first (a different file — no conflict).
        std::fs::write(repo.path().join("sibling.txt"), "agy work\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "agy: landed first"]).unwrap();

        assert_eq!(
            land(repo.path(), &wt, "main").unwrap(),
            Landed::RebasedThenFastForward,
            "a plain --ff-only would have failed here"
        );

        // Both quarks' work is on main, and history is still linear.
        assert!(repo.path().join("a.txt").exists());
        assert!(repo.path().join("sibling.txt").exists());
        assert!(git(repo.path(), &["log", "--merges", "--oneline"]).unwrap().is_empty());
    }

    /// A rebase a machine must not resolve. Report it; preserve the branch.
    #[test]
    fn a_conflicting_rebase_is_reported_and_the_branch_is_preserved() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("f.txt"), "opus version\n").unwrap();
        let sha = worktree::commit_turn(&wt, "opus: edit f").unwrap().unwrap();

        // main edits the SAME file, differently.
        std::fs::write(repo.path().join("f.txt"), "human version\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "human: edit f"]).unwrap();

        let landed = land(repo.path(), &wt, "main").unwrap();
        assert!(matches!(landed, Landed::Conflicted(_)), "got {landed:?}");

        // NOTHING was destroyed: the branch still points at the quark's commit, and
        // the tree is not left mid-rebase.
        assert_eq!(git(repo.path(), &["rev-parse", "quark/opus/01AAA"]).unwrap(), sha);
        // The worktree's real git dir — NOT `<path>/.git`, which in a linked worktree
        // is a file, so joining onto it would make this assertion vacuously true.
        let gitdir = crate::snapshot::git_dir(&wt.path).unwrap();
        assert!(!gitdir.join("rebase-merge").exists(), "no half-rebase left behind");
        assert!(!gitdir.join("rebase-apply").exists(), "no half-rebase left behind");
        // And the human's own file is untouched.
        assert_eq!(
            std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
            "human version\n"
        );
    }

    /// The gate's tests run **in the quark's worktree** — not in the daemon's cwd,
    /// and not in the human's checkout. Proven with a command that reports where it
    /// ran, so no real `cargo test` is spawned (that would recurse into this suite).
    #[tokio::test]
    async fn the_gate_runs_its_tests_inside_the_quarks_worktree() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();

        let (passed, out) = run_tests_with(&wt, "pwd", &[]).await.unwrap();
        assert!(passed);
        let ran_in = std::path::PathBuf::from(out.trim());
        assert_eq!(
            ran_in.canonicalize().unwrap(),
            wt.path.canonicalize().unwrap(),
            "the gate must test the quark's branch, in the quark's tree"
        );
    }

    #[tokio::test]
    async fn red_tests_are_reported_as_failure_with_their_output() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();

        let (passed, out) = run_tests_with(&wt, "sh", &["-c", "echo boom >&2; exit 1"])
            .await
            .unwrap();
        assert!(!passed, "a nonzero exit is a red suite");
        assert!(out.contains("boom"), "the human gets the output: {out}");
    }
}
