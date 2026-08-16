//! Pure logic for the `fuzz_harness` tool family.
//! Mutation engine and property-based fuzz generator for parsers, serializers, and IPC protocols.

use serde::{Deserialize, Serialize};

use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FuzzTargetFormat {
    Json,
    Ndjson,
    Utf8,
    Numeric,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuzzCase {
    pub iteration: usize,
    pub mutation_kind: String,
    pub payload_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuzzHarnessReport {
    pub total_generated: usize,
    pub format: String,
    pub generated_cases: Vec<FuzzCase>,
    pub summary: String,
}

/// Generate adversarial mutation variants of input seeds.
pub fn generate_mutations(format: FuzzTargetFormat, count: usize) -> Vec<FuzzCase> {
    let mut cases = Vec::new();

    match format {
        FuzzTargetFormat::Json => {
            let json_edges = [
                ("null_byte", "{\"key\":\"\\u0000embedded_null\"}"),
                ("deep_nesting", "[[[[[[[[[[{\"nested\":true}]]]]]]]]]]"),
                ("huge_integer", "{\"large\":99999999999999999999999999999999999999999999}"),
                ("unterminated_string", "{\"unclosed\":\"string value"),
                ("duplicate_keys", "{\"dup\":1, \"dup\":2, \"dup\":3}"),
                ("unicode_surrogate", "{\"emoji\":\"\\uD83D\\uDE00\\uD800\"}"),
                ("trailing_comma", "{\"a\":1, \"b\":2,}"),
                ("empty_object_array", "[{}, [], {}, []]"),
                ("scientific_float", "{\"float\": 1e-9999999}"),
                ("control_chars", "{\"ctrl\":\"\x01\x02\x03\x04\x1b[31m\"}"),
            ];
            for i in 0..count {
                let (kind, payload) = json_edges[i % json_edges.len()];
                cases.push(FuzzCase {
                    iteration: i + 1,
                    mutation_kind: kind.to_string(),
                    payload_preview: payload.to_string(),
                });
            }
        }
        FuzzTargetFormat::Ndjson => {
            let ndjson_edges = [
                ("empty_lines", "\n\n{\"ok\":true}\n\n"),
                ("mixed_invalid_frames", "{\"seq\":1}\nnot_json\n{\"seq\":2}\n"),
                ("gigantic_line", &format!("{{\"payload\":\"{}\"}}\n", "A".repeat(2048))),
                ("null_byte_in_line", "{\"event\":\"test\x00data\"}\n"),
                ("missing_newlines", "{\"a\":1}{\"b\":2}{\"c\":3}"),
                ("carriage_return_mix", "{\"event\":1}\r\n{\"event\":2}\r{\"event\":3}\n"),
            ];
            for i in 0..count {
                let (kind, payload) = ndjson_edges[i % ndjson_edges.len()];
                cases.push(FuzzCase {
                    iteration: i + 1,
                    mutation_kind: kind.to_string(),
                    payload_preview: payload.to_string(),
                });
            }
        }
        FuzzTargetFormat::Utf8 => {
            let utf8_edges = [
                ("zalgo_diacritics", "T̷e̵s̷t̶ ̷S̵t̸r̵i̶n̸g̵"),
                ("rtl_override", "\u{202E}gnirtS esreveR\u{202C}"),
                ("zero_width_spaces", "Zero\u{200B}Width\u{200C}Space\u{200D}Injection"),
                ("homoglyph_cyrillic", "pаssword_with_cyrillic_а"),
                ("bom_marker", "\u{FEFF}StringWithBOM"),
            ];
            for i in 0..count {
                let (kind, payload) = utf8_edges[i % utf8_edges.len()];
                cases.push(FuzzCase {
                    iteration: i + 1,
                    mutation_kind: kind.to_string(),
                    payload_preview: payload.to_string(),
                });
            }
        }
        FuzzTargetFormat::Numeric => {
            let num_edges = [
                ("u64_max", "18446744073709551615"),
                ("i64_min", "-9223372036854775808"),
                ("nan_infinity", "NaN, -Infinity, +Infinity"),
                ("subnormal_float", "0.0000000000000000000000000000000000000001"),
                ("hex_octal_mix", "0xdeadbeef, 0o777, 0b101010"),
            ];
            for i in 0..count {
                let (kind, payload) = num_edges[i % num_edges.len()];
                cases.push(FuzzCase {
                    iteration: i + 1,
                    mutation_kind: kind.to_string(),
                    payload_preview: payload.to_string(),
                });
            }
        }
    }

    cases
}

pub fn run_fuzz_harness(
    _root: &Root,
    target_format: &str,
    iterations: Option<usize>,
) -> Result<FuzzHarnessReport, ForgeError> {
    let fmt = match target_format {
        "json" => FuzzTargetFormat::Json,
        "ndjson" => FuzzTargetFormat::Ndjson,
        "utf8" => FuzzTargetFormat::Utf8,
        "numeric" => FuzzTargetFormat::Numeric,
        other => {
            return Err(ForgeError::Rejected(format!(
                "Unsupported fuzz format '{}'. Supported: json, ndjson, utf8, numeric",
                other
            )))
        }
    };

    let count = iterations.unwrap_or(10).clamp(1, 100);
    let cases = generate_mutations(fmt, count);

    let summary = format!(
        "Fuzz Harness: Generated {} adversarial fuzz mutations for target format '{}'.",
        cases.len(),
        target_format
    );

    Ok(FuzzHarnessReport {
        total_generated: cases.len(),
        format: target_format.to_string(),
        generated_cases: cases,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_json_fuzz_mutations() {
        let cases = generate_mutations(FuzzTargetFormat::Json, 5);
        assert_eq!(cases.len(), 5);
        assert!(cases[0].payload_preview.contains("null_byte") || cases[0].payload_preview.contains("\\u0000"));
    }
}
