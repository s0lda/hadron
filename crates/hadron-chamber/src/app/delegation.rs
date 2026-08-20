use chrono::{DateTime, Utc};
use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState, Ulid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationState {
    Pending,
    Working,
    Completed,
    Blocked,
    Error,
}

impl DelegationState {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            DelegationState::Pending => "Pending",
            DelegationState::Working => "Working",
            DelegationState::Completed => "Completed",
            DelegationState::Blocked => "Blocked",
            DelegationState::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Delegation {
    pub from: Actor,
    pub to: QuarkId,
    pub task: String,
    pub turn: Option<Ulid>,
    pub timestamp: DateTime<Utc>,
    pub state: DelegationState,
}

use std::collections::HashMap;

/// Pure function over `&[Event]` -> `Vec<Delegation>`.
/// Parses delegations from explicit `to` fields and unaddressed line-start `@mentions`,
/// resolving mention handles through `aliases` (display_name / handle -> seat QuarkId).
pub fn parse_delegations(events: &[Event], aliases: &HashMap<String, QuarkId>) -> Vec<Delegation> {
    let mut delegations = Vec::new();

    for (i, e) in events.iter().enumerate() {
        let targets = extract_delegation_targets(e, aliases);
        if targets.is_empty() {
            continue;
        }

        let task_text = match &e.kind {
            Kind::Assign { task, .. } => task.clone(),
            Kind::Message { body } => body.clone(),
            _ => continue,
        };

        for target in targets {
            let state = resolve_delegation_state(&target, &events[i + 1..]);
            delegations.push(Delegation {
                from: e.from.clone(),
                to: target,
                task: task_text.clone(),
                turn: e.turn.or(Some(e.id)),
                timestamp: e.ts,
                state,
            });
        }
    }

    delegations
}

/// Extract targeted QuarkIds from an event.
/// If `e.to` is Some(q) and `e.from != Actor::Quark(q)`, returns `vec![q]`.
/// If `e.to` is None and `e.kind` is Message/Assign, scans body for line-start `@mentions`.
fn extract_delegation_targets(e: &Event, aliases: &HashMap<String, QuarkId>) -> Vec<QuarkId> {
    if !matches!(e.kind, Kind::Message { .. } | Kind::Assign { .. }) {
        return vec![];
    }

    if let Some(ref to_quark) = e.to {
        let resolved_to = aliases
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(to_quark.as_str()))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| to_quark.clone());

        if e.from != Actor::Quark(resolved_to.clone()) {
            return vec![resolved_to];
        } else {
            return vec![];
        }
    }

    // Unaddressed message: scan line-start @mentions
    let body = match &e.kind {
        Kind::Message { body } => body.as_str(),
        Kind::Assign { task, .. } => task.as_str(),
        _ => return vec![],
    };

    parse_line_start_mentions(body, &e.from, aliases)
}

/// Parse line-start mentions `@quarkid` in `body`, excluding `sender`.
fn strip_list_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| trimmed.strip_prefix("> "))
    {
        return rest.trim_start();
    }
    // Check numbered list like "1. ", "2. "
    if let Some(pos) = trimmed.find(". ") {
        if trimmed[..pos].chars().all(|c| c.is_ascii_digit()) && !trimmed[..pos].is_empty() {
            return trimmed[pos + 2..].trim_start();
        }
    }
    trimmed
}

/// Ignores lines inside code fences and inline backticks.
/// Does NOT match bold `**@quark**` (starts with `*`, not `@`).
fn parse_line_start_mentions(body: &str, sender: &Actor, aliases: &HashMap<String, QuarkId>) -> Vec<QuarkId> {
    let mut targets: Vec<QuarkId> = Vec::new();
    let clean = hadron_gluon::router::strip_markdown_code(body);

    for line in clean.lines() {
        let trimmed = line.trim_start();

        // Strip leading list/bullet/quote markers like `- `, `* `, `1. `, `> `
        let content = strip_list_prefix(trimmed);

        // Must start with `@` (not `**@`)
        if let Some(rest) = content.strip_prefix('@') {
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(rest.len());
            let handle = &rest[..end];

            if !handle.is_empty() && handle != "team" {
                let qid = aliases
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(handle))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| QuarkId::new(handle));
                if *sender != Actor::Quark(qid.clone()) {
                    if !targets.contains(&qid) {
                        targets.push(qid);
                    }
                }
            }
        }
    }

    targets
}

