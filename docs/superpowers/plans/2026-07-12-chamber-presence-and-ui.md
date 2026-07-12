# Plan draft — presence, registry, and chamber UI

Status: **draft for Jake's approval**. Nothing below is built yet unless marked DONE.

> **Verification command for ALL chamber work — read this first.**
> `cargo test -p hadron-chamber` builds **without** the `gui` feature, so it never compiles
> `app.rs`. Every UI change validated only by that command is **unverified** — a type error in
> `app.rs` sails straight through a "16 passed" report (this actually happened: B13 shipped a
> double-`Arc` that didn't compile). Any chamber change must be verified with:
>
> ```
> cargo build -p hadron-chamber --features gui   # compiles app.rs
> cargo test  -p hadron-chamber --features gui   # + the model/config tests
> ```

Already landed on `main`: `@orchestrator` + `@team` aliases (`7af0424`), Markdown chat rendering (`36bd96b`).

---

## Track A — Presence (make the swarm legible)

The status plumbing already exists end to end: `Kind::Status { state: QuarkState }` is a real
field event and the chamber already colors every state (`theme.rs:59` `Excited`, `theme.rs:72`
`Thinking`). The gluon simply never emits `Excited`/`Thinking` — it only emits `Waiting`,
`Ground`, `Blocked`. The UI is wired to show a working quark and is never told one is working.

- **A1. Emit presence events. — DONE (working tree, uncommitted).** `engine.rs` now appends
  `Status { Excited }` immediately before `quark.excite()` and `Ground` after. A turn that
  *fails* now emits `Error` instead of stranding the quark as forever-Excited. 2 new tests;
  `cargo test -p hadron-gluon` → 76 passed, 0 failed (was 74).
- **A2. Typing dots.** The roster dot is *already* rendered from state (`app.rs:1745`
  `theme::presence(r.state)`), so A1 lights it up with no chamber change. Remaining work is only
  the *animation* (pulse while `Excited`). Honest limit: CLI adapters return a whole reply at the
  end of a turn, so dots mean "running, hasn't replied", not literal token-by-token typing.
- **A3. Swarm status tag (bottom-left). — ALREADY BUILT.** `swarm_status_tag` (`app.rs:1826`)
  already renders error → waiting → working → ready off the roster. It was never wrong; it was
  never *fed*. A1 makes it live. No work needed.

## Track B — Chamber UI polish (small, independent)

- **B1. Right rail margin + missing icon. — DONE by @agy (working tree).** `terminal_pane` now
  wraps its body in an inset rounded card matching `chat_pane`. Jake confirms visually.
- **B2. Window size + position persistence. — DONE by @agy, defect fixed by @opus (working
  tree).** `ChamberPrefs` gained `window_bounds`; startup restores it and falls back to centered
  if the saved origin is on no currently-connected display. **Defect found in review:** the first
  cut wrote `chamber.json` from inside `render()`, so dragging the window issued a disk write per
  frame (~60/sec). Replaced with a 500 ms debounced save — memory updates every frame, disk once
  the geometry settles. 2 new config tests (16 passed in `hadron-chamber`, was 14).
- **B3. Copy/paste from chat.** Selectable message text + `Ctrl+C`.
- **B4. Mode tag under the chat, `Shift+Tab` to cycle. — DECIDED.** `Shift+Tab` cycles the
  **global** mode. Important nuance from Jake: cycling global must **not** visually change the
  per-quark tags — those show `default` and only diverge when he manually grants a quark a special
  permission. So the per-quark tag renders an override *only where an override exists*; otherwise
  it reads as inheriting. Everything else is the orchestrator's call.
- **B5. Emojis in chat.** Font-fallback question in GPUI (emoji font in the fallback chain), not
  a parser one. Note this now interacts with **B7** — whatever font stack B7 sets must keep an
  emoji fallback, or emoji render as tofu.
- **B6. Display names in prompts.** @agy said "the human" because the projection carries quark ids
  but not Jake's configured display name. Small fix in `prompt.rs`; every quark then says "Jake".
- **B7. Chat font: Cascadia Code, size 13.65.** Set as the chamber's standard font. Must keep an
  emoji + CJK fallback behind it (see B5). Chat text is currently too large.
- **B8. Show the working directory.** The chamber knows the field path but never displays the
  project root. Status bar is the natural home.
- **B9. Swappable right rail: Terminal / File tree / Changes.** Today the rail is hardcoded to a
  Terminal placeholder. Generalize it to a segmented switcher — the collapse/resize plumbing
  (`Rail::Inspector`, persisted width) already exists and doesn't change. "Changes" is the git
  diff view, which is what makes B10 legible.
- **B10. Written/removed counters (totals / per job / per quark).** Lines added/removed. The field
  already carries `Kind::Edit` events and the engine already snapshots git state before a turn, so
  the data source likely exists — needs a read of what `Kind::Edit` actually records before
  scoping. Per-quark attribution is the interesting part: it's what makes the swarm's work
  legible ("agy touched 200 lines, opus touched 12").
- **B11. Stats on Chat / Log / Timeline.** Per-tab summary (event counts, tokens, edits). Depends
  on B10's counters.
- **B12. Fix scroll position across tab switches.** Chat → Timeline → Chat currently loses the
  scroll position. Cause: all three tabs share the single `chat_scroll: ScrollHandle`
  (`app.rs`), so switching tabs reuses one offset for three different content heights. Fix: one
  `ScrollHandle` per tab. Must preserve the existing stick-to-bottom-on-new-message behavior.

## Track C — Global quark registry ("friends list")

The membership-vs-energy split we agreed: **the field is the truth about active quarks; the
registry is the truth about available ones.** Gray = "known but not summoned" — a membership
state, not a `QuarkState`, so the event schema doesn't change.

- **C1. Global registry** at `~/.hadron/quarks.json` — every quark installed on this machine
  (Claude Code CLI, agy, later Copilot, …), independent of any project.
- **C2. Roster merge.** Chamber merges registry ∪ `team.json`: registry-only entries render gray
  and inert; team entries render with their live energy color from field events.
- **C3. Settings: add / remove / enable quarks.** UI over C1 + `team.json`.
- **C4. Hot team reload.** *The real engineering here.* `team.json` is read once at daemon start
  ("edit + restart the daemon to change the team"). Without this, adding a quark in Settings
  silently lies until restart.
- **C5. Quota → red.** Per-provider detection of rate-limit / session-limit errors → emit
  `Blocked`. Fiddly: Claude and agy report exhaustion differently. Ledger + `energy_limit`
  plumbing already exists in the engine; the *detection* does not.

## Track E — Metering: cost-per-job, per-quark totals, and limit awareness

This answers Jake's "advise here please". Nothing below is built. The good news is that the two
primitives it needs **already exist in the engine** — this is wiring, not new machinery.

### What already exists (verified by reading the code)

- **`Kind::EnergyReport { used_tokens }`** (`event.rs:95`) is a real field event, and the engine
  already appends it after every turn where `used_tokens > 0` (`engine.rs`).
- **The ledger** (`hadron-gluon/src/ledger.rs`) is a SQLite table already accumulating
  `used_tokens` per quark, with `record_usage()` already called by the engine.
- **A git snapshot commit is already created before *every* quark turn** — `engine.rs:235`,
  `snapshot::create(root, "before <quark>")`. This is the important one: it means per-quark line
  attribution is nearly free.
- **`snapshot::working_diff()`** (`snapshot.rs:102`) already returns `git diff HEAD`, and already
  feeds `Projection.git_diff`.

### The load-bearing caveat: tokens are Claude-only today

`claude.rs:98` parses real `usage` out of the CLI's JSON. **`agy` does not** — it goes through
`runner.rs:33-35`, which hardcodes `used_tokens: 0`. So a per-quark total that *leads with tokens*
would show Agy at **0 forever** and look broken. Two consequences:

- **Lead the UI with lines changed, not tokens.** Lines are available for *both* quarks.
- **E0 (prerequisite): make agy report tokens.** Until then, tokens are a Claude-only column and
  must be rendered as "—" (unknown), never "0" (did no work).

### The design

- **E1. Per-job cost, shown next to the message.** "Job" = **one quark turn** (one excite →
  one reply), which is exactly the unit Jake means by "when you send a message, it shows what that
  message cost". Two numbers, not one:
  - **lines `+a / −b`** — from `git diff --numstat <before-snapshot>` after the turn. Attributable
    per quark *because* the snapshot is taken per turn.
  - **tokens** — from `EnergyReport`, where the adapter reports it.

  *Note on "tokens times lines in and out":* I read this as "show me what the message cost", not a
  literal product — `tokens × lines` has no meaningful unit. Proposing the two-number form
  instead. **Jake to correct me if he really wants a single composite score.**
- **E2. Per-quark totals in the friend list.** Accumulate E1 across the session: lines ± and
  tokens, per quark, on the roster row. The ledger already does the token half.
- **E3. New field event** to carry E1, e.g. `Kind::WorkReport { added: u32, removed: u32 }`, so the
  chamber derives everything from the field and needs no git access of its own. Keeps the existing
  rule: the field is the single source of truth.
- **E4. Limit awareness (gluon/lattice — deferred by Jake to "after basics").** Feed *remaining
  budget* into the `Projection` so a quark can size its own work and refuse a job it can't finish.
  The projection already carries `mode`; this adds a budget alongside it. This is the real answer
  to "don't take a job too big" — a quark can only self-limit if it's *told* the limit, and today
  it never is. Pairs with **C5** (quota → red): the same detection that turns a quark red is what
  tells it to stop.

**Gate:** E1/E2 counters are git-gated — the snapshot only runs when `repo_root` is set. Outside a
repo, lines render as "—".

- **B13. GitHub semantics for code blocks.** Jake wants GFM-flavored fenced code blocks (syntax
  highlighting, and GFM extras like tables / task lists / strikethrough). **Likely already
  supported, not missing:** the vendored `gpui-component` text stack ships `SyntaxHighlighter`,
  `LanguageRegistry`, `HighlightTheme` (`text/node.rs:19`) and a `MarkdownExtensions` type — so
  this is probably *configuration* (pick a highlight theme matching our dark palette, enable GFM
  extensions), not implementation. Check before building.

## Track F — File tagging (`@file` mentions in chat)

Jake: mention `dev/hadron/README.md`, chat shows just `README.md`, hover reveals the full path.

- **F1. Chamber: the chip.** Parse a path token in the composer, render it as a chip showing the
  basename with the full path on hover. Purely presentational.
- **F2. Gluon: make it mean something.** The valuable half. A tagged file should ride the
  `Projection` as *attached context*, so the quark is handed the file rather than having to guess
  which `README.md` was meant. This is the same shape as `git_diff` on the projection today.
- **F3. Autocomplete.** Type `@` in the composer → fuzzy file picker over the repo. Nice-to-have.

**Design constraint that decides F1:** `@` is already the *routing* sigil (`@opus`, `@team`,
`@orchestrator`). Overloading it for files risks a path like `@src/main.rs` being parsed as a quark
id — and that risk is **real, not theoretical**: `validate_quark_id` (`registry.rs:39`) only
rejects empty/whitespace ids and the reserved names. It does **not** exclude `/` or `.`, so a quark
could legally be named `src/main.rs` today. Two options:

- **(a) A separate sigil for files** (e.g. `#`). Zero ambiguity, one more thing to remember.
- **(b) Keep `@`, resolve the roster first, treat the rest as a path** — but this is only safe if we
  *first* tighten `validate_quark_id` to forbid `/` and `.` in quark ids, making the two namespaces
  provably disjoint.

I lean **(b) plus the tightening**, because one sigil is better muscle memory and the tightening is
a few lines and independently correct. **Jake to confirm.**

## Track G — The always-available orchestrator

Jake: *"Orchestrator should always be available (dispatch task to other quarks or own agents
perhaps) so chat stays natural."*

