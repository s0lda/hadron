//! In-Flight Context Budget Auto-Governor.
//!
//! Compacts verbose tool stdout/stderr (e.g. massive compiler dumps, long test backtraces)
//! into concise, panic/error-preserving summaries before appending to the turn context.

#[derive(Debug, Clone)]
pub struct GovernorConfig {
    pub max_tool_bytes: usize,
    pub preserve_head_lines: usize,
    pub preserve_tail_lines: usize,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            max_tool_bytes: 4096,
            preserve_head_lines: 10,
            preserve_tail_lines: 15,
        }
    }
}

pub struct ContextGovernor;

impl ContextGovernor {
    /// Checks if a line contains critical diagnostic signals that should always be retained.
    pub fn is_critical_line(line: &str) -> bool {
        let l = line.trim();
        l.starts_with("error:")
            || l.starts_with("error[")
            || l.contains("panicked at")
            || l.contains("FAILED")
            || l.contains("test result:")
            || l.starts_with("Caused by:")
            || l.starts_with("Assertion failed")
    }

    /// Strong signal that a line comes from compiler, test-runner, or terminal command output.
    fn is_terminal_signal_line(line: &str) -> bool {
        let l = line.trim();
        l.starts_with("Compiling ")
            || l.starts_with("Checking ")
            || l.starts_with("Finished ")
            || l.starts_with("Running ")
            || l.starts_with("Updating ")
            || l.starts_with("Downloading ")
            || l.starts_with("Downloaded ")
            || l.starts_with("Doc-tests ")
            || l.starts_with("warning:")
            || l.starts_with("warning[")
            || l.starts_with("error:")
            || l.starts_with("error[")
            || l.starts_with("--> ")
            || l.starts_with("::: ")
            || l.starts_with("= note:")
            || l.starts_with("= help:")
            || l.contains("panicked at")
            || l.starts_with("test result:")
            || (l.starts_with("running ") && (l.ends_with(" test") || l.ends_with(" tests")))
            || (l.starts_with("test ")
                && (l.ends_with(" ... ok")
                    || l.ends_with(" ... FAILED")
                    || l.ends_with(" ... ignored")))
            || l.starts_with("wsl: Failed to mount")
            || l.contains("  chamber  pty: ")
    }

