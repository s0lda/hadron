# Unified Application Startup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provide single-command application launch by auto-spawning `hadron-gluon` from `hadron-chamber`, adding a "Close Gluon on Chamber Exit" preference setting in the UI, and shipping `assets/hadron.desktop`.

**Architecture:** `hadron-chamber` checks `gluon.lock` via `libc::flock`. If unlocked, it auto-spawns `hadron-gluon` relative to `current_exe()`. When closing, if `prefs.close_gluon_on_exit` is true, it terminates the spawned child process. A toggle switch in Settings controls `close_gluon_on_exit`.

**Tech Stack:** Rust (GPUI, `std::process`, `serde`), FreeDesktop Desktop Entry Specification.

## Global Constraints
- Preserve existing CLI behavior and support `--no-daemon` flag.
- Resolve `hadron-gluon` relative to `std::env::current_exe()`.
- Default `close_gluon_on_exit` to `false`.

---

### Task 1: Add `close_gluon_on_exit` to `ChamberPrefs`

**Files:**
- Modify: `crates/hadron-chamber/src/config.rs:37-90`
- Test: `crates/hadron-chamber/src/config.rs:144-282`

**Interfaces:**
- Produces: `ChamberPrefs::close_gluon_on_exit: bool`

- [ ] **Step 1: Write the failing unit test**

In `crates/hadron-chamber/src/config.rs` under `mod tests`:
```rust
#[test]
fn close_gluon_on_exit_defaults_to_false_and_round_trips() {
    let prefs = ChamberPrefs::default();
    assert!(!prefs.close_gluon_on_exit, "default must be false");

    let json = serde_json::to_string(&prefs).unwrap();
    assert!(json.contains("\"close_gluon_on_exit\":false"));

    let custom = ChamberPrefs {
        close_gluon_on_exit: true,
        ..Default::default()
    };
    let json_custom = serde_json::to_string(&custom).unwrap();
    let back: ChamberPrefs = serde_json::from_str(&json_custom).unwrap();
    assert!(back.close_gluon_on_exit);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber close_gluon_on_exit_defaults_to_false_and_round_trips`
Expected: FAIL due to missing field `close_gluon_on_exit` on `ChamberPrefs`.

- [ ] **Step 3: Add `close_gluon_on_exit` to `ChamberPrefs`**

In `crates/hadron-chamber/src/config.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamberPrefs {
    #[serde(default = "default_false")]
    pub roster_collapsed: bool,
    #[serde(default = "default_false")]
    pub inspector_collapsed: bool,
    #[serde(default = "default_false")]
    pub close_gluon_on_exit: bool,
    #[serde(default = "default_roster_width")]
    pub roster_width: f32,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_bounds: Option<WindowBoundsPrefs>,
    #[serde(default)]
    pub human: Identity,
    #[serde(default)]
    pub quarks: BTreeMap<String, Identity>,
}
```
And in `impl Default for ChamberPrefs`:
```rust
        ChamberPrefs {
            roster_collapsed: default_false(),
            inspector_collapsed: default_false(),
            close_gluon_on_exit: default_false(),
            roster_width: default_roster_width(),
            inspector_width: default_inspector_width(),
            window_bounds: None,
            human: Identity::default(),
            quarks: BTreeMap::new(),
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber close_gluon_on_exit_defaults_to_false_and_round_trips`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/config.rs
git commit -m "feat(chamber): add close_gluon_on_exit preference field"
```

---

### Task 2: Implement Gluon Auto-Spawn and Exit Cleanup in `hadron-chamber/src/main.rs`

**Files:**
- Modify: `crates/hadron-chamber/src/main.rs:34-116`

**Interfaces:**
- Consumes: `ChamberPrefs::close_gluon_on_exit`
- Produces: `Option<std::process::Child>` handle managed across application lifetime.

- [ ] **Step 1: Write helper function and tests for daemon path resolution**

In `crates/hadron-chamber/src/main.rs`:
```rust
fn resolve_gluon_binary() -> std::path::PathBuf {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join("hadron-gluon");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    std::path::PathBuf::from("hadron-gluon")
}
```

- [ ] **Step 2: Implement `--no-daemon` CLI parsing and auto-spawning**

In `crates/hadron-chamber/src/main.rs`:
```rust
    let args: Vec<String> = std::env::args().collect();
    let no_daemon = args.iter().any(|a| a == "--no-daemon");
    let path = args.into_iter().skip(1).find(|a| a != "--no-daemon");

    let mut spawned_gluon: Option<std::process::Child> = None;

    if let Some(p) = &path {
        // (Existing lock check)...
        if !gluon_running && !no_daemon {
            let gluon_bin = resolve_gluon_binary();
            eprintln!("hadron-chamber: auto-spawning daemon {:?} -- field {:?}", gluon_bin, p);
            match std::process::Command::new(&gluon_bin)
                .arg(p)
                .spawn()
            {
                Ok(child) => {
                    spawned_gluon = Some(child);
                }
                Err(e) => {
                    eprintln!("hadron-chamber: failed to spawn hadron-gluon: {}", e);
                }
            }
        }
    }
