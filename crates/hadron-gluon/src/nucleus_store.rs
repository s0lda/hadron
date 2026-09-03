use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::RwLock;
use std::sync::Arc;

#[derive(Clone)]
pub struct NucleusStore {
    root_dir: PathBuf,
    lock: Arc<RwLock<()>>,
}

impl NucleusStore {
    pub fn new(root_dir: &Path) -> Self {
        Self {
            root_dir: root_dir.to_path_buf(),
            lock: Arc::new(RwLock::new(())),
        }
    }

    pub async fn write_note(
        &self,
        slug: &str,
        fact: &str,
        why: &str,
        how_to_apply: &str,
    ) -> anyhow::Result<()> {
        let _guard = self.lock.write().await;
        let notes_dir = self.root_dir.join("notes");
        fs::create_dir_all(&notes_dir).await?;

        let note_path = notes_dir.join(format!("{slug}.md"));
        let note_body = format!(
            "---\nname: {slug}\ndescription: {why}\nmetadata:\n  type: project\n---\n\n{fact}\n\n**Why:** {why}\n\n**How to apply:** {how_to_apply}\n"
        );
        fs::write(&note_path, note_body).await?;

        // Update index.md
        let index_path = self.root_dir.join("index.md");
        let mut index = if index_path.exists() {
            fs::read_to_string(&index_path).await?
        } else {
            "# Memory index\n\n## Project Lessons\n\n".to_string()
        };

        let pointer = format!("- [{slug}](notes/{slug}.md) — {why}\n");
        if !index.contains(&format!("[{slug}]")) {
            index.push_str(&pointer);
            fs::write(&index_path, index).await?;
        }

        Ok(())
    }

    pub async fn read_note(&self, slug: &str) -> anyhow::Result<String> {
        let note_path = self.root_dir.join("notes").join(format!("{slug}.md"));
        let content = fs::read_to_string(note_path).await?;
        Ok(content)
    }
}
