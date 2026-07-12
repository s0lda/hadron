# Permission Modes — Manual Verify Checklist

The pure logic is unit-tested; the GPUI was written **blind** (no display). Run
`cargo run -p hadron-chamber --features gui -- <field.jsonl>` and confirm:

## Status bar
- [ ] Left shows a **status tag** (outline): "ready" (green) with an idle field.
- [ ] Right shows a **mode tag** next to the quark count: "ASK" (muted) by default.
- [ ] Clicking the mode tag **cycles** ASK → WRITE → AUTO → BYPASS → ASK, and the
      colour tracks the tier (muted → blue → amber → red). Tail the field: each
      click appends `{"kind":"mode_set","mode":"…","to":null}`.

## Roster (friends list)
- [ ] Each quark row shows `provider · model` (muted) under its name **iff** the
      seat is in `team.json`; otherwise the presence label.
- [ ] Each row has a small **mode tag** on the right. Inherited (global) modes
      render **outlined**; a per-quark override renders **solid**.
- [ ] Clicking a row's mode tag cycles **that quark's** mode and appends
      `{"kind":"mode_set","mode":"…","to":"<quark>"}` (a per-quark override).

## Permission toast (needs a pending `permission_req` in the field)
- [ ] Toast shows the quark + description + risk with **Approve / Always allow / Deny**.
- [ ] **Approve** appends `{"approved":true,"remember":false}`; toast clears.
- [ ] **Always allow** appends `{"approved":true,"remember":true}`; toast clears.
      In AUTO mode, the same op from that quark is then auto-approved silently.
- [ ] **Deny** appends `{"approved":false,…}`; toast clears.

## Terminal rail
- [ ] The terminal is the **full** right-sidebar view again — no god-mode toggle
      pills at the bottom.

## Tweak points if styling is off
- Tag sizes/variants: `mode_tag` / `swarm_status_tag` in `app.rs`.
- Mode labels/colours map: `mode_tag`.
