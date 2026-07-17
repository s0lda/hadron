# Transport-first Taxonomy + Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split a seat's smeared `provider` string into an authoritative `transport` axis (`cli`/`acp`/`sdk`) plus a pure `vendor`, so ids read `<transport>-<vendor>`, without changing any adapter behaviour.

**Architecture:** Rename `Seat.provider` → `vendor` (with a `provider` serde alias + prefix-stripping normalization so old `team.json` still resolves byte-for-byte), re-key the ACP catalogue on `vendor`, add a reserved-but-unimplemented `Transport::Sdk`, and migrate legacy ids (`agy`→`cli-agy`, `opus`→`cli-claude`) via one shared old→new map applied to both `team.json` and the chamber's `ChamberPrefs`.

**Tech Stack:** Rust workspace (`hadron-lattice`, `hadron-gluon`, `hadron-chamber`), serde, `async-trait`, gpui (chamber GUI). Tests via `cargo test`.

## Global Constraints

- Baseline gate, run before and after: `cargo test --workspace --features gui` (full, not filtered) — CLAUDE.md Rule 5.
- SSOT: exactly one field for vendor (`vendor`); do not keep a parallel `provider` field alive (Rule 3).
- Backward compatibility is by construction: an un-migrated `team.json` (carrying `provider`) MUST `resolve_team` to the identical `Vec<Seat>` as before (the `resolve_team` identity invariant).
- Do NOT change `claude.rs` / `agy.rs` / `acp.rs` adapter internals, prompt building, or routing semantics. `command` stays ACP-only in this sub-project.
- `Transport::Sdk` is reserved: it must never seat a quark; `from_seat` returns a clear not-implemented error.
- The legacy id-rename map is the single source of truth (`hadron_lattice::legacy_id_renames()`); every consumer (team.json rename, ChamberPrefs key move) reads from it — do not hardcode the map twice.
- Match existing style; remove unused imports/vars (Rule 10). Frequent commits.

---

### Task 1: Rename `Seat.provider` → `vendor` with back-compat parsing

Atomic cross-crate rename: a struct-field rename cannot compile in halves, so all `.provider` reads move together and the task ends on a green full-workspace gate.

**Files:**
- Modify: `crates/hadron-lattice/src/team.rs` (`Seat` field, `#[serde(alias)]`, `same_agent`, `Seat::cli`, new `normalize_vendor`, `parse_team` call site, tests)
- Modify: `crates/hadron-gluon/src/adapter/registry.rs:373,418,421` (`seat.provider` → `seat.vendor`)
- Modify: `crates/hadron-gluon/src/bin/hadron-gluon.rs:144,229` (log `seat.vendor`)
- Modify: `crates/hadron-chamber/src/app/providers.rs:96`, `crates/hadron-chamber/src/model.rs:601,611` (`seat.provider` → `seat.vendor`)
- Test: inline `#[cfg(test)]` in `crates/hadron-lattice/src/team.rs`

**Interfaces:**
- Produces: `Seat { vendor: String, .. }` (was `provider`); `pub fn Seat::normalize_vendor(&mut self)` strips a leading `cli-`/`acp-`/`sdk-` transport prefix from `vendor`.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing back-compat test**

Add to the `tests` module in `crates/hadron-lattice/src/team.rs`:

```rust
#[test]
fn legacy_provider_key_parses_into_vendor_stripped_of_transport_prefix() {
    // A team.json written before this change: ACP seat carries the smeared "acp-claude",
    // CLI seat carries the bare vendor "agy".
    let json = r#"{"quarks":[
        {"id":"acp-claude","provider":"acp-claude","model":"opus","flavor":"worker","transport":"acp"},
        {"id":"agy","provider":"agy","model":"flash","flavor":"orchestrator","transport":"cli"}
    ]}"#;
    let team = parse_team(json).expect("legacy team parses");
    assert_eq!(team.quarks[0].vendor, "claude", "acp- prefix stripped to pure vendor");
    assert_eq!(team.quarks[1].vendor, "agy", "bare vendor left as-is");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-lattice legacy_provider_key_parses_into_vendor -- --nocapture`
Expected: FAIL to compile — `Seat` has no field `vendor`.

- [ ] **Step 3: Rename the field, add the alias, and the normalizer**

In `Seat` (team.rs), rename the field and doc:

