use std::path::Path;
use super::*;

#[derive(Debug, Clone)]
pub(super) struct WorkspaceScan {
    pub(super) files: Vec<(String, bool)>,
    pub(super) git_ready: bool,
    pub(super) overdue_schedule_jobs: Vec<(String, String, String, String)>,
    pub(super) git_statuses: std::collections::HashMap<String, crate::vcs::GitStatus>,
    pub(super) working_diff: Option<Vec<crate::vcs::FileDiff>>,
    pub(super) git_branch_fingerprint: Option<String>,
    pub(super) git_branches: Option<Vec<crate::vcs::BranchInfo>>,
    pub(super) git_remote_branches: Option<Vec<crate::vcs::BranchInfo>>,
    pub(super) git_worktrees: Option<Vec<crate::vcs::WorktreeInfo>>,
    pub(super) git_log_graph: Option<String>,
    pub(super) selected_branch_diff: Option<(String, Option<Vec<crate::vcs::FileDiff>>)>,
    pub(super) repo_health: Option<hadron_gatekeeper::RepoHealthReport>,
    pub(super) remote_tracking: Option<crate::vcs::RemoteTrackingInfo>,
    pub(super) plan_paths: Vec<(std::time::SystemTime, String)>,
    pub(super) active_plan_content: Option<(String, String)>,
    pub(super) plan_landed_commits: std::collections::HashSet<String>,
    pub(super) nucleus_docs: (Option<String>, Option<String>, Option<String>),
    pub(super) nucleus_fingerprint: u64,
}

/// Resolve which commit hashes referenced by an active plan have landed in `main`.
/// Runs off the UI thread (in `WorkspaceScan::gather`, `refresh_mission_plan`, or `Chamber::new`).
pub(super) fn collect_plan_landed_commits(
    repo_root: &Path,
    content: &str,
) -> std::collections::HashSet<String> {
    let mut candidates = crate::app::mentions::collect_plan_commit_hashes(content);
    let graph = hadron_lattice::task_graph::TaskGraph::parse_from_markdown(content);
    for task in &graph.tasks {
        if let Some(ref hash) = task.commit_hash {
            candidates.insert(hash.clone());
        }
    }
    candidates
        .into_iter()
        .filter(|hash| crate::vcs::is_commit_merged(repo_root, hash, "main"))
        .collect()
}

pub(super) fn gather_overdue_schedule_jobs(
    repo_root: &std::path::Path,
) -> Vec<(String, String, String, String)> {
    if !repo_root
        .join(".hadron")
        .join("schedules")
        .join("schedules.json")
        .is_file()
    {
        return Vec::new();
    }
    let Ok(reg) = hadron_lattice::ScheduleRegistry::load(repo_root) else {
        return Vec::new();
    };
    let now = chrono::Utc::now();
    let evaluations = reg.evaluate_due_jobs(repo_root, now, 3600);
    let mut jobs = Vec::new();
    for (idx, status) in evaluations {
        if let hadron_lattice::DueStatus::OverdueAsk { due_at } = status {
            let job = &reg.jobs[idx];
            jobs.push((
                job.id.clone(),
                job.name.clone(),
                job.task_id.clone(),
                due_at.format("%Y-%m-%d %H:%M").to_string(),
            ));
        }
    }
    jobs
}

impl WorkspaceScan {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn gather(
        repo_root: &std::path::Path,
        file_tree_expanded: &std::collections::HashSet<String>,
        scan_diff: bool,
        scan_git: bool,
        scan_health: bool,
        verified_git_ready: bool,
        prev_branch_fingerprint: Option<&str>,
        active_plan_hint: Option<&str>,
        selected_branch: Option<&str>,
        selected_worktree_path: Option<&str>,
    ) -> Option<Self> {
        let files = crate::sys::list_workspace_files(repo_root, file_tree_expanded);
        let git_ready = if verified_git_ready && repo_root.join(".git").exists() {
            true
        } else {
            crate::vcs::is_git_repo(repo_root) && crate::vcs::has_commits(repo_root)
        };
        let overdue_schedule_jobs = gather_overdue_schedule_jobs(repo_root);
        let git_statuses = crate::vcs::get_git_statuses(repo_root);
        let working_diff = if scan_diff {
            crate::vcs::working_diff(repo_root)
        } else {
            None
        };
        let (git_branch_fingerprint, git_branches, git_remote_branches, git_worktrees, git_log_graph) =
            if scan_git {
                let fp = crate::vcs::branch_fingerprint(repo_root);
                if prev_branch_fingerprint != Some(fp.as_str()) {
                    let branches = Some(crate::vcs::list_branches(repo_root, "main"));
                    let remote_branches =
                        Some(crate::vcs::list_remote_branches(repo_root, "main"));
                    let worktrees = Some(crate::vcs::list_worktrees(repo_root));
                    let graph = crate::vcs::commit_graph(repo_root);
                    (Some(fp), branches, remote_branches, worktrees, graph)
                } else {
                    (Some(fp), None, None, None, None)
                }
            } else {
                (None, None, None, None, None)
            };
        let selected_branch_diff = if scan_diff {
            let rotated_branch = selected_worktree_path.and_then(|wt_path| {
                git_worktrees
                    .as_deref()
                    .and_then(|wts| wts.iter().find(|wt| wt.path == wt_path))
                    .and_then(|wt| wt.branch.clone())
            });
            let active_branch = rotated_branch
                .or_else(|| selected_branch.map(str::to_string));
            active_branch.map(|b| {
                let diff = crate::vcs::branch_diff(repo_root, "main", &b);
                (b, diff)
            })
        } else {
            None
        };
        let repo_health = if scan_health {
            Some(hadron_gatekeeper::RepoMonitor::check_repo(repo_root))
        } else {
            None
        };
        let remote_tracking = if scan_git || scan_health {
            Some(crate::vcs::get_remote_tracking_info(repo_root))
        } else {
            None
        };
        let plan_paths = scan_all_plan_paths(repo_root);
        let target_plan_rel = active_plan_hint
            .map(str::to_string)
            .or_else(|| newest_plan_from_list_with_mtime(&plan_paths).map(|(_, p)| p));
        let active_plan_content = target_plan_rel.and_then(|rel| {
            crate::sys::read_workspace_file(repo_root, &rel).map(|body| (rel, body))
        });
        if scan_git && git_branch_fingerprint.as_deref() != prev_branch_fingerprint {
            crate::vcs::clear_unmerged_commit_cache();
        }
        let plan_landed_commits = active_plan_content
            .as_ref()
            .map(|(_, body)| collect_plan_landed_commits(repo_root, body))
            .unwrap_or_default();
        let nucleus_dir = repo_root.join(".hadron").join("nucleus");
        let nucleus_docs = (
            std::fs::read_to_string(nucleus_dir.join("features.md")).ok(),
            std::fs::read_to_string(nucleus_dir.join("invariants").join("always.md")).ok(),
            std::fs::read_to_string(nucleus_dir.join("index.md")).ok(),
        );
        let nucleus_fingerprint =
            crate::app::render::visualizer::compute_nucleus_graph_fingerprint(repo_root);
        Some(Self {
            files,
            git_ready,
            overdue_schedule_jobs,
            git_statuses,
            working_diff,
            git_branch_fingerprint,
            git_branches,
            git_remote_branches,
            git_worktrees,
            git_log_graph,
            selected_branch_diff,
            repo_health,
            remote_tracking,
            plan_paths,
            active_plan_content,
            plan_landed_commits,
            nucleus_docs,
            nucleus_fingerprint,
        })
    }
}

