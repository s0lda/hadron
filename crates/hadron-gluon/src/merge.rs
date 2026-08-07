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

/// How a branch was brought up to date with `base` **before** the gate tested it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Synced {
    /// `base` had not moved since the branch was cut — nothing to replay.
    AlreadyCurrent,
    /// `base` moved; the branch was replayed on top of it. It may now be EMPTY —
    /// every commit already applied upstream by another route — which is a no-op
    /// land, not a failure.
    Rebased,
    /// The rebase hit conflicts a machine must not resolve. It was aborted; the
    /// branch is exactly where the quark left it.
    Conflicted(String),
}

/// How a branch reached the default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// Every commit was already on `base` — the work arrived by another route (a
    /// cherry-pick, a sibling branch), so nothing was merged. Reporting this as a
    /// fast-forward would tell the human a merge happened that did not.
    AlreadyLanded,
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
            Landed::AlreadyLanded => format!(
                "nothing to merge: every commit on `{branch}` is already on `{base}`."
            ),
            Landed::FastForward => format!("merged `{branch}` → `{base}` (fast-forward)."),
            Landed::RebasedThenFastForward => format!(
                "merged `{branch}` → `{base}` (`{base}` had moved, so the branch was rebased \
                 onto it and re-tested first)."
            ),
            Landed::Conflicted(err) => format!(
                "could not merge `{branch}` → `{base}`: rebasing onto `{base}` conflicts, and \
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

    /// Replay the branch on top of `base`, **before** `tests` runs. Defaults to the
    /// real git rebase: a fake runner exists to stub the expensive `tests`, and a
    /// fake that also skipped the rebase would stop testing the thing that lands.
    fn sync(&self, wt: &Worktree, base: &str) -> Synced {
        sync(wt, base)
    }

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

/// Pick the test command from the worktree's own manifest. Checked in a fixed
/// order because a repo can carry more than one manifest (a Rust workspace with
/// a `package.json` for its docs site is the common case), and Cargo is checked
/// FIRST: a `Cargo.toml` beside a `package.json` used to resolve to `npm test`,
/// which would have run the wrong suite on Hadron's own repo the day anyone added
/// a docs site. Cargo is also the fallback when no manifest is recognised at all —
/// a wrong-but-loud `cargo` failure beats silently gating on nothing.
pub fn detect_runner(worktree_path: &Path) -> (&'static str, &'static [&'static str]) {
    if worktree_path.join("Cargo.toml").is_file() {
        ("cargo", &["test", "--workspace"])
    } else if worktree_path.join("package.json").is_file() {
        ("npm", &["test"])
    } else if worktree_path.join("pyproject.toml").is_file()
        || worktree_path.join("setup.py").is_file()
    {
        ("pytest", &[])
    } else if worktree_path.join("go.mod").is_file() {
        ("go", &["test", "./..."])
    } else {
        ("cargo", &["test", "--workspace"])
    }
}

/// The production runner: `cargo test --workspace` and a local `--ff-only` merge.
pub struct CargoMergeRunner;

#[async_trait]
impl MergeRunner for CargoMergeRunner {
    async fn tests(&self, wt: &Worktree) -> anyhow::Result<(bool, String)> {
        let (program, args) = detect_runner(&wt.path);
        run_tests_with(wt, program, args).await
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

/// How long the gate will wait for a project's test suite before cutting it off.
///
/// **The gate runs a command nobody in this repo wrote.** `detect_runner` picks it
/// from the target project's own manifest, so a hung test in the human's project is
/// a hung `await` in the daemon — and the gate sits in the dispatch path *ahead* of
/// `worktree::ensure`, the snapshot and the `Status{Excited}` append, so a wedge here
/// writes no field event at all: every quark reads as "working…" forever with an
/// empty field and nothing to point at. `TURN_DEADLINE` does not cover this; it wraps
/// `quark.excite`, and the gate is outside it. Deliberately under that 30 minutes, so
/// a wedged gate stays inside the turn budget the human already expects.
///
/// Measured cause: a `cargo test --workspace` in a project whose own suite spun
/// forever kept a daemon from dispatching anything for hours.
pub const GATE_TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Run an arbitrary command as "the tests", in the worktree, under
/// [`GATE_TEST_DEADLINE`].
///
/// The command is a parameter so this is testable without spawning a real `cargo
/// test` (which, inside this workspace's own suite, would recurse). The production
/// call site pins `cargo test --workspace`.
pub async fn run_tests_with(
    wt: &Worktree,
    program: &str,
    args: &[&str],
) -> anyhow::Result<(bool, String)> {
    run_tests_within(wt, program, args, GATE_TEST_DEADLINE).await
}

/// [`run_tests_with`] with the deadline as a parameter. Tests use a tiny one.
pub async fn run_tests_within(
    wt: &Worktree,
    program: &str,
    args: &[&str],
    deadline: std::time::Duration,
) -> anyhow::Result<(bool, String)> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(&wt.path)
        // A test runner that reads stdin would otherwise block on the daemon's own
        // terminal, which is the same wedge by another route.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // The deadline arm below kills the group explicitly, but that is not the only
        // way this future dies: cancel the surrounding turn (a reboot, a shutdown) and
        // `wait_with_output` is dropped with the child still running. Same guarantee
        // `ProcessRunner` relies on (`engine/run.rs`'s watchdog comment).
        .kill_on_drop(true);

    // **The gate's cost lives or dies on this line** — and so does the disk. See
    // `worktree::shared_build_env` for why; the quark's own subprocess gets the very
    // same env, which is the point of it living in one place.
    cmd.envs(crate::worktree::shared_build_env(&wt.path));

    // Its own process group, so the deadline can kill the whole tree rather than just
    // the launcher. `cargo test` is a launcher: killing it orphans the test binary,
    // which is exactly the process that was still burning four CPU-hours after the
    // daemon that started it had given up on it.
    use hadron_lattice::sys::ConfigureProcessGroup;
    cmd.as_std_mut().set_process_group();

    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to run the gate's tests ({program}): {e}"))?;
    // Read before the handle is consumed by `wait_with_output`.
    let pid = child.id();
    if let Some(pid) = pid {
        crate::proc::register(pid);
    }

    let out = match tokio::time::timeout(deadline, child.wait_with_output()).await {
        Ok(out) => {
            if let Some(pid) = pid {
                crate::proc::unregister(pid);
            }
            out.map_err(|e| anyhow::anyhow!("failed to run the gate's tests ({program}): {e}"))?
        }
        // **Red, not `Err`.** An `Err` propagates out of `merge_gate` through
        // `run_until_quiesce`'s `?` into the daemon's "excite error (continuing)" arm,
        // which loops straight back onto the same branch and hangs again — a wedge
        // traded for a slow loop. A red gate is a state the engine already handles:
        // `Status{Blocked}` with a reason, the branch untouched, the quark excitable.
        Err(_) => {
            if let Some(pid) = pid {
                crate::proc::unregister(pid);
                kill_process_group(pid);
            }
            return Ok((
                false,
                format!(
                    "the gate's tests ({program}) did not finish within {}s and were killed. \
                     The branch is untouched and nothing was landed. A suite that hangs here \
                     stops the daemon dispatching ANY quark, so fix or exclude the hanging \
                     test before the next turn.",
                    deadline.as_secs(),
                ),
            ));
        }
    };
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), tail(&text)))
}

