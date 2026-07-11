//! The chamber's client-side window frame: a transparent window with a
//! `client_inset` drop-shadow margin and rounded, bordered content — the
//! approach Zed uses on Linux (and which works on WSLg, verified). Adapted from
//! gpui-component's `window_border`, which pins the corner radius to `0`; here we
//! round all four corners at [`FRAME_RADIUS`] and keep a themeable 1px border.
//!
//! `window_frame` must wrap the window's outermost content: it sets the client
//! inset and owns the resize-edge hit testing (`resize_edge` is copied verbatim
//! from the reference so drag-to-resize behaves identically).

use gpui::{
    div, point, prelude::FluentBuilder as _, px, App, BoxShadow, Decorations, Edges, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point,
    ResizeEdge, Size, Styled as _, Tiling, Window,
};
use gpui_component::ActiveTheme as _;

/// Transparent margin around the frame where the drop shadow is drawn.
const SHADOW_SIZE: Pixels = px(12.0);
const BORDER_SIZE: Pixels = px(1.0);
/// The corner radius the upstream frame hardcodes to zero. Public so the
/// titlebar can round its own top corners to match (else the close-button hover
/// spills past the rounded frame edge).
pub const FRAME_RADIUS: Pixels = px(10.0);
/// Half-width of the resize hit band on each side of the visible frame edge.
const RESIZE_HIT: Pixels = px(6.0);

/// Per-side inset of the visible frame from the outer window bounds (no inset on
/// a tiled edge, e.g. when snapped/maximized).
fn frame_insets(shadow: Pixels, tiling: &Tiling) -> Edges<Pixels> {
    let mut insets = Edges::all(shadow);
    if tiling.top {
        insets.top = px(0.0);
    }
    if tiling.bottom {
        insets.bottom = px(0.0);
    }
    if tiling.left {
        insets.left = px(0.0);
    }
    if tiling.right {
        insets.right = px(0.0);
    }
    insets
}

/// Wrap `content` in the transparent, shadowed, rounded window frame. Must be the
/// window's outermost element.
pub fn window_frame(window: &mut Window, cx: &App, content: impl IntoElement) -> impl IntoElement {
    let decorations = window.window_decorations();
    let border_color = cx.theme().window_border;
    let bg = cx.theme().background;

    match decorations {
        // Server-side decorations (a real title bar): no custom frame.
        Decorations::Server => div().size_full().bg(bg).child(content).into_any_element(),
        Decorations::Client { tiling } => {
            window.set_client_inset(SHADOW_SIZE);
            let fully_tiled = tiling.top && tiling.bottom && tiling.left && tiling.right;
            let shadow = if fully_tiled { px(0.0) } else { SHADOW_SIZE };

            div()
                .id("window-frame")
                .size_full()
                .bg(gpui::transparent_black())
                // The transparent inset where the shadow is cast.
                .when(!tiling.top, |d| d.pt(shadow))
                .when(!tiling.bottom, |d| d.pb(shadow))
                .when(!tiling.left, |d| d.pl(shadow))
                .when(!tiling.right, |d| d.pr(shadow))
                // Start a window resize when the press lands in an edge band.
                .on_mouse_down(MouseButton::Left, move |_, window, _| {
                    let Decorations::Client { tiling } = window.window_decorations() else {
                        return;
                    };
                    if tiling.top && tiling.bottom && tiling.left && tiling.right {
                        return;
                    }
                    let size = window.window_bounds().get_bounds().size;
                    let pos = window.mouse_position();
                    let insets = frame_insets(SHADOW_SIZE, &tiling);
                    if let Some(edge) = resize_edge(pos, size, insets, &tiling, RESIZE_HIT) {
                        window.start_window_resize(edge);
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .size_full()
                        .min_h_0()
                        .min_w_0()
                        .overflow_hidden()
                        .bg(bg)
                        .border_color(border_color)
                        .when(!tiling.top, |d| d.border_t(BORDER_SIZE))
                        .when(!tiling.bottom, |d| d.border_b(BORDER_SIZE))
                        .when(!tiling.left, |d| d.border_l(BORDER_SIZE))
                        .when(!tiling.right, |d| d.border_r(BORDER_SIZE))
                        .when(!tiling.top && !tiling.left, |d| d.rounded_tl(FRAME_RADIUS))
                        .when(!tiling.top && !tiling.right, |d| d.rounded_tr(FRAME_RADIUS))
                        .when(!tiling.bottom && !tiling.left, |d| {
                            d.rounded_bl(FRAME_RADIUS)
                        })
                        .when(!tiling.bottom && !tiling.right, |d| {
                            d.rounded_br(FRAME_RADIUS)
                        })
                        .when(!fully_tiled, |d| {
                            d.shadow(vec![BoxShadow {
                                color: Hsla {
                                    h: 0.0,
                                    s: 0.0,
                                    l: 0.0,
                                    a: 0.35,
                                },
                                blur_radius: shadow / 2.0,
                                spread_radius: px(0.0),
                                offset: point(px(0.0), px(0.0)),
                                inset: false,
                            }])
                        })
                        .child(content),
                )
                .into_any_element()
        }
    }
}

/// Which resize edge (if any) the pointer is over. Copied verbatim from
/// gpui-component's `window_border` so drag-to-resize matches the upstream frame.
fn resize_edge(
    pos: Point<Pixels>,
    size: Size<Pixels>,
    insets: Edges<Pixels>,
    tiling: &Tiling,
    hit_size: Pixels,
) -> Option<ResizeEdge> {
    let inner_left = insets.left;
    let inner_right = size.width - insets.right;
    let inner_top = insets.top;
    let inner_bottom = size.height - insets.bottom;

    let on_left = pos.x >= inner_left - hit_size
        && pos.x <= inner_left + hit_size
        && pos.y >= inner_top - hit_size
        && pos.y <= inner_bottom + hit_size;
    let on_right = pos.x >= inner_right - hit_size
        && pos.x <= inner_right + hit_size
        && pos.y >= inner_top - hit_size
        && pos.y <= inner_bottom + hit_size;
    let on_top = pos.y >= inner_top - hit_size
        && pos.y <= inner_top + hit_size
        && pos.x >= inner_left - hit_size
        && pos.x <= inner_right + hit_size;
    let on_bottom = pos.y >= inner_bottom - hit_size
        && pos.y <= inner_bottom + hit_size
        && pos.x >= inner_left - hit_size
        && pos.x <= inner_right + hit_size;

    if !tiling.top && !tiling.left && on_top && on_left {
        return Some(ResizeEdge::TopLeft);
    }
    if !tiling.top && !tiling.right && on_top && on_right {
        return Some(ResizeEdge::TopRight);
    }
    if !tiling.bottom && !tiling.left && on_bottom && on_left {
        return Some(ResizeEdge::BottomLeft);
    }
    if !tiling.bottom && !tiling.right && on_bottom && on_right {
        return Some(ResizeEdge::BottomRight);
    }
    if !tiling.top && on_top {
        return Some(ResizeEdge::Top);
    }
    if !tiling.bottom && on_bottom {
        return Some(ResizeEdge::Bottom);
    }
    if !tiling.left && on_left {
        return Some(ResizeEdge::Left);
    }
    if !tiling.right && on_right {
        return Some(ResizeEdge::Right);
    }
    None
}
