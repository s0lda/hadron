//! GPUI rendering components for interactive Mermaid diagram cards.

#[cfg(feature = "gui")]
use super::ast::*;
#[cfg(feature = "gui")]
use super::layout::*;
#[cfg(feature = "gui")]
use gpui::*;
#[cfg(feature = "gui")]
use gpui_component::scroll::ScrollableElement as _;
#[cfg(feature = "gui")]
use gpui_component::{h_flex, v_flex};
#[cfg(feature = "gui")]
use std::hash::{Hash, Hasher};


#[cfg(feature = "gui")]
const DIAGRAM_PALETTE: [u32; 8] = [
    0x60a5fa, // Blue
    0x34d399, // Emerald
    0xf59e0b, // Amber
    0xc084fc, // Purple
    0xf87171, // Rose
    0x2dd4bf, // Teal
    0x38bdf8, // Sky
    0xa78bfa, // Violet
];

/// Interactive Mermaid Diagram Card Component
#[cfg(feature = "gui")]
#[derive(IntoElement)]
pub struct MermaidCard {
    pub source: SharedString,
    pub diagram: Result<MermaidDiagram, String>,
}

#[cfg(feature = "gui")]
impl MermaidCard {
    pub fn new(source: impl Into<SharedString>) -> Self {
        let source_str = source.into();
        let parsed = super::parser::parse_mermaid(&source_str);
        Self {
            source: source_str,
            diagram: parsed,
        }
    }
}

#[cfg(feature = "gui")]
impl RenderOnce for MermaidCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let source = self.source.clone();
        let copy_source = self.source.clone();

        // Unique state key for view mode toggle (diagram vs raw code)
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.source.hash(&mut hasher);
        let hash = hasher.finish();
        let toggle_key = format!("mermaid-toggle-{hash}");
        let is_code_mode_entity = window
            .use_keyed_state(SharedString::from(toggle_key), cx, |_, _| false);
        let is_code_mode = *is_code_mode_entity.read(cx);
        let toggle_state = is_code_mode_entity.clone();



        let (title, metrics, layout_res) = match &self.diagram {
            Ok(diag) => (
                diag.title().to_string(),
                diag.metrics_summary(),
                Some(compute_layout(diag)),
            ),
            Err(err) => (
                "Mermaid (Syntax Error)".to_string(),
                err.clone(),
                None,
            ),
        };

        let is_error = self.diagram.is_err();

        v_flex()
            .w_full()
            .my_2()
            .rounded_lg()
            .bg(crate::theme::input_bg())
            .border_1()
            .border_color(crate::theme::border())
            .overflow_hidden()
            .child(
                // Header Bar
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .bg(crate::theme::bg_surface())
                    .border_b_1()
                    .border_color(crate::theme::border())
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(if is_error {
                                        gpui::rgba(0xef444420)
                                    } else {
                                        gpui::rgba(0x3b82f620)
                                    })
                                    .text_color(if is_error {
                                        gpui::rgb(0xef4444)
                                    } else {
                                        gpui::rgb(0x60a5fa)
                                    })
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(crate::theme::text_muted())
                                    .font_family(crate::fonts::MONO_FAMILY)
                                    .child(metrics),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                // Toggle Diagram / Code View
                                div()
                                    .id(SharedString::from(format!("toggle-{hash}")))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(if is_code_mode {
                                        crate::theme::bg_surface_raised()
                                    } else {
                                        gpui::rgba(0x00000000)
                                    })
                                    .border_1()
                                    .border_color(if is_code_mode {
                                        crate::theme::border()
                                    } else {
                                        gpui::rgba(0x00000000)
                                    })
                                    .text_xs()
                                    .text_color(if is_code_mode {
                                        crate::theme::text()
                                    } else {
                                        crate::theme::text_muted()
                                    })
                                    .cursor_pointer()
                                    .hover(|s| s.bg(crate::theme::bg_surface_raised()).text_color(crate::theme::text()))
                                    .child(if is_code_mode { "Diagram View" } else { "Source Code" })
                                    .on_click(move |_, _window, cx| {
                                        toggle_state.update(cx, |val, cx| {
                                            *val = !*val;
                                            cx.notify();
                                        });
                                    }),

                            )
                            .child(
                                // Copy Code Button
                                div()
                                    .id(SharedString::from(format!("copy-{hash}")))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .text_color(crate::theme::text_muted())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(crate::theme::bg_surface_raised()).text_color(crate::theme::text()))
                                    .child("Copy")
                                    .on_click(move |_, _window, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_source.to_string()));
                                    }),
                            ),
                    ),
            )
            .child(
                // Body View (Visual Diagram or Code)
                if is_code_mode || is_error || layout_res.is_none() {
                    // Code Block / Error fallback view
                    v_flex()
                        .w_full()
                        .p_3()
                        .bg(crate::theme::field_base())
                        .font_family(crate::fonts::MONO_FAMILY)
                        .text_xs()
                        .text_color(crate::theme::text())
                        .child(
                            div()
                                .overflow_x_scrollbar()
                                .child(source.to_string()),
                        )
                        .into_any_element()
                } else {
                    // Visual Interactive Diagram
                    match layout_res.unwrap() {
                        LayoutResult::Flowchart(f) => render_flowchart_canvas(f).into_any_element(),
                        LayoutResult::Sequence(s) => render_sequence_canvas(s).into_any_element(),
                        LayoutResult::Pie(p) => render_pie_canvas(p).into_any_element(),
                        LayoutResult::Raw { diagram_type, source } => v_flex()
                            .p_3()
                            .gap_2()
                            .font_family(crate::fonts::MONO_FAMILY)
                            .text_xs()
                            .child(
                                div()
                                    .text_color(crate::theme::text_muted())
                                    .child(format!("Diagram: {diagram_type}")),
                            )
                            .child(
                                div()
                                    .overflow_x_scrollbar()
                                    .child(source),
                            )
                            .into_any_element(),

                    }
                },
            )
    }
}

