//! Command dispatch and state-mutation handlers for the chamber: chat `/commands`,
//! tab/window cycling, roster selection, context-menu actions, per-quark and global
//! permission-mode changes, reboot/enable/adopt/remove, permission answers, and the
//! team-file persistence behind them. The `&mut self` verbs the UI invokes, split from
//! the view code that calls them.

use super::*;

impl Chamber {
    pub(super) fn handle_chat_command(
        &mut self,
        cmd: &str,
        args: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match cmd {
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
                let hadron_dir = match self.path.parent() {
                    Some(p) => p.to_path_buf(),
                    None => std::path::PathBuf::from(".hadron"),
                };
                let session_dir = hadron_dir.join("sessions").join(&session_id);
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
                        // wider Stats windows. `/clear` is the only writer of a new archive
                        // in this process, so this is the one place the cache must rebuild.
                        self.archived_messages =
                            crate::model::load_archived_messages(&hadron_dir.join("sessions"));
                        self.chat_message_ixs.clear();
                        self.chat_list_state.reset(0);
                        for scroll in &self.chat_scrolls {
                            scroll.scroll_to_bottom();
                        }
                        cx.notify();
                    }
                }
                true
            }
            "team-brainstorm" => {
                let body = format!("@team Let's brainstorm. {args}").trim().to_string();
                let ev = Event::new(Actor::Human, None, Kind::Message { body });
                if let Err(e) = io::append_event(&self.path, &ev) {
                    eprintln!("chamber: failed to append team-brainstorm message: {e}");
                } else {
                    let events = io::read_events(&self.path).unwrap_or_default();
                    self.reproject(&events);
                    
                    let old_chat_count = self.chat_message_ixs.len();
                    self.chat_message_ixs = self
                        .view
                        .messages
                        .iter()
                        .enumerate()
                        .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
                        .collect();
                    let new_chat_count = self.chat_message_ixs.len();
                    if new_chat_count > old_chat_count {
                        self.chat_list_state.splice(
                            old_chat_count..old_chat_count,
                            new_chat_count - old_chat_count,
                        );
                    }
                    for scroll in &self.chat_scrolls {
                        scroll.scroll_to_bottom();
                    }
                    self.chat_list_state.scroll_to_reveal_item(new_chat_count.saturating_sub(1));
                    cx.notify();
                }
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
                    let matches_target = self.view.roster.iter().any(|r| {
                        (r.id == target || r.display_name.as_deref() == Some(target))
                            && matches!(r.transport, hadron_lattice::Transport::Acp)
                    });
                    if matches_target {
                        let real_id = self.view.roster.iter()
                            .find(|r| r.id == target || r.display_name.as_deref() == Some(target))
                            .map(|r| &r.id)
                            .unwrap();
                        vec![Event::new(Actor::Human, Some(QuarkId::new(real_id)), Kind::Reboot)]
                    } else {
                        eprintln!("chamber: `/reboot` target not found or not a resident quark: {target}");
                        vec![]
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
                        .find(|r| r.id == t || r.display_name.as_deref() == Some(t))
                        .map(|r| r.id.clone())
                });

                if let Some(real_id) = target_id {
                    let qid = QuarkId::new(&real_id);
                    if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
                        existing.energy_limit = Some(limit_val);
                    } else if let Some(def) = self.global.get(&qid).cloned() {
                        let resolved = resolve_team(&self.team, &self.global);
                        if let Some(base) = resolved.get(&qid) {
                            let mut desired = base.clone();
                            desired.energy_limit = Some(limit_val);
                            let prev = self.team.roster.iter().find(|o| o.id == qid).cloned();
                            let ov = hadron_lattice::seat_override_delta(qid.clone(), &def, &desired, prev.as_ref());
                            self.team.roster.retain(|o| o.id != qid);
                            self.team.roster.push(ov);
                        }
                    }
                    self.save_repo_team(cx);
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
                            .find(|r| r.id == target || r.display_name.as_deref() == Some(target))
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
        for tab in [RightRailTab::FileTree, RightRailTab::Changes, RightRailTab::Plan] {
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
