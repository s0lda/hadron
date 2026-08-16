use serde::{Deserialize, Serialize};

/// High-level time-travel and session forking helper for Forge MCP tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeTravelReport {
    pub session_id: String,
    pub total_events: usize,
    pub last_turn_ulid: Option<String>,
    pub touched_files: Vec<String>,
    pub token_spend_summary: u64,
}

pub struct TimeTravelForge;

impl TimeTravelForge {
    /// Summarize an event stream file (NDJSON) for time-travel analysis.
    pub fn analyze_session_events(content: &str) -> TimeTravelReport {
        let mut total_events = 0;
        let mut last_turn_ulid = None;
        let mut files = std::collections::HashSet::new();
        let mut total_tokens = 0u64;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            total_events += 1;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(turn) = v.get("turn").and_then(|t| t.as_str()) {
                    last_turn_ulid = Some(turn.to_string());
                }
                if let Some(paths) = v.get("paths").and_then(|p| p.as_array()) {
                    for p in paths {
                        if let Some(s) = p.as_str() {
                            files.insert(s.to_string());
                        }
                    }
                }
                if let Some(usage) = v.get("usage") {
                    if let Some(spend) = usage.get("spend") {
                        let inp = spend.get("input").and_then(|i| i.as_u64()).unwrap_or(0);
                        let out = spend.get("output").and_then(|o| o.as_u64()).unwrap_or(0);
                        total_tokens += inp + out;
                    }
                }
            }
        }

        let mut touched_files: Vec<String> = files.into_iter().collect();
        touched_files.sort();

        TimeTravelReport {
            session_id: "active_session".into(),
            total_events,
            last_turn_ulid,
            touched_files,
            token_spend_summary: total_tokens,
        }
    }

    /// Rewind event NDJSON content to a specific turn ULID string.
    pub fn rewind_ndjson(content: &str, target_turn_ulid: &str) -> String {
        let mut result = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            result.push_str(trimmed);
            result.push('\n');
            if trimmed.contains(target_turn_ulid) {
                break;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_travel_forge() {
        let ndjson = r#"{"v":1,"id":"01M01","turn":"01M01_T","paths":["src/lib.rs"],"usage":{"spend":{"input":10,"output":5}}}
{"v":1,"id":"01M02","turn":"01M02_T","paths":["src/main.rs"],"usage":{"spend":{"input":20,"output":10}}}"#;

        let report = TimeTravelForge::analyze_session_events(ndjson);
        assert_eq!(report.total_events, 2);
        assert_eq!(report.last_turn_ulid.as_deref(), Some("01M02_T"));
        assert_eq!(report.touched_files, vec!["src/lib.rs", "src/main.rs"]);
        assert_eq!(report.token_spend_summary, 45);

        let rewound = TimeTravelForge::rewind_ndjson(ndjson, "01M01_T");
        assert!(rewound.contains("01M01"));
        assert!(!rewound.contains("01M02"));
    }
}
