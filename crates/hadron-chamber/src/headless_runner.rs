use std::path::{Path, PathBuf};
use hadron_lattice::io::append_event;
use hadron_lattice::term::{self, Source};
use hadron_lattice::{Actor, Event, Kind};

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
}
