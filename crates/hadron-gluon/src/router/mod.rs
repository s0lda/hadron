use hadron_lattice::{Actor, EnergyState, Event, Flavor, Kind, QuarkCard, QuarkId, QuarkState};

use crate::preons::Preon;

/// Which quark should be excited next.
///
/// v1 rule (stateless, reconstructed from the field): walk the field newest-first for
/// events that address a quark (`to = Some(q)`) and request a turn, and return the
/// first one the addressee has **not** answered — answered meaning `q` authored a reply
/// or a terminal/pause status after it ([`is_turn_completion`]). Nothing unanswered →
/// quiesce (`None`).
///
/// **The walk is the whole point.** This used to stop at the single newest addressed
/// request and return `None` if it was answered, which let one answered request MASK an
/// older unanswered one behind it. Live: a worker reports up to the orchestrator
/// (`Message → orch`), then the merge gate appends its own audit `PermissionReq` +
/// `PermissionGrant{→worker}` — an addressed turn-request — and the worker's `Ground`
/// answers that grant a moment later. The orchestrator's message was then never even
/// examined, so it was never excited and the human's chat sat silent until they typed
/// again (the retype works because it goes through `unaddressed_message_targets`, a
/// different path). See `an_answered_merge_grant_does_not_mask_an_older_unanswered_handoff`,
/// replayed from a real `field.jsonl`.
///
/// Walking back is safe because every path that addresses a quark also guarantees it a
/// terminal status: a disabled/exclusive/depleted seat gets `reroute_blocked`
/// (`Status{Blocked}`), a failed turn gets `Status{Error}`, and a merge gate whose
/// `land()` errors reroutes to `Blocked` rather than propagating (`engine/merge.rs`) —
/// which is what stops an unanswerable grant from being rediscovered forever.
///
/// Only the newest unanswered request is returned. Two quarks masked at once therefore
/// resolve one per dispatch pass, which the engine's re-read loop already does.
pub fn next_pending(events: &[Event]) -> Option<QuarkId> {
    events
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, e)| e.to.is_some() && is_turn_request(e))
        .map(|(idx, e)| (idx, e.to.clone().unwrap()))
        .find(|(idx, target)| !events[idx + 1..].iter().any(|e| is_turn_completion(e, target)))
        .map(|(_, target)| target)
}

/// Does event `e` **request or resume** a turn from the quark it addresses (`to =
/// Some(q)`)? A `Message` or `Assign` starts a new turn; a `PermissionGrant` resumes a
/// paused one (the engine re-dispatches on `to == quark`).
///
/// The SSOT for "an addressed event that wants a turn", the counterpart to
/// [`is_turn_completion`]. Control/config events that ALSO carry a `to` — `Reboot`
/// (force-restart), `ModeSet`/`ModeClear` (per-quark posture) — are deliberately
/// excluded: they address a quark to reconfigure it, not to make it work. Counting one
/// as pending gave the quark a spurious empty turn — most visibly on every `/clear`,
/// which appends one `Reboot` per resident quark, so the last-addressed reboot read as
/// an unanswered turn request and excited that quark.
pub fn is_turn_request(e: &Event) -> bool {
    matches!(
        e.kind,
        Kind::Message { .. } | Kind::Assign { .. } | Kind::PermissionGrant { .. }
    )
}

/// Does event `e` represent `quark` **completing** a turn — a reply (a `Message`) or a
/// terminal/pause status (`Ground`, `Error`, `Blocked`, `Waiting`)?
///
/// This is the single source of truth for "the quark did something that ends a turn",
/// shared by [`next_pending`] and the engine's `has_answered` so the two can never drift
/// into disagreeing about whether a quark is still pending. It deliberately EXCLUDES the
/// non-terminal `Excited`/`Thinking` statuses: those mean "started"/"working", not "done".
/// Counting an `Excited` as an answer is exactly what stranded a quark whose turn was
/// interrupted (a restart) after it went Excited but before it replied — the field kept it
/// marked answered and it was never re-dispatched.
pub fn is_turn_completion(e: &Event, quark: &QuarkId) -> bool {
    e.from == Actor::Quark(quark.clone())
        && (matches!(e.kind, Kind::Message { .. })
            || matches!(
                e.kind,
                Kind::Status {
                    state: QuarkState::Ground
                        | QuarkState::Error
                        | QuarkState::Blocked
                        | QuarkState::Waiting
                }
            ))
}

