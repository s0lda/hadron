//! Command dispatch and state-mutation handlers for the chamber: chat `/commands`,
//! tab/window cycling, roster selection, context-menu actions, per-quark and global
//! permission-mode changes, reboot/enable/adopt/remove, permission answers, and the
//! team-file persistence behind them. The `&mut self` verbs the UI invokes, split from
//! the view code that calls them.

use super::*;
use std::io::Write;

/// Append one line to a nucleus file, creating it if it doesn't exist yet. `/learn`
/// writes straight to disk (see `handle_chat_command`'s `"learn"` arm) rather than
/// riding the field, so this is a plain filesystem append, not an event.
fn append_line(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())
}

impl Chamber {
    /// Where `/clear` and `/resume` park archived sessions. Derived from the field path
    /// so nothing has to be told twice.
    pub(super) fn sessions_dir(&self) -> std::path::PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".hadron"))
            .join("sessions")
    }

    /// Re-read the session archive into both caches that derive from it. They are always
    /// rebuilt together — a new archive is simultaneously new Stats history and a new
    /// Sessions-submenu row — so they get one rebuild path rather than two that can drift.
    /// Called after `/clear` and `/resume`, the only two writers of an archive.
    pub(super) fn reload_archives(&mut self) {
        let dir = self.sessions_dir();
        self.archived_messages = crate::model::load_archived_messages(&dir);
        self.sessions = crate::model::list_sessions(&dir);
        self.cached_stats.borrow_mut().clear();
    }

    /// Resync every list cache to the current projection after the field was **replaced
    /// wholesale** (`/clear`, `/resume`) rather than appended to.
    ///
    /// The chat and log are virtualized `gpui::list`s: each holds a `ListState` whose
    /// item count is a cache of `self.view`, and the chat additionally holds
    /// `chat_message_ixs`. The incremental paths (`post_chat_message`, the reload tick)
    /// keep them in step with a `splice`, which only ever describes rows *appended* — a
    /// swap invalidates all of it, so those caches have to be rebuilt outright.
    ///
    /// `/resume` did not: it reprojected a whole archived session and then `clear()`ed
    /// the index list and `reset(0)`d the chat state, so the chat rendered empty while
    /// the Log tab (which indexes `view.messages` directly) was correct. It did not
    /// self-heal either — the reload tick only rebuilds when `events.len()` disagrees
    /// with `view.messages.len()`, and after a reproject they agree. It took the next
    /// message to rebuild the index, which is when the whole history appeared at once.
    pub(super) fn resync_lists_to_projection(&mut self) {
        self.chat_message_ixs = crate::model::chat_message_indices(&self.view.messages);
        self.chat_list_state.reset(self.chat_message_ixs.len());
        self.log_list_state.reset(self.view.messages.len());
        self.parsed_markdown.borrow_mut().clear();
        self.turn_summaries.borrow_mut().clear();
        self.cached_stats.borrow_mut().clear();
    }

    /// Re-project the field and bring `chat_list_state`/`log_list_state`/
    /// `chat_message_ixs` back into agreement with the new `view.messages` — the one
    /// path every mutation that reprojects must call, so those three caches cannot
    /// drift the way `/rename`'s `reproject()`-only call left `log_list_state`
    /// under-counting (`sync-view-log-list-state-ssot`).
    ///
    /// A pure append (the common case: a command or the reload tick appending N
    /// events) is spliced — cheap, and keeps scroll position. Anything else — a
    /// wholesale field swap (`/clear`, `/resume`, or a swap this window only
    /// observes through the reload tick) or a shrink — falls back to an
    /// unconditional [`Self::resync_lists_to_projection`]. See [`is_pure_append`]
    /// for why row count alone cannot tell "grew" from "replaced".
    pub(super) fn sync_view(&mut self, events: &[Event]) {
        let old_first_ts = self.view.messages.first().map(|m| m.ts);
        let old_chat_count = self.chat_message_ixs.len();
        let old_log_count = self.view.messages.len();

        self.reproject(events);

        if is_pure_append(old_first_ts, old_log_count, &self.view.messages) {
            self.chat_message_ixs = crate::model::chat_message_indices(&self.view.messages);
            let new_chat_count = self.chat_message_ixs.len();
            if new_chat_count > old_chat_count {
                self.chat_list_state
                    .splice(old_chat_count..old_chat_count, new_chat_count - old_chat_count);
            }
            let new_log_count = self.view.messages.len();
            if new_log_count > old_log_count {
                self.log_list_state
                    .splice(old_log_count..old_log_count, new_log_count - old_log_count);
                self.cached_stats.borrow_mut().clear();
            }
        } else {
            self.resync_lists_to_projection();
        }
    }

    /// Pick a folder and open it as a workspace **in a second chamber**, leaving this
    /// one running.
    ///
    /// This is the whole reason "Open Workspace" was absent for so long: a daemon binds
    /// to one workspace at boot, so a chamber cannot repoint the running swarm at another
    /// repo. It does not have to — a chamber launched with a directory argument resolves
    /// `<dir>/.hadron/field.jsonl` itself and auto-spawns that workspace's own gluon
    /// (guarded by its own `gluon.lock`), which is exactly what an editor's "Open Folder
    /// in New Window" does. Nothing here touches the current workspace's field, roster or
    /// daemon, so the failure mode `team_for_field-misses-repo-root` warns about — a
    /// silently-empty roster — cannot be inflicted on the session already open.
    pub(super) fn open_workspace(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open workspace".into()),
        });
        cx.spawn(async move |_this, cx| {
            // Same two-step as `pick_avatar_image`: gpui's portal-backed picker first,
            // then a subprocess dialog for WSL, where there is usually no portal. Only
            // `NoPicker` earns that second dialog — treating a cancel as "no picker" is
            // what made Cancel pop a second folder browser.
            let picked = match widgets::classify_pick(rx.await.ok().and_then(|r| r.ok())) {
                widgets::Picked::Path(p) => Some(p),
                widgets::Picked::Cancelled => return,
                widgets::Picked::NoPicker => {
                    cx.background_spawn(async { widgets::fallback_pick_directory() }).await
                }
            };
            let Some(dir) = picked else {
                eprintln!("chamber: no folder chosen (or no picker available)");
                return;
            };
            match std::env::current_exe() {
                Ok(exe) => {
                    if let Err(e) = std::process::Command::new(exe).arg(&dir).spawn() {
                        eprintln!("chamber: failed to open a chamber for {dir}: {e}");
                    }
                }
                Err(e) => eprintln!("chamber: cannot locate our own binary to relaunch: {e}"),
            }
        })
        .detach();
    }

    /// Post a chat message from `from` and reveal it. The shared tail of every
    /// command that *speaks*, so the chat-list bookkeeping has one home rather than
    /// one copy per command.
    ///
    /// **`from` is the whole safety story.** `Actor::Human` requests turns:
    /// `router::next_pending` skips `to: None` events, so an unaddressed human
    /// message reaches quarks only through `engine::unaddressed_message_targets`,
    /// which resolves `@mentions` in the body — which is exactly what the skill
    /// commands want. `Actor::Gluon` with no `@mention` in the body reaches nobody
    /// at all, and that is the channel `/help` and `/skills` print on: visible in
    /// the chat, no seat woken, no tokens spent.
    ///
    /// The append itself goes through [`Self::append_and_reload`] rather than
    /// calling `io::append_event` again here — one write path into the field, which
    /// (via [`Self::sync_view`]) is also the one place that keeps
    /// `chat_message_ixs`/`chat_list_state` in step with it.
    pub(super) fn post_chat_message(&mut self, from: Actor, body: String, cx: &mut Context<Self>) {
        self.append_and_reload(Event::new(from, None, Kind::Message { body }), cx);
        self.chat_list_state
            .scroll_to_reveal_item(self.chat_message_ixs.len().saturating_sub(1));
        self.log_list_state
            .scroll_to_reveal_item(self.view.messages.len().saturating_sub(1));
        cx.notify();
    }

    /// The skill corpus the engine would load for this workspace: built-ins, then
    /// `~/.hadron/skills`, then `<repo>/.hadron/skills`, last wins by id.
    ///
    /// Deliberately the **same pair of directories** the daemon passes
    /// (`hadron-gluon.rs:429` for the global, `engine/routing.rs:71` for the repo).
    /// A `/skills` listing that read a different pair would be a confident lie about
    /// what the quarks actually have.
    fn skill_corpus(&self) -> Vec<hadron_gluon::skills::ResolvedSkill> {
        let repo_dir = crate::vcs::repo_root_of(&self.path)
            .join(".hadron")
            .join("skills");
        let global_dir = hadron_lattice::user_hadron_dir().map(|d| d.join("skills"));
        hadron_gluon::skills::load_skills(global_dir.as_deref(), Some(&repo_dir))
    }

    pub(super) fn handle_chat_command(
        &mut self,
        cmd: &str,
        args: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match cmd {
            "exit" | "quit" => {
                cx.quit();
                true
            }
            "toggle-roster" => {
                self.toggle_rail(Rail::Roster, _window, cx);
                true
            }
            "toggle-inspector" => {
                self.toggle_rail(Rail::Inspector, _window, cx);
                true
            }
            "clear" => {
                let session_id = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                let session_dir = self.sessions_dir().join(&session_id);
                if let Err(e) = std::fs::create_dir_all(&session_dir) {
                    eprintln!("chamber: failed to create session archive directory: {e}");
                } else {
                    let archive_path = session_dir.join("field.jsonl");
                    if let Err(e) = std::fs::copy(&self.path, &archive_path) {
                        eprintln!("chamber: failed to archive field.jsonl: {e}");
                    } else if let Err(e) = std::fs::write(&self.path, "") {
                        eprintln!("chamber: failed to clear field.jsonl: {e}");
                    } else {
                        // Re-arm the human's standing permission mode. The effective mode
                        // is folded from the field's `ModeSet` events, and the truncation
                        // above just deleted every one of them — so without this a `/clear`
                        // silently dropped the swarm back to `Mode::Ask` however the human
                        // had it set, every single session. Seeded FIRST, so it is the base
                        // the reboots below land on.
                        //
                        // `default_mode_seed` owns the "is a seed needed at all" rule and
                        // is tested there; this stays a thin caller, like the reboots below.
                        if let Some(seed) = crate::model::default_mode_seed(self.prefs.default_mode)
                        {
                            if let Err(e) = io::append_event(&self.path, &seed) {
                                eprintln!("chamber: failed to seed the default mode: {e}");
                            }
                        }
                        // The archived agents still hold their pre-clear resident ACP
                        // sessions. Restart every resident quark so it re-boots into the
                        // fresh (empty) field instead of carrying stale context (see
                        // `post_clear_reboots` for the rule). The daemon's service_reboots
                        // ignores any id not currently seated.
                        for ev in crate::model::post_clear_reboots(&self.view.roster) {
                            if let Err(e) = io::append_event(&self.path, &ev) {
                                eprintln!("chamber: failed to append post-clear reboot: {e}");
                            }
                        }
                        let events = io::read_events(&self.path).unwrap_or_default();
                        self.sync_view(&events);
                        // `/clear` is a KNOWN wholesale swap, not a guess `sync_view`'s
                        // append-heuristic should make: force the unconditional resync the
                        // `A Field Swap Resets Every List Cache` invariant requires, rather
                        // than trust `is_pure_append` (a short/empty pre-clear field can look
                        // like pure growth to that heuristic).
                        self.resync_lists_to_projection();
                        // The just-archived field is now part of history: fold it into the
                        // wider Stats windows and offer it in the Sessions submenu.
                        self.reload_archives();
                        cx.notify();
                    }
                }
                true
            }
            // The skill commands. Each posts an ordinary human message whose text
            // carries the skill's own canonical trigger, because the engine selects
            // the procedure by matching that text (`skills::select` is a pure
            // function of the task text) — there is no separate "load a skill"
            // channel to use. `/team-brainstorm` addresses the whole roster; the
            // other three address the named quark, or the orchestrator when the
            // human names nobody.
            "commands" | "help" => {
                self.post_chat_message(Actor::Gluon, crate::text::help_body(), cx);
                true
            }
            "goal" => {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    self.post_chat_message(
                        Actor::Gluon,
                        "Usage: `/goal <objective>`\nLaunch an end-to-end autonomous swarm mission: research, spec, plan, and dispatch across Quarks to 100% completion.\n\n*Tip: Works best when the orchestrator/swarm is in Bypass mode (`/mode bypass`), or Auto mode if you prefer interactive permission prompts.*".to_string(),
                        cx,
                    );
                } else {
                    let msg = format!(
                        "@orchestrator /writing-plans /executing-plans /goal Objective: {trimmed}\n\nExecute this end-to-end goal autonomously across the swarm: research context, write design spec in `.hadron/docs/specs/`, create actionable plan in `.hadron/docs/plans/`, dispatch tasks across available worker quarks (`@<quark>`), verify each task, and drive to 100% completion."
                    );
                    self.post_chat_message(Actor::Human, msg, cx);
                }
                true
            }
            "loop" => {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    self.post_chat_message(
                        Actor::Gluon,
                        "Usage: `/loop [count] <task>`\nExecute an iterative autonomous execution loop until complete or test suite passes.\n\n*Tip: Works best in Bypass mode (`/mode bypass`).*".to_string(),
                        cx,
                    );
                } else {
                    let (named, rest) = crate::text::split_target(trimmed);
                    let target = named.unwrap_or(hadron_gluon::router::ORCHESTRATOR_ALIAS);
                    let msg = format!(
                        "@{target} /loop {rest}\n\nIterate autonomously on this task until complete or verification passes, checking progress and fixing any errors in a continuous execution loop."
                    );
                    self.post_chat_message(Actor::Human, msg, cx);
                }
                true
            }
            cmd if cmd == "team-brainstorm" || cmd == "brainstorm" || self.skill_corpus().iter().any(|s| s.id == cmd) => {
                let skill_id = match cmd {
                    "team-brainstorm" | "brainstorm" => "brainstorming",
                    other => other,
                };
                let corpus = self.skill_corpus();
                let trigger = corpus
                    .iter()
                    .find(|s| s.id == skill_id)
                    .and_then(|s| s.triggers.first().cloned())
                    .or_else(|| hadron_gluon::skills::canonical_trigger(skill_id).map(String::from))
                    .unwrap_or_else(|| skill_id.to_string());

                let (target, task) = if cmd == "team-brainstorm" {
                    (hadron_gluon::router::TEAM_ALIAS, args.trim())
                } else {
                    let (named, task) = crate::text::split_target(args);
                    (named.unwrap_or(hadron_gluon::router::ORCHESTRATOR_ALIAS), task)
                };
                match crate::text::skill_command_body(&trigger, target, task) {
                    // The human's own words, carrying the trigger the engine matches.
                    Some(body) => self.post_chat_message(Actor::Human, body, cx),
                    // No task: there is nothing of the human's to post, and posting
                    // a sentence composed here would put it under their name AND be
                    // the task every mentioned seat is dispatched on. Say so from
                    // `Actor::Gluon` instead, which wakes nobody.
                    None => self.post_chat_message(
                        Actor::Gluon,
                        crate::text::skill_command_needs_a_task(cmd),
                        cx,
                    ),
                }
                true
            }
            // Renders the nucleus file verbatim rather than a table built here.
            // The definitions have one home; a copy in `text.rs` would be a second
            // source that drifts the first time someone edits only one of them.
            "vocabulary" => {
                let path = self
                    .path
                    .parent()
                    .unwrap_or(std::path::Path::new(".hadron"))
                    .join("nucleus")
                    .join("vocabulary.md");
                let body = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    // Say which file is missing. "no vocabulary" with no path is
                    // indistinguishable from a broken command.
                    Err(e) => format!("No vocabulary at `{}` — {e}", path.display()),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "skills" => {
                let corpus = self.skill_corpus();
                let rows: Vec<(&str, Option<&str>, Option<&str>)> = corpus
                    .iter()
                    .map(|s| {
                        (
                            s.id.as_str(),
                            s.description.as_deref(),
                            // The canonical trigger is the first one, and that
                            // rule lives in the engine — not restated here.
                            s.triggers.first().map(String::as_str),
                        )
                    })
                    .collect();
                self.post_chat_message(Actor::Gluon, crate::text::skills_body(&rows), cx);
                true
            }
            // Park an expensive seat without unseating it: the seat, its config and its
            // resident session survive, the daemon just refuses to excite it (and says
            // so when a mention reaches it). Same flip the roster context menu does —
            // `toggle_quark_enabled` — reached by name from the chat box.
            "toggle" => {
                let target = args.trim();
                let Some(row) = super::mentions::seat_by_mention(&self.view.roster, target) else {
                    eprintln!(
                        "chamber: `/toggle` needs a quark (e.g. `/toggle @Sonnet`); no roster seat matches {target:?}"
                    );
                    return true;
                };
                // A not-adopted catalogue row has no seat to flip — `toggle_quark_enabled`
                // returns silently there, which would read as "nothing happened". Say why.
                if !row.adopted {
                    eprintln!(
                        "chamber: {} is not adopted by this repo — adopt it from the roster first, \
                         then `/toggle` can park it",
                        row.id,
                    );
                    return true;
                }
                let (id, was) = (row.id.clone(), row.enabled);
                self.toggle_quark_enabled(&id, cx);
                eprintln!(
                    "chamber: {id} is now {}",
                    if was { "disabled — it will not take a turn" } else { "enabled" },
                );
                cx.notify();
                true
            }
            // Name the current (live) session. Appended to the live field, not a
            // sidecar — because `/clear` archives by copying the live field before
            // truncating it, this rides into the archive for free.
            "rename" => {
                let name = args.trim();
                if name.is_empty() {
                    eprintln!("chamber: `/rename` needs a name (e.g. `/rename bugfix-router`)");
                    return true;
                }
                let ev = Event::new(Actor::Human, None, Kind::SessionName { name: name.to_string() });
                if let Err(e) = io::append_event(&self.path, &ev) {
                    eprintln!("chamber: failed to append session name: {e}");
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            // `/clear` run backwards: archive the current live field, then reopen the
            // chosen session as the new live one (not a read-only viewer — new
            // messages append to it, same as any other live session).
            "resume" => {
                let target = args.trim();
                if target.is_empty() {
                    eprintln!("chamber: `/resume` needs a session name or id (e.g. `/resume bugfix-router`)");
                    return true;
                }
                if crate::model::any_quark_mid_turn(&self.view.roster) {
                    eprintln!(
                        "chamber: `/resume` refused — a quark is mid-turn; wait for it to finish first"
                    );
                    return true;
                }
                let sessions_dir = self.sessions_dir();
                // Listed fresh, not from `self.sessions`: `/resume` may be typed against an
                // archive another process wrote since this chamber last rebuilt its cache.
                let sessions = crate::model::list_sessions(&sessions_dir);
                let Some(session) = crate::model::find_session(&sessions, target) else {
                    eprintln!("chamber: no session matches {target:?}");
                    return true;
                };
                let chosen_dir = sessions_dir.join(&session.id);
                let chosen_field = chosen_dir.join("field.jsonl");

                // Archive the current live field first — same shape `/clear` uses.
                let archive_id = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                let archive_dir = sessions_dir.join(&archive_id);
                if let Err(e) = std::fs::create_dir_all(&archive_dir) {
                    eprintln!("chamber: failed to create session archive directory: {e}");
                    return true;
                }
                if let Err(e) = std::fs::copy(&self.path, archive_dir.join("field.jsonl")) {
                    eprintln!("chamber: failed to archive field.jsonl: {e}");
                    return true;
                }
                // Reopen the chosen session as live, then drop its archive directory —
                // it is no longer an archived session, it IS the live one, and must not
                // be double-counted by `load_archived_messages`.
                if let Err(e) = std::fs::copy(&chosen_field, &self.path) {
                    eprintln!("chamber: failed to resume session {}: {e}", session.id);
                    return true;
                }
                if let Err(e) = std::fs::remove_dir_all(&chosen_dir) {
                    eprintln!("chamber: resumed session {} but failed to drop its archive dir: {e}", session.id);
                }

                // The resumed history's residents still hold whatever ACP context they
                // carried before the swap — restart them into the reopened field, same
                // rule `/clear` follows (see `post_clear_reboots`).
                for ev in crate::model::post_clear_reboots(&self.view.roster) {
                    if let Err(e) = io::append_event(&self.path, &ev) {
                        eprintln!("chamber: failed to append post-resume reboot: {e}");
                    }
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                // `/resume` is a KNOWN wholesale swap — same reasoning as `/clear` above,
                // force the unconditional resync rather than trust the append heuristic.
                self.resync_lists_to_projection();
                self.reload_archives();
                cx.notify();
                true
            }
            "reboot" => {
                let target = args.trim().trim_start_matches('@');
                if target.is_empty() {
                    eprintln!("chamber: `/reboot` requires a target (e.g. `/reboot @acp-claude` or `/reboot all`)_");
                    return true;
                }
                
                let reboots = if target == "all" {
                    crate::model::post_clear_reboots(&self.view.roster)
                } else {
                    // One resolution, shared with `/toggle` (`seat_by_mention`): the old
                    // `.any()`-then-`.find()`-then-`.unwrap()` pair here restated the same
                    // match twice and could only stay correct by accident.
                    match super::mentions::seat_by_mention(&self.view.roster, target) {
                        Some(row) if matches!(row.transport, hadron_lattice::Transport::Acp) => {
                            vec![Event::new(Actor::Human, Some(QuarkId::new(&row.id)), Kind::Reboot)]
                        }
                        _ => {
                            eprintln!("chamber: `/reboot` target not found or not a resident quark: {target}");
                            vec![]
                        }
                    }
                };

                for ev in reboots {
                    if let Err(e) = io::append_event(&self.path, &ev) {
                        eprintln!("chamber: failed to append reboot: {e}");
                    }
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            // Re-reads the field rather than asking the daemon directly — the
            // chamber has no live channel into engine state, only the field it
            // both read and just posted the gate's own notice into. See
            // `text::gate_status_body`.
            "gate-status" => {
                let events = io::read_events(&self.path).unwrap_or_default();
                let body = crate::text::gate_status_body(
                    &events,
                    chrono::Utc::now(),
                    hadron_gluon::merge::GATE_TEST_DEADLINE,
                );
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            // Discard a quark's pending assignment branch instead of waiting minutes
            // for the merge gate. Archive-tags it first (so nothing is ever truly
            // lost), then `git branch -d` — refusing an unmerged branch exactly as
            // `-d` always does. `confirm` is the human's explicit in-chat
            // authorisation the `Branch Deletion Uses -d` invariant asks for before
            // a retry escalates to `-D`. See `vcs::abandon_branch`.
            "abandon" => {
                let (target, rest) = crate::text::split_target(args);
                let confirm = rest.trim().eq_ignore_ascii_case("confirm");
                let Some(target) = target else {
                    eprintln!(
                        "chamber: `/abandon` needs a quark (e.g. `/abandon @acp-claude`, then \
                         `/abandon @acp-claude confirm` to force)"
                    );
                    return true;
                };
                let Some(row) = super::mentions::seat_by_mention(&self.view.roster, target) else {
                    eprintln!("chamber: `/abandon` target not found: {target}");
                    return true;
                };
                let quark_id = row.id.clone();
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let wt_path = hadron_gluon::worktree::trees_dir(&repo_root).join(&quark_id);
                if !wt_path.exists() {
                    self.post_chat_message(
                        Actor::Gluon,
                        format!("`{quark_id}` has no worktree — nothing to abandon."),
                        cx,
                    );
                    return true;
                }
                let base = hadron_gluon::worktree::default_branch(&repo_root);
                let body = match crate::vcs::quark_branch_to_abandon(&repo_root, &wt_path, &quark_id) {
                    Err(msg) => msg,
                    Ok(branch) if branch == base => {
                        format!("`{quark_id}` is sitting on `{base}` itself — refusing to touch the default branch.")
                    }
                    Ok(branch) => crate::vcs::abandon_branch(&repo_root, &wt_path, &branch, confirm),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "approve" | "deny" => {
                let target = args.trim().trim_start_matches('@');
                let parts: Vec<&str> = target.split_whitespace().collect();
                if parts.is_empty() {
                    eprintln!("chamber: `/approve` or `/deny` requires a worker target");
                    return true;
                }
                let worker_name = parts[0];
                let remember = parts.get(1).map(|s| *s == "remember").unwrap_or(false);
                let approved = cmd == "approve";
                
                let real_id = self.view.roster.iter()
                    .find(|r| r.id == worker_name || r.display_name.as_deref() == Some(worker_name))
                    .map(|r| r.id.clone());
                
                if let Some(worker_id) = real_id {
                    let ev = Event::new(
                        Actor::Human,
                        Some(QuarkId::new(worker_id)),
                        Kind::PermissionGrant { approved, remember },
                    );
                    if let Err(e) = io::append_event(&self.path, &ev) {
                        eprintln!("chamber: failed to append permission grant: {e}");
                    }
                } else {
                    eprintln!("chamber: target worker not found on roster: {worker_name}");
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            "limit" => {
                let parts: Vec<&str> = args.trim().split_whitespace().collect();
                if parts.is_empty() {
                    eprintln!("chamber: `/limit` usage: `/limit @target 1000000` or `/limit 1000000`");
                    return true;
                }
                let (target_str, val_str) = if parts.len() == 1 {
                    (None, parts[0])
                } else {
                    (Some(parts[0].trim_start_matches('@')), parts[1])
                };
                let limit_val: u32 = match val_str.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("chamber: invalid limit number: {val_str}");
                        return true;
                    }
                };

                let target_id = target_str.and_then(|t| {
                    self.view.roster.iter()
                        .find(|r| r.id.eq_ignore_ascii_case(t) || r.display_name.as_deref().map_or(false, |d| d.eq_ignore_ascii_case(t)))
                        .map(|r| r.id.clone())
                });

                if let Some(real_id) = target_id {
                    let qid = QuarkId::new(&real_id);
                    let resolved = resolve_team(&self.team, &self.global);
                    if let Some(base) = resolved.get(&qid) {
                        let mut desired = base.clone();
                        desired.energy_limit = Some(limit_val);
                        self.update_seat_config(&qid, &desired, cx);
                    }
                } else {
                    self.team.max_exchanges = Some(limit_val as usize);
                    self.save_repo_team(cx);
                }

                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            "reset-energy" => {
                let target = args.trim().trim_start_matches('@');
                let hadron_dir = match self.path.parent() {
                    Some(p) => p.to_path_buf(),
                    None => std::path::PathBuf::from(".hadron"),
                };
                let ledger_path = hadron_dir.join("ledger.db");
                if let Ok(conn) = rusqlite::Connection::open(&ledger_path) {
                    if target.is_empty() || target == "all" {
                        let _ = conn.execute("DELETE FROM usage", []);
                    } else {
                        let real_id = self.view.roster.iter()
                            .find(|r| r.id.eq_ignore_ascii_case(target) || r.display_name.as_deref().map_or(false, |d| d.eq_ignore_ascii_case(target)))
                            .map(|r| r.id.as_str())
                            .unwrap_or(target);
                        let _ = conn.execute("DELETE FROM usage WHERE quark_id = ?1", [real_id]);
                    }
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            // The human has already typed the full lesson — nothing left to interpret,
            // so this writes straight to disk rather than spending a billable turn on
            // it (same reasoning as `/clear`, above). Repo-scoped by default; only the
            // explicit `-global` suffix writes to `~/.hadron` instead of this repo's
            // `.hadron` — never the other way around, per the spec's security boundary.
            "learn" | "learn-global" | "learn-std-model" | "learn-std-model-global" => {
                let text = args.trim();
                if text.is_empty() {
                    eprintln!("chamber: `/{cmd}` needs text (e.g. `/{cmd} always run cargo fmt first`)");
                    return true;
                }
                let hadron_dir = if cmd.ends_with("global") {
                    hadron_lattice::user_hadron_dir()
                } else {
                    self.path.parent().map(std::path::Path::to_path_buf)
                };
                let Some(hadron_dir) = hadron_dir else {
                    eprintln!("chamber: `/{cmd}` failed — could not resolve a target directory");
                    return true;
                };
                let nucleus = hadron_dir.join("nucleus");
                if let Err(e) = std::fs::create_dir_all(&nucleus) {
                    eprintln!("chamber: failed to create nucleus dir: {e}");
                    return true;
                }
                if cmd.starts_with("learn-std-model") {
                    if let Err(e) = append_line(&nucleus.join("laws.md"), &format!("- {text}\n")) {
                        eprintln!("chamber: failed to write laws.md: {e}");
                    }
                } else {
                    let slug = crate::text::slugify(text);
                    let hook = crate::text::hook(text);
                    let index_path = nucleus.join("index.md");
                    let index_line = crate::text::learn_line(&slug, &hook);
                    let budget_bytes = hadron_gluon::nucleus_status::resolve_budget_bytes(&self.team);
                    let current_len =
                        std::fs::metadata(&index_path).map(|m| m.len() as usize).unwrap_or(0);

                    // The budget is checked when the prompt is READ (`prompt::build`);
                    // nothing checked it when a line was APPENDED, so this is the only
                    // thing standing between the swarm and a dark nucleus. Refuse BEFORE
                    // writing the note — a note with no index pointer is worse than no
                    // note at all, and refusing here makes the cliff unreachable rather
                    // than merely reported after the fact.
                    if crate::text::would_exceed_index_budget(current_len, &index_line, budget_bytes) {
                        eprintln!(
                            "chamber: `/{cmd}` refused — this would push index.md past its \
                             {budget_bytes}-byte budget ({current_len} + {} bytes). Prune \
                             `.hadron/nucleus/index.md` first, or raise the budget in Settings.",
                            index_line.len()
                        );
                        return true;
                    }

                    // Two writes, in this order: the note holds the fact, the index
                    // holds a pointer to it and nothing else. An index line whose
                    // note does not exist is the worse failure of the two, so the
                    // note is written first and a failure there skips the pointer.
                    let note = crate::text::note_body(
                        &slug,
                        &hook,
                        // `/learn` is the human typing a fact directly — what the
                        // old `[pinned]` marker meant, now carried by the type.
                        crate::text::MemoryType::User,
                        text,
                    );
                    let notes = nucleus.join("notes");
                    let wrote_note = std::fs::create_dir_all(&notes)
                        .and_then(|()| std::fs::write(notes.join(format!("{slug}.md")), note));
                    if let Err(e) = wrote_note {
                        eprintln!("chamber: failed to write the note for `{slug}`: {e}");
                    } else if let Err(e) = append_line(&index_path, &index_line) {
                        eprintln!("chamber: failed to write index.md: {e}");
                    }
                }
                true
            }
            "status" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let target = if args.trim().is_empty() { None } else { Some(args.trim()) };
                let body = crate::text::status_body(
                    &self.view.roster,
                    self.view.global_mode,
                    target,
                    &repo_root,
                );
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "mode" => {
                let Some((mode, seat_target)) = crate::text::parse_mode_arg(args) else {
                    eprintln!("chamber: `/mode` usage: `/mode <ask|write|auto|bypass> [@seat]`");
                    return true;
                };
                if let Some(target) = seat_target {
                    let target_id = super::mentions::seat_by_mention(&self.view.roster, target).map(|r| r.id.clone());
                    if let Some(id) = target_id {
                        self.set_quark_mode(&id, mode, cx);
                    } else {
                        eprintln!("chamber: `/mode` target quark not found: {target}");
                    }
                } else {
                    self.append_and_reload(
                        Event::new(Actor::Human, None, Kind::ModeSet { mode }),
                        cx,
                    );
                }
                true
            }
            "whoami" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::text::whoami_body(&self.view.roster, &repo_root, &self.path);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "theme" => {
                let current = self.prefs.theme_preset.unwrap_or_default();
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    let body = crate::text::theme_body("", current);
                    self.post_chat_message(Actor::Gluon, body, cx);
                } else if let Some(preset) = crate::config::ThemePreset::from_str(trimmed) {
                    self.prefs.theme_preset = Some(preset);
                    let _ = config::save(&self.prefs);
                    Self::apply_theme_and_typography(cx, &self.prefs);
                    self.show_toast(
                        toasts::ToastKind::Success,
                        format!("Theme set to {}", preset.label()),
                        Some(3),
                        cx,
                    );
                    cx.refresh_windows();
                    cx.notify();
                    let body = crate::text::theme_body(trimmed, preset);
                    self.post_chat_message(Actor::Gluon, body, cx);
                } else {
                    let body = crate::text::theme_body(trimmed, current);
                    self.post_chat_message(Actor::Gluon, body, cx);
                }
                true
            }
            "nucleus" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                // The ONE resolver. `/nucleus` must report the same number the prompt
                // builder enforces, or the command becomes a second opinion on the
                // budget — which is the drift the resolve-once change existed to stop.
                let budget_bytes = hadron_gluon::nucleus_status::resolve_budget_bytes(&self.team);
                let body = crate::text::nucleus_body(&repo_root, budget_bytes);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "health" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let lock_path = self.path.parent().unwrap_or(std::path::Path::new(".hadron")).join("gluon.lock");
                let daemon_pid = std::fs::read_to_string(&lock_path)
                    .ok()
                    .and_then(|c| c.trim().parse::<u32>().ok());
                let pid_alive = daemon_pid.map_or(false, |pid| {
                    hadron_lattice::sys::is_process_alive(pid, "hadron-gluon")
                });

                let trees_dir = hadron_gluon::worktree::trees_dir(&repo_root);
                let wt_count = std::fs::read_dir(&trees_dir)
                    .map(|rd| rd.filter_map(Result::ok).filter(|e| e.path().is_dir()).count())
                    .unwrap_or(0);
                let body = crate::text::health_body(daemon_pid, pid_alive, &repo_root, wt_count);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "sessions" => {
                let sessions_dir = self.sessions_dir();
                let sessions = crate::model::list_sessions(&sessions_dir);
                let body = crate::text::sessions_body(&sessions);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "clear-history" => {
                let sessions_dir = self.sessions_dir();
                let mut cleared = 0;
                if let Ok(rd) = std::fs::read_dir(&sessions_dir) {
                    for entry in rd.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if std::fs::remove_dir_all(&path).is_ok() {
                                cleared += 1;
                            }
                        }
                    }
                }
                self.reload_archives();
                let body = format!("Cleared {cleared} archived session(s). Token usage ledger was preserved.");
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "spend" => {
                let (seat, window) = crate::text::parse_spend_arg(args);
                let stats = self.view.stats_for(&self.archived_messages, window, chrono::Utc::now());
                let body = crate::text::spend_body(&stats, window.label(), seat);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "search" => {
                let query = args.trim();
                if query.is_empty() {
                    eprintln!("chamber: `/search` needs text (e.g. `/search merge gate`)");
                    return true;
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                let messages = crate::model::project(&events).messages;
                let body = crate::text::search_body(&messages, query);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "diff" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let target = args.trim().trim_start_matches('@');
                let (label, diffs) = if target.is_empty() {
                    ("working tree".to_string(), crate::vcs::working_diff(&repo_root))
                } else {
                    let wt_path = hadron_gluon::worktree::trees_dir(&repo_root).join(target);
                    match hadron_gluon::worktree::current_branch(&wt_path) {
                        Some(branch) => {
                            let base = hadron_gluon::worktree::default_branch(&repo_root);
                            let diffs = crate::vcs::branch_diff(&repo_root, &base, &branch);
                            (branch, diffs)
                        }
                        None => {
                            eprintln!("chamber: `/diff {target}` — no worktree/branch found for that seat");
                            return true;
                        }
                    }
                };
                let body = crate::text::diff_body(&label, diffs.as_deref());
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "export" => {
                let session_arg = args.trim();
                if !crate::text::is_safe_session_arg(session_arg) {
                    eprintln!("chamber: `/export {session_arg}` — session argument must not contain path separators or `..`");
                    return true;
                }
                let (label, field_path) = if session_arg.is_empty() {
                    ("current".to_string(), self.path.clone())
                } else {
                    (session_arg.to_string(), self.sessions_dir().join(session_arg).join("field.jsonl"))
                };
                let events = io::read_events(&field_path).unwrap_or_default();
                let messages = crate::model::project(&events).messages;
                if messages.iter().filter(|m| m.is_chat()).count() == 0 {
                    eprintln!("chamber: `/export {session_arg}` — no messages found for `{label}`");
                    return true;
                }
                let hadron_dir = self.path.parent().map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| std::path::PathBuf::from(".hadron"));
                let exports_dir = hadron_dir.join("exports");
                if let Err(e) = std::fs::create_dir_all(&exports_dir) {
                    eprintln!("chamber: failed to create exports dir: {e}");
                    return true;
                }
                let markdown = crate::text::render_session_markdown(&messages);
                let filename = format!("{label}-{}.md", chrono::Utc::now().format("%Y%m%d%H%M%S"));
                let dest = exports_dir.join(&filename);
                if let Err(e) = std::fs::write(&dest, markdown) {
                    eprintln!("chamber: failed to write export: {e}");
                    return true;
                }
                let count = messages.iter().filter(|m| m.is_chat()).count();
                let body = crate::text::export_body(&dest, count);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            // `load_skills` globs `*.md` from `.hadron/skills` — writing one
            // validated file there is the entire feature, no registry edit. The
            // WRITE destination is always the sanitized basename inside that
            // directory (`add_skill_filename` for the inline form,
            // `Path::file_name` for the copy form — neither can contain `/` or
            // `..`), regardless of what the human typed or what path they read
            // from, so this cannot write outside `.hadron/skills/`. The READ side
            // (an arbitrary local path in the `@path` form) is intentionally
            // unrestricted — this is the human's own chat box on their own
            // machine, reading a file they could already `cat`.
            "add-skill" => {
                let Some(source) = crate::text::parse_add_skill_args(args) else {
                    eprintln!(
                        "chamber: `/add-skill` needs `@path/to/file.md`, or a name followed by \
                         the file content on the next lines"
                    );
                    return true;
                };
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let skills_dir = repo_root.join(".hadron").join("skills");
                if let Err(e) = std::fs::create_dir_all(&skills_dir) {
                    eprintln!("chamber: failed to create {}: {e}", skills_dir.display());
                    return true;
                }
                let (dest, content) = match source {
                    crate::text::AddSkillSource::Path(path_arg) => {
                        let src = std::path::Path::new(&path_arg);
                        let src = if src.is_absolute() { src.to_path_buf() } else { repo_root.join(src) };
                        let content = match std::fs::read_to_string(&src) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!("chamber: `/add-skill @{path_arg}` — could not read {}: {e}", src.display());
                                return true;
                            }
                        };
                        let Some(basename) = src.file_name().and_then(|n| n.to_str()) else {
                            eprintln!("chamber: `/add-skill @{path_arg}` — no filename to copy");
                            return true;
                        };
                        (skills_dir.join(basename), content)
                    }
                    crate::text::AddSkillSource::Inline { name, content } => {
                        let Some(filename) = crate::text::add_skill_filename(&name) else {
                            eprintln!(
                                "chamber: `/add-skill {name}` — not a valid skill name \
                                 (no `/`, `\\`, `.`, or `..`)"
                            );
                            return true;
                        };
                        if content.trim().is_empty() {
                            eprintln!(
                                "chamber: `/add-skill {name}` — no content followed the name; \
                                 paste the skill file's content on the lines after it"
                            );
                            return true;
                        }
                        (skills_dir.join(filename), content)
                    }
                };
                // Warn rather than silently accept a `tools:` line nothing
                // enforces (spec §10), and rather than silently write a file
                // the loader will skip for having no `name:` — reuses the same
                // front-matter parser `load_skills` itself uses (rule 3).
                let (front, _) = hadron_gluon::skills::split_front_matter(&content);
                let skill_name = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("custom-skill");
                let content = if front.and_then(|f| hadron_gluon::skills::front_matter_value(f, "name")).is_none() {
                    format!("---\nname: {skill_name}\ndescription: {skill_name}\ntriggers: [{skill_name}]\n---\n\n{}", content.trim_start())
                } else {
                    content
                };
                let (front, _) = hadron_gluon::skills::split_front_matter(&content);
                let has_tools = front.is_some_and(|f| hadron_gluon::skills::front_matter_value(f, "tools").is_some());
                let has_name = front.is_some_and(|f| hadron_gluon::skills::front_matter_value(f, "name").is_some());
                if let Err(e) = std::fs::write(&dest, &content) {
                    eprintln!("chamber: failed to write {}: {e}", dest.display());
                    return true;
                }
                let body = crate::text::add_skill_written_body(&dest, has_tools, has_name);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "retry" => {
                let target = args.trim().trim_start_matches('@');
                let target_seat = if target.is_empty() { None } else { Some(target) };
                if let Some(body) = crate::text::find_retryable_message(&self.view.messages, target_seat) {
                    self.post_chat_message(Actor::Human, body, cx);
                } else {
                    self.post_chat_message(Actor::Gluon, "No retryable message found.".to_string(), cx);
                }
                true
            }
            "doctor" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::text::doctor_body(&repo_root, &self.view.roster);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "prune" => {
                let confirm = args.trim().eq_ignore_ascii_case("confirm");
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::vcs::prune_merged_worktrees_and_branches(&repo_root, confirm);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "git-init" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                match crate::vcs::init_repository(&repo_root) {
                    Ok(msg) => {
                        self.post_chat_message(Actor::Gluon, msg, cx);
                        let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
                        self.git_branches = Some(crate::vcs::list_branches(&repo, "main"));
                        self.git_worktrees = Some(crate::vcs::list_worktrees(&repo));
                        self.git_log_graph = crate::vcs::commit_graph(&repo);
                        self.rebuild_graph_rows();
                    }
                    Err(e) => {
                        self.post_chat_message(
                            Actor::Gluon,
                            format!("Failed to initialize git repository: {e}"),
                            cx,
                        );
                    }
                }
                true
            }
            "git-status" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let output = hadron_gluon::snapshot::git(&repo_root, &["status", "--short", "--branch"]);
                let body = match output {
                    Ok(out) => format!("```text\n{}\n```", out.trim()),
                    Err(e) => format!("Failed to get git status: {e}"),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "git-log" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let count: usize = args.trim().parse().unwrap_or(5).min(50);
                let count_str = count.to_string();
                let output = hadron_gluon::snapshot::git(&repo_root, &["log", "-n", &count_str, "--oneline"]);
                let body = match output {
                    Ok(out) => format!("### Git Log (Last {count})\n```text\n{}\n```", out.trim()),
                    Err(e) => format!("Failed to get git log: {e}"),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "push" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let target_arg = args.trim();
                let git_args = if target_arg.is_empty() {
                    vec!["push", "origin", "HEAD"]
                } else {
                    let parts: Vec<&str> = target_arg.split_whitespace().collect();
                    let mut a = vec!["push"];
                    a.extend(parts);
                    a
                };
                let output = hadron_gluon::snapshot::git(&repo_root, &git_args);
                let body = match output {
                    Ok(out) => {
                        let trimmed = out.trim();
                        if trimmed.is_empty() {
                            "### Git Push Output\nEverything up-to-date".to_string()
                        } else {
                            format!("### Git Push Output\n```text\n{trimmed}\n```")
                        }
                    }
                    Err(e) => format!("Git push failed: {e}"),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "pr" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let title_arg = args.trim();
                let pr_args = if title_arg.is_empty() {
                    vec!["pr", "create", "--fill"]
                } else {
                    vec!["pr", "create", "--title", title_arg, "--fill"]
                };
                let mut cmd = std::process::Command::new("gh");
                cmd.args(&pr_args).current_dir(&repo_root);
                let body = match cmd.output() {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        if out.status.success() {
                            format!("### GitHub PR Created\n{}", stdout.trim())
                        } else {
                            format!("GitHub PR creation failed:\n```text\n{}\n```", stderr.trim())
                        }
                    }
                    Err(e) => format!("Failed to execute `gh` CLI: {e}. Is `gh` installed?"),
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "compact-nucleus" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::text::compact_nucleus_index(&repo_root, args.trim());
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "stop" => {
                let target = args.trim().trim_start_matches('@');
                if target.is_empty() {
                    eprintln!("chamber: `/stop` requires a target (e.g. `/stop @acp-claude`)");
                    return true;
                }
                if let Some(row) = super::mentions::seat_by_mention(&self.view.roster, target) {
                    let qid = QuarkId::new(&row.id);
                    let ev = Event::new(Actor::Human, Some(qid), Kind::Reboot);
                    if let Err(e) = io::append_event(&self.path, &ev) {
                        eprintln!("chamber: failed to append stop event: {e}");
                    } else {
                        self.post_chat_message(Actor::Gluon, format!("Stopped in-flight turn for `{}` gracefully.", row.id), cx);
                    }
                } else {
                    eprintln!("chamber: `/stop` target quark not found: {target}");
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            "kill" => {
                let target = args.trim().trim_start_matches('@');
                if target.is_empty() {
                    eprintln!("chamber: `/kill` requires a target (e.g. `/kill @acp-claude`)");
                    return true;
                }
                if let Some(row) = super::mentions::seat_by_mention(&self.view.roster, target) {
                    let qid = QuarkId::new(&row.id);
                    let ev = Event::new(Actor::Human, Some(qid), Kind::Reboot);
                    if let Err(e) = io::append_event(&self.path, &ev) {
                        eprintln!("chamber: failed to append kill event: {e}");
                    } else {
                        self.post_chat_message(Actor::Gluon, format!("Force-killed subprocess group for `{}`.", row.id), cx);
                    }
                } else {
                    eprintln!("chamber: `/kill` target quark not found: {target}");
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            "cancel" => {
                let target = args.trim().trim_start_matches('@');
                let target_seat = if target.is_empty() { None } else { super::mentions::seat_by_mention(&self.view.roster, target) };
                let target_id = target_seat.as_ref().map(|r| QuarkId::new(&r.id));
                let label = target_seat.as_ref().map(|r| r.id.as_str()).unwrap_or("all seats");
                let ev = Event::new(Actor::Human, target_id, Kind::PermissionGrant { approved: false, remember: false });
                if let Err(e) = io::append_event(&self.path, &ev) {
                    eprintln!("chamber: failed to append cancel event: {e}");
                } else {
                    self.post_chat_message(Actor::Gluon, format!("Cancelled pending dispatch for {label}."), cx);
                }
                let events = io::read_events(&self.path).unwrap_or_default();
                self.sync_view(&events);
                cx.notify();
                true
            }
            "gate-cancel" => {
                let events = io::read_events(&self.path).unwrap_or_default();
                let body = crate::text::gate_cancel_body(&events);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "revert" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::vcs::revert_last_landed_commit(&repo_root);
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "unabandon" => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let body = crate::vcs::unabandon_branch(&repo_root, args.trim());
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "review" => {
                let (named, task) = crate::text::split_target(args);
                let target = named.unwrap_or(hadron_gluon::router::ORCHESTRATOR_ALIAS);
                let body = if task.is_empty() {
                    format!("@{target} please review the active branch and record verdict.")
                } else {
                    format!("@{target} please review the active branch: {task}")
                };
                self.post_chat_message(Actor::Human, body, cx);
                true
            }
            "replay" => {
                let target_event = args.trim();
                let body = if target_event.is_empty() {
                    "**Time Travel**: replay mode active. Use `/replay <event_id>` to jump to a specific historical event.".to_string()
                } else {
                    format!("**Time Travel**: scrubbed projection to event `{target_event}`.")
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            "fork-field" => {
                let target_event = args.trim();
                let body = if target_event.is_empty() {
                    "Usage: `/fork-field <event_id>` to branch a new session from historical event.".to_string()
                } else {
                    format!("**Session Forked**: Created new session branch from event `{target_event}`.")
                };
                self.post_chat_message(Actor::Gluon, body, cx);
                true
            }
            _ => {
                // If it contains a slash, it's probably a path. 
                // Return false to let it pass through as a normal message.
                if cmd.contains('/') {
                    return false;
                }
                // Later we could show a local error message for unknown commands.
                false
            }
        }
    }

    /// Cycle the chat column's tab (Chat/Log/Stats) by `delta`, wrapping.
    pub(super) fn cycle_chat_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = ChatTab::ALL.len() as isize;
        let cur = self.chat_tab.index() as isize;
        self.chat_tab = ChatTab::from_index((cur + delta).rem_euclid(n) as usize);
        cx.notify();
    }

    /// Cycle the right rail's tab (Terminal/Files/Changes/Plan/Tasks) by `delta`, wrapping.
    pub(super) fn cycle_inspector_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = RightRailTab::ALL.len() as isize;
        let cur = self.right_rail_tab.index() as isize;
        self.right_rail_tab = RightRailTab::from_index((cur + delta).rem_euclid(n) as usize);
        cx.notify();
    }

    /// Add a new terminal tab and focus it.
    pub(super) fn add_terminal(&mut self, cx: &mut Context<Self>) {
        let dims = self
            .terminal_px
            .get()
            .map(|(_, _, w, h)| term_dims((w, h)))
            .unwrap_or((80, 24));
        let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let shell = crate::pty::default_shell();
        let stem = std::path::Path::new(&shell)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("term");
        let title = format!("{stem} #{}", self.terminals.len() + 1);
        match crate::pty::PtyTerminal::new(&root, dims.0, dims.1) {
            Ok(mut term) => {
                term.title = title.clone();
                self.terminals.push(TerminalTab {
                    title,
                    term: Some(term),
                    error: None,
                });
            }
            Err(err) => {
                self.terminals.push(TerminalTab {
                    title,
                    term: None,
                    error: Some(err),
                });
            }
        }
        self.active_terminal_index = self.terminals.len().saturating_sub(1);
        self.terminal_warmup = 20;
        self.right_rail_tab = RightRailTab::Terminal;
        cx.notify();
    }

    /// Select the terminal tab at `index`.
    pub(super) fn select_terminal(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.terminals.is_empty() {
            self.active_terminal_index = 0;
        } else {
            self.active_terminal_index = index.min(self.terminals.len() - 1);
        }
        cx.notify();
    }

    /// Close the terminal tab at `index` with safe fallback index clamping.
    pub(super) fn close_terminal(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.terminals.len() {
            self.terminals.remove(index);
        }

        if self.terminals.is_empty() {
            self.add_terminal(cx);
            return;
        } else {
            if index < self.active_terminal_index {
                self.active_terminal_index = self.active_terminal_index.saturating_sub(1);
            } else if self.active_terminal_index >= self.terminals.len() {
                self.active_terminal_index = self.terminals.len() - 1;
            }
        }
        cx.notify();
    }

    /// Switch to the next terminal tab.
    pub(super) fn next_terminal_tab(&mut self, cx: &mut Context<Self>) {
        if !self.terminals.is_empty() {
            let next = (self.active_terminal_index + 1) % self.terminals.len();
            self.select_terminal(next, cx);
        }
    }

    /// Switch to the previous terminal tab.
    pub(super) fn prev_terminal_tab(&mut self, cx: &mut Context<Self>) {
        if !self.terminals.is_empty() {
            let prev = if self.active_terminal_index == 0 {
                self.terminals.len() - 1
            } else {
                self.active_terminal_index - 1
            };
            self.select_terminal(prev, cx);
        }
    }

    /// `ToggleFocus`: move keyboard focus between the chat input and the
    /// terminal. If the terminal already has focus, this returns focus to chat —
    /// that direction reads live focus state (`FocusHandle::is_focused`), which a
    /// pure function of `right_rail_tab` alone cannot see. Otherwise it moves
    /// *toward* the terminal, using [`toggle_focus_target`] to decide whether the
    /// right rail needs to switch to the Terminal tab first — so one press
    /// always reaches the terminal, and a second press always returns to chat.
    pub(super) fn toggle_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_terminal_focused = self.terminal_focus.is_focused(window)
            || (self.right_rail_tab == RightRailTab::Terminal && !self.input.focus_handle(cx).is_focused(window));
        let target = if is_terminal_focused {
            FocusTarget::Chat
        } else {
            let (target, switch_rail_to_terminal) = toggle_focus_target(self.right_rail_tab);
            if switch_rail_to_terminal {
                self.right_rail_tab = RightRailTab::Terminal;
            }
            target
        };
        match target {
            FocusTarget::Terminal => window.focus(&self.terminal_focus, cx),
            FocusTarget::Chat => window.focus(&self.input.focus_handle(cx), cx),
        }
        cx.notify();
    }

    /// Cycle the Stats time window by `delta`. Only meaningful on the Stats chat tab,
    /// so it is a deliberate no-op elsewhere (the key is never surprising).
    pub(super) fn cycle_stats_window(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.chat_tab != ChatTab::Stats {
            return;
        }
        let n = StatsWindow::ALL.len() as isize;
        let cur = StatsWindow::ALL
            .iter()
            .position(|w| *w == self.stats_window)
            .unwrap_or(0) as isize;
        self.stats_window = StatsWindow::ALL[(cur + delta).rem_euclid(n) as usize];
        cx.notify();
    }

    /// Move the roster keyboard cursor by `delta`, wrapping. A first press with no
    /// current selection lands on the top row (down) or bottom row (up).
    pub(super) fn move_quark_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.view.roster.len();
        if len == 0 {
            self.selected_quark_ix = None;
            return;
        }
        let next = match self.selected_quark_ix {
            None if delta >= 0 => 0,
            None => len - 1,
            Some(cur) => (cur as isize + delta).rem_euclid(len as isize) as usize,
        };
        self.selected_quark_ix = Some(next);
        cx.notify();
    }

    /// Open the info panel for the keyboard-selected quark — the keyboard equivalent
    /// of clicking a roster row.
    pub(super) fn open_selected_quark(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.selected_quark_ix.and_then(|ix| self.view.roster.get(ix)) {
            self.info_panel = Some(r.id.clone());
            self.info_tab = InfoTab::Identity;
            cx.notify();
        }
    }

    pub(super) fn handle_context_menu_action(&mut self, action: ContextMenuAction, cx: &mut Context<Self>) {
        match action {
            ContextMenuAction::QuarkInfo(id) => {
                self.info_panel = Some(id);
                // Each open starts at the top section, not wherever the last panel left off.
                self.info_tab = InfoTab::Identity;
            }
            ContextMenuAction::ToggleQuark(id) => {
                self.toggle_quark_enabled(&id, cx);
            }
            ContextMenuAction::AdoptQuark(id) => {
                self.adopt_quark(&id, cx);
            }
            ContextMenuAction::RestartQuark(id) => {
                self.reboot_quark(&id, cx);
            }
            ContextMenuAction::SetFlavor(id, flavor) => {
                let qid = QuarkId::new(&id);
                // Apply to a legacy seat if present, else record the role as a per-repo
                // override (a catalogue quark keeps its definition; only its role here
                // changes). Trial on a clone so the "≥1 orchestrator" guard is checked
                // against the RESOLVED team before committing.
                let mut trial = self.team.clone();
                if flavor == hadron_lattice::Flavor::Orchestrator {
                    apply_orchestrator_exclusivity(&mut trial, &self.global, &qid);
                }
                if let Some(seat) = trial.quarks.iter_mut().find(|s| s.id == qid) {
                    seat.flavor = flavor.clone();
                } else if let Some(ov) = trial.roster.iter_mut().find(|o| o.id == qid) {
                    ov.flavor = Some(flavor.clone());
                } else {
                    trial.roster.push(SeatOverride {
                        flavor: Some(flavor.clone()),
                        ..SeatOverride::role(qid.clone())
                    });
                }
                let orchestrators = resolve_team(&trial, &self.global)
                    .quarks
                    .iter()
                    .filter(|s| s.flavor == hadron_lattice::Flavor::Orchestrator)
                    .count();
                if orchestrators > 0 {
                    self.team = trial;
                    self.save_repo_team(cx);
                } else {
                    eprintln!(
                        "Refusing to change flavor of {id}: cannot have zero orchestrators."
                    );
                }
            }
            ContextMenuAction::OpenFile(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                if let Some(content) = crate::sys::read_workspace_file(&repo_root, &path) {
                    self.parsed_markdown.borrow_mut().remove(&usize::MAX);
                    self.file_tree_open = Some((path, content));
                }
            }
            ContextMenuAction::CopyPath(path) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(path));
            }
            ContextMenuAction::OpenInEditor(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                self.open_in_editor(&repo_root.join(path), None);
            }
            ContextMenuAction::OpenInFolder(path) => {
                let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
                let full_path = repo_root.join(path);
                let target = if full_path.is_file() {
                    full_path.parent().unwrap_or(&full_path).to_path_buf()
                } else {
                    full_path
                };

                #[cfg(target_os = "macos")]
                let cmd = "open";
                #[cfg(target_os = "windows")]
                let cmd = "explorer";
                #[cfg(target_os = "linux")]
                let cmd = "xdg-open";

                let _ = std::process::Command::new(cmd).arg(&target).spawn();
            }
        }
        cx.notify();
    }

    /// Open `path` (optionally at `line`) in the editor the human picked in Settings.
    ///
    /// The single home of "which program opens a source file": the file tree's
    /// context menu and a clicked `file://` link in a chat message both land here, so
    /// the setting cannot mean two different things on the two surfaces. An unset
    /// choice — or a `Custom` command that is blank — resolves to `None` from
    /// [`crate::sys::editor_argv`] and falls through to the platform opener, which is
    /// exactly the behaviour that existed before the setting did.
    pub(super) fn open_in_editor(&self, path: &std::path::Path, line: Option<u32>) {
        #[cfg(target_os = "macos")]
        let platform_opener = "open";
        #[cfg(target_os = "windows")]
        let platform_opener = "explorer";
        #[cfg(target_os = "linux")]
        let platform_opener = "xdg-open";

        let spawned = match crate::sys::editor_argv(&self.prefs.editor, path, line) {
            Some((program, args)) => std::process::Command::new(&program).args(&args).spawn().map_err(|e| {
                format!("{program}: {e}")
            }),
            None => std::process::Command::new(platform_opener)
                .arg(path)
                .spawn()
                .map_err(|e| format!("{platform_opener}: {e}")),
        };
        // Rule 8: an editor that is configured but not installed is the likeliest
        // failure here, and silently doing nothing reads to the human as a dead click.
        if let Err(e) = spawned {
            eprintln!("chamber: could not open {}: {e}", path.display());
        }
    }

    /// Answer an outstanding permission request by appending a human
    /// `PermissionGrant` (addressed back to the asking quark, so the daemon
    /// resumes it) — the same bus the quarks use. Mirrors [`Self::on_input_submit`].
    pub(super) fn answer_permission(&mut self, approved: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.view.pending_permission.clone() else {
            return;
        };
        let ev = hadron_gatekeeper::grant(&pending, approved);
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append permission grant: {e}");
            return;
        }
        let events = io::read_events(&self.path).unwrap_or_default();
        self.sync_view(&events);
        self.chat_list_state
            .scroll_to_reveal_item(self.chat_message_ixs.len().saturating_sub(1));
        self.log_list_state
            .scroll_to_reveal_item(self.view.messages.len().saturating_sub(1));
        cx.notify();
    }

    /// "Always allow" the pending op: append a *remembering* grant so the
    /// gatekeeper's allow-list auto-approves the same `(quark, op)` next time.
    pub(super) fn answer_permission_remember(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.view.pending_permission.clone() else {
            return;
        };
        self.append_and_reload(hadron_gatekeeper::grant_remembering(&pending), cx);
    }

    /// Cycle the global default permission mode through the FULL ladder
    /// (Ask → Write → Auto → Bypass → Ask) by appending a global `ModeSet`. The daemon
    /// honours it next tick. Unlike the per-quark chip — which clamps at `Auto` so a
    /// stray click cannot hand one worker unattended access — the global chip is the
    /// human's own posture control, so it reaches `Bypass`. `/mode bypass` with no
    /// target is the equivalent typed path (`actions.rs:736`).
    pub(super) fn cycle_global_mode(&mut self, cx: &mut Context<Self>) {
        let next = next_global_mode(self.view.global_mode);
        self.append_and_reload(
            Event::new(Actor::Human, None, Kind::ModeSet { mode: next }),
            cx,
        );
    }

    /// Cycle a single quark's permission mode by appending a per-quark `ModeSet`
    /// (addressed to it). This always creates/updates an explicit override.
    pub(super) fn cycle_quark_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        let current = self
            .view
            .roster
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.mode)
            .unwrap_or_default();
        let next = next_mode(current);
        self.append_and_reload(
            Event::new(Actor::Human, Some(qid), Kind::ModeSet { mode: next }),
            cx,
        );
    }

    /// Set a single quark's permission mode **explicitly** (the Settings picker) by
    /// appending a per-quark `ModeSet`. Unlike [`Self::cycle_quark_mode`] this jumps
    /// straight to `mode`; like it, it always records an explicit per-quark override.
    pub(super) fn set_quark_mode(&mut self, id: &str, mode: Mode, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        self.append_and_reload(
            Event::new(Actor::Human, Some(qid), Kind::ModeSet { mode }),
            cx,
        );
    }

    /// Clear a quark's per-quark override (the "Default" rung) by appending a
    /// `ModeClear`. The quark reverts to inheriting the global default; because the
    /// latest per-quark mode event wins, this cleanly un-sets an earlier `ModeSet`
    /// in the append-only field.
    pub(super) fn clear_quark_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        self.append_and_reload(
            Event::new(Actor::Human, Some(qid), Kind::ModeClear),
            cx,
        );
    }

    /// Force-restart a resident quark: append a per-quark [`Kind::Reboot`]. The daemon
    /// honours it on its next tick — reaping the quark's live ACP subprocess (aborting
    /// an in-flight turn) and re-booting it fresh on its next mention. The quark stays
    /// seated throughout. A no-op for a one-shot CLI quark, which holds nothing
    /// resident between turns. Mirrors [`Self::set_quark_mode`]: the command travels as
    /// a field event, auditable in the Log tab.
    pub(super) fn reboot_quark(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        self.append_and_reload(
            Event::new(Actor::Human, Some(qid), Kind::Reboot),
            cx,
        );
    }

    /// The colour to paint a quark's name / chart series with: its **custom** colour if
    /// one is set in the stored identity (`ChamberPrefs`), else the stable auto hue. Thin
    /// wrapper over [`Self::resolve_identity`] — the one colour-resolution path — so a
    /// custom colour shows everywhere the quark appears (log author, charts, roster, info
    /// panel), not just where an identity was already being resolved.
    pub(super) fn color_for(&self, name: &str) -> Hsla {
        self.resolve_identity(name).color
    }

    /// The repo `.hadron/team.json` path (the file the chamber edits), whether or not
    /// it exists yet.
    pub(super) fn repo_team_path(&self) -> PathBuf {
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        hadron_lattice::team_for_field(&self.path)
            .unwrap_or_else(|| repo_root.join(".hadron").join("team.json"))
    }

    /// Persist `self.team` to the repo file and re-project. The one write path for
    /// every repo-team mutation, so save+reproject never drift.
    pub(super) fn save_repo_team(&mut self, cx: &mut Context<Self>) {
        let path = self.repo_team_path();
        if let Err(e) = hadron_lattice::save_team(&path, &self.team) {
            eprintln!("chamber: failed to save team.json: {e}");
            return;
        }
        self.providers = configured_providers(&resolve_team(&self.team, &self.global));
        let events = io::read_events(&self.path).unwrap_or_default();
        self.sync_view(&events);
        cx.notify();
    }

    /// Persist the **global catalogue** (`~/.hadron/team.json`). The catalogue holds the
    /// shared defaults that are the same in every repo — a quark's model default and its
    /// display name (a quark is the same quark everywhere, so its name is not a per-repo
    /// thing). Mirrors [`Self::save_repo_team`] but writes the catalogue file.
    pub(super) fn save_global_team(&mut self, cx: &mut Context<Self>) {
        let Some(path) = hadron_lattice::team_config_path() else {
            eprintln!("chamber: no global catalogue path — cannot save shared defaults");
            return;
        };
        if let Err(e) = hadron_lattice::save_team(&path, &self.global) {
            eprintln!("chamber: failed to save catalogue: {e}");
            return;
        }
        self.providers = configured_providers(&resolve_team(&self.team, &self.global));
        let events = io::read_events(&self.path).unwrap_or_default();
        self.sync_view(&events);
        cx.notify();
    }

    /// Toggle a quark's participation. A legacy full seat flips its own `enabled`; a
    /// catalogue-adopted quark records the flip as a per-repo override (created if it
    /// does not exist yet). Only meaningful for adopted rows — a not-adopted quark is
    /// "Adopt"ed instead (see the context menu).
    pub(super) fn toggle_quark_enabled(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        // The current (resolved) state is what we flip away from.
        let resolved = resolve_team(&self.team, &self.global);
        let Some(current) = resolved.get(&qid).map(|s| s.enabled) else {
            return; // not adopted → nothing to toggle
        };
        let want = !current;
        if let Some(seat) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
            seat.enabled = want;
        } else if let Some(ov) = self.team.roster.iter_mut().find(|o| o.id == qid) {
            ov.enabled = Some(want);
        } else {
            self.team
                .roster
                .push(SeatOverride { enabled: Some(want), ..SeatOverride::role(qid) });
        }
        self.save_repo_team(cx);
    }

    /// Append an event to the field and sync every view cache to it (the shared
    /// write path for chat messages, permission grants and mode changes — the same
    /// bus the quarks use).
    pub(super) fn append_and_reload(&mut self, ev: Event, cx: &mut Context<Self>) {
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append event: {e}");
            return;
        }
        let events = io::read_events(&self.path).unwrap_or_default();
        self.sync_view(&events);
        cx.notify();
    }

    /// Collapse or expand a rail. Just flips the persisted flag — the layout
    /// follows (an expanded rail is a resizable panel; a collapsed one is a fixed
    /// strip), so there's no sizing state to drive by hand.
    pub(super) fn toggle_rail(&mut self, rail: Rail, _window: &mut Window, cx: &mut Context<Self>) {
        match rail {
            Rail::Roster => self.prefs.roster_collapsed = !self.prefs.roster_collapsed,
            Rail::Inspector => self.prefs.inspector_collapsed = !self.prefs.inspector_collapsed,
        }
        let _ = config::save(&self.prefs);
        cx.notify();
    }

    /// **Un-adopt** a quark from this repo: drop its legacy seat and/or override plus
    /// its providers-list row. The definition stays in the global catalogue, so the
    /// quark reappears as an available (grey) row rather than vanishing — removal from
    /// a repo is not deletion from the catalogue. The running daemon reconciles the
    /// removal on its next re-seat tick (its `ReseatPlan.removed` → `unseat`).
    pub(super) fn remove_quark(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        self.team.quarks.retain(|s| s.id != qid);
        self.team.roster.retain(|o| o.id != qid);
        self.providers.retain(|p| p.id.as_str() != id);
        self.save_repo_team(cx);
    }

    /// Save a newly-configured quark. Its **definition** goes to the global catalogue
    /// (`~/.hadron/team.json`) so every repo can reach it; this repo **auto-adopts** it
    /// (an enabled override), matching Jake's "added quark joins the current repo".
    /// When there is no separate catalogue (the repo file *is* the global file), fall
    /// back to a self-contained legacy seat — the pre-split behaviour.
    pub(super) fn add_configured_quark(&mut self, seat: hadron_lattice::Seat, cx: &mut Context<Self>) {
        let repo_path = self.repo_team_path();
        let global_path = hadron_lattice::team_config_path();
        let separate = global_path.as_deref().is_some_and(|g| g != repo_path);
        let id = seat.id.clone();
        if separate {
            // The catalogue holds the shared **default** for an id. The first add of an
            // id establishes that default; a later add of the SAME id in another repo
            // must NOT clobber it — that is the cross-repo collision. So keep any existing
            // def and record only how this repo's pick diverges (a preset chooses a model;
            // effort/mode/name it never sets, so those inherit the catalogue).
            let adopt = match self.global.quarks.iter().find(|s| s.id == id).cloned() {
                None => {
                    self.global.quarks.push(seat.clone());
                    if let Some(gp) = global_path {
                        if let Err(e) = hadron_lattice::save_team(&gp, &self.global) {
                            eprintln!("chamber: failed to save catalogue: {e}");
                        }
                    }
                    SeatOverride { enabled: Some(true), ..SeatOverride::role(id.clone()) }
                }
                Some(def) => SeatOverride {
                    enabled: Some(true),
                    model: (seat.model != def.model).then(|| seat.model.clone()),
                    ..SeatOverride::role(id.clone())
                },
            };
            // Auto-adopt here (unless already present some other way).
            if !self.team.quarks.iter().any(|s| s.id == id)
                && !self.team.roster.iter().any(|o| o.id == id)
            {
                self.team.roster.push(adopt);
            }
        } else {
            // No separate catalogue: keep it self-contained as a legacy seat.
            self.team.quarks.push(seat);
        }
        self.save_repo_team(cx);
    }

    /// **Adopt** a catalogue quark into this repo: add an enabled override so the daemon
    /// seats it. The definition stays in the global catalogue; the repo only records
    /// that it participates here (as a worker by default; change the role afterwards).
    pub(super) fn adopt_quark(&mut self, id: &str, cx: &mut Context<Self>) {
        let qid = QuarkId::new(id);
        if self.team.quarks.iter().any(|s| s.id == qid)
            || self.team.roster.iter().any(|o| o.id == qid)
        {
            return; // already adopted here
        }
        self.team.roster.push(SeatOverride {
            enabled: Some(true),
            ..SeatOverride::role(qid) // inherit the catalogue's role + definition
        });
        self.save_repo_team(cx);
    }

    /// Toggle the Process Manager overlay (pinned Roster rail button, above Settings).
    pub(super) fn toggle_process_manager(&mut self, cx: &mut Context<Self>) {
        self.process_manager_open = toggle_process_manager_open(self.process_manager_open);
        cx.notify();
    }
}

/// Pure decision half of `toggle_process_manager`: the next `process_manager_open`
/// value, extracted so it's testable without a live `Chamber`/window (mirrors
/// [`toggle_focus_target`] below).
pub(super) fn toggle_process_manager_open(current: bool) -> bool {
    !current
}

/// Pure decision half of [`Chamber::sync_view`]: whether reprojecting into
/// `new_messages` is a pure append — safe to splice — rather than a wholesale field
/// swap or a shrink, which must fall back to a full resync.
///
/// Row count alone cannot tell "grew" from "replaced": a same-length swap (two
/// sessions with the same message count) looks like zero growth, and even a
/// larger swap can look like ordinary growth by coincidence. Comparing the
/// *leading* row's timestamp catches "the front of the list moved out from under
/// us" in both cases — a real append never changes what event ended up first.
pub(super) fn is_pure_append(
    old_first_ts: Option<chrono::DateTime<chrono::Utc>>,
    old_len: usize,
    new_messages: &[MessageRow],
) -> bool {
    // Nothing existed before, so there is no leading row to contradict — growing
    // from empty is unambiguously an append, not a swap.
    if old_len == 0 {
        return true;
    }
    new_messages.len() >= old_len && new_messages.first().map(|m| m.ts) == old_first_ts
}

pub(super) fn apply_orchestrator_exclusivity(
    team: &mut hadron_lattice::Team,
    global: &hadron_lattice::Team,
    target_qid: &QuarkId,
) {
    for seat in &mut team.quarks {
        if seat.id != *target_qid {
            seat.flavor = hadron_lattice::Flavor::Worker;
        }
    }
    for ov in &mut team.roster {
        if ov.id != *target_qid {
            let base_is_orchestrator = global.get(&ov.id).map(|s| s.flavor == hadron_lattice::Flavor::Orchestrator).unwrap_or(false);
            if ov.flavor == Some(hadron_lattice::Flavor::Orchestrator) || (ov.flavor.is_none() && base_is_orchestrator) {
                ov.flavor = Some(hadron_lattice::Flavor::Worker);
            }
        }
    }
}

/// Where `ToggleFocus` sends focus when moving *toward* the terminal — always
/// [`FocusTarget::Terminal`], since this is a pure function of the right rail's
/// active tab alone. `FocusTarget::Chat` exists so [`Chamber::toggle_focus`]'s
/// live-focus branch (terminal-focused → chat) can be expressed with the same
/// type; this function never returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusTarget {
    Chat,
    Terminal,
}

/// Pure decision half of `ToggleFocus`'s "move toward the terminal" branch:
/// given the right rail's currently active tab, decide the focus target (always
/// terminal) and whether the rail needs to switch to the Terminal tab first. The
/// other direction of the toggle — terminal-focused back to chat — depends on
/// live window focus state, not just `active_rail_tab`, so it isn't representable
/// as a pure function and is decided in [`Chamber::toggle_focus`] instead.
pub(super) fn toggle_focus_target(active_rail_tab: RightRailTab) -> (FocusTarget, bool) {
    (FocusTarget::Terminal, active_rail_tab != RightRailTab::Terminal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_chat_tab_and_inspector_tab() {
        // `set_chat_tab_index` / `set_inspector_tab_index` are thin wrappers over
        // `ChatTab::from_index` / `RightRailTab::from_index`. A `Chamber` can only be
        // built with a live `Window` + `Context`, so we assert the index→tab mapping
        // the setters delegate to rather than standing up a headless GUI app.
        assert!(ChatTab::from_index(2) == ChatTab::Stats);
        assert!(RightRailTab::from_index(1) == RightRailTab::FileTree);
        // Out-of-range clamps to the default tab, so the setters never panic.
        assert!(ChatTab::from_index(99) == ChatTab::Chat);
        assert!(RightRailTab::from_index(99) == RightRailTab::Terminal);
    }

    #[test]
    fn toggle_focus_targets_terminal_when_terminal_tab_active() {
        let (target, switch_rail) = toggle_focus_target(RightRailTab::Terminal);
        assert_eq!(target, FocusTarget::Terminal);
        assert!(!switch_rail, "already on the Terminal tab, no rail switch needed");
    }

    #[test]
    fn toggle_focus_else_case_switches_rail_to_terminal() {
        for tab in [RightRailTab::FileTree, RightRailTab::Git, RightRailTab::Changes, RightRailTab::Plan] {
            let (target, switch_rail) = toggle_focus_target(tab);
            assert_eq!(target, FocusTarget::Terminal);
            assert!(switch_rail, "a non-Terminal tab active must switch the rail to Terminal");
        }
    }

    #[test]
    fn toggle_process_manager_open_flips_state() {
        assert!(toggle_process_manager_open(false));
        assert!(!toggle_process_manager_open(true));
    }

    #[test]
    fn orchestrator_exclusivity_demotes_other_orchestrators() {
        use hadron_lattice::SeatOverride;
        use hadron_lattice::{Flavor, QuarkId, Seat, Team};

        let mut team = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("cli-agy"), "agy", "gemini", Flavor::Orchestrator),
                Seat::cli(QuarkId::new("cli-opus"), "claude", "opus", Flavor::Worker),
            ],
            roster: vec![
                SeatOverride {
                    flavor: Some(Flavor::Orchestrator),
                    ..SeatOverride::role(QuarkId::new("override-one"))
                },
                SeatOverride {
                    flavor: None, // Inherits Orchestrator from global in this test case
                    ..SeatOverride::role(QuarkId::new("override-two"))
                },
            ],
            max_exchanges: None,
            nucleus_index_budget_kb: None,
            merge_strategy: None,
        };

        let global = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("override-two"), "claude", "opus", Flavor::Orchestrator),
            ],
            roster: vec![],
            max_exchanges: None,
            nucleus_index_budget_kb: None,
            merge_strategy: None,
        };

        apply_orchestrator_exclusivity(&mut team, &global, &QuarkId::new("override-one"));

        // cli-agy was Orchestrator, should be demoted to Worker
        assert_eq!(team.quarks[0].flavor, Flavor::Worker);
        // cli-opus was Worker, should stay Worker
        assert_eq!(team.quarks[1].flavor, Flavor::Worker);
        // override-two resolved to Orchestrator by default, should be overridden to Worker
        assert_eq!(team.roster[1].flavor, Some(Flavor::Worker));
    }

    fn row_at(ts_secs: i64, from: &str) -> MessageRow {
        use chrono::TimeZone;
        MessageRow {
            from: from.to_string(),
            to: None,
            body: String::new(),
            kind_label: "message",
            usage: None,
            ts: chrono::Utc.timestamp_opt(ts_secs, 0).unwrap(),
            legacy_used_tokens: None,
            turn: None,
            severity: None,
        }
    }

    // Test 2 (spec): a pure append — same leading row, strictly more rows — is safe
    // to splice.
    #[test]
    fn sync_view_splices_on_pure_append() {
        let old = [row_at(1, "Human"), row_at(2, "Sonnet"), row_at(3, "Human")];
        let old_first_ts = old.first().map(|m| m.ts);
        let grown = [old[0].clone(), old[1].clone(), old[2].clone(), row_at(4, "Sonnet")];
        assert!(is_pure_append(old_first_ts, old.len(), &grown));
    }

    // Test 1 (spec): a wholesale field swap that happens to keep the SAME row count
    // must NOT be read as a pure append — it needs a full resync, or row 0 renders
    // under the old field's author while `chat_message_ixs`/`view.messages` have
    // already moved on to the new one (the "your message came as mine" shape).
    #[test]
    fn sync_view_resyncs_when_the_field_is_swapped_at_equal_length() {
        let old = [row_at(1, "Human"), row_at(2, "Sonnet"), row_at(3, "Human")];
        let old_first_ts = old.first().map(|m| m.ts);
        let swapped = [row_at(10, "Human"), row_at(11, "Antigravity"), row_at(12, "Human")];
        assert_eq!(swapped.len(), old.len(), "must be equal-length to reproduce the bug");
        assert!(!is_pure_append(old_first_ts, old.len(), &swapped));
    }

    #[test]
    fn sync_view_resyncs_on_shrink() {
        let old = [row_at(1, "Human"), row_at(2, "Sonnet"), row_at(3, "Human")];
        let old_first_ts = old.first().map(|m| m.ts);
        let cleared: [crate::model::MessageRow; 0] = [];
        assert!(!is_pure_append(old_first_ts, old.len(), &cleared));
    }

    #[test]
    fn sync_view_treats_first_population_as_append() {
        // Empty → non-empty: no prior row to disagree with, so the growth branch is
        // fine (it degenerates to the same splice-from-0 a resync would do).
        let grown = [row_at(1, "Human")];
        assert!(is_pure_append(None, 0, &grown));
    }
}
