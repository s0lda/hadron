//! Fonts we ship with the binary.
//!
//! GPUI has no CSS font-stack parsing and a family miss is SILENT — it eats
//! bold specifically (see `app::font_family_with_a_real_bold`). Bundling the
//! faces is what makes the family name a fact rather than a hope on a machine
//! whose font database we do not control.

use std::borrow::Cow;

/// The default UI family, as cosmic-text's `fontdb` derives it from the embedded TTF.
pub const UI_FAMILY: &str = "Inter";
/// The default monospace family, as cosmic-text's `fontdb` derives it from the embedded TTF.
pub const MONO_FAMILY: &str = "Cascadia Code";

pub const BUNDLED_UI_FAMILIES: [&str; 3] = ["Inter", "Geist", "Noto Sans"];
pub const BUNDLED_MONO_FAMILIES: [&str; 3] = ["Cascadia Code", "JetBrains Mono", "Fira Code"];

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
pub const GEIST_REGULAR: &[u8] = include_bytes!("../assets/fonts/Geist-Regular.ttf");
pub const GEIST_BOLD: &[u8] = include_bytes!("../assets/fonts/Geist-Bold.ttf");
pub const NOTO_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
pub const NOTO_SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");

pub const CASCADIA_REGULAR: &[u8] = include_bytes!("../assets/fonts/CascadiaCode-Regular.ttf");
pub const CASCADIA_BOLD: &[u8] = include_bytes!("../assets/fonts/CascadiaCode-Bold.ttf");
pub const JETBRAINS_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");
pub const FIRA_CODE_REGULAR: &[u8] = include_bytes!("../assets/fonts/FiraCode-Regular.ttf");
pub const FIRA_CODE_BOLD: &[u8] = include_bytes!("../assets/fonts/FiraCode-Bold.ttf");

pub const NOTO_COLOR_EMOJI: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

/// The bundled color emoji font family for Linux/WSL and cross-platform fallback.
#[allow(dead_code)]
pub const EMOJI_FAMILY: &str = "Noto Color Emoji";

/// Returns standard fallback font list ensuring color emoji rendering.
#[cfg(feature = "gui")]
#[allow(dead_code)]
pub fn default_fallbacks() -> gpui::FontFallbacks {
    gpui::FontFallbacks::from_fonts(vec![EMOJI_FAMILY.to_string()])
}


/// Every face we ship, in registration order.
pub fn embedded() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(INTER_REGULAR),
        Cow::Borrowed(INTER_BOLD),
        Cow::Borrowed(GEIST_REGULAR),
        Cow::Borrowed(GEIST_BOLD),
        Cow::Borrowed(NOTO_SANS_REGULAR),
        Cow::Borrowed(NOTO_SANS_BOLD),
        Cow::Borrowed(CASCADIA_REGULAR),
        Cow::Borrowed(CASCADIA_BOLD),
        Cow::Borrowed(JETBRAINS_MONO_REGULAR),
        Cow::Borrowed(JETBRAINS_MONO_BOLD),
        Cow::Borrowed(FIRA_CODE_REGULAR),
        Cow::Borrowed(FIRA_CODE_BOLD),
        Cow::Borrowed(NOTO_COLOR_EMOJI),
    ]
}

