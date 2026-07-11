# Hadron Phase 5 (Invariants) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modularize the invariants methodology by introducing a `standard_model.md` and custom rulesets, injected deterministically by the Gluon engine to maximize provider prompt-caching.

**Architecture:** We add a `Kind::Assign` event to the lattice. The `Projection` receives an `available_invariants` list. When exciting a quark, the Gluon engine reads `standard_model.md` and any requested rule files from `.hadron/nucleus/invariants/`, concats them, and injects them into the projection.

**Tech Stack:** Rust (edition 2021)

## Global Constraints

- Rust edition: 2021. Use latest stable Rust.
- Field is append-only, never rewritten. Every writer only appends whole lines. History is immutable.
- Readers must tolerate unknown kind values.
- Vocabulary (use these exact names): quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, standard model.

---

### Task 1: Add `Kind::Assign` Event

**Files:**
- Modify: `crates/hadron-lattice/src/event.rs`
- Modify: `crates/hadron-chamber/src/model.rs`

**Interfaces:**
- Produces: `Kind::Assign { task: String, invariants: Vec<String> }` in `hadron_lattice::Kind`.
- Produces: Rendered UI row for `Assign` in `hadron-chamber`.

- [ ] **Step 1: Add `Kind::Assign`**
In `crates/hadron-lattice/src/event.rs`, add the `Assign` variant to `Kind`:
```rust
    Command { cmd: String, exit: i32, out_summary: String },
    Snapshot { git: String, label: String },
    EnergyReport { used_tokens: u32 },
    Assign { task: String, invariants: Vec<String> },
    /// Any kind this version does not understand...
```
Update the `Serialize`/`Deserialize` implementations (if manual) or ensure it maps to `{"kind": "assign", "task": "...", "invariants": [...]}`.

- [ ] **Step 2: Add `Assign` handling to `hadron-chamber`**
In `crates/hadron-chamber/src/model.rs`, update the `render_row` match statement (around line 58):
```rust
        Kind::EnergyReport { used_tokens } => (format!("used {used_tokens} tokens"), "energy_report"),
        Kind::Assign { task, invariants } => (format!("assigned: {task} (invariants: {:?})", invariants), "assign"),
        Kind::Unknown { kind, .. } => (format!("unrecognized event: {kind}"), "unrecognized"),
```

- [ ] **Step 3: Run tests**
Run: `cargo test -p hadron-lattice && cargo test -p hadron-chamber`
Expected: PASS.

- [ ] **Step 4: Commit**
```bash
git add crates/hadron-lattice/src/event.rs crates/hadron-chamber/src/model.rs
git commit -m "feat(lattice): add Kind::Assign event"
```

---

### Task 2: Add `available_invariants` to Projection

**Files:**
- Modify: `crates/hadron-lattice/src/projection.rs`

**Interfaces:**
- Produces: `available_invariants: Vec<String>` field in `Projection`.