#[cfg(feature = "gui")]
fn render_flowchart_canvas(layout: FlowchartLayout) -> impl IntoElement {
    let canvas_w = layout.canvas_width.max(300.0);
    let canvas_h = layout.canvas_height.max(120.0);
    let edges = layout.edges.clone();

    div()
        .w_full()
        .max_h(px(480.0))
        .overflow_scrollbar()
        .bg(crate::theme::field_base())
        .child(
            div()
                .relative()
                .w(px(canvas_w))
                .h(px(canvas_h))
                .flex_none()
                .child(
                    // Connector Line Canvas
                    gpui::canvas(
                        move |bounds, _, _| bounds,
                        move |bounds, _, window, _cx| {
                            for edge in &edges {
                                let x1 = bounds.origin.x + px(edge.start.x);
                                let y1 = bounds.origin.y + px(edge.start.y);
                                let x2 = bounds.origin.x + px(edge.end.x);
                                let y2 = bounds.origin.y + px(edge.end.y);
                                let c1x = bounds.origin.x + px(edge.control1.x);
                                let c1y = bounds.origin.y + px(edge.control1.y);
                                let c2x = bounds.origin.x + px(edge.control2.x);
                                let c2y = bounds.origin.y + px(edge.control2.y);

                                let stroke_w = match edge.style {
                                    EdgeStyle::ThickArrow | EdgeStyle::ThickLine => px(2.5),
                                    _ => px(1.5),
                                };

                                let color = match edge.style {
                                    EdgeStyle::ThickArrow | EdgeStyle::ThickLine => gpui::rgb(0x60a5fa),
                                    EdgeStyle::DottedArrow | EdgeStyle::DottedLine => gpui::rgb(0x94a3b8),
                                    _ => gpui::rgb(0x64748b),
                                };

                                let mut builder = gpui::PathBuilder::stroke(stroke_w);
                                builder.move_to(gpui::point(x1, y1));
                                builder.cubic_bezier_to(
                                    gpui::point(x2, y2),
                                    gpui::point(c1x, c1y),
                                    gpui::point(c2x, c2y),
                                );
                                if let Ok(path) = builder.build() {
                                    window.paint_path(path, color);
                                }

                                // Draw arrow marker head if applicable
                                if matches!(
                                    edge.style,
                                    EdgeStyle::SolidArrow | EdgeStyle::DottedArrow | EdgeStyle::ThickArrow
                                ) {
                                    let arrow_r = px(4.0);
                                    let arrow_bounds = gpui::Bounds {
                                        origin: gpui::point(x2 - arrow_r, y2 - arrow_r),
                                        size: gpui::size(arrow_r * 2.0, arrow_r * 2.0),
                                    };
                                    window.paint_quad(gpui::fill(arrow_bounds, color).corner_radii(arrow_r));
                                }
                            }
                        },
                    )
                    .w(px(canvas_w))
                    .h(px(canvas_h)),
                )
                // Subgraph Boundary Containers
                .children(layout.subgraphs.into_iter().map(|sub| {
                    div()
                        .absolute()
                        .left(px(sub.bounds.x))
                        .top(px(sub.bounds.y))
                        .w(px(sub.bounds.width))
                        .h(px(sub.bounds.height))
                        .rounded_lg()
                        .border_1()
                        .border_color(gpui::rgba(0xffffff15))
                        .bg(gpui::rgba(0xffffff05))
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::theme::text_muted())
                                .child(sub.title),
                        )
                }))
                // Positioned Node Tiles
                .children(layout.nodes.into_iter().map(|node| {
                    let (radius, icon_str) = match node.shape {
                        NodeShape::Rounded => (px(12.0), None),
                        NodeShape::Stadium => (px(19.0), None),
                        NodeShape::Circle | NodeShape::DoubleCircle => (px(19.0), None),
                        NodeShape::Diamond => (px(6.0), Some("◆ ")),
                        NodeShape::Cylinder => (px(6.0), Some("🗄 ")),
                        NodeShape::Hexagon => (px(6.0), Some("⬡ ")),
                        _ => (px(6.0), None),
                    };

                    div()
                        .absolute()
                        .left(px(node.bounds.x))
                        .top(px(node.bounds.y))
                        .w(px(node.bounds.width))
                        .h(px(node.bounds.height))
                        .px_3()
                        .rounded(radius)
                        .bg(crate::theme::bg_surface())
                        .border_1()
                        .border_color(crate::theme::border())
                        .shadow_sm()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.border_color(crate::theme::accent()).bg(crate::theme::bg_surface_raised()))
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .justify_center()
                                .size_full()
                                .children(icon_str.map(|ic| {
                                    div()
                                        .text_xs()
                                        .text_color(crate::theme::accent())
                                        .child(ic)
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(crate::theme::text())
                                        .font_family(crate::fonts::MONO_FAMILY)
                                        .overflow_hidden()
                                        .child(node.label),
                                ),
                        )
                }))
                // Edge Label Badges
                .children(layout.edges.into_iter().filter_map(|edge| {
                    edge.label.map(|lbl| {
                        div()
                            .absolute()
                            .left(px(edge.label_pos.x - 30.0))
                            .top(px(edge.label_pos.y - 10.0))
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(crate::theme::bg_surface_raised())
                            .border_1()
                            .border_color(crate::theme::border())
                            .text_xs()
                            .text_color(crate::theme::text_muted())
                            .font_family(crate::fonts::MONO_FAMILY)
                            .child(lbl)
                    })
                })),
        )
}

