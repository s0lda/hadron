# Handoff — "Proceed with All" batch — 2026-07-18

Branch: **`feat/chamber-ui-batch`** (off `main@dfccaa8`). All code committed, full gate green (`cargo test --workspace --features gui`: chamber 85, forge 9, gatekeeper 40, gluon 299, gluon-bin 6, lattice 109 — 0 failed). **Not pushed; not merged to main** — held for your call (see bottom).

## What shipped

| Item | Commit(s) | Notes |
|---|---|---|
| **File Tree folders expand** | `8a12f08`, `bbacddd` | Root cause: `FileTreeNode::insert` set `full_path` only on the leaf, so every folder node had `full_path == ""` → every folder row shared the gpui id `tree-row-`, colliding so expand clicks mis-routed. Fix: every node gets its running path. Extracted `FileTreeNode` to `sys.rs` with 5 unit tests. **See caveat below.** |
| **Your skill-path edits** | `96a9f84` | Committed as-is (specs→`.hadron/docs/specs/`, plans→`.hadron/docs/plans/`, ledger→`.hadron/memory/agents/progress.md`). |
| **Effort tag mirrors mode** | `7ca701b` | Grey "Default" when no per-quark effort, UPPERCASE value when set. Roster width 310→410px so both tags fit. |
| **SDK relabel** | `6045b29` | Reserved `Transport::Sdk` now reads "unsupported — use ACP or CLI" everywhere (was "reserved / not yet implemented"). `acp-agy` id/transport untouched — no taxonomy re-smear. |
| **Your decision answers** | `f405b75` | The inline answers in `DECISIONS-NEEDED-2026-07-18.md`. |
| **§2 activation machinery** | `299b3ed`, `5326168`, `d38b138` | Per-seat `commands: {allowed, not_allowed}` in `team.json` → folded into the gatekeeper's `AllowRules`/`DenyRules`. Default-OFF, inert until the No-Human-Mode toggle. **See the review catch below.** |

## Two things you should read closely

### 1. File Tree — verified half vs. needs-your-eyes half
The tree *building* is now test-proven (untracked folders carry their children; every folder gets a distinct non-empty `full_path`). The gpui *click-routing* half — that unique ids actually fix the expand click — I could not verify: the GUI can't be driven headless on WSL2. **If folders still don't expand when you run it**, the next suspect is NOT the tree building (that's proven) but the re-render path: does the folder-toggle's `cx.notify()` actually repaint the subtree? Start there, not from zero.

### 2. §2 machinery — an adversarial review caught a real bug (fixed)
The independent review of the gating fold found a **Critical**: config `commands.allowed` was leaking to the toggle-OFF path (`decide`'s plain-Auto+BashExec arm consults the same `allow` set), so a config allow-list auto-approved a command even with No-Human-Mode off — asymmetrically, since deny stayed inert off. Contradicted the stated invariant "config rules bite ONLY under No-Human-Mode." **Fixed** (`d38b138`): the whole fold is now gated on `no_human`. Also removed the fold from the merge gate (an **Important**: the merge op is a synthetic sentence, not a command, so command patterns were dead deny / a broad allow could silently auto-land a merge). The security property holds: a config `not_allowed` under No-Human-Mode → `AskHuman`, never `AskOrchestrator`, even under global Bypass.

**One design point for you:** the fix makes config `allowed` inert under plain Auto (toggle off), honoring the "§2-only" invariant. If you actually WANT `commands.allowed` to pre-authorize under plain Auto too (a reasonable other reading), that's a one-line change — but the safe default honors the stated contract.

**Rule-7 note for when you flip the toggle (from the whole-branch review):** a per-seat **Bypass pin auto-approves even a `not_allowed` op** — the `mode==Bypass` early return in `decide` sits *above* the deny check. This is your own "Bypass is bypass — all allowed" decision, applied per-seat: deny is absolute against the *orchestrator* / global-Bypass escalation, but NOT against an explicit per-seat Bypass pin. So a `not_allowed` list on a Bypass-pinned worker does nothing. Intended, but the sharpest edge — don't pin Bypass on a seat you also want a deny-list to constrain.

**Whole-branch review verdict: READY TO MERGE** — no Critical/Important findings; gate 548 passed / 0 failed; the full carry path has a confirmed runtime caller (boot + live-reload both route through `.with_commands`). Three Minor findings, all non-blocking (pathological `a//b` paths git never emits; the always-render effort chip you asked for; and confirmation the `commands` list never leaks into prompt text sent to a quark).

## Script tools — BLOCKED as specced, your architecture call

You said "proceed with it." I did the engineering and hit a wall — full writeup in **`docs/superpowers/FINDING-script-tools-blocked-2026-07-18.md`**. Short version: §3.3 needs *Hadron* to execute a custom named tool the model invokes, but over ACP Hadron is only the client that **gates the agent's own tools** (`on_receive_request`) — it advertises no `mcp_servers`/custom capability and can't inject or run a named tool. That path needs either the SDK adapter (you declined) or MCP-server config (a larger design). **I deliberately did not build a runner** — nothing could call it, and it'd be executing attacker-shaped code (the `forge-edit-by-hash` unwired-mechanism trap). The good news: the **achievable** version already shipped as §2 `commands.allowed` (pre-authorize `python3 .hadron/scripts/linter.py` on a seat; the quark runs it with its own shell, gated). Pick a path in the finding doc when you're back.

## A wrinkle to note
Your skill-path edit points plans/specs at `.hadron/docs/...`, but `.hadron/` is **gitignored** — so plans/specs saved there won't be tracked/committed. If you want them versioned, either un-ignore `.hadron/docs/` or point the paths back under `docs/`. (My internal plan + SDD ledger for this batch live there as intended-scratch; the script-tools finding is in tracked `docs/`.)

## Land / push
Held. You gated push on "done with all," and script-tools is now a decision-for-you, so "all" is qualified. Options when you're back: (a) merge `feat/chamber-ui-batch` to local main + push, (b) review first, (c) leave the §2 toggle off (it is) and push just the UI/File-Tree/SDK parts. Your call — I didn't want to push outward-facing on a qualified "done."
