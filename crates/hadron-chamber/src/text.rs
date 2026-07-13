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
        if c == '@' || c == ':' {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((c, query, idx));
        }
        if c.is_whitespace() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
