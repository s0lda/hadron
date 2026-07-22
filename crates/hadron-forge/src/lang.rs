#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    TypeScript,
    Go,
    Opaque,
}

pub fn lang_for_path(path: &str) -> Lang {
    match path.rsplit('.').next() {
        Some("rs") => Lang::Rust,
        Some("py") => Lang::Python,
        Some("ts") | Some("tsx") => Lang::TypeScript,
        Some("go") => Lang::Go,
        _ => Lang::Opaque,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_langs() {
        assert_eq!(lang_for_path("src/x.rs"), Lang::Rust);
        assert_eq!(lang_for_path("a/b.py"), Lang::Python);
        assert_eq!(lang_for_path("c.ts"), Lang::TypeScript);
        assert_eq!(lang_for_path("c.tsx"), Lang::TypeScript);
        assert_eq!(lang_for_path("m.go"), Lang::Go);
        assert_eq!(lang_for_path("Cargo.toml"), Lang::Opaque);
        assert_eq!(lang_for_path("README.md"), Lang::Opaque);
    }
}
