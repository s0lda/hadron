fn main() {
    // Use CARGO_CFG_TARGET_OS instead of #[cfg(windows)] so the resource
    // compilation runs correctly even when cross-compiling from a non-Windows
    // host (build.rs is compiled for the host, not the target).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = std::path::PathBuf::from(&manifest_dir);
            let ico_path = manifest_path.join("assets").join("hadron.ico");
            let fallback_ico_path = manifest_path
                .join("..")
                .join("..")
                .join("assets")
                .join("hadron.ico");

            let target_ico = if ico_path.exists() {
                Some(ico_path)
            } else if fallback_ico_path.exists() {
                Some(fallback_ico_path)
            } else {
                None
            };

            if let Some(ico) = target_ico {
                println!("cargo:rerun-if-changed={}", ico.display());
                let mut res = winres::WindowsResource::new();
                let path_str = ico.to_str().unwrap_or("assets/hadron.ico");
                res.set_icon(path_str);
                // VERSIONINFO metadata: FileDescription controls the name shown
                // in Task Manager's "Apps" column; ProductName appears in the
                // file's Properties → Details tab.
                res.set("FileDescription", "Hadron");
                res.set("ProductName", "Hadron");
                res.set("InternalName", "hadron.exe");
                if let Err(e) = res.compile() {
                    eprintln!("winres error: {e}");
                }
            } else {
                eprintln!("winres: icon path does not exist");
            }
        }
    }
}
