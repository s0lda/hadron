//! The chamber's client-side window frame: a **transparent window with rounded, bordered
//! corners** (Zed's Linux approach, which works on WSLg). The window is transparent so the
//! rounded corner cut-outs composite over the desktop; the frame div paints the opaque
//! housing ([`crate::theme::field_base`]) inside the arc, with a 1px hairline border.
//!
//! This was briefly flat-and-square to chase a CPU spike — but the spike was never the
//! rounded/transparent frame; it was *continuous repaints* (a pulsing animation + a 30fps
//! terminal pump) re-rastering the whole window in software. With those fixed, repaints are
//! rare, so the transparent rounded frame is affordable again. The one thing NOT restored
//! is the blurred drop-shadow: a blur is a large-kernel per-frame resample and stays banned
//! under llvmpipe. Depth comes from the border + the theme's glass tones instead.
//!
//! `window_frame` must wrap the window's outermost content: it owns the resize-edge
//! hit testing (`resize_edge` is copied verbatim from gpui-component's `window_border`
//! so drag-to-resize behaves identically).

use gpui::{
    div, prelude::FluentBuilder as _, px, App, Decorations, Edges, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Point, ResizeEdge, Size, Styled as _,
    Tiling, Window,
};
use gpui_component::ActiveTheme as _;

const BORDER_SIZE: Pixels = px(1.0);
/// Corner radius of the window housing. Public so the titlebar rounds its top corners to
/// match (else the close-button hover spills past the rounded frame edge).
pub const FRAME_RADIUS: Pixels = px(13.0);
/// Half-width of the resize hit band on each side of the window edge.
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

/// Wrap `content` in the opaque, flat, square window frame with a 1px hairline border.
/// Must be the window's outermost element.
pub fn window_frame(window: &mut Window, cx: &App, content: impl IntoElement) -> impl IntoElement {
    let decorations = window.window_decorations();
    let border_color = cx.theme().window_border;
    // The opaque housing painted inside the rounded arc. The window itself is transparent
    // (see app.rs `window_background`), so the corner triangles outside the arc show the
    // desktop — that is what makes the corners read as rounded.
    let bg = crate::theme::field_base();

    match decorations {
        // Server-side decorations (a real title bar): no custom frame.
        Decorations::Server => div().size_full().bg(bg).child(content).into_any_element(),
        Decorations::Client { tiling } => {
            // No client inset: no shadow margin to reserve, so the housing fills edge to
            // edge and the resize band sits at the very edge.
            window.set_client_inset(px(0.0));

            div()
                .id("window-frame")
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
                // Rounded housing corners (square on any tiled edge, e.g. snapped/maximized).
                .when(!tiling.top && !tiling.left, |d| d.rounded_tl(FRAME_RADIUS))
                .when(!tiling.top && !tiling.right, |d| d.rounded_tr(FRAME_RADIUS))
                .when(!tiling.bottom && !tiling.left, |d| d.rounded_bl(FRAME_RADIUS))
                .when(!tiling.bottom && !tiling.right, |d| d.rounded_br(FRAME_RADIUS))
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
                    let insets = frame_insets(px(0.0), &tiling);
                    if let Some(edge) = resize_edge(pos, size, insets, &tiling, RESIZE_HIT) {
                        window.start_window_resize(edge);
                    }
                })
                .child(content)
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
