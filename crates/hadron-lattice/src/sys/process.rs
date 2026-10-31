use std::process::Command;

/// Signal/terminate a child process and all its subprocesses (process group).
pub fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                TerminateProcess(handle, 1);
                CloseHandle(handle);
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

/// Gracefully terminate a `hadron-gluon` process identified by `pid` in `field_dir`,
/// escalating to process-group force kill if it does not exit within 1.5s.
///
/// Supports both host-spawned `hadron-gluon` processes and WSL-native Linux `hadron-gluon`
/// processes when `field_dir` is a Windows WSL UNC path (`\\wsl.localhost\<distro>\...`).
pub fn terminate_gluon_in_dir(pid: u32, field_dir: &std::path::Path) {
    #[cfg(unix)]
    {
        unsafe {
            let _ = libc::kill(pid as i32, libc::SIGTERM);
        }
        for _ in 0..30 {
            if !crate::sys::inspect::is_process_alive_in_dir(pid, field_dir) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[cfg(windows)]
    {
        if !crate::sys::inspect::is_process_alive(pid, "hadron-gluon")
            && crate::wsl::is_wsl_unc_path(field_dir)
        {
            let _ = crate::wsl::wsl_aware_command(field_dir, "kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            for _ in 0..30 {
                if !crate::sys::inspect::is_process_alive_in_dir(pid, field_dir) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = crate::wsl::wsl_aware_command(field_dir, "kill")
                .args(["-KILL", &format!("-{pid}"), &pid.to_string()])
                .status();
            for _ in 0..20 {
                if !crate::sys::inspect::is_process_alive_in_dir(pid, field_dir) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            return;
        }
    }
    if crate::sys::inspect::is_process_alive_in_dir(pid, field_dir) {
        kill_process_group(pid);
        for _ in 0..20 {
            if !crate::sys::inspect::is_process_alive_in_dir(pid, field_dir) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// Extension trait to configure process group creation cross-platform.
pub trait ConfigureProcessGroup {
    fn set_process_group(&mut self) -> &mut Self;
}

impl ConfigureProcessGroup for Command {
    fn set_process_group(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            self.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x00000200 | 0x08000000); // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        }
        self
    }
}
