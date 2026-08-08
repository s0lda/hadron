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
                Some(ico_path.clone())
            } else if fallback_ico_path.exists() {
                Some(fallback_ico_path.clone())
            } else {
                None
            };

            if let Some(ico) = target_ico {
                println!("cargo:rerun-if-changed={}", ico.display());
                let mut res = winres::WindowsResource::new();
                // Pass the full absolute path string to prevent build toolchain path mismatches
                res.set_icon(ico.to_str().unwrap());
                res.set_language(0x0409); // US English (0x0409) so Task Manager reads FileDescription
                // VERSIONINFO metadata: FileDescription controls the name shown
                // in Task Manager's "Apps" column; ProductName appears in the
                // file's Properties → Details tab.
                res.set("FileDescription", "Hadron");
                res.set("ProductName", "Hadron");
                res.set("InternalName", "hadron.exe");
                res.set("OriginalFilename", "hadron.exe");

                // VERSIONINFO block is not generated unless FileVersion is set!
                if let Ok(version) = std::env::var("CARGO_PKG_VERSION") {
                    res.set("FileVersion", &version);
                    res.set("ProductVersion", &version);
                }

                // Enable GPUI DPI-awareness & shell interaction by embedding a basic manifest
                res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#);

                if let Err(e) = res.compile() {
                    panic!("winres error: {e}");
                }
            } else {
                panic!("winres: icon path does not exist");
            }
        }
    }
}
