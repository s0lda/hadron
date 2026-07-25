//! Process group tracking and shutdown cleanup for quark subprocesses.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

static TRACKED_PROCESS_GROUPS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();

fn get_tracked() -> &'static Mutex<HashSet<u32>> {
    TRACKED_PROCESS_GROUPS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Register a spawned process group PID.
pub fn register(pid: u32) {
    if let Ok(mut set) = get_tracked().lock() {
        set.insert(pid);
    }
}

/// Unregister a finished process group PID.
pub fn unregister(pid: u32) {
    if let Ok(mut set) = get_tracked().lock() {
        set.remove(&pid);
    }
}

/// Kill all currently tracked process groups.
pub fn kill_all() {
    let pids: Vec<u32> = {
        if let Ok(mut set) = get_tracked().lock() {
            set.drain().collect()
        } else {
            Vec::new()
        }
    };
    for pid in pids {
        crate::merge::kill_process_group(pid);
    }
}

/// RAII Guard that calls [`kill_all`] when dropped.
pub struct ShutdownGuard;

impl ShutdownGuard {
    pub fn new() -> Self {
        ShutdownGuard
    }
}

impl Default for ShutdownGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        kill_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proc_registration() {
        register(999999);
        assert!(get_tracked().lock().unwrap().contains(&999999));
        unregister(999999);
        assert!(!get_tracked().lock().unwrap().contains(&999999));
    }
}
