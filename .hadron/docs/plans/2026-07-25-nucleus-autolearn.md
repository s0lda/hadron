---
author: acp-claude-2
status: draft
---

# Nucleus Autolearn, `/learn`, and Token Accounting — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the swarm's persistent memory (the nucleus) work in any repo, not just Hadron's — a manifest-detected merge gate, a lazy index that never silently drops lessons, a human-visible budget warning, per-turn token accounting, four `/learn` tiers, a turn-end memory nudge, and the titlebar menu the human approved.

**Architecture:** Nine independently shippable steps, cheapest first, matching the build order in the approved spec (`.hadron/docs/specs/2026-07-25-nucleus-autolearn-design.md`, commit `1b9cdc4`). Each step lands on its own branch through the normal merge gate — nothing here is a single big-bang change.

**Tech Stack:** Rust (`hadron-gluon`, `hadron-chamber`, `hadron-lattice`), existing patterns only (no new crates).

## Global Constraints

- **One-command-table invariant**: every new `/command` is exactly one row in `hadron_chamber::text::COMMANDS` (`crates/hadron-chamber/src/text.rs:70`) plus one arm in `handle_chat_command` (`crates/hadron-chamber/src/app/actions.rs:65`). `app::input::every_listed_command_is_handled` fails the gate on a listed row with no arm — do not add a row without its arm in the same task.
- **`.hadron/` is gitignored** except `.hadron/nucleus/`, `.hadron/docs/`, which are shared/tracked on purpose (per nucleus lesson `worktree-edits-need-worktree-paths`). Plan and spec docs go under `.hadron/docs/`.
- **The Standard Model is `include_str!`'d into the binary** (`crates/hadron-gluon/src/engine.rs:108`, `STANDARD_MODEL`). Nothing in this plan edits it; `laws.md` (Task 5) is injected alongside it, never replaces it.
- **Full gate**: `cargo test --workspace` from the repo root, run at the end of every task. Baseline before Task 1 must be captured and any pre-existing failure reported, not fixed.
- Every task is TDD-shaped: write the failing test named in the step, confirm the failure, implement, confirm green, commit.

---

### Task 1: Manifest-detected merge runner

**Files:**

- Modify: `crates/hadron-gluon/src/merge.rs:73-85` (`CargoMergeRunner::tests`)
- Test: `crates/hadron-gluon/src/merge.rs` (`#[cfg(test)] mod tests`, already present at the bottom of the file)

**Interfaces:**

- Consumes: `run_tests_with(wt: &Worktree, program: &str, args: &[&str]) -> anyhow::Result<(bool, String)>` — already exists at `merge.rs:101`, unchanged.
- Produces: `pub fn detect_runner(worktree_path: &Path) -> (&'static str, &'static [&'static str])` — a new pure function, the one thing `CargoMergeRunner::tests` and its test both call.