```

- [ ] **Step 3: Cleanup on exit if `close_gluon_on_exit` is set**

At the end of `main()` in `crates/hadron-chamber/src/main.rs`:
```rust
    let prefs = config::load();
    if prefs.close_gluon_on_exit {
        if let Some(mut child) = spawned_gluon {
            eprintln!("hadron-chamber: closing hadron-gluon on exit...");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
```

- [ ] **Step 4: Run workspace and GUI gates**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/main.rs
git commit -m "feat(chamber): auto-spawn hadron-gluon when unlocked and cleanup on exit if configured"
```

---

### Task 3: Add "Close Gluon on Chamber Exit" UI toggle in Settings Panel

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/overlay.rs` or `crates/hadron-chamber/src/app/settings/identity.rs`

**Interfaces:**
- Consumes: `ChamberPrefs::close_gluon_on_exit`
- Produces: UI Toggle for `close_gluon_on_exit` in Settings panel.

- [ ] **Step 1: Add setting toggle row in Settings overlay**

In `crates/hadron-chamber/src/app/settings/overlay.rs` (or `identity.rs` where general settings are rendered):
Add a setting row with a checkbox / switch component:
```rust
let close_gluon_on_exit = self.prefs.close_gluon_on_exit;
let toggle_row = div()
    .flex()
    .items_center()
    .justify_between()
    .py_2()
    .child(
        v_flex()
            .child(label("Close Gluon on Exit").font_bold())
            .child(label("Terminate hadron-gluon daemon process when Chamber closes").text_color(color_muted))
    )
    .child(
        switch("close-gluon-toggle", close_gluon_on_exit)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.prefs.close_gluon_on_exit = !this.prefs.close_gluon_on_exit;
                let _ = config::save(&this.prefs);
                cx.notify();
            }))
    );
```

- [ ] **Step 2: Verify GUI build**

Run: `cargo test -p hadron-chamber --features gui`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-chamber/src/app/settings/
git commit -m "feat(chamber): add Close Gluon on Exit setting switch to Settings panel"
```

---

### Task 4: Add Desktop Application Launcher Entry (`assets/hadron.desktop`)

**Files:**
- Create: `assets/hadron.desktop`

- [ ] **Step 1: Create `assets/hadron.desktop`**

```ini
[Desktop Entry]
Type=Application
Name=Hadron
Comment=Hyper-fast, natively compiled Rust multi-agent OS
Exec=hadron-chamber
Icon=hadron_app_icon
Categories=Development;IDE;
Terminal=false
StartupWMClass=hadron-chamber
```

- [ ] **Step 2: Verify icon asset exists**

Check `assets/hadron_app_icon.png` is present in `assets/`.

- [ ] **Step 3: Commit**

```bash
git add assets/hadron.desktop
git commit -m "feat(assets): add hadron.desktop launcher using hadron_app_icon"
```
