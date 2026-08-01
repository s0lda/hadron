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

use gpui::{rgb, rgba, Hsla, Rgba};

use hadron_lattice::QuarkState;

// --- the ambient field: a flat black housing (the frosted-glass-on-black look) ---
/// Layer 0 (Canvas Base): Deep graphite obsidian canvas base fill (`#0C0D10`).
pub fn canvas_base() -> Hsla {
    rgb(0x0c0d10).into()
}

/// The near-black base — the opaque tone painted behind the rounded corners and the dark
/// end of the field wash. Must NOT be translucent (it is the window fill; translucency
/// here would show the desktop, not the field). Just off pure black so the rounded corners
/// and any panel seam still read against it.
pub fn field_base() -> Rgba {
    rgb(0x0d0e12) // flat near-black — the field the frosted panels float on
}
/// The top of the field wash — a barely-lifted near-black, so the housing is a whisper
/// lighter at the top than behind the panels rather than a bright glow.
pub fn field_bright() -> Rgba {
    rgb(0x131419)
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
    rgba(0xc084fc15) // thinking / reasoning — top-right
}
pub fn glow_green() -> Rgba {
    rgba(0x2fcf8a1a) // available — bottom-left
}

// --- surfaces (recessed → raised) --- borderless glass surface hierarchy over cosmic obsidian base.
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

/// Layer 1 (Panels & Rails): Translucent graphite glass containers (`#131519f2`).
pub fn glass_surface() -> Hsla {
    rgba(0x131519f2).into()
}

/// Layer 2 (Floating Cards & Modals): Elevated warm dark charcoal glass cards (`#1A1C22f2`).
pub fn glass_card() -> Hsla {
    rgba(0x1a1c22f2).into()
}

/// Highlights / rims: Crisp 1px highlight rim (`rgba(255, 255, 255, 0.12)`).
pub fn glass_highlight() -> Hsla {
    rgba(0xffffff1f).into()
}

// --- vector status halo indicators ---
/// Active status halo (Soft Sapphire Blue `#60a5fa`) for tool execution / active state.
pub fn halo_active() -> Hsla {
    rgb(0x60a5fa).into()
}

/// Reasoning status halo (Soft Amethyst `#a78bfa`) for reasoning / thinking state.
pub fn halo_reasoning() -> Hsla {
    rgb(0xa78bfa).into()
}

/// Idle status halo (Soft Emerald `#34d399`) for ground / waiting / available state.
pub fn halo_idle() -> Hsla {
    rgb(0x34d399).into()
}

/// Error status halo (Soft Coral `#f87171`) for blocked / error state.
pub fn halo_error() -> Hsla {
    rgb(0xf87171).into()
}

/// Resolves the 8px GPU-native vector status halo indicator color for a given `QuarkState`.
pub fn halo_dot(state: QuarkState) -> Hsla {
    match state {
        QuarkState::Ground => halo_idle(),
        QuarkState::Excited => halo_active(),
        QuarkState::Thinking => halo_reasoning(),
        QuarkState::Waiting => halo_idle(),
        QuarkState::Blocked | QuarkState::Error => halo_error(),
    }
}

/// The fill for a **focused modal** (Settings card, Processes overlay, app menu) — opaque,
/// NOT glass. A modal the human is reading needs the field to stop dead behind it; a
/// low-alpha glass surface let the corner glows bleed through and washed out the text.
/// Anchored to [`field_base`] so every modal reads as the SAME flat near-black as the
/// quark-info and About panels (Jake's request — the old raised `0x161619` tone did not
/// match them). One token so every modal matches and none can drift back to transparent.
pub fn modal_surface() -> Rgba {
    field_base() // flat #101010 — matches the quark-info / About panels
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
    rgb(0xc084fc) // soft amethyst — active / addressed
}
/// A muted, low-alpha amethyst for chrome that should whisper rather than shout —
/// the focused chat-input border.
pub fn accent_soft() -> Rgba {
    rgba(0xc084fc66) // ~0.40 alpha amethyst
}
pub fn accent_secondary() -> Rgba {
    rgb(0xa855f7) // purple — thinking
}
pub fn danger() -> Rgba {
    rgb(0xef4444) // red — close-button hover
}
/// Markdown link colour in chat — light blue (sky), distinct from the amethyst accent.
pub fn link() -> Rgba {
    rgb(0x7dd3fc) // sky-300
}
pub fn link_hover() -> Rgba {
    rgb(0xbae6fd) // sky-200 — brighter on hover
}
pub fn link_active() -> Rgba {
    rgb(0x38bdf8) // sky-400 — darker while pressed
}

