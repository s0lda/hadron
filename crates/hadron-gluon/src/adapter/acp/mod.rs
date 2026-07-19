//! The ACP transport: a quark backed by a **resident** agent subprocess speaking
//! the Agent Client Protocol (JSON-RPC 2.0 over stdio).
//!
//! Hadron is the ACP **client**; the seated agent is the ACP **agent**. Where the
//! CLI adapters spawn a fresh process per turn and re-send the whole conversation
//! through argv/stdin, this boots the agent **once** and holds the connection: the
//! agent keeps the conversation, and a turn is a `session/prompt` on an existing
//! session.
//!
//! ## Why a thread, not a task
//!
//! The SDK's connection API is *scoped*: `connect_with(transport, |cx| async { … })`
//! runs the connection for exactly as long as its closure. That is a fine shape for
//! a one-shot client and the wrong shape for a `Quark`, whose `excite` is called
//! once per turn over minutes. So the closure is inverted into a **turn pump**: it
//! parks on an mpsc of turn requests and only returns when the channel is dropped,
//! which is what makes the session resident.
//!
//! The SDK is built on the `futures`/`async-process`/`blocking` stack rather than
//! tokio, so the pump gets a dedicated OS thread driven by `futures::executor::block_on`
//! instead of a `tokio::spawn`. `tokio::sync`'s channels are runtime-agnostic, so
//! they bridge the two sides cleanly. One thread per ACP quark, parked on a channel.
//!
//! ## What the agent tells us, and what we do with it
//!
//! - `session/update` → `agent_message_chunk`: the **only** place the reply text
//!   lives (`PromptResponse` carries no content), so we accumulate it. This is not
//!   the streaming feature — nothing is surfaced mid-turn; `excite` still returns
//!   once. It is simply how ACP hands over a message.
//! - `session/update` → `usage_update` `{ used, size }`: context tokens and the
//!   model's **real** window size. Straight into [`ContextUsage`].
//! - the `session/prompt` response → `usage` (feature `unstable_end_turn_token_usage`):
//!   cumulative token totals for the session. The per-turn cost is the **delta**.
//!   See [`turn_spend`].

use std::path::PathBuf;

use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, SeatCommands, TurnOutcome};

use crate::adapter::registry::AcpTarget;
use crate::adapter::runner::RedactedEnv;
use crate::quark::Quark;

mod model;
mod session;
mod spend;
#[cfg(test)]
mod tests;

use session::{AcpSession, LiveFeed};

// Re-exported: `AcpModel`/`probe`/`probe_selector` are the crate's public ACP
// surface (the chamber's Settings "Connect" wizard and model dropdown call these
// at `hadron_gluon::adapter::acp::{probe, probe_selector, AcpModel}`), so moving
// their implementation into the private `model` submodule must not change their
// public path. `turn_spend`/`SpendWatermark` were likewise `pub` at
// `hadron_gluon::adapter::acp::` before the split; re-exporting keeps that path
// (and brings `SpendWatermark` into scope for the struct field below).
pub use model::{probe, probe_selector, AcpModel};
pub use spend::{turn_spend, SpendWatermark};

pub struct AcpQuark {
    id: QuarkId,
    flavor: Flavor,
    /// The `@mention` name (see [`Quark::display_name`]); `None` = id-only.
    display_name: Option<String>,
    /// This quark's `@role` roles (see [`Quark::roles`]); empty = no roles.
    roles: Vec<String>,
    /// Whether this quark is scoped only to its roles (see [`Quark::exclusive`]).
    exclusive: bool,
    /// This quark's per-seat command allow/deny lists (see [`Quark::commands`]).
    commands: SeatCommands,
    /// This seat's resolved secret env — `(name, value)` pairs from
    /// `Seat::resolve_env`. Carried into the ACP boot's JSON stdio descriptor's
    /// `env` array (see [`acp_stdio_descriptor`]) and NOWHERE else.
    env: RedactedEnv,
    /// The model this seat **asks** for. It is not necessarily the one that runs: the
    /// agent advertises what it can offer on `session/new` and we match against that
    /// (see [`model_selector`] and [`resolve_model`]). The model that actually ran is
    /// on [`AcpSession::model`], because only the agent knows it.
    model: String,
    effort: Option<String>,
    mode_config: Option<String>,
    /// How to boot this agent.
    target: AcpTarget,
    /// `None` until the first turn: booting is lazy, exactly as the CLI path spawns
    /// nothing until `excite`.
    session: Option<AcpSession>,
    /// The watermark for [`turn_spend`].
    last_spend: SpendWatermark,
    /// Where to publish mid-turn activity. `None` = nobody is watching (tests, and
    /// any caller that has no field on disk), and the stream is simply dropped.
    live: Option<LiveFeed>,
    energy_limit: Option<u32>,
}

