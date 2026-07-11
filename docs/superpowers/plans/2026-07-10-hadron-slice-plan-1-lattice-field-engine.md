# Hadron Slice — Plan 1: Lattice Schema + Field Engine Core

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the headless coordination heart of Hadron — the shared `field.jsonl` bus schema and the gluon's sequential excite loop — provable end-to-end with a `MockQuark`, with zero API spend and zero GPUI.

**Architecture:** Two crates. `hadron-lattice` holds the pure-data protocol (the `Event`/`Kind` schema plus the `Projection`/`TurnOutcome`/roster types). `hadron-gluon` holds the engine: append/read the JSONL field, route the next addressee, and run a sequential turn loop that quiesces when no work remains and trips a backstop on runaway ping-pong. The `Quark` trait and a `MockQuark` let us test the whole loop deterministically.

**Tech Stack:** Rust (edition 2021), serde + serde_json, ulid, chrono, tokio, async-trait, anyhow. Dev: tempfile.

**This is Plan 1 of 4** for the Hadron vertical slice (spec: `docs/superpowers/specs/2026-07-10-hadron-vertical-slice-design.md`). Later plans add git safety + nucleus (2), real Claude/Antigravity adapters (3), and the GPUI chamber (4).

## Global Constraints

- **Rust edition:** `2021`. Use latest stable Rust.
- **Workspace layout:** crates live under `crates/`; `resolver = "2"`.
- **Field is append-only, never rewritten.** Every writer only appends whole lines. History is immutable.
- **Readers must tolerate unknown `kind` values** — never crash on an event kind this version doesn't know; preserve it verbatim as `Kind::Unknown`.
- **Sequential execution:** exactly one quark runs at a time (v1 has no locking; concurrency is deferred to the edit-by-hash pillar). The engine never excites two quarks concurrently.
- **Event `body` fields carry Markdown.** Models speak Markdown; the JSONL envelope is Hadron's transport.
- **Vocabulary (use these exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite.
- **Reserved actor names:** `human` and `gluon` are reserved; a quark id must not be either.
- **Local-only until now.** Execution begins by running `git init` (Task 1, Step 1). Commit after every task.
- **Future dependency (not this plan):** the chamber (Plan 4) pins `gpui = "0.2"` (crates.io, Apache-2.0). No GPUI in Plan 1.

---

### Task 1: Workspace + lattice roster types

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/hadron-lattice/Cargo.toml`
- Create: `crates/hadron-lattice/src/lib.rs`
- Create: `crates/hadron-lattice/src/quark.rs`

**Interfaces:**
- Produces: `QuarkId(pub String)` with `::new`, `::as_str`; `enum Flavor { Orchestrator, Worker }`; `enum EnergyState { Available, Depleted, Unknown }`; `struct QuarkCard { id: QuarkId, flavor: Flavor, energy: EnergyState }`. All serde-derived.

- [ ] **Step 1: Initialize the repo and workspace**

Run:
```bash
cd /home/Jake/dev/hadron
git init
```
Create `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/hadron-lattice"]
```
(Later plans add `crates/hadron-gluon` and `crates/hadron-chamber` to `members`.)

- [ ] **Step 2: Create the lattice crate manifest**

Create `crates/hadron-lattice/Cargo.toml`:
```toml
[package]
name = "hadron-lattice"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ulid = { version = "1", features = ["serde"] }
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 3: Write the failing test**

Create `crates/hadron-lattice/src/quark.rs`:
```rust
use serde::{Deserialize, Serialize};

/// Stable identifier for a quark (agent), e.g. "claude", "agy".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuarkId(pub String);

impl QuarkId {
    pub fn new(s: impl Into<String>) -> Self {
        QuarkId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A quark's role in the studio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    Orchestrator,
    Worker,
}

/// Coarse availability of a quark's budget/quota. v1 seam: always `Available`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnergyState {
    Available,
    Depleted,
    Unknown,
}

/// A roster entry shown to the orchestrator so it can assign work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarkCard {
    pub id: QuarkId,
    pub flavor: Flavor,
    pub energy: EnergyState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quark_card_round_trips() {
        let card = QuarkCard {
            id: QuarkId::new("claude"),
            flavor: Flavor::Orchestrator,
            energy: EnergyState::Available,
        };
        let json = serde_json::to_string(&card).unwrap();
        assert_eq!(
            json,
            r#"{"id":"claude","flavor":"orchestrator","energy":"available"}"#
        );
        let back: QuarkCard = serde_json::from_str(&json).unwrap();
        assert_eq!(card, back);
    }
}
```

Create `crates/hadron-lattice/src/lib.rs`:
```rust
mod quark;

pub use quark::*;
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p hadron-lattice`
Expected: FAIL — the crate does not compile yet only if a typo exists; if it compiles, the test should PASS. (This task's test is the smoke test for the scaffold; proceed once it passes.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p hadron-lattice quark_card_round_trips`
Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/hadron-lattice
git commit -m "feat(lattice): workspace + roster types (QuarkId, Flavor, EnergyState, QuarkCard)"
```

---

### Task 2: Actor + QuarkState with string serde

**Files:**
- Create: `crates/hadron-lattice/src/event.rs`
- Modify: `crates/hadron-lattice/src/lib.rs` (add `mod event; pub use event::*;`)

**Interfaces:**
- Consumes: `QuarkId` (Task 1).
- Produces: `enum Actor { Human, Gluon, Quark(QuarkId) }` serializing to a bare string (`"human"`, `"gluon"`, or the quark id); `enum QuarkState { Ground, Excited, Thinking, Waiting, Blocked, Error }` serializing snake_case.

- [ ] **Step 1: Write the failing test**

Create `crates/hadron-lattice/src/event.rs`:
```rust
use serde::{Deserialize, Serialize};

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
```

Update `crates/hadron-lattice/src/lib.rs`:
```rust
mod event;
mod quark;

pub use event::*;
pub use quark::*;
```

- [ ] **Step 2: Run test to verify it fails, then passes**

Run: `cargo test -p hadron-lattice event::`
Expected: PASS (three tests). If it fails to compile, fix the reported line before continuing.

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-lattice/src
git commit -m "feat(lattice): Actor (bare-string serde) + QuarkState"
```

---

### Task 3: Event + Kind with forward-compatible serde

This is the load-bearing schema task. `Kind` is an enum whose known variants flatten into the event envelope (`{"kind":"message","body":"…"}`) and whose **unknown** kinds are preserved verbatim as `Kind::Unknown`, so a future writer's new event type never crashes today's reader.

**Files:**
- Modify: `crates/hadron-lattice/src/event.rs` (append `Kind`, `Event`, their serde impls, `Event::new`)

**Interfaces:**
- Consumes: `Actor`, `QuarkState`, `QuarkId` (Tasks 1–2).
- Produces: `enum Kind { Message{body}, Status{state}, Edit{paths,git,summary}, Command{cmd,exit,out_summary}, Snapshot{git,label}, Unknown{kind,raw} }`; `struct Event { v:u32, id:Ulid, ts:DateTime<Utc>, from:Actor, to:Option<QuarkId>, kind:Kind }`; `Event::new(from, to, kind) -> Event` (stamps `v=1`, fresh `id`, `ts=now`).

- [ ] **Step 1: Write the failing test**

Append to `crates/hadron-lattice/src/event.rs` (add imports at top of file: `use chrono::{DateTime, Utc}; use serde_json::Value; use ulid::Ulid;`):
```rust
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
            other => Kind::Unknown {
                kind: other.to_string(),
                raw: Value::Object(map.clone()),
            },
        };
        Ok(Event { v, id, ts, from, to, kind })
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
        assert!(line.contains(r#""body":"# Build auth""#));
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
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p hadron-lattice event_tests::`
Expected: PASS (three tests). Watch specifically for `unknown_kind_is_preserved_not_crashed` — it proves the forward-compat rule.

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-lattice/src/event.rs
git commit -m "feat(lattice): Event + Kind with forward-compatible serde (Unknown preserved)"
```

---

### Task 4: Projection, TurnOutcome, and the roster/window types

**Files:**
- Create: `crates/hadron-lattice/src/projection.rs`
- Modify: `crates/hadron-lattice/src/lib.rs` (add `mod projection; pub use projection::*;`)

**Interfaces:**
- Consumes: `Event`, `QuarkCard` (Tasks 1, 3).
- Produces: `struct Projection { task, invariants, nucleus_digest, roster: Vec<QuarkCard>, field_window: Vec<Event>, git_diff }`; `struct TurnOutcome { message: Option<String> }`.

- [ ] **Step 1: Write the failing test**

Create `crates/hadron-lattice/src/projection.rs`:
```rust
use serde::{Deserialize, Serialize};

use crate::{Event, QuarkCard};

/// The curated context handed to a quark on excitation. The single chokepoint
/// where cost-control (what context), invariants (methodology), nucleus (project
/// SSOT), and roster (who to delegate to) converge.
// `PartialEq` but not `Eq` — contains `Vec<Event>`, and `Event` is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// The assignment this quark is being asked to act on.
    pub task: String,
    /// Enforced working protocol, injected as a Markdown preamble. v1: static.
    pub invariants: String,
    /// Relevant slice of the project SSOT (nucleus). v1: may be empty.
    pub nucleus_digest: String,
    /// Who exists, their flavor and energy — enables orchestration.
    pub roster: Vec<QuarkCard>,
    /// Recent relevant events. v1: a dumb recent window.
    pub field_window: Vec<Event>,
    /// Current working diff, not whole files. v1: may be empty.
    pub git_diff: String,
}

/// What an adapter returns after a turn. File mutations are NOT reported here —
/// the gluon derives them from git diff (Plan 2). A `None` message means the
/// quark produced no field message this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TurnOutcome {
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, EnergyState, Flavor, Kind, QuarkId};

    #[test]
    fn projection_holds_events_and_roster() {
        let proj = Projection {
            task: "Build auth".into(),
            invariants: "Snapshot before editing.".into(),
            nucleus_digest: String::new(),
            roster: vec![QuarkCard {
                id: QuarkId::new("agy"),
                flavor: Flavor::Worker,
                energy: EnergyState::Available,
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "go".into() },
            )],
            git_diff: String::new(),
        };
        assert_eq!(proj.roster.len(), 1);
        assert_eq!(proj.field_window.len(), 1);
    }

    #[test]
    fn turn_outcome_default_is_empty() {
        assert_eq!(TurnOutcome::default(), TurnOutcome { message: None });
    }
}
```

Update `crates/hadron-lattice/src/lib.rs`:
```rust
mod event;
mod projection;
mod quark;

