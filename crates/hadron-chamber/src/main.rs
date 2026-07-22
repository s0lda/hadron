//! The chamber: Hadron's viewer. Without `--features gui` this is a headless
//! smoke binary that projects a field file and prints row counts (proves the
//! model links without GPUI). With `gui`, it launches the GPUI window.

mod model;

// Layout persistence is used by the window; keep it compiled (and unit-tested)
// in the default build, but don't warn when the headless binary ignores it.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod config;
mod vcs;

// Pure text logic, deliberately NOT behind `gui`: the emoji crash-guard tests must
// run in `cargo test --workspace`, the gate we actually judge a change by.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod text;

#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod sys;

// The real PTY/VTE terminal engine. Deliberately NOT behind `gui`, so its
// headless tests (pump bytes through a real PTY, assert on the parsed grid) run
// in `cargo test --workspace` — the evidence that it is a terminal, not styling.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod pty;

#[cfg(feature = "gui")]
mod app;
#[cfg(feature = "gui")]
mod theme;
#[cfg(feature = "gui")]
mod window_frame;

fn main() {
    // `--no-daemon` attaches to an already-running gluon without auto-spawning one;
    // the field path is the first non-flag argument (so flag order does not matter).
    let args: Vec<String> = std::env::args().collect();
    let no_daemon = args.iter().any(|a| a == "--no-daemon");
    let path = resolve_field_path(&args);

    let mut chamber_lock_file = None;
    // The gluon child we spawn ourselves (gui only), kept only so we can reap it on
    // exit — the kill itself goes by PID, uniformly, whether we spawned the daemon or
    // attached to an existing one.
    #[cfg(feature = "gui")]
    let mut spawned_gluon: Option<std::process::Child> = None;
    // The daemon's PID, read from `gluon.lock` when it was already running, or taken
    // from the child we spawned. Populated in BOTH cases so close-on-exit can target
    // it either way.
    #[cfg(feature = "gui")]
    let mut gluon_pid: Option<u32> = None;
    // A gluon holds the lock but wrote no PID (a daemon from a build predating PID
    // tracking). Lets the exit path warn instead of silently no-op'ing.
    #[cfg(feature = "gui")]
    let mut gluon_running_no_pid = false;
    if let Some(p) = &path {
        let field_path = std::path::Path::new(p);
        let field_dir = hadron_lattice::hadron_dir_of(field_path);
        let _ = std::fs::create_dir_all(&field_dir);

        // Check second chamber instance
        let chamber_lock_path = field_dir.join("chamber.lock");
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&chamber_lock_path)
        {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    let fd = file.as_raw_fd();
                    let lock_res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
                    if lock_res == 0 {
                        chamber_lock_file = Some(file);
                    } else {
                        eprintln!("hadron-chamber: warning: another instance of chamber is already running.");
                    }
                }
                #[cfg(not(unix))]
                {
                    chamber_lock_file = Some(file);
                }
            }
            Err(e) => {
                eprintln!("hadron-chamber: warning: failed to open chamber lock file: {}", e);
            }
        }

        // Check if gluon is running
        let gluon_lock_path = field_dir.join("gluon.lock");
        let mut gluon_running = false;
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&gluon_lock_path)
        {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    let fd = file.as_raw_fd();
                    let lock_res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
                    if lock_res == 0 {
                        // Lock acquired successfully, so gluon is NOT running!
                        unsafe { libc::flock(fd, libc::LOCK_UN) };
                    } else {
                        // Lock failed, so gluon is running!
                        gluon_running = true;
                    }
                }
            }
            Err(_) => {}
        }

        if gluon_running {
            eprintln!("hadron-chamber: gluon is running.");
            #[cfg(feature = "gui")]
            {
                gluon_pid = read_lock_pid(&gluon_lock_path);
                gluon_running_no_pid = gluon_pid.is_none();
            }
        } else {
            eprintln!("hadron-chamber: gluon is not running.");
            // Single-command launch: bring the daemon up ourselves so the user only
            // runs the chamber. Headless (no gui) stays side-effect-free by design.
            #[cfg(feature = "gui")]
            if !no_daemon {
                let gluon_bin = resolve_gluon_binary();
                eprintln!(
                    "hadron-chamber: auto-spawning daemon {:?} on field {:?}",
                    gluon_bin, p
                );
                match std::process::Command::new(&gluon_bin).arg(p).spawn() {
                    Ok(child) => {
                        // Record the PID up front (same value the daemon writes to
                        // gluon.lock, without the read-back race) so the exit path can
                        // terminate it exactly as it would an already-running daemon.
                        gluon_pid = Some(child.id());
                        spawned_gluon = Some(child);
                    }
                    Err(e) => {
                        eprintln!("hadron-chamber: failed to spawn hadron-gluon: {}", e)
                    }
                }
            }
        }
    }

    #[cfg(feature = "gui")]
    {
        // Blocks until the window closes.
        app::run(path, chamber_lock_file);

        // On exit, honour the user's preference: kill the daemon when enabled. We
        // terminate by PID either way — spawned-by-us or attached-to-existing — so the
        // toggle behaves identically. SIGTERM (not SIGKILL) lets gluon flush the field
        // and drop its lock cleanly before it exits.
        if config::load().close_gluon_on_exit {
            if let Some(pid) = gluon_pid {
                eprintln!("hadron-chamber: closing hadron-gluon (PID {}) on exit...", pid);
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }
                // Reap the child if it was ours, so it does not linger as a zombie.
                if let Some(mut child) = spawned_gluon {
                    #[cfg(not(unix))]
                    let _ = child.kill();
                    let _ = child.wait();
                }
            } else if gluon_running_no_pid {
                // A gluon holds the lock but recorded no PID, so we cannot target it.
                // This is an older daemon build predating PID tracking — surface it
                // instead of silently doing nothing.
                eprintln!(
                    "hadron-chamber: 'close gluon on exit' is set, but the running hadron-gluon \
                     wrote no PID to gluon.lock (likely a daemon from an older build). \
                     Rebuild and restart hadron-gluon so it records its PID."
                );
            }
        }
    }

    #[cfg(not(feature = "gui"))]
    {
        let _ = (chamber_lock_file, no_daemon);
        run_headless(path);
    }
}

