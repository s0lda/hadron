//! Zero-Copy CoW Worktree Provisioning (Capability #9).
//!
//! Fast worktree cloning using filesystem Copy-on-Write (reflink/ioctl_ficlone)
//! with automatic fallback to hardlink or standard file copying.

use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CowStrategy {
    Reflink,
    Hardlink,
    Copy,
}

impl CowStrategy {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Reflink => "reflink (zero-copy CoW)",
            Self::Hardlink => "hardlink",
            Self::Copy => "standard copy",
        }
    }
}

/// Clones a single file using CoW reflink if supported, falling back to hardlink or copy.
pub fn clone_file_cow(src: &Path, dst: &Path) -> io::Result<CowStrategy> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    // Attempt 1: reflink on Linux
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::process::Command::new("cp")
            .arg("--reflink=always")
            .arg(src)
            .arg(dst)
            .output()
        {
            if status.status.success() {
                return Ok(CowStrategy::Reflink);
            }
        }
    }

    // Attempt 2: macOS clonefile API / cp -c
    #[cfg(target_os = "macos")]
    {
        if let Ok(status) = std::process::Command::new("cp")
            .arg("-c")
            .arg(src)
            .arg(dst)
            .output()
        {
            if status.status.success() {
                return Ok(CowStrategy::Reflink);
            }
        }
    }

    // Attempt 3: Hardlink
    if fs::hard_link(src, dst).is_ok() {
        return Ok(CowStrategy::Hardlink);
    }

    // Attempt 4: Standard copy fallback
    fs::copy(src, dst)?;
    Ok(CowStrategy::Copy)
}

/// Recursively provisions a zero-copy CoW worktree replica.
pub fn provision_cow_worktree(
    src_dir: &Path,
    dst_dir: &Path,
    ignored_subdirs: &[&str],
) -> io::Result<(usize, CowStrategy)> {
    fs::create_dir_all(dst_dir)?;
    let mut files_cloned = 0;
    let mut primary_strategy = CowStrategy::Copy;

    let mut stack = vec![src_dir.to_path_buf()];

    while let Some(current_src) = stack.pop() {
        for entry in fs::read_dir(&current_src)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            if ignored_subdirs.iter().any(|ignored| ignored == &file_name) {
                continue;
            }

            let rel_path = path.strip_prefix(src_dir).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let current_dst = dst_dir.join(rel_path);

            if path.is_dir() {
                fs::create_dir_all(&current_dst)?;
                stack.push(path);
            } else if path.is_file() {
                let strat = clone_file_cow(&path, &current_dst)?;
                if files_cloned == 0 {
                    primary_strategy = strat;
                }
                files_cloned += 1;
            }
        }
    }

    Ok((files_cloned, primary_strategy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cow_provisioning_and_fallback() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        fs::write(src.path().join("main.rs"), b"fn main() {}").unwrap();
        fs::create_dir_all(src.path().join("crates").join("lib")).unwrap();
        fs::write(
            src.path().join("crates").join("lib").join("lib.rs"),
            b"pub fn foo() {}",
        )
        .unwrap();
        fs::create_dir_all(src.path().join("node_modules")).unwrap();
        fs::write(src.path().join("node_modules").join("heavy.js"), b"// heavy").unwrap();

        let (count, strat) = provision_cow_worktree(
            src.path(),
            dst.path().join("worktree-cow").as_path(),
            &["node_modules", ".git"],
        )
        .unwrap();

        assert_eq!(count, 2);
        assert!(matches!(strat, CowStrategy::Reflink | CowStrategy::Hardlink | CowStrategy::Copy));
        assert!(dst.path().join("worktree-cow").join("main.rs").is_file());
        assert!(dst.path().join("worktree-cow").join("crates/lib/lib.rs").is_file());
        assert!(!dst.path().join("worktree-cow").join("node_modules").exists());
    }
}
