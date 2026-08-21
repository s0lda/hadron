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
#[allow(dead_code)]
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
        let repo = crate::vcs::repo_root_of(&self.path);

        // Determine which files to inspect:
        // 1. If a branch is selected in Git view, inspect files changed in that branch relative to main.
        // 2. Otherwise if working tree has changes, inspect working tree vs main/HEAD.
        let (base_ref, theirs_ref, files_to_check): (&str, &str, Vec<String>) = if let Some(ref branch) = self.git_selected_branch {
            let files = match &self.git_branch_diff {
                Some(diffs) => diffs.iter().map(|d| d.path.clone()).collect(),
                None => Vec::new(),
            };
            ("main", branch.as_str(), files)
        } else if let Some(ref diffs) = self.working_diff {
            let files = diffs.iter().map(|d| d.path.clone()).collect();
            ("main", "HEAD", files)
        } else {
            ("main", "HEAD", Vec::new())
        };

        if files_to_check.is_empty() {
            return v_flex()
                .size_full()
                .p_6()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::text())
                        .child("No 3-Way Differences"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .text_center()
                        .child("The working tree is clean relative to base `main`.\nSelect a branch in the Branches tab to compare 3-way alignment."),
                )
                .into_any_element();
        }

        let mut list = v_flex().gap_3().w_full();

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
                .child(div().w(px(35.0)).text_color(theme::syntax_comment()).child("Line"))
                .child(div().flex_1().text_color(theme::syntax_comment()).child(format!("Base ({base_ref})")))
                .child(div().flex_1().text_color(theme::syntax_type()).child("Ours (Worktree)"))
                .child(div().flex_1().text_color(theme::syntax_literal()).child(format!("Theirs ({theirs_ref})"))),
        );

        for file_path in files_to_check.iter().take(5) {
            let base_content = crate::vcs::show_file_at_ref(repo, base_ref, file_path).unwrap_or_default();
            let ours_content = std::fs::read_to_string(repo.join(file_path)).unwrap_or_default();
            let theirs_content = crate::vcs::show_file_at_ref(repo, theirs_ref, file_path).unwrap_or_default();

            let all_hunks = compute_three_way_diff(&base_content, &ours_content, &theirs_content);
            let changed_hunks: Vec<ThreeWayHunk> = all_hunks.iter().filter(|h| h.status != ThreeWayStatus::Same).cloned().collect();
            let changed_count = changed_hunks.len();

            let mut file_card = v_flex()
                .w_full()
                .p_2()
                .rounded_lg()
                .bg(theme::bg_surface())
                .border_1()
                .border_color(theme::glass_highlight())
                .gap_1p5();

            file_card = file_card.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                gpui::img(crate::symbols::file_icon_path(file_path))
                                    .size_3()
                                    .flex_none(),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_family(mono_font.clone())
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::accent())
                                    .child(file_path.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(mono_font.clone())
                            .text_color(theme::text_muted())
                            .child(format!("{} diff hunks", changed_count)),
                    ),
            );

            let mut hunks_list = v_flex().gap_0p5().w_full();
            // Show changed hunks or first 20 lines if clean
            let display_hunks: Vec<ThreeWayHunk> = if !changed_hunks.is_empty() {
                changed_hunks
            } else {
                all_hunks.into_iter().take(20).collect()
            };

            for hunk in display_hunks {
                let bg_color = match hunk.status {
                    ThreeWayStatus::Same => theme::term_bg(),
                    ThreeWayStatus::OursModified => gpui::rgba(0x79b8ff18),
                    ThreeWayStatus::TheirsModified => gpui::rgba(0x85e89d18),
                    ThreeWayStatus::Conflict => gpui::rgba(0xf9758325),
                };

                let base_str = if hunk.base.is_empty() { " ".to_string() } else { hunk.base };
                let ours_str = if hunk.ours.is_empty() { " ".to_string() } else { hunk.ours };
                let theirs_str = if hunk.theirs.is_empty() { " ".to_string() } else { hunk.theirs };

                hunks_list = hunks_list.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(bg_color)
                        .text_xs()
                        .font_family(mono_font.clone())
                        .child(
                            div()
                                .w(px(35.0))
                                .text_color(theme::syntax_comment())
                                .child(hunk.line_number.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(theme::text_muted())
                                .child(base_str),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(theme::text())
                                .child(ours_str),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(theme::text())
                                .child(theirs_str),
                        ),
                );
            }

            file_card = file_card.child(hunks_list);
            list = list.child(file_card);
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
                            .font_family(mono_font.clone())
                            .text_color(theme::accent())
                            .child(format!("{theirs_ref} ⟷ {base_ref}")),
                    ),
            )
            .child(list)
            .into_any_element()
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
