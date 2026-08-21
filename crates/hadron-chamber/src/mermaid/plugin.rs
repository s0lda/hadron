//! Markdown extension plugin for rendering ```mermaid blocks, badge pills, and images.

#[cfg(feature = "gui")]
use super::render::MermaidCard;
#[cfg(feature = "gui")]
use gpui::{
    div, prelude::FluentBuilder as _, px, App, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Window,
};
#[cfg(feature = "gui")]
use gpui_component::WindowExt as _;
#[cfg(feature = "gui")]
use gpui_component::text::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast,
};
#[cfg(feature = "gui")]
use std::hash::{Hash, Hasher};

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

/// Preprocesses markdown text by unwrapping presentation HTML container tags like
/// `<div align="center">`, `<div>`, `</div>`, `<center>`, `</center>`, `<br />` outside of
/// fenced code blocks, ensuring CommonMark parses nested markdown elements (headings, bold,
/// badges, images) into standard AST nodes instead of treating the entire block as raw HTML.
pub fn format_html_wrappers(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut in_code_block = false;

    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            if i > 0 {
                out.push('\n');
            }
            out.push_str(line);
            continue;
        }
        if in_code_block {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(line);
            continue;
        }

        let trimmed_line = line.trim();
        if (trimmed_line.starts_with("<div") && trimmed_line.ends_with('>'))
            || trimmed_line == "</div>"
            || trimmed_line == "<center>"
            || trimmed_line == "</center>"
            || trimmed_line == "<br />"
            || trimmed_line == "<br/>"
            || trimmed_line == "<br>"
        {
            continue;
        }

        if i > 0 && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    if text.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Structured data for a single badge / pill item.
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeItem {
    pub label: String,
    pub status: String,
    pub color_name: String,
    pub link_url: Option<String>,
}

/// Decodes percent-encoded characters and shields.io formatting in URLs.
pub fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex_str = format!("{c1}{c2}");
                if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                    out.push(byte as char);
                    continue;
                }
                out.push('%');
                out.push(c1);
                out.push(c2);
            } else {
                out.push('%');
                if let Some(c1) = h1 {
                    out.push(c1);
                }
            }
        } else if c == '+' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses shields.io, badgen.net, or raster badge URLs into structured badge items.
pub fn parse_shield_badge(url: &str) -> Option<BadgeItem> {
    let trimmed = url.trim();
    if trimmed.contains("shields.io/badge/") || trimmed.contains("raster.shields.io/badge/") {
        let badge_path = if let Some(idx) = trimmed.find("/badge/") {
            &trimmed[idx + 7..]
        } else {
            return None;
        };
        // Remove query parameters if any
        let clean_path = badge_path.split('?').next().unwrap_or(badge_path);
        // Strip extensions like .svg, .png, .json
        let clean_path = clean_path
            .strip_suffix(".svg")
            .or_else(|| clean_path.strip_suffix(".png"))
            .or_else(|| clean_path.strip_suffix(".json"))
            .unwrap_or(clean_path);

        // In shields.io syntax:
        // `--` represents an escaped dash `-`
        // `_` represents a space ` `
        // Format is: <label>-<status>-<color>
        let placeholder = clean_path.replace("--", "\x01");
        let parts: Vec<&str> = placeholder.split('-').collect();
        if parts.len() >= 3 {
            let label_raw = parts[0];
            let status_raw = parts[1..parts.len() - 1].join("-");
            let color_raw = parts[parts.len() - 1];

            let unescape = |s: &str| -> String {
                let with_dashes = s.replace('\x01', "-");
                let with_spaces = with_dashes.replace('_', " ");
                let decoded = urlencoding_decode(&with_spaces);
                decoded.trim().to_string()
            };

            let label = unescape(label_raw);
            let status = unescape(&status_raw);
            let color = color_raw.replace('\x01', "-").trim().to_string();

            if !label.is_empty() && !status.is_empty() {
                return Some(BadgeItem {
                    label,
                    status,
                    color_name: color,
                    link_url: None,
                });
            }
        } else if parts.len() == 2 {
            let status = parts[0].replace('\x01', "-").replace('_', " ");
            let color = parts[1].replace('\x01', "-").trim().to_string();
            return Some(BadgeItem {
                label: String::new(),
                status: urlencoding_decode(&status).trim().to_string(),
                color_name: color,
                link_url: None,
            });
        }
    } else if trimmed.contains("shields.io/static/v1") {
        let query = trimmed.split('?').nth(1)?;
        let mut label = String::new();
        let mut message = String::new();
        let mut color = "blue".to_string();
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                match k.to_ascii_lowercase().as_str() {
                    "label" => label = urlencoding_decode(v),
                    "message" => message = urlencoding_decode(v),
                    "color" => color = urlencoding_decode(v),
                    _ => {}
                }
            }
        }
        if !label.is_empty() || !message.is_empty() {
            return Some(BadgeItem {
                label,
                status: message,
                color_name: color,
                link_url: None,
            });
        }
    } else if trimmed.contains("badgen.net/badge/") {
        let badge_path = trimmed.split("/badge/").nth(1)?;
        let clean_path = badge_path.split('?').next().unwrap_or(badge_path);
        let parts: Vec<&str> = clean_path.split('/').collect();
        if parts.len() >= 3 {
            let label = urlencoding_decode(parts[0]).replace('_', " ");
            let status = urlencoding_decode(parts[1]).replace('_', " ");
            let color = parts[2].to_string();
            return Some(BadgeItem {
                label,
                status,
                color_name: color,
                link_url: None,
            });
        }
    }
    None
}

