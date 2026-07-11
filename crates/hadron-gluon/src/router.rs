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
pub fn parse_addressee(body: &str, roster: &[QuarkCard]) -> Option<QuarkId> {
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
            return Some(card.id.clone());
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
            QuarkCard { id: QuarkId::new("orch"), flavor: Flavor::Orchestrator, energy: EnergyState::Available },
            QuarkCard { id: QuarkId::new("worker"), flavor: Flavor::Worker, energy: EnergyState::Available },
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
            parse_addressee("Sure, @worker please handle it.", &roster()),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(parse_addressee("no mention here", &roster()), None);
        assert_eq!(parse_addressee("@ghost unknown", &roster()), None);
    }

}
