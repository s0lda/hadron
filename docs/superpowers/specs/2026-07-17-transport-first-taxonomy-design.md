# Transport-first taxonomy + migration (sub-project #2)

**Status:** design, awaiting review
**Date:** 2026-07-17
**Sub-project of:** the adapter-taxonomy rework. This is #2 of three, done **schema-first**:

1. Generic CLI transport ("custom CLI quark") — give `Transport::Cli` its own `command` + parse config; fold `agy` onto it; delete the bespoke `claude.rs` (Claude → ACP only).
2. **Transport-first taxonomy + migration (this spec).**
3. Real SDK adapter for `agy` — implement `Transport::Sdk` for real; retire the `acp-agy` python bridge in favour of an honest `sdk-agy`.

This spec covers **#2 only**. It changes the *shape* of a seat and the dispatch keyed off it; it adds **no** new transport behaviour (the `Sdk` variant is reserved, not implemented) and does not touch the CLI adapters' internals.

---

## 1. Problem

Today a seat's `provider` field smears two orthogonal facts into one string:

- **transport** — how the gluon talks to the agent (CLI subprocess vs ACP over stdio), and
- **vendor** — whose model it is (Claude, Antigravity/Gemini, Codex…).

A CLI seat carries `provider: "agy"` (pure vendor); an ACP seat carries `provider: "acp-claude"` (transport **and** vendor, glued). The `Transport` enum already exists as a *separate* field, so the transport is encoded **twice and inconsistently** — once in `transport`, once smuggled into the `provider` prefix. The ACP catalogue (`ACP_AGENTS`) then keys its boot commands off the glued string (`"acp-claude"`), and the chamber compounds the muddle by assigning `ConfiguredQuark.transport = seat.provider.clone()` (`chamber/app/providers.rs:96`).

The user wants the axis made honest and first-class: ids that read `<transport>-<vendor>` (`cli-agy`, `acp-claude`, `sdk-agy`), a `vendor` that is only the vendor, and `transport` as the single authoritative axis — the foundation both #1 (generic CLI `command`) and #3 (`sdk` adapter) plug into.

## 2. Non-goals

