# Hadron Permission Modes & Quark Legibility — Design Spec

> **Status:** authored autonomously 2026-07-12 while the user slept, under an explicit "orchestrator in bypass" delegation ("write the full spec, proceed to plans and implement… decide the best solution… write it down so we can review when I wake up"). This is the review artifact. Everything here is a decision made on the user's behalf; each non-obvious call carries a **Rationale** so it is cheap to overturn.

**Goal:** Replace the two independent god-mode toggles with a coherent **permission-mode ladder** (Ask / Write / Auto / Bypass) that is *per-quark with a global default*, learns an allow-list on first use, and is surfaced through **status-bar + roster tags** — and make each quark **legible** (which provider/model backs it) so trust decisions are informed. Also: get the toggles **out of the terminal rail** (the terminal is the full right sidebar, as before).

**Architecture:** The **field is the single source of truth** for modes and allow-rules — they are ordinary append-only events, folded by pure functions in `hadron-gatekeeper`. This makes modes live (a running daemon honours a mode change on its next tick) and persistent (re-opening a field replays its mode events) with no side config. The engine consults `gatekeeper::decide` per permission ask; the chamber renders modes as `gpui-component` `Tag`s and writes mode/grant events back to the field — the same steering bus the quarks use.

**Tech stack:** Rust workspace (lattice/gluon/gatekeeper/chamber/forge). `gpui` + `gpui-component` (`--features gui`) for the chamber. No new deps.

## Global Constraints (copied from standing project rules)

- **Zero API spend in tests.** All engine/gatekeeper tests use mock quarks/runners; no real CLI is invoked in the suite.
- **Vocabulary is fixed:** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge, watch, gatekeeper. New terms must compose from these. "Mode" is a UI-facing label for the gatekeeper's decision level; acceptable.
- **Chamber must not depend on the gluon runtime** (two-process decoupling). Chamber may depend on `hadron-lattice` and `hadron-gatekeeper` (pure), never `hadron-gluon`.
- **Additive lattice changes only** where possible (Gemini is concurrently active on `main`); hand-written `Kind` serde arms get matching serialize+deserialize.
- **TDD, pure-core + thin-seam.** Every decision function is a pure fn with exhaustive tests; GPUI is the thin untested seam.

---

## 1. What a quark is (settles the trust-granularity question)

A **quark is a seat at the table** — one identity bound to one adapter (CLI/provider) running one model. It is an *instance*, not a class:

- Same CLI, two models (Claude Code · Opus and Claude Code · Haiku) = **two quarks** with independent trust.
- Trust attaches to the **seat**, never to a `provider × model` matrix.

**Subagent principle (decided, doc-only):** the quark is the **trust boundary**. When a quark's vendor CLI spawns its own subagents, that happens inside the CLI process — Hadron's daemon only ever sees the quark's `TurnOutcome`, so it *cannot* name or gate subagents. Therefore subagents are **opaque**: you see what the quark *surfaces* (its edits, commands, permission requests), and the quark is accountable for its subtree. A quark's mode governs everything it surfaces, subagent-originated or not. Don't trust a quark to police its own subagents → give that seat a lower mode so its risky ops still gate.

**Legibility change (Q):** `QuarkCard` gains `provider: String` and `model: String` (additive; today it holds only `flavor` + `energy`). The roster shows `id · provider · model` next to the mode tag so the human's per-quark trust decision is informed.

Cross-seat handoff ("Sonnet executes Opus's plan") needs nothing new: the plan is a field `Message`/`Assign` addressed to the Sonnet seat; Sonnet executes under **Sonnet's** mode.

---

## 2. The permission-mode ladder

Two risk classes exist today: `Risk::WorkspaceEdit` and `Risk::BashExec`. The mode is a single dial on **how much permission authority the human delegates to the orchestrator**; every gated op bubbles worker → orchestrator → human and stops at the first level with authority.

