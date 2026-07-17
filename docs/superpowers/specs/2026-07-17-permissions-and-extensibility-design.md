# Design Spec: Unified Permissions Gating & Extensibility System

## 1. Goals
This document specifies the unified design for permissions gating under a global "No Human Mode" (Active Orchestrator Adjudication) and custom skills and agent persona extensions with tool and role-routing boundaries.

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
The gatekeeper's `decide()` function must accept the global resolved mode (`global_mode: Mode`) to determine the target escalation actor:
1.  **Check Resolved Worker Mode:** If the worker's resolved mode is `Bypass`, return `Decision::AutoApprove` immediately.
2.  **Check Global Allow-list:** If the command does *not* match any entry in the allow-list, determine escalation target:
    *   If `global_mode` is `Bypass` (No-Human mode), return `Decision::AskOrchestrator`.
    *   Otherwise (human mode), return `Decision::AskHuman`.
3.  **Check Per-Quark Deny-list:** If the command matches any entry in the quark's deny-list, return the matching escalation target (`Decision::AskOrchestrator` or `Decision::AskHuman`).
4.  **Otherwise:** Return `Decision::AutoApprove`.

#### Invariant: Deny Wins
If a command matches both an allowed condition and a per-quark deny condition, it is denied and escalated.

```
Command Requested
      │
      ▼
Matches Global Allow-list? ── No ──► Global Bypass? ── Yes ──► [AskOrchestrator]
      │ Yes                                          └── No ───► [AskHuman]
      ▼
Matches Per-Quark Deny-list? ── Yes ──► Global Bypass? ── Yes ──► [AskOrchestrator]
      │ No                                           └── No ───► [AskHuman]
      ▼
[AutoApprove]
```

### Escalation Loop via Orchestrator Adjudication
*   We add a new variant `Decision::AskOrchestrator` to the `Decision` enum in the `hadron-gatekeeper` crate.
*   We introduce a new actor type `Actor::Orchestrator` or `Actor::Quark(QuarkId)` for grants.
*   **Suspension Loop (bin/hadron-gluon.rs):**
    1.  When `decide()` returns `AskOrchestrator`, the worker turn is suspended, and the engine appends a `PermissionRequest` event.
    2.  The seat's state is set to `Paused(WaitingForOrchestrator)`.
    3.  During the engine's main daemon loop (`bin/hadron-gluon.rs`), when the swarm quiesces, the engine checks for any worker seats paused waiting for the orchestrator.
    4.  If present, it schedules a turn for the orchestrator quark (`@cli-agy`), injecting the pending `PermissionRequest` event details into its prompt.
    5.  The orchestrator reviews the context and appends a `PermissionGrant` or `PermissionDenial` event.
    6.  At the next engine step, the worker seat resumes.

---

## 3. Extensible Custom Skills & Tool Gating

We support loading custom `.md` skills at runtime from global and local project directories, merging them with built-in skills and enforcing tool limits.

### Directory Loading
*   Skills are loaded from:
    1.  Global directory: `~/.hadron/skills/*.md`
    2.  Local repo directory: `.hadron/skills/*.md`
*   **Merger & Priority:** The engine loads all files and merges them. A skill file in the local repo directory overrides a global or compile-time built-in skill of the same name.

### Engine-Level Tool Gating
Rather than relying on model compliance, the engine enforces tool boundaries.
*   A skill `.md` YAML front-matter can declare allowed tools:
    ```yaml
    ---
    name: reviewing-work
    tools: [read_file, grep_search]
    ---
    ```
*   **Enforcement for SDK Quarks (Registry Filtering):** The engine only registers/exposes the tools listed under `tools` in the tool definition/prompt context.
*   **Enforcement for ACP/CLI Quarks (Approval Gating):** Because Hadron does not control the external agent's internal tool registry, tool constraints are enforced at permission request time (in `on_receive_request` on `RequestPermissionRequest` in `acp.rs`). If the requested tool (identified by `req.tool_call.fields.kind` or `raw_input`) is not permitted under the active skill, the engine automatically responds with `PermissionOptionKind::RejectOnce` or escalates to `AskOrchestrator` / `AskHuman`.

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
*   **Execution Sandbox:** When the model invokes a custom tool, the engine executes the underlying script. Because "quarks-share-the-tree", execution sandbox behaves as a run-gated script execution path rather than containerized isolation (compiling and executing via `rustc`/`cargo` or `python3` within the current workspace directory).

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
Roster seats inside `team.json` can declare roles and exclusivity parameters (separate from the `Flavor` authority axis):
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
*   **Phase 1 (Soft Preference Routing via Mentions):** In `router.rs`, mentions like `@architect` or `@security-reviewer` are mapped to seats in `team.json` carrying the matching role. If a seat matches, the router prefers it. If none are enabled, the engine falls back to general worker quarks.
*   **Phase 2 (Strict Exclusivity Routing):** If a quark is marked `exclusive: true` for a role, the engine filters it out entirely for any task that does not match that role. If a task requires a role but no matching exclusive quark is available, the engine reports the routing failure back to the Orchestrator or human rather than stalling.

---

## 5. Prompt Bloat Optimization (Trimming the Skill Library)

### The Problem
Currently, for resident (ACP) quarks, the engine appends the entire skill corpus (`skills::corpus()`, all 15 skills verbatim) to the prompt context on every excitation. While intended to cache the skill list, re-sending all 15 markdown procedures verbatim on every excitation results in ~70-80k tokens of bloat per turn. This wastes prompt context, slows down model responses, and speeds up compaction/truncation cycles.

### The Solution
*   **Decommission `skills::corpus()`:** The engine will no longer inject the full library of skill text into resident (ACP) quark prompts.
*   **Targeted Injection:** For both CLI (one-shot) and ACP (resident) quarks, the engine will only append:
    1.  `skills::index()` (the brief bulleted index/list of available skills and their summaries).
    2.  The full body of the **active starting skill** (rendered via `skills::render(..., include_body = true)`).
*   **Result:** Wastes only ~4-5k tokens of skill overhead per turn, cutting down excitation bloat by ~90% and preserving the context window for actual field history and uncommitted diffs.

---

## 6. Proposed Implementation Files

*   `crates/hadron-gatekeeper/src/matrix.rs`:
    *   Add `Decision::AskOrchestrator` enum variant.
    *   Implement allow-list and deny-list parsing and double-table checking logic in `decide()`.
*   `crates/hadron-gluon/src/skills.rs`:
    *   Implement runtime filesystem traversal for local and global skill paths.
    *   Implement YAML parsing of front-matter `tools` config.
*   `crates/hadron-gluon/src/adapter/acp.rs`:
    *   Implement tool approval gating on `session/request_permission` matching skill constraints.
*   `bin/hadron-gluon.rs`:
    *   Implement the `Decision::AskOrchestrator` suspension loop and orchestrator active turn insertion inside the main daemon loop.
*   `crates/hadron-lattice/src/team.rs`:
    *   Add `roles` and `exclusive` fields to roster serialization/deserialization.
*   `crates/hadron-gluon/src/router.rs`:
    *   Implement soft-preference role matching and exclusive seat filtering based on role-mentions.
*   `crates/hadron-gluon/src/engine.rs`:
    *   Remove `skills::corpus()` injection from `invariants_text` and enforce `include_body = true` in `skills::render` for all turn types.


