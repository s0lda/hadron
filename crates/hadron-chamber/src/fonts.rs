//! Fonts we ship with the binary.
//!
//! GPUI has no CSS font-stack parsing and a family miss is SILENT — it eats
//! bold specifically (see `app::font_family_with_a_real_bold`). Bundling the
//! faces is what makes the family name a fact rather than a hope on a machine
//! whose font database we do not control.

use std::borrow::Cow;

/// The UI family, as cosmic-text's `fontdb` derives it from the embedded TTF.
pub const UI_FAMILY: &str = "Inter";
/// The monospace family, as cosmic-text's `fontdb` derives it from the embedded TTF.
pub const MONO_FAMILY: &str = "Cascadia Code";

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
pub const CASCADIA_REGULAR: &[u8] = include_bytes!("../assets/fonts/CascadiaCode-Regular.ttf");
pub const CASCADIA_BOLD: &[u8] = include_bytes!("../assets/fonts/CascadiaCode-Bold.ttf");

/// Every face we ship, in registration order.
pub fn embedded() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(INTER_REGULAR),
        Cow::Borrowed(INTER_BOLD),
        Cow::Borrowed(CASCADIA_REGULAR),
        Cow::Borrowed(CASCADIA_BOLD),
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
            ("CascadiaCode-Regular", CASCADIA_REGULAR),
            ("CascadiaCode-Bold", CASCADIA_BOLD),
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
        assert_ne!(CASCADIA_REGULAR, CASCADIA_BOLD, "Cascadia bold is the same file as regular");
        assert_eq!(embedded().len(), 4);
    }
}