#[cfg(feature = "gui")]
fn render_sequence_canvas(layout: SequenceLayout) -> impl IntoElement {
    let canvas_w = layout.canvas_width.max(300.0);
    let canvas_h = layout.canvas_height.max(120.0);
    let parts = layout.participants.clone();
    let msgs = layout.messages.clone();

    div()
        .w_full()
        .max_h(px(480.0))
        .overflow_scrollbar()
        .bg(crate::theme::field_base())
        .child(
            div()
                .relative()
                .w(px(canvas_w))
                .h(px(canvas_h))
                .flex_none()
                .child(
                    // Lifelines & Message Arrows Canvas
                    gpui::canvas(
                        move |bounds, _, _| bounds,
                        move |bounds, _, window, _cx| {
                            // Draw vertical dashed lifelines
                            for p in &parts {
                                let lx = bounds.origin.x + px(p.lifeline_x);
                                let y1 = bounds.origin.y + px(p.header_bounds.bottom());
                                let y2 = bounds.origin.y + bounds.size.height - px(16.0);

                                let mut cur_y = y1;
                                while cur_y < y2 {
                                    let segment_h = px(6.0).min(y2 - cur_y);
                                    let line_bounds = gpui::Bounds {
                                        origin: gpui::point(lx - px(0.75), cur_y),
                                        size: gpui::size(px(1.5), segment_h),
                                    };
                                    window.paint_quad(gpui::fill(line_bounds, gpui::rgba(0xffffff20)));
                                    cur_y += px(12.0);
                                }
                            }

                            // Draw horizontal message arrows
                            for msg in &msgs {
                                let x1 = bounds.origin.x + px(msg.start.x);
                                let x2 = bounds.origin.x + px(msg.end.x);
                                let y = bounds.origin.y + px(msg.start.y);
                                let line_color = if msg.is_dotted {
                                    gpui::rgb(0x94a3b8)
                                } else {
                                    gpui::rgb(0x60a5fa)
                                };

                                let min_x = x1.min(x2);
                                let max_x = x1.max(x2);
                                let line_bounds = gpui::Bounds {
                                    origin: gpui::point(min_x, y - px(1.0)),
                                    size: gpui::size(max_x - min_x, px(2.0)),
                                };
                                window.paint_quad(gpui::fill(line_bounds, line_color));

                                // Arrow point
                                let arrow_r = px(4.0);
                                let arrow_bounds = gpui::Bounds {
                                    origin: gpui::point(x2 - arrow_r, y - arrow_r),
                                    size: gpui::size(arrow_r * 2.0, arrow_r * 2.0),
                                };
                                window.paint_quad(gpui::fill(arrow_bounds, line_color).corner_radii(arrow_r));
                            }
                        },
                    )
                    .w(px(canvas_w))
                    .h(px(canvas_h)),
                )
                // Participant Header Cards
                .children(layout.participants.into_iter().map(|p| {
                    div()
                        .absolute()
                        .left(px(p.header_bounds.x))
                        .top(px(p.header_bounds.y))
                        .w(px(p.header_bounds.width))
                        .h(px(p.header_bounds.height))
                        .px_3()
                        .rounded_md()
                        .bg(crate::theme::bg_surface())
                        .border_1()
                        .border_color(crate::theme::border())
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::theme::text())
                                .child(p.label),
                        )
                }))
                // Message Text Labels
                .children(layout.messages.into_iter().map(|msg| {
                    let min_x = msg.start.x.min(msg.end.x);
                    let span_w = (msg.start.x - msg.end.x).abs();
                    div()
                        .absolute()
                        .left(px(min_x))
                        .top(px(msg.start.y - 18.0))
                        .w(px(span_w))
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(crate::theme::bg_surface_raised())
                                .border_1()
                                .border_color(crate::theme::border())
                                .text_xs()
                                .font_family(crate::fonts::MONO_FAMILY)
                                .text_color(crate::theme::text())
                                .child(msg.text),
                        )
                }))
                // Note boxes
                .children(layout.notes.into_iter().map(|note| {
                    div()
                        .absolute()
                        .left(px(note.bounds.x))
                        .top(px(note.bounds.y))
                        .w(px(note.bounds.width))
                        .h(px(note.bounds.height))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(gpui::rgba(0xf59e0b20))
                        .border_1()
                        .border_color(gpui::rgba(0xf59e0b50))
                        .text_xs()
                        .text_color(gpui::rgb(0xfbbf24))
                        .child(note.text)
                })),
        )
}

