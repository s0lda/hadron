//! Worktree discipline: **one worktree per quark, one branch per assignment.**
//!
//! ```text
//! <repo>/                    ← the human's tree. No quark ever writes here.
//!   .hadron/                 ← gitignored, so the trees never dirty `git status`
//!     trees/
//!       opus/                ← git worktree, HEAD = quark/opus/<assignment-ulid>
//!       agy/                 ← git worktree, HEAD = quark/agy/<assignment-ulid>
//! ```
//!
//! The path is stable per quark (bounding disk cost at N *quarks*, not N
//! assignments, and keeping a resumed `claude --resume` session's project
//! directory from moving under it); the branch inside it changes per assignment.
//!
//! **Nothing here ever runs `git reset --hard`.** On a quark's 2nd assignment its
//! worktree still sits on the *previous* assignment's branch — a reset would drag
//! that branch's pointer to main and orphan every commit on it that the merge gate
//! has not landed yet. `checkout -b <new> <default>` branches from the default
//! branch *without touching* the old one. A crashed quark's half-finished work is
//! evidence, not garbage: it is reclaimed by a startup scan ([`reclaim`]), which
//! reports dirty trees and prunes only what git has already lost.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use hadron_lattice::QuarkId;

use crate::snapshot::{git, git_ok};

/// Where a quark works. Stable per quark; the branch inside it changes per assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub quark: QuarkId,
    pub path: PathBuf,
    pub branch: String,
}

/// What a startup scan found in a surviving worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reclaimed {
    pub quark: QuarkId,
    pub path: PathBuf,
    pub branch: String,
    /// Uncommitted work left behind by a crashed quark or a killed daemon.
    /// Reported, never destroyed.
    pub dirty: bool,
}

/// `.hadron/trees` — inside the repo, but gitignored, so a worktree checked out
/// `.hadron/trees` — inside the repo, but gitignored, so a worktree checked out
/// here contains no `.hadron/` of its own (the dir is in no commit) and cannot
/// recurse.
pub fn trees_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".hadron").join("trees")
}

/// The branch a quark's assignment branches *from*, and the branch it may never
/// sit on. Resolved, not hardcoded: `origin/HEAD` if a remote names one, else
/// whichever of `main`/`master` actually exists, else `init.defaultBranch`.
pub fn default_branch(repo_root: &Path) -> String {
    if let Some(sym) = git_ok(repo_root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).ok().flatten() {
        if let Some(name) = sym.trim().strip_prefix("origin/") {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    for candidate in ["main", "master"] {
        let refname = format!("refs/heads/{candidate}");
        if git_ok(repo_root, &["show-ref", "--verify", "--quiet", &refname]).ok().flatten().is_some() {
            return candidate.to_string();
        }
    }
    git_ok(repo_root, &["config", "init.defaultBranch"])
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

/// The branch name for one assignment: `quark/<id>/<assignment-ulid>`.
/// ULID-named, so two assignments can never collide — even across restarts.
pub fn branch_name(quark: &QuarkId, assignment: &str) -> String {
    format!("quark/{}/{}", quark.as_str(), assignment)
}

/// Parse assignment ULID from a branch name (`quark/<id>/<ulid>`).
pub fn parse_assignment_from_branch(branch: &str) -> Option<ulid::Ulid> {
    let parts: Vec<&str> = branch.split('/').collect();
    if parts.len() >= 3 {
        ulid::Ulid::from_string(parts[parts.len() - 1]).ok()
    } else {
        None
    }
}

/// Uncommitted changes (tracked or not) in a worktree.
pub fn is_dirty(path: &Path) -> anyhow::Result<bool> {
    Ok(!git(path, &["status", "--porcelain"])?.trim().is_empty())
}

/// The branch HEAD points at, or `None` when HEAD is detached.
pub fn current_branch(path: &Path) -> Option<String> {
    git_ok(path, &["symbolic-ref", "--short", "HEAD"])
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The post-condition every path into [`ensure`] must satisfy: a quark works on
/// its own branch, never on the default branch and never on a detached HEAD (a
/// commit there is unreachable garbage). Checked *after* the work, so it also
/// catches a human — or a Bypass-mode CLI — who checked out `main` inside a
/// quark's tree behind the engine's back.
fn assert_not_default_branch(path: &Path, repo_root: &Path) -> anyhow::Result<String> {
    let default = default_branch(repo_root);
    match current_branch(path) {
        None => bail!(
            "worktree {} is on a detached HEAD — a quark's commits there would be unreachable",
            path.display()
        ),
        Some(b) if b == default => bail!(
            "worktree {} is on the default branch `{default}` — a quark must never work on it",
            path.display()
        ),
        Some(b) => Ok(b),
    }
}

/// Ensure `.hadron/trees/<quark>/` exists as a worktree, checked out on
/// `quark/<id>/<assignment>` branched from the default branch.
///
/// **Idempotent for the same assignment**: called twice it is a no-op returning the
/// same path and branch. That is the load-bearing case — a quark paused for a
/// permission grant must resume in the *same* tree on the *same* branch, or the
/// half-finished work it left uncommitted vanishes.
pub fn ensure(repo_root: &Path, quark: &QuarkId, assignment: &str) -> anyhow::Result<Worktree> {
    if assignment.trim().is_empty() {
        bail!("refusing to cut a branch for {}: no assignment", quark.as_str());
    }
    let path = trees_dir(repo_root).join(quark.as_str());
    let branch = branch_name(quark, assignment);
    let base = default_branch(repo_root);

    if !path.exists() {
        // git may still hold an admin record for a tree someone deleted behind its
        // back; prune before re-adding, or `worktree add` refuses the path.
        let _ = git(repo_root, &["worktree", "prune"]);
        let path_clean = hadron_lattice::sys::paths::simplified(&path);
        let path_s = path_clean.to_string_lossy().to_string();
        git(repo_root, &["worktree", "add", "--detach", &path_s, &base])
            .with_context(|| format!("git worktree add {path_s}"))?;
    }

    // Already on this assignment's branch → the resume / hand-off case. No-op.
    if current_branch(&path).as_deref() == Some(branch.as_str()) {
        assert_not_default_branch(&path, repo_root)?;
        return Ok(Worktree { quark: quark.clone(), path, branch });
    }

    // A NEW assignment. A dirty tree here means the previous assignment's
    // uncommitted edits would be carried onto the new branch — silent
    // cross-assignment contamination. Snapshot the dirty tree on its existing
    // branch first so the work is preserved and the worktree becomes clean.
    if is_dirty(&path)? {
        let current_b = current_branch(&path).unwrap_or_default();
        let prev_wt = Worktree {
            quark: quark.clone(),
            path: path.clone(),
            branch: current_b,
        };
        let msg = format!("{}: WIP snapshot before cutting branch {}", quark.as_str(), branch);
        commit_turn(&prev_wt, &msg).with_context(|| {
            format!(
                "snapshotting dirty worktree {} before cutting branch {}",
                path.display(),
                branch
            )
        })?;
    }

    let branch_exists = git_ok(
        repo_root,
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
    )?
    .is_some();
    if branch_exists {
        // The same assignment, whose worktree had been checked out elsewhere (or
        // detached by a fresh `worktree add`). Land back on it — do not re-cut it.
        git(&path, &["checkout", "-q", &branch])?;
    } else {
        // NOT `reset --hard`: that would move the PREVIOUS branch's pointer to the
        // default branch and orphan its unmerged commits. Branch from the default
        // branch instead, leaving the old branch exactly where it is.
        git(&path, &["checkout", "-q", "-b", &branch, &base])?;
    }

    assert_not_default_branch(&path, repo_root)?;
    Ok(Worktree { quark: quark.clone(), path, branch })
}

/// Every `.hadron/trees/*` worktree git currently knows about.
pub fn list(repo_root: &Path) -> anyhow::Result<Vec<Worktree>> {
    let out = git(repo_root, &["worktree", "list", "--porcelain"])?;
    let trees = trees_dir(repo_root);
    let mut found = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch = String::new();
    let flush = |path: &mut Option<PathBuf>, branch: &mut String, found: &mut Vec<Worktree>| {
        if let Some(p) = path.take() {
            if p.parent() == Some(trees.as_path()) {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    found.push(Worktree {
                        quark: QuarkId::new(name),
                        path: p.clone(),
                        branch: std::mem::take(branch),
                    });
                }
            }
        }
        branch.clear();
    };
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut found);
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = b.strip_prefix("refs/heads/").unwrap_or(b).to_string();
        }
    }
    flush(&mut path, &mut branch, &mut found);
    Ok(found)
}

