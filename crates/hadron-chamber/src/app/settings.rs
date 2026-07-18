//! The Settings overlay and provider-connection wizard: opening/closing the overlay,
//! editing the target identity's name/colour/avatar, committing those inputs, and
//! rendering the settings panels, nav rows, provider list, and session picker.

use super::*;
use hadron_lattice::secrets::SecretStore;

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
        let (name, path, model, effort, mode, secret_var, secret_is_set) = {
            let key = self.settings_target.key();
            let mut mdl = String::new();
            let mut eff = None;
            let mut mod_cfg = None;
            // Defaults for a non-quark target (Human/Providers), which never shows the
            // API-key field — overwritten below when `key` resolves to a seat.
            let mut var = DEFAULT_SECRET_VAR.to_string();
            let mut is_set = false;
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
                    // Show the first declared var name, or the default if none is declared
                    // yet — never the value, only ever the NAME (see `secret_status`).
                    var = seat
                        .secret_env
                        .first()
                        .cloned()
                        .unwrap_or_else(|| DEFAULT_SECRET_VAR.to_string());
                    is_set = secret_status(self.secret_store.as_ref(), &seat.id, &var);
                }
                self.prefs.quarks.get(key)
            };
            (
                id.and_then(|i| i.display_name.clone()).unwrap_or_default(),
                id.and_then(|i| i.image_path.clone()).unwrap_or_default(),
                mdl,
                eff.unwrap_or_default(),
                mod_cfg.unwrap_or_default(),
                var,
                is_set,
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
        self.settings_secret_var
            .update(cx, |s, cx| s.set_value(secret_var, window, cx));
        // Never populated from the store — write-only, always blank on (re)load.
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_is_set;
        // Team-wide, not per-identity — loaded unconditionally (not keyed off `key`) so
        // it stays in sync with `self.team.max_exchanges` regardless of which target the
        // overlay happens to be showing when this runs.
        let max_exchanges = self.team.max_exchanges.map(|n| n.to_string()).unwrap_or_default();
        self.settings_max_exchanges
            .update(cx, |s, cx| s.set_value(max_exchanges, window, cx));
    }

    /// Write the editor inputs back into the current target identity and persist.
    pub(super) fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.settings_name.read(cx).value().trim().to_string();
        let path = self.settings_path.read(cx).value().trim().to_string();
        let model_val = self.settings_model.read(cx).value().trim().to_string();
        let effort_val = self.settings_effort.read(cx).value().trim().to_string();
        let mode_val = self.settings_mode_config.read(cx).value().trim().to_string();

        // Team-wide "Max exchanges" (Providers panel) — gated on the Providers target,
        // like the per-quark model/effort/mode fields below are gated on `key`. This
        // must NOT be unconditional: `reload_if_changed` (mod.rs) assigns a freshly
        // polled `self.team` on an external team.json edit WITHOUT touching
        // `settings_max_exchanges`, so if that happened while Settings was open on any
        // OTHER panel, an unconditional read-diff-write here would compare the now-stale
        // input against the just-updated `self.team.max_exchanges` and silently write the
        // stale value back — reverting someone else's (or the daemon's own) edit the next
        // time any control on an unrelated panel commits (closing Settings, an
        // effort/mode/color click, …). Scoping the write to "the human is actually on the
        // Providers panel" makes that impossible: `select_settings_target` commits BEFORE
        // switching away, so an edit made while on Providers is still captured on
        // navigation, and a commit from any other panel never touches this field.
        // `load_settings_inputs` still re-syncs the input from `self.team` unconditionally
        // (every target, including the external-reload path via `reload_if_changed` ->
        // ... -> next `load_settings_inputs` call) — only the *write* is gated.
        if self.settings_target == SettingsTarget::Providers {
            let max_exchanges_val = self.settings_max_exchanges.read(cx).value().trim().to_string();
            let new_max_exchanges = parse_max_exchanges(&max_exchanges_val);
            if new_max_exchanges != self.team.max_exchanges {
                self.team.max_exchanges = new_max_exchanges;
                self.save_repo_team(cx);
            }
        }

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

    /// Write the masked value to the keychain and declare `var` in the current
    /// quark's `secret_env` if it is not already there. A no-op if the var name or
    /// value is blank. The var NAME is a property of the quark, not a per-repo
    /// knob — like `display_name`, it is written to the global catalogue seat when
    /// one exists, falling back to a local legacy seat (mirrors the `display_name`
    /// precedence above; unlike Model/Effort/Mode it cannot go through
    /// `seat_override_delta`, since `SeatOverride` has no `secret_env` field — the
    /// var declares the same quark everywhere, not a per-repo override). Clears the
    /// masked input afterward: the value is written, never re-shown.
    ///
    /// **Security**: `value` is a secret. It is passed straight to `SecretStore::set`
    /// (the OS credential store) and never logged, cloned into `team.json`, or held
    /// anywhere else on `self`.
    pub(super) fn set_settings_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let SettingsTarget::Quark(key) = self.settings_target.clone() else {
            return;
        };
        let qid = QuarkId::new(key);
        let var = self.settings_secret_var.read(cx).value().trim().to_string();
        let value = self.settings_secret_value.read(cx).value().to_string();
        if var.is_empty() || value.is_empty() {
            return;
        }
        if let Err(e) = self.secret_store.set(&qid, &var, &value) {
            eprintln!("chamber: failed to write secret to the OS credential store: {e}");
            return;
        }
        if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
            if declare_secret_var(g, &var) {
                self.save_global_team(cx);
            }
        } else if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
            if declare_secret_var(existing, &var) {
                self.save_repo_team(cx);
            }
        }
        // else: an event-only quark with no seatable definition — the value is still
        // written to the keychain (harmless), but there is no `secret_env` to declare
        // it in, matching the model/effort commit's same carve-out above.
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_status(self.secret_store.as_ref(), &qid, &var);
        cx.notify();
    }

    /// Delete the keychain value for the current quark's declared var and drop it
    /// from `secret_env` (same global-catalogue-first / legacy-seat-fallback
    /// precedence as `set_settings_secret`). Never displays the value.
    pub(super) fn clear_settings_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let SettingsTarget::Quark(key) = self.settings_target.clone() else {
            return;
        };
        let qid = QuarkId::new(key);
        let var = self.settings_secret_var.read(cx).value().trim().to_string();
        if var.is_empty() {
            return;
        }
        if let Err(e) = self.secret_store.delete(&qid, &var) {
            eprintln!("chamber: failed to clear secret from the OS credential store: {e}");
            return;
        }
        if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
            if undeclare_secret_var(g, &var) {
                self.save_global_team(cx);
            }
        } else if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
            if undeclare_secret_var(existing, &var) {
                self.save_repo_team(cx);
            }
        }
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_status(self.secret_store.as_ref(), &qid, &var);
        cx.notify();
    }

    /// The API-key panel: env-var name + masked value inputs, Set/Clear actions, and
    /// a read-only "key set" / "not set" status. The value input is write-only — it
    /// is never populated from the store (see `load_settings_inputs`,
    /// `set_settings_secret`, `clear_settings_secret`), so nothing here can render a
    /// stored secret back to the screen.
    pub(super) fn secret_field(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(200.0))
                            .child(Input::new(&self.settings_secret_var)),
                    )
                    .child(div().flex_1().child(Input::new(&self.settings_secret_value))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(text_button("settings-secret-set", "Set").on_click(cx.listener(
                        |this, _, window, cx| this.set_settings_secret(window, cx),
                    )))
                    .child(text_button("settings-secret-clear", "Clear").on_click(cx.listener(
                        |this, _, window, cx| this.clear_settings_secret(window, cx),
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(if self.settings_secret_status {
                                "key set"
                            } else {
                                "not set"
                            }),
                    ),
            )
            .into_any_element()
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
                    // The secret env-var value (e.g. `GEMINI_API_KEY`) goes to the OS
                    // keychain via `SecretStore`, never into team.json or this panel's
                    // rendered state — see `secret_field`.
                    .child(settings_field("API key", self.secret_field(cx)))
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
                    // Team-wide (not per-quark) policy, so it lives here rather than on any
                    // one identity's panel — the cap on quark↔quark exchanges before the
                    // daemon's backstop stops the swarm. Committed the same way the Model/
                    // Effort text fields are: on nav-away or Done, via `commit_settings_inputs`.
                    .child(settings_field(
                        "Max exchanges",
                        v_flex()
                            .gap_1()
                            .child(Input::new(&self.settings_max_exchanges))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(
                                        "Caps quark\u{2194}quark exchanges before the swarm \
                                         stops. Blank or 0 = daemon default.",
                                    ),
                            )
                            .into_any_element(),
                    ))
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
                    .map(|(id, name, description, cmd, args)| AgentDescriptor {
                        id: id.into(),
                        name: name.into(),
                        description: description.into(),
                        command: cmd.into(),
                        args: args.into_iter().map(String::from).collect(),
                    })
                    .collect::<Vec<_>>();

                // Case-insensitive substring match on name + command. Empty filter shows
                // all. (A custom provider is added via the "Custom CLI…" option, which
                // has its own working wizard — the old empty-command "Custom command…"
                // escape hatch was a dead end and has been removed.)
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
                                        // A human blurb for first-class agents; the raw
                                        // command line for best-effort presets (which have
                                        // no description) so they're still identifiable.
                                        if preset.description.is_empty() {
                                            format!("{} {}", preset.command, preset.args.join(" "))
                                        } else {
                                            preset.description.clone()
                                        },
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


                // Custom CLI: a generic `Transport::Cli` seat for a vendor with no ACP
                // agent (or none the human wants to probe right now) — a raw
                // program+args+prompt-channel, not a boot-and-probe like the presets
                // above. See `WizardState::CustomCli` / `cli_seat_from`.
                list = list.child(
                    h_flex()
                        .id("preset-custom-cli")
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
                                        .child("Custom CLI…"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child("A one-shot CLI Hadron has no built-in preset for"),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Configure →"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            // Fresh form every time the wizard is (re-)entered — a stale
                            // vendor/program from a prior custom-CLI attempt must not leak
                            // into this one.
                            this.custom_cli_vendor.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_program.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_args.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_model.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_flag.update(cx, |s, cx| s.set_value("", window, cx));
                            this.custom_cli_channel = CliChannelChoice::Stdin;
                            this.wizard_state = WizardState::CustomCli;
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
                                        cli: None,
                                        // A seat the human just proved and saved is on.
                                        enabled: true,
                                        effort: None,
                                        mode_config: None,
                                        roles: vec![],
                                        exclusive: false,
                                        commands: hadron_lattice::SeatCommands::default(),
                                        secret_env: Vec::new(),
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

            WizardState::CustomCli => {
                // No boot-and-probe here (unlike `Connecting`) — a custom CLI is saved
                // straight from its fields via `cli_seat_from`, the same way the ACP
                // path saves straight from a `ProviderState::Ready` probe result.
                let stdin_selected = self.custom_cli_channel == CliChannelChoice::Stdin;
                let channel_toggle = h_flex()
                    .gap_2()
                    .child(
                        div()
                            .id("cli-channel-stdin")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .when(stdin_selected, |d| {
                                d.bg(theme::accent()).text_color(theme::text())
                            })
                            .when(!stdin_selected, |d| {
                                d.text_color(theme::text_secondary())
                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                            })
                            .child("Stdin")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.custom_cli_channel = CliChannelChoice::Stdin;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("cli-channel-arg")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .when(!stdin_selected, |d| {
                                d.bg(theme::accent()).text_color(theme::text())
                            })
                            .when(stdin_selected, |d| {
                                d.text_color(theme::text_secondary())
                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                            })
                            .child("Argv flag")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.custom_cli_channel = CliChannelChoice::Arg;
                                cx.notify();
                            })),
                    );

                // Required fields, checked live so "Save" is only wired up once the
                // form can actually produce a valid seat (`cli_seat_from` needs both to
                // build `id`/`cli.program`).
                let vendor_text = self.custom_cli_vendor.read(cx).value().trim().to_string();
                let vendor_present = !vendor_text.is_empty();
                let program_present = !self.custom_cli_program.read(cx).value().trim().is_empty();
                // This wizard is the FIRST UI that feeds freely-typed text into a
                // `QuarkId` — which becomes a worktree DIRECTORY name, a git BRANCH ref
                // segment, and a live-file name. A vendor like "foo/bar" would derive
                // an id `validate_quark_id` (the SSOT for what's safe there) rejects, so
                // check it here too and never even wire up Save for a bad one.
                let vendor_id_valid = vendor_present && custom_cli_vendor_is_valid(&vendor_text);
                let can_save = vendor_id_valid && program_present;

                let mut form = v_flex()
                    .size_full()
                    .gap_4()
                    .child(text_button("back-custom-cli", "← Back").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.wizard_state = WizardState::PickPreset;
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme::text())
                            .child("Custom CLI"),
                    )
                    .child(settings_field(
                        "Vendor (short label, e.g. \"ollama\")",
                        Input::new(&self.custom_cli_vendor).into_any_element(),
                    ))
                    .child(settings_field(
                        "Program (the binary to spawn)",
                        Input::new(&self.custom_cli_program).into_any_element(),
                    ))
                    .child(settings_field(
                        "Args (space-separated, optional)",
                        Input::new(&self.custom_cli_args).into_any_element(),
                    ))
                    .child(settings_field(
                        "Model (optional)",
                        Input::new(&self.custom_cli_model).into_any_element(),
                    ))
                    .child(settings_field("Prompt channel", channel_toggle.into_any_element()));

                if !stdin_selected {
                    form = form.child(settings_field(
                        "Flag (blank = positional argument, e.g. \"--prompt\")",
                        Input::new(&self.custom_cli_flag).into_any_element(),
                    ));
                }

                if !can_save {
                    let msg = if vendor_present && !vendor_id_valid {
                        "Vendor may only use letters, digits, '.', '_', and '-' — it becomes \
                         part of a worktree path and a git branch name."
                    } else {
                        "Vendor and program are required."
                    };
                    form = form.child(div().text_sm().text_color(theme::text_muted()).child(msg));
                }

                form.child(
                    text_button("save-custom-cli", "Save Custom CLI").when(can_save, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| {
                            let vendor =
                                this.custom_cli_vendor.read(cx).value().trim().to_string();
                            let program =
                                this.custom_cli_program.read(cx).value().trim().to_string();
                            if vendor.is_empty() || program.is_empty() || !custom_cli_vendor_is_valid(&vendor) {
                                return; // re-checked: `can_save` already gates this button
                            }
                            let args: Vec<String> = this
                                .custom_cli_args
                                .read(cx)
                                .value()
                                .split_whitespace()
                                .map(str::to_string)
                                .collect();
                            let model =
                                this.custom_cli_model.read(cx).value().trim().to_string();
                            let flag = this.custom_cli_flag.read(cx).value().trim().to_string();
                            let channel = prompt_channel_from(this.custom_cli_channel, &flag);

                            // Pure derivation (unit-tested directly) — the same shape as the
                            // ACP path's inline `Seat { .. }` literal, just for a Cli
                            // transport + a generic `CliSpec` instead of an `AcpCommand`.
                            // `cli_seat_from` already normalizes a stray transport prefix off
                            // `vendor` BEFORE deriving `id` (see its doc comment), so — unlike
                            // the ACP path — there is no separate `seat.normalize_vendor()`
                            // call needed here; doing it after would desync id from vendor.
                            let seat = cli_seat_from(&vendor, &program, args, channel, &model);
                            // Advisory only, never blocking — same as the ACP path.
                            if !hadron_lattice::id_follows_convention(
                                seat.id.as_str(),
                                seat.transport,
                            ) {
                                eprintln!(
                                    "chamber: note — id '{}' does not match the '{}-' convention",
                                    seat.id.as_str(),
                                    seat.transport.code()
                                );
                            }

                            this.providers.push(ConfiguredQuark {
                                id: seat.id.0.clone(),
                                transport: seat.transport.code().to_string(),
                                state: ProviderState::Ready { model: seat.model.clone() },
                            });

                            this.add_configured_quark(seat, cx);

                            this.wizard_state = WizardState::None;
                            cx.notify();
                        }))
                    }),
                )
            }
        }
    }
}

