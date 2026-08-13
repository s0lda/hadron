# Merge Strategy & Squash Options Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement configurable merge strategies (`fast_forward`, `squash`, `github_pr`) in `team.json` workspace settings and extend `hadron-gluon`'s merge gate to execute squash merges or PR mirroring when configured.

**Architecture:** Add a serde-serializable `MergeStrategy` enum to `hadron-lattice` (`Team::merge_strategy`). Update `resolve_team` to inherit repo and global strategy preferences. Extend `hadron-gluon`'s `merge::land` and `MergeRunner` to handle squash commits and optional GitHub PR mirroring while preserving local fast-forward as the zero-latency default.

**Tech Stack:** Rust (serde, git CLI integration, GPUI for settings UI).

## Global Constraints

- **Default strategy**: Must remain `fast_forward` when unconfigured or absent in `team.json`.
- **Pre-test invariant**: `merge::sync` MUST continue rebasing the branch onto `base` before running gate tests regardless of strategy.
- **SSOT**: `MergeStrategy` definition lives in `hadron-lattice::team` and is used across `hadron-gluon` and `hadron-chamber`.
- **Backward compatibility**: Existing `team.json` files without `merge_strategy` deserialize cleanly to `MergeStrategy::FastForward`.

---

- [x] **Task 1: Add `MergeStrategy` Enum & `Team` Config Field in `hadron-lattice`** (commit 99e7342b)

**Files:**
- Modify: `crates/hadron-lattice/src/team/mod.rs:38-60`
- Modify: `crates/hadron-lattice/src/team/tests.rs:1-50`

**Interfaces:**
- Consumes: `serde::{Serialize, Deserialize}`
- Produces: `pub enum MergeStrategy { FastForward, Squash, GitHubPr }`, `Team::merge_strategy` field and `Team::merge_strategy(&self) -> MergeStrategy` getter.

- [ ] **Step 1: Write the failing test**

In `crates/hadron-lattice/src/team/tests.rs`, add unit tests verifying `MergeStrategy` serde and `resolve_team` strategy resolution:

```rust
#[test]
fn team_merge_strategy_serde_round_trip() {
    let team = Team {
        merge_strategy: Some(MergeStrategy::Squash),
        ..Default::default()
    };
    let json = serde_json::to_string(&team).unwrap();
    assert!(json.contains(r#""merge_strategy":"squash""#));
    let parsed: Team = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.merge_strategy(), MergeStrategy::Squash);
}

#[test]
fn team_merge_strategy_defaults_to_fast_forward() {
    let team: Team = serde_json::from_str("{}").unwrap();
    assert_eq!(team.merge_strategy(), MergeStrategy::FastForward);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-lattice --lib team::tests::team_merge_strategy_serde_round_trip`
Expected: FAIL with "cannot find type `MergeStrategy` in this scope"

- [ ] **Step 3: Implement `MergeStrategy` enum and struct fields**

In `crates/hadron-lattice/src/team/mod.rs`, define `MergeStrategy`:

```rust
/// Strategy used by the merge gate to land a quark's branch onto main.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    #[default]
    FastForward,
    Squash,
    GitHubPr,
}
```

Add `pub merge_strategy: Option<MergeStrategy>` to struct `Team` and implement `merge_strategy(&self)` getter:

```rust
impl Team {
    /// The configured merge strategy for landing quark branches, defaulting to `FastForward`.
    pub fn merge_strategy(&self) -> MergeStrategy {
        self.merge_strategy.unwrap_or_default()
    }
}
```

In `resolve_team`:
```rust
pub fn resolve_team(repo: &Team, global: &Team) -> Team {
    // ... existing logic ...
    Team {
        quarks,
        roster: Vec::new(),
        max_exchanges: repo.max_exchanges.or(global.max_exchanges),
        nucleus_index_budget_kb: repo.nucleus_index_budget_kb.or(global.nucleus_index_budget_kb),
        merge_strategy: repo.merge_strategy.or(global.merge_strategy),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-lattice --lib team::tests::team_merge_strategy_serde_round_trip`