```rust
    /// The pure vendor, e.g. "claude", "agy", "codex". WAS `provider`, which smeared
    /// vendor and transport ("acp-claude"); `transport` is now the authoritative axis.
    /// `#[serde(alias)]` keeps an un-migrated team.json (with `provider`) parsing; the
    /// prefix it may carry is stripped by `normalize_vendor` in `parse_team`.
    #[serde(alias = "provider")]
    pub vendor: String,
```

Add the normalizer (method on `Seat`, near `Seat::cli`):

```rust
impl Seat {
    /// Strip a leading transport prefix a legacy `provider` value may carry, leaving the
    /// pure vendor: "acp-claude" → "claude", "cli-agy" → "agy", "agy" → "agy". Idempotent.
    pub fn normalize_vendor(&mut self) {
        for prefix in ["cli-", "acp-", "sdk-"] {
            if let Some(rest) = self.vendor.strip_prefix(prefix) {
                self.vendor = rest.to_string();
                return;
            }
        }
    }
}
```

Update the three internal sites in team.rs:
- `same_agent`: change the destructure binding `provider` → `vendor` and the comparison `provider == &other.provider` → `vendor == &other.vendor`.
- `Seat::cli(...)`: rename the `provider:` field init to `vendor:` (keep the parameter name `provider` on the constructor signature — it is a vendor either way; or rename to `vendor` for clarity and fix call sites).

- [ ] **Step 4: Normalize on parse**

In `parse_team` (team.rs), after deserializing, normalize each seat's vendor. Find:

```rust
pub fn parse_team(text: &str) -> std::io::Result<Team> {
    serde_json::from_str(text).map_err(std::io::Error::other)
}
```

Change to:

```rust
pub fn parse_team(text: &str) -> std::io::Result<Team> {
    let mut team: Team = serde_json::from_str(text).map_err(std::io::Error::other)?;
    for seat in &mut team.quarks {
        seat.normalize_vendor();
    }
    Ok(team)
}
```

- [ ] **Step 5: Fix the remaining `.provider` reads across crates**

Update each site the grep in the plan lists to `seat.vendor`:
- `registry.rs`: `AcpTarget::for_provider(&seat.provider)` → keep call name for now (renamed in Task 3) but the argument becomes `&seat.vendor`; `from_provider(&seat.provider)` → `&seat.vendor`; the `let provider = seat.provider.as_str();` diagnostic → `seat.vendor.as_str()`.
- `bin/hadron-gluon.rs:144,229`: `seat.provider` → `seat.vendor`.
- `chamber/app/providers.rs:96`: `transport: seat.provider.clone()` → `transport: seat.vendor.clone()` (the proper transport/vendor split lands in Task 5; this only keeps it compiling).
- `chamber/model.rs:601,611`: `s.provider.clone()` / `g.provider.clone()` → `.vendor.clone()`.
- Fix any test in team.rs/model.rs asserting `.provider` (e.g. `assert_eq!(s.provider, "agy")` → `.vendor`), and any `Seat { provider: .. }` struct literal in tests → `vendor:`.

- [ ] **Step 6: Run the back-compat test and the full gate**

Run: `cargo test -p hadron-lattice legacy_provider_key_parses_into_vendor`
Expected: PASS.

Run: `cargo test --workspace --features gui`
Expected: PASS (whole workspace compiles and every existing test is green).

- [ ] **Step 7: Commit**

```bash
git add crates/hadron-lattice/src/team.rs crates/hadron-gluon/src/adapter/registry.rs crates/hadron-gluon/src/bin/hadron-gluon.rs crates/hadron-chamber/src/app/providers.rs crates/hadron-chamber/src/model.rs
git commit -m "refactor(lattice): rename Seat.provider -> vendor with back-compat parsing"
```

---

### Task 2: Reserved `Transport::Sdk` variant

**Files:**
- Modify: `crates/hadron-lattice/src/team.rs` (`Transport` enum)
- Modify: `crates/hadron-gluon/src/adapter/registry.rs` (`QuarkKind::from_seat` match)
- Test: inline `#[cfg(test)]` in `registry.rs`

**Interfaces:**
- Produces: `Transport::Sdk` (serialized `"sdk"`); `from_seat` on an `Sdk` seat returns `Err`.
- Consumes: `Seat`, `Transport` from Task 1.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `registry.rs` (reuse the `acp_seat` helper pattern already there; build a seat with `transport: Transport::Sdk`):

