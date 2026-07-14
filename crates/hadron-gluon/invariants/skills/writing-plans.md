# Skill: Writing plans

You are writing a plan this turn. **Write the plan. Do not implement it.** The
value of a plan is that a *different* quark can execute it without your context —
and your context dies when this turn ends.

## Where it goes

`docs/plans/YYYY-MM-DD-<slug>.md`, committed. Not `.hadron/` — that is gitignored,
and a plan that is not in git is a plan that evaporates. This is the whole point:
the work survives the turn that produced it.

## The header — required, exactly this

```markdown
---
author: <your quark id>
status: draft
---

# <Feature> — implementation plan

**Goal:** one sentence.
**Architecture:** 2–3 sentences on the approach.
**Baseline:** the gate command and the numbers it printed BEFORE any change.
```

The `author:` line is not decoration. The engine reads it, and a quark that tries
to execute or verify a plan it wrote is told to hand it off instead. If you omit
it, that check cannot fire and you have removed your own reviewer.

## The tasks

Write for an engineer who is a skilled developer and knows **nothing** about this
codebase — because that is literally true of the quark who picks it up. Every task:

```markdown
### Task N: <name>

**Files:** exact paths — `crates/x/src/y.rs:123`, create/modify/test.
**Interfaces:** what this consumes from earlier tasks, and the exact names and
types later tasks rely on. The executor sees only their own task.

- [ ] Write the failing test — with the actual test code
- [ ] Run it, watch it fail — the exact command, the expected failure
- [ ] Implement the minimum to pass — with the actual code
- [ ] Run the gate — the exact command
- [ ] Commit — `git add <explicit paths>` (never `-A`: the tree is shared)
```

**No placeholders.** These are plan failures, not shortcuts: "TBD", "add error
handling", "write tests for the above", "similar to Task N", or any reference to
a function or type that no task defines. If a step changes code, the code is in
the step. A plan that a fresh quark cannot execute without asking you a question
has failed, and you will not be there to answer it.

## Before you finish

Read the plan once against the request with fresh eyes: is every requirement
covered by a task, is every placeholder gone, and does a name used in Task 7 match
the name defined in Task 3? Fix what you find inline.

## Hand it off

Commit the plan, then hand it to a peer **by name** — the eligible quarks are
listed below. Do not execute it yourself, and do not hand it back to the human as
"a plan exists": say who is picking it up and for what.
