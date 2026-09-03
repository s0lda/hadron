//! Low-Level Transport Secret Masker.
//!
//! Masks API keys, private credentials, and high-entropy secret tokens in raw NDJSON
//! streams and log payloads before they are persisted to disk or sent across the bus.

use serde_json::Value;

pub struct TransportScrubber;

impl TransportScrubber {
    /// Mask potential secrets from a text string.
    pub fn scrub_string(input: &str) -> String {
        let mut result = input.to_string();

        // 1. GitHub tokens: ghp_..., github_pat_...
        result = Self::mask_prefix_pattern(&result, "ghp_", 36);
        result = Self::mask_prefix_pattern(&result, "github_pat_", 82);

        // 2. OpenAI / Anthropic keys: sk-..., sk-ant-...
        result = Self::mask_prefix_pattern(&result, "sk-ant-", 40);
        result = Self::mask_prefix_pattern(&result, "sk-", 48);

        // 3. AWS Access Keys: AKIA...
        result = Self::mask_prefix_pattern(&result, "AKIA", 16);

        // 4. Private keys
        if result.contains("-----BEGIN PRIVATE KEY-----") {
            result = result.replace("-----BEGIN PRIVATE KEY-----", "[REDACTED_PRIVATE_KEY_HEADER]");
        }
        if result.contains("-----BEGIN RSA PRIVATE KEY-----") {
            result = result.replace("-----BEGIN RSA PRIVATE KEY-----", "[REDACTED_PRIVATE_KEY_HEADER]");
        }

        result
    }

    fn mask_prefix_pattern(text: &str, prefix: &str, expected_len: usize) -> String {
        let mut out = String::new();
        let mut cursor = 0;

        while let Some(pos) = text[cursor..].find(prefix) {
            let abs_pos = cursor + pos;
            out.push_str(&text[cursor..abs_pos]);

            let start = abs_pos;
            let mut end = start + prefix.len();
            while end < text.len() && text.as_bytes()[end].is_ascii_alphanumeric() || (end < text.len() && (text.as_bytes()[end] == b'_' || text.as_bytes()[end] == b'-')) {
                end += 1;
            }

            let token = &text[start..end];
            if token.len() >= prefix.len() + 8 || token.len() >= expected_len {
                out.push_str("[REDACTED_SECRET]");
            } else {
                out.push_str(token);
            }
            cursor = end;
        }

        out.push_str(&text[cursor..]);
        out
    }

    /// Recursively scrub strings inside JSON values.
    pub fn scrub_json(value: &mut Value) {
        match value {
            Value::String(s) => {
                *s = Self::scrub_string(s);
            }
            Value::Array(arr) => {
                for item in arr {
                    Self::scrub_json(item);
                }
            }
            Value::Object(map) => {
                for (k, v) in map {
                    let key_lower = k.to_lowercase();
                    if key_lower.contains("secret")
                        || key_lower.contains("password")
                        || key_lower.contains("token")
                        || key_lower.contains("authorization")
                    {
                        if let Value::String(_) = v {
                            *v = Value::String("[REDACTED_KEY_VALUE]".to_string());
                            continue;
                        }
                    }
                    Self::scrub_json(v);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_string_patterns() {
        let raw = "Authorization: Bearer sk-ant-api03-abcdef1234567890abcdef1234567890\nAlso ghp_1234567890abcdef1234567890abcdef1234";
        let scrubbed = TransportScrubber::scrub_string(raw);
        assert!(!scrubbed.contains("sk-ant-api03"));
        assert!(!scrubbed.contains("ghp_1234567890"));
        assert!(scrubbed.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn test_scrub_json_recursive() {
        let mut json = serde_json::json!({
            "api_token": "super_secret_password_123",
            "nested": {
                "message": "Here is the key: sk-abcdef1234567890abcdef1234567890"
            }
        });

        TransportScrubber::scrub_json(&mut json);
        assert_eq!(json["api_token"], "[REDACTED_KEY_VALUE]");
        assert!(!json["nested"]["message"].as_str().unwrap().contains("sk-abcdef"));
        assert!(json["nested"]["message"].as_str().unwrap().contains("[REDACTED_SECRET]"));
    }
}
