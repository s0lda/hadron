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
    let explicit_path = args.iter().skip(1).find(|a| !a.starts_with('-')).cloned();

    let path = match explicit_path {
        Some(p) => Some(p),
        None => {
            let hadron_dir = std::path::Path::new(".hadron");
            if !hadron_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(hadron_dir) {
                    eprintln!("hadron-chamber: warning: failed to create .hadron directory: {}", e);
                }
            }
            let default_field = hadron_dir.join("field.jsonl");
            if !default_field.exists() {
                if let Err(e) = std::fs::File::create(&default_field) {
                    eprintln!("hadron-chamber: warning: failed to create .hadron/field.jsonl file: {}", e);
                }
            }
            Some(default_field.to_string_lossy().to_string())
        }
    };

    let mut chamber_lock_file = None;
    // The gluon child we spawn ourselves (gui only). We own its lifetime: it is the
    // only daemon we may kill on exit, and only when the user opted in.
    #[cfg(feature = "gui")]
    let mut spawned_gluon: Option<std::process::Child> = None;
    if let Some(p) = &path {
        let field_path = std::path::Path::new(p);
        let field_dir = field_path.parent().unwrap_or(std::path::Path::new("."));

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
                    Ok(child) => spawned_gluon = Some(child),
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

        // On exit, honour the user's preference: kill the daemon *we* spawned only
        // when it is set. Default is false, so gluon outlives the viewer as intended.
        if config::load().close_gluon_on_exit {
            if let Some(mut child) = spawned_gluon {
                eprintln!("hadron-chamber: closing hadron-gluon on exit...");
                let _ = child.kill();
                let _ = child.wait();
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
