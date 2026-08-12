# Hadron Command Suite Design Specification

Date: 2026-08-12  
Status: Approved  
Target Version: Hadron 0.2.0

## Overview

This specification details eight command families (`retry`, `doctor`, `prune`, `compact-nucleus`, `stop`/`kill`/`cancel`, `gate-cancel`, `revert`, `unabandon`) to be added to Hadron's GUI and daemon operational command surface. All commands are integrated into `COMMANDS` in [`crates/hadron-chamber/src/text.rs`](file:///home/Jake/dev/hadron/crates/hadron-chamber/src/text.rs) as the Single Source of Truth (SSOT).

---

## 1. `/retry`
- **Command Entry**:
  ```rust
  Command { name: "retry", detail: "Re-dispatch the last failed message or turn for a seat or global", arity: Arity::Line, arg: ArgSource::Quark, listed: true }
  ```
- **Behavior**:
  - Scans `view.messages` in `ChamberView` for the most recent message addressed to `@quark` (or any seat if unspecified) that failed or received an excitation error.
  - Re-posts the message body cleanly as a new `Actor::Human` message.

---

## 2. `/doctor`
- **Command Entry**:
  ```rust
  Command { name: "doctor", detail: "Run automated system diagnostics on daemon, locks, nucleus, fonts, and git worktrees", arity: Arity::None, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - Performs non-blocking, read-only diagnostic checks:
    1. **Daemon Lock**: Verifies PID in `gluon.lock` against `/proc/<pid>/comm`.
    2. **Nucleus Index**: Measures `.hadron/nucleus/index.md` byte size against the 32 KB limit and flags orphan notes in `notes/*.md`.
    3. **Font System**: Tests `font_family_with_a_real_bold` resolution.
    4. **ACP Executable Resolution**: Validates path resolution for installed/preset ACP seat executables via `ResolvedAcpTarget`.
    5. **Worktrees & Target Directory**: Scans worktrees for stale/dirty states and checks `~/.hadron/update/build` or tmpfs residue.
  - Outputs a structured `[ok|warn|fail]` diagnostic summary as `Actor::Gluon` with `to: None`.

---

## 3. `/prune`
- **Command Entry**:
  ```rust
  Command { name: "prune", detail: "Preview or clean up merged/stale quark worktrees and branches safely", arity: Arity::Line, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - `/prune` / `/prune preview`: Lists `quark/*` worktrees and branches that are fully merged into `main` or stale.
  - `/prune confirm`: Enforces Standard Model Rule 5 / VCS Invariants by writing `archive/<slug>` tags first, attempting `git branch -d` (never `-D` without archive tags), and removing merged worktree directories.

---

## 4. `/compact-nucleus`
- **Command Entry**:
  ```rust
  Command { name: "compact-nucleus", detail: "Audit and compact nucleus index against target budget limit (e.g. /compact-nucleus 24kb)", arity: Arity::Line, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - Parses an optional size target (e.g. `24kb`, defaulting to `32kb`).
  - Audits `.hadron/nucleus/index.md`: verifies 1-line pointer syntax `- [<slug>](notes/<slug>.md) — <hook>`, removes broken pointer lines pointing to missing files, and flags outdated post-mortems for review.

---

## 5. `/stop`, `/kill`, `/cancel`
- **Command Entries**:
  ```rust
  Command { name: "stop", detail: "Gracefully stop a quark's in-flight turn (waits up to 10s before SIGKILL)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
  Command { name: "kill", detail: "Immediately force-kill a quark's subprocess group (SIGKILL)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
  Command { name: "cancel", detail: "Cancel pending unhandled dispatch for a seat", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
  ```
- **Behavior**:
  - `/cancel @quark`: Retracts unhandled pending dispatch events for the seat in the gluon daemon queue.
  - `/stop @quark`: Sends graceful termination signal to seat, monitoring `live/<quark>.json` for up to 10s before resorting to SIGKILL.
  - `/kill @quark`: Sends immediate `SIGKILL` to the seat's subprocess group.

---

## 6. `/gate-cancel`
- **Command Entry**:
  ```rust
  Command { name: "gate-cancel", detail: "Force cancel a hung merge-gate run by killing its process group", arity: Arity::None, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - Obtains the active merge gate process group PID from the gate runner context.
  - Executes `kill(-pgid, SIGKILL)` on the process group and marks gate state as cancelled without restarting the daemon.

---

## 7. `/revert`
- **Command Entry**:
  ```rust
  Command { name: "revert", detail: "Revert the last landed commit on main via git revert without rewriting history", arity: Arity::Line, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - Identifies the latest landed commit SHA on `main` from gate ledger history.
  - Creates a new `git revert -n <sha>` commit preserving full history and passes it to the merge gate for validation and landing.

---

## 8. `/unabandon`
- **Command Entry**:
  ```rust
  Command { name: "unabandon", detail: "Restore an archived branch from its archive tag (e.g. /unabandon quark-slug)", arity: Arity::Line, arg: ArgSource::None, listed: true }
  ```
- **Behavior**:
  - Scans `git tag -l "archive/*"`.
  - Recreates the deleted branch at the exact SHA of `archive/<slug>` (`git branch <branch-name> archive/<slug>`) and checks out or restores the worktree.

---

## Safety & Invariants
- **No Untyped Commands**: All 8 command entries are registered in `COMMANDS` table in `crates/hadron-chamber/src/text.rs`.
- **Branch Safety**: `/prune` uses `git branch -d` after creating `archive/<slug>` tags.
- **Daemon Isolation**: `/gate-cancel` and `/kill` target specific process groups without corrupting `gluon.lock` or interrupting unaffected seats.
