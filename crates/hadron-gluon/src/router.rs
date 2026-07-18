use hadron_lattice::{Actor, EnergyState, Event, Flavor, Kind, QuarkCard, QuarkId, QuarkState};

/// Which quark should be excited next.
///
/// v1 rule (stateless, reconstructed from the field): find the most recent event
/// that addresses a quark (`to = Some(q)`). If `q` has authored any event since
/// that represents a reply (a message) or a terminal/pause status (ground, error, blocked, waiting),
/// that turn is already handled → quiesce (`None`). Otherwise `q` is pending.
pub fn next_pending(events: &[Event]) -> Option<QuarkId> {
    let idx = events
        .iter()
        .rposition(|e| e.to.is_some() && is_turn_request(e))?;
    let target = events[idx].to.clone().unwrap();
    let answered = events[idx + 1..].iter().any(|e| is_turn_completion(e, &target));
    if answered {
        None
    } else {
        Some(target)
    }
}

/// Does event `e` **request or resume** a turn from the quark it addresses (`to =
/// Some(q)`)? A `Message` or `Assign` starts a new turn; a `PermissionGrant` resumes a
/// paused one (the engine re-dispatches on `to == quark`).
///
/// The SSOT for "an addressed event that wants a turn", the counterpart to
/// [`is_turn_completion`]. Control/config events that ALSO carry a `to` — `Reboot`
/// (force-restart), `ModeSet`/`ModeClear` (per-quark posture) — are deliberately
/// excluded: they address a quark to reconfigure it, not to make it work. Counting one
/// as pending gave the quark a spurious empty turn — most visibly on every `/clear`,
/// which appends one `Reboot` per resident quark, so the last-addressed reboot read as
/// an unanswered turn request and excited that quark.
pub fn is_turn_request(e: &Event) -> bool {
    matches!(
        e.kind,
        Kind::Message { .. } | Kind::Assign { .. } | Kind::PermissionGrant { .. }
    )
}

