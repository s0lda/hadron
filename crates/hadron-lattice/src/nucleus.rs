use serde::{Deserialize, Serialize};

/// The manifest of a nucleus layer (`index.json`). `sources` lists the markdown
/// docs (relative names) that make up this layer, in the order they should be
/// digested. `last_verified_commit` is the project commit the knowledge was last
/// confirmed against — compared to HEAD to detect staleness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleusIndex {
    pub version: u32,
    #[serde(default)]
    pub last_verified_commit: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl Default for NucleusIndex {
    fn default() -> Self {
        NucleusIndex { version: 1, last_verified_commit: None, sources: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trips_and_defaults_missing_fields() {
        let idx: NucleusIndex = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(idx.sources.len(), 0);
        assert_eq!(idx.last_verified_commit, None);

        let full = NucleusIndex {
            version: 1,
            last_verified_commit: Some("abc123".into()),
            sources: vec!["map.md".into(), "conventions.md".into()],
        };
        let json = serde_json::to_string(&full).unwrap();
        let back: NucleusIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(full, back);
    }
}
