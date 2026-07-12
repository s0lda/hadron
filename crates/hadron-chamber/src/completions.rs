use anyhow::Result;
use gpui::{Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, Rope};
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};

pub struct ChatCompletionProvider {
    pub quarks: Vec<String>,
}

impl CompletionProvider for ChatCompletionProvider {
    fn completions(
        &self,
        _text: &Rope,
        _offset: usize,
        trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let trigger_char = trigger.trigger_character.unwrap_or_default();
        let is_mention = trigger_char.starts_with('@');
        let is_emoji = trigger_char.starts_with(':');
        let query = if is_mention {
            trigger_char.trim_start_matches('@').to_lowercase()
        } else if is_emoji {
            trigger_char.trim_start_matches(':').to_lowercase()
        } else {
            return Task::ready(Ok(CompletionResponse::Array(vec![])));
        };

        let mut items = Vec::new();

        if is_mention {
            for quark in &self.quarks {
                let quark_lower = quark.to_lowercase();
                if query.is_empty() || quark_lower.contains(&query) {
                    items.push(CompletionItem {
                        label: format!("@{}", quark_lower),
                        insert_text: Some(format!("{} ", quark_lower)),
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
                    if query.is_empty() || shortcode_lower.contains(&query) {
                        items.push(CompletionItem {
                            label: format!("{} :{}", emoji.as_str(), shortcode),
                            insert_text: Some(emoji.as_str().to_string()),
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
