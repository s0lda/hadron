# Capability-Gated Advanced Model Parameters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hide model parameters (Temperature, Top P, Max Tokens) in Chamber Settings for Quark seats that do not support them, and nest them inside a collapsible "Advanced Model Parameters" accordion section for seats that do support them.

**Architecture:** Extend `CliSpec` in `hadron-lattice` with optional model parameter CLI flags (`temperature_flag`, `top_p_flag`, `max_tokens_flag`) and add a `Seat::supports_model_params(&self)` capability method. Update `ChamberView` in `hadron-chamber` to track `settings_model_params_applies` and `settings_advanced_expanded`, hiding the parameter section when unsupported and rendering a collapsible accordion when supported.

**Tech Stack:** Rust, GPUI (`hadron-chamber`), `hadron-lattice` (`Seat`, `Transport`, `CliSpec`).

## Global Constraints

- **Single Source of Truth (SSOT)**: `Seat::supports_model_params(&self)` is the sole authority on whether model parameters apply to a Quark seat.
- **Backwards Compatibility**: Existing `CliSpec` deserialization defaults missing flag fields to `None`.
- **UI Invariant**: Settings auto-expands the "Advanced Model Parameters" accordion if any parameter is explicitly set, and keeps it collapsed by default otherwise.

---

### Task 1: Add parameter flag support to `CliSpec` and `supports_model_params` capability method to `Seat` in `hadron-lattice` (commit `e9a39b72`)

**Files:**
- Modify: `crates/hadron-lattice/src/team/transport.rs`
- Modify: `crates/hadron-lattice/src/team/seat.rs`
- Test: `crates/hadron-lattice/src/team/tests.rs`

**Interfaces:**
- Consumes: `Transport`, `CliSpec`, `Seat`
- Produces: `CliSpec::supports_model_params(&self) -> bool`, `Seat::supports_model_params(&self) -> bool`

- [ ] **Step 1: Write the failing tests**

In `crates/hadron-lattice/src/team/tests.rs`:
```rust
#[test]
fn seat_supports_model_params_capability() {
    let mut http_seat = Seat::default();
    http_seat.transport = Transport::Http;
    assert!(http_seat.supports_model_params(), "HTTP transport must support model params");

    let mut acp_seat = Seat::default();
    acp_seat.transport = Transport::Acp;
    assert!(acp_seat.supports_model_params(), "ACP transport must support model params");

    let mut cli_seat = Seat::default();
    cli_seat.transport = Transport::Cli;
    cli_seat.vendor = "claude".into();
    assert!(!cli_seat.supports_model_params(), "CLI seat without parameter flags must not support model params");

    let mut custom_cli_seat = Seat::default();
    custom_cli_seat.transport = Transport::Cli;
    let mut spec = CliSpec::generic("mycli".into(), vec![]);
    spec.temperature_flag = Some("--temperature".into());
    custom_cli_seat.cli = Some(spec);
    assert!(custom_cli_seat.supports_model_params(), "CLI seat with temperature_flag must support model params");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-lattice team::tests::seat_supports_model_params_capability`
Expected: FAIL with missing method `supports_model_params` or field `temperature_flag`

- [ ] **Step 3: Write minimal implementation**

In `crates/hadron-lattice/src/team/transport.rs`:
Add fields to `CliSpec`:
```rust
    /// Optional CLI flags for model parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_flag: Option<String>,
```
Add method to `CliSpec`:
```rust
impl CliSpec {
    pub fn supports_model_params(&self) -> bool {
        self.temperature_flag.is_some() || self.top_p_flag.is_some() || self.max_tokens_flag.is_some()
    }
}
```

In `crates/hadron-lattice/src/team/seat.rs`:
Add method to `Seat`:
```rust
impl Seat {
    pub fn supports_model_params(&self) -> bool {
        match self.transport {
            Transport::Http | Transport::Acp => true,
            Transport::Cli => {
                let spec = self.cli.as_ref().or_else(|| CliSpec::preset(&self.vendor).as_ref());
                spec.map(|s| s.supports_model_params()).unwrap_or(false)
            }
            Transport::Sdk => false,
        }
    }
}
```

