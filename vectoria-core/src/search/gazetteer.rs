//! # Query Understanding Pipeline — Stage 2: Catalog Gazetteer
//!
//! The gazetteer is an in-memory attribute lookup built incrementally from indexed
//! product metadata. It connects the query understanding pipeline to the actual
//! catalog — instead of pattern-matching against hardcoded keyword lists, Stage 2
//! matches against values that genuinely exist in the index.
//!
//! ## How it works
//!
//! **At index time** (`index()` → `add_product()`): for each configured field
//! (default: `brand`, `color`, `category`), the raw metadata value is stored in a
//! case-insensitive lookup table. First-seen casing wins; the stored original-case
//! value is what gets applied as a filter so it matches the metadata exactly.
//!
//! **At search time** (`search()` → `extract_filters()`): the cleaned query from
//! Stage 1 is checked against all tracked values, using word-boundary matching to
//! avoid partial matches ("Nikon" should not match inside "Nikonfuel").
//!
//! **Ambiguity rule**: if more than one value for a given field matches the query
//! (e.g. both "Nike" and "Adidas" appear in "Nike vs Adidas"), no filter is applied
//! for that field. Applying both would require an OR filter that is not yet
//! supported; applying either one would be wrong.
//!
//! ## Lifecycle and staleness
//!
//! The gazetteer is **append-only**: deleted products do not remove their values.
//! A stale value (brand deleted from catalog) produces a filter that matches zero
//! products — no results for that query token, which is arguably correct (the brand
//! really is gone). To fully clear stale entries, call `reindex_all()` on a fresh
//! `SearchEngine` instance; `index()` repopulates the gazetteer from scratch.
//!
//! ## Tunable points
//!
//! | Constant / API                  | Effect                                          |
//! |---------------------------------|-------------------------------------------------|
//! | `MIN_LEN`                       | Minimum value length stored (default 2)         |
//! | `Gazetteer::new(fields)`        | Fields to track (default brand/color/category)  |
//! | `with_gazetteer_fields()`       | Builder API to override tracked fields          |
//!
//! ## Filter priority (reminder)
//!
//! Gazetteer signals have the **lowest** priority. `query_parser` signals (price,
//! urgency) override them, and user-provided `req.filters` override everything.

use std::collections::HashMap;
use std::sync::RwLock;
use crate::model::Product;

/// Minimum character length for a value to be added to the gazetteer.
/// Avoids single-letter noise (e.g. "P" for a size field that also matches prepositions).
const MIN_LEN: usize = 2;

/// Catalog-derived attribute lookup built incrementally from indexed products.
///
/// At query time, [`Gazetteer::extract_filters`] matches query tokens against
/// known attribute values (brand, color, category, …) and returns filter
/// candidates. These are merged as auto-detected filters — user-provided
/// filters always win on key collision.
///
/// The gazetteer is append-only: deleted products leave their values in place.
/// Stale entries produce no harm beyond a filter that matches zero products.
pub struct Gazetteer {
    inner: RwLock<GazData>,
    /// Metadata fields to extract attribute values from.
    fields: Vec<String>,
}

#[derive(Default)]
struct GazData {
    /// field → { lowercase_value → original_case_value }
    /// Lowercase is used for case-insensitive query matching.
    /// Original case is used for filter application so it matches stored metadata.
    entries: HashMap<String, HashMap<String, String>>,
}

impl Gazetteer {
    pub fn new(fields: Vec<String>) -> Self {
        Self { inner: RwLock::new(GazData::default()), fields }
    }

    pub fn with_default_fields() -> Self {
        Self::new(vec!["brand".into(), "color".into(), "category".into()])
    }

    pub fn with_fields(mut self, fields: Vec<String>) -> Self {
        self.fields = fields;
        self
    }

    /// Update the gazetteer from a newly indexed product's metadata.
    /// First-seen casing wins when the same value appears with different cases.
    pub fn add_product(&self, product: &Product) {
        let mut data = self.inner.write().unwrap();
        for field in &self.fields {
            let Some(val) = product.metadata.get(field).and_then(|v| v.as_str()) else { continue };
            let v = val.trim();
            if v.len() < MIN_LEN { continue; }
            data.entries
                .entry(field.clone())
                .or_default()
                .entry(v.to_lowercase())
                .or_insert_with(|| v.to_string());
        }
    }