pub use event::*;
pub use projection::*;
pub use quark::*;
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p hadron-lattice projection`
Expected: PASS (two tests).

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-lattice/src
git commit -m "feat(lattice): Projection + TurnOutcome"
```

---

### Task 5: gluon crate + field append/read

**Files:**
- Create: `crates/hadron-gluon/Cargo.toml`
- Create: `crates/hadron-gluon/src/lib.rs`
- Create: `crates/hadron-gluon/src/field.rs`
- Modify: `Cargo.toml` (workspace members: add `crates/hadron-gluon`)

**Interfaces:**
- Consumes: `hadron_lattice::Event`.
- Produces: `field::append_event(path: &Path, event: &Event) -> std::io::Result<()>`; `field::read_events(path: &Path) -> std::io::Result<Vec<Event>>`.

- [ ] **Step 1: Add the crate to the workspace**

Update root `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/hadron-lattice", "crates/hadron-gluon"]
```
Create `crates/hadron-gluon/Cargo.toml`:
```toml
[package]
name = "hadron-gluon"
version = "0.1.0"
edition = "2021"

[dependencies]
hadron-lattice = { path = "../hadron-lattice" }
serde_json = "1"
anyhow = "1"
async-trait = "0.1"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the failing test**

Create `crates/hadron-gluon/src/field.rs`:
```rust
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use hadron_lattice::Event;

