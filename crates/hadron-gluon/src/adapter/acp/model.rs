use std::str::FromStr;

use hadron_lattice::Mode;

use agent_client_protocol::schema::v1::{
    InitializeRequest, NewSessionRequest, PermissionOptionKind, SessionConfigId,
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};

use crate::adapter::registry::AcpTarget;

/// Translate the resolved permission mode into an answer to ACP's *blocking*
/// `session/request_permission`.
///
/// This is deliberately the **narrow** version. ACP can express real per-tool,
/// human-in-the-loop gating (the agent blocks until we answer, and the options carry
/// `AllowAlways` / `RejectAlways` — the trust-on-first-use kinds the CLI path cannot
/// express). Wiring that to hadron's field-driven grant flow is a separate piece of
/// work, because the human's answer arrives asynchronously via the field while the
/// JSON-RPC call is held open. Until then we answer from the turn's posture alone:
///
/// - **Ask / Write** → reject. The quark may talk, not act unattended.
/// - **Auto / Bypass** → allow once.
///
/// `AllowAlways` is never selected: remembering a grant is the field's job, and this
/// function has no way to record one. Erring toward `*_once` keeps the blast radius
/// of a mistake to a single tool call.
pub(super) fn permission_choice(mode: Mode) -> PermissionOptionKind {
    match mode {
        Mode::Ask | Mode::Write => PermissionOptionKind::RejectOnce,
        Mode::Auto | Mode::Bypass => PermissionOptionKind::AllowOnce,
    }
}

/// One model the seated agent says it can actually run: the id that goes on the wire,
/// and the label a human should see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpModel {
    pub value: String,
    pub label: String,
}

/// The agent's **model selector**, exactly as it advertised it on `session/new`.
///
/// This is not a thing we invent — it is a thing the agent hands us and which, until
/// today, Hadron threw away. `config_options` is on `NewSessionResponse` in **v1**;
/// the model picker never needed a protocol migration, only a reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelector {
    /// What `session/set_config_option` must name to change it.
    pub config_id: SessionConfigId,
    pub current: String,
    pub available: Vec<AcpModel>,
}

/// Find the model selector among the options the agent advertised.
///
/// Selection is by `category == Model`, **not** by matching the option's id against a
/// name we guessed: an id like `"model"` is that agent's private business, and a client
/// that hard-codes it works for exactly one agent. The category is the contract.
///
/// Boolean options (`Fast mode`) and non-model selects (`Mode`, `Thought level`) are
/// not models and are ignored here.
pub fn model_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::Model)
}

pub fn effort_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::ThoughtLevel)
}

pub fn mode_selector(options: &[SessionConfigOption]) -> Option<ModelSelector> {
    config_selector(options, SessionConfigOptionCategory::Mode)
}

pub fn config_selector(options: &[SessionConfigOption], category: SessionConfigOptionCategory) -> Option<ModelSelector> {
    let opt = options
        .iter()
        .find(|o| o.category.as_ref() == Some(&category))?;

    let SessionConfigKind::Select(select) = &opt.kind else {
        // A model you cannot choose from a list is not a picker. Say nothing rather
        // than guess a shape.
        return None;
    };

    // The agent may group its models (e.g. by family). A group is a UI affordance;
    // for choosing, flatten it.
    let available: Vec<AcpModel> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(opts) => opts
            .iter()
            .map(|o| AcpModel { value: o.value.to_string(), label: o.name.clone() })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|g| g.options.iter())
            .map(|o| AcpModel { value: o.value.to_string(), label: o.name.clone() })
            .collect(),
        // The enum is `#[non_exhaustive]`: a future ACP may add a shape we have never
        // seen. An unknown shape means we cannot enumerate the models — which is not
        // the same as the agent having none, so offer nothing rather than a wrong list.
        _ => return None,
    };

    Some(ModelSelector {
        config_id: opt.id.clone(),
        current: select.current_value.to_string(),
        available,
    })
}

