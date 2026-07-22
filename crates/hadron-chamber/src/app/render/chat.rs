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
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("chat-body-scroll")
                    .size_full()
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
                    }),
            )
            .child(div().absolute().top_0().right_0().bottom_0().when(
                selected != ChatTab::Chat,
                |this| {
                    this.child(
                        Scrollbar::vertical(&self.chat_scrolls[selected.index()])
                            .scrollbar_show(ScrollbarShow::Hover),
                    )
                },
            ));

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
                            .bg(theme::input_bg())
                            // A hairline border lifts the field off the card behind it
                            // — the modern outlined-input look, using the shared token.
                            .border_1()
                            .border_color(if is_focused {
                                cx.theme().ring
                            } else {
                                theme::border().into()
                            })
                            .child(Input::new(&self.input).bordered(false).focus_bordered(false))
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
                                                    "Permission mode — Shift+Tab or click to cycle",
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
                                    crate::vcs::repo_root_of(&self.path).display().to_string(),
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
            .or_insert_with(|| color_mentions(body, roster))
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

    pub(super) fn chat_message_row(
        &self,
        id: &ResolvedIdentity,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
    ) -> impl IntoElement {
        let summary_chip = m.turn.as_ref().and_then(|turn_id| {
            let turn_events: Vec<&MessageRow> = self.view.messages.iter()
                .filter(|x| x.turn.as_ref() == Some(turn_id))
                .collect();
            if turn_events.is_empty() {
                return None;
            }
            let start_time = turn_events.iter().map(|x| x.ts).min()?;
            let duration_secs = (m.ts - start_time).num_seconds();
            let num_commands = turn_events.iter().filter(|x| x.kind_label == "command").count();
            let num_edits = turn_events.iter().filter(|x| x.kind_label == "edit").count();
            let num_tools = num_commands + num_edits;
            
            if duration_secs > 0 || num_tools > 0 {
                let mut parts = Vec::new();
                parts.push(format!("thought for {}s", duration_secs));
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
                    .child(self.markdown_body("chat-md", ix, &m.body, roster)),
            )
    }

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
            row = row.child(self.markdown_body("log-md", ix, &m.body, roster));
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
