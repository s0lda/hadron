use serde::{Deserialize, Serialize};

use crate::QuarkId;

/// Who authored an event. Serializes as a bare string: "human", "gluon",
/// or the quark's id. `human` and `gluon` are reserved names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Human,
    Gluon,
    Quark(QuarkId),
}

impl Serialize for Actor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let text = match self {
            Actor::Human => "human",
            Actor::Gluon => "gluon",
            Actor::Quark(q) => q.as_str(),
        };
        s.serialize_str(text)
    }
}

impl<'de> Deserialize<'de> for Actor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "human" => Actor::Human,
            "gluon" => Actor::Gluon,
            _ => Actor::Quark(QuarkId(s)),
        })
    }
}

/// Lifecycle state of a quark, used to drive the chamber roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarkState {
    Ground,
    Excited,
    Thinking,
    Waiting,
    Blocked,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_serializes_as_bare_string() {
        assert_eq!(serde_json::to_string(&Actor::Human).unwrap(), r#""human""#);
        assert_eq!(serde_json::to_string(&Actor::Gluon).unwrap(), r#""gluon""#);
        assert_eq!(
            serde_json::to_string(&Actor::Quark(QuarkId::new("claude"))).unwrap(),
            r#""claude""#
        );
    }

    #[test]
    fn actor_round_trips_quark_and_reserved() {
        for actor in [
            Actor::Human,
            Actor::Gluon,
            Actor::Quark(QuarkId::new("agy")),
        ] {
            let json = serde_json::to_string(&actor).unwrap();
            let back: Actor = serde_json::from_str(&json).unwrap();
            assert_eq!(actor, back);
        }
    }

    #[test]
    fn quark_state_is_snake_case() {
        assert_eq!(serde_json::to_string(&QuarkState::Ground).unwrap(), r#""ground""#);
        let back: QuarkState = serde_json::from_str(r#""excited""#).unwrap();
        assert_eq!(back, QuarkState::Excited);
    }
}
