# Custom Skill Slash Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable custom skill slash commands (such as `/joke`) to function correctly with default name/slash triggers, zero-argument invocations, and leading `@mention` prefixes in Hadron Chamber.

**Architecture:** Update skill parsing in `hadron-gluon` to default omitted triggers to `[id, /id]`, update `skill_command_body` in `hadron-chamber` to produce valid prompt strings when task text is empty, and update `split_leading_commands` in `hadron-chamber` to handle leading `@mention` prefixes before slash commands.

**Tech Stack:** Rust (hadron-gluon, hadron-chamber).

## Global Constraints
- All changes must pass existing unit tests across workspace.
- Maintain Standard Model Invariants (Rule 1, Rule 3, Rule 6, Rule 9, Rule 11).

---

- [x] **Task 1 (commit 93887c98)**: Default Triggers for Custom Skills in `hadron-gluon`
- [x] **Task 2 (commit a56a6d0f)**: Allow Zero-Argument Skill Commands in `hadron-chamber`
- [x] **Task 3 (commit 89c1df08)**: Support Mention Prefixes for Slash Commands in `split_leading_commands`
