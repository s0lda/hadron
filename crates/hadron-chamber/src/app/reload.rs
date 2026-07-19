use super::*;

impl super::Chamber {
    /// Re-project the field into the roster/log/session view, resolving the repo team
    /// against the global catalogue first so adopted quarks carry their full defs and
    /// available-but-not-adopted catalogue quarks show greyed. The one place the view
    /// is rebuilt, so every mutation path routes through the same resolve.
    pub(super) fn reproject(&mut self, events: &[Event]) {
        let resolved = resolve_team(&self.team, &self.global);
        self.view = model::project_with_team(events, &resolved, &self.global);
        self.update_active_plan();
    }

    pub(super) fn update_active_plan(&mut self) {
        let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let active_plan_path = self.view
            .messages
            .iter()
            .rev()
            .find_map(|m| {
                hadron_gluon::skills::plan_ref(&m.body)
                    .filter(|rel_path| repo.join(rel_path).is_file())
            });

        if let Some(rel_path) = active_plan_path {
            let path_str = rel_path.clone();
            if self.last_plan_path.as_deref() != Some(&path_str) {
                self.last_plan_path = Some(path_str);
                if let Some(content) = crate::sys::read_workspace_file(&repo, &rel_path) {
                    let tasks = parse_plan_tasks(&content);
                    if let Some((first_incomplete_task, _)) = tasks
                        .iter()
                        .find(|(_, steps)| steps.iter().any(|(_, done)| !*done))
                    {
                        self.plan_collapsed_tasks.remove(first_incomplete_task);
                    }
                }
            }
        } else {
            self.last_plan_path = None;
        }
    }

    /// Re-read the field; if it grew, re-project and repaint. Comparing event
    /// count to the current row count is a cheap change check (projection emits
    /// exactly one row per event), so an unchanged field costs only a read.
    pub(super) fn reload_if_changed(&mut self, cx: &mut Context<Self>) {
        // Only reproject on a successful read — a transient read error must not
        // blank the current view (which would flash to empty, then repopulate).
        if let Ok(events) = io::read_events(&self.path) {
            let mut changed = false;

            // The team files are edited out-of-band, not only through this window: the
            // daemon re-seats from team.json, and a quark is adopted/removed by writing
            // team.json (or the shared catalogue) directly. Poll them the same dumb way
            // the field is polled, so an externally added/removed quark shows in the
            // roster + Settings at once — instead of only after it first authors an
            // event (event-seeding), which is why a new quark used to "appear after
            // mention". A no-op after this window's own save: the reload matches what
            // was just written, so `!=` is false and nothing reprojects.
            //
            // STRICT read on purpose: `load_team` degrades a missing/half-written file to
            // an EMPTY team. Assigning that would blank the roster for a frame and — if a
            // settings save landed in the next ~400ms tick — persist an empty team.json,
            // which the polling daemon would read as "unseat the whole swarm". So parse
            // with the error kept and, on ANY read/parse failure, leave the in-memory team
            // untouched (mirrors the events guard above; `save_team`'s atomic rename is
            // what protects concurrent *writers*, this protects concurrent *readers*).
            let read_strict = |path: &std::path::Path| -> Option<Team> {
                std::fs::read_to_string(path)
                    .ok()
                    .and_then(|t| hadron_lattice::parse_team(&t).ok())
            };
            let repo_team = read_strict(&self.repo_team_path());
            let global_team = match hadron_lattice::team_config_path() {
                Some(p) => read_strict(&p),
                None => Some(Team::default()), // no catalogue configured → genuinely empty
            };
            let mut team_changed = false;
            if let (Some(repo_team), Some(global_team)) = (repo_team, global_team) {
                if repo_team != self.team || global_team != self.global {
                    self.team = repo_team;
                    self.global = global_team;
                    self.providers =
                        configured_providers(&resolve_team(&self.team, &self.global));
                    team_changed = true;
                    changed = true;
                }
            }

            if events.len() != self.view.messages.len() {
                // Decide *before* the content grows: if the user is parked at the
                // bottom, keep them there as the new message lands; if they've
                // scrolled up to read history, leave their position alone.
                let follow = self.chat_at_bottom();
                let old_log_count = self.view.messages.len();
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
                let new_log_count = self.view.messages.len();

                if new_chat_count > old_chat_count {
                    self.chat_list_state.splice(
                        old_chat_count..old_chat_count,
                        new_chat_count - old_chat_count,
                    );
                } else if new_chat_count < old_chat_count {
                    // Should not happen since field is append-only, but just in case
                    self.chat_list_state.reset(new_chat_count);
                }

                if new_log_count > old_log_count {
                    self.log_list_state.splice(
                        old_log_count..old_log_count,
                        new_log_count - old_log_count,
                    );
                } else if new_log_count < old_log_count {
                    self.log_list_state.reset(new_log_count);
                }

                if follow {
                    for scroll in &self.chat_scrolls {
                        scroll.scroll_to_bottom();
                    }
                    self.chat_list_state
                        .scroll_to_reveal_item(new_chat_count.saturating_sub(1));
                    self.log_list_state
                        .scroll_to_reveal_item(new_log_count.saturating_sub(1));
                }
                changed = true;
            } else if team_changed {
                // The message list is unchanged (same events) but the resolved team is
                // not — refresh the view so the new roster/Settings render. No
                // message-count change, so the virtualized chat/log lists need no splice.
                self.reproject(&events);
            }
            if self.right_rail_tab == RightRailTab::Changes {
                let root = crate::vcs::repo_root_of(&self.path);
                let diff = crate::vcs::working_diff(root);
                if diff != self.working_diff {
                    self.working_diff = diff;
                    changed = true;
                }
            }
            // The file tree is a live view of the disk, not a boot-time snapshot:
            // rescan while it is on screen, exactly as the Changes pane does.
            // Rescan files unconditionally so autocomplete mentions are always live,
            // regardless of which right rail tab is active.
            let root = crate::vcs::repo_root_of(&self.path);
            let files = crate::sys::list_workspace_files(root);
            if files != self.file_tree_paths {
                // Autocomplete offers only real, editable files — never muted gitignored
                // entries — mirroring the filter in `new`.
                *self.completion_files.borrow_mut() = files
                    .iter()
                    .filter(|(_, ignored)| !ignored)
                    .map(|(p, _)| p.clone())
                    .collect();
                self.file_tree_paths = files;
                changed = true;
            }
            if changed {
                cx.notify();
            }
        }
    }
}
