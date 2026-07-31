use super::*;
use hadron_lattice::{EnergyState, Flavor, Kind, Mode};

fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
    Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.into() })
}

fn roster() -> Vec<QuarkCard> {
    vec![
        QuarkCard { id: QuarkId::new("orch"), display_name: None, flavor: Flavor::Orchestrator, energy: EnergyState::Available, provider: String::new(), model: String::new(), roles: vec![], exclusive: false, commands: Default::default(), energy_limit: None, deny_skills: vec![], has_forge_tools: false },
        QuarkCard { id: QuarkId::new("worker"), display_name: None, flavor: Flavor::Worker, energy: EnergyState::Available, provider: String::new(), model: String::new(), roles: vec![], exclusive: false, commands: Default::default(), energy_limit: None, deny_skills: vec![], has_forge_tools: false },
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
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
        has_forge_tools: false,
    }
}

/// A preon named `name` preferring `role`, body empty (the router pass
/// under test never reads the body — that's a separate, deferred concern,
/// see the module doc). Mirrors `card()` for the preon-routing tests below.
fn preon(name: &str, role: &str) -> Preon {
    Preon { name: name.to_string(), preferred_role: Some(role.to_string()), body: String::new() }
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
    assert_eq!(human_mentions(body, &roster(), &[]), Vec::<QuarkId>::new());
    assert_eq!(parse_addressee(body, &roster(), None, &[]), None);
}

#[test]
fn fenced_code_blocks_and_midsentence_do_not_route() {
    let r = roster();
    assert_eq!(parse_addressee("@worker fix the router", &r, None, &[]), Some(QuarkId::new("worker")));
    assert_eq!(parse_addressee("contact @worker for info", &r, None, &[]), None);
    assert_eq!(parse_addressee("```\n@worker fix the router\n```", &r, None, &[]), None);
}