/// The build environment every quark subprocess and the merge gate share.
///
/// **This is the whole disk story.** A worktree is a fresh checkout with no
/// `target/` of its own, so an unqualified `cargo` there does a cold build *and*
/// grows a duplicate artifact tree per quark. Measured on this repo before the fix:
/// the main checkout's `target/` was 105 GB and three worktrees had grown 32–37 GB
/// each — 210 GB total, none of it ever pruned. Pointing every worktree at the main
/// checkout's `target/` makes them share one warm cache instead (the gate measured
/// 13s in a fresh worktree against a cold build's minutes).
///
/// `CARGO_INCREMENTAL=0` rides along because a quark gets a *fresh branch every
/// turn*, so incremental state is rebuilt more often than it is reused — and it was
/// 40 GB of the 105. The human's own interactive builds are untouched: this is a
/// spawn-time env, not a checked-in `.cargo/config.toml`.
///
/// Safe under concurrency: cargo takes a file lock on the target dir, so two quarks
/// queue rather than corrupt each other. Best-effort — if git cannot name the main
/// root (not a repo), the target dir is simply left to cargo's default.
pub fn shared_build_env(cwd: &Path) -> Vec<(String, String)> {
    let mut env = vec![("CARGO_INCREMENTAL".to_string(), "0".to_string())];
    if let Ok(root) = crate::snapshot::main_repo_root(cwd) {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            root.join("target").to_string_lossy().to_string(),
        ));
    }
    env
}

/// How long a build artifact must sit unused before [`reap_build_artifacts`]
/// reclaims it.
///
/// **3 days, chosen from the measured distribution — 7 days reclaimed literally
/// nothing.** Measured 2026-07-25 on the live 107G `target/debug`, counting the
/// file atimes the sweep actually reads:
///
/// | | >1d | >3d | >5d | >7d |
/// |---|---|---|---|---|
/// | `deps/` | 44G | 35G | 22G | **0** |
/// | `incremental/` | 41G | 28G | 17G | **0** |
///
/// Nothing in the tree is older than ~6 days, because this box rebuilds
/// constantly — so any threshold at or above 6 days is a sweep that never fires.
/// 3 days reclaims ~64G today and costs one cold rebuild only for someone
/// returning to a branch they have not built in three days. A wrongly-deleted
/// artifact is never lost work, only a slower next build, which is what lets the
/// threshold be this aggressive.
pub const ARTIFACT_REAP_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(3 * 24 * 60 * 60);

/// What one artifact sweep reclaimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArtifactReap {
    pub files_removed: u64,
    pub bytes_removed: u64,
}

/// Sweep stale entries out of `<repo_root>/target/debug/{incremental,deps,
/// .fingerprint,build}` (swept in that order — `incremental/` first, it is the
/// single largest reclaimable slice). Nothing has ever swept this dir: cargo has
/// no target-dir GC of its own (`-Zgc` is the `~/.cargo` *registry* cache, a
/// different thing entirely), so it only grows. Measured on this box before this
/// existed: **107G**, none of it older than 14 days, 22G untouched for >7 days.
///
/// **This is not primarily quark cleanup — it is ordinary build accumulation that
/// nothing collects, and most of it is not the quarks' doing.** `incremental/` is
/// 40G of the 107G and is **entirely human-origin**: [`shared_build_env`] sets
/// `CARGO_INCREMENTAL=0` for every quark subprocess and the merge gate, so neither
/// has ever written an incremental dir — only the human's own interactive builds
/// have. Of the rest (`deps/`, `.fingerprint/`, `build/`), attribution by the
/// object paths a `.d` file's dependencies name puts quark-worktree artifacts at
/// roughly 4% of attributable bytes; the remainder is the main checkout's own
/// accumulated builds. So the fix here is a general debug-artifact GC that cargo
/// itself lacks, not a per-quark cleanup — worth knowing before re-diagnosing this
/// as a quark-turn cost, which is the wrong story and cost real effort to correct.
///
/// Keyed on **access time, not modification time**. A fingerprint or rlib that a
/// build is still reusing has an old mtime (it was written once) but a fresh
/// atime (cargo reads it on every build to decide whether to reuse it) — an
/// mtime sweep would delete exactly the artifacts that are actively working.
/// This assumes the filesystem updates atime at all (a `relatime` mount, verified
/// on this box, does so at day granularity, which is the granularity this sweep
/// needs); a `noatime` mount would make every entry look permanently stale, and
/// this function does not defend against that.
///
/// **Safety here is structural, not measured**, unlike [`reap_idle_worktrees`]:
/// a build artifact has no unique content, so the worst outcome of deleting a
/// live one is a slower next build, not lost work. There is nothing to preserve,
/// so this does not reuse `is_dirty`/`commits_ahead`-style guards.
///
/// Takes `target/debug/.cargo-lock` **non-blocking** before touching anything: if
/// cargo already holds it, a build is in flight, and deleting artifacts out from
/// under a live build could delete a file mid-link. Returns `Ok(None)` in that
/// case — the whole sweep is skipped, not retried; the next daemon restart tries
/// again. Returns `Ok(Some(ArtifactReap::default()))` when there is nothing to
/// sweep (no `target/debug` yet, or nothing older than `min_age`).
pub fn reap_build_artifacts(repo_root: &Path, min_age: std::time::Duration) -> anyhow::Result<Option<ArtifactReap>> {
    reap_build_artifacts_maybe_dry(repo_root, min_age, false)
}

/// [`reap_build_artifacts`], with `dry_run: true` reporting exactly what it would
/// reclaim (same walk, same staleness check) without deleting anything — the way
/// to get a trustworthy number off a production `target/debug` before letting the
/// deleting form run unattended. Both forms share this one implementation, so a
/// dry-run number can never diverge from what a real run actually does.
pub fn reap_build_artifacts_maybe_dry(
    repo_root: &Path,
    min_age: std::time::Duration,
    dry_run: bool,
) -> anyhow::Result<Option<ArtifactReap>> {
    let debug_dir = repo_root.join("target").join("debug");
    if !debug_dir.is_dir() {
        return Ok(Some(ArtifactReap::default()));
    }

    let lock_path = debug_dir.join(".cargo-lock");
    // Held for the whole sweep; dropped at the end of this function.
    let _lock_file = match hold_target_lock(&lock_path)? {
        Some(held) => held,
        None => return Ok(None),
    };

    let cutoff = std::time::SystemTime::now()
        .checked_sub(min_age)
        .context("min_age is larger than the current time")?;
    let mut reap = ArtifactReap::default();
    for sub in ["incremental", "deps", ".fingerprint", "build"] {
        sweep_stale_entries(&debug_dir.join(sub), cutoff, dry_run, &mut reap)?;
    }
    // `lock_file` drops here, releasing the advisory lock.
    Ok(Some(reap))
}

/// How long the sweep keeps asking for `target/debug/.cargo-lock` before it
/// accepts that a build really is in flight: `LOCK_ATTEMPTS × LOCK_RETRY_PAUSE`.
///
/// **A single `try_lock` is not a reliable answer to "is cargo building?"** —
/// measured on this box, it reports `WouldBlock` with no holder at all. Reproduced
/// 2 runs in 12 of `cargo test -p hadron-gluon --lib`, 0 in 30 runs of the same
/// test alone and 0 in 8 single-threaded suite runs, so it needs concurrent load;
/// at the moment of failure `/proc/self/fd` held exactly ONE descriptor on that
/// inode — the one we had just opened ourselves. Root cause not established; what
/// is established is that one attempt is not enough. Waiting turns a false
/// "cargo is building" (which silently skips the whole sweep until the next
/// daemon start) into a real answer, and costs at most this long, once, at startup.
const LOCK_ATTEMPTS: u32 = 20;
const LOCK_RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(25);

