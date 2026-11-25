use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

pub struct AstBlastRadiusAnalyzer {
    symbol_to_tests: HashMap<String, HashSet<String>>,
}

impl AstBlastRadiusAnalyzer {
    pub fn new() -> Self {
        Self {
            symbol_to_tests: HashMap::new(),
        }
    }

    pub fn register_caller(&mut self, test_name: &str, symbol_name: &str) {
        self.symbol_to_tests
            .entry(symbol_name.to_string())
            .or_default()
            .insert(test_name.to_string());
    }

    pub fn find_impacted_tests(&self, changed_symbols: &[&str]) -> Vec<String> {
        let mut res = HashSet::new();
        for sym in changed_symbols {
            if let Some(tests) = self.symbol_to_tests.get(*sym) {
                res.extend(tests.clone());
            }
        }
        let mut list: Vec<String> = res.into_iter().collect();
        list.sort();
        list
    }

    pub fn find_affected_crates(changed_files: &[&str]) -> AffectedCratesResult {
        let mut crates = HashSet::new();
        for file in changed_files {
            let normalized = file.trim_start_matches("./");
            if normalized.starts_with(".hadron/")
                || normalized.starts_with("docs/")
                || normalized.ends_with(".md")
                || normalized.ends_with(".txt")
            {
                continue;
            }
            if normalized == "Cargo.toml"
                || normalized == "Cargo.lock"
                || normalized.starts_with(".cargo")
            {
                return AffectedCratesResult::WorkspaceWide(format!(
                    "Root build file changed: {normalized}"
                ));
            }
            if let Some(rest) = normalized.strip_prefix("crates/") {
                if let Some(crate_name) = rest.split('/').next() {
                    // crates/hadron-chamber declares package "hadron" in Cargo.toml
                    let pkg_name = if crate_name == "hadron-chamber" {
                        "hadron"
                    } else {
                        crate_name
                    };
                    crates.insert(pkg_name.to_string());
                    continue;
                }
            }
            // Any change outside crates/ (or top-level file) triggers full workspace test
            return AffectedCratesResult::WorkspaceWide(format!(
                "Non-crate file changed: {normalized}"
            ));
        }
        if crates.is_empty() {
            AffectedCratesResult::WorkspaceWide("No specific crate changes detected".to_string())
        } else {
            let mut list: Vec<String> = crates.into_iter().collect();
            list.sort();
            AffectedCratesResult::Specific(list)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AffectedCratesResult {
    Specific(Vec<String>),
    WorkspaceWide(String),
}

/// One workspace member as read from its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    /// Workspace-relative directory, `/`-separated, no trailing slash.
    dir: String,
    package: String,
    /// Package names of other members this one depends on (any dependency kind/target).
    deps: BTreeSet<String>,
}

/// Packages whose tests a change can affect: every directly modified workspace member
/// plus everything that transitively depends on it, read from the `Cargo.toml` files
/// in memory (no `cargo` subprocess). Fails closed to `WorkspaceWide` on any manifest,
/// lockfile, `.cargo/`, `.github/` or non-member code change, on anything it cannot
/// parse, and when the closure is the whole workspace anyway.
///
/// Files inside a member dir count for that member whatever their extension (a
/// `.md` may be `include_str!`ed); docs are only skipped outside every member.
pub fn resolve_affected_workspace_crates(workspace_root: &Path, changed_files: &[&str]) -> AffectedCratesResult {
    let members = match read_workspace_members(workspace_root) {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => return AffectedCratesResult::WorkspaceWide("No workspace members found".into()),
        Err(e) => return AffectedCratesResult::WorkspaceWide(format!("Workspace manifest unreadable: {e}")),
    };

    let mut direct = BTreeSet::new();
    for file in changed_files {
        let path = file.trim().replace('\\', "/");
        let path = path.trim_start_matches("./");
        let name = path.rsplit('/').next().unwrap_or(path);
        if name == "Cargo.toml" || name == "Cargo.lock" || path.starts_with(".cargo/") || path.starts_with(".github/") {
            return AffectedCratesResult::WorkspaceWide(format!("Build/CI file changed: {path}"));
        }
        let owner = members
            .iter()
            .filter(|m| path.starts_with(&format!("{}/", m.dir)))
            .max_by_key(|m| m.dir.len());
        match owner {
            Some(m) => {
                direct.insert(m.package.clone());
            }
            None if path.starts_with(".hadron/")
                || path.starts_with("docs/")
                || path.ends_with(".md")
                || path.ends_with(".txt") => {}
            None => return AffectedCratesResult::WorkspaceWide(format!("Non-member file changed: {path}")),
        }
    }
    if direct.is_empty() {
        return AffectedCratesResult::WorkspaceWide("No specific crate changes detected".into());
    }

    let mut closure = direct;
    let mut queue: Vec<String> = closure.iter().cloned().collect();
    while let Some(changed) = queue.pop() {
        for m in members.iter().filter(|m| m.deps.contains(&changed)) {
            if closure.insert(m.package.clone()) {
                queue.push(m.package.clone());
            }
        }
    }
    if closure.len() == members.len() {
        return AffectedCratesResult::WorkspaceWide("Change reaches every workspace member".into());
    }
    AffectedCratesResult::Specific(closure.into_iter().collect())
}

/// `(member dir, package name)` for every workspace member, or empty when the
/// manifests cannot be read — for callers that only use it to refine a heuristic.
pub(crate) fn member_packages_by_dir(root: &Path) -> Vec<(String, String)> {
    read_workspace_members(root)
        .map(|ms| ms.into_iter().map(|m| (m.dir, m.package)).collect())
        .unwrap_or_default()
}

fn read_workspace_members(root: &Path) -> Result<Vec<Member>, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let root_manifest = read(&root.join("Cargo.toml"))?;
    let dirs = workspace_member_dirs(&root_manifest)?;

    // (dir, package, raw dependency refs) — refs resolve to packages once all are known.
    let mut raw = Vec::new();
    for dir in dirs {
        let manifest = read(&root.join(&dir).join("Cargo.toml"))?;
        let package = package_name(&manifest).ok_or_else(|| format!("{dir}/Cargo.toml has no [package] name"))?;
        raw.push((dir.clone(), package, dependency_refs(&dir, &manifest)));
    }
    let by_dir: BTreeMap<&str, &str> = raw.iter().map(|(d, p, _)| (d.as_str(), p.as_str())).collect();
    let packages: BTreeSet<&str> = raw.iter().map(|(_, p, _)| p.as_str()).collect();
    Ok(raw
        .iter()
        .map(|(dir, package, refs)| Member {
            dir: dir.clone(),
            package: package.clone(),
            deps: refs
                .iter()
                .filter_map(|r| match r {
                    DepRef::Name(n) => packages.get(n.as_str()).copied(),
                    DepRef::Path(p) => by_dir.get(p.as_str()).copied(),
                })
                .filter(|p| p != package)
                .map(str::to_string)
                .collect(),
        })
        .collect())
}

/// Strip a trailing `# comment` that is not inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn quoted_strings(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

fn workspace_member_dirs(manifest: &str) -> Result<Vec<String>, String> {
    let mut section = String::new();
    let mut collecting: Option<String> = None;
    for line in manifest.lines().map(strip_comment) {
        let t = line.trim();
        if let Some(buf) = collecting.as_mut() {
            buf.push_str(t);
            if t.contains(']') {
                break;
            }
            continue;
        }
        if t.starts_with('[') {
            section = t.to_string();
        } else if section == "[workspace]" {
            if let Some(value) = t.strip_prefix("members").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
                collecting = Some(value.trim().to_string());
                if value.contains(']') {
                    break;
                }
            }
        }
    }
    let list = collecting.ok_or("no [workspace] members")?;
    if !list.trim_end().ends_with(']') {
        return Err("unterminated [workspace] members".into());
    }
    let dirs = quoted_strings(&list);
    if let Some(glob) = dirs.iter().find(|d| d.contains(['*', '?', '['])) {
        return Err(format!("glob member {glob:?} is not resolved without cargo"));
    }
    Ok(dirs.into_iter().map(|d| d.trim_start_matches("./").trim_end_matches('/').to_string()).collect())
}

