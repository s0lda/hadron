use std::path::Path;
use std::process::Command;
#[cfg(windows)]
use hadron_lattice::term::{self, Source};

/// Lists the files in the workspace *as they are on disk*, honouring `.gitignore`.
///
/// `git ls-files` alone lists the **index**, which is not what a file tree means:
/// a file deleted from the working tree is still in the index (so it would keep
/// showing), and a new untracked file is not in it at all (so it would never
/// appear). `--cached --others --exclude-standard` unions tracked and untracked,
/// and the `exists()` filter drops anything that is only in the index.
/// Each returned entry is `(path, is_ignored)`. Non-ignored (tracked or untracked)
/// files are listed individually; gitignored entries are unioned in with `--directory`
fn is_heavy_ignored_dir(rel_path: &str) -> bool {
    for c in rel_path.split('/') {
        match c {
            "target" | "node_modules" | ".git" | "trees" | "reaped" | "sessions"
            | "update" | "venv" | ".venv" | "dist" | "build" | "incremental" => return true,
            _ => {}
        }
    }
    false
}

/// Ignored entries, with wholly-ignored heavy directories collapsed to a single entry.
/// Non-heavy gitignored directories (e.g. `.hadron/docs/plans/`, `.hadron/nucleus/`, `internal/`)
/// are automatically expanded so internal plans and documentation are discoverable in chat completions.
pub fn list_workspace_files(
    repo_root: &Path,
    expanded_dirs: &std::collections::HashSet<String>,
) -> Vec<(String, bool)> {
    let mut files: Vec<(String, bool)> = Vec::new();

    // Tracked ∪ untracked, minus ignored — the real, editable files.
    if let Ok(output) = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--deduplicate",
        ])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if repo_root.join(line).exists() {
                    files.push((line.to_string(), false));
                }
            }
        }
    }

    // Ignored entries, with wholly-ignored heavy directories collapsed to a single entry.
    let mut ignored_dirs_to_expand = Vec::new();
    if let Ok(output) = Command::new("git")
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--deduplicate",
        ])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                // `--directory` never emits `.git/`, but guard anyway; `exists()` drops a
                // stale entry. A collapsed dir keeps its trailing `/` (checked verbatim).
                let bare = line.trim_end_matches('/');
                if !bare.is_empty() && !line.starts_with(".git/") && repo_root.join(bare).exists() {
                    files.push((line.to_string(), true));
                    if line.ends_with('/') {
                        let path_str = bare.to_string();
                        if !is_heavy_ignored_dir(&path_str) || expanded_dirs.contains(&path_str) {
                            ignored_dirs_to_expand.push(path_str);
                        }
                    }
                }
            }
        }
    }

    // Recursively expand gitignored directories
    let mut i = 0;
    while i < ignored_dirs_to_expand.len() {
        let dir_rel = ignored_dirs_to_expand[i].clone();
        let dir_abs = repo_root.join(&dir_rel);
        if let Ok(entries) = std::fs::read_dir(dir_abs) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let child_rel = format!("{dir_rel}/{name}");
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let entry_path = if is_dir {
                    format!("{child_rel}/")
                } else {
                    child_rel.clone()
                };
                files.push((entry_path, true));
                if is_dir {
                    if !is_heavy_ignored_dir(&child_rel) || expanded_dirs.contains(&child_rel) {
                        ignored_dirs_to_expand.push(child_rel);
                    }
                }
            }
        }
        i += 1;
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.dedup_by(|a, b| a.0 == b.0);
    files
}

/// One node of the File Tree the right-rail renders. Built by folding the flat
/// `(path, is_ignored)` list from [`list_workspace_files`] into a nested tree via
/// [`FileTreeNode::insert`]. Pure data (no gpui) so the tree-building logic is unit
/// testable; the chamber's `render_node` walks it to paint rows.
#[derive(Default, Debug)]
pub struct FileTreeNode {
    pub children: std::collections::BTreeMap<String, FileTreeNode>,
    pub is_file: bool,
    pub is_ignored: bool,
    /// The node's path from the tree root (e.g. `"src/app/render.rs"`). Set on
    /// EVERY node, interior folders included — an empty `full_path` on a folder
    /// made every folder row share the gpui id `tree-row-`, colliding so the
    /// expand click mis-routed (folders "wouldn't open") and folder context menus
    /// pointed at an empty path.
    pub full_path: String,
}

