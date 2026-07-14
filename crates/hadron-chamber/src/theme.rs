//! The chamber's color system — Jake's palette mapped to gpui colors. A near-black
//! layered-surface dark theme with a pink→purple energy accent. Discipline: the
//! accent is for active/interactive only; surfaces carry the space.
//!
//! The full palette is exposed even where not yet consumed (input focus, selected
//! states land next), so callers reach for the named token, not a raw hex.
#![allow(dead_code)]

use gpui::{rgb, rgba, Rgba};

use hadron_lattice::QuarkState;

// --- surfaces (darkest → raised) --- Jake's greyscale: 0x121314 / 0x191a1b / 0x252627.
pub fn bg() -> Rgba {
    rgb(0x121314) // base canvas
}
pub fn sidebar() -> Rgba {
    rgb(0x191a1b) // rails
}
pub fn surface() -> Rgba {
    rgb(0x202122) // chips/cards on the rails (a step above sidebar so they read)
}
pub fn surface_raised() -> Rgba {
    rgb(0x252627) // hover / active
}
pub fn input_bg() -> Rgba {
    rgb(0x191a1b)
}
pub fn popover() -> Rgba {
    rgb(0x252627) // slightly raised for context menus and popovers
}
pub fn border() -> Rgba {
    rgb(0x303133)
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
