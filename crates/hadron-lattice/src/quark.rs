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
    /// Roles this seat carries for `@role` routing — the router's read of `Seat.roles`
    /// (see `hadron_lattice::team::Seat`). Empty for a card built before role-routing
    /// existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// Whether this seat is scoped only to tasks naming one of its `roles` — the
    /// router's read of `Seat.exclusive`, carried here so the engine's dispatch filter
    /// can see it without re-reading `team.json`.
    #[serde(default, skip_serializing_if = "crate::team::is_false")]
    pub exclusive: bool,
    /// Per-seat command allow/deny lists — the router's read of `Seat.commands`,
    /// carried here so the engine's `decide()` call sites can fold them without
    /// re-reading `team.json`. Empty for a card built before this field existed.
    #[serde(default, skip_serializing_if = "crate::team::SeatCommands::is_empty")]
    pub commands: crate::team::SeatCommands,
    /// Per-seat energy limit (token ceiling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_limit: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::SeatCommands;

    #[test]
    fn quark_card_round_trips() {
        let card = QuarkCard {
            id: QuarkId::new("claude"),
            display_name: Some("Claude".into()),
            flavor: Flavor::Orchestrator,
            energy: EnergyState::Available,
            provider: "claude".into(),
            model: "opus-4.8".into(),
            roles: vec![],
            exclusive: false,
            commands: SeatCommands::default(),
            energy_limit: None,
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

    #[test]
    fn quark_card_round_trips_roles() {
        let mut card = QuarkCard {
            id: QuarkId::new("architect"),
            display_name: None,
            flavor: Flavor::Worker,
            energy: EnergyState::Available,
            provider: "claude".into(),
            model: "opus".into(),
            roles: vec![],
            exclusive: false,
            commands: SeatCommands::default(),
            energy_limit: None,
        };
        // Default (empty roles, not exclusive) must not appear in the JSON — back-compat
        // with a card built before role-routing existed.
        let json = serde_json::to_string(&card).unwrap();
        assert!(!json.contains("roles"), "{json}");
        assert!(!json.contains("exclusive"), "{json}");

        card.roles = vec!["architect".into()];
        card.exclusive = true;
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("\"roles\":[\"architect\"]"), "{json}");
        assert!(json.contains("\"exclusive\":true"), "{json}");
        let back: QuarkCard = serde_json::from_str(&json).unwrap();
        assert_eq!(card, back);

        // A card with no `roles`/`exclusive` key (written before role-routing) still
        // loads, empty/not-exclusive.
        let legacy = r#"{"id":"agy","flavor":"worker","energy":"available"}"#;
        let legacy_card: QuarkCard = serde_json::from_str(legacy).unwrap();
        assert!(legacy_card.roles.is_empty());
        assert!(!legacy_card.exclusive);
    }
}