impl FileTreeNode {
    /// `is_dir_leaf` marks a path that is itself a directory (a collapsed
    /// gitignored dir, kept with a trailing `/` by [`list_workspace_files`]) — its
    /// last component is a folder, not a file. Interior directories start
    /// un-ignored; [`FileTreeNode::resolve_ignores`] computes their flag from their
    /// children afterwards.
    pub fn insert(&mut self, path: &str, is_ignored: bool, is_dir_leaf: bool) {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        let mut current = self;
        let mut running = String::new();
        for (i, part) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            let is_file = last && !is_dir_leaf;
            if running.is_empty() {
                running = part.to_string();
            } else {
                running = format!("{running}/{part}");
            }
            current = current.children.entry(part.to_string()).or_default();
            // EVERY node gets its running path — not just the leaf (see `full_path`).
            if current.full_path.is_empty() {
                current.full_path = running.clone();
            }
            if last {
                current.is_file = is_file;
                current.is_ignored = is_ignored;
            }
        }
    }

    /// Bottom-up: a file/collapsed-dir keeps its own flag; a directory with
    /// children is ignored only when **every** child is. Returns this node's
    /// resolved ignored state so the parent can fold it in.
    pub fn resolve_ignores(&mut self) -> bool {
        if self.is_file || self.children.is_empty() {
            return self.is_ignored;
        }
        let mut all_ignored = true;
        for child in self.children.values_mut() {
            if !child.resolve_ignores() {
                all_ignored = false;
            }
        }
        self.is_ignored = all_ignored;
        all_ignored
    }
}

/// Children of a node, folders first then files, each group name-sorted — the
/// order the File Tree paints rows in.
pub fn sorted_children(node: &FileTreeNode) -> Vec<(&String, &FileTreeNode)> {
    let mut children: Vec<(&String, &FileTreeNode)> = node.children.iter().collect();
    children.sort_by(|(a_name, a), (b_name, b)| match (a.is_file, b.is_file) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => a_name.cmp(b_name),
    });
    children
}

/// Reads the contents of a workspace file.
pub fn read_workspace_file(repo_root: &Path, file_path: &str) -> Option<String> {
    let full_path = repo_root.join(file_path);
    if let (Ok(canon_root), Ok(canon_full)) = (repo_root.canonicalize(), full_path.canonicalize()) {
        if canon_full.starts_with(canon_root) {
            std::fs::read_to_string(canon_full).ok()
        } else {
            None
        }
    } else {
        None
    }
}

/// Prepares workspace file content for preview rendering in the file tree viewer.
///
/// Markdown files (`.md`, `.markdown`) are returned verbatim so they render formatted prose.
/// Source code, configuration, markup, and text files are wrapped in a Markdown code block
/// with their corresponding tree-sitter language identifier so `TextView::markdown` renders
/// full syntax highlighting. Enclosing backtick fences dynamically scale if the content contains
/// backticks.
pub fn format_file_preview(path: &str, content: &str) -> String {
    let path_obj = Path::new(path);
    let ext = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");
    let file_name = path_obj.file_name().and_then(|f| f.to_str()).unwrap_or("");

    let lower_ext = ext.to_lowercase();
    let lower_name = file_name.to_lowercase();

    if lower_ext == "md" || lower_ext == "markdown" {
        return crate::mermaid::plugin::format_html_wrappers(content);
    }

    let lang = match lower_ext.as_str() {
        "rs" => "rust",
        "toml" => "toml",
        "json" => "json",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "css" => "css",
        "scss" => "scss",
        "html" | "htm" => "html",
        "cpp" | "cxx" | "cc" | "c" | "h" | "hpp" => "cpp",
        "py" => "python",
        "sh" | "bash" | "zsh" => "bash",
        "yaml" | "yml" => "yaml",
        "sql" => "sql",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "diff" | "patch" => "diff",
        "xml" => "xml",
        "lua" => "lua",
        "zig" => "zig",
        "wgsl" => "wgsl",
        "mermaid" | "mmd" => "mermaid",
        _ => {
            if lower_name == "dockerfile" {
                "dockerfile"
            } else if lower_name == "makefile" {
                "makefile"
            } else {
                lower_ext.as_str()
            }
        }
    };

    // Determine backtick fence length to avoid collisions with backticks inside content
    let mut fence_len = 3;
    let mut cur_len = 0;
    for ch in content.chars() {
        if ch == '`' {
            cur_len += 1;
            if cur_len >= fence_len {
                fence_len = cur_len + 1;
            }
        } else {
            cur_len = 0;
        }
    }

    let fence = "`".repeat(fence_len);
    format!("{}{}\n{}\n{}", fence, lang, content, fence)
}


/// Which program opens a source file the human clicked — a chat message's
/// `file://` link, or the file tree's "Open in editor".
///
/// `System` means "let the platform decide" (`xdg-open`/`open`/`explorer`), which
/// is what happened before this existed: it resolves through the desktop's own
/// association, so a `.rs` file on a box where that association is Vim opens Vim.
///
/// Not a strict ladder. The Settings picker offers the named variants only, but a
/// hand-edited `chamber.json` may carry `{"Custom": "kate -l {line} {file}"}` and it
/// is honoured — the same tolerance `Team::nucleus_index_budget_kb` has.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EditorChoice {
    #[default]
    System,
    Zed,
    VsCode,
    Cursor,
    Sublime,
    /// A whitespace-separated command line. `{file}` and `{line}` are substituted
    /// where they appear; with no `{file}` the path is appended as the last argument.
    Custom(String),
}

