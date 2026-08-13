use super::*;

/// Which collapsible-diff panel a toggle click targets. The Changes rail and the
/// per-branch diff share one renderer (`file_diff_rows`) but keep independent
/// open-row sets, so opening a file in one doesn't open it in the other.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DiffPanel {
    Changes,
    Branch,
    Commit,
}

/// Lane colours, cycled by rail column, so the commit graph reads as coloured lanes
/// rather than monochrome ASCII. Distinct on the near-black field.
const LANE_COLORS: [u32; 6] = [0x34d399, 0x60a5fa, 0xf59e0b, 0xc084fc, 0x2dd4bf, 0xf87171];
/// A commit that has landed in `main` (green) vs one still in flight (rose); a
/// detached / unknown HEAD is neutral (it has no branch to be merged).
const MERGED_COLOR: u32 = 0x34d399;
const UNMERGED_COLOR: u32 = 0xf87171;
const NEUTRAL_COLOR: u32 = 0x94a3b8;
const ADD_COLOR: u32 = 0x34d399;
const DEL_COLOR: u32 = 0xf87171;
/// One graph rail column width in px when painting vector lanes.
const LANE_W: f32 = 16.0;
/// Cap for a `--decorate` ref chip so it cannot squeeze the subject out.
const DECO_CHIP_MAX_W: f32 = 190.0;

