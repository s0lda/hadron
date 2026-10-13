use super::*;
use gpui::{ElementId, SharedString};
use gpui_component::button::Button;

impl super::Chamber {
    pub(super) fn render_project_selector(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (top_radius, bottom_radius) = frame_corner_radii(window);
        let titlebar = self.selector_titlebar(window, cx);
        let search_text = self.selector_search.read(cx).value().to_lowercase();

        let recent_projects = self.prefs.recent_projects.clone();
        let filtered_projects: Vec<_> = recent_projects
            .into_iter()
            .filter(|p| {
                if search_text.is_empty() {
                    true
                } else {
                    p.display_name.to_lowercase().contains(&search_text)
                        || p.path.to_lowercase().contains(&search_text)
                }
            })
            .collect();

        let wsl_distros = crate::wsl::list_installed_wsl_distros();

        // Quark brand constellation dots
        let quark_dots = h_flex().gap_1p5().items_center().children(
            [0x60a5fau32, 0xc084fc, 0x34d399, 0xfbbf24]
                .into_iter()
                .map(|c| div().size(px(9.0)).rounded_full().bg(rgb(c)).into_any_element()),
        );

        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .rounded_t(top_radius)
            .rounded_b(bottom_radius)
            .bg(theme::bg_base())
            .text_color(theme::text())
            .child(titlebar)
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .child(
                        v_flex()
                            .w(px(740.0))
                            .max_h(px(680.0))
                            .p_6()
                            .gap_5()
                            .rounded(INNER_RADIUS)
                            .bg(theme::glass_card())
                            .border_1()
                            .border_color(theme::glass_highlight())
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_3()
                                            .child(quark_dots)
                                            .child(
                                                div()
                                                    .text_2xl()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(theme::text())
                                                    .child("Hadron Swarm Workspace"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .px_2()
                                                    .py_0p5()
                                                    .rounded_full()
                                                    .bg(theme::glass_surface())
                                                    .text_color(theme::text_muted())
                                                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                                            ),
                                    )
                                    .child(
                                        Button::new("open-folder")
                                            .icon(IconName::Folder)
                                            .label("Open Folder…")
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.open_workspace(cx);
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .child(
                                        div()
                                            .w_full()
                                            .child(self.selector_search.clone()),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .id("recent-projects-list")
                                    .flex_1()
                                    .gap_2()
                                    .overflow_y_scroll()
                                    .children(if filtered_projects.is_empty() {
                                        vec![
                                            div()
                                                .py_8()
                                                .text_center()
                                                .text_color(theme::text_muted())
                                                .child("No recent projects found. Click 'Open Folder…' to begin.")
                                                .into_any_element(),
                                        ]
                                    } else {
                                        filtered_projects
                                            .into_iter()
                                            .map(|proj| {
                                                let path = proj.path.clone();
                                                let path_for_delete = proj.path.clone();
                                                let is_wsl = proj.remote.as_ref().and_then(|r| match r {
                                                    config::ProjectRemote::Wsl { distro, .. } => Some(distro.clone()),
                                                    _ => None,
                                                });

                                                h_flex()
                                                    .id(ElementId::from(SharedString::from(format!("proj-{}", path))))
                                                    .w_full()
                                                    .items_center()
                                                    .justify_between()
                                                    .p_3()
                                                    .rounded(INNER_RADIUS)
                                                    .bg(theme::glass_surface())
                                                    .border_1()
                                                    .border_color(theme::glass_highlight())
                                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener({
                                                        let path = path.clone();
                                                        move |this, _, window, cx| {
                                                            this.open_project_path(&path, window, cx);
                                                        }
                                                    }))
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                h_flex()
                                                                    .items_center()
                                                                    .gap_2()
                                                                    .child(
                                                                        div()
                                                                            .text_sm()
                                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                                            .text_color(theme::text())
                                                                            .child(proj.display_name),
                                                                    )
                                                                    .children(is_wsl.map(|distro| {
                                                                        div()
                                                                            .text_xs()
                                                                            .px_1p5()
                                                                            .py_0p5()
                                                                            .rounded_sm()
                                                                            .bg(rgb(0x8b5cf6))
                                                                            .text_color(rgb(0xffffff))
                                                                            .child(format!("WSL: {distro}"))
                                                                            .into_any_element()
                                                                    })),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(theme::text_muted())
                                                                    .child(path),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .id(ElementId::from(SharedString::from(format!("del-proj-{}", path_for_delete))))
                                                            .p_1()
                                                            .rounded_sm()
                                                            .cursor_pointer()
                                                            .hover(|s| s.bg(rgba(0xff000033)))
                                                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                                            .on_click(cx.listener({
                                                                let path_to_del = path_for_delete.clone();
                                                                move |this, _, _window, cx| {
                                                                    cx.stop_propagation();
                                                                    this.prefs.remove_project(&path_to_del);
                                                                    let _ = config::save(&this.prefs);
                                                                    cx.notify();
                                                                }
                                                            }))
                                                            .child(
                                                                Icon::new(IconName::WindowClose)
                                                                    .small()
                                                                    .text_color(theme::text_muted()),
                                                            ),
                                                    )
                                                    .into_any_element()
                                            })
                                            .collect()
                                    }),
                            )
                            .children((!wsl_distros.is_empty()).then(|| {
                                v_flex()
                                    .pt_2()
                                    .border_t_1()
                                    .border_color(theme::glass_highlight())
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child("Detected WSL Distributions:"),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .children(
                                                wsl_distros
                                                    .into_iter()
                                                    .map(|d| {
                                                        let unc_path = crate::wsl::format_wsl_unc_path(&d, std::path::Path::new("/home"));
                                                        Button::new(ElementId::from(SharedString::from(format!("wsl-{}", d))))
                                                            .small()
                                                            .icon(IconName::Globe)
                                                            .label(d)
                                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                                this.open_project_path(&unc_path.to_string_lossy(), window, cx);
                                                            }))
                                                            .into_any_element()
                                                    }),
                                            ),
                                    )
                            })),
                    ),
            )
    }

    pub(crate) fn open_project_path(
        &mut self,
        path_str: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trimmed = path_str.trim().trim_matches(|c| c == '\'' || c == '"');
        let home = hadron_lattice::user_home_dir();
        let expanded = if trimmed == "~" {
            home.map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| trimmed.to_string())
        } else if let Some(stripped) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
            home.map(|h| h.join(stripped).to_string_lossy().to_string()).unwrap_or_else(|| trimmed.to_string())
        } else {
            trimmed.to_string()
        };
        let final_path = {
            let p = std::path::Path::new(&expanded);
            if p.is_absolute() || crate::wsl::is_wsl_unc_path(p) {
                expanded
            } else if let Ok(cwd) = std::env::current_dir() {
                cwd.join(p).to_string_lossy().to_string()
            } else {
                expanded
            }
        };

        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = std::process::Command::new(exe).arg(&final_path).spawn() {
                    self.toast_manager.push(
                        ToastKind::Error,
                        format!("Failed to open {final_path}: {e}"),
                        Some(3),
                    );
                } else if self.workspace_mode == WorkspaceMode::Selector {
                    cx.quit();
                } else {
                    self.project_switcher_open = false;
                    self.toast_manager.push(
                        ToastKind::Success,
                        format!("Opened {final_path} in new chamber window"),
                        Some(3),
                    );
                    cx.notify();
                }
            }
            Err(e) => {
                self.toast_manager.push(
                    ToastKind::Error,
                    format!("Failed to locate binary: {e}"),
                    Some(3),
                );
            }
        }
    }

    fn selector_titlebar(&self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let controls = h_flex()
            .items_center()
            .gap_1()
            .pr(px(8.0))
            .flex_shrink_0()
            .child(control_button("min", IconName::WindowMinimize, false))
            .child(control_button(
                "max",
                if window.is_maximized() {
                    IconName::WindowRestore
                } else {
                    IconName::WindowMaximize
                },
                false,
            ))
            .child(control_button("close", IconName::WindowClose, true));

        h_flex()
            .id("selector-titlebar")
            .h(px(40.0))
            .w_full()
            .flex_none()
            .items_center()
            .child(drag_region("drag-selector"))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h_full()
                    .items_center()
                    .justify_end()
                    .child(controls),
            )
    }
}