/// Append a single event as one JSON line. Line-atomic; creates the file if
/// missing. Never rewrites existing content.
pub fn append_event(path: &Path, event: &Event) -> std::io::Result<()> {
    let line = serde_json::to_string(event)?;
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Read every event in order. A missing file yields an empty vec. Blank lines
/// are skipped. A line that fails to parse is skipped rather than crashing the
/// reader (append-only integrity means a torn final line can be ignored).
pub fn read_events(path: &Path) -> std::io::Result<Vec<Event>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(ev) => out.push(ev),
            Err(_) => continue,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{Actor, Kind, QuarkId, QuarkState};
    use tempfile::tempdir;

    #[test]
    fn append_then_read_preserves_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");

        let e1 = Event::new(Actor::Human, Some(QuarkId::new("claude")), Kind::Message { body: "one".into() });
        let e2 = Event::new(Actor::Quark(QuarkId::new("claude")), None, Kind::Status { state: QuarkState::Ground });
        append_event(&path, &e1).unwrap();
        append_event(&path, &e2).unwrap();

        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], e1);
        assert_eq!(events[1], e2);
    }

    #[test]
    fn missing_file_reads_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.jsonl");
        assert_eq!(read_events(&path).unwrap().len(), 0);
    }

    #[test]
    fn unknown_kind_line_survives_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        std::fs::write(
            &path,
            "{\"v\":2,\"id\":\"01ARZ3NDEKTSV4RRFFQ69G5FAV\",\"ts\":\"2026-07-10T14:00:00Z\",\"from\":\"gluon\",\"to\":null,\"kind\":\"future_thing\",\"x\":1}\n",
        )
        .unwrap();
        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, hadron_lattice::Kind::Unknown { .. }));
    }

    #[test]
    fn torn_line_is_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        std::fs::write(&path, "{not valid json\n").unwrap();
        assert_eq!(read_events(&path).unwrap().len(), 0);
    }
}
```

Create `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod field;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p hadron-gluon field::`
Expected: PASS (four tests).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/hadron-gluon
git commit -m "feat(gluon): append-only field IO with unknown-kind tolerance"
```

