/// Check if `pid` is currently alive and matches `expected_name`.
pub fn is_process_alive(pid: u32, expected_name: &str) -> bool {
    #[cfg(unix)]
    {
        let proc_comm = std::path::Path::new("/proc")
            .join(pid.to_string())
            .join("comm");
        if proc_comm.exists() {
            if let Ok(comm) = std::fs::read_to_string(proc_comm) {
                return comm.trim() == expected_name;
            }
            return false;
        }
        // Fallback for non-Linux POSIX: test via signal 0
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FALSE, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let mut handle = OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
                FALSE,
                pid,
            );
            if handle.is_null() {
                handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
            }
            if handle.is_null() {
                return false;
            }

            let mut exit_code: u32 = 0;
            let active = GetExitCodeProcess(handle, &mut exit_code) != 0 && exit_code == STILL_ACTIVE as u32;
            if !active {
                CloseHandle(handle);
                return false;
            }

            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let success = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(handle);

            if success != 0 {
                let path_str = String::from_utf16_lossy(&buf[..size as usize]);
                let clean_path = path_str.trim_start_matches("\\?\\").trim_start_matches("//?/");
                return matches_process_image_name(clean_path, expected_name);
            }
            false
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, expected_name);
        true
    }
}

/// Check if `pid` is currently alive in the context of `dir`.
///
/// Checks the local host first via [`is_process_alive`] because a Windows `hadron.exe`
/// opening a WSL UNC workspace (`\\wsl.localhost\<distro>\...`) still spawns
/// `hadron-gluon.exe` as a native Windows process whose Windows PID is written to
/// `gluon.lock`. If the PID is not a live host process and `dir` is a WSL UNC path,
/// falls back to inspecting `/proc/<pid>/comm` inside the target WSL distribution.
pub fn is_process_alive_named_in_dir(pid: u32, dir: &std::path::Path, expected_name: &str) -> bool {
    if is_process_alive(pid, expected_name) {
        return true;
    }
    if let Some((distro, _)) = crate::wsl::parse_wsl_unc_path(dir) {
        let wsl_proc = crate::wsl::format_wsl_unc_path(&distro, std::path::Path::new("/proc"));
        if wsl_proc.is_dir() {
            let comm_path = wsl_proc.join(pid.to_string()).join("comm");
            return std::fs::read_to_string(comm_path)
                .map(|c| matches_process_image_name(c.trim(), expected_name))
                .unwrap_or(false);
        }
        #[cfg(windows)]
        {
            let mut cmd = crate::wsl::wsl_aware_command(dir, "kill");
            cmd.args(["-0", &pid.to_string()]);
            return cmd.status().map(|s| s.success()).unwrap_or(false);
        }
    }
    false
}

/// Check if `pid` of `hadron-gluon` is currently alive in the context of `dir`.
pub fn is_process_alive_in_dir(pid: u32, dir: &std::path::Path) -> bool {
    is_process_alive_named_in_dir(pid, dir, "hadron-gluon")
}

/// Match a process image path against `expected_name`, accounting for `.exe` and
/// in-place Windows update `.old` / `.old.<pid>` rename suffixes on running binaries.
pub fn matches_process_image_name(clean_path: &str, expected_name: &str) -> bool {
    let file_name = clean_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(clean_path);
    let expected_stem = expected_name.strip_suffix(".exe").unwrap_or(expected_name);
    let actual_stem = file_name
        .split(".exe")
        .next()
        .unwrap_or(file_name)
        .strip_suffix(".old")
        .unwrap_or_else(|| file_name.split(".exe").next().unwrap_or(file_name));
    actual_stem.eq_ignore_ascii_case(expected_stem)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cmdline: String,
    pub rss_bytes: Option<u64>,
}

