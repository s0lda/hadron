//! Hadron's own `agy` statusline shim — the only way to observe agy's quota.
//!
//! # Why this exists
//!
//! `agy` invokes its `statusLine` hook **even in `--print` mode**, feeding it a JSON
//! telemetry payload on **stdin** (verified live against agy 1.1.1). That payload is
//! the ONLY observable source of the human's remaining quota: agy never persists an
//! exhaustion error anywhere a subprocess can read it (every session log was grepped
//! for `quota`, `429`, `exceeded`, `resource_exhausted` — zero matches), so
//! failover-on-error-string is impossible. The statusline's `remaining_fraction` is
//! strictly better anyway: it arrives *before* anything fails, so policy can prevent
//! rather than react.
//!
//! # Correlating a payload with a turn
//!
//! The hook is a **child process of agy**, and agy is a child of the adapter — so it
//! inherits the adapter's environment. The adapter therefore stamps
//! [`OUT_ENV`] with a unique per-turn path, and the shim writes that turn's payload
//! exactly there. No polling, no guessing which of several concurrent quarks a
//! payload belongs to.
//!
//! Belt and braces: the shim ALSO writes a per-cwd "latest" capture
//! ([`latest_capture_path`]), so if a future agy version ever spawns its hook without
//! inheriting the environment, the adapter can still find the payload by working
//! directory. Both paths are hadron-owned; neither is the human's.
//!
//! # The human's statusline must keep working
//!
//! Hadron does not get to break the human's terminal. If [`CHAIN_ENV`] names their
//! original script, the shim feeds it the identical stdin and prints its output
//! verbatim — their statusline renders exactly as before, and hadron is invisible.
//!
//! # Privacy
//!
//! The payload carries the human's `email` and `plan_tier`. The **capture file** is
//! the raw payload and is hadron-private, never the field; only
//! [`hadron_lattice::parse_agy_statusline`] output — which has nowhere to put either —
//! ever reaches an event. [`status_line`] likewise never prints them.

use std::path::{Path, PathBuf};

use hadron_lattice::{parse_agy_statusline, AgyTelemetry};

/// Set by the adapter to the exact file this turn's payload must land in.
pub const OUT_ENV: &str = "HADRON_STATUSLINE_OUT";

/// Set (by the installer) to the statusline command hadron displaced, so the human's
/// own statusline keeps rendering. Absent → hadron prints its own compact line.
pub const CHAIN_ENV: &str = "HADRON_STATUSLINE_CHAIN";

/// Where per-cwd captures live: `~/.hadron/statusline/`. Hadron-owned; nothing here
/// is ever read by agy or the human's script.
pub fn capture_dir() -> Option<PathBuf> {
    hadron_lattice::user_hadron_dir().map(|d| d.join("statusline"))
}

/// A filesystem-safe key for a working directory. Two quarks in two worktrees must
/// not collide, and a path is not a filename — so the path is flattened, not hashed:
/// a human debugging a capture dir can still see which tree a file came from.
pub fn cwd_key(cwd: &Path) -> String {
    let s: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "root".into() } else { s }
}

/// The per-cwd "most recent payload seen in this directory" capture — the fallback
/// used when the hook did not inherit [`OUT_ENV`].
pub fn latest_capture_path(dir: &Path, cwd: &Path) -> PathBuf {
    dir.join(format!("{}.json", cwd_key(cwd)))
}

/// The compact status hadron prints when it is NOT chaining to the human's script.
///
/// Deliberately reports the **tightest applicable bucket**, not the average and not
/// the weekly alone: that is the one that will actually stop the next turn.
///
/// Never prints the email or the plan tier — see `status_line_never_leaks_identity`.
pub fn status_line(t: &AgyTelemetry) -> String {
    let model = t.model.as_deref().unwrap_or("?");
    let mut parts = vec![format!("hadron · {model}")];

    if let Some(ctx) = &t.usage.context {
        parts.push(format!(
            "ctx {:.1}% of {}k",
            ctx.used_percentage,
            ctx.context_window_size / 1024
        ));
    }
    if let Some(b) = t.model.as_deref().and_then(|m| t.usage.tightest_bucket(m)) {
        parts.push(format!("{} {:.0}% left", b.key, b.remaining_fraction * 100.0));
    }
    parts.join(" | ")
}

/// Persist a raw payload for a turn: to [`OUT_ENV`]'s path when the adapter set one,
/// and always to the per-cwd latest. Returns the paths written.
///
/// Errors are swallowed per-path on purpose: a statusline hook that fails hard would
/// break the human's `agy` session, and losing a telemetry sample is strictly less bad
/// than that. The adapter already treats a missing capture as "no telemetry".
fn persist(payload: &str, out_env: Option<PathBuf>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut written = Vec::new();
    let mut write = |p: PathBuf| {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&p, payload).is_ok() {
            written.push(p);
        }
    };

    if let Some(p) = out_env {
        write(p);
    }
    if let (Some(dir), Some(cwd)) = (capture_dir(), cwd) {
        write(latest_capture_path(&dir, cwd));
    }
    written
}

