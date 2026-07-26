use super::*;

impl super::Chamber {
    /// The center column: a segmented Chat / Log / Timeline tab bar over the
    /// selected view, with the human's message box pinned at the foot. The whole
    /// thing is a rounded, filled card that floats on the unified canvas.
    pub(super) fn chat_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One row per quark mid-turn, not just the first — the card and the
        // roster's blue dot must agree on who counts as "working".
        let live_dir = hadron_lattice::live::live_dir(&self.path);
        let active = active_quarks(&self.view.roster, |id| {
            hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new(id), chrono::Utc::now())
        });

        let live_card = (!active.is_empty()).then(|| {
            v_flex()
                .w_full()
                .overflow_hidden()
                .gap_1p5()
                .px_3()
                .py_2()
                .mb_2()
                .rounded_lg()
                .bg(theme::glass_surface())
                .border_1()
                .border_color(theme::glass_highlight())
                .children(active.into_iter().map(|(quark_id_str, text)| {
                    let identity = self.resolve_identity(&quark_id_str);
                    let name = identity.name;
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(identity.color)
                                .child(format!("{}:", name)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .truncate()
                                .child(text),
                        )
                }))
        });

        let selected = self.chat_tab;
        let tabs = TabBar::new("chat-tabs")
            .segmented()
            // Flat #101010 so the segmented bar dissolves into the field (Jake's request).
            .bg(theme::field_base())
            .selected_index(selected.index())
            .children(ChatTab::ALL.map(|t| {
                // The active tab reads as a dark cutout; give its label the pink
                // accent so the selection is unmistakable. Inactive tabs keep the
                // default muted label.
                if t.index() == selected.index() {
                    Tab::new().child(
                        div()
                            .text_color(theme::accent())
                            .child(t.label().to_string()),
                    )
                } else {
                    Tab::new().label(t.label())
                }
            }))
            .on_click(cx.listener(|this, ix: &usize, _window, cx| {
                this.chat_tab = ChatTab::from_index(*ix);
                cx.notify();
            }));

        let header = h_flex()
            .flex_none()
            .items_center()
            .px_3()
            .py_2()
            .child(tabs);

        // The scrolling viewport: the selected view stacks to its natural height
        // and scrolls *within* the card, instead of growing the card and pushing
        // the input (and the whole layout) off the bottom. The hover scrollbar is
        // an absolute sibling of the scrolled content (not a child of it, or it
        // would scroll away), reading the same handle.
        let scroll_container = div()
            .id("chat-body-scroll")
            .size_full()
            .relative()
            .child(match selected {
                ChatTab::Chat => self.chat_view(cx).into_any_element(),
                ChatTab::Log => self.log_view(cx).into_any_element(),
                ChatTab::Stats => div()
                    .id("session-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.chat_scrolls[selected.index()])
                    .child(self.stats_view(cx))
                    .into_any_element(),
            });

        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(match selected {
                ChatTab::Chat => scroll_container.vertical_scrollbar(&self.chat_list_state),
                ChatTab::Log => scroll_container.vertical_scrollbar(&self.log_list_state),
                ChatTab::Stats => scroll_container.vertical_scrollbar(&self.chat_scrolls[selected.index()]),
            });

        // The message box is only meaningful in Chat — you talk to the field
        // there. Log and Timeline are read-only views, so they get no input.
        let input =
            matches!(selected, ChatTab::Chat).then(|| {
                v_flex()
                    .flex_none()
                    .m_4()
                    // Anchor for the completion card, which is `.absolute()` above.
                    .relative()
                    // The focused Input binds Up/Down/Escape at the deepest node, so
                    // intercept those actions in the capture phase (ancestor-first)
                    // while a card is open — move the highlight / close it instead of
                    // moving the caret. Gated on `is_some()` so normal cursor movement
                    // is untouched when there is no card (advisor's trap #1).
                    .capture_action(cx.listener(|this, _: &MoveDown, _window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(1, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, _window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(-1, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, _window, cx| {
                        if this.completion.take().is_some() {
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .when(self.completion.is_some(), |el| {
                        el.child(self.completion_card_overlay(cx))
                    })
                    .when_some(live_card, |el, card| el.child(card))
                    .child({
                        let is_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
                        h_flex()
                            .px_1()
                            .rounded_lg()
                            .bg(theme::field_base())
                            // A hairline border lifts the field off the card behind it
                            // — the modern outlined-input look, using the shared token.
                            .border_1()
                            .border_color(if is_focused {
                                theme::accent_soft()
                            } else {
                                theme::border()
                            })
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .mt_2()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child("Global Mode:"),
                                    )
                                    .child(
                                        div()
                                            .id("global-mode")
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.cycle_global_mode(cx)
                                            }))
                                            .tooltip(|window, cx| {
                                                Tooltip::new(
                                                    // F6, not Shift+Tab: `shift-tab` was
                                                    // abandoned as the CycleMode chord (the
                                                    // text input claims it for OutdentInline),
                                                    // and the tooltip kept the dead one.
                                                    // `app/mod.rs::default_key_bindings` is
                                                    // the truth.
                                                    "Permission mode — F6 or click to cycle",
                                                )
                                                .build(window, cx)
                                            })
                                            // This chip IS the global mode selector, so it
                                            // must always show the live ASK/WRITE/AUTO/BYPASS,
                                            // never "Default". `is_default = false` does that
                                            // (the grey "Default" chip is only for a per-quark
                                            // roster row with no override).
                                            .child(mode_tag(self.view.global_mode, false)),
                                    ),
                            )
                            .child(
                                div().text_xs().text_color(theme::text_muted()).child(
                                    crate::vcs::format_working_dir(&self.path),
                                ),
                            ),
                    )
            });

        // The floating chat card: darker + rounded, inset from the lighter
        // unified space that shows around it.
        let card = v_flex()
            .flex_1()
            .min_h_0()
            .rounded(INNER_RADIUS)
            .overflow_hidden()
            // Glass: a faint top sheen + a hairline top highlight, so the dark
            // layer reads as a lit panel rather than a flat black rectangle.
            .bg(theme::glass_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(header)
            .children(self.gluon_stopped_toast(cx))
            .children(self.permission_toast(cx))
            .child(body)
            .children(input);

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

    /// The Chat tab: the conversation only (message events), styled like a chat
    /// with each author's avatar and name.
    pub(super) fn chat_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.chat_message_ixs.is_empty() {
            return v_flex()
                .p_4()
                .child(empty_hint("No messages yet — say something below."))
                .into_any_element();
        }

        let weak_view = cx.entity().downgrade();

        // Wrap the virtual list with padding
        v_flex()
            .size_full()
            .p_4()
            .child(
                gpui::list(self.chat_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |this, _cx| {
                            if let Some(&real_ix) = this.chat_message_ixs.get(ix) {
                                if let Some(m) = this.view.messages.get(real_ix) {
                                    let mut add_divider = false;
                                    if ix > 0 {
                                        if let Some(&prev_real_ix) = this.chat_message_ixs.get(ix - 1) {
                                            if let Some(prev_m) = this.view.messages.get(prev_real_ix) {
                                                if prev_m.ts.date_naive() != m.ts.date_naive() {
                                                    add_divider = true;
                                                }
                                            }
                                        }
                                    } else {
                                        add_divider = true;
                                    }
                                    
                                    let mut row = div().pb(px(16.0));
                                    if add_divider {
                                        let label = crate::model::date_divider_label(
                                            m.ts.date_naive(),
                                            chrono::Local::now().date_naive(),
                                        );
                                        row = row.child(
                                            div().flex().items_center().justify_center().pt_2().pb_6().child(
                                                div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(theme::text_muted()).child(label)
                                            )
                                        );
                                    }
                                    
                                    return row
                                        .child(this.chat_message_row(
                                            &this.resolve_identity(&m.from),
                                            m,
                                            real_ix,
                                            &this.view.roster,
                                        ))
                                        .into_any_element();
                                }
                            }
                            div().into_any_element()
                        })
                    } else {
                        div().into_any_element()
                    }
                })
                .size_full(),
            )
            .into_any_element()
    }

    /// The Log tab: every event on the field, compact (the raw activity).
    pub(super) fn log_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view.messages.is_empty() {
            return v_flex().gap_3().p_4()
                .child(empty_hint("The field is empty."))
                .into_any_element();
        }

        let weak_view = cx.entity().downgrade();

        v_flex()
            .size_full()
            .p_3()
            .child(
                gpui::list(self.log_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |this, cx| {
                            if let Some(m) = this.view.messages.get(ix) {
                                let mut add_divider = false;
                                if ix > 0 {
                                    if let Some(prev_m) = this.view.messages.get(ix - 1) {
                                        if prev_m.ts.date_naive() != m.ts.date_naive() {
                                            add_divider = true;
                                        }
                                    }
                                } else {
                                    add_divider = true;
                                }

                                let mut row = v_flex().w_full();
                                if add_divider {
                                    let label = crate::model::date_divider_label(
                                        m.ts.date_naive(),
                                        chrono::Local::now().date_naive(),
                                    );
                                    row = row.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .pt_3()
                                            .pb_2()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(theme::text_muted())
                                                    .child(label),
                                            ),
                                    );
                                }

                                let expanded = this.log_expanded.contains(&ix);
                                return row
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("log-row-{ix}")))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !this.log_expanded.remove(&ix) {
                                                    this.log_expanded.insert(ix);
                                                }
                                                cx.notify();
                                            }))
                                            .child(log_row(m, expanded, this.color_for(&m.from))),
                                    )
                                    .into_any_element();
                            }
                            div().into_any_element()
                        })
                    } else {
                        div().into_any_element()
                    }
                })
                .size_full(),
            )
            .into_any_element()
    }

    /// Render a message body as Markdown under an element id unique to `(view, ix)`.
    ///
    /// The id is load-bearing, not decoration. `gpui_component::text::markdown()`
    /// derives its `ElementId` from `Location::caller()`, so every row rendered from
    /// one call site would share a single id — and the `TextView`'s parsed state is
    /// keyed on that id. All messages would then share one state, whose `set_text`
    /// would see different text on every message and re-parse (and re-highlight) the
    /// Markdown for every row, every frame. Distinct ids give each row its own state,
    /// so `set_text` early-returns and the parse happens once per body.
    ///
    /// Keying on the positional `ix` is sound only because the field is append-only and
    /// rendered oldest-first, so a given message keeps its index for the window's life.
    /// If rows ever get reordered or filtered, key on a stable message id instead — the
    /// cache would silently stop helping, and no test would catch the regression.
    pub(super) fn markdown_body(
        &self,
        view: &'static str,
        ix: usize,
        body: &str,
        roster: &[crate::model::RosterRow],
    ) -> impl IntoElement {
        let mut cache = self.parsed_markdown.borrow_mut();
        let content = cache
            .entry(ix)
            .or_insert_with(|| resolve_mention_names(body, roster))
            .clone();

        div().text_size(px(13.65)).child(
            gpui_component::text::TextView::markdown((view, ix), content)
                .selectable(true)
                .style(markdown_style())
                .code_block_actions(|code_block, _window, _cx| {
                    gpui_component::clipboard::Clipboard::new("code-copy")
                        .value(code_block.code())
                        .tooltip("Copy")
                }),
        )
    }

    /// A gluon message rendered inside a severity card — the SINGLE place the
    /// accent colours live. Both the chat bubble and the Log row call this; before
    /// it, each carried its own copy of the match, which is the shape that already
    /// cost us `presence-dot-was-computed-in-three-places`.
    ///
    /// **The hue values are normalised 0..1, not degrees.** `gpui::hsla` does
    /// `h.clamp(0., 1.)` (`gpui-0.2.2/src/color.rs:338`), so the previous
    /// `hsla(40.0, …)` "amber" clamped to `1.0` — 360°, i.e. the *same red* as
    /// Error. Warning and Error were visually identical. Passing degrees here
    /// silently yields red for anything ≥ 1.
    fn severity_card(
        &self,
        severity: Option<hadron_lattice::Severity>,
        view: &'static str,
        ix: usize,
        body: &str,
        roster: &[crate::model::RosterRow],
        pad: gpui::Pixels,
    ) -> gpui::AnyElement {
        let Some((hue, fill_sat, border_sat)) = severity_accent(severity) else {
            return self.markdown_body(view, ix, body, roster).into_any_element();
        };
        div()
            .p(pad)
            .rounded_md()
            .bg(gpui::hsla(hue, fill_sat, 0.50, 0.10))
            .border_l(gpui::px(3.0))
            .border_color(gpui::hsla(hue, border_sat, 0.50, 1.0))
            .child(self.markdown_body(view, ix, body, roster))
            .into_any_element()
    }

    pub(super) fn chat_message_row(
        &self,
        id: &ResolvedIdentity,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
    ) -> impl IntoElement {
        let summary_chip = turn_summary_parts(&self.view.messages, m).and_then(|(duration_secs, num_tools)| {
            if duration_secs > 0 || num_tools > 0 {
                let mut parts = Vec::new();
                parts.push(format!("thought for {}", format_duration(duration_secs)));
                if num_tools > 0 {
                    parts.push(format!("ran {} tool{}", num_tools, if num_tools == 1 { "" } else { "s" }));
                }
                Some(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .mb_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("⟳ {}", parts.join(" · ")))
                        )
                )
            } else {
                None
            }
        });

        h_flex()
            .items_start()
            .gap_2p5()
            .child(identity_avatar(id, 28.0))
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(id.color).child(id.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(crate::model::format_clock(m.ts.with_timezone(&chrono::Local))),
                            )
                            .when_some(m.to.clone(), |this, to| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(format!("→ {to}")),
                                )
                            })
                            .when_some(m.usage.as_ref(), |this, u| {
                                let mut parts = Vec::new();
                                if let Some(ctx) = &u.context {
                                    parts.push(format!("ctx: {:.1}%", ctx.used_percentage));
                                }
                                if !u.spend.is_empty() {
                                    let fresh = u.spend.fresh().unwrap_or(0);
                                    let cached = u.spend.cached().unwrap_or(0);
                                    let cost_str = if let Some(c) = u.cost_usd() { format!(" (${:.2})", c) } else { "".to_string() };
                                    if cached > 0 {
                                        parts.push(format!(
                                            "spent: {} fresh, {} cached{}",
                                            fresh, cached, cost_str
                                        ));
                                    } else {
                                        parts.push(format!("spent: {} fresh{}", fresh, cost_str));
                                    }
                                }
                                if parts.is_empty() {
                                    this
                                } else {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .child(format!("({})", parts.join(" | "))),
                                    )
                                }
                            }),
                    )
                    .when_some(summary_chip, |this, chip| this.child(chip))
                    .child(self.severity_card(
                        m.severity,
                        "chat-md",
                        ix,
                        &m.body,
                        roster,
                        gpui::px(12.0),
                    )),
            )
    }

    #[allow(dead_code)] // superseded by chat_message_row; kept pending removal
    pub(super) fn message_row(
        &self,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_expanded = self.log_expanded_ixs.contains(&ix);
        
        let mut header_row = h_flex()
            .gap_2()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _window, _cx| {
                if this.log_expanded_ixs.contains(&ix) {
                    this.log_expanded_ixs.remove(&ix);
                } else {
                    this.log_expanded_ixs.insert(ix);
                }
            }))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::actor_hue(&m.from))
                            .child(if is_expanded { format!("▼ {}", m.from) } else { format!("▶ {}", m.from) }),
                    )
                    .when_some(m.to.clone(), |this, to| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(format!("→ {}", to)),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(crate::model::format_clock(m.ts.with_timezone(&chrono::Local))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("· {}", m.kind_label)),
                    ),
            );
            
        if let Some(u) = m.usage.as_ref() {
            let mut parts = Vec::new();
            if let Some(ctx) = &u.context {
                parts.push(format!("ctx: {:.1}%", ctx.used_percentage));
            }
            if !u.spend.is_empty() {
                let fresh = u.spend.fresh().unwrap_or(0);
                let cached = u.spend.cached().unwrap_or(0);
                let cost_str = if let Some(c) = u.cost_usd() { format!(" (${:.2})", c) } else { "".to_string() };
                if cached > 0 {
                    parts.push(format!("spent: {} fresh, {} cached{}", fresh, cached, cost_str));
                } else {
                    parts.push(format!("spent: {} fresh{}", fresh, cost_str));
                }
            }
            if !parts.is_empty() {
                header_row = header_row.child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!("({})", parts.join(" | "))),
                );
            }
        }
        
        let mut row = v_flex().gap_1().child(header_row);
        
        if is_expanded {
            row = row.child(self.severity_card(
                m.severity,
                "log-md",
                ix,
                &m.body,
                roster,
                gpui::px(8.0),
            ));
        } else {
            let snippet = m.body.lines().next().unwrap_or("").chars().take(80).collect::<String>();
            let suffix = if m.body.len() > snippet.len() { "..." } else { "" };
            row = row.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{}{}", snippet, suffix))
            );
        }

        row
    }
}

