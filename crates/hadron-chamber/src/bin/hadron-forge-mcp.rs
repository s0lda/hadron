//! The stdio MCP server the daemon hands to each ACP seat, shipped from the same
//! package so the daemon finds it as a sibling of its own `current_exe`
//! (`hadron-gluon/src/adapter/acp/session.rs:494`).
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hadron_forge_mcp::run().await
}