/// Format RSS byte count into human readable KB, MB, or GB string.
pub fn format_rss(rss_bytes: u64) -> String {
    if rss_bytes < 1024 * 1024 {
        format!("{} KB", rss_bytes / 1024)
    } else if rss_bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", (rss_bytes as f64) / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", (rss_bytes as f64) / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Helper to extract tree id from path containing `trees/<id>` or `trees\<id>`.
fn extract_tree_id_from_path(path: &std::path::Path) -> Option<String> {
    let s = path.to_string_lossy();
    let idx = s.find("trees/").or_else(|| s.find("trees\\"))?;
    let remainder = &s[idx + 6..];
    let id = remainder.split(['/', '\\']).next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Helper to extract tree id from NUL-separated environ bytes containing `HADRON_WSL_TAG=hadron:<id>`.
fn extract_tree_id_from_environ(environ_bytes: &[u8]) -> Option<String> {
    for entry in environ_bytes.split(|&b| b == 0) {
        if entry.starts_with(b"HADRON_WSL_TAG=hadron:") {
            let tag_bytes = &entry[b"HADRON_WSL_TAG=hadron:".len()..];
            if let Ok(tag) = std::str::from_utf8(tag_bytes) {
                if !tag.is_empty() {
                    return Some(tag.to_string());
                }
            }
        }
    }
    None
}

/// Enrich a process command line with worktree ` [trees/<id>]` tags derived from
/// `/proc/<pid>/cwd` symlinks and `HADRON_WSL_TAG=hadron:<id>` environment variables.
pub fn enrich_cmdline_with_worktree_and_env(
    cmdline: &str,
    cwd: Option<&std::path::Path>,
    environ_bytes: Option<&[u8]>,
) -> String {
    let tree_id = cwd
        .and_then(extract_tree_id_from_path)
        .or_else(|| environ_bytes.and_then(extract_tree_id_from_environ));

    if let Some(id) = tree_id {
        let tag_slash = format!("trees/{id}");
        let tag_backslash = format!("trees\\{id}");
        if !cmdline.contains(&tag_slash) && !cmdline.contains(&tag_backslash) {
            return format!("{cmdline} [trees/{id}]");
        }
    }
    cmdline.to_string()
}

/// Parse a `/proc/<pid>` directory into a [`ProcessInfo`].
fn parse_proc_pid_dir(proc_base: &std::path::Path, pid: u32) -> Option<ProcessInfo> {
    let pid_dir = proc_base.join(pid.to_string());
    if !pid_dir.is_dir() {
        return None;
    }

    let stat_content = std::fs::read_to_string(pid_dir.join("stat")).ok()?;
    let open_paren = stat_content.find('(')?;
    let close_paren = stat_content.rfind(')')?;
    let comm = stat_content[open_paren + 1..close_paren].to_string();
    let rest = stat_content[close_paren + 1..].trim();
    let mut parts = rest.split_whitespace();
    let _state = parts.next()?;
    let ppid: u32 = parts.next()?.parse().ok()?;

    let mut rss_bytes = None;
    if let Ok(status_content) = std::fs::read_to_string(pid_dir.join("status")) {
        for line in status_content.lines() {
            if line.starts_with("VmRSS:") {
                let tokens: Vec<&str> = line.split_whitespace().collect();
                if tokens.len() >= 2 {
                    if let Ok(kb) = tokens[1].parse::<u64>() {
                        rss_bytes = Some(kb * 1024);
                    }
                }
                break;
            }
        }
    }

    let raw_cmdline = match std::fs::read(pid_dir.join("cmdline")) {
        Ok(bytes) if !bytes.is_empty() => {
            let args: Vec<String> = bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect();
            if args.is_empty() {
                comm.clone()
            } else {
                args.join(" ")
            }
        }
        _ => comm.clone(),
    };

    let cwd = std::fs::read_link(pid_dir.join("cwd")).ok();
    let environ_bytes = std::fs::read(pid_dir.join("environ")).ok();
    let cmdline = enrich_cmdline_with_worktree_and_env(
        &raw_cmdline,
        cwd.as_deref(),
        environ_bytes.as_deref(),
    );

    Some(ProcessInfo {
        pid,
        ppid,
        name: comm,
        cmdline,
        rss_bytes,
    })
}

#[cfg(windows)]
fn inspect_windows_process(pid: u32) -> Option<ProcessInfo> {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let mut handle = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
            FALSE,
            pid,
        );
        if handle.is_null() {
            handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        }
        if handle.is_null() {
            return None;
        }

        let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let rss_bytes = if GetProcessMemoryInfo(
            handle,
            &mut pmc,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ) != 0 {
            Some(pmc.WorkingSetSize as u64)
        } else {
            None
        };

        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let success = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);

        if success != 0 {
            let path_str = String::from_utf16_lossy(&buf[..size as usize]);
            let clean = path_str.trim_start_matches(r"\\?\").trim_start_matches("//?/");
            let name = clean.rsplit(['\\', '/']).next().unwrap_or(clean).to_string();
            return Some(ProcessInfo {
                pid,
                ppid: 0,
                name,
                cmdline: clean.to_string(),
                rss_bytes,
            });
        }
        None
    }
}

