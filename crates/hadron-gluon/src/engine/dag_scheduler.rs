//! DAG Barrier Task Scheduler.
//!
//! First-class multi-task dependency graphs in Gluon that fan out independent tasks
//! in parallel execution waves and hold dependent turns at Merge Gate barriers until
//! parent branches pass gate verification and land.

use std::collections::{HashMap, HashSet, VecDeque};
use hadron_lattice::QuarkId;
use serde::{Deserialize, Serialize};

/// Execution and gate barrier state for an individual task node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarrierState {
    /// Waiting for upstream dependencies to pass Merge Gate.
    Pending,
    /// Dependencies satisfied; unblocked and ready for worker dispatch.
    Unblocked,
    /// Dispatched to an assigned worker quark in an isolated worktree branch.
    Running { assigned_quark: QuarkId, branch: String },
    /// Worker finished turn; held at Merge Gate barrier waiting for verification.
    GateHolding { assigned_quark: QuarkId, branch: String },
    /// Successfully verified and merged into base.
    GatePassed { commit_sha: String },
    /// Failed Merge Gate verification; holds downstream dependent tasks.
    GateFailed { reason: String },
}

/// An individual task in the barrier dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarrierTask {
    pub id: String,
    pub title: String,
    pub dependencies: HashSet<String>,
    pub state: BarrierState,
    pub files: Vec<String>,
}

/// A concurrent execution tier of independent tasks that can run in parallel across worker quarks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionWave {
    pub wave_index: usize,
    pub task_ids: Vec<String>,
}

/// DAG Barrier Task Scheduler managing wave fan-out and Merge Gate barriers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagBarrierScheduler {
    pub tasks: HashMap<String, BarrierTask>,
}

impl DagBarrierScheduler {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// Add a task to the barrier scheduler.
    pub fn add_task(&mut self, id: &str, title: &str, deps: &[&str]) -> Result<(), String> {
        self.add_task_with_files(id, title, deps, &[])
    }

    /// Add a task with tracked files.
    pub fn add_task_with_files(
        &mut self,
        id: &str,
        title: &str,
        deps: &[&str],
        files: &[&str],
    ) -> Result<(), String> {
        let task_id = id.trim().to_string();
        if self.tasks.contains_key(&task_id) {
            return Err(format!("Task '{}' already exists in DAG", task_id));
        }

        let dep_set: HashSet<String> = deps.iter().map(|s| s.trim().to_string()).collect();
        let file_list: Vec<String> = files.iter().map(|s| s.trim().to_string()).collect();

        let initial_state = if dep_set.is_empty() {
            BarrierState::Unblocked
        } else {
            BarrierState::Pending
        };

        let node = BarrierTask {
            id: task_id.clone(),
            title: title.trim().to_string(),
            dependencies: dep_set,
            state: initial_state,
            files: file_list,
        };

        self.tasks.insert(task_id.clone(), node);

        if self.has_cycle() {
            self.tasks.remove(&task_id);
            return Err(format!("Cycle detected when adding task '{}'", task_id));
        }

        Ok(())
    }

