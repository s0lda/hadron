use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use gpui::{Context, Window};
use super::Chamber;

pub const CARGO_INSTALL_ARGV: &[&str] = &[
    "cargo",
    "install",
    "--locked",
    "--git",
    "https://github.com/s0lda/hadron.git",
    "hadron",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    Available { version: String, commit: Option<String> },
    Installing { version: String },
    Installed { version: String },
    UpToDate,
    Failed(String),
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::Idle
    }
}

/// The numeric components of a `v`-prefixed release tag, or `None` for anything else.
///
/// **This repo's tags are not all releases.** `/abandon` writes an archive ref for every
/// branch it discards (`archive/<slug>`, see the branch-deletion invariant), and there
/// are several already. Without this filter, one `git push --tags` would make the update
/// pill offer "update to version archive/cli-agy-auto-update-20260727" and a click would
/// run `cargo install` against whatever `main` happened to be.
///
/// Returning the components rather than the string is what lets the caller order them
/// NUMERICALLY: `git ls-remote` sorts its output lexicographically, under which
/// `v0.10.0` sorts before `v0.9.0`, so "the last line" is not "the newest release".
fn release_version(tag: &str) -> Option<Vec<u64>> {
    // `git ls-remote` emits a peeled `refs/tags/v1.0^{}` line beside each annotated tag.
    let tag = tag.strip_suffix("^{}").unwrap_or(tag);
    let rest = tag.strip_prefix('v')?;
    let parts: Vec<u64> = rest.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    (!parts.is_empty()).then_some(parts)
}

pub fn parse_remote_update_info(remote_output: &str, current_version: &str) -> UpdateState {
    let current = release_version(&format!("v{current_version}"));
    let mut newest: Option<(Vec<u64>, String, String)> = None;

    for line in remote_output.trim().lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [sha, refname, ..] = parts[..] else { continue };
        let Some(tag) = refname.strip_prefix("refs/tags/") else { continue };
        let Some(key) = release_version(tag) else { continue };
        if newest.as_ref().is_none_or(|(best, _, _)| key > *best) {
            let version = tag.trim_end_matches("^{}").trim_start_matches('v').to_string();
            newest = Some((key, version, sha[..7.min(sha.len())].to_string()));
        }
    }

    match newest {
        // Strictly newer only. `!=` offered a *downgrade* to anyone running a build ahead
        // of the newest tag — every developer working from a checkout.
        Some((key, version, sha)) if current.as_ref().is_none_or(|c| key > *c) => {
            UpdateState::Available { version, commit: Some(sha) }
        }
        _ => UpdateState::UpToDate,
    }
}

/// The install command, built in one place so a test can inspect it without running it.
///
/// `cargo install --git` shells out to **git**, and a GUI process has no terminal: a
/// private/renamed remote, or an expired credential helper, makes git ask for a password
/// on stdin and wait for an answer that can never come — the pill would read
/// "Installing…" forever with no way to cancel. Nulling stdin and setting
/// `GIT_TERMINAL_PROMPT=0` turns that hang into a fast, legible failure. `hadron-gluon`
/// learned the same thing the hard way (`snapshot::git_with_env`).
/// `tag` is pinned with `--tag`, and that is not decoration: without it
/// `cargo install --git` builds the default branch's HEAD, so the pill would advertise
/// a release version and install whatever `main` happened to be at click time. Every flag
/// goes BEFORE the package spec — `cargo` reads the last positional as the package, and
/// a `--tag` appended after it is read as a second package name.
fn install_argv(tag: &str, build_dir: Option<&Path>) -> Vec<String> {
    let (pkg, head) = CARGO_INSTALL_ARGV
        .split_last()
        .expect("CARGO_INSTALL_ARGV carries at least a program and a package");
    let mut argv: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    argv.push("--tag".into());
    argv.push(tag.into());
    if let Some(dir) = build_dir {
        argv.push("--target-dir".into());
        argv.push(dir.to_string_lossy().into_owned());
    }
    argv.push(pkg.to_string());
    argv
}

/// What to type when the pill cannot run cargo itself — the same argv, minus the private
/// build directory, which is the click's business and not a human's. Everything that
/// decides *what gets installed* is still shared, so the advice and the click can never
/// install different things.
fn manual_install_hint(tag: &str) -> String {
    install_argv(tag, None).join(" ")
}

