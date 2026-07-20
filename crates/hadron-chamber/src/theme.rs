//! The chamber's color system — **white frosted glass floating on a black field**.
//! The housing is near-black; instrument panels are white at low alpha (so they frost to a
//! subtle dark glass rather than turning opaque-white), edged with a clean white sheen. The
//! quark-state hues (blue = working, purple = thinking, green = available, amber = waiting)
//! survive only as a whisper in the corners, so the field reads black but still has life.
//!
//! **What translucency is allowed — and what is NOT.** Under WSL the app software-renders
//! (llvmpipe, no GPU), so a repaint is expensive; the discipline is to keep repaints RARE,
//! not to ban translucency. Two things are the true CPU killers and are forbidden here:
//!   1. **Continuous animation** (a live `.with_animation`/repeating loop) — GPUI re-renders
//!      the whole window every frame it runs. There is none.
//!   2. **Blur** (backdrop-blur, blurred drop-shadows) — a large-kernel per-frame resample.
//!      There is none; "glass" here is alpha + a hairline sheen, never a blur of what's behind.
//! With those gone, repaints only happen on real change (or a throttled ~10fps while the
//! terminal streams), so alpha layers and static linear-gradient washes are affordable. GPUI
//! has no radial gradient, so the field is built from *layered* linear washes, not orbs.
//!
//! The full palette is exposed even where not yet consumed, so callers reach for the
//! named token, not a raw hex.
#![allow(dead_code)]

use gpui::{rgb, rgba, Rgba};

use hadron_lattice::QuarkState;

// --- the ambient field: a flat black housing (the frosted-glass-on-black look) ---
/// The near-black base — the opaque tone painted behind the rounded corners and the dark
/// end of the field wash. Must NOT be translucent (it is the window fill; translucency
/// here would show the desktop, not the field). Just off pure black so the rounded corners
/// and any panel seam still read against it.
pub fn field_base() -> Rgba {
    rgb(0x101010) // flat near-black — the field the frosted panels float on
}
/// The top of the field wash — a barely-lifted near-black, so the housing is a whisper
/// lighter at the top than behind the panels rather than a bright glow.
pub fn field_bright() -> Rgba {
    rgb(0x141417)
}
/// The near-black the wash settles into at the bottom / behind the panels.
pub fn field_deep() -> Rgba {
    rgb(0x08080a)
}

/// The quark-state hues, kept as a faint corner whisper — the same palette the presence
/// dots use, so the black field still carries the colours of the swarm's states, but at a
/// low enough alpha that the backdrop reads black, not as an aurora. Each is anchored to
/// one corner (see `app.rs`).
pub fn glow_blue() -> Rgba {
    rgba(0x4f83f01c) // working / excited — top-left
}
pub fn glow_pink() -> Rgba {
    rgba(0xb85cf018) // thinking — top-right
}
pub fn glow_green() -> Rgba {
    rgba(0x2fcf8a1a) // available — bottom-left
}

// --- surfaces (recessed → raised) --- white frosted glass over the black field.
/// Recessed inner surface (deepest wells). The least white in the ladder, so the black
/// field shows through most and it reads as the deepest well.
pub fn bg_base() -> Rgba {
    rgba(0xffffff08) // ~0.03 white over black
}

/// The window/content backdrop token — the opaque housing behind the whole scene. (Kept
/// under the old name so the one call site that sets the root fill still reads a token.)
pub fn window_glint() -> Rgba {
    field_base()
}
/// A step-lighter smoked tone for lifted chrome: the titlebar/status bars, tab strips, and
/// the small inner cards (message chips, the changed-files card).
pub fn bg_elevated() -> Rgba {
    rgba(0xffffff1a) // ~0.10 white — lifted chrome, the brightest frost in the ladder
}

/// The fill for the raised panels (chat + right rail cards): **white frosted glass** — a
/// low-alpha white that frosts the black field to a subtle dark glass (it does NOT go
/// opaque-white), seating the light text cleanly while catching the [`glass_highlight`] rim.
pub fn glass_surface() -> Rgba {
    rgba(0xffffff12) // ~0.07 white over black
}

/// The clean white rim around a glass panel — a low-alpha white that reads as the lit edge
/// of glass, not a hard seam.
pub fn glass_highlight() -> Rgba {
    rgba(0xffffff33) // ~0.20 white
}

/// The fill for a **focused modal** (Settings card, quark info panel) — opaque, NOT glass.
/// A modal the human is reading needs the field to stop dead behind it; a low-alpha glass
/// surface let the corner glows bleed through and washed out the text. Neutral near-black
/// so it reads as a raised instrument panel in the same zinc family as the titlebar and
/// cards — not the blue-violet holdover from the old bright-field palette that made
/// Settings clash with everything around it. One token so every modal matches and none can
/// drift back to transparent.
pub fn modal_surface() -> Rgba {
    rgb(0x161619) // a hair above the field base, neutral — a raised dark panel
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
    field_base() // flat #101010 field color for context menus (Jake's request)
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

/// The auto-assignment palette: distinct, legible hues a quark cycles through by name
/// when it has no custom colour. Exposed so a colour picker can offer them as presets.
pub const AUTO_PALETTE: [u32; 12] = [
    0x38bdf8, // sky
    0x34d399, // emerald
    0xa78bfa, // violet
    0xfbbf24, // amber
    0xfb7185, // rose
    0x94a3b8, // slate
    0x22d3ee, // cyan
    0x4ade80, // green
    0xfb923c, // orange
    0x818cf8, // indigo
    0xe879f9, // fuchsia
    0xf472b6, // pink
];

/// A stable hue for a chat author's header, so who-said-what scans at a glance.
/// Human/gluon are fixed; quarks cycle through [`AUTO_PALETTE`] by name. This is the
/// **fallback** — a quark with a custom colour resolves to that instead (see
/// `ChamberView`/`Chamber::color_for`).
pub fn actor_hue(actor: &str) -> Rgba {
    match actor {
        "human" => rgb(0xf5f5f6), // bright — the human
        "gluon" => rgb(0x60a5fa), // info blue — the system
        other => {
            let idx = (other.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32)) as usize)
                % AUTO_PALETTE.len();
            rgb(AUTO_PALETTE[idx])
        }
    }
}
