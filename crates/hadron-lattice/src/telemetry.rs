//! What a turn *cost* and what budget is *left*: context usage and quota.
//!
//! Two different questions, deliberately kept apart:
//!
//! - **Context** is per-conversation: how much of the model's context window this
//!   turn is occupying. Every provider can answer it.
//! - **Quota** is per-account: how much of the human's plan allowance is left.
//!   Only some providers answer it — `agy` does, `claude` does not.
//!
//! ## Quota is absent, not full
//!
//! `claude -p --output-format json` reports real tokens but has **no quota concept
//! at all**. Modelling that as "100% remaining" would be a lie the UI cannot
//! distinguish from a genuinely untouched budget. So quota is a `Vec` that is
//! simply **empty** for providers that do not report it, and every accessor returns
//! `Option`. Absent means absent.
//!
//! ## The buckets are independent, and you must take the minimum
//!
//! `agy` reports **four** buckets. Two *families* — `gemini-*` (Gemini models) and
//! `3p-*` (Claude / GPT-OSS models routed through agy) — each with a rolling **5h**
//! bucket AND a **weekly** bucket. They deplete independently: a human can be at 98%
//! on `3p-5h` and 5% on `3p-weekly`, and the weekly one is what will actually stop
//! them. Effective availability for a model is therefore the **minimum across every
//! bucket that applies to it**, never the weekly one alone and never a single
//! collapsed number. See [`Usage::effective_remaining`].
//!
//! ## Privacy
//!
//! The `agy` statusline payload also carries the human's `email` and `plan_tier`.
//! Neither is parsed here and neither has anywhere to live in these types, so
//! neither can reach the field, the log, or the projection. `email_is_never_parsed`
//! and `email_never_reaches_the_serialized_event` (in `event.rs`) are the
//! regression tests that keep it that way.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How much of the model's context window a turn is occupying.
///
/// `PartialEq` but not `Eq`: `used_percentage` is a float.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Tokens occupying the window. For `agy` this is `context_window.total_input_tokens`
    /// — the figure agy's own `used_percentage` is computed from (verified against a live
    /// payload: 635 / 1048576 = 0.0605%, exactly the `used_percentage` it reported).
    pub used_tokens: u32,
    /// The model's total window, e.g. 1_048_576 for Gemini 3.1 Pro.
    pub context_window_size: u32,
    /// Percent of the window used, as the provider itself computes it (0.0–100.0).
    /// Carried rather than recomputed so the UI shows the provider's own number.
    pub used_percentage: f64,
}

/// One rolling allowance window on the human's plan, e.g. `gemini-weekly`.
///
/// `PartialEq` but not `Eq`: `remaining_fraction` is a float.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaBucket {
    /// The provider's own bucket key, verbatim: `gemini-5h`, `gemini-weekly`,
    /// `3p-5h`, `3p-weekly`. Kept as a string rather than an enum so an unknown
    /// future bucket is still reported instead of silently dropped.
    pub key: String,
    /// Fraction of the allowance still available, 0.0–1.0. (0.0517783 = 5.2% left.)
    pub remaining_fraction: f64,
    /// When this bucket refills. `None` if the provider did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_time: Option<DateTime<Utc>>,
}

impl QuotaBucket {
    /// The family a bucket belongs to: the key up to its last `-`, so `gemini-weekly`
    /// → `gemini` and `3p-5h` → `3p`. This is what decides which buckets apply to a
    /// given model.
    pub fn family(&self) -> &str {
        self.key.rsplit_once('-').map(|(fam, _)| fam).unwrap_or(&self.key)
    }
}

/// Which quota family a model draws from.
///
/// `agy` routes non-Gemini models (Claude, GPT-OSS) through a third-party allowance
/// billed to the `3p-*` buckets, while Gemini models bill to `gemini-*`. The two
/// deplete independently — this is precisely why a single collapsed "quota" number
/// would be wrong.
pub fn quota_family_for_model(model: &str) -> &'static str {
    if model.trim_start().to_ascii_lowercase().starts_with("gemini") {
        "gemini"
    } else {
        "3p"
    }
}

/// Everything a turn can report about cost and remaining budget. Both halves are
/// independently optional: `claude` fills `context` and leaves `quota` empty; a
/// provider with no telemetry at all reports `Usage::default()`.
///
/// `PartialEq` but not `Eq` (contains floats).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    /// Every bucket the provider reported. **Empty means the provider has no quota
    /// concept** (claude) — NOT that the quota is full.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quota: Vec<QuotaBucket>,
}

impl Usage {
    /// Whether this carries anything at all worth reporting.
    pub fn is_empty(&self) -> bool {
        self.context.is_none() && self.quota.is_empty()
    }

