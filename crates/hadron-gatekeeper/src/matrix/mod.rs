//! The mode ladder: fold the field's `ModeSet` events into an effective mode
//! per quark, and decide whether a proposed op is pre-authorized or must ask a
//! human. Pure and offline — no events emitted, no daemon paused, no UI.

use std::collections::HashSet;

use hadron_lattice::{Actor, Event, Kind, Mode, QuarkId, Risk};

/// The set of `(quark, op)` pairs the human has chosen to *always* allow.
/// `op` is the self-declared `PermissionReq` description — the daemon never sees
/// the raw command on the CLI-adapter path, so matching is on the declared
/// string (exact match, or a trailing-`*` prefix glob — see [`op_matches`]).
pub type AllowRules = HashSet<(QuarkId, String)>;

/// The set of `(quark, op)` pairs the human has explicitly *denied* — only
/// consulted under No-Human-Mode (`decide(..., no_human = true, ...)`); a deny
/// match always wins over an allow match. Same matching rules as `AllowRules`.
pub type DenyRules = HashSet<(QuarkId, String)>;

/// The matrix's verdict for a single proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The mode pre-authorizes this op; proceed without a human prompt (the
    /// gluon grants — in `Bypass` this is the orchestrator's standing authority).
    AutoApprove,
    /// Pause and surface a permission request to the human.
    AskHuman,
    /// No-Human-Mode only (global `Bypass`): pause and surface the request to
    /// the orchestrator quark for adjudication instead of a human. Never
    /// returned unless `decide` is called with `no_human = true`. **Never**
    /// returned for a deny-listed op, even then — a human deny is an absolute
    /// floor the orchestrator cannot override (see `decide`'s deny branch).
    AskOrchestrator,
}

/// Exact match, or a trailing-`*` prefix glob: `"git push*"` matches any op
/// starting with `"git push"` (e.g. `"git push origin main"`). No other glob
/// syntax is supported — this is a hand-rolled, deliberately tiny matcher, not
/// a general glob engine.
pub fn op_matches(pattern: &str, op: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => op.starts_with(prefix),
        None => pattern == op,
    }
}

/// Whether any rule in `rules` names `quark` with a pattern matching `op`
/// (see [`op_matches`]). Shared by the allow- and deny-list checks under
/// No-Human-Mode.
fn rules_match(rules: &HashSet<(QuarkId, String)>, quark: &QuarkId, op: &str) -> bool {
    rules.iter().any(|(q, pattern)| q == quark && op_matches(pattern, op))
}

/// The effective global default mode: the latest `ModeSet` addressed to no one
/// (`to == None`), or `Mode::Ask` if none has been set.
pub fn global_mode(events: &[Event]) -> Mode {
    events
        .iter()
        .rev()
        .find_map(|e| match (&e.to, &e.kind) {
            (None, Kind::ModeSet { mode }) => Some(*mode),
            _ => None,
        })
        .unwrap_or_default()
}

/// The effective mode for `quark`: its latest per-quark override (a `ModeSet`
/// addressed to it) if any, otherwise the global default, otherwise `Mode::Ask`.
pub fn resolve_mode(events: &[Event], quark: &QuarkId) -> Mode {
    // The latest per-quark mode event wins. A `ModeSet` pins an override; a later
    // `ModeClear` (the "Default" rung) reverts the quark to the global default. The
    // global default itself only changes via a global `ModeSet` — so a per-quark
    // override survives any number of later global changes, which is the point.
    for e in events.iter().rev() {
        match (&e.to, &e.kind) {
            (Some(t), Kind::ModeSet { mode }) if t == quark => return *mode,
            (Some(t), Kind::ModeClear) if t == quark => break,
            _ => {}
        }
    }
    global_mode(events)
}

/// Whether `quark` carries an effective per-quark override (vs inheriting global):
/// the latest per-quark mode event is a `ModeSet`. Order-aware, so a `ModeClear`
/// after a `ModeSet` reads as "no override" — the quark is back on the global default.
pub fn has_override(events: &[Event], quark: &QuarkId) -> bool {
    for e in events.iter().rev() {
        match (&e.to, &e.kind) {
            (Some(t), Kind::ModeSet { .. }) if t == quark => return true,
            (Some(t), Kind::ModeClear) if t == quark => return false,
            _ => {}
        }
    }
    false
}

