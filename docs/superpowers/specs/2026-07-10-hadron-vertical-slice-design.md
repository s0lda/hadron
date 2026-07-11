# Hadron — Vertical Slice Design Spec

- **Date:** 2026-07-10
- **Status:** Design approved, pending implementation plan
- **Scope of this spec:** The **first vertical slice** only. The full Hadron studio is a multi-spec program; each deferred pillar gets its own spec → plan → build cycle. This document also maps the *whole territory* ("buy the land") so the slice never has to be rearchitected.

---

## 1. The spark & the thesis

Running Claude Code and Antigravity side-by-side, told to coordinate through a shared `progress.md`, produced emergent teamwork better than either agent alone — like two dev teams, but better at it. That is the discovery Hadron productizes:

> **A workspace where agents from any lab — Anthropic, Google, OpenAI, local, and eventually a custom model — self-organize like a great dev team, coordinating asynchronously through a shared bus, each assigned to what it is strongest at, with a human able to watch and steer.**

The enemy is **fragmentation**: every harness has its own walled garden (`.claude/`, `.agents/`, `.kimi/`) with its own config, tools, memory, permissions, budget. Hadron is the **single source of truth (SSOT)**: register models once, hold project knowledge once, set permissions once — and any model you "feed in" operates in that shared world.

**Cost thesis (a first-class North Star):** because the bus is the SSOT, Hadron controls exactly what each model sees each turn. This makes Hadron both a **context minimizer** (curated projections, not ballooning history) and a **demand smoother** (batched, cache-warm calls instead of spiky load). At scale this saves users money *and* reduces peak load for the labs.

---

## 2. Vocabulary (the ubiquitous language)

| Term | In the system | Physics |
|---|---|---|
| **Hadron** | the whole environment | a composite particle that binds quarks |
| **Quark** | an agent/citizen (Claude, Antigravity, later a native worker or the custom model) | the fundamental unit of intelligence |
| **field** (`field.jsonl`) | the shared append-only bus | particles interact through fields |
| **event** | one line in the field | a detected particle interaction |
| **gluon** (`hadron-gluon`) | the headless daemon | the force carrier that binds quarks |
| **lattice** (`hadron-lattice`) | shared protocol/schema crate | lattice QCD, the framework of quark interactions |
| **chamber** (`hadron-chamber`) | the GPUI viewer / chat app | a bubble chamber, where particle tracks are observed |
| **nucleus** | persistent per-project SSOT knowledge | the dense stable core quarks orbit |
| **flavor** | a quark's role/specialty (orchestrator, worker, graphics…) | quark flavors (up, down, charm…) |
| **energy** | token/cost/quota budget | running a quark costs energy |
| **excite** | wake a sleeping quark on a relevant field change | exciting a field produces a particle |

---

## 3. Scope: the slice vs. the bought land

| Subsystem | What it is | In v1 slice? |
|---|---|---|
| field + gluon core | append-only bus, watcher, router, excite loop | ✅ core |
| Quark adapters | **Claude + Antigravity only**, behind one trait | ✅ (2 adapters) |
| git safety | snapshot before edit, rollback on failure | ✅ minimal |
| nucleus | persistent project SSOT, session/repo/git-aware | ✅ minimal |
| orchestrator role | one quark holds `Orchestrator` flavor, assigns to workers | ✅ minimal |
| chamber | left roster + center tabbed chat/log | ✅ left + center, read-only |
| **execution model** | sequential turn-taking, quiesce, backstop | ✅ **core (see §9)** |
| Invariants (enforced methodology) | working protocol injected into projections | ◻️ seam (static preamble in v1) |
| energy system | connection type (sub / API key / CLI) + limit tracking | ◻️ seam (status only in v1) |
| permissions / gates | approve dangerous ops; God-mode | ◻️ v1 = auto-mode only |
| preview / matrix | cosmic animation, pulsing code boxes, diff view | ◻️ deferred, land bought |
| remote control | drive the studio from a remote client (web / mobile / another machine) | ◻️ deferred, land bought (field-access seam) |
| edit-by-hash | tree-sitter + blake3 block hashing, optimistic concurrency | ◻️ deferred **(must)**, schema reserved |
| ACP / MCP transports | standard agent protocols as future adapters | ◻️ deferred, trait seam preserved |
| native worker / custom-model seat | Hadron owns tool-calling; model slots in | ◻️ deferred, seam preserved |

---

## 4. Architecture: two decoupled processes, three crates

Two OS processes connected **only** through `field.jsonl`. This is deliberate: GPUI runs its own executor that conflicts with tokio in-process (the official bridge crate is unpublished). Splitting at the process boundary makes that conflict impossible, keeps the engine testable without a GUI, and is the literal embodiment of "the UI never talks to the AI directly."