- **No** generic-CLI `command`/parse config (that is #1). `command` stays ACP-only here.
- **No** SDK adapter implementation (that is #3). `Sdk` is a reserved, non-seatable enum variant.
- **No** change to `claude.rs`/`agy.rs`/`acp.rs` adapter internals, prompt building, or routing semantics.
- **No** change to the `resolve_team` / `SeatOverride` / catalogue *layering* model — only the field it carries.

## 3. Design

### 3.1 Schema — `hadron-lattice/src/team.rs`

**Split `provider` into `vendor` + the existing `transport`.**

```rust
pub enum Transport { #[default] Cli, Acp, Sdk }   // Sdk added, reserved

pub struct Seat {
    pub id: QuarkId,                 // free-form, unique; convention: "<transport>-<vendor>"
    pub display_name: Option<String>,
    pub vendor: String,              // WAS `provider`; now pure vendor: "claude", "agy", "codex"
    pub model: String,
    pub effort: Option<String>,
    pub mode_config: Option<String>,
    pub flavor: Flavor,
    pub transport: Transport,        // authoritative axis
    pub command: Option<AcpCommand>, // ACP-only in #2 (extended to CLI in #1)
    pub enabled: bool,
}
```

- `id` remains a plain `QuarkId` string (chosen over a structured id: `QuarkId` threads through routing, `field.jsonl` actors, live-dir names, and identity/color — too large a blast radius for #2, and a structured id forbids two seats of the same `transport+vendor`, which violates the documented *"same vendor + different model = a different seat"* rule).
- `vendor` is authoritative; the `id` prefix is **convention**, defaulted and validated softly (see 3.4), not derived.
- `same_agent` destructures the struct (it already does) so adding/renaming a field forces a decision at compile time — `vendor` joins the identity comparison exactly where `provider` was.

### 3.2 Registry / catalogue — `hadron-gluon/src/adapter/registry.rs`

The catalogue **is** the ACP catalogue, so transport is implied — re-key it on `vendor`:

- `AcpAgent.provider` → `AcpAgent.vendor`; entries change `"acp-claude"` → `"claude"`, `"acp-codex"` → `"codex"`, etc. (~40 rows, mechanical).
- `AcpTarget::for_provider(p)` → `for_vendor(v)`; `QuarkKind::from_provider` → `from_vendor` (CLI map: `"claude" → Claude`, `"agy" → Agy`).
- `QuarkKind::from_seat` dispatches on `transport` **first**:
  - `Cli` → `from_vendor(&seat.vendor)` (unchanged behaviour).
  - `Acp` → resolve boot command by `vendor`, else the seat's explicit `command` (unchanged behaviour).
  - `Sdk` → `bail!` with a clear *"sdk transport is reserved but not yet implemented — see sub-project #3"* (see 3.5).
- `provider_list()` (the chamber's provider view) reads `(vendor, name, program, args)`; the round-trip test that proves every catalogue row resolves stays, retargeted to `for_vendor`.

### 3.3 Backward-compat parsing + one-shot migration — `team.rs`

Two distinct mechanisms, deliberately separate:

**(a) Parse-time normalization (universal, pure, safe).** An un-migrated `team.json` has `provider` and no `vendor`. On parse, if `vendor` is absent, derive it from the old `provider` by stripping a leading `acp-`/`cli-`/`sdk-` transport prefix (`"acp-claude" → "claude"`, `"agy" → "agy"`, `"claude" → "claude"`). Implemented with `#[serde(alias = "provider")]` on `vendor` plus a normalization step in the loader. This preserves the sacred invariant: **an existing `team.json` resolves via `resolve_team` to the identical set of seats** it did before. Ids are *not* touched at parse time.

**(b) Id-rename migration (explicit, opt-in, tested).** The old→new map is the single source of truth, exported from `team.rs` as a constant/function (`legacy_id_renames() -> [(old, new)]`: `agy → cli-agy`, `opus → cli-claude`). Because the rename spans two crates, it is applied in two coordinated places off that one map:

- `team.rs` renames matching ids in a `Team` (a pass sibling to `migrate_to_catalogue`). `acp-*` ids already conform and are untouched; unknown/custom ids are left as-is (no surprise renames). Idempotent — a second run finds nothing to rename.
- the chamber, on load, applies the **same** map to **move `ChamberPrefs.quarks[old_id]` → `[new_id]`** so per-quark color/name/avatar survives (per the identity-system SSOT: identity lives in `ChamberPrefs.quarks[id]`). It cannot live in `team.rs` — the lattice crate does not know `ChamberPrefs` — so the map is the shared contract, not the code.

**Field history:** `field.jsonl` actor references to old ids are **not** rewritten. Per the decision for this repo, this dev repo's field history is simply reset when adopting the new naming; the migration function stays config-only (team.json + ChamberPrefs) and does not mutate the field. New routing/`@mentions` use the new ids.

### 3.4 Id validation — `registry.rs::validate_quark_id`

Keep the existing hard rules (non-empty, whitespace-free, not reserved `human`/`gluon`/`orchestrator`/`team`, unique). Add a **soft** convention check: if `id` does not start with `<transport>-`, log a warning but do not fail — this preserves free-form ids like `cli-agy-pro` and custom names while nudging toward the convention. New-seat creation in Settings **defaults** the id to `<transport>-<vendor>`.

### 3.5 Error handling

- `Sdk` seat → `from_seat` bails with the reserved-not-implemented message naming #3. Nothing seats it; nothing pretends it works.
- Unknown `vendor` on `Cli` → the existing bail, reworded to `unknown vendor {v:?} (expected "claude" or "agy")`.
- ACP seat, uncatalogued `vendor`, no `command` → unchanged "give it a command" guidance, now phrased in terms of `vendor`.

### 3.6 Chamber — `hadron-chamber`

- Fix `providers.rs:96` (`transport: seat.provider.clone()`): populate transport from `seat.transport` and vendor from `seat.vendor` — they are finally distinct.
- Roster row + Settings detail render `transport · vendor · model` (was `provider · model`); the "Provider" kv-row label becomes "Vendor", with transport shown separately.
- New-seat form defaults `id = <transport>-<vendor>`.

## 4. Data flow (unchanged except the field split)

`team.json` → `parse_team` (+ vendor normalization) → `resolve_team(repo, global)` → `Vec<Seat>` → `registry::build_seat` → `QuarkKind::from_seat` (dispatch on `transport`, resolve by `vendor`) → `Box<dyn Quark>`. Only the **key** the dispatch reads changes; the pipeline shape is identical.

## 5. Migration touchpoints (blast radius, verified)

| Crate | Files | Change |
|---|---|---|
| `hadron-lattice` | `team.rs` | `Seat.provider`→`vendor`; `Transport::Sdk`; `same_agent`; parse normalization; id-rename migration |
| `hadron-gluon` | `adapter/registry.rs` | catalogue re-key on `vendor`; `for_vendor`/`from_vendor`; `from_seat` transport-first + `Sdk` bail |
| `hadron-gluon` | `bin/hadron-gluon.rs` | log `seat.vendor` (2 sites) |
| `hadron-chamber` | `app/providers.rs`, `app/render.rs`, `app/widgets.rs`, `app/settings.rs`, `model.rs` | show `transport · vendor · model`; fix the provider/transport conflation; default new-seat id |

## 6. Testing

Preserve every existing invariant test green, and add coverage for the new ones:

- **Back-compat (new):** a pre-migration `team.json` (with `provider`) parses and `resolve_team`s to the **same seats** as its migrated (`vendor`) form — byte-for-byte identity of the resolved `Vec<Seat>`.
- **Vendor derivation (new):** `"acp-claude" → vendor "claude"`, `"agy" → "agy"`, `"cli-claude" → "claude"`.
- **Id-rename migration (new):** `agy → cli-agy`, `opus → cli-claude`; `acp-*` untouched; idempotent on a second run; `ChamberPrefs.quarks` key moves with the id.
- **Sdk reserved (new):** a `Sdk` seat fails `from_seat` with the not-implemented error; no other transport regresses.
- **Preserved:** `resolve_team` identity, `migrate_to_catalogue` idempotence, `same_agent` rebuild-on-change, the catalogue round-trip (retargeted to `for_vendor`).
- **Gate:** `cargo test --workspace --features gui` (full, not filtered) — the baseline per CLAUDE.md Rule 5.

## 7. Security note (Rule 7)

Touches config parsing of `team.json` (a file the human controls) and the boot-command catalogue. No new *external* input surface: `vendor` is matched against a fixed catalogue/CLI map exactly as `provider` was; an unknown `vendor` bails rather than executing anything. The `Sdk` variant executes nothing (reserved bail). Id-rename touches only local `team.json` + `chamber.json`. No new attack surface versus the current `provider` handling.

## 8. Open risks / what this spec does not solve

- The CLI transport still cannot carry a `command` until #1 — `cli-agy` resolves through the existing hard-coded `Agy` adapter, not config. That is intentional sequencing, not an oversight.
- A running daemon reseats the renamed `agy`/`opus` quarks (id change = different agent). Acceptable: CLI residency is in-memory and resets on restart anyway.
