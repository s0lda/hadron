//! The engine's hard cap on how long a quark's reply may be.
//!
//! Asking a model to be brief is prompt text, and prompt text does not enforce: every
//! quark in this swarm has been told to lead with the outcome, and every quark still
//! files a thousand-word report. The human reads the field, and the field is where the
//! cost lands — so the cap is applied *here*, in the engine, to the one place a reply
//! enters the field ([`crate::engine::Engine::finish_turn`]), where it is a fact about
//! the bytes rather than a request the model can talk itself out of.
//!
//! Two rules make this safe to do to somebody else's words:
//!
//! 1. **Routing survives, always.** `parse_addressee` routes on the FIRST line-leading
//!    `@mention`, and every worker hands back with a trailing `@orchestrator` line. Cut
//!    that line and the reply excites nobody: the work stops dead in the field and no
//!    compiler or test would ever say so. So mention lines are kept whatever the budget,
//!    and kept in their original order, which is what preserves the addressee.
//! 2. **Nothing is destroyed.** The untrimmed reply rides on the event envelope
//!    (`Event.full`), so the field on disk keeps every word. What is capped is what the
//!    human and the other quarks are made to *read*.

/// Lines a reply may have before the engine cuts it. A report that leads with the
/// outcome fits; a bible does not.
pub const MAX_LINES: usize = 14;

/// Characters a reply may have before the engine cuts it — a line budget alone is
/// trivially defeated by one enormous paragraph.
pub const MAX_CHARS: usize = 1000;

/// A reply as the field will actually see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    /// What goes in `Kind::Message` — within budget, and still routable.
    pub body: String,
    /// The original, when it did not fit. `None` means nothing was cut, and the
    /// distinction is the point: absent is not "trimmed to the same thing".
    pub full: Option<String>,
}

/// True when a line routes work: a line-leading `@mention`. Deliberately syntactic and
/// deliberately WIDER than `parse_addressee` (which also resolves the id against the
/// roster) — a mention of a quark that is not seated still carries the human's intent,
/// and the cost of keeping one extra line is a line. The cost of dropping the real one
/// is a turn that reports to nobody.
fn routes(line: &str) -> bool {
    line.trim_start()
        .strip_prefix('@')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_alphanumeric()))
}

/// Cap a quark's reply. Under budget, it is returned untouched — the common case must
/// be a no-op, or the engine is rewriting words nobody asked it to rewrite.
pub fn enforce(body: &str) -> Brief {
    let lines: Vec<&str> = body.lines().collect();
    if lines.len() <= MAX_LINES && body.chars().count() <= MAX_CHARS {
        return Brief { body: body.to_string(), full: None };
    }

    // The head, in order, until either budget runs out. Cutting on a line boundary is
    // what makes the result read as a message rather than as a truncated string.
    let mut kept: Vec<&str> = Vec::new();
    let mut chars = 0usize;
    for line in &lines {
        if kept.len() >= MAX_LINES || chars + line.chars().count() > MAX_CHARS {
            break;
        }
        chars += line.chars().count() + 1;
        kept.push(line);
    }

    // Every routing line the head did not reach, in original order. This is the rule
    // that keeps a trimmed report from silently unaddressing itself.
    let dropped_mentions: Vec<&str> =
        lines.iter().skip(kept.len()).copied().filter(|l| routes(l)).collect();

    let mut out = kept.join("\n");

    // A cut inside a fenced block leaves the fence open, and every line after it in the
    // chamber renders as code. Close it rather than hand the reader a broken document.
    if kept.iter().filter(|l| l.trim_start().starts_with("```")).count() % 2 == 1 {
        out.push_str("\n```");
    }

    let cut_lines = lines.len() - kept.len();
    out.push_str(&format!(
        "\n\n_[engine: trimmed — {cut} of {total} lines cut. Replies are capped at {max} \
         lines / {maxc} characters. Lead with the outcome; the full text is in the field.]_",
        cut = cut_lines,
        total = lines.len(),
        max = MAX_LINES,
        maxc = MAX_CHARS,
    ));

    for m in dropped_mentions {
        out.push('\n');
        out.push_str(m.trim_end());
    }

    Brief { body: out, full: Some(body.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_short_reply_is_returned_untouched() {
        let body = "Done. Tests pass.\n\n@orchestrator all green.";
        let brief = enforce(body);
        assert_eq!(brief.body, body, "the common case must be a no-op");
        assert_eq!(brief.full, None, "nothing was cut, so there is no original to keep");
    }

    #[test]
    fn an_over_long_reply_is_cut_to_the_line_budget() {
        let brief = enforce(&long(200));
        assert!(brief.full.is_some(), "the original is kept, not destroyed");
        let kept = brief.body.lines().filter(|l| l.starts_with("line ")).count();
        assert!(kept <= MAX_LINES, "cut to the budget, got {kept} content lines");
        assert!(brief.body.contains("engine: trimmed"), "the cut is declared, not silent");
        assert!(brief.body.starts_with("line 1"), "the head survives — the outcome leads");
    }

    /// The bug this whole module could most easily introduce: a worker's report hands
    /// back with a trailing `@orchestrator` line, and a naive "keep the first N lines"
    /// cut throws it away. The reply then routes to nobody, the orchestrator is never
    /// woken, and the work stops dead — with every test still green.
    #[test]
    fn a_trailing_orchestrator_line_survives_the_cut() {
        let body = format!("{}\n\n@orchestrator done, all green.", long(200));
        let brief = enforce(&body);
        assert!(
            brief.body.lines().any(|l| l.starts_with("@orchestrator")),
            "the routing line MUST survive or the reply excites nobody:\n{}",
            brief.body
        );
    }

    /// Routing is not just "a mention is present" — it is the FIRST line-leading mention
    /// that wins (`router::parse_addressee`). So a trim must not reorder them: a report
    /// that delegates to `@agy` before handing back to `@orchestrator` must still route
    /// to `@agy` afterwards.
    #[test]
    fn the_first_mention_is_still_the_first_mention_after_the_cut() {
        let body = format!("@agy please take the fork push.\n{}\n@orchestrator done.", long(200));
        let brief = enforce(&body);
        let first = brief.body.lines().find(|l| routes(l)).expect("a routing line survived");
        assert!(first.starts_with("@agy"), "the addressee must not change, got {first:?}");
        assert!(
            brief.body.lines().any(|l| l.starts_with("@orchestrator")),
            "and the hand-back is still there"
        );
    }

    #[test]
    fn one_enormous_paragraph_is_cut_too() {
        let body = "x".repeat(MAX_CHARS * 3);
        let brief = enforce(&body);
        assert!(brief.full.is_some(), "a single line can still be a bible");
        assert!(brief.body.contains("engine: trimmed"));
    }

    #[test]
    fn a_cut_inside_a_code_fence_closes_it() {
        let body = format!("Here is the output:\n```\n{}", long(200));
        let brief = enforce(&body);
        let fences = brief.body.lines().filter(|l| l.trim_start().starts_with("```")).count();
        assert_eq!(fences % 2, 0, "an open fence would render the rest as code:\n{}", brief.body);
    }

    /// A mention inside prose does not route (`parse_addressee` is line-leading only),
    /// so it must not be dragged out of the body and pinned to the end as if it did.
    #[test]
    fn a_mention_inside_prose_is_not_treated_as_routing() {
        assert!(!routes("as @agy said, the seat needs a key"));
        assert!(routes("@agy take the fork push"));
        assert!(routes("  @orchestrator done")); // indented, still line-leading
        assert!(!routes("email@host.com is not a mention"));
    }
}
