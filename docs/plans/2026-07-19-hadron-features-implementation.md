# Hadron Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement window focus selection bugfix, No-Human-Mode adjudication, Worktree isolation & Merge Gate, Live stream UI, Budget ceilings, and Foldable Plan tab.

**Architecture:** 
- Fix focus behavior in the GPUI window mouse event listener.
- Register `/approve` and `/deny` slash commands in the Chamber and parse them as SSOT commands in both Chamber and Gluon daemon.
- Wire worktree isolation and merge gate together using `CargoMergeRunner` in `hadron-gluon`.
- Read and display live activity JSON files in the Chamber's roster.
- Configure energy/cost limits on seats and enforce them in the engine using a wired sqlite ledger.
- Group plan checklists under tasks, showing them as collapsible accordions in the Plan rail.

**Tech Stack:** Rust, GPUI, Git, Sqlite (rusqlite), Serde.

## Global Constraints

- Passing tests only prove compile, find the caller (Rule 1)
- Reuse before you create (Rule 2)
- One definition, one place (SSOT) (Rule 3)
- Defense in depth: do not remove redundant checks (Rule 4)
- Know your baseline before you touch anything (Rule 5)
- Evidence, not adjectives (Rule 6)
- Security note if touches permissions, file access, process execution (Rule 7)
- Make invalid states unrepresentable (Rule 8)

---

### Task 1: Focus Hover-Selection Bugfix

**Files:**
- Modify: `crates/gpui-component/crates/ui/src/text/window_selection.rs:786-795`

**Interfaces:**
- Consumes: GPUI `MouseDownEvent`
- Produces: None (internal click filter)

- [x] **Step 1: Implement activation click filter**
  Locate `paint` method in `crates/gpui-component/crates/ui/src/text/window_selection.rs` and update the `MouseDownEvent` listener:
  ```rust
  window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
      if event.button != MouseButton::Left {
          return;
      }
      if event.first_mouse {
          return; // Ignore clicks whose sole purpose is window activation
      }
      if phase.capture() {
          GlobalState::global_mut(cx).suppress_text_selection = false;
          Root::update(window, cx, |root, _, cx| root.clear_text_selection(cx));
      } else if event.click_count == 1 {
          if GlobalState::global(cx).suppress_text_selection {
              return;
          }
          Root::update(window, cx, |root, window, cx| {
              root.start_text_selection(event.position, window, cx);
          });
      }
  });
  ```

- [x] **Step 2: Run workspace tests to verify they pass**
  Run: `cargo test --workspace`
  Expected: PASS

- [x] **Step 3: Commit**
  ```bash
  git add crates/gpui-component/crates/ui/src/text/window_selection.rs
  git commit -m "fix(chamber): ignore first_mouse clicks for text selection to prevent sticky highlight"
  ```

---

### Task 2: Slash Commands in Chamber

**Files:**
- Modify: `crates/hadron-chamber/src/app/actions.rs:136-146`
- Modify: `crates/hadron-chamber/src/app/input.rs:190-210`
- Modify: `crates/hadron-chamber/src/text.rs:210-223`

**Interfaces:**
- Consumes: Roster information via view reload
- Produces: `/approve @worker [remember]` and `/deny @worker` commands

