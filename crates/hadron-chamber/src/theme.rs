//! The chamber's color system — **dark glass floating on an ambient quark-state field**.
//! The near-black detector housing is washed with the quark-state hues (blue = working,
//! purple = thinking, green = available, amber = waiting) at low alpha; instrument panels
//! are translucent dark glass over that field, edged with a faint white sheen.
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

// --- the ambient field: a bright blue-violet glow (the "Built"/ChatGPT dark look) ---
/// The deep-violet base — the opaque tone painted behind the rounded corners and the dark
/// end of the field wash. Must NOT be translucent (it is the window fill; translucency
/// here would show the desktop, not the field).
pub fn field_base() -> Rgba {
    rgb(0x1a1740)
}
/// The bright periwinkle highlight — the light top/edge of the glow. The whole appeal of
/// the smoked-glass panels is that they sit over a genuinely BRIGHT field, so this is
/// vivid, not a tint.
pub fn field_bright() -> Rgba {
    rgb(0x9a9ce6)
}
/// The deep violet the wash settles into at the bottom / behind the panels.
pub fn field_deep() -> Rgba {
    rgb(0x141232)
}

/// The quark-state hues, brightened for the corner glows of the field — the same palette
/// the presence dots use, so the backdrop literally glows in the colours of the swarm's
/// states. Translucent so they blend over the bright base into a soft aurora; each is
/// anchored to one corner (see `app.rs`) to stay vivid instead of muddying in the centre.
pub fn glow_blue() -> Rgba {
    rgba(0x4f83f0b6) // working / excited — top-left
}
pub fn glow_pink() -> Rgba {
    rgba(0xb85cf0ac) // thinking — top-right
}
pub fn glow_green() -> Rgba {
    rgba(0x2fcf8ab2) // available — bottom-left
}

// --- surfaces (darkest → raised) --- dark smoked glass over the bright field.
/// Recessed inner surface (deepest wells). A dark smoked tone; the bright field tints
/// through just enough to feel like glass.
pub fn bg_base() -> Rgba {
    rgba(0x1a1834cc) // ~0.80 dark smoked
}

/// The window/content backdrop token — the opaque housing behind the whole scene. (Kept
/// under the old name so the one call site that sets the root fill still reads a token.)
pub fn window_glint() -> Rgba {
    field_base()
}
/// A step-lighter smoked tone for lifted chrome: the titlebar/status bars, tab strips, and
/// the small inner cards (message chips, the changed-files card).
pub fn bg_elevated() -> Rgba {
    rgba(0x272544cc) // ~0.80 lifted smoked glass
}

/// The fill for the raised panels (chat + right rail cards): **dark smoked glass** — a
/// dark, mostly-opaque violet tone that seats text cleanly while the bright field tints
/// through and catches the [`glass_highlight`] border, so it reads as a pane of glass.
pub fn glass_surface() -> Rgba {
    rgba(0x14122ac4) // tinted glass, midway between the earlier light + darker takes (~0.77)
}

/// The subtle light rim around a glass panel — a low-alpha periwinkle that reads as the
/// lit edge of glass catching the field, not a hard seam.
pub fn glass_highlight() -> Rgba {
    rgba(0xccccf53d) // ~0.24 light periwinkle
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
    rgb(0x1f1f25) // solid context-menu surface (opaque — no translucency to re-blend)
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
