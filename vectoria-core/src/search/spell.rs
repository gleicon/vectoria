use symspell::{AsciiStringStrategy, SymSpell, Verbosity};
use std::sync::RwLock;

/// Catalog-seeded spell corrector.
pub struct SpellCorrector {
    inner: RwLock<SymSpell<AsciiStringStrategy>>,
}

impl SpellCorrector {
    pub fn new() -> Self {
        Self { inner: RwLock::new(SymSpell::default()) }
    }

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

    pub fn correct(&self, query: &str) -> String {
        let spell = self.inner.read().unwrap();
        let lower = query.to_lowercase();

        // Only correct contiguous alphabetic runs; pass numeric/special-char tokens through.
        // "1/16th scale gravity wagon" → "1/16th" + correct("scale gravity wagon")
        // Prevents mangling product codes, sizes, model numbers.
        let mut result: Vec<String> = Vec::new();
        let mut alpha_run: Vec<&str> = Vec::new();

        let flush = |run: &mut Vec<&str>, out: &mut Vec<String>, sp: &SymSpell<AsciiStringStrategy>| {
            if run.is_empty() { return; }
            let text = run.join(" ");
            run.clear();
            out.push(correct_run(sp, &text));
        };

        for token in lower.split_whitespace() {
            if token.chars().all(|c| c.is_alphabetic()) {
                alpha_run.push(token);
            } else {
                flush(&mut alpha_run, &mut result, &spell);
                result.push(token.to_string());
            }
        }
        flush(&mut alpha_run, &mut result, &spell);

        result.join(" ")
    }
}

fn correct_run(spell: &SymSpell<AsciiStringStrategy>, text: &str) -> String {
    // Pre-pass: word_segmentation for long compound tokens (≥10 chars) not in dictionary.
    // Handles 3-word compounds lookup_compound's bigram split misses.
    let pre: String = text
        .split_whitespace()
        .map(|token| {
            if token.len() < 10 { return token.to_string(); }
            if spell.lookup(token, Verbosity::Top, 0).iter().any(|s| s.distance == 0) {
                return token.to_string();
            }
            let seg = spell.word_segmentation(token, 0);
            if seg.segmented_string != token && !seg.segmented_string.is_empty() {
                seg.segmented_string
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Main pass: fix per-word typos and handle 2-word splits/joins.
    let suggestions = spell.lookup_compound(&pre, 2);
    suggestions.into_iter()
        .next()
        .filter(|s| !s.term.is_empty())
        .map(|s| s.term)
        .unwrap_or(pre)
}

impl Default for SpellCorrector {
    fn default() -> Self {
        Self::new()
    }
}
