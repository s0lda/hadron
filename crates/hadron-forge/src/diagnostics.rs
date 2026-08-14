//! Pure logic for the `diagnostics` tool family.
//! Runs `cargo check --message-format=json` and parses JSON output lines into
//! compact diagnostic items `{file, line, level, message, code}`.

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub level: String,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn parse(input: &str) -> Option<Self> {
        let mut chars = input.trim_start().chars().peekable();
        Self::parse_val(&mut chars)
    }

    fn parse_val<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) -> Option<Self> {
        skip_ws(chars);
        let ch = *chars.peek()?;
        match ch {
            'n' => expect_str(chars, "null").map(|_| JsonValue::Null),
            't' => expect_str(chars, "true").map(|_| JsonValue::Bool(true)),
            'f' => expect_str(chars, "false").map(|_| JsonValue::Bool(false)),
            '"' => parse_string(chars).map(JsonValue::String),
            '[' => parse_array(chars),
            '{' => parse_object(chars),
            '-' | '0'..='9' => parse_number(chars),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) => Some(*n as u64),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }
}

fn skip_ws<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

fn expect_str<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
    expected: &str,
) -> Option<()> {
    for e in expected.chars() {
        if chars.next()? != e {
            return None;
        }
    }
    Some(())
}

fn parse_string<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut s = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(s),
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '/' => s.push('/'),
                    'b' => s.push('\x08'),
                    'f' => s.push('\x0c'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                        if hex.len() == 4 {
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(ch) = char::from_u32(code) {
                                    s.push(ch);
                                }
                            }
                        }
                    }
                    _ => s.push(escaped),
                }
            }
            _ => s.push(c),
        }
    }
    None
}

fn parse_number<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
) -> Option<JsonValue> {
    let mut num_str = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
            num_str.push(c);
            chars.next();
        } else {
            break;
        }
    }
    num_str.parse::<f64>().ok().map(JsonValue::Number)
}

fn parse_array<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) -> Option<JsonValue> {
    if chars.next()? != '[' {
        return None;
    }
    let mut items = Vec::new();
    loop {
        skip_ws(chars);
        if chars.peek() == Some(&']') {
            chars.next();
            return Some(JsonValue::Array(items));
        }
        let val = JsonValue::parse_val(chars)?;
        items.push(val);
        skip_ws(chars);
        match chars.peek() {
            Some(&',') => {
                chars.next();
            }
            Some(&']') => {
                chars.next();
                return Some(JsonValue::Array(items));
            }
            _ => return None,
        }
    }
}

fn parse_object<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
) -> Option<JsonValue> {
    if chars.next()? != '{' {
        return None;
    }
    let mut entries = Vec::new();
    loop {
        skip_ws(chars);
        if chars.peek() == Some(&'}') {
            chars.next();
            return Some(JsonValue::Object(entries));
        }
        let key = parse_string(chars)?;
        skip_ws(chars);
        if chars.next()? != ':' {
            return None;
        }
        let val = JsonValue::parse_val(chars)?;
        entries.push((key, val));
        skip_ws(chars);
        match chars.peek() {
            Some(&',') => {
                chars.next();
            }
            Some(&'}') => {
                chars.next();
                return Some(JsonValue::Object(entries));
            }
            _ => return None,
        }
    }
}

