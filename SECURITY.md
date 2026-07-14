# Security Policy

## Reporting a vulnerability

Please report security issues **privately** to **81304425+s0lda@users.noreply.github.com**, or via
GitHub's private vulnerability reporting on this repository. Do not open a public
issue for a vulnerability.

Include what you did, what happened, and what you expected. If you have a proof of
concept, say so — but you do not need one to file a report.

We aim to acknowledge a report within a few days and to keep you updated while we
work on it. If you would like credit in the fix, tell us the name to use.

## What Hadron actually does — read this before you deploy it

Hadron is not a sandbox. Being clear about that is more useful than a promise we
cannot keep.

**Hadron runs AI agents that execute code on your machine, in your repository, as
you.** The daemon (`hadron-gluon`) spawns agent processes — a CLI, or a resident
agent over the Agent Client Protocol — and those agents run shell commands, read
and write files, and make network calls with **your user's full authority**. There
is no container, no VM and no syscall filter between an agent and your home
directory.

Two consequences that people are consistently surprised by:

- **Agents share your working tree.** Worktree isolation exists in the engine but
  is not switched on today: a quark's edits land in the same checkout you are
  typing in, and a quark that commits carelessly can commit your uncommitted work.
- **Prompt injection is a real execution path.** An agent reads files, diffs and
  web pages. Content in any of those can be crafted to instruct the agent. A model
  that can run commands and can be talked into running the wrong one *is* the
  threat model. Treat any repository you point Hadron at as trusted input.

### The controls that do exist

- **Permission modes** (`ask` / `write` / `auto` / `bypass`) set the posture a
  quark boots with, from "read and propose only" up to unrestricted. This is a
  real gate on what a quark's own tooling is permitted to do, and it is enforced
  where the agent is spawned. It is a policy control, not a sandbox boundary.
- **The Chamber's terminal carries the human's authority by design.** It is your
  keyboard. It is deliberately *not* gated by permission mode, and quarks have no
  path to type into it.
- **The field (`field.jsonl`) is an append-only log** of everything that happened —
  every message, every turn, every token. If you want to know what an agent did,
  it is written down.

### Running it more safely

- Run Hadron in a VM or container if the repository or the task is not fully
  trusted.
- Keep quarks in `ask` or `write` mode unless you are watching. `bypass` means
  what it says.
- Do not keep secrets in the working tree. Agents read the tree.
- Review what a quark commits. The Changes pane exists for this.

## Supported versions

Hadron is pre-1.0 and moves fast. Security fixes land on `main`; there are no
maintained release branches yet. If you are running it, run `main`.
