use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use crate::QuarkId;

/// A structured task node in a plan's directed acyclic graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagTaskNode {
    pub id: String,
    pub title: String,
    pub dependencies: Vec<String>,
    pub files_create: Vec<String>,
    pub files_modify: Vec<String>,
    pub files_test: Vec<String>,
    pub assigned_quark: Option<QuarkId>,
    pub completed: bool,
    pub commit_hash: Option<String>,
}

impl DagTaskNode {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            dependencies: Vec::new(),
            files_create: Vec::new(),
            files_modify: Vec::new(),
            files_test: Vec::new(),
            assigned_quark: None,
            completed: false,
            commit_hash: None,
        }
    }
}

/// A single node in a multi-quark task dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub title: String,
    pub depends_on: Vec<String>,
    pub assigned_quark: Option<String>,
    pub completed: bool,
    pub commit_hash: Option<String>,
}

/// DAG representing task dependencies and topological execution waves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskGraph {
    pub tasks: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn new(tasks: Vec<TaskNode>) -> Self {
        Self { tasks }
    }

    /// Parse markdown formatted tasks containing `- [ ]` or `- [x]` checklist items.
    /// Supports dependency annotations like `(depends_on: [task-1, task-2])` or `(after: task-1)`.
    pub fn parse_from_markdown(content: &str) -> Self {
        let mut tasks = Vec::new();
        let mut auto_idx = 1;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") {
                let completed = trimmed.starts_with("- [x]");
                let text = trimmed[5..].trim();

                // Extract task id if present (e.g. "Task 1.1: Title" or "task-1: Title")
                let (id, title, depends_on, commit_hash) = Self::parse_task_line(text, auto_idx);
                auto_idx += 1;

                // Extract commit hash if in completed form (commit <hash>)
                tasks.push(TaskNode {
                    id,
                    title,
                    depends_on,
                    assigned_quark: None,
                    completed,
                    commit_hash,
                });
            }
        }

        Self { tasks }
    }

    fn parse_task_line(text: &str, auto_idx: usize) -> (String, String, Vec<String>, Option<String>) {
        let mut title = text.trim().to_string();
        let mut depends_on = Vec::new();
        let mut commit_hash = None;

        // Check for commit hash: "(commit <hash>)"
        if let Some(pos) = title.find("(commit ") {
            if let Some(end_pos) = title[pos..].find(')') {
                let hash_str = &title[pos + 8..pos + end_pos];
                commit_hash = Some(hash_str.trim().to_string());
                title = format!("{}{}", title[..pos].trim(), &title[pos + end_pos + 1..]);
            }
        }

        // Check for depends_on / after annotations
        if let Some(pos) = title.find("(depends_on:") {
            if let Some(end_pos) = title[pos..].find(')') {
                let inner = &title[pos + 12..pos + end_pos];
                let cleaned = inner.trim_matches(|c| c == '[' || c == ']' || c == ' ');
                for dep in cleaned.split(',') {
                    let d = dep.trim().trim_matches(|c| c == '*' || c == '`');
                    if !d.is_empty() {
                        depends_on.push(d.to_string());
                    }
                }
                title = format!("{}{}", title[..pos].trim(), &title[pos + end_pos + 1..]);
            }
        } else if let Some(pos) = title.find("(after:") {
            if let Some(end_pos) = title[pos..].find(')') {
                let inner = &title[pos + 7..pos + end_pos];
                for dep in inner.split(',') {
                    let d = dep.trim().trim_matches(|c| c == '*' || c == '`');
                    if !d.is_empty() {
                        depends_on.push(d.to_string());
                    }
                }
                title = format!("{}{}", title[..pos].trim(), &title[pos + end_pos + 1..]);
            }
        }

        // Strip leading/trailing formatting like **
        let clean_title = title.trim().trim_matches(|c| c == '*' || c == '_' || c == '`').trim();

        // Extract identifier from title prefix if available (e.g. "Task 1.1: ...")
        let id = if let Some(colon_pos) = clean_title.find(':') {
            let candidate = clean_title[..colon_pos]
                .trim()
                .trim_matches(|c| c == '*' || c == '_')
                .to_lowercase()
                .replace(' ', "-");
            if candidate.is_empty() {
                format!("task-{}", auto_idx)
            } else {
                candidate
            }
        } else {
            format!("task-{}", auto_idx)
        };

        let final_title = if let Some(colon_pos) = clean_title.find(':') {
            clean_title[colon_pos + 1..].trim().to_string()
        } else {
            clean_title.to_string()
        };

        (id, if final_title.is_empty() { clean_title.to_string() } else { final_title }, depends_on, commit_hash)
    }

    /// Compute concurrent execution waves via Kahn's topological sort.
    /// Each wave contains independent tasks that can be safely dispatched in parallel.
    pub fn compute_waves(&self) -> Result<Vec<Vec<TaskNode>>, String> {
        let mut in_degrees: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let mut map: HashMap<String, TaskNode> = HashMap::new();

        for t in &self.tasks {
            in_degrees.insert(t.id.clone(), 0);
            adj.insert(t.id.clone(), Vec::new());
            map.insert(t.id.clone(), t.clone());
        }

        for t in &self.tasks {
            for dep in &t.depends_on {
                if in_degrees.contains_key(dep) {
                    adj.get_mut(dep).unwrap().push(t.id.clone());
                    *in_degrees.get_mut(&t.id).unwrap() += 1;
                }
            }
        }

        let mut waves = Vec::new();
        let mut visited = HashSet::new();

        loop {
            // Find all unvisited nodes with in_degree == 0
            let current_wave_ids: Vec<String> = in_degrees
                .iter()
                .filter(|(id, &deg)| deg == 0 && !visited.contains(*id))
                .map(|(id, _)| (*id).clone())
                .collect();

            if current_wave_ids.is_empty() {
                break;
            }

            let mut wave_nodes = Vec::new();
            for id in &current_wave_ids {
                visited.insert(id.clone());
                wave_nodes.push(map.get(id).unwrap().clone());
            }

            // Decrement in_degree for dependent tasks
            for id in &current_wave_ids {
                for next_id in adj.get(id).unwrap() {
                    if let Some(deg) = in_degrees.get_mut(next_id) {
                        *deg = deg.saturating_sub(1);
                    }
                }
            }

            waves.push(wave_nodes);
        }

        if visited.len() < self.tasks.len() {
            return Err("Cycle detected in task dependency graph".to_string());
        }

        Ok(waves)
    }

    /// Return tasks that are ready for immediate execution (all dependencies satisfied and uncompleted).
    pub fn ready_tasks(&self) -> Vec<TaskNode> {
        let completed_ids: HashSet<String> = self
            .tasks
            .iter()
            .filter(|t| t.completed)
            .map(|t| t.id.clone())
            .collect();

        self.tasks
            .iter()
            .filter(|t| {
                if t.completed {
                    return false;
                }
                t.depends_on.iter().all(|dep| completed_ids.contains(dep))
            })
            .cloned()
            .collect()
    }

    /// Mark a task completed and optionally record its commit SHA.
    pub fn mark_completed(&mut self, id: &str, commit_hash: Option<&str>) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.completed = true;
            if let Some(hash) = commit_hash {
                task.commit_hash = Some(hash.to_string());
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_graph_parsing_and_waves() {
        let md = r#"
## Plan
- [ ] Task 1: Setup database
- [ ] Task 2: Build API (depends_on: [task-1])
- [ ] Task 3: Build Frontend (depends_on: [task-1])
- [ ] Task 4: End-to-end tests (depends_on: [task-2, task-3])
"#;
        let graph = TaskGraph::parse_from_markdown(md);
        assert_eq!(graph.tasks.len(), 4);
        assert_eq!(graph.tasks[0].id, "task-1");
        assert_eq!(graph.tasks[1].depends_on, vec!["task-1".to_string()]);

        let waves = graph.compute_waves().unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1); // Task 1
        assert_eq!(waves[1].len(), 2); // Task 2 and Task 3 in parallel
        assert_eq!(waves[2].len(), 1); // Task 4

        // Ready tasks should only be Task 1 initially
        let ready = graph.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "task-1");
    }

    #[test]
    fn test_task_graph_parsing_bold_formatting() {
        let md = r#"
- [ ] **Task 1: Setup database**
- [ ] **Task 2: Build API** (after: task-1)
- [x] **Task 3: Build Frontend** (commit 1234567)
"#;
        let graph = TaskGraph::parse_from_markdown(md);
        assert_eq!(graph.tasks.len(), 3);
        assert_eq!(graph.tasks[0].id, "task-1");
        assert_eq!(graph.tasks[0].title, "Setup database");
        assert_eq!(graph.tasks[1].id, "task-2");
        assert_eq!(graph.tasks[1].title, "Build API");
        assert_eq!(graph.tasks[1].depends_on, vec!["task-1".to_string()]);
        assert_eq!(graph.tasks[2].id, "task-3");
        assert_eq!(graph.tasks[2].title, "Build Frontend");
        assert!(graph.tasks[2].completed);
        assert_eq!(graph.tasks[2].commit_hash.as_deref(), Some("1234567"));
    }
}