impl EditorChoice {
    /// The label the Settings picker shows. Also the SSOT for the offered ladder's
    /// order, so a new variant cannot be added without deciding how it reads.
    pub fn label(&self) -> &str {
        match self {
            Self::System => "System default",
            Self::Zed => "Zed",
            Self::VsCode => "VS Code",
            Self::Cursor => "Cursor",
            Self::Sublime => "Sublime Text",
            Self::Custom(_) => "Custom",
        }
    }
}

/// The variants the Settings picker offers, in order. `Custom` is deliberately
/// absent: it has no one value to click, and is reachable by hand-editing.
pub const EDITOR_LADDER: [EditorChoice; 5] = [
    EditorChoice::System,
    EditorChoice::Zed,
    EditorChoice::VsCode,
    EditorChoice::Cursor,
    EditorChoice::Sublime,
];

/// Split a `file://` URL into the path it names and the line it points at.
///
/// `None` for **any other scheme** — that is the whole point: an `https://` link in a
/// chat message must keep going to the browser, so the caller falls back to the
/// platform opener rather than handing a URL to a code editor.
///
/// Understands the two fragment spellings a human writes by hand (`#L332`, `#332`)
/// and percent-decodes the path, because a URL is where `%20` comes from.
pub fn file_url_target(url: &str) -> Option<(std::path::PathBuf, Option<u32>)> {
    let rest = url.strip_prefix("file://")?;
    // `file:///path` (empty authority) is the only form we accept; a host would name
    // another machine, and we cannot open a file there.
    let rest = if rest.starts_with('/') { rest } else { return None };

    let (path, fragment) = match rest.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (rest, None),
    };
    let line = fragment
        .map(|f| f.trim_start_matches(['L', 'l']))
        .and_then(|f| f.parse::<u32>().ok())
        .filter(|n| *n > 0);

    Some((std::path::PathBuf::from(percent_decode(path)), line))
}

/// Minimal `%XX` decoder — enough for a filesystem path in a URL, and not worth a
/// dependency. A malformed escape is left verbatim rather than dropped, so a literal
/// `%` in a filename survives a round trip.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Read the two hex digits as BYTES, never as `&s[i+1..i+3]`: slicing a
            // `&str` by byte offset panics when the range cuts a multi-byte char, and
            // `%aé` is a URL a quark can write (invariants: Char Boundary Safety).
            let hex = [bytes[i + 1], bytes[i + 2]];
            if let Some(byte) = std::str::from_utf8(&hex)
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The program and arguments that open `path` (optionally at `line`) in `choice`.
///
/// `None` means **"no opinion — use the platform opener"**, which is exactly what
/// `EditorChoice::System` wants and what a caller already has code for. Every editor
/// spells "at line N" differently, so folding the line into the args here is the
/// reason this function exists rather than a bare program name.
pub fn editor_argv(choice: &EditorChoice, path: &Path, line: Option<u32>) -> Option<(String, Vec<String>)> {
    let file = path.to_string_lossy().to_string();
    let at_line = |sep: &str| match line {
        Some(n) => format!("{file}{sep}{n}"),
        None => file.clone(),
    };

    let (program, args) = match choice {
        EditorChoice::System => return None,
        EditorChoice::Zed => ("zed", vec![at_line(":")]),
        EditorChoice::VsCode => ("code", vec!["--goto".to_string(), at_line(":")]),
        EditorChoice::Cursor => ("cursor", vec!["--goto".to_string(), at_line(":")]),
        EditorChoice::Sublime => ("subl", vec![at_line(":")]),
        EditorChoice::Custom(cmd) => {
            let mut parts = cmd.split_whitespace().map(str::to_string).collect::<Vec<_>>();
            let program = if parts.is_empty() { return None } else { parts.remove(0) };
            let had_file = parts.iter().any(|p| p.contains("{file}"));
            let line_text = line.map(|n| n.to_string()).unwrap_or_default();
            for part in &mut parts {
                *part = part.replace("{file}", &file).replace("{line}", &line_text);
            }
            if !had_file {
                parts.push(file);
            }
            return Some((program, parts));
        }
    };
    Some((program.to_string(), args))
}

/// Returns true if running inside Windows Subsystem for Linux (WSL).
pub fn is_wsl() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("WSL_DISTRO_NAME").is_ok()
            || std::fs::read_to_string("/proc/version")
                .map(|v| v.to_lowercase().contains("microsoft") || v.contains("WSL"))
                .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Write text to the system clipboard across all supported platforms (WSL, Wayland, X11, macOS, Windows).
