use std::path::Path;
use chrono::Utc;
use hadron_lattice::{
    Actor, DueStatus, Event, Kind, MissedPolicy, ScheduleRegistry,
};
use crate::engine::Engine;
use crate::term::{self, Source};

pub const SCHEDULE_CHECK_INTERVAL_SECS: u64 = 30;
pub const GRACE_WINDOW_SECS: i64 = 3600; // 1 hour

/// Evaluates scheduled jobs in `<repo_root>/.hadron/schedules/schedules.json`.
/// Called periodically (every ~30s) from the gluon daemon loop at the safe point.
pub async fn check_schedules(
    repo_root: &Path,
    engine: &Engine,
) -> anyhow::Result<()> {
    let mut registry = match ScheduleRegistry::load(repo_root) {
        Ok(reg) => reg,
        Err(e) => {
            term::warn(Source::Gluon, &format!("failed to load scheduled tasks: {e:#}"));
            return Ok(());
        }
    };

    if registry.jobs.is_empty() {
        return Ok(());
    }

    let now = Utc::now();
    let evaluations = registry.evaluate_due_jobs(repo_root, now, GRACE_WINDOW_SECS);
    let mut modified = false;

    for (idx, status) in evaluations {
        let job = &mut registry.jobs[idx];
        match status {
            DueStatus::MissingTask { task_id: _, path } => {
                if !job.missing_task_warned {
                    job.missing_task_warned = true;
                    modified = true;
                    // Passive warning with NO line-leading @mention so no quark wakes
                    let warning_body = format!(
                        "⚠️ Scheduled job '{}' could not run: missing task file '{}'. \
                         Restore the folder or run `/schedule disable {}`.",
                        job.id,
                        path.display(),
                        job.id,
                    );
                    term::warn(Source::Gluon, &warning_body);
                    let _ = engine
                        .append(
                            Event::new(Actor::Gluon, None, Kind::Message { body: warning_body })
                                .with_severity(hadron_lattice::Severity::Warning),
                        )
                        .await;
                }
            }
            DueStatus::OverdueAsk { due_at } => {
                if job.missing_task_warned {
                    job.missing_task_warned = false;
                    modified = true;
                }

                // If policy is Ask, emit a passive notice once for this overdue slot
                let already_warned = job.overdue_warned_at.map_or(false, |t| t >= due_at);
                if !already_warned {
                    job.overdue_warned_at = Some(now);
                    modified = true;

                    let notice_body = format!(
                        "⚠️ Scheduled task '{}' (`{}`) is overdue (due {} UTC). \
                         Run `/schedule run {}` (or click [Run Now]) to proceed, or `/schedule skip {}` to skip.",
                        job.id,
                        job.task_id,
                        due_at.format("%Y-%m-%d %H:%M"),
                        job.id,
                        job.id,
                    );
                    term::info(Source::Gluon, &notice_body);
                    let _ = engine
                        .append(
                            Event::new(Actor::Gluon, None, Kind::Message { body: notice_body })
                                .with_severity(hadron_lattice::Severity::Info),
                        )
                        .await;
                }
            }
            DueStatus::Due { slot: _ } => {
                if job.missing_task_warned {
                    job.missing_task_warned = false;
                    modified = true;
                }

                let prompt = match ScheduleRegistry::load_task_markdown(repo_root, &job.task_id) {
                    Ok(p) => p,
                    Err(e) => {
                        term::warn(
                            Source::Gluon,
                            &format!("failed to load task markdown for job '{}': {e:#}", job.id),
                        );
                        continue;
                    }
                };

                let task_dir_rel = format!(".hadron/schedules/tasks/{}/", job.task_id);
                let target = if job.target.trim().is_empty() {
                    "@orchestrator".to_string()
                } else if job.target.starts_with('@') {
                    job.target.clone()
                } else {
                    format!("@{}", job.target)
                };

                // Format per spec:
                // [Scheduled Task: <id> (`<task_id>`)]
                // Schedule: `<cron>` | Task Dir: `.hadron/schedules/tasks/<task_id>/`
                // @orchestrator <TASK.md body>
                let body = format!(
                    "[Scheduled Task: {} (`{}`)]\nSchedule: `{}` | Task Dir: `{}`\n{} {}",
                    job.id,
                    job.task_id,
                    job.cron,
                    task_dir_rel,
                    target,
                    prompt.trim()
                );

                job.last_run_at = Some(now);
                job.overdue_warned_at = None;
                modified = true;

                term::info(
                    Source::Gluon,
                    &format!("dispatching scheduled task '{}' ({})", job.id, job.cron),
                );
                let _ = engine
                    .append(
                        Event::new(Actor::Gluon, None, Kind::Message { body })
                            .with_severity(hadron_lattice::Severity::Info),
                    )
                    .await;
            }
            DueStatus::NotDue => {
                if job.missing_task_warned {
                    if let Ok(path) = ScheduleRegistry::task_markdown_path(repo_root, &job.task_id) {
                        if path.is_file() {
                            job.missing_task_warned = false;
                            modified = true;
                        }
                    }
                }
            }
        }
    }

    if modified {
        let _ = registry.save(repo_root);
    }

    Ok(())
}
