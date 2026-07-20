use super::*;

impl super::Chamber {
    /// Submit the human's message on Enter (Shift+Enter inserts a newline).
    /// Appends an `Actor::Human` event to the field — the same bus the quarks
    /// use — then re-reads and re-projects so the new row appears immediately.
    pub(super) fn on_input_submit(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Typing rebuilds the completion card from the live text; it is our own
        // overlay, not the fork's LSP menu, so we drive it from the edit stream.
        if let InputEvent::Change = event {
            self.recompute_completion(cx);
            cx.notify();
            return;
        }
        let InputEvent::PressEnter { shift, .. } = event else {
            return;
        };
        // A live card claims Enter: it accepts the highlighted row instead of
        // sending the message (Shift+Enter always means newline, never accept).
        if !*shift && self.completion.is_some() {
            self.accept_completion(window, cx);
            return;
        }
        if *shift {
            let selected_idx = self.chat_tab.index();
            let scroll = self.chat_scrolls[selected_idx].clone();
            cx.on_next_frame(window, move |_, _, cx: &mut Context<Self>| {
                scroll.scroll_to_bottom();
                cx.notify();
            });
            return;
        }
        let full = input.read(cx).value().trim().to_string();
        if full.is_empty() {
            return;
        }

        // A line may begin with chained UI commands, then a normal message, e.g.
        // "/toggle-roster /clear ping the team". `split_leading_commands` peels the
        // leading `/command` tokens; the returned body is the untouched remainder, so a
        // multi-line message (Shift+Enter, a markdown list) keeps its newlines.
        let (cmds, body) = split_leading_commands(&full);
        for (cmd, args) in &cmds {
            self.handle_chat_command(cmd, args, window, cx);
        }
        let text = match body {
            Some(body) => body,
            None => {
                // Only recognised commands were present (`body` is `None` only when at
                // least one command ran): clear the box and stop before posting nothing.
                input.update(cx, |state, cx| state.set_value("", window, cx));
                return;
            }
        };

        // Write the raw text with `to: None`, leaving any `@mentions` in the body.
        // The daemon resolves addressees from the body, so ONE message can address
        // several quarks ("@opus do X and @agy do Y") — each is fanned out in turn.
        // (Stripping a single leading mention into `to` would drop the others.)
        let ev = Event::new(Actor::Human, None, Kind::Message { body: text });
        if let Err(e) = io::append_event(&self.path, &ev) {
            eprintln!("chamber: failed to append steering message: {e}");
            return;
        }

        input.update(cx, |state, cx| state.set_value("", window, cx));
        let events = io::read_events(&self.path).unwrap_or_default();
        let old_log_count = self.view.messages.len();
        self.reproject(&events);

        let old_chat_count = self.chat_message_ixs.len();
        self.chat_message_ixs = self
            .view
            .messages
            .iter()
            .enumerate()
            .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
            .collect();
        let new_chat_count = self.chat_message_ixs.len();
        let new_log_count = self.view.messages.len();
        
        if new_chat_count > old_chat_count {
            self.chat_list_state.splice(
                old_chat_count..old_chat_count,
                new_chat_count - old_chat_count,
            );
        }
        if new_log_count > old_log_count {
            self.log_list_state.splice(
                old_log_count..old_log_count,
                new_log_count - old_log_count,
            );
        }

        // The human just spoke — always snap to their new message.
        for scroll in &self.chat_scrolls {
            scroll.scroll_to_bottom();
        }
        self.chat_list_state
            .scroll_to_reveal_item(new_chat_count.saturating_sub(1));
        self.log_list_state
            .scroll_to_reveal_item(new_log_count.saturating_sub(1));
        cx.notify();
    }
    /// Rebuild the completion card from the input's current text and cursor.
    /// Sets `self.completion` to `None` when no `@`/`:`/`/` query is live.
    fn recompute_completion(&mut self, cx: &mut Context<Self>) {
        let state = self.input.read(cx);
        let text = state.value().to_string();
        let cursor = state.cursor();
        // Source the completion roster from the RESOLVED team, not `self.team`
        // (the raw repo file). Since the catalogue migration, a migrated repo's
        // `team.json` holds only role/state overrides — the full seat definitions
        // live in `self.global` — so reading `self.team.quarks` directly yielded an
        // empty list and only `@team`/`@orchestrator` ever autocompleted. Every
        // other roster consumer already goes through `resolve_team`.
        let quarks: Vec<(String, Option<String>)> = resolve_team(&self.team, &self.global)
            .quarks
            .iter()
            .map(|q| (q.id.0.clone(), q.display_name.clone()))
            .collect();
        let files = self.completion_files.borrow();
        let result = crate::text::completion_candidates(&text, cursor, &quarks, files.as_slice());
        drop(files);
        self.completion = result.map(|c| {
            self.completion_scroll.scroll_to_item(0);
            CompletionCard {
                start: c.start,
                candidates: c.candidates,
                selected: 0,
            }
        });
    }