/// The role alias every quark can address without knowing who currently holds the
/// role. `@orchestrator` resolves against the live roster, so re-flavouring the
/// team in `team.json` moves the escalation target with it and no prompt, test, or
/// invariant that names the role goes stale. Reserved in `validate_quark_id`, so a
/// quark can never take the alias as its own id.
pub const ORCHESTRATOR_ALIAS: &str = "orchestrator";

/// The broadcast alias: `@team` addresses every quark on the roster, which the
/// daemon then fans out to sequentially (each answers in turn) — the "everyone
/// report status" case.
///
/// Deliberately **human-only**. `parse_addressee` (the quark→quark path) does not
/// resolve it, because a quark that broadcasts to the team excites every other
/// quark, each of whom may broadcast back: an amplification loop bounded only by
/// the exchange backstop. A human can rally the team; a quark must name who it
/// wants.
pub const TEAM_ALIAS: &str = "team";

#[derive(Clone)]
enum ResolvedMention<'a> {
    Quark(&'a QuarkCard),
    Team,
}

/// Does `text` start (case-insensitively) with `target_name`, at a mention-safe word
/// boundary? The single boundary rule every mention matcher in this file shares —
/// `try_match` below and [`task_names_card_specifically`] both ride it, so a
/// multi-byte-char panic fixed once here can never resurface in a second hand-rolled
/// matcher.
///
/// `get(..n)` is `None` when `n` is past the end OR not a char boundary. The boundary
/// case is the one a raw `&text[..n]` slice panicked on: a multi-byte char (e.g. a
/// smart quote) straddling the candidate's byte length. Skipping is correct — a
/// prefix cut mid-char could never equal an ASCII target name. The character
/// immediately following the match must NOT be a valid intra-word mention character
/// (alphanumeric, '-', '_'), so `@Google` doesn't match `@GoogleBot`.
fn boundary_match(text: &str, target_name: &str) -> bool {
    let Some(prefix) = text.get(..target_name.len()) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case(target_name) {
        return false;
    }
    let next_char = text[target_name.len()..].chars().next();
    !matches!(next_char, Some(c) if c.is_alphanumeric() || c == '-' || c == '_')
}

/// First enabled (non-depleted) card in roster order carrying `role` (case-
/// insensitive) — "enabled" here meaning the same *selection* filter the role
/// pass below applies (a depleted seat is skipped), not the id/alias path's
/// "resolve even a disabled seat" rule.
///
/// This is the preon pass's counterpart to the role pass's fused text-match
/// loop below, not a literal extraction of it: the role pass matches TEXT
/// against roster role strings it doesn't know in advance (so it fuses the
/// match into `try_match`'s longest-wins scan); the preon pass already has
/// the exact `preferred_role` string in hand from a preon that matched by
/// name, so it needs only a direct roster-order lookup. Both agree on roster-
/// order-wins-ties and skip-depleted, so a role resolved by `@role` and by
/// `@preon-name` never disagree about which seat holds it — but the role
/// pass itself is deliberately left untouched: threading every card/role pair
/// through this instead risks shifting the existing equal-length tie order.
pub(crate) fn card_for_role<'a>(roster: &'a [QuarkCard], role: &str) -> Option<&'a QuarkCard> {
    roster.iter().find(|card| {
        card.energy != EnergyState::Depleted
            && card.roles.iter().any(|r| !r.is_empty() && r.eq_ignore_ascii_case(role))
    })
}

