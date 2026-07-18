# Custom skills/personas loading (§3.1-3.2, §4 personas) + custom script tools (§3.3) — spec — **DESIGN ONLY**

**Status:** design. §2 (skills/persona loading) is implementation-ready (a plan). **§3.3 custom script tools is spec + questions ONLY — a firm NO-BUILD (see the hard line below).**
**Date:** 2026-07-18
**Source:** §3 + §4-personas of `docs/superpowers/specs/2026-07-17-permissions-and-extensibility-design.md`
**Branch:** `feat/permissions-gating` (docs)

---

## Part A — Custom skills + persona loading (implementation-ready)

### Grounding
`crates/hadron-gluon/src/skills.rs` today ships a compile-time skill set: `index()` (the always-on menu), `render(match, self_id, handoff, include_body)`, `select(task)` (trigger matching). WS4§5 already trimmed injection to `index()` + the active skill's body for every quark. There is NO runtime `.md` loading yet.

### Design
Load custom `.md` skills (and personas) from disk at daemon start / roster reload, merging with the built-ins:
- **Skills:** `~/.hadron/skills/*.md` (global) + `.hadron/skills/*.md` (repo). Repo overrides global overrides built-in **by `name`** (YAML front-matter `name:`). Parse front-matter for `name`, `description`/summary (feeds `index()`), and `tools:` (see gating). The loaded set flows into `select()`/`index()`/`render()` exactly as the built-ins do — one merged corpus keyed by name.
- **Personas** (`~/.hadron/agents/*.md`, `.hadron/agents/*.md`): front-matter `name` + `preferred_role`. A persona is an addressable identity with instructions + a routing role — this is the layer WS4§4 (role routing) deferred. A persona's `preferred_role` feeds the same role-routing the seat `roles` do (a persona assigned to a seat contributes its role; or `@persona-name` routes to a seat carrying `preferred_role`). Reuse WS4§4's `roles`/`task_names_card_specifically` machinery.

### Engine-level tool gating (spec §3.2)
A skill's front-matter `tools: [read_file, grep_search]` bounds what the quark may do while that skill is active:
- **SDK quarks (registry filtering):** expose only the listed tools in the tool definitions/prompt context (needs the SDK adapter — sub-project #3, not built).
- **ACP/CLI quarks (approval gating):** Hadron doesn't own the external agent's tool registry, so enforce at permission-request time: in `acp.rs`'s `on_receive_request` (`RequestPermissionRequest`), if the requested tool (`req.tool_call.fields.kind`/`raw_input`) isn't permitted under the active skill, auto-respond `RejectOnce` (or escalate to the §2 `AskOrchestrator`/`AskHuman`). **This ties into the gatekeeper (spec §2) — build that first, then hang tool-gating off the same escalation.**

### Plan (skills/personas loading — buildable, verifiable)
1. `skills::load_dir(path) -> Vec<Skill>` — parse `.md` + YAML front-matter (a front-matter parser exists? check; else a minimal `---`-delimited split). Pure, tested against fixture `.md` files in a tempdir.
2. Merge order (built-in < global < repo) by `name`; tested.
3. Wire the merged set into `select`/`index`/`render` (replace the compile-time-only set with built-ins + loaded). Tempdir-based tests; no network, fully headless.
4. Persona loading mirrors skills; `preferred_role` feeds WS4§4 routing.
5. Tool-gating for ACP/CLI: defer until the §2 gatekeeper lands (it reuses the escalation path).

### Security note
Loading `.md` from `~/.hadron` and `.hadron` is reading **local files the human controls** — same trust as `team.json`. No execution. `tools:` gating is a *restriction*. The one caveat: a repo `.hadron/skills/*.md` overriding a built-in skill's *instructions* could change quark behavior — but it's the repo owner's own file. No new external surface.

---

## Part B — Custom script tools (§3.3) — **HARD NO-BUILD LINE**

### The firm line (why this is spec-only)
Spec §3.3 lets a skill declare `run_linter: ".hadron/skills/scripts/linter.py"` / `.rs` custom tools that the engine **compiles and executes** (`rustc`/`cargo`/`python3`) in the current workspace, "run-gated" rather than sandboxed. **This is the one thing in the entire permissions/extensibility spec that cannot be built or even TESTED unattended: a test of it executes arbitrary attacker-shaped code in the real tree.** Per the advisor and Rule 7, I will not implement, wire, or test this in an autonomous session. It needs the user present.

### Design (for the user to build, deliberately)
- Front-matter `tools:` entries with a `name: path` map register `name` as a first-class tool for the turn; invoking it executes the script.
- **Execution model — the core decision:** the spec says "run-gated script execution... compiling and executing via rustc/cargo or python3 within the current workspace directory" because "quarks-share-the-tree" (no containerization). This means a custom tool runs with the **daemon's full privileges in the shared checkout** — it can read secrets, write any file, run any command, hit the network. That is effectively arbitrary code execution granted to whatever authored the skill `.md`.

### DESIGN QUESTIONS FOR THE USER (must answer before ANY implementation)
1. **Provenance/trust:** who may author a script-tool skill? A repo `.hadron/skills/scripts/*.py` is committed by whoever has repo write — is that your trust boundary, or must script tools be global-only (`~/.hadron`, i.e. only you)? A malicious/compromised repo could ship a `.py` that exfiltrates on first quark turn.
2. **No sandbox — accepted?** The spec explicitly chooses no containerization ("quarks-share-the-tree"). Is running these with daemon privileges acceptable, or should they be sandboxed (a restricted subprocess: no network, read-only outside a scratch dir, resource limits, a syscall filter)? This is the single biggest safety decision in the whole spec.
3. **Gating:** should invoking a custom script tool go through the §2 gatekeeper (ask orchestrator/human before first run), or is declaring it in a loaded skill implicit authorization?
4. **Compilation of `.rs` via `rustc`/`cargo`:** compiling arbitrary Rust in the tree is heavier and riskier than running a `.py` — is `.rs` support worth it, or start `.py`-only (still dangerous, but simpler)?
5. **Which threat model:** is Hadron trusted-single-user (you run everything, scripts are your own) or multi-party (quarks/repos you don't fully trust)? The whole answer changes on this.

**Recommendation:** treat script-tools as OFF by default, global-authored-only, and — if built at all — behind an explicit per-run gatekeeper approval, until a sandbox story exists. But this is your call to make awake.
