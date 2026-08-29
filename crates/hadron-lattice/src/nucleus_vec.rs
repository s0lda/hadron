#[derive(Debug, Clone)]
pub struct VectorMatch {
    pub slug: String,
    pub description: String,
    pub score: f32,
}

pub struct NucleusVectorIndex {
    entries: Vec<(String, Vec<f32>, String)>,
}

impl NucleusVectorIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, slug: &str, embedding: Vec<f32>, desc: &str) {
        self.entries
            .push((slug.to_string(), embedding, desc.to_string()));
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Vec<VectorMatch> {
        let mut scored: Vec<VectorMatch> = self
            .entries
            .iter()
            .map(|(slug, emb, desc)| {
                let dot: f32 = emb.iter().zip(query.iter()).map(|(a, b)| a * b).sum();
                let mag_a: f32 = emb.iter().map(|a| a * a).sum::<f32>().sqrt();
                let mag_b: f32 = query.iter().map(|b| b * b).sum::<f32>().sqrt();
                let sim = if mag_a > 0.0 && mag_b > 0.0 {
                    dot / (mag_a * mag_b)
                } else {
                    0.0
                };
                VectorMatch {
                    slug: slug.clone(),
                    description: desc.clone(),
                    score: sim,
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_similarity_search() {
        let mut index = NucleusVectorIndex::new();
        index.insert("bug-copy", vec![1.0, 0.0, 0.0], "Chat copy shortcut missing");
        index.insert("bug-port", vec![0.0, 1.0, 0.0], "Port collision on test runner");

        let matches = index.search(&[0.9, 0.1, 0.0], 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].slug, "bug-copy");
    }
}
