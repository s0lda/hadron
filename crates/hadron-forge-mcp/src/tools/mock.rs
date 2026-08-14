//! The **mock** family: in-process HTTP and WebSocket mock servers.
//!
//! Spawns local mock servers on `127.0.0.1` for webhook testing, API contract verification,
//! and request assertions.

use std::collections::HashMap;

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::mock::MockRoute;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockStartArgs {
    /// Optional specific port to bind to on `127.0.0.1` (default: 0 for automatic ephemeral port).
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockRouteAddArgs {
    /// The port of the running mock server.
    pub port: u16,
    /// HTTP method: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, or `*`.
    pub method: String,
    /// Path to match (e.g. `/api/v1/webhook`).
    pub path: String,
    /// HTTP status code to return (default: 200).
    #[serde(default)]
    pub status: Option<u16>,
    /// Custom response headers.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Response body payload.
    #[serde(default)]
    pub body: Option<String>,
    /// Simulated response delay in milliseconds.
    #[serde(default)]
    pub delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockRequestsListArgs {
    /// The port of the running mock server.
    pub port: u16,
    /// Optional limit on the number of recent requests to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockAssertArgs {
    /// The port of the running mock server.
    pub port: u16,
    /// Expected HTTP method.
    #[serde(default)]
    pub method: Option<String>,
    /// Substring expected in the request path.
    #[serde(default)]
    pub path_contains: Option<String>,
    /// Substring expected in the request body.
    #[serde(default)]
    pub body_contains: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockStopArgs {
    /// The port of the mock server to stop.
    pub port: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MockListArgs {}

#[tool_router(router = mock_router, vis = "pub(super)")]
impl ForgeMcpServer {
    /// Start a local in-process mock HTTP server on 127.0.0.1.
    #[tool(
        name = "hadron_forge_mock_start",
        description = "Start an in-process mock HTTP server on 127.0.0.1. Returns the assigned port and base URL."
    )]
    pub async fn mock_start(
        &self,
        Parameters(args): Parameters<MockStartArgs>,
    ) -> Json<ToolResponse> {
        match self.mock_manager.start(args.port).await {
            Ok(summary) => match serde_json::to_string_pretty(&summary) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Add a mock route rule to an active mock server.
    #[tool(
        name = "hadron_forge_mock_route_add",
        description = "Register a mock route rule with status code, response headers, and response body."
    )]
    pub async fn mock_route_add(
        &self,
        Parameters(args): Parameters<MockRouteAddArgs>,
    ) -> Json<ToolResponse> {
        let route = MockRoute {
            method: args.method,
            path: args.path,
            status: args.status.unwrap_or(200),
            headers: args.headers.unwrap_or_default(),
            body: args.body.unwrap_or_else(|| r#"{"status":"ok"}"#.to_string()),
            delay_ms: args.delay_ms,
        };

        match self.mock_manager.add_route(args.port, route).await {
            Ok(()) => Json(ToolResponse::success(Some(format!(
                "Mock route registered on port {}",
                args.port
            )))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// List recorded requests received by the mock server.
    #[tool(
        name = "hadron_forge_mock_requests_list",
        description = "List all HTTP requests captured by the mock server's request journal."
    )]
    pub async fn mock_requests_list(
        &self,
        Parameters(args): Parameters<MockRequestsListArgs>,
    ) -> Json<ToolResponse> {
        match self.mock_manager.list_requests(args.port, args.limit).await {
            Ok(reqs) => match serde_json::to_string_pretty(&reqs) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Assert that a specific request matching method, path, or body was received.
    #[tool(
        name = "hadron_forge_mock_assert",
        description = "Assert that a request matching specific criteria (method, path substring, body substring) arrived at the mock server."
    )]
    pub async fn mock_assert(
        &self,
        Parameters(args): Parameters<MockAssertArgs>,
    ) -> Json<ToolResponse> {
        match self
            .mock_manager
            .assert_request(
                args.port,
                args.method.as_deref(),
                args.path_contains.as_deref(),
                args.body_contains.as_deref(),
            )
            .await
        {
            Ok(true) => Json(ToolResponse::success(Some(
                "Assertion passed: matching request was received".to_string(),
            ))),
            Ok(false) => Json(ToolResponse::error(
                "Assertion failed: no matching request found in journal",
            )),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Stop an active mock server.
    #[tool(
        name = "hadron_forge_mock_stop",
        description = "Stop an active mock server and close its listener."
    )]
    pub async fn mock_stop(
        &self,
        Parameters(args): Parameters<MockStopArgs>,
    ) -> Json<ToolResponse> {
        match self.mock_manager.stop(args.port).await {
            Ok(true) => Json(ToolResponse::success(Some(format!(
                "Mock server on port {} stopped",
                args.port
            )))),
            Ok(false) => Json(ToolResponse::error(format!(
                "No active mock server on port {}",
                args.port
            ))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// List all active mock servers.
    #[tool(
        name = "hadron_forge_mock_list",
        description = "List all running mock servers and their route/request counts."
    )]
    pub async fn mock_list(
        &self,
        _args: Parameters<MockListArgs>,
    ) -> Json<ToolResponse> {
        let list = self.mock_manager.list().await;
        match serde_json::to_string_pretty(&list) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn mock_router_starts_routes_and_asserts() {
        let temp = tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path().to_path_buf());

        let start_res = server
            .mock_start(Parameters(MockStartArgs { port: None }))
            .await;
        assert!(start_res.0.ok);
        let blocks = start_res.0.blocks.unwrap();
        let summary: serde_json::Value = serde_json::from_str(&blocks).unwrap();
        let port = summary["port"].as_u64().unwrap() as u16;

        let route_res = server
            .mock_route_add(Parameters(MockRouteAddArgs {
                port,
                method: "POST".into(),
                path: "/notify".into(),
                status: Some(200),
                headers: None,
                body: Some(r#"{"received":true}"#.into()),
                delay_ms: None,
            }))
            .await;
        assert!(route_res.0.ok);

        // Send HTTP request
        let mut client = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let req = "POST /notify HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 12\r\n\r\n{\"msg\":\"ping\"}";
        client.write_all(req.as_bytes()).await.unwrap();

        let mut buf = [0u8; 1024];
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("200 OK"));
        assert!(resp.contains(r#"{"received":true}"#));

        // Assert request
        let assert_res = server
            .mock_assert(Parameters(MockAssertArgs {
                port,
                method: Some("POST".into()),
                path_contains: Some("/notify".into()),
                body_contains: Some("ping".into()),
            }))
            .await;
        assert!(assert_res.0.ok);

        // Stop
        let stop_res = server.mock_stop(Parameters(MockStopArgs { port })).await;
        assert!(stop_res.0.ok);
    }
}
