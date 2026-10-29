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
            active
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
/// When `dir` is a WSL UNC path (`\\wsl.localhost\<distro>\...` or `\\wsl$\<distro>\...`),
/// execution routes inside the target WSL distribution via `wsl_aware_command(dir, "kill")`
/// with `-0 <pid>` so Linux daemon PIDs are verified natively in WSL rather than failing
/// Win32 `OpenProcess` calls on the Windows host.
/// Otherwise, checks process liveness on the local host.
pub fn is_process_alive_named_in_dir(pid: u32, dir: &std::path::Path, expected_name: &str) -> bool {
    if crate::wsl::is_wsl_unc_path(dir) {
        let mut cmd = crate::wsl::wsl_aware_command(dir, "kill");
        cmd.args(["-0", &pid.to_string()]);
        return cmd.status().map(|s| s.success()).unwrap_or(false);
    }
    is_process_alive(pid, expected_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        // Using current process id with current process name or fallback
        #[cfg(unix)]
        {
            let comm_path = PathBuf::from(format!("/proc/{current_pid}/comm"));
            if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                let name = comm.trim();
                assert!(is_process_alive_named_in_dir(current_pid, &local_dir, name));
            }
        }
        // Non-existent PID should always be false
        assert!(!is_process_alive_named_in_dir(999_999_999, &local_dir, "hadron-gluon"));
    }
}