```rust
#[test]
fn sdk_transport_is_reserved_and_not_seatable() {
    let mut seat = acp_seat("sdk-agy", "agy");
    seat.transport = Transport::Sdk;
    let err = QuarkKind::from_seat(&seat).expect_err("sdk must not resolve yet");
    assert!(
        err.to_string().contains("sdk") && err.to_string().contains("not yet implemented"),
        "error must name the reserved transport, got: {err}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-gluon sdk_transport_is_reserved -- --nocapture`
Expected: FAIL to compile — `Transport` has no variant `Sdk`.

- [ ] **Step 3: Add the variant**

In `team.rs`, extend the enum (keep `Cli` the default):

```rust
pub enum Transport {
    #[default]
    Cli,
    Acp,
    /// Reserved: a resident per-provider SDK adapter. Nameable (`sdk-agy`) so the axis is
    /// first-class, but not yet implemented — `from_seat` rejects it. Landed by sub-project #3.
    Sdk,
}
```

- [ ] **Step 4: Handle it in dispatch**

In `QuarkKind::from_seat` (registry.rs), the `match seat.transport` is now non-exhaustive and will not compile. Add the arm:

```rust
            Transport::Sdk => anyhow::bail!(
                "seat '{}' uses the sdk transport, which is reserved but not yet implemented \
                 (see sub-project #3); use transport \"cli\" or \"acp\" for now",
                seat.id.as_str()
            ),
```

- [ ] **Step 5: Run the test and the gate**

Run: `cargo test -p hadron-gluon sdk_transport_is_reserved`
Expected: PASS.
Run: `cargo test --workspace --features gui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-lattice/src/team.rs crates/hadron-gluon/src/adapter/registry.rs
git commit -m "feat(lattice): add reserved Transport::Sdk variant (rejected until #3)"
```

---

### Task 3: Re-key the ACP catalogue on `vendor`

**Files:**
- Modify: `crates/hadron-gluon/src/adapter/registry.rs` (`AcpAgentSpec.provider` → `vendor`; every `ACP_AGENTS` entry; `for_provider` → `for_vendor`; `from_provider` → `from_vendor`; `for_seat`; `provider_list`; tests)

**Interfaces:**
- Produces: `AcpTarget::for_vendor(vendor: &str) -> Option<AcpTarget>`; `QuarkKind::from_vendor(vendor: &str) -> anyhow::Result<QuarkKind>`; `AcpAgentSpec { vendor, name, program, args, proven }`.
- Consumes: `Seat.vendor`, `Transport` from Tasks 1–2.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `registry.rs`:

```rust
#[test]
fn catalogue_is_keyed_on_pure_vendor() {
    // The ACP catalogue is *the ACP catalogue*, so transport is implied: it keys on the
    // pure vendor "claude", not the old smeared "acp-claude".
    assert!(AcpTarget::for_vendor("claude").is_some(), "claude resolves by pure vendor");
    assert!(AcpTarget::for_vendor("acp-claude").is_none(), "the old smeared key is gone");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-gluon catalogue_is_keyed_on_pure_vendor -- --nocapture`
Expected: FAIL to compile — no `for_vendor`.

- [ ] **Step 3: Rename the catalogue field and its keys**

In `AcpAgentSpec`, rename `pub provider: &'static str` → `pub vendor: &'static str` (update the doc comment: "The pure vendor a seat carries, e.g. `\"claude\"`."). In every `ACP_AGENTS` entry, strip the `acp-` prefix from the key: `provider: "acp-claude"` → `vendor: "claude"`, `"acp-codex"` → `"codex"`, `"acp-gemini"` → `"gemini"`, `"acp-agy"` → `"agy"`, and likewise for all best-effort presets (`"acp-augment"` → `"augment"`, …). This is mechanical across ~40 rows.

- [ ] **Step 4: Rename the resolvers**

