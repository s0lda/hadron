    use super::*;
    use serde_json::json;

    fn ev(from: Actor, to: Option<&str>, kind: Kind) -> Event {
        Event::new(from, to.map(QuarkId::new), kind)
    }

    fn roster_entry(id: &str, transport: hadron_lattice::Transport) -> RosterRow {
        RosterRow {
            id: id.to_string(),
            display_name: None,
            state: QuarkState::Ground,
            mode: Mode::Ask,
            mode_is_override: false,
            vendor: String::new(),
            model: String::new(),
            flavor: None,
            transport,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }
    }

    use chrono::{TimeZone, Utc};

    /// `StatsWindow::cutoff`: Session and All-time are unbounded; Week/Month are rolling
    /// lower bounds relative to `now`.
    #[test]
    fn stats_window_cutoffs_bound_the_rolling_windows_only() {
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap();
        assert_eq!(StatsWindow::Session.cutoff(now), None);
        assert_eq!(StatsWindow::AllTime.cutoff(now), None);
        assert_eq!(
            StatsWindow::Week.cutoff(now),
            Some(Utc.with_ymd_and_hms(2026, 7, 9, 12, 0, 0).unwrap())
        );
        assert_eq!(
            StatsWindow::Month.cutoff(now),
            Some(Utc.with_ymd_and_hms(2026, 6, 16, 12, 0, 0).unwrap())
        );
        assert!(!StatsWindow::Session.includes_archives());
        assert!(StatsWindow::Week.includes_archives());
    }

    /// Build a quark reply carrying `fresh` tokens at time `ts`, so window folds have
    /// something to sum and a timestamp to filter on.
    fn spend_reply(quark: &str, fresh: u32, ts: DateTime<Utc>) -> Event {
        let mut reply = Event::new(
            Actor::Quark(QuarkId::new(quark)),
            None,
            Kind::Message { body: "done".into() },
        );
        reply.ts = ts;
        reply.usage = Some(hadron_lattice::Usage {
            model: None,
            spend: hadron_lattice::TokenSpend {
                input: Some(fresh),
                output: Some(0),
                cache_read: None,
                cache_write: None,
            },
            context: None,
            quota: vec![],
        });
        reply
    }

    /// `stats_for` filters by `ts`: a turn older than the window's cutoff is excluded;
    /// one inside is counted. Archived rows fold into the wider windows, not Session.
    #[test]
    fn stats_for_filters_by_timestamp_and_folds_archives_only_when_wide() {
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap();
        let recent = now - chrono::Duration::days(2); // inside Week
        let old = now - chrono::Duration::days(10); // outside Week, inside Month

        // Live field: one recent turn.
        let live = vec![
            ev(Actor::Human, Some("opus"), Kind::Message { body: "go".into() }),
            spend_reply("opus", 100, recent),
        ];
        let view = project(&live);
        // Attribution needs a live roster seat for "opus"; project gives it one (it
        // authored a message), so per_quark carries it.

        // Archived: an older turn (10 days back).
        let archived = project(&[spend_reply("opus", 40, old)]).messages;

        // Week: only the recent live turn — archived `old` is outside the 7-day cutoff.
        let week = view.stats_for(&archived, StatsWindow::Week, now);
        assert_eq!(week.total_fresh, 100, "week counts only the in-window turn");

        // Month: both — `old` (10d) is inside 30 days, and archives fold into wide windows.
        let month = view.stats_for(&archived, StatsWindow::Month, now);
        assert_eq!(month.total_fresh, 140, "month folds the archived turn in too");

        // Session: live field only, ignores the archived slice entirely.
        let session = view.stats_for(&archived, StatsWindow::Session, now);
        assert_eq!(session.total_fresh, 100, "session never folds archives");
    }

    /// `spend_timeline` accumulates fresh spend per quark and team over chronological
    /// steps, and drops a quark that appears but never spends.
    #[test]
    fn spend_timeline_accumulates_per_quark_and_team_dropping_silent_quarks() {
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap();
        let t = |secs| now - chrono::Duration::seconds(secs);
        // Chronological by ts: opus(-30), gemini(-20), opus(-10). `agy` appears (so it
        // holds a roster seat) but only via a no-spend status, so it must be dropped.
        let live = vec![
            spend_reply("opus", 100, t(30)),
            ev(
                Actor::Quark(QuarkId::new("agy")),
                None,
                Kind::Status { state: QuarkState::Ground },
            ),
            spend_reply("gemini", 40, t(20)),
            spend_reply("opus", 50, t(10)),
        ];
        let view = project(&live);

        let tl = view.spend_timeline(&[], StatsWindow::Session, now);

        // Only the quarks that spent, in roster (first-seen) order: opus then gemini.
        assert_eq!(tl.quarks, vec!["opus".to_string(), "gemini".to_string()]);
        assert_eq!(tl.points.len(), 3, "one point per spend event");

        // Team total is the running sum, monotonic: 100, 140, 190.
        let team: Vec<f64> = tl.points.iter().map(|p| p.team).collect();
        assert_eq!(team, vec![100.0, 140.0, 190.0]);

        // Final snapshot: opus 100+50, gemini 40, aligned to `quarks`.
        let last = tl.points.last().unwrap();
        assert_eq!(last.step, 3);
        assert_eq!(last.per_quark, vec![150.0, 40.0]);
        assert_eq!(last.team, 190.0);
    }

    /// `load_archived_messages` reads every `sessions/*/field.jsonl` and concatenates
    /// their projected rows; a missing directory yields no rows and no error.
    #[test]
    fn load_archived_messages_merges_sessions_and_tolerates_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");

        // Missing dir → empty, no panic.
        assert!(load_archived_messages(&sessions).is_empty());

        // Two archived sessions, each with one quark message.
        for (name, quark) in [("20260101_000000", "opus"), ("20260201_000000", "gemini")] {
            let sdir = sessions.join(name);
            std::fs::create_dir_all(&sdir).unwrap();
            let field = sdir.join("field.jsonl");
            hadron_lattice::io::append_event(
                &field,
                &Event::new(Actor::Quark(QuarkId::new(quark)), None, Kind::Message { body: "hi".into() }),
            )
            .unwrap();
        }

        let rows = load_archived_messages(&sessions);
        assert_eq!(rows.len(), 2, "one row per archived session message");
        // Sorted by session dir (timestamp) → opus (Jan) before gemini (Feb).
        assert_eq!(rows[0].from, "opus");
        assert_eq!(rows[1].from, "gemini");
    }

    /// `/clear` restarts every resident agent so the fresh field is a fresh session:
    /// one `Kind::Reboot` per ACP quark, addressed to it; CLI quarks (nothing resident)
    /// are skipped.
    #[test]
    fn post_clear_reboots_one_per_resident_quark_skipping_cli() {
        use hadron_lattice::Transport;
        let roster = vec![
            roster_entry("opus", Transport::Acp),
            roster_entry("agy", Transport::Cli),
            roster_entry("gemini", Transport::Acp),
        ];

        let reboots = post_clear_reboots(&roster);

        assert_eq!(reboots.len(), 2, "one reboot per ACP quark, none for the CLI quark");
        assert!(reboots.iter().all(|e| matches!(e.kind, Kind::Reboot)));
        assert!(reboots.iter().all(|e| e.from == Actor::Human));
        let targets: Vec<String> = reboots
            .iter()
            .filter_map(|e| e.to.as_ref().map(|q| q.as_str().to_string()))
            .collect();
        assert_eq!(targets, vec!["opus".to_string(), "gemini".to_string()]);
    }

    /// The Session tab reported zeros for every quark: it decided who was a quark by
    /// testing `from` for an `@` prefix, and `actor_str` renders a quark as its bare
    /// id. The filter matched nothing, so every statistic silently read as 0 — the
    /// tab looked like a swarm that had never spent a token.
    #[test]
    fn session_stats_attribute_to_the_quark_that_earned_them() {
        let spend = hadron_lattice::TokenSpend {
            input: Some(100),
            output: Some(20),
            cache_read: Some(9_000),
            cache_write: None,
        };
        let usage = hadron_lattice::Usage {
            model: Some("claude-3-opus-20240229".to_string()),
            spend: spend.clone(),
            context: Some(hadron_lattice::ContextUsage {
                used_tokens: 57_000,
                context_window_size: 200_000,
                used_percentage: 28.5,
            }),
            quota: vec![],
        };

        let mut reply = ev(
            Actor::Quark(QuarkId::new("opus")),
            None,
            Kind::Message {
                body: "done".into(),
            },
        );
        reply.usage = Some(usage);

        let evs = vec![
            ev(
                Actor::Human,
                Some("opus"),
                Kind::Message { body: "go".into() },
            ),
            reply,
        ];
        let stats = project(&evs).session_stats();

        let (id, s) = stats
            .per_quark
            .iter()
            .find(|(id, _)| id == "opus")
            .expect("opus holds a roster seat");
        assert_eq!(id, "opus");
        assert_eq!(s.turns, 1);
        assert_eq!(s.fresh, 120, "fresh is input+output, and it is NOT zero");
        assert_eq!(s.cached, 9_000, "cache is carried, separately");
        assert_eq!(s.context.as_ref().unwrap().used_tokens, 57_000);

        // The human's own message is not a turn, and never a quark's spend.
        assert_eq!(stats.total_turns, 1);
        assert_eq!(stats.total_fresh, 120);
    }

    /// The roster showed 14.4M against an ACP quark long after `fresh()` landed: the
    /// legacy fallback still folded a pre-components `used_tokens` into the same column,
    /// and for an ACP seat that number is ACP's cache-INCLUSIVE total. Different unit.
    /// A CLI seat's legacy number really is input+output, so that one still counts.
    #[test]
    fn a_legacy_acp_total_is_not_counted_as_fresh_tokens() {
        use hadron_lattice::{AcpCommand, Flavor, Seat, Transport};

        let team = Team {
            quarks: vec![
                Seat {
                    id: QuarkId::new("acp-claude"),
                    display_name: None,
                    vendor: "acp-claude".into(),
                    model: "x".into(),
                    effort: None,
                    mode_config: None,
                    flavor: Flavor::Worker,
                    transport: Transport::Acp,
                    command: Some(AcpCommand {
                        program: "npx".into(),
                        args: vec![],
                    }),
                    cli: None,
                    enabled: true,
                    roles: vec![],
                    exclusive: false,
                    commands: hadron_lattice::SeatCommands::default(),
                    secret_env: Vec::new(),
                    energy_limit: None,
                },
                Seat {
                    id: QuarkId::new("opus"),
                    display_name: None,
                    vendor: "claude".into(),
                    model: "opus".into(),
                    effort: None,
                    mode_config: None,
                    flavor: Flavor::Orchestrator,
                    transport: Transport::Cli,
                    command: None,
                    cli: None,
                    enabled: true,
                    roles: vec![],
                    exclusive: false,
                    commands: hadron_lattice::SeatCommands::default(),
                    secret_env: Vec::new(),
                    energy_limit: None,
                },
            ],
            roster: vec![],
            max_exchanges: None,
        };

        // Both legacy: no `usage` on the envelope, just the bare u32.
        let evs = vec![
            ev(
                Actor::Quark(QuarkId::new("acp-claude")),
                None,
                Kind::EnergyReport {
                    used_tokens: 1_307_987,
                },
            ),
            ev(
                Actor::Quark(QuarkId::new("opus")),
                None,
                Kind::EnergyReport { used_tokens: 5_338 },
            ),
        ];
        let view = project_with_team(&evs, &team, &Team::default());
        let row = |id: &str| view.roster.iter().find(|r| r.id == id).unwrap().clone();

        let acp = row("acp-claude");
        assert_eq!(
            acp.tokens, 0,
            "an ACP legacy total is a different unit — not fresh"
        );
        assert_eq!(
            acp.unknown_turns, 1,
            "and it is counted as unknown, not dropped"
        );

        let cli = row("opus");
        assert_eq!(
            cli.tokens, 5_338,
            "a CLI legacy total IS input+output — it counts"
        );
        assert_eq!(cli.unknown_turns, 0);
    }

    /// A provider that cannot see cache reports `None`, which means *unknown*.
    /// Counting it as 0 would claim we know it spent nothing.
    #[test]
    fn an_absent_component_is_not_counted_as_zero() {
        let mut reply = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message {
                body: "done".into(),
            },
        );
        reply.usage = Some(hadron_lattice::Usage {
            model: Some("gemini-1.5-pro".to_string()),
            spend: hadron_lattice::TokenSpend {
                input: Some(5),
                output: Some(5),
                cache_read: None,
                cache_write: None,
            },
            context: None,
            quota: vec![],
        });
        let stats = project(&[reply]).session_stats();
        let (_, s) = &stats.per_quark[0];
        assert_eq!(s.fresh, 10);
        assert_eq!(s.cached, 0, "no cache reported — nothing added");
        assert!(s.context.is_none());
        assert!(
            s.quota.is_empty(),
            "empty means no quota concept, not exhausted"
        );
    }

    #[test]
    fn global_mode_and_per_quark_override_are_surfaced() {
        let evs = vec![
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Auto }), // global Auto
            ev(
                Actor::Human,
                Some("agy"),
                Kind::ModeSet { mode: Mode::Bypass },
            ), // agy override
        ];
        let view = project(&evs);
        assert_eq!(view.global_mode, Mode::Auto);
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Bypass);
        assert!(agy.mode_is_override);
    }

    #[test]
    fn roster_row_inherits_global_and_carries_team_legibility() {
        use hadron_lattice::{Flavor, Seat};
        let team = Team {
            quarks: vec![Seat::cli(
                QuarkId::new("agy"),
                "agy",
                "gemini-3-pro",
                Flavor::Worker,
            )],
            roster: vec![],
            max_exchanges: None,
        };
        let evs = vec![
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Write }),
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
        ];
        let view = project_with_team(&evs, &team, &Team::default());
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Write, "inherits the global default");
        assert!(!agy.mode_is_override);
        assert_eq!(agy.vendor, "agy");
        assert_eq!(agy.model, "gemini-3-pro");
    }

    /// A catalogue quark that this repo has NOT adopted still gets a roster row —
    /// greyed (not adopted, inert), its legibility drawn from the catalogue — so the
    /// user sees "there to use when you want, but off".
    #[test]
    fn catalogue_quarks_not_adopted_here_show_as_available() {
        use hadron_lattice::{Flavor, Seat};
        // Resolved team: only "opus" is adopted here.
        let team = Team {
            quarks: vec![Seat::cli(
                QuarkId::new("opus"),
                "claude",
                "opus",
                Flavor::Orchestrator,
            )],
            roster: vec![],
            max_exchanges: None,
        };
        // Catalogue: "opus" (adopted) plus "gemini" (available, not adopted here).
        let global = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Orchestrator),
                Seat::cli(QuarkId::new("gemini"), "agy", "gemini-3-pro", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: None,
        };
        let view = project_with_team(&[], &team, &global);
        let opus = view.roster.iter().find(|r| r.id == "opus").unwrap();
        assert!(opus.adopted, "the seated quark is adopted");
        assert!(opus.enabled);
        let gemini = view.roster.iter().find(|r| r.id == "gemini").unwrap();
        assert!(!gemini.adopted, "the catalogue-only quark is not adopted");
        assert!(!gemini.enabled, "and shows inert (grey dot)");
        assert_eq!(gemini.vendor, "agy", "legibility comes from the catalogue");
    }

    #[test]
    fn mode_set_renders_as_a_row() {
        let view = project(&[ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Bypass })]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "mode_set");
        assert!(view.messages[0].body.contains("bypass"));
    }

    #[test]
    fn message_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("claude"),
            Kind::Message {
                body: "build it".into(),
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.from, "human");
        assert_eq!(row.to.as_deref(), Some("claude"));
        assert_eq!(row.body, "build it");
        assert_eq!(row.kind_label, "message");
    }

    #[test]
    fn pending_permission_is_surfaced_then_cleared_by_a_grant() {
        let req = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::PermissionReq {
                risk: hadron_gatekeeper::Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        // With an outstanding request, the view carries it (addressed to the asker).
        let view = project(std::slice::from_ref(&req));
        let pending = view
            .pending_permission
            .expect("outstanding request surfaced");
        assert_eq!(pending.quark, QuarkId::new("agy"));
        assert_eq!(pending.description, "cargo publish");

        // Once granted, the toast clears.
        let grant = ev(
            Actor::Human,
            Some("agy"),
            Kind::PermissionGrant {
                approved: true,
                remember: false,
            },
        );
        let view = project(&[req, grant]);
        assert!(view.pending_permission.is_none());
    }

    #[test]
    fn assign_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("agy"),
            Kind::Assign {
                task: "work".into(),
                invariants: vec!["no errors".into()],
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.kind_label, "assign");
        assert_eq!(row.body, "assigned: work (invariants: [\"no errors\"])");
    }

    #[test]
    fn latest_status_wins_in_roster() {
        let agy = || Actor::Quark(QuarkId::new("agy"));
        let view = project(&[
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Excited,
                },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Ground,
                },
            ),
        ]);
        let agy_row = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy_row.state, QuarkState::Ground); // latest wins
    }

    #[test]
    fn unknown_event_is_a_muted_row_not_dropped() {
        // Construct an Unknown by round-tripping a future-kind JSON line.
        let line = serde_json::to_string(&json!({
            "v": 2,
            "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "ts": "2026-07-10T14:00:00Z",
            "from": "gluon",
            "to": null,
            "kind": "edit_by_hash",
            "block_hash": "9f86d0"
        }))
        .unwrap();
        let e: Event = serde_json::from_str(&line).unwrap();
        let view = project(&[e]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "unrecognized");
        assert!(view.messages[0].body.contains("edit_by_hash"));
    }

    #[test]
    fn roster_includes_authors_and_addressees() {
        let view = project(&[
            ev(
                Actor::Human,
                Some("orch"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                Actor::Quark(QuarkId::new("orch")),
                Some("worker"),
                Kind::Message {
                    body: "@worker do it".into(),
                },
            ),
        ]);
        let ids: Vec<&str> = view.roster.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"orch"));
        assert!(ids.contains(&"worker"));
        // human is not a quark → not on the roster.
        assert!(!ids.contains(&"human"));
    }