fn package_name(manifest: &str) -> Option<String> {
    let mut section = "";
    for line in manifest.lines().map(strip_comment) {
        let t = line.trim();
        if t.starts_with('[') {
            section = t;
        } else if section == "[package]" {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == "name" {
                    return quoted_strings(v).into_iter().next();
                }
            }
        }
    }
    None
}

enum DepRef {
    Name(String),
    /// Workspace-relative dir of a `path = "..."` dependency.
    Path(String),
}

/// Every dependency named in `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]` (incl. `[target.*.…]` and `[dependencies.<name>]` forms),
/// as both its key and, when present, its resolved `path`.
fn dependency_refs(member_dir: &str, manifest: &str) -> Vec<DepRef> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut refs = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines().map(strip_comment) {
        let t = line.trim();
        if t.starts_with('[') {
            let header = t.trim_matches(|c| c == '[' || c == ']');
            let segments: Vec<&str> = header.split('.').collect();
            in_deps = segments.iter().any(|s| KINDS.contains(s));
            // `[dependencies.hadron-lattice]` names the dependency in the header.
            if let Some(pos) = segments.iter().rposition(|s| KINDS.contains(s)) {
                if let Some(name) = segments.get(pos + 1) {
                    refs.push(DepRef::Name(name.trim_matches('"').to_string()));
                }
            }
            continue;
        }
        if !in_deps {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else { continue };
        let key = key.trim().trim_matches('"');
        if key == "path" {
            refs.extend(quoted_strings(value).first().map(|p| DepRef::Path(join_rel(member_dir, p))));
            continue;
        }
        refs.push(DepRef::Name(key.to_string()));
        if let Some(pkg) = inline_value(value, "package") {
            refs.push(DepRef::Name(pkg));
        }
        if let Some(path) = inline_value(value, "path") {
            refs.push(DepRef::Path(join_rel(member_dir, &path)));
        }
    }
    refs
}

