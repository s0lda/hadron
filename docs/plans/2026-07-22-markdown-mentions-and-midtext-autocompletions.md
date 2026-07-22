# Markdown Mentions/Commands Styling & Mid-Text Autocompletions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore styling for `@mentions` and `/commands` inside `TextView::markdown` and enable mid-text `/command` autocompletions.

**Architecture:** Update `extract_completion_query` in `hadron-chamber` to recognize slash commands at any word boundary. Update `format/markdown.rs` in `gpui-component` to parse `@mention` and `/command` patterns within markdown text runs and apply styled `TextMark` spans with theme colors and bold styling.

**Tech Stack:** Rust, GPUI, `hadron-chamber`, `gpui-component`.

## Global Constraints

- Preserve `TextView::markdown` dark card code blocks with headers and copy buttons.
- Do not trigger slash commands on file paths like `src/app.rs`.
- All tests must pass in `cargo test -p hadron-chamber --features gui` and `cargo test --workspace`.

---

### Task 1: Mid-Text Slash Command Autocomplete

**Files:**
- Modify: `crates/hadron-chamber/src/text.rs:41-58`
- Test: `crates/hadron-chamber/src/text.rs:494-550`

**Interfaces:**
- Consumes: `extract_completion_query(text: &str, offset: usize) -> Option<(char, String, usize)>`
- Produces: Word-boundary `/` trigger detection for `completion_candidates`.

- [ ] **Step 1: Write failing unit test for mid-text slash command completion**

In `crates/hadron-chamber/src/text.rs` inside `mod tests`:
```rust
    #[test]
    fn finds_a_slash_command_trigger_mid_text() {
        assert_eq!(
            extract_completion_query("hello /team-br", 14),
            Some(('/', "team-br".to_string(), 6))
        );
        // File path should NOT trigger command completion
        assert_eq!(
            extract_completion_query("src/app.rs", 10),
            None
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p hadron-chamber --lib text::tests::finds_a_slash_command_trigger_mid_text`
Expected: FAIL with `left == None, right == Some(...)` because `/` mid-text was rejected by `idx == 0`.

- [ ] **Step 3: Update `extract_completion_query` to support word-boundary slashes**

In `crates/hadron-chamber/src/text.rs` replace line 49:
```rust
        let is_slash_trigger = c == '/' && (idx == 0 || before_cursor[..idx].chars().next_back().map_or(false, |ch| ch.is_whitespace() || ch == '\n'));
        if c == '@' || c == ':' || is_slash_trigger {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((c, query, idx));
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p hadron-chamber --lib text::tests::finds_a_slash_command_trigger_mid_text`
Expected: PASS

- [ ] **Step 5: Run full hadron-chamber test gate**

Run: `cargo test -p hadron-chamber --features gui`
Expected: PASS (122 tests passed)

- [ ] **Step 6: Commit**

```bash
git add crates/hadron-chamber/src/text.rs
git commit -m "feat(chamber): support mid-text slash command autocompletions at word boundaries"
```

---

### Task 2: Native Mention & Slash Command Highlighting in Markdown Parser

**Files:**
- Modify: `crates/gpui-component/crates/ui/src/text/format/markdown.rs:166-240`
- Test: `crates/gpui-component/crates/ui/src/text/format/markdown.rs`

**Interfaces:**
- Consumes: `Paragraph`, `InlineNode`, `TextMark`
- Produces: Colored and bold `TextMark` ranges for `@mentions` and `/commands` within markdown text runs.

- [ ] **Step 1: Write unit test for markdown mention and command parsing**

In `crates/gpui-component/crates/ui/src/text/format/markdown.rs` inside `mod tests`:
```rust
    #[test]
    fn parses_mentions_and_slash_commands_in_markdown() {
        let mut cx = NodeContext::default();
        let theme = HighlightTheme::default();
        let doc = parse("Hello @Sonnet and /team-brainstorm!", &mut cx, &theme).unwrap();
        assert!(!doc.blocks.is_empty());
    }
```

- [ ] **Step 2: Implement `@mention` and `/command` text mark styling in `parse_paragraph`**

In `crates/gpui-component/crates/ui/src/text/format/markdown.rs`, when parsing plain text nodes (`Node::Text`):
Scan the text for `@mention` tokens and `/command` tokens (at word boundaries), emitting `InlineNode` runs with `TextMark { bold: true, color: Some(hsla), ... }`.

- [ ] **Step 3: Verify tests pass**

Run: `cargo test -p hadron-chamber --features gui`
Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
cd /home/Jake/dev/hadron/crates/gpui-component && git add . && git commit -m "feat(ui): add native mention and command highlighting in markdown parser"
cd /home/Jake/dev/hadron/.hadron/trees/cli-agy && git add . && git commit -m "feat(chamber): wire native markdown mentions styling"
```