/// Resolve the `hadron-gluon` binary next to our own executable, falling back to
/// the bare name (resolved via `PATH`) only if the sibling is absent. Resolving
/// against `current_exe()` first means a `hadron-gluon` planted earlier on `PATH`
/// cannot be run in place of the one we shipped.
#[cfg(feature = "gui")]
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

/// Read the daemon's PID from a `gluon.lock` file. Returns `None` when the file is
/// missing, empty, or does not hold a parseable integer — the case that made the
/// "close gluon on exit" option appear broken: a daemon from a build predating PID
/// tracking holds the lock but leaves it empty, so there is nothing to terminate.
#[cfg(feature = "gui")]
fn read_lock_pid(lock_path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(lock_path)
        .ok()
        .and_then(|content| content.trim().parse::<u32>().ok())
}

#[cfg(not(feature = "gui"))]
fn run_headless(path: Option<String>) {
    match path {
        Some(p) => {
            let events =
                hadron_lattice::io::read_events(std::path::Path::new(&p)).unwrap_or_default();
            let view = model::project(&events);
            println!(
                "chamber: {} chat row(s), {} quark(s) on roster",
                view.messages.len(),
                view.roster.len()
            );
        }
        None => eprintln!("usage: hadron-chamber <field.jsonl>"),
    }
}

