use std::path::{Path, PathBuf};
use std::process::Command;

/// Binary targets shipped from this package and their distinct Windows `FileDescription`
/// metadata so Task Manager displays `Hadron`, `Hadron Gluon`, and `Hadron Forge MCP`
/// instead of labelling every child binary identically.
const BIN_TARGETS: &[(&str, &str, &str)] = &[
    ("hadron", "Hadron", "hadron.exe"),
    ("hadron-gluon", "Hadron Gluon", "hadron-gluon.exe"),
    ("hadron-forge-mcp", "Hadron Forge MCP", "hadron-forge-mcp.exe"),
];

fn find_windows_sdk_rc() -> Option<PathBuf> {
    let arch_subdir = match std::env::var("CARGO_CFG_TARGET_ARCH").ok().as_deref() {
        Some("aarch64") => "arm64",
        Some("x86") => "x86",
        _ => "x64",
    };

    let mut roots = Vec::new();
    if let Ok(output) = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
            "/reg:32",
        ])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().rev() {
                if line.trim_start().starts_with("KitsRoot") {
                    if let Some(idx) = line.find("REG_SZ") {
                        let kit: String = line[idx + 6..].trim().to_string();
                        if !kit.is_empty() {
                            roots.push(PathBuf::from(kit));
                        }
                    }
                }
            }
        }
    }
    roots.push(PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10"));

    for root in roots {
        let bin_dir = root.join("bin");
        if let Ok(entries) = std::fs::read_dir(&bin_dir) {
            let mut versioned: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            versioned.sort();
            for dir in versioned.into_iter().rev() {
                let candidate = dir.join(arch_subdir).join("rc.exe");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        let direct = bin_dir.join(arch_subdir).join("rc.exe");
        if direct.exists() {
            return Some(direct);
        }
    }
    None
}

fn compile_rc_for_bin(
    manifest_dir: &str,
    out_dir: &Path,
    bin_name: &str,
    rc_path: &Path,
) -> Result<PathBuf, String> {
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let out_o = out_dir.join(format!("{bin_name}.res.o"));
        let windres = if cfg!(windows) { "windres.exe" } else { "windres" };
        let status = Command::new(windres)
            .arg(format!("-I{manifest_dir}"))
            .arg(rc_path)
            .arg(&out_o)
            .status()
            .map_err(|e| format!("failed to spawn {windres}: {e}"))?;
        if !status.success() {
            return Err(format!("{windres} exited with {status}"));
        }
        Ok(out_o)
    } else {
        let out_lib = out_dir.join(format!("{bin_name}.res.lib"));
        let rc_exe = find_windows_sdk_rc().unwrap_or_else(|| PathBuf::from("rc.exe"));
        let output = Command::new(&rc_exe)
            .arg(format!("/I{manifest_dir}"))
            .arg(format!("/fo{}", out_lib.display()))
            .arg(rc_path)
            .output()
            .map_err(|e| format!("failed to spawn {}: {e}", rc_exe.display()))?;
        if !output.status.success() {
            return Err(format!(
                "rc.exe failed for {bin_name}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(out_lib)
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Use CARGO_CFG_TARGET_OS instead of #[cfg(windows)] so the resource
    // compilation runs correctly even when cross-compiling from a non-Windows
    // host (build.rs is compiled for the host, not the target).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = PathBuf::from(&manifest_dir);
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
                let ico_str = ico.to_str().expect("valid UTF-8 icon path");
                let out_dir = PathBuf::from(
                    std::env::var("OUT_DIR").expect("OUT_DIR set by cargo"),
                );
                let version = std::env::var("CARGO_PKG_VERSION").ok();

                for &(bin_name, file_description, exe_name) in BIN_TARGETS {
                    let mut res = winres::WindowsResource::new();
                    res.set_icon(ico_str);
                    res.set_language(0x0409); // US English (0x0409) so Task Manager reads FileDescription

                    let properties = [
                        ("FileDescription", file_description),
                        ("ProductName", "Hadron"),
                        ("CompanyName", "Hadron"),
                        ("LegalCopyright", "Apache-2.0"),
                        ("InternalName", exe_name),
                        ("OriginalFilename", exe_name),
                    ];
                    for (key, val) in &properties {
                        res.set(key, val);
                    }

                    if let Some(ver) = &version {
                        res.set("FileVersion", ver);
                        res.set("ProductVersion", ver);

                        let parts: Vec<u64> =
                            ver.split('.').filter_map(|s| s.parse().ok()).collect();
                        if parts.len() >= 3 {
                            let ver_u64 = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16);
                            res.set_version_info(winres::VersionInfo::FILEVERSION, ver_u64);
                            res.set_version_info(winres::VersionInfo::PRODUCTVERSION, ver_u64);
                        }
                    }

                    let rc_path = out_dir.join(format!("{bin_name}.rc"));
                    if let Err(e) = res.write_resource_file(&rc_path) {
                        if cfg!(windows) {
                            panic!("[hadron build.rs] failed to write {}: {e}", rc_path.display());
                        }
                        continue;
                    }

                    match compile_rc_for_bin(&manifest_dir, &out_dir, bin_name, &rc_path) {
                        Ok(compiled_res) => {
                            println!(
                                "cargo:rustc-link-arg-bin={bin_name}={}",
                                compiled_res.display()
                            );
                        }
                        Err(e) => {
                            if cfg!(windows) {
                                panic!("[hadron build.rs] winres compilation ERROR: {e}");
                            }
                        }
                    }
                }
            } else {
                println!("cargo:warning=[hadron build.rs] ERROR: icon path does not exist!");
                panic!("winres: icon path does not exist");
            }
        }
    }
}

