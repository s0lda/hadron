use std::path::Path;
use super::*;

#[derive(Debug, Clone)]
pub(super) struct WorkspaceScan {
    pub(super) files: Vec<(String, bool)>,
    pub(super) git_statuses: std::collections::HashMap<String, crate::vcs::GitStatus>,
    pub(super) working_diff: Option<Vec<crate::vcs::FileDiff>>,
    pub(super) git_branch_fingerprint: Option<String>,
}

impl WorkspaceScan {
    pub(super) fn gather(
        repo_root: &std::path::Path,
        file_tree_expanded: &std::collections::HashSet<String>,
        scan_diff: bool,
        scan_git: bool,
    ) -> Option<Self> {
        let files = crate::sys::list_workspace_files(repo_root, file_tree_expanded);
        let git_statuses = crate::vcs::get_git_statuses(repo_root);
        let working_diff = if scan_diff {
            crate::vcs::working_diff(repo_root)
        } else {
            None
        };
        let git_branch_fingerprint = if scan_git {
            Some(crate::vcs::branch_fingerprint(repo_root))
        } else {
            None
        };
        Some(Self {
            files,
            git_statuses,
            working_diff,
            git_branch_fingerprint,
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
        let alias_map: std::collections::HashMap<String, hadron_lattice::QuarkId> = self
            .view
            .roster
            .iter()
            .flat_map(|r| {
                let qid = hadron_lattice::QuarkId::new(&r.id);
                let mut pairs = vec![(r.id.clone(), qid.clone())];
                if let Some(ref dn) = r.display_name {
                    pairs.push((dn.clone(), qid.clone()));
                }
                if r.id.contains("agy") || r.id.contains("orchestrator") {
                    pairs.push(("orchestrator".to_string(), qid));
                }
                pairs
            })
            .collect();
        self.delegations = delegation::parse_delegations(events, &alias_map);
        self.update_active_plan();
    }

    pub(super) fn update_active_plan(&mut self) {
        let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let start_ix = self.manual_plan_override_at_message_len.unwrap_or(0);
        let recent_msgs = if start_ix < self.view.messages.len() {
            &self.view.messages[start_ix..]
        } else {
            &[]
        };

        let new_plan_from_msg = if !recent_msgs.is_empty() {
            resolve_active_plan_from_messages(&repo, recent_msgs)
        } else {
            None
        };

        let active_plan_path = if let Some(p) = new_plan_from_msg {
            self.manual_plan_override_at_message_len = None;
            Some(p)
        } else if let Some(ref manual) = self.last_plan_path {
            if repo.join(manual).is_file() {
                Some(manual.clone())
            } else {
                resolve_active_plan(&repo, &self.view.messages)
            }
        } else {
            resolve_active_plan(&repo, &self.view.messages)
        };

        if let Some(rel_path) = active_plan_path {
            let path_str = rel_path.clone();
            let path_changed = self.last_plan_path.as_deref() != Some(&path_str);
            if path_changed {
                self.last_plan_path = Some(path_str);
            }
            if let Some(content) = crate::sys::read_workspace_file(&repo, &rel_path) {
                let tasks = parse_plan_tasks(&content);
                // The Tasks tab titles a row by the plan task its dispatch names. The
                // parse happens here, once per reload, rather than in `model` (which is
                // pure over `&[Event]` and compiled without `gui`) or in the render
                // pass (which runs on every hover — nucleus `a-render-fn-runs-on-every-hover`).
                let headings: Vec<String> = tasks.iter().map(|(name, _)| name.clone()).collect();
                crate::model::tasks::retitle_from_plan(
                    &mut self.view.tasks,
                    &rel_path,
                    &headings,
                );
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

            let root = crate::vcs::repo_root_of(&self.path);
            let is_over = hadron_gluon::nucleus_status::index_over_budget(
                root,
                hadron_gluon::nucleus_status::resolve_budget_bytes(&self.team),
            );
            if is_over != self.nucleus_over_budget {
                self.nucleus_over_budget = is_over;
                changed = true;
            }

            if events.len() != self.view.messages.len() {
                self.sync_view(&events);
                self.chat_list_state
                    .scroll_to_reveal_item(self.chat_message_ixs.len().saturating_sub(1));
                self.log_list_state
                    .scroll_to_reveal_item(self.view.messages.len().saturating_sub(1));
                changed = true;
            } else if team_changed {
                // The message list is unchanged (same events) but the resolved team is
                // not — refresh the view so the new roster/Settings render. No
                // message-count change, so `sync_view` degenerates to a zero-delta splice.
                self.sync_view(&events);
            }
            // The file tree is a live view of the disk, not a boot-time snapshot, and
            // autocomplete mentions must stay live regardless of which rail tab is up —
            // so the scan runs every tick. It just no longer runs *here*: it is three
            // `git` subprocesses plus a `stat` per tracked file, and on the render
            // thread that blocked mouse/keyboard dispatch for the whole scan.
            if let Some(scan) = scan {
                if scan.files != self.file_tree_paths {
                    // Gitignored entries included, mirroring `new` — see the note there.
                    *self.completion_files.borrow_mut() = scan
                        .files
                        .iter()
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

                if scan.git_branch_fingerprint.is_some() && scan.git_branch_fingerprint != self.git_branch_fingerprint {
                    self.git_branch_fingerprint = scan.git_branch_fingerprint;
                    let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
                    self.git_branches = Some(crate::vcs::list_branches(&repo, "main"));
                    self.git_worktrees = Some(crate::vcs::list_worktrees(&repo));
                    self.git_log_graph = crate::vcs::commit_graph(&repo);
                    self.rebuild_graph_rows();
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
            let gluon_activity = hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new("gluon"), chrono::Utc::now());
            if self.last_live_activities.get("gluon") != Some(&gluon_activity) {
                self.last_live_activities.insert("gluon".to_string(), gluon_activity);
                live_activity_changed = true;
            }
            if live_activity_changed {
                changed = true;
            }

            // Gate heartbeats live outside the roster loop above (keyed by branch, not
            // quark id — `live::gates_dir`), so the Tasks tab needs its own change
            // detection to repaint when a gate starts, keeps running, or finishes.
            let gates_dir = hadron_lattice::live::gates_dir(&self.path);
            let gate_activities = hadron_lattice::live::gates(&gates_dir, chrono::Utc::now());
            if gate_activities != self.last_gate_activities {
                self.last_gate_activities = gate_activities;
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

            if self.toast_manager.prune(std::time::Instant::now()) {
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
/// Resolve the active implementation plan for a workspace:
/// 1. Newest-first scan of messages for an explicit plan file reference (`plan_ref`) or mention that exists on disk.
/// 2. If no plan is explicitly referenced in messages, scan `.hadron/docs/plans/`, `docs/plans/`, and worktrees for the newest plan on disk.
pub(crate) fn resolve_active_plan(repo: &Path, messages: &[crate::model::MessageRow]) -> Option<String> {
    resolve_active_plan_from_messages(repo, messages).or_else(|| scan_newest_plan(repo))
}

/// Newest-first scan of messages for an explicit or mentioned plan file path that exists on disk.
pub(crate) fn resolve_active_plan_from_messages(repo: &Path, messages: &[crate::model::MessageRow]) -> Option<String> {
    messages.iter().rev().find_map(|m| {
        find_plan_mention_in_text(repo, &m.body)
    })
}

fn resolve_plan_path(repo: &Path, raw_path: &str) -> Option<String> {
    let clean = raw_path
        .strip_prefix("file://")
        .or_else(|| raw_path.strip_prefix("file:"))
        .unwrap_or(raw_path);
    let clean = crate::vcs::strip_unc_prefix(clean);

    let joined = repo.join(&clean);
    if joined.is_file() {
        if let (Ok(canon_repo), Ok(canon_full)) = (repo.canonicalize(), joined.canonicalize()) {
            if canon_full.starts_with(&canon_repo) {
                if let Ok(rel) = canon_full.strip_prefix(&canon_repo) {
                    return Some(rel.to_string_lossy().to_string());
                }
            }
        }
        if let Ok(rel) = joined.strip_prefix(repo) {
            return Some(rel.to_string_lossy().to_string());
        }
        return Some(clean);
    }
    let abs_p = Path::new(&clean);
    if abs_p.is_file() {
        if let (Ok(canon_repo), Ok(canon_abs)) = (repo.canonicalize(), abs_p.canonicalize()) {
            if canon_abs.starts_with(&canon_repo) {
                if let Ok(rel) = canon_abs.strip_prefix(&canon_repo) {
                    return Some(rel.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn find_plan_mention_in_text(repo: &Path, text: &str) -> Option<String> {
    if let Some(p) = hadron_gluon::skills::plan_ref(text) {
        if let Some(resolved) = resolve_plan_path(repo, &p) {
            return Some(resolved);
        }
    }

    let all_plans = scan_all_plan_paths(repo);
    if all_plans.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']' || c == '<' || c == '>' || c == '"' || c == '\'' || c == '`')
        .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/' && c != '.' && c != '-' && c != '_'))
        .filter(|t| !t.is_empty())
        .collect();

    // 1. Direct path/filename match (e.g. "@master.md", "01-phase-1-reactive-orchestration-and-dag.md")
    for &tok in &tokens {
        let clean_tok = tok.strip_prefix('@').unwrap_or(tok);
        if clean_tok.ends_with(".md") {
            if let Some((_, rel)) = all_plans.iter().find(|(_, rel)| {
                rel == clean_tok || rel.ends_with(&format!("/{clean_tok}")) || rel.ends_with(clean_tok)
            }) {
                return Some(rel.clone());
            }
        }
    }

    // 2. Substring / fuzzy folder or filename match (e.g. "swarm-orchestration-and-teamwork/master.md" or typo in date "2026008-21-swarm...")
    for &tok in &tokens {
        let clean_tok = tok.strip_prefix('@').unwrap_or(tok);
        if clean_tok.contains("plan") || clean_tok.contains("swarm") || clean_tok.contains("capabilities") || clean_tok.ends_with(".md") {
            let needle = clean_tok.replace(|c: char| !c.is_ascii_alphanumeric(), "");
            if needle.len() >= 6 {
                if let Some((_, rel)) = all_plans.iter().find(|(_, rel)| {
                    let rel_clean = rel.replace(|c: char| !c.is_ascii_alphanumeric(), "");
                    rel_clean.contains(&needle) || needle.contains(&rel_clean)
                }) {
                    return Some(rel.clone());
                }
            }
        }
    }

    None
}

/// Scan all plan `.md` files across `.hadron/docs/plans/`, `docs/plans/`, and worktrees.
pub(crate) fn scan_all_plan_paths(repo: &Path) -> Vec<(std::time::SystemTime, String)> {
    let mut candidates: Vec<(std::time::SystemTime, String)> = Vec::new();

    let check_dirs = [
        repo.join(".hadron").join("docs").join("plans"),
        repo.join("docs").join("plans"),
    ];

    for dir in &check_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                    if file_name.starts_with('.') {
                        continue;
                    }
                    let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    if let Ok(rel) = path.strip_prefix(repo) {
                        candidates.push((mtime, rel.to_string_lossy().to_string()));
                    }
                } else if path.is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            let sub_path = sub_entry.path();
                            if sub_path.is_file() && sub_path.extension().and_then(|e| e.to_str()) == Some("md") {
                                let sub_file_name = sub_path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                                if sub_file_name.starts_with('.') {
                                    continue;
                                }
                                let mtime = sub_entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                if let Ok(rel) = sub_path.strip_prefix(repo) {
                                    candidates.push((mtime, rel.to_string_lossy().to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let trees_dir = repo.join(".hadron").join("trees");
    if let Ok(tree_entries) = std::fs::read_dir(&trees_dir) {
        for tree_entry in tree_entries.flatten() {
            if tree_entry.path().is_dir() {
                let tree_path = tree_entry.path();
                let tree_plan_dirs = [
                    tree_path.join(".hadron").join("docs").join("plans"),
                    tree_path.join("docs").join("plans"),
                ];
                for dir in &tree_plan_dirs {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                                let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                                if file_name.starts_with('.') {
                                    continue;
                                }
                                let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                if let Ok(rel) = path.strip_prefix(repo) {
                                    candidates.push((mtime, rel.to_string_lossy().to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates
}

/// Scan `.hadron/docs/plans/`, `docs/plans/`, and worktree trees for the most recently modified `.md` plan.
pub(crate) fn scan_newest_plan(repo: &Path) -> Option<String> {
    let mut candidates = scan_all_plan_paths(repo);
    candidates.sort_by(|a, b| {
        let is_master_a = a.1.ends_with("master.md") || a.1.ends_with("index.md");
        let is_master_b = b.1.ends_with("master.md") || b.1.ends_with("index.md");
        b.0.cmp(&a.0)
            .then_with(|| is_master_b.cmp(&is_master_a))
            .then_with(|| b.1.cmp(&a.1))
    });
    candidates.into_iter().next().map(|(_, rel)| rel)
}

pub(crate) fn format_plan_step_label(file_name: &str) -> String {
    if file_name == "master.md" || file_name == "index.md" {
        "Master Plan".to_string()
    } else if let Some(stripped) = file_name.strip_suffix(".md") {
        if let Some(pos) = stripped.find("-phase-") {
            let phase_part = &stripped[pos + 1..];
            let words: Vec<&str> = phase_part.split('-').collect();
            if words.len() >= 2 && words[0] == "phase" {
                if let Ok(n) = words[1].parse::<u32>() {
                    let rest = words[2..].join(" ");
                    if rest.is_empty() {
                        format!("Phase {n}")
                    } else {
                        format!("Phase {n}: {rest}")
                    }
                } else {
                    stripped.replace('-', " ")
                }
            } else {
                stripped.replace('-', " ")
            }
        } else {
            stripped.replace('-', " ")
        }
    } else {
        file_name.to_string()
    }
}

pub(crate) fn format_suite_title(suite: &str) -> String {
    let parts: Vec<&str> = suite.split('-').collect();
    if parts.len() >= 4 && parts[0].len() == 4 && parts[1].len() == 2 && parts[2].len() == 2 {
        let date = format!("{}-{}-{}", parts[0], parts[1], parts[2]);
        let words: Vec<String> = parts[3..]
            .iter()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect();
        let topic = words.join(" ");
        format!("{date} {topic}")
    } else {
        suite.replace('-', " ")
    }
}

/// An item in the Plan selector dropdown menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanDropdownItem {
    /// Section header / category (e.g. `📁 Current: 2026-08-21 Swarm Orchestration And Teamwork`)
    Header(String),
    /// Visual separator line between suites
    Separator,
    /// Selectable plan item
    Plan {
        /// Display label inside the dropdown menu (e.g. `Master Plan`, `Phase 1: Reactive Orchestration And DAG`)
        label: String,
        /// Relative path from repo root (e.g. `.hadron/docs/plans/.../master.md`)
        rel_path: String,
        /// Whether this plan is currently viewed/active
        is_active: bool,
    },
}

/// Discover and organize plans across `.hadron/docs/plans/`, `docs/plans/`, and worktrees into
/// clean suite groups with active suite prioritization, headers, and step labels for the Chamber dropdown.
pub(crate) fn scan_plan_dropdown_items(repo: &Path, active_plan_rel: &str) -> Vec<PlanDropdownItem> {
    let all_paths = scan_all_plan_paths(repo);
    if all_paths.is_empty() {
        return Vec::new();
    }

    #[derive(Debug, Clone)]
    struct PlanEntry {
        mtime: std::time::SystemTime,
        rel: String,
        suite: Option<String>,
        file_name: String,
    }

    let mut entries: Vec<PlanEntry> = Vec::new();
    let active_p = Path::new(active_plan_rel);
    let active_suite = if let Some(parent) = active_p.parent() {
        let parent_name = parent.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if parent_name != "plans" && !parent_name.is_empty() {
            Some(parent_name.to_string())
        } else {
            None
        }
    } else {
        None
    };

    for (mtime, rel) in all_paths {
        let p = Path::new(&rel);
        let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();
        let suite = if let Some(parent) = p.parent() {
            let parent_name = parent.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if parent_name != "plans" && !parent_name.is_empty() {
                Some(parent_name.to_string())
            } else {
                None
            }
        } else {
            None
        };

        entries.push(PlanEntry {
            mtime,
            rel,
            suite,
            file_name,
        });
    }

    // Group by suite
    let mut groups: std::collections::BTreeMap<Option<String>, Vec<PlanEntry>> = std::collections::BTreeMap::new();
    for entry in entries {
        groups.entry(entry.suite.clone()).or_default().push(entry);
    }

    // For each group, find max mtime and sort internal entries: master.md first, then filename
    struct SuiteBucket {
        suite: Option<String>,
        max_mtime: std::time::SystemTime,
        entries: Vec<PlanEntry>,
    }

    let mut buckets: Vec<SuiteBucket> = Vec::new();
    for (suite, mut list) in groups {
        let max_mtime = list.iter().map(|e| e.mtime).max().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        list.sort_by(|a, b| {
            let is_master_a = a.file_name == "master.md" || a.file_name == "index.md";
            let is_master_b = b.file_name == "master.md" || b.file_name == "index.md";
            is_master_b
                .cmp(&is_master_a)
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
        buckets.push(SuiteBucket {
            suite,
            max_mtime,
            entries: list,
        });
    }

    // Sort buckets:
    // 1. Active suite first
    // 2. Other suites sorted by max_mtime descending
    // 3. Root plans (None) last
    buckets.sort_by(|a, b| {
        let a_is_active = a.suite == active_suite && a.suite.is_some();
        let b_is_active = b.suite == active_suite && b.suite.is_some();
        if a_is_active != b_is_active {
            return b_is_active.cmp(&a_is_active);
        }
        if a.suite.is_some() != b.suite.is_some() {
            return b.suite.is_some().cmp(&a.suite.is_some());
        }
        b.max_mtime.cmp(&a.max_mtime)
    });

    let total_suites = buckets.iter().filter(|b| b.suite.is_some()).count();
    let mut dropdown_items = Vec::new();

    for (i, bucket) in buckets.into_iter().enumerate() {
        if i > 0 {
            dropdown_items.push(PlanDropdownItem::Separator);
        }

        let header_title = match &bucket.suite {
            Some(s) => {
                let formatted = format_suite_title(s);
                if bucket.suite == active_suite && total_suites > 1 {
                    format!("📁 Current: {formatted}")
                } else {
                    format!("📁 {formatted}")
                }
            }
            None => {
                if total_suites > 0 {
                    "📁 Other Workspace Plans".to_string()
                } else {
                    "📁 Implementation Plans".to_string()
                }
            }
        };

        dropdown_items.push(PlanDropdownItem::Header(header_title));

        for entry in bucket.entries {
            let label = format_plan_step_label(&entry.file_name);
            let is_active = entry.rel == active_plan_rel;
            dropdown_items.push(PlanDropdownItem::Plan {
                label,
                rel_path: entry.rel,
                is_active,
            });
        }
    }

    dropdown_items
}

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

        assert!(plan_collapsed_tasks.contains("Task 2"));
        assert!(!plan_collapsed_tasks.contains("Task 1"));
        assert_eq!(last_incomplete_task, Some("Task 1".to_string()));
    }

    #[test]
    fn test_scan_newest_plan_finds_latest_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let hadron_plans = root.join(".hadron").join("docs").join("plans");
        std::fs::create_dir_all(&hadron_plans).unwrap();

        let plan1 = hadron_plans.join("2026-08-01-old-plan.md");
        std::fs::write(&plan1, "# Old Plan\n- [x] Step 1\n").unwrap();

        let plan2 = hadron_plans.join("2026-08-14-new-plan.md");
        std::fs::write(&plan2, "# New Plan\n- [ ] Step 1\n").unwrap();

        let found = scan_newest_plan(root);
        assert!(found.is_some());
        let found_str = found.unwrap();
        assert!(found_str.contains("2026-08-14-new-plan.md"), "expected new plan, got {found_str}");

        // Now test resolve_active_plan with messages referencing the older plan explicitly
        let messages = vec![
            crate::model::MessageRow {
                from: "Human".to_string(),
                to: None,
                body: "Execute `2026-08-01-old-plan.md` in .hadron/docs/plans/2026-08-01-old-plan.md".to_string(),
                kind_label: "message",
                usage: None,
                ts: chrono::Utc::now(),
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            }
        ];
        let resolved = resolve_active_plan(root, &messages);
        assert!(resolved.is_some());
        assert!(resolved.unwrap().contains("2026-08-01-old-plan.md"));
    }

    #[test]
    fn test_scan_plan_dropdown_items_single_suite() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let subfolder = root.join(".hadron").join("docs").join("plans").join("2026-08-19-test-plan");
        std::fs::create_dir_all(&subfolder).unwrap();

        let master = subfolder.join("master.md");
        std::fs::write(&master, "# Master\n- [ ] Step 1\n").unwrap();

        let p1 = subfolder.join("01-phase-1-setup.md");
        std::fs::write(&p1, "# Phase 1\n- [ ] Step 1\n").unwrap();

        let p2 = subfolder.join("02-phase-2-build.md");
        std::fs::write(&p2, "# Phase 2\n- [ ] Step 1\n").unwrap();

        let active_rel = ".hadron/docs/plans/2026-08-19-test-plan/master.md";
        let items = scan_plan_dropdown_items(root, active_rel);

        let plan_items: Vec<_> = items.iter().filter_map(|item| match item {
            PlanDropdownItem::Plan { label, rel_path, is_active } => Some((label.as_str(), rel_path.as_str(), *is_active)),
            _ => None,
        }).collect();

        assert_eq!(plan_items.len(), 3);
        assert_eq!(plan_items[0], ("Master Plan", ".hadron/docs/plans/2026-08-19-test-plan/master.md", true));
        assert_eq!(plan_items[1], ("Phase 1: setup", ".hadron/docs/plans/2026-08-19-test-plan/01-phase-1-setup.md", false));
        assert_eq!(plan_items[2], ("Phase 2: build", ".hadron/docs/plans/2026-08-19-test-plan/02-phase-2-build.md", false));
    }

    #[test]
    fn test_multi_suite_plan_scanning_and_switching() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let suite1 = root.join(".hadron").join("docs").join("plans").join("2026-08-19-twenty-capabilities");
        let suite2 = root.join(".hadron").join("docs").join("plans").join("2026-08-21-swarm-orchestration");
        std::fs::create_dir_all(&suite1).unwrap();
        std::fs::create_dir_all(&suite2).unwrap();

        std::fs::write(suite1.join("master.md"), "# Twenty Caps Master\n").unwrap();
        std::fs::write(suite1.join("01-phase-1-quick-dx.md"), "# Phase 1\n").unwrap();
        std::fs::write(suite2.join("master.md"), "# Swarm Master\n").unwrap();
        std::fs::write(suite2.join("01-phase-1-reactive-dag.md"), "# Phase 1 DAG\n").unwrap();

        let active_rel = ".hadron/docs/plans/2026-08-19-twenty-capabilities/master.md";
        let items = scan_plan_dropdown_items(root, active_rel);

        let plan_items: Vec<_> = items.iter().filter_map(|item| match item {
            PlanDropdownItem::Plan { rel_path, .. } => Some(rel_path.as_str()),
            _ => None,
        }).collect();

        assert_eq!(plan_items.len(), 4);

        // Active suite comes first
        assert!(plan_items[0].contains("2026-08-19-twenty-capabilities/master.md"));
        assert!(plan_items[1].contains("2026-08-19-twenty-capabilities/01-phase-1-quick-dx.md"));
        // Second suite is also discoverable and selectable
        assert!(plan_items[2].contains("2026-08-21-swarm-orchestration/master.md"));
        assert!(plan_items[3].contains("2026-08-21-swarm-orchestration/01-phase-1-reactive-dag.md"));
    }

    #[test]
    fn test_scan_plan_dropdown_items_structure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let suite1 = root.join(".hadron").join("docs").join("plans").join("2026-08-21-swarm-orchestration-and-teamwork");
        let suite2 = root.join(".hadron").join("docs").join("plans").join("2026-08-19-twenty-capabilities");
        std::fs::create_dir_all(&suite1).unwrap();
        std::fs::create_dir_all(&suite2).unwrap();

        std::fs::write(suite1.join("master.md"), "# Swarm Master\n").unwrap();
        std::fs::write(suite1.join("01-phase-1-reactive-dag.md"), "# Phase 1 DAG\n").unwrap();
        std::fs::write(suite2.join("master.md"), "# Twenty Caps Master\n").unwrap();

        let active_rel = ".hadron/docs/plans/2026-08-21-swarm-orchestration-and-teamwork/01-phase-1-reactive-dag.md";
        let items = scan_plan_dropdown_items(root, active_rel);

        // First item is Current header for suite 1
        match &items[0] {
            PlanDropdownItem::Header(h) => {
                assert!(h.contains("Current:"), "header should mark current suite: {h}");
                assert!(h.contains("Swarm Orchestration And Teamwork"), "header title: {h}");
            }
            other => panic!("expected Header, got {other:?}"),
        }

        // Next items are master.md and 01-phase-1
        match &items[1] {
            PlanDropdownItem::Plan { label, is_active, .. } => {
                assert_eq!(label, "Master Plan");
                assert!(!is_active);
            }
            other => panic!("expected Plan, got {other:?}"),
        }

        match &items[2] {
            PlanDropdownItem::Plan { label, is_active, .. } => {
                assert_eq!(label, "Phase 1: reactive dag");
                assert!(is_active, "01-phase-1 should be active");
            }
            other => panic!("expected Plan, got {other:?}"),
        }

        // Separator between suites
        assert_eq!(items[3], PlanDropdownItem::Separator);

        // Suite 2 header
        match &items[4] {
            PlanDropdownItem::Header(h) => {
                assert!(h.contains("Twenty Capabilities"), "header title: {h}");
            }
            other => panic!("expected Header, got {other:?}"),
        }
    }

    #[test]
    fn test_plan_resolution_from_mentions_and_urls() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let suite = root.join(".hadron").join("docs").join("plans").join("2026-08-21-swarm-orchestration-and-teamwork");
        std::fs::create_dir_all(&suite).unwrap();
        let master = suite.join("master.md");
        std::fs::write(&master, "# Swarm Master\n").unwrap();

        // 1. file:// URL resolution
        let file_url = format!("file://{}", master.display());
        let messages = vec![
            crate::model::MessageRow {
                from: "Agy".to_string(),
                to: None,
                body: format!("Check [{}]({}) for details", master.display(), file_url),
                kind_label: "message",
                usage: None,
                ts: chrono::Utc::now(),
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            }
        ];
        let resolved = resolve_active_plan(root, &messages);
        assert!(resolved.is_some());
        assert!(resolved.unwrap().contains("2026-08-21-swarm-orchestration-and-teamwork/master.md"));

        // 2. @mention resolution
        let messages_mention = vec![
            crate::model::MessageRow {
                from: "Human".to_string(),
                to: None,
                body: "@.hadron/docs/plans/2026-08-21-swarm-orchestration-and-teamwork/master.md".to_string(),
                kind_label: "message",
                usage: None,
                ts: chrono::Utc::now(),
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            }
        ];
        let resolved_mention = resolve_active_plan(root, &messages_mention);
        assert!(resolved_mention.is_some());
        assert!(resolved_mention.unwrap().contains("2026-08-21-swarm-orchestration-and-teamwork/master.md"));

        // 3. Typo-tolerant folder mention resolution (e.g. "2026008-21-swarm...")
        let messages_typo = vec![
            crate::model::MessageRow {
                from: "Human".to_string(),
                to: None,
                body: "@.hadron/docs/plans/2026008-21-swarm-orchestration-and-teamwork/master.md".to_string(),
                kind_label: "message",
                usage: None,
                ts: chrono::Utc::now(),
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            }
        ];
        let resolved_typo = resolve_active_plan(root, &messages_typo);
        assert!(resolved_typo.is_some());
        assert!(resolved_typo.unwrap().contains("2026-08-21-swarm-orchestration-and-teamwork/master.md"));
    }
}
