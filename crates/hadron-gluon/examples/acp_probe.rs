//! A live ACP round-trip against a real agent binary — run by hand, not in CI.
//!
//! This exists to answer, empirically, the three questions the CLI transport
//! cannot: does a session stay alive across turns, does the agent stream what it
//! is doing while it works, and does it report token usage at end of turn.
//!
//! ```bash
//! cargo run -p hadron-gluon --example acp_probe -- "npx @zed-industries/claude-code-acp"
//! ```
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use std::path::PathBuf;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "npx @zed-industries/claude-code-acp".to_string());

    eprintln!("spawning agent: {command}");
    let agent = AcpAgent::from_str(&command)?;

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                // The channel the one-shot CLI does not have: progress *during* the turn.
                eprintln!("[notify] {:?}", notification.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                // The probe approves everything; the real adapter maps this onto the mode ladder.
                eprintln!("[permission] {:?}", request.tool_call);
                match request.options.first().map(|o| o.option_id.clone()) {
                    Some(id) => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                    )),
                    None => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            eprintln!("initialized: {:?}", init.agent_info);
            eprintln!("capabilities: {:?}", init.agent_capabilities);

            let session = connection
                .send_request(NewSessionRequest::new(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
                ))
                .block_task()
                .await?;
            let session_id = session.session_id;
            eprintln!("session: {session_id:?}");

            // Turn 1 opens the session with a fact only this turn knows.
            let one = connection
                .send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "Remember this codeword: XYLOPHONE-4711. Reply with only the word STORED."
                            .to_string(),
                    ))],
                ))
                .block_task()
                .await?;
            eprintln!("turn 1 stop: {:?}", one.stop_reason);
            eprintln!("turn 1 usage: {:?}", one.usage);

            // Turn 2 re-uses the SAME session id and re-sends no history. If the
            // codeword comes back, the session is genuinely resident.
            let two = connection
                .send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "What codeword did I ask you to remember? Reply with only the codeword."
                            .to_string(),
                    ))],
                ))
                .block_task()
                .await?;
            eprintln!("turn 2 stop: {:?}", two.stop_reason);
            eprintln!("turn 2 usage: {:?}", two.usage);

            Ok(())
        })
        .await?;

    Ok(())
}
