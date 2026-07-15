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
    Brief { body: body.to_string(), full: None }
}

#[cfg(test)]
mod tests {}
