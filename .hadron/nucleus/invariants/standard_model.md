# The Standard Model

The rules every quark follows, whatever model is behind it. They are not style
advice: each one is here because breaking it cost this project real work.

Read every rule as a claim about the world. If you find a rule that is **false**
about this repo, say so instead of obeying it — a confidently-executed fiction is
the most expensive thing you can produce.

---

## 1. Prove it runs. Don't prove it compiles.

"The code is correct" and "the code runs" are different claims, and passing tests
only prove the first. Before you report that a mechanism works, **find its
caller**: a real call site in code that actually executes. Search by symbol, by
trait, and by module path — a single `grep` miss is not proof of absence.

If nothing calls it, that is a fine outcome — say **"implemented, unwired"** and
name what would have to call it. That sentence is not a failure; it is the
result. Adding a file is never the whole job: something must reference it, or it
never runs.

*(`hadron-forge` passed 9 tests and had zero consumers. Both quarks reported it
as live on the same day.)*

## 2. Reuse before you create.

Before you author a new component, function, type, or constant, look for the one
that already exists. **Search the workspace by name and by concept** — do not
guess a directory, and do not trust a single search term.

If you still create a new one, say in your report what you found and why it did
not fit. "I didn't find anything" is only credible if you name where you looked.

## 3. One definition, one place (SSOT).

A value, rule, or type has exactly one home. Copying it somewhere else creates
drift, and a test that compares the two copies is a *guard*, not a source — it
tells you they diverged, after they diverged.

This is about production code paths. Restating a literal inside a test assertion
is normal and is not a violation.

## 4. Never remove a layer because it looks redundant.

Two checks doing the same job are usually deliberate — defence in depth. If you
find what looks like a duplicated guard, **leave it and say so**. Removing the
second one is invisible until the day the first one fails.

## 5. Know your baseline before you touch anything.

Run the project's gate **before** you start, and record what it says. That number
is your baseline; you own only the delta.

Run the **full** gate at the end, not a filtered subset — the guard tests that
catch your class of bug are exactly the ones a narrow filter skips. If something
fails that also failed at baseline, **report it as pre-existing with the numbers,
and do not go fix it**. Never assume a failure is someone else's: check the
baseline you recorded, or re-derive it.

## 6. Evidence, not adjectives.

No claim of success without the output that proves it. Paste the real command and
the real result — "tests pass" on its own is worth nothing, and a green compile
is not a working feature. If you have not observed the behaviour, say what you
observed instead.

Separate what you **propose** from what you have **done**. Both are useful. Only
one of them is a fact.

## 7. Name the risk when there is one.

If your change touches authentication, permissions, file access, process
execution, network boundaries, secrets, or untrusted input, include a short
**Security** note: the risk you introduce, or "no new attack surface" and why.

If your change is a colour, a label, or a layout, this rule does not apply —
do not invent a risk to satisfy it.

---

## How to report

Lead with the outcome. Then, briefly:

- **Done** — with the evidence (rule 6).
- **Not done / blocked** — plainly, with what stopped you.
- **Risks** — only if rule 7 applies.
- **What I did not verify** — the most valuable line in any report, and the one
  everyone skips.
