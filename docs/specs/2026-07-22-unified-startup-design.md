# Unified Application Startup and Gluon Lifecycle Management Design

## 1. Overview
Currently, starting Hadron requires running two commands in separate terminal sessions: one for `hadron-gluon` (the headless daemon) and one for `hadron-chamber` (the GPUI desktop app). This design unifies application launch so that running `hadron-chamber` (or clicking a compiled application icon) automatically detects and starts `hadron-gluon` if it is not already running. Additionally, it introduces a user-configurable setting to determine whether `hadron-gluon` should be terminated when `hadron-chamber` exits.

## 2. Goals & Success Criteria
- **Single-Command Startup**: Running `hadron-chamber` automatically starts `hadron-gluon` if no daemon process is active on the target `field.jsonl`.
- **Configurable Exit Behavior**: Add a preference in `ChamberPrefs` and Settings UI ("Close Gluon on Chamber Exit", default `false`).
- **CLI Flag `--no-daemon`**: Support bypassing auto-spawn for dev / multi-chamber / testing scenarios.
- **Desktop Application Launcher**: Provide `assets/hadron.desktop` and launcher integration pointing to the compiled executable.
- **Zero Regression on Headless Gluon**: Keep `hadron-gluon` fully decoupled so it can continue running as a standalone headless daemon when desired.

## 3. Architecture & Components

### 3.1 Startup & Lifecycle Detection (`hadron-chamber/src/main.rs`)
1. **Daemon Detection**: Utilize the existing `gluon.lock` file lock check via `libc::flock`.
2. **Auto-Spawn**:
   - If `gluon.lock` is unlocked (daemon not running) and `--no-daemon` is NOT passed in CLI arguments:
   - Resolve `hadron-gluon` binary path relative to `std::env::current_exe()` (preventing `$PATH` hijacking security risks).
   - Spawn `hadron-gluon` as a background process, capturing its PID or child process handle.
3. **Shutdown Handling**:
   - On `hadron-chamber` window close/exit, check `ChamberPrefs::close_gluon_on_exit`.
   - If `close_gluon_on_exit` is `true` AND `hadron-chamber` auto-spawned `hadron-gluon` (or holds the spawned PID), send `SIGTERM`/kill to the daemon process.

### 3.2 Preferences & Settings UI (`config.rs` & `app/settings.rs`)
1. **Config Struct**: Add `close_gluon_on_exit: bool` (default `false`) to `ChamberPrefs` in `crates/hadron-chamber/src/config.rs`.
2. **Settings UI**:
   - Render a toggle switch in the Settings panel: `"Close Gluon on Chamber Exit"`.
   - Mutating the switch updates `ChamberPrefs` and persists it via `config::save()`.

### 3.3 Desktop Launcher (`assets/hadron.desktop`)
Add a standard FreeDesktop `.desktop` entry in `assets/hadron.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Hadron
Comment=Hyper-fast, natively compiled Rust multi-agent OS
Exec=hadron-chamber
Icon=hadron
Categories=Development;IDE;
Terminal=false
```

## 4. Security & Isolation Considerations
- **Binary Resolution**: Always resolve the `hadron-gluon` executable relative to `std::env::current_exe()` (e.g. `exe_dir.join("hadron-gluon")`), never via generic `$PATH` search, preventing unauthorized binary execution.
- **Process Boundaries**: Process termination signal (`SIGTERM`) is scoped strictly to the daemon spawned by chamber or verified via PID/flock.

## 5. Testing Plan
1. **Lock Detection Unit Tests**: Verify lock acquisition and detection in `main.rs` tests.
2. **Config Serde Tests**: Unit test `ChamberPrefs` round-tripping for `close_gluon_on_exit`.
3. **Integration Verification**: Launch `hadron-chamber` without daemon -> verify daemon auto-starts and `field.jsonl` is watched. Verify `--no-daemon` skips spawning.
