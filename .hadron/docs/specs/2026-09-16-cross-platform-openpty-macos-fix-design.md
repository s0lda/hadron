# Cross-Platform `openpty` Winsize Mutability Fix Design

- **Topic:** Fix `crates/hadron-forge/src/pty.rs` compilation failure on macOS (`openpty` winsize mutability mismatch)
- **Date:** 2026-09-16
- **Status:** Approved (Bypass Mode)
- **Related Issue:** GitHub Issue #1 (`bug: crates/hadron-forge/src/pty.rs failure openpty at install`)

## 1. Problem Statement & Root Cause

When installing Hadron via `cargo install --locked --git https://github.com/s0lda/hadron.git hadron` on macOS, compilation of `hadron-forge` fails with:
```
error[E0308]: mismatched types
   --> crates/hadron-forge/src/pty.rs:140:21
    |
135 |                 libc::openpty(
    |                 ------------- arguments to this function are incorrect
...
140 |                     &ws,
    |                     ^^^ types differ in mutability
    |
    = note: expected raw pointer `*mut winsize`
                 found reference `&winsize`
```

### Root Cause
In POSIX/C library headers:
- On Linux (glibc/musl), `openpty` is declared with `const struct winsize *winp`. Consequently, in `libc` for Linux, `winp` has type `*const libc::winsize`.
- On macOS (Apple Darwin) and BSD variants (FreeBSD, OpenBSD, NetBSD), `openpty` is declared without `const`: `struct winsize *winp`. In `libc` for Apple/BSD targets, `winp` has type `*mut libc::winsize`.

In `crates/hadron-forge/src/pty.rs`:
```rust
let ws = libc::winsize { ... };
libc::openpty(..., &ws);
```
`&ws` has type `&libc::winsize`, which coerces to `*const libc::winsize` (valid on Linux), but cannot coerce to `*mut libc::winsize` (failing on macOS/BSD).

## 2. Evaluated Approaches

### Approach 1 (Recommended): Declare `mut ws` and pass `&mut ws`
```rust
let mut ws = libc::winsize {
    ws_row: terminal_rows,
    ws_col: terminal_cols,
    ws_xpixel: 0,
    ws_ypixel: 0,
};
libc::openpty(
    &mut master_fd,
    &mut slave_fd,
    std::ptr::null_mut(),
    std::ptr::null_mut(),
    &mut ws,
);
```
- **Rationale:**
  - In Rust FFI, `&mut T` coerces to `*mut T`.
  - On Darwin/BSD, `*mut libc::winsize` matches the parameter type directly.
  - On Linux/Android, `*mut libc::winsize` coerces cleanly to `*const libc::winsize`.
  - It works across all Unix platforms without `#[cfg]` divergence.
  - Avoids casting gymnastics or UB.

### Approach 2: Platform-specific `#[cfg(target_vendor = "apple")]` blocks
- Duplicate `libc::openpty` calls depending on OS.
- Rejected: Violates Standard Model Rule 3 (SSOT) and Rule 10 (Simplicity first). Adds maintenance overhead and misses other BSD targets.

### Approach 3: Raw pointer casting `&ws as *const _ as *mut _`
- Cast an immutable reference to `*mut`.
- Rejected: Less idiomatic and risks undefined behavior if an OS implementation mutates winsize.

## 3. Architecture & Impact Analysis

- **Affected File:** `crates/hadron-forge/src/pty.rs` (lines 127, 140).
- **Behavioral Impact:** Zero runtime behavioral change on Linux. Resolves build blocker on macOS/Darwin and BSD platforms.
- **Security Impact (Rule 7):** No new attack surface. Memory bounds and process group isolation invariants remain untouched.

## 4. Verification Strategy

1. Verify reproduction on `x86_64-apple-darwin` target before fix (confirmed via `rustc --target x86_64-apple-darwin`).
2. Verify resolution on `x86_64-apple-darwin` with `&mut ws`.
3. Verify all existing unit and integration tests pass on Linux: `cargo test -p hadron-forge`.
4. Distill learning into `.hadron/nucleus/notes/cross-platform-openpty-winsize-mutability.md` and update index.