/// Tries to find the longest target that matches the START of `text` (case-insensitively).
/// To prevent partial word matches (e.g. `@Google` matching `@GoogleBot`),
/// the character immediately following the match in `text` must NOT be a valid
/// intra-word mention character (alphanumeric, '-', '_'). Note that spaces ARE
/// allowed inside display names, but a matched name's boundary still applies.
///
/// `preons` adds a fourth, LOWEST-precedence pass (after id/alias/role): a
/// token matching a preon's `name` resolves as if it were that preon's
/// `preferred_role` — i.e. via [`card_for_role`]. A preon with no
/// `preferred_role`, or whose role holds no seat, simply never matches here —
/// no panic, no error, the same soft fall-through an unmatched `@role` gets.
fn match_longest_mention<'a>(
    text: &str,
    roster: &'a [QuarkCard],
    preons: &[Preon],
) -> Option<(usize, ResolvedMention<'a>)> {
    // A free function rather than a capturing closure: `best_match` is passed in
    // explicitly so the borrow ends when each call returns, letting the id/alias
    // pass and the role pass below run as two separate, sequential loops over
    // `best_match` instead of one closure holding it mutably borrowed for the
    // whole function (which would make `best_match.is_none()` unborrowable).
    fn try_match<'a>(
        text: &str,
        target_name: &str,
        resolution: ResolvedMention<'a>,
        best_match: &mut Option<(usize, ResolvedMention<'a>)>,
    ) {
        if boundary_match(text, target_name) {
            let len = target_name.len();
            if best_match.as_ref().map_or(true, |(best_len, _)| len > *best_len) {
                *best_match = Some((len, resolution));
            }
        }
    }

    let mut best_match: Option<(usize, ResolvedMention<'a>)> = None;

    try_match(text, TEAM_ALIAS, ResolvedMention::Team, &mut best_match);

    if let Some(orch) = roster.iter().find(|c| c.flavor == Flavor::Orchestrator) {
        try_match(text, ORCHESTRATOR_ALIAS, ResolvedMention::Quark(orch), &mut best_match);
    }

    for card in roster {
        try_match(text, card.id.as_str(), ResolvedMention::Quark(card), &mut best_match);
        if let Some(dn) = &card.display_name {
            try_match(text, dn.as_str(), ResolvedMention::Quark(card), &mut best_match);
        }
    }

    // Role resolution (Phase 1, soft): only attempted when NO id/alias/display-name
    // matched at all — a separate pass, not folded into the loop above, so id/alias
    // precedence cannot be lost to roster ordering. If it were one loop, a roster
    // with the role-carrying card before the id-carrying card would register the
    // role first and let it win the longest-match tie, silently inverting the
    // "id beats role" rule the spec requires.
    //
    // Filtered to non-depleted cards: this is a *selection* among candidates (like
    // the peer list at `engine.rs`'s `EnergyState::Depleted` filter), unlike an
    // explicit `@id`/alias mention, which resolves even a depleted/disabled seat on
    // purpose (`Engine::set_enabled`'s doc comment) so the engine can report "that
    // quark is disabled" instead of silently answering as someone else.
    if best_match.is_none() {
        for card in roster {
            if card.energy == EnergyState::Depleted {
                continue;
            }
            for role in &card.roles {
                if role.is_empty() {
                    continue;
                }
                try_match(text, role.as_str(), ResolvedMention::Quark(card), &mut best_match);
            }
        }
    }

    // Preon resolution (Phase 2, soft): only attempted when NOTHING above
    // matched — id, alias, AND role all keep precedence over a preon name,
    // same "separate, later pass" reasoning the role pass's own comment gives
    // for id/alias. A preon name resolves to whichever seat carries its
    // `preferred_role` (`card_for_role`); a preon with no `preferred_role`,
    // or whose role holds no seat, is simply skipped — soft fall-through, not
    // an error.
    if best_match.is_none() {
        for preon in preons {
            let Some(role) = &preon.preferred_role else { continue };
            let Some(card) = card_for_role(roster, role) else { continue };
            try_match(text, preon.name.as_str(), ResolvedMention::Quark(card), &mut best_match);
        }
    }

    best_match
}

/// Replaces all Markdown fenced code blocks (``` or ~~~) and inline code spans (`...`)
/// with ASCII spaces (preserving newlines `\n`), returning a String of the exact same
/// byte length as `body`.
///
/// This ensures that `@mentions` inside code blocks, code fences, or inline backticks
/// are never parsed as active routing targets, preventing spurious quark excitations.
/// Preserving byte length and newlines guarantees that line numbers and character offsets
/// outside code blocks match the original text 1-to-1.
pub fn strip_markdown_code(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = bytes.to_vec();
    let len = bytes.len();

    let mut i = 0;
    let mut in_fence: Option<(u8, usize)> = None;

    while i < len {
        let line_start = i;
        while i < len && bytes[i] != b'\n' {
            i += 1;
        }
        let line_end = i;
        if i < len && bytes[i] == b'\n' {
            i += 1;
        }

        let line_bytes = &bytes[line_start..line_end];
        let mut indent = 0;
        while indent < line_bytes.len() && line_bytes[indent] == b' ' {
            indent += 1;
        }
        let unindented = &line_bytes[indent..];

        if let Some((fence_char, fence_len)) = in_fence {
            if indent <= 3 && unindented.len() >= fence_len {
                let mut count = 0;
                while count < unindented.len() && unindented[count] == fence_char {
                    count += 1;
                }
                if count >= fence_len {
                    let trailing = &unindented[count..];
                    if trailing.iter().all(|&b| b == b' ' || b == b'\t' || b == b'\r') {
                        for b in &mut out[line_start..line_end] {
                            *b = b' ';
                        }
                        in_fence = None;
                        continue;
                    }
                }
            }
            for b in &mut out[line_start..line_end] {
                *b = b' ';
            }
        } else {
            if indent <= 3 && unindented.len() >= 3 {
                let first_char = unindented[0];
                if first_char == b'`' || first_char == b'~' {
                    let mut count = 0;
                    while count < unindented.len() && unindented[count] == first_char {
                        count += 1;
                    }
                    if count >= 3 {
                        let info = &unindented[count..];
                        let valid_info = if first_char == b'`' {
                            !info.contains(&b'`')
                        } else {
                            true
                        };
                        if valid_info {
                            in_fence = Some((first_char, count));
                            for b in &mut out[line_start..line_end] {
                                *b = b' ';
                            }
                            continue;
                        }
                    }
                }
            }

            // Inline backtick code spans
            let mut j = line_start;
            while j < line_end {
                if out[j] == b'`' {
                    let tick_start = j;
                    while j < line_end && out[j] == b'`' {
                        j += 1;
                    }
                    let tick_count = j - tick_start;

                    if let Some((_, close_end)) = find_closing_backticks(&out, j, tick_count) {
                        for k in tick_start..close_end {
                            if out[k] != b'\n' && out[k] != b'\r' {
                                out[k] = b' ';
                            }
                        }
                        j = close_end;
                    }
                } else {
                    j += 1;
                }
            }
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| body.to_string())
}

fn find_closing_backticks(bytes: &[u8], start: usize, count: usize) -> Option<(usize, usize)> {
    let mut k = start;
    let mut newlines = 0;
    while k < bytes.len() {
        if bytes[k] == b'\n' {
            newlines += 1;
            if newlines >= 2 {
                return None;
            }
            k += 1;
            continue;
        } else if !bytes[k].is_ascii_whitespace() {
            newlines = 0;
        }

        if bytes[k] == b'`' {
            let run_start = k;
            while k < bytes.len() && bytes[k] == b'`' {
                k += 1;
            }
            let run_len = k - run_start;
            if run_len == count {
                return Some((run_start, k));
            }
        } else {
            k += 1;
        }
    }
    None
}

/// Extract the addressee from a Markdown message: the first line that **starts**
/// with `@quarkid` (after optional leading whitespace) whose id is on the roster.
/// `@orchestrator` also routes here — that is the escalation path a worker uses to
/// put a decision back on whoever holds the role. Returns `None` (hand back to the
/// human / broadcast) if none match.
///
/// A delegation only counts when `@<id>` begins a line — a mention buried in prose
/// or *quoted* from history (e.g. a quark listing the conversation, which contains
/// other quarks' handles) does NOT route. This is what keeps a quark from routing
/// its reply to whoever it happened to name: the original whole-body scan tagged
/// opus's reply `→ agy` merely because it quoted the string `@agy`, spuriously
/// exciting agy. Sender-exclusion is kept as a belt-and-suspenders guard.
pub fn parse_addressee(
    body: &str,
    roster: &[QuarkCard],
    sender: Option<&QuarkId>,
    preons: &[Preon],
) -> Option<QuarkId> {
    let clean = strip_markdown_code(body);
    for line in clean.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(r) = trimmed.strip_prefix('@') {
            r
        } else if let Some(r) = trimmed.strip_prefix("**@") {
            r
        } else {
            continue;
        };
        if let Some((_, resolution)) = match_longest_mention(rest, roster, preons) {
            match resolution {
                ResolvedMention::Quark(card) => {
                    // Sender-exclusion also makes `@orchestrator` a no-op for the
                    // orchestrator itself: its own reply falls through to "no addressee"
                    // and control returns to the human, as it should.
                    if Some(&card.id) != sender {
                        return Some(card.id.clone());
                    }
                }
                ResolvedMention::Team => {}
            }
        }
    }
    None
}

/// Parses all unique line-start mentions in a message body, excluding the sender.
pub fn parse_all_addressees(
    body: &str,
    roster: &[QuarkCard],
    sender: Option<&QuarkId>,
    preons: &[Preon],
) -> Vec<QuarkId> {
    let mut out = Vec::new();
    let clean = strip_markdown_code(body);
    for line in clean.lines() {
        let trimmed = line.trim_start();
        let rest = if let Some(r) = trimmed.strip_prefix('@') {
            r
        } else if let Some(r) = trimmed.strip_prefix("**@") {
            r
        } else {
            continue;
        };
        if let Some((_, resolution)) = match_longest_mention(rest, roster, preons) {
            match resolution {
                ResolvedMention::Quark(card) => {
                    if Some(&card.id) != sender {
                        if !out.contains(&card.id) {
                            out.push(card.id.clone());
                        }
                    }
                }
                ResolvedMention::Team => {
                    for card in roster {
                        if Some(&card.id) != sender {
                            if !out.contains(&card.id) {
                                out.push(card.id.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Every roster quark id `@mentioned` ANYWHERE in a human message, in first-seen
/// order, deduped. Unlike `parse_addressee` (line-start only, for quark replies
/// where an incidental/quoted mention must not route), a human addresses whoever
/// they name — so "@opus do X and you @agy do Y" returns `[opus, agy]` and the
/// daemon fans the turn out to each. Mentions of ids not on the roster are
/// ignored; an `@` not starting a word (e.g. inside `email@host`) is not a mention.
/// Mentions inside Markdown code blocks (fenced ```/~~~ or inline `...`) are ignored.
pub fn human_mentions(body: &str, roster: &[QuarkCard], preons: &[Preon]) -> Vec<QuarkId> {
    let clean = strip_markdown_code(body);
    let mut out: Vec<QuarkId> = Vec::new();
    let mut i = 0;
    while let Some(at_idx) = clean[i..].find('@') {
        let actual_at = i + at_idx;
        let valid_start = actual_at == 0
            || clean.as_bytes()[actual_at - 1].is_ascii_whitespace()
            || (actual_at >= 2 && &clean[actual_at - 2..actual_at] == "**");

        if valid_start {
            let rest = &clean[actual_at + 1..];
            if let Some((match_len, resolution)) = match_longest_mention(rest, roster, preons) {
                match resolution {
                    ResolvedMention::Team => {
                        for card in roster {
                            if !out.contains(&card.id) {
                                out.push(card.id.clone());
                            }
                        }
                    }
                    ResolvedMention::Quark(card) => {
                        if !out.contains(&card.id) {
                            out.push(card.id.clone());
                        }
                    }
                }
                i = actual_at + 1 + match_len;
                continue;
            }
        }
        i = actual_at + 1;
    }
    out
}

/// Whether `task` specifically names `card` — by its own `@id`, its `display_name`
/// (case-insensitive), or a `@role` it carries — using the same char-boundary-safe
/// mention scan [`human_mentions`] uses ([`boundary_match`], inherited, not
/// re-implemented). `display_name` is checked because `match_longest_mention` (the
/// router's OWN resolver) treats it as a full alternate handle for the card — e.g.
/// `@Claude` routes to seat `acp-claude` via `with_display_name` — so an exclusive
/// card the router happily dispatches by its display name must not then be rejected
/// here for "not being addressed by role or @id"; a display name names exactly one
/// card, so admitting it introduces no broadcast-style leak the way `@team` would.
///
/// Mentions inside Markdown code blocks (fenced ```/~~~ or inline `...`) are ignored.
pub fn task_names_card_specifically(task: &str, card: &QuarkCard, preons: &[Preon]) -> bool {
    let clean = strip_markdown_code(task);
    let mut i = 0;
    while let Some(at_idx) = clean[i..].find('@') {
        let actual_at = i + at_idx;
        let valid_start = actual_at == 0
            || clean.as_bytes()[actual_at - 1].is_ascii_whitespace()
            || (actual_at >= 2 && &clean[actual_at - 2..actual_at] == "**");
        if valid_start {
            let rest = &clean[actual_at + 1..];
            if boundary_match(rest, card.id.as_str())
                || card.display_name.as_deref().is_some_and(|dn| boundary_match(rest, dn))
                || card.roles.iter().any(|role| !role.is_empty() && boundary_match(rest, role))
                || preons.iter().any(|p| {
                    p.preferred_role.as_deref().is_some_and(|pr| {
                        card.roles.iter().any(|role| role.eq_ignore_ascii_case(pr))
                    }) && boundary_match(rest, p.name.as_str())
                })
            {
                return true;
            }
        }
        i = actual_at + 1;
    }
    false
}

pub mod balancer;
pub use balancer::*;

#[cfg(test)]
mod tests;

