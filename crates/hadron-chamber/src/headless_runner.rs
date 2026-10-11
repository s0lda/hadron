use std::path::{Path, PathBuf};
use hadron_lattice::io::append_event;
use hadron_lattice::term::{self, Source};
use hadron_lattice::{Actor, Event, Kind};

/// High-level actions parsed from CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Version,
    Help,
    Update { version: Option<String> },
    Batch(HeadlessCommand),
    LaunchChamber,
}

/// Parses CLI arguments into a `CliAction`.
pub fn parse_cli_action(args: &[String]) -> CliAction {
    if args.len() < 2 {
        return CliAction::LaunchChamber;
    }
    match args[1].as_str() {
        "-V" | "--version" => CliAction::Version,
        "-h" | "--help" => CliAction::Help,
        "update" => {
            let mut version = None;
            let mut iter = args[2..].iter();
            while let Some(arg) = iter.next() {
                if arg == "--version" || arg == "-v" {
                    if let Some(val) = iter.next() {
                        version = Some(val.trim_start_matches('v').to_string());
                    }
                } else if arg == "--latest" {
                    version = None;
                } else if !arg.starts_with("--") {
                    version = Some(arg.trim_start_matches('v').to_string());
                }
            }
            CliAction::Update { version }
        }
        "run" | "ci" => {
            if let Some(cmd) = parse_headless_command(args) {
                CliAction::Batch(cmd)
            } else {
                CliAction::Help
            }
        }
        _ => CliAction::LaunchChamber,
    }
}

/// Prints top-level CLI help text.
pub fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Hadron {version}");
    println!("A desktop chamber for orchestrating a swarm of AI coding agents across isolated git worktrees\n");
    println!("USAGE:");
    println!("    hadron [OPTIONS] [FIELD_PATH]");
    println!("    hadron <SUBCOMMAND>\n");
    println!("OPTIONS:");
    println!("    -h, --help       Print help information");
    println!("    -V, --version    Print version information");
    println!("    --no-daemon      Attach to an already-running gluon daemon without auto-spawning one\n");
    println!("SUBCOMMANDS:");
    println!("    run <prompt>            Run a single headless swarm turn");
    println!("    ci [--plan <path>]      Run autonomous headless CI execution");
    println!("    update [--version <v>]  Self-update to latest or specific release version");
}

/// Runs self-update from the CLI.
pub fn run_cli_update(target_version: Option<&str>) -> Result<(), String> {
    term::info(
        Source::Chamber,
        &format!(
            "Hadron Updater: Preparing update (target: {})...",
            target_version.unwrap_or("latest")
        ),
    );

    let exe = std::env::current_exe().map_err(|e| format!("Cannot locate current binary: {e}"))?;
    let path_str = exe.to_string_lossy();
    if path_str.contains(".cargo") {
        let tag = target_version.map(|v| format!("v{}", v.trim_start_matches('v')));
        let mut cmd = std::process::Command::new("cargo");
        cmd.args(["install", "--locked", "--git", "https://github.com/s0lda/hadron.git"]);
        if let Some(t) = &tag {
            cmd.args(["--tag", t]);
        }
        cmd.arg("hadron");
        term::info(Source::Chamber, &format!("Executing cargo: {:?}", cmd));
        let status = cmd.status().map_err(|e| format!("Failed to run cargo: {e}"))?;
        if status.success() {
            term::info(Source::Chamber, "Hadron updated successfully via Cargo.");
            Ok(())
        } else {
            Err(format!("Cargo install failed with status: {status}"))
        }
    } else {
        let mut cmd = std::process::Command::new("sh");
        let mut sh_cmd = "curl -fsSL https://raw.githubusercontent.com/s0lda/hadron/main/install.sh | sh".to_string();
        if let Some(v) = target_version {
            sh_cmd = format!("curl -fsSL https://raw.githubusercontent.com/s0lda/hadron/main/install.sh | sh -s -- --version {v}");
        }
        if let Some(parent) = exe.parent() {
            sh_cmd = format!("{sh_cmd} -b {}", parent.display());
        }
        cmd.args(["-c", &sh_cmd]);
        term::info(Source::Chamber, &format!("Executing installer: {:?}", cmd));
        let status = cmd.status().map_err(|e| format!("Failed to run installer: {e}"))?;
        if status.success() {
            term::info(Source::Chamber, "Hadron updated successfully.");
            Ok(())
        } else {
            Err(format!("Installer failed with status: {status}"))
        }
    }
}

/// Subcommands supported by Hadron in headless runner mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessCommand {
    Run { prompt: String },
    Ci { plan_path: PathBuf },
}

/// Parses CLI arguments into a `HeadlessCommand` if matching `run` or `ci`.
pub fn parse_headless_command(args: &[String]) -> Option<HeadlessCommand> {
    if args.len() < 2 {
        return None;
    }
    match args[1].as_str() {
        "run" => {
            if args.len() >= 3 {
                Some(HeadlessCommand::Run {
                    prompt: args[2..].join(" "),
                })
            } else {
                None
            }
        }
        "ci" => {
            let mut plan_path = None;
            let mut iter = args[2..].iter();
            while let Some(arg) = iter.next() {
                if arg == "--plan" {
                    if let Some(val) = iter.next() {
                        plan_path = Some(PathBuf::from(val));
                    }
                } else if !arg.starts_with("--") {
                    plan_path = Some(PathBuf::from(arg));
                }
            }
            plan_path.map(|p| HeadlessCommand::Ci { plan_path: p })
        }
        _ => None,
    }
}