- `AcpTarget::for_provider` → `for_vendor`; body `.find(|a| a.vendor == vendor)`.
- The default-target test helper `AcpTarget::for_provider("acp-claude")` (line ~350) → `for_vendor("claude")`.
- `for_seat`: the fallback `AcpTarget::for_vendor(&seat.vendor)`.
- `QuarkKind::from_provider` → `from_vendor`; the `match provider` arms stay `"claude" => Claude`, `"agy" => Agy`, but reword the bail: `"unknown vendor {other:?} (expected \"claude\" or \"agy\")"`.
- `from_seat`: `Transport::Cli => QuarkKind::from_vendor(&seat.vendor)`.
- `provider_list()`: map `(a.vendor, a.name, a.program, a.args.to_vec())`.
- Update the catalogue round-trip test (`for_provider(a.provider)` near line 700 → `for_vendor(a.vendor)`), and `for_provider("acp-codex")`/`for_provider("no-such-agent")` in tests → `for_vendor("codex")`/`for_vendor("no-such-agent")`, and `acp.rs:1481` `for_provider("acp-agy")` → `for_vendor("agy")`.

- [ ] **Step 5: Run the test and the gate**

Run: `cargo test -p hadron-gluon catalogue_is_keyed_on_pure_vendor`
Expected: PASS.
Run: `cargo test --workspace --features gui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-gluon/src/adapter/registry.rs crates/hadron-gluon/src/adapter/acp.rs
git commit -m "refactor(gluon): re-key ACP catalogue on pure vendor (for_vendor/from_vendor)"
```

---

### Task 4: Legacy id-rename map + lattice rename pass

**Files:**
- Modify: `crates/hadron-lattice/src/team.rs` (add `legacy_id_renames`, `rename_legacy_ids`, `id_follows_convention`; export from crate root if needed)
- Test: inline `#[cfg(test)]` in `team.rs`

**Interfaces:**
- Produces:
  - `pub fn legacy_id_renames() -> &'static [(&'static str, &'static str)]` — the shared old→new map: `[("agy","cli-agy"), ("opus","cli-claude")]`.
  - `pub fn rename_legacy_ids(team: &mut Team)` — renames matching ids in both `team.quarks[].id` and `team.roster[].id`; idempotent.
  - `pub fn id_follows_convention(id: &str, transport: Transport) -> bool` — soft check: `id` starts with `"<transport>-"`.
- Consumes: `Team`, `Seat`, `SeatOverride`, `Transport`, `QuarkId`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `team.rs`:

```rust
#[test]
fn rename_legacy_ids_applies_the_map_to_quarks_and_roster_and_is_idempotent() {
    let mut team = Team {
        quarks: vec![
            Seat::cli(QuarkId::new("agy"), "agy", "flash", Flavor::Orchestrator),
            Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Worker),
        ],
        roster: vec![SeatOverride::role(QuarkId::new("agy"))],
        max_exchanges: None,
    };
    rename_legacy_ids(&mut team);
    assert_eq!(team.quarks[0].id.as_str(), "cli-agy");
    assert_eq!(team.quarks[1].id.as_str(), "cli-claude");
    assert_eq!(team.roster[0].id.as_str(), "cli-agy", "roster ids move too");

    let snapshot = team.clone();
    rename_legacy_ids(&mut team); // second run is a no-op
    assert_eq!(team, snapshot, "idempotent: nothing already-renamed changes");
}

#[test]
fn acp_ids_already_follow_convention_and_are_untouched() {
    let mut team = Team {
        quarks: vec![Seat {
            transport: Transport::Acp,
            ..Seat::cli(QuarkId::new("acp-claude"), "claude", "opus", Flavor::Worker)
        }],
        roster: vec![],
        max_exchanges: None,
    };
    rename_legacy_ids(&mut team);
    assert_eq!(team.quarks[0].id.as_str(), "acp-claude", "not in the map, unchanged");
    assert!(id_follows_convention("acp-claude", Transport::Acp));
    assert!(!id_follows_convention("agy", Transport::Cli));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p hadron-lattice rename_legacy_ids -- --nocapture`
Expected: FAIL to compile — `rename_legacy_ids` / `id_follows_convention` undefined.

- [ ] **Step 3: Implement the map, the pass, and the convention check**

Add to `team.rs`:

