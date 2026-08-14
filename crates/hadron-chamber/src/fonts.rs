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
    ]
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
        assert_eq!(embedded().len(), 12);
    }
}
