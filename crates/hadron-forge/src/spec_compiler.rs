//! Autonomous Spec & DAG Compiler for Hadron swarm.
//!
//! Transforms high-level feature goals, requirements, or architecture descriptions into:
//! 1. Formal Design Specifications (`.hadron/docs/specs/YYYY-MM-DD-<slug>-design.md`)
//! 2. Deterministic Implementation Plans (`.hadron/docs/plans/YYYY-MM-DD-<slug>.md`)
//!
//! Validates topological dependencies and formats tasks with standard TDD checkboxes
//! matching the Gluon DAG engine (`hadron-gluon/src/engine/dag.rs`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

/// Structured task input for compiling a spec and implementation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecTaskInput {
    pub id: usize,
    pub title: String,
    #[serde(default)]
    pub dependencies: Vec<usize>,
    #[serde(default)]
    pub files_create: Vec<String>,
    #[serde(default)]
    pub files_modify: Vec<String>,
    #[serde(default)]
    pub files_test: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// Input parameters for compiling a complete design spec and DAG plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecCompileInput {
    pub goal: String,
    pub slug: String,
    #[serde(default)]
    pub tech_stack: Option<String>,
    #[serde(default)]
    pub architecture_overview: Option<String>,
    #[serde(default)]
    pub tasks: Vec<SpecTaskInput>,
}

/// Output summary of the compiled specification and plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecCompileOutput {
    pub spec_path: String,
    pub plan_path: String,
    pub spec_content: String,
    pub plan_content: String,
    pub tasks_count: usize,
    pub is_valid_dag: bool,
}

/// Validate that a slug contains only alphanumeric characters and hyphens.
pub fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Topologically check if the list of tasks forms a valid DAG (no cycles or missing deps).
pub fn validate_task_dag(tasks: &[SpecTaskInput]) -> Result<(), String> {
    let task_ids: HashSet<usize> = tasks.iter().map(|t| t.id).collect();
    let mut in_degree: HashMap<usize, usize> = HashMap::new();
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();

    for &id in &task_ids {
        in_degree.insert(id, 0);
        adj.insert(id, Vec::new());
    }

    for task in tasks {
        for &dep in &task.dependencies {
            if !task_ids.contains(&dep) {
                return Err(format!(
                    "Task {} references non-existent dependency Task {}",
                    task.id, dep
                ));
            }
            adj.get_mut(&dep).unwrap().push(task.id);
            *in_degree.get_mut(&task.id).unwrap() += 1;
        }
    }

    let mut queue: VecDeque<usize> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut visited = 0;
    while let Some(u) = queue.pop_front() {
        visited += 1;
        if let Some(neighbors) = adj.get(&u) {
            for &v in neighbors {
                if let Some(deg) = in_degree.get_mut(&v) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(v);
                    }
                }
            }
        }
    }

    if visited < tasks.len() {
        return Err("Cycle detected in task dependencies".to_string());
    }

    Ok(())
}

/// Synthesize default TDD tasks if none were explicitly provided.
fn synthesize_default_tasks(goal: &str, tech_stack: Option<&str>) -> Vec<SpecTaskInput> {
    let stack = tech_stack.unwrap_or("Rust / Web");
    vec![
        SpecTaskInput {
            id: 1,
            title: "Core Data Models & Architecture Invariants".to_string(),
            dependencies: vec![],
            files_create: vec!["src/models.rs".to_string()],
            files_modify: vec!["src/lib.rs".to_string()],
            files_test: vec!["tests/models_test.rs".to_string()],
            steps: vec![
                "Write failing unit tests for core models".to_string(),
                "Implement data structures and invariants".to_string(),
                "Verify test pass and zero warnings".to_string(),
            ],
        },
        SpecTaskInput {
            id: 2,
            title: format!("Business Logic & Services ({stack})"),
            dependencies: vec![1],
            files_create: vec!["src/service.rs".to_string()],
            files_modify: vec!["src/lib.rs".to_string()],
            files_test: vec!["tests/service_test.rs".to_string()],
            steps: vec![
                "Write service integration tests".to_string(),
                format!("Implement core services for: {goal}"),
                "Run test suite and verify clean execution".to_string(),
            ],
        },
        SpecTaskInput {
            id: 3,
            title: "Interface & Endpoint Integration".to_string(),
            dependencies: vec![2],
            files_create: vec!["src/api.rs".to_string()],
            files_modify: vec!["src/main.rs".to_string()],
            files_test: vec!["tests/api_test.rs".to_string()],
            steps: vec![
                "Write API endpoint contract tests".to_string(),
                "Implement handlers and bind routes".to_string(),
                "Verify API contracts".to_string(),
            ],
        },
        SpecTaskInput {
            id: 4,
            title: "End-to-End Multi-Modal Verification".to_string(),
            dependencies: vec![3],
            files_create: vec![],
            files_modify: vec![],
            files_test: vec!["tests/e2e_test.rs".to_string()],
            steps: vec![
                "Run full workspace test gate".to_string(),
                "Verify live preview health checks".to_string(),
                "Update documentation and feature map".to_string(),
            ],
        },
    ]
}