/// Resolves a badge color name or hex into GPUI `Hsla` and high-contrast text color.
#[cfg(feature = "gui")]
pub fn badge_color(color_name: &str) -> (gpui::Hsla, gpui::Rgba) {
    let lower = color_name.to_ascii_lowercase();
    let lower = lower.trim_end_matches(".svg").trim_end_matches(".png");
    match lower {
        "blue" | "informational" => (gpui::hsla(217.0 / 360.0, 0.91, 0.60, 1.0), gpui::rgb(0xffffff)),
        "brightgreen" | "success" => (gpui::hsla(142.0 / 360.0, 0.71, 0.45, 1.0), gpui::rgb(0xffffff)),
        "green" => (gpui::hsla(142.0 / 360.0, 0.65, 0.40, 1.0), gpui::rgb(0xffffff)),
        "yellowgreen" => (gpui::hsla(84.0 / 360.0, 0.70, 0.45, 1.0), gpui::rgb(0xffffff)),
        "yellow" | "warning" => (gpui::hsla(48.0 / 360.0, 0.96, 0.53, 1.0), gpui::rgb(0x1e293b)),
        "orange" => (gpui::hsla(25.0 / 360.0, 0.95, 0.53, 1.0), gpui::rgb(0xffffff)),
        "red" | "critical" | "danger" | "error" => (gpui::hsla(0.0 / 360.0, 0.84, 0.60, 1.0), gpui::rgb(0xffffff)),
        "purple" | "blueviolet" => (gpui::hsla(271.0 / 360.0, 0.91, 0.65, 1.0), gpui::rgb(0xffffff)),
        "pink" | "ff69b4" => (gpui::hsla(330.0 / 360.0, 0.81, 0.60, 1.0), gpui::rgb(0xffffff)),
        "teal" | "cyan" => (gpui::hsla(189.0 / 360.0, 0.94, 0.43, 1.0), gpui::rgb(0xffffff)),
        "gray" | "grey" | "lightgrey" | "inactive" => (gpui::hsla(215.0 / 360.0, 0.16, 0.47, 1.0), gpui::rgb(0xffffff)),
        hex => {
            let clean_hex = hex.trim_start_matches('#');
            if clean_hex.len() == 6 {
                if let Ok(num) = u32::from_str_radix(clean_hex, 16) {
                    return (gpui::rgba(num).into(), gpui::rgb(0xffffff));
                }
            }
            (gpui::hsla(217.0 / 360.0, 0.91, 0.60, 1.0), gpui::rgb(0xffffff))
        }
    }
}

#[cfg(feature = "gui")]
#[derive(Clone, Debug, PartialEq)]
pub struct BadgeRowData {
    pub badges: Vec<BadgeItem>,
}

/// A MarkdownPlugin that intercepts rows of badges/shields and renders them as native styled GPUI pills.
#[cfg(feature = "gui")]
#[derive(Clone, Default)]
pub struct BadgePlugin;

