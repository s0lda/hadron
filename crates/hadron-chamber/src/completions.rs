use anyhow::Result;
use gpui::{Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, Rope};
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};

pub struct ChatCompletionProvider {
    pub quarks: Vec<String>,
    pub files: Vec<String>,
}

// The query parser lives in `text`, which is not feature-gated, so its emoji
// crash-guard tests run in `cargo test --workspace`. One definition, one place.
pub use crate::text::extract_completion_query;

impl CompletionProvider for ChatCompletionProvider {
    fn completions(
        &self,
        _text: &Rope,
        _offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let text_str = _text.to_string();
        let Some((trigger_char, query, start_offset)) = extract_completion_query(&text_str, _offset) else {
            return Task::ready(Ok(CompletionResponse::Array(vec![])));
        };

        let is_mention = trigger_char == '@';
        let is_emoji = trigger_char == ':';
        let query_lower = query.to_lowercase();

        use gpui_component::RopeExt;
        let range = lsp_types::Range {
            start: _text.offset_to_position(start_offset),
            end: _text.offset_to_position(_offset),
        };

        let mut items = Vec::new();

        if is_mention {
            for quark in &self.quarks {
                let quark_lower = quark.to_lowercase();
                if query_lower.is_empty() || quark_lower.contains(&query_lower) {
                    items.push(CompletionItem {
                        label: format!("@{}", quark_lower),
                        insert_text: Some(format!("@{} ", quark_lower)),
                        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range,
                            new_text: format!("@{} ", quark_lower),
                        })),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("Quark".to_string()),
                        filter_text: Some(format!("@{}", quark_lower)),
                        ..Default::default()
                    });
                }
            }
            for file in &self.files {
                let file_lower = file.to_lowercase();
                if query_lower.is_empty() || file_lower.contains(&query_lower) {
                    // The filter text must be a byte-prefix of the label (see `MenuRow`).
                    // It was the *lowercased* path, which for a non-ASCII filename can be
                    // a different byte length than the label — the same mid-character
                    // highlight that crashed the emoji rows. The menu does not filter on
                    // it (it only measures it), so the original-case path is safe here;
                    // case-insensitive matching is done above, by us.
                    let row = crate::text::MenuRow::new(format!("@{}", file), "");
                    items.push(CompletionItem {
                        label: row.label().to_string(),
                        insert_text: Some(format!("@{} ", file)),
                        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                            range,
                            new_text: format!("@{} ", file),
                        })),
                        kind: Some(CompletionItemKind::FILE),
                        detail: Some("File".to_string()),
                        filter_text: Some(row.filter_text().to_string()),
                        ..Default::default()
                    });
                }
            }
        } else if is_emoji {
            for emoji in emojis::iter() {
                if let Some(shortcode) = emoji.shortcode() {
                    let shortcode_lower = shortcode.to_lowercase();
                    if query_lower.is_empty() || shortcode_lower.contains(&query_lower) {
                        // `MenuRow` puts the shortcode FIRST. The menu highlights
                        // `label[0..filter_text.len()]`, and an emoji-leading label put
                        // that byte range inside the glyph — Jake's `::` crash.
                        let row = crate::text::MenuRow::new(format!(":{}", shortcode), emoji.as_str());
                        items.push(CompletionItem {
                            label: row.label().to_string(),
                            insert_text: Some(emoji.as_str().to_string()),
                            text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                                range,
                                new_text: emoji.as_str().to_string(),
                            })),
                            kind: Some(CompletionItemKind::TEXT),
                            detail: Some("Emoji".to_string()),
                            filter_text: Some(row.filter_text().to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(
        &self,
        _offset: usize,
        new_text: &str,
        _cx: &mut Context<InputState>,
    ) -> bool {
        is_trigger_text(new_text)
    }
}

/// Whether an edit should (re)run the completion provider.
///
/// `@` and `:` open the menu, but they must not be the *only* things that trigger it.
/// Each completion item carries an absolute `text_edit.range`, and the editor replaces
/// exactly that range on accept. If the provider is not re-run as the query is typed,
/// those items keep the range computed when the sigil was the whole query — `[@]` —
/// so accepting a suggestion after typing `@a` replaces only the `@` and strands the
/// `a`, yielding `@agy a`. Re-running on each subsequent character recomputes the range
/// against the live cursor, which is what makes the replacement cover the whole query.
///
/// A deletion (empty `new_text`) must re-run too, or backspacing leaves the range
/// pointing past the end of the shortened text.
fn is_trigger_text(new_text: &str) -> bool {
    // A multi-character edit is a paste, not a keystroke — it opens nothing. Any single
    // non-whitespace character keeps an open query live; if there is no sigil before the
    // cursor the provider returns no items and the menu closes on its own.
    new_text.is_empty()
        || (new_text.chars().count() == 1 && !new_text.chars().all(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_completion_query() {
        assert_eq!(
            extract_completion_query("@a", 2),
            Some(('@', "a".to_string(), 0))
        );
        assert_eq!(
            extract_completion_query("hello @agy", 10),
            Some(('@', "agy".to_string(), 6))
        );
        assert_eq!(
            extract_completion_query(":", 1),
            Some((':', "".to_string(), 0))
        );
        assert_eq!(
            extract_completion_query(":smil", 5),
            Some((':', "smil".to_string(), 0))
        );
        assert_eq!(
            extract_completion_query("hello@world", 11),
            Some(('@', "world".to_string(), 5))
        );
        assert_eq!(
            extract_completion_query("foo bar", 7),
            None
        );
    }

    // `text_with_emoji_does_not_panic_on_offset` moved to `text` — it guards a crash,
    // and here it only ran under `--features gui`, i.e. never in the workspace gate.

    /// The `@a` → `@agy a` bug. The sigil is not the only trigger: typing the query
    /// after it must re-run the provider, or the accepted item replaces a stale range
    /// covering only the `@`.
    #[test]
    fn typing_the_query_after_a_sigil_keeps_the_provider_live() {
        assert!(is_trigger_text("@"), "the sigil opens the menu");
        assert!(is_trigger_text(":"), "the emoji sigil opens the menu");
        assert!(is_trigger_text("a"), "typing the query must re-run the provider");
        assert!(is_trigger_text(""), "a deletion must re-run it, or the range outruns the text");

        assert!(!is_trigger_text(" "), "whitespace ends a query");
        assert!(!is_trigger_text("\n"), "so does a newline");
        assert!(!is_trigger_text("hello"), "a multi-char paste is not a keystroke");
    }

    /// The range handed to the editor must cover the WHOLE query, sigil included — the
    /// editor replaces exactly `[start, end)`. `[0, 1)` is the bug; `[0, 2)` is correct.
    /// Pinned as a round-trip through the same offset↔position conversions the editor
    /// uses on accept, since that pair is where a correct `start_offset` could still be
    /// mistranslated.
    #[test]
    fn the_edit_range_spans_the_whole_query_not_just_the_sigil() {
        use gpui_component::RopeExt;
        use gpui_component::input::Rope;

        let text = Rope::from("@a");
        let cursor = 2; // after "@a"
        let (_, _, start_offset) = extract_completion_query(&text.to_string(), cursor).unwrap();

        // What the provider puts on the item...
        let start = text.offset_to_position(start_offset);
        let end = text.offset_to_position(cursor);
        // ...and what the editor resolves it back to when the item is accepted.
        assert_eq!(text.position_to_offset(&start), 0, "range starts at the '@'");
        assert_eq!(text.position_to_offset(&end), 2, "range ends at the cursor, past the 'a'");
    }
}
