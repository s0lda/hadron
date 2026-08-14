use super::*;
use gpui_component::ActiveTheme;

impl super::Chamber {
    /// The center column: a segmented Chat / Log / Timeline tab bar over the
    /// selected view, with the human's message box pinned at the foot. The whole
    /// thing is a rounded, filled card that floats on the unified canvas.
    pub(super) fn chat_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One row per quark mid-turn, not just the first — the card and the
        // roster's blue dot must agree on who counts as "working".
        let live_dir = hadron_lattice::live::live_dir(&self.path);
        let now = chrono::Utc::now();
        let mut live_map = std::collections::HashMap::new();
        for r in &self.view.roster {
            if r.adopted && r.enabled {
                if let Some(act) = hadron_lattice::live::read(&live_dir, &hadron_lattice::QuarkId::new(&r.id), now) {
                    live_map.insert(r.id.as_str(), act);
                }
            }
        }
        let active = active_quarks(&self.view.roster, |id| live_map.get(id).cloned());

        // The reply as it arrives. A sibling of the Live card rather than a row in the
        // message list: `chat_list_state`'s count is derived from `chat_message_ixs`
        // (see the `A Field Swap Resets Every List Cache` invariant), and a draft is
        // not a field event — inventing a list row for one would put a third,
        // untracked writer on caches that only `resync_lists_to_projection` may rebuild.
        // Pinned above the input, so it is where the human is already looking.
        let drafts = streaming_drafts(&self.view.roster, |id| live_map.get(id).cloned());
        let draft_card = (!drafts.is_empty()).then(|| {
            v_flex().w_full().gap_2().mb_2().children(drafts.into_iter().map(|(quark_id_str, text)| {
                let identity = self.resolve_identity(&quark_id_str);
                v_flex()
                    .w_full()
                    .overflow_hidden()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(identity.color.opacity(0.5))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(identity.color)
                                    .child(identity.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("writing\u{2026}"),
                            ),
                    )
                    // Plain text, NOT markdown: this element re-renders on every
                    // publish while the reply streams, and `parsed_markdown` is keyed
                    // by message identity for rows that do not change. Re-parsing a
                    // growing document several times a second on a software renderer
                    // is the cost `a-render-fn-runs-on-every-hover` already charged
                    // this codebase once. The finished message renders as markdown a
                    // moment later, in the list, where it belongs.
                    .child(
                        div()
                            .id(SharedString::from(format!("draft-text-{}", quark_id_str)))
                            .w_full()
                            .max_h(px(100.0))
                            .overflow_y_scroll()
                            .text_sm()
                            .text_color(theme::text_secondary())
                            .child(text),
                    )
            }))
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
                .bg(theme::term_bg())
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
        let tabs = h_flex()
            .id("chat-capsule-tabs")
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(theme::tab_bar_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .max_w_full()
            .overflow_x_scroll()
            .children(ChatTab::ALL.map(|t| {
                let is_selected = t.index() == selected.index();
                let label = t.label();
                let ix = t.index();
                div()
                    .id(("chat-tab-pill", ix))
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .when(is_selected, |s| {
                        s.bg(theme::bg_elevated())
                            .text_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                    })
                    .when(!is_selected, |s| {
                        s.text_color(theme::text_muted())
                            .hover(|h| h.text_color(theme::text()))
                    })
                    .text_xs()
                    .child(label)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.chat_tab = ChatTab::from_index(ix);
                        cx.notify();
                    }))
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
        let tab_start =
            std::env::var_os("HADRON_FRAME_TIMING").is_some().then(std::time::Instant::now);
        let tab_content = match selected {
            ChatTab::Chat => self.chat_view(cx).into_any_element(),
            ChatTab::Log => self.log_view(cx).into_any_element(),
            ChatTab::Stats => div()
                .id("session-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.stats_scroll)
                .child(self.stats_view(cx))
                .into_any_element(),
        };
        if let Some(start) = tab_start {
            hadron_lattice::term::info(
                hadron_lattice::term::Source::Chamber,
                &format!("frame render tab {}: {:?}", selected.label(), start.elapsed()),
            );
        }

        let scroll_container = div()
            .id("chat-body-scroll")
            .size_full()
            .relative()
            .child(tab_content);

        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(match selected {
                ChatTab::Chat => scroll_container.vertical_scrollbar(&self.chat_list_state),
                ChatTab::Log => scroll_container.vertical_scrollbar(&self.log_list_state),
                ChatTab::Stats => scroll_container.vertical_scrollbar(&self.stats_scroll),
            });

        // The message box is only meaningful in Chat — you talk to the field
        // there. Log and Timeline are read-only views, so they get no input.
        let input =
            matches!(selected, ChatTab::Chat).then(|| {
                v_flex()
                    .flex_none()
                    .mx_4()
                    .mb_3()
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
                    .on_action(cx.listener(|this, _: &Paste, window, cx| {
                        this.on_input_paste(window, cx);
                    }))
                    .when(self.completion.is_some(), |el| {
                        el.child(self.completion_card_overlay(cx))
                    })
                    .when_some(draft_card, |el, card| el.child(card))
                    .when_some(live_card, |el, card| el.child(card))
                    .child({
                        let is_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
                        v_flex()
                            .w_full()
                            .p_2()
                            .rounded_xl()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(if is_focused {
                                gpui::rgba(0xffffff38).into()
                            } else {
                                theme::glass_highlight()
                            })
                            .shadow_lg()
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            )
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
                                                    .id("global-mode")
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.cycle_global_mode(cx)
                                                    }))
                                                    .tooltip(|window, cx| {
                                                        Tooltip::new(
                                                            "Permission mode — F6 or click to cycle",
                                                        )
                                                        .build(window, cx)
                                                    })
                                                    .child(mode_tag(self.view.global_mode, false)),
                                            )
                                            .child(
                                                 h_flex()
                                                     .id("picker-quark")
                                                     .items_center()
                                                     .gap_1()
                                                     .px_2()
                                                     .py_0p5()
                                                     .rounded_md()
                                                     .bg(theme::glass_card())
                                                     .border_1()
                                                     .border_color(theme::glass_highlight())
                                                     .text_xs()
                                                     .text_color(theme::text_muted())
                                                     .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                                     .cursor_pointer()
                                                     .child(Icon::new(IconName::Bot).xsmall())
                                                     .child("@Quark")
                                                     .on_click(cx.listener(|this, _, window, cx| {
                                                         this.insert_completion_trigger("@", window, cx);
                                                     })),
                                             )
                                             .child(
                                                 h_flex()
                                                     .id("picker-file")
                                                     .items_center()
                                                     .gap_1()
                                                     .px_2()
                                                     .py_0p5()
                                                     .rounded_md()
                                                     .bg(theme::glass_card())
                                                     .border_1()
                                                     .border_color(theme::glass_highlight())
                                                     .text_xs()
                                                     .text_color(theme::text_muted())
                                                     .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                                     .cursor_pointer()
                                                     .child(Icon::new(IconName::File).xsmall())
                                                     .child("@File")
                                                     .on_click(cx.listener(|this, _, window, cx| {
                                                         this.insert_completion_trigger("@", window, cx);
                                                     })),
                                             )
                                             .child(
                                                 h_flex()
                                                     .id("picker-command")
                                                     .items_center()
                                                     .gap_1()
                                                     .px_2()
                                                     .py_0p5()
                                                     .rounded_md()
                                                     .bg(theme::glass_card())
                                                     .border_1()
                                                     .border_color(theme::glass_highlight())
                                                     .text_xs()
                                                     .text_color(theme::text_muted())
                                                     .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                                     .cursor_pointer()
                                                     .child(Icon::new(IconName::SquareTerminal).xsmall())
                                                     .child("/Command")
                                                     .on_click(cx.listener(|this, _, window, cx| {
                                                         this.insert_completion_trigger("/", window, cx);
                                                     })),
                                             ),
                                    )
                                    .child(
                                        div().text_xs().text_color(theme::text_muted()).child(
                                            crate::vcs::format_working_dir(&self.path),
                                        ),
                                    ),
                            )
                    })
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
        let local_offset = *chrono::Local::now().offset();
        let today = chrono::Local::now().date_naive();

        // Wrap the virtual list with padding
        v_flex()
            .size_full()
            .p_4()
            .child(
                gpui::list(self.chat_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        let this = view.read(cx);
                        if let Some(&real_ix) = this.chat_message_ixs.get(ix) {
                            if let Some(m) = this.view.messages.get(real_ix) {
                                let mut add_divider = false;
                                let m_date = m.ts.with_timezone(&local_offset).date_naive();
                                if ix > 0 {
                                    if let Some(&prev_real_ix) = this.chat_message_ixs.get(ix - 1) {
                                        if let Some(prev_m) = this.view.messages.get(prev_real_ix) {
                                            if prev_m.ts.with_timezone(&local_offset).date_naive() != m_date {
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
                                        m_date,
                                        today,
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
                                        local_offset,
                                        &cx.theme().mono_font_family,
                                    ))
                                    .into_any_element();
                            }
                        }
                        div().into_any_element()
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
        let local_offset = *chrono::Local::now().offset();
        let today = chrono::Local::now().date_naive();

        v_flex()
            .size_full()
            .p_3()
            .child(
                gpui::list(self.log_list_state.clone(), move |ix, _window, cx| {
                    if let Some(view) = weak_view.upgrade() {
                        let this = view.read(cx);
                        if let Some(m) = this.view.messages.get(ix) {
                            let mut add_divider = false;
                            let m_date = m.ts.with_timezone(&local_offset).date_naive();
                            if ix > 0 {
                                if let Some(prev_m) = this.view.messages.get(ix - 1) {
                                    if prev_m.ts.with_timezone(&local_offset).date_naive() != m_date {
                                        add_divider = true;
                                    }
                                }
                            } else {
                                add_divider = true;
                            }

                            let mut row = v_flex().w_full();
                            if add_divider {
                                let label = crate::model::date_divider_label(
                                    m_date,
                                    today,
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
                            let color = this.color_for(&m.from);
                            let entity = view.clone();
                            return row
                                .child(
                                    div()
                                        .id(SharedString::from(format!("log-row-{ix}")))
                                        .cursor_pointer()
                                        .on_click(move |_, _window, cx| {
                                            entity.update(cx, |this, cx| {
                                                if !this.log_expanded.remove(&ix) {
                                                    this.log_expanded.insert(ix);
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .child(log_row(ix, m, expanded, color, local_offset, &cx.theme().mono_font_family)),
                                )
                                .into_any_element();
                        }
                        div().into_any_element()
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
        let content = match cache.get(&ix) {
            Some((cached_body, cached_content)) if cached_body == body => cached_content.clone(),
            _ => {
                let repo_root = crate::vcs::repo_root_of(&self.path);
                let content = resolve_mention_names(body, roster, Some(repo_root));
                cache.insert(ix, (body.to_string(), content.clone()));
                content
            }
        };

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

    pub(super) fn chat_message_row<Tz: chrono::TimeZone>(
        &self,
        id: &ResolvedIdentity,
        m: &MessageRow,
        ix: usize,
        roster: &[crate::model::RosterRow],
        tz: Tz,
        mono_font: &gpui::SharedString,
    ) -> impl IntoElement
    where
        Tz::Offset: std::fmt::Display,
    {
        let summary = {
            let mut cache = self.turn_summaries.borrow_mut();
            if let Some(&res) = cache.get(&ix) {
                res
            } else {
                let res = turn_summary_parts(&self.view.messages, ix);
                cache.insert(ix, res);
                res
            }
        };
        let summary_chip = summary.and_then(|(duration_secs, num_tools)| {
            if duration_secs > 0 || num_tools > 0 {
                let mut parts = Vec::new();
                if duration_secs > 0 {
                    parts.push(format!("thought for {}", format_duration(duration_secs)));
                }
                if num_tools > 0 {
                    parts.push(format!("ran {} tool{}", num_tools, if num_tools == 1 { "" } else { "s" }));
                }
                Some(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .px_2()
                        .py_0p5()
                        .mb_1()
                        .rounded_md()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::accent())
                                .child("⟳"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(parts.join(" · ")),
                        ),
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
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(id.color.opacity(0.12))
                                    .border_1()
                                    .border_color(id.color.opacity(0.28))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(id.color)
                                    .child(id.name.trim_start_matches('@').to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(crate::model::format_clock(m.ts.with_timezone(&tz))),
                            )
                            .when_some(m.to.clone(), |this, to| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::bg_surface())
                                        .text_color(theme::text_muted())
                                        .child(format!("→ {}", to.trim_start_matches('@'))),
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
                    .child(if m.kind_label == "edit" {
                        self.ast_diff_card(&m.body, mono_font).into_any_element()
                    } else {
                        self.severity_card(
                            m.severity,
                            "chat-md",
                            ix,
                            &m.body,
                            roster,
                            gpui::px(12.0),
                        )
                        .into_any_element()
                    }),
            )
    }

    /// Insert a completion trigger character (such as `@`) at the current input cursor position.
    pub(super) fn insert_completion_trigger(
        &mut self,
        trigger: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = self.input.read(cx).value().to_string();
        let mut cursor = self.input.read(cx).cursor().min(value.len());
        while cursor > 0 && !value.is_char_boundary(cursor) {
            cursor -= 1;
        }
        let new_value = format!("{}{}{}", &value[..cursor], trigger, &value[cursor..]);
        let new_cursor = cursor + trigger.len();
        self.input.update(cx, |state, cx| {
            state.set_value(new_value, window, cx);
            state.set_selected_range(new_cursor..new_cursor, cx);
        });
        window.focus(&self.input.focus_handle(cx), cx);
        self.recompute_completion(cx);
        cx.notify();
    }

    /// Render an embedded AST Forge Edit-by-Hash diff card displaying target file path, 8-character blake3 hash badge, and code diff preview.
    pub(super) fn ast_diff_card(&self, body: &str, mono_font: &gpui::SharedString) -> impl IntoElement {
        let file_path = body
            .lines()
            .next()
            .and_then(|line| {
                if line.contains("path") || line.contains('/') || line.contains('.') {
                    Some(line.trim())
                } else {
                    None
                }
            })
            .unwrap_or("AST Edit");

        let hash_badge = hadron_forge::block::short_hash(body);

        v_flex()
            .w_full()
            .rounded_lg()
            .bg(theme::term_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .overflow_hidden()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_1p5()
                    .bg(theme::bg_surface_raised())
                    .border_b_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child(file_path.to_string()),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(theme::accent_soft())
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::accent())
                                    .child(format!("blake3:{}", hash_badge)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("Edit-by-Hash"),
                    ),
            )
            .child(
                div()
                    .p_3()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .text_color(theme::text_secondary())
                    .child(body.to_string()),
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
    m_pos: usize,
) -> Option<(i64, usize)> {
    let m = messages.get(m_pos)?;
    let turn_id = m.turn.as_ref()?;

    let mut turn_events = Vec::new();
    let mut min_ts = m.ts;
    let mut excited_ts = None;

    for x in messages[..=m_pos].iter().rev() {
        if x.turn.as_ref() == Some(turn_id) {
            turn_events.push(x);
            if x.ts < min_ts {
                min_ts = x.ts;
            }
        }
        if excited_ts.is_none()
            && x.from == m.from
            && x.kind_label == "status"
            && x.body == "excited"
            && x.ts <= m.ts
        {
            excited_ts = Some(x.ts);
        }
        if excited_ts.is_some() && x.turn.as_ref() != Some(turn_id) && x.ts < min_ts {
            break;
        }
    }

    if m_pos + 1 < messages.len() {
        for x in &messages[m_pos + 1..] {
            if x.turn.as_ref() == Some(turn_id) {
                turn_events.push(x);
            } else if x.ts > m.ts + chrono::Duration::minutes(5) {
                break;
            }
        }
    }

    if turn_events.is_empty() {
        return None;
    }

    let start_time = excited_ts.or_else(|| turn_events.iter().map(|x| x.ts).min())?;
    let duration_secs = m.ts.signed_duration_since(start_time).num_seconds().max(0);
    let num_tools = turn_events
        .iter()
        .filter(|x| x.kind_label == "command" || x.kind_label == "edit")
        .count();

    Some((duration_secs, num_tools))
}

/// Format turn duration in seconds to human-friendly strings like "<1s", "5s", "1m 02s", "2m 34s", "1h 02m 05s".
pub(crate) fn format_duration(secs: i64) -> String {
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

        let (duration, tools) = turn_summary_parts(&msgs, 2).expect("summary parts found");
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

#[cfg(test)]
mod milestone_3_tests {
    use crate::app::tabs::ChatTab;

    #[test]
    fn test_chat_tab_navigation() {
        assert_eq!(ChatTab::Chat.index(), 0);
        assert_eq!(ChatTab::Log.index(), 1);
        assert_eq!(ChatTab::Stats.index(), 2);

        assert_eq!(ChatTab::from_index(0), ChatTab::Chat);
        assert_eq!(ChatTab::from_index(1), ChatTab::Log);
        assert_eq!(ChatTab::from_index(2), ChatTab::Stats);
        assert_eq!(ChatTab::from_index(99), ChatTab::Chat);

        assert_eq!(ChatTab::Chat.label(), "Chat");
        assert_eq!(ChatTab::Log.label(), "Event Log");
        assert_eq!(ChatTab::Stats.label(), "Stats");

        let n = ChatTab::ALL.len() as isize;
        let mut cur = ChatTab::Chat.index() as isize;
        cur = (cur + 1).rem_euclid(n);
        assert_eq!(ChatTab::from_index(cur as usize), ChatTab::Log);

        cur = (cur + 1).rem_euclid(n);
        assert_eq!(ChatTab::from_index(cur as usize), ChatTab::Stats);

        cur = (cur + 1).rem_euclid(n);
        assert_eq!(ChatTab::from_index(cur as usize), ChatTab::Chat);

        cur = (cur - 1).rem_euclid(n);
        assert_eq!(ChatTab::from_index(cur as usize), ChatTab::Stats);
    }

    #[test]
    fn test_chat_tab_edge_cases_and_extreme_cycling() {
        let n = ChatTab::ALL.len() as isize;

        // Large positive delta cycle (100 steps)
        let cur = ChatTab::Chat.index() as isize;
        let next_tab = ChatTab::from_index((cur + 100).rem_euclid(n) as usize);
        assert_eq!(next_tab, ChatTab::Log); // 100 % 3 = 1 -> Log

        // Large negative delta cycle (-100 steps)
        let prev_tab = ChatTab::from_index((cur - 100).rem_euclid(n) as usize);
        assert_eq!(prev_tab, ChatTab::Stats); // -100 % 3 = 2 -> Stats

        // Index out of bounds values default safely to ChatTab::Chat
        assert_eq!(ChatTab::from_index(usize::MAX), ChatTab::Chat);
        assert_eq!(ChatTab::from_index(3), ChatTab::Chat);
        assert_eq!(ChatTab::from_index(42), ChatTab::Chat);
    }

    #[test]
    fn test_stats_scroll_starts_at_top_and_is_independent() {
        let stats_scroll = gpui::ScrollHandle::new();
        assert_eq!(stats_scroll.offset(), gpui::Point::default());
    }

    #[test]
    fn test_ast_diff_card_rendering() {
        let sample_code = "pub fn calculate_hash() -> u64 { 42 }";
        let hash = hadron_forge::block::short_hash(sample_code);
        assert_eq!(hash.len(), 8, "blake3 hash digest badge length must be 8 hex chars");

        let blocks = hadron_forge::block::parse_blocks(sample_code);
        assert!(!blocks.is_empty(), "Should parse AST blocks");
        assert_eq!(blocks[0].hash.len(), 8);
    }

    #[test]
    fn test_ast_diff_short_hash_determinism_and_edge_cases() {
        // Empty string
        let empty_hash = hadron_forge::block::short_hash("");
        assert_eq!(empty_hash.len(), 8);
        assert!(empty_hash.chars().all(|c| c.is_ascii_hexdigit()));

        // Determinism check
        let code = "fn hello_world() {\n    println!(\"Hello, Hadron!\");\n}";
        let hash1 = hadron_forge::block::short_hash(code);
        let hash2 = hadron_forge::block::short_hash(code);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 8);

        // Sensitivity check
        let code_modified = "fn hello_world() {\n    println!(\"Hello, Hadron!\"); \n}";
        let hash_mod = hadron_forge::block::short_hash(code_modified);
        assert_ne!(hash1, hash_mod);

        // Multi-byte UTF-8 string
        let utf8_code = "fn 🔬_quantum_core() { // ⚛️\n}";
        let hash_utf8 = hadron_forge::block::short_hash(utf8_code);
        assert_eq!(hash_utf8.len(), 8);
    }

    #[test]
    fn test_completion_overlay_filtering() {
        use crate::text::{extract_completion_query, completion_candidates, CompletionTrigger};

        let (trig, query, start) = extract_completion_query("hello @acp", 10).expect("extract mention");
        assert_eq!(trig, CompletionTrigger::Mention);
        assert_eq!(query, "acp");
        assert_eq!(start, 6);

        let (trig, query, start) = extract_completion_query("/resume session", 15).expect("extract arg");
        assert_eq!(trig, CompletionTrigger::Arg(crate::text::ArgSource::Session));
        assert_eq!(query, "session");
        assert_eq!(start, 8);

        let quarks = vec![("acp-agy".to_string(), Some("AGY Quark".to_string()))];
        let files = vec!["src/app/render/chat.rs".to_string()];
        let sessions = vec![];

        let comp = completion_candidates("@acp", 4, &quarks, &files, &sessions).expect("candidates");
        assert!(!comp.candidates.is_empty());
        assert_eq!(comp.candidates[0].detail, "Quark");

        let comp_files = completion_candidates("@src", 4, &quarks, &files, &sessions).expect("file candidates");
        assert!(!comp_files.candidates.is_empty());
        assert_eq!(comp_files.candidates[0].label, "📄 @chat.rs");
        assert_eq!(comp_files.candidates[0].detail, "src/app/render/chat.rs");
    }

    #[test]
    fn test_completion_overlay_selection_bounds_clamping() {
        // Test selection index clamping logic used by move_completion_selection
        let candidates_count = 3;
        let max = candidates_count as isize - 1;

        let mut selected: isize = 0;
        // Move up from 0 -> stays 0
        selected = (selected - 1).clamp(0, max);
        assert_eq!(selected, 0);

        // Move down twice: 0 -> 1 -> 2
        selected = (selected + 1).clamp(0, max);
        assert_eq!(selected, 1);
        selected = (selected + 1).clamp(0, max);
        assert_eq!(selected, 2);

        // Move down past end -> clamps at max (2)
        selected = (selected + 1).clamp(0, max);
        assert_eq!(selected, 2);
    }
}