```
        HUMAN
         │  (omnibar / any text editor appends a Message)
         ▼
┌─────────────────────┐   reads    ┌──────────────────────────────┐
│  hadron-chamber     │  (polls)   │   .hadron/field.jsonl        │
│  pure GPUI          │◄───────────│   ← THE BUS (append-only)    │
│  read-only + omnibar│  appends   │   + session.toml + nucleus/  │
└─────────────────────┘  Message   └──────────────────────────────┘
                                         ▲ watches (notify) │ appends
                              ┌──────────┴───────────────────┐
                              │  hadron-gluon (daemon)       │
                              │  pure tokio, headless        │
                              │   watcher → router → excite  │
                              │   git safety (gix)           │
                              │   Quark adapters: Claude, Agy│
                              └──────────────────────────────┘
                                   spawns one at a time │
                              ┌────────────────────┴─────────┐
                              ▼                              ▼
                       `claude -p --session-id`      `agy --print`
```

- **`hadron-lattice`** — shared protocol. Structs + serde only, minimal deps. Defines `Event`, `Kind`, `Actor`, `Flavor`, `QuarkState`, `Projection`, `TurnOutcome`. The SSOT schema; the land everyone builds on.
- **`hadron-gluon`** — the daemon. **Pure tokio.** Owns the watcher, router, execution loop, git safety, and the two adapters. Never links GPUI. Fully testable headless.
- **`hadron-chamber`** — the viewer/chat app. **Pure GPUI.** Reads the field (polls on a GPUI timer — it has no tokio/notify), renders it. The omnibar is its one write path (append a human `Message`). Never links tokio.

---

## 5. The `field.jsonl` schema (the SSOT contract)

Append-only, newline-delimited JSON. One event per line.

```jsonl
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:01Z","from":"human","to":"claude","kind":"message","body":"Build the auth module per the plan. Coordinate with agy on the UI."}
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:02Z","from":"claude","to":null,"kind":"status","state":"excited"}
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:20Z","from":"gluon","to":null,"kind":"snapshot","git":"a1b2c3d","label":"pre-edit: claude"}
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:45Z","from":"claude","to":"agy","kind":"message","body":"Auth endpoints done. Your turn on the login form. Types in src/auth/mod.rs."}
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:45Z","from":"claude","to":null,"kind":"edit","paths":["src/auth/mod.rs"],"git":"e4f5a6b","summary":"add login/logout handlers"}
{"v":1,"id":"01J...","ts":"2026-07-10T14:03:46Z","from":"claude","to":null,"kind":"status","state":"ground"}
```

```rust
struct Event {
    v: u32,                 // schema version — hard forward-compat lever
    id: Ulid,               // monotonic, sortable, unique
    ts: DateTime<Utc>,
    from: Actor,            // Human | Quark(QuarkId) | Gluon
    to: Option<QuarkId>,    // addressing = who to EXCITE. None = broadcast/log
    #[serde(flatten)] kind: Kind,
}

#[serde(tag = "kind", rename_all = "snake_case")]
enum Kind {
    Message  { body: String },                                      // Markdown coordination — the progress.md magic
    Status   { state: QuarkState },                                 // ground | excited | thinking | waiting | blocked | error
    Edit     { paths: Vec<String>, git: String, summary: String },  // an in-place file mutation + its snapshot (derived from diff)
    Command  { cmd: String, exit: i32, out_summary: String },       // bash record
    Snapshot { git: String, label: String },                        // gluon auto-save marker
    // — reserved, unused in v1 (land bought): —
    // Permission { .. }  EnergyReport { .. }  EditByHash { block_hash, .. }
}
```

**Three forward-compat rules (non-negotiable):**
1. **Append-only, never rewrite.** History is immutable; every writer only adds lines. This makes concurrent appends line-atomic and git/undo coherent.
2. **`v` + tagged `kind` + unknown-tolerant readers.** The chamber and older daemons **must render an unknown `kind` generically and never crash.** This is what lets a future native worker emit `EditByHash` events into the same field while today's viewer still works. This single rule is the difference between buying land and repaving.
3. **`to` is the wake contract.** The router's whole job: watch the field; when a new event has `to: <quark>`, excite that quark with a projection. Everything smart later (batching, energy-aware throttling, rerouting) upgrades *how* the router decides to excite — the contract never changes.

---

## 6. Transport vs. content: JSONL envelopes, Markdown bodies

> **Models speak Markdown. Hadron speaks JSONL. The adapter is the translator.**