/// Take the same lock file cargo itself holds around the target dir, retrying for
/// up to [`LOCK_ATTEMPTS`] × [`LOCK_RETRY_PAUSE`]. `Ok(None)` means a build really
/// does hold it and the caller must not touch the directory.
///
/// Each attempt **re-opens** the path so it gets its own open file description.
/// That is load-bearing, not tidiness: hoisting the `open` out of the loop and
/// retrying `try_lock` on the same `File` fails 3 runs in 24 — and it fails
/// `a_briefly_held_lock_is_waited_out_rather_than_skipping_the_sweep`, i.e. a
/// description whose first `try_lock` failed does not reliably observe another
/// holder releasing the lock. Re-opening per attempt: 0 in 24.
fn hold_target_lock(lock_path: &Path) -> anyhow::Result<Option<std::fs::File>> {
    for attempt in 0..LOCK_ATTEMPTS {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(lock_path)
            .with_context(|| format!("opening {}", lock_path.display()))?;
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {
                if attempt + 1 < LOCK_ATTEMPTS {
                    std::thread::sleep(LOCK_RETRY_PAUSE);
                }
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(e).with_context(|| format!("locking {}", lock_path.display()))
            }
        }
    }
    Ok(None)
}

/// Where [`record_artifact_sweep`] appends, relative to the `.hadron/` dir.
pub const ARTIFACT_SWEEP_LOG: &str = "artifact-sweeps.log";

/// Append one line describing what a sweep reclaimed to
/// `<hadron_dir>/`[`ARTIFACT_SWEEP_LOG`].
///
/// The sweep deletes tens of GB, runs unattended on every daemon start, and its
/// only report was an `eprintln!` to whatever terminal launched the daemon —
/// `/dev/pts/0` on this box. So an hour after the first real run took
/// `target/debug` from 107G to 48G, the figure it printed was unrecoverable and
/// the run had to be reconstructed from surviving-sibling forensics. A run that
/// reclaimed nothing is recorded too: "it swept and found nothing" and "it never
/// ran" were indistinguishable, and telling them apart is half the value.
///
/// Append-only, and never read back by the daemon — this is an audit trail for
/// the human, not state.
pub fn record_artifact_sweep(hadron_dir: &Path, reap: &ArtifactReap) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(hadron_dir)?;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(hadron_dir.join(ARTIFACT_SWEEP_LOG))?;
    writeln!(
        log,
        "{} swept files={} bytes={}",
        chrono::Utc::now().to_rfc3339(),
        reap.files_removed,
        reap.bytes_removed,
    )
}

/// Remove every immediate child of `dir` whose **newest atime anywhere inside
/// it** is older than `cutoff` — a file is removed directly, a directory (e.g.
/// one `.fingerprint/<crate>-<hash>/` entry) is removed with everything inside
/// it. A missing `dir` (one of the four categories simply does not exist yet)
/// is not an error — there is nothing to sweep.
///
/// Deliberately NOT the entry's own atime for a directory. Confirmed live on a
/// real box (`touch -a` a `.fingerprint/<crate>-<hash>/` dir 8 days stale, then
/// run a cargo build that hits it as a pure cache hit): cargo reads one marker
/// file inside to verify the recorded hash, refreshing THAT file's atime, but
/// never touches the directory's own atime. A sweep keyed on the directory
/// entry alone would delete a fingerprint being reused every day.
fn sweep_stale_entries(
    dir: &Path,
    cutoff: std::time::SystemTime,
    dry_run: bool,
    reap: &mut ArtifactReap,
) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading an entry of {}", dir.display()))?;
        let path = entry.path();
        let atime = newest_atime(&path)?;
        if atime >= cutoff {
            continue;
        }
        let meta = entry.metadata().with_context(|| format!("stat-ing {}", path.display()))?;
        let bytes = dir_size(&path)?;
        if !dry_run {
            if meta.is_dir() {
                std::fs::remove_dir_all(&path).with_context(|| format!("removing {}", path.display()))?;
            } else {
                std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        reap.files_removed += 1;
        reap.bytes_removed += bytes;
    }
    Ok(())
}

/// The most recent access time anywhere under `path` — `path`'s own atime for a
/// file, or the max of every entry's `newest_atime` (recursively) for a
/// directory, EXCLUDING the directory's own (see below — it is not signal, it is
/// noise this function's own traversal would otherwise inject). One live file
/// anywhere inside is enough to keep the whole directory looking used.
///
/// Safe to call repeatedly, including once as a dry run and again for real:
/// `symlink_metadata(..).accessed()` is a `stat`, not an `open`+read, and `stat`
/// does not update atime (confirmed on this box — probed a file's atime before
/// and after a bare `stat`, unchanged) — so measuring a file never changes what
/// the next measurement of it will see.
fn newest_atime(path: &Path) -> anyhow::Result<std::time::SystemTime> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("stat-ing {}", path.display()))?;
    if !meta.is_dir() {
        return meta.accessed().with_context(|| format!("reading atime of {}", path.display()));
    }
    // A DIRECTORY'S OWN ATIME IS MEANINGLESS HERE and must not be part of the max.
    // Merely listing a directory bumps its atime — proven on this box: `touch -a -d
    // "8 days ago" dir; ls dir` moved the directory eight days forward while the
    // file inside stayed stale. `read_dir` below does exactly that, so seeding from
    // the directory made the sweep refresh the very timestamps it was about to
    // judge, and `.fingerprint/`, `incremental/` and `build/` became permanently
    // un-collectable — three of four categories, 41G of the 107G, measured.
    //
    // An empty directory therefore reports the epoch, i.e. maximally stale. That is
    // the right answer: a unit directory with no files left in it is dead.
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    for entry in std::fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        newest = newest.max(newest_atime(&entry?.path())?);
    }
    Ok(newest)
}

/// Total size in bytes of `path` — its own size if a file, or the recursive sum
/// of everything under it if a directory.
fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("stat-ing {}", path.display()))?;
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        total += dir_size(&entry?.path())?;
    }
    Ok(total)
}

/// Delete every `quark/*` branch already merged into `base`, and report the names.
///
/// **`-d`, never `-D`.** Lowercase refuses a branch whose commits are not in `base`,
/// which is exactly the guard we want: an errored turn's branch never reaches the
/// merge gate, and deleting it would silently destroy the only copy of that work.
/// Git also refuses a branch a worktree currently has checked out, so a live quark's
/// branch is safe by construction — which is why this runs at startup, not at land
/// time. Errors are swallowed per branch: a refusal is the guard doing its job, not
/// a failure of the sweep.
pub fn prune_merged_branches(repo_root: &Path, base: &str) -> anyhow::Result<Vec<String>> {
    let listed = git(
        repo_root,
        &["branch", "--list", "quark/*", "--merged", base, "--format=%(refname:short)"],
    )?;
    let mut pruned = Vec::new();
    for name in listed.lines().map(str::trim).filter(|l| !l.is_empty()) {
        // `git`, NOT `git_ok`: the latter maps a nonzero exit onto `Ok(None)`, so
        // `.is_ok()` on it is always true and this reported every refusal as a
        // deletion. Caught by `the_prune_skips_a_branch_a_worktree_has_checked_out`.
        if git(repo_root, &["branch", "-d", name]).is_ok() {
            pruned.push(name.to_string());
        }
    }
    Ok(pruned)
}

/// Where a reap parks anything it had to move out of a tree before removing it.
/// Gitignored like the rest of `.hadron/`, so it never dirties the human's status.
pub fn reaped_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".hadron").join("reaped")
}

/// Ignored files `git worktree remove` would silently destroy, moved out to
/// [`reaped_dir`] first. Returns one note per rescued path.
///
/// `status --porcelain` — what [`is_dirty`] runs — EXCLUDES ignored paths, and
/// `worktree remove` does not check them either: an ignored file in a tree with an
/// empty porcelain status is simply gone, exit 0. **Untracked** files are already
/// safe (git refuses to remove a tree holding them), so ignored files are exactly
/// the gap. Sparing on their presence instead would make the reap a permanent no-op
/// — every live tree holds some, and among the junk are plan documents a quark wrote
/// into its own tree by mistake. Moving is a rename on the same filesystem, so a
/// large ignored directory costs nothing.
fn preserve_ignored(repo_root: &Path, wt: &Worktree) -> anyhow::Result<Vec<String>> {
    let listed = git(&wt.path, &["status", "--porcelain", "--ignored"])?;
    let mut moved = Vec::new();
    for rel in listed.lines().filter_map(|l| l.strip_prefix("!! ")).map(str::trim) {
        if rel.is_empty() {
            continue;
        }
        let to = reaped_dir(repo_root).join(wt.quark.as_str()).join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(wt.path.join(rel), &to)
            .with_context(|| format!("rescuing ignored {rel} to {}", to.display()))?;
        moved.push(format!("kept ignored {rel} in {}", to.display()));
    }
    Ok(moved)
}

