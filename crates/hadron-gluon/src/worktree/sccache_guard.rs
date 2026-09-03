use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SccacheGuard {
    base_dir: PathBuf,
}

impl SccacheGuard {
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn build_env_for_quark(&self, quark_id: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("RUSTC_WRAPPER".to_string(), "sccache".to_string());
        let target_dir = self.base_dir.join("target/quarks").join(quark_id);
        map.insert("CARGO_TARGET_DIR".to_string(), target_dir.to_string_lossy().to_string());
        map
    }
}
