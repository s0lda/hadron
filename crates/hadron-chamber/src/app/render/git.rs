use super::*;

/// Which collapsible-diff panel a toggle click targets. The Changes rail and the
/// per-branch diff share one renderer (`file_diff_rows`) but keep independent
/// open-row sets, so opening a file in one doesn't open it in the other.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DiffPanel {
    Changes,
    Branch,
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
/// One graph rail column, in px — a fixed cell keeps lanes in column regardless of
/// the glyph's own advance width.
const RAIL_CELL_W: f32 = 9.0;
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
            GitSubtab::Graph => self.git_graph_section().into_any_element(),
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

    fn git_graph_section(&self) -> impl IntoElement {
        let body: gpui::AnyElement = match &self.git_log_graph {
            None => Self::muted("Failed to load commit graph.").into_any_element(),
            Some(graph) if graph.trim().is_empty() => {
                Self::muted("No commits.").into_any_element()
            }
            Some(graph) => {
                let rows = crate::vcs::parse_graph(graph);
                let mut list = v_flex().w_full().text_sm();
                for row in rows {
                    // One row = one line, always. Without the clip a long ref chip
                    // (`quark/acp-claude/01KY…`) forces the subject into a ~100px
                    // column that wraps character-by-character.
                    let mut line = h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .py_0p5()
                        .overflow_hidden()
                        .child(Self::render_rail(&row.rail));

                    if let Some(hash) = &row.hash {
                        // The commit's own lane is the column of its `*` marker, so its
                        // hash matches that lane's colour (the rail can run past it).
                        let lane = row.rail.chars().position(|c| c == '*').unwrap_or(0);
                        let lane_color = LANE_COLORS[lane % LANE_COLORS.len()];
                        line = line.child(
                            div()
                                .flex_none()
                                .font_family("Cascadia Code")
                                .text_color(gpui::rgb(lane_color))
                                .child(hash.clone()),
                        );
                        for dec in &row.decorations {
                            line = line.child(Self::deco_chip(&dec.name));
                        }
                        line = line.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme::text())
                                .child(row.subject.clone()),
                        );
                    }
                    list = list.child(line);
                }
                list.into_any_element()
            }
        };

        v_flex()
            .w_full()
            .gap_1()
            .child(Self::git_section_title("Commit graph"))
            .child(body)
    }

    /// The graph rail for one row: one span per column so each lane keeps its colour,
    /// with git's `*` commit marker drawn as a filled dot.
    fn render_rail(rail: &str) -> impl IntoElement {
        let mut h = h_flex().font_family("Cascadia Code").flex_none();
        for (col, ch) in rail.chars().enumerate() {
            let color = LANE_COLORS[col % LANE_COLORS.len()];
            let glyph = if ch == '*' { "●".to_string() } else { ch.to_string() };
            // Fixed cell width: `●` is not a Cascadia glyph, so on fallback it is
            // wider than a cell and the lanes drift out of column.
            h = h.child(
                div()
                    .w(px(RAIL_CELL_W))
                    .flex_none()
                    .text_color(gpui::rgb(color))
                    .child(glyph),
            );
        }
        h
    }

    /// A ref-label chip (branch/tag) from `--decorate`, e.g. `HEAD -> main`.
    /// Width-capped: a full `quark/<seat>/<ulid>` branch name is ~40 chars with no
    /// break opportunity, so an uncapped chip claims the whole row.
    fn deco_chip(label: &str) -> impl IntoElement {
        div()
            .flex_none()
            .max_w(px(DECO_CHIP_MAX_W))
            .truncate()
            .px_1()
            .rounded_sm()
            .text_xs()
            .bg(gpui::rgba(0x38bdf822))
            .text_color(gpui::rgb(0x38bdf8))
            .child(label.to_string())
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