#[derive(Clone, Default)]
pub(super) struct FieldWriteQueue {
    epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pending: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    queue: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Box<dyn FnOnce() + Send + 'static>>>>,
    draining: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for FieldWriteQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldWriteQueue")
            .field("epoch", &self.current_epoch())
            .field("pending", &self.has_pending_writes())
            .finish()
    }
}

impl FieldWriteQueue {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn current_epoch(&self) -> u64 {
        self.epoch.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(super) fn bump_epoch(&self) -> u64 {
        self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }

    pub(super) fn has_pending_writes(&self) -> bool {
        self.pending.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// Enqueue a local field disk mutation in strict FIFO order on the background executor.
    /// Increments `epoch` immediately on the UI thread so any already-in-flight `FieldPoll`
    /// carrying an older epoch cannot overwrite the optimistic in-memory `self.events`.
    pub(super) fn enqueue<F>(&self, cx: &mut Context<super::Chamber>, op: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.bump_epoch();
        self.pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(Box::new(op));
        }
        if self
            .draining
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
        {
            let queue = self.queue.clone();
            let pending = self.pending.clone();
            let draining = self.draining.clone();
            cx.background_executor()
                .spawn(async move {
                    Self::drain_loop(&queue, &pending, &draining);
                })
                .detach();
        }
    }

    fn drain_loop(
        queue: &std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Box<dyn FnOnce() + Send + 'static>>>>,
        pending: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
        draining: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        loop {
            loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some(job) = next else {
                    break;
                };
                job();
                pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
            draining.store(false, std::sync::atomic::Ordering::SeqCst);
            let still_has_items = queue.lock().ok().is_some_and(|q| !q.is_empty());
            if !still_has_items
                || draining
                    .compare_exchange(
                        false,
                        true,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    )
                    .is_err()
            {
                break;
            }
        }
    }

    /// Read `field.jsonl` for a background `FieldPoll` only when no local UI write is queued
    /// or executing, and verify the write epoch did not advance during the read.
    pub(super) fn read_events_if_quiescent(
        &self,
        path: &std::path::Path,
    ) -> (Option<Vec<Event>>, Option<u64>) {
        if self.has_pending_writes() {
            return (None, None);
        }
        let epoch_before = self.current_epoch();
        if self.has_pending_writes() {
            return (None, None);
        }
        let events = io::read_events(path)
            .ok()
            .or_else(|| crate::rpc_client::poll_field_via_rpc(path, 0, 0).map(|(evs, _)| evs));
        let epoch_after = self.current_epoch();
        if epoch_before != epoch_after || self.has_pending_writes() {
            return (None, None);
        }
        (events, Some(epoch_after))
    }

