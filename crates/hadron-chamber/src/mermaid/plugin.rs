//! Markdown extension plugin for rendering ```mermaid blocks as native diagram cards.

#[cfg(feature = "gui")]
use super::render::MermaidCard;
#[cfg(feature = "gui")]
use gpui::{App, IntoElement, Window};
#[cfg(feature = "gui")]
use gpui_component::text::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast,
};

#[cfg(feature = "gui")]
#[derive(Clone, Debug, PartialEq)]
pub struct MermaidBlockData {
    pub source: String,
}

/// A MarkdownPlugin that intercepts ```mermaid fenced code blocks
/// and renders them using the native GPUI `MermaidCard`.
#[cfg(feature = "gui")]
#[derive(Clone, Default)]
pub struct MermaidPlugin;

#[cfg(feature = "gui")]
impl MarkdownPlugin for MermaidPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "mermaid"
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        if let markdown_ast::Node::Code(code) = node {
            if let Some(lang) = &code.lang {
                if lang.trim().eq_ignore_ascii_case("mermaid") {
                    let source = code.value.clone();
                    return Some(
                        MarkdownNode::new("mermaid", MermaidBlockData {
                            source: source.clone(),
                        })
                        .text(source.clone())
                        .markdown(cx.node_source(node).unwrap_or_default()),
                    );
                }
            }
        }
        None
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        if let Some(data) = node.data::<MermaidBlockData>() {
            MermaidCard::new(data.source.clone()).into_any_element()
        } else {
            MermaidCard::new(node.as_text().to_string()).into_any_element()
        }
    }
}

/// Returns the standard MarkdownExtensions registry configured with Mermaid support.
#[cfg(feature = "gui")]
pub fn chamber_markdown_extensions() -> MarkdownExtensions {
    MarkdownExtensions::default().plugin(MermaidPlugin)
}