/// The `hadron-gluon --statusline` entry point: read agy's payload from stdin, save
/// it where the turn can find it, and print a status line (the human's own, if we
/// displaced one).
///
/// ALWAYS exits 0 and always prints *something*: agy renders this in the human's
/// terminal, and a shim that errors out would be hadron breaking a tool it does not
/// own.
pub fn run_shim() -> anyhow::Result<()> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;

    let telemetry = parse_agy_statusline(&payload).ok();
    // The payload names its own cwd; that is the key the adapter looks up.
    let cwd = serde_json::from_str::<serde_json::Value>(&payload)
        .ok()
        .and_then(|v| v.get("cwd").and_then(|c| c.as_str()).map(PathBuf::from));

    persist(&payload, std::env::var_os(OUT_ENV).map(PathBuf::from), cwd.as_deref());

    // The human's statusline, unchanged, if we displaced one.
    if let Some(chain) = std::env::var_os(CHAIN_ENV) {
        let chain = chain.to_string_lossy().to_string();
        if !chain.trim().is_empty() {
            if let Ok(out) = chain_to(&chain, &payload) {
                print!("{out}");
                return Ok(());
            }
        }
    }

    match telemetry {
        Some(t) => println!("{}", status_line(&t)),
        None => println!("hadron"),
    }
    Ok(())
}

