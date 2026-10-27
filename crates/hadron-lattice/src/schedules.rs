use std::collections::HashSet;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};

/// Policy for handling scheduled runs that were missed while Hadron was shut down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissedPolicy {
    /// Overdue runs post a passive notice with actionable [Run Now] / [Skip] prompt.
    #[default]
    Ask,
    /// Overdue runs are dispatched immediately upon startup.
    Run,
    /// Overdue runs are skipped, advancing to the next scheduled slot.
    Skip,
}

fn default_target() -> String {
    "@orchestrator".to_string()
}

fn default_true() -> bool {
    true
}

/// A scheduled task definition stored in `.hadron/schedules/schedules.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledJob {
    /// Unique identifier for this schedule (e.g. "weekly-sec").
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Safe identifier of the task folder under `.hadron/schedules/tasks/<task_id>/`.
    pub task_id: String,
    /// 5-field cron expression (or @hourly, @daily, @weekly, @monthly).
    pub cron: String,
    /// Target mention to address in the instruction (default: "@orchestrator").
    #[serde(default = "default_target")]
    pub target: String,
    /// Whether this schedule is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Policy for missed windows.
    #[serde(default)]
    pub missed_policy: MissedPolicy,
    /// Timestamp of the last successful or confirmed run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    /// Timestamp of the last skipped run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_skipped_at: Option<DateTime<Utc>>,
    /// Timestamp when an overdue warning notice was emitted for the current overdue window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overdue_warned_at: Option<DateTime<Utc>>,
    /// Guard ensuring a missing TASK.md warning is emitted only once until restored.
    #[serde(default)]
    pub missing_task_warned: bool,
}

impl ScheduledJob {
    pub fn new(id: impl Into<String>, name: impl Into<String>, task_id: impl Into<String>, cron: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            task_id: task_id.into(),
            cron: cron.into(),
            target: default_target(),
            enabled: true,
            missed_policy: MissedPolicy::Ask,
            last_run_at: None,
            last_skipped_at: None,
            overdue_warned_at: None,
            missing_task_warned: false,
        }
    }
}

/// Validates that a task_id is a single safe path segment and contains no traversal sequences.
pub fn validate_task_id(task_id: &str) -> anyhow::Result<()> {
    let trimmed = task_id.trim();
    if trimmed.is_empty() {
        anyhow::bail!("task_id cannot be empty");
    }
    if trimmed == "." || trimmed == ".." {
        anyhow::bail!("task_id cannot be '.' or '..'");
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        anyhow::bail!("task_id cannot contain path separators or null bytes");
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        anyhow::bail!("task_id can only contain alphanumeric characters, '-', '_', and '.'");
    }
    Ok(())
}

/// Evaluation result for a scheduled job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueStatus {
    /// Job is not due.
    NotDue,
    /// Job is due to run now.
    Due { slot: DateTime<Utc> },
    /// Job was missed across a closed window and requires human confirmation.
    OverdueAsk { due_at: DateTime<Utc> },
    /// Job is due or overdue, but the task definition TASK.md is missing.
    MissingTask { task_id: String, path: PathBuf },
}

/// Registry of scheduled jobs stored in `<repo_root>/.hadron/schedules/schedules.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRegistry {
    #[serde(default)]
    pub jobs: Vec<ScheduledJob>,
}

