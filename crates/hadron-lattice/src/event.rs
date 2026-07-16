use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

use crate::QuarkId;

/// Who authored an event. Serializes as a bare string: "human", "gluon",
/// or the quark's id. `human` and `gluon` are reserved names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Human,
    Gluon,
    Quark(QuarkId),
}

impl Serialize for Actor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let text = match self {
            Actor::Human => "human",
            Actor::Gluon => "gluon",
            Actor::Quark(q) => q.as_str(),
        };
        s.serialize_str(text)
    }
}

impl<'de> Deserialize<'de> for Actor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "human" => Actor::Human,
            "gluon" => Actor::Gluon,
            _ => Actor::Quark(QuarkId(s)),
        })
    }
}

/// Lifecycle state of a quark, used to drive the chamber roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarkState {
    Ground,
    Excited,
    Thinking,
    Waiting,
    Blocked,
    Error,
}

/// The category of a proposed operation, carried on a `PermissionReq`. Matched
/// against the effective mode by `hadron_gatekeeper::decide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Writing, editing, or deleting files inside the workspace.
    WorkspaceEdit,
    /// Executing a shell command (includes publish-class ops like `cargo publish`).
    BashExec,
}

/// How much permission authority the human delegates to the orchestrator for a
/// quark (or, globally, for the whole swarm). An autonomy ladder carried on a
/// `Kind::ModeSet` event — the field is the source of truth, so a running daemon
/// honours a change on its next tick and re-opening a field restores the setting.
///
/// - `Ask`: every op asks the human (pure conversation).
/// - `Write`: edits auto-approve; every command asks the human.
/// - `Auto`: edits auto-approve; a command asks the human once, then is
///   remembered (trust-on-first-use) per quark; off-list commands still ask.
/// - `Bypass`: the orchestrator owns it — the gluon auto-approves and the human
///   is never asked (the request + grant are still recorded for audit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Ask,
    Write,
    Auto,
    Bypass,
}

/// The payload of an event. Known variants flatten into the envelope under a
/// `"kind"` tag. Unknown kinds are preserved verbatim for forward-compat.
///
/// NOTE: derives `PartialEq` but not `Eq` — `Kind::Unknown` holds a
/// `serde_json::Value`, which is not `Eq` (JSON numbers may be floats).
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Message { body: String },
    Status { state: QuarkState },
    Edit { paths: Vec<String>, git: String, summary: String },
    Command { cmd: String, exit: i32, out_summary: String },
    Snapshot { git: String, label: String },
    EnergyReport { used_tokens: u32 },
    Assign { task: String, invariants: Vec<String> },
    PermissionReq { risk: Risk, description: String },
    PermissionGrant { approved: bool, remember: bool },
    /// Set the permission mode. The envelope's `to` field is the target:
    /// `Some(quark)` = a per-quark override, `None` = the global default.
    ModeSet { mode: Mode },
    /// Clear a **per-quark** permission override so the quark reverts to inheriting
    /// the global default (the "Default" rung in the UI). The envelope's `to` names
    /// the quark. This is how an append-only field expresses "un-set": the latest
    /// per-quark mode event wins, so a `ModeClear` after a `ModeSet` means inherit.
    ModeClear,
    /// Force-restart a **resident** quark: reap its live agent subprocess now and let
    /// it re-boot on its next turn. The envelope's `to` names the quark. A human-issued
    /// recovery for a wedged ACP agent; the daemon services it immediately (aborting an
    /// in-flight turn if one is running). A no-op for a quark that holds nothing resident
    /// (the CLI transports spawn per turn).
    Reboot,
    /// Any kind this version does not understand. `raw` holds the full set of
    /// non-envelope fields so the event can be re-serialized and displayed.
    Unknown { kind: String, raw: Value },
}

