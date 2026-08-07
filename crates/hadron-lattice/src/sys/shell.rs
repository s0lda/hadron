use std::process::Command;

/// Returns the system default shell binary and flag.
/// Unix: ("sh", "-c")
/// Windows: ("cmd.exe", "/C")
pub fn default_shell() -> (&'static str, &'static str) {
    #[cfg(windows)]
    {
        ("cmd.exe", "/C")
    }
    #[cfg(not(windows))]
    {
        ("sh", "-c")
    }
}

/// Builds a `std::process::Command` executing the given shell script string.
pub fn command_for_script(script: &str) -> Command {
    let (shell, flag) = default_shell();
    let mut cmd = Command::new(shell);
    cmd.arg(flag).arg(script);
    cmd
}
