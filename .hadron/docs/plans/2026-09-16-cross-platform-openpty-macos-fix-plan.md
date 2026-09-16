# Cross-Platform `openpty` macOS Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve GitHub Issue #1 by fixing the type mutability mismatch in `crates/hadron-forge/src/pty.rs` during `libc::openpty` invocation on macOS/Darwin.

**Architecture:** Update `ws` declaration to `mut ws` and pass `&mut ws` to `libc::openpty`. In Rust FFI, `&mut T` coerces to `*mut T` (matching macOS/BSD libc signature) and can coerce to `*const T` (matching Linux glibc signature), achieving seamless cross-platform compilation without `#[cfg]` branches.

**Tech Stack:** Rust, `libc` crate, POSIX/Darwin PTY APIs.

## Global Constraints

- Preserve all existing invariants in `crates/hadron-forge/src/pty.rs` (process group isolation, bounded ring buffer, argument jailing).
- No new attack surface or dependency changes.
- Ensure all tests in `hadron-forge` pass (`cargo test -p hadron-forge`).
- Ensure `x86_64-apple-darwin` compiles cleanly.

---

### Task 1: Fix `libc::openpty` `winsize` Mutability Mismatch (commit 7898ce1d)

**Files:**
- Modify: `crates/hadron-forge/src/pty.rs:125-145`

**Interfaces:**
- Consumes: `libc::winsize`, `libc::openpty`
- Produces: Cross-platform Unix PTY supervisor spawn method

- [x] **Step 1: Verify failing condition on Darwin target**
  Run `rustc --target x86_64-apple-darwin` snippet to confirm `E0308` type mismatch on `&ws`.

- [x] **Step 2: Update `pty.rs` to declare `mut ws` and pass `&mut ws`**
  In `crates/hadron-forge/src/pty.rs`, change `let ws = libc::winsize { ... };` to `let mut ws = ...` and pass `&mut ws` to `libc::openpty`.

- [x] **Step 3: Verify resolution on Darwin target**
  Run `rustc --target x86_64-apple-darwin` snippet with `&mut ws` to verify clean compilation.

- [x] **Step 4: Run full `hadron-forge` test suite on Linux**
  Run `cargo test -p hadron-forge` to confirm all 147 tests pass.

- [x] **Step 5: Commit implementation**
  Commit with message `fix(forge): make winsize mutable for cross-platform openpty on macOS`.

---

### Task 2: Distill Post-Mortem into Nucleus Memory

**Files:**
- Create: `.hadron/nucleus/notes/cross-platform-openpty-winsize-mutability.md`
- Modify: `.hadron/nucleus/index.md`

**Interfaces:**
- Consumes: Root cause analysis of `libc::openpty` signature differences
- Produces: Reusable knowledge note and routing pointer

- [ ] **Step 1: Write note `.hadron/nucleus/notes/cross-platform-openpty-winsize-mutability.md`**
  Record why `&mut ws` satisfies both `*mut winsize` (BSD/Apple) and `*const winsize` (Linux).

- [ ] **Step 2: Update `.hadron/nucleus/index.md`**
  Append routing pointer line capped at ~100 characters.

- [ ] **Step 3: Commit nucleus updates**
  Commit with message `docs(nucleus): record cross-platform openpty winsize mutability lesson`.
