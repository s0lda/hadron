//! Pure logic for the `wiretap` tool family.
//! Inspects, filters, parses, and asserts sequence invariants on NDJSON / JSON-RPC / ACP protocol streams.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WiretapAction {
    InspectNdjson,
    AssertSequence,
    FilterFrames,
    ValidateJson,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WiretapReport {
    pub total_frames: usize,
    pub matched_frames: usize,
    pub invalid_frames: usize,
    pub sequence_matched: bool,
    pub frames_sample: Vec<String>,
    pub summary: String,
}

/// Parse raw NDJSON text into JSON values.
pub fn parse_ndjson(input: &str) -> (Vec<Value>, Vec<(usize, String)>) {
    let mut values = Vec::new();
    let mut errors = Vec::new();

    for (idx, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => values.push(val),
            Err(e) => errors.push((idx + 1, format!("{}: {}", e, trimmed))),
        }
    }

    (values, errors)
}

/// Filter JSON values by a key/value query string or substring.
pub fn filter_ndjson_values(values: &[Value], query: &str) -> Vec<Value> {
    if query.trim().is_empty() {
        return values.to_vec();
    }
    let query_lower = query.to_lowercase();
    values
        .iter()
        .filter(|val| {
            let s = val.to_string().to_lowercase();
            s.contains(&query_lower)
        })
        .cloned()
        .collect()
}

/// Assert that a list of event patterns appears in sequential order within the stream.
pub fn assert_sequence(values: &[Value], expected_patterns: &[String]) -> (bool, Option<String>) {
    if expected_patterns.is_empty() {
        return (true, None);
    }

    let mut pattern_idx = 0;
    for (_frame_idx, val) in values.iter().enumerate() {
        let s = val.to_string();
        if s.contains(&expected_patterns[pattern_idx]) {
            pattern_idx += 1;
            if pattern_idx >= expected_patterns.len() {
                return (true, None);
            }
        }
    }

    (
        false,
        Some(format!(
            "Missing expected sequence step {} ('{}') after inspecting {} frames",
            pattern_idx + 1,
            &expected_patterns[pattern_idx],
            values.len()
        )),
    )
}

/// Execute wiretap operation against text payload or file.
pub fn run_wiretap(
    root: &Root,
    action: WiretapAction,
    file_path: Option<&str>,
    raw_payload: Option<&str>,
    match_query: Option<&str>,
    expected_sequence: Option<Vec<String>>,
) -> Result<WiretapReport, ForgeError> {
    let content = match (file_path, raw_payload) {
        (Some(path), _) => {
            let abs_path = resolve_jailed_path(root, path)?;
            std::fs::read_to_string(&abs_path)
                .map_err(|e| ForgeError::Io(format!("Failed reading file {path}: {e}")))?
        }
        (None, Some(payload)) => payload.to_string(),
        (None, None) => {
            return Err(ForgeError::Rejected(
                "Either file_path or raw_payload must be provided to wiretap".to_string(),
            ))
        }
    };

    let (values, errors) = parse_ndjson(&content);
    let total_frames = values.len() + errors.len();
    let invalid_frames = errors.len();

    match action {
        WiretapAction::InspectNdjson => {
            let sample: Vec<String> = values
                .iter()
                .take(20)
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .collect();
            let summary = format!(
                "Parsed {} valid NDJSON frames ({} invalid/malformed lines).",
                values.len(),
                invalid_frames
            );
            Ok(WiretapReport {
                total_frames,
                matched_frames: values.len(),
                invalid_frames,
                sequence_matched: true,
                frames_sample: sample,
                summary,
            })
        }
        WiretapAction::FilterFrames => {
            let q = match_query.unwrap_or("");
            let filtered = filter_ndjson_values(&values, q);
            let sample: Vec<String> = filtered
                .iter()
                .take(20)
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .collect();
            let summary = format!(
                "Filtered {} matching frames out of {} total using query '{}'.",
                filtered.len(),
                total_frames,
                q
            );
            Ok(WiretapReport {
                total_frames,
                matched_frames: filtered.len(),
                invalid_frames,
                sequence_matched: true,
                frames_sample: sample,
                summary,
            })
        }
        WiretapAction::AssertSequence => {
            let seq = expected_sequence.unwrap_or_default();
            let (matched, err_opt) = assert_sequence(&values, &seq);
            let summary = if matched {
                format!(
                    "Sequence assertion passed! All {} expected sequence steps found across {} frames.",
                    seq.len(),
                    values.len()
                )
            } else {
                format!("Sequence assertion failed: {}", err_opt.unwrap_or_default())
            };
            Ok(WiretapReport {
                total_frames,
                matched_frames: values.len(),
                invalid_frames,
                sequence_matched: matched,
                frames_sample: vec![],
                summary,
            })
        }
        WiretapAction::ValidateJson => {
            let summary = if invalid_frames == 0 {
                format!("All {} NDJSON frames are valid JSON.", values.len())
            } else {
                format!(
                    "JSON validation failure: {} invalid frames detected out of {}.",
                    invalid_frames, total_frames
                )
            };
            let sample = errors.iter().take(10).map(|(l, e)| format!("Line {}: {}", l, e)).collect();
            Ok(WiretapReport {
                total_frames,
                matched_frames: values.len(),
                invalid_frames,
                sequence_matched: invalid_frames == 0,
                frames_sample: sample,
                summary,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_filter_ndjson() {
        let ndjson = "{\"type\":\"init\"}\n{\"type\":\"delta\",\"text\":\"hi\"}\ninvalid json\n{\"type\":\"done\"}\n";
        let (values, errors) = parse_ndjson(ndjson);
        assert_eq!(values.len(), 3);
        assert_eq!(errors.len(), 1);

        let filtered = filter_ndjson_values(&values, "delta");
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn assert_sequence_checks_order() {
        let ndjson = "{\"event\":\"start\"}\n{\"event\":\"process\"}\n{\"event\":\"end\"}";
        let (values, _) = parse_ndjson(ndjson);

        let seq_pass = vec!["start".to_string(), "end".to_string()];
        let (ok, _) = assert_sequence(&values, &seq_pass);
        assert!(ok);

        let seq_fail = vec!["end".to_string(), "start".to_string()];
        let (ok2, _) = assert_sequence(&values, &seq_fail);
        assert!(!ok2);
    }
}