/// The accent for a gluon message's severity card: `(hue, fill_saturation,
/// border_saturation)`, or `None` for an unlabelled message (no card).
///
/// Pure and free-standing so the one thing that actually went wrong here is
/// testable: **`gpui::hsla` clamps hue to `0..=1`** (`gpui-0.2.2/src/color.rs:338`),
/// it does NOT take degrees. The original cards passed `40.0` for "amber", which
/// clamped to `1.0` — 360°, the same red as Error — so Warning and Error rendered
/// identically and nobody could tell. Every hue here is `degrees / 360.0`.
fn severity_accent(severity: Option<hadron_lattice::Severity>) -> Option<(f32, f32, f32)> {
    match severity? {
        hadron_lattice::Severity::Error => Some((0.0, 0.70, 0.85)), // red 0°
        hadron_lattice::Severity::Warning => Some((40.0 / 360.0, 0.90, 0.95)), // amber 40°
        hadron_lattice::Severity::Info => Some((210.0 / 360.0, 0.55, 0.70)), // blue 210°
    }
}

#[cfg(test)]
mod severity_tests {
    use super::severity_accent;
    use hadron_lattice::Severity;

    /// An unlabelled message renders bare — a card round every ordinary chat
    /// message would drown the ones that matter.
    #[test]
    fn no_severity_means_no_card() {
        assert_eq!(severity_accent(None), None);
    }