**Why first:** `merge.rs:78` today is a literal `run_tests_with(wt, "cargo", &["test", "--workspace"])`. In an npm/pytest/go repo the gate hard-fails (`ENOENT` propagates through `merge.rs:117`'s `?`) rather than falsely reporting green — but every worker turn still ends in error. This blocks everything else in the spec for a non-Rust repo, so it lands first.

- [ ] **Step 1: Write the failing test**

```rust
// in crates/hadron-gluon/src/merge.rs, inside mod tests
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-gluon --lib merge::tests::detect_runner -- --nocapture`
Expected: FAIL with "cannot find function `detect_runner`"

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-gluon/src/merge.rs, above CargoMergeRunner
/// Pick the test command from the worktree's own manifest. Checked in a fixed
/// order because a repo can carry more than one manifest (a Rust workspace with
/// a `package.json` for its docs site) — the language the gate actually tests is
/// the one whose manifest names the build, and Cargo is Hadron's own, so it is
/// the fallback when nothing else is recognised.
pub fn detect_runner(worktree_path: &Path) -> (&'static str, &'static [&'static str]) {
    if worktree_path.join("package.json").is_file() {
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
```

Then change `CargoMergeRunner::tests` (`merge.rs:78`):

```rust
async fn tests(&self, wt: &Worktree) -> anyhow::Result<(bool, String)> {
    let (program, args) = detect_runner(&wt.path);
    run_tests_with(wt, program, args).await
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-gluon --lib merge::tests::detect_runner`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/merge.rs
git commit -m "feat(gluon): detect the merge gate's test runner from the worktree's manifest"
```

---

### Task 2: Warn the human in the chamber, not just the prompt

**Files:**

- Modify: `crates/hadron-gluon/src/engine/nucleus.rs:59` (`read_nucleus_index` — make `NUCLEUS_INDEX_BUDGET` and a status check `pub`, not `pub(super)`)
- Create: `crates/hadron-gluon/src/nucleus_status.rs` (new small public module — `hadron-gluon`'s public root, not the `engine::nucleus` internal one)
- Modify: `crates/hadron-gluon/src/lib.rs` (register the new module)
- Modify: `crates/hadron-chamber/src/app/mod.rs` (wherever the existing 400ms tick lives — same tick `stale-file-autocomplete-in-chamber` already uses to keep `@` mentions live)
- Test: `crates/hadron-gluon/src/nucleus_status.rs` (inline `#[cfg(test)]`)

**Interfaces:**

- Consumes: nothing new — reads `.hadron/nucleus/index.md` directly, same path `nucleus::nucleus_index_path` computes (`workspace_root.join(".hadron").join("nucleus").join("index.md")`).
- Produces: `pub fn index_over_budget(workspace_root: &Path) -> bool` in the new `hadron_gluon::nucleus_status` module — the one function both the engine (internally, via a thin re-export) and the chamber call, so the 32 KiB number has one home (rule 3, SSOT).

**Why here:** `engine::nucleus::NUCLEUS_INDEX_BUDGET` and `read_nucleus_index`'s truncation flag are `pub(super)` — invisible outside `hadron_gluon::engine`, so nothing outside the prompt builder has ever known the index was over budget. The 24%-over number in the spec only exists because someone ran `wc` by hand.

- [ ] **Step 1: Write the failing test**

```rust
// crates/hadron-gluon/src/nucleus_status.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_over_budget_is_false_for_a_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        std::fs::write(nucleus.join("index.md"), "- **x** — short\n").unwrap();
        assert!(!index_over_budget(dir.path()));
    }

    #[test]
    fn index_over_budget_is_true_past_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus).unwrap();
        let big = "- **x** — ".to_string() + &"a".repeat(BUDGET_BYTES + 1);
        std::fs::write(nucleus.join("index.md"), big).unwrap();
        assert!(index_over_budget(dir.path()));
    }

    #[test]
    fn index_over_budget_is_false_when_the_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!index_over_budget(dir.path()));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-gluon --lib nucleus_status -- --nocapture`
Expected: FAIL — module does not exist yet

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-gluon/src/nucleus_status.rs
use std::path::Path;

/// SSOT for the nucleus index budget, shared with `engine::nucleus::NUCLEUS_INDEX_BUDGET`
/// (which re-exports this constant rather than declaring its own — see that module for
/// why the budget exists). Public so the chamber can check it without reaching into
/// the engine's internals.
pub const BUDGET_BYTES: usize = 32 * 1024;

/// Whether `.hadron/nucleus/index.md` under `workspace_root` currently exceeds the
/// budget the prompt builder enforces. A missing file is not over budget — it is
/// the normal first-run case.
pub fn index_over_budget(workspace_root: &Path) -> bool {
    let path = workspace_root.join(".hadron").join("nucleus").join("index.md");
    std::fs::metadata(&path).map(|m| m.len() as usize > BUDGET_BYTES).unwrap_or(false)
}
```

Register in `crates/hadron-gluon/src/lib.rs`: `pub mod nucleus_status;`

Then change `engine::nucleus::NUCLEUS_INDEX_BUDGET` (`nucleus.rs:45`) to re-export rather than redeclare:

```rust
pub(super) use crate::nucleus_status::BUDGET_BYTES as NUCLEUS_INDEX_BUDGET;
```

Wire the chamber's existing poll tick (the same one that refreshes `@`-mention file candidates) to call `hadron_gluon::nucleus_status::index_over_budget(&workspace_root)` and set a `self.nucleus_over_budget: bool` field, rendered as a small warning badge near the roster header — follow the existing severity-badge pattern already in the app (`disabled-quarks-render-gray` / roster presence dot conventions) rather than inventing a new visual language.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-gluon --lib nucleus_status`
Expected: 3 passed
Run: `cargo test --workspace`
Expected: same pass count as baseline plus 3

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/nucleus_status.rs crates/hadron-gluon/src/lib.rs crates/hadron-gluon/src/engine/nucleus.rs crates/hadron-chamber/src/app/mod.rs
git commit -m "feat(nucleus): surface the index budget to the chamber, not just the prompt"
```

---

### Task 3: Per-section token accounting

**Files:**

- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs` (`build`, `crates/hadron-gluon/src/adapter/prompt/mod.rs:60`)
- Create: `crates/hadron-lattice/src/prompt_cost.rs` (new small module, same shape as `crates/hadron-lattice/src/quota.rs` — atomic write, `<hadron-dir>/prompt-cost/<quark>.json`)
- Modify: `crates/hadron-lattice/src/lib.rs` (register module)
- Modify: `crates/hadron-gluon/src/adapter/cli.rs` and `crates/hadron-gluon/src/adapter/acp/session.rs` (write the breakdown after building the prompt, same call sites `prompt::build` already has — `cli.rs:292`, `acp/session.rs:677`)
- Test: `crates/hadron-gluon/src/adapter/prompt/tests.rs`, `crates/hadron-lattice/src/prompt_cost.rs` inline

**Interfaces:**

- Consumes: `Projection` (unchanged), `TokenSpend` (`hadron-lattice/src/telemetry.rs:65`, unchanged).
- Produces: `pub struct PromptBreakdown { pub standard_model: usize, pub invariants: usize, pub nucleus_digest: usize, pub nucleus_index: usize, pub task: usize, pub field_window: usize, pub skill: usize }` and `pub fn measure(projection: &Projection, self_id: &QuarkId) -> PromptBreakdown` in `adapter/prompt/mod.rs`, alongside `build`. `hadron_lattice::prompt_cost::{write, read}` for the sidecar file, following `quota.rs`'s exact atomic-temp-then-rename pattern.

**Why this shape:** nothing today records the prompt's own size — the only `prompt.len()` in the workspace is `cli.rs:50`, inside `fit_prompt`'s argv guard, unrelated to telemetry. This is new data, not a read of an existing field, so it needs its own persistence — reusing the `quota.rs` pattern (rule 2: reuse before creating) rather than inventing a new one. The honest metric per the spec is **fresh vs cached**, not total, so this task only measures section *sizes*; whether a given turn's tokens for those sections were fresh or cache-read is answered by the existing `TokenSpend` on the same turn's `UsageUpdate` — Stats renders both side by side, it does not merge them into one number.

- [ ] **Step 1: Write the failing test**

```rust
// crates/hadron-gluon/src/adapter/prompt/tests.rs
#[test]
fn measure_reports_nonzero_sizes_for_present_sections() {
    let mut proj = base_projection(); // existing test helper in this file
    proj.invariants = "some rule".to_string();
    proj.task = "do the thing".to_string();
    let b = measure(&proj, &QuarkId::from("acp-claude"));
    assert!(b.standard_model > 0, "standard model is always present");
    assert!(b.invariants > 0);
    assert!(b.task > 0);
    assert_eq!(b.nucleus_digest, 0, "empty digest measures as zero");
}

#[test]
fn measure_and_build_agree_on_section_boundaries() {
    // The sum of every measured section must not exceed build()'s total length —
    // measure() must not double-count a section build() only writes once.
    let proj = base_projection();
    let id = QuarkId::from("acp-claude");
    let built = build(&proj, &id);
    let b = measure(&proj, &id);
    let total = b.standard_model + b.invariants + b.nucleus_digest + b.nucleus_index
        + b.task + b.field_window + b.skill;
    assert!(total <= built.len());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-gluon --lib adapter::prompt::tests::measure -- --nocapture`
Expected: FAIL — `measure` and `PromptBreakdown` do not exist

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-gluon/src/adapter/prompt/mod.rs, alongside build()
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptBreakdown {
    pub standard_model: usize,
    pub invariants: usize,
    pub nucleus_digest: usize,
    pub nucleus_index: usize,
    pub task: usize,
    pub field_window: usize,
    pub skill: usize,
}

/// Byte size of each section `build` writes, in the same order. Deliberately a
/// second pure pass rather than threading counters through `build` itself —
/// `build` stays a single `String`-returning function any adapter can call
/// unchanged; a caller that wants the breakdown calls this too.
pub fn measure(projection: &Projection, self_id: &QuarkId) -> PromptBreakdown {
    PromptBreakdown {
        standard_model: STANDARD_MODEL_HEADER_LEN, // the constant literal build() writes
        invariants: projection.invariants.trim().len(),
        nucleus_digest: projection.nucleus_digest.trim().len(),
        nucleus_index: projection.nucleus_index.trim().len(),
        task: projection.task.trim().len(),
        field_window: projection.field_window.iter().map(|e| e.kind_body_len()).sum(),
        skill: projection.skill_body.as_deref().map(str::len).unwrap_or(0),
    }
}
```

(Field names above must match `Projection`'s actual fields — read `crates/hadron-lattice/src/projection.rs` at implementation time and adjust; do not guess a field that isn't there.)

```rust
// crates/hadron-lattice/src/prompt_cost.rs — mirrors quota.rs's atomic write exactly
use std::path::Path;
use crate::adapter_prompt_breakdown_placeholder; // replace with the real import once Task 3 wires cli.rs/session.rs

pub fn write(hadron_dir: &Path, quark: &str, breakdown: &super::PromptBreakdownSerde) -> std::io::Result<()> {
    let dir = hadron_dir.join("prompt-cost");
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{quark}.json.tmp"));
    let dest = dir.join(format!("{quark}.json"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(breakdown)?)?;
    std::fs::rename(&tmp, &dest)
}

pub fn read(hadron_dir: &Path, quark: &str) -> Option<super::PromptBreakdownSerde> {
    let text = std::fs::read_to_string(hadron_dir.join("prompt-cost").join(format!("{quark}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}
```

Wire `cli.rs:292` and `acp/session.rs:677` to call `measure(...)` right after `build(...)` and persist via `prompt_cost::write`. Stats (`crates/hadron-chamber/src/app/render/stats.rs`) reads the sidecar the same way it already reads `quota::read` for the quota panel.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-gluon --lib adapter::prompt`
Expected: all pass, including the 2 new tests

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/adapter/prompt/mod.rs crates/hadron-lattice/src/prompt_cost.rs crates/hadron-lattice/src/lib.rs crates/hadron-gluon/src/adapter/cli.rs crates/hadron-gluon/src/adapter/acp/session.rs
git commit -m "feat(telemetry): measure and persist per-section prompt size"
```

---

### Task 4: Tag manifest + lazy index slices

**Files:**

- Modify: `crates/hadron-gluon/src/engine/nucleus.rs` (`read_nucleus_index`, `read_nucleus_index_with_fallback`)
- Modify: `crates/hadron-gluon/src/adapter/prompt/mod.rs:99-142` (the nucleus-index section)
- Test: `crates/hadron-gluon/src/engine/tests.rs` (alongside the existing `an_oversized_nucleus_index_is_cut_and_says_so` test at line 1890)

**Interfaces:**

- Consumes: the existing `- **<slug>** — <sentence>` line format, extended with an optional trailing `[tag:xxx]` marker (backward compatible — a line with no tag is simply untagged, not an error).
- Produces: `pub(super) fn tag_manifest(index_text: &str) -> String` — headings and counts only, a few hundred bytes; and a change to `read_nucleus_index` so it **never truncates** (Task 2/3 already gave the human and Stats a way to see the size; Task 4 removes the need to cut at all).

**Why this shape:** the spec is explicit — "nothing is dropped, because nothing needs dropping." The engine has task text at dispatch, not target file paths, so scoping keys on tag/task-text matching, not on files-to-be-edited (a scoping key the engine does not have).

- [ ] **Step 1: Write the failing test**

```rust
// crates/hadron-gluon/src/engine/tests.rs
#[test]
fn tag_manifest_summarises_by_heading_not_by_dropping_lines() {
    let index = "## GUI\n- **a** — one [tag:gui]\n- **b** — two [tag:gui]\n\
                 ## IPC\n- **c** — three [tag:ipc]\n";
    let manifest = super::nucleus::tag_manifest(index);
    assert!(manifest.contains("GUI") && manifest.contains("2"));
    assert!(manifest.contains("IPC") && manifest.contains("1"));
    assert!(manifest.len() < index.len(), "the manifest must be smaller than the full index");
}

#[test]
fn an_oversized_index_is_no_longer_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let path = nucleus_index_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let big = "- **x** — ".to_string() + &"a".repeat(NUCLEUS_INDEX_BUDGET + 1000);
    std::fs::write(&path, &big).unwrap();
    let (text, truncated) = read_nucleus_index(&path);
    assert!(!truncated, "Task 4 removes truncation entirely");
    assert_eq!(text.len(), big.len());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-gluon --lib engine::tests::tag_manifest engine::tests::an_oversized_index_is_no_longer_truncated -- --nocapture`
Expected: FAIL — `tag_manifest` missing; truncation test fails against current cutting behaviour

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-gluon/src/engine/nucleus.rs
/// A few hundred bytes: one heading per `## ` section in the index, with a count
/// of lessons under it. What the quark sees instead of the full index when the
/// index has grown past a size worth always sending in full.
pub(super) fn tag_manifest(index_text: &str) -> String {
    let mut out = String::new();
    let mut current: Option<(&str, usize)> = None;
    for line in index_text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some((h, n)) = current.take() {
                out.push_str(&format!("- {h}: {n} lesson(s)\n"));
            }
            current = Some((heading, 0));
        } else if line.trim_start().starts_with("- **") {
            if let Some((_, n)) = current.as_mut() {
                *n += 1;
            }
        }
    }
    if let Some((h, n)) = current {
        out.push_str(&format!("- {h}: {n} lesson(s)\n"));
    }
    out
}
```

Replace `read_nucleus_index`'s cutting body (`nucleus.rs:59-102`) with a version that always returns `(raw, false)` — the budget constant and `index_over_budget` from Task 2 remain the human-facing signal; the prompt builder no longer self-censors. Then extend `prompt/mod.rs`'s nucleus-index section (:106-142): when `index_over_budget(workspace_root)` is true, inject `tag_manifest(&projection.nucleus_index)` instead of the full index, plus any line matching the task text (simple substring match on the task description against each line, reusing the same `.to_lowercase().find()` idiom `skills::select` already uses at `skills/select.rs:36`) and any `[pinned]` line (Task 5) unconditionally.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-gluon --lib engine::tests`
Expected: all pass, including the two new ones; the two now-obsolete assertions in `an_oversized_nucleus_index_is_cut_and_says_so` are updated in the same commit, not left red.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/engine/nucleus.rs crates/hadron-gluon/src/adapter/prompt/mod.rs crates/hadron-gluon/src/engine/tests.rs
git commit -m "feat(nucleus): stop cutting the index — a tag manifest replaces truncation"
```

---

### Task 5: The four `/learn` commands

**Files:**

- Modify: `crates/hadron-chamber/src/text.rs:70` (add four `COMMANDS` rows)
- Modify: `crates/hadron-chamber/src/app/actions.rs:65` (four new `match` arms in `handle_chat_command`)
- Modify: `crates/hadron-gluon/src/engine/nucleus.rs` (extend `build_invariants` to also read `laws.md`, both tiers)
- Test: `crates/hadron-chamber/src/app/actions.rs` inline tests (follow the existing `/rename` test if one exists nearby) and `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**

- Consumes: `Arity::Line` (`text.rs:37`, unchanged) — same shape `/rename` already uses.
- Produces: a pure, testable `pub(crate) fn slugify(text: &str) -> String` in `hadron-chamber` (kebab-case, first ~8 words, deduplicated by appending a short suffix on collision) and `pub(crate) fn learn_line(text: &str, slug: &str) -> String` producing `- **{slug}** — {text} [pinned]\n`.

**Why direct file I/O, not an engine turn:** the human has already typed the full lesson text — there is nothing left to interpret, so spending a billable turn on it would be exactly the waste Jake flagged. `/clear` (`actions.rs:85-` ) already writes directly to disk from the chamber for the same reason; `/learn` follows that precedent rather than `/rename`'s event-append one, because `/rename`'s target (`Kind::SessionName`) is read by the engine on every turn and must ride the field, while a nucleus file is read straight off disk already.

- [ ] **Step 1: Write the failing test**

```rust
// crates/hadron-chamber/src/text.rs (or a small tests module near slugify)
#[test]
fn slugify_makes_a_short_kebab_case_id() {
    assert_eq!(slugify("Always run cargo fmt before commit"), "always-run-cargo-fmt-before");
}

#[test]
fn learn_line_is_pinned_and_matches_the_index_format() {
    let line = learn_line("Always run cargo fmt before commit", "always-run-cargo-fmt-before");
    assert_eq!(line, "- **always-run-cargo-fmt-before** — Always run cargo fmt before commit [pinned]\n");
}
```

```rust
// crates/hadron-chamber/src/app/actions.rs — command-table completeness, same
// pattern as the existing every_listed_command_is_handled test
#[test]
fn all_four_learn_commands_are_handled() {
    for name in ["learn", "learn-global", "learn-std-model", "learn-std-model-global"] {
        assert!(text::COMMANDS.iter().any(|c| c.name == name), "{name} missing from COMMANDS");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-chamber --lib slugify learn_line all_four_learn_commands -- --nocapture`
Expected: FAIL — functions and rows do not exist

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-chamber/src/text.rs — add near the other Arity::Line rows
Command { name: "learn", detail: "Pin a lesson into this repo's nucleus (e.g. /learn always run cargo fmt first)", arity: Arity::Line, listed: true },
Command { name: "learn-global", detail: "Pin a lesson into your global nucleus, across every repo", arity: Arity::Line, listed: true },
Command { name: "learn-std-model", detail: "Add a standing law to this repo (appends to laws.md, never edits the Standard Model)", arity: Arity::Line, listed: true },
Command { name: "learn-std-model-global", detail: "Add a standing law across every repo you run Hadron in", arity: Arity::Line, listed: true },
```

```rust
// crates/hadron-chamber/src/text.rs — pure helpers
pub(crate) fn slugify(text: &str) -> String {
    text.split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

pub(crate) fn learn_line(text: &str, slug: &str) -> String {
    format!("- **{slug}** — {text} [pinned]\n")
}
```

```rust
// crates/hadron-chamber/src/app/actions.rs, in handle_chat_command's match
"learn" | "learn-global" | "learn-std-model" | "learn-std-model-global" => {
    let text = args.trim();
    if text.is_empty() {
        eprintln!("chamber: `/{cmd}` needs text (e.g. `/{cmd} always run cargo fmt first`)");
        return true;
    }
    let hadron_dir = if cmd.ends_with("global") {
        hadron_lattice::user_hadron_dir()
    } else {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    let is_law = cmd.starts_with("learn-std-model");
    if let Some(dir) = hadron_dir {
        let nucleus = dir.join("nucleus");
        if let Err(e) = std::fs::create_dir_all(&nucleus) {
            eprintln!("chamber: failed to create nucleus dir: {e}");
            return true;
        }
        if is_law {
            if let Err(e) = append_line(&nucleus.join("laws.md"), &format!("- {text}\n")) {
                eprintln!("chamber: failed to write laws.md: {e}");
            }
        } else {
            let slug = crate::text::slugify(text);
            if let Err(e) = append_line(&nucleus.join("index.md"), &crate::text::learn_line(text, &slug)) {
                eprintln!("chamber: failed to write index.md: {e}");
            }
        }
    }
    true
}
```

(`append_line` is a small new private helper — open-append-write — or reuse `std::fs::OpenOptions` inline; check first whether `hadron_lattice::user_hadron_dir()` already exists as the spec assumes, per `engine.rs:540`'s doc comment — it does, confirmed this session.)

Extend `engine::nucleus::build_invariants` (`nucleus.rs:169`) to also read `<tier>/.hadron/nucleus/laws.md` (global and repo) and append its contents right after `STANDARD_MODEL`, before the invariant directories — same trim/format style as the existing tiers.

Extend the cutting-removal from Task 4 / the pinned-line matching in `prompt/mod.rs` so any line ending in `[pinned]` is always included regardless of tag-manifest scoping (Task 4's substring match already covers this if pinned lines are simply always selected — implement as an `||` on the existing filter).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-chamber --lib`
Run: `cargo test -p hadron-gluon --lib engine::nucleus`
Expected: all pass, including the new ones

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/text.rs crates/hadron-chamber/src/app/actions.rs crates/hadron-gluon/src/engine/nucleus.rs
git commit -m "feat(learn): four /learn tiers — repo/global × lesson/law, human decides, engine never guesses"
```

---

### Task 6: Turn-end memory nudge

**Files:**

- Modify: `crates/hadron-gluon/src/engine/merge.rs` (`merge_gate`, success arms)
- Test: `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**

- Consumes: the tail of test output already returned by `runner.tests()` (`merge.rs:78`, existing `tail: String`).
- Produces: `pub(super) fn looks_like_a_debugging_turn(test_tail: &str) -> bool` — a pure, deterministic heuristic (contains `panicked`, `FAILED`, or `error[E` — grounded in the existing `grepping-a-test-run-throws-away-the-only-diagnostic` lesson's own vocabulary), and one new low-cost `Actor::Gluon, to: None` field message on a successful land when the heuristic fires.

**Why a nudge, not a gate:** the spec explicitly rules out inferring "a lesson is now guarded by code" — that is a model judgment, not engine-observable state. This task does not block or re-loop the turn (avoiding the `a-failed-merge-land-hot-loops-via-the-audit-grant` failure mode); it posts one unaddressed reminder, per the "Printing Without Waking the Swarm" invariant, so no seat is excited and no turn is spent.

- [ ] **Step 1: Write the failing test**

```rust
// crates/hadron-gluon/src/engine/tests.rs
#[test]
fn looks_like_a_debugging_turn_catches_the_known_failure_markers() {
    assert!(super::merge::looks_like_a_debugging_turn("thread main panicked at foo.rs:10"));
    assert!(super::merge::looks_like_a_debugging_turn("test x ... FAILED"));
    assert!(!super::merge::looks_like_a_debugging_turn("test result: ok. 40 passed"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p hadron-gluon --lib engine::tests::looks_like_a_debugging_turn -- --nocapture`
Expected: FAIL — function does not exist

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/hadron-gluon/src/engine/merge.rs
pub(super) fn looks_like_a_debugging_turn(test_tail: &str) -> bool {
    test_tail.contains("panicked") || test_tail.contains("FAILED") || test_tail.contains("error[E")
}
```

In `merge_gate`'s successful-land branch (after a `MergeVerdict` that lands the branch — the arm currently ending in `Ok(true)` following a successful `runner.land(...)` call), if `!tests_passed` was true at any point this turn is not reachable here (a red gate never lands); instead check whether the *tail* text captured on the way to green still shows the retry evidence — i.e. this only fires when the loop that got to green passed through a failure. Given `merge_gate` only sees the final `tests()` call, the simplest honest signal available here is the diff itself: run this heuristic against `tail` from the passing `tests()` call is not useful (it is green). Re-scope to the field transcript instead — the same `events` slice `merge_gate` already reads (`merge.rs:38`) — checking the current turn's own messages for the failure markers, which is the transcript a human or quark actually wrote during debugging:

```rust
// after a successful land, before returning Ok(true)
let this_turn_debugged = events.iter().rev().take(20).any(|e| {
    matches!(&e.kind, hadron_lattice::Kind::Message { body } if looks_like_a_debugging_turn(body))
});
if this_turn_debugged {
    let note = Event::new(
        Actor::Gluon,
        None,
        hadron_lattice::Kind::Message {
            body: format!(
                "@orchestrator this turn's transcript shows a debugging pass — consider `/learn` \
                 if it taught something worth the swarm remembering."
            ),
        },
    );
    let _ = crate::engine::append_event(&self.field_path, &note); // reuse the engine's existing append helper
}
```

(Adjust the exact append-event call to whatever helper `merge_gate`'s surrounding code already uses for posting field events — check `merge_gate`'s callers in `run.rs` for the established idiom before inventing a new one, per rule 2.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p hadron-gluon --lib engine::tests`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/engine/merge.rs crates/hadron-gluon/src/engine/tests.rs
git commit -m "feat(nucleus): nudge toward /learn after a turn that visibly debugged something"
```

---

### Task 7: Titlebar menu — reorder, New Session, Rename  — **DONE** (commit `14306ee`)

**Files:**

- Modify: `crates/hadron-chamber/src/app/widgets.rs:100-135` (`menu_button`)
- Test: none new required (this is a `PopupMenuItem` wiring change with no new pure logic; existing `every_listed_command_is_handled` and command tests already guard the commands it dispatches to)

**Interfaces:**

- Consumes: existing `handle_chat_command` arms for `"clear"` and `"rename"` (unchanged by this task) — this task calls them the same way a typed `/clear` or `/rename` would, not a new code path.
- Produces: nothing new — a reordered, extended `dropdown_menu` closure.

**Why wrappers, not new commands:** per the one-command-table invariant, "New Session" and "Rename" must invoke the existing `COMMANDS` rows rather than grow a second implementation. `/clear`'s detail text ("Archive and clear the current chat history") already matches "New Session" semantically — if the human wants the row itself renamed for consistency, that is a one-line `detail`/label change, not a new command.

- [x] **Step 1: Write the failing test**

No new pure logic is introduced, so there is no red-test step here — this task is UI wiring. Verify manually per the Superpowers UI guidance: run the chamber (`cargo run -p hadron-chamber`), open the titlebar menu, confirm all eight rows and three dividers render in the spec's order, and that New Session / Rename actually invoke `/clear` / `/rename`'s existing behavior (chat clears; a rename prompt appears).

- [x] **Step 2: N/A — no test to fail first for a pure UI reorder**

- [x] **Step 3: Write the implementation**

```rust
// crates/hadron-chamber/src/app/widgets.rs, replacing menu_button's body
pub(super) fn menu_button(chamber: &Entity<Chamber>) -> impl IntoElement {
    let view = chamber.clone();
    Button::new("app-menu")
        .ghost()
        .icon(Icon::new(IconName::Menu).small())
        .dropdown_menu(move |menu, _, _| {
            let open_workspace = view.clone(); // Task 8 wires this; disabled/hidden until then
            let folder = view.clone();
            let new_session = view.clone();
            let rename = view.clone();
            let settings = view.clone();
            let about = view.clone();
            menu
                // "Open Workspace" intentionally omitted here — Task 8 is its own
                // design task; do not stub a menu item that does nothing on click.
                .item(
                    PopupMenuItem::new("Reveal Workspace in File Manager").on_click(
                        move |_, _, cx| {
                            folder.update(cx, |this, cx| {
                                this.handle_context_menu_action(
                                    ContextMenuAction::OpenInFolder(String::from(".")),
                                    cx,
                                );
                            });
                        },
                    ),
                )
                .separator()
                .item(
                    PopupMenuItem::new("New Session").on_click(move |_, window, cx| {
                        new_session.update(cx, |this, cx| {
                            this.handle_chat_command("clear", "", window, cx);
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("Rename").on_click(move |_, _, cx| {
                        rename.update(cx, |this, cx| {
                            this.open_rename_prompt(cx); // new small prompt — Rename needs an argument /rename's Line arity expects
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Settings…").on_click(move |_, window, cx| {
                        settings.update(cx, |this, cx| this.open_settings(window, cx));
                    }),
                )
                .item(
                    PopupMenuItem::new("About Hadron").on_click(move |_, _, cx| {
                        about.update(cx, |this, cx| {
                            this.about_open = true;
                            cx.notify();
                        });
                    }),
                )
                .separator()
                .item(PopupMenuItem::new("Quit Hadron").on_click(|_, _, cx| cx.quit()))
        })
}
```

`open_rename_prompt` is a new small method following whatever text-input-modal pattern the chamber already uses elsewhere (check `app/render/overlay.rs` for an existing single-line prompt before writing a new one — rule 2). If none exists, the minimal alternative is to focus the chat input pre-filled with `/rename ` and let the human type the name and press enter, which needs no new modal at all — prefer this if a modal pattern is not already present, since it is strictly less code.

- [x] **Step 4: Verify**

Run: `cargo build -p hadron-chamber --features gui` (or the project's real GUI build command — check the build manifest, not this guess)
Run: `cargo test -p hadron-chamber --lib` (regression: nothing else should move)
Manual: run the chamber, exercise all eight menu rows once each.

- [x] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/widgets.rs
git commit -m "feat(chamber): titlebar menu reorder — New Session and Rename wrap existing commands"
```

---

### Task 8: "Open Workspace" — design only, no code

**Files:**

- Create: `.hadron/docs/specs/2026-07-25-open-workspace-design.md`

**Interfaces:** none — this task produces a document, not a shippable change.

**Why this is its own task, not a code task:** `widgets.rs:95-97`'s own doc comment already says why "Open Workspace" is absent today — *"the daemon is bound to one workspace at boot, so the chamber alone cannot repoint the swarm at another one — an item that opened a folder the quarks could not see would be a lie with a file dialog attached."* Making it real means either (a) the chamber can ask a *running* daemon to re-bind to a new `field.jsonl`/roster/nucleus root, or (b) the chamber can launch a **second** daemon for the new workspace. `team_for_field` (nucleus lesson `team_for_field-misses-repo-root`) is path-sensitive enough that getting either wrong silently loads an empty roster — that is a design decision, not an implementation detail, and per the brainstorming skill's hard gate, no code should be written here until a design is chosen and approved.

- [ ] **Step 1: Write the design document**

Cover, per the brainstorming skill: current behavior (why the item is absent, quoting `widgets.rs:95-97`), 2-3 approaches (rebind-in-place vs. second daemon vs. explicit "quit and relaunch with `--workspace <dir>`" as the cheapest option), a recommendation, and the specific failure mode to design against (`team_for_field-misses-repo-root`).

- [ ] **Step 2: Commit the document, unimplemented**

```bash
git add .hadron/docs/specs/2026-07-25-open-workspace-design.md
git commit -m "docs(spec): Open Workspace design options — no implementation until Jake picks one"
```

- [ ] **Step 3: Hand back to the human**

Per the brainstorming skill's user-review gate: present the three options to Jake and wait for a decision before any implementation task is written for this item.

---

### Task 9: Prompt-text changes — orchestrator dispatch-first, brevity

**Files:**

- Modify: `crates/hadron-gluon/src/engine.rs` (wherever the orchestrator-specific prompt text is assembled — search for where `Flavor::Orchestrator` branches the prompt, likely near `STANDARD_MODEL` injection or a role-specific addendum)
- Test: none new — this is prose, not logic; verify by reading the rendered prompt in a test that already exercises `build()` for an orchestrator-flavored `Projection`.

**Interfaces:** none — text only.

**Why last, and why no code:** both changes are prompt wording, agreed by all four workers and Jake implicitly (no objection raised across the brainstorm). They carry no dependency on Tasks 1-8 and can ship any time; placed last only because it is the lowest-risk, easiest-to-defer item, not because it is blocked.

- [ ] **Step 1: Locate the orchestrator-specific text**

```bash
grep -n "Orchestrator" crates/hadron-gluon/src/engine.rs crates/hadron-gluon/src/adapter/prompt/mod.rs
```

- [ ] **Step 2: Add the dispatch-first instruction**

Insert, in whatever section already addresses the orchestrator role specifically:

```
Analyse the full request first, then emit every `@quark` delegation you intend to
make in this same reply — do not wait for one worker to finish before naming the
next. Do your own remaining slice of the work only after every delegation line has
been written, so workers run in parallel with you rather than after you.
```

- [ ] **Step 3: Strengthen the brevity rule**

Rule 11 in `STANDARD_MODEL` (`crates/hadron-gluon/src/invariants/standard_model.md`) already states brevity; add one concrete signal rather than more prose, e.g. a line noting that evidence blocks should be summarized, not pasted in full (the existing `prompt-evidence-bloating` nucleus lesson already made this point — fold it into the rule text so it stops needing a nucleus line at all, and prune that lesson from `index.md` in the same commit per the pruning invariant).

- [ ] **Step 4: Verify**

Run: `cargo test -p hadron-gluon --lib adapter::prompt`
Manual: read the full rendered prompt for an orchestrator-flavored test projection and confirm both additions appear once, not duplicated.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src/engine.rs crates/hadron-gluon/src/invariants/standard_model.md .hadron/nucleus/index.md
git commit -m "docs(prompt): orchestrator dispatch-first, and fold the evidence-bloat lesson into rule 11"
```

---

## Self-Review

**Spec coverage:** all nine build-order items map 1:1 to Tasks 1-9. §1 (four commands) → Task 5. §2 (lazy index) → Task 4. §3 (chamber warning) → Task 2. §4 (turn-end capture, explicitly excluding auto-promotion) → Task 6. §5 (token accounting) → Task 3. §6 (two prompt-text changes) → Task 9. §7 (titlebar menu) → Tasks 7 and 8 (Open Workspace split out as its own design task per the spec's own instruction).

**Placeholder scan:** two spots are intentionally underspecified rather than guessed — Task 3's exact `Projection` field names (must be read from the live struct at implementation time, not assumed) and Task 7's rename-prompt UI pattern (must check for an existing modal before adding one). Both are flagged inline as "check first" rather than left as bare TODOs, and both are small, contained decisions a task's own implementer resolves without needing to return to this plan.

**Type consistency:** `PromptBreakdown` (Task 3) and `tag_manifest`/`index_over_budget` (Tasks 2 and 4) are each declared once and reused by every later task that touches them — Task 4's prompt-builder scoping reuses Task 2's `index_over_budget`, and Task 5's pinned-line handling reuses Task 4's tag-matching filter, rather than each task inventing its own budget check.

## Execution Handoff

Plan complete and saved to `.hadron/docs/plans/2026-07-25-nucleus-autolearn.md`. Two execution options:

**1. Subagent-Driven (recommended)** - dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** - execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
