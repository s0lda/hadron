//! The Gatekeeper's pure decision core: the permission-mode ladder. It folds the
//! field's `ModeSet` events into an effective mode per quark (per-quark override
//! over a global default), learns an allow-list from remembered grants, and
//! decides whether a proposed op is pre-authorized or must ask a human.
//!
//! Intentionally offline and side-effect-free: it does NOT classify commands,
//! emit events, pause the daemon, or render UI — the engine and chamber do that.

mod gate;
mod matrix;
mod merge;
pub mod mutation;
pub mod benchmark_guard;
pub mod invariant_synthesis;
pub mod sandbox;
pub mod mutation_quark;
pub mod cache_guard;

pub use gate::{any_pending_permission, grant, grant_remembering, pending_permission, PendingPermission};
pub use hadron_lattice::{Mode, Risk};
pub use matrix::{
    allow_rules, decide, effective_mode, global_mode, has_override, op_matches, resolve_mode,
    AllowRules, Decision, DenyRules,
};
pub use merge::{
    merge_approved, merge_decision, merge_op, BlockReason, BranchState, MergeVerdict,
};
pub use mutation::*;
pub use benchmark_guard::*;
pub use invariant_synthesis::*;
pub use sandbox::*;
pub use mutation_quark::*;
pub use cache_guard::*;
