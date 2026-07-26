//! Pure logic for the `cargo_tree` tool family.
//! Runs `cargo metadata --no-deps --format-version 1` and parses JSON output into
//! structured workspace package information (members, dependencies, features).

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoDependencyInfo {
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoPackageInfo {
    pub name: String,
    pub version: String,
    pub is_workspace_member: bool,
    pub dependencies: Vec<CargoDependencyInfo>,
    pub features: Vec<String>,
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

    fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            JsonValue::Object(entries) => Some(entries.as_slice()),
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

/// Parse `cargo metadata --no-deps --format-version 1` JSON string.
pub fn parse_cargo_metadata_json(raw_json: &str) -> Result<Vec<CargoPackageInfo>, ForgeError> {
    let root_val = JsonValue::parse(raw_json)
        .ok_or_else(|| ForgeError::Io("Invalid metadata JSON".to_string()))?;

    let member_ids: Vec<String> = root_val
        .get("workspace_members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let packages_arr = root_val
        .get("packages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ForgeError::Io("Missing packages field in metadata".to_string()))?;

    let mut result = Vec::new();

    for pkg_val in packages_arr {
        let name = pkg_val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let version = pkg_val
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let id = pkg_val
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let is_workspace_member = member_ids.contains(&id.to_string());

        let mut dependencies = Vec::new();
        if let Some(deps_arr) = pkg_val.get("dependencies").and_then(|v| v.as_array()) {
            for dep_val in deps_arr {
                let dep_name = dep_val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let req = dep_val
                    .get("req")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let kind = dep_val
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let optional = dep_val
                    .get("optional")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                dependencies.push(CargoDependencyInfo {
                    name: dep_name,
                    req,
                    kind,
                    optional,
                });
            }
        }

        let mut features = Vec::new();
        if let Some(feats_obj) = pkg_val.get("features").and_then(|v| v.as_object()) {
            for (feat_name, _) in feats_obj {
                features.push(feat_name.clone());
            }
            features.sort();
        }

        result.push(CargoPackageInfo {
            name,
            version,
            is_workspace_member,
            dependencies,
            features,
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

/// Run `cargo metadata --no-deps --format-version 1` and return parsed package information.
pub fn get_cargo_tree(
    root: &Root,
    package: Option<&str>,
) -> Result<Vec<CargoPackageInfo>, ForgeError> {
    let args = vec![
        "metadata".to_string(),
        "--no-deps".to_string(),
        "--format-version".to_string(),
        "1".to_string(),
    ];
    let exec_out = exec(root, Program::Cargo, &args, EXEC_DEADLINE)?;
    let mut packages = parse_cargo_metadata_json(&exec_out.stdout)?;
    if let Some(pkg) = package {
        let pkg_trim = pkg.trim();
        if !pkg_trim.is_empty() {
            packages.retain(|p| p.name == pkg_trim);
        }
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_metadata_json_extracts_packages_and_dependencies() {
        let fixture = r#"{
            "packages": [
                {
                    "name": "hadron-forge",
                    "version": "0.1.0",
                    "id": "hadron-forge 0.1.0 (path+file:///home/Jake/dev/hadron/crates/hadron-forge)",
                    "dependencies": [
                        {
                            "name": "blake3",
                            "req": "1",
                            "kind": null,
                            "optional": false
                        },
                        {
                            "name": "tempfile",
                            "req": "3",
                            "kind": "dev",
                            "optional": true
                        }
                    ],
                    "features": {
                        "default": ["std"],
                        "std": []
                    }
                }
            ],
            "workspace_members": [
                "hadron-forge 0.1.0 (path+file:///home/Jake/dev/hadron/crates/hadron-forge)"
            ]
        }"#;

        let packages = parse_cargo_metadata_json(fixture).unwrap();
        assert_eq!(packages.len(), 1);
        let pkg = &packages[0];
        assert_eq!(pkg.name, "hadron-forge");
        assert_eq!(pkg.version, "0.1.0");
        assert!(pkg.is_workspace_member);
        assert_eq!(pkg.features, vec!["default", "std"]);
        assert_eq!(pkg.dependencies.len(), 2);
        assert_eq!(
            pkg.dependencies[0],
            CargoDependencyInfo {
                name: "blake3".to_string(),
                req: "1".to_string(),
                kind: None,
                optional: false,
            }
        );
        assert_eq!(
            pkg.dependencies[1],
            CargoDependencyInfo {
                name: "tempfile".to_string(),
                req: "3".to_string(),
                kind: Some("dev".to_string()),
                optional: true,
            }
        );
    }

    #[test]
    fn parse_cargo_metadata_json_handles_invalid_json() {
        let err = parse_cargo_metadata_json("not json");
        assert!(err.is_err());
    }
}

