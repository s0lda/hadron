# Git Slash Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `/git-status`, `/git-log`, `/push`, and `/pr` slash commands to the Hadron Chamber chat interface.

**Architecture:** Register commands in `hadron_chamber::text::COMMANDS` SSOT table in `crates/hadron-chamber/src/text.rs`, implement non-blocking action handlers in `crates/hadron-chamber/src/app/actions.rs` using `snapshot::git_with_env` (bounded by `GIT_DEADLINE`), and update the `every_listed_command_is_handled` test in `crates/hadron-chamber/src/app/input.rs`.

**Tech Stack:** Rust, `hadron-chamber`, `hadron-gluon` snapshot git helper, GPUI events.

## Global Constraints

- **SSOT**: All commands must be registered in `COMMANDS` table in `crates/hadron-chamber/src/text.rs`.
- **Compiler Guard**: Handled match arms in `actions.rs` MUST match the `HANDLED` test list in `input.rs`.
- **Bounded Execution**: Git commands must use `snapshot::git_with_env` bounded by `GIT_DEADLINE` (120s max with stdin closed).
- **Non-blocking UI**: Commands post output as `Actor::Gluon` field events without blocking GPUI rendering.

---

- [x] **Task 1 (commit fedc7c22): Register Git Slash Commands in `COMMANDS` Table**
- [x] **Task 2 (commit 5436c172): Implement Command Handlers in `actions.rs`**
- [x] **Task 3 (commit 8c3375d5): Update Handled Commands Test and Verify Gate**
