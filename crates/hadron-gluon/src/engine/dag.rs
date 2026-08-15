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

/// A single actionable step in a plan task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub number: usize,
    pub description: String,
    pub completed: bool,
    pub commit: Option<String>,
}

/// A task within a markdown plan document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    pub dependencies: Vec<String>,
    pub files_create: Vec<String>,
    pub files_modify: Vec<String>,
    pub files_test: Vec<String>,
    pub steps: Vec<PlanStep>,
}

impl PlanTask {
    pub fn is_completed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.completed)
    }
}

/// A parsed markdown implementation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDocument {
    pub title: String,
    pub goal: Option<String>,
    pub tasks: Vec<PlanTask>,
}

impl PlanDocument {
    pub fn to_dag(&self) -> Result<TaskDag, DagError> {
        let mut dag = TaskDag::new();
        for task in &self.tasks {
            dag.add_task(&task.id, &task.title, task.dependencies.iter().map(|s| s.as_str()))?;
            if task.is_completed() {
                dag.complete_task(&task.id)?;
            }
        }
        Ok(dag)
    }
}

/// Parse a markdown plan string into a structured `PlanDocument`.
pub fn parse_plan_markdown(content: &str) -> Result<PlanDocument, DagError> {
    let mut title = String::new();
    let mut goal = None;
    let mut tasks = Vec::new();
    let mut current_task: Option<PlanTask> = None;
    let mut in_files = false;
    let mut in_interfaces = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("# ") && title.is_empty() {
            title = trimmed.trim_start_matches("# ").trim().to_string();
            continue;
        }

        if trimmed.starts_with("**Goal:**") {
            goal = Some(trimmed.trim_start_matches("**Goal:**").trim().to_string());
            continue;
        }

        if trimmed.starts_with("### Task ") || trimmed.starts_with("### Task") {
            if let Some(t) = current_task.take() {
                tasks.push(t);
            }
            in_files = false;
            in_interfaces = false;

            let header_content = trimmed.trim_start_matches("###").trim();
            let (id, task_title) = if let Some((id_part, title_part)) = header_content.split_once(':') {
                (id_part.trim().to_string(), title_part.trim().to_string())
            } else {
                (header_content.to_string(), header_content.to_string())
            };

            current_task = Some(PlanTask {
                id,
                title: task_title,
                dependencies: Vec::new(),
                files_create: Vec::new(),
                files_modify: Vec::new(),
                files_test: Vec::new(),
                steps: Vec::new(),
            });
            continue;
        }

        if let Some(ref mut task) = current_task {
            if trimmed.starts_with("**Files:**") {
                in_files = true;
                in_interfaces = false;
                continue;
            }
            if trimmed.starts_with("**Interfaces:**") {
                in_interfaces = true;
                in_files = false;
                continue;
            }

            if in_files {
                if let Some(rest) = trimmed.strip_prefix("- Create:") {
                    task.files_create.push(rest.trim().trim_matches('`').to_string());
                } else if let Some(rest) = trimmed.strip_prefix("- Modify:") {
                    task.files_modify.push(rest.trim().trim_matches('`').to_string());
                } else if let Some(rest) = trimmed.strip_prefix("- Test:") {
                    task.files_test.push(rest.trim().trim_matches('`').to_string());
                } else if !trimmed.starts_with('-') && !trimmed.is_empty() {
                    in_files = false;
                }
            }

            if in_interfaces || trimmed.contains("Consumes:") || trimmed.contains("Depends on:") {
                let dep_line = if let Some(rest) = trimmed.strip_prefix("- Consumes:") {
                    rest
                } else if let Some(rest) = trimmed.strip_prefix("Consumes:") {
                    rest
                } else if let Some(rest) = trimmed.strip_prefix("- Depends on:") {
                    rest
                } else if let Some(rest) = trimmed.strip_prefix("Depends on:") {
                    rest
                } else {
                    ""
                };

                for dep in dep_line.split(',') {
                    let d = dep.trim();
                    if !d.is_empty() && !task.dependencies.iter().any(|existing| existing == d) {
                        task.dependencies.push(d.to_string());
                    }
                }
            }

            // Checkbox parsing
            let (is_step, completed, after_box) = if let Some(rest) = trimmed.strip_prefix("- [x]") {
                (true, true, rest.trim())
            } else if let Some(rest) = trimmed.strip_prefix("- [X]") {
                (true, true, rest.trim())
            } else if let Some(rest) = trimmed.strip_prefix("- [ ]") {
                (true, false, rest.trim())
            } else {
                (false, false, "")
            };

            if is_step {
                let step_num = if after_box.starts_with("**Step ") {
                    let num_part = after_box.trim_start_matches("**Step ").split([':', ' ']).next().unwrap_or("0");
                    num_part.parse::<usize>().unwrap_or(task.steps.len() + 1)
                } else {
                    task.steps.len() + 1
                };

                let commit = if let Some(idx) = after_box.find("commit") {
                    let tail = &after_box[idx..];
                    tail.split('`').nth(1).map(|c| c.to_string())
                } else {
                    None
                };

                task.steps.push(PlanStep {
                    number: step_num,
                    description: after_box.to_string(),
                    completed,
                    commit,
                });
            }
        }
    }

    if let Some(t) = current_task {
        tasks.push(t);
    }

    Ok(PlanDocument {
        title,
        goal,
        tasks,
    })
}