    /// Move the card's highlight by `delta`, clamped to the list. No-op with no card.
    pub(super) fn move_completion_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(card) = &mut self.completion {
            let len = card.candidates.len();
            if len == 0 {
                return;
            }
            let max = len as isize - 1;
            card.selected = (card.selected as isize + delta).clamp(0, max) as usize;
            self.completion_scroll.scroll_to_item(card.selected);
            cx.notify();
        }
    }

    /// Accept the highlighted row: splice its `new_text` over `input[start..cursor]`
    /// and put the caret just after it. Byte offsets throughout — `cursor()` and
    /// `set_selected_range` are both documented UTF-8, and the cursor is clamped to a
    /// char boundary first, so this cannot slice mid-character (the emoji crash class).
    pub(super) fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.completion.take() else {
            return;
        };
        let Some(cand) = card.candidates.get(card.selected).or_else(|| card.candidates.first())
        else {
            cx.notify();
            return;
        };
        let new_text = cand.new_text.clone();
        let value = self.input.read(cx).value().to_string();
        let mut cursor = self.input.read(cx).cursor().min(value.len());
        while cursor > 0 && !value.is_char_boundary(cursor) {
            cursor -= 1;
        }
        let start = card.start.min(cursor);
        let new_value = format!("{}{}{}", &value[..start], new_text, &value[cursor..]);
        let new_cursor = start + new_text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(new_value, window, cx);
            state.set_selected_range(new_cursor..new_cursor, cx);
        });
        cx.notify();
    }
}

/// Launch the chamber window against a field file path.
/// Peel the leading `/command` tokens off a submitted chat line. Returns the commands
/// to run in order — each as `(name, args)` — and the leftover message body.
///
/// Only *leading* commands are recognised: the first token that is not a known command
/// ends parsing, and everything from there is the message (so a body can contain a
/// literal "/foo"). Zero-arg commands (`toggle-roster` / `toggle-inspector` / `clear`)
/// chain; `/team-brainstorm` takes the rest of the line as its argument. The body is a
/// slice of the ORIGINAL input, so internal newlines survive — it is never rebuilt from
/// whitespace-split tokens. `body` is `None` only when at least one command ran and no
/// message text remains, which is how the caller knows to clear the box and post nothing.
pub(super) fn split_leading_commands(full: &str) -> (Vec<(String, String)>, Option<String>) {
    const ZERO_ARG_CMDS: [&str; 3] = ["toggle-roster", "toggle-inspector", "clear"];
    let mut cmds = Vec::new();
    let mut rest = full;

    loop {
        let head = rest.trim_start();
        let tok_end = head.find(char::is_whitespace).unwrap_or(head.len());
        let token = &head[..tok_end];
        match token.strip_prefix('/').filter(|c| !c.is_empty()) {
            Some(cmd) if ZERO_ARG_CMDS.contains(&cmd) => {
                cmds.push((cmd.to_string(), String::new()));
                rest = &head[tok_end..];
            }
            Some("team-brainstorm") => {
                cmds.push(("team-brainstorm".to_string(), head[tok_end..].trim().to_string()));
                // team-brainstorm consumes the rest of the line, so nothing is left to post.
                return (cmds, None);
            }
            Some("reboot") => {
                cmds.push(("reboot".to_string(), head[tok_end..].trim().to_string()));
                // reboot consumes the rest of the line, so nothing is left to post.
                return (cmds, None);
            }
            Some("approve") => {
                cmds.push(("approve".to_string(), head[tok_end..].trim().to_string()));
                return (cmds, None);
            }
            Some("deny") => {
                cmds.push(("deny".to_string(), head[tok_end..].trim().to_string()));
                return (cmds, None);
            }
            // First non-command token: the untouched remainder is the message body.
            _ => break,
        }
    }

    let remaining = rest.trim();
    let body = (!remaining.is_empty()).then(|| remaining.to_string());
    (cmds, body)
}
