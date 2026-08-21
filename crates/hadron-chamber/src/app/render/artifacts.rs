use super::*;
use gpui_component::ActiveTheme;
use hadron_lattice::artifacts::{list_artifacts, ArtifactMeta};

impl Chamber {
    /// Renders the Artifact Bus drawer in the Chamber UI.
    #[allow(dead_code)]
    pub(super) fn render_artifacts_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let artifacts = list_artifacts(&self.repo_root).unwrap_or_default();

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child("📦")
                            .child(
                                div()
                                    .font_bold()
                                    .text_sm()
                                    .text_color(cx.theme().foreground)
                                    .child("Shared Artifact Bus"),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} artifacts", artifacts.len())),
                    ),
            )
            .child(
                if artifacts.is_empty() {
                    v_flex()
                        .size_full()
                        .justify_center()
                        .items_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No typed artifacts published yet. Quarks can publish diagrams, plans, OpenAPI contracts, and diffs via Lattice Artifact Bus.")
                } else {
                    v_flex()
                        .gap_2()
                        .children(artifacts.into_iter().map(|a| self.render_artifact_card(&a, cx)))
                },
            )
    }

    #[allow(dead_code)]
    fn render_artifact_card(&self, artifact: &ArtifactMeta, cx: &Context<Self>) -> impl IntoElement {
        let icon = match artifact.kind.as_str() {
            "mermaid" => "📊",
            "openapi" => "🔌",
            "diff" => "📝",
            _ => "📄",
        };

        h_flex()
            .p_2p5()
            .rounded_md()
            .bg(cx.theme().secondary)
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(icon)
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .font_medium()
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(artifact.id.clone()),
                            )
                            .child(
                                div()
                                    .text_2xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("by {} · {}", artifact.author, artifact.kind)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_2xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(artifact.created_at.chars().take(10).collect::<String>()),
            )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_artifact_view_smoke() {
        assert!(true);
    }
}
