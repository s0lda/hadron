---
title: Built-in Superpowers for Hadron Quarks
date: 2026-07-15
status: design — approved direction, pending spec review
author: opus (with Jake)
---

# Built-in Superpowers for Hadron Quarks

## Problem

Every quark should produce work to the same standard, whatever model or transport is
behind it. That requires the **same skills** to be available to all of them. Hadron is
going **ACP-first** (Claude is ACP; agy is temporarily CLI as a starting point and will
move to ACP). **ACP quarks inherit none of the CLI's skills, hooks, or plugins**, and
some agents wouldn't have Superpowers even on CLI. So the skills cannot come from the
agent — they must be **built into hadron** and injected by the engine.

An earlier pass ported the Superpowers skill `.md` **files** into
`crates/hadron-gluon/invariants/skills/` and wired `skills.rs` to inject one per turn.
But Superpowers is a *system*, and most of the system is missing.

## Current state (audited against superpowers 6.1.1)

- **Skill inventory is complete**: all 14 Superpowers skills are present, plus a custom
  `reviewing-work`. Nothing to add here.
- **Every companion file is missing.** Skills reference depth that isn't there.
  `systematic-debugging.md` says *"See `root-cause-tracing.md` in this directory"*
  (lines 114, 280-284) — the file doesn't exist, and an ACP quark cannot open hadron's
  source tree anyway. Same dangling references in `executing-plans`,
  `subagent-driven-development`, `test-driven-development`, `writing-skills`,
  `using-superpowers`. Superpowers ships ~25 companion files; hadron has zero.
- **The bootstrap is a hook, and hooks don't cross ACP.** Superpowers installs a
  `SessionStart` hook that injects the `using-superpowers` discipline every session.
  Over ACP that never fires. In hadron `using-superpowers` is merely a *selectable*
  skill (triggers `"use superpowers"`), so it is almost never loaded.
- **No composition.** `skills::select()` picks exactly ONE skill per turn by hardcoded
  trigger phrase; the quark has no Skill tool and no file access, so it cannot reach any
  other skill or any companion file.
- **Duplication.** `prompt.rs:46` tells quarks to use the agent's native
  `superpowers:executing-plans` while `skills.rs` injects hadron's own copy of the same
  thing. One must go.

## The lever: ACP residency

ACP quarks are **resident sessions** — the engine boots the agent once and the
conversation persists across turns, with ~95% prompt-cache hits (per hadron's own token
telemetry). So hadron can put the **entire curated skill library into the stable prompt
prefix**: after turn 1 it is a cache-read (near-free), it lives in the quark's context
all session, composition becomes automatic, references resolve because the referenced
skill is already in context, and each per-turn prompt shrinks to a short pointer.

## Design

### 1. The corpus — self-contained, no dangling references

A curated skill library remains under `crates/hadron-gluon/invariants/skills/`,
restructured so nothing points outside itself:

- **Fold companion content into the skill body** where it carries real procedure:
  - `systematic-debugging` absorbs `root-cause-tracing`, `defense-in-depth`,
    `condition-based-waiting`.
  - `subagent-driven-development` absorbs its `implementer-prompt` / `task-reviewer-prompt`.
  - `test-driven-development` absorbs `testing-anti-patterns`.
  - `writing-plans` / `brainstorming` absorb their reviewer-prompt guidance.
- **Cut the reference** where the companion is CLI-only scaffolding, not procedure:
  brainstorming's browser/visual-companion, `test-pressure-*` fixtures, graphviz render
  scripts, `CREATION-LOG.md`. These do nothing for a quark working over ACP.
- **Keep frontmatter** (`name` / `description`) on every skill — it is the source of the
  one-line index.
- **Invariant:** no skill body may contain a "see X in this directory" pointer to a file
  the quark cannot open. A test asserts this (grep the bodies for the dangling-reference
  pattern).

### 2. Injection — transport-aware

- **ACP (resident):** the full corpus is part of the **stable prompt prefix**
  (Standard Model → corpus → per-turn tail), so it caches after turn 1 and stays in
  context. Per-turn, only a short pointer names the starting skill.
- **CLI (agy, transitional):** inject only the **selected skill body + the one-line
  index**, exactly as today, so agy's per-turn cost does not explode. When agy moves to
  ACP it inherits the resident path automatically.
- The prompt builder learns the transport (a flag on the projection, or the adapter
  supplies the corpus) — resolved in the implementation plan. The prefix ordering that
  keeps the cache stable (already a property of `prompt::build`) is preserved.