/// Synchronize a plan step's checkbox to `- [x]` on disk in the markdown content.
pub fn sync_plan_checkbox(
    markdown: &str,
    task_id: &str,
    step_number: usize,
    commit: Option<&str>,
) -> Result<String, DagError> {
    let mut lines = Vec::new();
    let mut in_target_task = false;
    let mut found = false;

    let target_needle = format!("### {}", task_id);

    for line in markdown.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("### ") {
            if trimmed.starts_with(&target_needle) || trimmed.contains(task_id) {
                in_target_task = true;
            } else if in_target_task {
                in_target_task = false;
            }
        }

        if in_target_task && (trimmed.starts_with("- [ ]") || trimmed.starts_with("- [ ] **Step")) {
            let step_matches = trimmed.contains(&format!("Step {step_number}:"))
                || trimmed.contains(&format!("Step {step_number}"))
                || (!trimmed.contains("Step ") && !found);

            if step_matches {
                let commit_suffix = match commit {
                    Some(c) => format!(" (commit `{c}`)"),
                    None => String::new(),
                };
                let replaced = line.replace("- [ ]", "- [x]");
                if !replaced.contains("commit") && !commit_suffix.is_empty() {
                    lines.push(format!("{replaced}{commit_suffix}"));
                } else {
                    lines.push(replaced);
                }
                found = true;
                continue;
            }
        }

        lines.push(line.to_string());
    }

    if !found {
        return Err(DagError::TaskNotFound(format!("{task_id} step {step_number}")));
    }

    let mut result = lines.join("\n");
    if markdown.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
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

    #[test]
    fn plan_markdown_parser_builds_dag_and_syncs_checkboxes() {
        let sample_plan = r#"# Sample Plan
**Goal:** Build test feature

### Task 1: Create Scaffolding
**Files:**
- Create: `src/scaffold.rs`
- Test: `tests/scaffold_test.rs`

- [x] **Step 1: Write failing test** (commit `abc12345`)
- [x] **Step 2: Implement scaffolding** (commit `abc12345`)

### Task 2: Implement Logic
**Files:**
- Modify: `src/scaffold.rs`

**Interfaces:**
- Consumes: Task 1

- [ ] **Step 1: Write logic test**
- [ ] **Step 2: Implement logic**
"#;

        let doc = parse_plan_markdown(sample_plan).expect("plan must parse");
        assert_eq!(doc.tasks.len(), 2);
        assert_eq!(doc.tasks[0].id, "Task 1");
        assert_eq!(doc.tasks[0].steps.len(), 2);
        assert!(doc.tasks[0].is_completed());
        assert_eq!(doc.tasks[1].id, "Task 2");
        assert!(!doc.tasks[1].is_completed());
        assert_eq!(doc.tasks[1].dependencies, vec!["Task 1"]);

        let dag = doc.to_dag().expect("DAG must build");
        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "Task 2");

        // Sync checkbox on disk
        let updated = sync_plan_checkbox(sample_plan, "Task 2", 1, Some("def67890")).expect("sync success");
        assert!(updated.contains("- [x] **Step 1: Write logic test** (commit `def67890`)"));
    }
}

