# Design Spec: Unified Permissions Gating & Extensibility System

## 1. Goals
This document specifies the unified design for redesigning permissions gating under a global "No Human Mode" (Active Orchestrator Escalation) and enabling custom skills and agent persona extensions with tool and role-routing boundaries.

---

## 2. Redesigned Permissions Gating (No Human Mode)

Under the current system, when a command violates permission rules, the engine pauses the turn and waits for human input in the UI. In "No Human Mode" (orchestrator as the trust authority), permissions gating redirects to the Orchestrator for automated active adjudication.

### Global Bypass & Worker Clamping
*   When global mode resolves to `Bypass`, a worker's resolved mode defaults to `Auto` rather than inheriting the bypass status directly (unless the worker has an explicit per-quark override). This keeps the orchestrator in `Bypass` while workers are monitored under `Auto` mode.

### The Double-Table Check
We introduce a two-tier checking logic using two lists:
1.  **Global Allow-list:** Command prefixes or patterns explicitly permitted without human/orchestrator intervention.
2.  **Per-Quark Deny-list:** Specific command prefixes explicitly prohibited for a particular quark.

#### Resolution Flow:
1.  **Check Global Allow-list:** If the command does *not* match any entry in the allow-list, return `Decision::AskOrchestrator`.
2.  **Check Per-Quark Deny-list:** If the command matches any entry in the quark's deny-list, return `Decision::AskOrchestrator`.
3.  **Otherwise:** Return `Decision::AutoApprove`.

#### Invariant: Deny Wins
If a command matches both an allowed condition and a per-quark deny condition, it is denied/escalated (`AskOrchestrator`).

```
Command Requested
      │
      ▼
Matches Global Allow-list? ── No ──► [AskOrchestrator]
      │ Yes
      ▼
Matches Per-Quark Deny-list? ── Yes ──► [AskOrchestrator]
      │ No
      ▼
[AutoApprove]
```

### Escalation Loop via Orchestrator Adjudication
*   We add a new variant `Decision::AskOrchestrator` to the `Decision` enum in the `hadron-gatekeeper` crate.
*   When `decide()` returns `AskOrchestrator`, the `hadron-gluon` engine suspends the worker's active turn.
*   Instead of waiting for a UI human action, the engine generates an active LLM turn for the Orchestrator quark (`@cli-agy`).
*   The Orchestrator receives details of the suspended turn, the command, and its arguments.
*   The Orchestrator reviews the context and appends a `PermissionGrant` event to the event stream, either granting or denying the request.
*   Once the event is appended, the engine resumes the worker turn.

---

## 3. Extensible Custom Skills & Tool Gating

We support loading custom `.md` skills at runtime from global and local project directories, merging them with built-in skills and enforcing tool limits.

### Directory Loading
*   Skills are loaded from:
    1.  Global directory: `~/.hadron/skills/*.md`
    2.  Local repo directory: `.hadron/skills/*.md`
*   **Merger & Priority:** The engine loads all files and merges them. A skill file in the local repo directory overrides a global or compile-time built-in skill of the same name.

### Engine-Level Tool Gating
Rather than relying on model compliance, the engine dynamically registers and filters tools based on the active skill's front-matter configuration.
*   A skill `.md` YAML front-matter can declare allowed tools:
    ```yaml
    ---
    name: reviewing-work
    tools: [read_file, grep_search]
    ---
    ```
*   **Enforcement:** During the execution of a turn under a given skill, the engine only registers/exposes the tools listed under `tools`. If `run_command` is not listed, the model has no way to call it.
*   If `tools` is omitted, the quark defaults to the seat's normal tool access profile.

### Custom Script Tools
Skills can declare custom script helpers (`.py` or `.rs` files) as first-class tools:
```yaml
---
name: reviewing-work
tools:
  - read_file
  - grep_search
  - run_linter: ".hadron/skills/scripts/linter.py"
  - run_checker: ".hadron/skills/scripts/checker.rs"
---
```
*   **Synthesis:** The engine parses the custom tool mapping and registers `run_linter` and `run_checker` as first-class tools for the turn.
*   **Execution Sandbox:** When the model invokes a custom tool, the engine executes the underlying script in the sandbox using the matching environment (e.g., `python3` for `.py`, compiling and executing for `.rs` via `rustc`/`cargo`) and returns its output. General arbitrary command execution remains disabled.

---

## 4. Extensible Custom Agents & Role Routing

We support loading custom agent personas and matching tasks to specialized quarks using roles.

### Personas (`.hadron/agents/`)
*   Personas are loaded as `.md` files from:
    1.  Global: `~/.hadron/agents/*.md`
    2.  Local: `.hadron/agents/*.md`
*   They specify instructions and preferred routing roles in their front-matter:
    ```yaml
    ---
    name: security-reviewer
    preferred_role: security
    ---
    ```

### Quark Roles in `team.json`
Roster seats inside `team.json` can declare roles and exclusivity parameters:
```json
{
  "id": "acp-claude-security",
  "roles": ["security"],
  "exclusive": false
}
```
*   `roles`: A list of strings defining what functions the quark is suited for (e.g., `["architect"]`, `["security"]`).
*   `exclusive`: A boolean indicating if this quark is restricted *only* to tasks matching its roles.

### Routing Phases
*   **Phase 1 (Soft Preference Routing):** When a task prefers a role (e.g., `security`), the engine routes to an enabled quark carrying that role. If none are enabled, the engine falls back to general worker quarks.
*   **Phase 2 (Strict Exclusivity Routing):** If a quark is marked `exclusive: true` for a role (e.g., `video-editor`), the engine filters it out entirely for any task that does not match that role. If a task requires a role but no matching exclusive quark is available, the engine reports the routing failure back to the Orchestrator or human rather than stalling silently.

---

## 5. Proposed Implementation Files

*   `crates/hadron-gatekeeper/src/matrix.rs`:
    *   Add `Decision::AskOrchestrator` enum variant.
    *   Implement allow-list and deny-list parsing and double-table checking logic in `decide()`.
*   `crates/hadron-gluon/src/skills.rs`:
    *   Implement runtime filesystem traversal for local and global skill paths.
    *   Implement YAML parsing of front-matter `tools` config.
    *   Implement dynamic tool-filtering registration in the turn loop.
*   `crates/hadron-gluon/src/engine.rs`:
    *   Implement the `Decision::AskOrchestrator` suspension loop.
    *   Implement the Orchestrator adjudication active turn insertion.
*   `crates/hadron-lattice/src/team.rs`:
    *   Add `roles` and `exclusive` fields to roster serialization/deserialization.
*   `crates/hadron-gluon/src/router.rs`:
    *   Implement the soft-preference role matching and exclusive seat filtering.
