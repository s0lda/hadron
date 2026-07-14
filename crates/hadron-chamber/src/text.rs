//! Pure text logic shared by the UI — no `gpui`, no feature gate.
//!
//! `extract_completion_query` lived in `completions`, which imports `gpui` and is
//! therefore `#[cfg(feature = "gui")]`. That gating was incidental to the function
//! (it is pure `&str` work) but not to its **tests**: `cargo test --workspace` does
//! not build the `gui` feature, so the regression test guarding the emoji crash
//! never ran in the gate that decides whether we ship. A crash guard invisible to
//! the gate is a crash guard that will be broken again without anyone noticing.

/// Find the `@`/`:` completion trigger immediately before the cursor.
///
/// Returns the trigger char, the query typed after it, and its byte index.
///
/// **`offset` is a BYTE offset from the editor and may land anywhere** — including
/// past the end of the string, or in the middle of a multi-byte character. Slicing
/// `&text[..offset]` on either panics, and that panic is the emoji crash: type an
/// emoji into the chat box and the cursor sits at a byte offset that is not a char
/// boundary. Clamp into range, then walk back to the nearest boundary.
pub fn extract_completion_query(text: &str, offset: usize) -> Option<(char, String, usize)> {
    let mut safe_offset = offset.min(text.len());
    while safe_offset > 0 && !text.is_char_boundary(safe_offset) {
        safe_offset -= 1;
    }

    let before_cursor = &text[..safe_offset];
    for (idx, c) in before_cursor.char_indices().rev() {
        if c == '@' || c == ':' || (c == '/' && idx == 0) {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((c, query, idx));
        }
        if c.is_whitespace() {
            break;
        }
    }
    None
}

/// The label and filter text of one completion-menu row, built so the menu cannot
/// panic on it.
///
/// **Jake's `::` crash.** The completion menu highlights the matched prefix of a row
/// by byte range — `0..filter_text.len()` (gpui-component `completion_menu.rs`) — and
/// gpui `debug_assert!`s that both ends of a highlight range are char boundaries
/// (`gpui/src/elements/text.rs`). We used to build emoji rows as `"📺 :tv"`: the label
/// *starts* with a 4-byte emoji, and `":tv".len()` is 3, so the range `0..3` lands
/// **inside** the emoji and the assert fires. Instant crash, any query that surfaces
/// an emoji whose shortcode is shorter than the leading emoji is wide.
///
/// The fix is structural, not a check: a label that **begins with its own filter
/// text** always has a char boundary at `filter_text.len()`, because the filter text
/// is a prefix of it. This type is the only way to construct a row, so the crashing
/// shape cannot be written.
pub struct MenuRow {
    label: String,
    filter_text: String,
}

impl MenuRow {
    /// `filter_text` is what the user is matching against and what the menu
    /// highlights; `trailing` is anything shown after it (an emoji glyph, a hint).
    pub fn new(filter_text: impl Into<String>, trailing: &str) -> Self {
        let filter_text = filter_text.into();
        let label = if trailing.is_empty() {
            filter_text.clone()
        } else {
            format!("{filter_text} {trailing}")
        };
        Self { label, filter_text }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn filter_text(&self) -> &str {
        &self.filter_text
    }

    /// The byte range the menu will highlight. Always a char boundary, by construction.
    pub fn highlight_len(&self) -> usize {
        self.filter_text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard for Jake's `::` crash, over **every emoji the picker can offer** —
    /// not a hand-picked one. `:tv`, `:id` and `:vs` are the short shortcodes that
    /// actually fired it; an exhaustive sweep does not depend on us guessing right.
    #[test]
    fn no_emoji_row_can_put_the_menus_highlight_inside_a_multibyte_char() {
        for emoji in emojis::iter() {
            let Some(shortcode) = emoji.shortcode() else {
                continue;
            };
            let row = MenuRow::new(format!(":{shortcode}"), emoji.as_str());
            assert!(
                row.label().is_char_boundary(row.highlight_len()),
                "menu would slice {:?} at byte {} — mid-character, gpui panics",
                row.label(),
                row.highlight_len()
            );
        }
    }

    /// The mechanism itself, pinned: the label shape we *used* to build is the crash.
    /// If this ever stops holding, the bug was fixed somewhere else and `MenuRow` can go.
    #[test]
    fn the_old_emoji_first_label_really_does_land_mid_character() {
        let tv = emojis::get_by_shortcode("tv").expect(":tv is in the emojis crate");
        let old_label = format!("{} :tv", tv.as_str()); // what completions.rs built
        assert!(
            !old_label.is_char_boundary(":tv".len()),
            "the premise of the crash: byte 3 of {old_label:?} is inside the emoji"
        );
    }

    /// A row whose trailing text is a bare mention has nothing after the filter text,
    /// and the highlight covers the whole label.
    #[test]
    fn a_row_with_no_trailing_text_is_all_highlight() {
        let row = MenuRow::new("@opus", "");
        assert_eq!(row.label(), "@opus");
        assert_eq!(row.highlight_len(), row.label().len());
    }

    #[test]
    fn finds_a_mention_and_an_emoji_trigger() {
        assert_eq!(
            extract_completion_query("hey @op", 7),
            Some(('@', "op".to_string(), 4))
        );
        assert_eq!(
            extract_completion_query("nice :smi", 9),
            Some((':', "smi".to_string(), 5))
        );
        assert_eq!(extract_completion_query("no trigger", 10), None);
    }

    /// **Jake's crash.** Every byte offset into a string full of multi-byte
    /// characters must be survivable — including offsets that land *inside* an
    /// emoji, and offsets past the end. Exhaustive rather than exemplary: a single
    /// hand-picked offset proves nothing about the one the editor actually sends.
    #[test]
    fn no_byte_offset_into_a_multibyte_string_can_panic() {
        // A 4-byte emoji, a ZWJ family sequence, an accented char, a CJK char.
        let hostile = "Hi 🌍 @wor 👨‍👩‍👧‍👦 café 日本 :smi 🙃";

        for offset in 0..=hostile.len() + 8 {
            // Must not panic at ANY offset, boundary or not, in range or past the end.
            let _ = extract_completion_query(hostile, offset);
        }
    }

    /// Agy's original regression test, moved here from `completions` so it runs in
    /// `cargo test --workspace` rather than only under `--features gui`.
    #[test]
    fn text_with_emoji_does_not_panic_on_offset() {
        let text = "Hello 🌍 @world";
        // 🌍 occupies bytes 6..10.
        assert_eq!(extract_completion_query(text, 10), None);
        assert_eq!(
            extract_completion_query(text, text.len()),
            Some(('@', "world".to_string(), 11))
        );
        // An offset landing INSIDE the emoji must not crash.
        assert_eq!(extract_completion_query(text, 8), None);
    }

    /// The clamp must not silently change the answer for well-formed input: walking
    /// back from a mid-emoji offset should still find the trigger that precedes it.
    #[test]
    fn walking_back_from_inside_an_emoji_still_finds_the_trigger() {
        let text = "@ab🌍";
        let mid_emoji = text.len() - 2; // inside the 4-byte emoji
        assert!(!text.is_char_boundary(mid_emoji), "the test's premise");

        let (trigger, query, idx) = extract_completion_query(text, mid_emoji).unwrap();
        assert_eq!(trigger, '@');
        assert_eq!(idx, 0);
        assert_eq!(query, "ab", "the partial emoji is dropped, not sliced");
    }
}
