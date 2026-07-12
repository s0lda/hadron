use anyhow::Result;
use gpui::{Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, Rope};
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};

pub struct ChatCompletionProvider {
    pub quarks: Vec<String>,
}

pub fn extract_completion_query(text: &str, offset: usize) -> Option<(char, String, usize)> {
    let before_cursor = &text[..offset];
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
        } else if is_emoji {
            for emoji in emojis::iter() {
                if let Some(shortcode) = emoji.shortcode() {
                    let shortcode_lower = shortcode.to_lowercase();
                    if query_lower.is_empty() || shortcode_lower.contains(&query_lower) {
                        items.push(CompletionItem {
                            label: format!("{} :{}", emoji.as_str(), shortcode),
                            insert_text: Some(emoji.as_str().to_string()),
                            text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                                range,
                                new_text: emoji.as_str().to_string(),
                            })),
                            kind: Some(CompletionItemKind::TEXT),
                            detail: Some("Emoji".to_string()),
                            filter_text: Some(format!(":{}", shortcode)),
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
        new_text == "@" || new_text == ":"
    }
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
}
