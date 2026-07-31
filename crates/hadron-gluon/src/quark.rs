use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

/// A closure a filled [`CancelSlot`] calls to ask its current session to
/// gracefully cancel an in-flight turn. `Fn`, not `FnMut`: the same slot may
/// be read concurrently by the engine while the quark that filled it is busy.
type CancelFn = std::sync::Arc<dyn Fn() -> bool + Send + Sync>;

/// A cancel handle for whichever session (if any) currently occupies a seat's
/// lane. Handed to a quark ONCE, at seating, via [`Quark::attach_cancel_slot`]
/// — the exact calling convention [`Quark::become_chat_lane`] already
/// established. The quark fills it with a live cancel closure whenever it has
/// something cancellable (e.g. a booted resident session) and clears it when
/// it does not; the engine keeps its own clone and reads through THAT, never
/// through the quark's own lock — the whole reason this exists (Task 4 of
/// `.hadron/docs/plans/2026-07-31-responsive-orchestrator.md`): a turn holds
/// that lock for its entire duration, so a cancel routed through the quark
/// itself would wait for the very turn it exists to interrupt.
///
/// `Clone`: the engine's copy and the quark's copy share the SAME inner cell
/// on purpose — whichever side calls `set`, the other's `request_cancel` sees
/// it immediately, no re-attaching needed.
#[derive(Clone, Default)]
pub struct CancelSlot(std::sync::Arc<std::sync::Mutex<Option<CancelFn>>>);

impl CancelSlot {
    /// Fill or clear the slot's live cancel handle. `None` — an idle or
    /// never-booted transport — is what makes [`CancelSlot::request_cancel`]
    /// report `false`: an empty slot has nothing to cancel.
    pub fn set(&self, f: Option<CancelFn>) {
        *self.0.lock().unwrap() = f;
    }

    /// Ask whatever currently occupies the slot to cancel gracefully. `false`
    /// with no effect when the slot is empty — nothing booted, or (every CLI
    /// seat) a transport that never fills it, by construction: `Quark`'s
    /// default `attach_cancel_slot` is a no-op, so a CLI quark is never even
    /// handed a slot to fill in the first place.
    pub fn request_cancel(&self) -> bool {
        match &*self.0.lock().unwrap() {
            Some(f) => f(),
            None => false,
        }
    }
}

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
    /// The roles this quark plays for `@role` routing (e.g. `"security"`,
    /// `"architect"`). Carried on the quark for the same reason `display_name` is:
    /// the engine's roster card is always built with the right roles — including
    /// after a re-seat — rather than relying on a daemon-side population step that
    /// does not exist. Resolved from the seat; the adapter merely holds it.
    /// Defaults to empty — most quarks play no particular role.
    fn roles(&self) -> Vec<String> {
        Vec::new()
    }
    /// Whether this quark is scoped ONLY to tasks that name one of its `roles`.
    /// Carried the same way `roles` is. Defaults to `false` — most quarks stay in
    /// general dispatch.
    fn exclusive(&self) -> bool {
        false
    }
    /// Skill names this quark must NEVER be handed (hard lock). Matched against
    /// `skills::select`'s chosen name.
    fn deny_skills(&self) -> Vec<String> {
        Vec::new()
    }
    /// This quark's per-seat command allow/deny lists (see
    /// `hadron_lattice::SeatCommands`), folded into the gatekeeper's
    /// `AllowRules`/`DenyRules` under No-Human-Mode. Carried the same way
    /// `roles`/`exclusive` are. Defaults to empty — no config allow/deny.
    fn commands(&self) -> &hadron_lattice::SeatCommands {
        static EMPTY: hadron_lattice::SeatCommands =
            hadron_lattice::SeatCommands { allowed: Vec::new(), not_allowed: Vec::new() };
        &EMPTY
    }
    fn energy(&self) -> EnergyState;
    /// Per-seat budget energy limit (token ceiling).
    fn energy_limit(&self) -> Option<u32> {
        None
    }
    /// Whether this quark keeps its context **across turns** (a resident ACP session) or
    /// is re-spawned fresh each turn (a one-shot CLI process). The engine tracks this at
    /// seat time (`Engine::resident`) for whatever needs a seat's transport shape; skill
    /// injection no longer branches on it — resident and one-shot quarks alike now get
    /// the always-on index plus the active skill's full body, nothing more (WS4 §5).
    /// Defaults to `false` — most transports are one-shot; only residency is special.
    fn resident(&self) -> bool {
        false
    }
    /// Whether this quark has Hadron Forge MCP tools attached (e.g. resident ACP sessions).
    /// Defaults to `false` — CLI transports do not attach forge tools.
    fn has_forge_tools(&self) -> bool {
        false
    }
    /// Tell this instance it is an orchestrator seat's **chat lane** rather than its work
    /// lane (`Engine`'s `Lanes`). Called once, at seating, before any turn runs.
    ///
    /// Defaults to a **no-op**, which is correct for every transport that keeps no
    /// cross-turn conversation of its own: two ACP lanes are two independent
    /// `session/new` sessions (proved by the Task 1 probe), so neither can tread on the
    /// other and there is nothing to tell them. Only a CLI seat with
    /// `ResumeMode::Continue` — `agy` — has a single per-working-directory conversation
    /// that two lanes would interleave into, and `CliQuark` overrides this to make its
    /// chat lane stateless.
    ///
    /// A default-no-op rather than a required method on purpose: a transport added later
    /// is single-conversation only if it says so, and forgetting to implement this cannot
    /// silently corrupt a conversation that does not exist.
    fn become_chat_lane(&mut self) {}
    /// Hand this quark a [`CancelSlot`] to fill whenever it has something an
    /// in-flight turn can be gracefully cancelled through. Called once, at
    /// seating, before any turn runs — same convention as
    /// [`Quark::become_chat_lane`].
    ///
    /// Defaults to a no-op: a transport with no graceful-cancel primitive
    /// (every CLI seat) simply never overrides this, so it is never even
    /// handed a slot to fill — the slot's absence IS "this seat cannot be
    /// interrupted", no separate predicate needed.
    fn attach_cancel_slot(&mut self, _slot: CancelSlot) {}
    /// Run one turn against a projection and return the field message (if any).
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>;

    /// Reap any **resident** state (a live agent subprocess and its open session) so the
    /// next turn boots fresh. This is the human's force-restart: an ACP quark drops its
    /// session (killing the subprocess); a one-shot CLI quark holds nothing between turns,
    /// so the default is a no-op. Idempotent — calling it on a quark with no live session
    /// does nothing.
    fn reset_session(&mut self) {}
}