/// Run the displaced statusline command with the identical stdin agy gave us.
fn chain_to(command: &str, payload: &str) -> anyhow::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Read back the telemetry an agy turn produced.
///
/// `out` is the per-turn path the adapter stamped into [`OUT_ENV`]; `cwd` is the
/// fallback key. A capture that predates the turn is IGNORED — a stale payload from a
/// previous turn (or from the human's own interactive agy session in the same tree)
/// would report the wrong numbers with total confidence, which is worse than
/// reporting none.
pub fn read_turn_capture(
    out: &Path,
    cwd: &Path,
    started: std::time::SystemTime,
) -> Option<AgyTelemetry> {
    let fresh = |p: &Path| -> Option<String> {
        let meta = std::fs::metadata(p).ok()?;
        let mtime = meta.modified().ok()?;
        // A hook that fires in the same millisecond the turn started is this turn's.
        if mtime + std::time::Duration::from_millis(1) < started {
            return None;
        }
        std::fs::read_to_string(p).ok()
    };

    let payload = fresh(out).or_else(|| {
        let dir = capture_dir()?;
        fresh(&latest_capture_path(&dir, cwd))
    })?;

    parse_agy_statusline(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{ContextUsage, QuotaBucket, Usage};

    const LIVE_PAYLOAD: &str = r#"{"cwd":"/home/Jake/dev/hadron","session_id":"b66f1126","conversation_id":"b66f1126-aa1f-4b87-832a-e3779e49bd71","model":{"id":"Gemini 3.1 Pro (High)"},"context_window":{"total_input_tokens":635,"total_output_tokens":292,"context_window_size":1048576,"used_percentage":0.0605},"quota":{"3p-5h":{"remaining_fraction":0.980418,"reset_time":"2026-07-12T20:42:57Z"},"3p-weekly":{"remaining_fraction":0.7435596,"reset_time":"2026-07-18T19:20:41Z"},"gemini-5h":{"remaining_fraction":0.7873184,"reset_time":"2026-07-12T16:23:06Z"},"gemini-weekly":{"remaining_fraction":0.0517783,"reset_time":"2026-07-13T13:12:03Z"}},"plan_tier":"Google AI Ultra","email":"jake.private@gmail.com"}"#;

    fn telemetry() -> AgyTelemetry {
        parse_agy_statusline(LIVE_PAYLOAD).unwrap()
    }

    /// Two quarks in two worktrees must not overwrite each other's captures.
    #[test]
    fn cwd_key_is_filesystem_safe_and_distinct_per_tree() {
        let a = cwd_key(Path::new("/home/Jake/dev/hadron/.hadron/trees/agy"));
        let b = cwd_key(Path::new("/home/Jake/dev/hadron/.hadron/trees/opus"));
        assert_ne!(a, b, "different worktrees, different capture files");
        assert!(!a.contains('/'), "a key is a filename, not a path: {a}");
        assert!(!a.contains('.'), "no dot-segments to escape the dir: {a}");
        assert!(a.contains("agy"), "still legible to a human debugging it: {a}");
        assert_eq!(cwd_key(Path::new("/")), "root", "the degenerate path still names a file");
    }

    /// The status line reports the bucket that will actually stop the next turn —
    /// `gemini-weekly` at 5%, NOT `gemini-5h` at 79% and not an average of them.
    #[test]
    fn status_line_reports_the_tightest_applicable_bucket() {
        let line = status_line(&telemetry());
        assert!(line.contains("Gemini 3.1 Pro (High)"), "{line}");
        assert!(line.contains("gemini-weekly"), "the binding constraint, not the roomy one: {line}");
        assert!(line.contains("5% left"), "{line}");
        assert!(!line.contains("gemini-5h"), "the roomy bucket is not the headline: {line}");
        assert!(line.contains("ctx"), "{line}");
    }

    /// A Claude model routed through agy bills the `3p-*` buckets, so its status line
    /// must name a 3p bucket — never the gemini one, however empty that is.
    #[test]
    fn status_line_follows_the_models_own_quota_family() {
        let mut t = telemetry();
        t.model = Some("Claude Opus 4.6 (Thinking)".into());
        let line = status_line(&t);
        assert!(line.contains("3p-weekly"), "3p model → 3p bucket: {line}");
        assert!(!line.contains("gemini"), "a 3p model is unaffected by an empty gemini bucket: {line}");
    }

    /// PRIVACY. This string is printed into the human's terminal AND is the last thing
    /// touched before telemetry reaches the field. It must never carry identity.
    #[test]
    fn status_line_never_leaks_identity() {
        let line = status_line(&telemetry());
        assert!(!line.contains("jake.private@gmail.com"), "{line}");
        assert!(!line.contains('@'), "{line}");
        assert!(!line.contains("Ultra"), "{line}");
    }

    /// A provider with nothing to say still yields a printable line, not a panic.
    #[test]
    fn status_line_degrades_without_telemetry() {
        let empty = AgyTelemetry {
            conversation_id: None,
            model: None,
            usage: Usage::default(),
        };
        assert_eq!(status_line(&empty), "hadron · ?");
    }

    /// **THE discriminating test for correlation.** The adapter stamps a per-turn path;
    /// the shim writes there; the adapter reads exactly that turn's numbers back. No
    /// polling, no ambiguity between concurrent quarks.
    #[test]
    fn a_turns_capture_round_trips_through_the_per_turn_path() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("turn-1.json");
        let started = std::time::SystemTime::now();

        // What the shim does when the adapter set OUT_ENV.
        persist(LIVE_PAYLOAD, Some(out.clone()), None);

        let t = read_turn_capture(&out, Path::new("/nonexistent"), started).unwrap();
        assert_eq!(t.usage.spend.fresh(), Some(927));
        assert_eq!(t.conversation_id.as_deref(), Some("b66f1126-aa1f-4b87-832a-e3779e49bd71"));
        assert_eq!(t.usage.quota.len(), 4);
        assert!((t.usage.effective_remaining("Gemini 3.1 Pro (High)").unwrap() - 0.0517783).abs() < 1e-9);
    }

    /// **A stale capture must not be believed.** The human's own interactive `agy` in
    /// the same tree — or this quark's previous turn — leaves a payload behind.
    /// Reporting last turn's numbers as this turn's would be confidently wrong, which
    /// is worse than reporting nothing.
    #[test]
    fn a_capture_older_than_the_turn_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("turn.json");
        std::fs::write(&out, LIVE_PAYLOAD).unwrap();

        // The turn starts one minute AFTER that capture was written.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(
            read_turn_capture(&out, Path::new("/nonexistent"), later).is_none(),
            "a payload predating the turn is not this turn's"
        );

        // A missing capture (the shim is not installed) is simply no telemetry.
        assert!(read_turn_capture(
            &dir.path().join("never-written.json"),
            Path::new("/nonexistent"),
            std::time::SystemTime::now(),
        )
        .is_none());
    }

    /// The raw capture file is hadron-private and DOES contain the email — that is
    /// exactly why only the parsed telemetry may ever cross into an event. This test
    /// pins the boundary: what we persist is raw, what we hand out is scrubbed.
    #[test]
    fn the_parsed_telemetry_crossing_the_boundary_carries_no_identity() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("turn.json");
        persist(LIVE_PAYLOAD, Some(out.clone()), None);

        let t = read_turn_capture(&out, Path::new("/x"), std::time::SystemTime::now()).unwrap();
        let dump = format!("{t:?}");
        assert!(!dump.contains('@'), "no email escapes the shim boundary: {dump}");
        assert!(!dump.contains("Ultra"), "no plan tier escapes the shim boundary");
    }

    #[test]
    fn usage_survives_the_shim_with_every_bucket() {
        let t = telemetry();
        let want = Usage {
            spend: t.usage.spend.clone(),
            context: Some(ContextUsage {
                used_tokens: 635,
                context_window_size: 1_048_576,
                used_percentage: 0.0605,
            }),
            quota: vec![
                QuotaBucket { key: "3p-5h".into(), remaining_fraction: 0.980418, reset_time: t.usage.quota[0].reset_time },
                QuotaBucket { key: "3p-weekly".into(), remaining_fraction: 0.7435596, reset_time: t.usage.quota[1].reset_time },
                QuotaBucket { key: "gemini-5h".into(), remaining_fraction: 0.7873184, reset_time: t.usage.quota[2].reset_time },
                QuotaBucket { key: "gemini-weekly".into(), remaining_fraction: 0.0517783, reset_time: t.usage.quota[3].reset_time },
            ],
        };
        assert_eq!(t.usage, want);
    }
}