///
/// In addition to GPUI's internal clipboard, this bridges to native OS clipboard utilities
/// so that copied text is immediately accessible in external applications (host Windows clipboard in WSL,
/// Wayland/wl-copy, X11/xclip/xsel, macOS pbcopy).
pub fn copy_to_system_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    let text = text.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            if is_wsl() {
                // Under WSL, copy directly to the Windows host clipboard via clip.exe or PowerShell
                let mut copied = false;
                for prog in ["clip.exe", "/mnt/c/WINDOWS/system32/clip.exe"] {
                    if let Ok(mut child) = Command::new(prog)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        if let Some(mut stdin) = child.stdin.take() {
                            use std::io::Write;
                            let _ = stdin.write_all(text.as_bytes());
                            drop(stdin);
                        }
                        if child.wait().map(|s| s.success()).unwrap_or(false) {
                            copied = true;
                            break;
                        }
                    }
                }
                if !copied {
                    if let Ok(mut child) = Command::new("powershell.exe")
                        .args(["-NoProfile", "-Command", "$Input | Set-Clipboard"])
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        if let Some(mut stdin) = child.stdin.take() {
                            use std::io::Write;
                            let _ = stdin.write_all(text.as_bytes());
                            drop(stdin);
                        }
                        let _ = child.wait();
                    }
                }
            }

            // Also populate Wayland clipboard if Wayland is active and wl-copy is present
            if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("WAYLAND_SOCKET").is_some() {
                if let Ok(mut child) = Command::new("wl-copy")
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(text.as_bytes());
                        drop(stdin);
                    }
                    let _ = child.wait();
                }
            }

            // Also populate X11 clipboard if X11 is active (and not WSL)
            if std::env::var_os("DISPLAY").is_some() && !is_wsl() {
                for cmd in ["xclip", "xsel"] {
                    let mut cmd_builder = Command::new(cmd);
                    if cmd == "xclip" {
                        cmd_builder.args(["-selection", "clipboard", "-i"]);
                    } else {
                        cmd_builder.args(["-b", "-i"]);
                    }
                    if let Ok(mut child) = cmd_builder
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        if let Some(mut stdin) = child.stdin.take() {
                            use std::io::Write;
                            let _ = stdin.write_all(text.as_bytes());
                            drop(stdin);
                        }
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(mut child) = Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(text.as_bytes());
                    drop(stdin);
                }
                let _ = child.wait();
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(mut child) = Command::new("clip.exe")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(text.as_bytes());
                    drop(stdin);
                }
                let _ = child.wait();
            }
        }
    });
}

/// Opens a file path or URL using the platform's default viewer or browser.
///
/// Under WSL:
/// - Translates Linux file paths to Windows UNC paths via `wslpath -w` and dispatches
///   cleanly to `powershell.exe -NoProfile -Command "Start-Process '<winpath>'"` with null
///   standard I/O to avoid missing WSLENV environment errors or snapd stderr pollution.
/// - Web URLs (`http://`, `https://`) are dispatched to Windows PowerShell or `wslview`.
///
/// Under Native Linux:
/// - Spawns `xdg-open` with null standard I/O.
///
/// Under macOS:
/// - Spawns `open` with null standard I/O.
///
/// Under Windows:
/// - Spawns `explorer` or `powershell.exe Start-Process`.
pub fn open_path_or_url(target: &str) {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return;
    }

    #[cfg(target_os = "linux")]
    {
        if is_wsl() {
            let file_target = if let Some(stripped) = trimmed.strip_prefix("file://") {
                stripped
            } else {
                trimmed
            };

            // If it's a web URL (http:// or https://), launch via Windows PowerShell or wslview
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                let escaped = trimmed.replace('\'', "''");
                if let Ok(mut child) = Command::new("powershell.exe")
                    .args(["-NoProfile", "-Command", &format!("Start-Process '{escaped}'")])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    let _ = child.wait();
                    return;
                }
            } else {
                // For local files or folders, translate Linux path to Windows UNC path
                let win_path = Command::new("wslpath")
                    .arg("-w")
                    .arg(file_target)
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|s| !s.is_empty());

                if let Some(wp) = win_path {
                    let escaped_wp = wp.replace('\'', "''");
                    if let Ok(mut child) = Command::new("powershell.exe")
                        .args(["-NoProfile", "-Command", &format!("Start-Process '{escaped_wp}'")])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        let _ = child.wait();
                        return;
                    }
                }
            }

            // Fallback to wslview if available
            if let Ok(mut child) = Command::new("wslview")
                .arg(file_target)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                let _ = child.wait();
                return;
            }
        }

        // Native Linux fallback (or when WSL PowerShell is unavailable)
        let _ = Command::new("xdg-open")
            .arg(trimmed)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .arg(trimmed)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer")
            .arg(trimmed)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(windows)]
