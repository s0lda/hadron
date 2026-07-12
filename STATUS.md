# STATUS — Permission Modes & Quark Legibility (overnight build)

**Branch:** `worktree-permission-modes` (worktree at `.claude/worktrees/permission-modes`), off `main@e7e0d61`.
**State:** all 7 planned tasks done. **Full workspace green** (0 failures), clippy clean (incl. `--features gui`) apart from 3 pre-existing `assert_eq!`-bool warnings in Phase 3's `ledger.rs`.
**Built autonomously** under your "orchestrator in bypass" delegation. Nothing here was merged to `main` — it's one fast-forward away (see below).

## What this delivers

The whole "ask / write / auto / bypass" idea we brainstormed, end to end:

- **Mode ladder** as **field events** (`Kind::ModeSet`) — the field is the source of truth, so a running daemon honours a mode change on its next tick and re-opening a field restores it. This also retired the old two god-mode booleans **and** the "toggles don't reach the daemon" gap in one move.
- **Per-quark override over a global default** — set the swarm's baseline in the status bar; override a single quark from its roster row.
- **Trust-on-first-use allow-list** — `PermissionGrant.remember`; "Always allow" teaches a `(quark, op)` rule. **Auto** asks *you* on first use; **Bypass** auto-approves on the orchestrator's authority (audited, no prompt).
- **Legibility** — `QuarkCard`/roster show `provider · model` from `team.json`.
- **UI** — status-bar status + mode tags, roster mode tags, toast "Always allow"; god-mode toggles removed from the terminal (terminal is full again).
- **Team seating** — add a quark by editing `team.json`; the daemon seats the real `claude`/`agy` CLIs with `--model`.

Design rationale (incl. the subagent trust-boundary decision and the Bypass upgrade path) is in `docs/superpowers/specs/2026-07-12-permission-modes-design.md`. Task-by-task plan: `docs/superpowers/plans/2026-07-12-permission-modes.md`.

## Review the work

```
git -C .claude/worktrees/permission-modes log --oneline main..HEAD
git -C .claude/worktrees/permission-modes diff main..HEAD
```

7 commits: spec, plan, then one per layer (lattice → gatekeeper+engine → lattice team+chamber model → chamber GPUI → gluon seating).

## Merge (≈30 seconds; `main` is still at `e7e0d61`, so it fast-forwards)

```
cd /home/Jake/dev/hadron
git checkout main
git merge --ff-only worktree-permission-modes
cargo test --workspace          # confirm green in main's checkout
```

If Gemini has advanced `main` by the time you read this, `--ff-only` will refuse — then `git merge worktree-permission-modes` (a normal merge) or rebase the branch. The changes are mostly additive + confined to `hadron-gatekeeper` (mine) and the mode hook in `engine.rs`; conflicts, if any, would be small.

## Start using Hadron internally

1. **Seat your team** — copy the template and edit:
   ```
   mkdir -p ~/.config/hadron
   cp docs/superpowers/plans/team.example.json ~/.config/hadron/team.json
   ```
   Each entry: `{ "id", "provider": "claude"|"agy", "model", "flavor": "orchestrator"|"worker" }`.
   Same CLI + a different model = a **second entry** (a second quark).
2. **Run the daemon** (real CLIs — real budget): `cargo run -p hadron-gluon -- field.jsonl`
   (no `team.json` → deterministic mock quarks, zero spend).
3. **Run the chamber**: `cargo run -p hadron-chamber --features gui -- field.jsonl`
4. Set the global mode from the status bar; talk to a quark with `@id …`.

### Adding agy
Add an `agy` entry to `team.json` (already in the template) and restart the daemon — it will `seated agy — agy · <model> (Worker)`.

## ⚠️ Verify before trusting live runs

- **CLI flags are best-guess.** `claude --model/--resume/-p --output-format json` and `agy --print --model` follow the existing adapter conventions but were **not** checked against your installed CLI versions (no API spend in this session). Confirm the flags; adjust in `crates/hadron-gluon/src/adapter/{claude.rs,agy.rs}` if needed.
- **GPUI is unverified visually** (built blind). Run the checklist: `docs/superpowers/plans/2026-07-12-permission-modes-verify.md`.

## Deferred (own follow-ups, not blocking)

- **GUI "Add Quark" modal** (writes `team.json`) — config-file seating is the MVP.
- **Real orchestrator-*turn* adjudication for Bypass** — today Bypass auto-grants on standing authority; the upgrade swaps only the AutoApprove branch (same event shapes). See spec §2.
- **Prefix/glob allow-rules** — v1 matches the declared op string exactly.
- **Real `permission_req` emission from adapters** — adapters still self-declare `None`; the whole gate is exercised by engine-emitted/hand-written reqs until adapter prompt-work lands.
- **Sequential loop** — one quark per tick (same limit as the Phase-4 swarm loop).
