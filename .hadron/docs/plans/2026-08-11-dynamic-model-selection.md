# Dynamic Model Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable dynamic model discovery and UI selection for both the Antigravity SDK Python Bridge (`agy_acp.py`) and CLI Quarks (`CliSpec`) in Hadron Chamber Settings.

**Architecture:** Update `agy_acp.py` to advertise all supported Gemini models in ACP `configOptions` and handle dynamic model switching via `session/set_config_option`. Extend `CliSpec` with an optional `model_probe` field (e.g. `agy models`), implement CLI model probing in `hadron-gluon`, and wire both ACP and CLI model probes into Chamber Settings UI dropdowns.

**Tech Stack:** Python (Antigravity SDK), Rust (hadron-lattice, hadron-gluon, hadron-chamber, GPUI).

## Global Constraints

- **Single Source of Truth (SSOT)**: Model selectors use ACP standard `SessionConfigOptionCategory::Model` and `SessionConfigId("model")`.
- **Non-blocking UI**: All model probing (ACP or CLI executable) MUST run on background threads with timeouts.
- **Backwards Compatibility**: Existing `CliSpec` seats without `model_probe` retain their static text inputs.

---

### Task 1: Dynamic Model Discovery & Selection in `agy_acp.py` (Completed - commit a714b785)

**Files:**
- Modify: `crates/hadron-gluon/scripts/agy_acp.py`

**Interfaces:**
- Consumes: JSON-RPC requests (`session/new`, `session/set_config_option`, `session/prompt`)
- Produces: ACP `configOptions` listing Gemini models, dynamically configured `google.antigravity.Agent` instances

- [x] **Step 1: Write failing Python unit test for `agy_acp.py` model options**

Create `crates/hadron-gluon/scripts/test_agy_acp_models.py`:
```python
import json
import pytest
from agy_acp import session_config_response, SUPPORTED_MODELS

def test_session_config_response_lists_multiple_models():
    resp = session_config_response("test-session-123")
    assert resp["sessionId"] == "test-session-123"
    opts = resp["configOptions"]
    assert len(opts) == 1
    model_opt = opts[0]
    assert model_opt["id"] == "model"
    assert model_opt["category"] == "model"
    assert len(model_opt["options"]) >= 5
    assert any(o["value"] == "gemini-3.6-flash" for o in model_opt["options"])
    assert any(o["value"] == "gemini-3.6-pro" for o in model_opt["options"])
```

- [ ] **Step 2: Run Python test to verify it fails**

Run: `python3 -m pytest crates/hadron-gluon/scripts/test_agy_acp_models.py`
Expected: FAIL (cannot import `SUPPORTED_MODELS` / options length == 1)

- [ ] **Step 3: Implement dynamic models and `session/set_config_option` in `agy_acp.py`**

In `crates/hadron-gluon/scripts/agy_acp.py`:
```python
SUPPORTED_MODELS = [
    {"value": "gemini-3.6-flash", "name": "Gemini 3.6 Flash"},
    {"value": "gemini-3.6-pro", "name": "Gemini 3.6 Pro"},
    {"value": "gemini-3.5-flash", "name": "Gemini 3.5 Flash"},
    {"value": "gemini-3.5-pro", "name": "Gemini 3.5 Pro"},
    {"value": "gemini-3.1-pro", "name": "Gemini 3.1 Pro"},
]

def session_config_response(session_id):
    current_model = sessions.get(session_id, {}).get("model", SEAT_MODEL)
    return {
        "sessionId": session_id,
        "configOptions": [
            {
                "id": "model",
                "name": "Model",
                "type": "select",
                "category": "model",
                "currentValue": current_model,
                "options": SUPPORTED_MODELS
            }
        ]
    }
```
Update `session/set_config_option` handler:
```python
elif method == "session/set_config_option":
    session_id = params.get("sessionId")
    config_id = params.get("configId")
    value = str(params.get("value"))
    if session_id not in sessions:
        send_error(msg_id, f"unknown session {session_id!r}")
    elif config_id == "model":
        old_model = sessions[session_id].get("model")
        sessions[session_id]["model"] = value
        if old_model != value and sessions[session_id].get("agent") is not None:
            sessions[session_id]["agent"] = None
        send_response(msg_id, {})
```
And in `ensure_agent(session_data)`:
```python
    model_name = session_data.get("model", SEAT_MODEL)
    agent = Agent(
        config=LocalAgentConfig(
            api_key=api_key,
            model=model_name,
            policies=[policy.allow_all()],
            workspaces=[cwd] if cwd else None
        )
    )
```

