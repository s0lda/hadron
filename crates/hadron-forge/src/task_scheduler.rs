use serde::{Deserialize, Serialize};

/// Task scheduler wrapper in hadron-forge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTaskSummary {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub pending_tasks: usize,
    pub ready_tasks_count: usize,
}

pub struct PlanTaskScheduler;

impl PlanTaskScheduler {
    pub fn summarize_plan(markdown_content: &str) -> PlanTaskSummary {
        let mut total: usize = 0;
        let mut completed: usize = 0;

        for line in markdown_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ]") {
                total += 1;
            } else if trimmed.starts_with("- [x]") {
                total += 1;
                completed += 1;
            }
        }

        let pending = total.saturating_sub(completed);
        let ready = if pending > 0 { 1 } else { 0 };

        PlanTaskSummary {
            total_tasks: total,
            completed_tasks: completed,
            pending_tasks: pending,
            ready_tasks_count: ready,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_task_scheduler_summary() {
        let md = r#"
- [x] Task 1 (commit 123)
- [ ] Task 2
- [ ] Task 3
"#;
        let summary = PlanTaskScheduler::summarize_plan(md);
        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.pending_tasks, 2);
    }
}
