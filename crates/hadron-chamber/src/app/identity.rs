//! Quark display-identity resolution: the [`ResolvedIdentity`] a roster row renders
//! as (name, colour, avatar) after applying the user's overrides over code defaults,
//! plus the small colour/initials/avatar helpers behind it. Pure value work — no
//! `Chamber`, only leaf GPUI element construction for the avatar.

use super::*;

/// Pack an `(r, g, b)` triple into the `0xRRGGBB` gpui [`gpui::rgb`] expects.
pub(super) fn pack_rgb((r, g, b): (u8, u8, u8)) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// A fully-resolved display identity — what actually renders after applying the
/// user's [`Identity`] overrides over code defaults (id-derived name, hue color,
/// initials avatar).
pub(super) struct ResolvedIdentity {
    pub(super) name: String,
    pub(super) color: Hsla,
    pub(super) image: Option<String>,
}

/// The palette a user picks an identity color from (Settings). Kept small and
/// legible on the dark surfaces.
pub(super) const IDENTITY_SWATCHES: [u32; 14] = [
    0xe2e8f0, // soft slate white
    0xa78bfa, // soft amethyst
    0x9333ea, // muted royal purple
    0x60a5fa, // soft sapphire blue
    0x34d399, // soft mint emerald
    0xf59e0b, // soft warm amber
    0xf87171, // soft coral rose
    0x94a3b8, // cool slate
    0x3898ec, // soft cyan blue
    0x2dd4bf, // soft ice teal
    0x10b981, // muted sage green
    0xf97316, // soft terracotta orange
    0x818cf8, // soft dusk indigo
    0xc084fc, // soft orchid
];

/// Parse a `#rrggbb` string into a color.
pub(super) fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok().map(rgb)
}

