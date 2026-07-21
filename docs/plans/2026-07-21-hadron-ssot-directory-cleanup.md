---
author: agy
status: draft
---

# Hadron Workspace SSOT Directory Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce `.hadron/` as the sole Single Source of Truth (SSOT) directory across all Quarks and remove legacy `.agents/`, `.claude/`, and `.superpowers/` directories.

**Architecture:** Update Rule 3 in `crates/hadron-gluon/invariants/standard_model.md` to mandate `.hadron/` for all specs, plans, and memory, remove obsolete vendor directories, clean `.gitignore`, and verify the test gate.

**Tech Stack:** Rust, Markdown invariants.

## Global Constraints
- All Quarks MUST store and read memory, specs, plans, and roles under `.hadron/`.
- Quarks MUST NOT create vendor-specific directories like `.claude/`, `.agents/`, `.superpowers/`, `.openai/`, or `.gemini/`.

---

### Task 1: Update Standard Model Rule 3 Invariant

**Files:**

- Modify: `crates/hadron-gluon/invariants/standard_model.md:27-31`
- Test: `crates/hadron-gluon/src/engine/tests.rs`

**Interfaces:**

- Consumes: Existing Standard Model rules.
- Produces: Updated `standard_model.md` with explicit `.hadron/` SSOT constraint.

- [ ] **Step 1: Check baseline hadron-gluon test pass**

Run: `cargo test -p hadron-gluon`
Expected: PASS

- [ ] **Step 2: Update Standard Model Rule 3**

Modify `crates/hadron-gluon/invariants/standard_model.md` Rule 3 to add:

```markdown
## 3. One definition, one place (SSOT).

A value, rule, or type has exactly one home. Copying it creates drift, and a test that
compares two copies is a _guard_, not a source. (This is about production paths; restating
a literal in a test assertion is fine.)

All Quarks (regardless of provider or transport) MUST store, read, and maintain all memory,
specs, plans, and roles exclusively within `.hadron/` (`.hadron/memory/`, `.hadron/docs/specs/`,
`.hadron/docs/plans/`, `.hadron/roles/`). Never create or use vendor-specific subfolders
like `.claude/`, `.agents/`, `.superpowers/`, `.openai/`, or `.gemini/`.
```

- [ ] **Step 3: Run hadron-gluon test gate**

Run: `cargo test -p hadron-gluon`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-gluon/invariants/standard_model.md
git commit -m "docs(invariants): enforce .hadron/ as sole SSOT directory across all Quarks"
```

---

### Task 2: Remove Obsolete Directories & Clean `.gitignore`

**Files:**

- Delete: `.agents/`, `.claude/`, `.superpowers/`
- Modify: `.gitignore:20-22`

**Interfaces:**

- Consumes: Repository root directory structure.
- Produces: Clean checkout with no legacy provider folders.

- [ ] **Step 1: Remove legacy directories**

Run: `rm -rf .agents .claude .superpowers`

- [ ] **Step 2: Clean `.gitignore`**

Modify `.gitignore` to remove lines 20-22 (`.claude/` and `.superpowers/`).

- [ ] **Step 3: Verify git status**

Run: `git status`
Expected: `.gitignore` modified, deleted files staged/tracked.

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "refactor(repo): remove legacy .agents, .claude, and .superpowers directories"
```

---

### Task 3: Full Workspace Test Suite Verification

**Files:**

- Test: Full workspace test suite

- [ ] **Step 1: Run workspace gate with GUI feature**

Run: `cargo test --workspace --features gui`
Expected: PASS with 0 failures.

- [ ] **Step 2: Final git status check**

Run: `git status`
Expected: Clean working tree.