---

### Task 6: Quark trait, MockQuark, and routing

**Files:**
- Create: `crates/hadron-gluon/src/quark.rs`
- Create: `crates/hadron-gluon/src/mock.rs`
- Create: `crates/hadron-gluon/src/router.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`

**Interfaces:**
- Consumes: `hadron_lattice::{Projection, TurnOutcome, QuarkId, Flavor, EnergyState, Event, Kind, Actor, QuarkCard}`.
- Produces:
  - `trait Quark: Send { fn id(&self)->QuarkId; fn flavor(&self)->Flavor; fn energy(&self)->EnergyState; async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>; }` (via `#[async_trait]`).
  - `struct MockQuark` with `MockQuark::scripted(id, flavor, Vec<Option<String>>)` and `MockQuark::repeating(id, flavor, impl Into<String>)`.
  - `router::next_pending(events: &[Event]) -> Option<QuarkId>`.
  - `router::parse_addressee(body: &str, roster: &[QuarkCard]) -> Option<QuarkId>`.
  - `router::current_task(events: &[Event], target: &QuarkId) -> String`.

- [ ] **Step 1: Write the Quark trait and MockQuark**

Create `crates/hadron-gluon/src/quark.rs`:
```rust
use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

/// A citizen of the field. The gluon never knows whether this is a CLI harness,
/// a native API worker, or a future ACP/MCP adapter — only this contract.
#[async_trait]
pub trait Quark: Send {
    fn id(&self) -> QuarkId;
    fn flavor(&self) -> Flavor;
    fn energy(&self) -> EnergyState;
    /// Run one turn against a projection and return the field message (if any).
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome>;
}
```

Create `crates/hadron-gluon/src/mock.rs`:
```rust
use std::collections::VecDeque;

use async_trait::async_trait;
use hadron_lattice::{EnergyState, Flavor, Projection, QuarkId, TurnOutcome};

use crate::quark::Quark;

/// A deterministic quark for tests. Emits scripted messages in order; once the
/// script is exhausted it emits `repeating` (or `None`) on every further turn.
pub struct MockQuark {
    id: QuarkId,
    flavor: Flavor,
    scripted: VecDeque<Option<String>>,
    repeating: Option<String>,
}

impl MockQuark {
    /// Emit each queued message once, in order (`None` = a silent turn).
    pub fn scripted(id: QuarkId, flavor: Flavor, messages: Vec<Option<String>>) -> Self {
        MockQuark {
            id,
            flavor,
            scripted: messages.into_iter().collect(),
            repeating: None,
        }
    }

    /// Emit the same message on every turn forever (drives backstop tests).
    pub fn repeating(id: QuarkId, flavor: Flavor, message: impl Into<String>) -> Self {
        MockQuark {
            id,
            flavor,
            scripted: VecDeque::new(),
            repeating: Some(message.into()),
        }
    }
}

#[async_trait]
impl Quark for MockQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, _turn: Projection) -> anyhow::Result<TurnOutcome> {
        let message = self
            .scripted
            .pop_front()
            .unwrap_or_else(|| self.repeating.clone());
        Ok(TurnOutcome { message })
    }
}
```

