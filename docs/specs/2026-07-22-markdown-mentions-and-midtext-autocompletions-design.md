# Design Doc: Markdown Mentions/Commands Styling & Mid-Text Autocompletions

**Date:** 2026-07-22  
**Author:** @Agy  
**Status:** Proposed  

## Context & Problem Statement
With the transition of `hadron-chamber`'s chat rendering to `TextView::markdown` (providing modern dark card code blocks with headers and copy buttons), `color_mentions` HTML `<span style="...">` tags are no longer styled by markdown inline parsing. As a result, `@mentions` and `/commands` render as plain uncolored text. Additionally, slash command autocompletions in `text.rs` were restricted to the start of the message (`idx == 0`), preventing mid-text command completion (e.g. typing `hello /team-brainstorm`).

## Goals
1. **Markdown Mention & Command Styling**: Automatically detect `@mentions` (e.g., `@Sonnet`, `@Agy`, `@team`, `@orchestrator`) and slash commands (e.g., `/team-brainstorm`, `/plan`, `/reboot`) inside `TextView::markdown` and style them with distinct theme colors and bold text marks.
2. **Mid-Text Command Autocomplete**: Enable slash command autocompletion when typing `/` anywhere at a word boundary (start of string or preceded by whitespace/newline) in the chat input.

## Detailed Design

### 1. Mid-Text Slash Command Autocomplete (`hadron-chamber`)
- **Location**: `crates/hadron-chamber/src/text.rs` -> `extract_completion_query(text: &str, offset: usize)`.
- **Change**: Replace `(c == '/' && idx == 0)` with:
  ```rust
  c == '/' && (idx == 0 || before_cursor[..idx].chars().next_back().map_or(false, |ch| ch.is_whitespace() || ch == '\n'))
  ```
- **Behavior**: Typing `hello /team-br` with cursor at index 14 will trigger `extract_completion_query` with `('/', "team-br", 6)`, displaying `/team-brainstorm` as a completion candidate. File paths like `src/app.rs` will not trigger commands because `/` is preceded by alphanumeric characters.

### 2. Native Mention & Command Highlighting in `TextView::markdown` (`gpui-component`)
- **Location**: `crates/gpui-component/crates/ui/src/text/format/markdown.rs` -> `parse_paragraph`.
- **Change**: When building `InlineNode` text runs within paragraphs:
  - Detect `@name` patterns (e.g., `@team`, `@orchestrator`, `@acp-claude`, `@Sonnet`). Apply `TextMark { color: Some(theme_color), bold: true, ... }` to the `@mention` text range.
  - Detect `/cmd` patterns at word boundaries. Apply `TextMark { color: Some(fuchsia-400), bold: true, ... }` to the `/command` text range.
- **Code block exemption**: Mentions or slashes inside inline code (` `code` `) or fenced code blocks (` ``` `) remain unstyled code text.

## Verification & Testing
1. **Unit Tests**:
   - Test `extract_completion_query` in `crates/hadron-chamber/src/text.rs` with mid-text slash commands (e.g. `"talking /team-brainstorm"`).
   - Test markdown parsing of mentions and slash commands in `gpui-component`.
2. **Integration Gates**:
   - `cargo test -p hadron-chamber --features gui`
   - `cargo test --workspace`
