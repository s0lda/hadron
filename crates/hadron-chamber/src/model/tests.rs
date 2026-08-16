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

    /// `StatsWindow::cutoff`: Session, Current and All-time are unbounded; Week/Month are rolling
    /// lower bounds relative to `now`.
    #[test]
    fn stats_window_cutoffs_bound_the_rolling_windows_only() {
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap();
        assert_eq!(StatsWindow::Current.cutoff(now), None);
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
        assert!(!StatsWindow::Current.includes_archives());
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

    #[test]
    fn stats_for_current_filters_by_last_human_message() {
        let now = Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap();
        let t = |secs| now - chrono::Duration::seconds(secs);
        
        let mut first_msg = ev(Actor::Human, Some("opus"), Kind::Message { body: "first task".into() });
        first_msg.ts = t(50);
        let mut second_msg = ev(Actor::Human, Some("opus"), Kind::Message { body: "second task".into() });
        second_msg.ts = t(30);
        
        let live = vec![
            first_msg,
            spend_reply("opus", 50, t(40)),
            second_msg,
            spend_reply("opus", 100, t(20)),
        ];
        
        let view = project(&live);
        let current = view.stats_for(&[], StatsWindow::Current, now);
        let session = view.stats_for(&[], StatsWindow::Session, now);
        
        assert_eq!(current.total_fresh, 100, "current only contains second run");
        assert_eq!(session.total_fresh, 150, "session contains whole field");
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

    /// `downsample_spend_points` reduces point count while strictly preserving first/last points and monotonicity.
    #[test]
    fn downsample_spend_points_preserves_bounds_and_decimates() {
        let pts: Vec<SpendPoint> = (1..=500)
            .map(|i| SpendPoint {
                step: i,
                per_quark: vec![i as f64 * 10.0],
                team: i as f64 * 10.0,
            })
            .collect();

        // Downsample to max 50 points
        let downsampled = downsample_spend_points(&pts, 50);
        assert!(downsampled.len() <= 50);
        assert_eq!(downsampled.first().unwrap().step, 1);
        assert_eq!(downsampled.first().unwrap().team, 10.0);
        assert_eq!(downsampled.last().unwrap().step, 500);
        assert_eq!(downsampled.last().unwrap().team, 5000.0);

        // Monotonicity preserved
        for w in downsampled.windows(2) {
            assert!(w[0].step < w[1].step);
            assert!(w[0].team <= w[1].team);
        }

        // Small input is untouched
        let small = vec![
            SpendPoint { step: 1, per_quark: vec![10.0], team: 10.0 },
            SpendPoint { step: 2, per_quark: vec![20.0], team: 20.0 },
        ];
        assert_eq!(downsample_spend_points(&small, 50), small);
    }

    /// `downsample_turn_spend` and `downsample_context_points` preserve ends and bound lengths.
    #[test]
    fn downsample_turn_spend_and_context_points_preserve_ends() {
        let turns: Vec<TurnSpend> = (1..=300)
            .map(|t| TurnSpend {
                turn: t,
                fresh: (t * 5) as u32,
                cost_usd: Some(t as f64 * 0.01),
            })
            .collect();
        let ds_turns = downsample_turn_spend(&turns, 40);
        assert!(ds_turns.len() <= 40);
        assert_eq!(ds_turns.first().unwrap().turn, 1);
        assert_eq!(ds_turns.last().unwrap().turn, 300);

        let ctx: Vec<(usize, f64)> = (0..200).map(|i| (i, (i as f64) * 0.5)).collect();
        let ds_ctx = downsample_context_points(&ctx, 30);
        assert!(ds_ctx.len() <= 30);
        assert_eq!(ds_ctx.first().unwrap().0, 0);
        assert_eq!(ds_ctx.last().unwrap().0, 199);
    }

    /// `list_sessions` reads each archived session's directory-name id and the last
    /// `Kind::SessionName` event in its field, if any — `/rename` can run more than
    /// once, and the latest one is what the sessions menu/`/resume` should show.
    #[test]
    fn list_sessions_reads_the_latest_session_name_event_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");

        assert!(list_sessions(&sessions).is_empty(), "missing dir → no rows, no panic");

        let unnamed = sessions.join("20260101_000000");
        std::fs::create_dir_all(&unnamed).unwrap();
        hadron_lattice::io::append_event(
            &unnamed.join("field.jsonl"),
            &ev(Actor::Human, None, Kind::Message { body: "hi".into() }),
        )
        .unwrap();

        let named = sessions.join("20260201_000000");
        std::fs::create_dir_all(&named).unwrap();
        let field = named.join("field.jsonl");
        hadron_lattice::io::append_event(
            &field,
            &ev(Actor::Human, None, Kind::SessionName { name: "first".into() }),
        )
        .unwrap();
        hadron_lattice::io::append_event(
            &field,
            &ev(Actor::Human, None, Kind::SessionName { name: "bugfix-router".into() }),
        )
        .unwrap();

        let rows = list_sessions(&sessions);
        assert_eq!(
            rows,
            vec![
                SessionInfo { id: "20260101_000000".into(), name: None },
                SessionInfo { id: "20260201_000000".into(), name: Some("bugfix-router".into()) },
            ]
        );
    }

    /// `/resume <target>` accepts either the session id (the timestamp dir name,
    /// always unique) or the name set by `/rename` (case-insensitive — a human
    /// typing it back should not have to match case exactly). Id wins on a collision.
    #[test]
    fn find_session_matches_id_then_case_insensitive_name() {
        let sessions = vec![
            SessionInfo { id: "20260101_000000".into(), name: Some("Router Fix".into()) },
            SessionInfo { id: "20260201_000000".into(), name: None },
        ];
        assert_eq!(
            find_session(&sessions, "20260201_000000").map(|s| s.id.as_str()),
            Some("20260201_000000")
        );
        assert_eq!(
            find_session(&sessions, "router fix").map(|s| s.id.as_str()),
            Some("20260101_000000")
        );
        assert!(find_session(&sessions, "nope").is_none());
    }

    /// A session's menu label prefers the `/rename` name; without one it renders the
    /// timestamp id as a date a human can read, and an id that is not a timestamp at
    /// all is shown verbatim rather than mangled into a wrong date.
    #[test]
    fn session_label_prefers_the_name_then_a_readable_timestamp() {
        assert_eq!(
            SessionInfo { id: "20260725_101022".into(), name: Some("router fix".into()) }.label(),
            "router fix"
        );
        assert_eq!(
            SessionInfo { id: "20260725_101022".into(), name: None }.label(),
            "2026-07-25 10:10"
        );
        assert_eq!(SessionInfo { id: "scratch".into(), name: None }.label(), "scratch");
    }

    /// `/resume` swaps out the live field a running daemon appends to — refuse while
    /// any quark is Excited/Thinking, not just Ground/Blocked/Error.
    #[test]
    fn any_quark_mid_turn_true_only_for_excited_or_thinking() {
        use hadron_lattice::Transport;
        let mut r = roster_entry("acp-claude", Transport::Acp);
        assert!(!any_quark_mid_turn(&[r.clone()]));
        for busy in [QuarkState::Excited, QuarkState::Thinking] {
            r.state = busy;
            assert!(any_quark_mid_turn(&[r.clone()]), "{busy:?} should count as mid-turn");
        }
        for idle in [QuarkState::Ground, QuarkState::Blocked, QuarkState::Error, QuarkState::Waiting] {
            r.state = idle;
            assert!(!any_quark_mid_turn(&[r.clone()]), "{idle:?} should not count as mid-turn");
        }
    }

    /// The chat list renders a SUBSET of the projected rows, addressed by index into
    /// `view.messages` — so the index list and the projection must be rebuilt together
    /// or the chat renders rows that are not there. `/resume` rebuilt the projection and
    /// left the index list empty, which is why a resumed session showed an empty chat
    /// until the next message arrived. This is the one definition of "which rows are
    /// chat rows".
    #[test]
    fn chat_message_indices_picks_exactly_the_message_rows() {
        let team = Team { quarks: vec![], ..Default::default() };
        let evs = vec![
            ev(Actor::Human, None, Kind::Message { body: "one".into() }),
            ev(Actor::Gluon, None, Kind::Status { state: QuarkState::Excited }),
            ev(Actor::Human, None, Kind::Message { body: "two".into() }),
        ];
        let view = project_with_team(&evs, &team, &Team::default());

        let ixs = chat_message_indices(&view.messages);
        assert_eq!(ixs, vec![0, 2], "only the two messages are chat rows");
        assert!(ixs.iter().all(|&i| view.messages[i].is_chat()));
        assert!(!view.messages[1].is_chat(), "a status row is log-only");
        assert!(chat_message_indices(&[]).is_empty());
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

    /// Clearing session history removes all session directories while preserving external files like ledger.db.
    #[test]
    fn test_clear_session_history_removes_session_dirs_and_preserves_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let hadron_dir = dir.path().join(".hadron");
        let sessions_dir = hadron_dir.join("sessions");
        let ledger_file = hadron_dir.join("ledger.db");

        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(&ledger_file, "fake-ledger-data").unwrap();

        for name in ["20260101_000000", "20260201_000000"] {
            let sdir = sessions_dir.join(name);
            std::fs::create_dir_all(&sdir).unwrap();
            std::fs::write(sdir.join("field.jsonl"), "test").unwrap();
        }

        assert_eq!(list_sessions(&sessions_dir).len(), 2);

        // Perform clearing of session directories
        if let Ok(rd) = std::fs::read_dir(&sessions_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }

        assert!(list_sessions(&sessions_dir).is_empty());
        assert!(load_archived_messages(&sessions_dir).is_empty());
        assert!(ledger_file.exists(), "ledger.db must be preserved");
        assert_eq!(std::fs::read_to_string(&ledger_file).unwrap(), "fake-ledger-data");
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

    /// An empty `model` means two different things by transport, and the roster must not
    /// render them the same. A CLI seat with no `--model` has *deferred* to the tool's own
    /// config — a real, nameable state — whereas an ACP seat with no model genuinely has
    /// nothing to show. Guards the one home for that rule so the roster row and the Stats
    /// table cannot drift apart.
    #[test]
    fn an_empty_model_reads_as_cli_default_only_for_a_cli_seat() {
        use hadron_lattice::Transport;

        let cli = roster_entry("agy", Transport::Cli);
        assert!(cli.model.is_empty(), "fixture precondition");
        assert_eq!(cli.model_label(), "CLI default");

        let acp = roster_entry("opus", Transport::Acp);
        assert!(acp.model_label().is_empty(), "an ACP seat keeps the caller's own placeholder");

        let pinned = RosterRow { model: "opus[1m]".to_string(), ..roster_entry("opus", Transport::Acp) };
        assert_eq!(pinned.model_label(), "opus[1m]", "a configured model is shown verbatim");
        let pinned_cli = RosterRow { model: "Gemini 3.1 Pro (High)".to_string(), ..roster_entry("agy", Transport::Cli) };
        assert_eq!(pinned_cli.model_label(), "Gemini 3.1 Pro (High)", "never overrides a real value");
    }

    /// Process Manager rows: the daemon first (from the caller's live probe), then
    /// every *adopted* seat with a real status and only the control actions that
    /// mechanism actually supports — restart only for an enabled ACP seat, and the
    /// not-adopted seat omitted entirely (nothing to list — no process exists for it).
    #[test]
    fn build_process_rows_lists_daemon_then_adopted_seats_only() {
        use hadron_lattice::Transport;

        let mut resident = roster_entry("acp-claude", Transport::Acp);
        resident.state = QuarkState::Excited;

        let mut disabled_cli = roster_entry("cli-agy", Transport::Cli);
        disabled_cli.enabled = false;

        let mut not_adopted = roster_entry("acp-agy", Transport::Acp);
        not_adopted.adopted = false;

        let rows = build_process_rows(true, &[resident, disabled_cli, not_adopted]);

        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["gluon", "acp-claude", "cli-agy"],
            "not-adopted seat holds no process, so it's omitted, not greyed"
        );

        assert_eq!(rows[0].status, "Running");
        assert!(!rows[0].can_restart && !rows[0].can_toggle, "chamber has no daemon control path");

        assert_eq!(rows[1].status, "Excited");
        assert!(rows[1].can_restart, "enabled ACP seat can be force-restarted");

        assert_eq!(rows[2].status, "Disabled");
        assert!(!rows[2].can_restart, "a disabled seat holds nothing resident to reap");
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
                    deny_skills: vec![],
                    external_roots: vec![],
                    http_base_url: None,
                    model_params: hadron_lattice::ModelParams::default(),
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
                    deny_skills: vec![],
                    external_roots: vec![],
                    http_base_url: None,
                    model_params: hadron_lattice::ModelParams::default(),
                },
            ],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None,
            merge_strategy: None,
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
    fn stats_accumulation_does_not_overflow_u32() {
        let mut reply1 = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message {
                body: "turn 1".into(),
            },
        );
        reply1.usage = Some(hadron_lattice::Usage {
            model: Some("gemini-1.5-pro".to_string()),
            spend: hadron_lattice::TokenSpend {
                input: Some(3_000_000_000),
                output: Some(0),
                cache_read: Some(3_000_000_000),
                cache_write: None,
            },
            context: None,
            quota: vec![],
        });

        let mut reply2 = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message {
                body: "turn 2".into(),
            },
        );
        reply2.usage = Some(hadron_lattice::Usage {
            model: Some("gemini-1.5-pro".to_string()),
            spend: hadron_lattice::TokenSpend {
                input: Some(2_000_000_000),
                output: Some(0),
                cache_read: Some(2_000_000_000),
                cache_write: None,
            },
            context: None,
            quota: vec![],
        });

        let stats = project(&[reply1, reply2]).session_stats();
        let (_, s) = &stats.per_quark[0];
        assert_eq!(s.fresh, 5_000_000_000u64);
        assert_eq!(s.cached, 5_000_000_000u64);
        assert_eq!(stats.total_fresh, 5_000_000_000u64);
        assert_eq!(stats.total_cached, 5_000_000_000u64);
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
            nucleus_index_budget_kb: None,
            merge_strategy: None,
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
            nucleus_index_budget_kb: None,
            merge_strategy: None,
        };
        // Catalogue: "opus" (adopted) plus "gemini" (available, not adopted here).
        let global = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Orchestrator),
                Seat::cli(QuarkId::new("gemini"), "agy", "gemini-3-pro", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None,
            merge_strategy: None,
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

    /// A quark that only ever appears in the field's HISTORY gets no roster row. On
    /// 2026-08-01 a daemon that could not parse the catalogue seated a mock "claude",
    /// which answered one message; the id then sat in the roster with no vendor and no
    /// model for every later session, because roster membership was seeded from
    /// event-seen ids. Rebuilding did not remove it — nothing but `/clear` could.
    #[test]
    fn an_id_no_seat_source_knows_gets_no_roster_row() {
        use hadron_lattice::{Flavor, Seat};

        let seated = QuarkId::new("acp-claude");
        let team = Team {
            quarks: vec![Seat::cli(seated.clone(), "claude", "opus", Flavor::Orchestrator)],
            ..Default::default()
        };
        let evs = vec![
            ev(Actor::Quark(QuarkId::new("claude")), None, Kind::Message { body: "[Claude] acknowledged".into() }),
            ev(Actor::Quark(seated.clone()), None, Kind::Message { body: "real work".into() }),
        ];

        let view = project_with_team(&evs, &team, &Team::default());
        let ids: Vec<&str> = view.roster.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["acp-claude"], "the ghost id is not a member of the swarm");
        assert_eq!(view.messages.len(), 2, "its history still renders — only the roster row goes");

        // …but with NO seat source at all (a malformed team.json degrades to empty),
        // event-seen ids are all there is: filtering there would blank the roster.
        let view = project_with_team(&evs, &Team::default(), &Team::default());
        let ids: Vec<&str> = view.roster.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["claude", "acp-claude"], "no seats known → keep what spoke");
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

