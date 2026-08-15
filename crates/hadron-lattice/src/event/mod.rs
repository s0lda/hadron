use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use ulid::Ulid;

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

/// Message severity for system/gluon/quark notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Urgency level for an attention required field event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionUrgency {
    High,
    Blocker,
    Critical,
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
    /// High-priority attention signal requesting human or orchestrator intervention.
    AttentionRequired {
        urgency: AttentionUrgency,
        summary: String,
        action_needed: Option<String>,
    },
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
    /// Name the current session. Appended to the LIVE field by `/rename` (`to: None` —
    /// a session has no per-quark target); because `/clear` archives by copying the
    /// live field before truncating it, this event rides along into the archive for
    /// free. The latest one in a field wins, so renaming twice is not an error.
    SessionName { name: String },
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
    pub usage: Option<crate::Usage>,
    pub turn: Option<Ulid>,
    pub answers: Option<Ulid>,
    pub full: Option<String>,
    /// Optional notification classification (Info, Warning, Error) for system/gluon events.
    pub severity: Option<Severity>,
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
            severity: None,
        }
    }

    /// Attach a severity rating (Info, Warning, Error) to the event.
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
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
        if let Some(severity) = &self.severity {
            m.serialize_entry("severity", severity)?;
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
            Kind::AttentionRequired { urgency, summary, action_needed } => {
                m.serialize_entry("kind", "attention_required")?;
                m.serialize_entry("urgency", urgency)?;
                m.serialize_entry("summary", summary)?;
                if let Some(action) = action_needed {
                    m.serialize_entry("action_needed", action)?;
                }
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
            Kind::SessionName { name } => {
                m.serialize_entry("kind", "session_name")?;
                m.serialize_entry("name", name)?;
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
        let severity: Option<Severity> = match map.remove("severity") {
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
            "attention_required" => {
                let urgency = match map.remove("urgency") {
                    Some(v) => serde_json::from_value(v).map_err(D::Error::custom)?,
                    None => AttentionUrgency::High,
                };
                let summary = take_field(&mut map, "summary")?;
                let action_needed = match map.remove("action_needed") {
                    Some(Value::String(s)) => Some(s),
                    _ => None,
                };
                Kind::AttentionRequired {
                    urgency,
                    summary,
                    action_needed,
                }
            },
            "mode_set" => Kind::ModeSet {
                mode: take_field(&mut map, "mode")?,
            },
            "mode_clear" => Kind::ModeClear,
            "reboot" => Kind::Reboot,
            "session_name" => Kind::SessionName {
                name: take_field(&mut map, "name")?,
            },
            other => Kind::Unknown {
                kind: other.to_string(),
                raw: Value::Object(map.clone()),
            },
        };
        Ok(Event { v, id, ts, from, to, kind, usage, turn, answers, full, severity })
    }
}

#[cfg(test)]
mod tests;
