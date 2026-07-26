# Contributing to Hadron

Thanks for being here. Hadron is a multi-agent operating system: a daemon that
seats AI agents ("quarks") around one shared workspace, a protocol crate they all
speak, and a native GPUI app to watch it happen. Contributions are welcome from
humans and, yes, from agents.

## The gate

There is **one** gate, and you must run all of it:

```bash
cargo test --workspace   # ~970 tests, the GPUI app included
```

`gui` is in the chamber package's `default` features, so the workspace gate **does**
compile and run everything behind `#[cfg(feature = "gui")]`. (An earlier version of
this file claimed the opposite and told you to run a second `-p` gate; that was wrong,
and the package it named no longer exists — `hadron-chamber` is the *directory*, and
the package is now called `hadron`.) Run the filtered form only to iterate quickly:

```bash
cargo test -p hadron   # the chamber package, all three bin targets
```

A corollary that matters when you write tests: **pure logic belongs in a module
that is not feature-gated** (`text.rs`, `sys.rs`, `model.rs`), so its tests run in
the gate we actually judge changes by. A crash guard the gate cannot see is a
crash guard that will be broken again without anyone noticing.

Record the numbers *before* you start. That is your baseline; you own only the
delta. If something already failed before you touched it, say so with the numbers
and leave it alone.

## The Standard Model

Every quark in this project works to a short set of invariants, and they apply to
human contributors too. They live in
[`crates/hadron-gluon/invariants/`](crates/hadron-gluon/invariants/) and are
compiled into the daemon so they cannot drift from what the agents are told.

The short version:

1. **Prove it runs, don't prove it compiles.** A patch that builds is not a
   feature that works. Find the caller. If nothing calls your new code, say
   "implemented, unwired" and name what would have to call it — that is a
   perfectly good result, and it is honest.
2. **Reuse before you create.** Search by name *and* by concept before adding a
   type, a helper or a constant.
3. **One definition, one place.** A value has exactly one home. A second copy is
   drift waiting to happen.
4. **Don't delete a layer because it looks redundant.** Two guards doing the same
   job are usually defence in depth.
5. **Know your baseline.**
6. **Evidence, not adjectives.** "Tests pass" is worth nothing without the output.
7. **Name the risk** when you touch permissions, process execution, file access,
   the network, or untrusted input.
8. **Make invalid states unrepresentable.** Prefer a compiler error to a runtime
   check, a runtime check to a comment.

## Pull requests

- **One change, one PR.** A PR that fixes a bug *and* renames things is two PRs.
- **Say what you did not verify.** It is the most valuable line in any report and
  the one everyone skips.
- Separate what you **propose** from what you have **done**.
- Commit messages explain *why*, not *what* — the diff already says what.

## A note on the shared checkout

Agents in this project work in the same working tree as the human. If you are an
agent reading this: **stage by explicit path**. Never `git add -A`, `git add .`,
or `git commit -a` — they sweep up another quark's in-flight edits and the
human's, and commit them under your name. Commit each piece as it goes green; a
turn that dies loses everything not yet committed.

## Licence

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), the same licence that covers the project.