/// Roster chip color for a quark's lifecycle state (the status ramp).
pub fn quark_state(state: QuarkState) -> Rgba {
    match state {
        QuarkState::Ground => rgb(0x9ca2ad),   // neutral grey
        QuarkState::Excited => rgb(0xc084fc),  // soft amethyst — active
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
    0x60a5fa, // soft sapphire blue
    0x34d399, // soft mint emerald
    0xa78bfa, // soft amethyst
    0xf59e0b, // soft warm amber
    0xf87171, // soft coral rose
    0x94a3b8, // cool slate
    0x2dd4bf, // soft ice teal
    0x10b981, // muted sage green
    0xf97316, // soft terracotta orange
    0x818cf8, // soft dusk indigo
    0xc084fc, // soft orchid
    0x9333ea, // muted royal purple
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{rgb, rgba, Hsla};

    #[test]
    fn test_canvas_base_token() {
        let base = canvas_base();
        let expected: Hsla = rgb(0x0c0d10).into();
        assert_eq!(base, expected);
        assert_eq!(base.a, 1.0);
    }

    #[test]
    fn test_glass_surface_token() {
        let surface = glass_surface();
        let expected: Hsla = rgba(0x131519f2).into();
        assert_eq!(surface, expected);
    }

    #[test]
    fn test_glass_card_token() {
        let card = glass_card();
        let expected: Hsla = rgba(0x1a1c22f2).into();
        assert_eq!(card, expected);
    }

    #[test]
    fn test_glass_highlight_token() {
        let highlight = glass_highlight();
        let expected: Hsla = rgba(0xffffff1f).into();
        assert_eq!(highlight, expected);
    }

    #[test]
    fn test_halo_color_tokens() {
        assert_eq!(halo_active(), Hsla::from(rgb(0x60a5fa)));
        assert_eq!(halo_reasoning(), Hsla::from(rgb(0xa78bfa)));
        assert_eq!(halo_idle(), Hsla::from(rgb(0x34d399)));
        assert_eq!(halo_error(), Hsla::from(rgb(0xf87171)));
    }

    #[test]
    fn test_halo_dot_mapping() {
        assert_eq!(halo_dot(QuarkState::Excited), halo_active());
        assert_eq!(halo_dot(QuarkState::Thinking), halo_reasoning());
        assert_eq!(halo_dot(QuarkState::Ground), halo_idle());
        assert_eq!(halo_dot(QuarkState::Waiting), halo_idle());
        assert_eq!(halo_dot(QuarkState::Blocked), halo_error());
        assert_eq!(halo_dot(QuarkState::Error), halo_error());
    }

    #[test]
    fn test_glass_elevation_hierarchy() {
        let base = canvas_base();
        let surface = glass_surface();
        let card = glass_card();

        assert_ne!(base, surface, "Canvas base and glass surface must be distinct");
        assert_ne!(surface, card, "Glass surface and glass card must be distinct");
        assert_ne!(base, card, "Canvas base and glass card must be distinct");
    }

    #[test]
    fn test_halo_colors_mutually_distinct() {
        let active = halo_active();
        let reasoning = halo_reasoning();
        let idle = halo_idle();
        let error = halo_error();

        assert_ne!(active, reasoning);
        assert_ne!(active, idle);
        assert_ne!(active, error);
        assert_ne!(reasoning, idle);
        assert_ne!(reasoning, error);
        assert_ne!(idle, error);
    }

    #[test]
    fn test_translucency_invariants() {
        assert_eq!(canvas_base().a, 1.0, "Canvas base must be opaque");
        assert!(
            glass_surface().a > 0.8 && glass_surface().a <= 1.0,
            "Glass surface must be ~85-95% opacity"
        );
        assert!(
            glass_card().a > 0.8 && glass_card().a <= 1.0,
            "Glass card must be ~85-95% opacity"
        );
        assert!(
            glass_highlight().a < 0.25,
            "Glass highlight rim must be low-alpha white (<25%)"
        );
    }
}