/// Add `var` to the seat's `secret_env` if it is not already declared. Returns
/// whether the seat changed (so the caller knows team.json needs saving) — a
/// blank var or one already present is a no-op. Pure and gpui-free, so it is
/// unit-tested directly instead of through the Settings overlay.
pub(super) fn declare_secret_var(seat: &mut Seat, var: &str) -> bool {
    if var.is_empty() || seat.secret_env.iter().any(|v| v == var) {
        false
    } else {
        seat.secret_env.push(var.to_string());
        true
    }
}

/// Remove `var` from the seat's `secret_env` if present. Returns whether the seat
/// changed. Mirrors [`declare_secret_var`] for the Clear action.
pub(super) fn undeclare_secret_var(seat: &mut Seat, var: &str) -> bool {
    let before = seat.secret_env.len();
    seat.secret_env.retain(|v| v != var);
    seat.secret_env.len() != before
}

/// Whether `var`'s value is currently set in `store` for `seat` — the "key set" /
/// "not set" status the masked field shows. Only ever reports presence, never the
/// value: a store error (e.g. no credential service available, as on a headless
/// WSL2 session) reads the same as "not set" rather than surfacing the error here.
pub(super) fn secret_status(store: &dyn SecretStore, seat: &QuarkId, var: &str) -> bool {
    store.get(seat, var).is_ok_and(|o| o.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::secrets::MemoryStore;
    use hadron_lattice::{Flavor, Seat};

    fn seat(id: &str) -> Seat {
        Seat::cli(QuarkId::new(id), "agy", "gemini-3-pro", Flavor::Worker)
    }

    #[test]
    fn declare_secret_var_adds_when_absent() {
        let mut s = seat("acp-agy");
        assert!(s.secret_env.is_empty());

        let changed = declare_secret_var(&mut s, "GEMINI_API_KEY");

        assert!(changed, "adding a new var must report a change");
        assert_eq!(s.secret_env, vec!["GEMINI_API_KEY".to_string()]);
    }

    #[test]
    fn declare_secret_var_noop_when_present() {
        let mut s = seat("acp-agy");
        s.secret_env.push("GEMINI_API_KEY".to_string());

        let changed = declare_secret_var(&mut s, "GEMINI_API_KEY");

        assert!(!changed, "an already-declared var must not report a change");
        assert_eq!(s.secret_env, vec!["GEMINI_API_KEY".to_string()], "must not duplicate");
    }

    #[test]
    fn declare_secret_var_ignores_blank() {
        let mut s = seat("acp-agy");
        let changed = declare_secret_var(&mut s, "");
        assert!(!changed);
        assert!(s.secret_env.is_empty());
    }

    #[test]
    fn undeclare_secret_var_removes_when_present() {
        let mut s = seat("acp-agy");
        s.secret_env.push("GEMINI_API_KEY".to_string());

        let changed = undeclare_secret_var(&mut s, "GEMINI_API_KEY");

        assert!(changed);
        assert!(s.secret_env.is_empty());
    }

    #[test]
    fn undeclare_secret_var_noop_when_absent() {
        let mut s = seat("acp-agy");
        let changed = undeclare_secret_var(&mut s, "GEMINI_API_KEY");
        assert!(!changed);
        assert!(s.secret_env.is_empty());
    }

    /// The behaviour the masked field's status line depends on: setting a value
    /// via a `SecretStore` flips the status to "set", clearing it flips it back —
    /// exercised against a `MemoryStore`, never a real keychain.
    #[test]
    fn set_then_status_reports_set() {
        let store = MemoryStore::new();
        let id = QuarkId::new("acp-agy");
        let var = "GEMINI_API_KEY";

        assert!(!secret_status(&store, &id, var), "unset must report not-set");

        store.set(&id, var, "sk-live-value").unwrap();
        assert!(secret_status(&store, &id, var), "set must report set");

        store.delete(&id, var).unwrap();
        assert!(!secret_status(&store, &id, var), "cleared must report not-set again");
    }

}