/// Pack an `Hsla` into a `0xRRGGBB` value — the inverse of [`parse_hex`]'s `rgb`, so a
/// colour chosen in the picker round-trips through the stored `#rrggbb` string.
pub(super) fn hsla_to_hex(hsla: Hsla) -> u32 {
    let c: Rgba = hsla.into();
    let q = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u32;
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// Up to two uppercase initials from a display name, for the fallback avatar.
pub(super) fn initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter_map(|w| w.chars().next());
    match (words.next(), words.next()) {
        (Some(a), Some(b)) => format!("{a}{b}").to_uppercase(),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

/// The right [`gpui::ImageSource`] for an avatar path.
///
/// gpui's `From<String> for ImageSource` routes a non-URL string to a
/// `Resource::Embedded` — a lookup in the app's **bundled** asset source, not the
/// filesystem. A user's avatar file is never a bundled asset, so the load misses and
/// the avatar falls back to a blank grey circle (its `secondary` background, no
/// initials). A local file must go through `PathBuf`, which yields a `Resource::Path`
/// read with `fs::read`. Genuine http(s) URLs stay strings so they take the
/// `Resource::Uri` (HTTP) route unchanged.
pub(super) fn avatar_source(path: &str) -> gpui::ImageSource {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string().into()
    } else {
        std::path::PathBuf::from(path).into()
    }
}

/// Render an identity's avatar: the chosen image if set, else a colored circle
/// with the name's initials.
pub(super) fn identity_avatar(id: &ResolvedIdentity, diameter: f32) -> gpui::AnyElement {
    identity_avatar_with_state(id, diameter, None, true)
}

/// Resolve the border (width, color) for an identity avatar's aura ring based on quark state and enabled status.
///
/// When excited (working), the ring adopts the quark's own assigned identity color (`id_color`)
/// so it seamlessly blends with the UI while active. When thinking, it uses the reasoning halo.
/// When idle/disabled, it falls back to muted/disabled ring styling.
pub(super) fn avatar_ring_border(
    id_color: Hsla,
    state: Option<QuarkState>,
    enabled: bool,
) -> (f32, Hsla) {
    if !enabled {
        (1.0, theme::presence_disabled().into())
    } else if let Some(st) = state {
        match st {
            QuarkState::Excited => (1.0, id_color),
            QuarkState::Thinking => (1.0, theme::halo_dot(st)),
            _ => (1.0, id_color.opacity(0.35)),
        }
    } else {
        (1.0, id_color.opacity(0.35))
    }
}

/// Render an identity's avatar with state-driven ring styling (active quark color / reasoning ring, idle muted ring).
pub(super) fn identity_avatar_with_state(
    id: &ResolvedIdentity,
    diameter: f32,
    state: Option<QuarkState>,
    enabled: bool,
) -> gpui::AnyElement {
    let (border_width, border_color) = avatar_ring_border(id.color, state, enabled);

    let base_avatar = match &id.image {
        Some(path) => Avatar::new()
            .src(avatar_source(path))
            .with_size(Size::Size(px(diameter)))
            .border(px(border_width))
            .border_color(border_color)
            .into_any_element(),
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(px(diameter))
            .rounded_full()
            .bg(id.color.opacity(0.18))
            .border(px(border_width))
            .border_color(border_color)
            .text_color(id.color)
            .text_size(px(diameter * 0.4))
            .font_weight(gpui::FontWeight::BOLD)
            .child(initials(&id.name))
            .into_any_element(),
    };

    if enabled {
        base_avatar
    } else {
        div().opacity(0.6).child(base_avatar).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local avatar path must reach gpui as a `Resource::Path` (read with `fs::read`),
    /// not a `Resource::Embedded` (a bundled-asset lookup). Handing gpui a bare `String`
    /// picks the Embedded route, whose lookup misses for a user's file and leaves a blank
    /// grey avatar — the reported bug. http(s) URLs must stay on the Uri route.
    #[test]
    fn a_local_avatar_path_loads_from_the_filesystem_not_as_a_bundled_asset() {
        use gpui::{ImageSource, Resource};
        use std::path::PathBuf;

        // The bug: the old `.src(path.clone())` (a String) treats a local file as a
        // bundled asset (Resource::Embedded), whose lookup misses → blank grey avatar.
        let buggy: ImageSource = String::from("/home/jake/me.png").into();
        assert!(
            matches!(buggy, ImageSource::Resource(Resource::Embedded(_))),
            "a bare String path is looked up in the bundled asset source, which a user file misses"
        );

        // The fix: a filesystem path becomes a Resource::Path (fs::read).
        match avatar_source("/home/jake/me.png") {
            ImageSource::Resource(r) => {
                assert_eq!(r, Resource::Path(PathBuf::from("/home/jake/me.png").into()))
            }
            _ => panic!("expected a Resource source"),
        }
        // A remote URL stays a Uri so http(s) avatars keep working.
        match avatar_source("https://example.com/me.png") {
            ImageSource::Resource(Resource::Uri(_)) => {}
            _ => panic!("expected an http(s) URL to remain a Uri source"),
        }
    }

    #[test]
    fn avatar_ring_border_styling_per_state() {
        let custom_color = gpui::hsla(0.8, 0.7, 0.6, 1.0);

        // Excited (working): adopts the quark's own identity color
        let (width, color) = avatar_ring_border(custom_color, Some(QuarkState::Excited), true);
        assert_eq!(width, 1.0);
        assert_eq!(color, custom_color);

        // Thinking (reasoning): uses reasoning halo dot color
        let (width, color) = avatar_ring_border(custom_color, Some(QuarkState::Thinking), true);
        assert_eq!(width, 1.0);
        assert_eq!(color, theme::halo_dot(QuarkState::Thinking));

        // Ground / idle: uses muted quark color (opacity 0.35)
        let (width, color) = avatar_ring_border(custom_color, Some(QuarkState::Ground), true);
        assert_eq!(width, 1.0);
        assert_eq!(color, custom_color.opacity(0.35));

        // None state: uses muted quark color
        let (width, color) = avatar_ring_border(custom_color, None, true);
        assert_eq!(width, 1.0);
        assert_eq!(color, custom_color.opacity(0.35));

        // Disabled seat: uses presence_disabled
        let (width, color) = avatar_ring_border(custom_color, Some(QuarkState::Excited), false);
        assert_eq!(width, 1.0);
        assert_eq!(color, theme::presence_disabled().into());
    }
}