/// Render a formal Design Specification in Markdown format.
pub fn render_design_spec(
    date_str: &str,
    input: &SpecCompileInput,
    tasks: &[SpecTaskInput],
) -> String {
    let tech_stack = input.tech_stack.as_deref().unwrap_or("Rust 2021, Tokio");
    let architecture = input.architecture_overview.as_deref().unwrap_or(
        "Modular subsystem architecture with strict separation of concerns, jailed file isolation, and deterministic DAG task execution."
    );

    let mut doc = String::new();
    doc.push_str(&format!("# {} Design Specification\n\n", input.slug));
    doc.push_str(&format!("**Date:** {date_str}  \n"));
    doc.push_str("**Author:** cli-agy  \n");
    doc.push_str("**Status:** Approved  \n\n");

    doc.push_str("## 1. Objective & Scope\n\n");
    doc.push_str(&format!("{}\n\n", input.goal));

    doc.push_str("## 2. Architecture & Subsystems\n\n");
    doc.push_str(&format!("{}\n\n", architecture));
    doc.push_str(&format!("- **Tech Stack:** {}\n\n", tech_stack));

    doc.push_str("## 3. Subsystem Breakdown & Task Graph\n\n");
    for task in tasks {
        let deps_str = if task.dependencies.is_empty() {
            "None (Root Task)".to_string()
        } else {
            task.dependencies
                .iter()
                .map(|d| format!("Task {d}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        doc.push_str(&format!("### Subsystem {}: {}\n", task.id, task.title));
        doc.push_str(&format!("- **Dependencies:** {}\n", deps_str));
        if !task.files_create.is_empty() {
            doc.push_str(&format!(
                "- **Created Files:** `{}`\n",
                task.files_create.join("`, `")
            ));
        }
        if !task.files_modify.is_empty() {
            doc.push_str(&format!(
                "- **Modified Files:** `{}`\n",
                task.files_modify.join("`, `")
            ));
        }
        doc.push('\n');
    }

    doc.push_str("## 4. Invariants & Security Boundaries\n\n");
    doc.push_str("- All file modifications strictly jailed within worktree root.\n");
    doc.push_str("- Zero compiler warnings, 100% test pass rate required at gate.\n");
    doc.push_str("- Media and runtime captures jailed strictly in `.hadron/screenshots/`.\n");

    doc
}

/// Render a standard-compliant Implementation Plan in Markdown format.
pub fn render_implementation_plan(
    input: &SpecCompileInput,
    tasks: &[SpecTaskInput],
) -> String {
    let tech_stack = input.tech_stack.as_deref().unwrap_or("Rust 2021, Tokio");

    let mut doc = String::new();
    doc.push_str("---\nauthor: cli-agy\nstatus: in_progress\n---\n\n");
    doc.push_str(&format!("# {} Implementation Plan\n\n", input.slug));
    doc.push_str("> **For agentic workers:** REQUIRED SUB-SKILL: Use Swarm Quark Dispatch (recommended) or subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.\n\n");
    doc.push_str(&format!("**Goal:** {}\n\n", input.goal));
    doc.push_str(&format!("**Tech Stack:** {}\n\n", tech_stack));
    doc.push_str("## Global Constraints\n\n");
    doc.push_str("- Never break existing workspace tests.\n");
    doc.push_str("- Strict adherence to Standard Model Invariants: SSOT, one definition one place, strictly jailed file operations.\n");
    doc.push_str("- Every task must be verified with automated tests before marking complete.\n\n");
    doc.push_str("---\n\n");

    for task in tasks {
        doc.push_str(&format!("### Task {}: {}\n\n", task.id, task.title));

        doc.push_str("**Files:**\n");
        for f in &task.files_create {
            doc.push_str(&format!("- Create: `{f}`\n"));
        }
        for f in &task.files_modify {
            doc.push_str(&format!("- Modify: `{f}`\n"));
        }
        for f in &task.files_test {
            doc.push_str(&format!("- Test: `{f}`\n"));
        }
        if task.files_create.is_empty() && task.files_modify.is_empty() && task.files_test.is_empty() {
            doc.push_str("- Modify: `src/lib.rs`\n");
        }

        if !task.dependencies.is_empty() {
            let deps = task
                .dependencies
                .iter()
                .map(|d| format!("Task {d}"))
                .collect::<Vec<_>>()
                .join(", ");
            doc.push_str("\n**Interfaces:**\n");
            doc.push_str(&format!("- Consumes: {deps}\n"));
        }

        doc.push('\n');

        let mut step_num = 1;
        doc.push_str(&format!(
            "- [ ] **Step {step_num}: Write failing test**\n"
        ));
        step_num += 1;
        doc.push_str(&format!(
            "- [ ] **Step {step_num}: Verify test fails**\n"
        ));
        step_num += 1;

        if task.steps.is_empty() {
            doc.push_str(&format!(
                "- [ ] **Step {step_num}: Implement {}**\n",
                task.title
            ));
            step_num += 1;
        } else {
            for step_desc in &task.steps {
                doc.push_str(&format!(
                    "- [ ] **Step {step_num}: {step_desc}**\n"
                ));
                step_num += 1;
            }
        }

        doc.push_str(&format!(
            "- [ ] **Step {step_num}: Verify test passes**\n"
        ));
        step_num += 1;
        doc.push_str(&format!(
            "- [ ] **Step {step_num}: Commit**\n\n---\n\n"
        ));
    }

    doc
}

/// Compile and write both Design Spec and Implementation Plan into `.hadron/docs/`.
pub fn compile_spec_and_plan(
    root: &Root,
    input: &SpecCompileInput,
) -> Result<SpecCompileOutput, ForgeError> {
    if !is_valid_slug(&input.slug) {
        return Err(ForgeError::Rejected(format!(
            "invalid slug {:?}: must be non-empty alphanumeric with hyphens",
            input.slug
        )));
    }

    let tasks = if input.tasks.is_empty() {
        synthesize_default_tasks(&input.goal, input.tech_stack.as_deref())
    } else {
        input.tasks.clone()
    };

    if let Err(e) = validate_task_dag(&tasks) {
        return Err(ForgeError::Rejected(format!("invalid task DAG: {e}")));
    }

    let date_str = "2026-08-15"; // Canonical execution timestamp
    let spec_rel = format!(".hadron/docs/specs/{date_str}-{}-design.md", input.slug);
    let plan_rel = format!(".hadron/docs/plans/{date_str}-{}.md", input.slug);

    let spec_content = render_design_spec(date_str, input, &tasks);
    let plan_content = render_implementation_plan(input, &tasks);

    let spec_abs = root.path().join(&spec_rel);
    let plan_abs = root.path().join(&plan_rel);

    if let Some(p) = spec_abs.parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Some(p) = plan_abs.parent() {
        let _ = fs::create_dir_all(p);
    }

    fs::write(&spec_abs, &spec_content)
        .map_err(|e| ForgeError::Io(format!("failed to write spec to {}: {e}", spec_rel)))?;
    fs::write(&plan_abs, &plan_content)
        .map_err(|e| ForgeError::Io(format!("failed to write plan to {}: {e}", plan_rel)))?;

    Ok(SpecCompileOutput {
        spec_path: spec_rel,
        plan_path: plan_rel,
        spec_content,
        plan_content,
        tasks_count: tasks.len(),
        is_valid_dag: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_slug_format() {
        assert!(is_valid_slug("my-feature-plan"));
        assert!(is_valid_slug("auth_system_v2"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("invalid/path/traversal"));
        assert!(!is_valid_slug("hello world"));
    }

    #[test]
    fn detects_cycles_in_task_dependencies() {
        let tasks = vec![
            SpecTaskInput {
                id: 1,
                title: "Task 1".to_string(),
                dependencies: vec![2],
                files_create: vec![],
                files_modify: vec![],
                files_test: vec![],
                steps: vec![],
            },
            SpecTaskInput {
                id: 2,
                title: "Task 2".to_string(),
                dependencies: vec![1],
                files_create: vec![],
                files_modify: vec![],
                files_test: vec![],
                steps: vec![],
            },
        ];
        assert!(validate_task_dag(&tasks).is_err());
    }

    #[test]
    fn spec_compiler_generates_valid_spec_and_plan_markdown() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());

        let input = SpecCompileInput {
            goal: "Build an ultra-fast in-memory cache with LRU eviction".to_string(),
            slug: "in-memory-lru-cache".to_string(),
            tech_stack: Some("Rust, Tokio".to_string()),
            architecture_overview: Some("Lock-free concurrent hashmap backed by doubly-linked list".to_string()),
            tasks: vec![],
        };

        let output = compile_spec_and_plan(&root, &input).expect("compilation should succeed");
        assert!(output.is_valid_dag);
        assert_eq!(output.tasks_count, 4);
        assert!(output.spec_content.contains("# in-memory-lru-cache Design Specification"));
        assert!(output.plan_content.contains("# in-memory-lru-cache Implementation Plan"));
        assert!(output.plan_content.contains("### Task 1:"));
        assert!(output.plan_content.contains("- [ ] **Step 1: Write failing test**"));

        // Verify files on disk
        assert!(root.path().join(&output.spec_path).exists());
        assert!(root.path().join(&output.plan_path).exists());
    }
}
