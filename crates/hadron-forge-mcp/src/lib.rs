pub mod tools;

use anyhow::{Context, Result};
use rmcp::serve_server;
use std::env;
use std::path::PathBuf;
use tools::ForgeMcpServer;

/// The stdio MCP server entrypoint, in the library rather than in a `[[bin]]`, so the
/// single installable package (`hadron`) can carry a bin target that calls it. The
/// daemon finds this server as a sibling of its own `current_exe` (`session.rs:494`),
/// so all three binaries must install into one directory.
///
/// `argv[1]` is the project root the turn runs in; `argv[2]`, when given, overrides the
/// nucleus directory.
pub async fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let root_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        env::current_dir().context("Failed to get current directory")?
    };

    let server = if args.len() > 2 {
        ForgeMcpServer::with_nucleus(root_path, PathBuf::from(&args[2]))
    } else {
        ForgeMcpServer::new(root_path)
    };

    let running = serve_server(server, rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}