impl ScheduleRegistry {
    pub fn schedules_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".hadron").join("schedules")
    }

    pub fn schedules_file(repo_root: &Path) -> PathBuf {
        Self::schedules_dir(repo_root).join("schedules.json")
    }

    pub fn tasks_dir(repo_root: &Path) -> PathBuf {
        Self::schedules_dir(repo_root).join("tasks")
    }

    pub fn task_dir(repo_root: &Path, task_id: &str) -> anyhow::Result<PathBuf> {
        validate_task_id(task_id)?;
        Ok(Self::tasks_dir(repo_root).join(task_id))
    }

    pub fn task_markdown_path(repo_root: &Path, task_id: &str) -> anyhow::Result<PathBuf> {
        Ok(Self::task_dir(repo_root, task_id)?.join("TASK.md"))
    }

    /// Load the markdown prompt for a given task_id from `.hadron/schedules/tasks/<task_id>/TASK.md`.
    pub fn load_task_markdown(repo_root: &Path, task_id: &str) -> anyhow::Result<String> {
        let path = Self::task_markdown_path(repo_root, task_id)?;
        if !path.is_file() {
            anyhow::bail!("missing task prompt file: {}", path.display());
        }
        std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))
    }

    /// Load the schedule registry from `<repo_root>/.hadron/schedules/schedules.json`.
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let file = Self::schedules_file(repo_root);
        if !file.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&file)?;
        let reg: Self = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", file.display()))?;
        Ok(reg)
    }

    /// Save the schedule registry to `<repo_root>/.hadron/schedules/schedules.json`.
    pub fn save(&self, repo_root: &Path) -> anyhow::Result<()> {
        let dir = Self::schedules_dir(repo_root);
        std::fs::create_dir_all(&dir)?;
        let file = Self::schedules_file(repo_root);
        let content = serde_json::to_string_pretty(self)?;
        let tmp = dir.join(format!("schedules.{}.tmp", std::process::id()));
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }

    /// Find a job by ID.
    pub fn get(&self, id: &str) -> Option<&ScheduledJob> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Find a mutable job by ID.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ScheduledJob> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    /// Evaluate a single job against current time `now` and `grace_window_secs` (e.g. 3600s = 1h).
    pub fn evaluate_job(
        job: &ScheduledJob,
        repo_root: &Path,
        now: DateTime<Utc>,
        grace_window_secs: i64,
    ) -> DueStatus {
        if !job.enabled {
            return DueStatus::NotDue;
        }

        // Check if there is any due slot in (reference, now]
        let reference = job.last_run_at.max(job.last_skipped_at);
        let latest_slot = match find_latest_due_slot(&job.cron, reference, now) {
            Ok(Some(slot)) => slot,
            Ok(None) => return DueStatus::NotDue,
            Err(_) => return DueStatus::NotDue,
        };

        // If the task markdown is missing, report MissingTask immediately
        let task_file = match Self::task_markdown_path(repo_root, &job.task_id) {
            Ok(p) => p,
            Err(_) => {
                return DueStatus::MissingTask {
                    task_id: job.task_id.clone(),
                    path: repo_root.join(".hadron/schedules/tasks").join(&job.task_id).join("TASK.md"),
                }
            }
        };

        if !task_file.is_file() {
            return DueStatus::MissingTask {
                task_id: job.task_id.clone(),
                path: task_file,
            };
        }

        let age_secs = (now - latest_slot).num_seconds();
        if age_secs > grace_window_secs {
            match job.missed_policy {
                MissedPolicy::Ask => DueStatus::OverdueAsk { due_at: latest_slot },
                MissedPolicy::Run => DueStatus::Due { slot: latest_slot },
                MissedPolicy::Skip => DueStatus::NotDue,
            }
        } else {
            DueStatus::Due { slot: latest_slot }
        }
    }

    /// Evaluate all jobs in the registry, returning a list of `(job_index, DueStatus)`.
    pub fn evaluate_due_jobs(
        &self,
        repo_root: &Path,
        now: DateTime<Utc>,
        grace_window_secs: i64,
    ) -> Vec<(usize, DueStatus)> {
        let mut results = Vec::new();
        for (idx, job) in self.jobs.iter().enumerate() {
            let status = Self::evaluate_job(job, repo_root, now, grace_window_secs);
            if status != DueStatus::NotDue {
                results.push((idx, status));
            }
        }
        results
    }
}