    /// Check if the dependency graph contains any cycles using Kahn's algorithm.
    pub fn has_cycle(&self) -> bool {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for id in self.tasks.keys() {
            in_degree.insert(id.clone(), 0);
            adj.insert(id.clone(), Vec::new());
        }

        for (id, node) in &self.tasks {
            for dep in &node.dependencies {
                if adj.contains_key(dep) {
                    adj.get_mut(dep).unwrap().push(id.clone());
                    *in_degree.get_mut(id).unwrap() += 1;
                }
            }
        }

        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut visited = 0;
        while let Some(u) = queue.pop_front() {
            visited += 1;
            if let Some(neighbors) = adj.get(&u) {
                for v in neighbors {
                    if let Some(deg) = in_degree.get_mut(v) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(v.clone());
                        }
                    }
                }
            }
        }

        visited < self.tasks.len()
    }

    /// Compute parallel execution waves topologically.
    pub fn compute_execution_waves(&self) -> Result<Vec<ExecutionWave>, String> {
        if self.has_cycle() {
            return Err("Cannot compute waves: DAG contains cycles".to_string());
        }

        let mut levels: HashMap<String, usize> = HashMap::new();
        let mut max_level = 0;

        // Iteratively compute level for each node
        for (id, node) in &self.tasks {
            if node.dependencies.is_empty() {
                levels.insert(id.clone(), 0);
            }
        }

        let mut changed = true;
        let mut iterations = 0;
        let max_iter = self.tasks.len() + 1;

        while changed && iterations < max_iter {
            changed = false;
            iterations += 1;

            for (id, node) in &self.tasks {
                let mut dep_max = 0;
                let mut all_deps_known = true;

                for dep in &node.dependencies {
                    if let Some(&lvl) = levels.get(dep) {
                        dep_max = dep_max.max(lvl + 1);
                    } else {
                        all_deps_known = false;
                        break;
                    }
                }

                if all_deps_known {
                    let current_lvl = levels.get(id).copied().unwrap_or(0);
                    let new_lvl = current_lvl.max(dep_max);
                    if !levels.contains_key(id) || new_lvl != current_lvl {
                        levels.insert(id.clone(), new_lvl);
                        max_level = max_level.max(new_lvl);
                        changed = true;
                    }
                }
            }
        }

        let mut waves: Vec<ExecutionWave> = Vec::new();
        for wave_idx in 0..=max_level {
            let mut wave_tasks: Vec<String> = levels
                .iter()
                .filter(|(_, &lvl)| lvl == wave_idx)
                .map(|(id, _)| id.clone())
                .collect();

            wave_tasks.sort();
            if !wave_tasks.is_empty() {
                waves.push(ExecutionWave {
                    wave_index: wave_idx,
                    task_ids: wave_tasks,
                });
            }
        }

        Ok(waves)
    }

    /// Returns all tasks that are unblocked and ready for immediate dispatch.
    pub fn ready_frontier(&self) -> Vec<String> {
        let mut ready = Vec::new();
        for (id, task) in &self.tasks {
            if task.state == BarrierState::Unblocked {
                ready.push(id.clone());
            } else if task.state == BarrierState::Pending {
                let all_deps_passed = task.dependencies.iter().all(|dep| {
                    self.tasks
                        .get(dep)
                        .map(|t| matches!(t.state, BarrierState::GatePassed { .. }))
                        .unwrap_or(false)
                });
                if all_deps_passed {
                    ready.push(id.clone());
                }
            }
        }
        ready.sort();
        ready
    }

    /// Dispatch an unblocked task to a quark worker.
    pub fn dispatch_task(&mut self, id: &str, quark: QuarkId, branch: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| format!("Task '{}' not found in DAG", id))?;

        if !matches!(task.state, BarrierState::Unblocked | BarrierState::Pending) {
            return Err(format!("Task '{}' is not in dispatchable state", id));
        }

        task.state = BarrierState::Running {
            assigned_quark: quark,
            branch: branch.to_string(),
        };

        Ok(())
    }

    /// Hold completed turn at the Merge Gate barrier.
    pub fn hold_at_gate_barrier(&mut self, id: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| format!("Task '{}' not found in DAG", id))?;

        match &task.state {
            BarrierState::Running { assigned_quark, branch } => {
                task.state = BarrierState::GateHolding {
                    assigned_quark: assigned_quark.clone(),
                    branch: branch.clone(),
                };
                Ok(())
            }
            _ => Err(format!("Task '{}' is not running, cannot hold at gate", id)),
        }
    }

    /// Mark a task as having passed the Merge Gate and unblock downstream dependent tasks.
    pub fn pass_gate_barrier(&mut self, id: &str, commit_sha: &str) -> Result<Vec<String>, String> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| format!("Task '{}' not found in DAG", id))?;

        task.state = BarrierState::GatePassed {
            commit_sha: commit_sha.to_string(),
        };

        // Check which pending tasks are now unblocked
        let mut newly_unblocked = Vec::new();
        let task_ids: Vec<String> = self.tasks.keys().cloned().collect();

        for tid in task_ids {
            let is_now_ready = {
                let t = &self.tasks[&tid];
                if t.state == BarrierState::Pending {
                    t.dependencies.iter().all(|dep| {
                        self.tasks
                            .get(dep)
                            .map(|d| matches!(d.state, BarrierState::GatePassed { .. }))
                            .unwrap_or(false)
                    })
                } else {
                    false
                }
            };

            if is_now_ready {
                if let Some(t_mut) = self.tasks.get_mut(&tid) {
                    t_mut.state = BarrierState::Unblocked;
                    newly_unblocked.push(tid.clone());
                }
            }
        }

        newly_unblocked.sort();
        Ok(newly_unblocked)
    }

    /// Mark task as failed gate verification.
    pub fn fail_gate_barrier(&mut self, id: &str, reason: &str) -> Result<(), String> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| format!("Task '{}' not found in DAG", id))?;

        task.state = BarrierState::GateFailed {
            reason: reason.to_string(),
        };

        Ok(())
    }

    /// Returns true if all tasks in the DAG have passed the Merge Gate.
    pub fn is_all_completed(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .values()
                .all(|t| matches!(t.state, BarrierState::GatePassed { .. }))
    }

    /// Generate a structured Markdown report visualizing the DAG status and barriers.
    pub fn generate_status_report(&self) -> String {
        let mut out = String::new();
        out.push_str("# DAG Barrier Task Scheduler Status\n\n");

        if let Ok(waves) = self.compute_execution_waves() {
            out.push_str("### Execution Waves & Barriers\n\n");
            for wave in &waves {
                out.push_str(&format!("**Wave {}**:\n", wave.wave_index + 1));
                for tid in &wave.task_ids {
                    if let Some(t) = self.tasks.get(tid) {
                        let state_str = match &t.state {
                            BarrierState::Pending => "⏳ Pending (Waiting on Deps)".to_string(),
                            BarrierState::Unblocked => "🟢 Ready for Dispatch".to_string(),
                            BarrierState::Running { assigned_quark, branch } => {
                                format!("🏃 Running (@{} on `{}`)", assigned_quark.as_str(), branch)
                            }
                            BarrierState::GateHolding { assigned_quark, branch } => {
                                format!("🚧 Gate Holding (@{} on `{}`)", assigned_quark.as_str(), branch)
                            }
                            BarrierState::GatePassed { commit_sha } => {
                                format!("✅ Gate Passed (`{}`)", commit_sha)
                            }
                            BarrierState::GateFailed { reason } => {
                                format!("❌ Gate Failed ({})", reason)
                            }
                        };
                        let deps_str = if t.dependencies.is_empty() {
                            "None".to_string()
                        } else {
                            t.dependencies
                                .iter()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        out.push_str(&format!(
                            "- **`{}`**: {} | State: {} | Deps: [{}]\n",
                            t.id, t.title, state_str, deps_str
                        ));
                    }
                }
                out.push('\n');
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_barrier_lifecycle_and_wave_execution() {
        let mut scheduler = DagBarrierScheduler::new();

        // Wave 1: Independent tasks
        scheduler
            .add_task("task-db", "Setup SQLite schema", &[])
            .unwrap();
        scheduler
            .add_task("task-auth", "Setup JWT auth", &[])
            .unwrap();

        // Wave 2: Depends on Wave 1
        scheduler
            .add_task("task-api", "Implement REST handlers", &["task-db", "task-auth"])
            .unwrap();

        // Wave 3: Integration tests
        scheduler
            .add_task("task-e2e", "E2E integration suite", &["task-api"])
            .unwrap();

        // Check waves
        let waves = scheduler.compute_execution_waves().unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].task_ids, vec!["task-auth", "task-db"]);
        assert_eq!(waves[1].task_ids, vec!["task-api"]);
        assert_eq!(waves[2].task_ids, vec!["task-e2e"]);

        // Initial ready frontier
        let frontier = scheduler.ready_frontier();
        assert_eq!(frontier, vec!["task-auth", "task-db"]);

        // Dispatch task-db
        scheduler
            .dispatch_task("task-db", QuarkId::new("worker-1"), "quark/w1/db")
            .unwrap();
        // Hold at gate barrier
        scheduler.hold_at_gate_barrier("task-db").unwrap();
        // Pass gate barrier
        let unblocked = scheduler.pass_gate_barrier("task-db", "sha-db-123").unwrap();
        assert!(unblocked.is_empty(), "task-api still blocked on task-auth");

        // Dispatch and pass task-auth
        scheduler
            .dispatch_task("task-auth", QuarkId::new("worker-2"), "quark/w2/auth")
            .unwrap();
        scheduler.hold_at_gate_barrier("task-auth").unwrap();
        let unblocked2 = scheduler.pass_gate_barrier("task-auth", "sha-auth-456").unwrap();
        assert_eq!(unblocked2, vec!["task-api"], "task-api must now be unblocked!");

        // Ready frontier now contains task-api
        assert_eq!(scheduler.ready_frontier(), vec!["task-api"]);

        // Dispatch and pass task-api
        scheduler
            .dispatch_task("task-api", QuarkId::new("worker-1"), "quark/w1/api")
            .unwrap();
        scheduler.hold_at_gate_barrier("task-api").unwrap();
        let unblocked3 = scheduler.pass_gate_barrier("task-api", "sha-api-789").unwrap();
        assert_eq!(unblocked3, vec!["task-e2e"]);

        // Dispatch and pass task-e2e
        scheduler
            .dispatch_task("task-e2e", QuarkId::new("worker-3"), "quark/w3/e2e")
            .unwrap();
        scheduler.hold_at_gate_barrier("task-e2e").unwrap();
        scheduler.pass_gate_barrier("task-e2e", "sha-e2e-000").unwrap();

        assert!(scheduler.is_all_completed());

        let report = scheduler.generate_status_report();
        assert!(report.contains("DAG Barrier Task Scheduler Status"));
        assert!(report.contains("Gate Passed (`sha-e2e-000`)"));
    }

    #[test]
    fn test_cycle_rejection() {
        let mut scheduler = DagBarrierScheduler::new();
        scheduler.add_task("task-1", "Task 1", &["task-2"]).unwrap();
        let err = scheduler.add_task("task-2", "Task 2", &["task-1"]);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("Cycle detected"));
    }
}
