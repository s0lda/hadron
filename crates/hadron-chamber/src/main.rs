//! The chamber: Hadron's viewer. Without `--features gui` this is a headless
//! smoke binary that projects a field file and prints row counts (proves the
//! model links without GPUI). With `gui`, it launches the GPUI window.

mod model;

#[cfg_attr(not(feature = "gui"), allow(dead_code))]
mod config;
mod vcs;

use hadron_lattice::term::{self, Source};

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

    // Read BEFORE an update can run. `cargo install` replaces the binary by rename, which
    // unlinks the inode this process is executing, and `/proc/self/exe` then reads back as
    // `<path> (deleted)` — a path that does not exist. Captured here it is just the path,
    // and by the time the restart uses it the new binary is sitting at it.
    #[cfg(feature = "gui")]
    let own_exe = std::env::current_exe();

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
                        term::warn(
                            Source::Chamber,
                            "warning: another instance of chamber is already running.",
                        );
                    }
                }
                #[cfg(not(unix))]
                {
                    chamber_lock_file = Some(file);
                }
            }
            Err(e) => {
                term::warn(
                    Source::Chamber,
                    &format!("warning: failed to open chamber lock file: {e}"),
                );
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
            term::info(Source::Chamber, "gluon is running.");
            #[cfg(feature = "gui")]
            {
                gluon_pid = read_lock_pid(&gluon_lock_path);
                gluon_running_no_pid = gluon_pid.is_none();
            }
        } else {
            term::info(Source::Chamber, "gluon is not running.");
            // Single-command launch: bring the daemon up ourselves so the user only
            // runs the chamber. Headless (no gui) stays side-effect-free by design.
            #[cfg(feature = "gui")]
            if !no_daemon {
                let gluon_bin = resolve_gluon_binary();
                term::info(
                    Source::Chamber,
                    &format!(
                        "auto-spawning daemon {:?} on field {:?}",
                        gluon_bin, p
                    ),
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
                        term::error(
                            Source::Chamber,
                            &format!("failed to spawn hadron-gluon: {e}"),
                        )
                    }
                }
            }
        }
    }

    #[cfg(feature = "gui")]
    {
        // Blocks until the window closes.
        app::run(path, chamber_lock_file);

        let restarting = app::update::restart_after_update_requested();

        // On exit, honour the user's preference: kill the daemon when enabled. We
        // terminate by PID either way — spawned-by-us or attached-to-existing — so the
        // toggle behaves identically. SIGTERM (not SIGKILL) lets gluon flush the field
        // and drop its lock cleanly before it exits.
        if should_close_gluon(config::load().close_gluon_on_exit, restarting) {
            if let Some(pid) = gluon_pid {
                // The PID may have come out of `gluon.lock`, which outlives the daemon
                // that wrote it — so check the kernel still calls that process a
                // `hadron-gluon` before signalling it.
                #[cfg(unix)]
                if pid_names_a_live_gluon(std::path::Path::new(PROC_ROOT), pid) {
                    term::info(
                        Source::Chamber,
                        &format!("closing hadron-gluon (PID {}) on exit...", pid),
                    );
                    unsafe {
                        libc::kill(pid as libc::pid_t, libc::SIGTERM);
                    }
                } else {
                    term::warn(
                        Source::Chamber,
                        &format!(
                            "leaving PID {} alone on exit — it is not a live \
                             hadron-gluon, so gluon.lock is stale and that PID may now belong \
                             to an unrelated process.",
                            pid
                        ),
                    );
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
                term::warn(
                    Source::Chamber,
                    "hadron-chamber: 'close gluon on exit' is set, but the running hadron-gluon \
                     wrote no PID to gluon.lock (likely a daemon from an older build). \
                     Rebuild and restart hadron-gluon so it records its PID."
                );
            }
        }

        if restarting {
            // The successor decides whether to spawn a daemon by trying `gluon.lock`'s
            // flock, and the kernel only drops that when the old daemon's process is
            // gone. Relaunching straight after the SIGTERM would race it: the new
            // chamber would see the lock still held, attach to a daemon that is on its
            // way out, and come up with no swarm at all.
            #[cfg(unix)]
            if let Some(pid) = gluon_pid {
                if !wait_for_process_exit(std::path::Path::new(PROC_ROOT), pid, GLUON_EXIT_WAIT) {
                    term::warn(
                        Source::Chamber,
                        &format!(
                            "hadron-gluon (PID {}) is still running after {:?}; \
                             restarting anyway — the new chamber will attach to it rather than \
                             spawn a daemon on the new build.",
                            pid, GLUON_EXIT_WAIT
                        ),
                    );
                }
            }
            match &own_exe {
                Ok(exe) => relaunch(exe, &args[1..]),
                Err(e) => term::error(
                    Source::Chamber,
                    &format!(
                        "update installed, but this process could not find its own \
                         executable to restart into ({e}). Start Hadron again by hand."
                    ),
                ),
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

/// The name the kernel reports for the daemon in `<proc_root>/<pid>/comm`: the basename
/// of the binary `resolve_gluon_binary` runs, comfortably inside `comm`'s 15-char limit.
#[cfg(all(unix, feature = "gui"))]
const GLUON_COMM: &str = "hadron-gluon";

#[cfg(all(unix, feature = "gui"))]
const PROC_ROOT: &str = "/proc";

/// Whether `pid` may be signalled as the daemon when the chamber exits.
///
/// `gluon.lock` outlives the daemon that wrote it, so the PID read from it may name a
/// dead process whose number the OS has since handed to something else — and
/// `close_gluon_on_exit` would then SIGTERM a stranger. `<proc_root>/<pid>/comm` is the
/// kernel's own name for the process, so it settles the question. Where `/proc` is
/// absent (a non-Linux unix) there is nothing to check against and the signal goes out
/// exactly as it did before.
///
/// A `hadron-gluon` serving a DIFFERENT field still matches — `comm` carries no
/// arguments — so this bounds the blast radius to the daemon family, not to one field.
#[cfg(all(unix, feature = "gui"))]
fn pid_names_a_live_gluon(proc_root: &std::path::Path, pid: u32) -> bool {
    if !proc_root.is_dir() {
        return true;
    }
    match std::fs::read_to_string(proc_root.join(pid.to_string()).join("comm")) {
        Ok(comm) => comm.trim() == GLUON_COMM,
        Err(_) => false,
    }
}

/// Whether the daemon is stopped as the chamber exits.
///
/// `close_gluon_on_exit` is the human's standing preference for an ordinary close, and it
/// is off by default — but a restart-after-update is not an ordinary close. The daemon is
/// the binary that was just replaced, and leaving it up would mean an updated chamber
/// talking to a daemon from the previous release, which is the one arrangement the
/// update was supposed to end.
#[cfg(feature = "gui")]
fn should_close_gluon(preference: bool, restarting_after_update: bool) -> bool {
    preference || restarting_after_update
}

/// How long the restart waits for the daemon it just SIGTERMed to actually be gone.
#[cfg(all(unix, feature = "gui"))]
const GLUON_EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Block until `pid` is no longer a live `hadron-gluon`, or the deadline passes.
/// `true` means it is gone. Bounded, because a daemon that ignores SIGTERM must delay
/// the restart, never cancel it.
#[cfg(all(unix, feature = "gui"))]
fn wait_for_process_exit(
    proc_root: &std::path::Path,
    pid: u32,
    deadline: std::time::Duration,
) -> bool {
    let start = std::time::Instant::now();
    loop {
        if !pid_names_a_live_gluon(proc_root, pid) {
            return true;
        }
        if start.elapsed() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Become the newly-installed binary, with the arguments this process was given.
///
/// `exec` rather than spawn-and-exit: it keeps the PID and hands the successor a machine
/// with no second chamber on it, so `chamber.lock` cannot be contended by our own corpse.
/// That lock was moved into `app::run` and dropped when it returned, which is what
/// released it — not the exec.
#[cfg(feature = "gui")]
fn relaunch(exe: &std::path::Path, args: &[String]) {
    term::info(
        Source::Chamber,
        &format!("restarting into the updated binary at {exe:?}..."),
    );
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only returns on failure.
        let e = cmd.exec();
        term::error(
            Source::Chamber,
            &format!("could not restart into {exe:?}: {e}. Start Hadron again by hand."),
        );
    }
    #[cfg(not(unix))]
    if let Err(e) = cmd.spawn() {
        term::error(
            Source::Chamber,
            &format!("could not restart into {exe:?}: {e}. Start Hadron again by hand."),
        );
    }
}

#[cfg(not(feature = "gui"))]
fn run_headless(path: Option<String>) {
    match path {
        Some(p) => {
            let events =
                hadron_lattice::io::read_events(std::path::Path::new(&p)).unwrap_or_default();
            let view = model::project(&events);
            term::out(
                Source::Chamber,
                &format!(
                    "{} chat row(s), {} quark(s) on roster",
                    view.messages.len(),
                    view.roster.len()
                ),
            );
        }
        None => term::info(Source::Chamber, "usage: hadron-chamber <field.jsonl>"),
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
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    resolve_field_path_in(&cwd, args)
}

/// [`resolve_field_path`] with the current directory injected.
///
/// The bare-invocation arm **creates** `<cwd>/.hadron/field.jsonl` when it is absent,
/// and that is the product, not an oversight: `cd /some/dir && hadron` makes
/// `/some/dir` the workspace. Two directories are two independent swarms, and nothing
/// resolves back to wherever the binary was installed or built. The accepted
/// consequence is that a mistyped `cd` seeds `.hadron/` where you land — the directory
/// you are standing in is the answer, always. **Do not turn this into a refusal.**
///
/// The cwd is a parameter because the function used to read it implicitly through a
/// relative `Path::new(".hadron")`, which no test can exercise without `chdir` — and
/// `chdir` in a test race-corrupts every other test in the binary. So the contract
/// above was untested, one refactor away from silently becoming that refusal.
fn resolve_field_path_in(cwd: &std::path::Path, args: &[String]) -> Option<String> {
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
            let hadron_dir = cwd.join(".hadron");
            let hadron_dir = hadron_dir.as_path();
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

    /// The cwd's own field, as a string — what the bare arm must resolve to.
    fn field_in(dir: &std::path::Path) -> String {
        dir.join(".hadron")
            .join("field.jsonl")
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn test_resolve_field_path_ignores_binary_names() {
        let tmp = tempfile::tempdir().unwrap();
        let args = vec![
            "target/debug/hadron".to_string(),
            "hadron-chamber".to_string(),
        ];
        let path = resolve_field_path_in(tmp.path(), &args).unwrap();
        assert_eq!(path, field_in(tmp.path()));

        let args_gluon = vec!["target/debug/hadron".to_string(), "hadron-gluon".to_string()];
        let path_gluon = resolve_field_path_in(tmp.path(), &args_gluon).unwrap();
        assert_eq!(path_gluon, field_in(tmp.path()));
    }

    #[test]
    fn test_resolve_field_path_ignores_flags() {
        let tmp = tempfile::tempdir().unwrap();
        let args = vec![
            "target/debug/hadron".to_string(),
            "--no-daemon".to_string(),
            "hadron-chamber".to_string(),
        ];
        let path = resolve_field_path_in(tmp.path(), &args).unwrap();
        assert_eq!(path, field_in(tmp.path()));
    }

    /// The product contract: `cd /some/dir && hadron` makes `/some/dir` the workspace,
    /// creating `.hadron/field.jsonl` there. A directory with no workspace is not an
    /// error — it is a new workspace. This test exists because an earlier draft of the
    /// shipping plan proposed making a bare run REFUSE here; the cwd is the answer,
    /// always, and that must not be refactored away by accident.
    #[test]
    fn a_bare_invocation_roots_the_workspace_at_the_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let args = vec!["hadron".to_string()];
        assert_eq!(
            resolve_field_path_in(tmp.path(), &args),
            Some(field_in(tmp.path()))
        );
        assert!(
            tmp.path().join(".hadron").join("field.jsonl").exists(),
            "a bare run creates the field it resolves to"
        );
    }

    /// Two different directories are two different swarms. Nothing is shared between
    /// them, and neither resolves back to the directory `hadron` was installed from.
    #[test]
    fn two_directories_are_two_workspaces() {
        let tmp = tempfile::tempdir().unwrap();
        let one = tmp.path().join("project_1");
        let two = tmp.path().join("project_2");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();

        let bare = vec!["hadron".to_string()];
        assert_eq!(resolve_field_path_in(&one, &bare), Some(field_in(&one)));
        assert_eq!(resolve_field_path_in(&two, &bare), Some(field_in(&two)));
        assert_ne!(
            resolve_field_path_in(&one, &bare),
            resolve_field_path_in(&two, &bare)
        );
    }

    /// `cargo install` places one PACKAGE's bins in one directory, and this package is
    /// the only one in the workspace that has any — because the chamber resolves
    /// `hadron-gluon` as a sibling of its own `current_exe` (`main.rs:211`) and the
    /// daemon resolves `hadron-forge-mcp` as a sibling of ITS own (`session.rs:494`).
    /// Split them across packages and two of the three fall back to a bare PATH name,
    /// silently miss, and nothing ever names a path. The manifest is the source of
    /// truth for what an install produces, and it is readable without installing.
    #[test]
    fn the_package_carries_every_binary_the_sibling_chain_needs() {
        let manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
                .expect("own manifest is readable");
        let declared: Vec<&str> = manifest
            .split("[[bin]]")
            .skip(1)
            .filter_map(|section| {
                section
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("name = "))
                    .map(|n| n.trim().trim_matches('"'))
            })
            .collect();
        for expected in ["hadron", "hadron-gluon", "hadron-forge-mcp"] {
            assert!(
                declared.contains(&expected),
                "this package must declare a [[bin]] named {expected}; without it \
                 `cargo install` leaves the sibling chain broken. Declared: {declared:?}"
            );
        }
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

    /// `close_gluon_on_exit` SIGTERMs a PID read out of `gluon.lock`, which outlives the
    /// daemon that wrote it — so a stale lock plus a recycled PID means killing a
    /// stranger. Only a process the kernel still calls `hadron-gluon` may be signalled.
    #[cfg(unix)]
    #[test]
    fn only_a_live_hadron_gluon_may_be_signalled_on_exit() {
        let proc_root = tempfile::tempdir().unwrap();
        let comm = |pid: u32, name: &str| {
            let dir = proc_root.path().join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("comm"), format!("{}\n", name)).unwrap();
        };

        comm(1001, "hadron-gluon");
        assert!(pid_names_a_live_gluon(proc_root.path(), 1001));

        // The hazard: the daemon died, the OS reused its number for something else.
        comm(1002, "sshd");
        assert!(!pid_names_a_live_gluon(proc_root.path(), 1002));

        // A PID with no process at all — the stale-lock case Jake's my-cloud hit.
        assert!(!pid_names_a_live_gluon(proc_root.path(), 1003));

        // No `/proc` to consult (non-Linux unix): behave as before and signal.
        assert!(pid_names_a_live_gluon(
            &proc_root.path().join("no-such-proc"),
            1002
        ));

        // The real kernel agrees on the name this checks for.
        if std::path::Path::new("/proc/self/comm").exists() {
            let me = std::fs::read_to_string("/proc/self/comm").unwrap();
            assert!(!me.trim().is_empty());
            assert!(!pid_names_a_live_gluon(
                std::path::Path::new(PROC_ROOT),
                std::process::id()
            ));
        }
    }

    /// `close_gluon_on_exit` is off by default, and under that preference an
    /// update-restart would have left the *old* daemon running against a new chamber —
    /// exactly the split the update exists to end. A restart overrides the preference;
    /// nothing else does.
    #[test]
    fn a_restart_after_an_update_closes_the_daemon_whatever_the_preference_says() {
        assert!(should_close_gluon(false, true), "a restart must stop the old daemon");
        assert!(should_close_gluon(true, true));
        assert!(should_close_gluon(true, false), "the preference still holds on its own");
        assert!(
            !should_close_gluon(false, false),
            "an ordinary close with the preference off must leave the daemon alone"
        );
    }

    /// The successor decides whether to spawn a daemon from `gluon.lock`'s flock, which
    /// the kernel holds until the old daemon's process is really gone — so the restart
    /// waits for it. Bounded: a daemon that ignores SIGTERM delays the relaunch, it may
    /// never cancel it.
    #[cfg(unix)]
    #[test]
    fn the_restart_waits_for_the_daemon_to_go_but_never_forever() {
        let proc_root = tempfile::tempdir().unwrap();
        let dir = proc_root.path().join("2001");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("comm"), "hadron-gluon\n").unwrap();

        let start = std::time::Instant::now();
        assert!(
            !wait_for_process_exit(
                proc_root.path(),
                2001,
                std::time::Duration::from_millis(300)
            ),
            "a daemon that never exits must be reported as still running"
        );
        assert!(start.elapsed() >= std::time::Duration::from_millis(300), "it must actually wait");
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "and must not hang");

        // Gone: returns at once.
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(wait_for_process_exit(proc_root.path(), 2001, GLUON_EXIT_WAIT));
    }
}
