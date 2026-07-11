use hadron_lattice::{Flavor, QuarkId};

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

/// Declarative description of one quark to register: its id, role, and backing CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarkSpec {
    pub id: QuarkId,
    pub flavor: Flavor,
    pub kind: QuarkKind,
}

/// Enforce the naming contract: ids must be non-empty, whitespace-free tokens
/// (so `@mention` routing works), and must not collide with the reserved actor
/// names `human` / `gluon`.
pub fn validate_quark_id(id: &QuarkId) -> anyhow::Result<()> {
    let s = id.as_str();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        anyhow::bail!("quark id must be a non-empty, whitespace-free token (got {s:?})");
    }
    if s == "human" || s == "gluon" {
        anyhow::bail!("quark id '{s}' is reserved");
    }
    Ok(())
}

/// Validate the spec and build a live quark over a real `ProcessRunner`. Wiring
/// the runner does not spawn anything — the process is spawned only on `excite`.
pub fn build(spec: QuarkSpec) -> anyhow::Result<Box<dyn Quark>> {
    validate_quark_id(&spec.id)?;
    let quark: Box<dyn Quark> = match spec.kind {
        QuarkKind::Claude => Box::new(ClaudeQuark::new(spec.id, spec.flavor, ProcessRunner)),
        QuarkKind::Agy => Box::new(AgyQuark::new(spec.id, spec.flavor, ProcessRunner)),
    };
    Ok(quark)
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
        })
        .unwrap();
        assert_eq!(claude.id(), QuarkId::new("claude"));
        assert_eq!(claude.flavor(), Flavor::Orchestrator);

        let agy = build(QuarkSpec {
            id: QuarkId::new("agy"),
            flavor: Flavor::Worker,
            kind: QuarkKind::Agy,
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
        });
        assert!(err.is_err());
    }
}