impl AcpQuark {
    pub fn new(id: QuarkId, flavor: Flavor, model: impl Into<String>, effort: Option<String>, mode_config: Option<String>, target: AcpTarget) -> Self {
        AcpQuark {
            id,
            flavor,
            display_name: None,
            roles: Vec::new(),
            exclusive: false,
            commands: SeatCommands::default(),
            env: RedactedEnv::default(),
            model: model.into(),
            effort,
            mode_config,
            target,
            session: None,
            last_spend: SpendWatermark::default(),
            live: None,
            energy_limit: None,
        }
    }

    /// Set the energy limit.
    pub fn with_energy_limit(mut self, limit: Option<u32>) -> Self {
        self.energy_limit = limit;
        self
    }

    /// Stream this quark's mid-turn activity into `dir` (see `hadron_lattice::live`).
    /// The daemon calls this; a test that has no field does not, and the quark then
    /// publishes nothing.
    pub fn watching(mut self, dir: PathBuf) -> Self {
        self.live = Some(LiveFeed {
            dir,
            quark: self.id.clone(),
            last: std::sync::Arc::new(std::sync::Mutex::new(None)),
        });
        self
    }

    /// Set the `@mention` display name (from the resolved team config).
    pub fn with_display_name(mut self, name: Option<String>) -> Self {
        self.display_name = name;
        self
    }

    /// Set the `@role` roles and exclusivity (from the resolved seat).
    pub fn with_roles(mut self, roles: Vec<String>, exclusive: bool) -> Self {
        self.roles = roles;
        self.exclusive = exclusive;
        self
    }

    /// Set the per-seat command allow/deny lists (from the resolved seat).
    pub fn with_commands(mut self, commands: SeatCommands) -> Self {
        self.commands = commands;
        self
    }

    /// Set this seat's resolved secret env (from `Seat::resolve_env`), carried into
    /// the boot's JSON stdio descriptor.
    pub fn with_env(mut self, env: impl Into<RedactedEnv>) -> Self {
        self.env = env.into();
        self
    }

    /// The model the agent reported it is **actually** running, once a session is open.
    ///
    /// **Implemented, unwired.** Nothing consumes this yet, and that is a deliberate
    /// stopping point rather than an oversight: its home is a `model` field on
    /// `hadron_lattice::Usage`, so that a turn's telemetry records the model that ran
    /// and a turn can finally be *priced* (you cannot cost a turn you cannot attribute
    /// to a model). Adding that field touches every exhaustive `Usage { .. }` literal,
    /// two of which are in files another quark is mid-write in. It lands next, on a
    /// tree that is not moving.
    pub fn running_model(&self) -> Option<String> {
        self.session.as_ref()?.model.lock().unwrap().clone()
    }
}

#[async_trait]
impl Quark for AcpQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }
    fn roles(&self) -> Vec<String> {
        self.roles.clone()
    }
    fn exclusive(&self) -> bool {
        self.exclusive
    }
    fn commands(&self) -> &SeatCommands {
        &self.commands
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    fn energy_limit(&self) -> Option<u32> {
        self.energy_limit
    }
    /// An ACP quark is a **resident** session: the agent is booted once and keeps the
    /// conversation across turns, so the skill library injected on the first turn stays
    /// in its context (and is a prompt-cache read thereafter).
    fn resident(&self) -> bool {
        true
    }

    /// The turn ends the moment this returns, however it returns. Clearing the live
    /// feed here — rather than on the happy path inside [`AcpQuark::run_turn`] — is
    /// what makes "a turn that died still goes idle" true by construction: a quark
    /// whose agent crashed must not sit in the chamber `thinking` forever.
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let outcome = self.run_turn(turn).await;
        if let Some(feed) = &self.live {
            feed.clear();
        }
        outcome
    }

    /// Force-restart: drop the resident session. Dropping the [`AcpSession`] drops the
    /// `turns` channel, which ends the pump thread, tears down the connection, and reaps
    /// the agent subprocess (see the struct doc). The next turn re-boots from scratch.
    /// A no-op if no session is open, so it is safe to call on an idle quark.
    fn reset_session(&mut self) {
        self.session = None;
    }
}
