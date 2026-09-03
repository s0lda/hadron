pub mod adapter;
/// The daemon entrypoint. Lives here, not in a `[[bin]]`, so the one installable
/// package can ship it alongside the chamber — see [`cli::run`].
pub mod cli;
pub mod engine;
pub mod field;
pub mod mock;
pub mod preons;
pub mod quark;
pub mod reseat;
pub mod router;
pub mod secrets;
pub use secrets::KeyringStore;
pub mod skills;
pub mod snapshot;
pub mod ledger;
pub mod watch;
pub mod daemon;
pub mod worktree;
pub mod merge;
pub mod statusline;
pub mod nucleus_status;
pub mod proc;
pub mod env;
pub mod mesh;
pub mod breakpoints;
pub mod preon_evolution;
pub mod prompt_distiller;
pub mod cas;
pub mod scout;
pub mod tournament;

pub use mesh::*;
pub use breakpoints::*;
pub use preon_evolution::*;
pub use prompt_distiller::*;
pub use cas::*;
pub use scout::*;
pub use tournament::*;
pub use skills::review_board::*;
pub use skills::grill_me::*;
pub mod pty_injection;
pub use pty_injection::*;
pub mod vcr;
pub use vcr::*;
pub mod bakeoff;
pub use bakeoff::*;
pub mod checkpoint;
pub use checkpoint::*;
pub mod quark_probe;
pub use quark_probe::*;
pub mod pair_quark;
pub use pair_quark::*;
pub mod dream_daemon;
pub use dream_daemon::*;


