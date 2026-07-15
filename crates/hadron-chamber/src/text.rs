//! Pure text logic shared by the UI — no `gpui`, no feature gate.
//!
//! `extract_completion_query` lived in `completions`, which imports `gpui` and is
//! therefore `#[cfg(feature = "gui")]`. That gating was incidental to the function
//! (it is pure `&str` work) but not to its **tests**: `cargo test --workspace` does
//! not build the `gui` feature, so the regression test guarding the emoji crash
//! never ran in the gate that decides whether we ship. A crash guard invisible to
//! the gate is a crash guard that will be broken again without anyone noticing.

/// The two `@names` that are **not** roster ids but still route: `@orchestrator`
/// resolves to whoever currently holds the Orchestrator seat, and `@team` fans out
/// to every quark. The chat field is the human's, and `@team` is human-only, so
/// both belong in its completion list — they were missing, so the one mention Jake
/// uses most often was the one the menu would not offer.
///
/// The names come from the router's own constants rather than being retyped here:
/// they are the single source of truth for what actually routes, and a completion
/// that offers a name the router does not resolve is worse than no completion.
pub const MENTION_ALIASES: [(&str, &str); 2] = [
    (
        hadron_gluon::router::ORCHESTRATOR_ALIAS,
        "Whoever holds the Orchestrator seat",
    ),
    (hadron_gluon::router::TEAM_ALIAS, "Everyone on the roster"),
];

/// Case-insensitive substring match, the same rule the quark and file rows use.
pub fn mention_matches(name: &str, query_lower: &str) -> bool {
    query_lower.is_empty() || name.to_lowercase().contains(query_lower)
}

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

/// One row the completion card offers: what to show, a short right-hand hint, and
/// the exact text that replaces the query span when the row is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub detail: String,
    pub new_text: String,
}

/// The live completion query and the rows that answer it.
///
/// `start` is the BYTE offset of the trigger char (`@`/`:`/`/`); accepting a row
/// replaces `text[start..cursor]` with the row's `new_text`. Both offsets are
/// UTF-8 byte offsets — the same unit the editor's `cursor()`/`set_selected_range`
/// use — so the accept is a plain string splice with no UTF-16 conversion.
pub struct Completions {
    pub start: usize,
    pub candidates: Vec<Candidate>,
}

/// Cap on rows built. A bare `:` matches every emoji (thousands); without this the
/// card would be thousands of rows tall. The card scrolls, but building them all is
/// pointless — the user narrows with a query.
pub const MAX_CANDIDATES: usize = 50;