/// Inspect a process by `pid` locally or via WSL UNC path if `dir` points to WSL.
pub fn inspect_process(pid: u32, dir: &std::path::Path) -> Option<ProcessInfo> {
    #[cfg(unix)]
    {
        if let Some(info) = parse_proc_pid_dir(std::path::Path::new("/proc"), pid) {
            return Some(info);
        }
    }

    #[cfg(windows)]
    {
        if let Some(info) = inspect_windows_process(pid) {
            return Some(info);
        }
    }

    if let Some((distro, _)) = crate::wsl::parse_wsl_unc_path(dir) {
        let wsl_proc = crate::wsl::format_wsl_unc_path(&distro, std::path::Path::new("/proc"));
        if wsl_proc.is_dir() {
            if let Some(info) = parse_proc_pid_dir(&wsl_proc, pid) {
                return Some(info);
            }
        }
    }

    None
}

/// Collect a process tree containing `root_pids` and all transitive descendant processes.
pub fn collect_process_tree(root_pids: &[u32], dir: &std::path::Path) -> Vec<ProcessInfo> {
    let mut all_procs = Vec::new();

    let mut proc_dirs = Vec::new();
    #[cfg(unix)]
    {
        proc_dirs.push(std::path::PathBuf::from("/proc"));
    }
    if let Some((distro, _)) = crate::wsl::parse_wsl_unc_path(dir) {
        let wsl_proc = crate::wsl::format_wsl_unc_path(&distro, std::path::Path::new("/proc"));
        if wsl_proc.is_dir() {
            proc_dirs.push(wsl_proc);
        }
    }

    for proc_base in proc_dirs {
        if let Ok(entries) = std::fs::read_dir(&proc_base) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if let Ok(pid) = name.parse::<u32>() {
                        if let Some(info) = parse_proc_pid_dir(&proc_base, pid) {
                            if !all_procs.iter().any(|p: &ProcessInfo| p.pid == pid) {
                                all_procs.push(info);
                            }
                        }
                    }
                }
            }
        }
    }

    for &r in root_pids {
        if r > 0 && !all_procs.iter().any(|p| p.pid == r) {
            if let Some(info) = inspect_process(r, dir) {
                all_procs.push(info);
            }
        }
    }

    let mut included = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    for &r in root_pids {
        if r > 0 {
            included.insert(r);
            queue.push_back(r);
        }
    }
    for p in &all_procs {
        if p.cmdline.contains("trees/") || p.cmdline.contains("trees\\") {
            if included.insert(p.pid) {
                queue.push_back(p.pid);
            }
        }
    }

    let mut children_map: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for p in &all_procs {
        children_map.entry(p.ppid).or_default().push(p.pid);
    }

    while let Some(parent) = queue.pop_front() {
        if let Some(children) = children_map.get(&parent) {
            for &child in children {
                if included.insert(child) {
                    queue.push_back(child);
                }
            }
        }
    }

    let mut tree: Vec<ProcessInfo> = all_procs
        .into_iter()
        .filter(|p| included.contains(&p.pid))
        .collect();
    tree.sort_by_key(|p| p.pid);
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn format_rss_converts_bytes_to_readable_units() {
        assert_eq!(format_rss(512 * 1024), "512 KB");
        assert_eq!(format_rss(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(format_rss(1536 * 1024 * 1024), "1.50 GB");
    }

    #[test]
    fn inspect_process_current_pid_returns_info() {
        let current_pid = std::process::id();
        let local_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let info = inspect_process(current_pid, &local_dir);
        assert!(info.is_some(), "Current process must be inspectable");
        let info = info.unwrap();
        assert_eq!(info.pid, current_pid);

        let tree = collect_process_tree(&[current_pid], &local_dir);
        assert!(tree.iter().any(|p| p.pid == current_pid));
    }

    #[test]
    fn matches_windows_process_image_including_renamed_old_binaries() {
        assert!(matches_process_image_name(
            r"C:\Users\user\AppData\Local\Programs\Hadron\hadron-gluon.exe",
            "hadron-gluon"
        ));
        assert!(matches_process_image_name(
            r"C:\Users\user\AppData\Local\Programs\Hadron\hadron-gluon.exe.old",
            "hadron-gluon"
        ));
        assert!(matches_process_image_name(
            r"C:\Users\user\AppData\Local\Programs\Hadron\hadron-gluon.exe.old.4120",
            "hadron-gluon.exe"
        ));
        assert!(!matches_process_image_name(
            r"C:\Windows\System32\svchost.exe",
            "hadron-gluon"
        ));
    }

    #[test]
    fn is_process_alive_in_dir_detects_current_process_on_local_dir() {
        let current_pid = std::process::id();
        let local_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        //Using current process id with current process name or fallback
        #[cfg(unix)]
        {
            let comm_path = PathBuf::from(format!("/proc/{current_pid}/comm"));
            if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                let name = comm.trim();
                assert!(is_process_alive_named_in_dir(current_pid, &local_dir, name));
                let wsl_unc_dir = std::path::Path::new(r"\\wsl.localhost\Ubuntu\workspace\.hadron");
                assert!(
                    is_process_alive_named_in_dir(current_pid, wsl_unc_dir, name),
                    "host-spawned daemon PID must be detected as alive even when workspace is a WSL UNC path"
                );
            }
        }
        // Non-existent PID should always be false
        assert!(!is_process_alive_named_in_dir(999_999_999, &local_dir, "hadron-gluon"));
    }

    #[test]
    fn enrich_cmdline_with_worktree_and_env_extracts_from_cwd_and_env() {
        let cmd = "hadron-gluon --daemon";
        let cwd_path = std::path::PathBuf::from("/home/user/.hadron/trees/acp-agy/crates/hadron-lattice");
        let enriched = enrich_cmdline_with_worktree_and_env(cmd, Some(&cwd_path), None);
        assert_eq!(enriched, "hadron-gluon --daemon [trees/acp-agy]");

        // Idempotency check: if cmdline already has trees/acp-agy, return unchanged
        let already_enriched = enrich_cmdline_with_worktree_and_env(&enriched, Some(&cwd_path), None);
        assert_eq!(already_enriched, "hadron-gluon --daemon [trees/acp-agy]");

        // Windows backslash cwd check
        let win_cwd = std::path::PathBuf::from(r"C:\dev\.hadron\trees\acp-agy\crates");
        let enriched_win = enrich_cmdline_with_worktree_and_env("hadron.exe", Some(&win_cwd), None);
        assert_eq!(enriched_win, "hadron.exe [trees/acp-agy]");

        // Environment fallback check
        let env_bytes = b"FOO=bar\0HADRON_WSL_TAG=hadron:cli-agy\0BAR=baz\0";
        let enriched_env = enrich_cmdline_with_worktree_and_env("python test.py", None, Some(env_bytes));
        assert_eq!(enriched_env, "python test.py [trees/cli-agy]");

        // Unmatched path returns unchanged
        let plain_cwd = std::path::PathBuf::from("/home/user/project");
        assert_eq!(enrich_cmdline_with_worktree_and_env("cargo test", Some(&plain_cwd), None), "cargo test");
    }
}
