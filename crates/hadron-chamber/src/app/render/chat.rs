use super::*;
use gpui_component::ActiveTheme;

///Maximum viewport height (in px, ~10 lines of rendered Markdown) for the live
/// `#composer-preview-content` pane stacked above the active composer input.
/// Keeps long drafts scrollable without starving the chat history viewport or
/// pushing the editor input off-screen.
pub(super) const COMPOSER_PREVIEW_MAX_HEIGHT_PX: f32 = 220.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SkillBlockReason {
    ModeMismatch {
        current_mode: Mode,
        required_mode: Mode,
    },
    DeniedSkill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SkillBlockNotice {
    pub quark_id: String,
    pub skill_id: String,
    pub reason: SkillBlockReason,
}

/// Parses a structured Gluon skill-block notice from `body`:
/// - Mode mismatch: `"@<quark> is in <cur> mode and cannot run '<skill>' (requires <req> mode); skipping."`
/// - Denied skill: `"@<quark> does not have skill '<skill>' enabled (deny_skills); skipping."`
///   or `"@<quark> locks out '<skill>' tasks (deny_skills); skipping."`
/// Also strips an optional leading `"@orchestrator "` prefix added by `reroute_blocked`.
pub(super) fn parse_skill_block_notice(body: &str) -> Option<SkillBlockNotice> {
    let trimmed = body.trim();
    let rest = trimmed
        .strip_prefix("@orchestrator ")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let after_at = rest.strip_prefix('@')?;
    let space_idx = after_at.find(char::is_whitespace)?;
    let quark_id = after_at[..space_idx].trim().to_string();
    if quark_id.is_empty() {
        return None;
    }
    let tail = after_at[space_idx..].trim_start();

    if let Some(after_in) = tail.strip_prefix("is in ") {
        let (cur_str, after_cur) = after_in.split_once(" mode and cannot run '")?;
        let (skill_id, after_skill) = after_cur.split_once("' (requires ")?;
        let (req_str, _) = after_skill.split_once(" mode); skipping.")?;
        let current_mode = Mode::from_str_loose(cur_str.trim())?;
        let required_mode = Mode::from_str_loose(req_str.trim())?;
        return Some(SkillBlockNotice {
            quark_id,
            skill_id: skill_id.to_string(),
            reason: SkillBlockReason::ModeMismatch {
                current_mode,
                required_mode,
            },
        });
    }

    if let Some(after_not) = tail.strip_prefix("does not have skill '") {
        let (skill_id, _) = after_not.split_once("' enabled (deny_skills); skipping.")?;
        return Some(SkillBlockNotice {
            quark_id,
            skill_id: skill_id.to_string(),
            reason: SkillBlockReason::DeniedSkill,
        });
    }

    if let Some(after_locks) = tail.strip_prefix("locks out '") {
        let (skill_id, _) = after_locks.split_once("' tasks (deny_skills); skipping.")?;
        return Some(SkillBlockNotice {
            quark_id,
            skill_id: skill_id.to_string(),
            reason: SkillBlockReason::DeniedSkill,
        });
    }

    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OnboardingReadiness {
    pub git_ready: bool,
    pub active_quark_count: usize,
    /// `(id, vendor, model)` of the active `@orchestrator` seat, if one is seated and enabled.
    pub orchestrator_badge: Option<(String, String, String)>,
}

/// Pure evaluation of NUX onboarding readiness from Git status and resolved team seats.
pub(super) fn compute_onboarding_readiness(
    git_ready: bool,
    resolved: &hadron_lattice::Team,
) -> OnboardingReadiness {
    let enabled_seats: Vec<&hadron_lattice::Seat> =
        resolved.quarks.iter().filter(|s| s.enabled).collect();
    let active_quark_count = enabled_seats.len();
    let orch = enabled_seats
        .iter()
        .copied()
        .find(|s| s.flavor == hadron_lattice::Flavor::Orchestrator)
        .or_else(|| enabled_seats.first().copied())
        .map(|s| (s.id.as_str().to_string(), s.vendor.clone(), s.model.clone()));

    OnboardingReadiness {
        git_ready,
        active_quark_count,
        orchestrator_badge: orch,
    }
}

/// Computes `(bytes, usage_fraction_0_to_1)` for each of the 5 prompt anatomy categories:
/// `[Rules/Invariants, Nucleus Memory, Active Skill, Task/Directive, Transcript/Tools]`.
/// Fractions reflect actual byte usage (`bytes / total_bytes`) rather than equal 1/5th slices.
pub(super) fn prompt_anatomy_fractions(
    breakdown: &hadron_lattice::prompt_cost::PromptBreakdown,
) -> [(usize, f32); 5] {
    let rules = breakdown.standard_model + breakdown.invariants;
    let nucleus = breakdown.nucleus_digest + breakdown.nucleus_index;
    let skill = breakdown.active_skill;
    let task = breakdown.task;
    let transcript = breakdown.field_window;
    let total = rules + nucleus + skill + task + transcript;
    let frac = |bytes: usize| -> f32 {
        if total > 0 {
            bytes as f32 / total as f32
        } else {
            0.0
        }
    };
    [
        (rules, frac(rules)),
        (nucleus, frac(nucleus)),
        (skill, frac(skill)),
        (task, frac(task)),
        (transcript, frac(transcript)),
    ]
}

pub(super) fn prompt_anatomy_colors() -> [gpui::Hsla; 5] {
    [
        theme::info().into(),
        theme::success().into(),
        theme::accent().into(),
        theme::warning().into(),
        theme::error().into(),
    ]
}

impl super::Chamber {
    fn render_prompt_anatomy_circle(fractions: [(usize, f32); 5]) -> impl IntoElement {
        let colors = prompt_anatomy_colors();
        gpui::canvas(
            |_, _, _| {},
            move |bounds, _, window, _cx| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let cx_px = f32::from(bounds.origin.x) + w * 0.5;
                let cy_px = f32::from(bounds.origin.y) + h * 0.5;
                let radius = (w.min(h) * 0.5 - 2.0).max(4.0);
                let stroke_w = px(3.0);

                // Subtle full-circle background track ring
                let mut track = gpui::PathBuilder::stroke(stroke_w);
                let track_steps = 36usize;
                for k in 0..=track_steps {
                    let theta = (k as f32 / track_steps as f32) * std::f32::consts::TAU;
                    let pt = gpui::point(
                        px(cx_px + radius * theta.cos()),
                        px(cy_px + radius * theta.sin()),
                    );
                    if k == 0 {
                        track.move_to(pt);
                    } else {
                        track.line_to(pt);
                    }
                }
                if let Ok(path) = track.build() {
                    window.paint_path(path, theme::bg_elevated());
                }

                // Proportional arc segments starting at 12 o'clock (-PI / 2)
                let mut start_angle = -std::f32::consts::FRAC_PI_2;
                for (ix, (bytes, frac)) in fractions.iter().copied().enumerate() {
                    if bytes == 0 || frac <= 0.0 {
                        continue;
                    }
                    let sweep = frac * std::f32::consts::TAU;
                    let steps = ((sweep / std::f32::consts::TAU * 40.0).ceil() as usize).clamp(2, 40);
                    let mut arc = gpui::PathBuilder::stroke(stroke_w);
                    for k in 0..=steps {
                        let theta = start_angle + sweep * (k as f32 / steps as f32);
                        let pt = gpui::point(
                            px(cx_px + radius * theta.cos()),
                            px(cy_px + radius * theta.sin()),
                        );
                        if k == 0 {
                            arc.move_to(pt);
                        } else {
                            arc.line_to(pt);
                        }
                    }
                    if let Ok(path) = arc.build() {
                        window.paint_path(path, colors[ix]);
                    }
                    start_angle += sweep;
                }
            },
        )
        .size(px(18.0))
        .flex_none()
    }

    pub(super) fn render_prompt_anatomy_hud(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (quark_id, breakdown) = self
            .last_prompt_cost
            .clone()
            .map(|(id, b)| (Some(id), b))
            .unwrap_or_else(|| {
                (
                    None,
                    hadron_lattice::prompt_cost::PromptBreakdown {
                        standard_model: 4_000,
                        invariants: 16_000,
                        nucleus_digest: 2_000,
                        nucleus_index: 8_000,
                        active_skill: 3_000,
                        task: 2_000,
                        field_window: 25_000,
                    },
                )
            });

        let fractions = prompt_anatomy_fractions(&breakdown);
        let (rules, rules_frac) = fractions[0];
        let (nucleus, nucleus_frac) = fractions[1];
        let (skill, skill_frac) = fractions[2];
        let (task, task_frac) = fractions[3];
        let (transcript, transcript_frac) = fractions[4];

        let total_bytes = rules + nucleus + skill + task + transcript;
        let est_tokens = total_bytes / 4;
        let total_kb = total_bytes as f32 / 1024.0;

        let rules_pct = rules_frac * 100.0;
        let nucleus_pct = nucleus_frac * 100.0;
        let skill_pct = skill_frac * 100.0;
        let task_pct = task_frac * 100.0;
        let transcript_pct = transcript_frac * 100.0;

        let header_line = match quark_id.as_deref() {
            Some(id) => format!("Prompt Context Anatomy (@{id}):"),
            None => "Prompt Context Anatomy:".to_string(),
        };

        let colors = prompt_anatomy_colors();
        let items = [
            ("Rules/Invariants", rules, rules_pct, colors[0]),
            ("Nucleus Memory", nucleus, nucleus_pct, colors[1]),
            ("Active Skill", skill, skill_pct, colors[2]),
            ("Task/Directive", task, task_pct, colors[3]),
            ("Transcript/Tools", transcript, transcript_pct, colors[4]),
        ];
        let footer_line = format!("Total: {:.1} KB (~{} tokens)", total_kb, est_tokens);

        h_flex()
            .id("prompt-anatomy-hud")
            .flex_none()
            .items_center()
            .gap_2()
            .px_2p5()
            .py_1()
            .rounded_full()
            .bg(theme::tab_bar_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .tooltip(move |window, cx| {
                let header = header_line.clone();
                let footer = footer_line.clone();
                let items_clone = items;
                gpui_component::tooltip::Tooltip::element(move |_window, _cx| {
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::text())
                                .child(header.clone()),
                        )
                        .children(items_clone.iter().map(|(label, bytes, pct, color)| {
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .size(px(7.0))
                                        .rounded_full()
                                        .bg(*color)
                                        .flex_none(),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_secondary())
                                        .child(format!("{label}: {:.1} KB ({:.0}%)", *bytes as f32 / 1024.0, pct)),
                                )
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(footer.clone()),
                        )
                })
                .build(window, cx)
            })
            .child(Self::render_prompt_anatomy_circle(fractions))
            .child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(theme::text_muted())
                    .whitespace_nowrap()
                    .child(format!("{:.1} KB (~{} tok)", total_kb, est_tokens)),
            )
    }

    /// The center column: a segmented Chat / Log / Timeline tab bar over the
    /// selected view, with the human's message box pinned at the foot. The whole
    /// thing is a rounded, filled card that floats on the unified canvas.
    pub(super) fn chat_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One row per quark mid-turn, not just the first — the card and the
        // roster's blue dot must agree on who counts as "working".
        // Read from `self.last_live_activities` / `self.last_gate_activities` (polled off the render pass)
        // so hover/repaint frames never perform synchronous 9P file I/O.
        let now = chrono::Utc::now();
        let mut active = widgets::active_quarks_rich_with_tasks(
            &self.view.roster,
            &self.view.tasks,
            |id| self.last_live_activities.get(id).and_then(|a| a.clone()),
            now,
        );

        // Surface in-flight merge gates and Gluon system operations
        for gate_act in &self.last_gate_activities {
            let detail = if gate_act.detail.is_empty() {
                format!("merging `{}`", gate_act.quark.as_str())
            } else {
                widgets::humanize_activity_detail(&gate_act.detail)
            };
            let elapsed_secs = gate_act
                .started
                .or(Some(gate_act.at))
                .map(|st| (now - st).num_seconds().max(0) as u64);
            active.push(widgets::QuarkLiveCapsule {
                quark_id: "gluon".to_string(),
                doing: hadron_lattice::live::Doing::Gating,
                detail,
                elapsed_secs,
            });
        }

        if let Some(Some(gluon_act)) = self.last_live_activities.get("gluon").cloned() {
            if self.last_gate_activities.is_empty() {
                let detail = if gluon_act.detail.is_empty() {
                    gluon_act.doing.label().to_string()
                } else {
                    widgets::humanize_activity_detail(&gluon_act.detail)
                };
                let elapsed_secs = gluon_act
                    .started
                    .or(Some(gluon_act.at))
                    .map(|st| (now - st).num_seconds().max(0) as u64);
                active.push(widgets::QuarkLiveCapsule {
                    quark_id: "gluon".to_string(),
                    doing: gluon_act.doing,
                    detail,
                    elapsed_secs,
                });
            }
        }

        // The reply as it arrives. A sibling of the Live card rather than a row in the
        // message list: `chat_list_state`'s count is derived from `chat_message_ixs`
        // (see the `A Field Swap Resets Every List Cache` invariant), and a draft is
        // not a field event — inventing a list row for one would put a third,
        // untracked writer on caches that only `resync_lists_to_projection` may rebuild.
        // Pinned above the input, so it is where the human is already looking.
        let drafts = streaming_drafts(
            &self.view.roster,
            |id| self.last_live_activities.get(id).and_then(|a| a.clone()),
        );
        let draft_card = (!drafts.is_empty()).then(|| {
            v_flex()
                .id("draft-cards-list")
                .w_full()
                .gap_2()
                .mb_2()
                .max_h(px(220.0))
                .overflow_y_scroll()
                .children(drafts.into_iter().map(|(quark_id_str, text)| {
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
                                        .size(px(6.0))
                                        .rounded_full()
                                        .bg(identity.color),
                                )
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
                        // Capped to ~5 lines (100px) and scrollable so streaming replies
                        // do not blow up the input area and occlude the chat history.
                        .child(
                            div()
                                .id(SharedString::from(format!("draft-text-{}", quark_id_str)))
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .max_h(px(100.0))
                                .overflow_y_scroll()
                                .child(text),
                        )
                }))
        });

        let live_card = (!active.is_empty()).then(|| {
            let active_items: Vec<_> = active
                .into_iter()
                .map(|cap| {
                    let identity = self.resolve_identity(&cap.quark_id);
                    (cap, identity)
                })
                .collect();

            let max_name_len = active_items
                .iter()
                .map(|(_, identity)| identity.name.chars().count())
                .max()
                .unwrap_or(0);

            // Base width: 6px dot + 6px gap + ~8px per bold char + 4px breathing room.
            // Clamped between 38px (fits 3-letter names like Agy compactly) and 130px.
            let name_col_w = (max_name_len as f32 * 8.0 + 16.0).clamp(38.0, 130.0);

            v_flex()
                .id("live-cards-list")
                .w_full()
                .overflow_hidden()
                .gap_1()
                .px_2p5()
                .py_2()
                .mb_2()
                .max_h(px(120.0))
                .overflow_y_scroll()
                .rounded_lg()
                .bg(theme::term_bg())
                .border_1()
                .border_color(theme::glass_highlight())
                .children(active_items.into_iter().map(|(cap, identity)| {
                    let name = identity.name;
                    let (state_color, action_icon, action_label) = match cap.doing {
                        hadron_lattice::live::Doing::Thinking => {
                            (theme::halo_reasoning(), IconName::Bot, "thinking")
                        }
                        hadron_lattice::live::Doing::Working => {
                            let icon = if cap.quark_id == "gluon" {
                                IconName::Cpu
                            } else {
                                IconName::SquareTerminal
                            };
                            (theme::halo_active(), icon, "exec")
                        }
                        hadron_lattice::live::Doing::Planning => {
                            (theme::halo_active(), IconName::Folder, "planning")
                        }
                        hadron_lattice::live::Doing::Speaking => {
                            (theme::halo_reasoning(), IconName::Bot, "speaking")
                        }
                        hadron_lattice::live::Doing::Gating => {
                            (theme::halo_active(), IconName::CircleCheck, "gating")
                        }
                    };

                    let qid_click = cap.quark_id.clone();
                    let is_clickable = qid_click != "gluon";
                    let task_context = self
                        .view
                        .tasks
                        .iter()
                        .find(|t| t.to == cap.quark_id && t.state == crate::model::tasks::TaskState::Working)
                        .map(|t| widgets::humanize_activity_detail(&t.title))
                        .filter(|title| !title.is_empty() && title != &cap.detail);

                    h_flex()
                        .id(SharedString::from(format!("live-capsule-{}", cap.quark_id)))
                        .w_full()
                        .items_center()
                        .gap_2()
                        .px_1p5()
                        .py_1()
                        .rounded_md()
                        .when(is_clickable, |el| {
                            el.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.open_quark_info(qid_click.clone(), InfoTab::Home, cx);
                                }))
                        })
                        .hover(|s| s.bg(theme::glass_highlight()))
                        .child(
                            h_flex()
                                .w(px(name_col_w))
                                .flex_none()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .w_1p5()
                                        .h_1p5()
                                        .flex_none()
                                        .rounded_full()
                                        .bg(state_color),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(identity.color)
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(name),
                                ),
                        )
                        .child(
                            h_flex()
                                .w(px(78.0))
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(theme::glass_card())
                                .border_1()
                                .border_color(state_color.opacity(0.35))
                                .child(
                                    Icon::new(action_icon)
                                        .xsmall()
                                        .text_color(state_color),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(state_color)
                                        .child(action_label),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .whitespace_nowrap()
                                .truncate()
                                .child(cap.detail),
                        )
                        .when_some(task_context, |el, t_title| {
                            el.child(
                                div()
                                    .max_w(px(180.0))
                                    .flex_none()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(theme::bg_surface())
                                    .border_1()
                                    .border_color(theme::hairline_border())
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(t_title),
                            )
                        })
                        .when_some(cap.elapsed_secs, |el, secs| {
                            el.child(
                                h_flex()
                                    .min_w(px(48.0))
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(theme::bg_surface())
                                    .border_1()
                                    .border_color(theme::hairline_border())
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .text_color(theme::text_muted())
                                            .whitespace_nowrap()
                                            .child(format_duration(secs as i64)),
                                    ),
                            )
                        })
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
                    .border_1()
                    .when(is_selected, |s| {
                        s.bg(theme::bg_elevated())
                            .text_color(theme::accent())
                            .font_weight(gpui::FontWeight::BOLD)
                            .border_color(theme::hairline_border())
                    })
                    .when(!is_selected, |s| {
                        s.border_color(gpui::transparent_black())
                            .text_color(theme::text_muted())
                            .hover(|h| {
                                h.text_color(theme::text())
                                    .bg(theme::bg_surface_raised().opacity(0.5))
                            })
                    })
                    .text_xs()
                    .child(label)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.chat_tab = ChatTab::from_index(ix);
                        cx.notify();
                    }))
            }));

        let prompt_anatomy_hud = self.render_prompt_anatomy_hud(cx);
        let header = h_flex()
            .flex_none()
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_2()
            .child(tabs)
            .child(prompt_anatomy_hud);

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
                    .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(1, cx);
                            cx.stop_propagation();
                        } else if this.history_index.is_some() {
                            let state = this.input.read(cx);
                            let text = state.value().to_string();
                            let cursor = state.cursor().min(text.len());
                            let on_last_line = !text[cursor..].contains('\n');
                            if on_last_line && this.navigate_prompt_history(1, window, cx) {
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                        if this.completion.is_some() {
                            this.move_completion_selection(-1, cx);
                            cx.stop_propagation();
                        } else {
                            let state = this.input.read(cx);
                            let text = state.value().to_string();
                            let cursor = state.cursor().min(text.len());
                            let on_line_0 = !text[..cursor].contains('\n');
                            if on_line_0 && this.navigate_prompt_history(-1, window, cx) {
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &Escape, _window, cx| {
                        if this.completion.take().is_some() {
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                        if this.completion.is_some() {
                            this.accept_completion(window, cx);
                            cx.stop_propagation();
                        } else if this.accept_ghost_completion(window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
                        if this.completion.is_none() {
                            let (cursor, len) = {
                                let state = this.input.read(cx);
                                (state.cursor(), state.value().len())
                            };
                            if cursor >= len && this.accept_ghost_completion(window, cx) {
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .on_action(cx.listener(|this, _: &Paste, window, cx| {
                        this.on_input_paste(window, cx);
                    }))
                    .when(self.completion.is_some(), |el| {
                        el.child(self.completion_card_overlay(cx))
                    })
                    .children(self.overdue_schedule_banners(cx))
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
                            .when(self.composer_preview_mode, |el| {
                                let draft_text = self.input.read(cx).value().to_string();
                                if draft_text.trim().is_empty() {
                                    el.child(
                                        div()
                                            .id("composer-preview-empty")
                                            .w_full()
                                            .px_2()
                                            .py_1p5()
                                            .mb_1p5()
                                            .border_b_1()
                                            .border_color(theme::glass_highlight())
                                            .text_xs()
                                            .italic()
                                            .text_color(theme::text_muted())
                                            .child("Live preview — type below to see rendered Markdown."),
                                    )
                                } else {
                                    el.child(
                                        div()
                                            .id("composer-preview-container")
                                            .w_full()
                                            .relative()
                                            .mb_1p5()
                                            .border_b_1()
                                            .border_color(theme::glass_highlight())
                                            .child(
                                                div()
                                                    .id("composer-preview-content")
                                                    .w_full()
                                                    .max_h(px(COMPOSER_PREVIEW_MAX_HEIGHT_PX))
                                                    .overflow_y_scroll()
                                                    .track_scroll(&self.composer_preview_scroll)
                                                    .px_2()
                                                    .pb_2()
                                                    .child(self.markdown_body(
                                                        "composer-preview",
                                                        usize::MAX,
                                                        &draft_text,
                                                        &self.view.roster,
                                                    )),
                                            )
                                            .child(
                                                div().absolute().top_0().bottom_0().right_0().child(
                                                    Scrollbar::vertical(&self.composer_preview_scroll),
                                                ),
                                            ),
                                    )
                                }
                            })
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false),
                            )
                            .when_some(
                                self.ghost_completion
                                    .clone()
                                    .filter(|_| self.completion.is_none()),
                                |el, ghost| {
                                    el.child(
                                        h_flex()
                                            .id("ghost-completion-pill")
                                            .items_center()
                                            .gap_1()
                                            .px_2()
                                            .py_0p5()
                                            .mt_1()
                                            .rounded_md()
                                            .bg(theme::glass_card())
                                            .border_1()
                                            .border_color(theme::glass_highlight())
                                            .text_xs()
                                            .text_color(theme::text_muted())
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.accept_ghost_completion(window, cx);
                                            }))
                                            .child(
                                                div()
                                                    .px_1()
                                                    .rounded_xs()
                                                    .bg(theme::glass_highlight())
                                                    .text_color(theme::text_muted())
                                                    .child("Tab ↹"),
                                            )
                                            .child(
                                                div()
                                                    .text_color(theme::text_muted())
                                                    .truncate()
                                                    .child(format!("…{ghost}")),
                                            ),
                                    )
                                },
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
                                                div()
                                                    .id("global-sandbox")
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.cycle_sandbox_mode(cx)
                                                    }))
                                                    .tooltip(|window, cx| {
                                                        Tooltip::new(
                                                            "Sandbox confinement — F7 or click to cycle",
                                                        )
                                                        .build(window, cx)
                                                    })
                                                    .child(sandbox_tag(self.prefs.sandbox_mode)),
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
                                             )
                                             .child(
                                                 h_flex()
                                                     .id("composer-preview-toggle")
                                                     .items_center()
                                                     .gap_1()
                                                     .px_2()
                                                     .py_0p5()
                                                     .rounded_md()
                                                     .bg(if self.composer_preview_mode {
                                                         theme::glass_highlight()
                                                     } else {
                                                         theme::glass_card()
                                                     })
                                                     .border_1()
                                                     .border_color(if self.composer_preview_mode {
                                                         theme::accent().into()
                                                     } else {
                                                         theme::glass_highlight()
                                                     })
                                                     .text_xs()
                                                     .text_color(if self.composer_preview_mode {
                                                         theme::accent().into()
                                                     } else {
                                                         theme::text_muted()
                                                     })
                                                     .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                                     .cursor_pointer()
                                                     .child(Icon::new(if self.composer_preview_mode { IconName::EyeOff } else { IconName::Eye }).xsmall())
                                                     .child("Preview")
                                                     .on_click(cx.listener(|this, _, window, cx| {
                                                         this.composer_preview_mode = !this.composer_preview_mode;
                                                         if this.composer_preview_mode {
                                                             cx.on_next_frame(window, move |this, _window, cx: &mut Context<Self>| {
                                                                 let max_offset = this.composer_preview_scroll.max_offset();
                                                                 this.composer_preview_scroll
                                                                     .set_offset(gpui::point(px(0.0), -max_offset.y.abs()));
                                                                 cx.notify();
                                                             });
                                                         }
                                                         window.focus(&this.input.focus_handle(cx), cx);
                                                         cx.notify();
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

    /// Renders actionable banner pills for overdue scheduled tasks requiring confirmation (`MissedPolicy::Ask`).
    pub(super) fn overdue_schedule_banners(&self, cx: &mut Context<Self>) -> Vec<impl IntoElement> {
        if self.overdue_schedule_jobs.is_empty() {
            return Vec::new();
        }
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let mut banners = Vec::with_capacity(self.overdue_schedule_jobs.len());

        for (job_id, job_name, job_task, due_formatted) in &self.overdue_schedule_jobs {
            let job_id = job_id.clone();
            let job_name = job_name.clone();
            let job_task = job_task.clone();
            let due_formatted = due_formatted.clone();

            let run_id = job_id.clone();
            let run_root = repo_root.clone();
            let skip_id = job_id.clone();
            let skip_root = repo_root.clone();

            let banner = h_flex()
                .id(format!("overdue-schedule-banner-{}", job_id))
                .w_full()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3()
                .py_2()
                .mb_2()
                .rounded_xl()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::warning().opacity(0.35))
                .shadow_sm()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::Info)
                                .xsmall()
                                .text_color(theme::warning()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text())
                                .child(format!(
                                    "Scheduled task '{}' (`{}`) is overdue (due {} UTC)",
                                    job_name, job_task, due_formatted
                                )),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .flex_shrink_0()
                        .child(
                            h_flex()
                                .id(format!("run-scheduled-btn-{}", job_id))
                                .items_center()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme::accent())
                                .text_xs()
                                .text_color(theme::text())
                                .hover(|s| s.opacity(0.85))
                                .cursor_pointer()
                                .child("Run Now")
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.run_scheduled_job(&run_id, &run_root, cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id(format!("skip-scheduled-btn-{}", job_id))
                                .items_center()
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
                                .child("Skip")
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.skip_scheduled_job(&skip_id, &skip_root, cx);
                                })),
                        ),
                );

            banners.push(banner);
        }

        banners
    }

    /// Render the interactive NUX Onboarding & Readiness Card when the chat history is empty.
    pub(super) fn render_onboarding_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let git_ready =
            self.git_ready || self.git_branches.as_ref().is_some_and(|b| !b.is_empty());
        let resolved = resolve_team(&self.team, &self.global);
        let readiness = compute_onboarding_readiness(git_ready, &resolved);

        let git_row = if readiness.git_ready {
            v_flex()
                .id("nux-git-ready-row")
                .w_full()
                .p_3()
                .rounded_lg()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::success().opacity(0.35))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2p5()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .small()
                                .text_color(theme::success()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::text())
                                        .child("1. Git Worktree Physics Ready"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(
                                            "Repository initialized with base commit — Quarks operate in isolated `.hadron/trees/`.",
                                        ),
                                ),
                        ),
                )
        } else {
            v_flex()
                .id("nux-git-missing-row")
                .w_full()
                .gap_2p5()
                .p_3()
                .rounded_lg()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::warning().opacity(0.45))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2p5()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::Info)
                                .small()
                                .text_color(theme::warning()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::text())
                                        .child("1. Git Repository Required for Quark Worktrees"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(
                                            "Initialize Git here (creates `.gitignore` + root commit on `main`) or switch folders.",
                                        ),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .pl_6()
                        .child(
                            h_flex()
                                .id("nux-init-git-btn")
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(theme::accent())
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .hover(|s| s.opacity(0.88))
                                .cursor_pointer()
                                .child("Initialize Git Repository")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.init_git_from_onboarding(cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id("nux-select-folder-btn")
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(theme::glass_card())
                                .border_1()
                                .border_color(theme::glass_highlight())
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                .cursor_pointer()
                                .child("Select Folder…")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.open_workspace(cx);
                                })),
                        ),
                )
        };

        let swarm_row = if let Some((orch_id, vendor, model)) = readiness.orchestrator_badge {
            let orch_id_click = orch_id.clone();
            v_flex()
                .id("nux-swarm-ready-row")
                .w_full()
                .gap_2p5()
                .p_3()
                .rounded_lg()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::success().opacity(0.35))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2p5()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .small()
                                .text_color(theme::success()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::text())
                                        .child(format!(
                                            "2. Swarm Seated ({} Active Quark{})",
                                            readiness.active_quark_count,
                                            if readiness.active_quark_count == 1 { "" } else { "s" }
                                        )),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(
                                            "Primary @orchestrator routes tasks across worker Quarks automatically.",
                                        ),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .pl_6()
                        .child(
                            h_flex()
                                .id("nux-orchestrator-model-badge")
                                .items_center()
                                .gap_1p5()
                                .px_2p5()
                                .py_1()
                                .rounded_full()
                                .bg(theme::glass_card())
                                .border_1()
                                .border_color(theme::accent().opacity(0.5))
                                .text_xs()
                                .text_color(theme::accent())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::glass_highlight()))
                                .child(Icon::new(IconName::Bot).xsmall())
                                .child(format!("@{orch_id} · {vendor} · {model}"))
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.open_quark_info(orch_id_click.clone(), InfoTab::Home, cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id("nux-add-more-quarks-btn")
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(theme::glass_card())
                                .border_1()
                                .border_color(theme::glass_highlight())
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                .cursor_pointer()
                                .child("Add Quark…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_provider_wizard(window, cx);
                                })),
                        ),
                )
        } else {
            v_flex()
                .id("nux-swarm-missing-row")
                .w_full()
                .gap_2p5()
                .p_3()
                .rounded_lg()
                .bg(theme::bg_elevated())
                .border_1()
                .border_color(theme::warning().opacity(0.45))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2p5()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::Bot)
                                .small()
                                .text_color(theme::warning()),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::text())
                                        .child("2. Onboard Your First Quark (@orchestrator)"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(
                                            "Auto-detect installed CLI agents & API keys (first Quark becomes @orchestrator) or configure manually.",
                                        ),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .pl_6()
                        .child(
                            h_flex()
                                .id("nux-auto-detect-quarks-btn")
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(theme::accent())
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .hover(|s| s.opacity(0.88))
                                .cursor_pointer()
                                .child("Auto-Detect Agents & Keys")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.auto_detect_and_seed_quarks(window, cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id("nux-configure-providers-btn")
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(theme::glass_card())
                                .border_1()
                                .border_color(theme::glass_highlight())
                                .text_xs()
                                .text_color(theme::text_secondary())
                                .hover(|s| s.text_color(theme::text()).bg(theme::glass_highlight()))
                                .cursor_pointer()
                                .child("Configure Providers…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_provider_wizard(window, cx);
                                })),
                        ),
                )
        };

        let starter_prompts = [
            (
                "nux-starter-map",
                "Map Architecture",
                "@orchestrator Map this repository and summarize its architecture.",
            ),
            (
                "nux-starter-debate",
                "Start Debate",
                "/debate Evaluate architectural options and trade-offs for ",
            ),
            (
                "nux-starter-brainstorm",
                "Brainstorm Feature",
                "/brainstorm Let's design and plan ",
            ),
        ];

        v_flex()
            .id("nux-onboarding-card")
            .size_full()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(680.0))
                    .gap_4()
                    .p_5()
                    .rounded_xl()
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child("Welcome to Hadron"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(
                                        "Zero-config swarm setup for beginners and power users (`hadron init`). Complete the checklist below or type a message to begin.",
                                    ),
                            ),
                    )
                    .child(git_row)
                    .child(swarm_row)
                    .child(
                        v_flex()
                            .gap_2()
                            .pt_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::text_secondary())
                                    .child("3. Quick-Start Missions"),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .children(starter_prompts.into_iter().map(
                                        |(btn_id, label, prompt_text)| {
                                            h_flex()
                                                .id(btn_id)
                                                .items_center()
                                                .gap_1p5()
                                                .px_2p5()
                                                .py_1()
                                                .rounded_md()
                                                .bg(theme::bg_elevated())
                                                .border_1()
                                                .border_color(theme::glass_highlight())
                                                .text_xs()
                                                .text_color(theme::text_secondary())
                                                .hover(|s| {
                                                    s.text_color(theme::text())
                                                        .bg(theme::glass_highlight())
                                                })
                                                .cursor_pointer()
                                                .child(label)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.fill_starter_prompt(
                                                            prompt_text, window, cx,
                                                        );
                                                    },
                                                ))
                                        },
                                    )),
                            ),
                    ),
            )
    }

    /// The Chat tab: the conversation only (message events), styled like a chat
    /// with each author's avatar and name.
    pub(super) fn chat_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.chat_message_ixs.is_empty() {
            return self.render_onboarding_card(cx).into_any_element();
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
                                
                                let mut row = div().w_full().pb(px(16.0));
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
                                        view.clone(),
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
                            let from_id = this.resolve_identity(&m.from);
                            let to_id = m.to.as_deref().map(|t| this.resolve_identity(t));
                            let human_name = this.resolve_identity("human").name;
                            let repo_root = crate::vcs::repo_root_of(&this.path);
                            let resolved_body = resolve_mention_names_with_human(
                                &m.body,
                                &this.view.roster,
                                Some(repo_root),
                                Some(&human_name),
                            );
                            let entity = view.clone();
                            let msg_body = m.body.clone();
                            return row
                                .child(
                                    div()
                                        .id(SharedString::from(format!("log-row-{ix}")))
                                        .cursor_pointer()
                                        .on_click({
                                            let ent = entity.clone();
                                            move |_, window, cx| {
                                                use gpui_component::WindowExt as _;
                                                // If the user has highlighted text (e.g. mouse drag selection), don't toggle row expansion
                                                if !window.selected_text(cx).trim().is_empty() {
                                                    return;
                                                }
                                                ent.update(cx, |this, cx| {
                                                    if !this.log_expanded.remove(&ix) {
                                                        this.log_expanded.insert(ix);
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        })
                                        .context_menu({
                                            let text = msg_body.clone();
                                            let ent = view.clone();
                                            let event_line = m.format_event_line(&local_offset);
                                            let event_json = m.to_json();
                                            move |mut menu, window, cx| {
                                                use gpui_component::WindowExt as _;
                                                let sel = window.selected_text(cx);
                                                if !sel.trim().is_empty() {
                                                    let ent_c = ent.clone();
                                                    let sel_text = sel.clone();
                                                    menu = menu.item(PopupMenuItem::new("Copy Selected Text").on_click(
                                                        move |_, window, cx| {
                                                            let to_copy = {
                                                                let cur = window.selected_text(cx);
                                                                if !cur.is_empty() {
                                                                    cur
                                                                } else {
                                                                    sel_text.clone()
                                                                }
                                                            };
                                                            ent_c.update(cx, |this, cx| {
                                                                this.handle_context_menu_action(
                                                                    ContextMenuAction::CopyText(to_copy),
                                                                    cx,
                                                                );
                                                            });
                                                            window.refresh();
                                                        },
                                                    ));
                                                }
                                                let ent_line = ent.clone();
                                                let line_copy = event_line.clone();
                                                menu = menu.item(PopupMenuItem::new("Copy Event Line").on_click(
                                                    move |_, window, cx| {
                                                        let to_copy = line_copy.clone();
                                                        ent_line.update(cx, |this, cx| {
                                                            this.handle_context_menu_action(
                                                                ContextMenuAction::CopyText(to_copy),
                                                                cx,
                                                            );
                                                        });
                                                        window.refresh();
                                                    },
                                                ));
                                                let ent_m = ent.clone();
                                                let log_text = text.clone();
                                                menu = menu.item(PopupMenuItem::new("Copy Log Message").on_click(
                                                    move |_, window, cx| {
                                                        let to_copy = log_text.clone();
                                                        ent_m.update(cx, |this, cx| {
                                                            this.handle_context_menu_action(
                                                                ContextMenuAction::CopyText(to_copy),
                                                                cx,
                                                            );
                                                        });
                                                        window.refresh();
                                                    },
                                                ));
                                                let ent_j = ent.clone();
                                                let json_copy = event_json.clone();
                                                menu.item(PopupMenuItem::new("Copy Event as JSON").on_click(
                                                    move |_, window, cx| {
                                                        let to_copy = json_copy.clone();
                                                        ent_j.update(cx, |this, cx| {
                                                            this.handle_context_menu_action(
                                                                ContextMenuAction::CopyText(to_copy),
                                                                cx,
                                                            );
                                                        });
                                                        window.refresh();
                                                    },
                                                ))
                                            }
                                        })
                                        .child(log_row(ix, m, expanded, &from_id, to_id.as_ref(), &resolved_body, local_offset, &cx.theme().mono_font_family)),
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
                let human_name = self.resolve_identity("human").name;
                let resolved = resolve_mention_names_with_human(body, roster, Some(repo_root), Some(&human_name));
                let content = crate::syntax::annotate_bare_code_fences(&resolved);
                cache.insert(ix, (body.to_string(), content.clone()));
                content
            }
        };

        let ui_size = self.prefs.ui_font_size.unwrap_or(14.0);
        div()
            .id(SharedString::from(format!("md-body-{view}-{ix}")))
            .w_full()
            .min_w_0()
            .text_size(px(ui_size))
            .line_height(gpui::relative(1.55))
            .child(
                gpui_component::text::TextView::markdown((view, ix), content)
                    .selectable(true)
                    .style(markdown_style(self.prefs.ui_font_size, self.prefs.code_block_word_wrap))
                    .markdown_extensions(crate::mermaid::plugin::chamber_markdown_extensions())
                    .on_link_click(crate::app::handle_chat_link_click)
                    .code_block_actions(|code_block, _window, _cx| {
                        let code = code_block.code();
                        gpui_component::clipboard::Clipboard::new("code-copy")
                            .value(code)
                            .tooltip("Copy")
                            .on_copied(|val, _window, _cx| {
                                crate::sys::copy_to_system_clipboard(&val);
                            })
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
        let Some(accent) = severity_accent(severity) else {
            return self.markdown_body(view, ix, body, roster).into_any_element();
        };
        div()
            .w_full()
            .p(pad)
            .rounded_lg()
            .bg(accent.opacity(0.10))
            .border_l(gpui::px(3.0))
            .border_color(accent.opacity(0.75))
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
        chamber_entity: Entity<Self>,
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
        let (turn_commits, turn_files) = if m.from != "human" && m.from != "gluon" {
            extract_turn_artifacts(&m.body)
        } else {
            (Vec::new(), Vec::new())
        };
        let has_summary_or_artifacts = summary.is_some_and(|(d, t)| d > 0 || t > 0)
            || !turn_commits.is_empty()
            || !turn_files.is_empty();
        let summary_chip = has_summary_or_artifacts.then(|| {
            let mut row = h_flex().gap_1p5().items_center().flex_wrap().mb_1();
            if let Some((duration_secs, num_tools)) = summary {
                if duration_secs > 0 || num_tools > 0 {
                    let mut parts = Vec::new();
                    if duration_secs > 0 {
                        parts.push(format!("thought for {}", format_duration(duration_secs)));
                    }
                    if num_tools > 0 {
                        parts.push(format!(
                            "ran {} tool{}",
                            num_tools,
                            if num_tools == 1 { "" } else { "s" }
                        ));
                    }
                    row = row.child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .px_2()
                            .py_0p5()
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
                    );
                }
            }
            let primary_commit = turn_commits.first().cloned();
            for sha in turn_commits {
                let ent_git = chamber_entity.clone();
                let mono = mono_font.clone();
                let sha_click = sha.clone();
                row = row.child(
                    h_flex()
                        .id(SharedString::from(format!("turn-commit-{ix}-{sha}")))
                        .gap_1()
                        .items_center()
                        .px_1p5()
                        .py_0p5()
                        .rounded_md()
                        .bg(theme::success().opacity(0.12))
                        .border_1()
                        .border_color(theme::success().opacity(0.35))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::success().opacity(0.22)))
                        .on_click(move |_, _window, cx| {
                            let sha_c = sha_click.clone();
                            ent_git.update(cx, |this, cx| {
                                this.open_turn_diff(Some(sha_c), None, cx);
                            });
                        })
                        .child(
                            div()
                                .text_xs()
                                .font_family(mono)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme::success())
                                .child(format!("⊕ {sha}")),
                        ),
                );
            }
            for file_name in turn_files {
                let ent_diff = chamber_entity.clone();
                let mono = mono_font.clone();
                let file_click = file_name.clone();
                let commit_hint = primary_commit.clone();
                row = row.child(
                    h_flex()
                        .id(SharedString::from(format!("turn-file-{ix}-{file_name}")))
                        .gap_1()
                        .items_center()
                        .px_1p5()
                        .py_0p5()
                        .rounded_md()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::bg_surface_raised()))
                        .on_click(move |_, _window, cx| {
                            let f_c = file_click.clone();
                            let c_h = commit_hint.clone();
                            ent_diff.update(cx, |this, cx| {
                                this.open_turn_diff(c_h, Some(f_c), cx);
                            });
                        })
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::info())
                                .child("±"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family(mono)
                                .text_color(theme::text_secondary())
                                .child(file_name),
                        ),
                );
            }
            row
        });

        let msg_text = m.body.clone();
        let entity = chamber_entity.clone();
        let (row_gap, row_pad_y, avatar_size, bubble_pad) = match self.prefs.chat_density {
            crate::config::ChatDensity::Comfortable => (px(10.0), px(4.0), 28.0, px(12.0)),
            crate::config::ChatDensity::Compact => (px(5.0), px(1.0), 20.0, px(6.0)),
        };
        h_flex()
            .id(SharedString::from(format!("chat-msg-row-{ix}")))
            .w_full()
            .items_start()
            .gap(row_gap)
            .py(row_pad_y)
            .context_menu(move |mut menu, window, cx| {
                let text = msg_text.clone();
                let ent = entity.clone();
                use gpui_component::WindowExt as _;
                let sel = window.selected_text(cx);
                if !sel.trim().is_empty() {
                    let ent_c = ent.clone();
                    let sel_text = sel.clone();
                    menu = menu.item(PopupMenuItem::new("Copy Selected Text").on_click(
                        move |_, window, cx| {
                            let to_copy = {
                                let cur = window.selected_text(cx);
                                if !cur.is_empty() {
                                    cur
                                } else {
                                    sel_text.clone()
                                }
                            };
                            ent_c.update(cx, |this, cx| {
                                this.handle_context_menu_action(
                                    ContextMenuAction::CopyText(to_copy),
                                    cx,
                                );
                            });
                            window.refresh();
                        },
                    ));
                }
                let ent_m = ent.clone();
                let msg_body = text.clone();
                menu = menu.item(PopupMenuItem::new("Copy Message").on_click(
                    move |_, window, cx| {
                        let to_copy = msg_body.clone();
                        ent_m.update(cx, |this, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::CopyText(to_copy),
                                cx,
                            );
                        });
                        window.refresh();
                    },
                ));
                menu.item(PopupMenuItem::new("Promote to Nucleus Lesson").on_click(
                    move |_, window, cx| {
                        let first_line = text.lines().next().unwrap_or("lesson").trim();
                        let slug = first_line
                            .chars()
                            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
                            .collect::<String>()
                            .split_whitespace()
                            .take(4)
                            .collect::<Vec<_>>()
                            .join("-")
                            .to_ascii_lowercase();
                        let desc = if first_line.len() > 80 {
                            format!("{}…", &first_line[..80])
                        } else {
                            first_line.to_string()
                        };
                        ent.update(cx, |this, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::PromoteToNucleus {
                                    slug: if slug.is_empty() {
                                        format!("lesson-{}", chrono::Utc::now().timestamp())
                                    } else {
                                        slug
                                    },
                                    fact: text.clone(),
                                    description: if desc.is_empty() {
                                        "Lesson learned".to_string()
                                    } else {
                                        desc
                                    },
                                    },
                                cx,
                            );
                        });
                        window.refresh();
                    },
                ))
            })
            .child(identity_avatar(id, avatar_size))
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .w_full()
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
                                            .child(crate::model::format_timestamp(m.ts.with_timezone(&tz), self.prefs.timestamp_format)),
                                    )
                                    .when_some(m.to.clone(), |this, to| {
                                        let to_name = self.resolve_identity(to.trim_start_matches('@')).name;
                                        this.child(
                                            div()
                                                .text_xs()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(theme::bg_surface())
                                                .text_color(theme::text_muted())
                                                .child(format!("→ {}", to_name.trim_start_matches('@'))),
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
                            .child({
                                let copy_body = m.body.clone();
                                let ent_btn = chamber_entity.clone();
                                div()
                                    .id(SharedString::from(format!("copy-msg-btn-{ix}")))
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .hover(|s| s.bg(theme::bg_surface_raised()))
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .tooltip(|window, cx| Tooltip::new("Copy message text").build(window, cx))
                                    .on_click(move |_, window, cx| {
                                        let text = copy_body.clone();
                                        ent_btn.update(cx, |this, cx| {
                                            this.handle_context_menu_action(
                                                ContextMenuAction::CopyText(text),
                                                cx,
                                            );
                                        });
                                        window.refresh();
                                    })
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(Icon::new(IconName::Copy).xsmall())
                                            .child("Copy"),
                                    )
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
                            bubble_pad,
                        )
                        .into_any_element()
                    })
                    .when_some(
                        (m.from == "gluon")
                            .then(|| parse_skill_block_notice(&m.body))
                            .flatten(),
                        |this, notice| {
                            let resolved_team = resolve_team(&self.team, &self.global);
                            let preceding_task = self.view.messages[..ix]
                                .iter()
                                .rev()
                                .find(|prev| {
                                    prev.kind_label == "message"
                                        && prev.from != "gluon"
                                        && (prev.to.as_deref() == Some(notice.quark_id.as_str())
                                            || prev.body.contains(&format!("@{}", notice.quark_id)))
                                })
                                .map(|prev| prev.body.clone())
                                .unwrap_or_else(|| format!("/{}", notice.skill_id));

                            let mut action_bar = h_flex()
                                .id(SharedString::from(format!("skill-block-actions-{ix}")))
                                .items_center()
                                .gap_2()
                                .flex_wrap()
                                .mt_1();

                            if let SkillBlockReason::ModeMismatch { required_mode, .. } =
                                &notice.reason
                            {
                                let req_mode = *required_mode;
                                let qid = notice.quark_id.clone();
                                let resend_body = preceding_task.clone();
                                let ent_sw = chamber_entity.clone();
                                action_bar = action_bar.child(
                                    h_flex()
                                        .id(SharedString::from(format!(
                                            "skill-block-switch-mode-{ix}"
                                        )))
                                        .cursor_pointer()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(theme::accent_soft())
                                        .border_1()
                                        .border_color(theme::accent().opacity(0.45))
                                        .hover(|s| s.bg(theme::accent().opacity(0.22)))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme::accent())
                                        .on_click(move |_, window, cx| {
                                            let qid_c = qid.clone();
                                            let body_c = if resend_body.starts_with('@') {
                                                resend_body.clone()
                                            } else {
                                                format!("@{qid_c} {resend_body}")
                                            };
                                            ent_sw.update(cx, |this, cx| {
                                                this.append_and_reload(
                                                    Event::new(
                                                        Actor::Human,
                                                        Some(QuarkId::new(&qid_c)),
                                                        Kind::ModeSet { mode: req_mode },
                                                    ),
                                                    cx,
                                                );
                                                this.append_and_reload(
                                                    Event::new(
                                                        Actor::Human,
                                                        None,
                                                        Kind::Message { body: body_c },
                                                    ),
                                                    cx,
                                                );
                                            });
                                            window.refresh();
                                        })
                                        .child(format!(
                                            "Switch @{} to {} mode & retry",
                                            notice.quark_id,
                                            req_mode.as_str()
                                        )),
                                );
                            }

                            for r in &self.view.roster {
                                if r.id == notice.quark_id || !r.enabled {
                                    continue;
                                }
                                let denies_skill = resolved_team
                                    .quarks
                                    .iter()
                                    .find(|s| s.id.as_str() == r.id)
                                    .is_some_and(|s| {
                                        s.deny_skills
                                            .iter()
                                            .any(|d| d.eq_ignore_ascii_case(&notice.skill_id))
                                    });
                                if denies_skill {
                                    continue;
                                }
                                let peer_id = r.id.clone();
                                let peer_label =
                                    r.display_name.as_deref().unwrap_or(&r.id).to_string();
                                let blocked_id = notice.quark_id.clone();
                                let task_src = preceding_task.clone();
                                let ent_re = chamber_entity.clone();
                                action_bar = action_bar.child(
                                    h_flex()
                                        .id(SharedString::from(format!(
                                            "skill-block-reassign-{ix}-{peer_id}"
                                        )))
                                        .cursor_pointer()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(theme::bg_surface())
                                        .border_1()
                                        .border_color(theme::glass_highlight())
                                        .hover(|s| s.bg(theme::bg_surface_raised()))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme::text_secondary())
                                        .on_click(move |_, window, cx| {
                                            let old_tag = format!("@{blocked_id}");
                                            let new_tag = format!("@{peer_id}");
                                            let body_c = if task_src.contains(&old_tag) {
                                                task_src.replacen(&old_tag, &new_tag, 1)
                                            } else {
                                                format!("{new_tag} {task_src}")
                                            };
                                            ent_re.update(cx, |this, cx| {
                                                this.append_and_reload(
                                                    Event::new(
                                                        Actor::Human,
                                                        None,
                                                        Kind::Message { body: body_c },
                                                    ),
                                                    cx,
                                                );
                                            });
                                            window.refresh();
                                        })
                                        .child(format!("Assign to @{peer_label}")),
                                );
                            }

                            this.child(action_bar)
                        },
                    ),
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
                            .child(crate::model::format_timestamp(m.ts.with_timezone(&chrono::Local), self.prefs.timestamp_format)),
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

