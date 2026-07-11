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
/// against the human's god-mode policy by `hadron_gatekeeper::decide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Writing, editing, or deleting files inside the workspace.
    WorkspaceEdit,
    /// Executing a shell command (includes publish-class ops like `cargo publish`).
    BashExec,
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
    PermissionGrant { approved: bool },
    /// Any kind this version does not understand. `raw` holds the full set of
    /// non-envelope fields so the event can be re-serialized and displayed.
    Unknown { kind: String, raw: Value },
}

/// One line in the field. The envelope (`v/id/ts/from/to`) plus a flattened kind.
/// `PartialEq` but not `Eq` (contains `Kind`, which is not `Eq`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub v: u32,
    pub id: Ulid,
    pub ts: DateTime<Utc>,
    pub from: Actor,
    pub to: Option<QuarkId>,
    pub kind: Kind,
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
        }
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
            Kind::PermissionGrant { approved } => {
                m.serialize_entry("kind", "permission_grant")?;
                m.serialize_entry("approved", approved)?;
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
            },
            other => Kind::Unknown {
                kind: other.to_string(),
                raw: Value::Object(map.clone()),
            },
        };
        Ok(Event { v, id, ts, from, to, kind })
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
            Kind::PermissionGrant { approved: true },
        );
        let line = serde_json::to_string(&ev).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(ev, back);
        assert!(line.contains(r#""kind":"permission_grant""#));
        assert!(line.contains(r#""approved":true"#));
    }

    #[test]
    fn null_to_deserializes_as_none() {
        let line = r#"{"v":1,"id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","ts":"2026-07-10T14:00:00Z","from":"claude","to":null,"kind":"status","state":"ground"}"#;
        let ev: Event = serde_json::from_str(line).unwrap();
        assert_eq!(ev.to, None);
        assert_eq!(ev.kind, Kind::Status { state: QuarkState::Ground });
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
