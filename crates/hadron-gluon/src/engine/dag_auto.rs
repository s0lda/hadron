//! Hybrid DAG Auto-Dispatch Scheduler
//!
//! Evaluates task graphs, automatically identifies unblocked tasks in dependency waves,
//! tracks in-flight assignments to worker quarks, and advances downstream tasks when
//! commits land.

use std::collections::{HashMap, HashSet};
use hadron_lattice::{DagTaskNode, QuarkId};
use serde::{Deserialize, Serialize};

/// Scheduler managing the execution lifecycle of a markdown implementation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HybridDagScheduler {
    pub tasks: HashMap<String, DagTaskNode>,
    pub active_assignments: HashMap<String, QuarkId>,
}

impl HybridDagScheduler {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            active_assignments: HashMap::new(),
        }
    }

    /// Normalize task ID strings (e.g., "Task 1.1" -> "task-1.1", "task-1.1:" -> "task-1.1").
    pub fn normalize_id(id: &str) -> String {
        let trimmed = id.trim().trim_matches(|c| c == '*' || c == '_' || c == '`' || c == ':');
        let lower = trimmed.to_lowercase();
        let replaced = lower.replace(' ', "-");
        replaced.trim_matches('-').to_string()
    }

    /// Parse markdown implementation plan content.
    pub fn parse_plan(content: &str) -> Result<Self, String> {
        let mut tasks = HashMap::new();
        let mut current_task: Option<DagTaskNode> = None;
        let mut in_files = false;
        let mut in_interfaces = false;
        let mut task_steps_total = 0;
        let mut task_steps_completed = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("### Task ") || trimmed.starts_with("### Task") {
                if let Some(mut t) = current_task.take() {
                    if task_steps_total > 0 && task_steps_completed == task_steps_total {
                        t.completed = true;
                    }
                    tasks.insert(t.id.clone(), t);
                }
                in_files = false;
                in_interfaces = false;
                task_steps_total = 0;
                task_steps_completed = 0;

                let header_content = trimmed.trim_start_matches("###").trim();
                let (raw_id, task_title) = if let Some((id_part, title_part)) = header_content.split_once(':') {
                    (id_part.trim(), title_part.trim())
                } else {
                    (header_content, header_content)
                };

                let norm_id = Self::normalize_id(raw_id);
                current_task = Some(DagTaskNode {
                    id: norm_id,
                    title: task_title.to_string(),
                    dependencies: Vec::new(),
                    files_create: Vec::new(),
                    files_modify: Vec::new(),
                    files_test: Vec::new(),
                    assigned_quark: None,
                    completed: false,
                    commit_hash: None,
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
                        let d = Self::normalize_id(dep);
                        if !d.is_empty() && !task.dependencies.contains(&d) {
                            task.dependencies.push(d);
                        }
                    }
                }

                // Checkbox lines
                let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
                let is_unchecked = trimmed.starts_with("- [ ]");

                if is_checked || is_unchecked {
                    task_steps_total += 1;
                    if is_checked {
                        task_steps_completed += 1;
                        if let Some(idx) = trimmed.find("commit ") {
                            let tail = &trimmed[idx + 7..];
                            let hash = tail.split(|c| c == ')' || c == ' ' || c == '`').next().unwrap_or("");
                            if !hash.is_empty() {
                                task.commit_hash = Some(hash.to_string());
                            }
                        }
                    }
                }
            } else if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
                // List-style tasks: e.g. - [ ] Task 1.1: Title (depends_on: [task-1])
                let is_checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
                let after = trimmed[5..].trim();
                let clean = after.trim_matches(|c| c == '*' || c == '_').trim();
                let (raw_id, raw_title) = if let Some((id_p, title_p)) = clean.split_once(':') {
                    (id_p.trim(), title_p.trim())
                } else {
                    (clean, clean)
                };

                let norm_id = Self::normalize_id(raw_id);
                let mut depends_on = Vec::new();
                let mut commit_hash = None;

                if let Some(pos) = raw_title.find("(commit ") {
                    if let Some(end) = raw_title[pos..].find(')') {
                        let hash = &raw_title[pos + 8..pos + end];
                        commit_hash = Some(hash.trim().to_string());
                    }
                }

                if let Some(pos) = raw_title.find("(depends_on:") {
                    if let Some(end) = raw_title[pos..].find(')') {
                        let inner = &raw_title[pos + 12..pos + end];
                        let cleaned = inner.trim_matches(|c| c == '[' || c == ']' || c == ' ');
                        for dep in cleaned.split(',') {
                            let d = Self::normalize_id(dep);
                            if !d.is_empty() {
                                depends_on.push(d);
                            }
                        }
                    }
                }

                tasks.insert(norm_id.clone(), DagTaskNode {
                    id: norm_id,
                    title: raw_title.to_string(),
                    dependencies: depends_on,
                    files_create: Vec::new(),
                    files_modify: Vec::new(),
                    files_test: Vec::new(),
                    assigned_quark: None,
                    completed: is_checked,
                    commit_hash,
                });
            }
        }

        if let Some(mut t) = current_task {
            if task_steps_total > 0 && task_steps_completed == task_steps_total {
                t.completed = true;
            }
            tasks.insert(t.id.clone(), t);
        }

        Ok(Self {
            tasks,
            active_assignments: HashMap::new(),
        })
    }

    /// Return all tasks that are ready for immediate execution:
    /// not completed, not currently assigned, and all dependencies are completed.
    pub fn poll_ready_tasks(&self) -> Vec<DagTaskNode> {
        let completed_ids: HashSet<String> = self
            .tasks
            .values()
            .filter(|t| t.completed)
            .map(|t| t.id.clone())
            .collect();

        let mut ready: Vec<DagTaskNode> = self
            .tasks
            .values()
            .filter(|t| {
                if t.completed || self.active_assignments.contains_key(&t.id) {
                    return false;
                }
                t.dependencies.iter().all(|dep| {
                    let norm = Self::normalize_id(dep);
                    completed_ids.contains(&norm) || completed_ids.contains(dep)
                })
            })
            .cloned()
            .collect();

        ready.sort_by(|a, b| a.id.cmp(&b.id));
        ready
    }

    /// Assign an unblocked task to a quark seat.
    pub fn assign_task(&mut self, task_id: &str, quark_id: QuarkId) -> Result<(), String> {
        let norm_id = Self::normalize_id(task_id);
        if let Some(task) = self.tasks.get_mut(&norm_id) {
            task.assigned_quark = Some(quark_id.clone());
            self.active_assignments.insert(norm_id, quark_id);
            Ok(())
        } else if let Some(task) = self.tasks.get_mut(task_id) {
            task.assigned_quark = Some(quark_id.clone());
            self.active_assignments.insert(task_id.to_string(), quark_id);
            Ok(())
        } else {
            Err(format!("Task {} not found in DAG", task_id))
        }
    }

    /// Unassign a task (e.g. if worker crashed or was canceled).
    pub fn unassign_task(&mut self, task_id: &str) -> bool {
        let norm_id = Self::normalize_id(task_id);
        let mut removed = false;
        if self.active_assignments.remove(&norm_id).is_some() {
            removed = true;
        }
        if self.active_assignments.remove(task_id).is_some() {
            removed = true;
        }
        if let Some(task) = self.tasks.get_mut(&norm_id) {
            task.assigned_quark = None;
            removed = true;
        } else if let Some(task) = self.tasks.get_mut(task_id) {
            task.assigned_quark = None;
            removed = true;
        }
        removed
    }

    /// Mark a task as completed with commit SHA and unblock dependent tasks.
    /// Returns the newly unblocked ready tasks.
    pub fn mark_completed(&mut self, task_id: &str, commit_sha: &str) -> Vec<DagTaskNode> {
        let norm_id = Self::normalize_id(task_id);
        let id_to_use = if self.tasks.contains_key(&norm_id) {
            norm_id
        } else {
            task_id.to_string()
        };

        if let Some(task) = self.tasks.get_mut(&id_to_use) {
            task.completed = true;
            task.commit_hash = Some(commit_sha.to_string());
            task.assigned_quark = None;
        }
        self.active_assignments.remove(&id_to_use);

        self.poll_ready_tasks()
    }

    /// Total number of tasks in the DAG.
    pub fn total_tasks(&self) -> usize {
        self.tasks.len()
    }

    /// Number of completed tasks in the DAG.
    pub fn completed_count(&self) -> usize {
        self.tasks.values().filter(|t| t.completed).count()
    }

    /// Check if all tasks in the DAG have completed.
    pub fn is_all_completed(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.values().all(|t| t.completed)
    }

    /// Sync checkbox states and commit hashes into original plan markdown.
    pub fn sync_to_markdown(&self, original_md: &str) -> String {
        let mut out = String::new();
        let mut current_task_id: Option<String> = None;

        for line in original_md.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Task ") || trimmed.starts_with("### Task") {
                let header = trimmed.trim_start_matches("###").trim();
                let raw_id = header.split_once(':').map(|(id, _)| id.trim()).unwrap_or(header);
                let norm = Self::normalize_id(raw_id);
                current_task_id = Some(norm);
                out.push_str(line);
                out.push('\n');
                continue;
            }

            if let Some(ref tid) = current_task_id {
                if let Some(task) = self.tasks.get(tid) {
                    if task.completed && trimmed.starts_with("- [ ]") {
                        let replaced = if let Some(ref hash) = task.commit_hash {
                            line.replacen("- [ ]", &format!("- [x] (commit {})", hash), 1)
                        } else {
                            line.replacen("- [ ]", "- [x]", 1)
                        };
                        out.push_str(&replaced);
                        out.push('\n');
                        continue;
                    }
                }
            }

            out.push_str(line);
            out.push('\n');
        }

        out
    }
}