/// **The mode that survives `/clear`.** The effective mode is folded from the field's
/// `ModeSet` events and `/clear` truncates the field, so before this every new session
/// silently reopened on `Mode::Ask` however the human had it set. The seed re-arms it.
#[test]
fn a_non_default_mode_is_re_seeded_after_a_clear() {
    for mode in [Mode::Write, Mode::Auto, Mode::Bypass] {
        let seed = default_mode_seed(mode).expect("a non-default mode must be re-seeded");
        assert!(matches!(seed.kind, Kind::ModeSet { mode: m } if m == mode));
        assert_eq!(seed.to, None, "the GLOBAL default, not a per-quark override");
        // The gatekeeper is what actually reads it back; assert against that, not
        // against a second copy of the folding rule.
        assert_eq!(hadron_gatekeeper::global_mode(&[seed]), mode);
    }
}

/// `Mode::default()` needs no seed: an empty field already resolves to it, so writing
/// one would add a row that changes nothing — and a second definition of the floor
/// beside `Mode::default()` itself.
#[test]
fn the_default_mode_needs_no_seed() {
    assert!(default_mode_seed(Mode::default()).is_none());
    assert_eq!(hadron_gatekeeper::global_mode(&[]), Mode::default());
}

#[test]
fn attention_required_renders_in_chat_with_error_severity() {
    let event = Event::new(
        Actor::Quark(QuarkId::new("agy")),
        None,
        Kind::AttentionRequired {
            urgency: hadron_lattice::AttentionUrgency::Critical,
            summary: "Database connection failed".into(),
            action_needed: Some("Restart postgres".into()),
        },
    );
    let view = project(&[event]);
    assert_eq!(view.messages.len(), 1);
    let row = &view.messages[0];
    assert!(row.is_chat());
    assert!(row.body.contains("🚨 [Attention Required - Critical]"));
    assert!(row.body.contains("Database connection failed"));
    assert!(row.body.contains("Restart postgres"));
    assert_eq!(row.severity, Some(hadron_lattice::Severity::Error));
}