/// Parses a 5-field cron expression or macro and checks whether it matches a given UTC datetime.
pub fn cron_matches(cron: &str, dt: &DateTime<Utc>) -> anyhow::Result<bool> {
    let normalized = match cron.trim() {
        "@hourly" => "0 * * * *",
        "@daily" | "@midnight" => "0 0 * * *",
        "@weekly" => "0 0 * * 0",
        "@monthly" => "0 0 1 * *",
        "@yearly" | "@annually" => "0 0 1 1 *",
        other => other,
    };

    let fields: Vec<&str> = normalized.split_whitespace().collect();
    if fields.len() != 5 {
        anyhow::bail!("cron expression must have 5 fields (min, hour, dom, month, dow), got {}", fields.len());
    }

    let min_matches = parse_cron_field(fields[0], 0, 59)?.contains(&dt.minute());
    if !min_matches {
        return Ok(false);
    }

    let hour_matches = parse_cron_field(fields[1], 0, 23)?.contains(&dt.hour());
    if !hour_matches {
        return Ok(false);
    }

    let dom_matches = parse_cron_field(fields[2], 1, 31)?.contains(&dt.day());
    if !dom_matches {
        return Ok(false);
    }

    let month_matches = parse_cron_field(fields[3], 1, 12)?.contains(&dt.month());
    if !month_matches {
        return Ok(false);
    }

    // Chrono Sunday is 7 or 0 (Sun = 0, Mon = 1, ..., Sat = 6)
    let dow = dt.weekday().num_days_from_sunday();
    let dow_matches = parse_cron_field(fields[4], 0, 7)?.contains(&dow);
    if !dow_matches {
        return Ok(false);
    }

    Ok(true)
}

fn parse_cron_field(field: &str, min: u32, max: u32) -> anyhow::Result<HashSet<u32>> {
    let mut set = HashSet::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (range_str, step) = if let Some((r, s)) = part.split_once('/') {
            let step_val: u32 = s.parse().map_err(|_| anyhow::anyhow!("invalid step '{s}'"))?;
            if step_val == 0 {
                anyhow::bail!("step cannot be 0");
            }
            (r, step_val)
        } else {
            (part, 1)
        };

        let (start, end) = if range_str == "*" {
            (min, max)
        } else if let Some((a, b)) = range_str.split_once('-') {
            let a_val: u32 = a.parse().map_err(|_| anyhow::anyhow!("invalid range start '{a}'"))?;
            let b_val: u32 = b.parse().map_err(|_| anyhow::anyhow!("invalid range end '{b}'"))?;
            (a_val, b_val)
        } else {
            let val: u32 = range_str.parse().map_err(|_| anyhow::anyhow!("invalid number '{range_str}'"))?;
            (val, val)
        };

        if start > end || start < min || end > max {
            anyhow::bail!("value or range {start}-{end} out of bounds [{min}-{max}]");
        }

        let mut curr = start;
        while curr <= end {
            // Map 7 (Sunday) to 0 as well
            if curr == 7 && max == 7 {
                set.insert(0);
            }
            set.insert(curr);
            curr += step;
        }
    }
    Ok(set)
}

