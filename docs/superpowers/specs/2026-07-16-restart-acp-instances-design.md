# Restart ACP instances (+ roster mode/effort tags) — Design

**Date:** 2026-07-16
**Status:** Approved (Jake, in-conversation)

## Problem

A resident ACP agent subprocess can wedge (hung turn, dead adapter, orphaned
pipe). Today the only recovery is the 30-minute `TURN_DEADLINE`, or removing the
quark from the team entirely. The human needs a **manual, immediate "Restart"**
that reaps a specific quark's ACP subprocess while keeping the quark seated, so
it re-boots fresh on its next turn.

Bundled small request: the roster row shows a permission **mode** badge but no
**effort** — add an effort tag while working in this area.

## Decisions (from Jake)

- **Semantic:** *Restart in place.* Reap the subprocess, keep the quark seated;
  it re-boots lazily on its next `@mention`. (Not "stop until re-enabled".)
- **Timing:** *Immediate, including mid-turn.* A manual Restart is the human's
  explicit judgment that the quark is stuck, so it takes effect now — even if a
  turn is in flight, that turn is aborted and its progress lost. Automatic policy
  still never interrupts a long turn; only the 30-min deadline does. The human is
  the sole override.

## Behavioral contract

- **Idle quark** → `reset_session()` (`self.session = None`) immediately.
- **Mid-turn quark** → abort that quark's turn task (drops the turn future →
  `kill_on_drop` reaps the child, the same path the watchdog uses), append a
  terminal status, reset the session. Siblings keep running.
- The quark **stays seated** throughout and re-boots on its next turn.
- A CLI quark (claude/agy) holds nothing resident between turns → Restart is a
  no-op for it.

## Data flow

```
Chamber  ──append──▶  field.jsonl
  Event(Human, to=Some(quark), Kind::Reboot)
                          │
                          ▼
Daemon (hadron-gluon) run_until_quiesce loop
  polls the field every FIELD_POLL (already does this while turns run)
  → sees a new Reboot past the watermark
  → in-flight? abort its turn task + reset + ground.  idle? lock + reset.
```

Transport is a **field event** — the established command channel (mirrors
`ModeSet`/`ModeClear`), auditable in the Log tab, forward-compatible (an older
daemon decodes it as `Kind::Unknown` and ignores it).

## Components

### hadron-lattice
- `Kind::Reboot` — per-quark (envelope `to = Some(quark)`). Custom serde arm
  `"reboot"`. Round-trip test.

### Quark trait (hadron-gluon)
- `fn reset_session(&mut self) {}` — default no-op.
- `AcpQuark::reset_session` → `self.session = None` (drops the pump handle → the
  connection is torn down and the agent subprocess reaped, per acp.rs:77-79).

### Engine (hadron-gluon) — the substantive change
- Track `abort_handles: HashMap<QuarkId, AbortHandle>` alongside `in_flight`
  (from `turns.spawn`, which returns an `AbortHandle`).
- A persisted `reboot_watermark` = field length at loop entry, so pre-existing
  `Reboot` events (from before this daemon booted, when there is no live session
  to kill) are stale-ignored. Only reboots appended while the daemon runs are
  serviced.
- Inside `run_until_quiesce`'s existing `FIELD_POLL` service point, read `Reboot`
  events past the watermark. For each target:
  - **in-flight:** `abort_handles[q].abort()` (kills the child), then eagerly
    remove from `in_flight`, drop the abort handle, append
    `Status{Ground}` (a manual restart is not an error), `reset_session()`, and
    record `q` in a `rebooting: HashSet<QuarkId>`.
  - **idle:** lock the shared quark and `reset_session()`.
- **The panic-arm guard:** an aborted `JoinSet` task surfaces later as
  `Err(JoinError::is_cancelled)` in `join_next`. The existing `Err` arm treats a
  `JoinError` as a panic and grounds *every* in-flight quark + `abort_all()`
  (engine.rs:1501-1518) — catastrophic for a targeted reboot. Guard it: if
  `rebooting` is non-empty, a cancelled result is an intended reboot corpse —
  pop one from `rebooting` and continue (cleanup already done). Only fall through
  to the ground-everyone path when `rebooting` is empty (a real panic).

### Chamber
- `reboot_quark(qid)` appends `Event::new(Human, Some(qid), Kind::Reboot)` (mirror
  of `set_quark_mode`).
- A small **⟳ Restart** control on `Transport::Acp` roster rows, built by the
  caller with `cx.listener` and passed into `roster_row` as a pre-built element
  (same pattern as `mode_el`). Hidden for CLI quarks (no-op there).
- **B2:** add `effort: Option<String>` to `RosterRow`, populate it in the
  projection from the seat, and render an effort tag beside the mode badge.

## Error handling

- Reboot of an unknown/unseated id → logged no-op.
- Reboot of an idle quark whose `session == None` → harmless no-op.
- An abort that races a turn already finishing → absorbed by the `rebooting` set.

## Testing

- `hadron-lattice`: `Kind::Reboot` round-trips.
- `AcpQuark`: `reset_session()` turns a `Some` session into `None` and leaves the
  quark re-bootable (like the existing failed-boot test).
- Engine: reboot of an **idle** quark resets its session.
- Engine: reboot of an **in-flight** quark (a slow test quark) aborts its turn,
  grounds it, and does **not** ground its siblings.
- Engine: a pre-existing `Reboot` (before the watermark) is not serviced; a new
  one is.

## Out of scope (YAGNI)

- "Stop until re-enabled" — disable already exists (team.json toggle), and it
  deliberately preserves the session.
- Kill-all panic button.
- Restarting CLI quarks (nothing resident to kill).
