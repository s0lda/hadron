# Custom Skill Slash Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable custom skill slash commands (such as `/joke`) to function correctly with default name/slash triggers, zero-argument invocations, and leading `@mention` prefixes in Hadron Chamber.

**Architecture:** Update skill parsing in `hadron-gluon` to default omitted triggers to `[id, /id]`, update `skill_command_body` in `hadron-chamber` to produce valid prompt strings when task text is empty, and update `split_leading_commands` in `hadron-chamber` to handle leading `@mention` prefixes before slash commands.

**Tech Stack:** Rust (hadron-gluon, hadron-chamber).

## Global Constraints
- All changes must pass existing unit tests across workspace.
- Maintain Standard Model Invariants (Rule 1, Rule 3, Rule 6, Rule 9, Rule 11).

---

### Task 1: Default Triggers for Custom Skills in `hadron-gluon`

**Files:**
- Modify: `crates/hadron-gluon/src/skills/parse.rs:84-90`
- Test: `crates/hadron-gluon/src/skills/tests.rs`

**Interfaces:**
- Consumes: Skill front-matter parser in `parse_skill_file`
- Produces: `ResolvedSkill` with default `triggers: [id, /id]` when `triggers:` is omitted or empty

- [ ] **Step 1: Write failing test**
Add `custom_skill_omitting_triggers_defaults_to_name_and_slash_command` test in `crates/hadron-gluon/src/skills/tests.rs`.

- [ ] **Step 2: Run test to verify it fails**
Run `cargo test -p hadron-gluon --lib custom_skill_omitting_triggers_defaults`

- [ ] **Step 3: Implement default triggers in `parse.rs`**
Update `parse_skill_file` in `crates/hadron-gluon/src/skills/parse.rs` to add `[id, /id]` when `triggers` is empty.

- [ ] **Step 4: Run test to verify it passes**
Run `cargo test -p hadron-gluon --lib`

- [ ] **Step 5: Commit**
`git commit -m "fix(gluon): default skill triggers to name and slash command when omitted"`


### Task 2: Allow Zero-Argument Skill Commands in `hadron-chamber`

**Files:**
- Modify: `crates/hadron-chamber/src/text.rs:930-936`
- Test: `crates/hadron-chamber/src/text.rs:1680-1695`

**Interfaces:**
- Consumes: `skill_command_body(trigger, target, task)`
- Produces: `Some("@{target} Let's {trigger}")` when `task` is empty

- [ ] **Step 1: Update test assertions for zero-argument skill commands**
Update unit tests in `crates/hadron-chamber/src/text.rs`.

- [ ] **Step 2: Run test to verify it fails**
Run `cargo test -p hadron --lib skill_command_body`

- [ ] **Step 3: Update `skill_command_body` implementation**
Update `skill_command_body` in `crates/hadron-chamber/src/text.rs` to return `Some(format!("@{target} Let's {trigger}"))` when `task.is_empty()`.

- [ ] **Step 4: Run test to verify it passes**
Run `cargo test -p hadron --lib skill_command_body`

- [ ] **Step 5: Commit**
`git commit -m "fix(chamber): allow zero-argument skill slash commands"`


### Task 3: Support Mention Prefixes for Slash Commands in `split_leading_commands`

**Files:**
- Modify: `crates/hadron-chamber/src/app/input.rs:313-335`
- Test: `crates/hadron-chamber/src/app/input.rs`

**Interfaces:**
- Consumes: `split_leading_commands(full)`
- Produces: Parsed command list with target mention attached to `args` when command is preceded by `@mention`

- [ ] **Step 1: Write failing test for `@target /command`**
Add unit test in `crates/hadron-chamber/src/app/input.rs` for `split_leading_commands`.

- [ ] **Step 2: Run test to verify it fails**
Run `cargo test -p hadron --lib split_leading_commands_handles_leading_mentions`

- [ ] **Step 3: Implement mention stripping in `split_leading_commands`**
Update `split_leading_commands` in `crates/hadron-chamber/src/app/input.rs`.

- [ ] **Step 4: Run test to verify it passes**
Run `cargo test -p hadron --lib`

- [ ] **Step 5: Commit**
`git commit -m "fix(chamber): strip leading mention when parsing slash commands"`
