use super::*;

impl super::Chamber {
    /// Whether the settings target `id` resolves to an ACP seat or a probing CLI seat —
    /// the seats whose Model field is a live dropdown rather than a free-text box.
    pub(super) fn is_acp_quark(&self, id: &str) -> bool {
        let seat = resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .cloned();
        let Some(seat) = seat else { return false };
        if seat.transport == hadron_lattice::Transport::Acp {
            return true;
        }
        if seat.transport == hadron_lattice::Transport::Cli {
            let spec = seat.cli.clone().unwrap_or_else(|| {
                hadron_lattice::CliSpec::preset(&seat.vendor).unwrap_or_else(|| {
                    hadron_lattice::CliSpec::generic(
                        seat.command.as_ref().map(|c| c.program.clone()).unwrap_or_default(),
                        seat.command.as_ref().map(|c| c.args.clone()).unwrap_or_default(),
                    )
                })
            });
            return spec.model_probe.is_some();
        }
        false
    }

    /// Re-probe the ACP agent or CLI binary backing `id` for every config selector it offers,
    /// parking the result in `acp_model_probe` for the Settings dropdowns.
    pub(super) fn start_acp_model_probe(&mut self, id: &str, cx: &mut Context<Self>) {
        let seat_opt = resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .cloned();
        let Some(seat) = seat_opt else {
            self.acp_model_probe = None;
            return;
        };

        let id_str = id.to_string();
        if seat.transport == hadron_lattice::Transport::Acp {
            let target = hadron_gluon::adapter::registry::AcpTarget::for_seat_with_env(&seat, self.secret_store.as_ref());
            let Some(target) = target else {
                self.acp_model_probe = None;
                return;
            };
            self.acp_model_probe = Some(AcpModelProbe { id: id_str.clone(), state: AcpModelState::Probing });
            cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = cx
                        .background_spawn(async move {
                            hadron_gluon::adapter::acp::probe_selectors(&target)
                        })
                        .await;
                    this.update(&mut cx, |this, cx| {
                        if !matches!(&this.acp_model_probe, Some(p) if p.id == id_str) {
                            return;
                        }
                        let state = match result {
                            Ok(sel) if sel.model.is_none() && sel.effort.is_none() => {
                                AcpModelState::Unavailable("this agent offers no model picker".into())
                            }
                            Ok(sel) => AcpModelState::Ready { selectors: sel },
                            Err(e) => AcpModelState::Unavailable(format!("couldn't detect models: {e}")),
                        };
                        this.acp_model_probe = Some(AcpModelProbe { id: id_str, state });
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        } else if seat.transport == hadron_lattice::Transport::Cli {
            let cli_spec = seat.cli.clone().unwrap_or_else(|| {
                hadron_lattice::CliSpec::preset(&seat.vendor).unwrap_or_else(|| {
                    hadron_lattice::CliSpec::generic(
                        seat.command.as_ref().map(|c| c.program.clone()).unwrap_or_default(),
                        seat.command.as_ref().map(|c| c.args.clone()).unwrap_or_default(),
                    )
                })
            });
            if cli_spec.model_probe.is_some() {
                self.acp_model_probe = Some(AcpModelProbe { id: id_str.clone(), state: AcpModelState::Probing });
                cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                    let mut cx = cx.clone();
                    async move {
                        let result = cx
                            .background_spawn(async move {
                                hadron_gluon::adapter::cli::probe_cli_models(&cli_spec)
                            })
                            .await;
                        this.update(&mut cx, |this, cx| {
                            if !matches!(&this.acp_model_probe, Some(p) if p.id == id_str) {
                                return;
                            }
                            let state = match result {
                                Ok(selector) => AcpModelState::Ready {
                                    selectors: hadron_gluon::adapter::acp::AcpSelectors {
                                        model: Some(selector),
                                        ..Default::default()
                                    },
                                },
                                Err(e) => AcpModelState::Unavailable(format!("couldn't detect models: {e}")),
                            };
                            this.acp_model_probe = Some(AcpModelProbe { id: id_str, state });
                            cx.notify();
                        })
                        .ok();
                    }
                })
                .detach();
            } else {
                self.acp_model_probe = None;
            }
        } else {
            self.acp_model_probe = None;
        }
    }

