//! Embedded local semantic code and memory graph.
//!
//! Provides in-process, dependency-light semantic indexing and retrieval for
//! AST blocks, nucleus notes, and documentation chunks using term frequency
//! vector scoring and fast inverted index structures.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// One indexed code or memory chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticChunk {
    pub path: String,
    pub symbol: String,
    pub doc: String,
}

/// A search hit with relevance score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub chunk: SemanticChunk,
    pub score: f32,
}

/// In-memory semantic graph index.
#[derive(Debug, Clone, Default)]
pub struct SemanticGraphIndex {
    chunks: Vec<SemanticChunk>,
    // Inverted index mapping term -> set of chunk indices
    inverted_index: HashMap<String, HashSet<usize>>,
}

impl SemanticGraphIndex {
    /// Creates an empty in-memory semantic index.
    pub fn new_in_memory() -> Self {
        Self {
            chunks: Vec::new(),
            inverted_index: HashMap::new(),
        }
    }

    /// Indexes a single code or memory chunk.
    pub fn index_chunk(&mut self, path: &str, symbol: &str, doc: &str) -> anyhow::Result<()> {
        let chunk_idx = self.chunks.len();
        let chunk = SemanticChunk {
            path: path.to_string(),
            symbol: symbol.to_string(),
            doc: doc.to_string(),
        };

        // Tokenize path, symbol, doc
        let terms = tokenize_chunk(&chunk);
        for term in terms {
            self.inverted_index
                .entry(term)
                .or_default()
                .insert(chunk_idx);
        }

        self.chunks.push(chunk);
        Ok(())
    }

    /// Searches for the top `limit` relevant chunks matching `query`.
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SemanticSearchResult>> {
        if self.chunks.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let query_terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() >= 2)
            .map(|s| s.to_ascii_lowercase())
            .collect();

        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut scores: HashMap<usize, f32> = HashMap::new();

        for term in &query_terms {
            if let Some(matching_indices) = self.inverted_index.get(term) {
                for &idx in matching_indices {
                    let chunk = &self.chunks[idx];
                    let mut boost = 1.0f32;
                    if chunk.symbol.to_ascii_lowercase().contains(term) {
                        boost += 3.0;
                    }
                    if chunk.path.to_ascii_lowercase().contains(term) {
                        boost += 2.0;
                    }
                    *scores.entry(idx).or_insert(0.0) += boost;
                }
            }
        }

        let mut results: Vec<SemanticSearchResult> = scores
            .into_iter()
            .map(|(idx, score)| SemanticSearchResult {
                chunk: self.chunks[idx].clone(),
                score,
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.chunk.path.cmp(&b.chunk.path))
        });

        results.truncate(limit);
        Ok(results)
    }

    /// Returns the total number of indexed chunks.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Returns true if the index contains no chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

fn tokenize_chunk(chunk: &SemanticChunk) -> HashSet<String> {
    let mut set = HashSet::new();
    for text in [&chunk.path, &chunk.symbol, &chunk.doc] {
        for token in text.split(|c: char| !c.is_alphanumeric()) {
            if token.len() >= 2 {
                set.insert(token.to_ascii_lowercase());
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_graph_indexes_and_queries_code_chunks() {
        let mut index = SemanticGraphIndex::new_in_memory();
        index
            .index_chunk(
                "crates/hadron-gluon/src/merge.rs",
                "fn merge_gate()",
                "Merge runner gate logic and test suite execution",
            )
            .unwrap();
        index
            .index_chunk(
                "crates/hadron-chamber/src/app/render/chat.rs",
                "fn render_chat()",
                "Chat viewport and GPUI rendering widgets",
            )
            .unwrap();

        let hits = index.search("gate test runner", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk.path, "crates/hadron-gluon/src/merge.rs");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn empty_query_returns_empty() {
        let mut index = SemanticGraphIndex::new_in_memory();
        index.index_chunk("test.rs", "fn foo()", "doc").unwrap();
        let hits = index.search("", 5).unwrap();
        assert!(hits.is_empty());
    }
}
