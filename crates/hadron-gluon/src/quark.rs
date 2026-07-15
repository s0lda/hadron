use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

/// A citizen of the field. The gluon never knows whether this is a CLI harness,
/// a native API worker, or a future ACP/MCP adapter — only this contract.
#[async_trait]
pub trait Quark: Send {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;
    fn energy(&self) -> EnergyState;
    /// Whether this quark keeps its context **across turns** (a resident ACP session) or
    /// is re-spawned fresh each turn (a one-shot CLI process). The engine uses it to
    /// decide how to hand over skills: a resident quark gets the whole skill library once
    /// in its cache-stable prefix (so composition is free and it persists); a one-shot
    /// quark, which remembers nothing, gets only the selected skill's body each turn.
    /// Defaults to `false` — most transports are one-shot; only residency is special.
    fn resident(&self) -> bool {
        false
    }
    /// Run one turn against a projection and return the field message (if any).
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>;
}