    /// Match query tokens against catalog attribute values.
    ///
    /// Returns `(field, JSON_value)` pairs where exactly one catalog value for
    /// that field matches a word-boundary token in the query. Fields with zero
    /// or multiple matches are skipped — multiple matches signal an ambiguous
    /// query (e.g. "Nike Adidas comparison") where applying a brand filter
    /// would be wrong.
    pub fn extract_filters(&self, query: &str) -> Vec<(String, serde_json::Value)> {
        let lower = query.to_lowercase();
        let data = self.inner.read().unwrap();
        let mut out = Vec::new();

        'field: for (field, value_map) in &data.entries {
            let mut matched_orig: Option<&str> = None;
            for (lower_val, orig_val) in value_map {
                if lower_val.len() < MIN_LEN { continue; }
                if word_boundary_match(&lower, lower_val) {
                    if matched_orig.is_some() {
                        // Ambiguous: multiple values for this field in the query — skip.
                        continue 'field;
                    }
                    matched_orig = Some(orig_val.as_str());
                }
            }
            if let Some(orig) = matched_orig {
                out.push((field.clone(), serde_json::Value::String(orig.to_string())));
            }
        }

        out
    }

    /// Number of distinct values tracked across all fields. Useful for diagnostics.
    pub fn size(&self) -> usize {
        self.inner.read().unwrap().entries.values().map(|m| m.len()).sum()
    }
}

/// Returns true if `needle` appears in `haystack` at a word boundary
/// (not immediately adjacent to an alphanumeric byte on either side).
fn word_boundary_match(haystack: &str, needle: &str) -> bool {
    let hb = haystack.as_bytes();
    let needle_len = needle.len();
    let mut start = 0;
    while start + needle_len <= hb.len() {
        if let Some(rel) = haystack[start..].find(needle) {
            let abs = start + rel;
            let before_ok = abs == 0 || !hb[abs - 1].is_ascii_alphanumeric();
            let after = abs + needle_len;
            let after_ok = after >= hb.len() || !hb[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = abs + haystack[abs..].chars().next().map_or(1, |c| c.len_utf8());
        } else {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn product(id: &str, meta: serde_json::Value) -> Product {
        let mut p = Product::new(id, meta.as_object().cloned().unwrap_or_default().into_iter().collect());
        p.status = crate::model::ProductStatus::Indexed;
        p
    }

    #[test]
    fn test_single_brand_match() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "Nike", "color": "Black"})));
        gaz.add_product(&product("p2", json!({"brand": "Adidas"})));

        let filters = gaz.extract_filters("tênis nike running");
        let brand = filters.iter().find(|(f, _)| f == "brand");
        assert_eq!(brand.map(|(_, v)| v.as_str().unwrap()), Some("Nike"));
    }

    #[test]
    fn test_ambiguous_brands_skipped() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "Nike"})));
        gaz.add_product(&product("p2", json!({"brand": "Adidas"})));

        // Both brands present → ambiguous → no brand filter
        let filters = gaz.extract_filters("nike adidas comparison");
        assert!(!filters.iter().any(|(f, _)| f == "brand"));
    }

    #[test]
    fn test_word_boundary_prevents_partial_match() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "Nikon"})));

        // "nikon" should NOT match inside "nikonfuel"
        assert!(!word_boundary_match("nikonfuel running", "nikon"));
        // But should match "nikon camera"
        assert!(word_boundary_match("nikon camera", "nikon"));
    }

    #[test]
    fn test_color_extracted() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"color": "Preto"})));

        let filters = gaz.extract_filters("tênis preto running");
        let color = filters.iter().find(|(f, _)| f == "color");
        assert_eq!(color.map(|(_, v)| v.as_str().unwrap()), Some("Preto"));
    }

    #[test]
    fn test_case_insensitive_match_original_case_applied() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "New Balance"})));

        let filters = gaz.extract_filters("tênis new balance corrida");
        let brand = filters.iter().find(|(f, _)| f == "brand");
        // Original case preserved for filter application
        assert_eq!(brand.map(|(_, v)| v.as_str().unwrap()), Some("New Balance"));
    }

    #[test]
    fn test_empty_query_no_filters() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "Nike"})));
        assert!(gaz.extract_filters("").is_empty());
    }

    #[test]
    fn test_no_match_no_filters() {
        let gaz = Gazetteer::with_default_fields();
        gaz.add_product(&product("p1", json!({"brand": "Nike"})));
        assert!(gaz.extract_filters("tênis running calçado").is_empty());
    }

    // Regression: start = abs + 1 panics when the found position is the first byte of a
    // multi-byte char. "Ên" (ê = 2 bytes) found inside "bênigno" fails the before-boundary
    // check (preceded by 'b'), then the old code sliced at the continuation byte.
    #[test]
    fn test_word_boundary_utf8_no_panic() {
        assert!(!word_boundary_match("bênigno produto", "ên"));
        // Confirm a true word-boundary match still works with multi-byte chars.
        assert!(word_boundary_match("produto ên final", "ên"));
    }
}