impl super::Chamber {
    /// The Git rail: local branches (with merged-into-`main` status and click-to-diff),
    /// every worktree of this repo, and a coloured commit graph — so "is it merged?",
    /// "what did this branch change?" and "who else has a checkout" are answerable
    /// without leaving the chamber.
    pub(super) fn git_tab_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.git_subtab;
        let subtabs = h_flex()
            .id("git-capsule-subtabs")
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(theme::tab_bar_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .max_w_full()
            .overflow_x_scroll()
            .children(GitSubtab::ALL.map(|t| {
                let is_selected = t.index() == selected.index();
                let label = t.label();
                let ix = t.index();
                div()
                    .id(("git-subtab-pill", ix))
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .when(is_selected, |s| {
                        s.bg(theme::glass_highlight())
                            .text_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                    })
                    .when(!is_selected, |s| {
                        s.text_color(theme::text_muted())
                            .hover(|h| h.text_color(theme::text()))
                    })
                    .text_xs()
                    .child(label)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.git_subtab = GitSubtab::from_index(ix);
                        cx.notify();
                    }))
            }));

        // The Graph subtab virtualizes its own rows, so it owns its scrolling: a
        // `gpui::list` inside an `overflow_y_scroll` parent is unbounded in height and
        // would lay out every commit — exactly what the virtual list exists to avoid.
        let body = match selected {
            GitSubtab::Branches => div()
                .id("git-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.git_scroll)
                .px_3()
                .pb_3()
                .child(self.git_branches_section(cx))
                .into_any_element(),
            GitSubtab::Worktrees => div()
                .id("git-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.git_scroll)
                .px_3()
                .pb_3()
                .child(self.git_worktrees_section(cx))
                .into_any_element(),
            GitSubtab::Graph => div()
                .size_full()
                .px_3()
                .pb_3()
                .child(self.git_graph_section(cx))
                .into_any_element(),
            GitSubtab::Delegation => div()
                .id("git-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.git_scroll)
                .px_3()
                .pb_3()
                .child(self.git_delegation_section(cx))
                .into_any_element(),
        };
        let git_pane = div()
            .flex_1()
            .min_h_0()
            .relative()
            .text_sm()
            .text_color(theme::text())
            .child(body);

        let git_pane = match selected {
            GitSubtab::Graph => git_pane.vertical_scrollbar(&self.git_graph_list),
            _ => git_pane.vertical_scrollbar(&self.git_scroll),
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .child(h_flex().flex_none().px_3().py_2().child(subtabs))
            .child(git_pane)
    }

    fn muted(text: &'static str) -> impl IntoElement {
        div().text_color(theme::text_muted()).child(text)
    }

    /// A commit marker: a filled dot plus the short hash, both in `color`. The shared
    /// motif across branches, worktrees and the graph — status lives in the colour.
    fn commit_token(head: &str, color: u32) -> impl IntoElement {
        let short: String = head.chars().take(7).collect();
        h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .text_color(gpui::rgb(color))
            .child(div().child("●"))
            .child(div().font_family("Cascadia Code").child(short))
    }

    // ── Branches ───────────────────────────────────────────────────────────────

    fn git_branches_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: gpui::AnyElement = match &self.git_branches {
            None => Self::muted("Loading branches...").into_any_element(),
            Some(branches) if branches.is_empty() => {
                Self::muted("No local branches.").into_any_element()
            }
            Some(branches) => {
                let mut list = v_flex().w_full();
                for (ix, branch) in branches.iter().enumerate() {
                    let color = if branch.merged { MERGED_COLOR } else { UNMERGED_COLOR };
                    let is_selected =
                        self.git_selected_branch.as_deref() == Some(branch.name.as_str());
                    let name = branch.name.clone();
                    let row = h_flex()
                        .id(("branch-row", ix))
                        .w_full()
                        .gap_2()
                        .items_center()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::border()))
                        .when(is_selected, |d| d.bg(theme::border()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_branch(name.clone());
                            cx.notify();
                        }))
                        .child(Self::commit_token(&branch.head, color))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(branch.is_current, |d| d.text_color(theme::accent()))
                                .child(branch.name.clone()),
                        );
                    // The diff panel is an accordion under the row that was clicked.
                    // Appending it after the whole list instead put it thousands of
                    // pixels below the fold with 126 branches — the click looked dead.
                    let mut entry = v_flex().w_full().child(row);
                    if is_selected {
                        entry = entry.child(self.branch_diff_panel(&branch.name, cx));
                    }
                    list = list.child(entry);
                }
                list.into_any_element()
            }
        };

        rows
    }

    /// Compute (or toggle off) the diff of a branch against `main`. Clicking the
    /// already-selected branch clears the panel.
    fn select_branch(&mut self, name: String) {
        if self.git_selected_branch.as_deref() == Some(name.as_str()) {
            self.git_selected_branch = None;
            self.git_branch_diff = None;
            return;
        }
        let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        self.git_branch_diff = crate::vcs::branch_diff(&root, "main", &name);
        self.git_branch_open_ixs.clear();
        self.git_selected_branch = Some(name);
    }

    /// The changed-files panel for the selected branch: a `N files  +A −R` header over
    /// the shared collapsible file-diff list.
    fn branch_diff_panel(&self, branch: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let body: gpui::AnyElement = match &self.git_branch_diff {
            None => Self::muted("Could not diff this branch against main.").into_any_element(),
            Some(files) if files.is_empty() => {
                Self::muted("No changes relative to main.").into_any_element()
            }
            Some(files) => {
                let added: usize = files.iter().map(|f| f.added).sum();
                let removed: usize = files.iter().map(|f| f.removed).sum();
                let n = files.len();
                let stats = h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme::text_muted())
                            .child(format!("{n} file{}", if n == 1 { "" } else { "s" })),
                    )
                    .child(div().text_color(gpui::rgb(ADD_COLOR)).child(format!("+{added}")))
                    .child(div().text_color(gpui::rgb(DEL_COLOR)).child(format!("−{removed}")));
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(stats)
                    .child(self.file_diff_rows(files, &self.git_branch_open_ixs, DiffPanel::Branch, cx))
                    .into_any_element()
            }
        };

        let title = branch.to_string();
        v_flex()
            .w_full()
            .gap_2()
            .mb_2()
            .ml_2()
            .mr_2()
            .pl_3()
            .pr_3()
            .py_2()
            .border_l_2()
            .border_color(theme::border())
            .child(
                h_flex()
                    .id("branch-diff-close")
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_selected_branch = None;
                        this.git_branch_diff = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme::accent())
                            .child(format!("Changes in {title}")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("close ×"),
                    ),
            )
            .child(body)
    }

    /// Compute (or toggle off) the patch diff of a commit. Clicking the
    /// already-selected commit clears the panel.
    fn select_commit(&mut self, hash: String) {
        let previous = self.git_selected_commit.take();
        let toggled_off = previous.as_deref() == Some(hash.as_str());
        if toggled_off {
            self.git_commit_diff = None;
            self.git_commit_message = None;
        } else {
            let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
            self.git_commit_diff = crate::vcs::commit_diff(&root, &hash);
            self.git_commit_message = crate::vcs::commit_message(&root, &hash);
            self.git_commit_open_ixs.clear();
            self.git_selected_commit = Some(hash.clone());
        }
        // Both the row that just closed and the one that just opened changed height.
        let changed: Vec<&str> = previous
            .as_deref()
            .into_iter()
            .chain((!toggled_off).then_some(hash.as_str()))
            .collect();
        for h in changed {
            self.invalidate_graph_row(h);
        }
    }

    /// Tell the Graph subtab's virtual list that one row's height changed.
    ///
    /// `gpui::list` caches the height it measured for each item and only re-measures
    /// what it is told about — a selected row grows by its diff panel, so without this
    /// the list keeps the old height and the rows below it overlap. `splice` (rather
    /// than `reset`) invalidates the single row and leaves the scroll position alone.
    fn invalidate_graph_row(&mut self, full_hash: &str) {
        if let Some(ix) = self
            .git_graph_rows
            .iter()
            .position(|r| r.full_hash.as_deref() == Some(full_hash))
        {
            self.git_graph_list.splice(ix..ix + 1, 1);
        }
    }

    /// The changed-files panel for the selected commit: a `N files  +A −R` header over
    /// the shared collapsible file-diff list.
    fn commit_diff_panel(&self, commit_hash: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let body: gpui::AnyElement = match &self.git_commit_diff {
            None => Self::muted("Could not load diff for this commit.").into_any_element(),
            Some(files) if files.is_empty() => {
                Self::muted("No changes in this commit.").into_any_element()
            }
            Some(files) => {
                let added: usize = files.iter().map(|f| f.added).sum();
                let removed: usize = files.iter().map(|f| f.removed).sum();
                let n = files.len();
                let stats = h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme::text_muted())
                            .child(format!("{n} file{}", if n == 1 { "" } else { "s" })),
                    )
                    .child(div().text_color(gpui::rgb(ADD_COLOR)).child(format!("+{added}")))
                    .child(div().text_color(gpui::rgb(DEL_COLOR)).child(format!("−{removed}")));
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(stats)
                    .child(self.file_diff_rows(files, &self.git_commit_open_ixs, DiffPanel::Commit, cx))
                    .into_any_element()
            }
        };

        let short_hash: String = commit_hash.chars().take(7).collect();
        v_flex()
            .w_full()
            .gap_2()
            .mb_2()
            .ml_2()
            .mr_2()
            .pl_3()
            .pr_3()
            .py_2()
            .border_l_2()
            .border_color(theme::border())
            .child(
                h_flex()
                    .id("commit-diff-close")
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_selected_commit = None;
                        this.git_commit_diff = None;
                        this.git_commit_message = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme::accent())
                            .child(format!("Changes in commit {short_hash}")),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("close ×"),
                    ),
            )
            .when_some(self.git_commit_message.as_ref(), |this, msg| {
                if msg.trim().is_empty() {
                    return this;
                }
                let mut msg_box = v_flex().gap_0p5().w_full();
                for (ix, line) in msg.lines().enumerate() {
                    let is_subject = ix == 0;
                    msg_box = msg_box.child(
                        div()
                            .text_xs()
                            .when(is_subject, |d| d.text_color(theme::text()).font_weight(gpui::FontWeight::BOLD))
                            .when(!is_subject, |d| d.text_color(theme::text_muted()))
                            .child(if line.is_empty() { " ".to_string() } else { line.to_string() }),
                    );
                }
                this.child(
                    v_flex()
                        .w_full()
                        .p_2()
                        .rounded_md()
                        .bg(theme::glass_card())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .child(msg_box),
                )
            })
            .child(body)
    }

    // ── Worktrees ────────────────────────────────────────────────────────────────

    /// The branch a worktree row selects when clicked, and the single home of
    /// "is this row clickable at all" — `None` for a detached HEAD, which has no
    /// branch to diff and must therefore stay inert rather than look live.
    fn worktree_selects_branch(wt: &crate::vcs::WorktreeInfo) -> Option<String> {
        wt.branch.clone()
    }

    fn git_worktrees_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: gpui::AnyElement = match &self.git_worktrees {
            None => Self::muted("Loading worktrees...").into_any_element(),
            Some(worktrees) if worktrees.is_empty() => {
                Self::muted("No worktrees.").into_any_element()
            }
            Some(worktrees) => {
                let mut list = v_flex().w_full().gap_1();
                for (ix, wt) in worktrees.iter().enumerate() {
                    // Colour the commit token by the branch's merged status; a
                    // detached HEAD has no branch to be merged, so it stays neutral.
                    let color = match self.branch_merged(wt.branch.as_deref()) {
                        Some(true) => MERGED_COLOR,
                        Some(false) => UNMERGED_COLOR,
                        None => NEUTRAL_COLOR,
                    };
                    let branch_label =
                        wt.branch.clone().unwrap_or_else(|| "detached".to_string());
                    // Parity with a branch row: a worktree row selects its branch and
                    // opens the same diff panel. A detached worktree has no branch, so
                    // it keeps none of the affordances rather than becoming a dead click.
                    let selects = Self::worktree_selects_branch(wt);
                    let is_selected =
                        selects.is_some() && self.git_selected_branch == selects;
                    let card = v_flex()
                        .id(("worktree-row", ix))
                        .w_full()
                        .gap_1()
                        .py_1()
                        .px_2()
                        .border_b_1()
                        .border_color(theme::border())
                        .when_some(selects, |d, name| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(theme::border()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_branch(name.clone());
                                    cx.notify();
                                }))
                        })
                        .when(is_selected, |d| d.bg(theme::border()))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .justify_between()
                                .items_center()
                                .child(Self::commit_token(&wt.head, color))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_right()
                                        .text_color(theme::text_muted())
                                        .child(branch_label),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_xs()
                                .font_family("Cascadia Code")
                                .text_color(theme::text_muted())
                                .child(wt.path.clone()),
                        );
                    // The diff panel goes UNDER the row that was clicked, not after the
                    // whole list — the same accordion the branch rows use, and the same
                    // reason (a panel below 100 rows reads as a dead click).
                    let mut entry = v_flex().w_full().child(card);
                    if let (true, Some(name)) = (is_selected, wt.branch.as_deref()) {
                        entry = entry.child(self.branch_diff_panel(name, cx));
                    }
                    list = list.child(entry);
                }
                list.into_any_element()
            }
        };

        rows
    }

    /// Whether a branch (by name) has landed in `main` — looked up in the already-loaded
    /// branch list. `None` when the branch is unknown or absent (e.g. a detached HEAD).
    fn branch_merged(&self, branch: Option<&str>) -> Option<bool> {
        let name = branch?;
        self.git_branches
            .as_ref()?
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.merged)
    }

    // ── Graph ────────────────────────────────────────────────────────────────────

    /// Recompute the Graph subtab's rows from the raw `git log --graph` string: parse,
    /// drop swarm snapshots unless the toggle is on, collapse orphaned connector runs,
    /// and size the rail gutter from the widest lane.
    ///
    /// Derived state on purpose. GPUI re-renders the whole window on every hover
    /// transition, and doing this work *in* the render was what made the tab lag: an
    /// uncapped `git log` re-parsed per mouse-move. Call this wherever an input changes
    /// — the graph string or the snapshot toggle — and never from a render path.
    pub(in crate::app) fn rebuild_graph_rows(&mut self) {
        let show_snapshots = self.git_show_snapshots;
        let rows = match self.git_log_graph.as_deref() {
            Some(graph) if !graph.trim().is_empty() => {
                let kept: Vec<crate::vcs::GraphRow> = crate::vcs::parse_graph(graph)
                    .into_iter()
                    .filter(|r| show_snapshots || !Self::is_swarm_snapshot(r))
                    .collect();
                Self::collapse_connectors(kept)
            }
            _ => Vec::new(),
        };
        self.git_graph_max_lanes = rows
            .iter()
            .flat_map(|r| r.lanes.iter().map(|l| l.from_col.max(l.to_col)))
            .max()
            .map_or(1, |m| m + 1);
        self.git_graph_rows = rows;
        // Every cached item height belongs to the old row set.
        self.git_graph_list.reset(self.git_graph_rows.len());
    }

    /// A `hadron`-authored `before <seat>` commit — the daemon's own pre-turn snapshot.
    /// Roughly 60% of this repo's history, and never what a human came to the graph for.
    pub(super) fn is_swarm_snapshot(row: &crate::vcs::GraphRow) -> bool {
        row.author.as_deref() == Some("hadron") && row.subject.starts_with("before ")
    }

    fn git_graph_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_snapshots = self.git_show_snapshots;
        let header = h_flex()
            .w_full()
            .justify_end()
            .items_center()
            .child(
                h_flex()
                    .id("snapshot-toggle")
                    .gap_1p5()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.git_show_snapshots = !this.git_show_snapshots;
                        this.rebuild_graph_rows();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if show_snapshots { theme::accent() } else { theme::text_muted() })
                            .child(if show_snapshots { "✓ Swarm snapshots" } else { "Swarm snapshots" }),
                    ),
            );

        // Rows come from `git_graph_rows` (rebuilt on load and on the snapshot toggle),
        // never re-parsed here: this function runs on every hover transition.
        let body: gpui::AnyElement = if self.git_log_graph.is_none() {
            Self::muted("Failed to load commit graph.").into_any_element()
        } else if self.git_graph_rows.is_empty() {
            Self::muted("No commits.").into_any_element()
        } else {
            let weak_view = cx.entity().downgrade();
            div()
                .flex_1()
                .min_h_0()
                .child(
                    gpui::list(self.git_graph_list.clone(), move |ix, _window, cx| {
                        match weak_view.upgrade() {
                            Some(view) => view.update(cx, |this, cx| this.graph_row(ix, cx)),
                            None => div().into_any_element(),
                        }
                    })
                    .size_full(),
                )
                .into_any_element()
        };

        v_flex()
            .size_full()
            .gap_1()
            .text_sm()
            .child(header.flex_none())
            .child(body)
    }

    /// One row of the commit graph, built on demand by the virtual list — so only the
    /// rows on screen (plus overdraw) ever construct elements or paint a lane canvas.
    fn graph_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(row) = self.git_graph_rows.get(ix) else {
            return div().into_any_element();
        };
        let max_lanes = self.git_graph_max_lanes;
        let row_h = if row.hash.is_none() { 12.0 } else { 24.0 };

        let mut line = h_flex()
            .id(("graph-row", ix))
            .w_full()
            .gap_2()
            .items_center()
            .h(px(row_h))
            .px_1()
            .rounded_md()
            .overflow_hidden()
            .child(Self::render_rail_canvas(row, max_lanes, row_h));

        let Some(hash) = &row.hash else {
            return line.into_any_element();
        };

        let full_hash = row.full_hash.clone().unwrap_or_else(|| hash.clone());
        let is_selected = self.git_selected_commit.as_deref() == Some(full_hash.as_str());

        let lane = row.node_col.unwrap_or(0);
        let lane_color = LANE_COLORS[lane % LANE_COLORS.len()];
        line = line.child(
            div()
                .flex_none()
                .font_family("Cascadia Code")
                .text_color(gpui::rgb(lane_color))
                .child(hash.clone()),
        );

        // Cap displayed ref pills to max 2 + overflow count chip
        let refs = Self::distinct_refs(&row.decorations);
        let (display_decos, overflow_count) = if refs.len() > 2 {
            (&refs[..2], refs.len() - 2)
        } else {
            (&refs[..], 0)
        };
        for dec in display_decos {
            line = line.child(Self::ref_pill(dec));
        }
        if overflow_count > 0 {
            let overflow_names: Vec<String> = refs[2..].iter().map(|d| d.name.clone()).collect();
            let overflow_tip = overflow_names.join(", ");
            let elem_id = SharedString::from(format!("overflow-tags-{}", row.hash.as_deref().unwrap_or("")));
            line = line.child(
                div()
                    .id(elem_id)
                    .flex_none()
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .text_xs()
                    .bg(gpui::rgba(0x94a3b822))
                    .text_color(gpui::rgb(0x94a3b8))
                    .tooltip(move |window, cx| Tooltip::new(overflow_tip.clone()).build(window, cx))
                    .child(format!("+{}", overflow_count)),
            );
        }

        line = line.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme::text())
                .child(row.subject.clone()),
        );
        if let Some(author) = &row.author {
            line = line.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(author.clone()),
            );
        }
        if let Some(date) = &row.relative_date {
            line = line.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(date.clone()),
            );
        }

        let click_hash = full_hash.clone();
        line = line
            .cursor_pointer()
            .hover(|s| s.bg(theme::border()))
            .when(is_selected, |d| d.bg(theme::border()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_commit(click_hash.clone());
                cx.notify();
            }));

        let mut entry = v_flex().w_full().child(line);
        if is_selected {
            let continuation_rail = Self::render_rail_continuation_canvas(row, max_lanes);
            let expanded_panel = h_flex()
                .w_full()
                .px_1()
                .child(continuation_rail)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(self.commit_diff_panel(&full_hash, cx)),
                );
            entry = entry.child(expanded_panel);
        }
        entry.into_any_element()
    }

    /// Render vertical continuation lanes for an expanded commit row's diff panel
    /// so the commit graph line continues uninterrupted alongside the indented changes panel.
    fn render_rail_continuation_canvas(
        row: &crate::vcs::GraphRow,
        max_lanes: usize,
    ) -> impl IntoElement {
        let gutter_w = (max_lanes.max(1) as f32) * LANE_W;
        let mut active_cols = Vec::new();
        for lane in &row.lanes {
            if !active_cols.contains(&lane.to_col) {
                active_cols.push(lane.to_col);
            }
        }

        div()
            .w(px(gutter_w))
            .h_full()
            .flex_none()
            .child(
                gpui::canvas(
                    move |bounds, _, _| bounds,
                    move |bounds, _, window, _cx| {
                        let h = bounds.size.height;
                        for &col in &active_cols {
                            let x = bounds.origin.x + px((col as f32) * LANE_W + LANE_W / 2.0);
                            let y1 = bounds.origin.y;
                            let _y2 = bounds.origin.y + h;
                            let color = gpui::rgb(LANE_COLORS[col % LANE_COLORS.len()]);

                            let line_w = px(2.0);
                            let line_bounds = gpui::Bounds {
                                origin: gpui::point(x - line_w / 2.0, y1),
                                size: gpui::size(line_w, h),
                            };
                            window.paint_quad(gpui::fill(line_bounds, color));
                        }
                    },
                )
                .size_full(),
            )
    }

    /// Render graph lanes and commit dot on a GPUI canvas.
    fn render_rail_canvas(row: &crate::vcs::GraphRow, max_lanes: usize, row_h: f32) -> impl IntoElement {
        let lanes = row.lanes.clone();
        let node_col = row.node_col;
        let gutter_w = (max_lanes.max(1) as f32) * LANE_W;

        div()
            .w(px(gutter_w))
            .h(px(row_h))
            .flex_none()
            .child(
                gpui::canvas(
                    move |bounds, _, _| bounds,
                    move |bounds, _, window, _cx| {
                    let h = bounds.size.height;
                    for lane in &lanes {
                        let x1 = bounds.origin.x + px((lane.from_col as f32) * LANE_W + LANE_W / 2.0);
                        let y1 = bounds.origin.y;
                        let x2 = bounds.origin.x + px((lane.to_col as f32) * LANE_W + LANE_W / 2.0);
                        let y2 = bounds.origin.y + h;
                        let color = gpui::rgb(LANE_COLORS[Self::lane_color_index(lane) % LANE_COLORS.len()]);

                        if lane.from_col == lane.to_col {
                            let line_w = px(2.0);
                            let line_bounds = gpui::Bounds {
                                origin: gpui::point(x1 - line_w / 2.0, y1),
                                size: gpui::size(line_w, h),
                            };
                            window.paint_quad(gpui::fill(line_bounds, color));
                        } else {
                            let mut builder = gpui::PathBuilder::stroke(px(2.0));
                            builder.move_to(gpui::point(x1, y1));
                            let mid_y = y1 + (y2 - y1) / 2.0;
                            builder.cubic_bezier_to(
                                gpui::point(x2, y2),
                                gpui::point(x1, mid_y),
                                gpui::point(x2, mid_y),
                            );
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, color);
                            }
                        }
                    }

                    if let Some(col) = node_col {
                        let nx = bounds.origin.x + px((col as f32) * LANE_W + LANE_W / 2.0);
                        let ny = bounds.origin.y + h / 2.0;
                        let node_color = gpui::rgb(LANE_COLORS[col % LANE_COLORS.len()]);

                        // Outer ring (background field color)
                        let ring_r = px(5.5);
                        let ring_bounds = gpui::Bounds {
                            origin: gpui::point(nx - ring_r, ny - ring_r),
                            size: gpui::size(ring_r * 2.0, ring_r * 2.0),
                        };
                        window.paint_quad(gpui::fill(ring_bounds, theme::field_base()).corner_radii(ring_r));

                        // Inner filled node dot
                        let dot_r = px(4.0);
                        let dot_bounds = gpui::Bounds {
                            origin: gpui::point(nx - dot_r, ny - dot_r),
                            size: gpui::size(dot_r * 2.0, dot_r * 2.0),
                        };
                        window.paint_quad(gpui::fill(dot_bounds, node_color).corner_radii(dot_r));
                    }
                    },
                )
                // A `canvas` styles itself: its default size is `auto`, which resolves to
                // ZERO height inside this fixed-height parent (measured: `32px × 0px`).
                // Every vertical lane quad then had height 0 and vanished, every diagonal
                // flattened to a horizontal dash, and each node dot centred on the row's
                // top edge and got clipped to a semicircle — the "graph is still dashes"
                // Jake kept reporting. The canvas must carry the row's size itself.
                .w(px(gutter_w))
                .h(px(row_h)),
            )
    }

    /// Determine the color lane index for a lane segment.
    /// For vertical lines (from_col == to_col), uses from_col.
    /// For diagonal connectors (merges or branch creation), uses max(from_col, to_col)
    /// so that a branch connecting to/from main (col 0) is colored with the subbranch's color
    /// (e.g. blue for col 1) rather than main's color (green for col 0).
    pub(super) fn lane_color_index(lane: &crate::vcs::LaneSeg) -> usize {
        lane.from_col.max(lane.to_col)
    }

    /// Collapse each run of consecutive connector rows into a single strip carrying the
    /// union of the run's lane segments. Hiding the swarm-snapshot commits leaves their
    /// connector rows behind, so a merge drew the same `|/` hook once per orphaned row —
    /// a ladder of repeated curves separated by blank strips. One strip per run draws one
    /// curve per merge. Approximate by construction: a lane that travels several columns
    /// across several rows is flattened into one row's worth of segments.
    fn collapse_connectors(rows: Vec<crate::vcs::GraphRow>) -> Vec<crate::vcs::GraphRow> {
        let mut out: Vec<crate::vcs::GraphRow> = Vec::with_capacity(rows.len());
        let mut pending: Option<crate::vcs::GraphRow> = None;
        for row in rows {
            if row.hash.is_none() {
                match &mut pending {
                    Some(acc) => {
                        for seg in row.lanes {
                            if !acc.lanes.contains(&seg) {
                                acc.lanes.push(seg);
                            }
                        }
                    }
                    None => pending = Some(row),
                }
            } else {
                out.extend(pending.take());
                out.push(row);
            }
        }
        out.extend(pending);
        out
    }

    /// The ref pills to render for one commit, deduplicated by the *elided* label.
    /// Eliding `quark/<seat>/<ulid>` down to `<seat>` makes two different branches of
    /// the same seat render as the same chip, so a commit carrying two `acp-claude-2`
    /// branches showed `acp-claude-2  acp-claude-2`. Keep the first of each label (git
    /// lists HEAD and local branches first) so the `+N` chip counts only what is hidden.
    fn distinct_refs(decos: &[crate::vcs::RefDecoration]) -> Vec<crate::vcs::RefDecoration> {
        let mut seen = std::collections::HashSet::new();
        decos
            .iter()
            .filter(|d| seen.insert(Self::elide_ref_name(&d.name)))
            .cloned()
            .collect()
    }

    /// Elide long branch names like `quark/cli-agy/01KY8CNJT01HHWB...` -> `cli-agy`.
    /// Non-quark refs (e.g. `origin/main`, `origin/HEAD`, `main`) are preserved as-is.
    fn elide_ref_name(name: &str) -> String {
        if let Some(clean) = name.strip_prefix("quark/") {
            if let Some((seat, _ulid)) = clean.split_once('/') {
                return seat.to_string();
            }
        }
        name.to_string()
    }

    /// A styled ref pill badge depending on decoration kind (HEAD, Local, Remote, Tag).
    fn ref_pill(dec: &crate::vcs::RefDecoration) -> impl IntoElement {
        let (bg, text_color) = match dec.kind {
            crate::vcs::RefKind::Head => (gpui::rgba(0x34d39922), gpui::rgb(0x34d399)),
            crate::vcs::RefKind::LocalBranch => (gpui::rgba(0xa78bfa22), gpui::rgb(0xa78bfa)),
            crate::vcs::RefKind::RemoteBranch => (gpui::rgba(0x60a5fa22), gpui::rgb(0x60a5fa)),
            crate::vcs::RefKind::Tag => (gpui::rgba(0xfbbf2422), gpui::rgb(0xfbbf24)),
        };

        let short_name = Self::elide_ref_name(&dec.name);
        let full_name = dec.name.clone();

        let label_element = if dec.kind == crate::vcs::RefKind::Head {
            h_flex()
                .gap_1()
                .items_center()
                .child(div().child("●"))
                .child(div().child(short_name))
        } else {
            h_flex().child(short_name)
        };

        div()
            .id(SharedString::from(format!("ref-pill-{}", dec.name)))
            .flex_none()
            .max_w(px(DECO_CHIP_MAX_W))
            .truncate()
            .px_1p5()
            .py_0p5()
            .rounded_md()
            .text_xs()
            .bg(bg)
            .text_color(text_color)
            .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
            .child(label_element)
    }

    // ── Shared collapsible file-diff list (Changes + branch panel, SSOT) ──────────

    /// One collapsible row per changed file: `path  +A −R`, expanding to its hunks.
    /// Shared by the Changes rail and the per-branch diff so both render identically;
    /// `panel` only selects which open-row set a toggle click mutates.
    pub(super) fn file_diff_rows(
        &self,
        diffs: &[crate::vcs::FileDiff],
        open: &std::collections::HashSet<usize>,
        panel: DiffPanel,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut list = v_flex().w_full();
        for (ix, file) in diffs.iter().enumerate() {
            let title = h_flex()
                .flex_1()
                .gap_2()
                .items_center()
                .min_w_0()
                .child(div().flex_1().min_w_0().child(file.path.clone()))
                .child(div().text_color(gpui::rgb(ADD_COLOR)).child(format!("+{}", file.added)))
                .child(div().text_color(gpui::rgb(DEL_COLOR)).child(format!("−{}", file.removed)));

            let is_open = open.contains(&ix);
            let header = h_flex()
                .id(("diff-file", ix))
                .w_full()
                .gap_2()
                .justify_between()
                .items_center()
                .py_1()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    let set = match panel {
                        DiffPanel::Changes => &mut this.changes_open_ixs,
                        DiffPanel::Branch => &mut this.git_branch_open_ixs,
                        DiffPanel::Commit => &mut this.git_commit_open_ixs,
                    };
                    if set.contains(&ix) {
                        set.remove(&ix);
                    } else {
                        set.insert(ix);
                    }
                    // This panel is nested inside the selected commit's virtual-list
                    // row, so opening a file grows that row too.
                    if panel == DiffPanel::Commit {
                        if let Some(hash) = this.git_selected_commit.clone() {
                            this.invalidate_graph_row(&hash);
                        }
                    }
                    cx.notify();
                }))
                .child(title)
                .child(
                    Icon::new(if is_open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .small()
                    .text_color(theme::text_muted()),
                );

            let mut row = v_flex().w_full().child(header);
            if is_open {
                let mut lines = v_flex()
                    .w_full()
                    .text_sm()
                    .pt_2()
                    .font_family("Cascadia Code");
                for hunk in &file.hunks {
                    lines = lines.child(
                        div()
                            .w_full()
                            .px_2()
                            .py_1()
                            .text_color(theme::text_muted())
                            .child(hunk.header.clone()),
                    );
                    for line in &hunk.lines {
                        lines = lines.child(match line {
                            crate::vcs::DiffLine::Context(c) => div()
                                .w_full()
                                .px_2()
                                .text_color(theme::text())
                                .child(format!(" {c}")),
                            crate::vcs::DiffLine::Added(a) => div()
                                .w_full()
                                .px_2()
                                .bg(gpui::rgba(0x34d39922))
                                .text_color(gpui::rgb(ADD_COLOR))
                                .child(format!("+{a}")),
                            crate::vcs::DiffLine::Removed(r) => div()
                                .w_full()
                                .px_2()
                                .bg(gpui::rgba(0xfb718522))
                                .text_color(gpui::rgb(DEL_COLOR))
                                .child(format!("−{r}")),
                        });
                    }
                }
                row = row.child(lines);
            }
            list = list.child(row.border_b_1().border_color(theme::border()));
        }
        list.into_any_element()
    }

    // ── Delegation ─────────────────────────────────────────────────────────────

    fn git_delegation_section(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        use crate::app::delegation::DelegationState;

        if self.delegations.is_empty() {
            return Self::muted("No delegations recorded.").into_any_element();
        }

        let mut list = v_flex().w_full().gap_2();
        for (ix, del) in self.delegations.iter().enumerate().rev() {
            let (from_identity, from_str) = match &del.from {
                Actor::Human => {
                    let id = self.resolve_identity("human");
                    let name = id.name.clone();
                    (id, name)
                }
                Actor::Gluon => {
                    let id = self.resolve_identity("gluon");
                    let name = id.name.clone();
                    (id, name)
                }
                Actor::Quark(q) => {
                    let id = self.resolve_identity(q.as_str());
                    let name = format!("@{}", id.name);
                    (id, name)
                }
            };
            let to_identity = self.resolve_identity(del.to.as_str());
            let to_str = format!("@{}", to_identity.name);

            let (state_color, state_text) = match del.state {
                DelegationState::Pending => (0xf59e0b, "Pending"),
                DelegationState::Working => (0x60a5fa, "Working"),
                DelegationState::Completed => (0x34d399, "Completed"),
                DelegationState::Blocked => (0x94a3b8, "Blocked"),
                DelegationState::Error => (0xf87171, "Error"),
            };

            let first_line = del.task.lines().next().unwrap_or("").trim();
            let truncated_task: String = first_line.chars().take(80).collect();

            let card = v_flex()
                .id(("delegation-row", ix))
                .w_full()
                .p_2()
                .rounded_md()
                .bg(theme::glass_card())
                .border_1()
                .border_color(theme::glass_highlight())
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_xs()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(from_identity.color)
                                        .child(from_str),
                                )
                                .child(div().text_color(theme::text_muted()).child("➜"))
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(to_identity.color)
                                        .child(to_str),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_xs()
                                .text_color(gpui::rgb(state_color))
                                .child(div().child("●"))
                                .child(div().child(state_text)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .truncate()
                        .child(truncated_task),
                );

            list = list.child(card);
        }

        list.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::Chamber;
    use crate::vcs::{RefDecoration, RefKind, WorktreeInfo};

    fn worktree(branch: Option<&str>) -> WorktreeInfo {
        WorktreeInfo {
            path: "/tmp/wt".into(),
            head: "abc1234".into(),
            branch: branch.map(str::to_string),
        }
    }

    /// Worktree rows used to be inert while branch rows were clickable — the same
    /// list shape with a different affordance. They select their branch now, which
    /// makes "is this row clickable" a real decision: a **detached** worktree has no
    /// branch to select, so it must stay non-interactive rather than become a row
    /// that looks live and does nothing.
    #[test]
    fn a_detached_worktree_row_is_not_clickable() {
        assert_eq!(
            Chamber::worktree_selects_branch(&worktree(Some("main"))),
            Some("main".to_string())
        );
        assert_eq!(Chamber::worktree_selects_branch(&worktree(None)), None);
    }

    fn deco(name: &str, kind: RefKind) -> RefDecoration {
        RefDecoration { name: name.into(), kind }
    }

    /// Two branches of the same seat elide to the same label; the pill row must not
    /// render that label twice (`acp-claude-2  acp-claude-2` in the Graph tab).
    #[test]
    fn distinct_refs_collapses_same_seat_branches() {
        let decos = vec![
            deco("quark/acp-claude-2/01KY8GB52K8CY8YGE8N2TYPNBD", RefKind::LocalBranch),
            deco("quark/acp-claude-2/01KY8GPXR6JG18VZCQWRNPTN62", RefKind::LocalBranch),
        ];
        let out = Chamber::distinct_refs(&decos);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "quark/acp-claude-2/01KY8GB52K8CY8YGE8N2TYPNBD");
    }

    /// Distinct refs survive: HEAD, another seat, and a tag are three different pills.
    #[test]
    fn distinct_refs_keeps_different_labels() {
        let decos = vec![
            deco("main", RefKind::Head),
            deco("quark/cli-agy/01KY8CCE0YZV5X8NXMYNNNJMHF", RefKind::LocalBranch),
            deco("v1.0.0", RefKind::Tag),
        ];
        assert_eq!(Chamber::distinct_refs(&decos).len(), 3);
    }

    #[test]
    fn elide_ref_name_preserves_remote_branches() {
        assert_eq!(Chamber::elide_ref_name("origin/main"), "origin/main");
        assert_eq!(Chamber::elide_ref_name("origin/HEAD"), "origin/HEAD");
        assert_eq!(
            Chamber::elide_ref_name("quark/cli-agy/01KY8CCE0YZV5X8NXMYNNNJMHF"),
            "cli-agy"
        );
    }

    fn authored(author: &str, subject: &str) -> crate::vcs::GraphRow {
        crate::vcs::GraphRow {
            hash: Some("aaa1111".into()),
            author: Some(author.into()),
            subject: subject.into(),
            ..Default::default()
        }
    }

    /// The daemon's pre-turn snapshots are hidden by default — they are ~60% of this
    /// repo's history and nothing a human came to the graph to read.
    #[test]
    fn a_hadron_before_commit_is_a_swarm_snapshot() {
        assert!(Chamber::is_swarm_snapshot(&authored("hadron", "before acp-claude")));
    }

    /// Both halves of the rule matter: a human's commit that happens to start with
    /// "before", and a `hadron`-authored commit that is real work, must both survive.
    #[test]
    fn only_hadron_authored_before_commits_are_snapshots() {
        assert!(!Chamber::is_swarm_snapshot(&authored("Jake", "before we ship, fix this")));
        assert!(!Chamber::is_swarm_snapshot(&authored("hadron", "feat(chamber): real work")));
        assert!(!Chamber::is_swarm_snapshot(&crate::vcs::GraphRow::default()));
    }

    fn connector(lanes: Vec<crate::vcs::LaneSeg>) -> crate::vcs::GraphRow {
        crate::vcs::GraphRow { lanes, ..Default::default() }
    }

    fn commit(hash: &str) -> crate::vcs::GraphRow {
        crate::vcs::GraphRow { hash: Some(hash.into()), ..Default::default() }
    }

    /// A run of orphaned connector rows (left behind when snapshot commits are hidden)
    /// collapses to one strip, so a merge draws one curve instead of a ladder of them.
    #[test]
    fn collapse_connectors_folds_a_run_into_one_strip() {
        let seg = crate::vcs::LaneSeg { from_col: 1, to_col: 0 };
        let trunk = crate::vcs::LaneSeg { from_col: 0, to_col: 0 };
        let rows = vec![
            commit("aaa1111"),
            connector(vec![trunk.clone(), seg.clone()]),
            connector(vec![trunk.clone(), seg.clone()]),
            connector(vec![trunk.clone()]),
            commit("bbb2222"),
        ];
        let out = Chamber::collapse_connectors(rows);
        assert_eq!(out.len(), 3, "one strip between the two commits");
        assert_eq!(out[1].lanes, vec![trunk, seg]);
    }

    /// Commit rows are never merged, and a trailing run still renders.
    #[test]
    fn collapse_connectors_keeps_commits_and_a_trailing_run() {
        let trunk = crate::vcs::LaneSeg { from_col: 0, to_col: 0 };
        let rows = vec![
            commit("aaa1111"),
            commit("bbb2222"),
            connector(vec![trunk.clone()]),
            connector(vec![trunk.clone()]),
        ];
        let out = Chamber::collapse_connectors(rows);
        assert_eq!(out.len(), 3);
        assert!(out[2].hash.is_none());
    }

    #[test]
    fn lane_color_index_uses_subbranch_color_for_diagonal_connectors() {
        use crate::vcs::LaneSeg;
        // Main trunk (col 0) -> green (0)
        assert_eq!(Chamber::lane_color_index(&LaneSeg { from_col: 0, to_col: 0 }), 0);
        // Branch 1 trunk (col 1) -> blue (1)
        assert_eq!(Chamber::lane_color_index(&LaneSeg { from_col: 1, to_col: 1 }), 1);
        // Merge connector (main col 0 -> branch col 1) -> blue (1)
        assert_eq!(Chamber::lane_color_index(&LaneSeg { from_col: 0, to_col: 1 }), 1);
        // Creation connector (branch col 1 -> main col 0) -> blue (1)
        assert_eq!(Chamber::lane_color_index(&LaneSeg { from_col: 1, to_col: 0 }), 1);
    }

    #[test]
    fn render_rail_continuation_canvas_constructs_element() {
        use crate::vcs::LaneSeg;
        let row = crate::vcs::GraphRow {
            lanes: vec![
                LaneSeg { from_col: 0, to_col: 0 },
                LaneSeg { from_col: 1, to_col: 1 },
            ],
            ..Default::default()
        };
        // Verify continuation canvas helper constructs without panicking for multiple lanes
        let _elem = Chamber::render_rail_continuation_canvas(&row, 2);
    }

    #[test]
    fn select_commit_populates_message_and_diff() {
        let root = crate::vcs::repo_root_of(std::path::Path::new("."));
        let msg = crate::vcs::commit_message(root, "HEAD");
        let diff = crate::vcs::commit_diff(root, "HEAD");
        assert!(msg.is_some());
        assert!(!msg.unwrap().is_empty());
        assert!(diff.is_some());
    }
}
