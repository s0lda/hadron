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

/// Extract the addressee from a Markdown message: the first `@quarkid` mention
/// whose id is on the roster. Returns `None` (hand back to human) if none match.
pub fn parse_addressee(body: &str, roster: &[QuarkCard], sender: Option<&QuarkId>) -> Option<QuarkId> {
    for word in body.split_whitespace() {
        let Some(rest) = word.strip_prefix('@') else {
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
    fn parse_addressee_finds_mention() {
        assert_eq!(
            parse_addressee("Sure, @worker please handle it.", &roster(), None),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(parse_addressee("no mention here", &roster(), None), None);
        assert_eq!(parse_addressee("@ghost unknown", &roster(), None), None);
    }

    #[test]
    fn parse_addressee_ignores_sender() {
        let orch = QuarkId::new("orch");
        // An orchestrator's own mention is ignored, next valid target is found.
        assert_eq!(
            parse_addressee("I see @orch in history. @worker do the work.", &roster(), Some(&orch)),
            Some(QuarkId::new("worker"))
        );
        // If only the sender is mentioned, it returns None.
        assert_eq!(
            parse_addressee("I am @worker", &roster(), Some(&QuarkId::new("worker"))),
            None
        );
    }

}