    /// Provision the `agy` ACP bridge's venv for `id`, off the UI thread — a no-op
    /// 1c in `.hadron/docs/plans/2026-07-28-shippable-bridge-and-self-update.md`.
    /// Mirrors [`Self::start_acp_model_probe`]'s shape: a probe that resolves after the
    /// human has moved to another quark is dropped rather than mis-applied.
    pub(super) fn start_agy_bridge_provision(&mut self, id: &str, cx: &mut Context<Self>) {
        let is_agy_acp = resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .map(|s| s.transport == hadron_lattice::Transport::Acp && s.vendor == "agy")
            .unwrap_or(false);
        if !is_agy_acp {
            self.agy_bridge_probe = None;
            return;
        }
        // Refresh the script BEFORE the already-provisioned early return. It is a byte
        // compare and a write only when they differ, and it is the only thing that
        // carries a Hadron upgrade's new bridge into `~/.hadron`. Behind the return, a
        // machine that provisioned once would run the version of the script it first
        // installed for the rest of time, which is the opposite of what 1b is for.
        if let Err(e) = hadron_gluon::adapter::bridge::materialize_script() {
            self.agy_bridge_probe = Some(AgyBridgeProbe {
                id: id.to_string(),
                state: AgyBridgeState::Failed(e.to_string()),
            });
            return;
        }
        if hadron_gluon::adapter::bridge::is_provisioned() {
            self.agy_bridge_probe = None;
            return;
        }
        let id = id.to_string();
        self.agy_bridge_probe =
            Some(AgyBridgeProbe { id: id.clone(), state: AgyBridgeState::Provisioning });
        cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // Blocking subprocesses (`python3 -m venv`, `pip install`), off the UI
                // thread — a cold install must not freeze the window.
                let result = cx
                    .background_spawn(async move {
                        hadron_gluon::adapter::bridge::materialize_script()?;
                        hadron_gluon::adapter::bridge::provision_venv()
                    })
                    .await;
                this.update(&mut cx, |this, cx| {
                    // Only the still-open probe may write its result.
                    if !matches!(&this.agy_bridge_probe, Some(p) if p.id == id) {
                        return;
                    }
                    let state = match result {
                        Ok(_) => AgyBridgeState::Ready,
                        Err(e) => AgyBridgeState::Failed(e.to_string()),
                    };
                    this.agy_bridge_probe = Some(AgyBridgeProbe { id, state });
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// A small status note for an in-progress/failed `agy` bridge venv provisioning —
    /// `None` when there is nothing worth saying (no probe open, or it already
    /// succeeded — a working bridge needs no announcement).
    pub(super) fn agy_bridge_status_row(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let probe = self.agy_bridge_probe.as_ref()?;
        let (msg, is_error) = match &probe.state {
            AgyBridgeState::Provisioning => {
                ("Setting up the Antigravity bridge (python venv)…".to_string(), false)
            }
            AgyBridgeState::Ready => return None,
            AgyBridgeState::Failed(reason) => (format!("Bridge setup failed: {reason}"), true),
        };
        let base = div().text_xs().text_color(theme::text_muted());
        Some(if is_error {
            // Click-to-copy, same reasoning as the model-probe error note: a failure
            // reason (e.g. a `pip install` stderr tail) is often longer than the panel.
            let full = msg.clone();
            base.id("agy-bridge-note")
                .cursor_pointer()
                .hover(|s| s.text_color(theme::text_secondary()))
                .child(format!("{msg}  ·  click to copy"))
                .on_click(cx.listener(move |_this, _, _window, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(full.clone()));
                }))
                .into_any_element()
        } else {
            base.child(msg).into_any_element()
        })
    }

    /// The status note shared by every probed selector row (model, effort, …) — they
    /// all come from the one boot in `start_acp_model_probe`, so "detecting" and "the
    /// probe failed" mean the same thing regardless of which field is being rendered.
    fn probe_note(&self) -> Option<(String, bool)> {
        match self.acp_model_probe.as_ref().map(|p| &p.state) {
            Some(AcpModelState::Probing) => Some(("Detecting…".into(), false)),
            Some(AcpModelState::Unavailable(msg)) => Some((msg.clone(), true)),
            _ => None,
        }
    }


    /// The ACP Model **select**: the agent's offered models in the shared searchable
    /// dropdown list component, with an "Inherit" row (blank → catalogue default).
    pub(super) fn acp_model_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let models = match self.acp_model_probe.as_ref().map(|p| &p.state) {
            Some(AcpModelState::Ready { selectors }) => selectors
                .model
                .as_ref()
                .map(|sel| sel.available.iter().map(|m| m.value.clone()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let selected = self.settings_model.read(cx).value().trim().to_string();
        let key = (selected.clone(), models.clone());
        if self.acp_model_select_key.as_ref() != Some(&key) {
            self.acp_model_select_key = Some(key);
            let delegate = create_model_delegate("Inherit", &models, Some(&selected));
            self.acp_model_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !selected.is_empty() {
                    s.set_selected_value(&selected.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }

        let mut col = v_flex().gap_1p5().child(
            Select::new(&self.acp_model_select_state)
                .w_full()
                .min_w(px(280.0))
                .placeholder("Select model...")
                .search_placeholder("Search models..."),
        );
        if let Some((msg, is_error)) = self.probe_note() {
            let base = div().text_xs().text_color(theme::text_muted());
            let note_el = if is_error {
                let full = msg.clone();
                base.id("acp-model-note")
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::text_secondary()))
                    .child(format!("{msg}  ·  click to copy"))
                    .on_click(cx.listener(move |_this, _, _window, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(full.clone()));
                    }))
                    .into_any_element()
            } else {
                base.child(msg).into_any_element()
            };
            col = col.child(note_el);
        }
        col.into_any_element()
    }

    /// The ACP Effort dropdown component — over the agent's advertised `ThoughtLevel` options.
    pub(super) fn acp_effort_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selector = match self.acp_model_probe.as_ref().map(|p| &p.state) {
            Some(AcpModelState::Ready { selectors }) => selectors.effort.as_ref(),
            _ => None,
        };
        let current_value = self.settings_effort.read(cx).value().trim().to_string();
        let mut available = Vec::new();
        if let Some(sel) = selector {
            for m in &sel.available {
                available.push(m.value.clone());
            }
        }
        if available.is_empty() {
            available = vec!["low".into(), "medium".into(), "high".into(), "max".into()];
        }
        let key = (current_value.clone(), available.clone());
        if self.effort_select_key.as_ref() != Some(&key) {
            self.effort_select_key = Some(key);
            let delegate = create_model_delegate("Inherit", &available, Some(&current_value));
            self.effort_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !current_value.is_empty() {
                    s.set_selected_value(&current_value.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }

        Select::new(&self.effort_select_state)
            .placeholder("Select reasoning effort...")
            .into_any_element()
    }

    /// General Reasoning Effort dropdown for non-ACP quarks.
    pub(super) fn general_effort_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_value = self.settings_effort.read(cx).value().trim().to_string();
        let available = vec!["low".to_string(), "medium".to_string(), "high".to_string(), "max".to_string()];
        let key = (current_value.clone(), available.clone());
        if self.effort_select_key.as_ref() != Some(&key) {
            self.effort_select_key = Some(key);
            let delegate = create_model_delegate("Inherit", &available, Some(&current_value));
            self.effort_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !current_value.is_empty() {
                    s.set_selected_value(&current_value.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }

        Select::new(&self.effort_select_state)
            .placeholder("Select reasoning effort...")
            .into_any_element()
    }
}