/// SIGKILL the whole process group led by `pid` — the launcher *and* everything it
/// spawned. Best-effort: a group that has already exited is not an error.
///
/// Re-exported, not reimplemented: `hadron_lattice::sys` runs process management under the
/// same rule, and two copies of a kill is one copy that stops matching.
pub(crate) use hadron_lattice::sys::kill_process_group;


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
    // Every commit already on `base`. `--ff-only` succeeds here ("Already up to
    // date") and would report a merge that never happened — the case a branch is in
    // after `sync` replayed it onto a `base` that had already absorbed its work.
    if git(repo_root, &["merge-base", "--is-ancestor", &wt.branch, base]).is_ok() {
        return Ok(Landed::AlreadyLanded);
    }

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

/// Bring `wt`'s branch up to date with `base` **before** the gate tests it.
///
/// **Why before, and not only inside [`land`].** The gate used to test the branch
/// exactly as the quark left it and rebase only after the tests passed. Two things
/// fell out of that order, both observed live:
///
/// - A branch cut *before* a fix landed on `base` was tested WITHOUT that fix, so it
///   was reported red on every retry and could never heal: the fix was on `base` and
///   the branch was never shown it. (`quark/acp-agy/01KYC48…` burned two identical
///   gate cycles on a `mod nucleus is private` error already fixed on `main`.)
/// - The reverse: a rebase inside `land` produced a tree nobody had run, while
///   `Landed::RebasedThenFastForward`'s message told the human it had been
///   "re-tested first". Rebasing first is what makes that sentence true.
///
/// A conflict is aborted and reported — a machine must not guess at a resolution —
/// and nothing is ever deleted: the branch still points at the quark's own commits.
pub fn sync(wt: &Worktree, base: &str) -> Synced {
    // `base` is already an ancestor of the branch ⇒ it has not moved under us.
    if git(&wt.path, &["merge-base", "--is-ancestor", base, "HEAD"]).is_ok() {
        return Synced::AlreadyCurrent;
    }
    if let Err(e) = git(&wt.path, &["rebase", base]) {
        // Leave no half-rebase behind for the next turn to trip over.
        let _ = git(&wt.path, &["rebase", "--abort"]);
        return Synced::Conflicted(format!("{e:#}"));
    }
    Synced::Rebased
}

