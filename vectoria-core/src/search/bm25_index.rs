use bm25::{Language, SearchEngineBuilder};
use std::sync::RwLock;

/// In-memory BM25 index.
/// Rebuilt from corpus snapshot on each query — acceptable for catalogs < 500k products.
/// For larger catalogs, swap to EdgeStore's built-in FTS.
pub struct Bm25Index {
    /// (product_id, searchable_text) pairs.
    corpus: RwLock<Vec<(String, String)>>,
}

impl Bm25Index {
    pub fn new() -> Self {
        Self { corpus: RwLock::new(Vec::new()) }
    }

    /// Add or update a document.
    pub fn upsert(&self, id: &str, text: &str) {
        let mut corpus = self.corpus.write().unwrap();
        if let Some(pos) = corpus.iter().position(|(k, _)| k == id) {
            corpus[pos].1 = text.to_string();
        } else {
            corpus.push((id.to_string(), text.to_string()));
        }
    }

    /// Remove a document.
    pub fn remove(&self, id: &str) {
        let mut corpus = self.corpus.write().unwrap();
        corpus.retain(|(k, _)| k != id);
    }

    /// Search — returns (product_id, normalized_bm25_score).
    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        let corpus = self.corpus.read().unwrap();
        if corpus.is_empty() {
            return Vec::new();
        }
        let texts: Vec<&str> = corpus.iter().map(|(_, t)| t.as_str()).collect();
        // Use u32 document IDs (0..n as assigned by SearchEngineBuilder).
        let engine = SearchEngineBuilder::<u32>::with_corpus(Language::English, texts).build();
        engine
            .search(query, limit)
            .into_iter()
            .filter_map(|r| {
                corpus.get(r.document.id as usize).map(|(id, _)| (id.clone(), r.score))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.corpus.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpus.read().unwrap().is_empty()
    }
}

impl Default for Bm25Index {
    fn default() -> Self { Self::new() }
}