/// Parse raw NDJSON string output from `cargo check --message-format=json`.
pub fn parse_diagnostics_json(raw_json: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for line in raw_json.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(json) = JsonValue::parse(line) else {
            continue;
        };
        if json.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(msg_obj) = json.get("message") else {
            continue;
        };

        let message = msg_obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let level = msg_obj
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let code = match msg_obj.get("code") {
            Some(JsonValue::String(c)) => Some(c.clone()),
            Some(obj @ JsonValue::Object(_)) => {
                obj.get("code").and_then(|v| v.as_str()).map(|s| s.to_string())
            }
            _ => None,
        };

        let spans = msg_obj.get("spans").and_then(|v| v.as_array());
        let primary_span = spans.and_then(|arr| {
            arr.iter()
                .find(|s| s.get("is_primary").and_then(|v| v.as_bool()).unwrap_or(false))
                .or_else(|| arr.first())
        });

        let file = primary_span
            .and_then(|s| s.get("file_name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let line_num = primary_span
            .and_then(|s| s.get("line_start"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        diagnostics.push(Diagnostic {
            file,
            line: line_num,
            level,
            message,
            code,
        });
    }

    diagnostics
}

/// Run `cargo check --message-format=json` (optionally targeting `package`) and return parsed diagnostics.
/// Parse multi-language compiler and linter diagnostic outputs into structured items.
pub fn parse_diagnostics(toolchain: &str, output: &str) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    let norm_tool = toolchain.trim().to_lowercase();

    if norm_tool == "cargo" || norm_tool == "rust" || norm_tool == "rustc" {
        let json_diags = parse_diagnostics_json(output);
        if !json_diags.is_empty() {
            for d in json_diags {
                items.push(DiagnosticItem {
                    file: d.file,
                    line: d.line,
                    col: 1,
                    severity: d.level,
                    message: d.message,
                    code: d.code,
                });
            }
            return items;
        }
    }

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 1. TSC format: src/app.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.
        if let Some((file, rest)) = trimmed.split_once('(') {
            if let Some((coords, msg_part)) = rest.split_once("):") {
                if let Some((line_s, col_s)) = coords.split_once(',') {
                    if let (Ok(line_num), Ok(col_num)) = (
                        line_s.trim().parse::<usize>(),
                        col_s.trim().parse::<usize>(),
                    ) {
                        let msg_clean = msg_part.trim();
                        let (sev, code, message) = parse_message_severity_code(msg_clean);
                        items.push(DiagnosticItem {
                            file: file.trim().to_string(),
                            line: line_num,
                            col: col_num,
                            severity: sev,
                            message,
                            code,
                        });
                        continue;
                    }
                }
            }
        }

        // 2. Colon-separated format: file:line:col: [error/code] message
        // e.g. src/app.py:10:5: F401 [*] 'os' imported but unused
        // e.g. main.go:14:2: undefined: fmt.Printl
        let parts: Vec<&str> = trimmed.splitn(4, ':').collect();
        if parts.len() >= 4 {
            if let (Ok(line_num), Ok(col_num)) = (
                parts[1].trim().parse::<usize>(),
                parts[2].trim().parse::<usize>(),
            ) {
                let file = parts[0].trim().to_string();
                let msg_clean = parts[3].trim();
                let (sev, code, message) = parse_message_severity_code(msg_clean);
                items.push(DiagnosticItem {
                    file,
                    line: line_num,
                    col: col_num,
                    severity: sev,
                    message,
                    code,
                });
                continue;
            }
        }

        // 3. Pytest format: FAILED tests/test_calc.py::test_add - AssertionError: ...
        if let Some(rest) = trimmed.strip_prefix("FAILED ") {
            let (target, msg) = rest.split_once(" - ").unwrap_or((rest, ""));
            let file = target.split("::").next().unwrap_or(target).trim().to_string();
            items.push(DiagnosticItem {
                file,
                line: 1,
                col: 1,
                severity: "error".to_string(),
                message: if msg.is_empty() {
                    target.to_string()
                } else {
                    msg.to_string()
                },
                code: None,
            });
            continue;
        }
    }

    items
}

fn parse_message_severity_code(msg: &str) -> (String, Option<String>, String) {
    let lower = msg.to_lowercase();
    let severity = if lower.starts_with("error") {
        "error".to_string()
    } else if lower.starts_with("warning") || lower.starts_with("warn") {
        "warning".to_string()
    } else if lower.starts_with("note") || lower.starts_with("info") {
        "info".to_string()
    } else {
        "error".to_string()
    };

    let mut code = None;
    let mut message = msg.to_string();

    for token in msg.split_whitespace() {
        let clean_token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if (clean_token.starts_with("TS") && clean_token.len() > 2)
            || (clean_token.starts_with('E')
                && clean_token.len() > 3
                && clean_token[1..].chars().all(|c| c.is_ascii_digit()))
            || (clean_token.starts_with('F')
                && clean_token.len() > 3
                && clean_token[1..].chars().all(|c| c.is_ascii_digit()))
        {
            code = Some(clean_token.to_string());
            break;
        }
    }

    if let Some((_, rest)) = msg.split_once(':') {
        if !rest.trim().is_empty() {
            message = rest.trim().to_string();
        }
    }

    (severity, code, message)
}

/// Run `cargo check --message-format=json` (optionally targeting `package`) and return parsed diagnostics.
pub fn get_diagnostics(
    root: &Root,
    package: Option<&str>,
) -> Result<Vec<Diagnostic>, ForgeError> {
    let mut args = vec!["check".to_string(), "--message-format=json".to_string()];
    if let Some(pkg) = package {
        if !pkg.trim().is_empty() {
            args.push("-p".to_string());
            args.push(pkg.trim().to_string());
        }
    }
    let exec_out = exec(root, Program::Cargo, &args, EXEC_DEADLINE)?;
    Ok(parse_diagnostics_json(&exec_out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tsc_and_python_diagnostics() {
        let tsc_err =
            "src/app.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.";
        let items = parse_diagnostics("tsc", tsc_err);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].line, 12);
        assert_eq!(items[0].col, 5);
        assert_eq!(items[0].code.as_deref(), Some("TS2322"));

        let py_err = "FAILED tests/test_calc.py::test_add - AssertionError: assert 1 == 2";
        let py_items = parse_diagnostics("pytest", py_err);
        assert_eq!(py_items.len(), 1);
        assert_eq!(py_items[0].file, "tests/test_calc.py");
    }

    #[test]
    fn parse_diagnostics_json_extracts_messages_and_filters_artifacts() {
        let fixture = r#"
{"reason":"compiler-artifact","package_id":"hadron-forge 0.1.0"}
{"reason":"compiler-message","package_id":"hadron-forge 0.1.0","target":{"name":"hadron-forge"},"message":{"message":"unused variable: `x`","code":{"code":"unused_variables","explanation":null},"level":"warning","spans":[{"file_name":"src/lib.rs","byte_start":10,"byte_end":11,"line_start":15,"line_end":15,"column_start":5,"column_end":6,"is_primary":true,"text":[],"suggested_replacement":null,"suggestion_applicability":null,"expansion":null}],"children":[],"rendered":"warning: unused variable..."}}
{"reason":"compiler-message","package_id":"hadron-forge 0.1.0","target":{"name":"hadron-forge"},"message":{"message":"mismatched types","code":"E0308","level":"error","spans":[{"file_name":"src/main.rs","byte_start":20,"byte_end":25,"line_start":42,"line_end":42,"column_start":12,"column_end":17,"is_primary":true,"text":[],"suggested_replacement":null,"suggestion_applicability":null,"expansion":null}],"children":[],"rendered":"error: mismatched types..."}}
"#;

        let diags = parse_diagnostics_json(fixture);
        assert_eq!(diags.len(), 2);
        assert_eq!(
            diags[0],
            Diagnostic {
                file: "src/lib.rs".to_string(),
                line: 15,
                level: "warning".to_string(),
                message: "unused variable: `x`".to_string(),
                code: Some("unused_variables".to_string()),
            }
        );
        assert_eq!(
            diags[1],
            Diagnostic {
                file: "src/main.rs".to_string(),
                line: 42,
                level: "error".to_string(),
                message: "mismatched types".to_string(),
                code: Some("E0308".to_string()),
            }
        );
    }

    #[test]
    fn parse_diagnostics_json_handles_empty_spans_and_no_code() {
        let fixture = r#"{"reason":"compiler-message","package_id":"foo","message":{"message":"custom note","code":null,"level":"note","spans":[],"children":[],"rendered":"note: custom note"}}"#;
        let diags = parse_diagnostics_json(fixture);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].file, "");
        assert_eq!(diags[0].line, 0);
        assert_eq!(diags[0].level, "note");
        assert_eq!(diags[0].code, None);
    }

    #[test]
    fn get_diagnostics_refuses_path_escape_in_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let err = get_diagnostics(&root, Some("../evil"));
        assert!(matches!(err, Err(ForgeError::Rejected(_))));
    }
}


