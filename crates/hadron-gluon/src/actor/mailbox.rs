use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuarkMessage {
    TurnRequest {
        assignment_id: String,
        prompt: String,
    },
    CancelTurn {
        assignment_id: String,
    },
    Ping {
        timestamp_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmEvent {
    QuarkStatusChanged {
        quark: String,
        state: String,
    },
    FieldAppended {
        sequence: u64,
        author: String,
        summary: String,
    },
    TurnCompleted {
        quark: String,
        assignment_id: String,
        success: bool,
    },
}

#[derive(Clone)]
pub struct ActorMailbox {
    pub quark_id: String,
    pub sender: mpsc::Sender<QuarkMessage>,
}
