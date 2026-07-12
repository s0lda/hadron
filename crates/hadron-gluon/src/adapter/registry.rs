use hadron_lattice::{Flavor, QuarkId, Seat};

use crate::adapter::agy::AgyQuark;
use crate::adapter::claude::ClaudeQuark;
use crate::adapter::runner::ProcessRunner;
use crate::quark::Quark;

/// Which CLI backs a configured quark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuarkKind {
    Claude,
    Agy,
}

impl QuarkKind {
    /// Map a `Seat.provider` string to a backing CLI.
    pub fn from_provider(provider: &str) -> anyhow::Result<QuarkKind> {
        match provider {
            "claude" => Ok(QuarkKind::Claude),
            "agy" => Ok(QuarkKind::Agy),
            other => anyhow::bail!("unknown provider {other:?} (expected \"claude\" or \"agy\")"),
        }
    }
}

/// Declarative description of one quark to register: id, role, backing CLI, model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarkSpec {
    pub id: QuarkId,
    pub flavor: Flavor,
    pub kind: QuarkKind,
    pub model: String,
}

/// Enforce the naming contract: ids must be non-empty, whitespace-free tokens
/// (so `@mention` routing works), and must not collide with the reserved actor
/// names `human` / `gluon` or the `orchestrator` role alias (which routing
/// resolves to whoever holds the role, so an id of that name would shadow it).
pub fn validate_quark_id(id: &QuarkId) -> anyhow::Result<()> {
    let s = id.as_str();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        anyhow::bail!("quark id must be a non-empty, whitespace-free token (got {s:?})");
    }
    if s == "human"
        || s == "gluon"
        || s == crate::router::ORCHESTRATOR_ALIAS
        || s == crate::router::TEAM_ALIAS
    {
        anyhow::bail!("quark id '{s}' is reserved");
    }
    Ok(())
}

/// Validate the spec and build a live quark over a real `ProcessRunner`. Wiring
/// the runner does not spawn anything — the process is spawned only on `excite`.
pub fn build(spec: QuarkSpec) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&spec.id)?;
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Claude => {
            Box::new(ClaudeQuark::new(spec.id, spec.flavor, spec.model, ProcessRunner))
        }
        QuarkKind::Agy => Box::new(AgyQuark::new(spec.id, spec.flavor, spec.model, ProcessRunner)),
    };
    Ok(quark)
}

/// Build a live quark from a team-config `Seat` (id, provider, model, flavor).
pub fn build_seat(seat: &Seat) -> anyhow::Result<Box<dyn Quark>> {
    build(QuarkSpec {
        id: seat.id.clone(),
        flavor: seat.flavor.clone(),
        kind: QuarkKind::from_provider(&seat.provider)?,
        model: seat.model.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_reserved_and_malformed_ids() {
        assert!(validate_quark_id(&QuarkId::new("human")).is_err());
        assert!(validate_quark_id(&QuarkId::new("gluon")).is_err());
        assert!(validate_quark_id(&QuarkId::new("")).is_err());
        assert!(validate_quark_id(&QuarkId::new("  ")).is_err());
        assert!(validate_quark_id(&QuarkId::new("two words")).is_err());
    }

    #[test]
    fn accepts_normal_ids() {
        assert!(validate_quark_id(&QuarkId::new("claude")).is_ok());
        assert!(validate_quark_id(&QuarkId::new("agy")).is_ok());
        assert!(validate_quark_id(&QuarkId::new("worker-2")).is_ok());
    }

    #[test]
    fn build_wires_the_right_adapter() {
        let claude = build(QuarkSpec {
            id: QuarkId::new("claude"),
            flavor: Flavor::Orchestrator,
            kind: QuarkKind::Claude,
            model: "opus-4.8".into(),
        })
        .unwrap();
        assert_eq!(claude.id(), QuarkId::new("claude"));
        assert_eq!(claude.flavor(), Flavor::Orchestrator);

        let agy = build(QuarkSpec {
            id: QuarkId::new("agy"),
            flavor: Flavor::Worker,
            kind: QuarkKind::Agy,
            model: String::new(),
        })
        .unwrap();
        assert_eq!(agy.id(), QuarkId::new("agy"));
        assert_eq!(agy.flavor(), Flavor::Worker);
    }

    #[test]
    fn build_rejects_reserved_id() {
        let err = build(QuarkSpec {
            id: QuarkId::new("gluon"),
            flavor: Flavor::Worker,
            kind: QuarkKind::Agy,
            model: String::new(),
        });
        assert!(err.is_err());
    }

    #[test]
    fn build_seat_maps_provider_and_rejects_unknown() {
        use hadron_lattice::Seat;
        let seat = Seat {
            id: QuarkId::new("opus"),
            provider: "claude".into(),
            model: "opus-4.8".into(),
            flavor: Flavor::Orchestrator,
        };
        let q = build_seat(&seat).unwrap();
        assert_eq!(q.id(), QuarkId::new("opus"));

        let bad = Seat {
            id: QuarkId::new("x"),
            provider: "chatgpt".into(), // not wired yet
            model: "gpt-5".into(),
            flavor: Flavor::Worker,
        };
        assert!(build_seat(&bad).is_err());
    }
}
