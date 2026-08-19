use super::*;
use gpui_component::ActiveTheme;

/// Alignment status of a 3-way diff hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreeWayStatus {
    Same,
    OursModified,
    TheirsModified,
    Conflict,
}

/// A hunk comparing Base (ancestor), Ours (current worktree), and Theirs (target/incoming).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeWayHunk {
    pub line_number: usize,
    pub base: String,
    pub ours: String,
    pub theirs: String,
    pub status: ThreeWayStatus,
}

/// A file diff containing 3-way comparison hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeWayFileDiff {
    pub path: String,
    pub hunks: Vec<ThreeWayHunk>,
}

/// Computes a line-by-line 3-way diff between base, ours, and theirs.
pub fn compute_three_way_diff(base: &str, ours: &str, theirs: &str) -> Vec<ThreeWayHunk> {
    let base_lines: Vec<&str> = base.lines().collect();
    let ours_lines: Vec<&str> = ours.lines().collect();
    let theirs_lines: Vec<&str> = theirs.lines().collect();

    let max_len = base_lines.len().max(ours_lines.len()).max(theirs_lines.len());
    let mut hunks = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let b = base_lines.get(i).copied().unwrap_or("");
        let o = ours_lines.get(i).copied().unwrap_or("");
        let t = theirs_lines.get(i).copied().unwrap_or("");

        let status = if o == b && t == b {
            ThreeWayStatus::Same
        } else if o != b && t == b {
            ThreeWayStatus::OursModified
        } else if o == b && t != b {
            ThreeWayStatus::TheirsModified
        } else if o == t {
            ThreeWayStatus::OursModified // both made identical change
        } else {
            ThreeWayStatus::Conflict
        };

        hunks.push(ThreeWayHunk {
            line_number: i + 1,
            base: b.to_string(),
            ours: o.to_string(),
            theirs: t.to_string(),
            status,
        });
    }

    hunks
}

impl Chamber {
    /// Renders the 3-Way Visual Diff Inspector section (Capability #12).
    pub(super) fn git_diff_inspector_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mono_font = cx.theme().mono_font_family.clone();

        // Sample 3-way comparison between main base, worktree, and branch target
        let sample_base = "fn run_gate() {\n    let status = execute();\n    assert!(status.is_ok());\n}";
        let sample_ours = "fn run_gate() {\n    let status = execute_sandboxed();\n    assert!(status.is_ok());\n}";
        let sample_theirs = "fn run_gate() {\n    let status = execute();\n    log_gate_telemetry(&status);\n    assert!(status.is_ok());\n}";

        let hunks = compute_three_way_diff(sample_base, sample_ours, sample_theirs);

        let mut list = v_flex().gap_1().w_full();

        // 3-Way Column Headers
        list = list.child(
            h_flex()
                .w_full()
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded_md()
                .bg(theme::bg_surface())
                .border_1()
                .border_color(theme::glass_highlight())
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .child(div().w(px(40.0)).text_color(theme::text_muted()).child("Line"))
                .child(div().flex_1().text_color(theme::text_muted()).child("Base (Ancestor)"))
                .child(div().flex_1().text_color(gpui::rgb(0x60a5fa)).child("Ours (Worktree)"))
                .child(div().flex_1().text_color(gpui::rgb(0x34d399)).child("Theirs (Target)")),
        );

        for hunk in hunks {
            let bg_color = match hunk.status {
                ThreeWayStatus::Same => theme::term_bg(),
                ThreeWayStatus::OursModified => gpui::rgba(0x60a5fa15),
                ThreeWayStatus::TheirsModified => gpui::rgba(0x34d39915),
                ThreeWayStatus::Conflict => gpui::rgba(0xf8717125),
            };

            list = list.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(bg_color)
                    .text_xs()
                    .font_family(mono_font.clone())
                    .child(
                        div()
                            .w(px(40.0))
                            .text_color(theme::text_muted())
                            .child(hunk.line_number.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme::text_muted())
                            .child(hunk.base),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme::text())
                            .child(hunk.ours),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme::text())
                            .child(hunk.theirs),
                    ),
            );
        }

        v_flex()
            .gap_2()
            .w_full()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::text())
                            .child("3-Way Visual Diff Inspector (Base · Ours · Theirs)"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::accent())
                            .child("Active Branch Comparison"),
                    ),
            )
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_three_way_diff_clean_and_conflict() {
        let base = "line1\nline2\nline3";
        let ours = "line1\nline2_ours\nline3";
        let theirs = "line1\nline2_theirs\nline3";

        let hunks = compute_three_way_diff(base, ours, theirs);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].status, ThreeWayStatus::Same);
        assert_eq!(hunks[1].status, ThreeWayStatus::Conflict);
        assert_eq!(hunks[2].status, ThreeWayStatus::Same);
    }

    #[test]
    fn test_compute_three_way_diff_ours_only() {
        let base = "a\nb";
        let ours = "a_mod\nb";
        let theirs = "a\nb";

        let hunks = compute_three_way_diff(base, ours, theirs);
        assert_eq!(hunks[0].status, ThreeWayStatus::OursModified);
        assert_eq!(hunks[1].status, ThreeWayStatus::Same);
    }
}
