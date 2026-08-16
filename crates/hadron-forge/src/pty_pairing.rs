use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Live annotation pinned to a PTY output coordinate or text pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtyAnnotation {
    pub annotation_id: String,
    pub author_quark: String,
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub color_hint: Option<String>,
}

/// Shared PTY pairing session multiplexer.
#[derive(Debug, Clone, Default)]
pub struct PtyPairingBroker {
    active_subscribers: HashMap<String, Vec<String>>,
    annotations: HashMap<String, Vec<PtyAnnotation>>,
}

impl PtyPairingBroker {
    pub fn new() -> Self {
        Self {
            active_subscribers: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    pub fn subscribe(&mut self, session_id: &str, quark_id: &str) {
        self.active_subscribers
            .entry(session_id.to_string())
            .or_default()
            .push(quark_id.to_string());
    }

    pub fn add_annotation(&mut self, session_id: &str, annotation: PtyAnnotation) {
        self.annotations
            .entry(session_id.to_string())
            .or_default()
            .push(annotation);
    }

    pub fn get_annotations(&self, session_id: &str) -> Vec<PtyAnnotation> {
        self.annotations
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_pairing_broker() {
        let mut broker = PtyPairingBroker::new();
        broker.subscribe("pty-1", "agy");
        broker.subscribe("pty-1", "reviewer");

        broker.add_annotation(
            "pty-1",
            PtyAnnotation {
                annotation_id: "ann-1".into(),
                author_quark: "reviewer".into(),
                row: 10,
                col: 5,
                text: "Cargo build failed here".into(),
                color_hint: Some("red".into()),
            },
        );

        let annotations = broker.get_annotations("pty-1");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].author_quark, "reviewer");
    }
}