/// Where an update leaves its build tree and its log: `~/.hadron/update/`.
///
/// Both halves exist because a *failed* click used to leave nothing behind but rubbish.
/// `cargo install` builds in a fresh temp directory under `TMPDIR` and abandons it where
/// it died, so on 2026-07-30 four failed attempts left 6.3 GB of half-built workspace on
/// a 12 GB tmpfs — and, because each retry got a *new* temp directory, every one of them
/// restarted the 2,110-crate build from scratch. One stable, disk-backed directory fixes
/// both: nothing accumulates, and a retry after an interrupted attempt is incremental.
fn update_dir() -> Option<PathBuf> {
    Some(hadron_lattice::user_hadron_dir()?.join("update"))
}

fn update_build_dir() -> Option<PathBuf> {
    Some(update_dir()?.join("build"))
}

/// The full transcript of the last install attempt — the thing there was no such file as
/// when the update first failed and the question was "check the cargo install logs".
pub fn update_log_path() -> Option<PathBuf> {
    Some(update_dir()?.join("install.log"))
}

fn install_command(tag: &str) -> Command {
    let argv = install_argv(tag, update_build_dir().as_deref());
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// Write the whole attempt down before reducing it to something pill-sized. Best effort:
/// a log we could not write must not turn a successful install into a failed one, so
/// every error here is deliberately discarded and the caller is told by the `Option`.
fn write_install_log(command: &str, out: &Output) -> Option<PathBuf> {
    let path = update_log_path()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    let body = format!(
        "$ {}\n{}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
        command,
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    std::fs::write(&path, body).ok()?;
    Some(path)
}

/// The pill fits a few lines; a `cargo install` failure is hundreds. Keep the tail — the
/// part that names the error — and point at the log for the rest.
fn failure_message(status: &str, stderr: &str, log: Option<&Path>) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    let tail = if lines.len() > 5 {
        lines[lines.len() - 5..].join("\n")
    } else {
        stderr.trim().to_string()
    };
    let head = if tail.trim().is_empty() {
        format!("cargo install exited with {status}")
    } else {
        tail
    };
    match log {
        Some(p) => format!("{head}\n\nFull log: {}", p.display()),
        None => head,
    }
}

/// How long the finished pill stays on screen before the window goes away. An update
/// that vanished the moment it succeeded would read as a crash — this is the only reason
/// the restart is not immediate.
const RESTART_GRACE: Duration = Duration::from_secs(2);

/// Set exactly once, by the install that succeeded, and read by `main` after the window
/// has closed. A `static` because the two ends are on opposite sides of `app::run`:
/// `Chamber` does not outlive the window, and `main` owns the daemon teardown that has
/// to happen *between* the quit and the relaunch.
static RESTART_AFTER_UPDATE: AtomicBool = AtomicBool::new(false);

/// Whether the chamber quit in order to come back on the newly-installed binary, as
/// opposed to a human closing the window. `main` uses it for both halves of the restart:
/// stopping the daemon (which is still the *old* build) and relaunching.
pub fn restart_after_update_requested() -> bool {
    RESTART_AFTER_UPDATE.load(Ordering::SeqCst)
}

/// The one state that warrants a restart.
///
/// A restart is destructive — it stops the daemon, and with it every turn in flight — so
/// it may only ever follow an install that actually finished. `Failed` in particular
/// leaves the running binary untouched, and restarting into it would cost the human a
/// swarm for nothing.
fn warrants_restart(state: &UpdateState) -> bool {
    matches!(state, UpdateState::Installed { .. })
}

/// The tag naming release `version` — the inverse of [`release_version`], which is what
/// makes "compare the parsed version, then install the tag it came from" sound. Exact
/// because `release_version` only admits `v` + dot-separated integers, so there is
/// never a second `v` or a suffix to lose.
fn release_tag(version: &str) -> String {
    format!("v{version}")
}

pub fn perform_cargo_install(target_version: &str) -> UpdateState {
    let tag = release_tag(target_version);
    let mut cmd = install_command(&tag);
    // Debug-formatted from the `Command` itself, so the log's first line is what actually
    // ran — `--target-dir` and all — rather than a hand-kept copy of it.
    let shown = format!("{cmd:?}");
    let out = cmd.output();

    match out {
        Ok(o) if o.status.success() => {
            write_install_log(&shown, &o);
            UpdateState::Installed {
                version: target_version.to_string(),
            }
        }
        Ok(o) => {
            let log = write_install_log(&shown, &o);
            let msg = failure_message(
                &o.status.to_string(),
                &String::from_utf8_lossy(&o.stderr),
                log.as_deref(),
            );
            UpdateState::Failed(msg)
        }
        // The manual fallback is DERIVED from the command we would have run, tag and
        // all. Spelled out by hand it drifted the moment `--tag` was added, and advice
        // that installs something other than what the pill offered is worse than none.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => UpdateState::Failed(format!(
            "Cargo is not installed or not on PATH. Run manually: {}",
            manual_install_hint(&tag)
        )),
        Err(e) => UpdateState::Failed(format!("Failed to run cargo: {}", e)),
    }
}

