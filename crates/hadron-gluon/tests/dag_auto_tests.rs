use hadron_gluon::engine::dag_auto::HybridDagScheduler;
use hadron_lattice::QuarkId;

#[test]
fn test_hybrid_dag_parsing_and_auto_dispatch() {
    let markdown = r#"
# Phase 1: Test Plan

### Task 1.1: Setup Core Types
**Files:**
- Create: `src/core.rs`
- Test: `tests/core_tests.rs`

- [ ] **Step 1: Write tests**
- [ ] **Step 2: Implement core**

---

### Task 1.2: Build Service Layer
**Files:**
- Create: `src/service.rs`
**Interfaces:**
- Consumes: task-1.1

- [ ] **Step 1: Implement service**

---

### Task 1.3: Build Worker Client
**Files:**
- Create: `src/client.rs`
**Interfaces:**
- Consumes: task-1.1

- [ ] **Step 1: Implement client**

---

### Task 1.4: Integration Tests
**Files:**
- Modify: `tests/integration.rs`
**Interfaces:**
- Consumes: task-1.2, task-1.3

- [ ] **Step 1: Verify all**
"#;

    let mut scheduler = HybridDagScheduler::parse_plan(markdown).expect("Failed to parse plan");
    assert_eq!(scheduler.total_tasks(), 4);
    assert_eq!(scheduler.completed_count(), 0);

    // Initial ready tasks: only task-1.1
    let ready = scheduler.poll_ready_tasks();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, "task-1.1");
    assert_eq!(ready[0].files_create, vec!["src/core.rs"]);

    // Assign task-1.1 to a quark
    let q1 = QuarkId::new("worker-1");
    scheduler.assign_task("task-1.1", q1.clone()).unwrap();

    // Now no tasks are unassigned & ready
    let ready_now = scheduler.poll_ready_tasks();
    assert!(ready_now.is_empty());

    // Mark task-1.1 complete -> unlocks task-1.2 and task-1.3 in parallel
    let unblocked = scheduler.mark_completed("task-1.1", "a1b2c3d");
    assert_eq!(unblocked.len(), 2);
    let mut unblocked_ids: Vec<String> = unblocked.into_iter().map(|t| t.id).collect();
    unblocked_ids.sort();
    assert_eq!(unblocked_ids, vec!["task-1.2", "task-1.3"]);

    // Assign task-1.2 to worker-1 and task-1.3 to worker-2
    let q2 = QuarkId::new("worker-2");
    scheduler.assign_task("task-1.2", q1).unwrap();
    scheduler.assign_task("task-1.3", q2).unwrap();

    // Complete task-1.2 -> task-1.4 still blocked on task-1.3
    let unblocked_2 = scheduler.mark_completed("task-1.2", "e5f6g7h");
    assert!(unblocked_2.is_empty());

    // Complete task-1.3 -> task-1.4 unblocked
    let unblocked_3 = scheduler.mark_completed("task-1.3", "i9j0k1l");
    assert_eq!(unblocked_3.len(), 1);
    assert_eq!(unblocked_3[0].id, "task-1.4");

    // Complete task-1.4 -> all done
    scheduler.mark_completed("task-1.4", "m2n3o4p");
    assert!(scheduler.is_all_completed());
    assert_eq!(scheduler.completed_count(), 4);
}