/// Discard local uncommitted changes to `Cargo.lock` in `repo_root` ONLY if
/// `Cargo.lock` is the only modified file. `Cargo.lock` is derived from workspace
/// `Cargo.toml`s and local rust-analyzer/cargo execution frequently mutates it in the
/// target checkout, blocking fast-forward merges.
pub fn discard_local_lockfile_drift(repo_root: &Path) {
    if let Ok(Some(status)) = git_ok(repo_root, &["status", "--porcelain"]) {
        let lines: Vec<&str> = status
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.ends_with(".hadron") && !l.contains(".hadron/"))
            .collect();
        if lines.len() == 1 {
            let path_part = lines[0].split_whitespace().last().unwrap_or("");
            if path_part == "Cargo.lock" {
                if git(repo_root, &["checkout", "HEAD", "--", "Cargo.lock"]).is_err() {
                    let _ = std::fs::remove_file(repo_root.join("Cargo.lock"));
                }
            }
        }
    }
}

fn ff_only(repo_root: &Path, branch: &str) -> anyhow::Result<()> {
    discard_local_lockfile_drift(repo_root);
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

    /// **The reason `sync` exists.** A branch cut before a fix landed on `base` must
    /// be shown that fix BEFORE the gate tests it — otherwise it is tested against a
    /// tree that no longer exists, reported red, and can never heal itself: the fix is
    /// on `base` and the branch is never given it. Live cost: two identical gate cycles
    /// on `quark/acp-agy/01KYC48…` for an error `main` had already fixed.
    #[test]
    fn sync_replays_the_branch_onto_a_base_that_moved_so_the_tests_see_the_fix() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "opus work\n").unwrap();
        worktree::commit_turn(&wt, "opus: work").unwrap().unwrap();

        // The fix lands on main AFTER the branch was cut.
        std::fs::write(repo.path().join("fix.txt"), "the fix\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "the fix"]).unwrap();
        assert!(!wt.path.join("fix.txt").exists(), "the branch cannot see it yet");

        assert_eq!(sync(&wt, "main"), Synced::Rebased);
        assert!(wt.path.join("fix.txt").exists(), "the gate now tests the fixed tree");
        assert!(wt.path.join("a.txt").exists(), "and the quark's own work survives");
    }

    /// `base` has not moved: no rebase, no churn, and the branch is left alone.
    #[test]
    fn sync_is_a_no_op_when_the_base_has_not_moved() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "opus work\n").unwrap();
        let sha = worktree::commit_turn(&wt, "opus: work").unwrap().unwrap();

        assert_eq!(sync(&wt, "main"), Synced::AlreadyCurrent);
        assert_eq!(worktree::head(&wt.path).unwrap(), sha, "no commits were rewritten");
    }

    /// The work reached `base` by another route (someone cherry-picked it forward).
    /// The rebase empties the branch, and `land` must say so instead of reporting a
    /// fast-forward that never happened — `--ff-only` succeeds here ("Already up to
    /// date"), so without this the gate reports a merge it did not perform.
    #[test]
    fn a_branch_whose_work_already_landed_syncs_empty_and_lands_as_a_no_op() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "opus work\n").unwrap();
        let sha = worktree::commit_turn(&wt, "opus: work").unwrap().unwrap();

        // main moves on its own AND absorbs the same patch by cherry-pick, so the
        // copy on main is a different commit carrying an identical diff.
        std::fs::write(repo.path().join("other.txt"), "meanwhile\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "meanwhile, on main"]).unwrap();
        git(repo.path(), &["cherry-pick", &sha]).unwrap();
        assert_ne!(
            git(repo.path(), &["rev-parse", "main"]).unwrap(),
            sha,
            "the copy on main must be a DIFFERENT commit, or this proves nothing"
        );

        assert_eq!(sync(&wt, "main"), Synced::Rebased);
        assert_eq!(
            worktree::commits_ahead(&wt, "main").unwrap(),
            0,
            "the rebase drops a patch already upstream"
        );
        assert_eq!(land(repo.path(), &wt, "main").unwrap(), Landed::AlreadyLanded);
    }

    /// A rebase a machine must not resolve. Report it; preserve the branch.
    #[test]
    fn sync_reports_a_conflict_and_leaves_no_half_rebase() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("f.txt"), "opus version\n").unwrap();
        let sha = worktree::commit_turn(&wt, "opus: edit f").unwrap().unwrap();

        std::fs::write(repo.path().join("f.txt"), "human version\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "human: edit f"]).unwrap();

        assert!(matches!(sync(&wt, "main"), Synced::Conflicted(_)));
        assert_eq!(worktree::head(&wt.path).unwrap(), sha, "the branch is untouched");
        let gitdir = crate::snapshot::git_dir(&wt.path).unwrap();
        assert!(!gitdir.join("rebase-merge").exists(), "no half-rebase left behind");
        assert!(!gitdir.join("rebase-apply").exists(), "no half-rebase left behind");
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

    /// **The gate's cost.** A quark's worktree has no `target/` of its own, so without
    /// this the first `cargo test` in each quark's tree is a full cold build of the
    /// workspace — minutes each, and a duplicate 41 GB artifact tree per quark. The gate
    /// must point every worktree at the MAIN checkout's `target/`, or the enforcement it
    /// exists to provide costs more than it is worth.
    ///
    /// Asserted by having "the tests" print the variable, so no real cargo is spawned.
    #[tokio::test]
    async fn the_gate_reuses_the_main_checkouts_target_dir() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        // Both sides get canonicalized, so the dir has to exist to be resolved.
        std::fs::create_dir_all(repo.path().join("target")).unwrap();

        let (passed, out) = run_tests_with(&wt, "sh", &["-c", "echo $CARGO_TARGET_DIR"])
            .await
            .unwrap();
        assert!(passed);

        let got = std::path::PathBuf::from(out.trim());
        assert_eq!(
            got.canonicalize().unwrap(),
            repo.path().join("target").canonicalize().unwrap(),
            "the gate must build into the main checkout's target, not the worktree's"
        );
        // And emphatically NOT the worktree's own (cold, duplicated) target dir.
        assert_ne!(got, wt.path.join("target"));
    }

    /// **A hung suite must not wedge the daemon.** The gate runs a command chosen from
    /// the *target project's* manifest, on the dispatch path and ahead of every field
    /// append — so a suite that never returns stops the daemon dispatching anything at
    /// all, with no event written to point at. Measured live: a project whose own
    /// `cargo test` spun forever kept a daemon silent for hours.
    ///
    /// Two claims, and the second is the one that costs real machines: the gate gives
    /// up, and it kills the whole process TREE. `cargo test` is a launcher — killing
    /// only the launcher orphans the test binary, which is the process that was still
    /// burning CPU long after the daemon stopped waiting for it.
    #[tokio::test]
    async fn a_hung_suite_is_cut_at_the_deadline_and_its_whole_tree_killed() {
        let repo = git_repo();
        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        let marker = wt.path.join("still-alive");

        // A launcher that outlives nothing of its own but spawns a grandchild which
        // keeps touching a file — the shape of `cargo test` and its test binary.
        let script = format!(
            "sh -c 'while true; do date +%s%N > {}; sleep 0.05; done' & wait",
            marker.display()
        );
        let (passed, out) = run_tests_within(
            &wt,
            "sh",
            &["-c", &script],
            std::time::Duration::from_millis(400),
        )
        .await
        .unwrap();

        assert!(!passed, "a suite that never finishes is red, not green");
        assert!(
            out.contains("did not finish within"),
            "the human is told what happened: {out}"
        );

        // The grandchild must be dead: the marker stops moving.
        let first = std::fs::read_to_string(&marker).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let second = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(
            first, second,
            "the grandchild outlived the deadline — killing only the launcher orphans it"
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

    #[test]
    fn detect_runner_picks_cargo_for_a_rust_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        assert_eq!(detect_runner(dir.path()), ("cargo", &["test", "--workspace"][..]));
    }

    #[test]
    fn detect_runner_picks_npm_for_a_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runner(dir.path()), ("npm", &["test"][..]));
    }

    #[test]
    fn detect_runner_picks_pytest_for_a_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_runner(dir.path()), ("pytest", &[][..]));
    }

    #[test]
    fn detect_runner_picks_go_test_for_a_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x").unwrap();
        assert_eq!(detect_runner(dir.path()), ("go", &["test", "./..."][..]));
    }

    #[test]
    fn detect_runner_defaults_to_cargo_when_no_manifest_is_recognised() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_runner(dir.path()), ("cargo", &["test", "--workspace"][..]));
    }

    /// The order matters, and it used to be wrong: `package.json` was checked before
    /// `Cargo.toml`, so a Rust workspace that also carries a JS docs site would have
    /// gated on `npm test` instead of its own suite.
    #[test]
    fn detect_runner_prefers_cargo_over_a_sibling_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_runner(dir.path()), ("cargo", &["test", "--workspace"][..]));
    }

    #[test]
    fn land_discards_a_locally_regenerated_cargo_lock_before_merging() {
        let repo = git_repo();
        std::fs::write(repo.path().join("Cargo.lock"), "lock v1\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "init Cargo.lock"]).unwrap();

        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("Cargo.lock"), "lock v2 by opus\n").unwrap();
        worktree::commit_turn(&wt, "opus: update Cargo.lock").unwrap().unwrap();

        // Local drift in repo_root (only Cargo.lock is dirty)
        std::fs::write(repo.path().join("Cargo.lock"), "lock v1 mutated locally\n").unwrap();

        assert_eq!(land(repo.path(), &wt, "main").unwrap(), Landed::FastForward);
        assert_eq!(
            std::fs::read_to_string(repo.path().join("Cargo.lock")).unwrap(),
            "lock v2 by opus\n"
        );
    }

    #[test]
    fn land_still_refuses_when_a_real_file_is_dirty_in_repo_root() {
        let repo = git_repo();
        std::fs::write(repo.path().join("Cargo.lock"), "lock v1\n").unwrap();
        std::fs::write(repo.path().join("real.txt"), "real v1\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(repo.path(), &["commit", "-q", "-m", "init repo"]).unwrap();

        let wt = worktree::ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("real.txt"), "real v2 by opus\n").unwrap();
        worktree::commit_turn(&wt, "opus: work on real.txt").unwrap().unwrap();

        // Dirty real file in repo_root
        std::fs::write(repo.path().join("real.txt"), "real v1 dirty\n").unwrap();

        assert!(land(repo.path(), &wt, "main").is_err());
    }
}

