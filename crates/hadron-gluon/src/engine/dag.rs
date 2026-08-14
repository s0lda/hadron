//! DAG-aware task scheduler and multi-quark dependency tracker.
//!
//! Evaluates task graphs, detects topological cycles, and determines which
//! tasks are unblocked and ready for parallel dispatch to available worker quarks.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use hadron_lattice::QuarkId;
use serde::{Deserialize, Serialize};

/// Lifecycle state of an individual task node in the DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// Waiting on dependencies to complete.
    Pending,
    /// Currently being executed by an assigned quark.
    Running { assigned: QuarkId },
    /// Successfully completed and verified.
    Completed,
    /// Failed or blocked.
    Failed { reason: String },
}

/// A single node in the task dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    pub dependencies: HashSet<String>,
    pub state: TaskState,
}

/// Directed acyclic graph of interdependent swarm tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDag {
    tasks: HashMap<String, TaskNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DagError {
    DuplicateTask(String),
    TaskNotFound(String),
    CycleDetected(String),
    MissingDependency(String, String),
}

impl fmt::Display for DagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DagError::DuplicateTask(id) => write!(f, "task '{id}' already exists in DAG"),
            DagError::TaskNotFound(id) => write!(f, "task '{id}' does not exist"),
            DagError::CycleDetected(id) => write!(f, "cycle detected when adding task '{id}'"),
            DagError::MissingDependency(dep, task) => {
                write!(f, "dependency '{dep}' not found for task '{task}'")
            }
        }
    }
}

impl std::error::Error for DagError {}

impl TaskDag {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// Add a new task node to the DAG.
    pub fn add_task<S: Into<String>>(
        &mut self,
        id: impl Into<String>,
        title: impl Into<String>,
        deps: impl IntoIterator<Item = S>,
    ) -> Result<(), DagError> {
        let task_id = id.into();
        if self.tasks.contains_key(&task_id) {
            return Err(DagError::DuplicateTask(task_id));
        }

        let dep_set: HashSet<String> = deps.into_iter().map(|d| d.into()).collect();
        let node = TaskNode {
            id: task_id.clone(),
            title: title.into(),
            dependencies: dep_set,
            state: TaskState::Pending,
        };

        self.tasks.insert(task_id.clone(), node);

        if self.has_cycle() {
            self.tasks.remove(&task_id);
            return Err(DagError::CycleDetected(task_id));
        }

        Ok(())
    }

    /// List all tasks that are in `Pending` state with all dependencies `Completed`.
    pub fn ready_tasks(&self) -> Vec<TaskNode> {
        self.tasks
            .values()
            .filter(|node| {
                if node.state != TaskState::Pending {
                    return false;
                }
                node.dependencies.iter().all(|dep_id| {
                    self.tasks
                        .get(dep_id)
                        .map(|dep| dep.state == TaskState::Completed)
                        .unwrap_or(false)
                })
            })
            .cloned()
            .collect()
    }

    /// Assign and transition a ready task to `Running`.
    pub fn start_task(&mut self, id: &str, quark: QuarkId) -> Result<(), DagError> {
        let node = self.tasks.get_mut(id).ok_or_else(|| DagError::TaskNotFound(id.to_string()))?;
        node.state = TaskState::Running { assigned: quark };
        Ok(())
    }

    /// Mark a task as `Completed`.
    pub fn complete_task(&mut self, id: &str) -> Result<(), DagError> {
        let node = self.tasks.get_mut(id).ok_or_else(|| DagError::TaskNotFound(id.to_string()))?;
        node.state = TaskState::Completed;
        Ok(())
    }

    /// Mark a task as `Failed`.
    pub fn fail_task(&mut self, id: &str, reason: impl Into<String>) -> Result<(), DagError> {
        let node = self.tasks.get_mut(id).ok_or_else(|| DagError::TaskNotFound(id.to_string()))?;
        node.state = TaskState::Failed {
            reason: reason.into(),
        };
        Ok(())
    }

    /// Check if all tasks in the DAG are completed.
    pub fn is_all_completed(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.values().all(|t| t.state == TaskState::Completed)
    }

    /// Return the total number of tasks in the graph.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Get a specific task node.
    pub fn get(&self, id: &str) -> Option<&TaskNode> {
        self.tasks.get(id)
    }

    /// Topological sort cycle check using Kahn's algorithm.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_resolves_ready_tasks_in_dependency_order() {
        let mut dag = TaskDag::new();
        // Task A: independent
        dag.add_task("task_a", "Scaffold database", Vec::<&str>::new()).unwrap();
        // Task B: depends on A
        dag.add_task("task_b", "Migrate tables", vec!["task_a"]).unwrap();
        // Task C: independent
        dag.add_task("task_c", "Setup frontend", Vec::<&str>::new()).unwrap();
        // Task D: depends on B and C
        dag.add_task("task_d", "E2E integration", vec!["task_b", "task_c"]).unwrap();

        // Initially, task_a and task_c are ready
        let mut ready: Vec<String> = dag.ready_tasks().into_iter().map(|t| t.id).collect();
        ready.sort();
        assert_eq!(ready, vec!["task_a", "task_c"]);

        // Complete A
        dag.start_task("task_a", QuarkId::new("worker-1")).unwrap();
        dag.complete_task("task_a").unwrap();

        // Now task_b is ready alongside task_c
        let mut ready2: Vec<String> = dag.ready_tasks().into_iter().map(|t| t.id).collect();
        ready2.sort();
        assert_eq!(ready2, vec!["task_b", "task_c"]);

        // Complete B and C
        dag.start_task("task_b", QuarkId::new("worker-2")).unwrap();
        dag.complete_task("task_b").unwrap();
        dag.start_task("task_c", QuarkId::new("worker-1")).unwrap();
        dag.complete_task("task_c").unwrap();

        // Now task_d is ready
        let ready3: Vec<String> = dag.ready_tasks().into_iter().map(|t| t.id).collect();
        assert_eq!(ready3, vec!["task_d"]);

        dag.start_task("task_d", QuarkId::new("worker-3")).unwrap();
        dag.complete_task("task_d").unwrap();

        assert!(dag.is_all_completed());
    }

    #[test]
    fn dag_detects_and_rejects_cycles() {
        let mut dag = TaskDag::new();
        dag.add_task("task_1", "Task 1", vec!["task_2"]).unwrap();
        let err = dag.add_task("task_2", "Task 2", vec!["task_1"]).unwrap_err();
        assert!(matches!(err, DagError::CycleDetected(_)));
    }
}