/// Finds the most recent matching slot in `(reference, now]`.
/// Scans backward minute-by-minute up to 31 days or until `reference`.
pub fn find_latest_due_slot(
    cron: &str,
    reference: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    // Floor now to minute boundary
    let now_minute = now.with_second(0).unwrap_or(now).with_nanosecond(0).unwrap_or(now);

    // Limit lookback to 31 days (longest standard monthly interval)
    let max_lookback = now_minute - chrono::Duration::days(31);
    let floor = reference.unwrap_or(max_lookback).max(max_lookback);

    if now_minute <= floor && reference.is_some() {
        return Ok(None);
    }

    let mut cursor = now_minute;
    while cursor > floor {
        if cron_matches(cron, &cursor)? {
            return Ok(Some(cursor));
        }
        cursor -= chrono::Duration::minutes(1);
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    #[test]
    fn test_validate_task_id() {
        assert!(validate_task_id("security-audit").is_ok());
        assert!(validate_task_id("nightly_build_01").is_ok());
        assert!(validate_task_id("task.test").is_ok());

        assert!(validate_task_id("").is_err());
        assert!(validate_task_id(" ").is_err());
        assert!(validate_task_id("..").is_err());
        assert!(validate_task_id(".").is_err());
        assert!(validate_task_id("../etc").is_err());
        assert!(validate_task_id("foo/bar").is_err());
        assert!(validate_task_id("foo\\bar").is_err());
        assert!(validate_task_id("foo\0bar").is_err());
    }

    #[test]
    fn test_cron_matches() {
        // 2026-10-26 09:00:00 UTC is Monday
        let dt = Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap();
        assert_eq!(dt.weekday(), chrono::Weekday::Mon);

        assert!(cron_matches("0 9 * * 1", &dt).unwrap());
        assert!(cron_matches("0 9 * * *", &dt).unwrap());
        assert!(cron_matches("*/5 9 * * *", &dt).unwrap());
        assert!(cron_matches("0 8-10 * * 1-5", &dt).unwrap());
        assert!(!cron_matches("15 9 * * 1", &dt).unwrap());
        assert!(!cron_matches("0 10 * * 1", &dt).unwrap());
        assert!(!cron_matches("0 9 * * 2", &dt).unwrap()); // Tuesday

        assert!(cron_matches("@hourly", &dt).unwrap()); // minute 0 matches
        let dt_min5 = Utc.with_ymd_and_hms(2026, 10, 26, 9, 5, 0).unwrap();
        assert!(!cron_matches("@hourly", &dt_min5).unwrap());
    }

    #[test]
    fn test_registry_save_load() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        let mut reg = ScheduleRegistry::default();
        let job = ScheduledJob::new("sec-check", "Security Check", "sec_task", "@daily");
        reg.jobs.push(job);

        reg.save(repo).unwrap();

        let loaded = ScheduleRegistry::load(repo).unwrap();
        assert_eq!(loaded.jobs.len(), 1);
        assert_eq!(loaded.jobs[0].id, "sec-check");
        assert_eq!(loaded.jobs[0].cron, "@daily");
    }

    #[test]
    fn test_missing_task_detection() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        let mut job = ScheduledJob::new("job1", "Job 1", "nonexistent_task", "* * * * *");
        job.enabled = true;

        let now = Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap();
        let status = ScheduleRegistry::evaluate_job(&job, repo, now, 3600);

        match status {
            DueStatus::MissingTask { task_id, path } => {
                assert_eq!(task_id, "nonexistent_task");
                assert!(path.ends_with("TASK.md"));
            }
            other => panic!("expected MissingTask, got {:?}", other),
        }
    }

    #[test]
    fn test_due_and_overdue_ask_evaluation() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        // Create TASK.md for task 'daily_report'
        let task_dir = ScheduleRegistry::task_dir(repo, "daily_report").unwrap();
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("TASK.md"), "# Daily Report\nRun report.").unwrap();

        let mut job = ScheduledJob::new("daily-job", "Daily Job", "daily_report", "0 9 * * *");
        job.missed_policy = MissedPolicy::Ask;

        // Slot at 09:00. Now is 09:05 (5 mins later <= 1h grace window)
        let last_run = Utc.with_ymd_and_hms(2026, 10, 25, 9, 0, 0).unwrap();
        job.last_run_at = Some(last_run);

        let now_on_time = Utc.with_ymd_and_hms(2026, 10, 26, 9, 5, 0).unwrap();
        let status = ScheduleRegistry::evaluate_job(&job, repo, now_on_time, 3600);
        assert_eq!(status, DueStatus::Due { slot: Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap() });

        // Overdue: now is 15:00 (6 hours later > 1h grace window)
        let now_overdue = Utc.with_ymd_and_hms(2026, 10, 26, 15, 0, 0).unwrap();
        let status_overdue = ScheduleRegistry::evaluate_job(&job, repo, now_overdue, 3600);
        assert_eq!(status_overdue, DueStatus::OverdueAsk { due_at: Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap() });

        // If policy is Run, overdue returns Due
        job.missed_policy = MissedPolicy::Run;
        let status_run = ScheduleRegistry::evaluate_job(&job, repo, now_overdue, 3600);
        assert_eq!(status_run, DueStatus::Due { slot: Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap() });

        // If policy is Skip, overdue returns NotDue
        job.missed_policy = MissedPolicy::Skip;
        let status_skip = ScheduleRegistry::evaluate_job(&job, repo, now_overdue, 3600);
        assert_eq!(status_skip, DueStatus::NotDue);
    }
}
