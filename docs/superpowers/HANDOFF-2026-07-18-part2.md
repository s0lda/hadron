# Handoff addendum — interactive session (2026-07-18, part 2)

Follows `HANDOFF-2026-07-18.md`. This covers what we did after you woke and steered. All green, all on branches, nothing merged/pushed/activated.

## Test count: 514 passing, 0 failing (was 472 at the end of the overnight run).

## What shipped this session

### 3 UI tweaks — `feat/ui-fixes` @ `892717d`
- Mode tag reads **"Default"** for a quark with no per-quark override (colour still shows the resolved temperature).
- Tags **pinned absolute** to the row's top-right so they stop squishing the name/model column (was truncating the model).
- **Folded-rail avatars** padded (20px + gutter) so they don't touch the strip edges.
- **NEEDS YOUR EYES (pixel guesses):** the name line reserves `88px` for the tags; folded avatar is `20px`. If tags overlap a long name or the avatars look off, tell me the direction.
- These are on `feat/ui-fixes`, which **diverged** from the rest of the stack (the overnight branches forked before these). Say the word and I'll cherry-pick them onto whatever branch you run.

### `feat/settings-and-gating` (stacked on the permissions-gating docs branch) — three builds
1. **max_exchanges in Settings** — team-wide field in the Providers panel, persisted to `team.json`; a silent-revert race (external edit + commit-from-any-panel) was found in review and fixed (commit gated to the Providers panel). `None` = daemon default (12), not unlimited.
2. **§2 No-Human-Mode gating** — BUILT + opus-adversarially reviewed, **committed INERT**:
   - `Decision::AskOrchestrator` + `no_human`-gated double-table `decide` (deny-wins, glob, WorkspaceEdit carve-out); `effective_mode` worker-clamp (proven one-directional Bypass→Auto); suspend→adjudicate→resume loop behind `HADRON_NO_HUMAN_MODE` (default **OFF**).
   - **Additive invariant exhaustively proven:** toggle OFF ⇒ byte-for-byte today's gate; `AskOrchestrator` unreachable; scheduler dormant. Safe to keep.
   - **DO NOT flip the toggle** until the 4 activation preconditions land (in the spec): (a) validate grant authorship (else a worker self-approves once a grant-tool exists), (b) wire a deny source + define always-human command classes, (c) [DECIDED: Bypass pin wins over deny], (d) hard-block field-level denials.
   - **Key finding:** the clamp is a **no-op for ACP quarks in-turn** (ACP maps Auto & Bypass both to allow-once). Activating No-Human-Mode won't route real ACP tool-calls to the orchestrator until the SDK per-tool adapter (sub-project #3) lands. This is built *ahead* of that layer.
3. **Custom skills loading** — `~/.hadron/skills/` (global) + `<repo>/.hadron/skills/` merged with built-ins by `name` (repo > global > built-in), wired into the engine. Fully tested headless; back-compat proven byte-for-byte; a real test-hermeticity bug (engine reading your live `~/.hadron`) was reproduced in review and closed with an injectable seam. **Personas (`.hadron/agents/`) and tool-gating deferred** — personas reuse this loader; tool-gating hangs off §2.

## Decisions you made
- No-Human-Mode policy: **clamp + consult orchestrator** (not bypass-for-all).
- Deny vs a per-quark Bypass pin: **Bypass pin wins** (a specific human trust signal supersedes the general deny-list).

## Still open / your call
- The UI pixel values (above).
- Whether/when to pursue the §2 activation preconditions (and the SDK adapter that makes it real for ACP).
- Personas loading + tool-gating (spec written; reuse the skills loader).
- Script-tools (§3): firm no-build until you answer the sandbox/provenance questions (script-tools spec).
- Branch consolidation: the stack is deep (`feat/settings-and-gating` is the tip with max_exchanges+§2+skills). Tell me how you want to merge/rebase and I'll do it.
- Reminder from part 1: rebuild the chamber before judging the Shift+Enter scroll (it was a no-op fix — likely a stale binary).