JSONL answers *"how do I append an addressable, ordered, machine-routable event?"* Markdown answers *"what does a model read and write most fluently?"* Both are used where strong. Models **never** touch `field.jsonl` directly: they read a rendered **Markdown projection** and write **Markdown**, which the adapter wraps into a JSONL envelope (stamping `id/ts/from/to`). Antigravity *cannot* emit structured JSON at all — this is forced, not merely preferred.

| Surface | Format | Owner | Why |
|---|---|---|---|
| `field.jsonl` | JSONL (Markdown in `body`) | Hadron | routing, ordering, append-safety, forward-compat |
| Projection to a quark | Markdown brief | Hadron renders | models read Markdown best; also the cost lever |
| Quark output | Markdown / prose | model → adapter wraps | agy can't do JSON; Claude is fluent in MD anyway |
| `nucleus/*.md` | Markdown | quarks read/write | model-facing project knowledge |
| `nucleus/index.json` | JSON | Hadron | machine index: feature→files, `last_verified_commit` |
| Invariants | Markdown | Hadron | injected into projection as a preamble |

The chamber's **Chat tab is a rendered Markdown view of the field** — the `progress.md` reading experience, driven by a structured bus underneath. *(Optional bought land: the gluon may also emit a read-only rendered `progress.md` mirror for humans/tools that want the plain-Markdown artifact.)*

---

## 7. The Quark trait (the asymmetry-hiding seam)

The gluon must never know whether it is talking to `claude`, `agy`, or a future native/ACP/MCP worker.

```rust
trait Quark {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;                        // Orchestrator | Worker(specialty)
    fn energy(&self) -> EnergyState;                   // seam: Available in v1; quota-aware later
    async fn excite(&mut self, turn: Projection) -> Result<TurnOutcome>;
}

struct Projection {          // the single chokepoint: cost-control + Invariants + nucleus + roster converge here
    task: String,            //   the assignment
    invariants: String,     //   enforced methodology (v1: static Markdown preamble)
    nucleus_digest: String,  //   relevant slice of the project SSOT
    roster: Vec<QuarkCard>,  //   who exists, their flavor + energy — enables orchestration
    field_window: Vec<Event>,//   recent relevant events (dumb window in v1)
    git_diff: String,        //   current working diff, not whole files
}

struct TurnOutcome {
    message: Option<String>, //   the quark's field message (coordination), as Markdown
    // file mutations are NOT reported here — they are captured by the gluon via git diff (see §10)
}
```

Each quark emits **two separable things**: (a) a **field message** (coordination) and (b) **file mutations** (in-place edits via its own tools). The adapter returns (a); the gluon derives (b) from `git diff`.

**The two adapters, asymmetry fully contained:**

| | `ClaudeAdapter` | `AgyAdapter` |
|---|---|---|
| Invoke | `claude -p --session-id <uuid> --output-format json` | `agy --dangerously-skip-permissions --print "…" < /dev/null` (`--print` **must be last**) |
| Message capture | parse JSON `result` field | prompt agy to `write_file` → `.hadron/tmp/<turn>.out`; read it back (stdout is unreliable on non-TTY) |
| Resume / context | Hadron **owns** the pre-assigned UUID (clean) | scrape `conversation=<cid>` from `~/.gemini/antigravity-cli/log/cli-*.log`; **serialize per workspace** |
| Done signal | `result` message | process exit (non-zero + stderr = error) |

Both return a `TurnOutcome`. The gluon appends its message + any derived `Edit` events, then excites whoever is addressed next. A future ACP/MCP/native adapter implements the *same trait* — nothing above it changes.

---

## 8. The nucleus (project SSOT), session/repo/git-aware

The antidote to fragmentation and a major energy saver: a persistent, shared project brain every quark reads on arrival and updates as it works, so no quark re-explores what another already mapped.

```
<project-repo>/.hadron/
  field.jsonl              # the bus — session-scoped, archived on new session
  session.toml             # ACTIVE session: id, deployed quarks, orchestrator, git baseline (branch+commit)
  nucleus/                 # durable project SSOT → repo-aware
    map.md                 #   architecture, features, where-things-live
    conventions.md         #   this repo's patterns / house rules
    index.json             #   feature → files/symbols + last_verified_commit
    sessions/<id>.md       #   per-session provenance: what THIS session learned/changed
  snapshots/               # git-safety refs bookkeeping
  tmp/<turn>/              # quark artifact staging (agy result, generated files)
```