    /// Every bucket that applies to `model` — i.e. every bucket in the model's family.
    pub fn buckets_for_model(&self, model: &str) -> Vec<&QuotaBucket> {
        let family = quota_family_for_model(model);
        self.quota.iter().filter(|b| b.family() == family).collect()
    }

    /// **The number that actually matters**: the fraction of allowance left for
    /// `model`, as the **minimum across every bucket that applies to it**.
    ///
    /// A human at 98% on `3p-5h` but 5% on `3p-weekly` has 5% left, not 98% and not
    /// some average — the weekly bucket is what will stop the next turn. Taking the
    /// min is the whole point.
    ///
    /// `None` when the provider reports no applicable bucket (claude always; agy if
    /// it ever drops a family). `None` means *unknown*, not *full*.
    pub fn effective_remaining(&self, model: &str) -> Option<f64> {
        self.buckets_for_model(model)
            .into_iter()
            .map(|b| b.remaining_fraction)
            // total_cmp: NaN-safe, and f64 is not Ord.
            .min_by(|a, b| a.total_cmp(b))
    }

    /// The bucket that is closest to running out for `model` — the one a UI should
    /// name when it warns ("gemini-weekly: 5% left"). Same min as
    /// [`Usage::effective_remaining`], but returns *which* bucket it was.
    pub fn tightest_bucket(&self, model: &str) -> Option<&QuotaBucket> {
        self.buckets_for_model(model)
            .into_iter()
            .min_by(|a, b| a.remaining_fraction.total_cmp(&b.remaining_fraction))
    }
}

/// What one `agy` turn cost, parsed out of the CLI's statusline telemetry payload.
///
/// `agy` invokes its `statusLine` hook **even in `--print` mode**, feeding it this
/// JSON on stdin (verified live against agy 1.1.1). It is the ONLY observable source
/// of quota: agy never persists an exhaustion error anywhere a subprocess can see it
/// (every session log was grepped for `quota`/`429`/`exceeded`/`resource_exhausted`
/// — zero matches), so error-string failover is impossible. `remaining_fraction` is
/// strictly better anyway: it is available *before* anything fails, which makes
/// prevention possible instead of reaction.
#[derive(Debug, Clone, PartialEq)]
pub struct AgyTelemetry {
    /// Tokens billed for this turn: `total_input_tokens + total_output_tokens`.
    /// Deliberately parallel to the claude adapter, which sums `input_tokens +
    /// output_tokens` and likewise excludes cache reads.
    pub used_tokens: u32,
    /// agy's conversation id — the handle `agy --conversation <id>` resumes.
    pub conversation_id: Option<String>,
    /// The model agy actually ran (which is NOT always the one we asked for).
    pub model: Option<String>,
    pub usage: Usage,
}

