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
| 10 | Elevation: shadow scale + radii in `theme.rs` | ⬜ | **agy, in flight** |
| 11 | Font-weight hierarchy (names bold, metadata muted/xs) | ⬜ | **agy, in flight** |
| 12 | Chat input: paste long text does not scroll to cursor | ⬜ | **agy, in flight** |
| 13 | Timeline → card feed, **grouped by turn** not by event | ⬜ | design agreed; unassigned |
| 14 | Log timestamps | ⬜ | |
| 15 | Discord-style date dividers in chat | ⬜ | |
| 16 | Session → **Stats** with session/day/week/month/total | ⬜ | needs a **cached rollup**, not a per-frame replay of `field.jsonl` |
| 17 | Mode tag anchored top-right + wider rail | ⬜ | no `left_0` on the overlay — it eats clicks |
| 18 | Changes pane: show which quark authored a diff | ⬜ | blocked on worktree isolation (#24) |
| 19 | Zed-style terminal (VTE grid) | ⬜ | adopt `alacritty_terminal`, do not hand-roll |
| 20 | **Live preview of what a quark is doing** (stream) | ⬜ | the agent already sends it; `acp.rs:425` drops it |
| 21 | Completion-popup width | ⬜ | **blocked**: hard-coded const in the fork; needs a fork push and this checkout has no remote |

## Engine / protocol

| # | Item | State | Evidence / owner |
|---|---|---|---|
| 22 | `@team` / `@orchestrator` in mention completions | ✅ | `46792ff` |
| 23 | Worker reports **up** to the orchestrator, not into the void | ✅ | `c5b89c2` |
| 24 | Quark display names + `@Display Name` routing | ✅ | `7095593` |
| 25 | **Model selection** (per-seat) | ✅ | `e59993d` — proven live: seat asked `haiku`, agent ran `haiku`. **ACP v1, no v2 migration needed** |
| 26 | A message sent mid-turn is **queued, not eaten** | ✅ | `b80ba5e` — was silently destroying Jake's second message |
| 27 | **Worktree isolation ON** (each turn owns its tree) | ⬜ | **opus, in flight** — precondition for #18, #28, #29 |
| 28 | Machine-checked Definition of Done (claims vs facts) | ⬜ | blocked on #27 |
| 29 | Skills: declared procedures + engine-checked exit criteria | ⬜ | blocked on #27. Injected via the `invariants/` seam — not a plugin system |
| 30 | Per-turn **$ cost** + diff stats | ⬜ | needs `model` on `hadron_lattice::Usage`; `AcpQuark::running_model()` is **implemented, unwired** |
| 31 | **ACP provider catalogue** — which *agents* can we seat | ⬜ | **never answered.** `docs/research/acp-clients.md` answers who *consumes* ACP, a different question |
| 32 | Add a **GPT/Codex** seat | ⬜ | see #31; the seat mechanism takes any ACP command, the *catalogue* is what is missing |
| 33 | Settings → Providers actually **probes** the agent | ⬜ | "Connect" flips a state enum; it never boots the agent, so `model` comes back empty |
| 34 | Agy on an SDK instead of the CLI | ⬜ | map done (`ec71852`); **the SDK is Python-only** — see risk below |
| 35 | Retire the redundant Claude CLI seat | ⬜ | after #27 |
| 36 | `/quark::command` syntax | ⬜ | |
| 37 | Effort / Mode pickers per quark | ⬜ | **free** — same v1 `session/set_config_option` call as #25 |
| 38 | Warn once when the GPU is a software rasterizer | ⬜ | this box renders Hadron on **lavapipe**; the "lag" is a missing Vulkan driver |

---

## Known blockers, stated plainly

- **Worktree isolation (#27) is load-bearing.** `engine.rs` falls back to `cwd.unwrap_or(workspace_root)`, so turns share one checkout. Nothing that attributes work to a turn — authorship, enforcement, a Definition of Done — is sound until it is on.
- **Agy's SDK (#34) is Python-only.** There is no Rust SDK and no documented HTTP surface for the agent abstraction. An SDK agy means Hadron drives a Python process; that is a real decision, not a detail.
- **This checkout has no git remote.** Anything needing a fork push (#21) cannot be done by a quark.