/// `key = "v"` inside an inline table `{ ..., key = "v", ... }`.
fn inline_value(table: &str, key: &str) -> Option<String> {
    table.trim().strip_prefix('{')?.split(',').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == key).then(|| quoted_strings(v).into_iter().next()).flatten()
    })
}

/// Lexically resolve `rel` against `base` (both `/`-separated, workspace-relative).
fn join_rel(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    let rel = rel.replace('\\', "/");
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blast_radius_slicing() {
        let mut analyzer = AstBlastRadiusAnalyzer::new();
        analyzer.register_caller("test_port_mesh", "PortMesh::allocate");
        analyzer.register_caller("test_vcr_record", "VcrTape::record");

        let impacted = analyzer.find_impacted_tests(&["PortMesh::allocate"]);
        assert_eq!(impacted, vec!["test_port_mesh".to_string()]);
    }

    #[test]
    fn test_affected_crates_detection() {
        let changed_single = vec!["crates/hadron-chamber/src/main.rs"];
        let res = AstBlastRadiusAnalyzer::find_affected_crates(&changed_single);
        assert_eq!(
            res,
            AffectedCratesResult::Specific(vec!["hadron".to_string()])
        );

        let changed_forge = vec!["crates/hadron-forge/src/lib.rs"];
        let res_forge = AstBlastRadiusAnalyzer::find_affected_crates(&changed_forge);
        assert_eq!(
            res_forge,
            AffectedCratesResult::Specific(vec!["hadron-forge".to_string()])
        );

        let changed_root = vec!["Cargo.toml", "crates/hadron-chamber/src/main.rs"];
        let res_root = AstBlastRadiusAnalyzer::find_affected_crates(&changed_root);
        assert!(matches!(res_root, AffectedCratesResult::WorkspaceWide(_)));
    }

    /// core ← mid ← app (package "app-pkg" in dir `crates/app`), plus an unrelated `leaf`.
    fn fixture_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let w = |rel: &str, body: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w(
            "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\n  \"crates/core\", # the root\n  \"crates/mid\",\n  \"crates/app\",\n  \"crates/leaf\",\n]\ndefault-members = [\"crates/app\"]\n",
        );
        w("crates/core/Cargo.toml", "[package]\nname = \"core\"\n\n[dependencies]\nserde = \"1\"\n");
        w(
            "crates/mid/Cargo.toml",
            "[package]\nname = \"mid\"\n\n[dependencies.core]\npath = \"../core\"\n",
        );
        w(
            "crates/app/Cargo.toml",
            "[package]\nname = \"app-pkg\"\n\n[[bin]]\nname = \"core\"\n\n[target.'cfg(unix)'.dependencies]\nrenamed = { package = \"mid\", path = \"../mid\" }\n",
        );
        w("crates/leaf/Cargo.toml", "[package]\nname = \"leaf\"\n\n[dev-dependencies]\ntempfile = \"3\"\n");
        dir
    }

    fn specific(names: &[&str]) -> AffectedCratesResult {
        AffectedCratesResult::Specific(names.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn workspace_closure_isolates_leaves_and_follows_reverse_deps() {
        let ws = fixture_workspace();
        let root = ws.path();
        assert_eq!(resolve_affected_workspace_crates(root, &["crates/leaf/src/lib.rs"]), specific(&["leaf"]));
        assert_eq!(
            resolve_affected_workspace_crates(root, &["crates/app/src/main.rs"]),
            specific(&["app-pkg"]),
            "dir name maps to the manifest's package name"
        );
        assert_eq!(
            resolve_affected_workspace_crates(root, &["crates/mid/src/lib.rs", "docs/x.md"]),
            specific(&["app-pkg", "mid"]),
            "renamed target-specific dependency is still a reverse edge"
        );
        assert_eq!(
            resolve_affected_workspace_crates(root, &["./crates/core/src/lib.rs"]),
            specific(&["app-pkg", "core", "mid"]),
            "upstream edit reaches its transitive dependents"
        );
        assert_eq!(
            resolve_affected_workspace_crates(root, &["crates/core/README.md"]),
            specific(&["app-pkg", "core", "mid"]),
            "a file inside a member counts for it whatever the extension"
        );
    }

    #[test]
    fn workspace_closure_fails_closed() {
        let ws = fixture_workspace();
        let root = ws.path();
        let wide = |files: &[&str]| {
            matches!(resolve_affected_workspace_crates(root, files), AffectedCratesResult::WorkspaceWide(_))
        };
        assert!(wide(&["Cargo.toml"]));
        assert!(wide(&["Cargo.lock", "crates/leaf/src/lib.rs"]));
        assert!(wide(&["crates/leaf/Cargo.toml"]), "member manifests change resolution too");
        assert!(wide(&[".cargo/config.toml"]));
        assert!(wide(&[".github/workflows/ci.yml"]));
        assert!(wide(&["assets/icon.png"]), "non-member code/data");
        assert!(wide(&["docs/only.md", ".hadron/x.json"]), "nothing to slice");
        assert!(wide(&["crates/core/src/lib.rs", "crates/leaf/src/lib.rs"]), "closure is every member");
        assert!(!wide(&["crates/leaf/src/lib.rs"]));

        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\"]\n").unwrap();
        assert!(wide(&["crates/leaf/src/lib.rs"]), "globs are not expanded without cargo");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"solo\"\n").unwrap();
        assert!(wide(&["src/lib.rs"]), "not a workspace");
    }

    #[test]
    fn this_repository_maps_chamber_to_hadron() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("crates/<name> sits two levels under the repo root");
        assert_eq!(
            resolve_affected_workspace_crates(&root, &["crates/hadron-chamber/src/main.rs"]),
            specific(&["hadron"])
        );
        let AffectedCratesResult::Specific(gk) =
            resolve_affected_workspace_crates(&root, &["crates/hadron-gatekeeper/src/lib.rs"])
        else {
            panic!("gatekeeper is not depended on by lattice, so the closure is not workspace-wide");
        };
        for downstream in ["hadron", "hadron-forge-mcp", "hadron-gatekeeper", "hadron-gluon"] {
            assert!(gk.contains(&downstream.to_string()), "{downstream} missing from {gk:?}");
        }
    }
}