#[cfg(feature = "gui")]
impl MarkdownPlugin for BadgePlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "badge-row"
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        match node {
            markdown_ast::Node::Paragraph(p) => {
                let mut badges = Vec::new();
                for child in &p.children {
                    match child {
                        markdown_ast::Node::Link(link) => {
                            for link_child in &link.children {
                                if let markdown_ast::Node::Image(img) = link_child {
                                    if let Some(mut badge) = parse_shield_badge(&img.url) {
                                        badge.link_url = Some(link.url.clone());
                                        badges.push(badge);
                                    }
                                }
                            }
                        }
                        markdown_ast::Node::Image(img) => {
                            if let Some(badge) = parse_shield_badge(&img.url) {
                                badges.push(badge);
                            }
                        }
                        _ => {}
                    }
                }
                if !badges.is_empty() {
                    return Some(
                        MarkdownNode::new("badge-row", BadgeRowData { badges })
                            .markdown(cx.node_source(node).unwrap_or_default()),
                    );
                }
            }
            markdown_ast::Node::Link(link) => {
                for link_child in &link.children {
                    if let markdown_ast::Node::Image(img) = link_child {
                        if let Some(mut badge) = parse_shield_badge(&img.url) {
                            badge.link_url = Some(link.url.clone());
                            return Some(
                                MarkdownNode::new("badge-row", BadgeRowData {
                                    badges: vec![badge],
                                })
                                .markdown(cx.node_source(node).unwrap_or_default()),
                            );
                        }
                    }
                }
            }
            markdown_ast::Node::Image(img) => {
                if let Some(badge) = parse_shield_badge(&img.url) {
                    return Some(
                        MarkdownNode::new("badge-row", BadgeRowData {
                            badges: vec![badge],
                        })
                        .markdown(cx.node_source(node).unwrap_or_default()),
                    );
                }
            }
            _ => {}
        }
        None
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Some(data) = node.data::<BadgeRowData>() else {
            return div().into_any_element();
        };

        let mut row = gpui_component::h_flex()
            .gap_2()
            .flex_wrap()
            .my_2()
            .items_center();

        for (idx, badge) in data.badges.iter().enumerate() {
            let (bg_color, text_color) = badge_color(&badge.color_name);
            let link_url = badge.link_url.clone();
            let tooltip_text = link_url
                .clone()
                .unwrap_or_else(|| format!("{}: {}", badge.label, badge.status));

            let pill = div()
                .id(ElementId::NamedInteger("badge-pill".into(), idx as u64))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(crate::theme::border())
                .overflow_hidden()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(SharedString::from(tooltip_text.clone()))
                        .build(window, cx)
                })
                .when_some(link_url, |this, url| {
                    this.on_click(move |_, window, cx| {
                        window.end_text_selection(cx);
                        cx.stop_propagation();
                        if url.starts_with("http://") || url.starts_with("https://") {
                            cx.open_url(&url);
                        } else {
                            let repo_root = std::env::current_dir().unwrap_or_default();
                            let candidate = repo_root.join(url.trim_start_matches("./"));
                            if candidate.exists() {
                                cx.open_url(&format!("file://{}", candidate.display()));
                            } else {
                                cx.open_url(&url);
                            }
                        }
                    })
                })
                .when(!badge.label.is_empty(), |this| {
                    this.child(
                        div()
                            .px_2()
                            .py(px(2.0))
                            .bg(crate::theme::bg_surface())
                            .text_color(crate::theme::text())
                            .child(badge.label.clone()),
                    )
                })
                .child(
                    div()
                        .px_2()
                        .py(px(2.0))
                        .bg(bg_color)
                        .text_color(text_color)
                        .child(badge.status.clone()),
                );

            row = row.child(pill);
        }

        row.into_any_element()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageBlockData {
    pub url: String,
    pub alt: Option<String>,
    pub title: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

/// Parses HTML `<img>` elements into structured image data.
pub fn parse_html_img(html: &str) -> Option<ImageBlockData> {
    let trimmed = html.trim();
    if !trimmed.contains("<img") {
        return None;
    }
    let extract_attr = |attr_name: &str| -> Option<String> {
        let pattern = format!("{attr_name}=\"");
        if let Some(start) = trimmed.find(&pattern) {
            let after = &trimmed[start + pattern.len()..];
            if let Some(end) = after.find('"') {
                return Some(after[..end].to_string());
            }
        }
        let pattern_single = format!("{attr_name}='");
        if let Some(start) = trimmed.find(&pattern_single) {
            let after = &trimmed[start + pattern_single.len()..];
            if let Some(end) = after.find('\'') {
                return Some(after[..end].to_string());
            }
        }
        None
    };

    let src = extract_attr("src")?;
    let alt = extract_attr("alt");
    let title = extract_attr("title");
    let width = extract_attr("width").and_then(|w| w.parse::<f32>().ok());
    let height = extract_attr("height").and_then(|h| h.parse::<f32>().ok());

    Some(ImageBlockData {
        url: src,
        alt,
        title,
        width,
        height,
    })
}

/// Resolves an image path to an absolute path on disk if it exists.
pub fn resolve_image_path(url_str: &str, repo_root: &std::path::Path) -> Option<std::path::PathBuf> {
    let cleaned = if let Some(stripped) = url_str.strip_prefix("file://") {
        stripped
    } else {
        url_str
    };

    let path = std::path::Path::new(cleaned);
    if path.is_absolute() && path.is_file() {
        return Some(path.to_path_buf());
    }

    let candidate = repo_root.join(cleaned.trim_start_matches("./"));
    if candidate.is_file() {
        return Some(candidate);
    }

    if let Ok(canon) = candidate.canonicalize() {
        if canon.is_file() {
            return Some(canon);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_candidate = cwd.join(cleaned.trim_start_matches("./"));
        if cwd_candidate.is_file() {
            return Some(cwd_candidate);
        }
    }

    let fallback_root = std::path::Path::new("/home/Jake/dev/hadron");
    let fallback_candidate = fallback_root.join(cleaned.trim_start_matches("./"));
    if fallback_candidate.is_file() {
        return Some(fallback_candidate);
    }

    None
}

/// A MarkdownPlugin that renders image blocks and HTML `<img>` elements with native GPUI image rasterization.
#[cfg(feature = "gui")]
#[derive(Clone, Default)]
pub struct ImagePlugin;

#[cfg(feature = "gui")]
impl MarkdownPlugin for ImagePlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "image-card"
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        match node {
            markdown_ast::Node::Image(img) => {
                if parse_shield_badge(&img.url).is_none() {
                    return Some(
                        MarkdownNode::new("image-card", ImageBlockData {
                            url: img.url.clone(),
                            alt: if img.alt.is_empty() { None } else { Some(img.alt.clone()) },
                            title: img.title.clone(),
                            width: None,
                            height: None,
                        })
                        .markdown(cx.node_source(node).unwrap_or_default()),
                    );
                }
            }
            markdown_ast::Node::Paragraph(p) => {
                if p.children.len() == 1 {
                    if let markdown_ast::Node::Image(img) = &p.children[0] {
                        if parse_shield_badge(&img.url).is_none() {
                            return Some(
                                MarkdownNode::new("image-card", ImageBlockData {
                                    url: img.url.clone(),
                                    alt: if img.alt.is_empty() { None } else { Some(img.alt.clone()) },
                                    title: img.title.clone(),
                                    width: None,
                                    height: None,
                                })
                                .markdown(cx.node_source(node).unwrap_or_default()),
                            );
                        }
                    }
                }
            }
            markdown_ast::Node::Html(html) => {
                if let Some(data) = parse_html_img(&html.value) {
                    if parse_shield_badge(&data.url).is_none() {
                        return Some(
                            MarkdownNode::new("image-card", data)
                                .markdown(cx.node_source(node).unwrap_or_default()),
                        );
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Some(data) = node.data::<ImageBlockData>() else {
            return div().into_any_element();
        };

        let repo_root = std::env::current_dir().unwrap_or_default();
        let resolved = resolve_image_path(&data.url, &repo_root);

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.url.hash(&mut hasher);
        let hash = hasher.finish();

        if let Some(abs_path) = resolved {
            let mut container = gpui_component::v_flex()
                .id(ElementId::NamedInteger("markdown-img-card".into(), hash))
                .my_3()
                .p_2()
                .rounded_lg()
                .bg(crate::theme::bg_surface())
                .border_1()
                .border_color(crate::theme::border())
                .items_center();

            let mut img_elem = gpui::img(abs_path)
                .id("img-asset")
                .max_w_full()
                .rounded_md()
                .object_fit(gpui::ObjectFit::Contain);

            if let Some(w) = data.width {
                img_elem = img_elem.max_w(px(w));
            }

            container = container.child(img_elem);

            if let Some(alt) = &data.alt {
                if !alt.is_empty() {
                    container = container.child(
                        div()
                            .mt_2()
                            .text_xs()
                            .text_color(crate::theme::text_secondary())
                            .italic()
                            .child(alt.clone()),
                    );
                }
            }

            container.into_any_element()
        } else {
            gpui_component::v_flex()
                .id(ElementId::NamedInteger("markdown-img-fallback".into(), hash))
                .my_2()
                .p_3()
                .rounded_md()
                .bg(crate::theme::bg_surface())
                .border_1()
                .border_color(crate::theme::border())
                .gap_1()
                .child(
                    gpui_component::h_flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_sm().child("🖼️"))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(crate::theme::text())
                                .child(data.alt.clone().unwrap_or_else(|| "Image".to_string())),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::text_muted())
                        .truncate()
                        .child(data.url.clone()),
                )
                .into_any_element()
        }
    }
}

/// Returns the standard MarkdownExtensions registry configured with Mermaid, Badge pills, and Image rendering.
#[cfg(feature = "gui")]
pub fn chamber_markdown_extensions() -> MarkdownExtensions {
    MarkdownExtensions::default()
        .plugin(MermaidPlugin)
        .plugin(BadgePlugin)
        .plugin(ImagePlugin)
}

