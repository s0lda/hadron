use std::path::Path;

use anyhow::Context;
use hadron_lattice::NucleusIndex;

/// A resolved nucleus: ordered `(name, markdown)` docs after layering, plus the
/// project commit the knowledge was last verified against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nucleus {
    pub docs: Vec<(String, String)>,
    pub last_verified_commit: Option<String>,
}

/// Read one layer directory: parse `index.json`, then read each listed source
/// doc. A missing directory or missing index yields an empty layer (not an
/// error) — layers are optional.
fn read_layer(dir: &Path) -> anyhow::Result<(NucleusIndex, Vec<(String, String)>)> {
    let index_path = dir.join("index.json");
    let index: NucleusIndex = match std::fs::read_to_string(&index_path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", index_path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((NucleusIndex::default(), Vec::new()))
        }
        Err(e) => return Err(e).context("reading nucleus index"),
    };
    let mut docs = Vec::new();
    for name in &index.sources {
        let doc_path = dir.join(name);
        match std::fs::read_to_string(&doc_path) {
            Ok(body) => docs.push((name.clone(), body)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, // listed but absent: skip
            Err(e) => return Err(e).with_context(|| format!("reading {}", doc_path.display())),
        }
    }
    Ok((index, docs))
}

/// Load and layer the nucleus. Repo layer is the base; global layer strictly
/// overrides same-named docs and appends new ones. Either layer may be `None`.
pub fn load(repo_layer: Option<&Path>, global_layer: Option<&Path>) -> anyhow::Result<Nucleus> {
    let (repo_index, mut docs) = match repo_layer {
        Some(dir) => read_layer(dir)?,
        None => (NucleusIndex::default(), Vec::new()),
    };

    if let Some(dir) = global_layer {
        let (_global_index, global_docs) = read_layer(dir)?;
        for (name, body) in global_docs {
            match docs.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = body,     // strict override
                None => docs.push((name, body)), // new global-only doc
            }
        }
    }

    Ok(Nucleus { docs, last_verified_commit: repo_index.last_verified_commit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_layer(dir: &Path, index: &str, files: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("index.json"), index).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
    }

    #[test]
    fn repo_only_loads_in_source_order() {
        let dir = tempdir().unwrap();
        write_layer(
            dir.path(),
            r#"{"version":1,"last_verified_commit":"abc","sources":["map.md","conventions.md"]}"#,
            &[("map.md", "# Map"), ("conventions.md", "# Conv")],
        );
        let n = load(Some(dir.path()), None).unwrap();
        assert_eq!(n.docs.len(), 2);
        assert_eq!(n.docs[0].0, "map.md");
        assert_eq!(n.docs[1].0, "conventions.md");
        assert_eq!(n.last_verified_commit, Some("abc".into()));
    }

    #[test]
    fn global_strictly_overrides_repo() {
        let repo = tempdir().unwrap();
        let global = tempdir().unwrap();
        write_layer(
            repo.path(),
            r#"{"version":1,"sources":["conventions.md"]}"#,
            &[("conventions.md", "REPO rules")],
        );
        write_layer(
            global.path(),
            r#"{"version":1,"sources":["conventions.md","user.md"]}"#,
            &[("conventions.md", "GLOBAL rules"), ("user.md", "my prefs")],
        );
        let n = load(Some(repo.path()), Some(global.path())).unwrap();
        // conventions.md is overridden by global; user.md appended.
        let conv = n.docs.iter().find(|(name, _)| name == "conventions.md").unwrap();
        assert_eq!(conv.1, "GLOBAL rules");
        assert!(n.docs.iter().any(|(name, _)| name == "user.md"));
    }

    #[test]
    fn missing_layers_are_empty_not_errors() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let n = load(Some(&missing), None).unwrap();
        assert_eq!(n.docs.len(), 0);
        assert_eq!(n.last_verified_commit, None);
    }
}
