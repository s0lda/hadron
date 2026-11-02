use super::*;
use hadron_lattice::{EnergyState, Flavor, Kind, Mode};

fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
    Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.into() })
}

fn roster() -> Vec<QuarkCard> {
    vec![
        QuarkCard { id: QuarkId::new("orch"), display_name: None, flavor: Flavor::Orchestrator, energy: EnergyState::Available, provider: String::new(), model: String::new(), commands: Default::default(), energy_limit: None, deny_skills: vec![], has_forge_tools: false },
        QuarkCard { id: QuarkId::new("worker"), display_name: None, flavor: Flavor::Worker, energy: EnergyState::Available, provider: String::new(), model: String::new(), commands: Default::default(), energy_limit: None, deny_skills: vec![], has_forge_tools: false },
    ]
}

fn card(id: &str, _unused_roles: &[&str]) -> QuarkCard {
    QuarkCard {
        id: QuarkId::new(id),
        display_name: None,
        flavor: Flavor::Worker,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
        has_forge_tools: false,
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

/// Users type `@Opus` as often as `@opus`; a mention that misses on case would
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
/// Claude only woke when the human typed again (that path is `unaddressed_message_targets`,
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
    assert_eq!(human_mentions("mail me at user@orch.dev", &roster(), &[]), Vec::<QuarkId>::new());
}

// ---- task_names_card_specifically -------------------------------------------

#[test]
fn team_broadcast_does_not_name_a_specific_card() {
    let sec = card("sec", &[]);
    assert!(!task_names_card_specifically("@team status check", &sec, &[]));
}

#[test]
fn explicit_id_names_the_card() {
    let sec = card("sec", &[]);
    assert!(task_names_card_specifically("please loop in @sec on this", &sec, &[]));
}

#[test]
fn unrelated_task_does_not_name_the_card() {
    let sec = card("sec", &[]);
    assert!(!task_names_card_specifically("please fix the css typo", &sec, &[]));
}

#[test]
fn orchestrator_alias_does_not_name_an_unrelated_card() {
    let sec = card("sec", &[]);
    assert!(!task_names_card_specifically("@orchestrator please advise", &sec, &[]));
}

#[test]
fn multibyte_char_straddling_a_candidate_length_does_not_panic_here_either() {
    let sec = card("sec", &[]);
    let task = "@abcdefghij’klmn and that’s all";
    assert!(!task_names_card_specifically(task, &sec, &[]));
}

#[test]
fn display_name_names_the_card() {
    let mut claude = card("acp-claude", &[]);
    claude.display_name = Some("Claude".to_string());
    assert!(task_names_card_specifically("@Claude handle this", &claude, &[]));
    assert!(task_names_card_specifically("@claude handle this", &claude, &[]));
}

#[test]
fn team_broadcast_does_not_name_a_card_via_its_display_name() {
    let mut claude = card("acp-claude", &[]);
    claude.display_name = Some("Claude".to_string());
    assert!(!task_names_card_specifically("@team status check", &claude, &[]));
}

#[test]
fn code_blocks_and_inline_spans_do_not_excite_quarks() {
    let r = vec![
        card("orch", &[]),
        card("worker", &[]),
        card("ollama", &[]),
        card("claude", &[]),
        card("agy", &[]),
    ];

    // 1. Fenced code block (```) containing mentions must be ignored
    let msg_fenced = "In @README.md please update:\n```md\nCollaborative Multi-LLM Chat: @claude, @ollama, @agy\n```\n@orch please review";
    assert_eq!(human_mentions(msg_fenced, &r, &[]), vec![QuarkId::new("orch")]);

    // 2. Tilde fence (~~~) containing mentions must be ignored
    let msg_tilde = "~~~rust\nlet x = \"@worker\";\n~~~\n@orch go";
    assert_eq!(human_mentions(msg_tilde, &r, &[]), vec![QuarkId::new("orch")]);

    // 3. Inline code backticks (`...`) must be ignored
    let msg_inline = "Mix models like `@claude` and `@ollama` together, @orch please check";
    assert_eq!(human_mentions(msg_inline, &r, &[]), vec![QuarkId::new("orch")]);

    // 4. Double backtick inline span (``...``)
    let msg_double_ticks = "Check ``@worker and @claude`` here and ask @orch";
    assert_eq!(human_mentions(msg_double_ticks, &r, &[]), vec![QuarkId::new("orch")]);

    // 5. Code block only with no outside mentions returns empty
    let msg_only_code = "```\n@worker\n@ollama\n```";
    assert_eq!(human_mentions(msg_only_code, &r, &[]), Vec::<QuarkId>::new());

    // 6. Inline code only returns empty
    let msg_only_inline = "`@worker` and `@ollama`";
    assert_eq!(human_mentions(msg_only_inline, &r, &[]), Vec::<QuarkId>::new());

    // 7. parse_addressee and parse_all_addressees also ignore code blocks & spans
    assert_eq!(parse_addressee(msg_fenced, &r, None, &[]), Some(QuarkId::new("orch")));
    assert_eq!(parse_addressee("`@worker` do this", &r, None, &[]), None);
    assert_eq!(parse_all_addressees(msg_fenced, &r, None, &[]), vec![QuarkId::new("orch")]);

    // 8. task_names_card_specifically ignores code blocks & spans
    let worker_card = &r[1];
    assert!(!task_names_card_specifically("Fix README: ``` @worker ```", worker_card, &[]));
    assert!(!task_names_card_specifically("Fix README: `@worker`", worker_card, &[]));
    assert!(task_names_card_specifically("Assigning to @worker for fix", worker_card, &[]));
}

#[test]
fn an_interrupted_turn_ground_does_not_mask_a_newer_interrupting_request() {
    let worker = QuarkId::new("worker");
    let req1 = msg(Actor::Human, Some("worker"), "task 1");
    let req2 = msg(Actor::Human, Some("worker"), "task 2 interrupting");
    let events = vec![
        req1.clone(),
        req2.clone(),
        Event::new(Actor::Quark(worker.clone()), None, Kind::Status { state: QuarkState::Ground })
            .answering(Some(req1.id)),
    ];
    assert_eq!(next_pending(&events), Some(worker));
}

#[test]
fn gluon_gate_handback_message_is_answered_by_continued_assignment() {
    let worker = QuarkId::new("worker");
    let assignment_id = ulid::Ulid::new();
    let handback = Event::new(
        Actor::Gluon,
        Some(worker.clone()),
        Kind::Message {
            body: "@worker The merge gate stopped this branch from landing...".into(),
        },
    )
    .with_answers(assignment_id);

    // Prior to turn completion, it is pending
    assert_eq!(next_pending(&[handback.clone()]), Some(worker.clone()));

    // After the quark finishes its turn with answers == assignment_id, it is answered
    let completion = Event::new(
        Actor::Quark(worker.clone()),
        None,
        Kind::Status { state: QuarkState::Ground },
    )
    .answering(Some(assignment_id));

    assert_eq!(next_pending(&[handback.clone(), completion]), None);

    // Also answered if the turn completion is a Message answering the assignment
    let completion_msg = Event::new(
        Actor::Quark(worker),
        None,
        Kind::Message { body: "fixed".into() },
    )
    .answering(Some(assignment_id));

    assert_eq!(next_pending(&[handback, completion_msg]), None);
}

#[test]
fn multiple_addressed_steers_quiesce_once_the_newest_is_answered() {
    let worker = QuarkId::new("cli-agy");
    let req0 = msg(Actor::Human, None, "initial task");
    let steer1 = msg(Actor::Human, Some("cli-agy"), "@cli-agy [STEERING GUIDANCE]: first steer");
    let steer2 = msg(Actor::Human, Some("cli-agy"), "@cli-agy [STEERING GUIDANCE]: second steer");
    let ground0 = Event::new(
        Actor::Quark(worker.clone()),
        None,
        Kind::Status { state: QuarkState::Ground },
    )
    .answering(Some(req0.id));

    // Before steer2 is answered, worker is pending
    assert_eq!(
        next_pending(&[req0.clone(), steer1.clone(), steer2.clone(), ground0.clone()]),
        Some(worker.clone())
    );

    // Once worker completes a turn answering steer2, steer1 is superseded and the field quiesces
    let assign2 = Event::new(
        Actor::Gluon,
        Some(worker.clone()),
        Kind::Assign {
            task: "@cli-agy [STEERING GUIDANCE]: second steer".into(),
            invariants: vec![],
        },
    )
    .with_answers(steer2.id);
    let reply2 = Event::new(
        Actor::Quark(worker.clone()),
        None,
        Kind::Message { body: "acknowledged".into() },
    )
    .answering(Some(steer2.id));
    let ground2 = Event::new(
        Actor::Quark(worker),
        None,
        Kind::Status { state: QuarkState::Ground },
    )
    .answering(Some(steer2.id));

    let events = vec![req0, steer1, steer2, ground0, assign2, reply2, ground2];
    assert_eq!(next_pending(&events), None);
}

#[test]
fn orchestrator_parallel_self_dispatch_with_peers_and_self_alias() {
    let r = roster();
    let orch = QuarkId::new("orch");

    // A lone `@orchestrator` from the orchestrator itself is still excluded so it
    // never accidentally loops on a single-target reply.
    assert_eq!(
        parse_all_addressees("@orchestrator thinking aloud", &r, Some(&orch), &[]),
        Vec::<QuarkId>::new()
    );

    // When the orchestrator fans out to a peer worker AND assigns a slice to itself
    // via `@orchestrator` or `@self`, both the worker and the orchestrator are returned.
    assert_eq!(
        parse_all_addressees(
            "@worker implement task 1\n@orchestrator implement task 2",
            &r,
            Some(&orch),
            &[],
        ),
        vec![QuarkId::new("worker"), QuarkId::new("orch")]
    );
    assert_eq!(
        parse_all_addressees(
            "@worker implement task 1\n@self implement task 2",
            &r,
            Some(&orch),
            &[],
        ),
        vec![QuarkId::new("worker"), QuarkId::new("orch")]
    );
    assert_eq!(
        parse_all_addressees("@self continue wave 2", &r, Some(&orch), &[]),
        vec![QuarkId::new("orch")]
    );

    // `neutralize_dispatched_mentions` prefixes `✓ ` only on lines targeting already-dispatched peers.
    let mut dispatched = std::collections::HashSet::new();
    dispatched.insert(QuarkId::new("worker"));
    let neutralized = neutralize_dispatched_mentions(
        "@worker implement task 1\n@self implement task 2",
        &r,
        Some(&orch),
        &[],
        &dispatched,
    );
    assert_eq!(
        neutralized,
        "✓ @worker implement task 1\n@self implement task 2"
    );
    assert_eq!(
        parse_all_addressees(&neutralized, &r, Some(&orch), &[]),
        vec![QuarkId::new("orch")]
    );
}

#[test]
fn test_reserved_and_colliding_display_names_ignored() {
    let r = vec![
        QuarkCard {
            id: QuarkId::new("orch"),
            display_name: None,
            flavor: Flavor::Orchestrator,
            energy: EnergyState::Available,
            provider: String::new(),
            model: String::new(),
            commands: Default::default(),
            energy_limit: None,
            deny_skills: vec![],
            has_forge_tools: false,
        },
        QuarkCard {
            id: QuarkId::new("worker-1"),
            display_name: Some("orchestrator".to_string()), // reserved!
            flavor: Flavor::Worker,
            energy: EnergyState::Available,
            provider: String::new(),
            model: String::new(),
            commands: Default::default(),
            energy_limit: None,
            deny_skills: vec![],
            has_forge_tools: false,
        },
        QuarkCard {
            id: QuarkId::new("worker-2"),
            display_name: Some("worker-1".to_string()), // collides with worker-1 ID!
            flavor: Flavor::Worker,
            energy: EnergyState::Available,
            provider: String::new(),
            model: String::new(),
            commands: Default::default(),
            energy_limit: None,
            deny_skills: vec![],
            has_forge_tools: false,
        },
    ];

    // @orchestrator must resolve to the real orchestrator, NOT worker-1
    assert_eq!(
        parse_addressee("@orchestrator do work", &r, None, &[]),
        Some(QuarkId::new("orch"))
    );

    // @worker-1 must resolve to worker-1, NOT worker-2 (even though worker-2 has display_name "worker-1")
    assert_eq!(
        parse_addressee("@worker-1 do work", &r, None, &[]),
        Some(QuarkId::new("worker-1"))
    );
}

#[test]
fn extract_target_segment_isolates_per_quark_clauses() {
    let r = vec![
        card("quark1", &[]),
        card("quark2", &[]),
        card("quark3", &[]),
    ];

    // Single quark mentioned: returns empty string so caller uses full message
    assert_eq!(
        extract_target_segment("@quark1 /brainstorming", &QuarkId::new("quark1"), &r),
        ""
    );

    // Multi-line per-quark instructions
    let multi_line = "@quark1 /brainstorming design the auth flow\n@quark2 /plan write the migration steps";
    assert_eq!(
        extract_target_segment(multi_line, &QuarkId::new("quark1"), &r),
        "@quark1 /brainstorming design the auth flow"
    );
    assert_eq!(
        extract_target_segment(multi_line, &QuarkId::new("quark2"), &r),
        "@quark2 /plan write the migration steps"
    );

    // Conjoined targets ("@quark1 and @quark2 ... and @quark3 ...")
    let conjoined = "@quark1 and @quark2 /brainstorming design auth and @quark3 /plan write steps";
    assert_eq!(
        extract_target_segment(conjoined, &QuarkId::new("quark1"), &r),
        "@quark1 and @quark2 /brainstorming design auth and"
    );
    assert_eq!(
        extract_target_segment(conjoined, &QuarkId::new("quark2"), &r),
        "@quark1 and @quark2 /brainstorming design auth and"
    );
    assert_eq!(
        extract_target_segment(conjoined, &QuarkId::new("quark3"), &r),
        "@quark3 /plan write steps"
    );
}

#[test]
fn unstamped_blocked_status_does_not_mask_prior_answered_handback() {
    let agy = QuarkId::new("cli-agy");
    let assignment = ulid::Ulid::new();

    // 1. Merge gate handback addressed to cli-agy continuing `assignment`
    let handback = Event::new(
        Actor::Gluon,
        Some(agy.clone()),
        Kind::Message {
            body: "@orchestrator The merge gate stopped this branch from landing.".into(),
        },
    )
    .with_answers(assignment);

    // 2. cli-agy answers the handback
    let reply = Event::new(
        Actor::Quark(agy.clone()),
        None,
        Kind::Message { body: "fixed".into() },
    )
    .with_answers(assignment);

    // 3. Subsequent merge attempt issues an audit PermissionGrant and fails with Blocked (unstamped)
    let grant = Event::new(
        Actor::Gluon,
        Some(agy.clone()),
        Kind::PermissionGrant { approved: true, remember: false },
    );
    let blocked = Event::new(
        Actor::Quark(agy.clone()),
        None,
        Kind::Status { state: QuarkState::Blocked },
    );

    let events = vec![handback, reply, grant, blocked];
    assert!(
        is_request_answered(&events[1..], &events[0], &agy),
        "handback answered by `reply` must remain answered even when a later unstamped `Blocked` status follows"
    );
    assert_eq!(next_pending(&events), None);
}



