#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    TypeScript,
    Go,
    C,
    Cpp,
    Java,
    CSharp,
    JavaScript,
    Ruby,
    Php,
    Html,
    Css,
    Sql,
    Opaque,
}

pub fn lang_for_path(path: &str) -> Lang {
    match path.rsplit('.').next() {
        Some("rs") => Lang::Rust,
        Some("py") => Lang::Python,
        Some("ts") | Some("tsx") => Lang::TypeScript,
        Some("go") => Lang::Go,
        Some("c") | Some("h") => Lang::C,
        Some("cpp") | Some("hpp") | Some("cc") | Some("cxx") | Some("hh") => Lang::Cpp,
        Some("java") => Lang::Java,
        Some("cs") => Lang::CSharp,
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Lang::JavaScript,
        Some("rb") => Lang::Ruby,
        Some("php") => Lang::Php,
        Some("html") | Some("htm") => Lang::Html,
        Some("css") | Some("scss") => Lang::Css,
        Some("sql") => Lang::Sql,
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

    #[test]
    fn new_language_extensions_map_correctly() {
        assert_eq!(lang_for_path("main.c"), Lang::C);
        assert_eq!(lang_for_path("main.cpp"), Lang::Cpp);
        assert_eq!(lang_for_path("Main.java"), Lang::Java);
        assert_eq!(lang_for_path("Program.cs"), Lang::CSharp);
        assert_eq!(lang_for_path("app.js"), Lang::JavaScript);
        assert_eq!(lang_for_path("app.jsx"), Lang::JavaScript);
        assert_eq!(lang_for_path("script.rb"), Lang::Ruby);
        assert_eq!(lang_for_path("index.php"), Lang::Php);
        assert_eq!(lang_for_path("page.html"), Lang::Html);
        assert_eq!(lang_for_path("style.css"), Lang::Css);
        assert_eq!(lang_for_path("query.sql"), Lang::Sql);
    }
}
