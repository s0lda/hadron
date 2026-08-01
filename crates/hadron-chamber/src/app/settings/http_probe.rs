use super::*;

impl super::Chamber {
    /// Whether the settings target `id` resolves to a `Transport::Http` seat — the
    /// seats whose Model field is a live, searchable list (re-probed from the
    /// endpoint) rather than a free-text box. Mirrors [`Self::is_acp_quark`].
    pub(super) fn is_http_quark(&self, id: &str) -> bool {
        resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .map(|s| s.transport == hadron_lattice::Transport::Http)
            .unwrap_or(false)
    }

    /// Re-probe the HTTP seat backing `id` for its model list, parking the result
    /// in `http_model_probe` for the Settings picker. A no-op (clears the probe)
    /// for a non-Http quark or a seat whose vendor doesn't resolve to a target.
    /// Mirrors [`Self::start_acp_model_probe`]'s shape exactly — a method, not an
    /// probe that resolves after the human has moved to another quark is dropped.
    pub(super) fn start_http_model_probe(&mut self, id: &str, cx: &mut Context<Self>) {
        let target = resolve_team(&self.team, &self.global)
            .get(&QuarkId::new(id))
            .and_then(|seat| hadron_gluon::adapter::local::HttpTarget::for_seat_with_env(seat, self.secret_store.as_ref()));
        let Some(target) = target else {
            self.http_model_probe = None;
            return;
        };
        let id = id.to_string();
        self.http_model_probe = Some(HttpModelProbe { id: id.clone(), state: HttpModelState::Probing });
        cx.spawn(|this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // Blocking GET, off the UI thread — an unreachable server must not
                // freeze the window (mirrors the Connect wizard's probe).
                let result = cx
                    .background_spawn(async move { hadron_gluon::adapter::local::fetch_models(&target) })
                    .await;
                this.update(&mut cx, |this, cx| {
                    // Only the still-open probe may write its result.
                    if !matches!(&this.http_model_probe, Some(p) if p.id == id) {
                        return;
                    }
                    let state = match result {
                        Ok(models) if !models.is_empty() => {
                            HttpModelState::Ready { models: models.into_iter().map(|m| m.id).collect() }
                        }
                        Ok(_) => HttpModelState::Unavailable("connected, but no models loaded".into()),
                        Err(e) => HttpModelState::Unavailable(e.to_string()),
                    };
                    this.http_model_probe = Some(HttpModelProbe { id, state });
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// A small status note for an in-progress/failed HTTP model probe — `None` once
    /// the list is ready or before the first probe. Mirrors `acp_probe`'s
    /// `probe_note`.
    fn http_probe_note(&self) -> Option<(String, bool)> {
        match self.http_model_probe.as_ref().map(|p| &p.state) {
            Some(HttpModelState::Probing) => Some(("Detecting\u{2026}".into(), false)),
            Some(HttpModelState::Unavailable(msg)) => Some((msg.clone(), true)),
            _ => None,
        }
    }

    /// The HTTP seat Model **picker**: the endpoint's offered models in the shared
    /// searchable list, with an "Inherit" row (blank — no per-repo override).
    /// Blank stays editable/committable through `commit_settings_inputs` exactly as
    /// the free-text field was, so nothing regresses if the probe fails.
    pub(super) fn http_model_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let models = match self.http_model_probe.as_ref().map(|p| &p.state) {
            Some(HttpModelState::Ready { models }) => models.clone(),
            _ => Vec::new(),
        };
        let selected = self.settings_model.read(cx).value().trim().to_string();
        let key = (selected.clone(), models.clone());
        if self.http_model_select_key.as_ref() != Some(&key) {
            self.http_model_select_key = Some(key);
            let delegate = create_model_delegate("Inherit", &models, Some(&selected));
            self.http_model_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !selected.is_empty() {
                    s.set_selected_value(&selected.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }

        let mut col = v_flex().gap_1p5().child(
            Select::new(&self.http_model_select_state)
                .placeholder("Select model...")
                .search_placeholder("Search models..."),
        );
        if let Some((msg, is_error)) = self.http_probe_note() {
            let base = div().text_xs().text_color(theme::text_muted());
            let note_el = if is_error {
                let full = msg.clone();
                base.id("http-model-note")
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::text_secondary()))
                    .child(format!("{msg}  \u{b7}  click to copy"))
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

    /// A general Model select component for any seat (e.g. CLI seats) using native Select.
    pub(super) fn general_model_select(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected = self.settings_model.read(cx).value().trim().to_string();
        if self.general_model_select_key.as_ref() != Some(&selected) {
            self.general_model_select_key = Some(selected.clone());
            let delegate = create_model_delegate("Inherit", &[], Some(&selected));
            self.general_model_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !selected.is_empty() {
                    s.set_selected_value(&selected.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }
        Select::new(&self.general_model_select_state)
            .placeholder("Select model...")
            .search_placeholder("Search models...")
            .into_any_element()
    }
}