```rust
/// The one-shot legacy id renames, in one place so every consumer (the team.json pass
/// below and the chamber's ChamberPrefs key move) reads the SAME map. Only the two
/// built-ins that predate the `<transport>-<vendor>` convention; every other id is left
/// alone, so a user's custom id is never surprise-renamed.
pub fn legacy_id_renames() -> &'static [(&'static str, &'static str)] {
    &[("agy", "cli-agy"), ("opus", "cli-claude")]
}

/// Apply [`legacy_id_renames`] to a team in place: both full-seat ids and roster override
/// ids (a roster entry references a catalogue id, so it must move in lockstep). Idempotent
/// — an already-renamed id is not in the map's left column, so a second run changes nothing.
pub fn rename_legacy_ids(team: &mut Team) {
    let rename = |id: &mut QuarkId| {
        if let Some((_, new)) = legacy_id_renames().iter().find(|(old, _)| *old == id.as_str()) {
            *id = QuarkId::new(*new);
        }
    };
    for seat in &mut team.quarks {
        rename(&mut seat.id);
    }
    for ov in &mut team.roster {
        rename(&mut ov.id);
    }
}

/// Soft convention check: does `id` start with its transport prefix (`cli-`, `acp-`, `sdk-`)?
/// Advisory only — used to default new-seat ids and to warn, never to reject (custom ids like
/// `cli-agy-pro` stay legal).
pub fn id_follows_convention(id: &str, transport: Transport) -> bool {
    let prefix = match transport {
        Transport::Cli => "cli-",
        Transport::Acp => "acp-",
        Transport::Sdk => "sdk-",
    };
    id.starts_with(prefix)
}
```

If the crate root (`lib.rs`) re-exports team.rs items explicitly, add `legacy_id_renames, rename_legacy_ids, id_follows_convention` to that `pub use`.

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo test -p hadron-lattice rename_legacy_ids acp_ids_already_follow_convention`
Expected: PASS.
Run: `cargo test --workspace --features gui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-lattice/src/team.rs crates/hadron-lattice/src/lib.rs
git commit -m "feat(lattice): legacy id-rename map + pass (agy->cli-agy, opus->cli-claude)"
```

---

### Task 5: Chamber — move ChamberPrefs identity keys + split transport/vendor display

**Files:**
- Modify: `crates/hadron-chamber/src/config.rs` (add a `ChamberPrefs::rename_quark_ids` helper using the shared map)
- Modify: `crates/hadron-chamber/src/app/providers.rs` (`configured_providers`: transport from `seat.transport`; migration wrapper that renames ids on launch)
- Modify: `crates/hadron-chamber/src/app/render.rs:180,320`, `crates/hadron-chamber/src/app/widgets.rs:212-217` (show `transport · vendor · model`)
- Test: inline `#[cfg(test)]` in `config.rs`

**Interfaces:**
- Produces: `pub fn ChamberPrefs::rename_quark_ids(&mut self, renames: &[(&str, &str)])` — moves `quarks[old]` → `quarks[new]`.
- Consumes: `hadron_lattice::legacy_id_renames`, `ConfiguredQuark`, `Seat.{transport,vendor,model}`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `config.rs`:

```rust
#[test]
fn rename_quark_ids_moves_identity_to_the_new_key() {
    let mut prefs = ChamberPrefs::default();
    prefs.quarks.insert("agy".to_string(), Identity::default());
    prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
    assert!(prefs.quarks.contains_key("cli-agy"), "identity moved to the new id");
    assert!(!prefs.quarks.contains_key("agy"), "old key gone");
    // Idempotent: a second run finds nothing to move.
    prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
    assert!(prefs.quarks.contains_key("cli-agy"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --features gui rename_quark_ids_moves_identity -- --nocapture`
Expected: FAIL to compile — no `rename_quark_ids`.

- [ ] **Step 3: Implement the prefs key-move**

In `config.rs`, add to `impl ChamberPrefs`:

```rust
    /// Move per-quark identity (colour/name/avatar) to a renamed id, so the taxonomy
    /// migration does not reset a quark's appearance. Reads the SAME map as the team.json
    /// rename (`hadron_lattice::legacy_id_renames`) — passed in so the SSOT stays in lattice.
    pub fn rename_quark_ids(&mut self, renames: &[(&str, &str)]) {
        for (old, new) in renames {
            if let Some(identity) = self.quarks.remove(*old) {
                self.quarks.entry(new.to_string()).or_insert(identity);
            }
        }
    }
```

- [ ] **Step 4: Fix the transport/vendor display split**

In `providers.rs::configured_providers`, the `ConfiguredQuark.transport` field was being fed the vendor. Feed it the real transport and keep vendor separate for the roster row:

