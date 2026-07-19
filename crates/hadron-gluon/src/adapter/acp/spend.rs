use hadron_lattice::TokenSpend;

use agent_client_protocol::schema::v1::Usage as AcpUsage;

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
///
/// **This no longer touches `total_tokens`**, and that is the point. `total_tokens`
/// is "sum of all token types" — cache reads included — so using it made an ACP quark
/// report ~200x what a CLI quark reported for the same work. The components are
/// carried separately and [`hadron_lattice::TokenSpend::fresh`] is the only thing that
/// adds any of them up.
///
/// The guard, kept per-component: if a counter goes *backwards*, the agent either
/// restarted its count or reports per-turn despite the schema saying cumulative.
/// Saturating to 0 would silently drop that turn's cost, so a backwards counter is
/// read as an absolute for that turn instead.
pub fn turn_spend(last: SpendWatermark, usage: Option<&AcpUsage>) -> (TokenSpend, SpendWatermark) {
    let Some(u) = usage else {
        // The agent does not implement end-turn usage. Absent is absent: report
        // nothing (not zero) and do not move the watermark.
        return (TokenSpend::default(), last);
    };
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
