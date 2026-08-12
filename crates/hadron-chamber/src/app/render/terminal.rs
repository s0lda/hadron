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
            .bg(theme::glass_card())
            .border_1()
            .border_color(theme::glass_highlight())
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
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .bg(theme::glass_surface())
                    .border_b_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .overflow_x_scrollbar()
                            .children(self.terminals.iter().enumerate().map(|(ix, tab)| {
                                let is_active = ix == self.active_terminal_index;
                                let bg_color = if is_active {
                                    theme::glass_card()
                                } else {
                                    theme::canvas_base()
                                };
                                let text_col = if is_active {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                };

                                h_flex()
                                    .id(SharedString::from(format!("terminal-tab-{ix}")))
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_lg()
                                    .bg(bg_color)
                                    .text_xs()
                                    .text_color(text_col)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.select_terminal(ix, cx);
                                    }))
                                    .child(tab.title.clone())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("close-terminal-tab-{ix}")))
                                            .px_1()
                                            .rounded_sm()
                                            .hover(|s| s.bg(theme::glass_highlight()))
                                            .text_color(theme::text_muted())
                                            .child("×")
                                            .on_click(cx.listener(move |this, _, _window, cx| {
                                                this.close_terminal(ix, cx);
                                            })),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .id("add-terminal-tab")
                            .flex_shrink_0()
                            .px_2()
                            .py_0p5()
                            .rounded_lg()
                            .bg(theme::glass_card())
                            .text_xs()
                            .text_color(theme::text_muted())
                            .hover(|s| s.text_color(theme::text()))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.add_terminal(cx);
                            }))
                            .child("+"),
                    );

                let active_term = self.active_terminal();
                let active_err = self.active_terminal_error();

                let grid: gpui::AnyElement = if let Some(term) = active_term {
                    let snap = term.snapshot();
                    let mut lines = v_flex()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(TERM_FONT))
                        .line_height(px(TERM_CELL_H))
                        .size_full()
                        .p_3();
                    for line in &snap.lines {
                        let mut row = h_flex()
                            .h(px(TERM_CELL_H))
                            .whitespace_nowrap()
                            .min_w_0();
                        let mut line_empty = true;
                        for run in &line.runs {
                            if !run.text.is_empty() {
                                line_empty = false;
                                let mut run_div = div()
                                    .text_color(gpui::rgb(pack_rgb(run.fg)))
                                    .bg(gpui::rgb(pack_rgb(run.bg)));
                                if run.has_cursor {
                                    run_div = run_div
                                        .border_l(px(2.0))
                                        .border_color(gpui::rgb(pack_rgb(run.fg)));
                                }
                                row = row.child(run_div.child(run.text.clone()));
                            }
                        }
                        if line_empty {
                            row = row.child(div().child(" "));
                        }
                        lines = lines.child(row);
                    }
                    lines.into_any_element()
                } else {
                    let msg = active_err.unwrap_or("starting shell…");
                    div()
                        .flex_1()
                        .p_3()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(TERM_FONT))
                        .text_color(theme::text_muted())
                        .child(msg.to_string())
                        .into_any_element()
                };

                let px_cell = self.terminal_px.clone();
                let size_probe = gpui::canvas(
                    move |bounds, _, _| {
                        px_cell.set(Some((
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.y),
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height),
                        )));
                    },
                    |_, _: (), _, _| {},
                )
                .absolute()
                .size_full();

                let screen = div()
                    .id("terminal-screen")
                    .track_focus(&self.terminal_focus)
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .bg(theme::term_bg())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&this.terminal_focus, cx);
                            if let (Some(term), Some((row, col, right))) =
                                (this.active_terminal(), this.terminal_cell_at(ev.position))
                            {
                                term.selection_start(row, col, right, ev.click_count);
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, cx| {
                        if ev.pressed_button == Some(MouseButton::Left) {
                            if let (Some(term), Some((row, col, right))) =
                                (this.active_terminal(), this.terminal_cell_at(ev.position))
                            {
                                term.selection_update(row, col, right);
                                cx.notify();
                            }
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _window, cx| {
                        if let Some(term) = this.active_terminal() {
                            let lines = match ev.delta {
                                gpui::ScrollDelta::Lines(delta) => (delta.y * 3.0) as i32,
                                gpui::ScrollDelta::Pixels(delta) => {
                                    (f32::from(delta.y) / TERM_CELL_H * 3.0) as i32
                                }
                            };
                            if lines != 0 {
                                term.scroll(lines);
                                cx.notify();
                            }
                        }
                    }))
                    .on_key_down(cx.listener(Self::on_terminal_key))
                    .child(size_probe)
                    .child(grid);

                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .gap_2()
                    .child(sub_tab_bar)
                    .child(screen)
                    .into_any_element()
            }
            RightRailTab::FileTree => {
                let mut list = v_flex().size_full();
                if let Some((path, content)) = &self.file_tree_open {
                    list = list
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .p_2()
                                .bg(theme::bg_surface_raised())
                                .child(div().text_color(theme::text()).child(path.clone()))
                                .child(text_button("close-file", "Close").on_click(cx.listener(
                                    |this, _, _window, cx| {
                                        this.parsed_markdown.borrow_mut().remove(&usize::MAX);
                                        this.file_tree_open = None;
                                        cx.notify();
                                    },
                                ))),
                        )
                        .child(
                            div()
                                .id("file-tree-open-container")
                                .flex_1()
                                .min_h_0()
                                .relative()
                                .child(
                                    div()
                                        .id("file-tree-open")
                                        .size_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.file_tree_open_scroll)
                                        .p_2()
                                        .bg(theme::input_bg())
                                        .text_color(theme::text())
                                        // Use a fixed index like usize::MAX for the file tree markdown cache
                                        .child(self.markdown_body(
                                            "file-tree-open",
                                            usize::MAX,
                                            content,
                                            &[],
                                        )),
                                )
                                .vertical_scrollbar(&self.file_tree_open_scroll),
                        );

                    v_flex().flex_1().child(list).into_any_element()
                } else {
                    use crate::sys::{sorted_children, FileTreeNode};

                    let mut root_node = FileTreeNode::default();
                    for (file, is_ignored) in &self.file_tree_paths {
                        root_node.insert(file, *is_ignored, file.ends_with('/'));
                    }
                    root_node.resolve_ignores();

                    let repo_root =
                        crate::vcs::repo_root_of(std::path::Path::new(&self.path)).to_path_buf();

                    fn render_node(
                        name: &str,
                        node: &FileTreeNode,
                        depth: usize,
                        cx: &mut Context<Chamber>,
                        repo_root: &std::path::PathBuf,
                        current_path: String,
                        expanded_set: &std::collections::HashSet<String>,
                        git_statuses: &std::collections::HashMap<String, crate::vcs::GitStatus>,
                    ) -> gpui::AnyElement {
                        let mut list = v_flex().w_full();
                        // root node has empty name and we don't render it directly
                        if name.is_empty() {
                            for (child_name, child_node) in sorted_children(node) {
                                let child_path = child_name.clone();
                                list = list.child(render_node(
                                    child_name,
                                    child_node,
                                    depth,
                                    cx,
                                    repo_root,
                                    child_path,
                                    expanded_set,
                                    git_statuses,
                                ));
                            }
                            return list.into_any_element();
                        }

                        let is_expanded = expanded_set.contains(&current_path);

                        // Stable per-path id — see the roster row: a context menu on
                        // an id-less element shares its state with every sibling.
                        let row = h_flex()
                            .id(SharedString::from(format!("tree-row-{}", node.full_path)))
                            .w_full()
                            .px_2()
                            .py_1()
                            .pl(gpui::px(depth as f32 * 12.0 + 8.0))
                            .rounded_sm()
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                            .cursor_pointer()
                            // Gitignored entries read as present-but-inactive: muted text.
                            .text_color(if node.is_ignored {
                                theme::text_muted()
                            } else {
                                match git_statuses.get(&node.full_path) {
                                    Some(crate::vcs::GitStatus::Modified) => gpui::rgb(0xf59e0b),
                                    Some(crate::vcs::GitStatus::Added) => gpui::rgb(0x34d399),
                                    Some(crate::vcs::GitStatus::Deleted) => gpui::rgb(0xf87171),
                                    None => theme::text(),
                                }
                            })
                            .font_family("Cascadia Code")
                            .text_size(gpui::px(13.56))
                            .gap_2()
                            .child(if node.is_file {
                                Icon::new(IconName::File)
                                    .small()
                                    .text_color(theme::text_muted())
                                    .into_any_element()
                            } else {
                                Icon::new(if is_expanded {
                                    IconName::FolderOpen
                                } else {
                                    IconName::Folder
                                })
                                .small()
                                .text_color(theme::text_muted())
                                .into_any_element()
                            })
                            .child(div().child(name.to_string()));

                        if node.is_file {
                            let file_name = node.full_path.clone();
                            let file_path = node.full_path.clone();
                            let repo = repo_root.clone();
                            let on_dbl_click = cx.listener(
                                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                    if event.button == gpui::MouseButton::Left
                                        && event.click_count == 2
                                    {
                                        if let Some(content) =
                                            crate::sys::read_workspace_file(&repo, &file_name)
                                        {
                                            this.parsed_markdown.borrow_mut().remove(&usize::MAX);
                                            this.file_tree_open =
                                                Some((file_name.clone(), content));
                                            cx.notify();
                                        }
                                    }
                                },
                            );

                            let path_clone = file_path.clone();
                            let view = cx.entity().clone();

                            list = list.child(
                                row.on_mouse_down(gpui::MouseButton::Left, on_dbl_click)
                                    .context_menu(move |mut menu, _, _| {
                                        let path1 = path_clone.clone();
                                        let view1 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Open File").on_click(
                                            move |_, window, cx| {
                                                view1.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::OpenFile(path1.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        let path2 = path_clone.clone();
                                        let view2 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Editor").on_click(
                                                move |_, window, cx| {
                                                    view2.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInEditor(
                                                                path2.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path3 = path_clone.clone();
                                        let view3 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Folder").on_click(
                                                move |_, window, cx| {
                                                    view3.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInFolder(
                                                                path3.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path4 = path_clone.clone();
                                        let view4 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Copy Path").on_click(
                                            move |_, window, cx| {
                                                view4.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::CopyPath(path4.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        menu
                                    }),
                            );
                        } else {
                            let toggle_path = current_path.clone();
                            let on_click = cx.listener(
                                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                    if event.button == gpui::MouseButton::Left {
                                        if this.file_tree_expanded.contains(&toggle_path) {
                                            this.file_tree_expanded.remove(&toggle_path);
                                        } else {
                                            this.file_tree_expanded.insert(toggle_path.clone());
                                        }
                                        let repo_root = crate::vcs::repo_root_of(&this.path);
                                        this.file_tree_paths = crate::sys::list_workspace_files(repo_root, &this.file_tree_expanded);
                                        this.git_statuses = crate::vcs::get_git_statuses(repo_root);
                                        cx.notify();
                                    }
                                },
                            );

                            let folder_path = node.full_path.clone();
                            let view = cx.entity().clone();

                            list = list.child(
                                row.on_mouse_down(gpui::MouseButton::Left, on_click)
                                    .context_menu(move |mut menu, _, _| {
                                        let path1 = folder_path.clone();
                                        let view1 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Editor").on_click(
                                                move |_, window, cx| {
                                                    view1.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInEditor(
                                                                path1.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path2 = folder_path.clone();
                                        let view2 = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Open in Folder").on_click(
                                                move |_, window, cx| {
                                                    view2.update(cx, |this, cx| {
                                                        this.handle_context_menu_action(
                                                            ContextMenuAction::OpenInFolder(
                                                                path2.clone(),
                                                            ),
                                                            cx,
                                                        )
                                                    });
                                                    window.refresh();
                                                },
                                            ),
                                        );

                                        let path3 = folder_path.clone();
                                        let view3 = view.clone();
                                        menu = menu.item(PopupMenuItem::new("Copy Path").on_click(
                                            move |_, window, cx| {
                                                view3.update(cx, |this, cx| {
                                                    this.handle_context_menu_action(
                                                        ContextMenuAction::CopyPath(path3.clone()),
                                                        cx,
                                                    )
                                                });
                                                window.refresh();
                                            },
                                        ));

                                        menu
                                    }),
                            );

                            if is_expanded {
                                for (child_name, child_node) in sorted_children(node) {
                                    let child_path = format!("{}/{}", current_path, child_name);
                                    list = list.child(render_node(
                                        child_name,
                                        child_node,
                                        depth + 1,
                                        cx,
                                        repo_root,
                                        child_path,
                                        expanded_set,
                                        git_statuses,
                                    ));
                                }
                            }
                        }
                        list.into_any_element()
                    }

                    div()
                        .id("file-tree-list")
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(
                            div()
                                .id("file-tree-scroll-content")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.file_tree_scroll)
                                .p_2()
                                .child(render_node(
                                    "",
                                    &root_node,
                                    0,
                                    cx,
                                    &repo_root,
                                    String::new(),
                                    &self.file_tree_expanded,
                                    &self.git_statuses,
                                )),
                        )
                        .child(
                            div().absolute().top_0().bottom_0().right_0().child(
                                Scrollbar::vertical(&self.file_tree_scroll)
                                    .scrollbar_show(ScrollbarShow::Always),
                            ),
                        )
                        .into_any_element()
                }
            }
            RightRailTab::Git => self.git_tab_content(cx).into_any_element(),
            RightRailTab::Changes => {
                let diff_content = match &self.working_diff {
                    Some(diffs) if diffs.is_empty() => div()
                        .p_4()
                        .text_color(theme::text_muted())
                        .child("No changes in working tree.")
                        .into_any_element(),
                    Some(diffs) => self.file_diff_rows(
                        diffs,
                        &self.changes_open_ixs,
                        super::git::DiffPanel::Changes,
                        cx,
                    ),
                    None => div()
                        .p_4()
                        .text_color(theme::text_muted())
                        .child("Failed to load diff.")
                        .into_any_element(),
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("changes-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.changes_scroll)
                            .p_3()
                            .text_sm()
                            .text_color(theme::text())
                            .child(diff_content),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.changes_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                    )
                    .into_any_element()
            }
            RightRailTab::Plan => {
                let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();

                // The active plan is the most-recently-mentioned plan file in the field:
                // scan message bodies newest-first for a `plans/….md` reference. This
                // covers the orchestrator's assignment ("execute docs/…/plan.md") and any
                // later re-reference. The projection has no `task` field, so the field's
                // messages are the source of truth.
                let active_plan_path = self
                    .view
                    .messages
                    .iter()
                    .rev()
                    .find_map(|m| {
                        hadron_gluon::skills::plan_ref(&m.body)
                            .filter(|rel_path| repo.join(rel_path).is_file())
                    });

                // Resolve the referenced plan to its on-disk content in one step; either
                // the reference or the file may be absent (a plan can be named before it
                // is written, or removed after).
                let resolved = active_plan_path.and_then(|rel_path| {
                    crate::sys::read_workspace_file(&repo, &rel_path)
                        .map(|content| (rel_path, content))
                });

                let plan_element = match resolved {
                    Some((rel_path, content)) => {
                        let (total, completed, _) = parse_plan_progress(&content);
                        let frac = if total > 0 {
                            completed as f32 / total as f32
                        } else {
                            0.0
                        };
                        let pct = (frac * 100.0).round() as usize;

                        let mut list = v_flex().gap_2().p_3().w_full();
                        list = list.child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_sm()
                                .child(format!("Active Plan: {rel_path}")),
                        );
                        list = list.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("{completed}/{total} steps complete ({pct}%)")),
                        );
                        list = list.child(progress_meter(frac, gpui::rgb(0x34d399)));

                        let task_groups = parse_plan_tasks(&content);
                        for (task_name, steps) in task_groups {
                            if steps.is_empty() {
                                continue;
                            }
                            let is_collapsed = self.plan_collapsed_tasks.contains(&task_name);
                            let name_clone = task_name.clone();

                            let header_title = if task_name.is_empty() {
                                "Overview".to_string()
                            } else {
                                task_name.clone()
                            };

                            let id_str = if task_name.is_empty() {
                                "task-header-general".to_string()
                            } else {
                                format!("task-header-{}", task_name)
                            };

                            let all_done = steps.iter().all(|(_, done)| *done);
                            let status_marker = if all_done {
                                Icon::new(IconName::CircleCheck)
                                    .small()
                                    .text_color(gpui::rgb(0x34d399))
                                    .into_any_element()
                            } else {
                                div()
                                    .size(px(14.0))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(theme::text_muted())
                                    .into_any_element()
                            };

                            let header = h_flex()
                                .id(gpui::SharedString::from(id_str))
                                .w_full()
                                .items_center()
                                .gap_2()
                                .py_1()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.plan_collapsed_tasks.contains(&name_clone) {
                                        this.plan_collapsed_tasks.remove(&name_clone);
                                    } else {
                                        this.plan_collapsed_tasks.insert(name_clone.clone());
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    Icon::new(if is_collapsed {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .small()
                                    .text_color(theme::text_muted())
                                )
                                .child(status_marker)
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_sm()
                                        .text_color(theme::text())
                                        .child(header_title)
                                );

                            let mut task_container = v_flex().w_full().gap_2().child(header);

                            if !is_collapsed {
                                let mut steps_list = v_flex().w_full().gap_2().pl_4();
                                for (step_desc, done) in steps {
                                    let marker = if done {
                                        Icon::new(IconName::CircleCheck)
                                            .small()
                                            .text_color(gpui::rgb(0x34d399))
                                            .into_any_element()
                                    } else {
                                        // No hollow-circle glyph ships in the icon set, so draw one:
                                        // a small ringed dot reads as an empty checkbox.
                                        div()
                                            .size(px(14.0))
                                            .flex_shrink_0()
                                            .mt(px(2.0))
                                            .rounded_full()
                                            .border_1()
                                            .border_color(theme::text_muted())
                                            .into_any_element()
                                    };

                                    steps_list = steps_list.child(
                                        h_flex().w_full().gap_2().items_start().child(marker).child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_sm()
                                                .text_color(if done {
                                                    theme::text_muted()
                                                } else {
                                                    theme::text()
                                                })
                                                .child(step_desc),
                                        ),
                                    );
                                }
                                task_container = task_container.child(steps_list);
                            }

                            list = list.child(task_container);
                        }
                        list.into_any_element()
                    }
                    None => div()
                        .p_4()
                        .text_color(theme::text_muted())
                        .child("No active implementation plan referenced in the field yet.")
                        .into_any_element(),
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("plan-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.plan_scroll)
                            .text_sm()
                            .text_color(theme::text())
                            .child(plan_element),
                    )
                    .child(
                        div().absolute().top_0().bottom_0().right_0().child(
                            Scrollbar::vertical(&self.plan_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                    )
                    .into_any_element()
            }
            RightRailTab::Tasks => {
                let now = chrono::Utc::now();
                let render_now = self.task_scrub.unwrap_or(now);

                let scrubber = self.task_timeline_scrubber(now, cx);

                let tasks_to_render: Vec<&crate::model::SwarmTask> = if let Some(at) = self.task_scrub {
                    model::tasks::tasks_at(&self.view.tasks, at)
                } else {
                    self.view.tasks.iter().collect()
                };

                // A gate heartbeat has no history in the field (the plan's own rule:
                // never write mid-gate progress to `field.jsonl`), so it only ever
                // shows live — scrubbing to a past instant shows what the field
                // recorded then, not what a gate happens to be doing right now.
                let live_gate_rows: Vec<crate::model::SwarmTask> = if self.task_scrub.is_none() {
                    let gates_dir = hadron_lattice::live::gates_dir(&self.path);
                    model::tasks::live_rows(&gates_dir, now)
                } else {
                    Vec::new()
                };

                let list = if tasks_to_render.is_empty() && live_gate_rows.is_empty() {
                    let hint = if self.task_scrub.is_some() {
                        "No swarm tasks active at this scrubbed time."
                    } else {
                        "No swarm tasks yet."
                    };
                    div().p_4().child(empty_hint(hint)).into_any_element()
                } else {
                    let mut col = v_flex().gap_1().p_2().w_full();
                    for t in &live_gate_rows {
                        let to = self.resolve_identity(&t.to);
                        let from = self.resolve_identity(&t.from);
                        col = col.child(task_row(t, render_now, &to, &from));
                    }
                    for t in tasks_to_render {
                        let to = self.resolve_identity(&t.to);
                        let from = self.resolve_identity(&t.from);
                        col = col.child(task_row(t, render_now, &to, &from));
                    }
                    col.into_any_element()
                };

                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(scrubber)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(
                                div()
                                    .id("tasks-scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.tasks_scroll)
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child(list),
                            )
                            .child(
                                div().absolute().top_0().bottom_0().right_0().child(
                                    Scrollbar::vertical(&self.tasks_scroll)
                                        .scrollbar_show(ScrollbarShow::Always),
                                ),
                            ),
                    )
                    .into_any_element()
            }
        };

        if let Some(start) = tab_start {
            hadron_lattice::term::info(
                hadron_lattice::term::Source::Chamber,
                &format!("frame render tab {}: {:?}", selected.label(), start.elapsed()),
            );
        }

        let card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            // Glass, matching the chat card: faint sheen + hairline top highlight.
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(header)
            .child(content);

        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .p_2()
            // No fill here: the ambient field is the backdrop, so the card reads as a
            // single pane of glass floating on it. A second fill would stack with the
            // card's translucent glass and hide the field; the p_2 gutter shows it.
            .child(card)
    }

    fn task_timeline_scrubber(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let span_bounds = model::tasks::span(&self.view.tasks, now);
        let Some((start_time, end_time)) = span_bounds else {
            return div().into_any_element();
        };

        let span_ms = (end_time - start_time).num_milliseconds().max(1) as f64;
        let current_at = self.task_scrub.unwrap_or(now);
        let is_scrubbing = self.task_scrub.is_some();

        let live_pill = if is_scrubbing {
            div()
                .id("task-scrub-live-pill")
                .px_2()
                .py_0p5()
                .rounded_full()
                .cursor_pointer()
                .bg(theme::accent())
                .text_color(theme::field_base())
                .font_weight(gpui::FontWeight::BOLD)
                .text_xs()
                .child("Live")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.task_scrub = None;
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div()
                .px_2()
                .py_0p5()
                .text_xs()
                .text_color(gpui::rgb(0x34d399))
                .font_weight(gpui::FontWeight::BOLD)
                .child("● Live")
                .into_any_element()
        };

        let time_label = div()
            .text_xs()
            .text_color(theme::text_muted())
            .font_family("Cascadia Code")
            .child(current_at.format("%H:%M:%S").to_string());

        let header = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .px_3()
            .py_1()
            .child(live_pill)
            .child(time_label);

        let task_ticks: Vec<f32> = self
            .view
            .tasks
            .iter()
            .map(|t| {
                let ms = (t.asked_at - start_time).num_milliseconds() as f64;
                ((ms / span_ms).clamp(0.0, 1.0)) as f32
            })
            .collect();

        let current_pct = (((current_at - start_time).num_milliseconds() as f64 / span_ms)
            .clamp(0.0, 1.0)) as f32;

        let track_bounds_cell = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let track_paint = track_bounds_cell.clone();
        let track_click = track_bounds_cell.clone();
        let track_drag = track_bounds_cell.clone();

        let track_canvas = gpui::canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                track_paint.set(bounds);
                let w = bounds.size.width;
                let h = bounds.size.height;

                let bg_quad = gpui::Bounds {
                    origin: bounds.origin,
                    size: bounds.size,
                };
                window.paint_quad(gpui::fill(bg_quad, theme::bg_base()).corner_radii(px(4.0)));

                let tick_color = theme::text_muted();
                for &tick_pct in &task_ticks {
                    let tx = bounds.origin.x + w * tick_pct;
                    let tick_bounds = gpui::Bounds {
                        origin: gpui::point(tx, bounds.origin.y),
                        size: gpui::size(px(1.5), h),
                    };
                    window.paint_quad(gpui::fill(tick_bounds, tick_color));
                }

                let handle_x = bounds.origin.x + w * current_pct;
                let handle_w = px(3.0);
                let handle_bounds = gpui::Bounds {
                    origin: gpui::point(handle_x - handle_w / 2.0, bounds.origin.y),
                    size: gpui::size(handle_w, h),
                };
                window.paint_quad(gpui::fill(handle_bounds, theme::accent()).corner_radii(px(1.5)));
            },
        )
        .size_full();

        let track_interactive = div()
            .id("task-scrub-track")
            .w_full()
            .h(px(12.0))
            .cursor_pointer()
            .child(track_canvas)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.seek_task_scrub(event.position.x, track_click.get(), start_time, end_time, cx);
                }),
            )
            // The press has to keep being tracked once the pointer leaves a 12px-tall
            // strip, which it does immediately in any real drag. `on_mouse_move` on the
            // track stops firing there; GPUI's drag system does not. Same mechanism the
            // fork's own `Slider` uses (`gpui-component/src/slider.rs:691`).
            .on_drag(TaskScrubDrag, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                move |this, e: &gpui::DragMoveEvent<TaskScrubDrag>, _window, cx| {
                    this.seek_task_scrub(
                        e.event.position.x,
                        track_drag.get(),
                        start_time,
                        end_time,
                        cx,
                    );
                },
            ));

        v_flex()
            .w_full()
            .px_3()
            .pb_2()
            .border_b_1()
            .border_color(theme::border())
            .child(header)
            .child(track_interactive)
            .into_any_element()
    }

    /// Move the scrub head to the pixel `x` on a track of `bounds` spanning
    /// `start`..`end`. The click path and the drag path both land here so they cannot
    /// disagree about where a pixel is in time; the mapping itself is
    /// [`model::tasks::instant_at_fraction`], which is where it is tested.
    fn seek_task_scrub(
        &mut self,
        x: gpui::Pixels,
        bounds: gpui::Bounds<gpui::Pixels>,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        cx: &mut Context<Self>,
    ) {
        // Before the first paint the cell still holds a zero-width default: a press then
        // names no instant rather than dividing by nothing.
        if bounds.size.width <= px(0.0) {
            return;
        }
        let fraction = ((x - bounds.origin.x) / bounds.size.width) as f64;
        self.task_scrub = Some(model::tasks::instant_at_fraction(start, end, fraction));
        cx.notify();
    }
}

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
