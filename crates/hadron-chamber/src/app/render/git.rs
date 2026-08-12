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
                        s.bg(theme::bg_elevated())
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
}