/// One line in the field. The envelope (`v/id/ts/from/to`) plus a flattened kind,
/// plus optional telemetry.
/// `PartialEq` but not `Eq` (contains `Kind`, which is not `Eq`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub v: u32,
    pub id: Ulid,
    pub ts: DateTime<Utc>,
    pub from: Actor,
    pub to: Option<QuarkId>,
    pub kind: Kind,
    /// What the turn cost and what budget is left — context usage and quota buckets.
    /// In practice only ever set on an `energy_report`; `None` everywhere else.
    ///
    /// WHY THE ENVELOPE AND NOT A NEW `Kind`: `Kind` is matched **exhaustively, with
    /// no wildcard arm, by a crate this one does not own** (the chamber's
    /// `model.rs`). Adding a variant to `Kind` — or a field to `Kind::EnergyReport` —
    /// is therefore a *breaking* change that fails the workspace build until every
    /// downstream reader is edited in lockstep. An additive `Option` field on the
    /// envelope is source-compatible for every reader (nothing destructures `Event`;
    /// they all go through [`Event::new`] and field access), so the schema can grow
    /// without the lattice reaching into a UI it does not own. The tradeoff is that
    /// telemetry is envelope-shaped rather than payload-shaped; if `Kind` ever gains
    /// a wildcard arm downstream, this is the first thing that should move into it.
    pub usage: Option<crate::Usage>,
    /// **Which turn produced this event.** Every event one turn emits — its reply, its
    /// energy report, its edits — carries the same id.
    ///
    /// WHY IT EXISTS: telemetry rides on the `energy_report` and the reply rides on a
    /// *separate* `message`. Without this, a reader wanting to say "this reply cost X"
    /// has nothing to join on but **adjacency** — and adjacency is a guess that breaks
    /// the first time two quarks answer at once, which is the normal case here (turns
    /// run in parallel, and the field is a single interleaved log). A number that is
    /// right most of the time is the exact class of quiet lie this codebase keeps
    /// finding, so the link is made explicit on the wire instead of inferred in the UI.
    ///
    /// WHY A TURN ID AND NOT A COPY OF `Usage` ON THE MESSAGE: that would give one fact
    /// two homes, which is the SSOT violation `TokenSpend` was just built to end. It
    /// also only links those two events, where an id groups the whole turn.
    ///
    /// `None` for every event written before this existed, and for events the engine
    /// emits outside a turn (a human message, a mode set).
    pub turn: Option<Ulid>,
    /// **Which message this event is an answer to** — the id of the *assignment* that
    /// drove the turn that emitted it.
    ///
    /// WHY IT EXISTS: without it, "has this quark answered the human yet?" can only be
    /// asked as *"has it authored anything since?"* — and that is **wrong whenever the
    /// human speaks while the quark is already working.** The quark finishes the turn it
    /// was on, its reply lands after the newer message, and the newer message is marked
    /// answered by a reply that could not possibly have seen it. The human's message is
    /// then dropped, silently, forever. That is not a hypothetical: it is what happens
    /// every time Jake types a second time while the orchestrator is mid-turn.
    ///
    /// So the link is made explicit on the wire rather than inferred from order. An
    /// event that answers assignment `A` says so; a message nobody has answered stays
    /// pending, however many other replies have flown past it.
    ///
    /// `None` for events written before this existed (they keep the old, order-based
    /// reading — see `human_message_targets`) and for events emitted outside a turn.
    pub answers: Option<Ulid>,
    /// **The reply as the quark actually wrote it, when the engine trimmed it.**
    ///
    /// The engine caps how long a reply may be (`gluon::brevity`) because asking a model
    /// to be brief is prompt text, and prompt text does not enforce. The cap changes what
    /// the human and the other quarks are made to *read* — it must not change what the
    /// swarm can still recover, or a trim would be a quiet deletion of evidence.
    ///
    /// So `body` carries the capped text and this carries the original. `None` is the
    /// normal case and means exactly "nothing was cut" — absent is not "the same as
    /// `body`", and a reader must not synthesise one from the other.
    pub full: Option<String>,
}

impl Event {
    /// Construct a fresh event, stamping schema version, a new ULID, and now().
    pub fn new(from: Actor, to: Option<QuarkId>, kind: Kind) -> Self {
        Event {
            v: 1,
            id: Ulid::new(),
            ts: Utc::now(),
            from,
            to,
            kind,
            usage: None,
            turn: None,
            answers: None,
            full: None,
        }
    }

    /// Keep the untrimmed reply alongside the trimmed one. Called by the engine when —
    /// and only when — [`crate::Event::full`] would differ from the body it is shipping.
    pub fn with_full(mut self, full: String) -> Self {
        self.full = Some(full);
        self
    }