/// Commits the tree's own reflog can still reach that `base` cannot, pinned under
/// `refs/reaped/<quark>/` so gc cannot take them. Returns one note per archived commit.
///
/// The containment guard proves the CURRENT head is safe and says nothing about a
/// commit orphaned by `--amend` or `reset` *inside* the tree. Its only holder is
/// `.git/worktrees/<id>/logs/HEAD`, which is per-worktree and which `worktree remove`
/// deletes — after which `gc` destroys the object. A Bypass quark drives git directly,
/// so this is reachable in normal operation, not a contrivance.
fn archive_orphans(repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Vec<String>> {
    let Ok(log) = git(&wt.path, &["reflog", "show", "HEAD", "--format=%H"]) else {
        // No reflog to read is not proof of safety, but it is also not something we
        // can act on; the containment guard has already passed for HEAD.
        return Ok(Vec::new());
    };
    let mut archived: Vec<String> = Vec::new();
    for sha in log.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if archived.iter().any(|note| note.contains(sha)) {
            continue;
        }
        if git(&wt.path, &["merge-base", "--is-ancestor", sha, base]).is_ok() {
            continue;
        }
        let refname = format!("refs/reaped/{}/{sha}", wt.quark.as_str());
        git(repo_root, &["update-ref", &refname, sha])
            .with_context(|| format!("archiving orphaned commit {sha} as {refname}"))?;
        archived.push(format!("archived orphaned commit {sha} as {refname}"));
    }
    Ok(archived)
}

/// What a reap did with one worktree whose quark is no longer taking turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reap {
    /// The directory is gone. Its branch held nothing the default branch does not
    /// already have, and no worktree holds the ref any more, so the merged-branch
    /// sweep that runs next deletes it too. `preserved` names everything that had to
    /// be moved or pinned first — empty for a tree that held only tracked, landed work.
    Removed { quark: QuarkId, path: PathBuf, preserved: Vec<String> },
    /// Left exactly where it is, with the reason — always because it holds something
    /// the default branch does not.
    Spared { quark: QuarkId, path: PathBuf, why: String },
}

/// Remove the worktrees of quarks that are not taking turns — **only** when the tree
/// holds nothing `base` does not already have.
///
/// This is the disk story at scale. A worktree is stable per quark, so N seats cost N
/// checkouts of the repo, and until this existed *nothing ever removed one*: [`ensure`]
/// creates, [`reclaim`] prunes only what git had already lost. Switch a seat off, or
/// delete it from the roster, and its checkout stays forever. Twenty seats in a
/// monorepo is twenty working trees, most of them for quarks nobody is using.
///
/// **Losslessness is the whole contract**, and it is two checks: a dirty tree is a
/// crashed quark's evidence, and a branch ahead of `base` is work the merge gate never
/// landed. Either one and the tree is SPARED and reported — this never trades work for
/// disk. What it removes is by construction recoverable: `ensure` recreates the tree on
/// the quark's next turn, and every commit in it is already on `base`.
pub fn reap_idle_worktrees(
    repo_root: &Path,
    keep: &[QuarkId],
    base: &str,
) -> anyhow::Result<Vec<Reap>> {
    let mut out = Vec::new();
    for wt in list(repo_root)? {
        if keep.contains(&wt.quark) {
            continue;
        }
        // Nothing here is swallowed: a check that cannot be answered spares the tree
        // and says why, because "I could not tell" must never read as "safe to delete".
        let why = match is_dirty(&wt.path) {
            Err(e) => Some(format!("cannot read its status: {e:#}")),
            Ok(true) => Some("uncommitted work".to_string()),
            // POSITIVE proof, and deliberately not `commits_ahead`: that one is built on
            // `git_ok`, which maps a nonzero exit onto `Ok(None)` → `0` → "nothing to
            // land", so a git failure would read as permission to delete. `merge-base
            // --is-ancestor` succeeds only when HEAD is genuinely contained in `base`;
            // "not an ancestor" and "git broke" both land in the Err arm and both spare.
            // It reads HEAD, not the branch name, so a detached tree is measured too.
            Ok(false) => match git(&wt.path, &["merge-base", "--is-ancestor", "HEAD", base]) {
                Ok(_) => None,
                Err(_) => Some(format!("cannot prove its HEAD is already on {base}")),
            },
        };
        // Rescue what `worktree remove` would take with it — ignored files it deletes
        // in silence, and commits held only by the per-worktree reflog it unlinks.
        // Both run BEFORE the removal, and a failure in either spares the tree.
        let mut preserved = Vec::new();
        let why = match why {
            Some(why) => Some(why),
            None => archive_orphans(repo_root, &wt, base)
                .and_then(|notes| {
                    preserved.extend(notes);
                    preserve_ignored(repo_root, &wt)
                })
                .and_then(|notes| {
                    preserved.extend(notes);
                    git(repo_root, &["worktree", "remove", &wt.path.to_string_lossy()])
                })
                .err()
                .map(|e| format!("nothing was removed — {e:#}")),
        };
        out.push(match why {
            Some(why) => Reap::Spared { quark: wt.quark, path: wt.path, why },
            None => Reap::Removed { quark: wt.quark, path: wt.path, preserved },
        });
    }
    Ok(out)
}

/// Startup reclamation. Prunes worktrees git has lost track of (the dir was deleted
/// behind its back), then REPORTS what survives — including any dirty tree left by
/// a crashed quark or a killed daemon.
///
/// It deliberately does not clean, reset, or delete: the engine cannot even name the
/// quark that panicked (a `JoinError` doesn't carry it), so cleanup can't ride the
/// crash path — and a half-finished turn's edits are evidence the human may want.
/// The next `ensure` for a *new* assignment will refuse a dirty tree loudly, which
/// is the right place for that decision.
pub fn reclaim(repo_root: &Path) -> anyhow::Result<Vec<Reclaimed>> {
    let _ = git(repo_root, &["worktree", "prune"]);
    let mut out = Vec::new();
    for wt in list(repo_root)? {
        let dirty = is_dirty(&wt.path).unwrap_or(false);
        out.push(Reclaimed { quark: wt.quark, path: wt.path, branch: wt.branch, dirty });
    }
    Ok(out)
}

/// This worktree's HEAD commit, or `None` in a repo with no commits.
pub fn head(path: &Path) -> Option<String> {
    git_ok(path, &["rev-parse", "--verify", "HEAD"]).ok().flatten()
}

/// End the turn on a commit. `Ok(None)` = nothing to commit — a pure-conversation
/// turn, a read-only `Ask`-posture turn, or a `Bypass` quark that already committed
/// its own work. Callers must therefore compare HEAD rather than assume *we* were
/// the ones who committed.
pub fn commit_turn(wt: &Worktree, message: &str) -> anyhow::Result<Option<String>> {
    if !is_dirty(&wt.path)? {
        return Ok(None);
    }
    git(&wt.path, &["add", "-A"])?;
    git(&wt.path, &["commit", "-q", "-m", message])?;
    Ok(head(&wt.path))
}

/// `git diff <base>...HEAD` — by construction exactly and only this quark's work on
/// this assignment. This is the authoritative attribution, and what the projection
/// shows the quark ("everything you have done so far"). The old `git diff HEAD`
/// (uncommitted changes) is empty once a turn ends on a commit, and under
/// concurrency it could show a *sibling's* edits.
pub fn branch_diff(wt: &Worktree, base: &str) -> anyhow::Result<String> {
    if head(&wt.path).is_none() {
        return Ok(String::new());
    }
    let range = format!("{base}...HEAD");
    Ok(git_ok(&wt.path, &["diff", &range])?.unwrap_or_default())
}

/// How many commits this branch is ahead of `base` — the merge gate's "is there
/// anything to land?" question.
pub fn commits_ahead(wt: &Worktree, base: &str) -> anyhow::Result<usize> {
    let range = format!("{base}..HEAD");
    let out = git_ok(&wt.path, &["rev-list", "--count", &range])?.unwrap_or_default();
    Ok(out.trim().parse().unwrap_or(0))
}

