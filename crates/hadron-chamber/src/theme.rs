//! The chamber's color system — Jake's palette mapped to gpui colors. A near-black
//! layered-surface dark theme with a pink→purple energy accent. Discipline: the
//! accent is for active/interactive only; surfaces carry the space.
//!
//! The full palette is exposed even where not yet consumed (input focus, selected
//! states land next), so callers reach for the named token, not a raw hex.
#![allow(dead_code)]

use gpui::{linear_color_stop, linear_gradient, rgb, rgba, Background, Rgba};

use hadron_lattice::QuarkState;

// --- surfaces (darkest → raised) --- Zinc palette for tiered elevation.
pub fn bg_base() -> Rgba {
    rgb(0x09090b) // zinc-950 - main workspace background
}

/// The window's background: `bg_elevated` with a faint top-down glint — a hair
/// lighter at the very top, settling to the flat base by ~a third down. One home
/// for the gradient so every surface that wants the same depth reads from here.
pub fn window_glint() -> Background {
    linear_gradient(
        180.0,
        linear_color_stop(rgb(0x202024), 0.0),
        linear_color_stop(bg_elevated(), 0.34),
    )
}
pub fn bg_elevated() -> Rgba {
    rgb(0x18181b) // zinc-900 - sidebars and tabs
}

/// The fill for the two darkest layers (chat + right rail cards): a faint
/// top-down sheen over the near-black base, so the surface reads as a lit glass
/// panel instead of a flat black rectangle. Pairs with [`glass_highlight`] on the
/// top edge. GPUI has no backdrop blur, so depth is faked with layered tone.
pub fn glass_surface() -> Background {
    linear_gradient(
        180.0,
        linear_color_stop(rgba(0x09090b40), 0.0), // 25% opacity top sheen
        linear_color_stop(rgba(0x09090b73), 1.0), // 45% opacity base
    )
}

/// Hairline top-edge highlight for the glass layers — white at ~8%, the light
/// catching the lip of the panel.
pub fn glass_highlight() -> Rgba {
    rgba(0xffffff14)
}

// --- terminal (a Zed-like screen) ---
/// The terminal screen surface — a touch off pure black so text has contrast.
pub fn term_bg() -> Rgba {
    rgb(0x0c0c0e)
}
/// Default terminal output foreground — a soft off-white, brighter than muted
/// body text so command output reads like a real console.
pub fn term_fg() -> Rgba {
    rgb(0xd0d3d8)
}
/// The shell prompt (`user@host: cwd$`) — the classic terminal green.
pub fn term_prompt() -> Rgba {
    rgb(0x4ade80)
}
pub fn bg_surface() -> Rgba {
    rgb(0x27272a) // zinc-800 - modals, cards, chips
}
pub fn bg_surface_raised() -> Rgba {
    rgb(0x3f3f46) // zinc-700 - hover / active
}
pub fn input_bg() -> Rgba {
    rgb(0x18181b) // zinc-900
}
pub fn popover() -> Rgba {
    rgb(0x27272a) // zinc-800 - context menus
}
pub fn border() -> Rgba {
    rgb(0x3f3f46) // zinc-700
}

// --- text tiers ---
pub fn text() -> Rgba {
    rgb(0xd4d4d8) // soft off-white (zinc-300)
}
pub fn text_secondary() -> Rgba {
    rgba(0xd4d4d8c2) // 0.76
}
pub fn text_muted() -> Rgba {
    rgba(0xd4d4d894) // 0.58
}

// --- accents (the energy gradient) ---
pub fn accent() -> Rgba {
    rgb(0xec4899) // pink — active / addressed
}
pub fn accent_secondary() -> Rgba {
    rgb(0xa855f7) // purple — thinking
}
pub fn danger() -> Rgba {
    rgb(0xef4444) // red — close-button hover
}

/// Roster chip color for a quark's lifecycle state (the status ramp).
pub fn quark_state(state: QuarkState) -> Rgba {
    match state {
        QuarkState::Ground => rgb(0x9ca2ad),   // neutral grey
        QuarkState::Excited => rgb(0xec4899),  // pink — active
        QuarkState::Thinking => rgb(0xa855f7), // purple
        QuarkState::Waiting => rgb(0xfbbf24),  // amber
        QuarkState::Blocked | QuarkState::Error => rgb(0xf87171), // red
    }
}

/// Presence-dot color for the roster user-list — Jake's traffic-light semantics
/// (green available · blue working · amber waiting · red unavailable), distinct
/// from the [`quark_state`] accent ramp used elsewhere.
pub fn presence(state: QuarkState) -> Rgba {
    match state {
        QuarkState::Ground => rgb(0x22c55e), // green — available
        QuarkState::Excited | QuarkState::Thinking => rgb(0x3b82f6), // blue — working
        QuarkState::Waiting => rgb(0xf59e0b), // amber — waiting on a decision
        QuarkState::Blocked | QuarkState::Error => rgb(0xef4444), // red — unavailable
    }
}

pub fn presence_disabled() -> Rgba {
    rgb(0x71717a) // zinc-500 gray
}

/// One-word presence label matching [`presence`], for tooltips/subtitles.
pub fn presence_label(state: QuarkState) -> &'static str {
    match state {
        QuarkState::Ground => "available",
        QuarkState::Excited | QuarkState::Thinking => "working",
        QuarkState::Waiting => "waiting",
        QuarkState::Blocked | QuarkState::Error => "unavailable",
    }
}

/// A stable hue for a chat author's header, so who-said-what scans at a glance.
/// Human/gluon are fixed; quarks cycle through the chart palette by name.
pub fn actor_hue(actor: &str) -> Rgba {
    match actor {
        "human" => rgb(0xf5f5f6), // bright — the human
        "gluon" => rgb(0x60a5fa), // info blue — the system
        other => {
            const CHART: [u32; 6] = [0x38bdf8, 0x34d399, 0xa78bfa, 0xfbbf24, 0xfb7185, 0x94a3b8];
            let idx =
                (other.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32)) as usize) % CHART.len();
            rgb(CHART[idx])
        }
    }
}
