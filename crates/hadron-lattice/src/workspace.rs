use std::path::Path;

/// The project root that owns a field path — `<root>/.hadron/field.jsonl` → `<root>`.
/// A field sitting outside a `.hadron/` directory is taken to be in the root already.
///
/// Lives in the lattice because both ends of the two-process architecture need the
/// same rule: the chamber renders a repo's files/changes from it, and the gluon
/// resolves the repo it worktrees quarks into from it. One rule, one place.
pub fn repo_root_of(field_path: &Path) -> &Path {
    let Some(parent) = field_path.parent() else {
        return field_path;
    };
    let root = if parent.file_name() == Some(std::ffi::OsStr::new(".hadron")) {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_repo_root_is_the_parent_of_the_hadron_dir() {
        let field = PathBuf::from("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/home/jake/dev/hadron"));
    }

    #[test]
    fn a_field_outside_a_hadron_dir_is_already_in_the_root() {
        let field = PathBuf::from("/tmp/scratch/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/tmp/scratch"));
    }

    #[test]
    fn a_bare_field_name_resolves_to_the_current_dir() {
        let field = PathBuf::from("field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("."));
    }
}