- [ ] **Step 4: Run Python test to verify it passes**

Run: `python3 -m pytest crates/hadron-gluon/scripts/test_agy_acp_models.py`
Expected: PASS

- [ ] **Step 5: Commit changes**

```bash
git add crates/hadron-gluon/scripts/agy_acp.py crates/hadron-gluon/scripts/test_agy_acp_models.py
git commit -m "feat(acp): add multi-model support and dynamic switching to agy_acp.py"
```

---

### Task 2: CLI Model Probing Data Structures & Prober

**Files:**
- Modify: `crates/hadron-lattice/src/team/transport.rs`
- Modify: `crates/hadron-gluon/src/adapter/cli.rs`
- Modify: `crates/hadron-gluon/src/adapter/registry/tests.rs`

**Interfaces:**
- Consumes: `CliSpec`
- Produces: `CliProbeSpec`, `probe_cli_models(&CliSpec) -> anyhow::Result<ModelSelector>`

- [ ] **Step 1: Add `CliProbeSpec` to `hadron-lattice/src/team/transport.rs` and write unit test**

In `crates/hadron-lattice/src/team/transport.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliProbeSpec {
    pub args: Vec<String>,
}
```
Add `pub model_probe: Option<CliProbeSpec>` to `CliSpec`.
Update `CliSpec::agy()`:
```rust
model_probe: Some(CliProbeSpec { args: vec!["models".to_string()] }),
```
In `crates/hadron-lattice/src/team/tests.rs`, add a test verifying `CliSpec::agy().model_probe` is `Some`.

- [ ] **Step 2: Run test to verify it compiles and passes**

Run: `cargo test -p hadron-lattice team::tests`
Expected: PASS

- [ ] **Step 3: Implement `probe_cli_models` in `hadron-gluon/src/adapter/cli.rs`**

In `crates/hadron-gluon/src/adapter/cli.rs`:
```rust
use crate::adapter::acp::{AcpModel, ModelSelector};
use agent_client_protocol::schema::v1::SessionConfigId;

pub fn probe_cli_models(spec: &CliSpec) -> anyhow::Result<ModelSelector> {
    let probe_spec = spec.model_probe.as_ref().ok_or_else(|| anyhow::anyhow!("no model_probe configured"))?;
    let output = std::process::Command::new(&spec.program)
        .args(&probe_spec.args)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("model probe process exited with status {}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut available = Vec::new();
    for line in stdout.lines() {
        let clean = line.trim();
        if clean.is_empty() || clean.contains("Fetching available models") {
            continue;
        }
        let parts: Vec<&str> = clean.split_whitespace().collect();
        if !parts.is_empty() {
            let value = parts[0].to_string();
            let label = if parts.len() > 1 { parts[1..].join(" ") } else { value.clone() };
            available.push(AcpModel { value, label });
        }
    }
    Ok(ModelSelector {
        config_id: SessionConfigId("model".to_string()),
        current: "".to_string(),
        available,
    })
}
```

- [ ] **Step 4: Run tests to verify implementation**

Run: `cargo test -p hadron-gluon adapter::cli::tests`
Expected: PASS

- [ ] **Step 5: Commit changes**

```bash
git add crates/hadron-lattice/src/team/transport.rs crates/hadron-gluon/src/adapter/cli.rs crates/hadron-lattice/src/team/tests.rs
git commit -m "feat(cli): add model_probe to CliSpec and implement probe_cli_models"
```

---

### Task 3: Integrate CLI & ACP Model Probing in Chamber Settings UI