#[cfg(feature = "gui")]
fn render_pie_canvas(layout: PieLayout) -> impl IntoElement {
    v_flex()
        .w_full()
        .p_4()
        .gap_3()
        .bg(crate::theme::field_base())
        .children(layout.title.map(|t| {
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(crate::theme::text())
                .child(t)
        }))
        // Stacked visual percentage bar
        .child(
            h_flex()
                .w_full()
                .h(px(20.0))
                .rounded_md()
                .overflow_hidden()
                .children(layout.slices.iter().map(|s| {
                    let color = gpui::rgb(DIAGRAM_PALETTE[s.color_index % DIAGRAM_PALETTE.len()]);
                    div()
                        .h_full()
                        .w(gpui::relative(s.percentage as f32 / 100.0))
                        .bg(color)
                })),
        )
        // Legend items with values and percentages
        .child(
            v_flex()
                .gap_1p5()
                .children(layout.slices.into_iter().map(|s| {
                    let color = gpui::rgb(DIAGRAM_PALETTE[s.color_index % DIAGRAM_PALETTE.len()]);
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(
                            div()
                                .size(px(10.0))
                                .rounded_sm()
                                .bg(color),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(crate::theme::text())
                                .child(s.label),
                        )
                        .child(
                            div()
                                .text_color(crate::theme::text_muted())
                                .font_family(crate::fonts::MONO_FAMILY)
                                .child(format!("{:.1}% ({})", s.percentage, s.value)),
                        )
                })),
        )
}

