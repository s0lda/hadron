# Hadron backlog

The single source of truth for what is asked, what is landed, and what is *seen working*.
Prose ledgers in chat scroll away and the field truncates; this file does not.

**Three buckets, and the middle one is the one we kept lying about:**

| | meaning |
|---|---|
| ✅ **verified** | it is in `main` **and** someone has observed it working |
| 🟡 **landed, unclicked** | it compiles, the gate is green, **nobody has ever run it** — a quark cannot click |
| ⬜ **open** | not started, or in flight |

Update this file in the same commit as the work. Whoever lands it moves the line.

---

## UI / chamber

| # | Item | State | Evidence / owner |
|---|---|---|---|
| 1 | Right rail decoupled from the chat tab | ✅ | `5b76d33` |
| 2 | File tree shows the disk, not `git ls-files` | ✅ | `5b76d33` |
| 3 | Context-menu rows get stable ids | 🟡 | `5b76d33` — never clicked |
| 4 | Context menu background matches the theme | 🟡 | `4998eda` — never clicked |
| 5 | Typography: sans UI / mono code split | 🟡 | `4998eda` — never clicked |
| 6 | App menu (About, Reveal Workspace, Quit) | 🟡 | `a54bbfb` — never clicked |
| 7 | Log lag (virtual list) | ✅ | `b5a21f3` |
| 8 | Log accordions (fold big bodies) | 🟡 | `b5a21f3` |
| 9 | Session-tab zeroed stats | ✅ | `a8e2258` |
| 10 | Elevation: shadow scale + radii in `theme.rs` | ✅ | `4366717` |
| 11 | Font-weight hierarchy (names bold, metadata muted/xs) | ✅ | **agy, completed** |
| 12 | Chat input: paste long text does not scroll to cursor | ✅ | **agy, completed** |
| 13 | Timeline → card feed, **grouped by turn** not by event | ⬜ | design agreed; unassigned |
| 14 | Log timestamps | ⬜ | |
| 15 | Discord-style date dividers in chat | ⬜ | |
| 16 | Session → **Stats** with session/day/week/month/total | ⬜ | needs a **cached rollup**, not a per-frame replay of `field.jsonl` |
| 17 | Mode tag anchored top-right + wider rail | ⬜ | no `left_0` on the overlay — it eats clicks |
| 18 | Changes pane: show which quark authored a diff | ⬜ | blocked on worktree isolation (#24) |
| 19 | Zed-style terminal (VTE grid) | ⬜ | adopt `alacritty_terminal`, do not hand-roll |
| 20 | **Live preview of what a quark is doing** (stream) | ✅ | `89a08d9` |
| 21 | Completion-popup width | ⬜ | **blocked**: hard-coded const in the fork; needs a fork push and this checkout has no remote |

## Engine / protocol

| # | Item | State | Evidence / owner |
|---|---|---|---|
| 22 | `@team` / `@orchestrator` in mention completions | ✅ | `46792ff` |
| 23 | Worker reports **up** to the orchestrator, not into the void | ✅ | `c5b89c2` |
| 24 | Quark display names + `@Display Name` routing | ✅ | `7095593` |
| 25 | **Model selection** (per-seat) | ✅ | `e59993d` — proven live: seat asked `haiku`, agent ran `haiku`. **ACP v1, no v2 migration needed** |
| 26 | A message sent mid-turn is **queued, not eaten** | ✅ | `b80ba5e` — was silently destroying Jake's second message |
| 27 | **Worktree isolation ON** (each turn owns its tree) | 🛑 | **Kept OFF by Orchestrator** to avoid breaking shared tree workflow and in-flight agy changes. Needs `with_merge_gate` first. |
| 28 | Machine-checked Definition of Done (claims vs facts) | ⬜ | blocked on #27 |
| 29 | Skills: declared procedures + engine-checked exit criteria | ⬜ | blocked on #27. Injected via the `invariants/` seam — not a plugin system |
| 30 | Per-turn **$ cost** + diff stats | ✅ | `bbda914` |
| 31 | **ACP provider catalogue** — which *agents* can we seat | ✅ | `docs/research/acp-providers.md` — the 36 agents upstream lists as ACP **servers**, with what it takes to seat each. Distinct from `acp-clients.md` (editors) |
| 32 | Add a **GPT/Codex** seat | 🟡 | preset `acp-codex` is in `ACP_AGENTS` and pinned by a test; boots `npx -y @agentclientprotocol/codex-acp@latest` (bundles its own codex; needs a ChatGPT login or `OPENAI_API_KEY`). **`proven: false`** — never live-booted here, the sandbox refused to run an unnamed npm package. One "Connect" with a human present closes it |
| 33 | Settings → Providers actually **probes** the agent | ✅ | `bbda914` — "Connect" now spawns a probe task and transitions to `Ready { model }` |
| 34 | Agy on an SDK instead of the CLI | ✅ | Python adapter built using JSON-RPC via venv |
| 35 | Retire the redundant Claude CLI seat | ⬜ | after #27 |
| 36 | `/quark::command` syntax | ⬜ | |
| 37 | Effort / Mode pickers per quark | ✅ | `bbda914` |
| 38 | Warn once when the GPU is a software rasterizer | ⬜ | this box renders Hadron on **lavapipe**; the "lag" is a missing Vulkan driver |

---

## Known blockers, stated plainly

- **Worktree isolation (#27) is load-bearing, fully built, and OFF by one line — but turning it on is a product decision, not a refactor.**
  - *Why it is off:* not a bug. `bin/hadron-gluon.rs:253` builds the engine with `Engine::new`, never `.with_git(repo)`, so `repo_root` is `None`. The bin's own module doc still calls `with_git` a deliberate mock-era hold ("held for a human-present session") — a comment that went stale when the bin grew real adapters. `with_git` is the one line left behind.
  - *What is already built:* per-quark worktree, per-assignment branch, branch-diff attribution, `Kind::Edit`, a merge gate with rebase-on-concurrency. All of it, tested.
  - *Correction to the earlier read:* with `repo_root = None` a turn's commit is **not mis-attributed to a sibling — it is not attributed at all.** `TurnTree` is never constructed (`engine.rs:1186`), so the `head_before` comparison and `Kind::Edit` never run. Attribution is dormant, not broken. Pinned by `without_worktree_isolation_the_engine_attributes_no_commit`.
  - *The attribution property is now proven* (`concurrent_commits_are_attributed_to_the_turn_that_made_them`): two turns committing concurrently are each credited with their own commit and their own files. Negative control: forced onto one shared tree, quark `a` is credited with `["a.txt", "b.txt"]` — the misattribution, reproduced.
  - **The trap:** `with_merge_gate` is *also* unwired (no caller outside tests), and `merge_gate` early-returns when it is `None`. So flipping `with_git` **alone** parks every quark's work on a `quark/<id>/<ulid>` branch inside gitignored `.hadron/trees/` that nothing ever merges. Jake's tree would show nothing and the swarm would look dead. **Isolation must land together with `with_merge_gate(CargoMergeRunner)`, or not at all.**
  - **What Jake gives up if it goes on:** (a) he stops seeing quarks edit files in his own tree; (b) every completed assignment runs `cargo test --workspace` in the quark's worktree before landing — minutes per turn; (c) landing does `git merge --ff-only` in his checkout, which fails if he has uncommitted edits to a file the quark touched. That last one bites today: the tree is dirty right now.
- **Agy's SDK (#34) is Python-only.** There is no Rust SDK and no documented HTTP surface for the agent abstraction. An SDK agy means Hadron drives a Python process; that is a real decision, not a detail.
- **This checkout has no git remote.** Anything needing a fork push (#21) cannot be done by a quark.
