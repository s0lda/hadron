---
name: memory-curation
description: Maintain .hadron/nucleus (Rule 9) — distill lessons into notes/, prune index.md, and update features.md
---

# Memory Curation

Maintain the shared swarm memory under `.hadron/nucleus/` according to Standard Model Rule 9.

## Core Principles (Rule 9)
1. **The Index is a Routing Table, Not a Ledger:** One line per lesson (`- [<slug>](notes/<slug>.md) — <hook>`), hook capped at ~100 characters. Content in `index.md` is a bug.
2. **One Fact Per File:** Each lesson in `notes/<slug>.md` captures exactly one non-obvious learning.
3. **Strict Post-Mortem Only:** Never record routine feature notes or what the code already says.
4. **Update or Delete Before Create:** Edit existing notes or prune obsolete lessons.

## Curation Steps

### 1. Audit `index.md` Budget & Formatting
- Verify total byte size of `index.md` stays comfortably within the 32 KB budget.
- Verify every line in `index.md` conforms to the routing format: `- [<slug>](notes/<slug>.md) — <hook>`.

### 2. Prune Stale Lessons & Promote Invariants
- Delete lessons made obsolete by architectural rewrites or permanent structural guards.
- If a lesson is resolved by enforcing a permanent codebase constraint, move that constraint to `.hadron/nucleus/invariants/always.md` and remove the post-mortem from `index.md`.

### 3. Upkeep Feature Map (`features.md`)
- Update component statuses, entrypoint file paths, and high-level architectural references.