```rust
        .map(|seat| ConfiguredQuark {
            id: seat.id.0.clone(),
            transport: match seat.transport {
                hadron_lattice::Transport::Cli => "cli",
                hadron_lattice::Transport::Acp => "acp",
                hadron_lattice::Transport::Sdk => "sdk",
            }
            .to_string(),
            state: ProviderState::Ready { model: seat.model.clone() },
        })
```

In `render.rs` and `widgets.rs`, the roster-row / Settings detail currently reads `roster_row.provider` / `r.provider`. These come from the chamber's own roster-row struct (`RosterRow`) — locate its `provider` field (grep `struct RosterRow`), rename it to `vendor`, and add a `transport: String`. Render `format!("{} · {} · {}", transport, cap(&vendor), cap(&model))` in `widgets.rs:217` (and the empty-cases above it), and change the `render.rs:320` `kv_row("Provider", ...)` label to `kv_row("Vendor", roster_row.vendor.clone())` plus a `kv_row("Transport", roster_row.transport.clone())`. Update `model.rs` where `RosterRow` is populated (lines ~601/611) to fill both `vendor` and `transport` from the seat.

- [ ] **Step 5: Wire the id-rename into launch migration**

In `providers.rs`, next to `migrate_repo_to_catalogue`, add a launch-time pass that renames ids in both team files and the prefs, off the shared map. Call it from the same launch path that already calls `migrate_repo_to_catalogue` (grep its call site in `app/mod.rs`/`actions.rs`):

```rust
/// One-shot: rename legacy ids to the `<transport>-<vendor>` convention across the repo
/// team, the global catalogue, and the chamber's per-quark identity — all off the single
/// `legacy_id_renames` map. Idempotent; safe to call every launch.
pub(super) fn migrate_legacy_ids(
    repo_path: &std::path::Path,
    global_path: &std::path::Path,
    prefs: &mut ChamberPrefs,
) {
    for path in [repo_path, global_path] {
        let mut team = load_team(path);
        let before = team.clone();
        hadron_lattice::rename_legacy_ids(&mut team);
        if team != before {
            if let Err(e) = hadron_lattice::save_team(path, &team) {
                eprintln!("chamber: legacy id-rename failed to write {}: {e}", path.display());
            }
        }
    }
    prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
}
```

(Persist `prefs` via whatever save the chamber already uses after mutating `ChamberPrefs`.)

- [ ] **Step 6: Run the test and the gate**

Run: `cargo test -p hadron-chamber --features gui rename_quark_ids_moves_identity`
Expected: PASS.
Run: `cargo test --workspace --features gui`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/hadron-chamber/src/config.rs crates/hadron-chamber/src/app/providers.rs crates/hadron-chamber/src/app/render.rs crates/hadron-chamber/src/app/widgets.rs crates/hadron-chamber/src/model.rs
git commit -m "feat(chamber): migrate identity keys on id-rename; show transport·vendor·model"
```

---

### Task 6: New-seat id defaults to `<transport>-<vendor>` + back-compat resolve proof

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings.rs:1220` (new `ConfiguredQuark`/seat creation defaults the id)
- Test: inline `#[cfg(test)]` in `crates/hadron-lattice/src/team.rs` (the byte-for-byte resolve proof)

**Interfaces:**
- Consumes: `id_follows_convention`, `resolve_team`, `Seat`.

- [ ] **Step 1: Write the failing back-compat resolve test**

Add to the `tests` module in `team.rs`:

```rust
#[test]
fn a_pre_migration_team_resolves_to_the_same_seats_as_its_migrated_form() {
    // Legacy shape: smeared `provider`, legacy ids.
    let legacy = r#"{"quarks":[
        {"id":"agy","provider":"agy","model":"flash","flavor":"orchestrator","transport":"cli"},
        {"id":"acp-claude","provider":"acp-claude","model":"opus","flavor":"worker","transport":"acp"}
    ]}"#;
    let mut before = parse_team(legacy).unwrap();

    // Migrated shape: pure vendor + renamed cli- id, same behaviour.
    let migrated = r#"{"quarks":[
        {"id":"cli-agy","vendor":"agy","model":"flash","flavor":"orchestrator","transport":"cli"},
        {"id":"acp-claude","vendor":"claude","model":"opus","flavor":"worker","transport":"acp"}
    ]}"#;
    let after = parse_team(migrated).unwrap();

    // Vendor + transport + model + flavor must match seat-for-seat after the id-rename.
    rename_legacy_ids(&mut before);
    let empty = Team::default();
    let rb = resolve_team(&before, &empty);
    let ra = resolve_team(&after, &empty);
    let key = |t: &Team| t.quarks.iter()
        .map(|s| (s.id.0.clone(), s.vendor.clone(), s.transport, s.model.clone(), s.flavor.clone()))
        .collect::<Vec<_>>();
    assert_eq!(key(&rb), key(&ra), "legacy and migrated forms resolve identically");
}
```

