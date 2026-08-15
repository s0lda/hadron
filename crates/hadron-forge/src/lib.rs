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
pub mod process;
pub mod ast_symbols;
pub mod lsp;
pub mod ast_merge;
pub mod conflict_resolve;
pub mod tia;
pub mod screenshot;
pub mod pty;
pub mod mock;
pub mod sqlite;
pub mod gate;

pub use ast_merge::*;
pub use conflict_resolve::*;
pub use tia::*;
pub use screenshot::*;
pub use pty::*;
pub use mock::*;
pub use sqlite::*;
pub use gate::*;



