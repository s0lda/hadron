---
author: acp-claude-2
status: draft
---

# Nucleus as the Real, Single Knowledge Store — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `.hadron/nucleus/` the swarm's one real, live knowledge root — wiring the currently-dead nucleus digest for real, giving rule 9's promised `features.md` an actual reader, and moving the lessons ledger (`memory/index.md` + `notes/`) there — without ever showing a quark an empty memory during the transition.

**Architecture:** Two new free functions on `hadron_gluon::engine` (`build_nucleus_digest`, `migrate_legacy_memory`) that the daemon bin calls directly at boot — no resurrection of the unused `nucleus::load`/`digest`/index.json ceremony (rules 2/10: `engine::memory`'s existing directory-scan pattern already does this job one directory over, and index.json has zero maintained instances). The lessons reader is repointed to `.hadron/nucleus/` with a content-level fallback to `.hadron/memory/` so a quark never sees empty memory in the window before migration runs. `.hadron/` is gitignored, so a plan-time file `mv` accomplishes nothing for Jake's real checkout — the boot-time migration is the only mechanism that can actually relocate his files (rule 1: prove the caller, not the compile).

**Tech Stack:** Rust, `std::fs`, existing `hadron-gluon` engine/routing/prompt modules.

## Global Constraints

- **Baseline gate (must stay green):** `cargo test --workspace` = 120/18/40/332(8 ignored)/6/127 passed across the six test crates. Run the FULL gate before and after.
- **`.hadron/` is gitignored** — this plan doc is committed with `git add -f`.
- **Scope split (rule 10 — say so, don't force it):** this plan is **Unit A only** — real digest content, features reader, path repoint, boot migration, fallback read, and the rule-9/injection-block *path* text. It deliberately does **NOT** rename `memory_path`/`memory_notes_dir`/`memory_truncated` (Projection fields), `read_memory_index`, or the `"(memory index)"` header wording — that pure-rename pass (Unit B) has the widest blast radius (Projection is consumed by `cli.rs`, `acp/tests.rs`, `prompt/tests.rs`, `engine/tests.rs`) and is pure churn with zero new behavior; it is the natural last commit once Unit A is proven, or a clean hand-off point. Flagged to `@orchestrator` at the end, not silently dropped.
- **Rule 1 honesty:** a unit test on the new functions does not discharge "wired." The proof is: `bin/hadron-gluon.rs` calls them, and a test builds the `Engine` the same way the bin does and asserts the projection's `nucleus_digest` is non-empty from a real file. Booting the live daemon is out of reach for a plan-execution turn — say so, don't fake it.

---

## Task 1: `build_nucleus_digest` — the features-map reader rule 9 promises

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs` (new `pub fn`, near `with_nucleus`)
- Test: inline `#[cfg(test)]` in `engine.rs` or `engine/tests.rs`

**Interfaces:**
- Produces: `pub fn build_nucleus_digest(workspace_root: &Path) -> String` — reads `<workspace_root>/.hadron/nucleus/features.md`; missing file → `String::new()` (not an error — a fresh project has no feature map yet).

- [ ] **Step 1: Write the failing test**
```rust
#[test]
fn build_nucleus_digest_reads_features_md() {
    let dir = tempfile::tempdir().unwrap();
    let nucleus = dir.path().join(".hadron").join("nucleus");
    std::fs::create_dir_all(&nucleus).unwrap();
    std::fs::write(nucleus.join("features.md"), "## Widget\nstatus: shipped\n").unwrap();
    let digest = build_nucleus_digest(dir.path());
    assert!(digest.contains("Widget"));
}

#[test]
fn build_nucleus_digest_is_empty_when_no_features_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(build_nucleus_digest(dir.path()), "");
}
```
- [ ] **Step 2:** Run `cargo test -p hadron-gluon build_nucleus_digest` → FAIL (function undefined).
- [ ] **Step 3: Implement**
```rust
pub fn build_nucleus_digest(workspace_root: &Path) -> String {
    let path = workspace_root.join(".hadron").join("nucleus").join("features.md");
    std::fs::read_to_string(path).unwrap_or_default()
}
```
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(nucleus): read features.md into the nucleus digest`.

---

## Task 2: Wire the digest into the daemon for real (discharges `nucleus-load-digest-is-unwired`)

**Files:**
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs` (the `Engine::new(...)` builder chain, ~line 342)
- Test: `crates/hadron-gluon/src/engine/tests.rs` (a projection-level test, since the bin itself has no test harness)

**Interfaces:**
- Consumes: `build_nucleus_digest` (Task 1), `Engine::with_nucleus` (existing, `engine.rs:481`).

- [ ] **Step 1: Write the failing/characterizing test** — build the `Engine` exactly as the bin does (git + nucleus, no mocking) and assert the projection carries real content:
```rust
#[tokio::test]
async fn nucleus_digest_renders_from_a_real_features_file() {
    let dir = tempfile::tempdir().unwrap();
    let nucleus = dir.path().join(".hadron").join("nucleus");
    std::fs::create_dir_all(&nucleus).unwrap();
    std::fs::write(nucleus.join("features.md"), "## Login\nstatus: done\n").unwrap();
    let digest = crate::engine::build_nucleus_digest(dir.path());
    let engine = Engine::new(dir.path().join("field.jsonl"), vec![], 12).with_nucleus(digest);
    // (existing test helpers construct a minimal event set / target — mirror
    // the pattern already used by `projection_carries_nucleus_digest` at
    // engine/tests.rs:1268, which this test sits beside.)
    let turn = engine.projection_for(&[], &QuarkId::new("x"), None, String::new(), None);
    assert!(turn.nucleus_digest.contains("Login"));
}
```
- [ ] **Step 2:** Run → this should already PASS (it only exercises `with_nucleus`, which already works) — the point of this test is to pin the *real* composition (`build_nucleus_digest` → `with_nucleus`) that the bin will use, not to prove new engine behavior. If it fails, the composition itself is wrong — stop and re-check Task 1.
- [ ] **Step 3: Wire the bin** — in `bin/hadron-gluon.rs`, add to the existing builder chain (do not reorder the other calls):
```rust
    let engine = Engine::new(args.field_path.clone(), quarks, max_exchanges)
        .with_git(repo_root.clone())
        .with_merge_gate(std::sync::Arc::new(hadron_gluon::merge::CargoMergeRunner))
        .with_nucleus(hadron_gluon::engine::build_nucleus_digest(&repo_root))
        .with_global_skills_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("skills")))
        .with_global_agents_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("agents")));
```
(`repo_root` already exists at this call site — `.clone()` it since `with_git` also consumes it by value.)
- [ ] **Step 4:** `cargo build -p hadron-gluon --bin hadron-gluon` → compiles.
- [ ] **Step 5:** Commit `feat(gluon): wire nucleus digest into the daemon bin (discharges nucleus-load-digest-is-unwired)`.

**What this does NOT prove:** that the live daemon, booted for real against Jake's `.hadron/`, renders the section in an actual prompt sent to an actual model. That needs a running daemon + a live turn — out of reach this turn. State this plainly in the report; do not imply it was observed live.

---

## Task 3: Repoint the lessons reader to `.hadron/nucleus/`, with a legacy fallback

**Files:**
- Modify: `crates/hadron-gluon/src/engine/memory.rs`
- Modify: `crates/hadron-gluon/src/engine/routing.rs` (the one call site, `~line 418-419`)
- Modify: `crates/hadron-gluon/src/engine/tests.rs` (repoint the existing path assertion — `~line 1763`, currently asserts `.ends_with("memory/index.md")`)
- Test: inline in `memory.rs`

**Interfaces:**
- Produces: `pub(super) fn read_memory_index_with_fallback(workspace_root: &Path) -> (String, bool)`.
- `memory_index_path`/`memory_notes_dir` now return `.hadron/nucleus/index.md` / `.hadron/nucleus/notes` — same function names, new values (SSOT: the prompt header text in `prompt/mod.rs` already renders whatever these return, so repointing the value is enough — no second place to edit).

- [ ] **Step 1: Failing tests** — path functions now point at nucleus; fallback reads legacy content when nucleus is empty:
```rust
#[test]
fn memory_paths_now_live_under_nucleus() {
    let root = std::path::Path::new("/repo");
    assert_eq!(memory_index_path(root), std::path::PathBuf::from("/repo/.hadron/nucleus/index.md"));
    assert_eq!(memory_notes_dir(root), std::path::PathBuf::from("/repo/.hadron/nucleus/notes"));
}

#[test]
fn fallback_reads_legacy_memory_when_nucleus_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("index.md"), "- **old-lesson** — from before the move\n").unwrap();
    // nucleus/index.md does not exist yet — migration hasn't run.
    let (text, truncated) = read_memory_index_with_fallback(dir.path());
    assert!(text.contains("old-lesson"));
    assert!(!truncated);
}

#[test]
fn fallback_prefers_nucleus_once_it_has_content() {
    let dir = tempfile::tempdir().unwrap();
    let nucleus = dir.path().join(".hadron").join("nucleus");
    std::fs::create_dir_all(&nucleus).unwrap();
    std::fs::write(nucleus.join("index.md"), "- **new-lesson** — after the move\n").unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("index.md"), "- **old-lesson** — should not appear\n").unwrap();
    let (text, _) = read_memory_index_with_fallback(dir.path());
    assert!(text.contains("new-lesson"));
    assert!(!text.contains("old-lesson"));
}
```
- [ ] **Step 2:** Run → FAIL (function undefined; path assertions fail against the old `.hadron/memory/...` values).
- [ ] **Step 3: Implement** in `memory.rs`:
```rust
fn nucleus_lessons_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("nucleus")
}

fn legacy_memory_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("memory")
}

pub(super) fn memory_index_path(workspace_root: &std::path::Path) -> std::path::PathBuf {
    nucleus_lessons_dir(workspace_root).join("index.md")
}

pub(super) fn memory_notes_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    nucleus_lessons_dir(workspace_root).join("notes")
}

/// Read the lessons index from its home (`.hadron/nucleus/index.md`),
/// falling back to the pre-migration legacy location
/// (`.hadron/memory/index.md`) if nucleus is empty — so a quark is never
/// shown an empty memory in the window before `Engine::migrate_legacy_memory`
/// has run at daemon boot (see Task 4).
pub(super) fn read_memory_index_with_fallback(workspace_root: &std::path::Path) -> (String, bool) {
    let (text, truncated) = read_memory_index(&memory_index_path(workspace_root));
    if !text.trim().is_empty() {
        return (text, truncated);
    }
    read_memory_index(&legacy_memory_dir(workspace_root).join("index.md"))
}
```
(`read_memory_index(path)` itself — the budget/truncation logic — is unchanged; this only changes what feeds it.)
- [ ] **Step 4:** Update `routing.rs`'s one call site:
```rust
let memory_path = memory_index_path(&workspace_root);
let (memory, memory_truncated) = read_memory_index_with_fallback(&workspace_root);
```
- [ ] **Step 5:** Update the stale assertion in `engine/tests.rs` (~line 1763) from `.ends_with("memory/index.md")` to `.ends_with("nucleus/index.md")` (and the neighboring `memory/notes` assertion to `nucleus/notes`) — this is the expected, intentional behavior change under test, not a workaround.
- [ ] **Step 6:** Run `cargo test -p hadron-gluon` → all PASS, including the repointed test.
- [ ] **Step 7:** Commit `feat(nucleus): repoint the lessons reader to .hadron/nucleus/, with a legacy fallback`.

---

## Task 4: Boot-time migration (the only thing that can actually move Jake's files)

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs` (new `pub fn`, near `build_nucleus_digest`)
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs` (call it once, before the event loop)
- Test: inline in `engine.rs` or `engine/tests.rs`

**Interfaces:**
- Produces: `pub fn migrate_legacy_memory(workspace_root: &Path) -> std::io::Result<bool>` — `Ok(true)` if it moved files, `Ok(false)` if already migrated or nothing to migrate.

- [ ] **Step 1: Failing tests** — moves real content; idempotent on a second call; no-op on a fresh project:
```rust
#[test]
fn migrates_legacy_index_and_notes_into_nucleus() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(legacy.join("notes")).unwrap();
    std::fs::write(legacy.join("index.md"), "- **x** — real content\n").unwrap();
    std::fs::write(legacy.join("notes").join("x.md"), "the long version").unwrap();

    let moved = migrate_legacy_memory(dir.path()).unwrap();
    assert!(moved);
    let nucleus = dir.path().join(".hadron").join("nucleus");
    assert_eq!(std::fs::read_to_string(nucleus.join("index.md")).unwrap(), "- **x** — real content\n");
    assert_eq!(std::fs::read_to_string(nucleus.join("notes").join("x.md")).unwrap(), "the long version");
}

#[test]
fn migration_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(".hadron").join("memory");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("index.md"), "old").unwrap();
    assert!(migrate_legacy_memory(dir.path()).unwrap());
    // Second boot: nucleus/index.md now exists — must not touch anything again.
    std::fs::write(legacy.join("index.md"), "should be ignored now").unwrap();
    assert!(!migrate_legacy_memory(dir.path()).unwrap());
    let nucleus = dir.path().join(".hadron").join("nucleus");
    assert_eq!(std::fs::read_to_string(nucleus.join("index.md")).unwrap(), "old");
}

#[test]
fn fresh_project_has_nothing_to_migrate() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!migrate_legacy_memory(dir.path()).unwrap());
}
```
- [ ] **Step 2:** Run → FAIL (function undefined).
- [ ] **Step 3: Implement**
```rust
/// One-time, idempotent migration of the legacy `.hadron/memory/` lessons
/// ledger into `.hadron/nucleus/`, the swarm's single knowledge root.
///
/// `.hadron/` is gitignored, so this is the ONLY thing that can relocate a
/// user's real on-disk lessons — a quark-worktree `mv` would be invisible to
/// the daemon that actually reads them (`dot-hadron-is-gitignored`). Called
/// once at daemon boot, before any turn can run, so there is no race with a
/// quark writing a fresh `nucleus/index.md` before migration sees it.
pub fn migrate_legacy_memory(workspace_root: &Path) -> std::io::Result<bool> {
    let nucleus_dir = workspace_root.join(".hadron").join("nucleus");
    let legacy_dir = workspace_root.join(".hadron").join("memory");
    if nucleus_dir.join("index.md").exists() {
        return Ok(false);
    }
    if !legacy_dir.join("index.md").exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&nucleus_dir)?;
    std::fs::rename(legacy_dir.join("index.md"), nucleus_dir.join("index.md"))?;
    let legacy_notes = legacy_dir.join("notes");
    if legacy_notes.exists() {
        std::fs::rename(legacy_notes, nucleus_dir.join("notes"))?;
    }
    Ok(true)
}
```
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5: Wire it into the bin**, before the engine is constructed (so `build_nucleus_digest`/the reader see post-migration state on this very boot):
```rust
    if let Err(e) = hadron_gluon::engine::migrate_legacy_memory(&repo_root) {
        eprintln!("hadron-gluon: memory→nucleus migration failed (non-fatal): {e:#}");
    }
```
Place this immediately before the `let engine = Engine::new(...)` line (~342), after `repo_root` is computed. A failure is logged, not fatal — the fallback reader (Task 3) covers the gap.
- [ ] **Step 6:** `cargo build -p hadron-gluon --bin hadron-gluon` → compiles.
- [ ] **Step 7:** Commit `feat(nucleus): self-healing boot migration from .hadron/memory/ to .hadron/nucleus/`.

**Security (rule 7):** `migrate_legacy_memory` only ever touches two hardcoded, repo-relative subpaths (`.hadron/memory`, `.hadron/nucleus`) it derives itself from `workspace_root` — no user- or LLM-supplied path is involved. No new attack surface.

---

## Task 5: Standard Model text — name `.hadron/nucleus/` where it currently implies `.hadron/memory/`

**Files:**
- Modify: `crates/hadron-gluon/invariants/standard_model.md` (rule 9)
- Test: none (prose; the injection block's *paths* are already covered by Task 3's Projection values — this task is the surrounding prose only)

- [ ] **Step 1:** In rule 9, add one clarifying line naming the physical home and the new features-map reader, without touching the "Lessons Index (`index.md`)" bullet's wording (that stays correct as-is):
```markdown
## 9. Maintain the memory: Index, Features, and Invariants.

At the start of every turn, you are handed the memory **index** — the only thing carrying state between sessions. Keep the memory ecosystem clean and compact. The memory is **shared**: a lesson one quark pays for is a lesson none of you pays for twice. All three live under `.hadron/nucleus/` — `index.md`/`notes/` (lessons), `invariants/` (already there), and `features.md` (read automatically into every prompt's nucleus digest).
1. **Lessons Index (`index.md`)**: ...
```
(Keep bullets 1–3 exactly as they are otherwise — only the intro line changes.)
- [ ] **Step 2:** `cargo build -p hadron-gluon` → compiles (this file is `include_str!`'d — a syntax slip would still compile fine since it's a plain string, but confirm the build succeeds as a sanity check that nothing else broke).
- [ ] **Step 3:** Commit `docs(standard-model): rule 9 names .hadron/nucleus/ as the physical home`.

---

## Final gate (rule 5)

- [ ] `cargo test --workspace` — expect ≥120/18/40/332(8 ignored, or fewer once any repointed)/6/127 passed, 0 new failures. Baseline recorded above; report the delta.
- [ ] `cargo test -p hadron-gluon` in isolation — the crate every task here touches.
- [ ] Update `.hadron/memory/index.md`'s own index entry for `nucleus-load-digest-is-unwired`: it is corrected by this work (the loader now HAS a caller) — note the correction, don't just delete the lesson (the "unwired" history is still true of the *old* `nucleus::load`/`digest` module, which this plan does not touch or delete).

## Self-review notes (done while writing)

- **Coverage:** every item from the dispatch maps to a task — wire loader for real (Task 2), features-map reader (Task 1), consolidate memory (Task 3+4), Standard Model text (Task 5). Self-healing migration + fallback (the follow-up message) is Task 4 + Task 3's fallback.
- **Explicitly out of scope, flagged, not forced:** retiring the word "memory" from Rust identifiers and the prompt header (`read_memory_index`→`read_nucleus_index`, `Projection.memory_path`→`nucleus_index_path`, `"(memory index)"`→`"(nucleus index)"`) — Unit B, per the Global Constraints note above. Also out of scope: deleting the old dead `hadron-gluon::nucleus::load`/`digest` module — Claude's dispatch asked to wire *a* nucleus digest for real, which this plan does via the simpler proven dir-scan pattern (rules 2/10) rather than resurrecting the unused index.json-manifest system; the old module is now provably still dead and a candidate for deletion, called out in the report for `@orchestrator` to ratify rather than deleted unilaterally mid-plan.
- **Type/name consistency:** `build_nucleus_digest`, `migrate_legacy_memory`, `read_memory_index_with_fallback` are used identically across Tasks 1→4; `memory_index_path`/`memory_notes_dir` keep their existing names (value-only change) per the Unit A/B split.
