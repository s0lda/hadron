use super::*;

impl super::Chamber {
    /// The right rail: the swappable Terminal / File Tree / Changes pane.
    /// (Internally still `Rail::Inspector` for collapse/size.)
    pub(super) fn terminal_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.right_rail_tab;

        let tabs = TabBar::new("right-rail-tabs")
            .segmented()
            // Flat #101010 so the segmented bar dissolves into the field instead of
            // sitting on a now-invisible lighter fill (Jake's request).
            .bg(theme::field_base())
            .selected_index(selected.index())
            .children(RightRailTab::ALL.map(|t| {
                if t.index() == selected.index() {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(t.label().to_string()),
                    )
                } else {
                    Tab::new().child(div().child(t.label()))
                }
            }))
            .on_click(cx.listener(move |this, ix: &usize, _window, cx| {
                this.right_rail_tab = RightRailTab::from_index(*ix);
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
            }));

        let header = h_flex()
            .id("inspector-toggle")
            .w_full()
            .justify_between()
            .items_center()
            .px_3()
            .py_2()
            .text_sm()
            .text_color(theme::text_muted())
            .child(tabs)
            .child(Icon::new(IconName::PanelRightClose).small())
            .active(|s| s.opacity(0.6))
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_rail(Rail::Inspector, window, cx)),
            );

        let content = match selected {
            RightRailTab::Terminal => {
                // The live grid: one styled row per terminal line, each line a
                // few coalesced same-colour runs (not one element per cell — this
                // box CPU-rasterises every frame). The block cursor is an inverted
                // cell baked into the snapshot.
                let grid: gpui::AnyElement = if let Some(term) = &self.terminal {
                    let snap = term.snapshot();
                    let mut lines = v_flex()
                        .font_family("Cascadia Code")
                        .text_size(px(TERM_FONT))
                        .line_height(px(TERM_CELL_H))
                        .size_full();
                    for line in &snap.lines {
                        let mut row = h_flex()
                            .h(px(TERM_CELL_H))
                            .whitespace_nowrap()
                            .min_w_0();
                        let mut line_empty = true;
                        for run in &line.runs {
                            if !run.text.is_empty() {
                                line_empty = false;
                                row = row.child(
                                    div()
                                        .text_color(gpui::rgb(pack_rgb(run.fg)))
                                        .bg(gpui::rgb(pack_rgb(run.bg)))
                                        .child(run.text.clone())
                                );
                            }
                        }
                        if line_empty {
                            row = row.child(div().child(" "));
                        }
                        lines = lines.child(row);
                    }
                    lines.into_any_element()
                } else {
                    div()
                        .flex_1()
                        .p_3()
                        .font_family("Cascadia Code")
                        .text_size(px(TERM_FONT))
                        .text_color(theme::text_muted())
                        .child("starting shell…")
                        .into_any_element()
                };

                // A paint-time probe: report the screen's pixel bounds so the pump
                // loop can size the PTY to fit. It paints nothing.
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

                // The terminal "screen": a focusable dark surface. Clicking focuses
                // it; while focused, keystrokes stream to the PTY (`on_terminal_key`).
                let screen = div()
                    .id("terminal-screen")
                    .track_focus(&self.terminal_focus)
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .relative()
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::border())
                    .bg(theme::term_bg())
                    // Mouse-down focuses the screen and anchors a text selection
                    // (double/triple-click widen to word/line); dragging extends it,
                    // and copy (`on_terminal_key`) reads it back via `selection_text`.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&this.terminal_focus, cx);
                            if let (Some(term), Some((row, col, right))) =
                                (this.terminal.as_ref(), this.terminal_cell_at(ev.position))
                            {
                                term.selection_start(row, col, right, ev.click_count);
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, cx| {
                        if ev.pressed_button == Some(MouseButton::Left) {
                            if let (Some(term), Some((row, col, right))) =
                                (this.terminal.as_ref(), this.terminal_cell_at(ev.position))
                            {
                                term.selection_update(row, col, right);
                                cx.notify();
                            }
                        }
                    }))
                    .on_key_down(cx.listener(Self::on_terminal_key))
                    .child(size_probe)
                    .child(grid);

                v_flex()
                    .flex_1()
                    // Without min-height:0 this flex child grows to the terminal grid's
                    // content height and spills past the container's bottom edge.
                    .min_h_0()
                    .p_3()
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
                                .child(
                                    div().absolute().top_0().bottom_0().right_0().child(
                                        Scrollbar::vertical(&self.file_tree_open_scroll)
                                            .scrollbar_show(ScrollbarShow::Hover),
                                    ),
                                ),
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
                                    Some(crate::vcs::GitStatus::Modified) => gpui::rgb(0xfb923c),
                                    Some(crate::vcs::GitStatus::Added) => gpui::rgb(0x22c55e),
                                    Some(crate::vcs::GitStatus::Deleted) => gpui::rgb(0xef4444),
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
                                    .scrollbar_show(ScrollbarShow::Hover),
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
                                .scrollbar_show(ScrollbarShow::Hover),
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
                                .scrollbar_show(ScrollbarShow::Hover),
                        ),
                    )
                    .into_any_element()
            }
        };

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
}