pub fn init_windows_app_icon() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, LoadIconW, SendMessageW, SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM,
        ICON_BIG, ICON_SMALL, WM_SETICON,
    };

    static INITIALIZED: AtomicBool = AtomicBool::new(false);
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }

    term::info(Source::Chamber, "windows: initializing AppUserModelID...");

    let app_id: Vec<u16> = OsStr::new("Hadron.Chamber")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let res = windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
        if res == 0 {
            term::info(Source::Chamber, "windows: set AppUserModelID to 'Hadron.Chamber'");
        } else {
            term::error(Source::Chamber, &format!("windows: SetCurrentProcessExplicitAppUserModelID failed with HRESULT: {res:#x}"));
        }

        let h_instance = GetModuleHandleW(std::ptr::null());
        let h_icon = LoadIconW(h_instance, 1 as _); // 1 = IDI_APPLICATION from resource
        if !h_icon.is_null() {
            unsafe extern "system" fn enum_win(hwnd: HWND, lparam: isize) -> i32 {
                let h_icon = lparam as windows_sys::Win32::UI::WindowsAndMessaging::HICON;
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as _, h_icon as _);
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as _, h_icon as _);
                SetClassLongPtrW(hwnd, GCLP_HICON, h_icon as _);
                SetClassLongPtrW(hwnd, GCLP_HICONSM, h_icon as _);
                1
            }
            let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId();
            EnumThreadWindows(tid, Some(enum_win), h_icon as isize);
        }
    }
}



#[cfg(not(windows))]
pub fn init_windows_app_icon() {}

/// Send a native desktop OS notification asynchronously (Linux notify-send, WSL PowerShell toast, macOS osascript, Windows PowerShell).
pub fn send_desktop_notification(title: &str, body: &str, _sound: bool) {
    if title.is_empty() && body.is_empty() {
        return;
    }
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            if is_wsl() {
                // WSL: send via PowerShell toast on Windows host
                let ps_script = format!(
                    "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                     $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                     $textNodes = $template.GetElementsByTagName('text'); \
                     $textNodes.Item(0).AppendChild($template.CreateTextNode('{}')) > $null; \
                     $textNodes.Item(1).AppendChild($template.CreateTextNode('{}')) > $null; \
                     $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
                     [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Hadron Swarm').Show($toast);",
                    title.replace('\'', "''"),
                    body.replace('\'', "''")
                );
                if let Ok(mut child) = Command::new("powershell.exe")
                    .args(["-NoProfile", "-Command", &ps_script])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    let _ = child.wait();
                }
            }

            // Also send via notify-send if present (Linux native desktop / X11 / Wayland)
            if let Ok(mut child) = Command::new("notify-send")
                .args(["-a", "Hadron", &title, &body])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                let _ = child.wait();
            }
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                body.replace('"', "\\\""),
                title.replace('"', "\\\"")
            );
            if let Ok(mut child) = Command::new("osascript")
                .args(["-e", &script])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                let _ = child.wait();
            }
        }

        #[cfg(target_os = "windows")]
        {
            let ps_script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $textNodes = $template.GetElementsByTagName('text'); \
                 $textNodes.Item(0).AppendChild($template.CreateTextNode('{}')) > $null; \
                 $textNodes.Item(1).AppendChild($template.CreateTextNode('{}')) > $null; \
                 $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Hadron Swarm').Show($toast);",
                title.replace('\'', "''"),
                body.replace('\'', "''")
            );
            if let Ok(mut child) = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &ps_script])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                let _ = child.wait();
            }
        }
    });
}