| Mode | Edit | Bash — first time | Bash — repeat | First-use adjudicator |
|------|------|-------------------|---------------|-----------------------|
| **Ask** | ask you | ask you | ask you *(no memory)* | You |
| **Write** | auto | ask you | ask you *(no memory)* | You |
| **Auto** | auto | ask you | **auto (remembered)** | **You** |
| **Bypass** | auto | auto | auto | **Orchestrator** (you out of loop) |

- **Write** = you permit every command, every time (edits flow).
- **Auto** = you permit a command *once* (**Always allow**), then it is on that quark's allow-list; off-list commands still ask you.
- **Bypass** = the orchestrator owns it; the gluon auto-approves on its standing authority and you are never asked. The `permission_req` + auto-grant are still written to the field (audit trail) so you *see* it after the fact.

**Auto vs Bypass, precisely:** in Auto the human is genuinely in the loop on first use (real toast, `Always allow` learns the rule); in Bypass the human is never in the loop (gluon auto-grants). That is the "decision belongs to the orchestrator, not you" boundary.

**Rationale (why a ladder, not the two booleans):** the booleans allowed the nonsensical "bypass bash but not edits" state and had no place for a learned allow-list or an orchestrator-owned tier. The ladder is a total order that maps 1:1 to the four things the user described and collapses the odd states.

**Decided upgrade path (not built now):** Bypass currently means "gluon auto-grants attributed to the orchestrator's standing authority" — no LLM turn. A future upgrade routes the first-use `permission_req` to a real orchestrator *turn* that adjudicates; same event shapes, so it is a drop-in swap of the AutoApprove branch. Documented so we don't over-build now.

---

## 3. Data model

### 3.1 lattice (`hadron-lattice`)

