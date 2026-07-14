use serde::{Deserialize, Serialize};

/// Stable identifier for a quark (agent), e.g. "claude", "agy".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuarkId(pub String);

impl QuarkId {
    pub fn new(s: impl Into<String>) -> Self {
        QuarkId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A quark's role in the studio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    Orchestrator,
    Worker,
}

/// Coarse availability of a quark's budget/quota. v1 seam: always `Available`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnergyState {
    Available,
    Depleted,
    Unknown,
}

/// A roster entry shown to the orchestrator so it can assign work. `provider`
/// (the backing CLI/vendor, e.g. "claude", "agy") and `model` (e.g. "opus-4.8")
/// make a seat legible so the human's per-quark trust decision is informed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarkCard {
    pub id: QuarkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub flavor: Flavor,
    pub energy: EnergyState,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quark_card_round_trips() {
        let card = QuarkCard {
            id: QuarkId::new("claude"),
            display_name: Some("Claude".into()),
            flavor: Flavor::Orchestrator,
            energy: EnergyState::Available,
            provider: "claude".into(),
            model: "opus-4.8".into(),
        };
        let json = serde_json::to_string(&card).unwrap();
        assert_eq!(json, r#"{"id":"claude","display_name":"Claude","flavor":"orchestrator","energy":"available","provider":"claude","model":"opus-4.8"}"#);
        let back: QuarkCard = serde_json::from_str(&json).unwrap();
        assert_eq!(card, back);
    }

    #[test]
    fn quark_card_without_provider_model_defaults_empty() {
        // A card written before legibility fields exist still loads.
        let json = r#"{"id":"agy","flavor":"worker","energy":"available"}"#;
        let card: QuarkCard = serde_json::from_str(json).unwrap();
        assert_eq!(card.provider, "");
        assert_eq!(card.model, "");
    }
}
