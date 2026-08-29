use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VcrKey {
    pub method: String,
    pub uri: String,
    pub body_hash: String,
}

pub struct VcrTape {
    entries: HashMap<VcrKey, String>,
}

impl VcrTape {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn record(&mut self, method: &str, uri: &str, body: &str, response: &str) {
        let key = VcrKey {
            method: method.to_string(),
            uri: uri.to_string(),
            body_hash: format!("{:x}", md5_or_simple(body)),
        };
        self.entries.insert(key, response.to_string());
    }

    pub fn replay(&self, method: &str, uri: &str, body: &str) -> Option<String> {
        let key = VcrKey {
            method: method.to_string(),
            uri: uri.to_string(),
            body_hash: format!("{:x}", md5_or_simple(body)),
        };
        self.entries.get(&key).cloned()
    }
}

fn md5_or_simple(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcr_record_and_replay_cycle() {
        let mut cassette = VcrTape::new();
        cassette.record("GET", "https://api.anthropic.com/v1/models", "", "{\"models\":[]}");

        let replayed = cassette.replay("GET", "https://api.anthropic.com/v1/models", "");
        assert_eq!(replayed, Some("{\"models\":[]}".to_string()));

        let miss = cassette.replay("POST", "https://api.anthropic.com/v1/messages", "{}");
        assert_eq!(miss, None);
    }
}
