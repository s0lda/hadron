use std::process::Command;
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
fn install_command() -> Command {
    let mut cmd = Command::new(CARGO_INSTALL_ARGV[0]);
    cmd.args(&CARGO_INSTALL_ARGV[1..])
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

pub fn perform_cargo_install(target_version: &str) -> UpdateState {
    let out = install_command().output();

    match out {
        Ok(o) if o.status.success() => UpdateState::Installed {
            version: target_version.to_string(),
        },
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let lines: Vec<&str> = stderr.lines().collect();
            let tail = if lines.len() > 5 {
                lines[lines.len() - 5..].join("\n")
            } else {
                stderr.trim().to_string()
            };
            let msg = if tail.trim().is_empty() {
                format!("cargo install exited with status {}", o.status)
            } else {
                tail
            };
            UpdateState::Failed(msg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => UpdateState::Failed(
            "Cargo is not installed or not on PATH. Run manually: cargo install --locked --git https://github.com/s0lda/hadron.git hadron".into(),
        ),
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

                    let _ = this.update(cx, |chamber, cx| {
                        chamber.update_state = state;
                        cx.notify();
                    });
                })
                .detach();
            }
            UpdateState::Installing { .. } => {
                // Idempotent against double clicks while installation is in progress.
            }
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
        let cmd = install_command();
        let prompt = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("GIT_TERMINAL_PROMPT"))
            .map(|(_, v)| v);
        assert_eq!(prompt, Some(Some(std::ffi::OsStr::new("0"))));
        assert_eq!(cmd.get_program(), std::ffi::OsStr::new("cargo"));
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