    /// The regression that motivated extracting this: hue must be normalised, so
    /// every accent has to survive `hsla`'s `clamp(0., 1.)` unchanged. A value ≥ 1
    /// silently becomes red.
    #[test]
    fn every_hue_is_normalised_not_degrees() {
        for sev in [Severity::Info, Severity::Warning, Severity::Error] {
            let (h, ..) = severity_accent(Some(sev)).unwrap();
            assert_eq!(h, h.clamp(0.0, 1.0), "{sev:?} hue {h} is not in 0..=1 — gpui would clamp it to red");
        }
    }

    /// …and the three must be visually distinct. Before the fix, Warning and Error
    /// both landed on red and the card told the human nothing.
    #[test]
    fn the_three_severities_are_different_colours() {
        let hues: Vec<f32> = [Severity::Info, Severity::Warning, Severity::Error]
            .into_iter()
            .map(|s| severity_accent(Some(s)).unwrap().0)
            .collect();
        for (i, a) in hues.iter().enumerate() {
            for b in &hues[i + 1..] {
                assert!((a - b).abs() > 0.02, "two severities share hue {a}");
            }
        }
    }
}

/// Helper to compute turn duration (seconds) and tool count for a given message row.
/// Resolves turn start time from the preceding `Status::Excited` event for the same actor
/// if available, falling back to the earliest event associated with the turn ULID.
pub(super) fn turn_summary_parts(
    messages: &[MessageRow],
    m: &MessageRow,
) -> Option<(i64, usize)> {
    let turn_id = m.turn.as_ref()?;
    let turn_events: Vec<&MessageRow> = messages
        .iter()
        .filter(|x| x.turn.as_ref() == Some(turn_id))
        .collect();
    if turn_events.is_empty() {
        return None;
    }
    let start_time = messages
        .iter()
        .rev()
        .find(|x| x.ts <= m.ts && x.from == m.from && x.kind_label == "status" && x.body == "excited")
        .map(|x| x.ts)
        .or_else(|| turn_events.iter().map(|x| x.ts).min())?;
    let duration_secs = m.ts.signed_duration_since(start_time).num_seconds().max(0);
    let num_commands = turn_events.iter().filter(|x| x.kind_label == "command").count();
    let num_edits = turn_events.iter().filter(|x| x.kind_label == "edit").count();
    let num_tools = num_commands + num_edits;
    Some((duration_secs, num_tools))
}

