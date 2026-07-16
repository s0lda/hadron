use hadron_lattice::{Actor, Event, Flavor, Kind, QuarkCard, QuarkId, QuarkState};

/// Which quark should be excited next.
///
/// v1 rule (stateless, reconstructed from the field): find the most recent event
/// that addresses a quark (`to = Some(q)`). If `q` has authored any event since
/// that represents a reply (a message) or a terminal/pause status (ground, error, blocked, waiting),
/// that turn is already handled → quiesce (`None`). Otherwise `q` is pending.
pub fn next_pending(events: &[Event]) -> Option<QuarkId> {
    let idx = events.iter().rposition(|e| e.to.is_some())?;
    let target = events[idx].to.clone().unwrap();
    let answered = events[idx + 1..].iter().any(|e| {
        e.from == Actor::Quark(target.clone())
            && (matches!(e.kind, Kind::Message { .. })
                || matches!(
                    e.kind,
                    Kind::Status {
                        state: QuarkState::Ground
                            | QuarkState::Error
                            | QuarkState::Blocked
                            | QuarkState::Waiting
                    }
                ))
    });
    if answered {
        None
    } else {
        Some(target)
    }
}

/// The role alias every quark can address without knowing who currently holds the
/// role. `@orchestrator` resolves against the live roster, so re-flavouring the
/// team in `team.json` moves the escalation target with it and no prompt, test, or
/// invariant that names the role goes stale. Reserved in `validate_quark_id`, so a
/// quark can never take the alias as its own id.
pub const ORCHESTRATOR_ALIAS: &str = "orchestrator";

/// The broadcast alias: `@team` addresses every quark on the roster, which the
/// daemon then fans out to sequentially (each answers in turn) — the "everyone
/// report status" case.
///
/// Deliberately **human-only**. `parse_addressee` (the quark→quark path) does not
/// resolve it, because a quark that broadcasts to the team excites every other
/// quark, each of whom may broadcast back: an amplification loop bounded only by
/// the exchange backstop. A human can rally the team; a quark must name who it
/// wants.
pub const TEAM_ALIAS: &str = "team";

#[derive(Clone)]
enum ResolvedMention<'a> {
    Quark(&'a QuarkCard),
    Team,
}

/// Tries to find the longest target that matches the START of `text` (case-insensitively).
/// To prevent partial word matches (e.g. `@Google` matching `@GoogleBot`),
/// the character immediately following the match in `text` must NOT be a valid
/// intra-word mention character (alphanumeric, '-', '_'). Note that spaces ARE
/// allowed inside display names, but a matched name's boundary still applies.
fn match_longest_mention<'a>(text: &str, roster: &'a [QuarkCard]) -> Option<(usize, ResolvedMention<'a>)> {
    let mut best_match: Option<(usize, ResolvedMention<'a>)> = None;

    let mut try_match = |target_name: &str, resolution: ResolvedMention<'a>| {
        if text.len() >= target_name.len() {
            let prefix = &text[..target_name.len()];
            if prefix.eq_ignore_ascii_case(target_name) {
                let next_char = text[target_name.len()..].chars().next();
                let boundary_ok = match next_char {
                    Some(c) if c.is_alphanumeric() || c == '-' || c == '_' => false,
                    _ => true,
                };
                
                if boundary_ok {
                    let len = target_name.len();
                    if best_match.as_ref().map_or(true, |(best_len, _)| len > *best_len) {
                        best_match = Some((len, resolution.clone()));
                    }
                }
            }
        }
    };

    try_match(TEAM_ALIAS, ResolvedMention::Team);
    
    if let Some(orch) = roster.iter().find(|c| c.flavor == Flavor::Orchestrator) {
        try_match(ORCHESTRATOR_ALIAS, ResolvedMention::Quark(orch));
    }

    for card in roster {
        try_match(card.id.as_str(), ResolvedMention::Quark(card));
        if let Some(dn) = &card.display_name {
            try_match(dn.as_str(), ResolvedMention::Quark(card));
        }
    }

    best_match
}

