use hadron_lattice::{Actor, Kind, Projection, QuarkId};

/// Render one field event as a Markdown transcript line: `**from → to:** body`.
fn render_event_line(from: &Actor, to: &Option<QuarkId>, body: &str) -> String {
    let from_s = match from {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    };
    match to {
        Some(t) => format!("**{from_s} → {}:** {body}", t.as_str()),
        None => format!("**{from_s}:** {body}"),
    }
}

/// Build the full Markdown prompt handed to a quark's CLI for one turn.
/// Deterministic and side-effect-free so it can be unit-tested exactly.
pub fn build(projection: &Projection) -> String {
    let mut p = String::new();

    // 1. Invariants — the enforced working protocol.
    if !projection.invariants.trim().is_empty() {
        p.push_str("# Working protocol (Invariants)\n");
        p.push_str(projection.invariants.trim());
        p.push_str("\n\n");
    }

    // 2. Nucleus digest — project SSOT context.
    if !projection.nucleus_digest.trim().is_empty() {
        p.push_str("# Project knowledge (nucleus)\n");
        p.push_str(projection.nucleus_digest.trim());
        p.push_str("\n\n");
    }

    // 3. The task.
    p.push_str("# Your task\n");
    p.push_str(projection.task.trim());
    p.push_str("\n\n");

    // 4. Recent field transcript.
    if !projection.field_window.is_empty() {
        p.push_str("# Recent field (most recent last)\n");
        for e in &projection.field_window {
            if let Kind::Message { body } = &e.kind {
                p.push_str(&render_event_line(&e.from, &e.to, body));
                p.push('\n');
            }
        }
        p.push('\n');
    }

    // 5. Working diff.
    if !projection.git_diff.trim().is_empty() {
        p.push_str("# Current working diff\n```diff\n");
        p.push_str(projection.git_diff.trim());
        p.push_str("\n```\n\n");
    }

    // 6. Handoff reminder — how to keep the loop coordinating / quiescing.
    p.push_str("# How to respond\n");
    p.push_str(
        "Reply in Markdown. To delegate, start a line with `@<quark-id>` and the request. \
         When the overall task is complete, reply WITHOUT any `@mention` to hand control back \
         to the human.\n",
    );

    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{EnergyState, Event, Flavor, QuarkCard};

    fn projection(task: &str) -> Projection {
        Projection {
            task: task.into(),
            invariants: "Snapshot before editing. Use @mentions.".into(),
            available_invariants: vec![],
            nucleus_digest: "## map.md\nauth lives in src/auth".into(),
            roster: vec![QuarkCard {
                id: QuarkId::new("agy"),
                flavor: Flavor::Worker,
                energy: EnergyState::Available,
                provider: String::new(),
                model: String::new(),
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "start the auth work".into() },
            )],
            git_diff: String::new(),
        }
    }

    #[test]
    fn prompt_contains_all_sections() {
        let p = build(&projection("Build login"));
        assert!(p.contains("# Working protocol (Invariants)"));
        assert!(p.contains("Snapshot before editing"));
        assert!(p.contains("# Project knowledge (nucleus)"));
        assert!(p.contains("auth lives in src/auth"));
        assert!(p.contains("# Your task"));
        assert!(p.contains("Build login"));
        assert!(p.contains("# Recent field"));
        assert!(p.contains("**human → claude:** start the auth work"));
        assert!(p.contains("@<quark-id>"));
    }

    #[test]
    fn empty_optional_sections_are_omitted() {
        let mut proj = projection("t");
        proj.invariants = String::new();
        proj.nucleus_digest = String::new();
        proj.git_diff = String::new();
        let p = build(&proj);
        assert!(!p.contains("Invariants"));
        assert!(!p.contains("nucleus"));
        assert!(!p.contains("working diff"));
        assert!(p.contains("# Your task"));
    }
}
