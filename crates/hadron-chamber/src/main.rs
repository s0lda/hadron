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
    let path = std::env::args().nth(1);

    let mut chamber_lock_file = None;
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
        }
    }

    #[cfg(feature = "gui")]
    {
        app::run(path, chamber_lock_file);
    }

    #[cfg(not(feature = "gui"))]
    {
        let _ = chamber_lock_file;
        run_headless(path);
    }
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