/// Scaling mode for the interactive markdown ImageCard.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageScaleMode {
    Fit,
    Pct50,
    Pct100,
    Pct150,
}

#[cfg(feature = "gui")]
impl ImageScaleMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fit => "Fit",
            Self::Pct50 => "50%",
            Self::Pct100 => "100%",
            Self::Pct150 => "150%",
        }
    }
}

/// Interactive Markdown Image Card with responsive scaling, dimensions metadata, pan/scroll, and external viewer integration.
#[cfg(feature = "gui")]
#[derive(IntoElement)]
pub struct ImageCard {
    pub data: super::plugin::ImageBlockData,
    pub abs_path: std::path::PathBuf,
}

#[cfg(feature = "gui")]
impl ImageCard {
    pub fn new(data: super::plugin::ImageBlockData, abs_path: std::path::PathBuf) -> Self {
        Self { data, abs_path }
    }
}

#[cfg(feature = "gui")]
impl RenderOnce for ImageCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let abs_path = self.abs_path.clone();
        let file_name = abs_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string();

        let (dims, size) = super::plugin::probe_image_meta(&abs_path);

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.data.url.hash(&mut hasher);
        abs_path.hash(&mut hasher);
        let hash = hasher.finish();

        let scale_key = format!("img-scale-{hash}");
        let scale_entity = window.use_keyed_state(SharedString::from(scale_key), cx, |_, _| ImageScaleMode::Fit);
        let scale_mode = *scale_entity.read(cx);

        let (orig_w, orig_h) = dims.unwrap_or_else(|| {
            (
                self.data.width.unwrap_or(900.0) as u32,
                self.data.height.unwrap_or(600.0) as u32,
            )
        });

        let mut card = v_flex()
            .w_full()
            .max_w_full()
            .my_3()
            .rounded_lg()
            .bg(crate::theme::input_bg())
            .border_1()
            .border_color(crate::theme::border())
            .overflow_hidden();

        // 1. Header Toolbar
        let copy_path = abs_path.display().to_string();
        let open_path = abs_path.clone();

        let mut dim_text = format!("{orig_w} × {orig_h} px");
        if let Some(bytes) = size {
            dim_text.push_str(&format!(" · {}", super::plugin::format_bytes(bytes)));
        }

        let header = h_flex()
            .w_full()
            .px_3()
            .py_1p5()
            .items_center()
            .justify_between()
            .bg(crate::theme::bg_surface())
            .border_b_1()
            .border_color(crate::theme::border())
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(gpui::rgba(0x3b82f620))
                            .text_color(gpui::rgb(0x60a5fa))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("🖼️ {}", file_name)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(crate::theme::text_muted())
                            .font_family(crate::fonts::MONO_FAMILY)
                            .child(dim_text),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    // Scale presets: Fit, 50%, 100%, 150%
                    .children([ImageScaleMode::Fit, ImageScaleMode::Pct50, ImageScaleMode::Pct100, ImageScaleMode::Pct150].iter().map(|&mode| {
                        let is_active = scale_mode == mode;
                        let target_mode = mode;
                        let scale_ent = scale_entity.clone();
                        div()
                            .id(SharedString::from(format!("img-scale-btn-{hash}-{:?}", mode)))
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(if is_active {
                                crate::theme::bg_surface_raised()
                            } else {
                                gpui::rgba(0x00000000)
                            })
                            .border_1()
                            .border_color(if is_active {
                                crate::theme::border()
                            } else {
                                gpui::rgba(0x00000000)
                            })
                            .text_xs()
                            .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                            .text_color(if is_active {
                                crate::theme::text()
                            } else {
                                crate::theme::text_muted()
                            })
                            .cursor_pointer()
                            .hover(|s| s.bg(crate::theme::bg_surface_raised()).text_color(crate::theme::text()))
                            .child(mode.label())
                            .on_click(move |_, _window, cx| {
                                scale_ent.update(cx, |val, cx| {
                                    *val = target_mode;
                                    cx.notify();
                                });
                            })
                    }))
                    .child(
                        div()
                            .w(px(1.0))
                            .h(px(12.0))
                            .mx_1()
                            .bg(crate::theme::border()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("img-open-{hash}")))
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .text_xs()
                            .text_color(crate::theme::text_muted())
                            .cursor_pointer()
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new("Open with default image viewer").build(window, cx)
                            })
                            .hover(|s| s.bg(crate::theme::bg_surface_raised()).text_color(crate::theme::text()))
                            .child("Open")
                            .on_click(move |_, _window, cx| {
                                cx.open_url(&format!("file://{}", open_path.display()));
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("img-copy-{hash}")))
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .text_xs()
                            .text_color(crate::theme::text_muted())
                            .cursor_pointer()
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new("Copy absolute path").build(window, cx)
                            })
                            .hover(|s| s.bg(crate::theme::bg_surface_raised()).text_color(crate::theme::text()))
                            .child("Copy")
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_path.clone()));
                            }),
                    ),
            );

        card = card.child(header);

        // 2. Image Viewport Body
        match scale_mode {
            ImageScaleMode::Fit => {
                let zoom_ent = scale_entity.clone();
                let mut img_elem = gpui::img(abs_path)
                    .id("img-asset")
                    .max_w_full()
                    .max_h(px(540.0))
                    .rounded_md()
                    .object_fit(gpui::ObjectFit::Contain);

                if let Some(w) = self.data.width {
                    img_elem = img_elem.max_w(px(w));
                }

                let body = v_flex()
                    .id(SharedString::from(format!("img-fit-body-{hash}")))
                    .w_full()
                    .max_w_full()
                    .p_3()
                    .items_center()
                    .justify_center()
                    .bg(crate::theme::field_base())
                    .cursor_pointer()
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Click to zoom to 100%").build(window, cx)
                    })
                    .on_click(move |_, _window, cx| {
                        zoom_ent.update(cx, |val, cx| {
                            *val = ImageScaleMode::Pct100;
                            cx.notify();
                        });
                    })
                    .child(img_elem);

                card = card.child(body);
            }
            ImageScaleMode::Pct50 | ImageScaleMode::Pct100 | ImageScaleMode::Pct150 => {
                let factor = match scale_mode {
                    ImageScaleMode::Fit => 1.0,
                    ImageScaleMode::Pct50 => 0.5,
                    ImageScaleMode::Pct100 => 1.0,
                    ImageScaleMode::Pct150 => 1.5,
                };
                let target_w = (orig_w as f32 * factor).max(60.0);
                let target_h = (orig_h as f32 * factor).max(40.0);

                let img_elem = gpui::img(abs_path)
                    .id("img-asset")
                    .w(px(target_w))
                    .h(px(target_h))
                    .rounded_md()
                    .object_fit(gpui::ObjectFit::Contain);

                let body = div()
                    .id(SharedString::from(format!("img-scroll-{hash}")))
                    .w_full()
                    .max_h(px(580.0))
                    .overflow_scrollbar()
                    .p_3()
                    .bg(crate::theme::field_base())
                    .child(
                        div()
                            .w(px(target_w))
                            .h(px(target_h))
                            .child(img_elem),
                    );

                card = card.child(body);
            }
        }

        // 3. Caption / Alt Text Footer
        if let Some(caption) = self.data.alt.as_ref().or(self.data.title.as_ref()) {
            if !caption.is_empty() {
                card = card.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .border_t_1()
                        .border_color(crate::theme::border())
                        .bg(crate::theme::bg_surface())
                        .text_xs()
                        .text_color(crate::theme::text_secondary())
                        .italic()
                        .child(caption.clone()),
                );
            }
        }

        card
    }
}

