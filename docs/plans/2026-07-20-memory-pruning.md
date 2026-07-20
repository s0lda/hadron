# Memory Pruning and Rule 9 Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clean up the codebase memory system by pruning deprecated, obsolete, and changelog-style entries from `index.md` and `notes/`, and upgrading Rule 9 in `standard_model.md` to prevent future context bloating and changelogging.

**Architecture:** We will modify the static Rule 9 standard model guidelines to define a strict quality criteria for memory lessons, allow curation/pruning, and explicitly separate codebase invariants from lesson post-mortems. Then, we will delete deprecated and changelogging entries from the index and remove their corresponding note files.

**Tech Stack:** Markdown, standard git commands.

## Global Constraints
- Do not record what the code already says.
- Maintain index and note file consistency (deleted index entries must have their note files deleted).
- Ensure all tests pass.

---

### Task 1: Upgrade standard_model.md Rule 9

**Files:**
- Modify: `crates/hadron-gluon/invariants/standard_model.md:82-87`

**Interfaces:**
- Produces: Updated Rule 9 guidelines enforced on all quarks.

- [ ] **Step 1: Write the updated Rule 9 content**

Modify `crates/hadron-gluon/invariants/standard_model.md` to update Rule 9:

```markdown
## 9. Maintain the memory: Index, Features, and Invariants.

At the start of every turn, you are handed the memory **index** — the only thing carrying state between sessions. Keep the memory ecosystem clean and compact. The memory is **shared**: a lesson one quark pays for is a lesson none of you pays for twice.
1. **Lessons Index (`index.md`)**: A curated ledger of active engineering mistakes, pitfalls, and post-mortems. One short line per lesson: `- [<slug>](notes/<slug>.md) — <the lesson, in one sentence>`. Notes go in `notes/`. 
   - **Strict Post-Mortem Only**: Do NOT record normal feature implementations, requirements changes, or what the code already says.
   - **Pruning Allowed**: Curation is active: when a lesson becomes obsolete due to structural code changes or is replaced, delete the deprecated lines and their notes to prevent prompt token bloat.
2. **Feature Map (`features.md`)**: Track high-level features, their status, and their entrypoint files. Update this map when you add, modify, or deprecate functionality.
3. **Invariants Registry (`invariants.md`)**: Track operational constraints, rendering rules, environment quirks, and protocol boundaries. If a lesson is resolved by enforcing a permanent codebase constraint, move that constraint to `invariants.md` and prune the post-mortem from `index.md`.
```

- [ ] **Step 2: Verify the change**

Verify the standard_model.md compiles or has no errors.
Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-gluon/invariants/standard_model.md
git commit -m "chore(invariants): upgrade standard model Rule 9 memory rules"
```

---

### Task 2: Prune index.md and notes/

**Files:**
- Modify: `.hadron/memory/index.md`
- Delete: `.hadron/memory/notes/api-key-field-shows-for-sdk-and-acp-only.md`
- Delete: `.hadron/memory/notes/context-progress-bar-on-current-tab-only.md`
- Delete: `.hadron/memory/notes/non-adopted-quarks-use-catalogue-defaults.md`
- Delete: `.hadron/memory/notes/decomposed-memory-ledger-prevents-bloat.md`
- Delete: `.hadron/memory/notes/acp-python-is-a-hallucination.md`

- [ ] **Step 1: Edit index.md to remove deprecated, obsolete, and changelog-style entries**

Remove the following lines from `.hadron/memory/index.md`:
- `statusline-test-timing-flake`
- `acp-python-is-a-hallucination`
- `api-key-field-shows-for-sdk-and-acp-only`
- `non-adopted-quarks-use-catalogue-defaults`
- `prune-prompts-not-replies`
- `never-truncate-prompts-for-brevity` (Replaced by prompt level style rules, no longer needed in index)
- `claude-code-ignores-rules-without-CLAUDE-md`
- `live-card-auto-detect-active-quark`
- `claude-code-ignores-rules-without-CLAUDE-md-deprecated`
- `decomposed-memory-ledger-prevents-bloat`
- `context-progress-bar-on-current-tab-only`

- [ ] **Step 2: Delete corresponding note files**

Run the following commands to delete the unused note files:
```bash
rm .hadron/memory/notes/api-key-field-shows-for-sdk-and-acp-only.md
rm .hadron/memory/notes/context-progress-bar-on-current-tab-only.md
rm .hadron/memory/notes/non-adopted-quarks-use-catalogue-defaults.md
rm .hadron/memory/notes/decomposed-memory-ledger-prevents-bloat.md
rm .hadron/memory/notes/acp-python-is-a-hallucination.md
```

- [ ] **Step 3: Run full workspace test gate**

Run: `cargo test --workspace --features gui`
Expected: PASS

- [ ] **Step 4: Commit and clean up**

```bash
git add .hadron/memory/index.md
git rm .hadron/memory/notes/api-key-field-shows-for-sdk-and-acp-only.md
git rm .hadron/memory/notes/context-progress-bar-on-current-tab-only.md
git rm .hadron/memory/notes/non-adopted-quarks-use-catalogue-defaults.md
git rm .hadron/memory/notes/decomposed-memory-ledger-prevents-bloat.md
git rm .hadron/memory/notes/acp-python-is-a-hallucination.md
git commit -m "chore(memory): prune deprecated, obsolete, and changelog-style entries from index and notes"
```