/// Executes a headless batch command, appending a turn event to the field.
pub fn run_headless_batch(cmd: HeadlessCommand, repo_root: &Path) -> Result<(), String> {
    let field_path = repo_root.join(".hadron").join("field.jsonl");
    if let Some(parent) = field_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match cmd {
        HeadlessCommand::Run { prompt } => {
            term::info(
                Source::Chamber,
                &format!("Headless Runner: Submitting prompt to swarm: \"{}\"", prompt),
            );
            let ev = Event::new(Actor::Human, None, Kind::Message { body: prompt });
            append_event(&field_path, &ev).map_err(|e| format!("Failed to append event: {e}"))?;
            term::info(Source::Chamber, "Turn submitted successfully to field.");
            Ok(())
        }
        HeadlessCommand::Ci { plan_path } => {
            let resolved_plan = if plan_path.is_absolute() {
                plan_path
            } else {
                repo_root.join(plan_path)
            };

            if !resolved_plan.exists() {
                return Err(format!("Plan file not found: {}", resolved_plan.display()));
            }

            term::info(
                Source::Chamber,
                &format!("Headless CI: Executing plan at {}", resolved_plan.display()),
            );
            let content = std::fs::read_to_string(&resolved_plan)
                .map_err(|e| format!("Failed to read plan: {e}"))?;
            let prompt = format!(
                "Execute CI plan: {}\n\nContent:\n{}",
                resolved_plan.display(),
                content
            );
            let ev = Event::new(Actor::Human, None, Kind::Message { body: prompt });
            append_event(&field_path, &ev).map_err(|e| format!("Failed to append event: {e}"))?;
            term::info(Source::Chamber, "CI plan submitted successfully to field.");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::io::read_events;
    use tempfile::tempdir;

    #[test]
    fn test_parse_headless_command_run() {
        let args = vec!["hadron".into(), "run".into(), "Fix".into(), "the".into(), "bug".into()];
        let cmd = parse_headless_command(&args);
        assert_eq!(
            cmd,
            Some(HeadlessCommand::Run {
                prompt: "Fix the bug".into()
            })
        );
    }

    #[test]
    fn test_parse_headless_command_ci_flag() {
        let args = vec![
            "hadron".into(),
            "ci".into(),
            "--plan".into(),
            ".hadron/docs/plans/task.md".into(),
        ];
        let cmd = parse_headless_command(&args);
        assert_eq!(
            cmd,
            Some(HeadlessCommand::Ci {
                plan_path: PathBuf::from(".hadron/docs/plans/task.md")
            })
        );
    }

    #[test]
    fn test_parse_headless_command_ci_positional() {
        let args = vec![
            "hadron".into(),
            "ci".into(),
            ".hadron/docs/plans/task.md".into(),
        ];
        let cmd = parse_headless_command(&args);
        assert_eq!(
            cmd,
            Some(HeadlessCommand::Ci {
                plan_path: PathBuf::from(".hadron/docs/plans/task.md")
            })
        );
    }

    #[test]
    fn test_parse_headless_command_none() {
        let args = vec!["hadron".into(), "--no-daemon".into()];
        assert_eq!(parse_headless_command(&args), None);

        let args_empty = vec!["hadron".into()];
        assert_eq!(parse_headless_command(&args_empty), None);
    }

    #[test]
    fn test_run_headless_batch_run_appends_event() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();

        let cmd = HeadlessCommand::Run {
            prompt: "Refactor router".into(),
        };
        let res = run_headless_batch(cmd, repo_root);
        assert!(res.is_ok());

        let field_path = repo_root.join(".hadron").join("field.jsonl");
        assert!(field_path.exists());

        let events = read_events(&field_path).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            Kind::Message { body } => assert_eq!(body, "Refactor router"),
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_run_headless_batch_ci_missing_plan() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();

        let cmd = HeadlessCommand::Ci {
            plan_path: PathBuf::from("non_existent_plan.md"),
        };
        let res = run_headless_batch(cmd, repo_root);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Plan file not found"));
    }

    #[test]
    fn test_run_headless_batch_ci_valid_plan() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        let plan_file = repo_root.join("plan.md");
        std::fs::write(&plan_file, "# CI Plan\n- [ ] Task 1").unwrap();

        let cmd = HeadlessCommand::Ci {
            plan_path: PathBuf::from("plan.md"),
        };
        let res = run_headless_batch(cmd, repo_root);
        assert!(res.is_ok());

        let field_path = repo_root.join(".hadron").join("field.jsonl");
        let events = read_events(&field_path).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            Kind::Message { body } => {
                assert!(body.contains("Execute CI plan:"));
                assert!(body.contains("# CI Plan\n- [ ] Task 1"));
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_parse_cli_actions() {
        assert_eq!(
            parse_cli_action(&["hadron".into(), "-V".into()]),
            CliAction::Version
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "--version".into()]),
            CliAction::Version
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "-h".into()]),
            CliAction::Help
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "--help".into()]),
            CliAction::Help
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "update".into()]),
            CliAction::Update { version: None }
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "update".into(), "--version".into(), "0.32.0".into()]),
            CliAction::Update { version: Some("0.32.0".into()) }
        );
        assert_eq!(
            parse_cli_action(&["hadron".into(), "run".into(), "test prompt".into()]),
            CliAction::Batch(HeadlessCommand::Run { prompt: "test prompt".into() })
        );
        assert_eq!(
            parse_cli_action(&["hadron".into()]),
            CliAction::LaunchChamber
        );
    }
}