**`Mode`** — new enum, lives in lattice because modes are field-event payload (like `Risk`, which already moved here):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode { #[default] Ask, Write, Auto, Bypass }
```

Default `Ask` = safest (delegate nothing).

**`Kind::ModeSet { mode: Mode }`** — new event kind. The event envelope's `to` field carries the target: `to: Some(quark)` = per-quark override, `to: None` = global default. Emitted by `Actor::Human` (from the chamber) or `Actor::Gluon`. Hand-written serde arms: tag `"mode_set"`, field `"mode"`. **Not** task-bearing → the engine's trigger-finder (already filtered to `Assign|Message`) ignores it, so a ModeSet addressed to a quark never excites a turn.

**`Kind::PermissionGrant { approved: bool, remember: bool }`** — add `remember` (serde `default` false, additive; old grants deserialize with `remember=false`). `remember=true && approved=true` teaches an allow-rule.

**`QuarkCard`** — add `pub provider: String` and `pub model: String`. Round-trip test updated.

### 3.2 gatekeeper (`hadron-gatekeeper`) — the pure decision core

**Delete** `Policy` (the two-bool interim) and the old `decide(risk, policy)`. Replace with:

```rust
pub use hadron_lattice::Mode;

/// (quark, op) pairs the human has chosen to always allow. `op` is the
/// self-declared PermissionReq description (exact match — the daemon never
/// sees the raw command on the CLI-adapter path).
pub type AllowRules = std::collections::HashSet<(QuarkId, String)>;

/// Fold ModeSet events → the effective mode for `quark`:
/// latest per-quark override wins, else latest global, else Mode::Ask.
pub fn resolve_mode(events: &[Event], quark: &QuarkId) -> Mode;

/// The effective GLOBAL default (latest to:None ModeSet, else Ask).
pub fn global_mode(events: &[Event]) -> Mode;

/// Fold remembered approvals → the allow-list. A PermissionGrant with
/// remember=true,approved=true is paired with the PermissionReq it answered
/// (grant.to == req.from) to recover (quark, description).
pub fn allow_rules(events: &[Event]) -> AllowRules;

pub enum Decision { AutoApprove, AskHuman }

/// The verdict for one self-declared op.
pub fn decide(mode: Mode, risk: Risk, op: &str, quark: &QuarkId, rules: &AllowRules) -> Decision;
```

`decide` truth table:

| mode \ risk | WorkspaceEdit | BashExec |
|-------------|---------------|----------|
| Ask | AskHuman | AskHuman |
| Write | AutoApprove | AskHuman |
| Auto | AutoApprove | `rules.contains((quark,op))` ? AutoApprove : AskHuman |
| Bypass | AutoApprove | AutoApprove |

**`gate.rs`:** `PendingPermission` unchanged. `grant(pending, approved)` stays (→ `remember=false`); add `grant_remembering(pending)` → `PermissionGrant { approved:true, remember:true }` addressed to the quark. `pending_permission` unchanged (matches any `PermissionGrant`, remember-agnostic).

### 3.3 Why field-as-SSOT (no config for modes)

**Rationale:** a field *is* durable state (the `.jsonl` on disk), so folding ModeSet events gives persistence for free and makes a running daemon honour a live mode change on its next tick — this dissolves the previously-deferred "god-mode toggles don't reach a running daemon" gap without a side channel. It is also trivially unit-testable (pure event folds). Cost: a brand-new field starts at `Ask` until modes are set — acceptable and safe.

`ChamberPrefs.policy` (the interim persisted `Policy`) is **removed**; layout/identity prefs are untouched.

---

## 4. Engine wiring (`hadron-gluon`)

- Remove the `policy` field and `with_policy`. `Engine::new(field_path, quarks, max_exchanges)` unchanged otherwise.
- In the permission hook (after appending `PermissionReq`):
  ```
  let mode  = gatekeeper::resolve_mode(&events, &target);
  let rules = gatekeeper::allow_rules(&events);
  match gatekeeper::decide(mode, risk, &op, &target, &rules) {
    AutoApprove => { append PermissionGrant{approved:true,remember:false} from Gluon to target; exchanges+=1; continue; }
    AskHuman    => { append Status{Waiting}; return Ok(()); }
  }
  ```
  (`op` = the ask's description; capture it before moving into the req event.)
- Re-read `events` after appending the req so `resolve_mode`/`allow_rules` see current state (the hook already re-reads per loop iteration; verify the local `events` binding is fresh — if not, re-read before resolving).

Grants keep flowing through `next_pending` (addressed events) — unchanged. Human `Always allow` (`remember=true`) resumes the quark now AND is folded into `allow_rules` for next time; no special engine handling.

**Tests (mock quarks, no API):** replace the 3 policy tests with mode tests — Ask pauses; Write auto-approves an edit but pauses on bash; Auto pauses on first bash then auto-approves after a remembered grant; Bypass auto-approves bash; per-quark override beats global; task survives a permission round-trip (keep the `recorded[1]=="hello"` assertion).

---

## 5. Chamber UI (`hadron-chamber`)

### 5.1 model.rs (pure, tested)
- `ChamberView` gains `global_mode: Mode`.
- `RosterRow` gains `mode: Mode`, `mode_is_override: bool`, `provider: String`, `model: String`.
  - `mode`/`mode_is_override` from `resolve_mode` + presence of a per-quark ModeSet.
  - `provider`/`model`: folded from the **team config** (§6) the chamber loads, keyed by quark id; empty string when unknown.
- `pending_permission` unchanged (drives the toast).

### 5.2 app.rs (GPUI — thin seam, implemented blind, compiles under `--features gui`)
- **Status bar** (`status_bar`): replace the two muted text spans with two `Tag`s —
  - a **status tag** (`Tag::new().outline()`, variant by state: ready→secondary/success, excited/thinking→info, waiting→warning, error→danger),
  - a **mode tag** for the global mode (variant by tier: Ask→secondary, Write→info, Auto→warning, Bypass→danger), clickable → **mode picker** (a small `PopupMenu`/cycling button that appends `ModeSet{mode}` with `to:None`).
- **Roster rows** (`roster_pane`): each quark row shows `provider · model` (muted) + a small mode `Tag` (override = solid, inherited = outline + `·g` affordance). Clicking it opens a per-quark mode picker → appends `ModeSet{mode}` with `to:Some(quark)`.
- **Toast** (`permission_toast`): add an **Always allow** button between Approve and Deny → appends `grant_remembering(pending)`. Approve stays `grant(pending,true)`, Deny `grant(pending,false)`.
- **Remove** `god_mode_section`, `toggle_policy`, `god_toggle_row`, and the `.child(self.god_mode_section(cx))` in `terminal_pane` — the terminal returns to its full, unshared view.

**Rationale (blind GPUI):** no display is available to this agent; §5.1 logic is unit-tested, §5.2 compiles under `--features gui`. A manual-verify checklist ships in the plan.

---

## 6. Team seating (minimal-T, makes Hadron usable internally)

To "add a quark and start using Hadron," the daemon must know which seats exist and their provider/model, and the adapters must target a model.

- **Team config** `team.json` (in the config dir, next to `chamber.json`), read by **both** daemon and chamber:
  ```json
  { "quarks": [
    { "id": "opus",  "provider": "claude", "model": "opus-4.8",     "flavor": "orchestrator" },
    { "id": "agy",   "provider": "agy",    "model": "gemini-3-pro",  "flavor": "worker" }
  ] }
  ```
  Pure loader in a new `hadron-gluon::team` module (serde + tests); chamber reads it read-only for §5.1 provider/model.
- **Adapters** gain a `model: String`; `ClaudeQuark` adds `--model <model>` to its args, `AgyQuark` the agy equivalent. Constructor becomes `new(id, flavor, model, runner)`. Existing `CliInvocation`-shape tests updated (assert the model arg); still mock-runner only (no API).
- **daemon bin** (`bin/hadron-gluon.rs`): load `team.json`, instantiate the real adapters (via `registry`) instead of `DemoQuark`, run `Engine::serve`. Guard: if `team.json` is missing, keep today's demo behaviour and print a hint.

**Deferred to a follow-up spec (T proper):** the GUI **Add-Quark** modal (pick provider/model/role, write `team.json`). Config-file seating is the MVP; the modal is polish.

**Rationale:** this is the smallest change that makes the app runnable end-to-end with real CLIs while honouring zero-API-in-tests (only the seams are unit-tested; the live run is the user's, with their keys, in the morning).

---

## 7. Testing strategy

- **Pure cores (exhaustive, fast):** `Mode`/`ModeSet`/`PermissionGrant.remember`/`QuarkCard` round-trips (lattice); `resolve_mode`/`global_mode`/`allow_rules`/`decide` truth-table + per-quark-override + TOFU learning (gatekeeper); `team.json` load (gluon); `ChamberView`/`RosterRow` derivation (chamber model).
- **Engine (mock quarks):** the five mode behaviours in §4.
- **Seam (blind):** GPUI compiles under `--features gui`; manual-verify checklist in the plan.
- **Regression:** full `cargo test --workspace` green after every task; `--features gui` clippy clean.

## 8. Out of scope (explicit)

- GUI Add-Quark modal (own spec).
- Real orchestrator-*turn* adjudication for Bypass (documented upgrade path).
- Prefix/glob allow-rule matching (v1 is exact-match on the declared op).
- Concurrent/parallel excitation (the loop stays sequential — same limit as the Phase-4 swarm loop).
- Per-subagent visibility (decided out by the trust-boundary principle).

## 9. Build order (feeds the plan)

1. lattice: `Mode`, `Kind::ModeSet`, `PermissionGrant.remember`, `QuarkCard.{provider,model}`.
2. gatekeeper: delete `Policy`; add `Mode` re-export, `resolve_mode`/`global_mode`/`allow_rules`/`decide`, `grant_remembering`.
3. engine: swap the permission hook to mode-based; drop `policy`/`with_policy`; mode tests.
4. chamber model: `global_mode` + `RosterRow` mode/provider/model derivation; team loader read.
5. chamber app (blind): status/mode/roster tags, pickers, toast Always-allow, remove terminal toggles.
6. team seating: `team` module, adapter `--model`, daemon bin wiring.
7. STATUS doc + memory + manual-verify checklist.
