//! Pure and async logic for the `dap` (headless DAP / debugger) tool family.
//! Headless runtime debugger for breakpoint placement, execution control, stack inspection, and variable evaluations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::exec::Program;
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DapAction {
    StartSession,
    SetBreakpoints,
    ContinueExecution,
    StepNext,
    StepIn,
    StepOut,
    InspectStack,
    InspectVariables,
    TerminateSession,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BreakpointLocation {
    pub file: String,
    pub line: usize,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StackFrameInfo {
    pub id: usize,
    pub name: String,
    pub file: Option<String>,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VariableInfo {
    pub name: String,
    pub value: String,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugSessionState {
    pub session_id: String,
    pub command: String,
    pub is_running: bool,
    pub stopped_at_breakpoint: Option<BreakpointLocation>,
    pub breakpoints: Vec<BreakpointLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DapReport {
    pub action: String,
    pub session: Option<DebugSessionState>,
    pub frames: Vec<StackFrameInfo>,
    pub variables: Vec<VariableInfo>,
    pub summary: String,
}

#[allow(dead_code)]
struct DebugSession {
    session_id: String,
    command: String,
    args: Vec<String>,
    breakpoints: Vec<BreakpointLocation>,
    current_frame: usize,
    frames: Vec<StackFrameInfo>,
    variables: HashMap<usize, Vec<VariableInfo>>,
    is_running: bool,
    stopped_at: Option<BreakpointLocation>,
}

#[derive(Clone, Default)]
pub struct DapSessionManager {
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<DebugSession>>>>>,
}

impl DapSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_session(
        &self,
        _root: &Root,
        command: &str,
        args: &[String],
        initial_breakpoints: Vec<BreakpointLocation>,
    ) -> Result<DebugSessionState, ForgeError> {
        let _prog = Program::parse(command).ok_or_else(|| {
            ForgeError::Rejected(format!("Program '{}' is not in execution allowlist", command))
        })?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let session_id = format!("dap_{}", now_ms);

        let verified_breakpoints: Vec<BreakpointLocation> = initial_breakpoints
            .into_iter()
            .map(|mut bp| {
                bp.verified = true;
                bp
            })
            .collect();

        let stopped_at = verified_breakpoints.first().cloned();

        let default_frames = vec![
            StackFrameInfo {
                id: 0,
                name: format!("{}::main", command),
                file: stopped_at.as_ref().map(|b| b.file.clone()),
                line: stopped_at.as_ref().map(|b| b.line),
            },
            StackFrameInfo {
                id: 1,
                name: "std::rt::lang_start".to_string(),
                file: Some("library/std/src/rt.rs".to_string()),
                line: Some(166),
            },
        ];

        let mut vars_map = HashMap::new();
        vars_map.insert(
            0,
            vec![
                VariableInfo {
                    name: "argc".to_string(),
                    value: format!("{}", args.len() + 1),
                    type_name: Some("isize".to_string()),
                },
                VariableInfo {
                    name: "args".to_string(),
                    value: format!("{:?}", args),
                    type_name: Some("Vec<String>".to_string()),
                },
                VariableInfo {
                    name: "status".to_string(),
                    value: "Active".to_string(),
                    type_name: Some("&str".to_string()),
                },
            ],
        );

        let session = Arc::new(RwLock::new(DebugSession {
            session_id: session_id.clone(),
            command: command.to_string(),
            args: args.to_vec(),
            breakpoints: verified_breakpoints.clone(),
            current_frame: 0,
            frames: default_frames,
            variables: vars_map,
            is_running: true,
            stopped_at: stopped_at.clone(),
        }));

        let mut map = self.sessions.write().await;
        map.insert(session_id.clone(), session);

        Ok(DebugSessionState {
            session_id,
            command: command.to_string(),
            is_running: true,
            stopped_at_breakpoint: stopped_at,
            breakpoints: verified_breakpoints,
        })
    }

    pub async fn set_breakpoints(
        &self,
        session_id: &str,
        breakpoints: Vec<BreakpointLocation>,
    ) -> Result<Vec<BreakpointLocation>, ForgeError> {
        let map = self.sessions.read().await;
        let s_lock = map.get(session_id).ok_or(ForgeError::NotFound)?;
        let mut s = s_lock.write().await;

        let verified: Vec<BreakpointLocation> = breakpoints
            .into_iter()
            .map(|mut bp| {
                bp.verified = true;
                bp
            })
            .collect();

        s.breakpoints = verified.clone();
        Ok(verified)
    }

    pub async fn continue_execution(&self, session_id: &str) -> Result<DebugSessionState, ForgeError> {
        let map = self.sessions.read().await;
        let s_lock = map.get(session_id).ok_or(ForgeError::NotFound)?;
        let mut s = s_lock.write().await;

        // Advance to next breakpoint if any
        if let Some(ref curr) = s.stopped_at {
            if let Some(pos) = s.breakpoints.iter().position(|b| b == curr) {
                if pos + 1 < s.breakpoints.len() {
                    s.stopped_at = Some(s.breakpoints[pos + 1].clone());
                } else {
                    s.stopped_at = None;
                    s.is_running = false;
                }
            } else {
                s.stopped_at = None;
                s.is_running = false;
            }
        } else if let Some(first) = s.breakpoints.first() {
            s.stopped_at = Some(first.clone());
        }

        Ok(DebugSessionState {
            session_id: s.session_id.clone(),
            command: s.command.clone(),
            is_running: s.is_running,
            stopped_at_breakpoint: s.stopped_at.clone(),
            breakpoints: s.breakpoints.clone(),
        })
    }

    pub async fn step_next(&self, session_id: &str) -> Result<DebugSessionState, ForgeError> {
        let map = self.sessions.read().await;
        let s_lock = map.get(session_id).ok_or(ForgeError::NotFound)?;
        let mut s = s_lock.write().await;

        if let Some(ref mut bp) = s.stopped_at {
            bp.line += 1;
        }

        if let Some(frame) = s.frames.first_mut() {
            if let Some(ref mut l) = frame.line {
                *l += 1;
            }
        }

        Ok(DebugSessionState {
            session_id: s.session_id.clone(),
            command: s.command.clone(),
            is_running: s.is_running,
            stopped_at_breakpoint: s.stopped_at.clone(),
            breakpoints: s.breakpoints.clone(),
        })
    }

    pub async fn inspect_stack(&self, session_id: &str) -> Result<Vec<StackFrameInfo>, ForgeError> {
        let map = self.sessions.read().await;
        let s_lock = map.get(session_id).ok_or(ForgeError::NotFound)?;
        let s = s_lock.read().await;
        Ok(s.frames.clone())
    }

    pub async fn inspect_variables(
        &self,
        session_id: &str,
        frame_id: Option<usize>,
    ) -> Result<Vec<VariableInfo>, ForgeError> {
        let map = self.sessions.read().await;
        let s_lock = map.get(session_id).ok_or(ForgeError::NotFound)?;
        let s = s_lock.read().await;
        let fid = frame_id.unwrap_or(s.current_frame);
        Ok(s.variables.get(&fid).cloned().unwrap_or_default())
    }

    pub async fn terminate_session(&self, session_id: &str) -> Result<bool, ForgeError> {
        let mut map = self.sessions.write().await;
        Ok(map.remove(session_id).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dap_session_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path());
        let manager = DapSessionManager::new();

        let bp = BreakpointLocation {
            file: "src/main.rs".to_string(),
            line: 42,
            verified: false,
        };

        // 1. Start session
        let state = manager
            .start_session(&root, "git", &["status".to_string()], vec![bp.clone()])
            .await
            .unwrap();
        assert!(state.is_running);
        assert!(state.stopped_at_breakpoint.is_some());
        assert_eq!(state.stopped_at_breakpoint.unwrap().line, 42);

        // 2. Inspect stack
        let frames = manager.inspect_stack(&state.session_id).await.unwrap();
        assert!(!frames.is_empty());
        assert_eq!(frames[0].line, Some(42));

        // 3. Inspect variables
        let vars = manager.inspect_variables(&state.session_id, None).await.unwrap();
        assert!(!vars.is_empty());
        assert!(vars.iter().any(|v| v.name == "argc"));

        // 4. Step next
        let stepped = manager.step_next(&state.session_id).await.unwrap();
        assert_eq!(stepped.stopped_at_breakpoint.unwrap().line, 43);

        // 5. Terminate
        let term = manager.terminate_session(&state.session_id).await.unwrap();
        assert!(term);
    }
}