    /// Returns true when `line` is a valid continuation line inside a contiguous terminal/compiler
    /// output dump (such as rustc source snippets, gutter bars, caret underlines, or notes)
    /// and NOT Markdown prose, headings, lists, tables, or slash-command documentation.
    fn is_terminal_continuation_line(line: &str) -> bool {
        if Self::is_terminal_signal_line(line) {
            return true;
        }
        let l = line.trim();
        if l.is_empty() {
            return false;
        }
        // Never treat Markdown headings, bold bullets, @mentions, slash commands, or tables as dump lines.
        if l.starts_with('#')
            || l.starts_with("**")
            || l.starts_with("- **")
            || l.starts_with("* **")
            || l.starts_with('@')
            || l.starts_with("- `/")
            || l.starts_with('/')
            || (l.starts_with('|') && l.ends_with('|') && l.len() > 2)
        {
            return false;
        }
        // Rustc diagnostic gutter lines: `|`, `| ...`, or `123 | ...`
        if l == "|" || (l.starts_with("| ") && !l.ends_with('|')) {
            return true;
        }
        if let Some((prefix, _rest)) = l.split_once('|') {
            let p = prefix.trim();
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return true;
            }
        }
        if l == "..."
            || l.starts_with("note:")
            || l.starts_with("help:")
            || l.starts_with("For more information about this error")
            || l.starts_with("Some errors have detailed explanations")
            || l.starts_with("Caused by:")
            || l.starts_with("Log: file://")
            || l.starts_with("<truncated ")
            || l.starts_with("The command exited with code")
            || l == "Output:"
        {
            return true;
        }
        // Indented compiler/test continuation lines (e.g. `   Compiling ...` or rustc help blocks)
        let leading_spaces = line.len().saturating_sub(line.trim_start_matches(' ').len());
        if leading_spaces >= 2
            && !l.starts_with("- ")
            && !l.starts_with("* ")
            && !l.chars().next().is_some_and(|c| c.is_ascii_digit() && l.contains(". "))
        {
            return true;
        }
        false
    }

    /// Returns true when `raw` is a standalone tool dump OR contains `<SYSTEM_MESSAGE>` wakeup
    /// blocks or an embedded contiguous terminal/compiler dump block that requires governance.
    pub fn is_raw_tool_dump(raw: &str) -> bool {
        if raw.contains("<SYSTEM_MESSAGE>") && raw.contains("</SYSTEM_MESSAGE>") {
            return true;
        }
        if Self::govern_message(raw, &GovernorConfig::default()) != raw {
            return true;
        }
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#')
            || trimmed.starts_with("**")
            || trimmed.starts_with('@')
            || trimmed.starts_with("- `/")
            || trimmed.starts_with('/')
        {
            return false;
        }
        let mut tool_signal_lines = 0usize;
        let mut markdown_lines = 0usize;
        for line in raw.lines() {
            let l = line.trim();
            if l.starts_with('#') || l.starts_with("- **") || (l.starts_with("| ") && l.ends_with('|')) {
                markdown_lines += 1;
            }
            if Self::is_terminal_signal_line(line) {
                tool_signal_lines += 1;
            }
        }
        tool_signal_lines >= 3 && markdown_lines == 0
    }

    fn compact_lines_slice(lines: &[&str], config: &GovernorConfig) -> Vec<String> {
        let total_lines = lines.len();
        if total_lines <= config.preserve_head_lines + config.preserve_tail_lines {
            return lines.iter().map(|s| (*s).to_string()).collect();
        }

        let head = &lines[..config.preserve_head_lines];
        let tail = &lines[total_lines - config.preserve_tail_lines..];
        let middle = &lines[config.preserve_head_lines..total_lines - config.preserve_tail_lines];

        // Harvest critical error/panic lines (plus an immediate `--> ` location line if present).
        let mut critical_middle = Vec::new();
        let mut idx = 0;
        while idx < middle.len() {
            if Self::is_critical_line(middle[idx]) {
                critical_middle.push(middle[idx]);
                if idx + 1 < middle.len() && middle[idx + 1].trim_start().starts_with("--> ") {
                    critical_middle.push(middle[idx + 1]);
                    idx += 1;
                }
            }
            idx += 1;
        }

        let omitted_count = middle.len().saturating_sub(critical_middle.len());
        if omitted_count == 0 {
            return lines.iter().map(|s| (*s).to_string()).collect();
        }

        let mut out = Vec::with_capacity(
            config.preserve_head_lines + config.preserve_tail_lines + critical_middle.len() + 3,
        );
        for line in head {
            out.push((*line).to_string());
        }
        out.push(String::new());
        out.push(format!(
            "[... ContextGovernor: omitted {} verbose output lines ...]",
            omitted_count
        ));
        out.push(String::new());
        for line in critical_middle {
            out.push(line.to_string());
        }
        for line in tail {
            out.push((*line).to_string());
        }
        out
    }

    /// Compacts raw tool stdout/stderr according to governor budget limits.
    pub fn compact_tool_output(raw: &str, config: &GovernorConfig) -> String {
        if raw.len() <= config.max_tool_bytes {
            return raw.to_string();
        }

        let lines: Vec<&str> = raw.lines().collect();
        let total_lines = lines.len();

        if total_lines <= config.preserve_head_lines + config.preserve_tail_lines {
            return raw.to_string();
        }

        let mut out = Self::compact_lines_slice(&lines, config).join("\n");
        out.push('\n');
        out
    }

    /// Strip Antigravity SDK `<SYSTEM_MESSAGE>...</SYSTEM_MESSAGE>` background-task wakeup
    /// blocks (and their `!The system automatically notified us of a message:` headers) when
    /// followed by real assistant response text, or compact them if the message contains
    /// nothing else. Also trims orphaned mid-turn step narration preceding a complete
    /// structured report (`**Done**:` + `- **Done**:` + `- **Evidence**:`) and redundant
    /// trailing 1-line `**Done**: Task complete...` sign-off stubs after a structured report.
    fn strip_or_compact_system_messages(raw: &str, config: &GovernorConfig) -> (String, bool) {
        if !raw.contains("<SYSTEM_MESSAGE>") || !raw.contains("</SYSTEM_MESSAGE>") {
            return (raw.to_string(), false);
        }

        let lines: Vec<&str> = raw.lines().collect();
        let mut outside_lines: Vec<&str> = Vec::new();
        let mut inside_lines: Vec<&str> = Vec::new();
        let mut in_sys = false;
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if !in_sys {
                if trimmed == "!The system automatically notified us of a message:"
                    && i + 1 < lines.len()
                    && lines[i + 1].trim().starts_with("<SYSTEM_MESSAGE>")
                {
                    in_sys = true;
                    i += 2;
                    continue;
                }
                if trimmed.starts_with("<SYSTEM_MESSAGE>") {
                    in_sys = true;
                    i += 1;
                    continue;
                }
                outside_lines.push(lines[i]);
            } else if let Some(pos) = trimmed.find("</SYSTEM_MESSAGE>") {
                in_sys = false;
                let after = trimmed[pos + "</SYSTEM_MESSAGE>".len()..]
                    .trim_start_matches(['-', '+', ' ']);
                if !after.is_empty() {
                    outside_lines.push(after);
                }
            } else {
                inside_lines.push(lines[i]);
            }
            i += 1;
        }

        let has_outside = outside_lines.iter().any(|l| !l.trim().is_empty());
        if !has_outside {
            let inner = inside_lines.join("\n");
            return (Self::compact_tool_output(&inner, config), true);
        }

        // If `outside_lines` contains a full structured report (`**Done**:` followed by
        // `- **Done**:` and `- **Evidence**:`), drop any orphaned mid-turn step chatter
        // that preceded the report between `<SYSTEM_MESSAGE>` blocks.
        let cleaned = Self::isolate_structured_report_if_present(&outside_lines);
        let mut result = cleaned.join("\n");
        if raw.ends_with('\n') && !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        (result, true)
    }

    /// When a multi-step turn contains a complete structured report (`**Done**:` with
    /// `- **Done**:` and `- **Evidence**:`), find the start of that report (including its
    /// leading `@mention` line if present) and strip any trailing 1-2 line
    /// `@...\n**Done**: Task complete...` stubs that lack bullets after the report.
    pub fn isolate_structured_report_if_present<'a>(lines: &[&'a str]) -> Vec<&'a str> {
        let has_done_bullet = lines.iter().any(|l| l.trim().starts_with("- **Done**:"));
        let has_evidence_bullet = lines
            .iter()
            .any(|l| l.trim().starts_with("- **Evidence**:"));
        if !has_done_bullet || !has_evidence_bullet {
            return lines.to_vec();
        }

        let done_bullet_ix = lines
            .iter()
            .position(|l| l.trim().starts_with("- **Done**:"))
            .unwrap_or(0);

        // Find the `**Done**:` headline immediately preceding `done_bullet_ix`.
        let mut report_start = 0;
        for idx in (0..done_bullet_ix).rev() {
            let t = lines[idx].trim();
            if t.starts_with("**Done**:") {
                report_start = idx;
                if idx > 0 && lines[idx - 1].trim().starts_with('@') {
                    report_start = idx - 1;
                }
                break;
            }
        }

        let mut slice = &lines[report_start..];

        // Strip trailing redundant `@mention` + `**Done**: Task complete...` 1-2 line stubs
        // that appear after the structured report's bullets.
        let last_bullet_ix = slice
            .iter()
            .rposition(|l| {
                let t = l.trim();
                t.starts_with("- **") || t.starts_with("  - ")
            })
            .unwrap_or(slice.len().saturating_sub(1));

        let mut trim_end = slice.len();
        for idx in (last_bullet_ix + 1)..slice.len() {
            let t = slice[idx].trim();
            if t.starts_with('@')
                && idx + 1 < slice.len()
                && slice[idx + 1].trim().starts_with("**Done**:")
            {
                let rest_has_bullets = slice[idx + 1..]
                    .iter()
                    .any(|l| l.trim().starts_with("- **Done**:"));
                if !rest_has_bullets {
                    trim_end = idx;
                    break;
                }
            } else if t.starts_with("**Done**:") && idx > report_start + 1 {
                let rest_has_bullets = slice[idx..]
                    .iter()
                    .any(|l| l.trim().starts_with("- **Done**:"));
                if !rest_has_bullets {
                    trim_end = idx;
                    break;
                }
            }
        }
        slice = &slice[..trim_end];
        while slice.last().is_some_and(|l| l.trim().is_empty()) {
            slice = &slice[..slice.len() - 1];
        }
        slice.to_vec()
    }

    /// Surgically govern a message:
    /// 1. Strip or compact Antigravity `<SYSTEM_MESSAGE>...</SYSTEM_MESSAGE>` wakeup blocks.
    /// 2. Trim trailing redundant `@mention\n**Done**: Task complete...` stubs after a full
    ///    structured report.
    /// 3. Compact any contiguous terminal/compiler/test dump spans (`> preserve_head + preserve_tail`
    ///    lines with `>= 3` strong terminal signal lines) in-place while preserving all
    ///    surrounding Markdown prose, `@mentions`, `/commands`, tables, and source code verbatim.
    pub fn govern_message(raw: &str, config: &GovernorConfig) -> String {
        if raw.trim().is_empty() {
            return raw.to_string();
        }

        let (after_sys, _) = Self::strip_or_compact_system_messages(raw, config);
        let raw_lines: Vec<&str> = after_sys.lines().collect();

        // Also strip trailing redundant `**Done**: Task complete` stubs even when no
        // `<SYSTEM_MESSAGE>` block was present.
        let lines = if raw_lines.iter().any(|l| l.trim().starts_with("- **Done**:"))
            && raw_lines
                .iter()
                .filter(|l| l.trim().starts_with("**Done**:"))
                .count()
                >= 2
        {
            Self::isolate_structured_report_if_present(&raw_lines)
        } else {
            raw_lines
        };

        let min_span_lines = config.preserve_head_lines + config.preserve_tail_lines;
        if lines.len() <= min_span_lines {
            let mut out = lines.join("\n");
            if raw.ends_with('\n') && !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            return out;
        }

        let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
        let mut i = 0;
        while i < lines.len() {
            if Self::is_terminal_signal_line(lines[i]) {
                let start = i;
                let mut end = i + 1;
                let mut signal_count = 1usize;

                while end < lines.len() {
                    if Self::is_terminal_continuation_line(lines[end]) {
                        if Self::is_terminal_signal_line(lines[end]) {
                            signal_count += 1;
                        }
                        end += 1;
                    } else if lines[end].trim().is_empty()
                        && end + 1 < lines.len()
                        && Self::is_terminal_continuation_line(lines[end + 1])
                    {
                        end += 1;
                    } else {
                        break;
                    }
                }

                let span = &lines[start..end];
                let span_bytes: usize = span.iter().map(|l| l.len() + 1).sum();
                if signal_count >= 3
                    && span.len() > min_span_lines
                    && (span_bytes > config.max_tool_bytes || span.len() > min_span_lines + 10)
                {
                    out_lines.extend(Self::compact_lines_slice(span, config));
                    i = end;
                    continue;
                }
            }

            out_lines.push(lines[i].to_string());
            i += 1;
        }

        let mut result = out_lines.join("\n");
        if raw.ends_with('\n') && !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_governor_short_output() {
        let short = "Everything passed cleanly in 0.05s.";
        let config = GovernorConfig::default();
        let result = ContextGovernor::compact_tool_output(short, &config);
        assert_eq!(result, short);
    }

    #[test]
    fn test_context_governor_compacts_and_retains_critical_lines() {
        let mut lines = Vec::new();
        for i in 0..100 {
            if i == 50 {
                lines.push("error[E0308]: mismatched types expected usize, found u32");
            } else if i == 51 {
                lines.push("panicked at src/main.rs:12: assertion failed");
            } else {
                lines.push("Compiling some_dependency v0.1.0 ...");
            }
        }
        let raw = lines.join("\n");
        let config = GovernorConfig {
            max_tool_bytes: 200,
            preserve_head_lines: 5,
            preserve_tail_lines: 5,
        };

        let compacted = ContextGovernor::compact_tool_output(&raw, &config);
        assert!(compacted.len() < raw.len());
        assert!(compacted.contains("ContextGovernor: omitted"));
        assert!(compacted.contains("error[E0308]: mismatched types"));
        assert!(compacted.contains("panicked at src/main.rs:12"));
    }

    #[test]
    fn test_is_raw_tool_dump_distinguishes_compiler_output_from_markdown_and_commands() {
        let compiler_dump = "Compiling foo v0.1.0\nChecking bar v0.2.0\nerror[E0308]: mismatched types\nFinished dev profile";
        assert!(ContextGovernor::is_raw_tool_dump(compiler_dump));

        let commands_help = "/clear — archive the field\n/commands — list all commands\n/team-brainstorm — brainstorm with the team";
        assert!(!ContextGovernor::is_raw_tool_dump(commands_help));

        let brainstorm = "@orchestrator\n### 1. Root Cause Analysis\n- **Done**: verified";
        assert!(!ContextGovernor::is_raw_tool_dump(brainstorm));
    }

    #[test]
    fn test_govern_message_strips_agy_system_message_and_trailing_done_stubs() {
        let mut sys_dump = String::from(
            "!The system automatically notified us of a message:\n\
             <SYSTEM_MESSAGE>\n\
             [Message] timestamp=2026-11-04T20:41:25Z sender=task-201 priority=MESSAGE_PRIORITY_HIGH content=Task finished:\n\
             The command exited with code 0.\n\
             Output:\n",
        );
        for i in 0..45 {
            sys_dump.push_str(&format!("   Compiling dep_{i} v0.1.0\n"));
        }
        sys_dump.push_str(
            "</SYSTEM_MESSAGE>---------------+\n\
             `cargo check --tests --workspace` EXIT CODE 0!\n\
             @Agy @orchestrator\n\
             **Done**: Audited and decoupled UI-thread render/settings paths in commit `eb44760a`.\n\n\
             - **Done**:\n\
               - Cached 3-way diff hunks in `diff_inspector.rs`.\n\
             - **Evidence**:\n\
               - `cargo test -p hadron`: Passed.\n\
             - **Risks**: No new attack surface.\n\
             @Agy @orchestrator\n\
             **Done**: Task complete. Latest commit `eb44760a` verified in log.\n\
             @Agy @orchestrator\n\
             **Done**: Tasks cleaned up. Ready for next instructions.",
        );

        let governed = ContextGovernor::govern_message(&sys_dump, &GovernorConfig::default());
        assert!(!governed.contains("<SYSTEM_MESSAGE>"));
        assert!(!governed.contains("Compiling dep_"));
        assert!(!governed.contains("EXIT CODE 0!"));
        assert!(!governed.contains("Ready for next instructions."));
        assert!(governed.starts_with("@Agy @orchestrator\n**Done**: Audited and decoupled"));
        assert!(governed.contains("- **Evidence**:"));
    }

    #[test]
    fn test_govern_message_compacts_embedded_compiler_dump_inside_mention_message() {
        let mut msg = String::from(
            "@acp-agy The merge gate stopped this branch from landing. Nothing was merged and nothing was lost.\n\n\
             wsl: Failed to mount Z:\\, see dmesg for more details.\n",
        );
        for i in 0..40 {
            msg.push_str(&format!(
                "warning: function `unused_{i}` is never used\n  --> crates/hadron-chamber/src/text.rs:{i}:8\n   |\n{i} | pub fn unused_{i}() {{}}\n   |        ^^^^^^^^^\n\n"
            ));
        }
        msg.push_str(
            "error[E0560]: struct `ScheduledJob` has no field named `prompt`\n  --> crates/hadron-chamber/src/app/settings/tests.rs:14:9\n\n\
             You are still on your own branch in your own worktree — fix it THERE.",
        );

        let governed = ContextGovernor::govern_message(&msg, &GovernorConfig::default());
        assert!(governed.starts_with("@acp-agy The merge gate stopped this branch"));
        assert!(governed.contains("[... ContextGovernor: omitted"));
        assert!(governed.contains("error[E0560]: struct `ScheduledJob` has no field named `prompt`"));
        assert!(governed.contains("--> crates/hadron-chamber/src/app/settings/tests.rs:14:9"));
        assert!(governed.ends_with("You are still on your own branch in your own worktree — fix it THERE."));
        assert!(governed.len() < msg.len() / 2);
    }
}
