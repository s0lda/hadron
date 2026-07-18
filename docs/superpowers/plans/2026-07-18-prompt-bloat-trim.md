# Prompt-bloat Trim Implementation Plan (WS4 §5)

> **For agentic workers:** REQUIRED SUB-SKILL: subagent-driven-development / executing-plans. Checkbox steps.

**Goal:** Stop dumping the full skill corpus (~70-80k tokens) into resident (ACP) quark prompts every turn; give every quark only `skills::index()` + the active skill's full body.

**Architecture:** In `engine.rs`'s projection builder, delete the resident-only `skills::corpus()` push and change the active-skill `render(...)` `include_body` from `!resident` to `true`. Remove the now-unused `skills::corpus()`.

**Tech Stack:** Rust (hadron-gluon). cargo test.

## Global Constraints
- Baseline gate before/after: `cargo test --workspace --features gui` (full).
- INERT: cargo test/check only, never run binaries; tempdirs only; don't touch live ~/.hadron.
- Additive-safety: `skills::index()` (the menu) stays injected every turn, so composition still works. Only the verbatim bodies of non-active skills are removed.
- Update — do not delete — existing tests that encoded the old resident-corpus behaviour; each such change is called out in the report (it's the intended contract change, not papering over a regression).
- One focused commit.

---

### Task 1: Trim the skill injection to index + active-skill body for all quarks

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs` (projection builder ~808-866: remove corpus block; `include_body` `!resident`→`true`; update doc comments)
- Modify: `crates/hadron-gluon/src/skills.rs` (remove `pub fn corpus()` if unused after; keep `index()`/`render()`/`select()`)
- Test: inline `#[cfg(test)]` engine/skills tests

- [ ] **Step 1: Write the discriminating failing test** (engine test module). Build a projection for a **resident** target on a task that matches a known skill (use the same fixtures existing engine tests use for the projection/skill path — grep `skills::select`/`resident`/`Projection` in engine.rs tests). Assert on `projection.invariants`:
  - contains the skill **index** (a stable substring `skills::index()` emits — e.g. a known skill name/summary line);
  - contains the **matched** skill's body (a distinctive line from that skill's markdown);
  - does NOT contain a body-only line from a DIFFERENT, non-matched skill (the line that would appear only if the whole corpus were dumped).
  Name it e.g. `resident_quark_gets_index_plus_active_body_not_the_whole_corpus`.

- [ ] **Step 2: Run — expect FAIL** (`cargo test -p hadron-gluon resident_quark_gets_index_plus_active_body`). Today a resident quark's invariants contain the whole corpus, so the "does NOT contain other-skill body" assertion fails.

- [ ] **Step 3: Implement the trim** in `engine.rs`:
  - Delete the block: `let resident = self.resident.contains(target); if resident { invariants_text.push_str(&skills::corpus()); }` (keep the `resident` binding ONLY if it's used elsewhere in the function; if its only use was the corpus + the `!resident` render arg, remove it).
  - In the `skills::render(&m, target, &Handoff{..}, <include_body>)` call, set `include_body = true` (was `!resident`).
  - Update the doc comments (the paragraphs at ~830-835 and ~858-859 explaining the resident-corpus split) to describe the new contract: every quark gets the index + the active skill's body; residents no longer receive the whole library.

- [ ] **Step 4: Remove dead `corpus()`** — if `skills::corpus()` now has no non-test callers (grep confirms), delete it from `skills.rs`. If a test still wants "render all," convert that need to a `#[cfg(test)]` helper rather than keeping a public `corpus()`; prefer outright deletion (SSOT).

- [ ] **Step 5: Update old-contract tests** — grep for tests asserting corpus presence for resident quarks (e.g. asserting a non-active skill's body is present for a resident target). Update each to the new contract (index + active body only). List every test changed in the report with the reason.

- [ ] **Step 6: Run** focused tests + full gate. Expect PASS.

- [ ] **Step 7: Commit** — `git add crates/hadron-gluon/src/engine.rs crates/hadron-gluon/src/skills.rs && git commit -m "perf(gluon): trim resident prompt to index + active skill (drop skills::corpus)"`

---

## Self-Review
- Spec §2 change (delete corpus block; `include_body=true`; remove corpus()) → Task 1 Steps 3-4. §4 testing (resident discriminating test; CLI unchanged; update old-contract tests) → Steps 1,5. §3 safety (index still injected) → preserved (line 828 push_str(index()) untouched). ✓
- Placeholder scan: the discriminating substrings are described by their source (`skills::index()` output line, matched skill body line, other-skill body line) — the implementer must read the actual skill fixtures to pick concrete strings; that's a read, not a placeholder. ✓