/// Walk subsequent events to determine the status of a delegation to `target`.
fn resolve_delegation_state(target: &QuarkId, subsequent: &[Event]) -> DelegationState {
    let mut current_state = DelegationState::Pending;

    for e in subsequent {
        if e.from == Actor::Quark(target.clone()) {
            match &e.kind {
                Kind::Status { state } => match state {
                    QuarkState::Thinking | QuarkState::Excited => {
                        if current_state != DelegationState::Completed {
                            current_state = DelegationState::Working;
                        }
                    }
                    QuarkState::Ground => {
                        current_state = DelegationState::Completed;
                    }
                    QuarkState::Error => {
                        current_state = DelegationState::Error;
                    }
                    QuarkState::Blocked => {
                        current_state = DelegationState::Blocked;
                    }
                    QuarkState::Waiting => {}
                },
                Kind::Message { .. } | Kind::Edit { .. } | Kind::Command { .. } => {
                    current_state = DelegationState::Completed;
                }
                _ => {}
            }
        }
    }

    current_state
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState};

    #[test]
    fn explicit_to_creates_delegation() {
        let e = Event::new(
            Actor::Human,
            Some(QuarkId::new("acp-claude")),
            Kind::Message {
                body: "do work".into(),
            },
        );
        let dels = parse_delegations(&[e], &HashMap::new());
        assert_eq!(dels.len(), 1);
        assert_eq!(dels[0].from, Actor::Human);
        assert_eq!(dels[0].to, QuarkId::new("acp-claude"));
        assert_eq!(dels[0].task, "do work");
        assert_eq!(dels[0].state, DelegationState::Pending);
    }

    #[test]
    fn line_start_mentions_create_fanout_delegations() {
        let body = "@acp-claude Take Task 1\n@acp-claude-2 Take Task 2";
        let e = Event::new(Actor::Human, None, Kind::Message { body: body.into() });
        let dels = parse_delegations(&[e], &HashMap::new());
        assert_eq!(dels.len(), 2);
        assert_eq!(dels[0].to, QuarkId::new("acp-claude"));
        assert_eq!(dels[1].to, QuarkId::new("acp-claude-2"));
    }

    #[test]
    fn bold_mentions_are_ignored() {
        let body = "**@acp-claude** Take Task 1";
        let e = Event::new(Actor::Human, None, Kind::Message { body: body.into() });
        let dels = parse_delegations(&[e], &HashMap::new());
        assert!(dels.is_empty(), "bold mentions must not route or create delegations");
    }

    #[test]
    fn fenced_code_blocks_are_ignored() {
        let body = "```\n@acp-claude inside code block\n```";
        let e = Event::new(Actor::Human, None, Kind::Message { body: body.into() });
        let dels = parse_delegations(&[e], &HashMap::new());
        assert!(dels.is_empty(), "mentions inside code blocks must be ignored");
    }

    #[test]
    fn delegation_state_transitions_to_working_and_completed() {
        let e1 = Event::new(
            Actor::Human,
            Some(QuarkId::new("acp-claude")),
            Kind::Message {
                body: "do work".into(),
            },
        );
        let e2 = Event::new(
            Actor::Quark(QuarkId::new("acp-claude")),
            None,
            Kind::Status {
                state: QuarkState::Excited,
            },
        );
        let e3 = Event::new(
            Actor::Quark(QuarkId::new("acp-claude")),
            None,
            Kind::Message {
                body: "done".into(),
            },
        );

        let dels_pending = parse_delegations(&[e1.clone()], &HashMap::new());
        assert_eq!(dels_pending[0].state, DelegationState::Pending);

        let dels_working = parse_delegations(&[e1.clone(), e2.clone()], &HashMap::new());
        assert_eq!(dels_working[0].state, DelegationState::Working);

        let dels_completed = parse_delegations(&[e1.clone(), e2.clone(), e3.clone()], &HashMap::new());
        assert_eq!(dels_completed[0].state, DelegationState::Completed);
    }

    #[test]
    fn display_name_mention_resolves_to_seat_id_and_completes() {
        let mut aliases = HashMap::new();
        aliases.insert("Agy".into(), QuarkId::new("cli-agy"));

        let e1 = Event::new(
            Actor::Human,
            None,
            Kind::Message {
                body: "@Agy do X".into(),
            },
        );
        let e2 = Event::new(
            Actor::Quark(QuarkId::new("cli-agy")),
            None,
            Kind::Status {
                state: QuarkState::Ground,
            },
        );

        let dels = parse_delegations(&[e1, e2], &aliases);
        assert_eq!(dels.len(), 1);
        assert_eq!(dels[0].to, QuarkId::new("cli-agy"));
        assert_eq!(dels[0].state, DelegationState::Completed);
    }

    #[test]
    fn test_bullet_and_orchestrator_mentions() {
        let mut aliases = HashMap::new();
        aliases.insert("orchestrator".into(), QuarkId::new("cli-agy"));
        aliases.insert("Agy".into(), QuarkId::new("cli-agy"));

        let body = "- @Agy please run tests\n* @acp-claude fix bugs\n> @lattice-bot orchestrate swarm";
        let e = Event::new(Actor::Human, None, Kind::Message { body: body.into() });
        let dels = parse_delegations(&[e], &aliases);
        assert_eq!(dels.len(), 3);
        assert_eq!(dels[0].to, QuarkId::new("cli-agy"));
        assert_eq!(dels[1].to, QuarkId::new("acp-claude"));
        assert_eq!(dels[2].to, QuarkId::new("lattice-bot"));
    }
}