/// The semantic theme accent for a gluon message's severity card, or `None` for
/// an unlabelled message (no card).
fn severity_accent(severity: Option<hadron_lattice::Severity>) -> Option<gpui::Rgba> {
    match severity? {
        hadron_lattice::Severity::Error => Some(crate::theme::error()),
        hadron_lattice::Severity::Warning => Some(crate::theme::warning()),
        hadron_lattice::Severity::Info => Some(crate::theme::info()),
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

    /// Every severity maps directly to its semantic theme token (`info`, `warning`, `error`).
    #[test]
    fn every_severity_matches_theme_token() {
        assert_eq!(severity_accent(Some(Severity::Info)), Some(crate::theme::info()));
        assert_eq!(severity_accent(Some(Severity::Warning)), Some(crate::theme::warning()));
        assert_eq!(severity_accent(Some(Severity::Error)), Some(crate::theme::error()));
    }

    /// …and the three must be visually distinct. Before the fix, Warning and Error
    /// both landed on red and the card told the human nothing.
    #[test]
    fn the_three_severities_are_different_colours() {
        let hues: Vec<f32> = [Severity::Info, Severity::Warning, Severity::Error]
            .into_iter()
            .map(|s| gpui::Hsla::from(severity_accent(Some(s)).unwrap()).h)
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

/// Extracts short git commit hashes (7–10 hex chars) and touched source file basenames
/// mentioned in a quark's message body for in-stream turn diff chips.
pub(super) fn extract_turn_artifacts(body: &str) -> (Vec<String>, Vec<String>) {
    let mut commits: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();

    for token in body.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '`' | '(' | ')' | '[' | ']' | ',' | '.' | ':' | ';' | '"' | '\''
            )
        });
        if cleaned.is_empty() {
            continue;
        }
        // Check for 7–10 char hex hash that contains both digits and hex letters (to avoid plain words/numbers)
        if (7..=10).contains(&cleaned.len())
            && cleaned.chars().all(|c| c.is_ascii_hexdigit())
            && cleaned.chars().any(|c| c.is_ascii_digit())
            && cleaned.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
        {
            let lower = cleaned.to_ascii_lowercase();
            if !commits.contains(&lower) && commits.len() < 4 {
                commits.push(lower);
            }
            continue;
        }
        // Check for file paths/basenames with source extensions
        let path_part = cleaned
            .split('#')
            .next()
            .unwrap_or(cleaned)
            .trim_start_matches("file://");
        let basename = path_part.rsplit('/').next().unwrap_or(path_part);
        let has_src_ext = [
            ".rs", ".py", ".toml", ".md", ".ts", ".tsx", ".js", ".json", ".sh", ".ps1",
        ]
        .iter()
        .any(|ext| basename.ends_with(ext) && basename.len() > ext.len());
        if has_src_ext
            && basename
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            && !files.iter().any(|f| f == basename)
            && files.len() < 5
        {
            files.push(basename.to_string());
        }
    }

    (commits, files)
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
        let files = vec![("src/app/render/chat.rs".to_string(), false)];
        let sessions = vec![];

        let comp = completion_candidates("@acp", 4, &quarks, &files, &sessions).expect("candidates");
        assert!(!comp.candidates.is_empty());
        assert_eq!(comp.candidates[0].detail, "Quark");

        let comp_files = completion_candidates("@src", 4, &quarks, &files, &sessions).expect("file candidates");
        assert!(!comp_files.candidates.is_empty());
        assert_eq!(comp_files.candidates[0].label, "📄 @chat.rs");
        assert_eq!(comp_files.candidates[0].detail, "src/app/render/chat.rs");

        let gitignored_files = vec![
            (".hadron/docs/plans/2026-08-21-swarm-orchestration-and-teamwork/master.md".to_string(), true),
        ];
        let comp_plans = completion_candidates("@master", 7, &quarks, &gitignored_files, &sessions).expect("plan candidates");
        assert_eq!(comp_plans.candidates[0].label, "📄 @master.md");
        assert_eq!(comp_plans.candidates[0].detail, ".hadron/docs/plans/2026-08-21-swarm-orchestration-and-teamwork/master.md");
        assert_eq!(comp_plans.candidates[0].new_text, "@.hadron/docs/plans/2026-08-21-swarm-orchestration-and-teamwork/master.md ");
    }

    #[test]
    fn test_completion_overlay_selection_bounds_cycling() {
        // Test selection index circular wrap-around logic used by move_completion_selection
        let candidates_count = 3;
        let cycle = |cur: usize, delta: isize| -> usize {
            let next = cur as isize + delta;
            if next < 0 {
                candidates_count - 1
            } else if next >= candidates_count as isize {
                0
            } else {
                next as usize
            }
        };

        let mut selected = 0;
        // Move up from 0 -> wraps around to last index (2)
        selected = cycle(selected, -1);
        assert_eq!(selected, 2);

        // Move up again -> 1
        selected = cycle(selected, -1);
        assert_eq!(selected, 1);

        // Move down: 1 -> 2
        selected = cycle(selected, 1);
        assert_eq!(selected, 2);

        // Move down past end from 2 -> wraps around to 0
        selected = cycle(selected, 1);
        assert_eq!(selected, 0);
    }

    #[test]
    fn test_extract_turn_artifacts_parses_commits_and_files() {
        let msg = "Committed `a1b2c3d4` touching [roster.rs](file:///path/to/roster.rs#L10-L20) and `crates/hadron-gluon/scripts/agy_acp.py`.";
        let (commits, files) = super::extract_turn_artifacts(msg);
        assert_eq!(commits, vec!["a1b2c3d4".to_string()]);
        assert_eq!(files, vec!["roster.rs".to_string(), "agy_acp.py".to_string()]);
    }

    #[test]
    fn test_composer_preview_max_height_bounds() {
        assert_eq!(super::COMPOSER_PREVIEW_MAX_HEIGHT_PX, 220.0);
        assert!((120.0..=300.0).contains(&super::COMPOSER_PREVIEW_MAX_HEIGHT_PX));
    }

    #[test]
    fn parse_skill_block_notice_recognizes_mode_mismatch_and_denied_skill() {
        use super::{parse_skill_block_notice, SkillBlockNotice, SkillBlockReason};
        use hadron_lattice::Mode;

        let mode_msg = "@orchestrator @acp-claude is in ask mode and cannot run 'executing-plans' (requires write mode); skipping.";
        assert_eq!(
            parse_skill_block_notice(mode_msg),
            Some(SkillBlockNotice {
                quark_id: "acp-claude".to_string(),
                skill_id: "executing-plans".to_string(),
                reason: SkillBlockReason::ModeMismatch {
                    current_mode: Mode::Ask,
                    required_mode: Mode::Write,
                },
            })
        );

        let deny_msg = "@cli-codex does not have skill 'release' enabled (deny_skills); skipping.";
        assert_eq!(
            parse_skill_block_notice(deny_msg),
            Some(SkillBlockNotice {
                quark_id: "cli-codex".to_string(),
                skill_id: "release".to_string(),
                reason: SkillBlockReason::DeniedSkill,
            })
        );

        let locks_msg = "@cli-codex locks out 'writing-plans' tasks (deny_skills); skipping.";
        assert_eq!(
            parse_skill_block_notice(locks_msg),
            Some(SkillBlockNotice {
                quark_id: "cli-codex".to_string(),
                skill_id: "writing-plans".to_string(),
                reason: SkillBlockReason::DeniedSkill,
            })
        );

        assert_eq!(parse_skill_block_notice("regular chat message"), None);
    }

    #[test]
    fn prompt_anatomy_fractions_scales_by_actual_usage_not_equal_fifths() {
        let breakdown = hadron_lattice::prompt_cost::PromptBreakdown {
            standard_model: 1_000,
            invariants: 3_000,     // rules = 4,000 (40%)
            nucleus_digest: 1_000,
            nucleus_index: 1_000,  // nucleus = 2,000 (20%)
            active_skill: 0,       // skill = 0 (0%)
            task: 1_000,           // task = 1,000 (10%)
            field_window: 3_000,   // transcript = 3,000 (30%)
        };
        let fractions = super::prompt_anatomy_fractions(&breakdown);
        assert_eq!(fractions[0].0, 4_000);
        assert!((fractions[0].1 - 0.40).abs() < 1e-5);
        assert_eq!(fractions[1].0, 2_000);
        assert!((fractions[1].1 - 0.20).abs() < 1e-5);
        assert_eq!(fractions[2].0, 0);
        assert_eq!(fractions[2].1, 0.0);
        assert_eq!(fractions[3].0, 1_000);
        assert!((fractions[3].1 - 0.10).abs() < 1e-5);
        assert_eq!(fractions[4].0, 3_000);
        assert!((fractions[4].1 - 0.30).abs() < 1e-5);
    }

    #[test]
    fn prompt_anatomy_colors_returns_five_colors() {
        let colors = super::prompt_anatomy_colors();
        assert_eq!(colors.len(), 5);
    }

    #[test]
    fn compute_onboarding_readiness_identifies_orchestrator_badge_and_active_counts() {
        let empty_team = hadron_lattice::Team::default();
        let r0 = super::compute_onboarding_readiness(false, &empty_team);
        assert!(!r0.git_ready);
        assert_eq!(r0.active_quark_count, 0);
        assert_eq!(r0.orchestrator_badge, None);

        let mut worker = hadron_lattice::Seat::cli(
            hadron_lattice::QuarkId::new("cli-codex"),
            "codex",
            "gpt-5",
            hadron_lattice::Flavor::Worker,
        );
        worker.enabled = true;
        let mut orch = hadron_lattice::Seat::cli(
            hadron_lattice::QuarkId::new("cli-agy"),
            "agy",
            "gemini-3-pro",
            hadron_lattice::Flavor::Orchestrator,
        );
        orch.enabled = true;
        let mut disabled = hadron_lattice::Seat::cli(
            hadron_lattice::QuarkId::new("cli-claude"),
            "claude",
            "claude-opus-4-8",
            hadron_lattice::Flavor::Worker,
        );
        disabled.enabled = false;

        let team = hadron_lattice::Team {
            quarks: vec![worker, orch, disabled],
            ..Default::default()
        };
        let r1 = super::compute_onboarding_readiness(true, &team);
        assert!(r1.git_ready);
        assert_eq!(r1.active_quark_count, 2);
        assert_eq!(
            r1.orchestrator_badge,
            Some((
                "cli-agy".to_string(),
                "agy".to_string(),
                "gemini-3-pro".to_string()
            ))
        );
    }
}