- [ ] **Step 1: Register autocomplete commands in text completions**
  In `crates/hadron-chamber/src/text.rs` under `completion_candidates` '`/'` match:
  ```rust
  let cmds = [
      ("clear", "Archive and clear the current chat history"),
      ("team-brainstorm", "Kick off brainstorming with the team"),
      ("reboot", "Force-restart a resident quark (e.g. /reboot @acp-claude or /reboot all)"),
      ("approve", "Approve a pending permission request (e.g. /approve @worker or /approve @worker remember)"),
      ("deny", "Deny a pending permission request (e.g. /deny @worker)"),
      ("toggle-roster", "Toggle the Roster sidebar"),
      ...
  ];
  ```

- [ ] **Step 2: Parse slash commands in split_leading_commands**
  In `crates/hadron-chamber/src/app/input.rs` in `split_leading_commands`:
  ```rust
  Some("reboot") => {
      cmds.push(("reboot".to_string(), head[tok_end..].trim().to_string()));
      return (cmds, None);
  }
  Some("approve") => {
      cmds.push(("approve".to_string(), head[tok_end..].trim().to_string()));
      return (cmds, None);
  }
  Some("deny") => {
      cmds.push(("deny".to_string(), head[tok_end..].trim().to_string()));
      return (cmds, None);
  }
  ```

- [ ] **Step 3: Handle commands in actions.rs**
  In `crates/hadron-chamber/src/app/actions.rs` in `handle_chat_command`:
  ```rust
  "approve" | "deny" => {
      let target = args.trim().trim_start_matches('@');
      let parts: Vec<&str> = target.split_whitespace().collect();
      if parts.is_empty() {
          eprintln!("chamber: `/approve` or `/deny` requires a worker target");
          return true;
      }
      let worker_name = parts[0];
      let remember = parts.get(1).map(|s| *s == "remember").unwrap_or(false);
      let approved = cmd == "approve";
      
      let real_id = self.view.roster.iter()
          .find(|r| r.id == worker_name || r.display_name.as_deref() == Some(worker_name))
          .map(|r| r.id.clone());
      
      if let Some(worker_id) = real_id {
          let ev = Event::new(
              Actor::Human,
              Some(worker_id),
              Kind::PermissionGrant { approved, remember },
          );
          if let Err(e) = io::append_event(&self.path, &ev) {
              eprintln!("chamber: failed to append permission grant: {e}");
          }
      } else {
          eprintln!("chamber: target worker not found on roster: {worker_name}");
      }
      let events = io::read_events(&self.path).unwrap_or_default();
      self.reproject(&events);
      cx.notify();
      true
  }
  ```

- [ ] **Step 4: Verify compiles & tests pass**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/hadron-chamber/src/text.rs crates/hadron-chamber/src/app/input.rs crates/hadron-chamber/src/app/actions.rs
  git commit -m "feat(chamber): add /approve and /deny slash commands for permissions"
  ```

---

### Task 3: No-Human-Mode Adjudication Loop in Gluon

**Files:**
- Modify: `crates/hadron-gluon/src/engine/turn.rs:130-225`

**Interfaces:**
- Consumes: Orchestrator `/approve` / `/deny` command replies
- Produces: Resumed worker turn via `PermissionGrant` events

- [ ] **Step 1: Implement recursion guard in decision path**
  In `crates/hadron-gluon/src/engine/turn.rs` inside `finish_turn` where `hadron_gatekeeper::decide` is called:
  ```rust
  let asker_is_orchestrator = self.orchestrator_id().as_ref() == Some(target);
  let decision = hadron_gatekeeper::decide(mode, global, self.no_human, risk, &op, target, &rules, &deny);
  let decision = if asker_is_orchestrator && decision == hadron_gatekeeper::Decision::AskOrchestrator {
      hadron_gatekeeper::Decision::AskHuman
  } else {
      decision
  };
  ```

- [ ] **Step 2: Parse orchestrator's slash commands in finish_turn**
  In `crates/hadron-gluon/src/engine/turn.rs` in `finish_turn` right before appending `Kind::Message`:
  ```rust
  if let Some(body) = outcome.message.as_ref() {
      if self.is_orchestrator(target) {
          let body_trimmed = body.trim();
          if body_trimmed.starts_with("/approve ") || body_trimmed.starts_with("/deny ") {
              let parts: Vec<&str> = body_trimmed.split_whitespace().collect();
              if parts.len() >= 2 {
                  let cmd = parts[0];
                  let worker_name = parts[1].trim_start_matches('@');
                  let remember = parts.get(2).map(|s| *s == "remember").unwrap_or(false);
                  let approved = cmd == "/approve";
                  
                  if let Some(worker_id) = self.roster.iter().find(|c| c.id.as_str() == worker_name || c.display_name.as_deref() == Some(worker_name)).map(|c| c.id.clone()) {
                      let grant_ev = Event::new(
                          Actor::Quark(target.clone()),
                          Some(worker_id),
                          Kind::PermissionGrant { approved, remember },
                      );
                      self.append(grant_ev).await?;
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 3: Run gluon tests to verify implementation**
  Run: `cargo test -p hadron-gluon`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add crates/hadron-gluon/src/engine/turn.rs
  git commit -m "feat(gluon): implement orchestrator slash command parser and recursion guard"
  ```

