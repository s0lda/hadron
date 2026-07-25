# Nucleus: autolearn, `/learn`, and token accounting

**Status**: approved by Jake 2026-07-25 — next step is an implementation plan
**Author**: @Claude (orchestrator), from a `@team` brainstorm with @Agy, @Antigravity, @Codex, @Sonnet

---

## The problem

Hadron is a tool for orchestrating LLM work in **any** repo, not a Hadron-specific
harness. Three things stop a swarm from learning in someone else's checkout:

1. **The engine only ever reads the nucleus.** Writing is a prose instruction in the
   Standard Model, so a swarm learns only if the model feels like it that turn.
2. **The index is silently cut.** It is 40,702 chars against a 32,768-char budget
   (measured 2026-07-25) — 24% over. `nucleus.rs` keeps the header plus the newest
   lines that fit and **drops the middle**. Every quark this session was told
   "lessons are missing from it" and no human ever saw that.
3. **Nobody can see what a turn costs.** There is no per-turn number for what Hadron
   injects, so "is the nucleus worth its tokens" is unanswerable.

## What already works — do not rebuild it

Verified this session against the repo, not assumed:

| Leg | Verdict | Evidence |
|---|---|---|
| Custom skills load from `<repo>/.hadron/skills` | **wired** | `engine/routing.rs:71` |
| A *custom* skill is selected by its triggers | **wired** | `skills/select.rs:32` is generic over `&[ResolvedSkill]` — no builtin-only gate |
| A custom skill's body reaches the prompt | **wired** | `engine/routing.rs:429` `skills::render(&m, …)` |
| Custom preons load from `<repo>/.hadron/preons` | **wired** | `engine.rs:608` |
| `@preon-name` routes to a seat | **wired** | `router/mod.rs:238-241`, via `preferred_role` → `card_for_role` |

**User-loadable skills and preons are done.** That ask needs no work. The doc comment in
`preons.rs:8` ("routing … is a separate concern (the router), not this one") describes a
correct module boundary, not a missing feature.

## The blocker that outranks memory

