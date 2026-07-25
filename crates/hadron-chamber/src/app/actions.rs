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
    /// calling `io::append_event` again here — one write path into the field.
    pub(super) fn post_chat_message(&mut self, from: Actor, body: String, cx: &mut Context<Self>) {
        let old_chat_count = self.chat_message_ixs.len();
        self.append_and_reload(Event::new(from, None, Kind::Message { body }), cx);

        self.chat_message_ixs = crate::model::chat_message_indices(&self.view.messages);
        let new_chat_count = self.chat_message_ixs.len();
        // A failed append leaves the count unchanged, so this splices nothing —
        // the error is already reported by `append_and_reload`.
        if new_chat_count > old_chat_count {
            self.chat_list_state
                .splice(old_chat_count..old_chat_count, new_chat_count - old_chat_count);
        }
        for scroll in &self.chat_scrolls {
            scroll.scroll_to_bottom();
        }
        self.chat_list_state
            .scroll_to_reveal_item(new_chat_count.saturating_sub(1));
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
                        self.reproject(&events);
                        // The just-archived field is now part of history: fold it into the
                        // wider Stats windows and offer it in the Sessions submenu.
                        self.reload_archives();
                        self.resync_lists_to_projection();
                        for scroll in &self.chat_scrolls {
                            scroll.scroll_to_bottom();
                        }
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
            "team-brainstorm" | "brainstorm" | "writing-plans" | "executing-plans" => {
                let skill_id = match cmd {
                    "team-brainstorm" | "brainstorm" => "brainstorming",
                    "writing-plans" => "writing-plans",
                    _ => "executing-plans",
                };
                let Some(trigger) = hadron_gluon::skills::canonical_trigger(skill_id) else {
                    // Unreachable unless someone removes a skill from the engine's
                    // corpus. Say so rather than post a message that selects nothing.
                    eprintln!(
                        "chamber: `/{cmd}` found no skill `{skill_id}` in the engine — not posting"
                    );
                    return true;
                };
                let (target, task) = if cmd == "team-brainstorm" {
                    (hadron_gluon::router::TEAM_ALIAS, args.trim())
                } else {
                    let (named, task) = crate::text::split_target(args);
                    (named.unwrap_or(hadron_gluon::router::ORCHESTRATOR_ALIAS), task)
                };
                match crate::text::skill_command_body(trigger, target, task) {
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
            // Discovery. Both print from `Actor::Gluon` with no `@mention`, which
            // reaches no seat: `next_pending` skips `to: None` (`router/mod.rs:37`)
            // and `unaddressed_message_targets` resolves mentions in the body, of
            // which there are none. Answering "what can I type" must not cost a turn.
            "help" => {
                self.post_chat_message(Actor::Gluon, crate::text::help_body(), cx);
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
                self.reproject(&events);
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
                self.reproject(&events);
                self.reload_archives();
                self.resync_lists_to_projection();
                for scroll in &self.chat_scrolls {
                    scroll.scroll_to_bottom();
                }
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
                self.reproject(&events);
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
                self.reproject(&events);
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
                self.reproject(&events);
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
                self.reproject(&events);
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
                    // Two writes, in this order: the note holds the fact, the index
                    // holds a pointer to it and nothing else. An index line whose
                    // note does not exist is the worse failure of the two, so the
                    // note is written first and a failure there skips the pointer.
                    let slug = crate::text::slugify(text);
                    let hook = crate::text::hook(text);
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
                    } else if let Err(e) = append_line(
                        &nucleus.join("index.md"),
                        &crate::text::learn_line(&slug, &hook),
                    ) {
                        eprintln!("chamber: failed to write index.md: {e}");
                    }
                }
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

    /// Cycle the right rail's tab (Terminal/Files/Changes/Plan) by `delta`, wrapping.
    pub(super) fn cycle_inspector_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = RightRailTab::ALL.len() as isize;
        let cur = self.right_rail_tab.index() as isize;
        self.right_rail_tab = RightRailTab::from_index((cur + delta).rem_euclid(n) as usize);
        cx.notify();
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
                let full_path = repo_root.join(path);

                #[cfg(target_os = "macos")]
                let default_cmd = "open";
                #[cfg(target_os = "windows")]
                let default_cmd = "explorer";
                #[cfg(target_os = "linux")]
                let default_cmd = "xdg-open";

                let editor = std::env::var("EDITOR").unwrap_or_else(|_| default_cmd.to_string());
                let _ = std::process::Command::new(&editor).arg(&full_path).spawn();
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
        self.reproject(&events);
        for scroll in &self.chat_scrolls {
            scroll.scroll_to_bottom();
        }
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

    /// Cycle the global default permission mode (Ask → Write → Auto → Bypass →
    /// Ask) by appending a global `ModeSet`. The daemon honours it next tick.
    pub(super) fn cycle_global_mode(&mut self, cx: &mut Context<Self>) {
        let next = next_mode(self.view.global_mode);
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
        self.reproject(&events);
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
        self.reproject(&events);
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

    /// Append an event to the field and re-project the view (the shared write
    /// path for permission grants and mode changes — the same bus the quarks use).
    pub(super) fn append_and_reload(&mut self, ev: Event, cx: &mut Context<Self>) {
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append event: {e}");
            return;
        }
        let events = io::read_events(&self.path).unwrap_or_default();
        self.reproject(&events);
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
        };

        let global = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("override-two"), "claude", "opus", Flavor::Orchestrator),
            ],
            roster: vec![],
            max_exchanges: None,
        };

        apply_orchestrator_exclusivity(&mut team, &global, &QuarkId::new("override-one"));

        // cli-agy was Orchestrator, should be demoted to Worker
        assert_eq!(team.quarks[0].flavor, Flavor::Worker);
        // cli-opus was Worker, should stay Worker
        assert_eq!(team.quarks[1].flavor, Flavor::Worker);
        // override-two resolved to Orchestrator by default, should be overridden to Worker
        assert_eq!(team.roster[1].flavor, Some(Flavor::Worker));
    }
}
