# Hadron Phase 6 slice 3b — Chamber Gatekeeper UI (record)

> Implemented blind (no display available to this agent). The pure logic is unit-tested; the GPUI rendering compiles under `--features gui` but needs **visual verification on the user's desktop** (see checklist).

**Goal:** Give the human the non-blocking modal the roadmap describes: an Approve/Deny toast when a quark is waiting on permission, and god-mode toggle switches.

## What shipped

**Testable (unit-tested, headless):**
- `gatekeeper::PendingPermission` now carries the asking `quark`, and `gatekeeper::grant(&pending, approved)` builds the human `PermissionGrant` addressed back to it (so the daemon resumes the right quark). (2 gatekeeper tests.)
- `model::ChamberView` gained `pending_permission`, computed in `project()` — so every reload/refresh surfaces (or clears) the outstanding request. (1 model test.)
- `config::ChamberPrefs` gained a persisted god-mode `policy`. (1 config test.)

**GPUI (compiles, needs visual verify):**
- `Chamber::permission_toast` — a banner injected into the chat card (below the tab header) with the quark, description, risk, and **Approve / Deny** buttons. On click, `answer_permission` appends `gatekeeper::grant(...)` to the field and reloads — the same steering bus as human messages.
- `Chamber::god_mode_section` in the right (Terminal) rail — two independent toggle pills ("Auto-approve edits", "Bypass bash prompts") that flip `prefs.policy` and persist via `config::save`.

## Manual verification checklist (run `cargo run -p hadron-chamber --features gui -- <field.jsonl>`)

- [ ] With a field containing an unanswered `permission_req` (a quark `from`, `Kind::PermissionReq`), the toast appears below the chat tabs showing the quark + description + risk.
- [ ] Clicking **Approve** appends `{"kind":"permission_grant","approved":true,"to":"<quark>"}` (tail the field) and the toast disappears.
- [ ] Clicking **Deny** appends `approved:false` and the toast disappears.
- [ ] The god-mode pills in the Terminal rail toggle ON/OFF (accent when ON) and survive a restart (persisted to `chamber.json`).
- [ ] Toast styling reads well in the dark theme (accent, spacing) — adjust `permission_toast` / `god_toggle_row` if not.

## Known gaps / deferred

- **God-mode toggles don't reach a running daemon.** The chamber persists `policy` to `chamber.json`, but the daemon reads its `Policy` at construction (`Engine::with_policy`). Live propagation needs either the daemon reading the config, or (cleaner, on-thesis) a `Kind::PolicySet` field-event the daemon honors each tick — a follow-up slice.
- **Real quarks still don't emit `permission_req`** (adapters set `TurnOutcome.permission = None`); the toast is exercised by hand-written/engine-emitted requests until adapter prompt-work lands.
- **Sequential pause is global** (a waiting quark quiesces the whole loop) — same limit as the Phase-4 swarm loop.
