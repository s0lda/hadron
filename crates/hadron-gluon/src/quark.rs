use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

/// A citizen of the field. The gluon never knows whether this is a CLI harness,
/// a native API worker, or a future ACP/MCP adapter — only this contract.
#[async_trait]
pub trait Quark: Send {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;
    fn energy(&self) -> EnergyState;
    /// Run one turn against a projection and return the field message (if any).
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>;
}