- [ ] **Step 2: Write the failing routing test**

Create `crates/hadron-gluon/src/router.rs`:
```rust
use hadron_lattice::{Actor, Event, Kind, QuarkCard, QuarkId};

/// Which quark should be excited next.
///
/// v1 rule (stateless, reconstructed from the field): find the most recent event
/// that addresses a quark (`to = Some(q)`). If `q` has authored any event since,
/// that turn is already handled → quiesce (`None`). Otherwise `q` is pending.
pub fn next_pending(events: &[Event]) -> Option<QuarkId> {
    let idx = events.iter().rposition(|e| e.to.is_some())?;
    let target = events[idx].to.clone().unwrap();
    let answered = events[idx + 1..]
        .iter()
        .any(|e| e.from == Actor::Quark(target.clone()));
    if answered {
        None
    } else {
        Some(target)
    }
}

/// Extract the addressee from a Markdown message: the first `@quarkid` mention
/// whose id is on the roster. Returns `None` (hand back to human) if none match.
pub fn parse_addressee(body: &str, roster: &[QuarkCard]) -> Option<QuarkId> {
    for word in body.split_whitespace() {
        let Some(rest) = word.strip_prefix('@') else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        if let Some(card) = roster.iter().find(|c| c.id.as_str() == name) {
            return Some(card.id.clone());
        }
    }
    None
}

/// The most recent Message addressed to `target` — what it was last asked to do.
pub fn current_task(events: &[Event], target: &QuarkId) -> String {
    events
        .iter()
        .rev()
        .find_map(|e| match (&e.to, &e.kind) {
            (Some(to), Kind::Message { body }) if to == target => Some(body.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::{EnergyState, Flavor};

    fn msg(from: Actor, to: Option<&str>, body: &str) -> Event {
        Event::new(from, to.map(QuarkId::new), Kind::Message { body: body.into() })
    }

    fn roster() -> Vec<QuarkCard> {
        vec![
            QuarkCard { id: QuarkId::new("orch"), flavor: Flavor::Orchestrator, energy: EnergyState::Available },
            QuarkCard { id: QuarkId::new("worker"), flavor: Flavor::Worker, energy: EnergyState::Available },
        ]
    }

    #[test]
    fn pending_is_unanswered_addressee() {
        let events = vec![msg(Actor::Human, Some("orch"), "go")];
        assert_eq!(next_pending(&events), Some(QuarkId::new("orch")));
    }

    #[test]
    fn answered_addressee_quiesces() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), None, "done, back to you"),
        ];
        assert_eq!(next_pending(&events), None);
    }

    #[test]
    fn handoff_routes_to_next_quark() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "go"),
            msg(Actor::Quark(QuarkId::new("orch")), Some("worker"), "@worker do the UI"),
        ];
        assert_eq!(next_pending(&events), Some(QuarkId::new("worker")));
    }

    #[test]
    fn parse_addressee_finds_mention() {
        assert_eq!(
            parse_addressee("Sure, @worker please handle it.", &roster()),
            Some(QuarkId::new("worker"))
        );
        assert_eq!(parse_addressee("no mention here", &roster()), None);
        assert_eq!(parse_addressee("@ghost unknown", &roster()), None);
    }

    #[test]
    fn current_task_is_last_message_to_target() {
        let events = vec![
            msg(Actor::Human, Some("orch"), "first"),
            msg(Actor::Quark(QuarkId::new("orch")), Some("worker"), "@worker second"),
        ];
        assert_eq!(current_task(&events, &QuarkId::new("worker")), "@worker second");
    }
}
```