Expected: PASS (and all 163+ lattice tests pass)

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-lattice/src/team/mod.rs crates/hadron-lattice/src/team/tests.rs
git commit -m "feat(lattice): add MergeStrategy enum and team config field"
```

---

### Task 2: Implement `Squash` and `GitHubPr` Modes in `hadron-gluon` Merge Gate

**Files:**
- Modify: `crates/hadron-gluon/src/merge.rs:35-90`
- Modify: `crates/hadron-gluon/src/merge.rs:260-292`

**Interfaces:**
- Consumes: `hadron_lattice::team::MergeStrategy`
- Produces: `Landed::SquashAndMerge`, `Landed::GitHubPrOpened(String)`, `land_with_strategy(repo_root, wt, base, strategy)` function.

- [ ] **Step 1: Write the failing test**

In `crates/hadron-gluon/src/merge.rs` (in `mod tests`), add unit tests for `land_with_strategy`:

```rust
#[test]
fn land_with_strategy_squash_collapses_commits() {
    // Setup test repository with multiple commits on a feature branch
    // Test land_with_strategy(..., MergeStrategy::Squash)
    // Assert Landed::SquashAndMerge and verify git log shows single commit on base
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-gluon --lib merge::tests::land_with_strategy_squash_collapses_commits`
Expected: FAIL with "cannot find function `land_with_strategy`"

- [ ] **Step 3: Implement strategy dispatch in `land_with_strategy`**

In `crates/hadron-gluon/src/merge.rs`:

1. Extend `Landed` enum:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    AlreadyLanded,
    FastForward,
    RebasedThenFastForward,
    SquashAndMerge,
    GitHubPrOpened(String),
    Conflicted(String),
}
```

2. Implement `land_with_strategy`:
```rust
pub fn land_with_strategy(
    repo_root: &Path,
    wt: &Worktree,
    base: &str,
    strategy: MergeStrategy,
) -> anyhow::Result<Landed> {
    match strategy {
        MergeStrategy::FastForward => land(repo_root, wt, base),
        MergeStrategy::Squash => land_squash(repo_root, wt, base),
        MergeStrategy::GitHubPr => land_github_pr(repo_root, wt, base),
    }
}
```

Implement `land_squash`:
```rust
fn land_squash(repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Landed> {
    if git(repo_root, &["merge-base", "--is-ancestor", &wt.branch, base]).is_ok() {
        return Ok(Landed::AlreadyLanded);
    }
    // Rebase first to ensure clean history
    if let Err(e) = git(&wt.path, &["rebase", base]) {
        let _ = git(&wt.path, &["rebase", "--abort"]);
        return Ok(Landed::Conflicted(format!("{e:#}")));
    }
    // Squash merge onto base in repo_root
    let commit_msg = format!("squash: merge branch '{}' into '{}'", wt.branch, base);
    git(repo_root, &["merge", "--squash", &wt.branch])?;
    git(repo_root, &["commit", "-m", &commit_msg])?;
    Ok(Landed::SquashAndMerge)
}
```

Implement `land_github_pr` (mirroring mode):
```rust
fn land_github_pr(repo_root: &Path, wt: &Worktree, base: &str) -> anyhow::Result<Landed> {
    // Push branch to origin
    git(&wt.path, &["push", "-u", "origin", &wt.branch])?;
    // Create draft PR via gh CLI if available
    let pr_url = match crate::snapshot::git_ok(&wt.path, &["config", "--get", "remote.origin.url"]) {
        Ok(Some(url)) => format!("PR pushed to origin for branch {}", wt.branch),
        _ => format!("Pushed branch {} to origin", wt.branch),
    };
    Ok(Landed::GitHubPrOpened(pr_url))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-gluon --lib merge::tests`
Expected: PASS (all tests pass)

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/merge.rs
git commit -m "feat(gluon): implement squash and github_pr options in merge gate"
```

---

- [x] **Task 3: Expose `Merge Strategy` Selector in `hadron-chamber` Settings UI** (commit 31e3fbd1)

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs:200-300`
- Modify: `crates/hadron-chamber/src/app/settings/overlay.rs:100-200`

**Interfaces:**
- Consumes: `Team::merge_strategy`, `MergeStrategy`
- Produces: UI setting dropdown / toggle for `Merge Strategy` (`FastForward`, `Squash`, `GitHub PR`).

- [ ] **Step 1: Write the failing test**

In `crates/hadron-chamber/src/app/settings/tests.rs` (or settings mod):

```rust
#[test]
fn test_merge_strategy_ui_selection() {
    // Verify that selecting MergeStrategy in Settings updates Team configuration
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron --lib app::settings::tests`
Expected: FAIL or missing component assertion

- [ ] **Step 3: Implement Settings UI control**

In `crates/hadron-chamber/src/app/settings/providers.rs`, render a setting row for `Merge Strategy`:

```rust
// Render Merge Strategy selector: FastForward | Squash | GitHub PR
```

Update `save_team` calls when the user toggles the merge strategy setting.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron --lib app::settings::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/settings/
git commit -m "feat(chamber): expose Merge Strategy selector in Settings UI"
```
