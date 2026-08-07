# Native Windows Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement native cross-platform support (`x86_64-pc-windows-msvc`) for `hadron`, `hadron-gluon`, and `hadron-forge-mcp` without requiring WSL2, encapsulating platform differences inside `hadron_lattice::sys`.

**Architecture:** Create `hadron_lattice::sys` containing submodules for process management (`sys::process`), process inspection (`sys::inspect`), paths (`sys::paths`), and shell execution (`sys::shell`), then refactor `hadron-gluon`, `hadron-forge`, and `hadron-chamber` call sites to use these abstractions.

**Tech Stack:** Rust 2021 (`hadron-lattice`, `hadron-gluon`, `hadron-forge`, `hadron-chamber`), `windows-sys` (on Windows target).

---

## Global Constraints

- Enforce Single Source of Truth (SSOT): All OS-specific process/signal/proc abstractions belong inside `hadron_lattice::sys`.
- Preserve 100% existing Linux/POSIX behavior on `#[cfg(unix)]`.
- Maintain full compatibility with cargo unit testing and `cargo check --workspace`.

---

### Task 1: Create `hadron_lattice::sys` Platform Abstraction Module

**Files:**
- Create: `crates/hadron-lattice/src/sys/mod.rs`
- Create: `crates/hadron-lattice/src/sys/process.rs`
- Create: `crates/hadron-lattice/src/sys/inspect.rs`
- Create: `crates/hadron-lattice/src/sys/paths.rs`
- Create: `crates/hadron-lattice/src/sys/shell.rs`
- Modify: `crates/hadron-lattice/Cargo.toml`
- Modify: `crates/hadron-lattice/src/lib.rs`

**Interfaces:**
- Produces: `pub mod sys;` with `sys::process::kill_process_group`, `sys::process::set_process_group`, `sys::inspect::is_process_alive`, `sys::paths::normalize_path`, `sys::shell::default_shell`

- [x] **Step 1: Update `crates/hadron-lattice/Cargo.toml` to add `windows-sys` under target cfg**


Add `windows-sys` for Windows targets with required features (`Win32_System_JobObjects`, `Win32_System_Threading`, `Win32_Foundation`, `Win32_System_ProcessStatus`):

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_System_JobObjects",
    "Win32_System_Threading",
    "Win32_System_ProcessStatus",
] }
```

- [x] **Step 2: Implement `sys::process` in `crates/hadron-lattice/src/sys/process.rs`**

Provide cross-platform process group creation and teardown:
- `kill_process_group(pid: u32)`:
  - On Unix (`#[cfg(unix)]`): calls `libc::kill(-(pid as i32), libc::SIGKILL)`.
  - On Windows (`#[cfg(windows)]`): uses Job Objects / `windows-sys` process termination.
- `set_process_group(cmd: &mut std::process::Command)` / `tokio::process::Command`:
  - On Unix: calls `.process_group(0)`.
  - On Windows: configures Job Object handle association on spawn.

- [x] **Step 3: Implement `sys::inspect` in `crates/hadron-lattice/src/sys/inspect.rs`**

Provide cross-platform PID and process name verification (`is_process_alive(pid: u32, expected_name: &str) -> bool`):
- On Unix: checks `/proc/<pid>/cmdline` or `/proc/<pid>/comm`.
- On Windows: uses `OpenProcess` and `QueryFullProcessImageNameW` via `windows-sys`.

- [x] **Step 4: Implement `sys::paths` and `sys::shell` in `crates/hadron-lattice/src/sys/`**

- `sys::paths`: Handles Windows drive letter, UNC path, and backslash normalization.
- `sys::shell`: Exports `default_shell() -> (&'static str, &'static str)` (`("sh", "-c")` on Unix vs `("cmd.exe", "/C")` / `("powershell.exe", "-Command")` on Windows).

- [x] **Step 5: Export `pub mod sys;` in `crates/hadron-lattice/src/lib.rs` and verify build**

Run: `cargo check -p hadron-lattice`  
Expected: Clean compilation.

---

### Task 2: Refactor `hadron-gluon` Process Management and Inspection

**Files:**
- Modify: `crates/hadron-gluon/src/proc.rs`
- Modify: `crates/hadron-gluon/src/merge.rs`
- Modify: `crates/hadron-gluon/src/adapter/runner.rs`
- Modify: `crates/hadron-gluon/src/statusline.rs`
- Modify: `crates/hadron-gluon/src/main.rs`

**Interfaces:**
- Consumes: `hadron_lattice::sys::process`, `hadron_lattice::sys::inspect`, `hadron_lattice::sys::shell`

- [x] **Step 1: Replace direct `libc::kill` and `process_group(0)` in `hadron-gluon`**

Refactor `runner.rs`, `proc.rs`, `merge.rs`, and `statusline.rs` to use `hadron_lattice::sys::process` and `hadron_lattice::sys::shell`.

- [x] **Step 2: Replace `/proc` inspections in `hadron-gluon/src/main.rs`**

Update PID verification logic (`pid_names_a_live_gluon`) in `hadron-gluon/src/main.rs` to call `hadron_lattice::sys::inspect::is_process_alive`.

- [x] **Step 3: Check compilation of `hadron-gluon`**

Run: `cargo check -p hadron-gluon`  
Expected: Clean compilation.

---

### Task 3: Refactor Command & PTY Execution in `hadron-forge` & `hadron-chamber`

**Files:**
- Modify: `crates/hadron-forge/src/exec.rs`
- Modify: `crates/hadron-chamber/src/main.rs`
- Modify: `crates/hadron-chamber/src/pty.rs`
- Modify: `crates/hadron-chamber/src/app/actions.rs`

**Interfaces:**
- Consumes: `hadron_lattice::sys`

- [x] **Step 1: Refactor `hadron-forge/src/exec.rs` to use `sys::process`**

Replace internal `kill_process_group` in `hadron-forge/src/exec.rs` with re-export or call to `hadron_lattice::sys::process::kill_process_group`.

- [x] **Step 2: Refactor `/proc` references in `hadron-chamber`**

Update `main.rs`, `pty.rs`, and `actions.rs` in `hadron-chamber` to use `hadron_lattice::sys::inspect` or `cfg(target_os = "linux")` wrappers for Linux-specific PTY thread counting.

- [x] **Step 3: Run full workspace test suite**

Run: `cargo test --workspace`  
Expected: All tests pass cleanly.

---

### Task 4: Commit and Gate Verification

**Files:**
- Modify: `docs/plans/2026-08-07-windows-support-implementation-plan.md`

- [x] **Step 1: Run full workspace check and test gate**

Run: `cargo check --workspace && cargo test --workspace`  
Expected: 0 errors, 0 test failures.

- [x] **Step 2: Commit implementation plan & code changes**


```bash
git add docs/plans/2026-08-07-windows-support-implementation-plan.md crates/
git commit -m "feat(sys): implement hadron_lattice::sys cross-platform abstraction layer for Windows support"
```
