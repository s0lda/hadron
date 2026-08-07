fn main() {
    #[cfg(windows)]
    {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let ico_path = std::path::PathBuf::from(manifest_dir)
                .join("..")
                .join("..")
                .join("assets")
                .join("hadron.ico");
            if ico_path.exists() {
                let mut res = winres::WindowsResource::new();
                res.set_icon(ico_path.to_str().unwrap_or("../../assets/hadron.ico"));
                if let Err(e) = res.compile() {
                    eprintln!("winres error: {e}");
                }
            } else {
                eprintln!("winres: icon path does not exist: {}", ico_path.display());
            }
        }
    }
}