- [ ] **Step 2: Run test to verify it fails or passes**

Run: `cargo test -p hadron-lattice a_pre_migration_team_resolves -- --nocapture`
Expected: PASS if Tasks 1+4 are correct (this test is the *proof* they compose). If it FAILS, the vendor normalization or the id-rename is wrong — fix before proceeding.

- [ ] **Step 3: Default the new-seat id in Settings**

At `settings.rs:1220` where a new `ConfiguredQuark` is pushed (the "add quark" wizard), when the user has chosen a transport + vendor but not typed an id, default it to `format!("{transport}-{vendor}")`. Use `id_follows_convention` to decide whether to warn (advisory `eprintln!`/status note) when a hand-typed id does not match — never block the save. Show the concrete code for the default:

```rust
let id = if typed_id.trim().is_empty() {
    format!("{transport}-{vendor}") // e.g. "cli-agy"
} else {
    typed_id.trim().to_string()
};
if !hadron_lattice::id_follows_convention(&id, transport_enum) {
    // advisory only: the convention is a nudge, not a gate (cli-agy-pro stays legal)
    eprintln!("chamber: note — id '{id}' does not match the '{transport}-' convention");
}
```

- [ ] **Step 4: Run the tests and the full gate**

Run: `cargo test -p hadron-lattice a_pre_migration_team_resolves`
Expected: PASS.
Run: `cargo test --workspace --features gui`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/src/app/settings.rs crates/hadron-lattice/src/team.rs
git commit -m "feat(chamber): default new-seat id to <transport>-<vendor>; prove back-compat resolve"
```

---

## Post-plan manual step (this repo only, not a task)

The dev repo's `.hadron/field.jsonl` and `.hadron/team.json` still reference the old ids in *history*. Per the migration decision, reset this repo's field rather than rewrite old-id actor references: stop any running gluon daemon, then archive/clear `.hadron/field.jsonl` and let the chamber's launch migration rewrite `team.json`/global to the new ids. Verify the swarm re-seats `cli-agy` (and any `acp-*`) cleanly before resuming work. Do this with no live daemon running (see the "live swarm shares the checkout" hazard).

## Self-Review

- **Spec §3.1 schema split** → Task 1 (`vendor`) + Task 2 (`Sdk`). ✓
- **Spec §3.2 catalogue re-key / transport-first dispatch** → Task 3 + Task 2 (`from_seat` Sdk arm). ✓
- **Spec §3.3(a) parse normalization** → Task 1 Steps 3–4; proof in Task 6 Step 1. ✓
- **Spec §3.3(b) id-rename shared map + ChamberPrefs move** → Task 4 (map + team pass) + Task 5 (prefs move + launch wiring). ✓
- **Spec §3.4 soft convention + default id** → Task 4 (`id_follows_convention`) + Task 6 (default + advisory warn). ✓
- **Spec §3.5 error handling** → Task 2 (Sdk bail) + Task 3 (unknown-vendor reword). ✓
- **Spec §3.6 chamber display** → Task 5 Step 4. ✓
- **Spec §6 testing (back-compat, vendor derivation, id-rename, sdk reserved, preserved invariants, gate)** → Tasks 1/2/3/4/6 tests + every task's full-gate step. ✓
- **Spec §7 security** → no new external input surface; unknown vendor/sdk bail without executing. ✓
- Placeholder scan: no TBD/TODO; every code step shows code. ✓
- Type consistency: `vendor` (not `provider`) after Task 1; `for_vendor`/`from_vendor` after Task 3; `legacy_id_renames`/`rename_legacy_ids`/`id_follows_convention`/`rename_quark_ids` names used consistently across Tasks 4–6. ✓