/// Resolve what the **seat** asked for against what the **agent** actually offers, and
/// return the wire value to set — or `None` to leave the agent's own default alone.
///
/// Matching is deliberately forgiving, because a seat's `model` is typed by a human:
/// exact id first, then the human label, then a case-insensitive substring of either.
/// `"opus"` should find `"claude-opus-4-8"`, and `"Sonnet"` should find `"Sonnet 4.5"`.
///
/// It returns `None` when the seat asked for nothing, when the request is *already* the
/// current model, or when nothing matches. That last case is the important one: an
/// unmatched model is **not** an error that should kill the turn — the agent has a
/// perfectly good default — but it must be visible, so the caller warns.
pub fn resolve_model(selector: &ModelSelector, wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        return None;
    }
    let lower = wanted.to_lowercase();

    let hit = selector
        .available
        .iter()
        .find(|m| m.value == wanted)
        .or_else(|| selector.available.iter().find(|m| m.value.eq_ignore_ascii_case(wanted)))
        .or_else(|| selector.available.iter().find(|m| m.label.eq_ignore_ascii_case(wanted)))
        .or_else(|| {
            selector.available.iter().find(|m| {
                m.value.to_lowercase().contains(&lower) || m.label.to_lowercase().contains(&lower)
            })
        })?;

    // Already there. Setting it again is a needless round trip, and it would make the
    // "we switched the model" log line a lie.
    if hit.value == selector.current {
        return None;
    }
    Some(hit.value.clone())
}

/// Boot an ACP agent, complete the `initialize` handshake, read back who answered,
/// and shut it down. **Blocking** — call it off the UI thread.
///
/// This is what "Connect" in Settings means: proof that the command in the seat
/// actually boots and speaks ACP, before the human is told the provider is ready.
/// It deliberately opens **no session** and answers **no permission request** — a
/// session is a turn, a turn is the daemon's job, and a UI that can approve a tool
/// call is a permission ladder with a hole in it.
///
/// Returns the agent's own name (ACP's `agent_info`), or the reason it failed.
pub fn probe(target: &AcpTarget) -> anyhow::Result<String> {
    let (name, opts) = probe_session(target)?;
    // The handshake result the human is shown: the agent's own current model when it
    // offers a picker, else the agent's name — proof that *something* answered.
    Ok(match model_selector(&opts) {
        Some(selector) => selector.current,
        None => name.unwrap_or_else(|| "unnamed agent".into()),
    })
}

/// The agent's advertised **model selector** — the offered models plus its current
/// (default) pick — or `None` when the agent offers no model picker. Same boot as
/// [`probe`]; the chamber re-probes with this each time an ACP quark's Settings open,
/// so the model dropdown reflects the agent's live lineup rather than a cached guess.
pub fn probe_selector(target: &AcpTarget) -> anyhow::Result<Option<ModelSelector>> {
    let (_name, opts) = probe_session(target)?;
    Ok(model_selector(&opts))
}

use super::session::acp_stdio_descriptor;

/// Boot an ACP agent, complete `initialize` + `session/new`, read back the agent's
/// name and its advertised `config_options`, and shut it down. The shared core of
/// [`probe`] and [`probe_selector`] — one boot path, so a change to the handshake
/// (or the 120s guard) can never drift between the two callers. **Blocking.**
fn probe_session(target: &AcpTarget) -> anyhow::Result<(Option<String>, Vec<SessionConfigOption>)> {
    type Probed = (Option<String>, Vec<SessionConfigOption>);
    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<Probed>>();

    let target_clone = target.clone();
    // Same shape as `boot`: the SDK's connection API is scoped to its closure and
    // wants its own executor, so it gets its own thread.
    std::thread::Builder::new()
        .name("hadron-acp-probe".to_string())
        .spawn(move || {
            let outcome: anyhow::Result<Probed> = futures::executor::block_on(async move {
                let display_command = target_clone.command_line();
                let agent_source = acp_stdio_descriptor(&target_clone.program, &target_clone.args, &target_clone.env);
                let agent = AcpAgent::from_str(&agent_source)
                    .map_err(|e| anyhow::anyhow!("bad ACP command {display_command:?}: {e}"))?;
                let probed = agent_client_protocol::Client
                    .builder()
                    .name("hadron")
                    .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
                        let init = cx
                            .send_request(InitializeRequest::new(ProtocolVersion::V1))
                            .block_task()
                            .await?;
                        let sess = cx
                            .send_request(NewSessionRequest::new(std::env::temp_dir()))
                            .block_task()
                            .await?;
                        let opts = sess.config_options.unwrap_or_default();
                        Ok((init.agent_info.map(|i| i.name), opts))
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("ACP handshake failed: {e}"))?;
                Ok(probed)
            });
            let _ = tx.send(outcome);
        })?;

    // A boot that never answers must fail, not hang the Settings window forever.
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(result) => result,
        Err(_) => anyhow::bail!("ACP agent did not answer `initialize` within 120s"),
    }
}
