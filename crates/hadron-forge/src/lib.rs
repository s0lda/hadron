//! `hadron-forge` — the edit-by-hash core: parse source into logical blocks,
//! hash them, and reconcile concurrent edits by hash. The block layer is pure;
//! [`exec`] is the one module that spawns a process, and it is jailed and bounded.
pub mod block;
pub mod cargo_tree;
pub mod diagnostics;
pub mod edit;
pub mod exec;
pub mod file;
pub mod git;
pub mod inspect;
pub mod lang;
pub mod nucleus;