/// Does event `e` represent `quark` **completing** a turn — a reply (a `Message`) or a
/// terminal/pause status (`Ground`, `Error`, `Blocked`, `Waiting`)?
///
/// This is the single source of truth for "the quark did something that ends a turn",
/// shared by [`next_pending`] and the engine's `has_answered` so the two can never drift
/// into disagreeing about whether a quark is still pending. It deliberately EXCLUDES the
/// non-terminal `Excited`/`Thinking` statuses: those mean "started"/"working", not "done".
/// Counting an `Excited` as an answer is exactly what stranded a quark whose turn was
/// interrupted (a restart) after it went Excited but before it replied — the field kept it
/// marked answered and it was never re-dispatched.
pub fn is_turn_completion(e: &Event, quark: &QuarkId) -> bool {
    e.from == Actor::Quark(quark.clone())
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

/// Does `text` start (case-insensitively) with `target_name`, at a mention-safe word
/// boundary? The single boundary rule every mention matcher in this file shares —
/// `try_match` below and [`task_names_card_specifically`] both ride it, so a
/// multi-byte-char panic fixed once here can never resurface in a second hand-rolled
/// matcher.
///
/// `get(..n)` is `None` when `n` is past the end OR not a char boundary. The boundary
/// case is the one a raw `&text[..n]` slice panicked on: a multi-byte char (e.g. a
/// smart quote) straddling the candidate's byte length. Skipping is correct — a
/// prefix cut mid-char could never equal an ASCII target name. The character
/// immediately following the match must NOT be a valid intra-word mention character
/// (alphanumeric, '-', '_'), so `@Google` doesn't match `@GoogleBot`.
fn boundary_match(text: &str, target_name: &str) -> bool {
    let Some(prefix) = text.get(..target_name.len()) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case(target_name) {
        return false;
    }
    let next_char = text[target_name.len()..].chars().next();
    !matches!(next_char, Some(c) if c.is_alphanumeric() || c == '-' || c == '_')
}

/// Tries to find the longest target that matches the START of `text` (case-insensitively).
/// To prevent partial word matches (e.g. `@Google` matching `@GoogleBot`),
/// the character immediately following the match in `text` must NOT be a valid
/// intra-word mention character (alphanumeric, '-', '_'). Note that spaces ARE
/// allowed inside display names, but a matched name's boundary still applies.
fn match_longest_mention<'a>(text: &str, roster: &'a [QuarkCard]) -> Option<(usize, ResolvedMention<'a>)> {
    // A free function rather than a capturing closure: `best_match` is passed in
    // explicitly so the borrow ends when each call returns, letting the id/alias
    // pass and the role pass below run as two separate, sequential loops over
    // `best_match` instead of one closure holding it mutably borrowed for the
    // whole function (which would make `best_match.is_none()` unborrowable).
    fn try_match<'a>(
        text: &str,
        target_name: &str,
        resolution: ResolvedMention<'a>,
        best_match: &mut Option<(usize, ResolvedMention<'a>)>,
    ) {
        if boundary_match(text, target_name) {
            let len = target_name.len();
            if best_match.as_ref().map_or(true, |(best_len, _)| len > *best_len) {
                *best_match = Some((len, resolution));
            }
        }
    }

    let mut best_match: Option<(usize, ResolvedMention<'a>)> = None;

    try_match(text, TEAM_ALIAS, ResolvedMention::Team, &mut best_match);

    if let Some(orch) = roster.iter().find(|c| c.flavor == Flavor::Orchestrator) {
        try_match(text, ORCHESTRATOR_ALIAS, ResolvedMention::Quark(orch), &mut best_match);
    }

    for card in roster {
        try_match(text, card.id.as_str(), ResolvedMention::Quark(card), &mut best_match);
        if let Some(dn) = &card.display_name {
            try_match(text, dn.as_str(), ResolvedMention::Quark(card), &mut best_match);
        }
    }

    // Role resolution (Phase 1, soft): only attempted when NO id/alias/display-name
    // matched at all — a separate pass, not folded into the loop above, so id/alias
    // precedence cannot be lost to roster ordering. If it were one loop, a roster
    // with the role-carrying card before the id-carrying card would register the
    // role first and let it win the longest-match tie, silently inverting the
    // "id beats role" rule the spec requires.
    //
    // Filtered to non-depleted cards: this is a *selection* among candidates (like
    // the peer list at `engine.rs`'s `EnergyState::Depleted` filter), unlike an
    // explicit `@id`/alias mention, which resolves even a depleted/disabled seat on
    // purpose (`Engine::set_enabled`'s doc comment) so the engine can report "that
    // quark is disabled" instead of silently answering as someone else.
    if best_match.is_none() {
        for card in roster {
            if card.energy == EnergyState::Depleted {
                continue;
            }
            for role in &card.roles {
                if role.is_empty() {
                    continue;
                }
                try_match(text, role.as_str(), ResolvedMention::Quark(card), &mut best_match);
            }
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

/// Whether `task` specifically names `card` — by its own `@id` (case-insensitive) or
/// by a `@role` it carries — using the same char-boundary-safe mention scan
/// [`human_mentions`] uses ([`boundary_match`], inherited, not re-implemented).
///
/// This is the eligibility test an `exclusive` card must pass (WS4 §4 Phase 2), and
/// it is deliberately narrower than `human_mentions`: a `@team` broadcast expands to
/// EVERY card there, which would make "addressed to everyone" indistinguishable from
/// "addressed to this card specifically" — exactly the gap that let an exclusive
/// `security` card slip into a plain `@team status` turn. Here `@team` and
/// `@orchestrator` are never treated as naming `card` (no expansion, no alias
/// resolution) — only a literal `@<id>` or `@<role>` counts. `"@team we have a
/// @security incident"` still admits the `security` card, because the `@security`
/// token — not the `@team` one — matches its role.
///
/// Also deliberately skips `match_longest_mention`'s full-roster tie-break: the
/// question here is "does the task match THIS card's own id/role," not "did the
/// router's `@role` resolution land on this exact card." Two cards can share a role,
/// and a task naming that role is a role-matching task for BOTH of them even though
/// `@role` mention-routing only ever picks one (roster-order tie-break).
pub fn task_names_card_specifically(task: &str, card: &QuarkCard) -> bool {
    let mut i = 0;
    while let Some(at_idx) = task[i..].find('@') {
        let actual_at = i + at_idx;
        let valid_start = actual_at == 0 || task.as_bytes()[actual_at - 1].is_ascii_whitespace();
        if valid_start {
            let rest = &task[actual_at + 1..];
            if boundary_match(rest, card.id.as_str())
                || card.roles.iter().any(|role| !role.is_empty() && boundary_match(rest, role))
            {
                return true;
            }
        }
        i = actual_at + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{EnergyState, Flavor, Kind, Mode};

    fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
        Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.into() })
    }

    fn roster() -> Vec<QuarkCard> {
        vec![
            QuarkCard { id: QuarkId::new("orch"), display_name: None, flavor: Flavor::Orchestrator, energy: EnergyState::Available, provider: String::new(), model: String::new(), roles: vec![], exclusive: false },
            QuarkCard { id: QuarkId::new("worker"), display_name: None, flavor: Flavor::Worker, energy: EnergyState::Available, provider: String::new(), model: String::new(), roles: vec![], exclusive: false },
        ]
    }

    /// A worker seat carrying `roles`, for the `@role` routing tests below.
    fn card(id: &str, roles: &[&str]) -> QuarkCard {
        QuarkCard {
            id: QuarkId::new(id),
            display_name: None,
            flavor: Flavor::Worker,
            energy: EnergyState::Available,
            provider: String::new(),
            model: String::new(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            exclusive: false,
        }
    }

    /// A message whose bytes place a multi-byte char (e.g. the smart apostrophe
    /// '’', 3 bytes) across a candidate mention's byte length must not panic. The
    /// 12-byte `@orchestrator` alias is the real-world trigger: any human line
    /// `@…’…` with '’' straddling byte 12 hit `&text[..12]` at a non-char-boundary
    /// and brought the whole daemon down (router.rs:78).
    #[test]
    fn multibyte_char_straddling_a_candidate_length_does_not_panic() {
        // rest after '@' = "abcdefghij’klmn": '’' occupies bytes 10..13, so byte 12
        // (the "orchestrator" alias length) lands INSIDE it.
        let body = "@abcdefghij’klmn and that’s all";
        assert_eq!(human_mentions(body, &roster()), Vec::<QuarkId>::new());
        assert_eq!(parse_addressee(body, &roster(), None), None);
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

    /// A `Kind::Reboot` addresses a quark only to force-restart it — it is NOT a turn
    /// request. `/clear` appends one reboot per resident quark, so a reboot that counted
    /// as pending would hand the last-addressed quark a spurious empty turn (the
    /// "`/clear` triggers codex" bug).
    #[test]
    fn a_reboot_is_not_a_pending_turn() {
        // Post-`/clear`: the field holds only reboots, one per resident quark.
        let post_clear = vec![
            Event::new(Actor::Human, Some(QuarkId::new("orch")), Kind::Reboot),
            Event::new(Actor::Human, Some(QuarkId::new("worker")), Kind::Reboot),
        ];
        assert_eq!(next_pending(&post_clear), None);

        // A reboot after an already-answered message must not re-excite the quark.
        let answered_then_rebooted = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), None, "done"),
            Event::new(Actor::Human, Some(QuarkId::new("orch")), Kind::Reboot),
        ];
        assert_eq!(next_pending(&answered_then_rebooted), None);
    }

    /// A per-quark `ModeSet` also carries a `to`, but changing a quark's permission
    /// posture must never start a turn.
    #[test]
    fn a_per_quark_mode_change_is_not_a_pending_turn() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), None, "done"),
            Event::new(
                Actor::Human,
                Some(QuarkId::new("orch")),
                Kind::ModeSet { mode: Mode::Bypass },
            ),
        ];
        assert_eq!(next_pending(&events), None);
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

    /// Phase 1 soft `@role` routing: a token that is neither a quark id nor a
    /// reserved alias resolves to the (enabled) card whose `roles` carries it.
    #[test]
    fn role_mention_resolves_to_the_role_holder() {
        let r = vec![card("qa1", &["architect"]), card("worker", &[])];
        assert_eq!(human_mentions("@architect do X", &r), vec![QuarkId::new("qa1")]);
    }

    /// A role that matches no seat falls through to the existing no-match
    /// behaviour (empty result) — never a panic or hard error.
    #[test]
    fn role_falls_back_softly_when_no_seat_has_it() {
        let r = vec![card("qa1", &["architect"])];
        assert_eq!(human_mentions("@nobody do X", &r), Vec::<QuarkId>::new());
        assert_eq!(parse_addressee("@nobody do X", &r, None), None);
    }

    /// A card whose ID equals the token wins over a different card whose ROLE
    /// equals the same token — id precedence over role. The role-carrying card
    /// is placed FIRST in roster order so the test actually exercises the
    /// precedence logic rather than passing by roster-order coincidence (a
    /// naive "match roles inline with ids" implementation would register the
    /// role first and let it win the length tie).
    #[test]
    fn id_precedence_over_role() {
        let r = vec![card("has_role", &["architect"]), card("architect", &[])];
        assert_eq!(human_mentions("@architect go", &r), vec![QuarkId::new("architect")]);
    }

    /// `@team`/`@orchestrator` are reserved aliases and must resolve before a
    /// same-named role is even considered.
    #[test]
    fn team_and_orchestrator_alias_beat_a_same_named_role() {
        let mut r = roster(); // has "orch" as Flavor::Orchestrator
        r.push(card("team_role_holder", &["team"]));
        r.push(card("orch_role_holder", &["orchestrator"]));
        assert_eq!(human_mentions("@team status", &r), vec![
            QuarkId::new("orch"),
            QuarkId::new("worker"),
            QuarkId::new("team_role_holder"),
            QuarkId::new("orch_role_holder"),
        ]);
        assert_eq!(
            parse_addressee("@orchestrator your call", &r, Some(&QuarkId::new("worker"))),
            Some(QuarkId::new("orch"))
        );
    }

    /// Role matching is case-insensitive, same as id/alias matching.
    #[test]
    fn role_match_is_case_insensitive() {
        let r = vec![card("qa1", &["architect"])];
        assert_eq!(human_mentions("@Architect do X", &r), vec![QuarkId::new("qa1")]);
    }

    /// Two cards share a role; the first in roster order wins (deterministic
    /// tie-break — a tuning point for later, not least-busy yet).
    #[test]
    fn role_tiebreak_is_roster_order() {
        let r = vec![card("second", &["architect"]), card("first", &["architect"])];
        assert_eq!(human_mentions("@architect go", &r), vec![QuarkId::new("second")]);
    }

    /// A depleted seat is skipped in the role SELECTION pass — this is picking
    /// among candidates (like the peer list `engine.rs` builds for skill
    /// delegation), not resolving an explicit id, so it follows that filter's
    /// convention rather than the id/alias path's "resolve even a disabled
    /// seat" convention.
    #[test]
    fn depleted_seat_is_skipped_by_role_selection() {
        let mut depleted = card("tired", &["architect"]);
        depleted.energy = EnergyState::Depleted;
        let r = vec![depleted, card("fresh", &["architect"])];
        assert_eq!(human_mentions("@architect go", &r), vec![QuarkId::new("fresh")]);
    }

    // ---- task_names_card_specifically (WS4 §4 Phase 2 exclusivity eligibility) --

    /// The gap this function exists to close: `human_mentions` expands `@team` to
    /// the whole roster, so a plain broadcast must NOT read as "named this card
    /// specifically" — no role, no id, just everyone.
    #[test]
    fn team_broadcast_does_not_name_a_specific_card() {
        let sec = card("sec", &["security"]);
        assert!(!task_names_card_specifically("@team status check", &sec));
    }

    /// A `@team` broadcast that ALSO names the card's role still counts — the
    /// exclusion above is about `@team` itself conferring no naming power, not about
    /// broadcasts being disqualified wholesale.
    #[test]
    fn team_broadcast_naming_the_role_still_names_the_card() {
        let sec = card("sec", &["security"]);
        assert!(task_names_card_specifically("@team we have a @security incident", &sec));
    }

    /// An explicit `@id` names the card, independent of any role.
    #[test]
    fn explicit_id_names_the_card() {
        let sec = card("sec", &["security"]);
        assert!(task_names_card_specifically("please loop in @sec on this", &sec));
    }

    /// A task naming neither the card's id nor any of its roles does not name it.
    #[test]
    fn unrelated_task_does_not_name_the_card() {
        let sec = card("sec", &["security"]);
        assert!(!task_names_card_specifically("please fix the css typo", &sec));
    }

    /// Two cards share a role: a task naming that role names BOTH of them, even
    /// though `@role` mention-routing (`human_mentions`/`match_longest_mention`)
    /// only ever picks one via roster-order tie-break. Eligibility is "is this a
    /// role-matching task for this card", not "did routing land on this card".
    #[test]
    fn a_shared_role_names_every_card_that_carries_it() {
        let first = card("first", &["architect"]);
        let second = card("second", &["architect"]);
        assert!(task_names_card_specifically("@architect go", &first));
        assert!(task_names_card_specifically("@architect go", &second));
    }

    /// `@orchestrator` is a reserved alias, never a card's own id or role — it must
    /// not be read as naming an unrelated card.
    #[test]
    fn orchestrator_alias_does_not_name_an_unrelated_card() {
        let sec = card("sec", &["security"]);
        assert!(!task_names_card_specifically("@orchestrator please advise", &sec));
    }

    /// Inherits `boundary_match`'s char-boundary safety: a multi-byte char (the
    /// smart apostrophe '’', 3 bytes) straddling a candidate's byte length must not
    /// panic — the same real-world trigger as
    /// `multibyte_char_straddling_a_candidate_length_does_not_panic` above, now
    /// exercised through the new function instead of a second hand-rolled matcher.
    #[test]
    fn multibyte_char_straddling_a_candidate_length_does_not_panic_here_either() {
        let sec = card("sec", &["security"]);
        let task = "@abcdefghij’klmn and that’s all";
        assert!(!task_names_card_specifically(task, &sec));
    }
}
