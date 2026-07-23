use super::*;

impl super::Chamber {
    /// The Git rail: local branches (with merged-into-`main` status), every worktree
    /// of this repo, and a short `git log --graph` — so "is it merged?" and "who else
    /// has a checkout" are answerable without leaving the chamber.
    pub(super) fn git_tab_content(&self) -> impl IntoElement {
        let branches_section = self.git_branches_section();
        let worktrees_section = self.git_worktrees_section();
        let graph_section = self.git_graph_section();

        let body = v_flex()
            .w_full()
            .gap_4()
            .child(branches_section)
            .child(worktrees_section)
            .child(graph_section);

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
                    .p_3()
                    .text_sm()
                    .text_color(theme::text())
                    .child(body),
            )
            .child(
                div().absolute().top_0().bottom_0().right_0().child(
                    Scrollbar::vertical(&self.git_scroll).scrollbar_show(ScrollbarShow::Hover),
                ),
            )
    }

    fn git_section_title(title: &'static str) -> impl IntoElement {
        div()
            .text_xs()
            .text_color(theme::text_muted())
            .child(title)
    }

    fn git_branches_section(&self) -> impl IntoElement {
        let rows: gpui::AnyElement = match &self.git_branches {
            None => div()
                .text_color(theme::text_muted())
                .child("Failed to load branches.")
                .into_any_element(),
            Some(branches) if branches.is_empty() => div()
                .text_color(theme::text_muted())
                .child("No local branches.")
                .into_any_element(),
            Some(branches) => {
                let mut list = v_flex().w_full();
                for branch in branches {
                    let (badge_text, badge_color) = if branch.merged {
                        ("merged", gpui::rgb(0x34d399))
                    } else {
                        ("unmerged", gpui::rgb(0xfb7185))
                    };
                    let row = h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .py_1()
                        .child(
                            div()
                                .when(branch.is_current, |d| d.text_color(theme::accent()))
                                .child(branch.name.clone()),
                        )
                        .child(
                            div()
                                .text_color(theme::text_muted())
                                .font_family("Cascadia Code")
                                .child(branch.head.clone()),
                        )
                        .child(div().text_color(badge_color).child(badge_text));
                    list = list.child(row.border_b_1().border_color(theme::border()));
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

    fn git_worktrees_section(&self) -> impl IntoElement {
        let rows: gpui::AnyElement = match &self.git_worktrees {
            None => div()
                .text_color(theme::text_muted())
                .child("Failed to load worktrees.")
                .into_any_element(),
            Some(worktrees) if worktrees.is_empty() => div()
                .text_color(theme::text_muted())
                .child("No worktrees.")
                .into_any_element(),
            Some(worktrees) => {
                let mut list = v_flex().w_full();
                for wt in worktrees {
                    let branch_label = wt.branch.clone().unwrap_or_else(|| "(detached)".to_string());
                    let row = h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .py_1()
                        .child(div().flex_1().min_w_0().child(wt.path.clone()))
                        .child(div().text_color(theme::text_muted()).child(branch_label))
                        .child(
                            div()
                                .text_color(theme::text_muted())
                                .font_family("Cascadia Code")
                                .child(wt.head.clone()),
                        );
                    list = list.child(row.border_b_1().border_color(theme::border()));
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

    fn git_graph_section(&self) -> impl IntoElement {
        let graph: gpui::AnyElement = match &self.git_log_graph {
            None => div()
                .text_color(theme::text_muted())
                .child("Failed to load commit graph.")
                .into_any_element(),
            Some(graph) if graph.trim().is_empty() => div()
                .text_color(theme::text_muted())
                .child("No commits.")
                .into_any_element(),
            Some(graph) => div()
                .w_full()
                .font_family("Cascadia Code")
                .whitespace_normal()
                .child(graph.clone())
                .into_any_element(),
        };

        v_flex()
            .w_full()
            .gap_1()
            .child(Self::git_section_title("Commit graph"))
            .child(graph)
    }
}