/// Plays a synthesized audio tone asynchronously using platform audio backends (WSL PowerShell Beep, Linux terminal bell/beep, macOS osascript, Windows Console Beep).
pub fn play_audio_tone(freq_hz: u32, duration_ms: u32, volume: f32) {
    if volume <= 0.0 || freq_hz == 0 || duration_ms == 0 {
        return;
    }
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            if is_wsl() {
                let script = format!("[Console]::Beep({}, {})", freq_hz.clamp(37, 32767), duration_ms.clamp(10, 5000));
                if let Ok(mut child) = Command::new("powershell.exe")
                    .args(["-NoProfile", "-Command", &script])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    let _ = child.wait();
                }
            } else {
                let script = format!("beep -f {} -l {} 2>/dev/null || printf '\\a'", freq_hz, duration_ms);
                let _ = Command::new("sh")
                    .args(["-c", &script])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }

        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("osascript")
                .args(["-e", "beep"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }

        #[cfg(target_os = "windows")]
        {
            let script = format!("[Console]::Beep({}, {})", freq_hz.clamp(37, 32767), duration_ms.clamp(10, 5000));
            if let Ok(mut child) = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &script])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                let _ = child.wait();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // -- file_url_target: what a clicked link actually carries --

    /// The exact shape a quark posts into the field.
    #[test]
    fn a_file_url_with_a_line_fragment_yields_both() {
        let (path, line) = file_url_target("file:///home/j/src/app/mod.rs#L332").unwrap();
        assert_eq!(path, Path::new("/home/j/src/app/mod.rs"));
        assert_eq!(line, Some(332));
    }

    #[test]
    fn a_file_url_without_a_fragment_has_no_line() {
        let (path, line) = file_url_target("file:///tmp/a.rs").unwrap();
        assert_eq!(path, Path::new("/tmp/a.rs"));
        assert_eq!(line, None);
    }

    #[test]
    fn a_bare_number_fragment_is_a_line_too() {
        assert_eq!(file_url_target("file:///tmp/a.rs#12").unwrap().1, Some(12));
    }

    /// A `#section-heading` anchor is not a line, and must not become one.
    #[test]
    fn a_non_numeric_fragment_is_not_a_line() {
        assert_eq!(file_url_target("file:///tmp/a.md#install").unwrap().1, None);
    }

    /// A malformed escape whose "hex digits" are half a multi-byte char. Slicing the
    /// `&str` here panics; reading the bytes does not. The assertion is secondary —
    /// that this returns at all is the point.
    #[test]
    fn a_malformed_escape_across_a_char_boundary_does_not_panic() {
        let (path, _) = file_url_target("file:///tmp/%aé.rs").unwrap();
        assert_eq!(path, Path::new("/tmp/%aé.rs"));
    }

    #[test]
    fn a_percent_encoded_path_is_decoded() {
        let (path, _) = file_url_target("file:///tmp/my%20notes.md").unwrap();
        assert_eq!(path, Path::new("/tmp/my notes.md"));
    }

    /// The hard requirement: a web link must fall through to the browser, never
    /// reach a code editor.
    #[test]
    fn a_web_url_is_not_a_file_target() {
        assert!(file_url_target("https://example.com/a.rs").is_none());
        assert!(file_url_target("http://example.com").is_none());
        assert!(file_url_target("mailto:a@b.c").is_none());
        // A host component names another machine — we cannot open its files.
        assert!(file_url_target("file://otherbox/tmp/a.rs").is_none());
    }

    // -- editor_argv: each editor spells "at line N" its own way --

    #[test]
    fn system_has_no_opinion_so_the_platform_opener_stays() {
        assert_eq!(editor_argv(&EditorChoice::System, Path::new("/tmp/a.rs"), Some(9)), None);
    }

    #[test]
    fn zed_takes_a_colon_suffix() {
        let (prog, args) = editor_argv(&EditorChoice::Zed, Path::new("/tmp/a.rs"), Some(332)).unwrap();
        assert_eq!(prog, "zed");
        assert_eq!(args, vec!["/tmp/a.rs:332"]);
    }

    #[test]
    fn vscode_and_cursor_need_the_goto_flag() {
        let (prog, args) = editor_argv(&EditorChoice::VsCode, Path::new("/tmp/a.rs"), Some(7)).unwrap();
        assert_eq!((prog.as_str(), args), ("code", vec!["--goto".to_string(), "/tmp/a.rs:7".to_string()]));
        let (prog, args) = editor_argv(&EditorChoice::Cursor, Path::new("/tmp/a.rs"), None).unwrap();
        assert_eq!((prog.as_str(), args), ("cursor", vec!["--goto".to_string(), "/tmp/a.rs".to_string()]));
    }

    /// No line means no `:N` suffix — `zed /tmp/a.rs:` would be a path that does not exist.
    #[test]
    fn no_line_means_no_suffix() {
        let (_, args) = editor_argv(&EditorChoice::Zed, Path::new("/tmp/a.rs"), None).unwrap();
        assert_eq!(args, vec!["/tmp/a.rs"]);
    }

    #[test]
    fn a_custom_command_substitutes_placeholders() {
        let choice = EditorChoice::Custom("kate -l {line} {file}".to_string());
        let (prog, args) = editor_argv(&choice, Path::new("/tmp/a.rs"), Some(4)).unwrap();
        assert_eq!((prog.as_str(), args), ("kate", vec!["-l".to_string(), "4".to_string(), "/tmp/a.rs".to_string()]));
    }

    /// A custom command that never mentions `{file}` still gets the path — otherwise
    /// it would open the editor on nothing.
    #[test]
    fn a_custom_command_without_a_placeholder_appends_the_path() {
        let choice = EditorChoice::Custom("emacs".to_string());
        let (prog, args) = editor_argv(&choice, Path::new("/tmp/a.rs"), Some(4)).unwrap();
        assert_eq!((prog.as_str(), args), ("emacs", vec!["/tmp/a.rs".to_string()]));
    }

    #[test]
    fn a_blank_custom_command_falls_back_to_the_platform() {
        assert_eq!(editor_argv(&EditorChoice::Custom("   ".into()), Path::new("/tmp/a.rs"), None), None);
    }

    /// The picker's ladder and the labels are one decision, not two.
    #[test]
    fn every_offered_choice_has_a_label() {
        let labels: Vec<&str> = EDITOR_LADDER.iter().map(|c| c.label()).collect();
        assert_eq!(labels, vec!["System default", "Zed", "VS Code", "Cursor", "Sublime Text"]);
        assert_eq!(EditorChoice::default(), EditorChoice::System);
    }

    #[test]
    fn file_tree_listing_and_opening_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap();

        let test_file = "test.txt";
        fs::write(root.join(test_file), "hello world").unwrap();

        Command::new("git")
            .args(["add", test_file])
            .current_dir(root)
            .output()
            .unwrap();

        let files = list_workspace_files(root, &std::collections::HashSet::new());
        assert_eq!(files, vec![("test.txt".to_string(), false)]);

        let content = read_workspace_file(root, "test.txt");
        assert_eq!(content, Some("hello world".to_string()));
    }

    /// The file tree is a view of the **disk**, not of git's index. Jake deleted
    /// two screenshots and added five; the tree kept showing the deleted ones and
    /// never showed the new ones, because `git ls-files` reports the index.
    ///
    /// Gitignored files are now surfaced too (rendered muted), flagged `is_ignored = true`,
    /// so this also asserts they appear with the right flag rather than being dropped.
    #[test]
    fn the_file_tree_shows_the_disk_not_the_index() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap();

        // Tracked and staged, then deleted from the working tree: still in the
        // index, gone from disk — it must NOT be listed.
        fs::write(root.join("deleted.png"), "old").unwrap();
        Command::new("git")
            .args(["add", "deleted.png"])
            .current_dir(root)
            .output()
            .unwrap();
        fs::remove_file(root.join("deleted.png")).unwrap();

        // Never added to git: it must be listed anyway.
        fs::write(root.join("brand-new.png"), "new").unwrap();

        // Ignored: it is now surfaced (muted), flagged is_ignored = true.
        fs::write(root.join(".gitignore"), "ignored.tmp\n").unwrap();
        fs::write(root.join("ignored.tmp"), "noise").unwrap();

        let files = list_workspace_files(root, &std::collections::HashSet::new());
        assert!(
            files.contains(&("brand-new.png".to_string(), false)),
            "an untracked file on disk must appear in the tree (not ignored), got {files:?}"
        );
        assert!(
            !files.iter().any(|(p, _)| p == "deleted.png"),
            "a file deleted from disk must not linger because it is still in the index, got {files:?}"
        );
        assert!(
            files.contains(&("ignored.tmp".to_string(), true)),
            "a gitignored file must now appear flagged as ignored, got {files:?}"
        );
    }

    #[test]
    fn gitignored_non_heavy_directories_are_automatically_expanded() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap();

        fs::write(root.join(".gitignore"), ".hadron/\ntarget/\n").unwrap();
        fs::create_dir_all(root.join(".hadron/docs/plans")).unwrap();
        fs::write(root.join(".hadron/docs/plans/master.md"), "# Master Plan").unwrap();
        fs::create_dir_all(root.join(".hadron/trees/peer1")).unwrap();
        fs::write(root.join(".hadron/trees/peer1/scratch.txt"), "heavy tree file").unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/app.bin"), "binary").unwrap();

        let files = list_workspace_files(root, &std::collections::HashSet::new());
        // .hadron/docs/plans/master.md is indexed automatically
        assert!(
            files.contains(&(".hadron/docs/plans/master.md".to_string(), true)),
            "internal plans in gitignored non-heavy dirs must be discoverable: {files:?}"
        );
        // Heavy dirs (target/ and .hadron/trees/) remain collapsed
        assert!(
            files.contains(&("target/".to_string(), true)),
            "target/ must remain collapsed: {files:?}"
        );
        assert!(
            !files.contains(&("target/debug/app.bin".to_string(), true)),
            "target inner files must not be traversed: {files:?}"
        );
        assert!(
            files.contains(&(".hadron/trees/".to_string(), true)),
            ".hadron/trees/ must remain collapsed: {files:?}"
        );
        assert!(
            !files.contains(&(".hadron/trees/peer1/scratch.txt".to_string(), true)),
            "heavy subtrees must not be traversed: {files:?}"
        );
    }

    #[test]
    fn insert_builds_folder_nodes_with_children_and_full_paths() {
        // The user's case: an untracked file arrives as its full path (git lists
        // untracked files individually, not collapsed). The folder node MUST exist
        // and MUST carry the child — otherwise expanding it shows nothing.
        let mut root = FileTreeNode::default();
        root.insert("newdir/newfile.txt", false, false);

        let dir = root
            .children
            .get("newdir")
            .expect("intermediate folder node must be created");
        assert!(!dir.is_file, "a folder node is not a file");
        assert_eq!(dir.full_path, "newdir", "folder full_path is its running path");

        let file = dir
            .children
            .get("newfile.txt")
            .expect("the folder must carry its child — this is what expand renders");
        assert!(file.is_file);
        assert_eq!(file.full_path, "newdir/newfile.txt");
    }

    #[test]
    fn every_folder_node_gets_a_distinct_nonempty_full_path() {
        // Regression guard for the id-collision bug: a leaf-only full_path left
        // every folder row sharing the gpui id `tree-row-`.
        let mut root = FileTreeNode::default();
        root.insert("src/app/render.rs", false, false);
        root.insert("crates/lib.rs", false, false);

        let src = &root.children["src"];
        let app = &src.children["app"];
        let crates = &root.children["crates"];
        assert_eq!(src.full_path, "src");
        assert_eq!(app.full_path, "src/app");
        assert_eq!(crates.full_path, "crates");
        // Distinct, non-empty ids for every folder — the collision is impossible now.
        for p in [&src.full_path, &app.full_path, &crates.full_path] {
            assert!(!p.is_empty());
        }
        assert_ne!(src.full_path, crates.full_path);
    }

    #[test]
    fn collapsed_ignored_dir_is_a_childless_leaf() {
        // A wholly-ignored dir arrives collapsed with a trailing slash (is_dir_leaf).
        // It is a folder with no children — correctly nothing to expand.
        let mut root = FileTreeNode::default();
        root.insert("target/", true, true);
        let node = &root.children["target"];
        assert!(!node.is_file);
        assert!(node.is_ignored);
        assert!(node.children.is_empty(), "collapsed dir has no children");
        assert_eq!(node.full_path, "target");
    }

    #[test]
    fn resolve_ignores_marks_a_folder_ignored_only_when_all_children_are() {
        let mut root = FileTreeNode::default();
        root.insert("mixed/tracked.rs", false, false);
        root.insert("mixed/ignored.tmp", true, false);
        root.insert("allignored/a.tmp", true, false);
        root.resolve_ignores();

        assert!(
            !root.children["mixed"].is_ignored,
            "a folder with one tracked child is not ignored"
        );
        assert!(
            root.children["allignored"].is_ignored,
            "a folder whose every child is ignored folds to ignored"
        );
    }

    #[test]
    fn sorted_children_puts_folders_before_files_then_sorts_by_name() {
        let mut root = FileTreeNode::default();
        root.insert("zeta.rs", false, false); // file
        root.insert("alpha/a.rs", false, false); // folder
        root.insert("beta.rs", false, false); // file
        let order: Vec<&str> = sorted_children(&root)
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(order, vec!["alpha", "beta.rs", "zeta.rs"]);
    }

    #[test]
    fn read_workspace_file_prevents_directory_traversal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let parent = root.parent().unwrap();
        let sibling = parent.join("sibling_dir");
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("secret.txt"), "secret data").unwrap();

        // Try reading a file that is outside repo_root using traversal
        let content = read_workspace_file(root, "../sibling_dir/secret.txt");
        assert_eq!(content, None);
    }

    #[test]
    fn read_workspace_file_gitignored_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let target_dir = root.join("target").join("debug");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("test.d"), "ignored file content").unwrap();

        let content = read_workspace_file(root, "target/debug/test.d");
        assert_eq!(content, Some("ignored file content".to_string()));
    }

    #[test]
    fn read_workspace_file_relative_root() {
        let content = read_workspace_file(Path::new("."), "Cargo.toml");
        assert!(content.is_some());
        assert!(content.unwrap().contains("hadron"));
    }

    #[test]
    fn format_file_preview_handles_markdown_and_code_extensions() {
        // Markdown returns verbatim
        assert_eq!(
            format_file_preview("doc.md", "# Hello\nWorld"),
            "# Hello\nWorld"
        );

        // Rust extension wraps in ```rust
        assert_eq!(
            format_file_preview("src/main.rs", "fn main() {}"),
            "```rust\nfn main() {}\n```"
        );

        // CPP extension wraps in ```cpp
        assert_eq!(
            format_file_preview("main.cpp", "int main() {}"),
            "```cpp\nint main() {}\n```"
        );

        // CSS extension wraps in ```css
        assert_eq!(
            format_file_preview("style.css", "body { margin: 0; }"),
            "```css\nbody { margin: 0; }\n```"
        );

        // Mermaid extension wraps in ```mermaid
        assert_eq!(
            format_file_preview("diagram.mermaid", "graph TD\nA --> B"),
            "```mermaid\ngraph TD\nA --> B\n```"
        );
        assert_eq!(
            format_file_preview("flow.mmd", "graph LR\nA --> B"),
            "```mermaid\ngraph LR\nA --> B\n```"
        );

        // Backtick collision expands fence length
        let code_with_backticks = "/// ```\n/// example\n/// ```";
        assert_eq!(
            format_file_preview("lib.rs", code_with_backticks),
            "````rust\n/// ```\n/// example\n/// ```\n````"
        );
    }

    #[test]
    fn test_send_desktop_notification_no_panic() {
        send_desktop_notification("", "", true);
        send_desktop_notification("Hadron Swarm", "Test Notification", true);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    #[test]
    fn test_is_wsl_and_open_path_or_url_no_panic() {
        let _ = is_wsl();
        // Empty target should early-return without doing anything
        open_path_or_url("");
        open_path_or_url("   ");
    }

    #[test]
    fn test_copy_to_system_clipboard_handles_empty_and_text() {
        copy_to_system_clipboard("");
        copy_to_system_clipboard("test clipboard text");
        copy_to_system_clipboard("Unexposed Adjustable Settings\n1. Audio & Haptics 👥\n2. Swarm Watchdog 🧠");
        // Give background thread a short tick
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    #[test]
    fn test_play_audio_tone_no_panic() {
        play_audio_tone(0, 0, 0.0);
        play_audio_tone(880, 50, 0.5);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}