    pub(super) fn is_poll_fresh(&self, poll_epoch: Option<u64>) -> bool {
        !self.has_pending_writes() && poll_epoch == Some(self.current_epoch())
    }
}

/// Pure decision helper for [`super::Chamber::reload_if_changed`]: whether a background
/// `FieldPoll` snapshot should replace the chamber's in-memory `local_events` and `view`.
pub(super) fn should_sync_polled_events(
    local_events: &[Event],
    view_message_len: usize,
    polled_events: &[Event],
    poll_is_fresh: bool,
) -> bool {
    if !poll_is_fresh {
        return false;
    }
    let stale_behind_local_append = !polled_events.is_empty()
        && polled_events.len() < local_events.len()
        && local_events.get(polled_events.len() - 1).map(|e| e.id)
            == polled_events.last().map(|e| e.id);
    if stale_behind_local_append {
        return false;
    }
    polled_events.len() != view_message_len
        || polled_events.len() != local_events.len()
        || polled_events.first().map(|e| e.id) != local_events.first().map(|e| e.id)
        || polled_events.last().map(|e| e.id) != local_events.last().map(|e| e.id)
}

/// Cadence of the timer-driven `FieldPoll`: the whole live tail when no
/// `daemon/subscribe` stream is up, and the heavy-scan cadence when one is.
pub(super) const FIELD_POLL_INTERVAL: Duration = Duration::from_millis(400);
/// Floor between push-woken polls, so a burst of `LiveUpdated` pushes during a turn
/// collapses into one re-read per gap instead of back-to-back field reads.
pub(super) const PUSH_POLL_MIN_GAP: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PollTrigger {
    Timer,
    Push,
}

/// Wait for the next background `FieldPoll`: the timer due at `timer_due`, or — while
/// `push` holds an acknowledged `daemon/subscribe` stream — the first daemon push,
/// though no sooner than [`PUSH_POLL_MIN_GAP`] after `last_poll`. Disconnected (or no
/// subscriber at all) it is exactly the timer, so the disk poll is the fallback.
pub(super) async fn wait_for_field_poll(
    executor: &gpui::BackgroundExecutor,
    push: Option<&crate::rpc_client::PushSignal>,
    timer_due: std::time::Instant,
    last_poll: std::time::Instant,
) -> PollTrigger {
    let gap_end = (last_poll + PUSH_POLL_MIN_GAP).min(timer_due);
    let now = std::time::Instant::now();
    if now < gap_end {
        executor.timer(gap_end - now).await;
    }
    let mut timer = executor.timer(timer_due.saturating_duration_since(std::time::Instant::now()));
    let Some(push) = push else {
        timer.await;
        return PollTrigger::Timer;
    };
    std::future::poll_fn(|cx| {
        if push.poll_dirty(cx).is_ready() {
            return std::task::Poll::Ready(PollTrigger::Push);
        }
        std::future::Future::poll(std::pin::Pin::new(&mut timer), cx)
            .map(|()| PollTrigger::Timer)
    })
    .await
}

#[derive(Debug, Clone)]
pub(super) struct FieldPoll {
    pub(super) events: Option<Vec<Event>>,
    pub(super) field_epoch: Option<u64>,
    pub(super) team_before: Team,
    pub(super) global_before: Team,
    pub(super) repo_team: Option<Team>,
    pub(super) global_team: Option<Team>,
    pub(super) is_over_budget: bool,
    pub(super) custom_skill_ids: Vec<String>,
    pub(super) scan: Option<WorkspaceScan>,
    pub(super) live_activities: Vec<(String, Option<hadron_lattice::live::Activity>)>,
    pub(super) gate_activities: Vec<hadron_lattice::live::Activity>,
    pub(super) latest_prompt_cost:
        Option<(String, hadron_lattice::prompt_cost::PromptBreakdown)>,
    pub(super) gluon_now_running: bool,
    pub(super) gluon_pid: Option<u32>,
    pub(super) hadron_processes: Option<Vec<hadron_lattice::sys::inspect::ProcessInfo>>,
    pub(super) rpc_endpoint: Option<hadron_lattice::wire::RpcEndpointMetadata>,
}

impl FieldPoll {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn gather(
        path: &std::path::Path,
        field_write_queue: &FieldWriteQueue,
        repo_team_path: &std::path::Path,
        team_before: Team,
        global_before: Team,
        roster_ids: &[String],
        file_tree_expanded: &std::collections::HashSet<String>,
        do_workspace_scan: bool,
        scan_diff: bool,
        scan_git: bool,
        scan_health: bool,
        verified_git_ready: bool,
        prev_branch_fingerprint: Option<&str>,
        active_plan_hint: Option<&str>,
        scan_processes: bool,
        terminal_pids: &[u32],
        selected_branch: Option<&str>,
        selected_worktree_path: Option<&str>,
    ) -> Self {
        let (events, field_epoch) = field_write_queue.read_events_if_quiescent(path);
        let read_strict = |p: &std::path::Path| -> Option<Team> {
            if !p.exists() {
                return Some(Team::default());
            }
            std::fs::read_to_string(p)
                .ok()
                .and_then(|t| hadron_lattice::parse_team(&t).ok())
        };
        let repo_team = read_strict(repo_team_path);
        let mut global_team = match hadron_lattice::team_config_path() {
            Some(p) => read_strict(&p),
            None => Some(Team::default()),
        };
        if let Some(ref mut gt) = global_team {
            hadron_lattice::merge_wsl_catalogue(gt, path);
        }

        let root = crate::vcs::repo_root_of(path);
        let budget_team = repo_team.as_ref().unwrap_or(&team_before);
        let is_over_budget = hadron_gluon::nucleus_status::index_over_budget(
            root,
            hadron_gluon::nucleus_status::resolve_budget_bytes(budget_team),
        );
        let custom_skill_ids = crate::text::custom_skill_names(Some(root));

        let scan = if do_workspace_scan {
            WorkspaceScan::gather(
                root,
                file_tree_expanded,
                scan_diff,
                scan_git,
                scan_health,
                verified_git_ready,
                prev_branch_fingerprint,
                active_plan_hint,
                selected_branch,
                selected_worktree_path,
            )
        } else {
            None
        };

        let now = chrono::Utc::now();
        let live_dir = hadron_lattice::live::live_dir(path);
        let mut live_activities = Vec::with_capacity(roster_ids.len() + 1);
        for id in roster_ids {
            let activity =
                hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new(id), now);
            live_activities.push((id.clone(), activity));
        }
        let gluon_activity =
            hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new("gluon"), now);
        live_activities.push(("gluon".to_string(), gluon_activity));

        let gates_dir = hadron_lattice::live::gates_dir(path);
        let gate_activities = hadron_lattice::live::gates(&gates_dir, now);
        let prompt_cost_dir = hadron_lattice::prompt_cost::prompt_cost_dir(path);
        let latest_prompt_cost =
            hadron_lattice::prompt_cost::load_latest(&prompt_cost_dir, roster_ids);
        let gluon_pid = crate::sys::probe_gluon_pid(path);
        let gluon_now_running = gluon_pid.is_some() || super::Chamber::probe_gluon_running(path);
        let hadron_processes = if scan_processes {
            let mut roots = vec![std::process::id()];
            if let Some(gpid) = gluon_pid {
                roots.push(gpid);
            }
            for &tpid in terminal_pids {
                roots.push(tpid);
            }
            let field_dir = hadron_lattice::hadron_dir_of(path);
            Some(hadron_lattice::sys::inspect::collect_process_tree(&roots, &field_dir))
        } else {
            None
        };
        let rpc_endpoint = crate::rpc_client::active_endpoint(path);

        Self {
            events,
            field_epoch,
            team_before,
            global_before,
            repo_team,
            global_team,
            is_over_budget,
            custom_skill_ids,
            scan,
            live_activities,
            gate_activities,
            latest_prompt_cost,
            gluon_now_running,
            gluon_pid,
            hadron_processes,
            rpc_endpoint,
        }
    }
}


