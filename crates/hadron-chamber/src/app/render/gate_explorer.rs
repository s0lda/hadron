//! Live merge-gate test output explorer for `Mission > Gate`.
//!
//! Strictly display-only on the render path: reads exclusively from in-memory
//! [`QuarkGateStream`] entries buffered on [`Chamber`].

use super::*;
use gpui_component::ActiveTheme;
use std::collections::VecDeque;

/// Maximum number of raw stdout/stderr lines retained per quark gate stream.
pub(crate) const GATE_STREAM_LINE_CAP: usize = 500;

/// Filter pills for the `Mission > Gate` test list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::app) enum GateTestFilter {
    #[default]
    All,
    Failed,
    Passed,
}

impl GateTestFilter {
    pub(super) const ALL: [GateTestFilter; 3] = [
        GateTestFilter::All,
        GateTestFilter::Failed,
        GateTestFilter::Passed,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            GateTestFilter::All => "All",
            GateTestFilter::Failed => "Failed",
            GateTestFilter::Passed => "Passed",
        }
    }
}

/// Outcome of a single parsed `cargo test` case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum GateTestStatus {
    Passed,
    Failed,
    Ignored,
}

/// One test row displayed in the `Mission > Gate` Test Explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct GateTestRow {
    pub name: String,
    pub status: GateTestStatus,
}

/// Per-quark live merge-gate stream buffer and parsed test summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct QuarkGateStream {
    pub quark: String,
    pub active: bool,
    pub passed_count: usize,
    pub failed_count: usize,
    pub ignored_count: usize,
    pub tests: Vec<GateTestRow>,
    pub lines: VecDeque<String>,
    pub last_verdict: Option<bool>,
}

enum ParsedGateLine {
    Case { name: String, status: GateTestStatus },
    Summary {
        passed: usize,
        failed: usize,
        ignored: usize,
    },
}