/// Format turn duration in seconds to human-friendly strings like "<1s", "5s", "1m 02s", "2m 34s", "1h 02m 05s".
pub(super) fn format_duration(secs: i64) -> String {
    if secs < 1 {
        return "<1s".to_string();
    }
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;

    if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, mins, s)
    } else if mins > 0 {
        format!("{}m {:02}s", mins, s)
    } else {
        format!("{}s", s)
    }
}

#[cfg(test)]
mod turn_summary_tests {
    use super::turn_summary_parts;
    use crate::model::MessageRow;
    use chrono::{Duration, Utc};

    #[test]
    fn calculates_actual_turn_duration_from_excited_status() {
        let now = Utc::now();
        let turn_id = "01J00000000000000000000000".to_string();
        let qid = "acp-agy".to_string();

        let msgs = vec![
            // Excited status event when turn starts at T=0
            MessageRow {
                from: qid.clone(),
                to: None,
                body: "excited".to_string(),
                kind_label: "status",
                usage: None,
                ts: now,
                legacy_used_tokens: None,
                turn: None,
                severity: None,
            },
            // Tool execution event at T=15s
            MessageRow {
                from: qid.clone(),
                to: None,
                body: "edited 1 path".to_string(),
                kind_label: "edit",
                usage: None,
                ts: now + Duration::seconds(15),
                legacy_used_tokens: None,
                turn: Some(turn_id.clone()),
                severity: None,
            },
            // Final message row at T=15s
            MessageRow {
                from: qid.clone(),
                to: None,
                body: "done".to_string(),
                kind_label: "message",
                usage: None,
                ts: now + Duration::seconds(15),
                legacy_used_tokens: None,
                turn: Some(turn_id.clone()),
                severity: None,
            },
        ];

        let m = &msgs[2];
        let (duration, tools) = turn_summary_parts(&msgs, m).expect("summary parts found");
        assert_eq!(duration, 15, "turn duration should be 15 seconds, not 0");
        assert_eq!(tools, 1, "should count 1 tool");
    }

    #[test]
    fn formats_duration_human_friendly() {
        use super::format_duration;
        assert_eq!(format_duration(0), "<1s");
        assert_eq!(format_duration(5), "5s");
        assert_eq!(format_duration(60), "1m 00s");
        assert_eq!(format_duration(62), "1m 02s");
        assert_eq!(format_duration(154), "2m 34s");
        assert_eq!(format_duration(530), "8m 50s");
        assert_eq!(format_duration(3725), "1h 02m 05s");
    }
}

