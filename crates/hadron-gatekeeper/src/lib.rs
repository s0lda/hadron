//! The Gatekeeper's pure decision core: the permission-mode ladder. It folds the
//! field's `ModeSet` events into an effective mode per quark (per-quark override
//! over a global default), learns an allow-list from remembered grants, and
//! decides whether a proposed op is pre-authorized or must ask a human.
//!
//! Intentionally offline and side-effect-free: it does NOT classify commands,
//! emit events, pause the daemon, or render UI — the engine and chamber do that.

mod gate;
mod matrix;

pub use gate::{grant, grant_remembering, pending_permission, PendingPermission};
pub use hadron_lattice::{Mode, Risk};
pub use matrix::{
    allow_rules, decide, global_mode, has_override, resolve_mode, AllowRules, Decision,
};