/// The worker-clamped mode (spec §2 C, user-confirmed clamp policy): under
/// No-Human-Mode, once the global default is `Bypass`, a non-orchestrator
/// worker with no explicit per-quark override is clamped down to `Auto`
/// rather than inheriting the orchestrator's standing `Bypass` authority. An
/// explicit per-quark override (via `has_override`) survives the clamp — a
/// human who deliberately pinned a worker to `Bypass` keeps that pin. The
/// orchestrator seat (`is_orchestrator = true`) is never clamped.
///
/// Outside that one case — `no_human == false`, or the global default is not
/// `Bypass` — this is exactly `resolve_mode`. Reuses `resolve_mode`,
/// `global_mode`, and `has_override`; does not duplicate their event-scanning
/// logic.
pub fn effective_mode(events: &[Event], quark: &QuarkId, no_human: bool, is_orchestrator: bool) -> Mode {
    if no_human
        && !is_orchestrator
        && global_mode(events) == Mode::Bypass
        && !has_override(events, quark)
    {
        return Mode::Auto;
    }
    resolve_mode(events, quark)
}

/// Fold remembered approvals into the allow-list. A `PermissionGrant` with
/// `remember == true && approved == true`, addressed to a quark, teaches a rule;
/// the op is recovered by pairing the grant with the most recent preceding
/// `PermissionReq` from that same quark (its `description`).
pub fn allow_rules(events: &[Event]) -> AllowRules {
    let mut rules = AllowRules::new();
    for (i, e) in events.iter().enumerate() {
        let Kind::PermissionGrant { approved: true, remember: true } = &e.kind else {
            continue;
        };
        let Some(quark) = &e.to else { continue };
        // Find the op: the nearest earlier PermissionReq authored by this quark.
        let op = events[..i].iter().rev().find_map(|p| match (&p.from, &p.kind) {
            (Actor::Quark(qk), Kind::PermissionReq { description, .. }) if qk == quark => {
                Some(description.clone())
            }
            _ => None,
        });
        if let Some(op) = op {
            rules.insert((quark.clone(), op));
        }
    }
    rules
}

