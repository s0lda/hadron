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
Yes, `ProtocolVersion::V2` negotiation is backward-compatible. 
According to the `InitializeResponse` schema in v2, the `protocol_version` field represents:
> "The protocol version the client specified if supported by the agent, or the latest protocol version supported by the agent. The client should disconnect, if it doesn't support this version."

This means if we send `ProtocolVersion::V2` in our `InitializeRequest` to an agent that only speaks v1, the agent will reply with `protocol_version: ProtocolVersion::V1`. We can inspect the response and either gracefully fall back to parsing and sending v1 messages (if we retain v1 support), or disconnect if we strict-require v2.

## 3. Does `claude-code-acp` advertise model config options?
**I could not reach `claude-code-acp` to check its response.**
Attempts to spawn `npx -y @agentclientprotocol/claude-agent-acp` (with and without `CLAUDECODE` unset) and send an `initialize` JSON-RPC request hung without yielding a response on `stdout`. Per instructions, I am stating this rather than guessing its capabilities.
