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

/// A roster entry shown to the orchestrator so it can assign work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarkCard {
    pub id: QuarkId,
    pub flavor: Flavor,
    pub energy: EnergyState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quark_card_round_trips() {
        let card = QuarkCard {
            id: QuarkId::new("claude"),
            flavor: Flavor::Orchestrator,
            energy: EnergyState::Available,
        };
        let json = serde_json::to_string(&card).unwrap();
        assert_eq!(
            json,
            r#"{"id":"claude","flavor":"orchestrator","energy":"available"}"#
        );
        let back: QuarkCard = serde_json::from_str(&json).unwrap();
        assert_eq!(card, back);
    }
}