/// Decide a single self-declared op under the effective `mode`.
///
/// Two tables, selected by `no_human` (additive — see below):
///
/// **`no_human == false` (today's table, byte-for-byte — `global`/`deny` ignored,
/// `AskOrchestrator` never returned):**
///
/// | mode \ risk | WorkspaceEdit | BashExec                         |
/// |-------------|---------------|----------------------------------|
/// | Ask         | AskHuman      | AskHuman                         |
/// | Write       | AutoApprove   | AskHuman                         |
/// | Auto        | AutoApprove   | allow-listed ? AutoApprove : Ask |
/// | Bypass      | AutoApprove   | AutoApprove                      |
///
/// **`no_human == true` (No-Human-Mode, worker `mode` already clamped by the
/// caller — see spec §2). The double-table only refines the `BashExec`
/// escalation; `WorkspaceEdit` rides the base risk/mode ladder — worktree
/// isolation is why edits are low-risk, and Write/Auto/Bypass all auto-approve
/// them today, allow-list or not:**
/// 1. `mode == Bypass` → `AutoApprove` (an explicit per-quark standing grant).
/// 2. `risk == WorkspaceEdit`: `mode == Ask` → escalate; else (Write/Auto/Bypass,
///    Bypass already handled above) → `AutoApprove`.
/// 3. `risk == BashExec`: a `deny` match (exact or trailing-`*` glob) → **`AskHuman`,
///    unconditionally** — deny wins even over an allow match, and deny is an
///    absolute floor the orchestrator can never override (see the SECURITY note
///    on the deny branch below); `mode == Ask` or `mode == Write` → escalate;
///    `mode == Auto` → an `allow` match ? `AutoApprove` : escalate.
///
/// Where `escalate = AskOrchestrator` if `global == Bypass`, else `AskHuman`
/// (No-Human-Mode itself is only meaningful once the global default is
/// `Bypass`; short of that it degrades to the ordinary human-ask path).
/// `escalate` is used for every NON-deny path; the deny match always resolves
/// to `AskHuman` directly, bypassing `escalate` entirely.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    mode: Mode,
    global: Mode,
    no_human: bool,
    risk: Risk,
    op: &str,
    quark: &QuarkId,
    allow: &AllowRules,
    deny: &DenyRules,
) -> Decision {
    if !no_human {
        // Byte-for-byte today's logic. `global` and `deny` are intentionally
        // unused here — see `additive_off_matches_todays_matrix`.
        return match (mode, risk) {
            (Mode::Ask, _) => Decision::AskHuman,
            // Write / Auto / Bypass all auto-approve edits.
            (_, Risk::WorkspaceEdit) => Decision::AutoApprove,
            (Mode::Write, Risk::BashExec) => Decision::AskHuman,
            (Mode::Auto, Risk::BashExec) => {
                if allow.contains(&(quark.clone(), op.to_string())) {
                    Decision::AutoApprove
                } else {
                    Decision::AskHuman
                }
            }
            (Mode::Bypass, Risk::BashExec) => Decision::AutoApprove,
        };
    }

    if mode == Mode::Bypass {
        return Decision::AutoApprove;
    }
    let escalate = if global == Mode::Bypass { Decision::AskOrchestrator } else { Decision::AskHuman };

    if risk == Risk::WorkspaceEdit {
        // The double-table only refines BashExec; edits ride the base ladder
        // free (Write/Auto/Bypass all auto-approve — Bypass is handled above).
        return if mode == Mode::Ask { escalate } else { Decision::AutoApprove };
    }

    // Risk::BashExec: deny wins over allow, then the base ladder per mode, with
    // Auto's allow-list softening the ask exactly as the today-table does.
    //
    // SECURITY: deny is absolute AGAINST THE ORCHESTRATOR — a deny-listed op is
    // `AskHuman` even under global `Bypass`, NEVER `AskOrchestrator`, so the
    // orchestrator LLM is structurally unable to grant what a human explicitly
    // denied. Only the general (non-deny) escalation below consults `global` and
    // may become `AskOrchestrator`.
    //
    // POLICY (user-decided 2026-07-18: "Bypass pin wins"): a human's explicit
    // per-quark `Bypass` pin is handled ABOVE this deny check (line ~209), so a
    // Bypass-pinned worker auto-approves even a deny-listed op — a more-specific
    // human trust signal ("run everything on THIS worker") supersedes the general
    // deny-list. This is intentional. If you ever want deny to beat even a Bypass pin,
    // not even by my most-trusted worker"), move this `rules_match(deny,..)` block
    // ABOVE the `mode == Bypass` return and update `worker_bypass_override_auto_approves`.
    if rules_match(deny, quark, op) {
        return Decision::AskHuman;
    }
    match mode {
        Mode::Ask | Mode::Write => escalate,
        Mode::Auto => {
            if rules_match(allow, quark, op) {
                Decision::AutoApprove
            } else {
                escalate
            }
        }
        Mode::Bypass => unreachable!("handled by the early Bypass return above"),
    }
}

/// What a tool call actually does, coarse enough for the mode ladder to answer
/// it without a human. Three classes, because three is what the ladder
/// distinguishes: `Write` means "edits auto-approve; every command asks you".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// Reads the worktree and changes nothing (read a file, list a directory,
    /// grep, show a diff).
    Read,
    /// Changes a file through the sanctioned, jailed edit path.
    Edit,
    /// Runs a program.
    Exec,
}

/// Whether `mode` pre-authorizes a tool of this class on a path where **nobody
/// can be asked**: the agent is mid-turn and blocking on our answer, so the only
/// two outcomes are allow and refuse.
///
/// This is [`decide`]'s ladder with its ask-a-human rungs collapsed to refusal,
/// and it is deliberately the narrow version: `Auto` allows a command outright
/// rather than consulting the allow-list, because trust-on-first-use needs a
/// human to answer the first time and there is no one here to answer. The
/// blast radius of that choice is one tool call, inside a worktree jail.
///
/// `Ask` refuses everything, reads included — the rung's promise is "the quark
/// may talk, not act", and a read that only *looks* harmless is still the quark
/// acting (see `notes/ask-posture-rejects-forge-reads-too.md`).
pub fn tool_allowed(mode: Mode, class: ToolClass) -> bool {
    match (mode, class) {
        (Mode::Ask, _) => false,
        (_, ToolClass::Read | ToolClass::Edit) => true,
        (Mode::Write, ToolClass::Exec) => false,
        // Auto and Bypass; Ask and Write are both answered above.
        (_, ToolClass::Exec) => true,
    }
}

#[cfg(test)]
mod tests;
