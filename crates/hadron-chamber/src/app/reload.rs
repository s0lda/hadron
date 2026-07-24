use super::*;

#[derive(Debug, Clone)]
pub(super) struct WorkspaceScan {
    pub(super) files: Vec<(String, bool)>,
    pub(super) git_statuses: std::collections::HashMap<String, crate::vcs::GitStatus>,
    pub(super) working_diff: Option<Vec<crate::vcs::FileDiff>>,
}

impl WorkspaceScan {
    pub(super) fn gather(
        repo_root: &std::path::Path,
        file_tree_expanded: &std::collections::HashSet<String>,
        scan_diff: bool,
    ) -> Option<Self> {
        let files = crate::sys::list_workspace_files(repo_root, file_tree_expanded);
        let git_statuses = crate::vcs::get_git_statuses(repo_root);
        let working_diff = if scan_diff {
            crate::vcs::working_diff(repo_root)
        } else {
            None
        };
        Some(Self {
            files,
            git_statuses,
            working_diff,
        })
    }
}


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
                    .filter(|rel_path| {
                        let full_path = repo.join(rel_path);
                        if let (Ok(canon_repo), Ok(canon_full)) = (repo.canonicalize(), full_path.canonicalize()) {
                            canon_full.is_file() && canon_full.starts_with(canon_repo)
                        } else {
                            false
                        }
                    })
            });

        if let Some(rel_path) = active_plan_path {
            let path_str = rel_path.clone();
            let path_changed = self.last_plan_path.as_deref() != Some(&path_str);
            if path_changed {
                self.last_plan_path = Some(path_str);
            }
            if let Some(content) = crate::sys::read_workspace_file(&repo, &rel_path) {
                let tasks = parse_plan_tasks(&content);
                Self::calculate_collapsed_tasks(
                    &tasks,
                    &mut self.plan_collapsed_tasks,
                    &mut self.last_incomplete_task,
                    path_changed,
                );
            }
        } else {
            self.last_plan_path = None;
            self.last_incomplete_task = None;
        }
    }

    fn calculate_collapsed_tasks(
        tasks: &[(String, Vec<(String, bool)>)],
        plan_collapsed_tasks: &mut std::collections::HashSet<String>,
        last_incomplete_task: &mut Option<String>,
        path_changed: bool,
    ) {
        if path_changed {
            plan_collapsed_tasks.clear();
            for (task_name, _) in tasks {
                plan_collapsed_tasks.insert(task_name.clone());
            }
            if let Some((first_incomplete_task, _)) = tasks
                .iter()
                .find(|(_, steps)| steps.iter().any(|(_, done)| !*done))
            {
                plan_collapsed_tasks.remove(first_incomplete_task);
                *last_incomplete_task = Some(first_incomplete_task.clone());
            } else {
                *last_incomplete_task = None;
            }
        } else {
            if let Some((first_incomplete_task, _)) = tasks
                .iter()
                .find(|(_, steps)| steps.iter().any(|(_, done)| !*done))
            {
                if last_incomplete_task.as_ref() != Some(first_incomplete_task) {
                    if let Some(old_task) = last_incomplete_task.take() {
                        plan_collapsed_tasks.insert(old_task);
                    }
                    plan_collapsed_tasks.remove(first_incomplete_task);
                    *last_incomplete_task = Some(first_incomplete_task.clone());
                }
            }
        }
    }

    /// Re-read the field; if it grew, re-project and repaint. Comparing event
    /// count to the current row count is a cheap change check (projection emits
    /// exactly one row per event), so an unchanged field costs only a read.
    ///
    /// `scan` carries the workspace state the tick gathered **off** the render
    /// thread ([`WorkspaceScan::gather`]). It is `Option` because the scan can
    /// fail to be produced (the entity went away mid-tick); `None` leaves the
    /// file tree / git statuses exactly as they are rather than blanking them.
    pub(super) fn reload_if_changed(
        &mut self,
        scan: Option<WorkspaceScan>,
        cx: &mut Context<Self>,
    ) {
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
            // The file tree is a live view of the disk, not a boot-time snapshot, and
            // autocomplete mentions must stay live regardless of which rail tab is up —
            // so the scan runs every tick. It just no longer runs *here*: it is three
            // `git` subprocesses plus a `stat` per tracked file, and on the render
            // thread that blocked mouse/keyboard dispatch for the whole scan.
            if let Some(scan) = scan {
                if scan.files != self.file_tree_paths {
                    // Autocomplete offers only real, editable files — never muted gitignored
                    // entries — mirroring the filter in `new`.
                    *self.completion_files.borrow_mut() = scan
                        .files
                        .iter()
                        .filter(|(_, ignored)| !ignored)
                        .map(|(p, _)| p.clone())
                        .collect();
                    self.file_tree_paths = scan.files;
                    changed = true;
                }

                if scan.git_statuses != self.git_statuses {
                    self.git_statuses = scan.git_statuses;
                    changed = true;
                }

                // Not-scanned is "the Changes pane wasn't on screen", not "there is no
                // diff" — leave the stored diff alone then, exactly as the old inline
                // `if tab == Changes` guard did.
                if scan.working_diff.is_some() && scan.working_diff != self.working_diff {
                    self.working_diff = scan.working_diff;
                    changed = true;
                }
            }

            let live_dir = hadron_lattice::live::live_dir(&self.path);
            let mut live_activity_changed = false;
            for r in &self.view.roster {
                let activity = hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new(&r.id), chrono::Utc::now());
                if self.last_live_activities.get(&r.id) != Some(&activity) {
                    self.last_live_activities.insert(r.id.clone(), activity);
                    live_activity_changed = true;
                }
            }
            if live_activity_changed {
                changed = true;
            }

            // Critical, must-notice event: gluon holds the daemon lock exclusively, so
            // a stopped daemon means no quark in the swarm can take a turn until it's
            // restarted. Fire the banner only on the running→stopped edge (not every
            // tick it stays down) and clear it on stopped→running.
            let gluon_now_running = self.gluon_running();
            if let Some(went_running) = gluon_running_edge(self.last_gluon_running, gluon_now_running) {
                self.last_gluon_running = gluon_now_running;
                self.gluon_stopped_notice = !went_running;
                changed = true;
            }

            if changed {
                cx.notify();
            }
        }
    }
}