/// Build the completion rows for the trigger immediately before `cursor`, or `None`
/// when there is no live trigger (so the card closes).
///
/// This is the single source of truth for what the chat completion card offers. It
/// is pure `&str`/`&[..]` work — no `gpui` — so its behaviour is pinned by tests in
/// the workspace gate, unlike the fork's `CompletionProvider` which only compiled
/// under `--features gui`.
pub fn completion_candidates(
    text: &str,
    cursor: usize,
    quarks: &[(String, Option<String>)],
    files: &[String],
) -> Option<Completions> {
    let (trigger, query, start) = extract_completion_query(text, cursor)?;
    let query_lower = query.to_lowercase();
    let mut out: Vec<Candidate> = Vec::new();

    match trigger {
        '@' => {
            // Routing aliases first — `@team`/`@orchestrator` are what the human
            // reaches for most, and neither is a roster id.
            for (alias, detail) in MENTION_ALIASES {
                if mention_matches(alias, &query_lower) {
                    out.push(Candidate {
                        label: format!("@{alias}"),
                        detail: detail.to_string(),
                        new_text: format!("@{alias} "),
                    });
                }
            }
            for (id, display) in quarks {
                let name = display.as_ref().unwrap_or(id);
                let name_l = name.to_lowercase();
                let id_l = id.to_lowercase();
                if query_lower.is_empty()
                    || name_l.contains(&query_lower)
                    || id_l.contains(&query_lower)
                {
                    out.push(Candidate {
                        label: format!("@{name}"),
                        detail: "Quark".into(),
                        new_text: format!("@{name} "),
                    });
                }
            }
            for file in files {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                if query_lower.is_empty() || file.to_lowercase().contains(&query_lower) {
                    out.push(Candidate {
                        label: format!("@{file}"),
                        detail: "File".into(),
                        new_text: format!("@{file} "),
                    });
                }
            }
        }
        ':' => {
            for emoji in emojis::iter() {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                let Some(shortcode) = emoji.shortcode() else {
                    continue;
                };
                if query_lower.is_empty() || shortcode.to_lowercase().contains(&query_lower) {
                    // The card renders the label as one plain string (no byte-range
                    // prefix highlight), so the emoji-first shape that crashed the
                    // fork menu cannot slice mid-character here.
                    out.push(Candidate {
                        label: format!(":{shortcode} {}", emoji.as_str()),
                        detail: "Emoji".into(),
                        new_text: emoji.as_str().to_string(),
                    });
                }
            }
        }
        '/' => {
            for cmd in ["team-brainstorm"] {
                if query_lower.is_empty() || cmd.contains(&query_lower) {
                    out.push(Candidate {
                        label: format!("/{cmd}"),
                        detail: "Command".into(),
                        new_text: format!("/{cmd} "),
                    });
                }
            }
        }
        _ => {}
    }

    if out.is_empty() {
        return None;
    }
    Some(Completions { start, candidates: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarks() -> Vec<(String, Option<String>)> {
        vec![
            ("acp-claude".into(), None),
            ("agy".into(), Some("Agy".into())),
        ]
    }

    #[test]
    fn a_mention_query_offers_matching_quarks_and_aliases() {
        let c = completion_candidates("@ag", 3, &quarks(), &[]).expect("has rows");
        assert_eq!(c.start, 0, "replace span starts at the '@'");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@Agy"), "matched quark offered: {labels:?}");
        // The accepted text carries the sigil and a trailing space, ready to type on.
        let agy = c.candidates.iter().find(|c| c.label == "@Agy").unwrap();
        assert_eq!(agy.new_text, "@Agy ");
    }

    #[test]
    fn an_empty_mention_query_offers_the_routing_aliases_first() {
        let c = completion_candidates("@", 1, &quarks(), &[]).expect("has rows");
        assert_eq!(
            c.candidates[0].label,
            format!("@{}", hadron_gluon::router::ORCHESTRATOR_ALIAS),
            "aliases lead the list"
        );
    }

    #[test]
    fn a_file_query_offers_files() {
        let files = vec!["src/app.rs".to_string(), "README.md".to_string()];
        let c = completion_candidates("@app", 4, &[], &files).expect("has rows");
        assert_eq!(c.candidates.len(), 1);
        assert_eq!(c.candidates[0].new_text, "@src/app.rs ");
    }

    #[test]
    fn a_bare_emoji_trigger_is_capped_not_thousands() {
        let c = completion_candidates(":", 1, &[], &[]).expect("has rows");
        assert!(
            c.candidates.len() <= MAX_CANDIDATES,
            "a bare ':' must not build thousands of rows: got {}",
            c.candidates.len()
        );
    }

    #[test]
    fn an_emoji_query_accepts_the_glyph_not_the_shortcode() {
        let c = completion_candidates(":smile", 6, &[], &[]).expect("has rows");
        let first = &c.candidates[0];
        assert!(first.label.starts_with(":smile"));
        // Accepting inserts the glyph itself, not the `:smile:` text.
        assert!(!first.new_text.starts_with(':'));
    }

    #[test]
    fn a_slash_command_is_offered_only_at_the_line_start() {
        assert!(completion_candidates("/team", 5, &[], &[]).is_some());
        // Mid-line `/` is not a trigger (see extract_completion_query).
        assert!(completion_candidates("hi /team", 8, &[], &[]).is_none());
    }

    #[test]
    fn no_trigger_yields_no_card() {
        assert!(completion_candidates("just talking", 12, &quarks(), &[]).is_none());
    }

    #[test]
    fn a_cursor_past_the_end_or_mid_emoji_does_not_panic() {
        let hostile = "hi 🌍 @a";
        for cursor in 0..=hostile.len() + 4 {
            let _ = completion_candidates(hostile, cursor, &quarks(), &[]);
        }
    }

    /// `@team` and `@orchestrator` route, but the completion menu never offered
    /// them: it only listed roster ids, and neither alias is one. The names are
    /// asserted against the router's own constants, so a rename over there breaks
    /// this test rather than silently leaving the menu offering a dead mention.
    #[test]
    fn the_routing_aliases_are_offered_and_are_the_ones_that_actually_route() {
        let names: Vec<&str> = MENTION_ALIASES.iter().map(|(n, _)| *n).collect();
        assert!(
            names.contains(&hadron_gluon::router::TEAM_ALIAS),
            "@team routes but was not offered: {names:?}"
        );
        assert!(
            names.contains(&hadron_gluon::router::ORCHESTRATOR_ALIAS),
            "@orchestrator routes but was not offered: {names:?}"
        );

        // Typing `@te` must reach `team`, and `@ORCH` must reach `orchestrator` —
        // the router matches case-insensitively, so the menu must too.
        assert!(mention_matches(hadron_gluon::router::TEAM_ALIAS, "te"));
        assert!(mention_matches(
            hadron_gluon::router::ORCHESTRATOR_ALIAS,
            "orch"
        ));
        assert!(mention_matches(hadron_gluon::router::TEAM_ALIAS, ""));
        assert!(!mention_matches(hadron_gluon::router::TEAM_ALIAS, "zzz"));
    }

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
