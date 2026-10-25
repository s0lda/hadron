use hadron_lattice::TokenSpend;
use serde::{Deserialize, Serialize};

use agent_client_protocol::schema::v1::Usage as AcpUsage;

/// Normalized end-turn spend report supporting both session-cumulative ACP counters
/// and per-turn (`cumulative: Some(false)`) SDK bridge counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpendReport {
    #[serde(default)]
    pub input: Option<u32>,
    #[serde(default)]
    pub output: Option<u32>,
    #[serde(default)]
    pub cache_read: Option<u32>,
    #[serde(default)]
    pub cache_write: Option<u32>,
    #[serde(default)]
    pub cumulative: Option<bool>,
}

/// The cumulative counters an ACP session has reported so far — the watermark the
/// per-turn deltas are measured against. Cumulative, so `u64`: a long session can
/// out-grow a `u32` on cache reads alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpendWatermark {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// **The per-turn spend, by component, from cumulative counters.**
///
/// ACP's end-turn `Usage` is documented as session-cumulative — `input_tokens` is
/// "total input tokens across all turns". Hadron's spend is **per-turn** (it feeds a
/// ledger that sums it), so reporting the cumulative figure would make every turn
/// re-bill the whole session and the ledger would grow quadratically.
///
/// So: keep the last cumulative reading per component, and report the difference.
/// When an adapter explicitly marks `_meta.cumulative = false` (such as `agy_acp.py`,
/// whose `response.usage_metadata` is already scoped to the single turn), the values
/// are recorded directly as per-turn spend without subtracting the previous watermark.
pub fn turn_spend(last: SpendWatermark, usage: Option<&AcpUsage>) -> (TokenSpend, SpendWatermark) {
    let Some(u) = usage else {
        // The agent does not implement end-turn usage. Absent is absent: report
        // nothing (not zero) and do not move the watermark.
        return (TokenSpend::default(), last);
    };
    let is_per_turn = u
        .meta
        .as_ref()
        .and_then(|m| m.get("cumulative"))
        .and_then(|v| v.as_bool())
        == Some(false);
    let report = SpendReport {
        input: Some(u.input_tokens.min(u32::MAX as u64) as u32),
        output: Some(u.output_tokens.min(u32::MAX as u64) as u32),
        cache_read: u.cached_read_tokens.map(|n| n.min(u32::MAX as u64) as u32),
        cache_write: u.cached_write_tokens.map(|n| n.min(u32::MAX as u64) as u32),
        cumulative: Some(!is_per_turn),
    };
    if is_per_turn {
        return turn_spend_from_report(last, &report, None);
    }
    let delta = |now: u64, prev: u64| -> u32 {
        let d = if now >= prev { now - prev } else { now };
        d.min(u32::MAX as u64) as u32
    };
    let spend = TokenSpend {
        input: Some(delta(u.input_tokens, last.input)),
        output: Some(delta(u.output_tokens, last.output)),
        // Absent stays absent: an agent that reports no cache columns gets `None`,
        // never `Some(0)`.
        cache_read: u.cached_read_tokens.map(|n| delta(n, last.cache_read)),
        cache_write: u.cached_write_tokens.map(|n| delta(n, last.cache_write)),
    };
    let next = SpendWatermark {
        input: u.input_tokens,
        output: u.output_tokens,
        // A component the agent did not report must not reset its watermark, or the
        // next real reading comes out as a bogus delta against zero.
        cache_read: u.cached_read_tokens.unwrap_or(last.cache_read),
        cache_write: u.cached_write_tokens.unwrap_or(last.cache_write),
    };
    (spend, next)
}

/// Compute per-turn [`TokenSpend`] from a [`SpendReport`], falling back to `context`
/// (`usage_update` `(used, size)`) when explicit token spend is absent.
pub fn turn_spend_from_report(
    last: SpendWatermark,
    report: &SpendReport,
    context: Option<(u64, u64)>,
) -> (TokenSpend, SpendWatermark) {
    if report.input.is_none() && report.output.is_none() && report.cache_read.is_none() && report.cache_write.is_none() {
        if let Some((used, _size)) = context.filter(|(u, _)| *u > 0) {
            let d = if used >= last.input { used - last.input } else { used };
            let spend = TokenSpend {
                input: Some(d.min(u32::MAX as u64) as u32),
                ..Default::default()
            };
            let next = SpendWatermark { input: used, ..last };
            return (spend, next);
        }
        return (TokenSpend::default(), last);
    }
    if report.cumulative == Some(false) {
        let spend = TokenSpend {
            input: report.input,
            output: report.output,
            cache_read: report.cache_read,
            cache_write: report.cache_write,
        };
        let next = SpendWatermark {
            input: last.input.saturating_add(report.input.unwrap_or(0) as u64),
            output: last.output.saturating_add(report.output.unwrap_or(0) as u64),
            cache_read: last.cache_read.saturating_add(report.cache_read.unwrap_or(0) as u64),
            cache_write: last.cache_write.saturating_add(report.cache_write.unwrap_or(0) as u64),
        };
        return (spend, next);
    }
    let delta = |now: u64, prev: u64| -> u32 {
        let d = if now >= prev { now - prev } else { now };
        d.min(u32::MAX as u64) as u32
    };
    let in_u64 = report.input.unwrap_or(0) as u64;
    let out_u64 = report.output.unwrap_or(0) as u64;
    let spend = TokenSpend {
        input: report.input.map(|_| delta(in_u64, last.input)),
        output: report.output.map(|_| delta(out_u64, last.output)),
        cache_read: report.cache_read.map(|n| delta(n as u64, last.cache_read)),
        cache_write: report.cache_write.map(|n| delta(n as u64, last.cache_write)),
    };
    let next = SpendWatermark {
        input: if report.input.is_some() { in_u64 } else { last.input },
        output: if report.output.is_some() { out_u64 } else { last.output },
        cache_read: report.cache_read.map(|n| n as u64).unwrap_or(last.cache_read),
        cache_write: report.cache_write.map(|n| n as u64).unwrap_or(last.cache_write),
    };
    (spend, next)
}

/// Compute per-turn [`TokenSpend`] from ACP end-turn [`AcpUsage`], falling back to
/// `context` (`usage_update` `(used, size)`) when `usage` is `None`.
pub fn turn_spend_with_context(
    last: SpendWatermark,
    usage: Option<&AcpUsage>,
    context: Option<(u64, u64)>,
) -> (TokenSpend, SpendWatermark) {
    if usage.is_some() {
        return turn_spend(last, usage);
    }
    turn_spend_from_report(last, &SpendReport::default(), context)
}
