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
        use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
            if handle.is_null() {
                return false;
            }
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let success = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(handle);

            if success != 0 {
                let path_str = String::from_utf16_lossy(&buf[..size as usize]);
                let path = std::path::Path::new(&path_str);
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let expected_stem = expected_name.strip_suffix(".exe").unwrap_or(expected_name);
                    return stem.eq_ignore_ascii_case(expected_stem);
                }
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
