use super::*;

impl super::Chamber {
    /// Put `text` in the chat box, park the cursor at its end and focus it.
    ///
    /// The titlebar's "Rename" uses this: `/rename` takes a name, and rather than grow
    /// a modal to ask for one, the menu hands the human a half-typed command in the
    /// surface they already type in.
    pub(super) fn prefill_chat_input(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_selected_range(end..end, cx);
        });
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

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
        self.chat_message_ixs = crate::model::chat_message_indices(&self.view.messages);
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
/// Peel the `/command` lines out of a submitted chat message. Returns the commands to
/// run in order — each as `(name, args)` — and the leftover message body.
///
/// A command is recognised at the start of **any line**, not only the first. It used to
/// be leading-only, which meant typing a paragraph and then `/team-brainstorm …` on its
/// own line silently posted the whole thing as chat and excited nobody. Two guards keep
/// that permissiveness honest:
///
/// - the token after the slash must be a name in [`crate::text::COMMANDS`], so a path
///   (`/usr/bin/foo`) or a stray slash stays ordinary text; and
/// - a line inside a ``` fence is never a command, because the human wrote it literally.
///   (The mention pass follows the same rule — see `color-mentions-ignores-code-blocks`.)
///
/// [`Arity::None`] commands chain with each other and with text on the same line;
/// [`Arity::Line`] commands take the rest of *their own line*, and following lines are
/// still scanned. Body lines are kept verbatim, so internal newlines, indentation and
/// markdown survive — the body is never rebuilt from whitespace-split tokens. `body` is
/// `None` when nothing but commands remain, which is how the caller knows to clear the
/// box and post nothing.
///
/// **A command absent from [`crate::text::COMMANDS`] is not "unhandled" — it is posted
/// as chat.** That was the whole failure mode when the parser kept its own lists:
/// `handle_chat_command` could have a perfectly good arm for a command that never
/// reached it. The table is now the single source of truth for the menu and for this
/// parser both; the remaining un-checkable link is the `match` in `handle_chat_command`,
/// which `every_listed_command_is_handled` guards.
pub(super) fn split_leading_commands(full: &str) -> (Vec<(String, String)>, Option<String>) {
    let mut cmds = Vec::new();
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_fence = false;

    // An UNBALANCED fence must not suppress commands. With a single stray ``` —
    // easily typed, and common when pasting a fragment — a toggle-on-first-sight
    // parser flips `in_fence` and never flips it back, so every command below it
    // is silently swallowed into the body. That is exactly the "offered but
    // becomes chat" failure this parser exists to prevent, reached by a different
    // door. So: fences only mask when they come in pairs. An odd count means the
    // human did not really fence anything, and we parse the message as prose.
    let fenced = full
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 0;

    for line in full.lines() {
        if fenced && line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            body_lines.push(line);
            continue;
        }
        if in_fence {
            body_lines.push(line);
            continue;
        }

        // Peel every command at the head of this line. `consumed` distinguishes
        // "this line began with a command" from "this line is plain text": only in
        // the first case may we push the trimmed remainder, since trimming a plain
        // line would eat markdown indentation.
        let mut rest = line;
        let mut consumed = false;
        loop {
            let head = rest.trim_start();
            let tok_end = head.find(char::is_whitespace).unwrap_or(head.len());
            let Some(cmd) = head[..tok_end]
                .strip_prefix('/')
                .filter(|c| !c.is_empty())
                .and_then(crate::text::command)
            else {
                break;
            };
            consumed = true;
            match cmd.arity {
                crate::text::Arity::None => {
                    cmds.push((cmd.name.to_string(), String::new()));
                    rest = &head[tok_end..];
                }
                crate::text::Arity::Line => {
                    cmds.push((cmd.name.to_string(), head[tok_end..].trim().to_string()));
                    rest = "";
                    break;
                }
            }
        }

        match (consumed, rest.trim().is_empty()) {
            (_, true) => {}
            (true, false) => body_lines.push(rest.trim()),
            (false, false) => body_lines.push(line),
        }
    }

    let body = body_lines.join("\n");
    let body = body.trim();
    let body = (!body.is_empty()).then(|| body.to_string());
    (cmds, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The titlebar menu's "New Session" and "Rename" are wrappers over `/clear` and
    /// `/rename`. Nothing in the type system ties the menu to the table, so renaming
    /// either row would leave a menu item that silently does nothing — this is the
    /// guard for that.
    #[test]
    fn the_titlebar_menu_wraps_commands_that_exist() {
        assert!(crate::text::command("clear").is_some(), "New Session drives /clear");
        let rename = crate::text::command("rename").expect("Rename drives /rename");
        // "Rename" prefills `/rename ` rather than running it, because the command
        // takes an argument. If that ever stops being true, the prefill is wrong.
        assert_eq!(rename.arity, crate::text::Arity::Line);
    }

    #[test]
    fn test_split_leading_commands_exit() {
        let (cmds, body) = split_leading_commands("/exit");
        assert_eq!(cmds, vec![("exit".to_string(), "".to_string())]);
        assert_eq!(body, None);

        let (cmds, body) = split_leading_commands("/quit");
        assert_eq!(cmds, vec![("quit".to_string(), "".to_string())]);
        assert_eq!(body, None);
    }

    #[test]
    fn test_split_leading_commands_rename_and_resume() {
        let (cmds, body) = split_leading_commands("/rename bugfix-router");
        assert_eq!(cmds, vec![("rename".to_string(), "bugfix-router".to_string())]);
        assert_eq!(body, None);

        let (cmds, body) = split_leading_commands("/resume 20260201_000000");
        assert_eq!(cmds, vec![("resume".to_string(), "20260201_000000".to_string())]);
        assert_eq!(body, None);
    }

    /// The reported bug: a command typed on its own line *after* some prose was
    /// swept into the message body and excited nobody.
    #[test]
    fn a_command_on_a_later_line_runs_and_the_prose_still_posts() {
        let (cmds, body) = split_leading_commands(
            "here are my thoughts\n/team-brainstorm commands, suggest improvements\nand a closing line",
        );
        assert_eq!(
            cmds,
            vec![(
                "team-brainstorm".to_string(),
                "commands, suggest improvements".to_string()
            )]
        );
        assert_eq!(
            body.as_deref(),
            Some("here are my thoughts\nand a closing line"),
            "the surrounding prose is still posted, and keeps its line structure"
        );
    }

    /// A human quoting a command in a fence wrote it literally; running it would be
    /// the same class of bug as substituting an `@mention` inside backticks.
    #[test]
    fn a_command_inside_a_fence_stays_text() {
        let (cmds, body) = split_leading_commands("look:\n```\n/clear\n```\ndone");
        assert!(cmds.is_empty(), "fenced command must not run: {cmds:?}");
        assert_eq!(body.as_deref(), Some("look:\n```\n/clear\n```\ndone"));
    }

    /// An unbalanced fence must not silently swallow the commands below it —
    /// found in review of `c3b978d`, and the same failure class the table fixes.
    #[test]
    fn an_unclosed_fence_does_not_swallow_later_commands() {
        let unclosed = "```\nsome code, never closed\n/team-brainstorm ship it\nmore text";
        let (cmds, body) = split_leading_commands(unclosed);
        assert_eq!(
            cmds,
            vec![("team-brainstorm".to_string(), "ship it".to_string())],
            "a stray ``` must not suppress a real command"
        );
        assert!(body.is_some());

        // A balanced pair still masks, which is the whole point of the guard.
        let closed = "```\n/clear\n```\nafter";
        let (cmds, _) = split_leading_commands(closed);
        assert!(cmds.is_empty(), "a closed fence still protects its contents: {cmds:?}");
    }

    /// Only names in the table are commands, so a path is not one.
    #[test]
    fn a_slash_that_is_not_a_command_stays_text() {
        let (cmds, body) = split_leading_commands("/usr/bin/env is where it lives");
        assert!(cmds.is_empty(), "a path must not parse as a command: {cmds:?}");
        assert_eq!(body.as_deref(), Some("/usr/bin/env is where it lives"));

        let (cmds, _) = split_leading_commands("/teamwork-preview how does this work?");
        assert!(
            cmds.is_empty(),
            "a retired command is text again, not a silent no-op: {cmds:?}"
        );
    }

    /// Zero-arg commands chained on one line, ahead of a message — the behaviour the
    /// original parser had, kept.
    #[test]
    fn zero_arg_commands_chain_ahead_of_a_message() {
        let (cmds, body) = split_leading_commands("/toggle-roster /clear ping the team");
        assert_eq!(
            cmds,
            vec![
                ("toggle-roster".to_string(), String::new()),
                ("clear".to_string(), String::new()),
            ]
        );
        assert_eq!(body.as_deref(), Some("ping the team"));
    }

    /// Plain prose keeps its indentation — the body is never rebuilt from tokens.
    #[test]
    fn an_indented_body_line_keeps_its_indentation() {
        let (_, body) = split_leading_commands("/clear\n- one\n    - nested");
        assert_eq!(body.as_deref(), Some("- one\n    - nested"));
    }

    /// **The guard that closes the loop the compiler cannot.** `COMMANDS` is the
    /// source of truth for the menu and this parser, but `handle_chat_command`'s
    /// `match` is plain control flow — nothing makes a table entry have an arm. This
    /// restates the handled set as a literal so that adding a row to `COMMANDS`
    /// without an arm fails the gate instead of shipping another silent no-op.
    ///
    /// Keep this list in sync with the `match` in `app::actions`, and nowhere else:
    /// it is a guard, not a second source.
    #[test]
    fn every_listed_command_is_handled() {
        const HANDLED: &[&str] = &[
            "help",
            "skills",
            "vocabulary",
            "exit",
            "quit",
            "toggle-roster",
            "toggle-inspector",
            "clear",
            "team-brainstorm",
            "brainstorm",
            "writing-plans",
            "executing-plans",
            "toggle",
            "rename",
            "resume",
            "reboot",
            "approve",
            "deny",
            "limit",
            "reset-energy",
            "learn",
            "learn-global",
            "learn-std-model",
            "learn-std-model-global",
        ];
        for cmd in crate::text::COMMANDS {
            assert!(
                HANDLED.contains(&cmd.name),
                "/{} is in COMMANDS but has no arm in handle_chat_command — \
                 it would be offered by the menu and silently do nothing",
                cmd.name
            );
        }
        for name in HANDLED {
            assert!(
                crate::text::command(name).is_some(),
                "handle_chat_command has an arm for /{name}, which is not in COMMANDS — \
                 the arm is unreachable"
            );
        }
    }

    /// Every command in the table round-trips through the parser with its declared
    /// arity. Proves the parser reads the table rather than a copy of it.
    #[test]
    fn every_command_in_the_table_parses() {
        for cmd in crate::text::COMMANDS {
            let line = format!("/{} some argument", cmd.name);
            let (cmds, body) = split_leading_commands(&line);
            assert_eq!(cmds.len(), 1, "/{} did not parse", cmd.name);
            assert_eq!(cmds[0].0, cmd.name);
            match cmd.arity {
                crate::text::Arity::Line => {
                    assert_eq!(cmds[0].1, "some argument", "/{} lost its argument", cmd.name);
                    assert_eq!(body, None);
                }
                crate::text::Arity::None => {
                    assert_eq!(cmds[0].1, "", "/{} must take no argument", cmd.name);
                    assert_eq!(body.as_deref(), Some("some argument"));
                }
            }
        }
    }
}
