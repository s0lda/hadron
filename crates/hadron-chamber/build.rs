fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Use CARGO_CFG_TARGET_OS instead of #[cfg(windows)] so the resource
    // compilation runs correctly even when cross-compiling from a non-Windows
    // host (build.rs is compiled for the host, not the target).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    println!("cargo:warning=[hadron build.rs] Target OS: {target_os}, Target Env: {target_env}");

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
                println!("cargo:warning=[hadron build.rs] Using icon path: {}", ico.display());

                let mut res = winres::WindowsResource::new();
                // Pass the full absolute path string to prevent build toolchain path mismatches
                let ico_str = ico.to_str().expect("valid UTF-8 icon path");
                res.set_icon(ico_str);
                res.set_language(0x0409); // US English (0x0409) so Task Manager reads FileDescription

                // VERSIONINFO metadata: FileDescription controls the name shown
                // in Task Manager's "Apps" column; ProductName appears in the
                // file's Properties -> Details tab.
                let properties = [
                    ("FileDescription", "Hadron"),
                    ("ProductName", "Hadron"),
                    ("CompanyName", "Hadron"),
                    ("LegalCopyright", "Apache-2.0"),
                    ("InternalName", "hadron.exe"),
                    ("OriginalFilename", "hadron.exe"),
                ];

                for (key, val) in &properties {
                    res.set(key, val);
                    println!("cargo:warning=[hadron build.rs] Added metadata property: {key} = '{val}'");
                }

                if let Ok(version) = std::env::var("CARGO_PKG_VERSION") {
                    res.set("FileVersion", &version);
                    res.set("ProductVersion", &version);
                    println!("cargo:warning=[hadron build.rs] Added version string metadata: FileVersion = '{version}', ProductVersion = '{version}'");

                    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
                    if parts.len() >= 3 {
                        let ver_u64 = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16);
                        res.set_version_info(winres::VersionInfo::FILEVERSION, ver_u64);
                        res.set_version_info(winres::VersionInfo::PRODUCTVERSION, ver_u64);
                        println!("cargo:warning=[hadron build.rs] Added numeric version info u64: {ver_u64:#x} ({}.{}.{})", parts[0], parts[1], parts[2]);
                    }
                }

                println!("cargo:warning=[hadron build.rs] Compiling Windows resources via winres...");
                match res.compile() {
                    Ok(_) => {
                        println!("cargo:warning=[hadron build.rs] winres resource compilation completed successfully!");
                        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
                        let out_path = std::path::PathBuf::from(&out_dir);
                        let res_lib = out_path.join("resource.lib");
                        let res_res = out_path.join("resource.res");
                        let res_o = out_path.join("resource.o");

                        if res_lib.exists() {
                            println!("cargo:rustc-link-arg={}", res_lib.display());
                            println!("cargo:warning=[hadron build.rs] Added rustc-link-arg: {}", res_lib.display());
                        } else if res_res.exists() {
                            println!("cargo:rustc-link-arg={}", res_res.display());
                            println!("cargo:warning=[hadron build.rs] Added rustc-link-arg: {}", res_res.display());
                        } else if res_o.exists() {
                            println!("cargo:rustc-link-arg={}", res_o.display());
                            println!("cargo:warning=[hadron build.rs] Added rustc-link-arg: {}", res_o.display());
                        }
                    }
                    Err(e) => panic!("[hadron build.rs] winres compilation ERROR: {e}"),
                }

            } else {
                println!("cargo:warning=[hadron build.rs] ERROR: icon path does not exist!");
                panic!("winres: icon path does not exist");
            }
        }
    } else {
        println!("cargo:warning=[hadron build.rs] Non-Windows target OS '{target_os}', skipping winres");
    }
}