/// Extract the addressee from a Markdown message: the first line that **starts**
/// with `@quarkid` (after optional leading whitespace) whose id is on the roster.
/// `@orchestrator` also routes here — that is the escalation path a worker uses to
/// put a decision back on whoever holds the role. Returns `None` (hand back to the
/// human / broadcast) if none match.
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
        if let Some((_, resolution)) = match_longest_mention(rest, roster) {
            match resolution {
                ResolvedMention::Quark(card) => {
                    // Sender-exclusion also makes `@orchestrator` a no-op for the
                    // orchestrator itself: its own reply falls through to "no addressee"
                    // and control returns to the human, as it should.
                    if Some(&card.id) != sender {
                        return Some(card.id.clone());
                    }
                }
                ResolvedMention::Team => {}
            }
        }
    }
    None
}

/// Every roster quark id `@mentioned` ANYWHERE in a human message, in first-seen
/// order, deduped. Unlike `parse_addressee` (line-start only, for quark replies
/// where an incidental/quoted mention must not route), a human addresses whoever
/// they name — so "@opus do X and you @agy do Y" returns `[opus, agy]` and the
/// daemon fans the turn out to each. Mentions of ids not on the roster are
/// ignored; an `@` not starting a word (e.g. inside `email@host`) is not a mention.
pub fn human_mentions(body: &str, roster: &[QuarkCard]) -> Vec<QuarkId> {
    let mut out: Vec<QuarkId> = Vec::new();
    let mut i = 0;
    while let Some(at_idx) = body[i..].find('@') {
        let actual_at = i + at_idx;
        let valid_start = actual_at == 0 || body.as_bytes()[actual_at - 1].is_ascii_whitespace();
        
        if valid_start {
            let rest = &body[actual_at + 1..];
            if let Some((match_len, resolution)) = match_longest_mention(rest, roster) {
                match resolution {
                    ResolvedMention::Team => {
                        for card in roster {
                            if !out.contains(&card.id) {
                                out.push(card.id.clone());
                            }
                        }
                    }
                    ResolvedMention::Quark(card) => {
                        if !out.contains(&card.id) {
                            out.push(card.id.clone());
                        }
                    }
                }
                i = actual_at + 1 + match_len;
                continue;
            }
        }
        i = actual_at + 1;
    }
    out
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
            QuarkCard { id: QuarkId::new("orch"), display_name: None, flavor: Flavor::Orchestrator, energy: EnergyState::Available, provider: String::new(), model: String::new() },
            QuarkCard { id: QuarkId::new("worker"), display_name: None, flavor: Flavor::Worker, energy: EnergyState::Available, provider: String::new(), model: String::new() },
        ]
    }

    /// The escalation path: a worker addresses the ROLE, and it lands on whoever
    /// holds it — no worker ever hardcodes an orchestrator's id.
    #[test]
    fn orchestrator_alias_resolves_to_the_role_holder() {
        let worker = QuarkId::new("worker");
        assert_eq!(
            parse_addressee("@orchestrator which schema should I use?", &roster(), Some(&worker)),
            Some(QuarkId::new("orch"))
        );
        // A human can address the role too, alongside plain id mentions.
        assert_eq!(human_mentions("@orchestrator take this", &roster()), vec![QuarkId::new("orch")]);
        // Id mentions keep working — the alias is an addition, not a replacement.
        assert_eq!(
            human_mentions("@orch do X and @worker do Y", &roster()),
            vec![QuarkId::new("orch"), QuarkId::new("worker")]
        );
    }

    /// `@team` rallies the whole roster: the human addresses everyone once and the
    /// daemon fans the turn out to each in sequence (the status-check case).
    #[test]
    fn team_alias_addresses_the_whole_roster() {
        assert_eq!(
            human_mentions("@team report progress please", &roster()),
            vec![QuarkId::new("orch"), QuarkId::new("worker")]
        );
        // Mixing `@team` with an id names each quark once, not twice.
        assert_eq!(
            human_mentions("@team status, @worker especially you", &roster()),
            vec![QuarkId::new("orch"), QuarkId::new("worker")]
        );
    }

    /// A quark broadcasting to `@team` would excite every other quark, each of whom
    /// could broadcast back — an amplification loop. The quark→quark path does not
    /// resolve the alias at all: a quark must name who it wants.
    #[test]
    fn a_quark_cannot_broadcast_to_the_team() {
        let worker = QuarkId::new("worker");
        assert_eq!(parse_addressee("@team status?", &roster(), Some(&worker)), None);
    }

    /// Re-flavouring the team retargets the alias with no code or prompt change —
    /// the whole point of routing by role instead of by id.
    #[test]
    fn orchestrator_alias_follows_the_role_across_a_reflavour() {
        let mut reflavoured = roster();
        reflavoured[0].flavor = Flavor::Worker; // orch demoted…
        reflavoured[1].flavor = Flavor::Orchestrator; // …worker promoted
        assert_eq!(
            parse_addressee("@orchestrator your call", &reflavoured, Some(&QuarkId::new("orch"))),
            Some(QuarkId::new("worker"))
        );
    }

    /// Jake types `@Opus` as often as `@opus`; a mention that misses on case would
    /// silently route nowhere rather than fail loudly, so pin both ids and aliases.
    #[test]
    fn mentions_resolve_regardless_of_case() {
        let r = roster();
        assert_eq!(
            parse_addressee("@Worker take this", &r, Some(&QuarkId::new("orch"))),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(
            parse_addressee("@ORCHESTRATOR your call", &r, Some(&QuarkId::new("worker"))),
            Some(QuarkId::new("orch"))
        );
        assert_eq!(
            human_mentions("@Team report progress", &r),
            vec![QuarkId::new("orch"), QuarkId::new("worker")]
        );
    }

    /// The orchestrator writing `@orchestrator` addresses itself — sender-exclusion
    /// makes that a no-op, so the reply falls through to "no addressee" and control
    /// returns to the human instead of the orchestrator exciting itself forever.
    #[test]
    fn orchestrator_cannot_escalate_to_itself() {
        let orch = QuarkId::new("orch");
        assert_eq!(parse_addressee("@orchestrator hmm", &roster(), Some(&orch)), None);
    }

    /// With nobody holding the role, the alias resolves to nobody rather than
    /// guessing a target.
    #[test]
    fn orchestrator_alias_resolves_to_nobody_on_an_orchestrator_less_roster() {
        let workers_only: Vec<QuarkCard> =
            roster().into_iter().map(|mut c| { c.flavor = Flavor::Worker; c }).collect();
        assert_eq!(parse_addressee("@orchestrator anyone?", &workers_only, None), None);
        assert_eq!(human_mentions("@orchestrator anyone?", &workers_only), Vec::<QuarkId>::new());
    }

    #[test]
    fn pending_is_unanswered_addressee() {
        let events = vec![msg(Actor::Human, Some("orch"), "go")];
        assert_eq!(next_pending(&events), Some(QuarkId::new("orch")));
    }

    #[test]
    fn excited_status_does_not_count_as_answered() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            Event::new(
                Actor::Quark(QuarkId::new("orch")),
                None,
                Kind::Status { state: QuarkState::Excited },
            ),
        ];
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

    #[test]
    fn human_mentions_finds_every_mention_anywhere_deduped_in_order() {
        // A human addresses whoever they name, mid-sentence and in any order —
        // this is the multi-dispatch case: "@orch do X and you @worker do Y".
        assert_eq!(
            human_mentions("@orch please proceed and you @worker start task 3", &roster()),
            vec![QuarkId::new("orch"), QuarkId::new("worker")]
        );
        // Order follows first appearance; duplicates collapse.
        assert_eq!(
            human_mentions("@worker and @orch, then @worker again", &roster()),
            vec![QuarkId::new("worker"), QuarkId::new("orch")]
        );
        // Punctuation ends a handle; unknown ids and bare '@' are ignored.
        assert_eq!(human_mentions("hey @orch, thanks!", &roster()), vec![QuarkId::new("orch")]);
        assert_eq!(human_mentions("@ghost @nobody nothing here", &roster()), Vec::<QuarkId>::new());
        // An '@' not starting a word (an email) is not a mention.
        assert_eq!(human_mentions("mail me at jake@orch.dev", &roster()), Vec::<QuarkId>::new());
    }
}