impl super::Chamber {
    /// Re-project the field into the roster/log/session view, resolving the repo team
    /// against the global catalogue first so adopted quarks carry their full defs and
    /// available-but-not-adopted catalogue quarks show greyed. The one place the view
    /// is rebuilt, so every mutation path routes through the same resolve.
    pub(super) fn reproject(&mut self, events: &[Event]) {
        self.events = events.to_vec();
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
        let is_wsl = crate::wsl::is_wsl_unc_path(&repo);
        let all_plans = self.cached_plan_paths.clone();

        let new_plan_from_msg = if let Some(start_ix) = self.manual_plan_override_at_message_len {
            if start_ix < self.view.messages.len() {
                resolve_active_plan_from_messages_with_plans(
                    &repo,
                    &self.view.messages[start_ix..],
                    &all_plans,
                )
            } else {
                None
            }
        } else {
            None
        };

        let active_plan_path = if let Some(p) = new_plan_from_msg {
            self.manual_plan_override_at_message_len = None;
            Some(p)
        } else if self.manual_plan_override_at_message_len.is_some() {
            if let Some(ref manual) = self.last_plan_path {
                if all_plans.iter().any(|(_, p)| p == manual) {
                    Some(manual.clone())
                } else {
                    self.manual_plan_override_at_message_len = None;
                    resolve_active_plan_with_plans(&repo, &self.view.messages, &all_plans)
                }
            } else {
                self.manual_plan_override_at_message_len = None;
                resolve_active_plan_with_plans(&repo, &self.view.messages, &all_plans)
            }
        } else {
            resolve_active_plan_with_plans(&repo, &self.view.messages, &all_plans)
        };

        if let Some(rel_path) = active_plan_path {
            let path_str = rel_path.clone();
            let path_changed = self.last_plan_path.as_deref() != Some(&path_str);
            if path_changed {
                self.last_plan_path = Some(path_str);
            }
            let content_opt = match &self.cached_active_plan_content {
                Some((cached_rel, cached_body)) if cached_rel == &rel_path => {
                    Some(cached_body.clone())
                }
                _ => None,
            };
            if let Some(content) = content_opt {
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
            self.cached_active_plan_content = None;
        }

        if !is_wsl {
            for i in 0..self.view.tasks.len() {
                let body = self.view.tasks[i].body.clone();
                if let Some(named) = hadron_gluon::skills::research_ref(&body) {
                    if let Some(content) = crate::sys::read_workspace_file(&repo, &named) {
                        if let Some(title) = hadron_lattice::nucleus::parse_research_title(&content) {
                            crate::model::tasks::retitle_from_research(
                                &mut self.view.tasks,
                                &named,
                                &title,
                            );
                        }
                    }
                }
            }
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
        poll: FieldPoll,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;

        // The team files are edited out-of-band, not only through this window: the
        // daemon re-seats from team.json, and a quark is adopted/removed by writing
        // team.json (or the shared catalogue) directly. Poll them the same dumb way
        // the field is polled, so an externally added/removed quark shows in the
        // roster + Settings at once — instead of only after it first authors an
        // event (event-seeding), which is why a new quark used to "appear after
        // mention". A no-op after this window's own save: the reload matches what
        // was just written, so `!=` is false and nothing reprojects.
        let mut team_changed = false;
        if self.team == poll.team_before && self.global == poll.global_before {
            if let (Some(repo_team), Some(global_team)) = (poll.repo_team, poll.global_team) {
                if repo_team != self.team || global_team != self.global {
                    self.team = repo_team;
                    self.global = global_team;
                    self.providers =
                        configured_providers(&resolve_team(&self.team, &self.global));
                    team_changed = true;
                    changed = true;
                }
            }
        }

        if poll.is_over_budget != self.nucleus_over_budget {
            self.nucleus_over_budget = poll.is_over_budget;
            changed = true;
        }
        if poll.custom_skill_ids != self.cached_skill_ids {
            self.cached_skill_ids = poll.custom_skill_ids;
            changed = true;
        }

        // Only reproject on a successful, fresh read — a transient read error or an
        // in-flight poll that raced with a local UI write/clear must not clobber the
        // current view.
        if let Some(events) = poll.events {
            let poll_is_fresh = self.field_write_queue.is_poll_fresh(poll.field_epoch);
            if should_sync_polled_events(
                &self.events,
                self.view.messages.len(),
                &events,
                poll_is_fresh,
            ) {
                self.sync_view(&events);
                self.chat_list_state
                    .scroll_to_reveal_item(self.chat_message_ixs.len().saturating_sub(1));
                self.log_list_state
                    .scroll_to_reveal_item(self.log_message_ixs.len().saturating_sub(1));
                changed = true;
            } else if team_changed {
                // The message list is unchanged (same events) but the resolved team is
                // not — refresh the view so the new roster/Settings render. No
                // message-count change, so `sync_view` degenerates to a zero-delta splice.
                let local = self.events.clone();
                self.sync_view(&local);
            }
        } else if team_changed {
            let local = self.events.clone();
            self.sync_view(&local);
        }

        if true {
            // The file tree is a live view of the disk, not a boot-time snapshot, and
            // autocomplete mentions must stay live regardless of which rail tab is up —
            // so the scan runs every tick. It just no longer runs *here*: it is three
            // `git` subprocesses plus a `stat` per tracked file, and on the render
            // thread that blocked mouse/keyboard dispatch for the whole scan.
            if let Some(scan) = poll.scan {
                let plan_paths_changed = scan.plan_paths != self.cached_plan_paths;
                let plan_content_changed = scan.active_plan_content.is_some()
                    && scan.active_plan_content != self.cached_active_plan_content;
                let plan_landed_changed = scan.plan_landed_commits != self.cached_plan_landed_commits;
                if plan_paths_changed || plan_content_changed || plan_landed_changed {
                    if plan_paths_changed {
                        self.cached_plan_paths = scan.plan_paths;
                        self.manual_plan_override_at_message_len = None;
                    }
                    if let Some(plan_content) = scan.active_plan_content {
                        self.cached_active_plan_content = Some(plan_content);
                    } else if plan_paths_changed {
                        self.cached_active_plan_content = None;
                    }
                    self.cached_plan_landed_commits = scan.plan_landed_commits;
                    *self.cached_plan_dag.borrow_mut() = None;
                    self.update_active_plan();
                    changed = true;
                }
                if scan.nucleus_docs != self.cached_nucleus_docs
                    || scan.nucleus_fingerprint != self.cached_nucleus_fingerprint
                {
                    let was_initial = self.cached_nucleus_fingerprint == 0
                        && self.cached_nucleus_docs == (None, None, None);
                    self.cached_nucleus_docs = scan.nucleus_docs;
                    self.cached_nucleus_fingerprint = scan.nucleus_fingerprint;
                    if !was_initial && self.right_rail_tab == RightRailTab::Visualizer {
                        self.refresh_nucleus_graph(cx);
                    }
                    changed = true;
                }
                if scan.files != self.file_tree_paths {
                    // Gitignored entries included, mirroring `new` — see the note there.
                    *self.completion_files.borrow_mut() = scan.files.clone();
                    self.file_tree_paths = scan.files;
                    if self.right_rail_tab == RightRailTab::Visualizer {
                        self.warm_visualizer_graphs(cx);
                    }
                    changed = true;
                }

                self.git_ready_verified = true;
                if scan.git_ready != self.git_ready {
                    self.git_ready = scan.git_ready;
                    changed = true;
                }

                if scan.overdue_schedule_jobs != self.overdue_schedule_jobs {
                    self.overdue_schedule_jobs = scan.overdue_schedule_jobs;
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
                    *self.cached_three_way_diff.borrow_mut() = None;
                    changed = true;
                }

                if scan.git_branch_fingerprint.is_some()
                    && (scan.git_branch_fingerprint != self.git_branch_fingerprint
                        || self.git_branches.is_none()
                        || self.git_remote_branches.is_none())
                {
                    let fp_changed = scan.git_branch_fingerprint != self.git_branch_fingerprint;
                    self.git_branch_fingerprint = scan.git_branch_fingerprint;
                    if fp_changed {
                        crate::vcs::clear_unmerged_commit_cache();
                        *self.cached_plan_dag.borrow_mut() = None;
                        *self.cached_three_way_diff.borrow_mut() = None;
                    }
                    if let Some(branches) = scan.git_branches {
                        self.git_branches = Some(branches);
                    }
                    if let Some(remote_branches) = scan.git_remote_branches {
                        self.git_remote_branches = Some(remote_branches);
                    }
                    if let Some(worktrees) = scan.git_worktrees {
                        self.git_worktrees = Some(worktrees);
                    }
                    if scan.git_log_graph.is_some() {
                        self.git_log_graph = scan.git_log_graph;
                        self.rebuild_graph_rows();
                    }
                    changed = true;
                }

                if let Some((branch, diff)) = scan.selected_branch_diff {
                    if self.git_selected_branch.is_some() {
                        if self.git_selected_branch.as_deref() != Some(branch.as_str()) {
                            self.git_selected_branch = Some(branch);
                            changed = true;
                        }
                        if self.git_branch_diff != diff {
                            self.git_branch_diff = diff;
                            changed = true;
                        }
                    }
                }

                if let Some(health) = scan.repo_health {
                    if self.repo_health_report.as_ref() != Some(&health) {
                        self.repo_health_report = Some(health);
                        changed = true;
                    }
                }

                if let Some(remote) = scan.remote_tracking {
                    if self.git_remote_info.as_ref() != Some(&remote) {
                        self.git_remote_info = Some(remote);
                        changed = true;
                    }
                }
            }

            let mut live_activity_changed = false;
            for (id, activity) in poll.live_activities {
                if self.last_live_activities.get(&id) != Some(&activity) {
                    self.last_live_activities.insert(id, activity);
                    live_activity_changed = true;
                }
            }
            if live_activity_changed {
                changed = true;
            }

            // Gate heartbeats live outside the roster loop above (keyed by branch, not
            // quark id — `live::gates_dir`), so the Tasks tab needs its own change
            // detection to repaint when a gate starts, keeps running, or finishes.
            if poll.gate_activities != self.last_gate_activities {
                self.last_gate_activities = poll.gate_activities;
                changed = true;
            }
            if poll.latest_prompt_cost != self.last_prompt_cost {
                self.last_prompt_cost = poll.latest_prompt_cost;
                changed = true;
            }

            // Critical, must-notice event: gluon holds the daemon lock exclusively, so
            // a stopped daemon means no quark in the swarm can take a turn until it's
            // restarted. Fire the banner only on the running→stopped edge (not every
            // tick it stays down) and clear it on stopped→running.
            if let Some(went_running) = gluon_running_edge(self.last_gluon_running, poll.gluon_now_running) {
                self.last_gluon_running = poll.gluon_now_running;
                self.gluon_stopped_notice = !went_running;
                changed = true;
            }
            if poll.gluon_pid != self.last_gluon_pid {
                self.last_gluon_pid = poll.gluon_pid;
                changed = true;
            }
            if let Some(procs) = poll.hadron_processes {
                if procs != self.last_hadron_processes {
                    self.last_hadron_processes = procs;
                    if self.process_manager_open {
                        changed = true;
                    }
                }
            }
            if poll.rpc_endpoint != self.rpc_endpoint {
                self.rpc_endpoint = poll.rpc_endpoint;
                changed = true;
            }

            if self.toast_manager.prune(std::time::Instant::now()) {
                changed = true;
            }

            // While any quark turn, merge gate, or working task is in flight, tick
            // the live elapsed timer once per second so the badge advances smoothly
            // even between 5s heartbeats — with zero repaints when the swarm is idle.
            let any_live_timer = !self.last_gate_activities.is_empty()
                || self.last_live_activities.values().any(|a| a.is_some())
                || self.view.roster.iter().any(|r| {
                    r.adopted
                        && r.enabled
                        && matches!(r.state, QuarkState::Excited | QuarkState::Thinking)
                })
                || self
                    .view
                    .tasks
                    .iter()
                    .any(|t| t.state == crate::model::tasks::TaskState::Working);
            if any_live_timer {
                let now_sec = chrono::Utc::now().timestamp();
                if self.last_live_timer_sec != Some(now_sec) {
                    self.last_live_timer_sec = Some(now_sec);
                    changed = true;
                }
            } else {
                self.last_live_timer_sec = None;
            }

            if self.right_rail_tab == RightRailTab::Git
                && self.git_subtab == GitSubtab::Remote
                && self.github_issues.is_none()
                && self.github_status_msg.is_none()
            {
                self.refresh_remote_and_github(cx);
            }

            if changed {
                cx.notify();
            }
        }
    }

    pub(super) fn refresh_mission_plan(&mut self, cx: &mut Context<Self>) {
        let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let active_hint = self.last_plan_path.clone();
        cx.spawn(async move |this, cx| {
            let (fp, plan_paths, active_content, landed_commits) = cx
                .background_executor()
                .spawn(async move {
                    crate::vcs::clear_unmerged_commit_cache();
                    let fp = crate::vcs::branch_fingerprint(&root);
                    let plan_paths = scan_all_plan_paths(&root);
                    let target = active_hint
                        .or_else(|| newest_plan_from_list_with_mtime(&plan_paths).map(|(_, p)| p));
                    let active_content = target.and_then(|rel| {
                        crate::sys::read_workspace_file(&root, &rel).map(|body| (rel, body))
                    });
                    let landed_commits = active_content
                        .as_ref()
                        .map(|(_, body)| collect_plan_landed_commits(&root, body))
                        .unwrap_or_default();
                    (fp, plan_paths, active_content, landed_commits)
                })
                .await;
            let _ = this.update(cx, |chamber, cx| {
                chamber.git_branch_fingerprint = Some(fp);
                chamber.cached_plan_paths = plan_paths;
                chamber.cached_active_plan_content = active_content;
                chamber.cached_plan_landed_commits = landed_commits;
                *chamber.cached_plan_dag.borrow_mut() = None;
                chamber.update_active_plan();
                cx.notify();
            });
        })
        .detach();
    }
}

/// Resolve the active implementation plan for a workspace:
/// 1. Newest-first scan of messages for an explicit plan file reference (`plan_ref`) or mention that exists on disk.
/// 2. If no plan is explicitly referenced in messages, scan `.hadron/docs/plans/`, `docs/plans/`, and worktrees for the newest plan on disk.
#[cfg(test)]
pub(crate) fn resolve_active_plan(repo: &Path, messages: &[crate::model::MessageRow]) -> Option<String> {
    let all_plans = scan_all_plan_paths(repo);
    resolve_active_plan_with_plans(repo, messages, &all_plans)
}

pub(crate) fn resolve_active_plan_with_plans(
    _repo: &Path,
    messages: &[crate::model::MessageRow],
    all_plans: &[(std::time::SystemTime, String)],
) -> Option<String> {
    let newest_disk = newest_plan_from_list_with_mtime(all_plans);
    let newest_msg = resolve_active_plan_from_messages_with_plans_and_ts(messages, all_plans);

    match (newest_msg, newest_disk) {
        (Some((msg_ts, msg_plan)), Some((disk_mtime, disk_plan))) => {
            let msg_sys_time = std::time::SystemTime::from(msg_ts);
            if disk_mtime > msg_sys_time {
                Some(disk_plan)
            } else {
                Some(msg_plan)
            }
        }
        (Some((_, msg_plan)), None) => Some(msg_plan),
        (None, Some((_, disk_plan))) => Some(disk_plan),
        (None, None) => None,
    }
}

pub(crate) fn resolve_active_plan_from_messages_with_plans(
    _repo: &Path,
    messages: &[crate::model::MessageRow],
    all_plans: &[(std::time::SystemTime, String)],
) -> Option<String> {
    resolve_active_plan_from_messages_with_plans_and_ts(messages, all_plans)
        .map(|(_, plan)| plan)
}

fn resolve_active_plan_from_messages_with_plans_and_ts(
    messages: &[crate::model::MessageRow],
    all_plans: &[(std::time::SystemTime, String)],
) -> Option<(chrono::DateTime<chrono::Utc>, String)> {
    messages.iter().rev().find_map(|m| {
        find_plan_mention_in_text_with_plans(&m.body, all_plans)
            .map(|plan| (m.ts, plan))
    })
}

fn find_plan_mention_in_text_with_plans(
    text: &str,
    all_plans: &[(std::time::SystemTime, String)],
) -> Option<String> {
    if let Some(p) = hadron_gluon::skills::plan_ref(text) {
        let clean = p
            .strip_prefix("file://")
            .or_else(|| p.strip_prefix("file:"))
            .unwrap_or(&p);
        if let Some((_, rel)) = all_plans.iter().find(|(_, rel)| {
            rel == clean || rel.ends_with(&format!("/{clean}")) || clean.ends_with(rel.as_str())
        }) {
            return Some(rel.clone());
        }
    }

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

    let mut collect_plan_dir = |dir: &Path| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                    if file_name.starts_with('.') {
                        continue;
                    }
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    if let Ok(rel) = path.strip_prefix(repo) {
                        candidates.push((mtime, rel.to_string_lossy().to_string()));
                    }
                } else if path.is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            let sub_path = sub_entry.path();
                            if sub_path.is_file()
                                && sub_path.extension().and_then(|e| e.to_str()) == Some("md")
                            {
                                let sub_file_name =
                                    sub_path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                                if sub_file_name.starts_with('.') {
                                    continue;
                                }
                                let mtime = sub_entry
                                    .metadata()
                                    .and_then(|m| m.modified())
                                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                if let Ok(rel) = sub_path.strip_prefix(repo) {
                                    candidates.push((mtime, rel.to_string_lossy().to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    let check_dirs = [
        repo.join(".hadron").join("docs").join("plans"),
        repo.join("docs").join("plans"),
    ];

    for dir in &check_dirs {
        collect_plan_dir(dir);
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
                    collect_plan_dir(dir);
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates
}

fn extract_plan_date_prefix(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    if bytes.len() >= 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
    {
        Some(name[..10].to_string())
    } else {
        None
    }
}

fn plan_rel_date_prefix(rel: &str) -> Option<String> {
    let p = Path::new(rel);
    if let Some(parent) = p.parent() {
        let parent_name = parent.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if parent_name != "plans" && !parent_name.is_empty() {
            if let Some(d) = extract_plan_date_prefix(parent_name) {
                return Some(d);
            }
        }
    }
    let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
    extract_plan_date_prefix(file_name)
}

fn newest_plan_from_list_with_mtime(
    all_plans: &[(std::time::SystemTime, String)],
) -> Option<(std::time::SystemTime, String)> {
    let mut candidates = all_plans.to_vec();
    candidates.sort_by(|a, b| {
        let date_a = plan_rel_date_prefix(&a.1);
        let date_b = plan_rel_date_prefix(&b.1);
        let is_master_a = a.1.ends_with("master.md") || a.1.ends_with("index.md");
        let is_master_b = b.1.ends_with("master.md") || b.1.ends_with("index.md");
        match (&date_a, &date_b) {
            (Some(da), Some(db)) if da != db => db.cmp(da),
            _ => b
                .0
                .cmp(&a.0)
                .then_with(|| date_b.cmp(&date_a))
                .then_with(|| is_master_b.cmp(&is_master_a))
                .then_with(|| b.1.cmp(&a.1)),
        }
    });
    candidates.into_iter().next()
}

#[cfg(test)]
fn newest_plan_from_list(all_plans: &[(std::time::SystemTime, String)]) -> Option<String> {
    newest_plan_from_list_with_mtime(all_plans).map(|(_, rel)| rel)
}

/// Scan `.hadron/docs/plans/`, `docs/plans/`, and worktree trees for the most recently modified `.md` plan.
#[cfg(test)]
pub(crate) fn scan_newest_plan(repo: &Path) -> Option<String> {
    let candidates = scan_all_plan_paths(repo);
    newest_plan_from_list(&candidates)
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
        } else if extract_plan_date_prefix(stripped).is_some() {
            format_suite_title(stripped)
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
/// chronological newest-first order across both multi-plan folder suites and single-file plans.
#[cfg(test)]
pub(crate) fn scan_plan_dropdown_items(repo: &Path, active_plan_rel: &str) -> Vec<PlanDropdownItem> {
    let all_paths = scan_all_plan_paths(repo);
    build_plan_dropdown_items(&all_paths, active_plan_rel)
}

pub(crate) fn build_plan_dropdown_items(
    all_paths: &[(std::time::SystemTime, String)],
    active_plan_rel: &str,
) -> Vec<PlanDropdownItem> {
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
        let p = Path::new(rel);
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
            mtime: *mtime,
            rel: rel.clone(),
            suite,
            file_name,
        });
    }

    // Group folder suites by suite name; treat each single-file root plan (`suite == None`)
    // as its own chronological bucket so single-file plans and multi-plan folder suites
    // are interleaved strictly newest-first.
    let mut suite_groups: std::collections::BTreeMap<String, Vec<PlanEntry>> =
        std::collections::BTreeMap::new();
    let mut standalone_entries: Vec<PlanEntry> = Vec::new();
    for entry in entries {
        if let Some(ref s) = entry.suite {
            suite_groups.entry(s.clone()).or_default().push(entry);
        } else {
            standalone_entries.push(entry);
        }
    }

    struct SuiteBucket {
        suite: Option<String>,
        sort_key: String,
        date_prefix: Option<String>,
        max_mtime: std::time::SystemTime,
        is_active_bucket: bool,
        entries: Vec<PlanEntry>,
    }

    let mut buckets: Vec<SuiteBucket> = Vec::new();
    for (suite_name, mut list) in suite_groups {
        let max_mtime = list
            .iter()
            .map(|e| e.mtime)
            .max()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        list.sort_by(|a, b| {
            let is_master_a = a.file_name == "master.md" || a.file_name == "index.md";
            let is_master_b = b.file_name == "master.md" || b.file_name == "index.md";
            is_master_b
                .cmp(&is_master_a)
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
        let is_active_bucket = active_suite.as_deref() == Some(suite_name.as_str());
        let date_prefix = extract_plan_date_prefix(&suite_name);
        buckets.push(SuiteBucket {
            suite: Some(suite_name.clone()),
            sort_key: suite_name,
            date_prefix,
            max_mtime,
            is_active_bucket,
            entries: list,
        });
    }

    for entry in standalone_entries {
        let stem = entry.file_name.strip_suffix(".md").unwrap_or(&entry.file_name).to_string();
        let date_prefix = extract_plan_date_prefix(&entry.file_name);
        let is_active_bucket = entry.rel == active_plan_rel;
        let max_mtime = entry.mtime;
        buckets.push(SuiteBucket {
            suite: None,
            sort_key: stem,
            date_prefix,
            max_mtime,
            is_active_bucket,
            entries: vec![entry],
        });
    }

    // Sort all buckets strictly chronologically (newest first), regardless of whether a plan
    // is a single-file root plan or a multi-plan folder suite:
    // 1. YYYY-MM-DD date prefix descending when both buckets carry distinct date prefixes
    // 2. Filesystem mtime descending
    // 3. Sort key descending as deterministic tie-breaker
    buckets.sort_by(|a, b| match (&a.date_prefix, &b.date_prefix) {
        (Some(da), Some(db)) if da != db => db.cmp(da),
        _ => b
            .max_mtime
            .cmp(&a.max_mtime)
            .then_with(|| b.sort_key.cmp(&a.sort_key)),
    });

    let total_buckets = buckets.len();
    let mut dropdown_items = Vec::new();

    for (i, bucket) in buckets.into_iter().enumerate() {
        if i > 0 {
            dropdown_items.push(PlanDropdownItem::Separator);
        }

        let formatted = format_suite_title(&bucket.sort_key);
        let icon = if bucket.suite.is_some() { "📁" } else { "📄" };
        let header_title = if bucket.is_active_bucket && total_buckets > 1 {
            format!("{icon} Current: {formatted}")
        } else {
            format!("{icon} {formatted}")
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
            PlanDropdownItem::Plan { rel_path, is_active, .. } => Some((rel_path.as_str(), *is_active)),
            _ => None,
        }).collect();

        assert_eq!(plan_items.len(), 4);

        // Chronological newest-first: 2026-08-21 comes first, 2026-08-19 second (with active marker preserved)
        assert!(plan_items[0].0.contains("2026-08-21-swarm-orchestration/master.md"));
        assert!(!plan_items[0].1);
        assert!(plan_items[1].0.contains("2026-08-21-swarm-orchestration/01-phase-1-reactive-dag.md"));
        assert!(!plan_items[1].1);
        assert!(plan_items[2].0.contains("2026-08-19-twenty-capabilities/master.md"));
        assert!(plan_items[2].1);
        assert!(plan_items[3].0.contains("2026-08-19-twenty-capabilities/01-phase-1-quick-dx.md"));
        assert!(!plan_items[3].1);
    }

    #[test]
    fn test_chronological_interleaving_single_and_suite_plans_and_autoselect() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let plans_dir = root.join(".hadron").join("docs").join("plans");
        let suite_old = plans_dir.join("2026-08-19-twenty-capabilities");
        let suite_new = plans_dir.join("2026-08-21-swarm-orchestration");
        std::fs::create_dir_all(&suite_old).unwrap();
        std::fs::create_dir_all(&suite_new).unwrap();

        std::fs::write(suite_old.join("master.md"), "# Twenty Caps\n").unwrap();
        let single_mid = plans_dir.join("2026-08-20-single-file-plan.md");
        std::fs::write(&single_mid, "# Single Mid Plan\n").unwrap();
        let single_newest = plans_dir.join("2026-08-22-latest-hotfix.md");
        std::fs::write(&single_newest, "# Latest Hotfix\n").unwrap();
        std::fs::write(suite_new.join("master.md"), "# Swarm Master\n").unwrap();

        let items = scan_plan_dropdown_items(root, ".hadron/docs/plans/2026-08-20-single-file-plan.md");
        let plan_paths: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                PlanDropdownItem::Plan { rel_path, .. } => Some(rel_path.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(plan_paths.len(), 4);
        assert!(plan_paths[0].ends_with("2026-08-22-latest-hotfix.md"));
        assert!(plan_paths[1].ends_with("2026-08-21-swarm-orchestration/master.md"));
        assert!(plan_paths[2].ends_with("2026-08-20-single-file-plan.md"));
        assert!(plan_paths[3].ends_with("2026-08-19-twenty-capabilities/master.md"));

        // Verify auto-selection prefers a newly created/modified plan on disk over an older message mention
        let old_msg_ts = chrono::Utc::now() - chrono::Duration::seconds(60);
        let messages = vec![crate::model::MessageRow {
            from: "Human".to_string(),
            to: None,
            body: "Old message mentioning .hadron/docs/plans/2026-08-19-twenty-capabilities/master.md".to_string(),
            kind_label: "message",
            usage: None,
            ts: old_msg_ts,
            legacy_used_tokens: None,
            turn: None,
            severity: None,
        }];
        let resolved = resolve_active_plan(root, &messages).unwrap();
        assert!(
            resolved.ends_with("2026-08-22-latest-hotfix.md"),
            "expected newest plan created after old message, got {resolved}"
        );
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

    #[test]
    fn test_scan_all_plan_paths_includes_worktree_plans() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        let wt_plan_dir = root.join(".hadron").join("trees").join("quark-1").join("docs").join("plans");
        std::fs::create_dir_all(&wt_plan_dir).unwrap();
        let plan_file = wt_plan_dir.join("2026-10-29-quark-plan.md");
        std::fs::write(&plan_file, "# Worktree Plan\n").unwrap();

        let plans = scan_all_plan_paths(root);
        assert!(
            plans.iter().any(|(_, path)| path.contains("2026-10-29-quark-plan.md")),
            "expected worktree plan to be scanned, got: {plans:?}"
        );
    }

    #[test]
    fn test_gather_overdue_schedule_jobs_fast_returns_empty_when_unscheduled() {
        let temp = tempfile::tempdir().unwrap();
        let jobs = gather_overdue_schedule_jobs(temp.path());
        assert!(jobs.is_empty());
    }

    #[test]
    fn should_sync_polled_events_rejects_stale_poll_after_clear() {
        let old_session: Vec<Event> = (0..10)
            .map(|i| Event::new(Actor::Human, None, Kind::Message { body: format!("msg {i}") }))
            .collect();
        let post_clear = vec![
            Event::new(
                Actor::Human,
                None,
                Kind::Command {
                    cmd: "/clear".into(),
                    exit: 0,
                    out_summary: "reset".into(),
                },
            ),
            Event::new(Actor::Human, None, Kind::ModeSet { mode: Mode::Bypass }),
        ];

        // When an in-flight FieldPoll captured `old_session` before `/clear` bumped the write
        // epoch (`poll_is_fresh == false`), `should_sync_polled_events` MUST reject it so the
        // old chat never flashes back for 1 second after `/clear`.
        assert!(!should_sync_polled_events(
            &post_clear,
            post_clear.len(),
            &old_session,
            false
        ));
    }

    #[test]
    fn should_sync_polled_events_rejects_stale_poll_behind_local_append() {
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "first".into() });
        let e2 = Event::new(Actor::Human, None, Kind::Message { body: "second".into() });
        let local = vec![e1.clone(), e2];
        let polled = vec![e1];

        assert!(!should_sync_polled_events(&local, local.len(), &polled, true));
    }

    #[test]
    fn should_sync_polled_events_syncs_when_local_events_desynced_from_view() {
        let e1 = Event::new(Actor::Human, None, Kind::Message { body: "first".into() });
        let e2 = Event::new(Actor::Human, None, Kind::Message { body: "second".into() });
        let polled = vec![e1, e2];

        // Even if `view_message_len` already equals `polled.len()`, an empty `local_events`
        // must sync so the first local append does not wipe prior chat history.
        assert!(should_sync_polled_events(&[], 2, &polled, true));
    }

    #[test]
    fn field_write_queue_epoch_and_quiescent_read_guard() {
        let temp = tempfile::tempdir().unwrap();
        let field_path = temp.path().join("field.jsonl");
        let ev = Event::new(Actor::Human, None, Kind::Message { body: "hello".into() });
        io::append_event(&field_path, &ev).unwrap();

        let q = FieldWriteQueue::new();
        assert_eq!(q.current_epoch(), 0);
        let (read_evs, epoch) = q.read_events_if_quiescent(&field_path);
        assert_eq!(epoch, Some(0));
        assert_eq!(read_evs.unwrap().len(), 1);
        assert!(q.is_poll_fresh(Some(0)));

        // Bumping epoch invalidates any poll from epoch 0.
        q.bump_epoch();
        assert!(!q.is_poll_fresh(Some(0)));
        assert!(q.is_poll_fresh(Some(1)));
    }
}