**Files:**
- Modify: `crates/hadron-chamber/src/app/settings/acp_probe.rs`
- Modify: `crates/hadron-chamber/src/app/settings/providers.rs`

**Interfaces:**
- Consumes: `AcpTarget` or `CliSpec`
- Produces: `AcpModelProbe` containing probed models for UI rendering

- [ ] **Step 1: Update `start_acp_model_probe` in `acp_probe.rs` to probe CLI models**

In `crates/hadron-chamber/src/app/settings/acp_probe.rs`:
```rust
pub(super) fn start_acp_model_probe(&mut self, id: &str, cx: &mut Context<Self>) {
    let seat_opt = resolve_team(&self.team, &self.global).get(&QuarkId::new(id)).cloned();
    let Some(seat) = seat_opt else {
        self.acp_model_probe = None;
        return;
    };

    let id_str = id.to_string();
    if seat.transport == hadron_lattice::Transport::Acp {
        let target = hadron_gluon::adapter::registry::AcpTarget::for_seat_with_env(&seat, self.secret_store.as_ref());
        let Some(target) = target else {
            self.acp_model_probe = None;
            return;
        };
        self.acp_model_probe = Some(AcpModelProbe { id: id_str.clone(), state: AcpModelState::Probing });
        cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = cx.background_spawn(async move {
                    hadron_gluon::adapter::acp::probe_selectors(&target)
                }).await;
                this.update(&mut cx, |this, cx| {
                    if !matches!(&this.acp_model_probe, Some(p) if p.id == id_str) { return; }
                    let state = match result {
                        Ok(sel) if sel.model.is_none() => AcpModelState::Unavailable("this agent offers no model picker".into()),
                        Ok(sel) => AcpModelState::Ready { selectors: sel },
                        Err(e) => AcpModelState::Unavailable(format!("couldn't detect models: {e}")),
                    };
                    this.acp_model_probe = Some(AcpModelProbe { id: id_str, state });
                    cx.notify();
                }).ok();
            }
        }).detach();
    } else if seat.transport == hadron_lattice::Transport::Cli {
        let cli_spec = hadron_lattice::CliSpec::preset(&seat.vendor)
            .unwrap_or_else(|| hadron_lattice::CliSpec::generic(seat.program.clone(), vec![]));
        if cli_spec.model_probe.is_some() {
            self.acp_model_probe = Some(AcpModelProbe { id: id_str.clone(), state: AcpModelState::Probing });
            cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx.background_spawn(async move {
                        hadron_gluon::adapter::cli::probe_cli_models(&cli_spec)
                    }).await;
                    this.update(&mut cx, |this, cx| {
                        if !matches!(&this.acp_model_probe, Some(p) if p.id == id_str) { return; }
                        let state = match result {
                            Ok(selector) => AcpModelState::Ready {
                                selectors: hadron_gluon::adapter::acp::AcpSelectors {
                                    model: Some(selector),
                                    ..Default::default()
                                }
                            },
                            Err(e) => AcpModelState::Unavailable(format!("couldn't detect models: {e}")),
                        };
                        this.acp_model_probe = Some(AcpModelProbe { id: id_str, state });
                        cx.notify();
                    }).ok();
                }
            }).detach();
        } else {
            self.acp_model_probe = None;
        }
    } else {
        self.acp_model_probe = None;
    }
}
```

- [ ] **Step 2: Update Settings UI in `providers.rs` to render model select for probing CLI seats**

In `crates/hadron-chamber/src/app/settings/providers.rs`:
Update model field rendering logic so that if `acp_model_probe` is ready for the current seat (whether ACP or CLI), it renders `acp_model_select` dropdown instead of static text input.

- [ ] **Step 3: Run workspace compilation and test suite**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 4: Commit changes**

```bash
git add crates/hadron-chamber/src/app/settings/acp_probe.rs crates/hadron-chamber/src/app/settings/providers.rs
git commit -m "feat(ui): render probed model dropdown for both ACP seats and probing CLI seats"
```
