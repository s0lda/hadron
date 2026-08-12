use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SkillInfo {
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_global: bool,
}

impl super::Chamber {
    /// A compact segmented picker for a free-string session field (Effort / Mode),
    /// replacing the old free-text input. The seat field stays an `Option<String>`;
    /// the "Default" chip clears it. A stored value that is not one of `options` is
    /// preserved as its own selected chip, so editing an agent's uncommon value here
    /// never silently blanks it (the daemon matches the string against what the agent
    /// advertises, so an off-list value is simply not applied — never destructive).
    pub(super) fn session_select(
        &self,
        key: &'static str,
        field: &Entity<InputState>,
        options: &[&'static str],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let current = field.read(cx).value().trim().to_string();
        // (label, value-to-store, selected)
        let mut chips: Vec<(String, String, bool)> =
            vec![("Default".to_string(), String::new(), current.is_empty())];
        for o in options {
            chips.push((o.to_string(), o.to_string(), current.eq_ignore_ascii_case(o)));
        }
        if !current.is_empty() && !options.iter().any(|o| current.eq_ignore_ascii_case(o)) {
            chips.push((current.clone(), current.clone(), true));
        }
        let mut row = h_flex().gap_1p5().flex_wrap();
        for (label, store, selected) in chips {
            let f = field.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("sel-{key}-{label}")))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::glass_card())
                            .border_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::accent())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        f.update(cx, |s, cx| s.set_value(store.clone(), window, cx));
                        this.commit_settings_inputs(cx);
                        cx.notify();
                    })),
            );
        }
        row.into_any_element()
    }

    /// Open the native file picker to choose an avatar image; the choice is parked
    /// in `pending_image_pick` for `render` to apply (see the field's doc — the
    /// picker task has no `Window`, which `set_value` needs).
    pub(super) fn pick_avatar_image(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose avatar image".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match crate::app::widgets::classify_pick(
                rx.await.ok().and_then(|r| r.ok()),
            ) {
                crate::app::widgets::Picked::Path(p) => Some(p),
                crate::app::widgets::Picked::Cancelled => return,
                crate::app::widgets::Picked::NoPicker => {
                    cx.background_spawn(async { fallback_pick_image() }).await
                }
            };
            let _ = this.update(cx, |this, cx| {
                match picked {
                    Some(path) => this.pending_image_pick = Some(path),
                    None => eprintln!(
                        "chamber: could not open a file picker (no xdg-desktop-portal, and \
                         no zenity/powershell fallback) — paste an image path into the field."
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Set the current target's accent/avatar color from a swatch.
    pub(crate) fn set_settings_color(&mut self, hex: u32, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        if let Some(id) = self.settings_identity_mut() {
            id.color = Some(format!("#{hex:06x}"));
            let _ = config::save(&self.prefs);
            cx.notify();
        }
    }

    /// Clear the current target's image (falling back to color + initials).
    pub(super) fn clear_settings_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(identity) = self.settings_identity_mut() {
            identity.image_path = None;
            self.settings_path
                .update(cx, |s, cx| s.set_value("", window, cx));
            let _ = config::save(&self.prefs);
            cx.notify();
        }
    }

    pub(super) fn loaded_skills(&self) -> Vec<SkillInfo> {
        let mut skills = Vec::new();

        // Helper to scan a dir for markdown files
        let mut scan_dir = |dir: &std::path::Path, is_global: bool| {
            if let Ok(rd) = std::fs::read_dir(dir) {
                let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
                entries.sort_by_key(|e| e.path());
                for entry in entries {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let name = stem.to_string();
                            if !skills.iter().any(|s: &SkillInfo| s.name == name) {
                                skills.push(SkillInfo {
                                    name,
                                    path,
                                    is_global,
                                });
                            }
                        }
                    }
                }
            }
        };

        // 1. Repo skills (.hadron/skills and fallback .hadron/preons)
        let hadron_dir = match self.path.parent() {
            Some(p) => p.to_path_buf(),
            None => std::path::PathBuf::from(".hadron"),
        };
        scan_dir(&hadron_dir.join("skills"), false);
        scan_dir(&hadron_dir.join("preons"), false);

        // 2. Global skills (~/.hadron/skills and fallback ~/.hadron/preons)
        if let Some(user_dir) = hadron_lattice::user_hadron_dir() {
            scan_dir(&user_dir.join("skills"), true);
            scan_dir(&user_dir.join("preons"), true);
        }

        skills
    }

    pub(super) fn available_skills(&self) -> Vec<String> {
        let mut list: Vec<String> = hadron_gluon::skills::builtins()
            .into_iter()
            .map(|s| s.id)
            .collect();
        for skill in self.loaded_skills() {
            if !list.iter().any(|r| r.eq_ignore_ascii_case(&skill.name)) {
                list.push(skill.name);
            }
        }
        list
    }

    pub(super) fn add_custom_skill(&self, skill_name: &str, is_global: bool) -> Option<std::path::PathBuf> {
        let clean = skill_name.trim().to_lowercase();
        if clean.is_empty() {
            return None;
        }
        let target_dir = if is_global {
            hadron_lattice::user_hadron_dir()?.join("skills")
        } else {
            match self.path.parent() {
                Some(p) => p.join("skills"),
                None => std::path::PathBuf::from(".hadron/skills"),
            }
        };

        if std::fs::create_dir_all(&target_dir).is_ok() {
            let skill_file = target_dir.join(format!("{clean}.md"));
            let content = format!("---\nname: {clean}\n---\n\n# Skill: {clean}\nCustom skill procedure and instructions.\n");
            if std::fs::write(&skill_file, content).is_ok() {
                Some(skill_file)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(super) fn delete_skill_file(&self, path: &std::path::Path) -> bool {
        if path.exists() {
            std::fs::remove_file(path).is_ok()
        } else {
            false
        }
    }

    pub(super) fn skill_selector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let available = self.available_skills();
        let current_roles_val = self.settings_roles.read(cx).value().trim().to_string();
        let current_roles: Vec<String> = current_roles_val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mut row = h_flex().gap_1p5().flex_wrap();
        for skill in &available {
            let selected = current_roles.iter().any(|r| r.eq_ignore_ascii_case(skill));
            let f = self.settings_roles.clone();
            let p = skill.clone();
            let current_roles_clone = current_roles.clone();

            row = row.child(
                div()
                    .id(SharedString::from(format!("skill-chip-{}", skill)))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::glass_card())
                            .border_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::accent())
                    })
                    .when(!selected, |d| {
                        d.bg(theme::bg_surface())
                            .border_color(theme::border())
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                    })
                    .child(skill.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut new_roles = current_roles_clone.clone();
                        let target_skill = p.to_string();
                        if let Some(pos) = new_roles.iter().position(|x| x.eq_ignore_ascii_case(&target_skill)) {
                            new_roles.remove(pos);
                        } else {
                            new_roles.push(target_skill);
                        }
                        f.update(cx, |s, cx| s.set_value(new_roles.join(", "), window, cx));
                        this.commit_settings_inputs(cx);
                        cx.notify();
                    })),
            );
        }

        row.into_any_element()
    }
}
