pub mod inspect;
pub mod paths;
pub mod process;
pub mod shell;

pub use inspect::is_process_alive;
pub use paths::{normalize_path, normalize_path_str};
pub use process::{kill_process_group, ConfigureProcessGroup};
pub use shell::{command_for_script, default_shell};