/// Parse the `agy` statusline payload.
///
/// Deliberately field-by-field rather than `#[derive(Deserialize)]` on a mirror
/// struct: the payload also carries `email` and `plan_tier`, and the surest way to
/// keep the human's email out of the field is for the parser to have no place to put
/// it. Everything not named here is dropped on the floor.
pub fn parse_agy_statusline(json: &str) -> serde_json::Result<AgyTelemetry> {
    let v: serde_json::Value = serde_json::from_str(json)?;

    let cw = v.get("context_window");
    let num = |parent: Option<&serde_json::Value>, key: &str| -> u32 {
        parent
            .and_then(|p| p.get(key))
            .and_then(|n| n.as_u64())
            .unwrap_or(0) as u32
    };

    let used_tokens = num(cw, "total_input_tokens") + num(cw, "total_output_tokens");

    // agy computes `used_percentage` from `total_input_tokens` against the window
    // (635 / 1048576 = 0.0605%, matching the live payload exactly), so that is the
    // figure that belongs in `ContextUsage::used_tokens`.
    let context = cw.map(|_| ContextUsage {
        used_tokens: num(cw, "total_input_tokens"),
        context_window_size: num(cw, "context_window_size"),
        used_percentage: cw
            .and_then(|c| c.get("used_percentage"))
            .and_then(|n| n.as_f64())
            .unwrap_or(0.0),
    });

    let mut quota: Vec<QuotaBucket> = v
        .get("quota")
        .and_then(|q| q.as_object())
        .map(|buckets| {
            buckets
                .iter()
                .filter_map(|(key, b)| {
                    Some(QuotaBucket {
                        key: key.clone(),
                        remaining_fraction: b.get("remaining_fraction")?.as_f64()?,
                        reset_time: b
                            .get("reset_time")
                            .and_then(|t| t.as_str())
                            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                            .map(|t| t.with_timezone(&Utc)),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Stable order: a serde_json map is already sorted, but be explicit — the field is
    // a durable log and a flapping key order would churn every diff of it.
    quota.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(AgyTelemetry {
        used_tokens,
        conversation_id: v
            .get("conversation_id")
            .and_then(|s| s.as_str())
            .map(str::to_string),
        model: v
            .get("model")
            .and_then(|m| m.get("id"))
            .and_then(|s| s.as_str())
            .map(str::to_string),
        usage: Usage { context, quota },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The REAL payload captured from a live `agy --print` run (agy 1.1.1). The
    /// human's email is replaced, but every other value is verbatim — this is the
    /// ground-truth schema, so the parser is tested against what agy actually emits
    /// rather than what we imagine it emits.
    const LIVE_PAYLOAD: &str = r#"{"cwd":"/home/Jake/dev/hadron","session_id":"b66f1126-aa1f-4b87-832a-e3779e49bd71","conversation_id":"b66f1126-aa1f-4b87-832a-e3779e49bd71","transcript_path":"/home/Jake/.gemini/antigravity/brain/b66f1126/.system_generated/logs/transcript.jsonl","model":{"id":"Gemini 3.1 Pro (High)","display_name":"Gemini 3.1 Pro (High)"},"workspace":{"current_dir":"/home/Jake/dev/hadron","project_dir":"/home/Jake/dev/hadron"},"version":"1.1.1","context_window":{"total_input_tokens":635,"total_output_tokens":292,"context_window_size":1048576,"used_percentage":0.060558319091796875,"remaining_percentage":99.9394416809082,"current_usage":{"input_tokens":11368,"output_tokens":288,"cache_creation_input_tokens":0,"cache_read_input_tokens":12187}},"exceeds_200k_tokens":false,"product":"antigravity","quota":{"3p-5h":{"remaining_fraction":0.980418,"reset_time":"2026-07-12T20:42:57Z","reset_in_seconds":15950},"3p-weekly":{"remaining_fraction":0.7435596,"reset_time":"2026-07-18T19:20:41Z","reset_in_seconds":529414},"gemini-5h":{"remaining_fraction":0.7873184,"reset_time":"2026-07-12T16:23:06Z","reset_in_seconds":359},"gemini-weekly":{"remaining_fraction":0.0517783,"reset_time":"2026-07-13T13:12:03Z","reset_in_seconds":75296}},"agent_state":"idle","vcs":{"type":"git"},"sandbox":{"enabled":false},"plan_tier":"Google AI Ultra","email":"someone@example.com","terminal_width":80}"#;

    #[test]
    fn parses_the_live_payload() {
        let t = parse_agy_statusline(LIVE_PAYLOAD).unwrap();

        // Turn cost: total_input + total_output (cache excluded, as in claude).
        assert_eq!(t.used_tokens, 635 + 292);
        assert_eq!(t.conversation_id.as_deref(), Some("b66f1126-aa1f-4b87-832a-e3779e49bd71"));
        assert_eq!(t.model.as_deref(), Some("Gemini 3.1 Pro (High)"));

        let ctx = t.usage.context.as_ref().unwrap();
        assert_eq!(ctx.used_tokens, 635);
        assert_eq!(ctx.context_window_size, 1_048_576);
        assert!((ctx.used_percentage - 0.060558319091796875).abs() < 1e-9);

        // All FOUR buckets, none collapsed away.
        assert_eq!(t.usage.quota.len(), 4);
        let keys: Vec<&str> = t.usage.quota.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, vec!["3p-5h", "3p-weekly", "gemini-5h", "gemini-weekly"]);
        let weekly = t.usage.quota.iter().find(|b| b.key == "gemini-weekly").unwrap();
        assert!((weekly.remaining_fraction - 0.0517783).abs() < 1e-9);
        assert_eq!(
            weekly.reset_time.unwrap().to_rfc3339(),
            "2026-07-13T13:12:03+00:00"
        );
    }

    /// PRIVACY. The payload carries the human's email and plan tier. The parser must
    /// have nowhere to put them — a `Debug` dump of everything we retained is the
    /// broadest net available for "did any of it survive".
    #[test]
    fn email_and_plan_tier_are_never_parsed() {
        let payload = LIVE_PAYLOAD.replace("someone@example.com", "secret.human@gmail.com");
        let t = parse_agy_statusline(&payload).unwrap();

        let dump = format!("{t:?}");
        assert!(!dump.contains("secret.human@gmail.com"), "email must not survive parsing");
        assert!(!dump.contains('@'), "no email-shaped anything survives: {dump}");
        assert!(!dump.contains("Google AI Ultra"), "plan tier must not survive parsing");

        // and the same through serialization, which is what actually hits the field
        let json = serde_json::to_string(&t.usage).unwrap();
        assert!(!json.contains("secret.human"));
        assert!(!json.contains("Ultra"));
    }

    /// **THE discriminating test.** Effective availability is the MIN across the
    /// buckets that apply — never the weekly alone, never the 5h alone, never an
    /// average. In the live payload a Gemini model is at 78.7% on `gemini-5h` but
    /// 5.2% on `gemini-weekly`: it has 5.2% left, and any other answer would send a
    /// turn into a wall.
    #[test]
    fn effective_remaining_is_the_min_across_the_buckets_that_apply() {
        let t = parse_agy_statusline(LIVE_PAYLOAD).unwrap();
        let u = &t.usage;

        // Gemini family: min(gemini-5h 0.787, gemini-weekly 0.0518) = 0.0518.
        let gem = u.effective_remaining("Gemini 3.1 Pro (High)").unwrap();
        assert!((gem - 0.0517783).abs() < 1e-9, "gemini takes the weekly bucket, got {gem}");

        // 3p family: min(3p-5h 0.980, 3p-weekly 0.744) = 0.744. A DIFFERENT number —
        // the families are independent, which is why one collapsed figure cannot work.
        let claude_via_agy = u.effective_remaining("Claude Opus 4.6 (Thinking)").unwrap();
        assert!((claude_via_agy - 0.7435596).abs() < 1e-9, "3p takes its own weekly, got {claude_via_agy}");
        assert!(claude_via_agy > gem, "the two families deplete independently");

        // and it names WHICH bucket is the binding constraint
        assert_eq!(u.tightest_bucket("Gemini 3.1 Pro (High)").unwrap().key, "gemini-weekly");
        assert_eq!(u.tightest_bucket("GPT-OSS 120B (Medium)").unwrap().key, "3p-weekly");
    }

    #[test]
    fn models_map_to_their_quota_family() {
        assert_eq!(quota_family_for_model("Gemini 3.1 Pro (High)"), "gemini");
        assert_eq!(quota_family_for_model("Gemini 3.5 Flash (Low)"), "gemini");
        // Everything agy routes as a third party bills the 3p buckets.
        assert_eq!(quota_family_for_model("Claude Opus 4.6 (Thinking)"), "3p");
        assert_eq!(quota_family_for_model("Claude Sonnet 4.6 (Thinking)"), "3p");
        assert_eq!(quota_family_for_model("GPT-OSS 120B (Medium)"), "3p");
        // An empty/unknown model is not silently treated as Gemini.
        assert_eq!(quota_family_for_model(""), "3p");
    }

    #[test]
    fn a_bucket_knows_its_family() {
        let b = |k: &str| QuotaBucket { key: k.into(), remaining_fraction: 1.0, reset_time: None };
        assert_eq!(b("gemini-weekly").family(), "gemini");
        assert_eq!(b("3p-5h").family(), "3p");
        // A key with no dash is its own family rather than a panic.
        assert_eq!(b("weird").family(), "weird");
    }

    /// **Keep claude honest.** claude reports real tokens and has NO quota concept.
    /// Absent must stay absent: `effective_remaining` returns `None`, NOT 1.0. A fake
    /// "100% left" is indistinguishable from a real full budget and would make the UI
    /// lie.
    #[test]
    fn a_provider_without_quota_reports_absent_not_full() {
        let claude = Usage {
            context: Some(ContextUsage {
                used_tokens: 12_000,
                context_window_size: 200_000,
                used_percentage: 6.0,
            }),
            quota: vec![],
        };

        assert_eq!(claude.effective_remaining("opus"), None, "absent, not 1.0");
        assert_eq!(claude.tightest_bucket("opus"), None);
        assert!(claude.context.is_some(), "but its real token numbers are there");
        assert!(!claude.is_empty());

        // and an empty quota does not serialize a misleading key at all
        let json = serde_json::to_string(&claude).unwrap();
        assert!(!json.contains("quota"), "no phantom quota in the field: {json}");
    }

    #[test]
    fn usage_round_trips() {
        let t = parse_agy_statusline(LIVE_PAYLOAD).unwrap();
        let json = serde_json::to_string(&t.usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t.usage);
    }

    /// A payload from a provider/version that omits sections must degrade, not panic.
    #[test]
    fn missing_sections_degrade_to_empty() {
        let t = parse_agy_statusline(r#"{"session_id":"x"}"#).unwrap();
        assert_eq!(t.used_tokens, 0);
        assert!(t.usage.is_empty());
        assert_eq!(t.usage.effective_remaining("Gemini 3.1 Pro (High)"), None);
        assert!(parse_agy_statusline("not json").is_err());
    }
}
