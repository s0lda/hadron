use super::*;
use gpui_component::ActiveTheme;

use super::*;
use gpui_component::ActiveTheme;

impl super::Chamber {
    /// The right rail: the swappable Terminal / File Tree / Changes pane.
    /// (Internally still `Rail::Inspector` for collapse/size.)
    pub(super) fn terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.right_rail_tab;
        let tabs = h_flex()
            .id("right-rail-capsule-tabs")
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(theme::tab_bar_bg())
            .max_w_full()
            .overflow_x_scroll()
            .children(RightRailTab::ALL.map(|t| {
                let is_selected = t.index() == selected.index();
                let label = t.label();
                let ix = t.index();
                div()
                    .id(("right-rail-tab-pill", ix))
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
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.right_rail_tab = RightRailTab::from_index(ix);
                        if this.right_rail_tab == RightRailTab::Terminal {
                            window.focus(&this.terminal_focus, cx);
                        }
                        if this.right_rail_tab == RightRailTab::Changes {
                            let root = crate::vcs::repo_root_of(&this.path);
                            this.working_diff = crate::vcs::working_diff(root);
                        }
                        if this.right_rail_tab == RightRailTab::Git {
                            let root = crate::vcs::repo_root_of(&this.path);
                            this.git_branch_fingerprint = Some(crate::vcs::branch_fingerprint(root));
                            this.git_branches = Some(crate::vcs::list_branches(root, "main"));
                            this.git_worktrees = Some(crate::vcs::list_worktrees(root));
                            this.git_log_graph = crate::vcs::commit_graph(root);
                            this.rebuild_graph_rows();
                        }
                        cx.notify();
                    }))
            }));

        let close_btn = div()
            .id("inspector-close-btn")
            .cursor_pointer()
            .active(|s| s.opacity(0.6))
            .child(Icon::new(IconName::PanelRightClose).small())
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_rail(Rail::Inspector, window, cx)),
            );

        let header = h_flex()
            .id("inspector-header")
            .w_full()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .text_sm()
            .text_color(theme::text_muted())
            .child(tabs)
            .child(close_btn);

        let tab_start =
            std::env::var_os("HADRON_FRAME_TIMING").is_some().then(std::time::Instant::now);

        let content = match selected {
            RightRailTab::Terminal => {
                let sub_tab_bar = h_flex()
                    .id("sub_tab_bar")
                    .items_center()
                    .gap_1()
                    .p_1()
                    .rounded_full()
                    .bg(theme::tab_bar_bg())
                    .max_w_full()
                    .overflow_x_scroll()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .overflow_x_scrollbar()
                            .children(self.terminals.iter().enumerate().map(|(ix, tab)| {
                                let is_active = ix == self.active_terminal_index;
                                h_flex()
                                    .id(SharedString::from(format!("terminal-tab-{ix}")))
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_1()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .when(is_active, |s| {
                                        s.bg(theme::bg_elevated())
                                            .text_color(theme::accent())
                                            .font_weight(gpui::FontWeight::BOLD)
                                    })
                                    .when(!is_active, |s| {
                                        s.text_color(theme::text_muted())
                                            .hover(|h| h.text_color(theme::text()))
                                    })
                                    .text_xs()
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.select_terminal(ix, cx);
                                    }))
                                    .child(tab.title.clone())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("close-terminal-tab-{ix}")))
                                            .flex_shrink_0()
                                            .w_4()
                                            .h_4()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .hover(|h| {
                                                h.bg(theme::bg_elevated())
                                                    .text_color(theme::text())
                                            })
                                            .child("×")
                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                this.close_terminal(ix, cx);
                                            }))
                                            .into_any_element(),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .id("add-terminal-tab")
                            .flex_shrink_0()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .text_color(theme::text_muted())
                            .hover(|h| h.text_color(theme::text()))
                            .text_xs()
                            .child("+")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.add_terminal(cx);
                            })),
                    );

/// The drag payload for the Tasks-tab scrubber head. It renders nothing — the track
/// paints its own head — and exists only because GPUI keys drag tracking on the type.
#[derive(Clone)]
struct TaskScrubDrag;

impl Render for TaskScrubDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabState {
    pub titles: Vec<String>,
    pub active_index: usize,
}

impl Default for TerminalTabState {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl TerminalTabState {
    pub fn new() -> Self {
        Self {
            titles: Vec::new(),
            active_index: 0,
        }
    }

    pub fn add_tab(&mut self, title: impl Into<String>) -> usize {
        self.titles.push(title.into());
        self.active_index = self.titles.len().saturating_sub(1);
        self.active_index
    }

