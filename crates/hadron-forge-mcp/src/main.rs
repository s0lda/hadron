mod tools;

use anyhow::{Context, Result};
use rmcp::serve_server;
use std::env;
use std::path::PathBuf;
use tools::ForgeMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
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
