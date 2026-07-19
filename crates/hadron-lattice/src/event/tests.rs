use super::*;

#[test]
fn actor_serializes_as_bare_string() {
    assert_eq!(serde_json::to_string(&Actor::Human).unwrap(), r#""human""#);
    assert_eq!(serde_json::to_string(&Actor::Gluon).unwrap(), r#""gluon""#);
    assert_eq!(
        serde_json::to_string(&Actor::Quark(QuarkId::new("claude"))).unwrap(),
        r#""claude""#
    );
}

#[test]
fn actor_round_trips_quark_and_reserved() {
    for actor in [
        Actor::Human,
        Actor::Gluon,
        Actor::Quark(QuarkId::new("agy")),
    ] {
        let json = serde_json::to_string(&actor).unwrap();
        let back: Actor = serde_json::from_str(&json).unwrap();
        assert_eq!(actor, back);
    }
}

#[test]
fn quark_state_is_snake_case() {
    assert_eq!(serde_json::to_string(&QuarkState::Ground).unwrap(), r#""ground""#);
    let back: QuarkState = serde_json::from_str(r#""excited""#).unwrap();
    assert_eq!(back, QuarkState::Excited);
}

#[cfg(test)]
mod event_tests {
    use super::*;

    #[test]
    fn message_event_round_trips() {
        let ev = Event::new(
            Actor::Human,
            Some(QuarkId::new("claude")),
            Kind::Message { body: "# Build auth".into() },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        // envelope keys present, kind flattened
        assert!(line.contains(r#""kind":"message""#));
        assert!(line.contains(r##""body":"# Build auth""##));
    }

    #[test]
    fn assign_event_round_trips() {
        let ev = Event::new(
            Actor::Human,
            Some(QuarkId::new("claude")),
            Kind::Assign {
                task: "fix tests".into(),
                invariants: vec!["pass".into()],
            },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"assign""#));
        assert!(line.contains(r#""task":"fix tests""#));
        assert!(line.contains(r#""invariants":["pass"]"#));
    }

    #[test]
    fn permission_req_round_trips() {
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            Some(QuarkId::new("human")),
            Kind::PermissionReq {
                risk: Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_req""#));
        assert!(line.contains(r#""risk":"bash_exec""#));
        assert!(line.contains(r#""description":"cargo publish""#));
    }

    #[test]
    fn permission_grant_round_trips() {
        let ev = Event::new(
            Actor::Human,
            None,
            Kind::PermissionGrant { approved: true, remember: true },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_grant""#));
        assert!(line.contains(r#""approved":true"#));
        assert!(line.contains(r#""remember":true"#));
    }

    #[test]
    fn permission_grant_without_remember_defaults_false() {
        // A grant written before the mode ladder has no `remember` field.
        let line = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"human","to":"agy","kind":"permission_grant","approved":true}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.kind, Kind::PermissionGrant { approved: true, remember: false });
    }

    #[test]
    fn mode_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Mode::Ask).unwrap(), r#""ask""#);
        assert_eq!(serde_json::to_string(&Mode::Bypass).unwrap(), r#""bypass""#);
        assert_eq!(Mode::default(), Mode::Ask);
        let back: Mode = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(back, Mode::Auto);
    }

    #[test]
    fn mode_set_round_trips_global_and_per_quark() {
        // Global default (to: None).
        let global = Event::new(Actor::Human, None, Kind::ModeSet { mode: Mode::Auto });
        let line = serde_json::to_string(&global).unwrap();
        assert!(line.contains(r#""kind":"mode_set""#));
        assert!(line.contains(r#""mode":"auto""#));
        assert_eq!(serde_json::from_str::<Event>(&line).unwrap(), global);

        // Per-quark override (to: Some(quark)).
        let per = Event::new(
            Actor::Human,
            Some(QuarkId::new("agy")),
            Kind::ModeSet { mode: Mode::Bypass },
        );
        let back: Event = serde_json::from_str(&serde_json::to_string(&per).unwrap()).unwrap();
        assert_eq!(back, per);
        assert_eq!(back.to, Some(QuarkId::new("agy")));
    }

    #[test]
    fn mode_clear_round_trips() {
        // The "Default" rung: a per-quark clear that reverts to the global default.
        let ev = Event::new(Actor::Human, Some(QuarkId::new("opus")), Kind::ModeClear);
        let line = serde_json::to_string(&ev).unwrap();
        assert!(line.contains(r#""kind":"mode_clear""#));
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.kind, Kind::ModeClear);
        assert_eq!(back.to, Some(QuarkId::new("opus")));
    }

    #[test]
    fn reboot_round_trips() {
        // A human-issued force-restart, targeted at one quark.
        let ev = Event::new(Actor::Human, Some(QuarkId::new("acp-claude")), Kind::Reboot);
        let line = serde_json::to_string(&ev).unwrap();
        assert!(line.contains(r#""kind":"reboot""#));
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.kind, Kind::Reboot);
        assert_eq!(back.to, Some(QuarkId::new("acp-claude")));
    }

    #[test]
    fn null_to_deserializes_as_none() {
        let line = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"claude","to":null,"kind":"status","state":"ground"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.to, None);
        assert_eq!(ev.kind, Kind::Status { state: QuarkState::Ground });
    }

    /// An energy report carries the turn's context + quota, and survives a round trip.
    #[test]
    fn energy_report_carries_usage_and_round_trips() {
        use crate::{ContextUsage, QuotaBucket, TokenSpend, Usage};

        let usage = Usage {
            model: Some("haiku".to_string()),
            spend: TokenSpend::default(),
            context: Some(ContextUsage {
                used_tokens: 635,
                context_window_size: 1_048_576,
                used_percentage: 0.0605,
            }),
            quota: vec![QuotaBucket {
                key: "gemini-weekly".into(),
                remaining_fraction: 0.0517783,
                reset_time: None,
            }],
        };
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::EnergyReport { used_tokens: 927 },
        )
        .with_usage(usage.clone());

        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.usage.unwrap(), usage);
        assert!(line.contains(r#""kind":"energy_report""#));
        assert!(line.contains(r#""used_tokens":927"#));
        assert!(line.contains(r#""gemini-weekly""#));
    }

    /// **PRIVACY, at the boundary that matters.** The field (`.hadron/field.jsonl`) is
    /// a durable, human-readable log the chamber renders. The agy statusline payload
    /// that feeds an energy report also carries the human's email address and plan
    /// tier. Neither may EVER reach a serialized event — this is the test that fails
    /// loudly if someone ever widens the parser to a derived mirror struct.
    #[test]
    fn email_never_reaches_the_serialized_event() {
        // The real payload shape, with a real-looking email in it.
        let payload = r#"{"cwd":"/home/Jake/dev/hadron","conversation_id":"b66f1126","model":{"id":"Gemini 3.1 Pro (High)"},"context_window":{"total_input_tokens":635,"total_output_tokens":292,"context_window_size":1048576,"used_percentage":0.06},"quota":{"gemini-weekly":{"remaining_fraction":0.0517783,"reset_time":"2026-07-13T13:12:03Z"}},"plan_tier":"Google AI Ultra","email":"jake.private@gmail.com","terminal_width":80}"#;

        let t = crate::parse_agy_statusline(payload).unwrap();
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::EnergyReport { used_tokens: t.usage.spend.fresh().unwrap_or(0) },
        )
        .with_usage(t.usage);

        let line = serde_json::to_string(&ev).unwrap();
        assert!(!line.contains("jake.private@gmail.com"), "email in the field: {line}");
        assert!(!line.contains("gmail"), "no email fragment in the field: {line}");
        assert!(!line.contains('@'), "nothing email-shaped in the field: {line}");
        assert!(!line.contains("Google AI Ultra"), "plan tier in the field: {line}");
        assert!(!line.contains("Ultra"), "no plan-tier fragment: {line}");
        // ...while the numbers we DO want are there.
        assert!(line.contains(r#""used_tokens":927"#));
        assert!(line.contains("0.0517783"));
    }

    /// A turn with nothing to report writes no hollow `usage` key at all, and every
    /// event written before telemetry existed still loads.
    #[test]
    fn usage_is_absent_by_default_and_legacy_events_still_load() {
        let plain = Event::new(Actor::Human, None, Kind::Message { body: "hi".into() });
        let line = serde_json::to_string(&plain).unwrap();
        assert!(!line.contains("usage"), "no phantom usage key: {line}");

        // Empty usage attaches nothing rather than an empty object.
        let hollow = Event::new(Actor::Gluon, None, Kind::EnergyReport { used_tokens: 1 })
            .with_usage(crate::Usage::default());
        assert_eq!(hollow.usage, None);

        // A pre-telemetry event on disk.
        let legacy = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"agy","to":null,"kind":"energy_report","used_tokens":42}"#;
        let ev: Event = serde_json::from_str(legacy).unwrap();
        assert_eq!(ev.usage, None);
        assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: 42 });
    }

    /// **The history must not break.** `field.jsonl` is an append-only log, and Jake's
    /// live one holds 70 `energy_report` events written in the OLD shape — a bare
    /// `used_tokens` and no `usage` at all. Widening the event must keep reading every
    /// one of them, or the token split silently eats his history.
    ///
    /// These four lines are copied **verbatim** out of the live
    /// `.hadron/field.jsonl`, not hand-written to match the parser. (They have to be
    /// embedded: `.hadron/` is gitignored, so a test that only read the real file
    /// would silently pass by never running anywhere else — see
    /// [`the_live_field_still_parses`], which does read it, when it is there.)
    #[test]
    fn real_legacy_energy_reports_from_the_live_field_still_parse() {
        let real_lines = [
            r#"{"v":1,"id":"01KXANDETK9A8MBT5YPZKQYQXK","ts":"2026-07-12T07:59:35.251134877Z","from":"opus","to":null,"kind":"energy_report","used_tokens":66}"#,
            r#"{"v":1,"id":"01KXANF0Q8HBFFAT3V5G43DJXZ","ts":"2026-07-12T08:00:26.344776590Z","from":"opus","to":null,"kind":"energy_report","used_tokens":202}"#,
            // The 298k turn that started all of this — the old ACP total, cache included.
            r#"{"v":1,"id":"01KXEKRWQXCMV9CAW3E11WSH3F","ts":"2026-07-13T20:47:50.525410311Z","from":"acp-claude","to":null,"kind":"energy_report","used_tokens":298033}"#,
            r#"{"v":1,"id":"01KXEKMHH7HDRVK54X4JED1JHH","ts":"2026-07-13T20:45:27.975116252Z","from":"opus","to":null,"kind":"energy_report","used_tokens":2318}"#,
        ];

        let want = [66u32, 202, 298_033, 2318];
        for (line, expect) in real_lines.iter().zip(want) {
            let ev: Event = serde_json::from_str(line).expect("a real logged line must still parse");
            assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: expect });
            // The old lines carry no components, and that is honest: we never knew them.
            assert_eq!(ev.usage, None, "a legacy line has no telemetry to invent");
        }
    }

    /// A new-shape event carries the components on the envelope while keeping the
    /// legacy `used_tokens` on the payload — so an old reader (the chamber, which
    /// matches `Kind` exhaustively) still sees a number it understands, and a new one
    /// can tell fresh work from cache traffic.
    #[test]
    fn a_new_energy_report_carries_components_and_stays_readable_by_old_readers() {
        use crate::{TokenSpend, Usage};

        // acp-claude's real turn-1 numbers.
        let spend = TokenSpend {
            input: Some(8),
            output: Some(2_390),
            cache_read: Some(190_454),
            cache_write: Some(44_338),
        };
        let fresh = spend.fresh().unwrap();
        assert_eq!(fresh, 2_398, "the comparable unit: cache is NOT in here");

        let ev = Event::new(
            Actor::Quark(QuarkId::new("acp-claude")),
            None,
            Kind::EnergyReport { used_tokens: fresh },
        )
        .with_usage(Usage { spend: spend.clone(), ..Default::default() });

        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();

        // The legacy payload field is still there, and now it means something
        // comparable to what a CLI quark reports — not a cache-inflated total.
        assert_eq!(back.kind, Kind::EnergyReport { used_tokens: 2_398 });
        // And the cache is carried, not discarded: this is what makes a big number
        // explicable instead of alarming.
        let got = back.usage.expect("components ride on the envelope").spend;
        assert_eq!(got, spend);
        assert_eq!(got.cached(), Some(234_792));
    }

    /// Parse Jake's **actual** log, every line of it, if it is present. This is the
    /// one that would catch a shape we never thought to embed above. It is skipped
    /// (not failed) when `.hadron/` is absent, because it is gitignored and will not
    /// exist in a fresh clone — the embedded test above is what runs there.
    #[test]
    fn the_live_field_still_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.hadron/field.jsonl");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("skipped: no live field at {}", path.display());
            return;
        };

        let mut energy = 0usize;
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let ev: Event = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("live field line {} no longer parses: {e}", n + 1));
            if matches!(ev.kind, Kind::EnergyReport { .. }) {
                energy += 1;
            }
        }
        assert!(energy > 0, "the live field should hold energy reports; found none");
        eprintln!("live field: {} lines, {energy} energy_report", text.lines().count());
    }

    /// The turn id is additive. Every line already in Jake's `field.jsonl` was written
    /// without one, and must still parse — with `turn: None`, which honestly means
    /// "we do not know", not a fabricated link.
    #[test]
    fn legacy_lines_have_no_turn_id_and_still_parse() {
        // Verbatim from the live field.
        let legacy = r#"{"v":1,"id":"01KXEKRWQXCMV9CAW3E11WSH3F","ts":"2026-07-13T20:47:50.525410311Z","from":"acp-claude","to":null,"kind":"energy_report","used_tokens":298033}"#;
        let ev: Event = serde_json::from_str(legacy).unwrap();
        assert_eq!(ev.turn, None, "unknown, not invented");
        assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: 298_033 });

        // And an event with no turn writes no phantom key.
        let plain = Event::new(Actor::Human, None, Kind::Message { body: "hi".into() });
        let line = serde_json::to_string(&plain).unwrap();
        assert!(!line.contains("turn"), "no phantom turn key: {line}");
    }

    /// The join the chamber will actually do: two separate events, one turn.
    #[test]
    fn a_turn_id_links_a_reply_to_its_own_telemetry_across_two_events() {
        use crate::{TokenSpend, Usage};
        let turn = Ulid::new();
        let q = QuarkId::new("acp-claude");

        let reply = Event::new(Actor::Quark(q.clone()), None, Kind::Message { body: "done".into() })
            .with_turn(turn);
        let energy = Event::new(Actor::Quark(q.clone()), None, Kind::EnergyReport { used_tokens: 2_398 })
            .with_usage(Usage {
                spend: TokenSpend { input: Some(8), output: Some(2_390), cache_read: Some(190_454), cache_write: None },
                ..Default::default()
            })
            .with_turn(turn);

        // Through the wire and back — the link has to survive serialization to be worth
        // anything, since the chamber reads the file, not these structs.
        let reply: Event = serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap();
        let energy: Event = serde_json::from_str(&serde_json::to_string(&energy).unwrap()).unwrap();

        assert_eq!(reply.turn, energy.turn);
        assert_ne!(reply.id, energy.id, "distinct events");
        let joined = [&reply, &energy]
            .into_iter()
            .find(|e| e.turn == reply.turn && e.usage.is_some())
            .and_then(|e| e.usage.clone())
            .expect("joined on the turn id, not on adjacency");
        assert_eq!(joined.spend.fresh(), Some(2_398));
    }

    #[test]
    fn unknown_kind_is_preserved_not_crashed() {
        // A future event type today's reader has never seen.
        let line = r#"{"v":2,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"gluon","to":null,"kind":"edit_by_hash","block_hash":"9f86d0","summary":"future"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match &ev.kind {
            Kind::Unknown { kind, raw } => {
                assert_eq!(kind, "edit_by_hash");
                assert_eq!(raw["block_hash"], "9f86d0");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
        // and it re-serializes without loss of the unknown fields
        let reline = serde_json::to_string(&ev).unwrap();
        assert!(reline.contains(r#""kind":"edit_by_hash""#));
        assert!(reline.contains(r#""block_hash":"9f86d0""#));
    }
}
