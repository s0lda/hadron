# Native Windows Support Design Spec

**Date:** 2026-08-07  
**Status:** Approved  
**Target:** Native `x86_64-pc-windows-msvc` build support for `hadron` GUI, `hadron-gluon` daemon, and `hadron-forge-mcp` via `cargo install --git` without requiring WSL2.

---

## 1. Goal & Requirements
Provide full native cross-platform support for Windows (MSVC target) across all Hadron components:
1. **Zero-WSL Requirement**: `hadron`, `hadron-gluon`, and `hadron-forge-mcp` build and run natively on Windows.
2. **Process Management**: Replace Linux-specific process handling (`/proc`, `process_group(0)`, `libc::kill`) with cross-platform primitives.
3. **Single Source of Truth (SSOT)**: Encapsulate all OS differences inside `hadron_lattice::sys` rather than scattering `#[cfg(windows)]` throughout feature crates.

---

## 2. Platform Abstraction Layer (`hadron_lattice::sys`)

### 2.1 Process Group & Signal Management (`sys::process`)
- **Unix (`#[cfg(unix)]`)**: Preserves existing `process_group(0)` and `libc::kill(-pid, sig)` behavior.
- **Windows (`#[cfg(windows)]`)**:
  - Uses Windows Job Objects (`CreateJobObjectW`, `AssignProcessToJobObject`, `SetInformationJobObject`) via `windows-sys`.
  - Configures `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so child processes (e.g. `cargo test` runs, ACP agents) terminate automatically when parent handles close or timeout expires.

### 2.2 Process Inspection (`sys::inspect`)
- **Unix (`#[cfg(unix)]`)**: Reads `/proc/<pid>/cmdline` or `/proc/<pid>/comm` to verify daemon lock ownership (`gluon.lock`).
- **Windows (`#[cfg(windows)]`)**:
  - Uses `windows-sys` (`OpenProcess`, `QueryFullProcessImageNameW`) to verify whether PID belongs to a live `hadron-gluon.exe` process.

### 2.3 Path & Shell Execution (`sys::paths` & `sys::shell`)
- **Path Anchoring (`AcpTarget::resolved()`)**: Supports Windows drive letters (`C:\`), backslashes (`\`), and UNC paths alongside POSIX paths.
- **Command Invocation**: Abstracts default shell execution (`cmd.exe /C` or `powershell.exe -Command` on Windows vs `sh -c` on Unix).

### 2.4 File Locking (`gluon.lock`)
- Ensures `fs2` / file locking semantics operate reliably across daemon restarts on Windows mandatory file lock semantics.

---

## 3. Component Migration Scope

1. **`crates/hadron-lattice`**: Add `sys` module containing `process`, `inspect`, `paths`, and `shell`.
2. **`crates/hadron-gluon`**: Refactor `runner.rs`, `cli.rs`, `merge.rs`, `main.rs`, and lock logic to use `hadron_lattice::sys`.
3. **`crates/hadron-forge`**: Update PTY / command execution helpers to use `sys::shell` and Windows ConPTY fallback.
4. **`crates/hadron-chamber`**: Verify GPUI compilation dependencies and font fallback on Windows graphics backends.

---

## 4. Security Considerations
- Process job objects guarantee child process termination on Windows, eliminating orphaned process leaks during timeouts.
- Lock file PID verification prevents process spoofing during daemon startup.
