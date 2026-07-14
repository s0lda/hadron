# ACP v2 Migration Research

## 1. Type Differences (v1 to v2)
We currently import the following types from `agent_client_protocol::schema::v1` in `adapter/acp.rs`:
`ContentBlock`, `InitializeRequest`, `NewSessionRequest`, `PermissionOptionKind`, `PromptRequest`, `RequestPermissionOutcome`, `RequestPermissionRequest`, `RequestPermissionResponse`, `SelectedPermissionOutcome`, `SessionNotification`, `SessionUpdate`, `StopReason`, `TextContent`, `Usage`.

The following types **changed shape** in v2 (fields added, removed, renamed, or struct renamed):

- **`ContentBlock`**: Added a new `Other(OtherContentBlock)` catch-all variant for future compatibility.
- **`InitializeRequest`**: 
  - `client_info: Option<Implementation>` is now renamed to `info` and made required (`Implementation`).
  - `client_capabilities: ClientCapabilities` is renamed to `capabilities`.
- **`NewSessionRequest`**: `mcp_servers` now skips serializing if empty (`#[serde(skip_serializing_if = "Vec::is_empty")]`).
- **`PermissionOptionKind`**: Added a new `Other(String)` catch-all variant.
- **`RequestPermissionOutcome`**: Added a new `Other(OtherRequestPermissionOutcome)` catch-all variant.
- **`RequestPermissionRequest`**: 
  - Added required field `title: String`.
  - Added optional fields `description: Option<String>` and `subject: Option<RequestPermissionSubject>`. 
  - Removed `tool_call` (it was replaced by `subject`).
  - `options` now explicitly requires at least 1 item (`#[schemars(length(min = 1))]`).
- **`RequestPermissionResponse`**: No structural field shape change for the client wrapper, but the inner `outcome` field changed due to the `RequestPermissionOutcome` modifications above.
- **`SessionNotification`**: Renamed completely to `UpdateSessionNotification` (v2 drops the generic `SessionNotification` wrapper in favor of specific top-level types like `UpdateSessionNotification` and `CancelSessionNotification`).
- **`SessionUpdate`**: 
  - Removed variants: `ToolCall`, `Plan`, `CurrentModeUpdate`. 
  - Added variants: `UserMessage`, `AgentMessage`, `AgentThought`, `StateUpdate`, `ToolCallContentChunk`, `Other(OtherSessionUpdate)`.
- **`StopReason`**: Added a new `Other(String)` variant.

*(Types with no shape changes: `PromptRequest`, `SelectedPermissionOutcome`, `TextContent`, `Usage`)*

## 2. Backward Compatibility of `ProtocolVersion::V2` Negotiation
**No, `ProtocolVersion::V2` negotiation is NOT gracefully backward-compatible in practice.**
While the spec suggests an agent should reply with the protocol version it supports, offering `ProtocolVersion::V2` to `claude-agent-acp 0.59.0` fails. It answers with `protocolVersion: 1` inside a v1-shaped body that the strict v2 types cannot deserialize, causing the connection to die immediately. Migrating our types to v2 would break our only working ACP agent.

## 3. Does `claude-code-acp` advertise model config options?
**Yes, and it was in v1 all along.**
`claude-agent-acp` advertises FIVE config selectors over **v1** on the `session/new` response: `Mode`, `Model`, `Effort`, `Fast mode`, and `Agent`. We were receiving this model list on every session and discarding it. `session/set_config_option` with the `Model` category exists in v1, and model selection requires no protocol migration.

*Correction on earlier hanging behavior:* The previous probe hung (rather than erroring) because `CLAUDECODE` was set in the environment. Claude Code refuses to nest inside another Claude Code session. Running it with `env -u CLAUDECODE` answers correctly.