/// The files this branch touched, relative to `base` — the `Kind::Edit` event's paths.
pub fn changed_paths(wt: &Worktree, base: &str) -> anyhow::Result<Vec<String>> {
    let range = format!("{base}...HEAD");
    let out = git_ok(&wt.path, &["diff", "--name-only", &range])?.unwrap_or_default();
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A temp git repo with one commit on `main`, so HEAD and a default branch exist.
    pub(crate) fn git_repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(root.join("f.txt"), "x\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-q", "-m", "init"]).unwrap();
        dir
    }

    fn q(id: &str) -> QuarkId {
        QuarkId::new(id)
    }

    /// The disk fix: a worktree's build env points at the MAIN checkout's `target/`,
    /// not at one of its own. Without this every quark grows a duplicate artifact
    /// tree (measured: 32–37 GB each, three of them).
    #[test]
    fn a_worktree_builds_into_the_main_checkouts_target() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        let env = shared_build_env(&wt.path);

        let target = env.iter().find(|(k, _)| k == "CARGO_TARGET_DIR").expect("target dir is set");
        // The main root is what git reports, so compare against the canonicalized
        // repo dir — `git_repo` lives under a temp dir that is a symlink on some boxes.
        let expected = std::fs::canonicalize(repo.path()).unwrap().join("target");
        assert_eq!(
            PathBuf::from(&target.1),
            expected,
            "the worktree must build into the main checkout's target, not {:?}",
            wt.path.join("target"),
        );
        assert!(
            !target.1.contains(".hadron/trees"),
            "a target under the worktree is exactly the 37 GB duplicate this prevents",
        );
        assert_eq!(
            env.iter().find(|(k, _)| k == "CARGO_INCREMENTAL").map(|(_, v)| v.as_str()),
            Some("0"),
            "a fresh branch per turn rebuilds incremental state more often than it reuses it",
        );
    }

    /// Outside a repo there is no main root to name, so we set no target dir at all
    /// and let cargo do what it always did — rather than inventing a path.
    #[test]
    fn shared_build_env_names_no_target_outside_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let env = shared_build_env(dir.path());
        assert!(env.iter().all(|(k, _)| k != "CARGO_TARGET_DIR"));
    }

    /// A merged branch is swept; an unmerged one SURVIVES. The second half is the
    /// point: an errored turn never reaches the merge gate, and `-d` refusing it is
    /// the only thing standing between that work and deletion.
    #[test]
    fn the_prune_takes_merged_branches_and_spares_unmerged_work() {
        let repo = git_repo();
        let root = repo.path();

        // A merged branch: branch, then fast-forward main onto it.
        git(root, &["checkout", "-q", "-b", "quark/opus/01MERGED"]).unwrap();
        std::fs::write(root.join("merged.txt"), "landed\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-q", "-m", "landed work"]).unwrap();
        git(root, &["checkout", "-q", "main"]).unwrap();
        git(root, &["merge", "-q", "--ff-only", "quark/opus/01MERGED"]).unwrap();

        // An unmerged branch: a turn that errored before the gate ever saw it.
        git(root, &["checkout", "-q", "-b", "quark/agy/01STRANDED"]).unwrap();
        std::fs::write(root.join("stranded.txt"), "never landed\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-q", "-m", "work the gate never saw"]).unwrap();
        git(root, &["checkout", "-q", "main"]).unwrap();

        let pruned = prune_merged_branches(root, "main").unwrap();

        assert_eq!(pruned, vec!["quark/opus/01MERGED".to_string()]);
        let left = git(root, &["branch", "--list", "quark/*", "--format=%(refname:short)"]).unwrap();
        assert!(left.contains("quark/agy/01STRANDED"), "unmerged work must survive the sweep");
        assert!(!left.contains("quark/opus/01MERGED"));
    }

    /// A live quark's branch is checked out in its worktree, so git refuses to delete
    /// it even once it has landed — the sweep must survive that refusal, not abort on it.
    #[test]
    fn the_prune_skips_a_branch_a_worktree_has_checked_out() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01LIVE").unwrap();
        // Branched from main with no commits of its own, so it counts as merged.
        let pruned = prune_merged_branches(repo.path(), "main").unwrap();

        assert!(pruned.is_empty(), "git refuses a checked-out branch; the sweep reports none");
        assert_eq!(current_branch(&wt.path).as_deref(), Some("quark/opus/01LIVE"));
    }

    /// The disk story at scale: a quark that is no longer taking turns keeps a full
    /// checkout of the repo forever. Reaping it also un-holds its branch, so the
    /// merged-branch sweep that runs next can finally delete the ref — one mechanism,
    /// not two.
    #[test]
    fn reap_removes_an_idle_tree_and_the_sweep_then_takes_its_branch() {
        let repo = git_repo();
        let root = repo.path();
        let live = ensure(root, &q("opus"), "01LIVE").unwrap();
        let idle = ensure(root, &q("codex"), "01IDLE").unwrap();

        let reaped = reap_idle_worktrees(root, &[q("opus")], "main").unwrap();

        assert_eq!(
            reaped,
            vec![Reap::Removed {
                quark: q("codex"),
                path: idle.path.clone(),
                preserved: Vec::new(),
            }],
            "a tree holding only tracked, landed work needs nothing rescued",
        );
        assert!(!idle.path.exists(), "the idle tree is gone from disk");
        assert!(live.path.is_dir(), "a quark still taking turns keeps its tree");

        let pruned = prune_merged_branches(root, "main").unwrap();
        assert_eq!(
            pruned,
            vec!["quark/codex/01IDLE".to_string()],
            "no worktree holds the branch now, so the existing sweep takes it",
        );
    }

    /// Losslessness, half one: a crashed quark's uncommitted edits are evidence. The
    /// tree is spared and named, never removed to save disk.
    #[test]
    fn reap_spares_an_idle_tree_with_uncommitted_work() {
        let repo = git_repo();
        let idle = ensure(repo.path(), &q("codex"), "01IDLE").unwrap();
        std::fs::write(idle.path.join("half-done.txt"), "mid-turn\n").unwrap();

        let reaped = reap_idle_worktrees(repo.path(), &[], "main").unwrap();

        assert!(idle.path.is_dir(), "uncommitted work is never destroyed to reclaim disk");
        assert!(
            matches!(reaped.as_slice(), [Reap::Spared { why, .. }] if why.contains("uncommitted")),
            "expected a spared-with-reason, got {reaped:?}",
        );
    }

    /// Losslessness, half two: a branch ahead of the default branch is work the merge
    /// gate never landed. Removing the tree would un-hold the ref, and `-d` would then
    /// be the only thing standing between that work and the next sweep.
    #[test]
    fn reap_spares_an_idle_tree_whose_branch_has_not_landed() {
        let repo = git_repo();
        let idle = ensure(repo.path(), &q("codex"), "01IDLE").unwrap();
        std::fs::write(idle.path.join("work.txt"), "never landed\n").unwrap();
        commit_turn(&idle, "codex: work the gate never saw").unwrap().unwrap();

        let reaped = reap_idle_worktrees(repo.path(), &[], "main").unwrap();

        assert!(idle.path.is_dir(), "unlanded commits keep their tree");
        assert!(
            matches!(reaped.as_slice(), [Reap::Spared { why, .. }] if why.contains("not")),
            "expected a spared-with-reason, got {reaped:?}",
        );
    }

    /// `status --porcelain` — what `is_dirty` runs — EXCLUDES ignored paths, and
    /// `git worktree remove` deletes them without a word. Proven in a scratch repo: an
    /// ignored `secret.log` in a tree whose porcelain status is empty is gone after a
    /// clean `worktree remove`, exit 0. Sparing on ignored files instead would make the
    /// reap a permanent no-op — every live tree here holds some (`.hadron/*.lock`,
    /// `.remember/`, and plan docs a quark wrote into its own tree by mistake).
    #[test]
    fn reap_preserves_ignored_files_before_removing_the_tree() {
        let repo = git_repo();
        let root = repo.path();
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-q", "-m", "ignore logs"]).unwrap();
        let idle = ensure(root, &q("codex"), "01IDLE").unwrap();
        std::fs::write(idle.path.join("secret.log"), "irreplaceable\n").unwrap();

        let reaped = reap_idle_worktrees(root, &[], "main").unwrap();

        assert!(!idle.path.exists(), "the tree is still reaped");
        let kept = reaped_dir(root).join("codex").join("secret.log");
        assert_eq!(
            std::fs::read_to_string(&kept).ok().as_deref(),
            Some("irreplaceable\n"),
            "the ignored file must survive at {}, got {reaped:?}",
            kept.display(),
        );
    }

    /// `merge-base --is-ancestor HEAD base` proves the CURRENT head is safe and says
    /// nothing about a commit orphaned by `--amend`/`reset` inside the tree. Its only
    /// holder is `.git/worktrees/<id>/logs/HEAD`, which is per-worktree and which
    /// `git worktree remove` deletes — proven end-to-end in a scratch repo: amend, land
    /// the new HEAD, remove the tree, `gc --prune=now`, and `cat-file -e <orphan>` fails.
    #[test]
    fn reap_archives_a_commit_orphaned_inside_the_tree() {
        let repo = git_repo();
        let root = repo.path();
        let idle = ensure(root, &q("codex"), "01IDLE").unwrap();
        std::fs::write(idle.path.join("w.txt"), "v1\n").unwrap();
        commit_turn(&idle, "codex: v1").unwrap().unwrap();
        let orphan = git(&idle.path, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        std::fs::write(idle.path.join("w.txt"), "v2\n").unwrap();
        git(&idle.path, &["add", "."]).unwrap();
        git(&idle.path, &["commit", "-q", "--amend", "-m", "codex: v2"]).unwrap();
        // The amended HEAD lands, so the reap's containment guard passes.
        git(root, &["merge", "-q", "--no-ff", "-m", "merge", &idle.branch]).unwrap();

        let reaped = reap_idle_worktrees(root, &[], "main").unwrap();

        assert!(!idle.path.exists(), "the tree is still reaped, got {reaped:?}");
        git(root, &["reflog", "expire", "--expire=now", "--all"]).unwrap();
        let _ = git(root, &["gc", "--prune=now"]);
        assert!(
            git(root, &["cat-file", "-e", &orphan]).is_ok(),
            "the orphaned commit {orphan} must survive gc via an archive ref",
        );
    }

    /// The one that discriminates. `commits_ahead` is built on `git_ok`, which maps a
    /// nonzero exit onto `Ok(None)` → `0` → "nothing to land" — so a measurement that
    /// FAILS reads as permission to delete. Give it a base no git command can resolve:
    /// the positive `merge-base --is-ancestor` proof spares the tree, the counting one
    /// removes it. Same family as `git_ok-makes-is_ok-always-true`.
    #[test]
    fn reap_spares_a_tree_it_cannot_measure_against_base() {
        let repo = git_repo();
        let idle = ensure(repo.path(), &q("codex"), "01IDLE").unwrap();

        let reaped = reap_idle_worktrees(repo.path(), &[], "no-such-base").unwrap();

        assert!(idle.path.is_dir(), "a tree nobody could measure is never removed");
        assert!(
            matches!(reaped.as_slice(), [Reap::Spared { .. }]),
            "expected a spared tree, got {reaped:?}",
        );
    }

    /// The measurement reads HEAD, not the branch name. A detached tree reports an
    /// empty branch from [`list`], so anything keyed on the branch would measure
    /// nothing and read it as landed — and a commit on a detached HEAD is precisely
    /// the work nothing else can find.
    #[test]
    fn reap_spares_an_idle_tree_holding_a_commit_on_a_detached_head() {
        let repo = git_repo();
        let idle = ensure(repo.path(), &q("codex"), "01IDLE").unwrap();
        std::fs::write(idle.path.join("work.txt"), "unreachable\n").unwrap();
        commit_turn(&idle, "codex: work on a branch it is about to leave").unwrap().unwrap();
        git(&idle.path, &["checkout", "-q", "--detach"]).unwrap();

        let reaped = reap_idle_worktrees(repo.path(), &[], "main").unwrap();

        assert!(idle.path.is_dir(), "a detached HEAD's commit is still work");
        assert!(
            matches!(reaped.as_slice(), [Reap::Spared { .. }]),
            "expected a spared tree, got {reaped:?}",
        );
    }

    #[test]
    fn ensure_creates_a_worktree_on_a_new_branch() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        assert!(wt.path.is_dir(), "the worktree exists on disk");
        assert_eq!(wt.path, repo.path().join(".hadron/trees/opus"));
        assert_eq!(wt.branch, "quark/opus/01AAA");
        assert_eq!(current_branch(&wt.path).as_deref(), Some("quark/opus/01AAA"));
        // The default branch is untouched by the quark's checkout.
        assert_eq!(current_branch(repo.path()).as_deref(), Some("main"));
    }

    /// A worktree checked out inside the (gitignored) `.hadron/` does NOT contain a
    /// `.hadron/` of its own — the dir is in no commit — so there is no recursion.
    /// Flagged as risk-to-verify in the plan; verified here.
    #[test]
    fn a_worktree_under_dot_hadron_does_not_recurse() {
        let repo = git_repo();
        std::fs::create_dir_all(repo.path().join(".hadron")).unwrap();
        std::fs::write(repo.path().join(".hadron/field.jsonl"), "{}\n").unwrap();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        assert!(!wt.path.join(".hadron").exists(), "no nested .hadron in the checkout");
        // …and the quark's tree is clean: the parent's .hadron is not visible in it.
        assert!(!is_dirty(&wt.path).unwrap());
    }

    /// THE idempotency case: a quark paused for a permission grant must resume in the
    /// same tree, on the same branch, with its uncommitted work still there.
    #[test]
    fn ensure_is_idempotent_for_the_same_assignment() {
        let repo = git_repo();
        let a = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(a.path.join("half-done.txt"), "mid-turn\n").unwrap();

        let b = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        assert_eq!(a, b, "same path, same branch, no error");
        assert!(b.path.join("half-done.txt").exists(), "the paused work survived");
    }

    #[test]
    fn ensure_snapshots_a_dirty_tree_instead_of_refusing_the_new_assignment() {
        let repo = git_repo();
        let a = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(a.path.join("uncommitted.txt"), "stray\n").unwrap();

        let b = ensure(repo.path(), &q("opus"), "01BBB").expect("snapshots dirty tree and cuts new branch");
        assert_eq!(b.branch, "quark/opus/01BBB");
        assert_eq!(b.path, a.path);

        // The dirty file was committed on the previous branch `01AAA`…
        let show = git(repo.path(), &["show", "quark/opus/01AAA:uncommitted.txt"]).unwrap();
        assert_eq!(show.trim(), "stray");

        // …and is NOT present on the new branch `01BBB` (which branched clean from main).
        assert!(!b.path.join("uncommitted.txt").exists());
        assert_eq!(current_branch(&b.path).as_deref(), Some("quark/opus/01BBB"));
    }

    #[test]
    fn ensure_refuses_a_turn_with_no_assignment() {
        let repo = git_repo();
        assert!(ensure(repo.path(), &q("opus"), "").is_err());
    }

    #[test]
    fn two_quarks_get_distinct_worktrees() {
        let repo = git_repo();
        let a = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        let b = ensure(repo.path(), &q("agy"), "01BBB").unwrap();
        assert_ne!(a.path, b.path);
        assert_ne!(a.branch, b.branch);

        std::fs::write(a.path.join("a.txt"), "from a\n").unwrap();
        assert!(!b.path.join("a.txt").exists(), "an edit in one is invisible in the other");
    }

    /// The anti-`reset --hard` test. Assignment A commits and is NOT merged; a new
    /// assignment B then runs. A's branch must still resolve, and still contain A's
    /// commit — otherwise the merge gate's "denied merge? the branch stays" promise
    /// is a lie.
    #[test]
    fn a_new_assignment_does_not_destroy_the_previous_branch() {
        let repo = git_repo();
        let a = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(a.path.join("a.txt"), "work\n").unwrap();
        let sha = commit_turn(&a, "opus: did A").unwrap().expect("committed");

        let b = ensure(repo.path(), &q("opus"), "01BBB").unwrap();
        assert_eq!(b.branch, "quark/opus/01BBB");
        assert_eq!(b.path, a.path, "same stable path");

        // A's branch still points at A's commit…
        let a_head = git(repo.path(), &["rev-parse", "quark/opus/01AAA"]).unwrap();
        assert_eq!(a_head, sha);
        // …and B branched from main, so it does not carry A's file.
        assert!(!b.path.join("a.txt").exists());
        assert_eq!(commits_ahead(&b, "main").unwrap(), 0);
    }

    /// Git gives us the first line of defence for free: `main` is already checked out
    /// in the parent worktree, so a plain `git checkout main` inside a quark's tree is
    /// *refused by git itself*. Forcing it takes `--ignore-other-worktrees`, which is
    /// exactly what a determined human — or a Bypass-mode CLI — would have to do.
    ///
    /// `ensure` then RECOVERS rather than erroring: it checks the quark back onto its
    /// own branch. That is strictly better than refusing to run — the post-condition
    /// (a quark never works on the default branch) holds either way, which is what
    /// `assert_not_default_branch` exists to guarantee.
    #[test]
    fn a_quark_forced_onto_the_default_branch_is_put_back_on_its_own() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();

        // Git refuses the plain checkout on its own.
        assert!(git(&wt.path, &["checkout", "-q", "main"]).is_err(), "git blocks it first");

        // Force past git, as a Bypass CLI could.
        git(&wt.path, &["checkout", "-q", "--ignore-other-worktrees", "main"]).unwrap();
        assert_eq!(current_branch(&wt.path).as_deref(), Some("main"), "it really is on main");

        // The next turn puts it back where it belongs, and never works on main.
        let again = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        assert_eq!(again.branch, "quark/opus/01AAA");
        assert_eq!(current_branch(&wt.path).as_deref(), Some("quark/opus/01AAA"));
    }

    /// The post-condition itself, exercised directly: a quark sitting on the default
    /// branch is a hard error, because a commit there lands in the human's history.
    #[test]
    fn the_guard_refuses_the_default_branch() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        git(&wt.path, &["checkout", "-q", "--ignore-other-worktrees", "main"]).unwrap();
        let err = assert_not_default_branch(&wt.path, repo.path()).unwrap_err().to_string();
        assert!(err.contains("default branch"), "refused: {err}");
    }

    #[test]
    fn detached_head_is_refused() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        let sha = head(&wt.path).unwrap();
        git(&wt.path, &["checkout", "-q", "--detach", &sha]).unwrap();
        let err = assert_not_default_branch(&wt.path, repo.path()).unwrap_err().to_string();
        assert!(err.contains("detached"), "refused: {err}");
    }

    #[test]
    fn reclaim_prunes_an_orphaned_worktree() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        // Kill the tree behind git's back, as a crashed machine would.
        std::fs::remove_dir_all(&wt.path).unwrap();
        assert_eq!(list(repo.path()).unwrap().len(), 1, "git still records it");

        let reclaimed = reclaim(repo.path()).unwrap();
        assert!(reclaimed.is_empty(), "the orphan is gone");
        assert!(list(repo.path()).unwrap().is_empty(), "git's worktree list is clean");
    }

    #[test]
    fn reclaim_reports_but_does_not_destroy_a_dirty_tree() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("half.txt"), "crashed mid-turn\n").unwrap();

        let reclaimed = reclaim(repo.path()).unwrap();
        assert_eq!(reclaimed.len(), 1);
        assert!(reclaimed[0].dirty, "the dirty tree is REPORTED");
        assert_eq!(reclaimed[0].branch, "quark/opus/01AAA");
        assert!(wt.path.join("half.txt").exists(), "and NOT destroyed");
    }

    #[test]
    fn commit_turn_is_none_when_there_is_nothing_to_commit() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        assert_eq!(commit_turn(&wt, "opus: said hello").unwrap(), None);
        assert_eq!(commits_ahead(&wt, "main").unwrap(), 0);
    }

    #[test]
    fn branch_diff_is_the_whole_assignment_not_the_working_diff() {
        let repo = git_repo();
        let wt = ensure(repo.path(), &q("opus"), "01AAA").unwrap();
        std::fs::write(wt.path.join("a.txt"), "one\n").unwrap();
        commit_turn(&wt, "opus: turn 1").unwrap().unwrap();

        // Committed, so `git diff HEAD` (the old working_diff) is empty…
        assert_eq!(crate::snapshot::working_diff(&wt.path).unwrap(), "");
        // …but the branch diff still shows the whole assignment.
        let diff = branch_diff(&wt, "main").unwrap();
        assert!(diff.contains("a.txt"), "branch diff carries turn 1: {diff}");
        assert!(diff.contains("+one"));
        assert_eq!(changed_paths(&wt, "main").unwrap(), vec!["a.txt".to_string()]);
        assert_eq!(commits_ahead(&wt, "main").unwrap(), 1);
    }

    // ---- reap_build_artifacts ----

    use std::fs::{File, FileTimes};
    use std::time::{Duration, SystemTime};

    /// Write `path` (creating parent dirs) with `contents`, then back-date its
    /// access time by `age` — the only way to make a freshly-created test fixture
    /// look like a stale build artifact.
    fn write_aged(path: &Path, contents: &[u8], age: Duration) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        let f = File::options().write(true).open(path).unwrap();
        let when = SystemTime::now() - age;
        f.set_times(FileTimes::new().set_accessed(when).set_modified(when)).unwrap();
    }

    #[test]
    fn a_missing_target_dir_is_nothing_to_sweep() {
        let repo = tempfile::tempdir().unwrap();
        let reap = reap_build_artifacts(repo.path(), ARTIFACT_REAP_MIN_AGE).unwrap();
        assert_eq!(reap, Some(ArtifactReap::default()));
    }

    #[test]
    fn a_stale_file_is_removed_and_counted() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("stale.rlib"), b"0123456789", Duration::from_secs(10 * 86400));

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(reap, ArtifactReap { files_removed: 1, bytes_removed: 10 });
        assert!(!debug.join("deps").join("stale.rlib").exists());
    }

    #[test]
    fn a_fresh_file_is_spared() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        // 1 day old, well inside the 7-day threshold — must survive.
        write_aged(&debug.join("deps").join("fresh.rlib"), b"x", Duration::from_secs(86400));

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(reap, ArtifactReap::default(), "a fresh entry must not be swept");
        assert!(debug.join("deps").join("fresh.rlib").exists());
    }

    #[test]
    fn a_stale_fingerprint_directory_is_removed_wholesale_and_its_bytes_summed() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        let fp = debug.join(".fingerprint").join("hadron-gluon-abcd1234");
        // Every file inside, and the directory itself, uniformly 10 days stale.
        write_aged(&fp.join("lib-hadron_gluon.json"), b"12345", Duration::from_secs(10 * 86400));
        write_aged(&fp.join("invoked.timestamp"), b"12", Duration::from_secs(10 * 86400));
        let old = SystemTime::now() - Duration::from_secs(10 * 86400);
        let dir_handle = File::open(&fp).unwrap();
        dir_handle.set_times(FileTimes::new().set_accessed(old).set_modified(old)).unwrap();

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(reap, ArtifactReap { files_removed: 1, bytes_removed: 7 });
        assert!(!fp.exists());
    }

    /// The bug found live on the real box: `cargo build`ing a crate whose
    /// fingerprint is already up to date touches ONE file inside its
    /// `.fingerprint/<crate>-<hash>/` directory (verifying the recorded hash) but
    /// never the directory's own atime — confirmed with `touch -a` + a real cargo
    /// build against this box's actual target dir. A sweep keyed on the
    /// directory's own atime would therefore delete a fingerprint a crate is
    /// being rebuilt against every day, forcing a full rebuild it never needed.
    /// The mirror of the test below, and the one that was missing: a directory
    /// whose OWN atime is fresh but whose every file is stale must still be swept.
    ///
    /// A directory's atime is bumped by merely listing it — proven on this box:
    /// `touch -a -d "8 days ago" dir; ls dir` moved the directory's atime eight
    /// days forward while the file inside stayed stale. `read_dir` does the same,
    /// so **the sweep's own walk refreshes every directory it inspects**, as does
    /// any `ls`, `du`, or backup. Seeding the staleness check with the directory's
    /// own atime therefore made `.fingerprint/`, `incremental/` and `build/`
    /// permanently un-collectable — three of the four categories, and 41G of the
    /// 107G. Only the atimes of FILES mean anything here.
    #[test]
    fn a_directory_touched_by_a_mere_listing_is_still_swept_if_its_contents_are_stale() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        let unit = debug.join("incremental").join("hadron_lattice-abcd1234");
        write_aged(&unit.join("dep-graph.bin"), b"stale", Duration::from_secs(10 * 86400));
        write_aged(&unit.join("query-cache.bin"), b"stale", Duration::from_secs(10 * 86400));
        // The directory itself looks brand new — exactly what a `read_dir`/`ls` leaves behind.
        let now = SystemTime::now();
        File::open(&unit)
            .unwrap()
            .set_times(FileTimes::new().set_accessed(now).set_modified(now))
            .unwrap();

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(3 * 86400))
            .unwrap()
            .unwrap();
        assert!(
            reap.files_removed > 0,
            "a dir whose files are all stale must be swept even though listing it \
             refreshed the dir's own atime; reclaimed {reap:?}"
        );
        assert!(!unit.exists(), "the stale unit should be gone");
    }

    #[test]
    fn a_fingerprint_directory_with_one_fresh_file_inside_is_not_swept() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        let fp = debug.join(".fingerprint").join("hadron-lattice-abcd1234");
        write_aged(&fp.join("lib-hadron_lattice.json"), b"stale", Duration::from_secs(10 * 86400));
        // The directory itself is stale...
        let old = SystemTime::now() - Duration::from_secs(10 * 86400);
        let dir_handle = File::open(&fp).unwrap();
        dir_handle.set_times(FileTimes::new().set_accessed(old).set_modified(old)).unwrap();
        // ...but one file inside was read moments ago, matching a real cache hit.
        write_aged(&fp.join("lib-hadron_lattice"), b"fresh", Duration::ZERO);

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(reap, ArtifactReap::default(), "a directory holding one live file must survive");
        assert!(fp.exists());
    }

    #[test]
    fn a_held_lock_skips_the_whole_sweep() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("stale.rlib"), b"x", Duration::from_secs(10 * 86400));
        std::fs::create_dir_all(&debug).unwrap();
        let lock_path = debug.join(".cargo-lock");
        let held = File::options().create(true).write(true).open(&lock_path).unwrap();
        held.lock().unwrap(); // simulates a live cargo build holding the lock

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap();
        assert_eq!(reap, None, "a held lock must skip the sweep entirely, not wait for it");
        assert!(debug.join("deps").join("stale.rlib").exists(), "nothing may be removed while the lock is held");
    }

    /// `.cargo-lock` is a file that PERSISTS across builds — cargo leaves it on
    /// disk once created, and only locks it while a build runs. A real one on
    /// this box was 0 bytes, dated weeks old, with nothing building. If this
    /// sweep only checked whether the file *exists*, it would refuse to run
    /// forever; it must actually attempt (and release) the advisory lock.
    #[test]
    fn a_present_but_unlocked_lock_file_does_not_skip_the_sweep() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("stale.rlib"), b"x", Duration::from_secs(10 * 86400));
        // A stale lock file, present but held by nobody — exactly what a
        // completed build leaves behind, weeks after the fact.
        let lock_path = debug.join(".cargo-lock");
        std::fs::write(&lock_path, b"").unwrap();

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap();
        assert!(reap.is_some(), "an existing-but-unlocked lock file must not be mistaken for a live build");
        assert_eq!(reap.unwrap().files_removed, 1);
        assert!(!debug.join("deps").join("stale.rlib").exists(), "the sweep must actually run");
    }

    #[test]
    fn all_four_categories_are_swept() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        let old = Duration::from_secs(10 * 86400);
        write_aged(&debug.join("deps").join("a.rlib"), b"1", old);
        write_aged(&debug.join(".fingerprint").join("a-hash").join("f"), b"1", old);
        write_aged(&debug.join("incremental").join("a-hash").join("f"), b"1", old);
        write_aged(&debug.join("build").join("a-hash").join("f"), b"1", old);
        for sub in [".fingerprint", "incremental", "build"] {
            let d = debug.join(sub).join("a-hash");
            let h = File::open(&d).unwrap();
            let t = SystemTime::now() - old;
            h.set_times(FileTimes::new().set_accessed(t).set_modified(t)).unwrap();
        }

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(reap.files_removed, 4, "one stale entry per category, all four swept");
    }

    /// `dry_run` must report exactly what a real sweep would reclaim, without
    /// touching the filesystem — the way to get a trustworthy number off a
    /// production box before the first real (deleting) run.
    #[test]
    fn dry_run_reports_the_reclaim_without_deleting_anything() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("stale.rlib"), b"0123456789", Duration::from_secs(10 * 86400));
        write_aged(&debug.join("deps").join("fresh.rlib"), b"x", Duration::from_secs(86400));

        let reap = reap_build_artifacts_maybe_dry(repo.path(), Duration::from_secs(7 * 86400), true)
            .unwrap()
            .unwrap();
        assert_eq!(reap, ArtifactReap { files_removed: 1, bytes_removed: 10 }, "counts the stale entry only");
        assert!(debug.join("deps").join("stale.rlib").exists(), "dry_run must not delete the stale file");
        assert!(debug.join("deps").join("fresh.rlib").exists());
    }

    /// The number a dry run reports and the number a real run reclaims must
    /// agree exactly, on the same fixture — proves dry_run isn't a second,
    /// possibly-diverging code path.
    #[test]
    fn dry_run_and_a_real_run_agree_on_the_same_fixture() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("a.rlib"), b"12345", Duration::from_secs(10 * 86400));
        write_aged(&debug.join("incremental").join("a-hash").join("f"), b"1234567", Duration::from_secs(10 * 86400));
        let old = SystemTime::now() - Duration::from_secs(10 * 86400);
        let dir_handle = File::open(&debug.join("incremental").join("a-hash")).unwrap();
        dir_handle.set_times(FileTimes::new().set_accessed(old).set_modified(old)).unwrap();

        let dry = reap_build_artifacts_maybe_dry(repo.path(), Duration::from_secs(7 * 86400), true)
            .unwrap()
            .unwrap();
        let real = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap().unwrap();
        assert_eq!(dry, real, "a dry run must predict exactly what the real run reclaims");
    }

    /// A lock held only briefly must NOT skip the sweep: one `try_lock` reported
    /// `WouldBlock` with no holder under concurrent load, which silently skipped
    /// the entire sweep until the next daemon start. The retry has to outlast a
    /// short hold, or it buys nothing.
    #[test]
    fn a_briefly_held_lock_is_waited_out_rather_than_skipping_the_sweep() {
        let repo = tempfile::tempdir().unwrap();
        let debug = repo.path().join("target").join("debug");
        write_aged(&debug.join("deps").join("stale.rlib"), b"0123456789", Duration::from_secs(10 * 86400));
        let lock_path = debug.join(".cargo-lock");

        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let f = File::options().create(true).write(true).open(&lock_path).unwrap();
            f.lock().unwrap();
            locked_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(25));
            // `f` drops here, releasing well inside LOCK_ATTEMPTS * LOCK_RETRY_PAUSE.
        });
        locked_rx.recv().unwrap();

        let reap = reap_build_artifacts(repo.path(), Duration::from_secs(7 * 86400)).unwrap();
        holder.join().unwrap();
        assert_eq!(
            reap,
            Some(ArtifactReap { files_removed: 1, bytes_removed: 10 }),
            "a lock released mid-retry must not read as \"cargo is building\""
        );
    }

    /// The sweep's only report was an `eprintln!` to the daemon's terminal, so an
    /// hour after it deleted 59G nobody could say what it had taken. Every run
    /// must leave one auditable line behind, including a run that took nothing —
    /// "it swept and found nothing" and "it never ran" were indistinguishable.
    #[test]
    fn every_sweep_appends_one_auditable_line() {
        let dir = tempfile::tempdir().unwrap();
        record_artifact_sweep(
            dir.path(),
            &ArtifactReap { files_removed: 6366, bytes_removed: 69_635_695_497 },
        )
        .unwrap();
        record_artifact_sweep(dir.path(), &ArtifactReap::default()).unwrap();

        let log = std::fs::read_to_string(dir.path().join(ARTIFACT_SWEEP_LOG)).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "one line per run, appended not overwritten: {log}");
        assert!(
            lines[0].contains("files=6366") && lines[0].contains("bytes=69635695497"),
            "the exact figures are the whole point: {}",
            lines[0]
        );
        assert!(lines[1].contains("files=0"), "a zero sweep is still recorded: {}", lines[1]);
    }
}
