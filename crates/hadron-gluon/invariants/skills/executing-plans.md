# Skill: Executing plans

You are executing a written plan this turn. Someone else wrote it, and they are
not here — the plan is the whole of what they can tell you.

## 1. Read it critically, before you touch anything

Open the plan file. Then check it against the repo, because a plan is a claim
about a codebase that may have moved since it was written:

- Do the files it names still exist, at the paths it gives?
- Do the functions and types it references actually exist?
- Is its recorded **baseline** still the baseline? Run the gate yourself and
  compare. A plan written against a green tree does not authorise you to inherit
  someone else's red one — and a failure you did not cause is not yours to fix.

If the plan is wrong about the world, **say so and stop**. A confidently executed
fiction is the most expensive thing you can produce. Report what contradicts it
and hand it back to the author.

## 2. Execute it exactly

Task by task, step by step, in order. The steps are bite-sized on purpose.

- Run every verification the plan specifies. Do not skip one because the code
  "obviously" works — that is the assumption the verification exists to kill.
- **Commit as each task goes green**, staging by explicit path (`git add <path>`).
  Never `git add -A`: the tree is shared with the human and every other quark, and
  a sweep commits their in-flight work under your name. A turn that dies with
  uncommitted work loses it entirely.
- Tick the `- [ ]` boxes in the plan file as you go, and commit that too. That
  checklist is the only thing that tells the next quark where you stopped.

## 3. Stop when blocked — do not improvise

Stop and report if a dependency is missing, a step's instruction is ambiguous, a
verification fails repeatedly, or the plan contradicts what you find in the code.
Guessing produces work that looks finished and is not. Say plainly which task you
reached, what stopped you, and what you left committed.

## 4. Do not verify your own execution

When the tasks are done, the work needs a reviewer who is not you. Hand it to a
peer **by name** from the eligible list below, and tell them what to check: the
plan, your commits, and the gate.

Report honestly, in three buckets — **verified** (you ran it and watched it),
**landed but unclicked** (it compiles and is committed; nobody has exercised it),
and **not done**. Only the first is finished.
