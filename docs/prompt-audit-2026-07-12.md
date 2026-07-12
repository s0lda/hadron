# Prompt audit — 2026-07-12

Every string Hadron injects into a quark's turn, where it comes from, and what is
wrong with it. Findings only; no changes applied. Verified by reading the source and
the live workspace, not from memory.

The prompt is assembled in `crates/hadron-gluon/src/adapter/prompt.rs::build()` and
handed to the CLI as one argv element. It has nine sections, in this order.

| # | Section | Source | Live status |
|---|---------|--------|-------------|
| 0 | `# Who you are` | `self_id` | OK |
| 1 | `# Working protocol (Invariants)` | `.hadron/nucleus/invariants/*.md` | **EMPTY — dir does not exist** |
| 2 | `# Project knowledge (nucleus)` | `engine.nucleus_digest` | **EMPTY — no nucleus** |
| 3 | `# Your task` | driver / routed message | OK |
| 4 | `# Your authority this turn` | `mode_guidance(mode)` | OK |
| 5 | `# Where you are` | `cwd` + `isolated` | Fixed today (`9710e3b`) |
| 6 | `# Recent field` | last 48 KiB of events | See F3 |
| 7 | `# Current working diff` | `git diff` | See F4 |
| 8 | `# How to respond` + role clauses + truthfulness | static | See F5, F6 |

## F1 — The two sections meant to carry shared rules are empty (highest value)

`build_invariants` (`engine.rs:93`) reads `.hadron/nucleus/invariants/`, always
including `standard_model.md`. **That directory does not exist in this workspace.**
`nucleus_digest` is likewise empty. So sections 1 and 2 — the only two designed to
carry a working protocol and project knowledge — contribute *nothing* to any live
turn.

This is exactly the gap behind "agy has no memory and I have huge memory files". The
mechanism to give every quark the same rules **already exists and is wired**; nobody
has written the files. This is the cheapest possible step toward one shared OS: it is
authoring, not engineering.

**Advice.** Create `.hadron/nucleus/invariants/standard_model.md` holding the rules
this session proved we need, and which currently live only in my (Claude-specific,
non-portable) memory directory:
- Verify before reporting. A test that proves a *patch applied* is not a test that
  proves the *behaviour reaches the user* — the `TextMark::merge` bug and the three
  rounds of `<mark>` both died on exactly this distinction.
- Read the library's source before believing a mechanism can work.
- Commit your own work; check `git ls-files --others --exclude-standard` before
  reporting a clean tree.
- Never `git add -A` in a shared tree.
- A negative result reported early beats a positive result reported wrongly.

## F2 — The prompt asserted a world that did not exist (fixed today)

Section 5 unconditionally told every quark it had "your own checkout, isolated…
changes reach `main` only through the merge gate". The engine falls back to the
shared workspace root (`engine.rs:513`) and there is no merge gate in the codebase.
Agy obeyed the fiction: four refusals to merge, and 235 lines of finished work left
uncommitted. Fixed in `9710e3b` — `Projection::isolated` now records reality and the
shared arm tells the quark to commit its own work.

**The lesson generalises, and it should be the audit's organising principle:** every
sentence in the prompt is a *claim about the world*, and an obedient model will act
on a false claim exactly as faithfully as a true one. So the audit question for each
line is not "is this good advice" but **"is this still true?"**

## F3 — The field window is truncated, and the quark is only sometimes told

`bounded_window` (48 KiB) drops the oldest events. In the agy path, `fit_prompt`
inserts an explicit `[transcript truncated: …]` marker — good. But the engine-level
`bounded_window` truncation happens **silently** for every provider. A quark that
cannot see the human's earlier instruction, and does not know it cannot see it, will
confidently act on a partial picture.

**Advice.** Emit the truncation marker at the engine level, so it is a property of
the projection, not of one adapter. Now that resident sessions are in (`--continue`),
also reconsider the window: we may be re-sending history the CLI already holds.

## F4 — The working diff is injected with no attribution

Section 7 pastes the whole `git diff`. In a shared tree, that diff contains **other
quarks' in-flight edits and the human's**, presented with no indication of whose they
are. A quark reasonably reads it as *its own* prior work.

**Advice.** Label it: "this diff is the shared tree and may contain edits by other
quarks or the human; it is not necessarily yours." Or scope it to paths the quark
touched.

## F5 — The truthfulness clause is an exhortation, not a mechanism

"Never state completed work — commits, passing tests, file edits — that you did not
perform." Asking a model to be honest does not make it able to check. Every
confabulated report this session passed straight through this sentence.

**Advice.** Keep it, but stop relying on it. The mechanism is **claims-vs-facts**:
after a turn, the engine reconciles the quark's claims against ground truth it
already holds (the forge's commit record; a re-run of build and tests) and writes the
delta into the field. That converts "I verified it" from a sentence into a fact — and
it is what makes a cheap executor safe in any seat.

## F6 — Role clauses are sound; two small notes

`is_orchestrator` / `is_worker` are flavour-driven and role-addressed
(`@orchestrator`), so re-flavouring `team.json` retargets them without touching text.
That part is right. Two notes:
- The worker escalation clause is correctly suppressed when no orchestrator exists.
- The hand-back convention (drop the `@mention` to return to the human) is a
  behaviour a weaker model forgets; forgetting silently burns the exchange budget.
  Worth an engine-side nudge rather than only a prompt sentence.

## F7 — Mode guidance is the one clause that already does this right

`mode_guidance` exists precisely because a model not told its constraints "confidently
reports commits and passing tests it never ran (observed live)". It is the template
for the rest: **state the true constraint, in the prompt, in the model's own terms.**
Section 5's fix was applying this same lesson. F3 and F4 are the two places it has
not been applied yet.

## Suggested order for tomorrow

1. **Write the invariants** (F1) — no code, largest effect, and it is the shared-OS
   foundation.
2. **Truth-check the remaining claims** (F3, F4) — the same class of bug as F2, still
   live.
3. **Claims-vs-facts** (F5) — the mechanism that stops us needing to trust prose.
