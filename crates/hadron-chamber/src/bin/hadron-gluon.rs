//! The headless swarm daemon, shipped from the same package as `hadron` so the two
//! land in one directory: the chamber resolves the daemon as a sibling of its own
//! `current_exe` (`main.rs:211`). See [`hadron_gluon::cli::run`] for the entrypoint.
#[tokio::main]
async fn main() {
    hadron_gluon::cli::run().await
}