Update `Seat` destructuring in `same_agent` if needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-lattice team::tests::seat_supports_model_params_capability`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-lattice/src/team/
git commit -m "feat(lattice): add supports_model_params capability check to Seat and CliSpec"
```

---

### Task 2: Integrate capability hiding and collapsible accordion into Chamber Settings UI

**Files:**
- Modify: `crates/hadron-chamber/src/app/mod.rs`
- Modify: `crates/hadron-chamber/src/app/settings/mod.rs`
- Modify: `crates/hadron-chamber/src/app/settings/overlay.rs`
- Test: `crates/hadron-chamber/src/model/tests.rs`

**Interfaces:**
- Consumes: `Seat::supports_model_params(&self)`
- Produces: `settings_model_params_applies: bool`, `settings_advanced_expanded: bool` in `ChamberView`

- [ ] **Step 1: Write the failing test**

In `crates/hadron-chamber/src/model/tests.rs`:
Add unit test verifying `supports_model_params` state handling when loading settings.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber model::tests`

- [ ] **Step 3: Write minimal implementation**

1. In `crates/hadron-chamber/src/app/mod.rs`:
Add fields to `ChamberView`:
```rust
    settings_model_params_applies: bool,
    settings_advanced_expanded: bool,
```
Initialize them to `false` in `ChamberView::new`.

2. In `crates/hadron-chamber/src/app/settings/mod.rs`:
Update `load_settings_inputs` to return `supports_model_params`:
```rust
let supports_params = if let Some(seat) = resolved.get(&QuarkId::new(key)).or_else(|| self.global.get(&QuarkId::new(key))) {
    seat.supports_model_params()
} else {
    false
};
```
And set state:
```rust
self.settings_model_params_applies = supports_params;
self.settings_advanced_expanded = !temp_str.is_empty() || !top_p_str.is_empty() || !max_tokens_str.is_empty();
```

3. In `crates/hadron-chamber/src/app/settings/overlay.rs`:
Wrap the `Temperature`, `Top P`, and `Max tokens` input controls inside `.when(self.settings_model_params_applies, |v| ...)` and render the collapsible accordion header:
```rust
.when(self.settings_model_params_applies, |v| {
    v.child(
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_advanced_expanded = !this.settings_advanced_expanded;
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Icon::new(if self.settings_advanced_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                text("Advanced Model Parameters")
                                    .size_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground),
                            ),
                    ),
            )
            .when(self.settings_advanced_expanded, |adv| {
                adv.child(settings_field(
                    "Temperature",
                    Some("Sampling temperature (e.g. 0.1 for code, 0.8 for creative). Blank = vendor default."),
                    Input::new(&self.settings_temperature).w_full().into_any_element(),
                ))
                .child(settings_field(
                    "Top P",
                    Some("Nucleus sampling probability (e.g. 0.95). Blank = vendor default."),
                    Input::new(&self.settings_top_p).w_full().into_any_element(),
                ))
                .child(settings_field(
                    "Max tokens",
                    Some("Max response token limit. Blank = vendor default."),
                    Input::new(&self.settings_max_tokens).w_full().into_any_element(),
                ))
            }),
    )
})
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-chamber/
git commit -m "feat(chamber): capability-gate and collapse model parameters in Settings UI"
```

---

### Task 3: Full Workspace Verification

**Files:**
- All touched files across `hadron-lattice` and `hadron-chamber`

- [ ] **Step 1: Run full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS (all tests pass)

- [ ] **Step 2: Commit plan status update**

```bash
git add .hadron/docs/plans/2026-08-11-capability-gated-advanced-model-parameters.md
git commit -m "docs: complete capability-gated advanced model parameters plan"
```