    /// Record which assignment this event answers. The engine stamps it on every event
    /// a turn emits, so "the human is still waiting" is a fact in the log rather than a
    /// guess about what came after what.
    pub fn with_answers(mut self, assignment: Ulid) -> Self {
        self.answers = Some(assignment);
        self
    }

    /// [`Event::with_answers`] for a caller that may or may not have an assignment —
    /// a turn with no task-bearing driver answers nothing, and must not claim to.
    pub fn answering(mut self, assignment: Option<Ulid>) -> Self {
        self.answers = assignment;
        self
    }

    /// Stamp this event with the turn that produced it. The engine calls it on every
    /// event a single turn emits, so a reader can join a reply to its own telemetry
    /// instead of guessing by adjacency.
    pub fn with_turn(mut self, turn: Ulid) -> Self {
        self.turn = Some(turn);
        self
    }

    /// Attach telemetry to an event. Empty usage attaches nothing, so a provider with
    /// no numbers to report (a mock, a silent turn) never writes a hollow `usage: {}`
    /// into the field.
    pub fn with_usage(mut self, usage: crate::Usage) -> Self {
        if !usage.is_empty() {
            self.usage = Some(usage);
        }
        self
    }
}

impl Serialize for Event {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("v", &self.v)?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("ts", &self.ts)?;
        m.serialize_entry("from", &self.from)?;
        m.serialize_entry("to", &self.to)?;
        if let Some(usage) = &self.usage {
            m.serialize_entry("usage", usage)?;
        }
        if let Some(turn) = &self.turn {
            m.serialize_entry("turn", turn)?;
        }
        if let Some(answers) = &self.answers {
            m.serialize_entry("answers", answers)?;
        }
        if let Some(full) = &self.full {
            m.serialize_entry("full", full)?;
        }
        match &self.kind {
            Kind::Message { body } => {
                m.serialize_entry("kind", "message")?;
                m.serialize_entry("body", body)?;
            }
            Kind::Status { state } => {
                m.serialize_entry("kind", "status")?;
                m.serialize_entry("state", state)?;
            }
            Kind::Edit { paths, git, summary } => {
                m.serialize_entry("kind", "edit")?;
                m.serialize_entry("paths", paths)?;
                m.serialize_entry("git", git)?;
                m.serialize_entry("summary", summary)?;
            }
            Kind::Command { cmd, exit, out_summary } => {
                m.serialize_entry("kind", "command")?;
                m.serialize_entry("cmd", cmd)?;
                m.serialize_entry("exit", exit)?;
                m.serialize_entry("out_summary", out_summary)?;
            }
            Kind::Snapshot { git, label } => {
                m.serialize_entry("kind", "snapshot")?;
                m.serialize_entry("git", git)?;
                m.serialize_entry("label", label)?;
            }
            Kind::EnergyReport { used_tokens } => {
                m.serialize_entry("kind", "energy_report")?;
                m.serialize_entry("used_tokens", used_tokens)?;
            }
            Kind::Assign { task, invariants } => {
                m.serialize_entry("kind", "assign")?;
                m.serialize_entry("task", task)?;
                m.serialize_entry("invariants", invariants)?;
            }
            Kind::PermissionReq { risk, description } => {
                m.serialize_entry("kind", "permission_req")?;
                m.serialize_entry("risk", risk)?;
                m.serialize_entry("description", description)?;
            }
            Kind::PermissionGrant { approved, remember } => {
                m.serialize_entry("kind", "permission_grant")?;
                m.serialize_entry("approved", approved)?;
                m.serialize_entry("remember", remember)?;
            }
            Kind::ModeSet { mode } => {
                m.serialize_entry("kind", "mode_set")?;
                m.serialize_entry("mode", mode)?;
            }
            Kind::ModeClear => {
                m.serialize_entry("kind", "mode_clear")?;
            }
            Kind::Reboot => {
                m.serialize_entry("kind", "reboot")?;
            }
            Kind::Unknown { kind, raw } => {
                m.serialize_entry("kind", kind)?;
                if let Value::Object(obj) = raw {
                    for (k, val) in obj {
                        m.serialize_entry(k, val)?;
                    }
                }
            }
        }
        m.end()
    }
}