Update `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod field;
pub mod mock;
pub mod quark;
pub mod router;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p hadron-gluon router::`
Expected: PASS (five tests). Also run `cargo build -p hadron-gluon` to confirm `quark.rs` and `mock.rs` compile.

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-gluon/src
git commit -m "feat(gluon): Quark trait, MockQuark, and field routing"
```

---

### Task 7: The engine — sequential excite loop with quiesce + backstop

**Files:**
- Create: `crates/hadron-gluon/src/engine.rs`
- Modify: `crates/hadron-gluon/src/lib.rs` (add `pub mod engine;`)

**Interfaces:**
- Consumes: `field::{append_event, read_events}`, `router::{next_pending, parse_addressee, current_task}`, `quark::Quark`, `hadron_lattice::*`.
- Produces: `struct Engine`; `Engine::new(field_path: PathBuf, quarks: Vec<Box<dyn Quark>>, invariants: String, max_exchanges: usize) -> Engine`; `async Engine::run_until_quiesce(&mut self) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the engine**

Create `crates/hadron-gluon/src/engine.rs`:
```rust
use std::collections::HashMap;
use std::path::PathBuf;

use hadron_lattice::{Actor, Event, Kind, Projection, QuarkCard, QuarkId, QuarkState};

use crate::field::{append_event, read_events};
use crate::quark::Quark;
use crate::router::{current_task, next_pending, parse_addressee};

/// Drives the sequential coordination loop over a single field file.
pub struct Engine {
    field_path: PathBuf,
    quarks: HashMap<QuarkId, Box<dyn Quark>>,
    roster: Vec<QuarkCard>,
    invariants: String,
    max_exchanges: usize,
}

impl Engine {
    pub fn new(
        field_path: PathBuf,
        quarks: Vec<Box<dyn Quark>>,
        invariants: String,
        max_exchanges: usize,
    ) -> Self {
        let roster = quarks
            .iter()
            .map(|q| QuarkCard {
                id: q.id(),
                flavor: q.flavor(),
                energy: q.energy(),
            })
            .collect();
        let quarks = quarks.into_iter().map(|q| (q.id(), q)).collect();
        Engine {
            field_path,
            quarks,
            roster,
            invariants,
            max_exchanges,
        }
    }

    /// Excite quarks one at a time until no addressee is pending (quiesce) or the
    /// per-human-turn exchange budget is exhausted (backstop).
    pub async fn run_until_quiesce(&mut self) -> anyhow::Result<()> {
        let mut exchanges = 0usize;
        loop {
            let events = read_events(&self.field_path)?;

            let target = match next_pending(&events) {
                Some(q) => q,
                None => return Ok(()), // quiesce: control returns to the human
            };

            if exchanges >= self.max_exchanges {
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Gluon,
                        None,
                        Kind::Message {
                            body: format!(
                                "⚠️ backstop reached ({} exchanges); returning control to the human.",
                                self.max_exchanges
                            ),
                        },
                    ),
                )?;
                return Ok(());
            }

            let projection = Projection {
                task: current_task(&events, &target),
                invariants: self.invariants.clone(),
                nucleus_digest: String::new(),
                roster: self.roster.clone(),
                field_window: events.clone(),
                git_diff: String::new(),
            };

            let quark = self
                .quarks
                .get_mut(&target)
                .ok_or_else(|| anyhow::anyhow!("no such quark on roster: {}", target.as_str()))?;
            let outcome = quark.excite(projection).await?;

            if let Some(body) = outcome.message {
                let to = parse_addressee(&body, &self.roster);
                append_event(
                    &self.field_path,
                    &Event::new(Actor::Quark(target.clone()), to, Kind::Message { body }),
                )?;
            }
            append_event(
                &self.field_path,
                &Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Ground },
                ),
            )?;

            exchanges += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::append_event;
    use crate::mock::MockQuark;
    use hadron_lattice::{Actor, Flavor, Kind, QuarkId};
    use tempfile::tempdir;

    fn seed_human_message(path: &std::path::Path, to: &str, body: &str) {
        append_event(
            path,
            &Event::new(
                Actor::Human,
                Some(QuarkId::new(to)),
                Kind::Message { body: body.into() },
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn orchestrated_handoff_runs_then_quiesces() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        seed_human_message(&path, "orch", "Build the thing. @worker will help.");

        let orch = MockQuark::scripted(
            QuarkId::new("orch"),
            Flavor::Orchestrator,
            vec![
                Some("Starting. @worker please build the UI.".into()),
                Some("All done. Handing back to the human.".into()),
            ],
        );
        let worker = MockQuark::scripted(
            QuarkId::new("worker"),
            Flavor::Worker,
            vec![Some("UI complete. @orch back to you.".into())],
        );

        let mut engine = Engine::new(
            path.clone(),
            vec![Box::new(orch), Box::new(worker)],
            "Coordinate via @mentions.".into(),
            10,
        );
        engine.run_until_quiesce().await.unwrap();

        let events = read_events(&path).unwrap();
        let messages: Vec<&str> = events
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::Message { body } => Some(body.as_str()),
                _ => None,
            })
            .collect();
        // human, orch->worker, worker->orch, orch->human (handback)
        assert_eq!(messages.len(), 4);
        assert!(messages[1].contains("@worker"));
        assert!(messages[2].contains("@orch"));
        assert!(messages[3].contains("Handing back"));
        // Quiesced cleanly: no backstop message.
        assert!(!messages.iter().any(|m| m.contains("backstop")));
    }

    #[tokio::test]
    async fn runaway_pingpong_trips_backstop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("field.jsonl");
        seed_human_message(&path, "orch", "start");

        // Both quarks address each other forever.
        let orch = MockQuark::repeating(QuarkId::new("orch"), Flavor::Orchestrator, "@worker go");
        let worker = MockQuark::repeating(QuarkId::new("worker"), Flavor::Worker, "@orch go");

        let mut engine = Engine::new(
            path.clone(),
            vec![Box::new(orch), Box::new(worker)],
            "x".into(),
            4,
        );
        engine.run_until_quiesce().await.unwrap();

        let events = read_events(&path).unwrap();
        let backstops = events
            .iter()
            .filter(|e| matches!(&e.kind, Kind::Message { body } if body.contains("backstop")))
            .count();
        assert_eq!(backstops, 1, "exactly one backstop message should be appended");
        // The loop bounded the number of quark turns.
        let ground_statuses = events
            .iter()
            .filter(|e| matches!(e.kind, Kind::Status { state: QuarkState::Ground }))
            .count();
        assert_eq!(ground_statuses, 4, "exactly max_exchanges turns ran");
    }
}
```