**The problem, stated precisely.** Today the orchestrator is a *quark like any other*: when Jake
sends a message, the orchestrator is excited, runs a whole CLI turn, and the field waits. If that
turn is long, Jake is talking to a wall — which is exactly the "am I being ignored" complaint, only
now it's structural rather than a missing status event. Presence (A1) makes the wait *visible*; it
doesn't make it *shorter*.

**The shape of the fix.** Separate *conversing* from *working*:

- **G1. The orchestrator delegates instead of doing.** Long work goes to workers (or to the
  orchestrator's own sub-agents), and the orchestrator's own turn stays short — acknowledge,
  decide, dispatch, hand back. Chat stays responsive because the orchestrator is rarely the one
  blocking.
- **G2. Don't block the orchestrator on a worker's turn.** Today `run_until_quiesce` processes
  pending events in sequence (`engine.rs`). For the orchestrator to answer Jake *while* @agy is
  mid-turn, quark turns must run **concurrently**, not serially. This is the real engineering, and
  it is a genuine change to the engine's execution model — worth its own plan.
- **G3. Sub-agents.** A quark spawning its own agents is *already* possible (both CLIs can), but
  the field can't see them, so their work is invisible and unattributed. If we want them legible,
  they need to surface as field events. Ties directly into **E4** (budget) — an orchestrator that
  fans out to N agents can burn a session limit fast, and today nothing stops it.

**Order:** G1 is mostly a *prompt* change and is cheap. G2 is the substantive one and should not be
attempted casually — serial execution is currently what makes the field's ordering easy to reason
about. G3 after both.

### Advice on "changes" = git diff — two mechanisms, two purposes

Jake asked whether "Changes" is a git diff. Both, but they're *different mechanisms* and it's
worth not conflating them:

- **The Changes rail (B9)** is a **view**: `git diff HEAD` of the working tree — what the swarm has
  changed and not yet committed. `working_diff()` already does this. Simple, and correct.
- **The per-quark counters (E1/E2)** are **attribution**: git diff *cannot* tell you who changed
  what. Attribution comes from diffing each turn against that turn's before-snapshot. That's why
  the per-turn snapshot at `engine.rs:235` is the load-bearing piece.

## Track D — Later / needs its own spec

- Context + session windows UI (raised earlier, still unbuilt) — closely related to **E4**.
- Skills per quark.
- Worker's unaddressed reply hands back to the orchestrator instead of the human — changes the
  core "no mention = human" contract, so it stays a separate decision.

---

## Proposed order

0. **B2 restore bug** — **FIXED.** Size and origin now restore independently; the Wayland origin
   check could previously veto the saved size, so the window always reopened at 1440×900.
   *Unverified:* whether WSLg honours the requested size at creation. Jake's restart settles it.
1. ~~**A1 / A3**~~ — **DONE.** Presence is live end to end.
2. ~~**B1 + B2**~~ — **DONE** (B2's render-loop write fixed in review).
3. **Now:** B7 (font), B8 (working dir), B12 (scroll) — small, visible, all in `app.rs`.
   In parallel: B3 (copy/paste), B4 (mode tag + `Shift+Tab`), B6 (display names, in `prompt.rs`).
4. **Then:** B9 (swappable rail) → B10 (counters) → B11 (stats). B9 must land before B11, since
   "Changes" is where counters become legible; B10 must land before B11, which consumes them.
5. **Then:** C1 → C2 → C3 → C4 → C5 (registry, then the hot-reload that makes it honest).
6. **Then:** D (spec first).

Rough split while both quarks are seated: **@agy takes the chamber (`hadron-chamber`)**, **@opus
takes the engine/prompt side (`hadron-gluon`)** — that keeps two quarks off the same file, which
is the practical constraint that bit us this session.

## Decided

- The quark registry is **global** (user-level), not per-project — Jake was explicit. Proposed
  path `~/.hadron/quarks.json`, alongside the existing `chamber.json`.
- Gray dot = membership, not energy. No change to the field event schema.
- `Shift+Tab` cycles the **global** mode. Per-quark tags keep showing `default` and only diverge
  on a manually granted per-quark permission.
- Bottom-left tag is **swarm-level** (`ready` / `working` / `waiting` / `error`) — already built
  and now fed by A1.

## Open questions for Jake

1. **B10 counters — what's a "job"?** Totals and per-quark are well-defined. "Per job" needs a
   unit: is a job one human request (a turn), or one task from start to quiesce? I'd use *one
   human message → quiesce* as the job boundary, since that's the natural unit in the field.
2. **B9 — is "Changes" a git diff of the working tree, or only the swarm's edits this session?**
   The former is simpler; the latter is more useful for judging what the quarks did.
