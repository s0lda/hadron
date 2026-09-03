//! Diagnostic Crash-Dump & `/proc` Watchdog Probe.
//!
//! Samples `/proc/<pid>/wchan`, child process trees, memory usage, and open file descriptors
//! when a quark has been silent or stuck, generating a forensic post-mortem before termination.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessForensicReport {
    pub pid: u32,
    pub wchan: String,
    pub vm_rss_kb: u64,
    pub child_pids: Vec<u32>,
    pub open_fd_count: usize,
    pub timestamp: u64,
}

#[derive(Debug, Default, Clone)]
pub struct QuarkProbe;

impl QuarkProbe {
    pub fn new() -> Self {
        Self
    }

    /// Captures a forensic dump from `/proc/<pid>` on Linux, or fallback diagnostics.
    pub fn capture_dump(pid: u32) -> Option<ProcessForensicReport> {
        let proc_dir = format!("/proc/{pid}");
        let proc_path = Path::new(&proc_dir);
        if !proc_path.exists() {
            return None;
        }

        let wchan = fs::read_to_string(proc_path.join("wchan"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        // Read statm: 2nd value is resident set size (in pages)
        let vm_rss_kb = if let Ok(statm) = fs::read_to_string(proc_path.join("statm")) {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if parts.len() >= 2 {
                parts[1].parse::<u64>().unwrap_or(0) * 4 // assume 4KB pages
            } else {
                0
            }
        } else {
            0
        };

        // Enumerate child processes from task/<pid>/children if available
        let mut child_pids = Vec::new();
        if let Ok(children_str) = fs::read_to_string(proc_path.join(format!("task/{pid}/children"))) {
            for token in children_str.split_whitespace() {
                if let Ok(cpid) = token.parse::<u32>() {
                    child_pids.push(cpid);
                }
            }
        }

        // Count open file descriptors
        let open_fd_count = if let Ok(entries) = fs::read_dir(proc_path.join("fd")) {
            entries.flatten().count()
        } else {
            0
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Some(ProcessForensicReport {
            pid,
            wchan,
            vm_rss_kb,
            child_pids,
            open_fd_count,
            timestamp,
        })
    }

    /// Formats the forensic report into a markdown post-mortem summary.
    pub fn format_dump(report: &ProcessForensicReport) -> String {
        format!(
            "### Quark Forensic Crash Dump (PID: {})\n\
             - **Wait Channel (wchan)**: `{}`\n\
             - **Resident Set (RSS)**: {} KB\n\
             - **Open File Descriptors**: {}\n\
             - **Child Processes**: {:?}\n\
             - **Timestamp**: {}",
            report.pid,
            report.wchan,
            report.vm_rss_kb,
            report.open_fd_count,
            report.child_pids,
            report.timestamp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_dump_current_process() {
        let current_pid = std::process::id();
        let dump = QuarkProbe::capture_dump(current_pid);
        assert!(dump.is_some());
        let report = dump.unwrap();
        assert_eq!(report.pid, current_pid);
        assert!(report.open_fd_count > 0);

        let formatted = QuarkProbe::format_dump(&report);
        assert!(formatted.contains("Quark Forensic Crash Dump"));
        assert!(formatted.contains(&current_pid.to_string()));
    }

    #[test]
    fn test_capture_dump_nonexistent_pid() {
        let dump = QuarkProbe::capture_dump(999_999_999);
        assert!(dump.is_none());
    }
}
