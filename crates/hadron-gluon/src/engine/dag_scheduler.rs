use std::collections::{HashMap, HashSet};

pub struct DagScheduler {
    dependencies: HashMap<String, HashSet<String>>,
    completed: HashSet<String>,
}

impl DagScheduler {
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
            completed: HashSet::new(),
        }
    }

    pub fn add_task(&mut self, id: &str, deps: &[&str]) {
        self.dependencies.insert(
            id.to_string(),
            deps.iter().map(|s| s.to_string()).collect(),
        );
    }

    pub fn mark_complete(&mut self, id: &str) {
        self.completed.insert(id.to_string());
    }

    pub fn ready_frontier(&self) -> Vec<String> {
        let mut ready = Vec::new();
        for (task, deps) in &self.dependencies {
            if !self.completed.contains(task) && deps.iter().all(|d| self.completed.contains(d)) {
                ready.push(task.clone());
            }
        }
        ready.sort();
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_frontier_dispatch() {
        let mut dag = DagScheduler::new();
        dag.add_task("task-1", &[]);
        dag.add_task("task-2", &[]);
        dag.add_task("task-3", &["task-1", "task-2"]);

        let frontier = dag.ready_frontier();
        assert_eq!(frontier, vec!["task-1", "task-2"]);

        dag.mark_complete("task-1");
        assert_eq!(dag.ready_frontier(), vec!["task-2"]);

        dag.mark_complete("task-2");
        assert_eq!(dag.ready_frontier(), vec!["task-3"]);
    }
}
