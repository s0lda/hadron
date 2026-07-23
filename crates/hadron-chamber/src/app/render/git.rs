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
const LANE_COLORS: [u32; 6] = [0x34d399, 0x38bdf8, 0xfbbf24, 0xa78bfa, 0xfb7185, 0x2dd4bf];
/// A commit that has landed in `main` (green) vs one still in flight (rose); a
/// detached / unknown HEAD is neutral (it has no branch to be merged).
const MERGED_COLOR: u32 = 0x34d399;
const UNMERGED_COLOR: u32 = 0xfb7185;
const NEUTRAL_COLOR: u32 = 0x94a3b8;
const ADD_COLOR: u32 = 0x34d399;
const DEL_COLOR: u32 = 0xfb7185;
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
        let subtabs = TabBar::new("git-subtabs")
            .segmented()
            .bg(theme::field_base())
            .selected_index(selected.index())
            .children(GitSubtab::ALL.map(|t| {
                if t.index() == selected.index() {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(t.label().to_string()),
                    )
                } else {
                    Tab::new().label(t.label())
                }
            }))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.git_subtab = GitSubtab::from_index(*ix);
                cx.notify();
            }));

        let body = match selected {
            GitSubtab::Branches => self.git_branches_section(cx).into_any_element(),
            GitSubtab::Worktrees => self.git_worktrees_section().into_any_element(),
            GitSubtab::Graph => self.git_graph_section(cx).into_any_element(),
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .child(h_flex().flex_none().px_3().py_2().child(subtabs))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("git-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.git_scroll)
                            .px_3()
                            .pb_3()
                            .text_sm()
                            .text_color(theme::text())
                            .child(body),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.git_scroll)
                                .scrollbar_show(ScrollbarShow::Hover),
                        ),
                    ),
            )
    }

    fn git_section_title(title: &'static str) -> impl IntoElement {
        div()
            .text_xs()
            .text_color(theme::text_muted())
            .child(title)
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
            None => Self::muted("Failed to load branches.").into_any_element(),
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

        v_flex()
            .w_full()
            .gap_1()
            .child(Self::git_section_title("Branches"))
            .child(rows)
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
            .pl_3()
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
        if self.git_selected_commit.as_deref() == Some(hash.as_str()) {
            self.git_selected_commit = None;
            self.git_commit_diff = None;
            return;
        }
        let root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        self.git_commit_diff = crate::vcs::commit_diff(&root, &hash);
        self.git_commit_open_ixs.clear();
        self.git_selected_commit = Some(hash);
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
            .pl_3()
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
            .child(body)
    }

    // ── Worktrees ────────────────────────────────────────────────────────────────

    fn git_worktrees_section(&self) -> impl IntoElement {
        let rows: gpui::AnyElement = match &self.git_worktrees {
            None => Self::muted("Failed to load worktrees.").into_any_element(),
            Some(worktrees) if worktrees.is_empty() => {
                Self::muted("No worktrees.").into_any_element()
            }
            Some(worktrees) => {
                let mut list = v_flex().w_full().gap_1();
                for wt in worktrees {
                    // Colour the commit token by the branch's merged status; a
                    // detached HEAD has no branch to be merged, so it stays neutral.
                    let color = match self.branch_merged(wt.branch.as_deref()) {
                        Some(true) => MERGED_COLOR,
                        Some(false) => UNMERGED_COLOR,
                        None => NEUTRAL_COLOR,
                    };
                    let branch_label =
                        wt.branch.clone().unwrap_or_else(|| "detached".to_string());
                    let card = v_flex()
                        .w_full()
                        .gap_1()
                        .py_1()
                        .px_2()
                        .border_b_1()
                        .border_color(theme::border())
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
                    list = list.child(card);
                }
                list.into_any_element()
            }
        };

        v_flex()
            .w_full()
            .gap_1()
            .child(Self::git_section_title("Worktrees"))
            .child(rows)
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

    fn git_graph_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_snapshots = self.git_show_snapshots;
        let header = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(Self::git_section_title("Commit graph"))
            .child(
                h_flex()
                    .id("snapshot-toggle")
                    .gap_1p5()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.git_show_snapshots = !this.git_show_snapshots;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if show_snapshots { theme::accent() } else { theme::text_muted() })
                            .child(if show_snapshots { "✓ Swarm snapshots" } else { "Swarm snapshots" }),
                    ),
            );

        let body: gpui::AnyElement = match &self.git_log_graph {
            None => Self::muted("Failed to load commit graph.").into_any_element(),
            Some(graph) if graph.trim().is_empty() => {
                Self::muted("No commits.").into_any_element()
            }
            Some(graph) => {
                let all_rows = crate::vcs::parse_graph(graph);
                let rows: Vec<crate::vcs::GraphRow> = all_rows
                    .into_iter()
                    .filter(|r| {
                        if show_snapshots {
                            return true;
                        }
                        if let (Some(author), subject) = (r.author.as_deref(), r.subject.as_str()) {
                            if author == "hadron" && subject.starts_with("before ") {
                                return false;
                            }
                        }
                        true
                    })
                    .collect();

                let max_lanes = rows
                    .iter()
                    .flat_map(|r| r.lanes.iter().map(|l| l.from_col.max(l.to_col)))
                    .max()
                    .map_or(1, |m| m + 1);

                let mut list = v_flex().w_full().text_sm();
                for (ix, row) in rows.into_iter().enumerate() {
                    let is_connector = row.hash.is_none();
                    let row_h = if is_connector { 12.0 } else { 24.0 };

                    let mut line = h_flex()
                        .id(("graph-row", ix))
                        .w_full()
                        .gap_2()
                        .items_center()
                        .h(px(row_h))
                        .px_1()
                        .rounded_md()
                        .overflow_hidden()
                        .child(Self::render_rail_canvas(&row, max_lanes, row_h));

                    if let Some(hash) = &row.hash {
                        let full_hash = row.full_hash.clone().unwrap_or_else(|| hash.clone());
                        let is_selected =
                            self.git_selected_commit.as_deref() == Some(full_hash.as_str());

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
                        let (display_decos, overflow_count) = if row.decorations.len() > 2 {
                            (&row.decorations[..2], row.decorations.len() - 2)
                        } else {
                            (&row.decorations[..], 0)
                        };
                        for dec in display_decos {
                            line = line.child(Self::ref_pill(dec));
                        }
                        if overflow_count > 0 {
                            line = line.child(
                                div()
                                    .flex_none()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .text_xs()
                                    .bg(gpui::rgba(0x94a3b822))
                                    .text_color(gpui::rgb(0x94a3b8))
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
                            entry = entry.child(self.commit_diff_panel(&full_hash, cx));
                        }
                        list = list.child(entry);
                    } else {
                        list = list.child(line);
                    }
                }
                list.into_any_element()
            }
        };

        v_flex()
            .w_full()
            .gap_1()
            .child(header)
            .child(body)
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
            .child(gpui::canvas(
                move |bounds, _, _| bounds,
                move |bounds, _, window, _cx| {
                    let h = bounds.size.height;
                    for lane in &lanes {
                        let x1 = bounds.origin.x + px((lane.from_col as f32) * LANE_W + LANE_W / 2.0);
                        let y1 = bounds.origin.y;
                        let x2 = bounds.origin.x + px((lane.to_col as f32) * LANE_W + LANE_W / 2.0);
                        let y2 = bounds.origin.y + h;
                        let color = gpui::rgb(LANE_COLORS[lane.from_col % LANE_COLORS.len()]);

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
                            builder.curve_to(
                                gpui::point(x2, y2),
                                gpui::point(x1, y1 + (y2 - y1) / 2.0),
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
            ))
    }

    /// Elide long branch names like `quark/cli-agy/01KY8CNJT01HHWB...` -> `cli-agy`.
    fn elide_ref_name(name: &str) -> String {
        let clean = name.strip_prefix("quark/").unwrap_or(name);
        if let Some((seat, _ulid)) = clean.split_once('/') {
            seat.to_string()
        } else {
            clean.to_string()
        }
    }

    /// A styled ref pill badge depending on decoration kind (HEAD, Local, Remote, Tag).
    fn ref_pill(dec: &crate::vcs::RefDecoration) -> impl IntoElement {
        let (bg, text_color) = match dec.kind {
            crate::vcs::RefKind::Head => (gpui::rgba(0x34d39922), gpui::rgb(0x34d399)),
            crate::vcs::RefKind::LocalBranch => (gpui::rgba(0xa78bfa22), gpui::rgb(0xa78bfa)),
            crate::vcs::RefKind::RemoteBranch => (gpui::rgba(0x38bdf822), gpui::rgb(0x38bdf8)),
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
}