/// The escalation path: a worker addresses the ROLE, and it lands on whoever
/// holds it — no worker ever hardcodes an orchestrator's id.
#[test]
fn orchestrator_alias_resolves_to_the_role_holder() {
    let worker = QuarkId::new("worker");
    assert_eq!(
        parse_addressee("@orchestrator which schema should I use?", &roster(), Some(&worker), &[]),
        Some(QuarkId::new("orch"))
    );
    // A human can address the role too, alongside plain id mentions.
    assert_eq!(human_mentions("@orchestrator take this", &roster(), &[]), vec![QuarkId::new("orch")]);
    // Id mentions keep working — the alias is an addition, not a replacement.
    assert_eq!(
        human_mentions("@orch do X and @worker do Y", &roster(), &[]),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
}

/// `@team` rallies the whole roster: the human addresses everyone once and the
/// daemon fans the turn out to each in sequence (the status-check case).
#[test]
fn team_alias_addresses_the_whole_roster() {
    assert_eq!(
        human_mentions("@team report progress please", &roster(), &[]),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
    // Mixing `@team` with an id names each quark once, not twice.
    assert_eq!(
        human_mentions("@team status, @worker especially you", &roster(), &[]),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
}

/// A quark broadcasting to `@team` would excite every other quark, each of whom
/// could broadcast back — an amplification loop. The quark→quark path does not
/// resolve the alias at all: a quark must name who it wants.
#[test]
fn a_quark_cannot_broadcast_to_the_team() {
    let worker = QuarkId::new("worker");
    assert_eq!(parse_addressee("@team status?", &roster(), Some(&worker), &[]), None);
}

/// Re-flavouring the team retargets the alias with no code or prompt change —
/// the whole point of routing by role instead of by id.
#[test]
fn orchestrator_alias_follows_the_role_across_a_reflavour() {
    let mut reflavoured = roster();
    reflavoured[0].flavor = Flavor::Worker; // orch demoted…
    reflavoured[1].flavor = Flavor::Orchestrator; // …worker promoted
    assert_eq!(
        parse_addressee("@orchestrator your call", &reflavoured, Some(&QuarkId::new("orch")), &[]),
        Some(QuarkId::new("worker"))
    );
}

/// Jake types `@Opus` as often as `@opus`; a mention that misses on case would
/// silently route nowhere rather than fail loudly, so pin both ids and aliases.
#[test]
fn mentions_resolve_regardless_of_case() {
    let r = roster();
    assert_eq!(
        parse_addressee("@Worker take this", &r, Some(&QuarkId::new("orch")), &[]),
        Some(QuarkId::new("worker"))
    );
    assert_eq!(
        parse_addressee("@ORCHESTRATOR your call", &r, Some(&QuarkId::new("worker")), &[]),
        Some(QuarkId::new("orch"))
    );
    assert_eq!(
        human_mentions("@Team report progress", &r, &[]),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
}

/// The orchestrator writing `@orchestrator` addresses itself — sender-exclusion
/// makes that a no-op, so the reply falls through to "no addressee" and control
/// returns to the human instead of the orchestrator exciting itself forever.
#[test]
fn orchestrator_cannot_escalate_to_itself() {
    let orch = QuarkId::new("orch");
    assert_eq!(parse_addressee("@orchestrator hmm", &roster(), Some(&orch), &[]), None);
}

/// With nobody holding the role, the alias resolves to nobody rather than
/// guessing a target.
#[test]
fn orchestrator_alias_resolves_to_nobody_on_an_orchestrator_less_roster() {
    let workers_only: Vec<QuarkCard> =
        roster().into_iter().map(|mut c| { c.flavor = Flavor::Worker; c }).collect();
    assert_eq!(parse_addressee("@orchestrator anyone?", &workers_only, None, &[]), None);
    assert_eq!(human_mentions("@orchestrator anyone?", &workers_only, &[]), Vec::<QuarkId>::new());
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

/// **The masked-orchestrator bug, replayed from `.hadron/field.jsonl`.**
///
/// A worker reports up (`Message → orch`), and the merge gate then appends its own
/// audit `PermissionReq` + `PermissionGrant{→worker}` before landing the branch. The
/// grant is ADDRESSED and `is_turn_request` counts it, so it became the newest
/// addressed request — and the worker's own `Ground` immediately answered it. The old
/// single-`rposition` scan therefore stopped at the grant, saw it answered, and
/// returned `None`: the orchestrator's still-unanswered message was never even looked
/// at. Live symptom — "Sonnet reported to @orchestrator and Claude never got excited";
/// Claude only woke when Jake typed again (that path is `unaddressed_message_targets`,
/// which is unaffected).
#[test]
fn an_answered_merge_grant_does_not_mask_an_older_unanswered_handoff() {
    let worker = QuarkId::new("worker");
    let events = vec![
        msg(Actor::Quark(worker.clone()), Some("orch"), "@orchestrator task complete"),
        Event::new(
            Actor::Quark(worker.clone()),
            None,
            Kind::PermissionReq {
                risk: hadron_gatekeeper::Risk::BashExec,
                description: "merge".into(),
            },
        ),
        Event::new(
            Actor::Gluon,
            Some(worker.clone()),
            Kind::PermissionGrant { approved: true, remember: false },
        ),
        msg(Actor::Gluon, None, "merged `quark/worker/01ABC` → `main`."),
        Event::new(Actor::Quark(worker), None, Kind::Status { state: QuarkState::Ground }),
    ];
    assert_eq!(next_pending(&events), Some(QuarkId::new("orch")));
}

/// The walk past an answered request must not resurrect an already-served one:
/// two addressed requests, both answered, is still a quiesce.
#[test]
fn walking_past_an_answered_request_still_quiesces_when_all_are_answered() {
    let events = vec![
        msg(Actor::Human, Some("orch"), "go"),
        msg(Actor::Quark(QuarkId::new("orch")), None, "done"),
        msg(Actor::Human, Some("worker"), "you too"),
        msg(Actor::Quark(QuarkId::new("worker")), None, "also done"),
    ];
    assert_eq!(next_pending(&events), None);
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
        parse_addressee("@worker please handle it.", &roster(), None, &[]),
        Some(QuarkId::new("worker"))
    );
    assert_eq!(
        parse_addressee("Here's the plan.\n@worker execute it.", &roster(), None, &[]),
        Some(QuarkId::new("worker"))
    );
    assert_eq!(
        parse_addressee("**@worker** please handle it.", &roster(), None, &[]),
        Some(QuarkId::new("worker"))
    );
    assert_eq!(
        human_mentions("**@worker** take over", &roster(), &[]),
        vec![QuarkId::new("worker")]
    );
    assert_eq!(parse_addressee("no mention here", &roster(), None, &[]), None);
    assert_eq!(parse_addressee("@ghost unknown", &roster(), None, &[]), None);
}

#[test]
fn parse_addressee_ignores_mid_line_and_quoted_mentions() {
    // The regression: a mention buried in prose does NOT route — this is the
    // bug where a quark listing the conversation quoted "@agy"/"@worker" and
    // its reply got mis-routed there, spuriously exciting that quark.
    assert_eq!(
        parse_addressee("Sure, @worker please handle it.", &roster(), None, &[]),
        None,
        "mid-line mention must not route"
    );
    let quoted = "I can see these messages:\n1. human: @worker do X\n2. @orch replied\nThat's all.";
    assert_eq!(
        parse_addressee(quoted, &roster(), Some(&QuarkId::new("orch")), &[]),
        None,
        "quoted mentions inside a numbered list must not route"
    );
}

#[test]
fn parse_addressee_ignores_sender() {
    // A quark starting a line with its OWN handle must not self-address.
    let worker = QuarkId::new("worker");
    assert_eq!(parse_addressee("@worker I'm on it", &roster(), Some(&worker), &[]), None);
    // A line-starting mention of a DIFFERENT quark still routes.
    assert_eq!(
        parse_addressee("@worker take over", &roster(), Some(&QuarkId::new("orch")), &[]),
        Some(QuarkId::new("worker"))
    );
}

#[test]
fn human_mentions_finds_every_mention_anywhere_deduped_in_order() {
    // A human addresses whoever they name, mid-sentence and in any order —
    // this is the multi-dispatch case: "@orch do X and you @worker do Y".
    assert_eq!(
        human_mentions("@orch please proceed and you @worker start task 3", &roster(), &[]),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
    // Order follows first appearance; duplicates collapse.
    assert_eq!(
        human_mentions("@worker and @orch, then @worker again", &roster(), &[]),
        vec![QuarkId::new("worker"), QuarkId::new("orch")]
    );
    // Punctuation ends a handle; unknown ids and bare '@' are ignored.
    assert_eq!(human_mentions("hey @orch, thanks!", &roster(), &[]), vec![QuarkId::new("orch")]);
    assert_eq!(human_mentions("@ghost @nobody nothing here", &roster(), &[]), Vec::<QuarkId>::new());
    // An '@' not starting a word (an email) is not a mention.
    assert_eq!(human_mentions("mail me at jake@orch.dev", &roster(), &[]), Vec::<QuarkId>::new());
}

/// Phase 1 soft `@role` routing: a token that is neither a quark id nor a
/// reserved alias resolves to the (enabled) card whose `roles` carries it.
#[test]
fn role_mention_resolves_to_the_role_holder() {
    let r = vec![card("qa1", &["architect"]), card("worker", &[])];
    assert_eq!(human_mentions("@architect do X", &r, &[]), vec![QuarkId::new("qa1")]);
}

/// A role that matches no seat falls through to the existing no-match
/// behaviour (empty result) — never a panic or hard error.
#[test]
fn role_falls_back_softly_when_no_seat_has_it() {
    let r = vec![card("qa1", &["architect"])];
    assert_eq!(human_mentions("@nobody do X", &r, &[]), Vec::<QuarkId>::new());
    assert_eq!(parse_addressee("@nobody do X", &r, None, &[]), None);
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
    assert_eq!(human_mentions("@architect go", &r, &[]), vec![QuarkId::new("architect")]);
}

/// `@team`/`@orchestrator` are reserved aliases and must resolve before a
/// same-named role is even considered.
#[test]
fn team_and_orchestrator_alias_beat_a_same_named_role() {
    let mut r = roster(); // has "orch" as Flavor::Orchestrator
    r.push(card("team_role_holder", &["team"]));
    r.push(card("orch_role_holder", &["orchestrator"]));
    assert_eq!(human_mentions("@team status", &r, &[]), vec![
        QuarkId::new("orch"),
        QuarkId::new("worker"),
        QuarkId::new("team_role_holder"),
        QuarkId::new("orch_role_holder"),
    ]);
    assert_eq!(
        parse_addressee("@orchestrator your call", &r, Some(&QuarkId::new("worker")), &[]),
        Some(QuarkId::new("orch"))
    );
}

/// Role matching is case-insensitive, same as id/alias matching.
#[test]
fn role_match_is_case_insensitive() {
    let r = vec![card("qa1", &["architect"])];
    assert_eq!(human_mentions("@Architect do X", &r, &[]), vec![QuarkId::new("qa1")]);
}

/// Two cards share a role; the first in roster order wins (deterministic
/// tie-break — a tuning point for later, not least-busy yet).
#[test]
fn role_tiebreak_is_roster_order() {
    let r = vec![card("second", &["architect"]), card("first", &["architect"])];
    assert_eq!(human_mentions("@architect go", &r, &[]), vec![QuarkId::new("second")]);
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
    assert_eq!(human_mentions("@architect go", &r, &[]), vec![QuarkId::new("fresh")]);
}

// ---- task_names_card_specifically (WS4 §4 Phase 2 exclusivity eligibility) --

/// The gap this function exists to close: `human_mentions` expands `@team` to
/// the whole roster, so a plain broadcast must NOT read as "named this card
/// specifically" — no role, no id, just everyone.
#[test]
fn team_broadcast_does_not_name_a_specific_card() {
    let sec = card("sec", &["security"]);
    assert!(!task_names_card_specifically("@team status check", &sec, &[]));
}

/// A `@team` broadcast that ALSO names the card's role still counts — the
/// exclusion above is about `@team` itself conferring no naming power, not about
/// broadcasts being disqualified wholesale.
#[test]
fn team_broadcast_naming_the_role_still_names_the_card() {
    let sec = card("sec", &["security"]);
    assert!(task_names_card_specifically("@team we have a @security incident", &sec, &[]));
}

/// An explicit `@id` names the card, independent of any role.
#[test]
fn explicit_id_names_the_card() {
    let sec = card("sec", &["security"]);
    assert!(task_names_card_specifically("please loop in @sec on this", &sec, &[]));
}

/// A task naming neither the card's id nor any of its roles does not name it.
#[test]
fn unrelated_task_does_not_name_the_card() {
    let sec = card("sec", &["security"]);
    assert!(!task_names_card_specifically("please fix the css typo", &sec, &[]));
}

/// Two cards share a role: a task naming that role names BOTH of them, even
/// though `@role` mention-routing (`human_mentions`/`match_longest_mention`)
/// only ever picks one via roster-order tie-break. Eligibility is "is this a
/// role-matching task for this card", not "did routing land on this card".
#[test]
fn a_shared_role_names_every_card_that_carries_it() {
    let first = card("first", &["architect"]);
    let second = card("second", &["architect"]);
    assert!(task_names_card_specifically("@architect go", &first, &[]));
    assert!(task_names_card_specifically("@architect go", &second, &[]));
}

/// `@orchestrator` is a reserved alias, never a card's own id or role — it must
/// not be read as naming an unrelated card.
#[test]
fn orchestrator_alias_does_not_name_an_unrelated_card() {
    let sec = card("sec", &["security"]);
    assert!(!task_names_card_specifically("@orchestrator please advise", &sec, &[]));
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
    assert!(!task_names_card_specifically(task, &sec, &[]));
}

/// **Whole-branch review follow-up.** `match_longest_mention` resolves
/// `@DisplayName` to a card independent of `id`/`roles` — this function must
/// agree, or a card the router happily dispatches by display name gets rejected
/// here as "not addressed by role or @id".
#[test]
fn display_name_names_the_card() {
    let mut claude = card("acp-claude", &["security"]);
    claude.display_name = Some("Claude".to_string());
    assert!(task_names_card_specifically("@Claude handle this", &claude, &[]));
    // Case-insensitive, same as id/role matching.
    assert!(task_names_card_specifically("@claude handle this", &claude, &[]));
}

/// A display name is still just a specific-card handle, not a broadcast: `@team`
/// must not name the card even when it carries a display name.
#[test]
fn team_broadcast_does_not_name_a_card_via_its_display_name() {
    let mut claude = card("acp-claude", &["security"]);
    claude.display_name = Some("Claude".to_string());
    assert!(!task_names_card_specifically("@team status check", &claude, &[]));
}

// ---- preon-name routing (spec §4: `@preon-name` routes via preferred_role) --

/// The core case: `@security-reviewer` is neither a card id, an alias, nor a
/// role — it's a preon whose `preferred_role` is `security`, and that role
/// is carried by `sec`. The preon pass resolves the name to that seat.
#[test]
fn preon_routes_to_a_seat_with_its_preferred_role() {
    let r = vec![card("sec", &["security"])];
    let p = vec![preon("security-reviewer", "security")];
    assert_eq!(human_mentions("@security-reviewer go", &r, &p), vec![QuarkId::new("sec")]);
}

/// A preon whose `preferred_role` holds no seat on the roster falls through
/// softly — empty result, no panic — exactly like an unmatched `@role`.
#[test]
fn preon_with_no_matching_seat_falls_back_softly() {
    let r = vec![card("sec", &["security"])];
    let p = vec![preon("security-reviewer", "nobody-has-this-role")];
    assert_eq!(human_mentions("@security-reviewer go", &r, &p), Vec::<QuarkId>::new());
    assert_eq!(parse_addressee("@security-reviewer go", &r, None, &p), None);
}

/// A card id, or a reserved alias, named the same as a preon wins — id and
/// alias precedence over a preon name, same as over a role.
#[test]
fn id_and_alias_beat_a_preon_name() {
    // A card whose id IS the preon's name wins over the preon resolving to
    // a DIFFERENT seat via its preferred_role.
    let r = vec![card("security-reviewer", &[]), card("sec", &["security"])];
    let p = vec![preon("security-reviewer", "security")];
    assert_eq!(human_mentions("@security-reviewer go", &r, &p), vec![QuarkId::new("security-reviewer")]);

    // `@team`, named the same as a preon, still broadcasts rather than
    // resolving through the preon's preferred_role.
    let p2 = vec![preon("team", "worker")];
    assert_eq!(
        human_mentions("@team go", &roster(), &p2),
        vec![QuarkId::new("orch"), QuarkId::new("worker")]
    );
}

/// Preon-name matching is case-insensitive, same as id/alias/role matching.
#[test]
fn preon_match_is_case_insensitive() {
    let r = vec![card("sec", &["security"])];
    let p = vec![preon("security-reviewer", "security")];
    assert_eq!(human_mentions("@Security-Reviewer go", &r, &p), vec![QuarkId::new("sec")]);
}

/// The back-compat pin: an EMPTY preons slice must route byte-for-byte the
/// way WS4§4 did before this pass existed, across id, alias, role, and the
/// soft-fallback cases — the preon pass runs, finds nothing, and does not
/// change a single outcome.
#[test]
fn no_preons_is_todays_routing() {
    let r = vec![card("has_role", &["architect"]), card("architect", &[])];
    assert_eq!(human_mentions("@architect go", &r, &[]), vec![QuarkId::new("architect")]);
    assert_eq!(human_mentions("@team status", &roster(), &[]), vec![QuarkId::new("orch"), QuarkId::new("worker")]);
    assert_eq!(
        parse_addressee("@orchestrator your call", &roster(), Some(&QuarkId::new("worker")), &[]),
        Some(QuarkId::new("orch"))
    );
    assert_eq!(human_mentions("@ghost @nobody nothing here", &roster(), &[]), Vec::<QuarkId>::new());
}

/// A task naming a preon is treated as naming every card that carries the
/// preon's `preferred_role` — the same "a shared role names every card that
/// carries it" rule `a_shared_role_names_every_card_that_carries_it` pins for
/// `@role`, extended to `@preon-name`.
#[test]
fn preon_name_in_a_task_names_the_role_holder_specifically() {
    let sec = card("sec", &["security"]);
    let p = vec![preon("security-reviewer", "security")];
    assert!(task_names_card_specifically("@security-reviewer please look", &sec, &p));
    assert!(!task_names_card_specifically("please fix the css typo", &sec, &p));
}

// ---- review follow-up: role pass / card_for_role cross-checks -------------

/// Direct cross-check of the drift risk `card_for_role`'s doc comment flags:
/// the role pass (fused text-match loop) and the preon pass (`card_for_role`,
/// a separate direct lookup) must independently agree on which seat a SHARED
/// role resolves to. Two cards carry "security"; `@security` (role pass) and
/// `@security-reviewer` (preon pass, `preferred_role: security`) must land
/// on the exact same first-roster-order card — this is not tautological: the
/// two assertions exercise two genuinely different code paths
/// (`try_match`-fused role loop vs. `card_for_role`'s `.find()`) against the
/// same roster, not the same call twice.
///
/// The depleted variant re-runs both against a roster where the FIRST
/// same-role card is `Depleted`, proving the two paths also agree on
/// *skipping* it and falling through to the second — the other half of the
/// shared "skip-depleted, roster-order-wins" rule.
#[test]
fn preon_and_its_role_resolve_to_the_same_card() {
    let r = vec![card("sec-a", &["security"]), card("sec-b", &["security"])];
    let p = vec![preon("security-reviewer", "security")];

    let via_role = human_mentions("@security go", &r, &p);
    let via_preon = human_mentions("@security-reviewer go", &r, &p);
    assert_eq!(via_role, vec![QuarkId::new("sec-a")], "the role pass must land on the first same-role card");
    assert_eq!(via_preon, vec![QuarkId::new("sec-a")], "the preon pass must land on the SAME card");
    assert_eq!(via_role, via_preon, "role pass and preon pass must never disagree about a shared role");

    // Same roster, first same-role card now Depleted — both paths must skip
    // it identically and resolve to the second.
    let mut first_depleted = r.clone();
    first_depleted[0].energy = EnergyState::Depleted;
    assert_eq!(
        human_mentions("@security go", &first_depleted, &p),
        vec![QuarkId::new("sec-b")],
        "the role pass must skip a depleted seat"
    );
    assert_eq!(
        human_mentions("@security-reviewer go", &first_depleted, &p),
        vec![QuarkId::new("sec-b")],
        "the preon pass must skip a depleted seat the same way"
    );
}

/// A card's OWN role beats a preon named identically: the role pass runs
/// BEFORE the preon pass (`match_longest_mention`'s gate), so `@architect`
/// resolves to whichever seat carries the role `architect`, never through a
/// preon whose `name` happens to also be `architect` — even when that
/// preon's `preferred_role` points somewhere else entirely.
#[test]
fn a_cards_role_beats_a_same_named_preon() {
    let r = vec![card("qa1", &["architect"])];
    // A preon named identically to the role, but preferring a DIFFERENT
    // role held by no one — if the preon pass ever won here, this would
    // resolve to nobody instead of `qa1`.
    let p = vec![preon("architect", "some-other-role-nobody-has")];
    assert_eq!(human_mentions("@architect go", &r, &p), vec![QuarkId::new("qa1")]);

    // A same-length preon name ties with the role text under
    // `try_match`'s strict `len > *best_len` comparison, so the assertion
    // above alone would still pass even if the preon pass's precedence
    // GATE (`if best_match.is_none()`) were deleted — the role pass simply
    // runs first in source order and the tie never overwrites it. This
    // second case is gate-sensitive: a preon name LONGER than the role
    // text is a legal literal match too (spaces are valid inside a mention
    // name, per `match_longest_mention`'s own doc comment), so if the
    // preon pass were ever allowed to run unconditionally, its strictly
    // LONGER match would overwrite the role pass's shorter one under plain
    // longest-match rules. Only the gate stops that.
    let r2 = vec![card("qa1", &["architect"]), card("qa3", &["support"])];
    let p2 = vec![preon("architect team", "support")];
    assert_eq!(
        human_mentions("@architect team please help", &r2, &p2),
        vec![QuarkId::new("qa1")],
        "role precedence must hold even when a preon name is a literally LONGER match"
    );
}