`merge.rs:78` — `CargoMergeRunner::tests` runs a literal `cargo test --workspace`. There
is no repo-type detection anywhere. In an npm/pytest/go repo the gate **hard-fails**
(`ENOENT` propagates through `run_tests_with`'s `?` at `merge.rs:117`) — it does not
falsely report green, which is the one saving grace. But every worker turn ends in error.

**Memory improvements would land into a swarm that cannot complete a turn there.** A
manifest-detected runner is one small, bounded change and belongs before, or beside, all
of the below. Whether cross-repo support is in scope now is Jake's call; this spec is
written assuming it is.

---

## Design

### 1. Four commands, no engine guessing

The human states the tier; the engine never infers it.

| command | writes to | tier |
|---|---|---|
| `/learn <text>` | `<repo>/.hadron/nucleus/index.md` + `notes/<slug>.md` | lesson, **pinned** |
| `/learn-global <text>` | `~/.hadron/nucleus/index.md` + `notes/` | lesson, pinned |
| `/learn-std-model <text>` | `<repo>/.hadron/nucleus/laws.md` | law |
| `/learn-std-model-global <text>` | `~/.hadron/nucleus/laws.md` | law |

Repo-scoped is the default, global is explicit. This is also the security boundary: a
cloned repo's `.hadron/` must never silently install a global directive into a user's
home.

**`/learn-std-model` appends to `laws.md`; it does not edit the Standard Model.** The
Standard Model is `include_str!`'d into the binary (`engine.rs:108`) on purpose —
`.hadron/` is gitignored, so anything that must survive a clone lives in the binary.
`laws.md` is injected immediately after it, carrying the same authority in the prompt.

Each command is one row in `text::COMMANDS` and one arm in `handle_chat_command` — the
one-command-table invariant already forces both, and
`every_listed_command_is_handled` fails the gate on a row with no arm.

A `[pinned]` lesson is exempt from pruning and from the budget cut. That is the whole
difference between it and an ordinary lesson.

### 2. Stop cutting the index — make it lazy instead

Today: whole index injected, capped, middle dropped. Instead, extend the split that
already works. `index.md` → `notes/` is lazy two-tier; make it three:

- Lessons carry a `[tag:…]` marker. The engine injects a **tag manifest** — headings and
  counts, a few hundred chars — plus pinned lines and anything matching the task text.
- A quark opens the one slice it needs with its own tools, exactly as it already opens a
  note.
- **Nothing is dropped, because nothing needs dropping.**

Scoping cannot key on "files the quark will edit" — at dispatch the engine has task text,
not target paths. Tag matching against task text is what is actually available.

### 3. Warn the human, not the prompt

Today the only thing that knows the index is over budget is the quark reading it. The
chamber shows a warning when `index.md` crosses its budget. This is the smallest piece
here and arguably the most valuable: the 24%-over number above only exists because
someone ran `wc` by hand.

### 4. Turn-end capture

The merge gate requires a memory decision at turn end rather than hoping for one — same
hook point as `record_artifact_sweep`. This is what makes a swarm learn in a repo nobody
has curated.

**Explicitly out of scope: automatic promotion to a law by detecting "a test guard was
added".** There is no observable event that means *this lesson is now guarded by code* —
that is a model judgment, not a state the daemon can read. Four of us reached this
independently. Promotion and pruning are quark actions; the engine enforces the budget
and makes the action cheap.

### 5. Token accounting in Stats

`prompt::build` (`adapter/prompt/mod.rs:60`) appends sections in order — Standard Model,
invariants, nucleus digest, index, task, field window, skill. Measure each as it is
pushed and carry the breakdown on the turn's usage event.

`TokenSpend` (`telemetry.rs:65`) already splits `input` / `output` / `cache_read` /
`cache_write`, with `None` meaning *unknown, not zero*. **The honest metric is fresh vs
cached**, not total — a re-sent nucleus is cache-read and roughly an order of magnitude
cheaper than fresh input.

Nothing today records the prompt's own size: the only `prompt.len()` in the workspace is
`cli.rs:50`, inside `fit_prompt`'s argv guard. This is a new field, not a read of
existing data.

### 6. Two prompt-text changes, no design needed

- **Orchestrator dispatches before working.** Analyse → emit every `@quark` delegation →
  then do your own slice, so workers run in parallel with you instead of after you.
- **Brevity.** Rule 11 exists and is not landing. A prose rule does not enforce itself —
  the same lesson as the index budget. Worth considering a measured length signal rather
  than another sentence of prose.

### 7. The titlebar menu — the missing three lines

The surface is the three-line (hamburger) menu in `widgets.rs:100-135`. It has four items
today; Jake's list has seven. **The three missing are Open Workspace, New Session, and
Rename** — that is the "3 lines" exactly.

Target, with dividers as specified:

```
Open Workspace
Reveal Workspace in File Manager
─────
New Session
Rename
─────
Settings
About Hadron
─────
Quit Hadron
```

Note the order also changes: Settings moves from first to sixth.

Two of the three are thin wrappers over behaviour that already exists, and by the
one-command-table invariant they must invoke the existing `COMMANDS` row rather than grow
a second implementation:

- **New Session** → `/clear` ("archive and clear the current chat history"). If "New
  Session" is the better name for that concept, rename the row rather than add a second.
- **Rename** → `/rename`, which needs a text prompt from the menu path since the command
  takes an argument.

**Open Workspace is the one real feature here, and it is not yet defined.** It means
pointing the chamber at a different repo — a directory picker, then re-resolving
`field.jsonl`, the roster, and the nucleus for that workspace. `team_for_field` is
path-sensitive (a nested field file loads an empty roster), so this is not a one-liner. It
should be its own task in the plan, and possibly its own design pass; the other two are
menu wiring.

---

## Build order

Cheapest first; each step is independently shippable.

1. **Manifest-detected merge runner** (`merge.rs:73-79`) — unblocks any non-Rust repo.
2. **Chamber budget warning** — tiny, and makes the next step's value visible.
3. **Per-section token accounting** — the number that says whether step 4 is worth it.
4. **Tag manifest + lazy index slices** — stops the silent cut.
5. **The four `/learn` commands.**
6. **Turn-end capture hook.**
7. **Titlebar menu** — Reveal/Settings/About reorder plus New Session and Rename.
8. **Open Workspace** — the only undefined item; may need its own design pass.
9. Prompt-text changes — ship any time, no dependency.