- [ ] **Step 1: Update Projection Struct**
In `crates/hadron-lattice/src/projection.rs`, add the field:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projection {
    pub task: String,
    pub invariants: String,
    #[serde(default)]
    pub available_invariants: Vec<String>,
    pub nucleus_digest: String,
// ... existing fields
```
Update the `Projection::default()` implementation or tests if they construct `Projection` manually to include `available_invariants: vec![]`.

- [ ] **Step 2: Run tests**
Run: `cargo test -p hadron-lattice`
Expected: PASS.

- [ ] **Step 3: Commit**
```bash
git add crates/hadron-lattice/src/projection.rs
git commit -m "feat(lattice): add available_invariants to Projection"
```

---

### Task 3: Gluon Engine Invariants Injection

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs`

**Interfaces:**
- Consumes: `.hadron/nucleus/invariants/*.md` files.
- Produces: Populated `invariants` and `available_invariants` strings in the `Projection` built by the engine.

- [ ] **Step 1: Implement Invariants Reader**
In `crates/hadron-gluon/src/engine.rs`, create a helper function that reads `.hadron/nucleus/invariants/`:
```rust
use std::fs;

fn build_invariants(workspace_root: &std::path::Path, requested: &[String]) -> (String, Vec<String>) {
    let mut combined = String::new();
    let invariants_dir = workspace_root.join(".hadron").join("nucleus").join("invariants");
    let mut available = Vec::new();
    
    // Always include standard_model.md if it exists
    let sm_path = invariants_dir.join("standard_model.md");
    if sm_path.exists() {
        if let Ok(content) = fs::read_to_string(&sm_path) {
            combined.push_str(&content);
            combined.push('\n');
        }
    }

    if invariants_dir.exists() {
        if let Ok(entries) = fs::read_dir(&invariants_dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.ends_with(".md") && name != "standard_model.md" {
                        available.push(name.trim_end_matches(".md").to_string());
                    }
                }
            }
        }
    }
    
    available.sort();
    
    // Sort requested invariants to ensure deterministic cache hits
    let mut requested_sorted = requested.to_vec();
    requested_sorted.sort();

    for req in requested_sorted {
        let req_path = invariants_dir.join(format!("{}.md", req));
        if req_path.exists() {
            if let Ok(content) = fs::read_to_string(&req_path) {
                combined.push_str(&format!("\n# Rule: {}\n", req));
                combined.push_str(&content);
                combined.push('\n');
            }
        }
    }

    (combined.trim().to_string(), available)
}
```

- [ ] **Step 2: Wire helper into projection building**
In `crates/hadron-gluon/src/engine.rs`, when building the `Projection` (inside `run_until_quiesce` just before calling `excite`), use the helper. Note: you need to get the `requested` invariants if the triggering event was an `Assign`. 
```rust
            let mut requested_invariants = vec![];
            let mut task_desc = String::new();
            
            // Find the most recent event targeting this quark to get its task context
            if let Some(trigger) = events.iter().rev().find(|e| e.to.as_ref() == Some(&target)) {
                match &trigger.kind {
                    hadron_lattice::Kind::Assign { task, invariants } => {
                        task_desc = task.clone();
                        requested_invariants = invariants.clone();
                    }
                    hadron_lattice::Kind::Message { body } => {
                        task_desc = body.clone();
                    }
                    _ => {}
                }
            }
            
            let workspace_root = self.field_path.parent().unwrap().parent().unwrap();
            let (invariants_text, available_invariants) = build_invariants(workspace_root, &requested_invariants);
```
Pass these into the `Projection` struct:
```rust
            let projection = hadron_lattice::Projection {
                task: task_desc,
                invariants: invariants_text,
                available_invariants,
                nucleus_digest: "".to_string(), // keep existing behavior
                roster: roster_cards,
                field_window: events.clone(),
                git_diff: working_diff,
            };
```

- [ ] **Step 3: Update Engine Tests**
In `crates/hadron-gluon/src/engine.rs` tests, ensure tests pass. They might use a temp dir.

- [ ] **Step 4: Run tests**
Run: `cargo test -p hadron-gluon engine::`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/hadron-gluon/src/engine.rs
git commit -m "feat(gluon): deterministic invariant injection for prompt caching"
```

---

### Task 4: Vocabulary Update (README.md)

**Files:**
- Modify: `README.md` (project root)

- [ ] **Step 1: Update README.md**
Add a section detailing the physical metaphors used in Hadron:
```markdown
## The Vocabulary

Hadron uses particle physics as a metaphor for its architecture. This creates a cohesive, single-source-of-truth vernacular.

| Term | Meaning in Hadron | Physics Metaphor |
|---|---|---|
| **Hadron** | The whole environment/studio | A composite particle that binds quarks |
| **Quark** | An agent or citizen (e.g., Claude, Antigravity) | The fundamental unit of intelligence |
| **Field** | The shared append-only bus (`field.jsonl`) | Particles interact through fields |
| **Event** | One line in the field | A detected particle interaction |
| **Gluon** | The headless daemon (`hadron-gluon`) | The force carrier that binds quarks |
| **Lattice** | The shared protocol/schema crate | The framework of quark interactions |
| **Chamber** | The GPUI viewer / chat app | A bubble chamber, where tracks are observed |
| **Nucleus** | Persistent per-project SSOT knowledge | The dense stable core quarks orbit |
| **Flavor** | A quark's role (Orchestrator, Worker) | Quark flavors (up, down, charm...) |
| **Energy** | Token / cost budget tracking | Running a quark costs energy |
| **Excite** | Waking a sleeping quark to run | Exciting a field produces a particle |
| **Standard Model** | Base invariants (`standard_model.md`) | The baseline laws of physics |
```

- [ ] **Step 2: Commit**
```bash
git add README.md
git commit -m "docs: add vocabulary to README"
```