fn take_field<T, E>(map: &mut serde_json::Map<String, Value>, key: &str) -> Result<T, E>
where
    T: serde::de::DeserializeOwned,
    E: serde::de::Error,
{
    let val = map
        .remove(key)
        .ok_or_else(|| E::custom(format!("missing field `{key}`")))?;
    serde_json::from_value(val).map_err(E::custom)
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let mut map = serde_json::Map::<String, Value>::deserialize(d)?;
        let v: u32 = take_field(&mut map, "v")?;
        let id: Ulid = take_field(&mut map, "id")?;
        let ts: DateTime<Utc> = take_field(&mut map, "ts")?;
        let from: Actor = take_field(&mut map, "from")?;
        let to: Option<QuarkId> = match map.remove("to") {
            None | Some(Value::Null) => None,
            Some(val) => Some(serde_json::from_value(val).map_err(D::Error::custom)?),
        };
        // Additive: every event written before telemetry existed simply has no `usage`.
        // Taken BEFORE the kind tag so it never leaks into `Kind::Unknown`'s `raw`.
        let usage: Option<crate::Usage> = match map.remove("usage") {
            None | Some(Value::Null) => None,
            Some(val) => Some(serde_json::from_value(val).map_err(D::Error::custom)?),
        };
        // Additive, and taken BEFORE the kind tag so it never leaks into
        // `Kind::Unknown`'s `raw`. Absent on every line written before turn ids existed.
        let turn: Option<Ulid> = match map.remove("turn") {
            None | Some(Value::Null) => None,
            Some(val) => Some(serde_json::from_value(val).map_err(D::Error::custom)?),
        };
        // Same treatment, same reason. Absent on every line written before an event
        // could say WHICH message it answers — and `None` there means "unknown", which
        // is exactly why the legacy reading has to be kept alongside the new one.
        let answers: Option<Ulid> = match map.remove("answers") {
            None | Some(Value::Null) => None,
            Some(val) => Some(serde_json::from_value(val).map_err(D::Error::custom)?),
        };
        // The untrimmed reply, when the engine capped one. Taken before the kind tag for
        // the same reason as the others: otherwise it lands in `Kind::Unknown`'s `raw`.
        let full: Option<String> = match map.remove("full") {
            None | Some(Value::Null) => None,
            Some(val) => Some(serde_json::from_value(val).map_err(D::Error::custom)?),
        };
        let kind_tag: String = take_field(&mut map, "kind")?;
        let kind = match kind_tag.as_str() {
            "message" => Kind::Message {
                body: take_field(&mut map, "body")?,
            },
            "status" => Kind::Status {
                state: take_field(&mut map, "state")?,
            },
            "edit" => Kind::Edit {
                paths: take_field(&mut map, "paths")?,
                git: take_field(&mut map, "git")?,
                summary: take_field(&mut map, "summary")?,
            },
            "command" => Kind::Command {
                cmd: take_field(&mut map, "cmd")?,
                exit: take_field(&mut map, "exit")?,
                out_summary: take_field(&mut map, "out_summary")?,
            },
            "snapshot" => Kind::Snapshot {
                git: take_field(&mut map, "git")?,
                label: take_field(&mut map, "label")?,
            },
            "energy_report" => Kind::EnergyReport {
                used_tokens: take_field(&mut map, "used_tokens")?,
            },
            "assign" => Kind::Assign {
                task: take_field(&mut map, "task")?,
                invariants: take_field(&mut map, "invariants")?,
            },
            "permission_req" => Kind::PermissionReq {
                risk: take_field(&mut map, "risk")?,
                description: take_field(&mut map, "description")?,
            },
            "permission_grant" => Kind::PermissionGrant {
                approved: take_field(&mut map, "approved")?,
                // Additive field: legacy grants (pre-mode-ladder) omit it → false.
                remember: match map.remove("remember") {
                    None | Some(Value::Null) => false,
                    Some(val) => serde_json::from_value(val).map_err(D::Error::custom)?,
                },
            },
            "mode_set" => Kind::ModeSet {
                mode: take_field(&mut map, "mode")?,
            },
            "mode_clear" => Kind::ModeClear,
            "reboot" => Kind::Reboot,
            other => Kind::Unknown {
                kind: other.to_string(),
                raw: Value::Object(map.clone()),
            },
        };
        Ok(Event { v, id, ts, from, to, kind, usage, turn, answers, full })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_serializes_as_bare_string() {
        assert_eq!(serde_json::to_string(&Actor::Human).unwrap(), r#""human""#);
        assert_eq!(serde_json::to_string(&Actor::Gluon).unwrap(), r#""gluon""#);
        assert_eq!(
            serde_json::to_string(&Actor::Quark(QuarkId::new("claude"))).unwrap(),
            r#""claude""#
        );
    }

    #[test]
    fn actor_round_trips_quark_and_reserved() {
        for actor in [
            Actor::Human,
            Actor::Gluon,
            Actor::Quark(QuarkId::new("agy")),
        ] {
            let json = serde_json::to_string(&actor).unwrap();
            let back: Actor = serde_json::from_str(&json).unwrap();
            assert_eq!(actor, back);
        }
    }

    #[test]
    fn quark_state_is_snake_case() {
        assert_eq!(serde_json::to_string(&QuarkState::Ground).unwrap(), r#""ground""#);
        let back: QuarkState = serde_json::from_str(r#""excited""#).unwrap();
        assert_eq!(back, QuarkState::Excited);
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;

    #[test]
    fn message_event_round_trips() {
        let ev = Event::new(
            Actor::Human,
            Some(QuarkId::new("claude")),
            Kind::Message { body: "# Build auth".into() },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        // envelope keys present, kind flattened
        assert!(line.contains(r#""kind":"message""#));
        assert!(line.contains(r##""body":"# Build auth""##));
    }

    #[test]
    fn assign_event_round_trips() {
        let ev = Event::new(
            Actor::Human,
            Some(QuarkId::new("claude")),
            Kind::Assign {
                task: "fix tests".into(),
                invariants: vec!["pass".into()],
            },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"assign""#));
        assert!(line.contains(r#""task":"fix tests""#));
        assert!(line.contains(r#""invariants":["pass"]"#));
    }

    #[test]
    fn permission_req_round_trips() {
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            Some(QuarkId::new("human")),
            Kind::PermissionReq {
                risk: Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_req""#));
        assert!(line.contains(r#""risk":"bash_exec""#));
        assert!(line.contains(r#""description":"cargo publish""#));
    }

    #[test]
    fn permission_grant_round_trips() {
        let ev = Event::new(
            Actor::Human,
            None,
            Kind::PermissionGrant { approved: true, remember: true },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_grant""#));
        assert!(line.contains(r#""approved":true"#));
        assert!(line.contains(r#""remember":true"#));
    }

    #[test]
    fn permission_grant_without_remember_defaults_false() {
        // A grant written before the mode ladder has no `remember` field.
        let line = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"human","to":"agy","kind":"permission_grant","approved":true}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.kind, Kind::PermissionGrant { approved: true, remember: false });
    }

    #[test]
    fn mode_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Mode::Ask).unwrap(), r#""ask""#);
        assert_eq!(serde_json::to_string(&Mode::Bypass).unwrap(), r#""bypass""#);
        assert_eq!(Mode::default(), Mode::Ask);
        let back: Mode = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(back, Mode::Auto);
    }

    #[test]
    fn mode_set_round_trips_global_and_per_quark() {
        // Global default (to: None).
        let global = Event::new(Actor::Human, None, Kind::ModeSet { mode: Mode::Auto });
        let line = serde_json::to_string(&global).unwrap();
        assert!(line.contains(r#""kind":"mode_set""#));
        assert!(line.contains(r#""mode":"auto""#));
        assert_eq!(serde_json::from_str::<Event>(&line).unwrap(), global);

        // Per-quark override (to: Some(quark)).
        let per = Event::new(
            Actor::Human,
            Some(QuarkId::new("agy")),
            Kind::ModeSet { mode: Mode::Bypass },
        );
        let back: Event = serde_json::from_str(&serde_json::to_string(&per).unwrap()).unwrap();
        assert_eq!(back, per);
        assert_eq!(back.to, Some(QuarkId::new("agy")));
    }

    #[test]
    fn mode_clear_round_trips() {
        // The "Default" rung: a per-quark clear that reverts to the global default.
        let ev = Event::new(Actor::Human, Some(QuarkId::new("opus")), Kind::ModeClear);
        let line = serde_json::to_string(&ev).unwrap();
        assert!(line.contains(r#""kind":"mode_clear""#));
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.kind, Kind::ModeClear);
        assert_eq!(back.to, Some(QuarkId::new("opus")));
    }

    #[test]
    fn reboot_round_trips() {
        // A human-issued force-restart, targeted at one quark.
        let ev = Event::new(Actor::Human, Some(QuarkId::new("acp-claude")), Kind::Reboot);
        let line = serde_json::to_string(&ev).unwrap();
        assert!(line.contains(r#""kind":"reboot""#));
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.kind, Kind::Reboot);
        assert_eq!(back.to, Some(QuarkId::new("acp-claude")));
    }

    #[test]
    fn null_to_deserializes_as_none() {
        let line = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"claude","to":null,"kind":"status","state":"ground"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.to, None);
        assert_eq!(ev.kind, Kind::Status { state: QuarkState::Ground });
    }

    /// An energy report carries the turn's context + quota, and survives a round trip.
    #[test]
    fn energy_report_carries_usage_and_round_trips() {
        use crate::{ContextUsage, QuotaBucket, TokenSpend, Usage};

        let usage = Usage {
            model: Some("haiku".to_string()),
            spend: TokenSpend::default(),
            context: Some(ContextUsage {
                used_tokens: 635,
                context_window_size: 1_048_576,
                used_percentage: 0.0605,
            }),
            quota: vec![QuotaBucket {
                key: "gemini-weekly".into(),
                remaining_fraction: 0.0517783,
                reset_time: None,
            }],
        };
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::EnergyReport { used_tokens: 927 },
        )
        .with_usage(usage.clone());

        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.usage.unwrap(), usage);
        assert!(line.contains(r#""kind":"energy_report""#));
        assert!(line.contains(r#""used_tokens":927"#));
        assert!(line.contains(r#""gemini-weekly""#));
    }

    /// **PRIVACY, at the boundary that matters.** The field (`.hadron/field.jsonl`) is
    /// a durable, human-readable log the chamber renders. The agy statusline payload
    /// that feeds an energy report also carries the human's email address and plan
    /// tier. Neither may EVER reach a serialized event — this is the test that fails
    /// loudly if someone ever widens the parser to a derived mirror struct.
    #[test]
    fn email_never_reaches_the_serialized_event() {
        // The real payload shape, with a real-looking email in it.
        let payload = r#"{"cwd":"/home/Jake/dev/hadron","conversation_id":"b66f1126","model":{"id":"Gemini 3.1 Pro (High)"},"context_window":{"total_input_tokens":635,"total_output_tokens":292,"context_window_size":1048576,"used_percentage":0.06},"quota":{"gemini-weekly":{"remaining_fraction":0.0517783,"reset_time":"2026-07-13T13:12:03Z"}},"plan_tier":"Google AI Ultra","email":"secret.human@gmail.com","terminal_width":80}"#;

        let t = crate::parse_agy_statusline(payload).unwrap();
        let ev = Event::new(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::EnergyReport { used_tokens: t.usage.spend.fresh().unwrap_or(0) },
        )
        .with_usage(t.usage);

        let line = serde_json::to_string(&ev).unwrap();
        assert!(!line.contains("secret.human@gmail.com"), "email in the field: {line}");
        assert!(!line.contains("gmail"), "no email fragment in the field: {line}");
        assert!(!line.contains('@'), "nothing email-shaped in the field: {line}");
        assert!(!line.contains("Google AI Ultra"), "plan tier in the field: {line}");
        assert!(!line.contains("Ultra"), "no plan-tier fragment: {line}");
        // ...while the numbers we DO want are there.
        assert!(line.contains(r#""used_tokens":927"#));
        assert!(line.contains("0.0517783"));
    }

    /// A turn with nothing to report writes no hollow `usage` key at all, and every
    /// event written before telemetry existed still loads.
    #[test]
    fn usage_is_absent_by_default_and_legacy_events_still_load() {
        let plain = Event::new(Actor::Human, None, Kind::Message { body: "hi".into() });
        let line = serde_json::to_string(&plain).unwrap();
        assert!(!line.contains("usage"), "no phantom usage key: {line}");

        // Empty usage attaches nothing rather than an empty object.
        let hollow = Event::new(Actor::Gluon, None, Kind::EnergyReport { used_tokens: 1 })
            .with_usage(crate::Usage::default());
        assert_eq!(hollow.usage, None);

        // A pre-telemetry event on disk.
        let legacy = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"agy","to":null,"kind":"energy_report","used_tokens":42}"#;
        let ev: Event = serde_json::from_str(legacy).unwrap();
        assert_eq!(ev.usage, None);
        assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: 42 });
    }

    /// **The history must not break.** `field.jsonl` is an append-only log, and Jake's
    /// live one holds 70 `energy_report` events written in the OLD shape — a bare
    /// `used_tokens` and no `usage` at all. Widening the event must keep reading every
    /// one of them, or the token split silently eats his history.
    ///
    /// These four lines are copied **verbatim** out of the live
    /// `.hadron/field.jsonl`, not hand-written to match the parser. (They have to be
    /// embedded: `.hadron/` is gitignored, so a test that only read the real file
    /// would silently pass by never running anywhere else — see
    /// [`the_live_field_still_parses`], which does read it, when it is there.)
    #[test]
    fn real_legacy_energy_reports_from_the_live_field_still_parse() {
        let real_lines = [
            r#"{"v":1,"id":"01KXANDETK9A8MBT5YPZKQYQXK","ts":"2026-07-12T07:59:35.251134877Z","from":"opus","to":null,"kind":"energy_report","used_tokens":66}"#,
            r#"{"v":1,"id":"01KXANF0Q8HBFFAT3V5G43DJXZ","ts":"2026-07-12T08:00:26.344776590Z","from":"opus","to":null,"kind":"energy_report","used_tokens":202}"#,
            // The 298k turn that started all of this — the old ACP total, cache included.
            r#"{"v":1,"id":"01KXEKRWQXCMV9CAW3E11WSH3F","ts":"2026-07-13T20:47:50.525410311Z","from":"acp-claude","to":null,"kind":"energy_report","used_tokens":298033}"#,
            r#"{"v":1,"id":"01KXEKMHH7HDRVK54X4JED1JHH","ts":"2026-07-13T20:45:27.975116252Z","from":"opus","to":null,"kind":"energy_report","used_tokens":2318}"#,
        ];

        let want = [66u32, 202, 298_033, 2318];
        for (line, expect) in real_lines.iter().zip(want) {
            let ev: Event = serde_json::from_str(line).expect("a real logged line must still parse");
            assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: expect });
            // The old lines carry no components, and that is honest: we never knew them.
            assert_eq!(ev.usage, None, "a legacy line has no telemetry to invent");
        }
    }

    /// A new-shape event carries the components on the envelope while keeping the
    /// legacy `used_tokens` on the payload — so an old reader (the chamber, which
    /// matches `Kind` exhaustively) still sees a number it understands, and a new one
    /// can tell fresh work from cache traffic.
    #[test]
    fn a_new_energy_report_carries_components_and_stays_readable_by_old_readers() {
        use crate::{TokenSpend, Usage};

        // acp-claude's real turn-1 numbers.
        let spend = TokenSpend {
            input: Some(8),
            output: Some(2_390),
            cache_read: Some(190_454),
            cache_write: Some(44_338),
        };
        let fresh = spend.fresh().unwrap();
        assert_eq!(fresh, 2_398, "the comparable unit: cache is NOT in here");

        let ev = Event::new(
            Actor::Quark(QuarkId::new("acp-claude")),
            None,
            Kind::EnergyReport { used_tokens: fresh },
        )
        .with_usage(Usage { spend: spend.clone(), ..Default::default() });

        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();

        // The legacy payload field is still there, and now it means something
        // comparable to what a CLI quark reports — not a cache-inflated total.
        assert_eq!(back.kind, Kind::EnergyReport { used_tokens: 2_398 });
        // And the cache is carried, not discarded: this is what makes a big number
        // explicable instead of alarming.
        let got = back.usage.expect("components ride on the envelope").spend;
        assert_eq!(got, spend);
        assert_eq!(got.cached(), Some(234_792));
    }

    /// Parse Jake's **actual** log, every line of it, if it is present. This is the
    /// one that would catch a shape we never thought to embed above. It is skipped
    /// (not failed) when `.hadron/` is absent, because it is gitignored and will not
    /// exist in a fresh clone — the embedded test above is what runs there.
    #[test]
    fn the_live_field_still_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.hadron/field.jsonl");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("skipped: no live field at {}", path.display());
            return;
        };

        let mut energy = 0usize;
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let ev: Event = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("live field line {} no longer parses: {e}", n + 1));
            if matches!(ev.kind, Kind::EnergyReport { .. }) {
                energy += 1;
            }
        }
        assert!(energy > 0, "the live field should hold energy reports; found none");
        eprintln!("live field: {} lines, {energy} energy_report", text.lines().count());
    }

    /// The turn id is additive. Every line already in Jake's `field.jsonl` was written
    /// without one, and must still parse — with `turn: None`, which honestly means
    /// "we do not know", not a fabricated link.
    #[test]
    fn legacy_lines_have_no_turn_id_and_still_parse() {
        // Verbatim from the live field.
        let legacy = r#"{"v":1,"id":"01KXEKRWQXCMV9CAW3E11WSH3F","ts":"2026-07-13T20:47:50.525410311Z","from":"acp-claude","to":null,"kind":"energy_report","used_tokens":298033}"#;
        let ev: Event = serde_json::from_str(legacy).unwrap();
        assert_eq!(ev.turn, None, "unknown, not invented");
        assert_eq!(ev.kind, Kind::EnergyReport { used_tokens: 298_033 });

        // And an event with no turn writes no phantom key.
        let plain = Event::new(Actor::Human, None, Kind::Message { body: "hi".into() });
        let line = serde_json::to_string(&plain).unwrap();
        assert!(!line.contains("turn"), "no phantom turn key: {line}");
    }

    /// The join the chamber will actually do: two separate events, one turn.
    #[test]
    fn a_turn_id_links_a_reply_to_its_own_telemetry_across_two_events() {
        use crate::{TokenSpend, Usage};
        let turn = Ulid::new();
        let q = QuarkId::new("acp-claude");

        let reply = Event::new(Actor::Quark(q.clone()), None, Kind::Message { body: "done".into() })
            .with_turn(turn);
        let energy = Event::new(Actor::Quark(q.clone()), None, Kind::EnergyReport { used_tokens: 2_398 })
            .with_usage(Usage {
                spend: TokenSpend { input: Some(8), output: Some(2_390), cache_read: Some(190_454), cache_write: None },
                ..Default::default()
            })
            .with_turn(turn);

        // Through the wire and back — the link has to survive serialization to be worth
        // anything, since the chamber reads the file, not these structs.
        let reply: Event = serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap();
        let energy: Event = serde_json::from_str(&serde_json::to_string(&energy).unwrap()).unwrap();

        assert_eq!(reply.turn, energy.turn);
        assert_ne!(reply.id, energy.id, "distinct events");
        let joined = [&reply, &energy]
            .into_iter()
            .find(|e| e.turn == reply.turn && e.usage.is_some())
            .and_then(|e| e.usage.clone())
            .expect("joined on the turn id, not on adjacency");
        assert_eq!(joined.spend.fresh(), Some(2_398));
    }

    #[test]
    fn unknown_kind_is_preserved_not_crashed() {
        // A future event type today's reader has never seen.
        let line = r#"{"v":2,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"gluon","to":null,"kind":"edit_by_hash","block_hash":"9f86d0","summary":"future"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        match &ev.kind {
            Kind::Unknown { kind, raw } => {
                assert_eq!(kind, "edit_by_hash");
                assert_eq!(raw["block_hash"], "9f86d0");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
        // and it re-serializes without loss of the unknown fields
        let reline = serde_json::to_string(&ev).unwrap();
        assert!(reline.contains(r#""kind":"edit_by_hash""#));
        assert!(reline.contains(r#""block_hash":"9f86d0""#));
    }
}
