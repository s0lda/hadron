//! Synthetic Fixture & Offline Mock Synthesizer.
//!
//! Generates deterministic in-memory fixtures and mock routes from JSON schemas and
//! API specifications, eliminating network flakiness and external dependencies in tests.

use std::collections::HashMap;
use serde_json::Value;
use crate::mock::MockRoute;

pub struct MockSynthesizer;

impl MockSynthesizer {
    /// Synthesizes a mock route with deterministic JSON fixture from a simplified schema or template.
    pub fn synthesize_route(
        path: &str,
        method: &str,
        status: u16,
        schema_json: &str,
    ) -> Result<MockRoute, String> {
        let parsed: Value = serde_json::from_str(schema_json)
            .map_err(|e| format!("Invalid JSON schema: {}", e))?;

        let sample_body = Self::generate_mock_value(&parsed);
        let body_str = serde_json::to_string_pretty(&sample_body)
            .unwrap_or_else(|_| "{}".to_string());

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        Ok(MockRoute {
            method: method.to_uppercase(),
            path: path.to_string(),
            status,
            headers,
            body: body_str,
            delay_ms: None,
        })
    }

    fn generate_mock_value(schema: &Value) -> Value {
        match schema {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), Self::generate_mock_value(v));
                }
                Value::Object(out)
            }
            Value::Array(arr) => {
                if let Some(first) = arr.first() {
                    Value::Array(vec![Self::generate_mock_value(first)])
                } else {
                    Value::Array(Vec::new())
                }
            }
            Value::String(s) => {
                match s.as_str() {
                    "string" => Value::String("mock_string_value".to_string()),
                    "number" | "integer" => Value::Number(42.into()),
                    "boolean" => Value::Bool(true),
                    _ => Value::String(s.clone()),
                }
            }
            Value::Number(n) => Value::Number(n.clone()),
            Value::Bool(b) => Value::Bool(*b),
            Value::Null => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthesize_route_from_schema() {
        let schema = r#"{
            "id": "integer",
            "name": "string",
            "active": "boolean",
            "roles": ["string"]
        }"#;

        let route = MockSynthesizer::synthesize_route("/api/v1/users", "POST", 201, schema).unwrap();
        assert_eq!(route.method, "POST");
        assert_eq!(route.path, "/api/v1/users");
        assert_eq!(route.status, 201);
        assert_eq!(route.headers.get("content-type").unwrap(), "application/json");

        let body_val: Value = serde_json::from_str(&route.body).unwrap();
        assert_eq!(body_val["id"], 42);
        assert_eq!(body_val["name"], "mock_string_value");
        assert_eq!(body_val["active"], true);
        assert_eq!(body_val["roles"][0], "mock_string_value");
    }
}