---

### Task 4: Worktree Isolation & Merge Gate Activation

**Files:**
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs:307-315`

**Interfaces:**
- Consumes: `hadron_lattice::workspace::repo_root_of`
- Produces: isolated worktree run + Cargo merge checks

- [ ] **Step 1: Wire repo root and merge gate into the engine**
  Update engine initialization in `crates/hadron-gluon/src/bin/hadron-gluon.rs`:
  ```rust
  let repo_root = hadron_lattice::workspace::repo_root_of(&args.field_path).to_path_buf();
  let mut engine = Engine::new(args.field_path.clone(), quarks, max_exchanges)
      .with_git(repo_root)
      .with_merge_gate(std::sync::Arc::new(hadron_gluon::merge::CargoMergeRunner))
      .with_global_skills_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("skills")))
      .with_global_agents_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("agents")));
  ```

- [ ] **Step 2: Run all tests in the workspace**
  Run: `cargo test --workspace`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add crates/hadron-gluon/src/bin/hadron-gluon.rs
  git commit -m "feat(gluon): enable worktree isolation and wire CargoMergeRunner"
  ```

---

### Task 5: Live Mid-Turn Stream UI

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/roster.rs:22-181`
- Modify: `crates/hadron-chamber/src/app/widgets.rs:174-249`

**Interfaces:**
- Consumes: Volatile `live/` directory files
- Produces: Subtitle stream updates under active worker rows

- [ ] **Step 1: Update roster_row signature**
  In `crates/hadron-chamber/src/app/widgets.rs` update `roster_row` signature:
  ```rust
  pub(super) fn roster_row(
      id: &ResolvedIdentity,
      r: &RosterRow,
      activity: Option<hadron_lattice::live::Activity>,
      controls: gpui::AnyElement,
  ) -> impl IntoElement {
  ```

- [ ] **Step 2: Render active subtitle in widgets.rs**
  In `crates/hadron-chamber/src/app/widgets.rs` replace `detail_1` parsing logic:
  ```rust
  let detail_1: SharedString = if let Some(act) = activity {
      format!("{}: {}", act.doing.label(), act.detail).into()
  } else if r.vendor.is_empty() && r.model.is_empty() {
      label.into()
  } else if r.model.is_empty() {
      format!("{} · {}", transport_label, cap(&r.vendor)).into()
  } else {
      format!("{} · {} · {}", transport_label, cap(&r.vendor), cap(&r.model)).into()
  };
  ```

- [ ] **Step 3: Read and pass live activity in roster.rs**
  In `crates/hadron-chamber/src/app/render/roster.rs` update the loop over roster rows:
  ```rust
  let live_dir = hadron_lattice::live::live_dir(&self.path);
  for (ix, r) in self.view.roster.iter().enumerate() {
      let is_selected = self.selected_quark_ix == Some(ix);
      let activity = hadron_lattice::live::read(&live_dir, &r.id, chrono::Utc::now());
      ...
      .child(roster_row(&self.resolve_identity(&r.id), r, activity, controls));
  ```

- [ ] **Step 4: Verify compile and tests**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/hadron-chamber/src/app/widgets.rs crates/hadron-chamber/src/app/render/roster.rs
  git commit -m "feat(chamber): display live stream activities under roster rows"
  ```

---

### Task 6: Budget Ceilings

**Files:**
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs:307-315`
- Modify: `crates/hadron-lattice/src/team/seat.rs:80-100` and `227-268` (Seat / SeatOverride serialization)

**Interfaces:**
- Consumes: Configured cost limits in `team.json`
- Produces: Depletion checks and execution blocks

- [ ] **Step 1: Add budget fields to seat.rs**
  Add optional `energy_limit` to `Seat` and `SeatOverride` in `crates/hadron-lattice/src/team/seat.rs`:
  In `Seat`:
  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub energy_limit: Option<u32>,
  ```
  In `SeatOverride`:
  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub energy_limit: Option<u32>,
  ```
  And update `Seat::cli` constructor with `energy_limit: None` and `SeatOverride::role` constructor with `energy_limit: None`.
  Update `resolve_team` in `crates/hadron-lattice/src/team/mod.rs` to propagate `energy_limit`:
  ```rust
  if let Some(limit) = ov.energy_limit {
      seat.energy_limit = Some(limit);
  }
  ```

- [ ] **Step 2: Wire ledger in bin/hadron-gluon.rs**
  In `crates/hadron-gluon/src/bin/hadron-gluon.rs`:
  ```rust
  let ledger_path = args.field_path.parent().unwrap_or(std::path::Path::new(".")).join("ledger.db");
  let ledger = hadron_gluon::ledger::Ledger::open(&ledger_path)?;
  let global_limit = 500_000u32;
  let mut engine = engine.with_ledger(ledger, global_limit);
  ```

- [ ] **Step 3: Parse per-quark limits in engine's is_depleted check**
  In `crates/hadron-gluon/src/engine/run.rs` in the dispatch loop:
  ```rust
  if let Some(ledger) = &self.ledger {
      // Find the limit on the seat (if custom defined), otherwise fall back to self.energy_limit
      let limit = self.roster.iter()
          .find(|c| c.id == target)
          .and_then(|c| c.energy_limit)
          .unwrap_or(self.energy_limit);
      if ledger.is_depleted(&target, limit)? {
          let msg = format!("⚠️ Quark {} is depleted (exceeded {} tokens).", target.as_str(), limit);
          self.reroute_blocked(&target, &msg).await?;
          continue;
      }
  }
  ```

- [ ] **Step 4: Run workspace tests to verify compatibility**
  Run: `cargo test --workspace`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add crates/hadron-lattice/src/team/seat.rs crates/hadron-lattice/src/team/mod.rs crates/hadron-gluon/src/bin/hadron-gluon.rs crates/hadron-gluon/src/engine/run.rs
  git commit -m "feat(gluon): wire energy ledger and support custom per-quark budget ceilings"
  ```

---

### Task 7: Foldable Plan Tab

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs:30-100` (state initialization)
- Modify: `crates/hadron-chamber/src/app/render/terminal.rs:655-720`

**Interfaces:**
- Consumes: Markdown headings parsed from plans on disk
- Produces: Collapsible plan checklist views grouped under tasks

- [ ] **Step 1: Add toggle state to Chamber struct**
  In `crates/hadron-chamber/src/app/mod.rs` inside the `Chamber` struct:
  ```rust
  pub(super) plan_collapsed_tasks: std::collections::HashSet<String>,
  ```
  And initialize it in `Chamber::new`:
  ```rust
  plan_collapsed_tasks: std::collections::HashSet::new(),
  ```

- [ ] **Step 2: Parse tasks and render accordions in terminal.rs**
  In `crates/hadron-chamber/src/app/render/terminal.rs` inside `RightRailTab::Plan`:
  Replace the rendering loop to group steps under task headings:
  - Loop through tasks and build a grouped structure: `Vec<(String, Vec<(String, bool)>)>` where the first String is the task name.
  - Render each task name as a row with a Chevron icon:
    - If `plan_collapsed_tasks` contains the task name, show `ChevronRight` and do NOT show the steps.
    - Else show `ChevronDown` and render the checklist steps below.
    - Set the click listener on the header to toggle the entry in `plan_collapsed_tasks` and call `cx.notify()`.
  - Auto-expand logic: when loading a new plan file, find the first task with an incomplete step, and ensure it is NOT in `plan_collapsed_tasks`.

- [ ] **Step 3: Run chamber tests to verify compiles**
  Run: `cargo test -p hadron-chamber --features gui`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add crates/hadron-chamber/src/app/mod.rs crates/hadron-chamber/src/app/render/terminal.rs
  git commit -m "feat(chamber): render foldable plan accordions in the Plan rail"
  ```
