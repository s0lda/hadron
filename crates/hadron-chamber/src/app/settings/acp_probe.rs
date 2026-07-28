use super::*;

impl super::Chamber {
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
            .and_then(|seat| hadron_gluon::adapter::registry::AcpTarget::for_seat_with_env(seat, self.secret_store.as_ref()));
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

    /// Provision the `agy` ACP bridge's venv for `id`, off the UI thread — a no-op
    /// (clearing any stale probe) for any seat that isn't an `agy` ACP seat, or one
    /// whose venv already exists. This is deliberately NOT part of seating
    /// (`QuarkKind::from_seat`/`AcpTarget::resolved`): that only checks the boot
    /// command's paths are well-formed, never that the venv is actually on disk, so a
    /// minutes-long `pip install` here can never block seating or dispatch — see task
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
        // (message, is_error). An error note is click-to-copy: a failed probe's
        // reason is often longer than the panel is wide, so the human must be able
        // to lift the whole thing rather than read a truncated head.
        let mut note: Option<(String, bool)> = None;
        match self.acp_model_probe.as_ref().map(|p| &p.state) {
            Some(AcpModelState::Probing) => note = Some(("Detecting models…".into(), false)),
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
            Some(AcpModelState::Unavailable(msg)) => note = Some((msg.clone(), true)),
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
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .text_xs()
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
        if let Some((msg, is_error)) = note {
            let base = div().text_xs().text_color(theme::text_muted());
            let note_el = if is_error {
                // Click-to-copy the full reason — a probe failure is often longer
                // than the panel, so lift the whole thing to the clipboard rather
                // than leave the human squinting at a truncated head.
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
}
