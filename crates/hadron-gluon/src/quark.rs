use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

/// A citizen of the field. The gluon never knows whether this is a CLI harness,
/// a native API worker, or a future ACP/MCP adapter — only this contract.
#[async_trait]
pub trait Quark: Send {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;
    /// The human-facing name the router matches `@mentions` against (e.g. `@Claude` for
    /// the seat whose id is `acp-claude`). `None` means "only the id is addressable".
    /// Carried on the quark so the engine's roster card is always built with the right
    /// name — including after a re-seat, where a name populated out-of-band would be lost.
    /// The name is resolved from the (global) team config; the adapter merely holds it.
    fn display_name(&self) -> Option<String> {
        None
    }
    fn energy(&self) -> EnergyState;
    /// Whether this quark keeps its context **across turns** (a resident ACP session) or
    /// is re-spawned fresh each turn (a one-shot CLI process). The engine tracks this at
    /// seat time (`Engine::resident`) for whatever needs a seat's transport shape; skill
    /// injection no longer branches on it — resident and one-shot quarks alike now get
    /// the always-on index plus the active skill's full body, nothing more (WS4 §5).
    /// Defaults to `false` — most transports are one-shot; only residency is special.
    fn resident(&self) -> bool {
        false
    }
    /// Run one turn against a projection and return the field message (if any).
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>;

    /// Reap any **resident** state (a live agent subprocess and its open session) so the
    /// next turn boots fresh. This is the human's force-restart: an ACP quark drops its
    /// session (killing the subprocess); a one-shot CLI quark holds nothing between turns,
    /// so the default is a no-op. Idempotent — calling it on a quark with no live session
    /// does nothing.
    fn reset_session(&mut self) {}
}