/// Local cargo-test line parser; Wave 2 unifies this onto `hadron_lattice::wire::parse_cargo_test_line`.
fn parse_gate_test_line(line: &str) -> Option<ParsedGateLine> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("test ") {
        if let Some((name, outcome)) = rest.rsplit_once(" ... ") {
            let status = match outcome.trim() {
                "ok" => GateTestStatus::Passed,
                "FAILED" => GateTestStatus::Failed,
                "ignored" => GateTestStatus::Ignored,
                _ => return None,
            };
            let name = name.trim();
            if !name.is_empty() {
                return Some(ParsedGateLine::Case {
                    name: name.to_string(),
                    status,
                });
            }
        }
    }

    if let Some(rest) = trimmed.strip_prefix("test result:") {
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut ignored = 0usize;
        let mut matched_any = false;
        for part in rest.split(';') {
            let tokens: Vec<&str> = part.split_whitespace().collect();
            if tokens.len() >= 2 {
                if let Ok(n) = tokens[tokens.len() - 2].parse::<usize>() {
                    match tokens[tokens.len() - 1] {
                        "passed" => {
                            passed = n;
                            matched_any = true;
                        }
                        "failed" => {
                            failed = n;
                            matched_any = true;
                        }
                        "ignored" => {
                            ignored = n;
                            matched_any = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if matched_any {
            return Some(ParsedGateLine::Summary {
                passed,
                failed,
                ignored,
            });
        }
    }

    None
}

impl QuarkGateStream {
    pub(super) fn new(quark: impl Into<String>) -> Self {
        Self {
            quark: quark.into(),
            active: true,
            passed_count: 0,
            failed_count: 0,
            ignored_count: 0,
            tests: Vec::new(),
            lines: VecDeque::new(),
            last_verdict: None,
        }
    }

    /// Ingest one output line from `stream` (`"stdout"` or `"stderr"`), updating test rows and bounded tail.
    pub(super) fn ingest_line(&mut self, stream: &str, line: &str) {
        // Starting a new gate run after a previous verdict resets stale results.
        if !self.active && self.last_verdict.is_some() {
            self.passed_count = 0;
            self.failed_count = 0;
            self.ignored_count = 0;
            self.tests.clear();
            self.lines.clear();
            self.last_verdict = None;
        }
        self.active = true;

        let display_line = if stream.eq_ignore_ascii_case("stderr") {
            format!("[stderr] {line}")
        } else {
            line.to_string()
        };
        while self.lines.len() >= GATE_STREAM_LINE_CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(display_line);

        if let Some(parsed) = parse_gate_test_line(line) {
            match parsed {
                ParsedGateLine::Case { name, status } => {
                    if let Some(existing) = self.tests.iter_mut().find(|t| t.name == name) {
                        existing.status = status;
                    } else {
                        self.tests.push(GateTestRow { name, status });
                    }
                    self.recount_from_tests();
                }
                ParsedGateLine::Summary {
                    passed,
                    failed,
                    ignored,
                } => {
                    self.passed_count = self.passed_count.max(passed);
                    self.failed_count = self.failed_count.max(failed);
                    self.ignored_count = self.ignored_count.max(ignored);
                }
            }
        }
    }

    fn recount_from_tests(&mut self) {
        let mut p = 0usize;
        let mut f = 0usize;
        let mut i = 0usize;
        for row in &self.tests {
            match row.status {
                GateTestStatus::Passed => p += 1,
                GateTestStatus::Failed => f += 1,
                GateTestStatus::Ignored => i += 1,
            }
        }
        self.passed_count = p;
        self.failed_count = f;
        self.ignored_count = i;
    }

    /// Mark the active merge-gate run as finished with an optional pass/fail verdict.
    pub(super) fn mark_finished(&mut self, passed: Option<bool>) {
        self.active = false;
        self.last_verdict = passed.or_else(|| Some(self.failed_count == 0));
    }
}

pub(super) fn gate_test_fix_message(quark: &str, test_name: &str) -> String {
    format!(
        "@{quark} Fix failing test `{test_name}` in your worktree, run cargo test to verify it passes, commit, and end your turn so the merge gate retries."
    )
}

impl Chamber {
    /// Buffer one live gate output line for `quark` and auto-select active stream.
    #[allow(dead_code)]
    pub(in crate::app) fn ingest_gate_output_line(&mut self, quark: &str, stream: &str, line: &str) {
        let entry = self
            .gate_streams
            .entry(quark.to_string())
            .or_insert_with(|| QuarkGateStream::new(quark));
        entry.ingest_line(stream, line);

        let should_select = match self.selected_gate_quark.as_deref() {
            None => true,
            Some(sel) => self
                .gate_streams
                .get(sel)
                .map(|s| !s.active && sel != quark)
                .unwrap_or(true),
        };
        if should_select {
            self.selected_gate_quark = Some(quark.to_string());
        }
    }

    /// Mark `quark`'s merge-gate stream finished with `passed` verdict.
    #[allow(dead_code)]
    pub(in crate::app) fn mark_gate_finished(&mut self, quark: &str, passed: Option<bool>) {
        if let Some(entry) = self.gate_streams.get_mut(quark) {
            entry.mark_finished(passed);
        }
    }

    pub(super) fn render_gate_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = v_flex().flex_1().min_h_0().w_full().gap_3().p_3();

        if self.gate_streams.is_empty() {
            return root.child(
                v_flex()
                    .w_full()
                    .p_4()
                    .gap_2()
                    .rounded_lg()
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("Merge Gate Test Explorer"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("No merge-gate runs streamed yet. Live cargo test progress, failing tests, and streaming build output appear here automatically when a quark enters the merge gate."),
                    ),
            );
        }

        // Quark selector pills
        let active_key = self
            .selected_gate_quark
            .as_ref()
            .filter(|k| self.gate_streams.contains_key(*k))
            .cloned()
            .or_else(|| self.gate_streams.keys().next().cloned());

        let mut quark_pills = h_flex().w_full().gap_1p5().flex_wrap();
        for (ix, (quark_id, stream)) in self.gate_streams.iter().enumerate() {
            let is_sel = active_key.as_deref() == Some(quark_id.as_str());
            let (status_label, status_color) = if stream.active {
                ("Running", theme::info())
            } else if stream.last_verdict == Some(true) {
                ("Passed", theme::success())
            } else {
                ("Failed", theme::error())
            };
            let q_select = quark_id.clone();
            quark_pills = quark_pills.child(
                h_flex()
                    .id(("gate-quark-pill", ix))
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if is_sel {
                        theme::glass_highlight()
                    } else {
                        theme::bg_surface().into()
                    })
                    .border_1()
                    .border_color(if is_sel {
                        status_color.opacity(0.6).into()
                    } else {
                        theme::glass_highlight()
                    })
                    .child(div().size(px(7.0)).rounded_full().bg(status_color))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(if is_sel {
                                gpui::FontWeight::BOLD
                            } else {
                                gpui::FontWeight::MEDIUM
                            })
                            .text_color(theme::text())
                            .child(format!(
                                "@{} · {} ({}/{}/{})",
                                quark_id,
                                status_label,
                                stream.passed_count,
                                stream.failed_count,
                                stream.ignored_count
                            )),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_gate_quark = Some(q_select.clone());
                        cx.notify();
                    })),
            );
        }
        root = root.child(quark_pills);

        let Some(selected_key) = active_key else {
            return root;
        };
        let Some(stream) = self.gate_streams.get(&selected_key) else {
            return root;
        };

        // Summary + Filter bar + [Retry Gate]
        let q_retry = stream.quark.clone();
        let (badge_text, badge_color) = if stream.active {
            ("RUNNING", theme::info())
        } else if stream.last_verdict == Some(true) {
            ("GREEN GATE", theme::success())
        } else {
            ("RED GATE", theme::error())
        };

        let mut filter_pills = h_flex().gap_1().items_center();
        for filter in GateTestFilter::ALL {
            let is_sel = self.gate_test_filter == filter;
            filter_pills = filter_pills.child(
                div()
                    .id(("gate-filter-pill", filter as usize))
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .font_weight(if is_sel {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::MEDIUM
                    })
                    .bg(if is_sel {
                        theme::accent_soft()
                    } else {
                        theme::bg_base()
                    })
                    .text_color(if is_sel {
                        theme::accent()
                    } else {
                        theme::text_muted()
                    })
                    .child(filter.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.gate_test_filter = filter;
                        cx.notify();
                    })),
            );
        }

        let header_card = v_flex()
            .w_full()
            .p_2p5()
            .gap_2()
            .rounded_lg()
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::glass_highlight())
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(badge_color.opacity(0.16))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(badge_color)
                                    .child(badge_text),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_secondary())
                                    .child(format!(
                                        "{} passed · {} failed · {} ignored",
                                        stream.passed_count,
                                        stream.failed_count,
                                        stream.ignored_count
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("gate-explorer-retry-btn")
                            .flex_shrink_0()
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme::accent_soft())
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .child("Retry Gate")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.chat_tab = ChatTab::Chat;
                                this.post_chat_message(
                                    Actor::Human,
                                    crate::app::render::chat::gate_retry_message(&q_retry),
                                    cx,
                                );
                            })),
                    ),
            )
            .child(filter_pills);
        root = root.child(header_card);

        // Filtered test rows
        let filtered_tests: Vec<&GateTestRow> = stream
            .tests
            .iter()
            .filter(|t| match self.gate_test_filter {
                GateTestFilter::All => true,
                GateTestFilter::Failed => t.status == GateTestStatus::Failed,
                GateTestFilter::Passed => t.status == GateTestStatus::Passed,
            })
            .collect();

        let mut test_list = v_flex().w_full().gap_1();
        if filtered_tests.is_empty() {
            test_list = test_list.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child("No matching test cases recorded yet."),
            );
        } else {
            for (ix, row) in filtered_tests.iter().enumerate() {
                let (tag, tag_color) = match row.status {
                    GateTestStatus::Passed => ("PASS", theme::success()),
                    GateTestStatus::Failed => ("FAIL", theme::error()),
                    GateTestStatus::Ignored => ("SKIP", theme::warning()),
                };
                let q_fix = stream.quark.clone();
                let test_name = row.name.clone();

                let mut item = h_flex()
                    .id(("gate-test-row", ix))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::bg_surface_raised())
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_1()
                            .py_0p5()
                            .rounded_sm()
                            .bg(tag_color.opacity(0.16))
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(tag_color)
                            .child(tag),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .truncate()
                            .text_color(theme::text())
                            .child(row.name.clone()),
                    );

                if row.status == GateTestStatus::Failed {
                    item = item.child(
                        div()
                            .id(("gate-test-dispatch-fix", ix))
                            .flex_shrink_0()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme::error().opacity(0.16))
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::error())
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .child("Dispatch Fix")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.chat_tab = ChatTab::Chat;
                                this.post_chat_message(
                                    Actor::Human,
                                    gate_test_fix_message(&q_fix, &test_name),
                                    cx,
                                );
                            })),
                    );
                }

                test_list = test_list.child(item);
            }
        }

        let test_box = div()
            .id("gate-test-list-box")
            .w_full()
            .max_h(px(220.0))
            .overflow_y_scroll()
            .child(test_list);
        root = root.child(test_box);

        // Bounded scrollable live output tail
        let mono_font = cx.theme().mono_font_family.clone();
        let mut tail_lines = v_flex().w_full().gap_0p5().p_2();
        for (ix, line) in stream.lines.iter().enumerate() {
            let line_color = if line.contains("FAILED") || line.starts_with("[stderr] error") {
                theme::error()
            } else if line.ends_with("... ok") || line.contains("test result: ok") {
                theme::success()
            } else {
                theme::text_secondary()
            };
            tail_lines = tail_lines.child(
                div()
                    .id(("gate-tail-line", ix))
                    .w_full()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .text_color(line_color)
                    .child(line.clone()),
            );
        }

        let tail_container = div()
            .relative()
            .w_full()
            .rounded_md()
            .bg(theme::bg_base())
            .border_1()
            .border_color(theme::border())
            .child(
                div()
                    .id("gate-output-tail")
                    .w_full()
                    .max_h(px(260.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.gate_output_scroll)
                    .child(tail_lines),
            )
            .child(
                div().absolute().top_0().bottom_0().right_0().child(
                    Scrollbar::vertical(&self.gate_output_scroll)
                        .scrollbar_show(ScrollbarShow::Always),
                ),
            );

        root.child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::text_muted())
                .child(format!("Live Output ({} lines)", stream.lines.len())),
        )
        .child(tail_container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quark_gate_stream_ingest_parses_cases_and_summary() {
        let mut s = QuarkGateStream::new("cli-agy");
        s.ingest_line("stdout", "   Compiling hadron-lattice v0.33.54");
        s.ingest_line("stdout", "test wire::tests::roundtrip ... ok");
        s.ingest_line("stdout", "test wire::tests::bad_frame ... FAILED");
        s.ingest_line("stdout", "test wire::tests::slow_case ... ignored");
        s.ingest_line(
            "stdout",
            "test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out",
        );

        assert!(s.active);
        assert_eq!(s.passed_count, 1);
        assert_eq!(s.failed_count, 1);
        assert_eq!(s.ignored_count, 1);
        assert_eq!(s.tests.len(), 3);
        assert_eq!(s.tests[0].status, GateTestStatus::Passed);
        assert_eq!(s.tests[1].status, GateTestStatus::Failed);
        assert_eq!(s.tests[2].status, GateTestStatus::Ignored);

        s.mark_finished(None);
        assert!(!s.active);
        assert_eq!(s.last_verdict, Some(false));
    }

    #[test]
    fn test_quark_gate_stream_enforces_500_line_cap_and_resets_after_verdict() {
        let mut s = QuarkGateStream::new("acp-agy");
        for i in 0..620 {
            s.ingest_line("stdout", &format!("line {i}"));
        }
        assert_eq!(s.lines.len(), GATE_STREAM_LINE_CAP);
        assert_eq!(s.lines.front().map(String::as_str), Some("line 120"));
        assert_eq!(s.lines.back().map(String::as_str), Some("line 619"));

        s.mark_finished(Some(true));
        assert!(!s.active);
        assert_eq!(s.last_verdict, Some(true));

        // New line after finished verdict starts a clean run
        s.ingest_line("stderr", "error[E0425]: cannot find value");
        assert!(s.active);
        assert_eq!(s.last_verdict, None);
        assert_eq!(s.lines.len(), 1);
        assert_eq!(
            s.lines.front().map(String::as_str),
            Some("[stderr] error[E0425]: cannot find value")
        );
    }

    #[test]
    fn test_gate_test_fix_message_targets_quark_and_test() {
        let msg = gate_test_fix_message("cli-agy-2", "liveness::tests::flags_unwired");
        assert!(msg.starts_with("@cli-agy-2 "));
        assert!(msg.contains("`liveness::tests::flags_unwired`"));
    }
}