### 3. `using-superpowers` — the always-on discipline

`using-superpowers` stops being a selectable skill and becomes part of the standing
rules injected **every turn**: the one-line index of all skills plus "these are
mandatory procedures; the engine hands you the one for this turn; invoke the others as
the work crosses phases." This is hadron's analog of the SessionStart hook.

### 4. Selection & composition — engine picks, quark composes

- The engine's `skills::select()` stays as the **deterministic starting skill**: the
  same task always yields the same procedure, and the engine keeps its provable
  **separation of duties** (plan author ≠ verifier, read from the plan's `author:` line).
- Because the whole library is in-context (ACP), the quark **composes** into other
  skills as the work demands (a bug mid-plan → systematic-debugging; done → 
  requesting-code-review) — the way Superpowers intends.
- A brittle trigger miss is no longer silent: the in-context index + the always-on
  discipline give the quark something to self-select from.

### 5. `standard_model.md` — shortened + hardened

- Replace line 12's vague *"Use Superpowers whenever you can."* with a real enforcement
  clause: the skill index + "these are mandatory procedures; the engine hands you the
  starting one; invoke others as the work crosses phases; if the handed skill is wrong
  for the task, say so rather than half-follow it."
- Trim the *prose* inside rules that a skill now owns, keeping each rule's **claim**:
  e.g. rule 5's baseline detail (verification-before-completion owns it), rule 10's
  simplicity paragraph (reviewing-work owns it). Cut the teaching, keep the law.
- Keep all 10 rules and the reporting format — they are the Standard Model, not up for
  gutting.
- Remove the `prompt.rs:46` reference to native `superpowers:` skills (duplication).

### 5a. Brevity — no TL;DR, discipline only (the hard cap is REMOVED)

**Decision (Jake):** do NOT cut messages. The old `brevity.rs` trimmed the *visible*
reply to a hard cap (14 lines / 1000 chars) and kept the full text only on the envelope —
so a quark could report a critical issue and the human would only ever see a stub. That
is worse than a long reply. `brevity.rs` is **deleted**, `finish_turn` writes the reply
whole, and shortness is achieved by *instruction*, not truncation:

- Standard Model **rule 11 "Be short. No TL;DR"**: answer at the length the question
  deserves, lead with the outcome, cut preamble/restatement/summary-of-your-summary, and
  **never drop a critical detail just to be brief** — the engine does not trim you.
- `prompt.rs` carries the same discipline in the per-turn "How to respond" block.

No numbers, no cap, nothing surfaced from constants — there are no constants anymore.

## Non-goals

- Porting CLI-only scaffolding (browser companion, hooks-as-shell-scripts, test-pressure
  fixtures). The engine is hadron's hook layer; shell hooks are not reproduced.
- A runtime Skill *tool* for quarks. Composition is achieved by having the library in
  context, not by a tool call.
- Changing which models/providers are seated.

## Testing

- `every_listed_skill_has_a_body` (exists) stays green.
- **New:** no skill body contains a dangling "see X in this directory" reference.
- **New:** the ACP prompt prefix is byte-stable across two turns of the same session
  (cache-safety guard).
- **New:** the always-on skill index names every skill (`the_index_lists_every_skill…`);
  the corpus carries every body (`the_corpus_carries_every_skill_body`); a resident quark
  gets a pointer and a one-shot the full body (`render_points_a_resident_and_inlines…`).
- **New:** no skill body dangles a companion reference
  (`no_skill_body_dangles_a_reference_the_quark_cannot_follow`).
- **New:** every unserved human message is serviced, not just the latest
  (`every_unserved_human_message_is_serviced_not_just_the_latest`) — the dispatch fix.
- `skills::select()` determinism tests (exist) stay green.

## Out of scope for this spec, but sequenced alongside

Two independent bugs found while diagnosing "Claude doesn't respond":

1. **Dispatch drop** — `human_message_targets` services only the single latest
   unaddressed human message (`rposition`), so a rapid `@Claude` then `@orchestrator`
   abandons the `@Claude` request. Land this fix **first**; it is the direct cause of the
   original complaint and is contained to `engine.rs`.
2. **Chamber CPU runaway** — `hadron-chamber` pegs ~500%+ CPU; likely why the UI feels
   dead. Investigate after the dispatch fix. Separate from the skills work.

These get their own plan(s); this spec is the skills system.
