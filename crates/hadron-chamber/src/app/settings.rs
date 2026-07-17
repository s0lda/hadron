//! The Settings overlay and provider-connection wizard: opening/closing the overlay,
//! editing the target identity's name/colour/avatar, committing those inputs, and
//! rendering the settings panels, nav rows, provider list, and session picker.

use super::*;

impl Chamber {
    /// Open the Settings overlay, editing the human's identity first.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.settings_target = SettingsTarget::Human;
        self.load_settings_inputs(window, cx);
        cx.notify();
    }

    /// Commit the name/image inputs, then close the overlay and refocus root.
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        self.settings_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A mutable handle to the identity currently being edited (creating an
    /// empty quark entry on first edit).
    pub(super) fn settings_identity_mut(&mut self) -> Option<&mut Identity> {
        match &self.settings_target {
            SettingsTarget::Human => Some(&mut self.prefs.human),
            SettingsTarget::Quark(id) => Some(self.prefs.quarks.entry(id.clone()).or_default()),
            SettingsTarget::Providers => None,
        }
    }

    /// The stored color override for the current target, if any (`#rrggbb`).
    pub(super) fn settings_color(&self) -> Option<String> {
        let key = self.settings_target.key();
        let id = if key == "human" {
            Some(&self.prefs.human)
        } else {
            self.prefs.quarks.get(key)
        };
        id.and_then(|i| i.color.clone())
    }

    /// Load the current target's name + image path into the editor inputs.
    pub(super) fn load_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, path, model, effort, mode) = {
            let key = self.settings_target.key();
            let mut mdl = String::new();
            let mut eff = None;
            let mut mod_cfg = None;
            let id = if key == "human" {
                Some(&self.prefs.human)
            } else {
                // Read model/effort/mode from the RESOLVED seat, not just a legacy one — an
                // adopted quark's definition lives in the catalogue, so a legacy-only
                // lookup would show blank and a later commit could wipe the real value.
                // These show the *effective* value (catalogue default + any repo override).
                let resolved = resolve_team(&self.team, &self.global);
                if let Some(seat) = resolved.get(&QuarkId::new(key)) {
                    mdl = seat.model.clone();
                    eff = seat.effort.clone();
                    mod_cfg = seat.mode_config.clone();
                }
                self.prefs.quarks.get(key)
            };
            (
                id.and_then(|i| i.display_name.clone()).unwrap_or_default(),
                id.and_then(|i| i.image_path.clone()).unwrap_or_default(),
                mdl,
                eff.unwrap_or_default(),
                mod_cfg.unwrap_or_default(),
            )
        };
        self.settings_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.settings_path
            .update(cx, |s, cx| s.set_value(path, window, cx));
        self.settings_model
            .update(cx, |s, cx| s.set_value(model, window, cx));
        self.settings_effort
            .update(cx, |s, cx| s.set_value(effort, window, cx));
        self.settings_mode_config
            .update(cx, |s, cx| s.set_value(mode, window, cx));
    }

    /// Write the editor inputs back into the current target identity and persist.
    pub(super) fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.settings_name.read(cx).value().trim().to_string();
        let path = self.settings_path.read(cx).value().trim().to_string();
        let model_val = self.settings_model.read(cx).value().trim().to_string();
        let effort_val = self.settings_effort.read(cx).value().trim().to_string();
        let mode_val = self.settings_mode_config.read(cx).value().trim().to_string();

        let key = self.settings_target.key();
        if key != "human" && key != "providers" {
            let qid = QuarkId::new(key);
            // The definition knobs (model/effort/mode/display name) are **per-repo**. How
            // they persist depends on how the quark is seated here:
            //  - a self-contained legacy seat pins the values directly on the seat;
            //  - a catalogue-adopted quark records only the *delta from the catalogue
            //    default* as a per-repo override, so the shared default (and every other
            //    repo) is untouched and a field left at the default inherits it.
            // Only write when a value actually changed, so merely opening Settings for an
            // adopted quark never un-adopts it or rewrites the file.
            let resolved = resolve_team(&self.team, &self.global);
            let def = self.global.get(&qid).cloned();
            if let Some(base) = resolved.get(&qid).cloned() {
                let new_effort = (!effort_val.is_empty()).then_some(effort_val);
                let new_mode = (!mode_val.is_empty()).then_some(mode_val);
                let new_name = (!name.is_empty()).then_some(name.clone());
                // A blank Model means "inherit the catalogue default" (or, for a
                // self-contained seat with no catalogue, keep what it already runs).
                let new_model = if model_val.is_empty() {
                    def.as_ref().map(|d| d.model.clone()).unwrap_or_else(|| base.model.clone())
                } else {
                    model_val.clone()
                };
                // The **display name is global** — a quark is the same quark in every repo,
                // so its name lives in the catalogue, never as a per-repo override. That is
                // also what the router matches `@mentions` against, so a name set once here
                // resolves everywhere. (A purely-local legacy seat has no catalogue entry, so
                // its name lives on the seat itself — the only place it can.)
                if base.display_name != new_name {
                    if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
                        g.display_name = new_name.clone();
                        self.save_global_team(cx);
                    } else if let Some(existing) =
                        self.team.quarks.iter_mut().find(|s| s.id == qid)
                    {
                        existing.display_name = new_name.clone();
                        self.save_repo_team(cx);
                    }
                }

                // Model / effort / mode are **per-repo** knobs (unchanged behaviour). The
                // name is deliberately NOT among them — it was handled globally above, and
                // the delta below inherits `def`'s name so no per-repo name override is ever
                // written.
                let knobs_changed = base.model != new_model
                    || base.effort != new_effort
                    || base.mode_config != new_mode;
                if knobs_changed {
                    if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
                        // Self-contained legacy seat — pin the values on it directly.
                        existing.model = new_model;
                        existing.effort = new_effort;
                        existing.mode_config = new_mode;
                        self.save_repo_team(cx);
                    } else if let Some(def) = def {
                        // Adopted via the catalogue — write a delta override (only what
                        // differs from the shared default), preserving any existing
                        // role/participation override. `seat_override_delta` is the tested
                        // inverse of resolve_team's def-layering. `display_name` inherits the
                        // def, so the name never becomes a per-repo override.
                        let desired = hadron_lattice::Seat {
                            model: new_model,
                            effort: new_effort,
                            mode_config: new_mode,
                            // display_name inherits `def` (via the spread) — names are global.
                            ..def.clone()
                        };
                        let prev = self.team.roster.iter().find(|o| o.id == qid).cloned();
                        let ov = hadron_lattice::seat_override_delta(
                            qid.clone(),
                            &def,
                            &desired,
                            prev.as_ref(),
                        );
                        self.team.roster.retain(|o| o.id != qid);
                        self.team.roster.push(ov);
                        self.save_repo_team(cx);
                    }
                    // else: an event-only quark with no seatable definition — nothing to
                    // persist a per-repo knob against.
                }
            }
        }

        if let Some(id) = self.settings_identity_mut() {
            id.display_name = (!name.is_empty()).then_some(name);
            id.image_path = (!path.is_empty()).then_some(path);
            let _ = config::save(&self.prefs);
        }
    }

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
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::accent())
                            .border_color(theme::accent())
                            .text_color(theme::text())
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
            // gpui's native picker first — the right choice on macOS/Windows and on a
            // Linux desktop with an `xdg-desktop-portal`. Under WSL there usually is no
            // portal on the bus, so this resolves to `Err`/`None` almost immediately
            // (which the old code swallowed, so Browse looked dead).
            let native = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .and_then(|v| v.into_iter().next())
                .map(|p| p.to_string_lossy().into_owned());
            // Fall back to a subprocess dialog on a background thread when the native
            // picker gave nothing (portal missing). Blocking, so keep it off the UI.
            let picked = match native {
                Some(p) => Some(p),
                None => cx.background_spawn(async { fallback_pick_image() }).await,
            };
            let _ = this.update(cx, |this, cx| {
                match picked {
                    Some(path) => this.pending_image_pick = Some(path),
                    // Never silently: if even the fallback found no picker, say so (the
                    // text field still takes a pasted path).
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

    /// Switch which identity the overlay edits (committing the current one).
    pub(super) fn select_settings_target(
        &mut self,
        target: SettingsTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_settings_inputs(cx);
        self.settings_target = target.clone();
        self.load_settings_inputs(window, cx);
        // An ACP quark's Model field is a dropdown of what the agent actually offers, so
        // (re-)probe it on open. Anything else clears the probe and keeps the text field.
        match &target {
            SettingsTarget::Quark(id) => self.start_acp_model_probe(&id.clone(), cx),
            _ => self.acp_model_probe = None,
        }
        cx.notify();
    }

    /// Whether the settings target `id` resolves to an ACP seat — the seats whose Model
    /// field is a live dropdown (re-probed from the agent) rather than a free-text box.
    pub(super) fn is_acp_quark(&self, id: &str) -> bool {
        resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .map(|s| s.transport == hadron_lattice::Transport::Acp)
            .unwrap_or(false)
    }

    /// Re-probe the ACP agent backing `id` for the models it offers, parking the result
    /// in `acp_model_probe` for the Settings dropdown. A no-op (clears the probe) for a
    /// non-ACP quark or a seat with no bootable command — those keep the free-text field.
    /// The boot runs off the UI thread; a probe that resolves after the human has moved
    /// to another quark is dropped (the id no longer matches), so it can't cross-populate.
    pub(super) fn start_acp_model_probe(&mut self, id: &str, cx: &mut Context<Self>) {
        let target = resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .and_then(hadron_gluon::adapter::registry::AcpTarget::for_seat);
        let Some(target) = target else {
            self.acp_model_probe = None;
            return;
        };
        let id = id.to_string();
        self.acp_model_probe = Some(AcpModelProbe { id: id.clone(), state: AcpModelState::Probing });
        cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // Blocking ACP boot, off the UI thread — a slow `npx` must not freeze the
                // window (mirrors the Connect wizard's probe).
                let result = cx
                    .background_spawn(async move {
                        hadron_gluon::adapter::acp::probe_selector(&target)
                    })
                    .await;
                this.update(&mut cx, |this, cx| {
                    // Only the still-open probe may write its result.
                    if !matches!(&this.acp_model_probe, Some(p) if p.id == id) {
                        return;
                    }
                    let state = match result {
                        Ok(Some(sel)) => AcpModelState::Ready {
                            models: sel.available,
                            current: sel.current,
                        },
                        Ok(None) => {
                            AcpModelState::Unavailable("this agent offers no model picker".into())
                        }
                        Err(e) => AcpModelState::Unavailable(format!("couldn't detect models: {e}")),
                    };
                    this.acp_model_probe = Some(AcpModelProbe { id, state });
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// The ACP Model **dropdown**: the agent's offered models as clickable chips, with a
    /// "Default" chip (blank → the agent's own current model, which the daemon leaves
    /// alone). Shows a "Detecting…" note while probing and, if the probe failed or the
    /// agent offers no picker, just the always-safe "Default". Clicking a chip writes the
    /// wire value into `settings_model` and commits — the same path the text field used.
    pub(super) fn acp_model_select(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_model = self.settings_model.read(cx).value().trim().to_string();
        // (label, value-to-store, selected). The blank chip is "Inherit" — an empty seat
        // model, which `commit_settings_inputs` resolves to the shared catalogue default.
        // That is a different thing from the agent's *own* current model (marked "·
        // default" among the offered chips below), so it gets its own honest label.
        let mut chips: Vec<(String, String, bool)> =
            vec![("Inherit".to_string(), String::new(), current_model.is_empty())];
        let mut note: Option<String> = None;
        match self.acp_model_probe.as_ref().map(|p| &p.state) {
            Some(AcpModelState::Probing) => note = Some("Detecting models…".into()),
            Some(AcpModelState::Ready { models, current }) => {
                for m in models {
                    // The agent's current pick is the "Default" the blank chip resolves to,
                    // so annotate it rather than let it look like a separate option.
                    let label = if &m.value == current {
                        format!("{} · default", m.label)
                    } else {
                        m.label.clone()
                    };
                    let selected = current_model.eq_ignore_ascii_case(&m.value);
                    chips.push((label, m.value.clone(), selected));
                }
            }
            Some(AcpModelState::Unavailable(msg)) => note = Some(msg.clone()),
            None => {}
        }
        // A model the seat pinned that the agent didn't offer still shows, selected — so
        // an edit made before this feature (or against a changed lineup) is never hidden.
        if !current_model.is_empty()
            && !chips.iter().any(|(_, v, _)| v.eq_ignore_ascii_case(&current_model))
        {
            chips.push((current_model.clone(), current_model.clone(), true));
        }

        let mut row = h_flex().gap_1p5().flex_wrap();
        for (ix, (label, store, selected)) in chips.into_iter().enumerate() {
            let f = self.settings_model.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("acp-model-{ix}")))
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .text_sm()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.bg(theme::accent())
                            .border_color(theme::accent())
                            .text_color(theme::text())
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

        let mut col = v_flex().gap_1p5().child(row);
        if let Some(n) = note {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(n),
            );
        }
        col.into_any_element()
    }

    /// Set the current target's accent/avatar color from a swatch.
    pub(super) fn set_settings_color(&mut self, hex: u32, cx: &mut Context<Self>) {
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

    /// Reset the current target to its code defaults.
    pub(super) fn reset_settings_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.settings_target.clone() {
            SettingsTarget::Human => self.prefs.human = Identity::default(),
            SettingsTarget::Quark(id) => {
                self.prefs.quarks.remove(&id);
            }
            SettingsTarget::Providers => {}
        }
        self.load_settings_inputs(window, cx);
        let _ = config::save(&self.prefs);
        cx.notify();
    }

    pub(super) fn settings_overlay(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.settings_target.clone();

        // Left nav: every editable identity — the human, then each quark.
        let mut nav = v_flex()
            .gap_0p5()
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("GLOBAL"),
            )
            .child(self.settings_nav_row(SettingsTarget::Providers, &target, cx))
            .child(
                div()
                    .px_1()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("IDENTITIES"),
            )
            .child(self.settings_nav_row(SettingsTarget::Human, &target, cx));
        for r in &self.view.roster {
            nav =
                nav.child(self.settings_nav_row(SettingsTarget::Quark(r.id.clone()), &target, cx));
        }

        // Live preview: the target resolved, but with the in-progress name/image
        // from the inputs so it tracks typing.
        let live_name = self.settings_name.read(cx).value().to_string();
        let live_path = self.settings_path.read(cx).value().trim().to_string();
        let mut preview = self.resolve_identity(target.key());
        if !live_name.trim().is_empty() {
            preview.name = live_name;
        }
        preview.image = (!live_path.is_empty()).then_some(live_path);
        let preview_row = h_flex()
            .items_center()
            .gap_3()
            .child(identity_avatar(&preview, 44.0))
            .child(div().text_color(preview.color).child(preview.name.clone()));

        // Color swatches; the stored color (if any) gets a bright ring.
        let selected = self.settings_color();
        let mut swatches = h_flex().gap_2().flex_wrap();
        for hex in IDENTITY_SWATCHES {
            let is_sel = selected.as_deref() == Some(format!("#{hex:06x}").as_str());
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("swatch-{hex:06x}")))
                    .size(px(22.0))
                    .rounded_full()
                    .bg(rgb(hex))
                    .border_2()
                    .border_color(if is_sel {
                        theme::text()
                    } else {
                        theme::border()
                    })
                    .hover(|s| s.border_color(theme::text_secondary()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_settings_color(hex, cx))),
            );
        }
        // A full picker for any colour beyond the presets; its Change event writes the
        // identity's colour (subscribed in `new`). Not value-synced to the target on
        // switch — the swatch ring already shows the current selection, and syncing would
        // re-emit Change and write the colour back on every target change.
        swatches = swatches.child(ColorPicker::new(&self.color_picker).label("Custom"));

        // Left sidebar: a recessed, scrollable nav column of identities.
        let sidebar = v_flex()
            .flex_none()
            .w(px(190.0))
            .h_full()
            .p_2()
            .gap_2()
            .bg(theme::bg_base())
            .border_r(px(1.0))
            .border_color(theme::border())
            .child(div().px_1().text_color(theme::text()).child("Settings"))
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("SETTINGS"),
            )
            .child(
                div()
                    .id("settings-nav-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(nav),
            );

        // Right panel: header (who + close), the scrollable editor fields, and a
        // pinned footer (Reset / Done).
        let header = h_flex()
            .flex_none()
            .items_center()
            .justify_between()
            .child(div().text_color(theme::text_secondary()).child(
                if target == SettingsTarget::Providers {
                    "Providers".to_string()
                } else {
                    format!("Editing {}", preview.name)
                },
            ))
            .child(
                div()
                    .id("settings-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .rounded_full()
                    .text_color(theme::text_secondary())
                    .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                    .child(Icon::new(IconName::WindowClose).small())
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx))),
            );

        let fields = if target == SettingsTarget::Providers {
            self.providers_view(cx).into_any_element()
        } else {
            let is_quark = matches!(target, SettingsTarget::Quark(_));
            // ACP quarks get a live model dropdown (re-probed from the agent) in place of
            // the free-text Model box; everything else keeps the text field.
            let acp_quark = matches!(&target, SettingsTarget::Quark(id) if self.is_acp_quark(id));
            v_flex()
                .gap_4()
                .child(settings_field("Preview", preview_row.into_any_element()))
                .child(settings_field(
                    "Display name",
                    Input::new(&self.settings_name).into_any_element(),
                ))
                // Model + Effort configure an agent's session, so they are quark-only. The
                // human has no such controls. Model here is a **per-repo** override: blank
                // inherits the catalogue default (e.g. acp-claude = Opus), a value pins this
                // repo (= Sonnet) without touching the shared catalogue or any other repo.
                //
                // Permission is the single authority control — it REPLACES the old ACP
                // "Mode" field (default/plan/acceptEdits/bypassPermissions), which set the
                // same posture axis by a cruder route. The ladder subsumes it: it is live,
                // turn-granular, and applied over any boot-time `mode_config` per turn. The
                // `mode_config` seat field is intentionally left in place (still round-trips
                // through load/commit) but is no longer human-editable here.
                .when(is_quark, |v| {
                    let model_field = if acp_quark {
                        self.acp_model_select(cx)
                    } else {
                        Input::new(&self.settings_model).into_any_element()
                    };
                    v.child(settings_field("Model", model_field))
                    .child(settings_field(
                        "Effort",
                        self.session_select(
                            "effort",
                            &self.settings_effort,
                            &["low", "medium", "high"],
                            cx,
                        ),
                    ))
                    // The permission ladder: how much authority the human delegates to
                    // this quark (Ask → Bypass). Stored on the field as a per-quark
                    // `ModeSet`, so it is live-honoured and independent of team.json. A
                    // per-quark choice persists even when the global default later changes.
                    .child(settings_field("Permission", self.mode_select(target.key(), cx)))
                })
                .child(settings_field("Color", swatches.into_any_element()))
                .child(settings_field(
                    "Image",
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().child(Input::new(&self.settings_path)))
                        .child(text_button("settings-browse-img", "Browse…").on_click(
                            cx.listener(|this, _, _, cx| this.pick_avatar_image(cx)),
                        ))
                        .child(text_button("settings-clear-img", "Clear").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_settings_image(window, cx)
                            }),
                        ))
                        .into_any_element(),
                ))
                .into_any_element()
        };

        let footer = if target == SettingsTarget::Providers {
            div().into_any_element()
        } else {
            h_flex()
                .flex_none()
                .justify_between()
                .pt_1()
                .child(text_button("settings-reset", "Reset to default").on_click(
                    cx.listener(|this, _, window, cx| this.reset_settings_target(window, cx)),
                ))
                .child(
                    div()
                        .id("settings-done")
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme::accent())
                        .text_color(theme::text())
                        .hover(|s| s.opacity(0.9))
                        .active(|s| s.opacity(0.8))
                        .child("Done")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                        ),
                )
                .into_any_element()
        };
        let panel = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .p_4()
            .gap_4()
            .child(header)
            .child(
                div()
                    .id("settings-fields-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(fields),
            )
            .child(footer);

        let card = h_flex()
            .occlude()
            .w_full()
            .h_full()
            .max_w(px(960.0))
            .max_h(px(640.0))
            .rounded_lg()
            .overflow_hidden()
            // Opaque: a focused settings modal shouldn't let the bright field bleed through
            // (it read as too transparent). Solid, not glass — shared with the info panel.
            .bg(theme::modal_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(sidebar)
            .child(panel);

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .p_8()
            .flex()
            // Center on both axes deterministically (was relying on default
            // align + a top margin, which sank the card to the window's foot).
            .flex_col()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
            .child(card)
    }

    /// One row in the Settings identity nav: avatar + name, highlighted when it's
    /// the identity currently being edited.
    pub(super) fn settings_nav_row(
        &self,
        who: SettingsTarget,
        current: &SettingsTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let resolved = self.resolve_identity(who.key());
        let selected = &who == current;
        let id = SharedString::from(format!("settings-id-{}", who.key()));
        h_flex()
            .id(id)
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1p5()
            .rounded_md()
            .bg(if selected {
                theme::bg_surface_raised()
            } else {
                theme::bg_base()
            })
            .hover(|s| s.bg(theme::bg_surface()))
            .child(if who == SettingsTarget::Providers {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .text_color(theme::text_muted())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element()
            } else {
                identity_avatar(&resolved, 24.0).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(if selected {
                        theme::text()
                    } else {
                        theme::text_secondary()
                    })
                    .child(if who == SettingsTarget::Providers {
                        "Providers".to_string()
                    } else {
                        resolved.name.clone()
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_settings_target(who.clone(), window, cx)
            }))
    }

    pub(super) fn providers_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.wizard_state {
            WizardState::None => {
                let mut list = v_flex().gap_3();
                for provider in &self.providers {
                    let (state_text, state_color) = match &provider.state {
                        ProviderState::NotConnected => {
                            ("Not Connected".to_string(), theme::text_muted())
                        }
                        ProviderState::Connecting => {
                            ("Connecting…".to_string(), theme::text_muted())
                        }
                        ProviderState::NeedsAuth(_) => ("Needs Auth".to_string(), theme::accent()),
                        ProviderState::Ready { model } => {
                            (format!("Ready ({})", model), gpui::rgb(0x22c55e))
                        }
                        ProviderState::Failed(e) => (format!("Failed: {}", e), theme::danger()),
                    };
                    list = list.child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(theme::text())
                                            .child(provider.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("Transport: {}", provider.transport)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size(px(8.0))
                                                    .rounded_full()
                                                    .bg(state_color),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(theme::text_secondary())
                                                    .child(state_text),
                                            ),
                                    )
                                    .child({
                                        let pid = provider.id.clone();
                                        text_button(
                                            format!("remove-{}", provider.id),
                                            "Remove",
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _window, cx| {
                                                this.remove_quark(&pid, cx);
                                            },
                                        ))
                                    }),
                            ),
                    );
                }

                v_flex()
                    .size_full()
                    .gap_6()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(theme::text())
                                    .child("Configured Providers"),
                            )
                            .child(text_button("add-quark", "Add Quark").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.wizard_state = WizardState::PickPreset;
                                    cx.notify();
                                },
                            ))),
                    )
                    // Scroll the roster so a long provider list stays reachable while the
                    // "Configured Providers" header + Add Quark button stay pinned. Same
                    // reason as the preset list: a `size_full` wizard can't be scrolled by
                    // the ancestor, so extra rows would clip off the card.
                    .child(
                        div()
                            .id("providers-list-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(list),
                    )
            }
            WizardState::PickPreset => {
                let presets = hadron_gluon::adapter::registry::QuarkKind::available_presets()
                    .into_iter()
                    .map(|(id, name, cmd, args)| AgentDescriptor {
                        id: id.into(),
                        name: name.into(),
                        command: cmd.into(),
                        args: args.into_iter().map(String::from).collect(),
                    })
                    .collect::<Vec<_>>();

                // Case-insensitive substring match on name + command. Empty filter shows
                // all; the "Custom command…" escape hatch below is always appended,
                // unfiltered, so the list is never a dead end.
                let filter = self.preset_filter.read(cx).value().trim().to_lowercase();
                let presets: Vec<_> = presets
                    .into_iter()
                    .filter(|p| {
                        filter.is_empty()
                            || p.name.to_lowercase().contains(&filter)
                            || p.command.to_lowercase().contains(&filter)
                    })
                    .collect();

                let mut list = v_flex().gap_2();
                for preset in presets {
                    let preset_clone = preset.clone();
                    list = list.child(
                        h_flex()
                            .id(SharedString::from(format!("preset-{}", preset.id)))
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(theme::bg_surface())
                            .border_1()
                            .border_color(theme::border())
                            .hover(|s| s.bg(theme::bg_surface_raised()))
                            .cursor_pointer()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(theme::text())
                                            .child(preset.name.clone()),
                                    )
                                    .child(div().text_xs().text_color(theme::text_muted()).child(
                                        format!("{} {}", preset.command, preset.args.join(" ")),
                                    )),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_muted())
                                    .child("Configure →"),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.wizard_state = WizardState::Connecting(
                                    preset_clone.clone(),
                                    ProviderState::NotConnected,
                                );
                                cx.notify();
                            })),
                    );
                }

                // Add custom option
                list = list.child(
                    h_flex()
                        .id("preset-custom")
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::border())
                        .hover(|s| s.bg(theme::bg_surface_raised()))
                        .cursor_pointer()
                        .child(
                            div()
                                .text_base()
                                .text_color(theme::text())
                                .child("Custom command…"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Configure →"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.wizard_state = WizardState::Connecting(
                                AgentDescriptor {
                                    id: "custom".into(),
                                    name: "Custom".into(),
                                    command: "".into(),
                                    args: vec![],
                                },
                                ProviderState::NotConnected,
                            );
                            cx.notify();
                        })),
                );

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-wizard", "← Back").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.wizard_state = WizardState::None;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Select a Preset"),
                    )
                    // Search box: filters the catalogue as you type (name + command), so
                    // the right provider is one query away instead of a scroll through ~37.
                    .child(Input::new(&self.preset_filter))
                    // The catalogue is ~37 presets — taller than the card. Give the list
                    // its own bounded scroll region (like `settings-nav-scroll`) so every
                    // provider is reachable while Back/title stay pinned. Without this the
                    // `size_full` wizard reports full height to the ancestor scroll, which
                    // then can't scroll, and everything past the first few presets clips.
                    .child(
                        div()
                            .id("preset-list-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(list),
                    )
            }

            WizardState::Connecting(desc, state) => {
                let desc_clone = desc.clone();
                let state_ui = match state {
                    ProviderState::Connecting => v_flex()
                        .gap_4()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Connecting..."),
                        )
                        .into_any_element(),
                    ProviderState::NotConnected => {
                        v_flex()
                            .gap_4()
                            .child(text_button("connect-btn", "Connect").on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.wizard_state = WizardState::Connecting(
                                        desc_clone.clone(),
                                        ProviderState::Connecting,
                                    );
                                    cx.notify();

                                    // Connect = boot the agent and complete ACP's `initialize`.
                                    // The probe lives in the daemon (`hadron-gluon`), which is the
                                    // thing that will actually drive this agent — so the UI cannot
                                    // claim a provider works over a client the daemon never uses.
                                    let target = hadron_gluon::adapter::registry::AcpTarget {
                                        program: desc_clone.command.clone(),
                                        args: desc_clone.args.clone(),
                                    };
                                    let desc_for_task = desc_clone.clone();
                                    cx.spawn(
                                        |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                            // The async block outlives the borrow, so it gets its own
                                            // handle on the app rather than holding a reference.
                                            let mut cx = cx.clone();
                                            async move {
                                                // Blocking boot, off the UI thread: a slow `npx` must not
                                                // freeze the window.
                                                let result = cx
                                                    .background_spawn(async move {
                                                        hadron_gluon::adapter::acp::probe(&target)
                                                    })
                                                    .await
                                                    .map_err(|e| e.to_string());

                                                this.update(&mut cx, |this, cx| {
                                                    let state = match result {
                                                        Ok(model) => ProviderState::Ready { model },
                                                        Err(e) => ProviderState::Failed(e),
                                                    };
                                                    this.wizard_state = WizardState::Connecting(
                                                        desc_for_task,
                                                        state,
                                                    );
                                                    cx.notify();
                                                })
                                                .ok();
                                            }
                                        },
                                    )
                                    .detach();
                                },
                            )))
                            .into_any_element()
                    }
                    ProviderState::NeedsAuth(methods) => {
                        let mut auth_list = v_flex().gap_2();
                        for method in methods {
                            let method_clone = method.clone();
                            let desc_inner = desc.clone();
                            auth_list = auth_list.child(
                                v_flex()
                                    .gap_2()
                                    .p_3()
                                    .border_1()
                                    .border_color(theme::border())
                                    .rounded_md()
                                    .child(
                                        div().text_color(theme::text()).child(method.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_muted())
                                            .child(method.description.clone()),
                                    )
                                    .child(
                                        text_button(
                                            &format!("auth-btn-{}", method.id),
                                            &method.name,
                                        )
                                        .on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.wizard_state = WizardState::Connecting(
                                                    desc_inner.clone(),
                                                    ProviderState::Connecting,
                                                );
                                                cx.notify();

                                                let target = hadron_gluon::adapter::registry::AcpTarget {
                                                    program: desc_inner.command.clone(),
                                                    args: desc_inner.args.clone(),
                                                };
                                                let desc_for_task = desc_inner.clone();
                                                cx.spawn(
                                                    |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                                                        let mut cx = cx.clone();
                                                        async move {
                                                            let result = cx
                                                                .background_spawn(async move {
                                                                    hadron_gluon::adapter::acp::probe(&target)
                                                                })
                                                                .await
                                                                .map_err(|e| e.to_string());

                                                            this.update(&mut cx, |this, cx| {
                                                                let state = match result {
                                                                    Ok(model) => ProviderState::Ready { model },
                                                                    Err(e) => ProviderState::Failed(e),
                                                                };
                                                                this.wizard_state = WizardState::Connecting(
                                                                    desc_for_task,
                                                                    state,
                                                                );
                                                                cx.notify();
                                                            })
                                                            .ok();
                                                        }
                                                    },
                                                )
                                                .detach();
                                            },
                                        )),
                                    ),
                            );
                        }
                        auth_list.into_any_element()
                    }
                    ProviderState::Ready { model } => {
                        let desc_inner = desc.clone();
                        let state_inner = state.clone();
                        let model_inner = model.clone();
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .text_color(theme::accent())
                                    .child(format!("Ready! Model available: {}", model)),
                            )
                            .child(text_button("save-provider", "Save Provider").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    // `desc_inner.id` is the PURE vendor now (Task 3 re-keyed
                                    // `available_presets()`/`AgentDescriptor` off `AcpAgentSpec.vendor`,
                                    // e.g. "claude" — it no longer carries the old smeared "acp-claude"
                                    // preset key). The seat's id is the `<transport>-<vendor>` form,
                                    // derived once via `conventional_id` and reused for BOTH records
                                    // below so they never diverge — `remove_quark` keys the roster off
                                    // `ConfiguredQuark.id`, so it must match `Seat.id` exactly.
                                    let seat_id = hadron_lattice::Transport::Acp.conventional_id(&desc_inner.id);

                                    this.providers.push(ConfiguredQuark {
                                        id: seat_id.clone(),
                                        transport: "acp".to_string(),
                                        state: state_inner.clone(),
                                    });

                                    // An ACP seat, and it carries the command the wizard
                                    // just proved boots — so the daemon reaches this agent
                                    // over the same transport the human tested it on. Its
                                    // definition lands in the global catalogue; this repo
                                    // auto-adopts it (see `add_configured_quark`).
                                    let mut seat = hadron_lattice::Seat {
                                        id: hadron_lattice::QuarkId::new(&seat_id),
                                        display_name: None,
                                        vendor: desc_inner.id.clone(),
                                        model: model_inner.clone(),
                                        flavor: hadron_lattice::Flavor::Worker, // default flavor
                                        transport: hadron_lattice::Transport::Acp,
                                        command: Some(hadron_lattice::AcpCommand {
                                            program: desc_inner.command.clone(),
                                            args: desc_inner.args.clone(),
                                        }),
                                        // A seat the human just proved and saved is on.
                                        enabled: true,
                                        effort: None,
                                        mode_config: None,
                                    };
                                    // `vendor` is already pure (Task 3's re-keyed preset list), so this
                                    // is a no-op today — left in as a defensive strip in case a vendor
                                    // string ever carries a transport prefix again.
                                    seat.normalize_vendor();
                                    // Advisory only, never blocking: `seat_id` is already built from
                                    // `conventional_id`, so this is dormant on this common path — it
                                    // stays as future-proofing for a later custom-CLI id path where a
                                    // hand-typed id might not match its transport prefix.
                                    if !hadron_lattice::id_follows_convention(seat.id.as_str(), seat.transport) {
                                        eprintln!(
                                            "chamber: note — id '{}' does not match the '{}-' convention",
                                            seat.id.as_str(),
                                            seat.transport.code()
                                        );
                                    }
                                    this.add_configured_quark(seat, cx);

                                    this.wizard_state = WizardState::None;
                                    cx.notify();
                                }),
                            ))
                            .into_any_element()
                    }
                    ProviderState::Failed(err) => div()
                        .text_color(theme::text_secondary())
                        .child(err.clone())
                        .into_any_element(),
                };

                v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-presets", "← Back").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child(format!("Connecting to {}", desc.name)),
                    )
                    // Scroll the connection form (config fields / auth / errors can run tall)
                    // while Back + title stay pinned — the ancestor can't scroll a `size_full`
                    // wizard, so a tall form would otherwise clip off the card's bottom.
                    .child(
                        div()
                            .id("connecting-form-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(state_ui),
                    )
            }
        }
    }
}
