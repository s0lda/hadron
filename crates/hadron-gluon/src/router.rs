use hadron_lattice::{Actor, Event, QuarkCard, QuarkId};

/// Which quark should be excited next.
///
/// v1 rule (stateless, reconstructed from the field): find the most recent event
/// that addresses a quark (`to = Some(q)`). If `q` has authored any event since,
/// that turn is already handled → quiesce (`None`). Otherwise `q` is pending.
pub fn next_pending(events: &[Event]) -> Option<QuarkId> {
    let idx = events.iter().rposition(|e| e.to.is_some())?;
    let target = events[idx].to.clone().unwrap();
    let answered = events[idx + 1..]
        .iter()
        .any(|e| e.from == Actor::Quark(target.clone()));
    if answered {
        None
    } else {
        Some(target)
    }
}

/// Extract the addressee from a Markdown message: the first line that **starts**
/// with `@quarkid` (after optional leading whitespace) whose id is on the roster.
/// Returns `None` (hand back to the human / broadcast) if none match.
///
/// A delegation only counts when `@<id>` begins a line — a mention buried in prose
/// or *quoted* from history (e.g. a quark listing the conversation, which contains
/// other quarks' handles) does NOT route. This is what keeps a quark from routing
/// its reply to whoever it happened to name: the original whole-body scan tagged
/// opus's reply `→ agy` merely because it quoted the string `@agy`, spuriously
/// exciting agy. Sender-exclusion is kept as a belt-and-suspenders guard.
pub fn parse_addressee(body: &str, roster: &[QuarkCard], sender: Option<&QuarkId>) -> Option<QuarkId> {
    for line in body.lines() {
        let Some(rest) = line.trim_start().strip_prefix('@') else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        if let Some(card) = roster.iter().find(|c| c.id.as_str() == name) {
            if Some(&card.id) != sender {
                return Some(card.id.clone());
            }
        }
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{EnergyState, Flavor, Kind};

    fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
        Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.into() })
    }

    fn roster() -> Vec<QuarkCard> {
        vec![
            QuarkCard { id: QuarkId::new("orch"), flavor: Flavor::Orchestrator, energy: EnergyState::Available, provider: String::new(), model: String::new() },
            QuarkCard { id: QuarkId::new("worker"), flavor: Flavor::Worker, energy: EnergyState::Available, provider: String::new(), model: String::new() },
        ]
    }

    #[test]
    fn pending_is_unanswered_addressee() {
        let events = vec![msg(Actor::Human, Some("orch"), "go")];
        assert_eq!(next_pending(&events), Some(QuarkId::new("orch")));
    }

    #[test]
    fn answered_addressee_quiesces() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), None, "done, back to you"),
        ];
        assert_eq!(next_pending(&events), None);
    }

    #[test]
    fn handoff_routes_to_next_quark() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), Some("worker"), "@worker do the UI"),
        ];
        assert_eq!(next_pending(&events), Some(QuarkId::new("worker")));
    }

    #[test]
    fn parse_addressee_matches_a_line_starting_mention() {
        // A delegation begins a line (optionally indented).
        assert_eq!(
            parse_addressee("@worker please handle it.", &roster(), None),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(
            parse_addressee("Here's the plan.\n@worker execute it.", &roster(), None),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(parse_addressee("no mention here", &roster(), None), None);
        assert_eq!(parse_addressee("@ghost unknown", &roster(), None), None);
    }

    #[test]
    fn parse_addressee_ignores_mid_line_and_quoted_mentions() {
        // The regression: a mention buried in prose does NOT route — this is the
        // bug where a quark listing the conversation quoted "@agy"/"@worker" and
        // its reply got mis-routed there, spuriously exciting that quark.
        assert_eq!(
            parse_addressee("Sure, @worker please handle it.", &roster(), None),
            None,
            "mid-line mention must not route"
        );
        let quoted = "I can see these messages:\n1. human: @worker do X\n2. @orch replied\nThat's all.";
        assert_eq!(
            parse_addressee(quoted, &roster(), Some(&QuarkId::new("orch"))),
            None,
            "quoted mentions inside a numbered list must not route"
        );
    }

    #[test]
    fn parse_addressee_ignores_sender() {
        // A quark starting a line with its OWN handle must not self-address.
        let worker = QuarkId::new("worker");
        assert_eq!(parse_addressee("@worker I'm on it", &roster(), Some(&worker)), None);
        // A line-starting mention of a DIFFERENT quark still routes.
        assert_eq!(
            parse_addressee("@worker take over", &roster(), Some(&QuarkId::new("orch"))),
            Some(QuarkId::new("worker"))
        );
    }
}
