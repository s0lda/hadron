//! The Gatekeeper's pure decision core: the "bypass matrix" mapping a proposed
//! operation's risk against the human's god-mode policy.
//!
//! This crate is intentionally offline and side-effect-free. It does NOT classify
//! commands, emit events, pause the daemon, or render UI — those are later slices.

mod matrix;

pub use matrix::{decide, Decision, Policy, Risk};