/// Resolve the field path from command line arguments.
///
/// Skips flag arguments (starting with `-`) and binary/crate names (e.g. `hadron-chamber`,
/// `hadron-gluon`), so `cargo run hadron-chamber` or `cargo run --bin hadron-chamber` does
/// not misinterpret the binary name as the field path. If an explicit path argument is given
/// that is a directory, resolves to `.hadron/field.jsonl` (or `field.jsonl`) inside that directory.
/// Otherwise defaults to `.hadron/field.jsonl` in the current directory.
fn resolve_field_path(args: &[String]) -> Option<String> {
    let current_exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()));

    let explicit_arg = args.iter().skip(1).find(|a| {
        if a.starts_with('-') {
            return false;
        }
        let is_bin_name = a.as_str() == "hadron-chamber"
            || a.as_str() == "hadron-gluon"
            || a.as_str() == "hadron"
            || current_exe_name.as_deref() == Some(a.as_str());
        !is_bin_name
    });

    match explicit_arg {
        Some(raw_arg) => {
            let path = std::path::Path::new(raw_arg);
            if path.is_dir() {
                let hadron_field = path.join(".hadron").join("field.jsonl");
                let direct_field = path.join("field.jsonl");
                if direct_field.exists() && !hadron_field.exists() {
                    Some(direct_field.to_string_lossy().to_string())
                } else {
                    let hadron_dir = path.join(".hadron");
                    if !hadron_dir.exists() {
                        let _ = std::fs::create_dir_all(&hadron_dir);
                    }
                    if !hadron_field.exists() {
                        let _ = std::fs::File::create(&hadron_field);
                    }
                    Some(hadron_field.to_string_lossy().to_string())
                }
            } else {
                Some(raw_arg.clone())
            }
        }
        None => {
            let hadron_dir = std::path::Path::new(".hadron");
            if !hadron_dir.exists() {
                let _ = std::fs::create_dir_all(hadron_dir);
            }
            let default_field = hadron_dir.join("field.jsonl");
            if !default_field.exists() {
                let _ = std::fs::File::create(&default_field);
            }
            Some(default_field.to_string_lossy().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_field_path_ignores_binary_names() {
        let args = vec![
            "target/debug/hadron-chamber".to_string(),
            "hadron-chamber".to_string(),
        ];
        let path = resolve_field_path(&args).unwrap();
        assert_eq!(path, ".hadron/field.jsonl");

        let args_gluon = vec![
            "target/debug/hadron-chamber".to_string(),
            "hadron-gluon".to_string(),
        ];
        let path_gluon = resolve_field_path(&args_gluon).unwrap();
        assert_eq!(path_gluon, ".hadron/field.jsonl");
    }

    #[test]
    fn test_resolve_field_path_ignores_flags() {
        let args = vec![
            "target/debug/hadron-chamber".to_string(),
            "--no-daemon".to_string(),
            "hadron-chamber".to_string(),
        ];
        let path = resolve_field_path(&args).unwrap();
        assert_eq!(path, ".hadron/field.jsonl");
    }

    #[test]
    fn test_resolve_field_path_accepts_explicit_jsonl_file() {
        let args = vec![
            "target/debug/hadron-chamber".to_string(),
            "custom/path/field.jsonl".to_string(),
        ];
        let path = resolve_field_path(&args).unwrap();
        assert_eq!(path, "custom/path/field.jsonl");
    }

    #[test]
    fn read_lock_pid_parses_valid_and_rejects_empty() {
        let dir = tempfile::tempdir().unwrap();

        // A valid PID line (as hadron-gluon writes it: `writeln!(f, "{}", pid)`).
        let valid = dir.path().join("valid.lock");
        std::fs::write(&valid, "213779\n").unwrap();
        assert_eq!(read_lock_pid(&valid), Some(213779));

        // The bug's root cause: a daemon predating PID tracking holds the lock but
        // leaves it empty — must read as None, not a bogus PID.
        let empty = dir.path().join("empty.lock");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(read_lock_pid(&empty), None);

        // Whitespace-only and garbage are equally unusable.
        let blank = dir.path().join("blank.lock");
        std::fs::write(&blank, "  \n").unwrap();
        assert_eq!(read_lock_pid(&blank), None);

        // A missing file is None, not a panic.
        assert_eq!(read_lock_pid(&dir.path().join("nope.lock")), None);
    }
}