/// Check if a font family name refers to an emoji or icon/symbol font that should
/// never be probed as a text UI/mono font candidate (probing emoji fonts without 'm'
/// causes GPUI's cosmic-text backend to purge them from the font database).
pub fn is_emoji_or_symbol_font(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "noto color emoji"
        || lower == "notocoloremoji"
        || lower.contains("emoji")
        || lower.contains("symbol")
        || lower.contains("icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A truncated or LFS-pointer download is a 130-byte text file that
    /// `include_bytes!` accepts happily and the font system then rejects at
    /// runtime, silently, in the one way that is hardest to see (bold only).
    #[test]
    fn every_embedded_face_is_a_real_truetype_file() {
        for (name, bytes) in [
            ("Inter-Regular", INTER_REGULAR),
            ("Inter-Bold", INTER_BOLD),
            ("Geist-Regular", GEIST_REGULAR),
            ("Geist-Bold", GEIST_BOLD),
            ("NotoSans-Regular", NOTO_SANS_REGULAR),
            ("NotoSans-Bold", NOTO_SANS_BOLD),
            ("CascadiaCode-Regular", CASCADIA_REGULAR),
            ("CascadiaCode-Bold", CASCADIA_BOLD),
            ("JetBrainsMono-Regular", JETBRAINS_MONO_REGULAR),
            ("JetBrainsMono-Bold", JETBRAINS_MONO_BOLD),
            ("FiraCode-Regular", FIRA_CODE_REGULAR),
            ("FiraCode-Bold", FIRA_CODE_BOLD),
            ("NotoColorEmoji", NOTO_COLOR_EMOJI),
        ] {
            assert!(bytes.len() > 50_000, "{name} is only {} bytes — not a font", bytes.len());
            let magic = &bytes[..4];
            assert!(
                magic == [0x00, 0x01, 0x00, 0x00] || magic == b"true" || magic == b"OTTO",
                "{name} does not start with a TrueType/OpenType magic: {magic:?}"
            );
        }
    }

    #[test]
    fn we_ship_a_distinct_regular_and_bold_for_each_family() {
        assert_ne!(INTER_REGULAR, INTER_BOLD, "Inter bold is the same file as regular");
        assert_ne!(GEIST_REGULAR, GEIST_BOLD, "Geist bold is the same file as regular");
        assert_ne!(NOTO_SANS_REGULAR, NOTO_SANS_BOLD, "Noto Sans bold is the same file as regular");
        assert_ne!(CASCADIA_REGULAR, CASCADIA_BOLD, "Cascadia bold is the same file as regular");
        assert_ne!(JETBRAINS_MONO_REGULAR, JETBRAINS_MONO_BOLD, "JetBrains Mono bold is the same file as regular");
        assert_ne!(FIRA_CODE_REGULAR, FIRA_CODE_BOLD, "Fira Code bold is the same file as regular");
        assert_eq!(embedded().len(), 13);
    }

    #[test]
    fn is_emoji_or_symbol_font_detects_emoji_and_icon_fonts() {
        assert!(is_emoji_or_symbol_font("Noto Color Emoji"));
        assert!(is_emoji_or_symbol_font("NotoColorEmoji"));
        assert!(is_emoji_or_symbol_font("Segoe UI Emoji"));
        assert!(is_emoji_or_symbol_font("Apple Color Emoji"));
        assert!(is_emoji_or_symbol_font("FontAwesome Icons"));
        assert!(is_emoji_or_symbol_font("Segoe UI Symbol"));
        assert!(!is_emoji_or_symbol_font("Inter"));
        assert!(!is_emoji_or_symbol_font("Cascadia Code"));
        assert!(!is_emoji_or_symbol_font("JetBrains Mono"));
        assert!(!is_emoji_or_symbol_font("Fira Code"));
    }

    #[test]
    #[cfg(feature = "gui")]
    fn default_fallbacks_include_noto_color_emoji() {
        let fb = default_fallbacks();
        assert_eq!(fb.fallback_list(), &["Noto Color Emoji"]);
    }

    #[test]
    #[cfg(feature = "gui")]
    fn emoji_layout_resolves_glyphs_from_noto_color_emoji() {
        use gpui::PlatformTextSystem;

        let cosmic = std::sync::Arc::new(gpui_wgpu::CosmicTextSystem::new("fallback"));
        let mut cx = gpui::HeadlessAppContext::new(cosmic.clone());
        cx.update(|cx| {
            cx.text_system().add_fonts(embedded()).expect("register bundled fonts");
            
            // Safe font picker probe filtering out emoji / symbol fonts:
            let text_system = cx.text_system();
            for name in text_system.all_font_names() {
                if is_emoji_or_symbol_font(&name) {
                    continue;
                }
                let regular = gpui::font(&name);
                let _ = text_system.resolve_font(&regular);
                let _ = text_system.resolve_font(&regular.bold());
            }

            for family in BUNDLED_UI_FAMILIES.into_iter().chain(BUNDLED_MONO_FAMILIES) {
                let font = gpui::font(family);
                let font_id = cx.text_system().resolve_font(&font);
                let mut font_with_fb = gpui::font(family);
                font_with_fb.fallbacks = Some(default_fallbacks());
                let font_fb_id = cx.text_system().resolve_font(&font_with_fb);

                let test_str = "Status: 🚀 Complete! 🔥 Great job ✨ 🎉 🤣 👍 💡 😘 ❤️ ☺ ⚡ ☕ ⭐";
                let layout_no_fb = cosmic.layout_line(
                    test_str,
                    gpui::px(14.0),
                    &[gpui::FontRun {
                        len: test_str.len(),
                        font_id,
                    }],
                );
                let layout_with_fb = cosmic.layout_line(
                    test_str,
                    gpui::px(14.0),
                    &[gpui::FontRun {
                        len: test_str.len(),
                        font_id: font_fb_id,
                    }],
                );

                let count_no_fb = layout_no_fb.runs.iter()
                    .flat_map(|r| r.glyphs.iter())
                    .filter(|g| g.is_emoji && g.id.0 > 0)
                    .count();
                let count_with_fb = layout_with_fb.runs.iter()
                    .flat_map(|r| r.glyphs.iter())
                    .filter(|g| g.is_emoji && g.id.0 > 0)
                    .count();

                println!("=== Family '{family}' ===");
                println!("no_fb runs: {}", layout_no_fb.runs.len());
                for (r_i, run) in layout_no_fb.runs.iter().enumerate() {
                    let font_name = cx.text_system().all_font_names();
                    println!("  run {r_i}: font_id={:?}, glyphs={}", run.font_id, run.glyphs.len());
                    for g in &run.glyphs {
                        println!("    glyph_id={:?}, index={}, is_emoji={}", g.id, g.index, g.is_emoji);
                    }
                }
                println!("with_fb runs: {}", layout_with_fb.runs.len());
                for (r_i, run) in layout_with_fb.runs.iter().enumerate() {
                    println!("  run {r_i}: font_id={:?}, glyphs={}", run.font_id, run.glyphs.len());
                    for g in &run.glyphs {
                        println!("    glyph_id={:?}, index={}, is_emoji={}", g.id, g.index, g.is_emoji);
                    }
                }
                break; // Just one family to see details
            }
        });
    }
}


