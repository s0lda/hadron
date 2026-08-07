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
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
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
            self.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }
        self
    }
}
