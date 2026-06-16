use bm25::{Language, SearchEngine, SearchEngineBuilder};
use std::sync::Mutex;

#[derive(Default)]
struct BM25Inner {
    corpus: Vec<(String, String)>,
    engine: Option<SearchEngine<u32>>,
}

#[derive(Default)]
pub struct Bm25Index {
    inner: Mutex<BM25Inner>,
}

impl Bm25Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, id: &str, text: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(pos) = inner.corpus.iter().position(|(k, _)| k == id) {
            inner.corpus[pos].1 = text.to_string();
        } else {
            inner.corpus.push((id.to_string(), text.to_string()));
        }
        inner.engine = None;
    }

    pub fn remove(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.corpus.retain(|(k, _)| k != id);
        inner.engine = None;
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        let mut inner = self.inner.lock().unwrap();
        if inner.corpus.is_empty() {
            return Vec::new();
        }
        if inner.engine.is_none() {
            let texts: Vec<&str> = inner.corpus.iter().map(|(_, t)| t.as_str()).collect();
            inner.engine = Some(SearchEngineBuilder::<u32>::with_corpus(Language::English, texts).build());
        }
        let engine = inner.engine.as_ref().unwrap();
        engine
            .search(query, limit)
            .into_iter()
            .filter_map(|r| {
                inner.corpus.get(r.document.id as usize).map(|(id, _)| (id.clone(), r.score))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().corpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().corpus.is_empty()
    }
}