Update `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod engine;
pub mod field;
pub mod mock;
pub mod quark;
pub mod router;
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p hadron-gluon engine::`
Expected: PASS (two tests) — `orchestrated_handoff_runs_then_quiesces` proves the coordination loop + clean quiesce; `runaway_pingpong_trips_backstop` proves the runaway guard.

- [ ] **Step 3: Run the whole workspace green**

Run: `cargo test`
Expected: PASS — all lattice + gluon tests (roster, actor, event/forward-compat, projection, field IO, routing, engine).

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-gluon/src
git commit -m "feat(gluon): sequential excite engine with quiesce + backstop"
```

---

## Plan 1 Definition of Done

- `cargo test` passes across the workspace.
- A `MockQuark`-driven orchestrated handoff runs multiple sequential turns through a real `field.jsonl` file and quiesces cleanly.
- A runaway ping-pong is bounded by the backstop.
- The field schema round-trips and **preserves unknown event kinds** (forward-compat proven).
- No git safety, no nucleus, no real adapters, no GPUI yet — those are Plans 2–4.

## Notes for the plan author (carried into later plans)

- **Task/`current_task`:** v1 sets `Projection.task` to the last Message addressed to the target. Good enough for the slice; the nucleus digest (Plan 2) will enrich it.
- **Addressing convention:** quarks delegate via a leading `@quarkid` mention parsed by `router::parse_addressee`. The Invariants preamble (Plan 3) must instruct real quarks to use it. This is the v1 stand-in for structured routing.
- **`Actor` reserved-name collision:** a quark literally named `human`/`gluon` would mis-parse. Enforce at quark-registration time in Plan 3.
- **Engine re-reads the whole field each turn.** Fine for v1 (files are small, turns are slow). The projection-window seam (spec §13) is where this becomes a bounded read later.
- **The `field` module is the remote-control seam (spec §15).** All field access goes through `field::append_event` / `field::read_events` — nothing else touches the file directly. Keep it that way: a future network transport (the gluon exposing read/append over a socket or HTTP/WS for remote clients) swaps in behind this boundary with no change to the engine, router, or schema.
