use symspell::{AsciiStringStrategy, SymSpell, Verbosity};
use std::sync::RwLock;

/// Catalog-seeded spell corrector.
/// Vocabulary is built from product text as products are indexed.
/// SymSpell algorithm: sub-millisecond correction.
pub struct SpellCorrector {
    inner: RwLock<SymSpell<AsciiStringStrategy>>,
}

impl SpellCorrector {
    pub fn new() -> Self {
        Self { inner: RwLock::new(SymSpell::default()) }
    }

    /// Add product text to the vocabulary.
    pub fn add_text(&self, text: &str) {
        let mut spell = self.inner.write().unwrap();
        for word in text.split_whitespace() {
            let word = word.to_lowercase();
            let word = word.trim_matches(|c: char| !c.is_alphabetic());
            if word.len() >= 2 {
                spell.load_dictionary_line(&format!("{} 1", word), 0, 1, " ");
            }
        }
    }

    /// Correct a query. Returns corrected string (may be unchanged if already correct).
    pub fn correct(&self, query: &str) -> String {
        let spell = self.inner.read().unwrap();
        query
            .split_whitespace()
            .map(|word| {
                let lower = word.to_lowercase();
                let suggestions = spell.lookup(&lower, Verbosity::Top, 2);
                if let Some(best) = suggestions.first() {
                    if best.distance <= 2 && !best.term.is_empty() {
                        // Preserve original case for single-char diff
                        return best.term.clone();
                    }
                }
                word.to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Default for SpellCorrector {
    fn default() -> Self {
        Self::new()
    }
}