/// Whether `last → now` crosses a gluon running/stopped edge: `Some(now)` on a
/// transition (so the caller re-toasts once, not every poll it stays down),
/// `None` on steady state.
fn gluon_running_edge(last: bool, now: bool) -> Option<bool> {
    (last != now).then_some(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gluon_running_edge_fires_only_on_transitions() {
        assert_eq!(gluon_running_edge(true, false), Some(false)); // running → stopped
        assert_eq!(gluon_running_edge(false, true), Some(true)); // stopped → running
        assert_eq!(gluon_running_edge(true, true), None); // steady running, no re-toast
        assert_eq!(gluon_running_edge(false, false), None); // steady stopped, no re-toast
    }

    #[test]
    fn test_calculate_collapsed_tasks_path_changed() {
        let tasks = vec![
            ("Task 1".to_string(), vec![("Step 1".to_string(), true), ("Step 2".to_string(), false)]),
            ("Task 2".to_string(), vec![("Step 3".to_string(), false)]),
        ];
        let mut plan_collapsed_tasks = std::collections::HashSet::new();
        let mut last_incomplete_task = None;

        Chamber::calculate_collapsed_tasks(&tasks, &mut plan_collapsed_tasks, &mut last_incomplete_task, true);

        assert_eq!(plan_collapsed_tasks.len(), 1);
        assert!(plan_collapsed_tasks.contains("Task 2"));
        assert!(!plan_collapsed_tasks.contains("Task 1"));
        assert_eq!(last_incomplete_task, Some("Task 1".to_string()));
    }

    #[test]
    fn test_calculate_collapsed_tasks_transition() {
        let tasks = vec![
            ("Task 1".to_string(), vec![("Step 1".to_string(), true), ("Step 2".to_string(), true)]),
            ("Task 2".to_string(), vec![("Step 3".to_string(), false)]),
        ];
        let mut plan_collapsed_tasks = std::collections::HashSet::new();
        plan_collapsed_tasks.insert("Task 2".to_string());
        let mut last_incomplete_task = Some("Task 1".to_string());

        Chamber::calculate_collapsed_tasks(&tasks, &mut plan_collapsed_tasks, &mut last_incomplete_task, false);

        assert_eq!(plan_collapsed_tasks.len(), 1);
        assert!(plan_collapsed_tasks.contains("Task 1"));
        assert!(!plan_collapsed_tasks.contains("Task 2"));
        assert_eq!(last_incomplete_task, Some("Task 2".to_string()));
    }

    #[test]
    fn test_calculate_collapsed_tasks_no_change() {
        let tasks = vec![
            ("Task 1".to_string(), vec![("Step 1".to_string(), true), ("Step 2".to_string(), false)]),
            ("Task 2".to_string(), vec![("Step 3".to_string(), false)]),
        ];
        let mut plan_collapsed_tasks = std::collections::HashSet::new();
        plan_collapsed_tasks.insert("Task 2".to_string());
        let mut last_incomplete_task = Some("Task 1".to_string());

        Chamber::calculate_collapsed_tasks(&tasks, &mut plan_collapsed_tasks, &mut last_incomplete_task, false);

        assert_eq!(plan_collapsed_tasks.len(), 1);
        assert!(plan_collapsed_tasks.contains("Task 2"));
        assert!(!plan_collapsed_tasks.contains("Task 1"));
        assert_eq!(last_incomplete_task, Some("Task 1".to_string()));
    }
}