- **Repo-aware:** `.hadron/` lives at the repo root; the nucleus is bound to that repo and **git-aware** — each `index.json` entry carries `last_verified_commit`. When the repo advances past that commit, the gluon flags the entry **stale**, and a quark must re-verify before trusting it. This stops the SSOT from rotting into confident hallucination.
- **Session-aware:** `session.toml` names the active session, deployed quarks, the orchestrator, and the git baseline. Durable nucleus knowledge survives sessions; each session appends provenance under `nucleus/sessions/`. A session begins when the daemon attaches to a `.hadron/` workspace; the prior field is archived.

**Layered nucleus (bought land, not v1):** beyond the per-repo nucleus, a **global/user-level nucleus** lives in the OS-level Hadron config dir (`~/.config/hadron/`, `AppData\Roaming\hadron\`, `Library/Application Support/hadron/` — alongside the global usage db). It holds preferences the user wants applied to *every* project. The two cascade: **global user preferences apply broadly, per-repo overrides win for that project**, and a project may declare itself **strict** to lock its own rules against global preference. Exact precedence + override semantics are settled when this pillar is built; v1 ships only the per-repo nucleus.

---

## 9. Execution model: sequential, quiesce, backstop (operational heart)

**v1 runs exactly one quark at a time. Sequential, never concurrent.** This is *forced* by three constraints already in the design, not an apology:

1. Both quarks edit the **same working tree** in place with their own tools.
2. v1 has **no locking** (edit-by-hash is deferred). Git snapshots let you *undo* corruption; they do not *prevent* two concurrent in-place edits from clobbering each other.
3. Antigravity must be serialized per-workspace anyway (the log-scrape resume is ambiguous under concurrency).

Sequential execution is also what makes the git-safety story correct: **snapshot → excite one quark → diff → append → next.**

**The loop:**
1. A new event with `to: <quark>` appears in the field.
2. Gluon takes a git snapshot (`Snapshot` event + shadow-ref commit).
3. Gluon builds the `Projection` and excites that one quark; waits for its `TurnOutcome`.
4. Gluon derives `Edit` events from `git diff` vs the snapshot, appends the quark's message + edits + `Status: ground`.
5. If a new `to:` is now pending, go to 1 for that quark. Otherwise quiesce.

**Quiesce (return control to human):** the system yields to the human when **no event has an unhandled `to:` and all quarks are at `ground`.**

**Backstop (runaway protection):** a **max-exchanges / energy cap per human turn**. Claude's `--max-turns` bounds a single quark; the gluon bounds the *cross-quark* ping-pong. Prevents two agents from looping forever and draining the account.

---

## 10. Edit capture vs. artifact placement (two distinct write-paths)

- **In-place edits (the common case):** CLI harnesses edit the working tree directly with their own tools; they do **not** hand Hadron a patch. So the gluon **derives `Edit` events from `git diff` against the pre-turn snapshot** — the model does not self-report paths. This is the source of truth for what changed.
- **Artifacts (staged, then placed):** when a quark produces a deliverable at a chosen path (agy's result hand-off, a graphics quark's PNG, a generated file), the quark writes to a **Hadron-controlled staging path** (`.hadron/tmp/<turn>/…`); the gluon then **validates → snapshots → moves to the final location → records an `Edit`.** Never trust a model to place files at exact final paths. The "Hadron moves it" rule applies to **this** path, not to in-place edits.

---

## 11. Git safety

- **`gix` (gitoxide)** — pure-Rust, zero dependency on the host git install.
- Snapshots live in a **shadow ref namespace** (`refs/hadron/snapshots/*`) so the user's real branch and HEAD stay pristine — Hadron's auto-saves never pollute their history.
- Before any edit-producing turn: `Snapshot` event + shadow-ref commit (`pre-edit: <quark>`).
- **Rollback:** if a quark runs a command that fails (`cargo check`/build), the gluon reverts to the last snapshot and **re-excites the quark with the failure in its projection** — hallucinate, undo, rethink. v1: rollback on command failure + manual undo. Per-block revert arrives with edit-by-hash.

---

## 12. The chamber (chat app)

```
┌──────────────┬─────────────────────────────┬──────────────┐
│  QUARKS      │   ⌈ Chat ⌉ ⌈ Log ⌉  (tabs)   │  SIDEBAR     │
│ (friend list)│  Chat: neat, human-friendly  │ (future land)│
│ ● claude  ⚡  │    you ⇄ orchestrator,       │  ▪ preview   │
│   orchestr.  │    milestones, decisions     │  ▪ code diff │
│ ● agy     ⚡  │  Log: raw field — every event│  ▪ nucleus   │
│ ○ codex   💤 │                             │              │
│ + add quark  │                             │              │
└──────────────┴─────────────────────────────┴──────────────┘
   ⚡ has energy  💤 idle    [ omnibar: talk to the orchestrator ]
```

- **Left — roster:** deployed quarks, flavor, status, energy. (v1: Claude + Antigravity, statuses live.)
- **Center — tabbed:** *Chat* is the filtered friendly Markdown projection (human ⇄ orchestrator, milestones); *Log* is the raw field. Both are projections of `field.jsonl`.
- **Right — sidebar:** deferred land (preview/cosmic animation, code diff, nucleus browser).
- **Reads via GPUI-timer polling** (no tokio/notify in the viewer). **Writes** only the human `Message` via the omnibar — the chamber is a second, append-only writer to the field, which is safe under the append-only rule.
- Confirmed facts: `gpui = "0.2"` (crates.io, Apache-2.0, permissive — safe to distribute); pre-1.0, pin exactly; macOS most mature, Windows/Linux rougher edges.

---

## 13. Cost / energy strategy

Two compounding levers, both seams in v1:
1. **Provider prompt-caching (free, v1):** keep each quark's session stable (`--session-id`/`--resume`) and turns temporally close so the cached prefix stays warm. Re-invoke-per-turn is cheap when caches don't go cold.
2. **Context projection (land bought):** the quark receives a curated `Projection`, not the raw growing field. v1 projection is a dumb recent window; the seam later hosts summarization, diff-based context, edit-by-hash blocks, and cheapest-capable-model routing.

Plus **demand smoothing** (land bought): the router batches/debounces excitation instead of firing on every micro-edit — fewer, fatter, cache-warm calls reduce user spend and lab peak load.

---

## 14. Testing strategy

The two-process split pays off here:
- **The entire engine is testable with zero GPUI and zero API spend.** A `MockQuark` implementing the trait emits scripted `TurnOutcome`s → deterministic tests for routing, field append/ordering, quiesce/backstop, projection-building, nucleus staleness, snapshot/rollback — offline and free.
- **Golden `field.jsonl` fixtures** drive both engine assertions and the chamber (feed a fixture field, assert the render). The chamber is tested as a pure renderer.
- **Live integration** with real `claude` + `agy` behind an opt-in flag (costs energy) validates the two real adapters end-to-end.

---

## 15. Deferred subsystems (land bought, seams preserved)

Each gets its own spec → plan → build cycle later. None require rearchitecting the slice:

- **Invariants (enforced methodology)** — richer, per-project, versioned working protocol beyond the static preamble.
- **energy system** — connection types (subscription / API key / installed CLI), session-limit + quota tracking, the SQLite usage ledger, and rate-limit-aware rerouting.
- **permissions / gates** — non-blocking permission events + chamber modals; God-mode levels. (v1 = auto-mode.)
- **edit-by-hash (must)** — tree-sitter + blake3 block hashing, optimistic concurrency; unlocks safe *concurrent* multi-quark editing (retiring the v1 sequential constraint) and per-block context/revert. `EditByHash` event reserved; `Projection.git_diff` already avoids whole-file context.
- **ACP / MCP transports** — standard agent protocols as alternate adapters behind the `Quark` trait (cleaner than CLI `--print` + log-scraping).
- **native worker / custom-model seat** — Hadron owns the tool-calling loop against raw APIs; the custom model slots into the orchestrator seat. Same `Quark` trait.
- **preview / matrix UI** — cosmic animation of quarks working; pulsing bounding-box over hashed code blocks; live diff view.
- **remote control** — drive the studio from a remote client (web / mobile / another machine). The headless-daemon + file-bus split already enables this: a remote client is *just another field consumer*. Land bought now: field access stays behind the `field` module seam (local file today; a network transport — the gluon exposing read/append over a local socket or HTTP/WS — later), and auth/permission scoping is reserved for remote actors (a remote human, or a remote quark, is an `Actor` like any other). No remote surface in v1; the seam guarantees no rearchitecture when it lands.

---

## 16. Naming resolved

- **Invariants** — the enforced-methodology pillar: the working protocol every quark must uphold on every turn. Chosen for its dual resonance — physics invariants (conserved quantities) and software invariants (conditions that must always hold) — signalling "correctness guarantees," which is what the pillar is.

---

## 17. Success criteria for the slice

The slice is proven when: from the chamber, a human assigns a real task to the **orchestrator** quark; the orchestrator and the **worker** quark coordinate through `field.jsonl` across multiple turns (sequentially), read and update the **nucleus**, edit the shared repo with **git snapshots** taken before each edit, the human **watches it live** in the chamber, and the loop **quiesces** cleanly (or is rolled back on failure) — recreating the original `progress.md` magic as a real, observable product with two real quarks (Claude + Antigravity).
