use bm25::{Language, SearchEngine, SearchEngineBuilder};
use std::sync::Mutex;

#[derive(Default)]
struct BM25Inner {
    corpus: Vec<(String, String)>,
    engine: Option<SearchEngine<u32>>,
    generation: u64,
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
        inner.generation += 1;
    }

    pub fn remove(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.corpus.retain(|(k, _)| k != id);
        inner.engine = None;
        inner.generation += 1;
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        // Fast path: engine already cached — search under lock and return.
        {
            let inner = self.inner.lock().unwrap();
            if inner.corpus.is_empty() {
                return Vec::new();
            }
            if let Some(engine) = &inner.engine {
                return Self::run_search(engine, &inner.corpus, query, limit);
            }
        }

        // Slow path: snapshot corpus, build engine without holding the lock, then
        // install under write-lock only if the corpus hasn't changed (generation check).
        let (snapshot, snap_gen) = {
            let inner = self.inner.lock().unwrap();
            (inner.corpus.clone(), inner.generation)
        };
        let texts: Vec<&str> = snapshot.iter().map(|(_, t)| t.as_str()).collect();
        let new_engine = SearchEngineBuilder::<u32>::with_corpus(Language::English, texts).build();

        let mut inner = self.inner.lock().unwrap();
        if inner.generation == snap_gen {
            inner.engine = Some(new_engine);
            Self::run_search(inner.engine.as_ref().unwrap(), &inner.corpus, query, limit)
        } else {
            // Corpus mutated during our build — rebuild under lock (rare concurrent-write path).
            let texts: Vec<&str> = inner.corpus.iter().map(|(_, t)| t.as_str()).collect();
            inner.engine = Some(SearchEngineBuilder::<u32>::with_corpus(Language::English, texts).build());
            Self::run_search(inner.engine.as_ref().unwrap(), &inner.corpus, query, limit)
        }
    }

    fn run_search(engine: &SearchEngine<u32>, corpus: &[(String, String)], query: &str, limit: usize) -> Vec<(String, f32)> {
        engine
            .search(query, limit)
            .into_iter()
            .filter_map(|r| corpus.get(r.document.id as usize).map(|(id, _)| (id.clone(), r.score)))
            .collect()
    }

    pub fn suggest(&self, prefix: &str, limit: usize) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        let prefix_lower = prefix.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut suggestions = Vec::new();
        for (_, text) in &inner.corpus {
            for word in text.split_whitespace() {
                let w: String = word.chars().filter(|c| c.is_alphabetic()).collect::<String>().to_lowercase();
                if w.len() > prefix_lower.len() && w.starts_with(&prefix_lower) && seen.insert(w.clone()) {
                    suggestions.push(w);
                    if suggestions.len() >= limit { return suggestions; }
                }
            }
        }
        suggestions
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().corpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().corpus.is_empty()
    }
}