#[test]
fn stats_aggregates_multi_protocol_and_activity_metrics() {
    let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let ev1 = ev(Actor::Human, Some("opus"), Kind::Message { body: "task 1".into() });
    let mut ev2 = spend_reply("opus", 150, now);
    if let Some(ref mut u) = ev2.usage {
        u.spend.input = Some(100);
        u.spend.output = Some(50);
        u.spend.cache_read = Some(400);
        u.spend.cache_write = Some(50);
    }
    let ev3 = ev(Actor::Quark(QuarkId::new("opus")), None, Kind::Edit {
        paths: vec!["src/main.rs".into()],
        git: "abc1234".into(),
        summary: "updated entrypoint".into(),
    });
    let ev4 = ev(Actor::Quark(QuarkId::new("opus")), None, Kind::Command {
        cmd: "cargo check".into(),
        exit: 0,
        out_summary: String::new(),
    });

    let view = project(&[ev1, ev2, ev3, ev4]);
    let stats = view.stats_for(&[], StatsWindow::Session, now);

    assert_eq!(stats.total_turns, 1);
    assert_eq!(stats.total_fresh, 150);
    assert_eq!(stats.total_input, 100);
    assert_eq!(stats.total_output, 50);
    assert_eq!(stats.total_cache_read, 400);
    assert_eq!(stats.total_cache_write, 50);
    assert_eq!(stats.total_cached, 450);
    assert_eq!(stats.total_edits, 1);
    assert_eq!(stats.total_commands, 1);
    assert!(stats.protocol_turns.contains_key("cli") || stats.protocol_turns.contains_key("acp"));
}