impl Chamber {
    pub(super) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update_state, UpdateState::Installing { .. }) {
            return;
        }
        self.update_state = UpdateState::Checking;
        cx.notify();

        let current_version = env!("CARGO_PKG_VERSION");
        cx.spawn(async move |this, cx| {
            let state = cx
                .background_executor()
                .spawn(async move {
                    let out = Command::new("git")
                        .args(["ls-remote", "https://github.com/s0lda/hadron", "HEAD", "refs/tags/*"])
                        .output();
                    match out {
                        Ok(o) if o.status.success() => {
                            let text = String::from_utf8_lossy(&o.stdout);
                            parse_remote_update_info(&text, current_version)
                        }
                        _ => UpdateState::Failed("Could not query update repository".into()),
                    }
                })
                .await;

            let _ = this.update(cx, |chamber, cx| {
                chamber.update_state = state;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn trigger_update_flow(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match &self.update_state {
            UpdateState::Available { version, .. } => {
                let target_version = version.clone();
                self.update_state = UpdateState::Installing {
                    version: target_version.clone(),
                };
                cx.notify();

                cx.spawn(async move |this, cx| {
                    let state = cx
                        .background_executor()
                        .spawn(async move { perform_cargo_install(&target_version) })
                        .await;

                    let restart = warrants_restart(&state);
                    let _ = this.update(cx, |chamber, cx| {
                        chamber.update_state = state;
                        cx.notify();
                    });

                    // The installed binary is on disk but this process is still the old
                    // one, and so is the daemon it spawned — a fact the pill could only
                    // ever *ask* the human to act on, and which cost a manual kill every
                    // time. Quitting here is what makes the update take effect: `main`
                    // stops the daemon and relaunches once the window is down.
                    //
                    // The flag is raised INSIDE the same update as the quit, and only if
                    // that update succeeds. Raised beforehand it could outlive a failed
                    // quit — an app already shutting down when the install landed — and
                    // `main`, which reads it after `app::run` returns, would then relaunch
                    // a chamber the human had deliberately closed.
                    if restart {
                        cx.background_executor().timer(RESTART_GRACE).await;
                        let quit = this.update(cx, |_chamber, cx| {
                            RESTART_AFTER_UPDATE.store(true, Ordering::SeqCst);
                            cx.quit();
                        });
                        if quit.is_err() {
                            eprintln!(
                                "hadron-chamber: the update is installed, but this chamber was \
                                 already shutting down and could not restart itself. Start \
                                 Hadron again to run the new build."
                            );
                        }
                    }
                })
                .detach();
            }
            UpdateState::Installing { .. } => {
                // Idempotent against double clicks while installation is in progress.
            }
            // Inert for the same reason, one step later: the pill stays clickable during
            // `RESTART_GRACE`, and without this arm a click would fall through to
            // `check_for_updates` and overwrite the state with `Checking` — leaving the
            // human looking at "Checking…" when the window disappears a second later.
            UpdateState::Installed { .. } => {}
            _ => {
                self.check_for_updates(cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_install_argv_matches_readme_spec() {
        assert_eq!(
            CARGO_INSTALL_ARGV,
            &["cargo", "install", "--locked", "--git", "https://github.com/s0lda/hadron.git", "hadron"]
        );
    }

    /// A credential prompt from the `git` that `cargo install --git` drives would wait
    /// forever behind a GUI with no terminal, leaving the pill stuck on "Installing…".
    /// (Stdio has no getter, so the `Stdio::null()` half of that pair is not asserted
    /// here — only the env var is observable.)
    #[test]
    fn the_install_command_can_never_wait_on_a_credential_prompt() {
        let cmd = install_command("v0.1.0");
        let prompt = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("GIT_TERMINAL_PROMPT"))
            .map(|(_, v)| v);
        assert_eq!(prompt, Some(Some(std::ffi::OsStr::new("0"))));
        assert_eq!(cmd.get_program(), std::ffi::OsStr::new("cargo"));
    }

    /// The pill advertises a TAG and must install that tag. Without `--tag`,
    /// `cargo install --git` builds the default branch's HEAD, so the moment `main`
    /// moves past the release the click installs code the offered version never named.
    #[test]
    fn the_install_pins_the_tag_it_offered() {
        let cmd = install_command("v0.1.1");
        let args: Vec<String> =
            cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let at = args.iter().position(|a| a == "--tag").expect("no --tag in {args:?}");
        assert_eq!(args[at + 1], "v0.1.1", "the tag must follow the flag");
        // `cargo install` reads the LAST positional as the package spec; a `--tag`
        // appended after it would be read as a second package name.
        assert_eq!(args.last().map(String::as_str), Some("hadron"));
    }

    /// Cargo's own temp target directory is created under `TMPDIR` and **abandoned where
    /// it died**: four failed clicks on 2026-07-30 left `/tmp/cargo-install*` dirs holding
    /// 6.3 GB of a 12 GB tmpfs, and every retry started the 2,110-crate build from zero.
    /// A stable directory under `~/.hadron` is both the leak fix and the retry fix.
    #[test]
    fn the_install_builds_where_a_retry_can_resume_from() {
        let Some(build) = update_build_dir() else { return };
        let cmd = install_command("v0.1.1");
        let args: Vec<String> =
            cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let at = args.iter().position(|a| a == "--target-dir").expect("no --target-dir");
        assert_eq!(args[at + 1], build.to_string_lossy());
        assert!(!build.starts_with("/tmp"), "the build must not land on a tmpfs: {build:?}");
    }

    /// The hint is pasted into a human's shell, so it carries only what a human needs:
    /// the private build directory is the click's business, not theirs. This is the one
    /// deliberate difference between the hint and the command — everything that decides
    /// *what gets installed* (`--locked`, `--git`, `--tag`, the package) is still shared.
    #[test]
    fn the_manual_hint_is_a_command_a_human_can_paste() {
        let hint = manual_install_hint("v0.1.1");
        assert_eq!(
            hint,
            "cargo install --locked --git https://github.com/s0lda/hadron.git --tag v0.1.1 hadron"
        );
    }

    /// The pill has room for a few lines and a `cargo install` failure is hundreds. Until
    /// now the rest was simply dropped — asked to "check the cargo install logs" after a
    /// failed update, there were none to check.
    #[test]
    fn a_failure_names_the_log_that_holds_the_rest_of_it() {
        let stderr = (1..=40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let msg = failure_message("exit status: 101", &stderr, Some(Path::new("/tmp/i.log")));
        assert!(msg.contains("line 40"), "the tail must survive: {msg}");
        assert!(msg.contains("/tmp/i.log"), "the log must be named: {msg}");
        assert!(!msg.contains("line 1\n"), "only the tail belongs in a pill: {msg}");
    }

    /// A cargo that dies without a word (killed, or gone from PATH mid-run) left the pill
    /// saying nothing at all; the status is then the only fact there is.
    #[test]
    fn a_silent_failure_still_says_something() {
        let msg = failure_message("signal: 9 (SIGKILL)", "   \n", None);
        assert!(msg.contains("signal: 9"), "{msg}");
    }

    /// `release_tag` is `release_version`'s inverse, and the pill's correctness rests on
    /// that: it parses a tag into a version to compare, then rebuilds the tag to install.
    #[test]
    fn a_release_tag_round_trips_through_the_version_it_parses_to() {
        for tag in ["v0.1.0", "v0.1.1", "v1.0", "v10.20.30"] {
            assert!(release_version(tag).is_some(), "{tag} should parse");
            let version = tag.trim_start_matches('v');
            assert_eq!(release_tag(version), tag);
        }
    }

    /// `/abandon` writes an `archive/<slug>` tag for every branch it discards, and this
    /// repo already has several. One `git push --tags` and a non-release tag would have
    /// been offered as a version — with a `cargo install` behind the click.
    #[test]
    fn a_non_release_tag_is_never_offered_as_an_update() {
        let out = "aaaaaaaaaaaa\trefs/tags/archive/cli-agy-auto-update-20260727\n\
                   bbbbbbbbbbbb\trefs/tags/archive/acp-claude-01KYB03ZAW\n\
                   cccccccccccc\tHEAD";
        assert_eq!(parse_remote_update_info(out, "0.1.0"), UpdateState::UpToDate);
    }

    /// `git ls-remote` sorts lexicographically, under which `v0.10.0` precedes `v0.9.0`.
    /// Taking the last line would have offered 0.9.0 as the upgrade from 0.10.0's repo.
    #[test]
    fn the_newest_release_is_chosen_numerically_not_lexicographically() {
        let out = "aaaaaaaaaaaa\trefs/tags/v0.10.0\nbbbbbbbbbbbb\trefs/tags/v0.9.0";
        assert_eq!(
            parse_remote_update_info(out, "0.1.0"),
            UpdateState::Available { version: "0.10.0".into(), commit: Some("aaaaaaa".into()) }
        );
    }

    /// A developer running a build ahead of the newest tag was offered a DOWNGRADE,
    /// because the old check was `!=` rather than "strictly newer".
    #[test]
    fn a_tag_older_than_the_running_build_is_not_an_update() {
        let out = "aaaaaaaaaaaa\trefs/tags/v0.1.0";
        assert_eq!(parse_remote_update_info(out, "0.2.0"), UpdateState::UpToDate);
    }

    /// An annotated tag emits a peeled `^{}` line beside itself; both must read as one
    /// version, and neither may leak `^{}` into what the pill shows.
    #[test]
    fn an_annotated_tags_peeled_line_is_the_same_version() {
        let out = "aaaaaaaaaaaa\trefs/tags/v0.2.0\nbbbbbbbbbbbb\trefs/tags/v0.2.0^{}";
        match parse_remote_update_info(out, "0.1.0") {
            UpdateState::Available { version, .. } => assert_eq!(version, "0.2.0"),
            other => panic!("expected an update, got {other:?}"),
        }
    }

    /// The restart stops the daemon and every turn on it, so it must follow *only* an
    /// install that finished. `Failed` is the one that matters: it leaves the running
    /// binary exactly as it was, and restarting into it would cost a swarm for nothing.
    #[test]
    fn only_a_finished_install_warrants_a_restart() {
        assert!(warrants_restart(&UpdateState::Installed { version: "0.1.2".into() }));
        for state in [
            UpdateState::Idle,
            UpdateState::Checking,
            UpdateState::UpToDate,
            UpdateState::Available { version: "0.1.2".into(), commit: None },
            UpdateState::Installing { version: "0.1.2".into() },
            UpdateState::Failed("boom".into()),
        ] {
            assert!(!warrants_restart(&state), "{state:?} must not restart the chamber");
        }
    }

    #[test]
    fn test_parse_remote_update_info_available_tag() {
        let output = "e4f5a6b7c8d9 refs/tags/v0.2.0";
        let state = parse_remote_update_info(output, "0.1.0");
        assert_eq!(
            state,
            UpdateState::Available {
                version: "0.2.0".to_string(),
                commit: Some("e4f5a6b".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_remote_update_info_up_to_date() {
        let output = "e4f5a6b7c8d9 refs/tags/v0.1.0";
        let state = parse_remote_update_info(output, "0.1.0");
        assert_eq!(state, UpdateState::UpToDate);
    }

    #[test]
    fn test_parse_remote_update_info_empty() {
        let state = parse_remote_update_info("", "0.1.0");
        assert_eq!(state, UpdateState::UpToDate);
    }
}

