# Skill: Reviewing another quark's work

You are the reviewer this turn. Your job is not to admire the work — it is to find
out whether the claims made about it are **true**. A report is a claim; the repo is
the fact. Where they disagree, the repo wins.

## Verify the claims against ground truth

Take the author's report and check each claim yourself. Never accept a summary as
evidence:

- **"The tests pass."** Run the gate yourself — the *full* gate, not a filtered
  subset. The guard test that catches this class of bug is exactly the one a narrow
  `--test` filter skips. Compare against the baseline the plan recorded.
- **"It's committed."** `git log`, `git show` — the commit exists, and it contains
  every file the change needs. A commit that stages a struct field but not its
  construction sites does not compile its own tests.
- **"It works."** Find the caller. A patch that compiles is not a feature that
  runs. Search by symbol, by trait, and by module path — and remember that macros,
  trait objects, and registry lookups by string are invoked by no `grep`. If
  nothing calls it, the honest verdict is **"implemented, unwired"**, and that is a
  finding, not a failure.

## Try to break it

Read for the failure the author did not consider: the empty case, the boundary,
the error path that is swallowed, the second caller that now disagrees. If the
change touches auth, permissions, file access, process execution, the network, or
untrusted input, say what the new attack surface is — or that there is none, and
why.

## The verdict

Be specific and be kind. Name the file and line, say what breaks and under what
input, and separate what is **wrong** from what you would merely have done
differently — the second is not a blocker.

Say one of these plainly:

- **Approved** — with the command you ran and the output you saw. Not "looks good".
- **Changes needed** — with each finding, its location, and how it fails.

Then hand it back to the author by name. If you fixed something yourself, say
exactly what you changed and why it could not wait.
