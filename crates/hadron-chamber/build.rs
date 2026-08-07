fn main() {
    #[cfg(windows)]
    {
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
                let mut res = winres::WindowsResource::new();
                res.set_icon(ico.to_str().unwrap_or("assets/hadron.ico"));
                if let Err(e) = res.compile() {
                    eprintln!("winres error: {e}");
                }
            } else {
                eprintln!("winres: icon path does not exist");
            }
        }
    }
}