    pub fn select_tab(&mut self, index: usize) -> bool {
        if index < self.titles.len() {
            self.active_index = index;
            true
        } else {
            false
        }
    }

    pub fn close_tab(&mut self, closing_index: usize) -> Option<usize> {
        if closing_index >= self.titles.len() {
            return None;
        }

        self.titles.remove(closing_index);
        if self.titles.is_empty() {
            self.active_index = 0;
        } else if closing_index < self.active_index {
            self.active_index = self.active_index.saturating_sub(1);
        } else if self.active_index >= self.titles.len() {
            self.active_index = self.titles.len() - 1;
        }
        Some(self.active_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_tab_addition_and_auto_focus() {
        let mut state = TerminalTabState::new();
        assert_eq!(state.titles.len(), 0);

        let idx0 = state.add_tab("bash #1");
        assert_eq!(idx0, 0);
        assert_eq!(state.active_index, 0);

        let idx1 = state.add_tab("bash #2");
        assert_eq!(idx1, 1);
        assert_eq!(state.active_index, 1, "Adding tab #2 must auto-focus index 1");
        assert_eq!(state.titles.len(), 2);
    }

    #[test]
    fn test_terminal_tab_selection_valid_and_out_of_bounds() {
        let mut state = TerminalTabState::new();
        state.add_tab("bash #1");
        state.add_tab("bash #2");
        state.add_tab("bash #3");

        assert_eq!(state.active_index, 2);

        // Select valid index 0
        assert!(state.select_tab(0));
        assert_eq!(state.active_index, 0);

        // Select valid index 1
        assert!(state.select_tab(1));
        assert_eq!(state.active_index, 1);

        // Select invalid index 99 -> must return false and preserve active_index=1
        assert!(!state.select_tab(99));
        assert_eq!(state.active_index, 1, "Out of bounds selection must not corrupt active_index");
    }

    #[test]
    fn test_close_middle_active_tab_fallback() {
        let mut state = TerminalTabState::new();
        state.add_tab("tab 0");
        state.add_tab("tab 1");
        state.add_tab("tab 2");

        state.select_tab(1); // Active tab is "tab 1"
        let new_active = state.close_tab(1);

        assert_eq!(new_active, Some(1));
        assert_eq!(state.titles.len(), 2);
        assert_eq!(state.titles[0], "tab 0");
        assert_eq!(state.titles[1], "tab 2");
        assert_eq!(state.active_index, 1, "Active index remains 1, now pointing to 'tab 2'");
    }

    #[test]
    fn test_close_tail_active_tab_fallback() {
        let mut state = TerminalTabState::new();
        state.add_tab("tab 0");
        state.add_tab("tab 1");
        state.add_tab("tab 2");

        state.select_tab(2); // Active tab is tail (index 2)
        let new_active = state.close_tab(2);

        assert_eq!(new_active, Some(1));
        assert_eq!(state.titles.len(), 2);
        assert_eq!(state.active_index, 1, "Closing tail tab decrements active_index to new tail (1)");
    }

    #[test]
    fn test_close_preceding_tab_adjusts_active_index() {
        let mut state = TerminalTabState::new();
        state.add_tab("tab 0");
        state.add_tab("tab 1");
        state.add_tab("tab 2");

        state.select_tab(2); // Active is "tab 2" at index 2
        state.close_tab(0);  // Close preceding tab "tab 0"

        assert_eq!(state.titles.len(), 2);
        assert_eq!(state.titles[1], "tab 2");
        assert_eq!(state.active_index, 1, "Active index shifted from 2 to 1 to track 'tab 2'");
    }

    #[test]
    fn test_close_following_tab_preserves_active_index() {
        let mut state = TerminalTabState::new();
        state.add_tab("tab 0");
        state.add_tab("tab 1");
        state.add_tab("tab 2");

        state.select_tab(0); // Active is "tab 0" at index 0
        state.close_tab(2);  // Close following tab "tab 2"

        assert_eq!(state.titles.len(), 2);
        assert_eq!(state.active_index, 0, "Active index remains 0");
    }

    #[test]
    fn test_close_last_remaining_tab_safe_fallback() {
        let mut state = TerminalTabState::new();
        state.add_tab("tab 0");

        let new_active = state.close_tab(0);

        assert_eq!(new_active, Some(0));
        assert_eq!(state.titles.len(), 0);
        assert_eq!(state.active_index, 0, "Active index remains 0 when vector becomes empty");
    }
}
