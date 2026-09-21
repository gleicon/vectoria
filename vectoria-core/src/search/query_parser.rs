//! # Query Understanding Pipeline — Stage 1: Structural Extraction
//!
//! This module converts a raw free-text search query into a `ParsedQuery` that
//! carries structured signals for the retrieval layer. It runs synchronously on
//! every search call with no I/O, no ML inference, and no new dependencies.
//!
//! ## Pipeline overview
//!
//! ```text
//! raw query
//!     │
//!     ▼  Stage 1 — QueryParser (this module)
//!     │  • Price ceiling          "até R$200"   → filter price_max=200
//!     │  • Delivery urgency       "entrega hoje" → filter in_stock=true
//!     │                                          + availability weight ×2
//!     │  • CEP removal            "01310-100"   → stripped from search_q
//!     │  • CPF / CNPJ removal     "123.456.789-09" → stripped (with checksum)
//!     │  • Phone number removal   "(11) 99999-9999" → stripped
//!     │
//!     ▼  Stage 2 — Gazetteer  (gazetteer.rs)
//!     │  • Brand / Color / Category token matching against catalog values
//!     │    "Nike running" → brand=Nike (if Nike exists in the index)
//!     │
//!     ▼  Merge: gazetteer < query_parser < user-provided filters
//!     │
//!     ▼  Retrieval (BM25 + vector, candidate pool, RRF fusion)
//! ```
//!
//! ## Tunable points
//!
//! | Constant / field         | Location              | Effect                              |
//! |--------------------------|----------------------|--------------------------------------|
//! | `URGENCY_TERMS`          | this file            | Keywords that trigger in_stock=true  |
//! | `PRICE_PREFIXES`         | this file            | Patterns that introduce a price value|
//! | `AVAILABILITY_BOOST`     | this file            | Weight multiplier when urgency fires |
//! | `MIN_PRICE`              | this file            | Sanity floor — ignore prices below   |
//! | `MAX_PRICE`              | this file            | Sanity ceiling — ignore prices above |
//! | `Gazetteer::fields`      | gazetteer.rs         | Metadata fields tracked for matching |
//! | `Gazetteer::MIN_LEN`     | gazetteer.rs         | Min value length added to gazetteer  |
//! | `RRF_K`                  | scoring.rs           | Rank-sensitivity for RRF fusion      |
//! | `MAX_CANDIDATE_POOL`     | model/mod.rs         | Hard cap on retrieve-then-rerank pool|
//!
//! ## Filter priority
//!
//! Later entries override earlier ones on key collision:
//!
//! ```text
//! gazetteer.extract_filters()   →  lowest (catalog-derived, speculative)
//! query_parser.parse().filters  →  middle (structural, high-confidence)
//! req.filters (user-provided)   →  highest (explicit, always respected)
//! ```
//!
//! ## BR Structured-type detection
//!
//! CPF, CNPJ, and phone numbers are detected and stripped from the retrieval
//! query. They are structural noise that produces zero BM25 hits and can
//! confuse the BM25 scorer by consuming rare-term IDF budget.
//!
//! CPF and CNPJ detection uses checksum validation (Receita Federal algorithm)
//! to avoid false positives from numeric sequences that happen to have the
//! right digit count.
//!
//! | Type  | Formats detected                      | Checksum validated |
//! |-------|---------------------------------------|--------------------|
//! | CPF   | NNN.NNN.NNN-NN  /  NNNNNNNNNNN       | Yes                |
//! | CNPJ  | NN.NNN.NNN/NNNN-NN  /  NNNNNNNNNNNNNN| Yes                |
//! | Phone | (NN) NNNNN-NNNN  /  (NN) NNNN-NNNN   | Format only        |

use std::collections::HashMap;
use std::ops::Range;

// ── Tunable constants ────────────────────────────────────────────────────────

/// Keywords (lowercased) that signal delivery urgency.
/// When any of these appear in the query, `in_stock=true` is auto-applied as a
/// filter and the availability ranking weight is boosted by `AVAILABILITY_BOOST`.
///
/// **To extend:** add new terms here. Terms are matched as substrings of the
/// lowercased query, so multi-word phrases work ("entrega rápida").
const URGENCY_TERMS: &[&str] = &[
    "hoje",
    "amanhã",
    "amanha",
    "urgente",
    "urgência",
    "urgencia",
    "entrega rápida",
    "entrega rapida",
    "express",
    "imediato",
    "imediata",
    "rápido",
    "rapido",
    "same day",
    "no mesmo dia",
];

/// Multiplier applied to `RankingWeights::availability` when delivery urgency
/// is detected. Only applied when the caller has not provided explicit
/// `ranking_weights` in the `SearchRequest`.
///
/// **To tune:** increase to push in-stock products higher vs. out-of-stock;
/// decrease toward 1.0 to make urgency detection less aggressive.
const AVAILABILITY_BOOST: f32 = 2.0;

/// Price-ceiling prefixes, longest first to avoid partial prefix overlap
/// (e.g. "menos de" must be tried before "de " to match "menos de R$50").
///
/// **To extend:** add new patterns here. The parser extracts the BRL number
/// that immediately follows the matched prefix.
const PRICE_PREFIXES: &[&str] = &[
    "por até r$",
    "por ate r$",
    "menos de r$",
    "menos de ",
    "até r$",
    "até ",
    "ate r$",
    "ate ",
    "por r$",
    "r$",
];

/// Sanity bounds for extracted price values. Numbers outside this range are
/// ignored — they are likely not prices (e.g. a product model number "4000").
///
/// **To tune:** widen MAX_PRICE for luxury catalogs; adjust for non-BRL currencies.
const MIN_PRICE: f64 = 0.01;
const MAX_PRICE: f64 = 999_999.99;

// ── Public surface ───────────────────────────────────────────────────────────

pub struct ParsedQuery {
    /// Search query after structural noise removal (CEP, CPF, CNPJ, phones).
    /// Price and urgency tokens are intentionally kept — they contribute BM25
    /// signal ("tênis entrega rápida" still surface products mentioning quick delivery).
    pub q: String,
    /// Auto-extracted filters. Merged downstream; user filters override on collision.
    pub filters: HashMap<String, serde_json::Value>,
    /// Multiplier for `RankingWeights::availability`. 1.0 = no change.
    pub availability_boost: f32,
    /// Human-readable list of what was removed or extracted (for `QueryContext` / explain).
    pub applied: Vec<String>,
}

/// Parse a raw query string into structured signals.
///
/// Always returns a valid `ParsedQuery`. Never panics on arbitrary input.
/// The cleaned query `q` is guaranteed to be non-empty when the raw query is
/// non-empty (structural removal only removes recognized patterns, never
/// content tokens).
pub fn parse(raw: &str) -> ParsedQuery {
    let lower = raw.to_lowercase();
    let mut filters: HashMap<String, serde_json::Value> = HashMap::new();
    let mut applied: Vec<String> = Vec::new();

    // Price ceiling
    if let Some(price) = extract_price_max(&lower) {
        filters.insert("price_max".into(), serde_json::Value::from(price));
        applied.push(format!("price_max={price:.2}"));
    }

    // Delivery urgency
    if URGENCY_TERMS.iter().any(|t| lower.contains(t)) {
        filters.insert("in_stock".into(), serde_json::Value::Bool(true));
        applied.push("in_stock=true".into());
    }

    let availability_boost = if filters.contains_key("in_stock") { AVAILABILITY_BOOST } else { 1.0 };

    // Structural noise removal: CEP, CPF, CNPJ, phone numbers
    let (q, removed) = remove_br_noise(raw);
    applied.extend(removed);

    ParsedQuery { q, filters, availability_boost, applied }
}

// ── Price extraction ─────────────────────────────────────────────────────────

fn extract_price_max(lower: &str) -> Option<f64> {
    for prefix in PRICE_PREFIXES {
        if let Some(pos) = lower.find(prefix) {
            let after = &lower[pos + prefix.len()..];
            if let Some(v) = parse_brl_number(after) {
                if v >= MIN_PRICE && v <= MAX_PRICE {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Parse a Brazilian decimal number at the start of `s`.
/// Accepts digits optionally followed by a comma or dot and more digits.
/// Returns `None` if no digit appears at the start.
fn parse_brl_number(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b',' || bytes[i] == b'.') {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    s[..i].replace(',', ".").parse::<f64>().ok()
}

// ── Brazilian structured-type noise removal ───────────────────────────────────

/// Remove CEP, CPF, CNPJ, and phone numbers from the query.
/// Returns the cleaned string and a list of type labels removed (for explain).
fn remove_br_noise(s: &str) -> (String, Vec<String>) {
    let bytes = s.as_bytes();
    let mut spans: Vec<(Range<usize>, &'static str)> = Vec::new();

    let mut i = 0;
    while i < bytes.len() {
        // Try longest structured patterns first to avoid consuming a prefix
        // that is the start of a longer pattern (e.g. CNPJ starts with two digits
        // that could also be CEP digits — CNPJ must be tried first).
        if let Some(span) = try_cnpj_formatted(bytes, i) {
            i = span.end;
            spans.push((span, "cnpj"));
        } else if let Some(span) = try_cnpj_raw(bytes, i) {
            i = span.end;
            spans.push((span, "cnpj"));
        } else if let Some(span) = try_cpf_formatted(bytes, i) {
            i = span.end;
            spans.push((span, "cpf"));
        } else if let Some(span) = try_cpf_raw(bytes, i) {
            i = span.end;
            spans.push((span, "cpf"));
        } else if let Some(span) = try_br_phone(bytes, i) {
            i = span.end;
            spans.push((span, "phone"));
        } else if let Some(span) = try_cep(bytes, i) {
            i = span.end;
            spans.push((span, "cep"));
        } else {
            i += s[i..].chars().next().map_or(1, |c| c.len_utf8());
        }
    }

    if spans.is_empty() {
        return (s.to_string(), vec![]);
    }

    // Collect removed type labels (deduplicated, sorted)
    let mut labels: Vec<&str> = spans.iter().map(|(_, label)| *label).collect();
    labels.sort_unstable();
    labels.dedup();
    let removed: Vec<String> = labels.iter().map(|l| format!("removed:{l}")).collect();

    // Build cleaned string by copying regions between spans
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    for (span, _) in &spans {
        out.push_str(&s[pos..span.start]);
        pos = span.end;
    }
    out.push_str(&s[pos..]);
    let q = out.split_whitespace().collect::<Vec<_>>().join(" ");

    (q, removed)
}

// ── CEP ──────────────────────────────────────────────────────────────────────
// Format: NNNNN-NNN or NNNNNNNN (Brazilian postal code, 8 digits)

fn try_cep(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() { return None; }
    if i + 5 > bytes.len() || !bytes[i..i + 5].iter().all(|b| b.is_ascii_digit()) { return None; }
    let mut j = i + 5;
    if j < bytes.len() && bytes[j] == b'-' { j += 1; }
    if j + 3 > bytes.len() || !bytes[j..j + 3].iter().all(|b| b.is_ascii_digit()) { return None; }
    let end = j + 3;
    if end < bytes.len() && bytes[end].is_ascii_alphanumeric() { return None; }
    Some(i..end)
}

// ── CPF ───────────────────────────────────────────────────────────────────────
// Formats: NNN.NNN.NNN-NN (14 chars) or 11 consecutive digits at word boundary.
// Checksum: Receita Federal two-verifier algorithm. All-same-digit sequences
// (000.000.000-00 etc.) are explicitly rejected — they pass the math but are invalid.

fn try_cpf_formatted(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    // NNN.NNN.NNN-NN = 14 bytes
    if i + 14 > bytes.len() { return None; }
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() { return None; }
    let b = &bytes[i..i + 14];
    if !b[..3].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[3] != b'.' { return None; }
    if !b[4..7].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[7] != b'.' { return None; }
    if !b[8..11].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[11] != b'-' { return None; }
    if !b[12..14].iter().all(|x| x.is_ascii_digit()) { return None; }
    if i + 14 < bytes.len() && bytes[i + 14].is_ascii_alphanumeric() { return None; }

    let d = [b[0]-b'0', b[1]-b'0', b[2]-b'0', b[4]-b'0', b[5]-b'0', b[6]-b'0',
             b[8]-b'0', b[9]-b'0', b[10]-b'0', b[12]-b'0', b[13]-b'0'];
    if validate_cpf(&d) { Some(i..i + 14) } else { None }
}

fn try_cpf_raw(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    if i + 11 > bytes.len() { return None; }
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() { return None; }
    if !bytes[i..i + 11].iter().all(|b| b.is_ascii_digit()) { return None; }
    if i + 11 < bytes.len() && bytes[i + 11].is_ascii_alphanumeric() { return None; }
    let d: Vec<u8> = bytes[i..i + 11].iter().map(|b| b - b'0').collect();
    if validate_cpf(&d) { Some(i..i + 11) } else { None }
}

/// Receita Federal CPF checksum algorithm.
/// Returns false for all-same-digit sequences and invalid verifier digits.
fn validate_cpf(d: &[u8]) -> bool {
    if d.len() != 11 { return false; }
    if d.windows(2).all(|w| w[0] == w[1]) { return false; }
    let s1: u32 = d[..9].iter().enumerate().map(|(i, &x)| x as u32 * (10 - i as u32)).sum();
    let v1 = if s1 % 11 < 2 { 0 } else { 11 - s1 % 11 };
    if v1 != d[9] as u32 { return false; }
    let s2: u32 = d[..10].iter().enumerate().map(|(i, &x)| x as u32 * (11 - i as u32)).sum();
    let v2 = if s2 % 11 < 2 { 0 } else { 11 - s2 % 11 };
    v2 == d[10] as u32
}

// ── CNPJ ──────────────────────────────────────────────────────────────────────
// Formats: NN.NNN.NNN/NNNN-NN (18 chars) or 14 consecutive digits at word boundary.
// Checksum: Receita Federal two-verifier algorithm with weights [5..2,9..2] / [6..2,9..2].

fn try_cnpj_formatted(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    // NN.NNN.NNN/NNNN-NN = 18 bytes
    if i + 18 > bytes.len() { return None; }
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() { return None; }
    let b = &bytes[i..i + 18];
    if !b[..2].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[2] != b'.' { return None; }
    if !b[3..6].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[6] != b'.' { return None; }
    if !b[7..10].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[10] != b'/' { return None; }
    if !b[11..15].iter().all(|x| x.is_ascii_digit()) { return None; }
    if b[15] != b'-' { return None; }
    if !b[16..18].iter().all(|x| x.is_ascii_digit()) { return None; }
    if i + 18 < bytes.len() && bytes[i + 18].is_ascii_alphanumeric() { return None; }

    let d = [b[0]-b'0', b[1]-b'0', b[3]-b'0', b[4]-b'0', b[5]-b'0',
             b[7]-b'0', b[8]-b'0', b[9]-b'0', b[11]-b'0', b[12]-b'0',
             b[13]-b'0', b[14]-b'0', b[16]-b'0', b[17]-b'0'];
    if validate_cnpj(&d) { Some(i..i + 18) } else { None }
}

fn try_cnpj_raw(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    if i + 14 > bytes.len() { return None; }
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() { return None; }
    if !bytes[i..i + 14].iter().all(|b| b.is_ascii_digit()) { return None; }
    if i + 14 < bytes.len() && bytes[i + 14].is_ascii_alphanumeric() { return None; }
    let d: Vec<u8> = bytes[i..i + 14].iter().map(|b| b - b'0').collect();
    if validate_cnpj(&d) { Some(i..i + 14) } else { None }
}

/// Receita Federal CNPJ checksum algorithm.
/// weights_v1 = [5,4,3,2,9,8,7,6,5,4,3,2], weights_v2 = [6,5,4,3,2,9,8,7,6,5,4,3,2]
fn validate_cnpj(d: &[u8]) -> bool {
    if d.len() != 14 { return false; }
    if d.windows(2).all(|w| w[0] == w[1]) { return false; }
    const W1: [u32; 12] = [5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    const W2: [u32; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    let s1: u32 = d[..12].iter().zip(W1.iter()).map(|(&x, &w)| x as u32 * w).sum();
    let v1 = if s1 % 11 < 2 { 0 } else { 11 - s1 % 11 };
    if v1 != d[12] as u32 { return false; }
    let s2: u32 = d[..13].iter().zip(W2.iter()).map(|(&x, &w)| x as u32 * w).sum();
    let v2 = if s2 % 11 < 2 { 0 } else { 11 - s2 % 11 };
    v2 == d[13] as u32
}

// ── Brazilian phone numbers ────────────────────────────────────────────────────
// Supported formats (DDD required, 9-digit mobile and 8-digit landline):
//   (NN) NNNNN-NNNN   mobile with space
//   (NN) NNNN-NNNN    landline with space
//   (NN)NNNNN-NNNN    mobile without space
//   (NN)NNNN-NNNN     landline without space
//
// No checksum available. Detection relies on format only — the outer word-boundary
// check and the mandatory opening parenthesis + area-code + hyphen reduce false positives.

fn try_br_phone(bytes: &[u8], i: usize) -> Option<Range<usize>> {
    if i >= bytes.len() || bytes[i] != b'(' { return None; }
    if i + 14 > bytes.len() { return None; } // minimum possible length
    if !bytes[i + 1].is_ascii_digit() || !bytes[i + 2].is_ascii_digit() { return None; }
    if bytes[i + 3] != b')' { return None; }

    // Optional space after closing paren
    let mut j = i + 4;
    if j < bytes.len() && bytes[j] == b' ' { j += 1; }

    // First digit group (4 or 5 digits)
    let dstart = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() { j += 1; }
    let first = j - dstart;
    if first != 4 && first != 5 { return None; }
    if j >= bytes.len() || bytes[j] != b'-' { return None; }
    j += 1;

    // Second digit group (always 4 digits for Brazilian numbers)
    let sstart = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() { j += 1; }
    if j - sstart != 4 { return None; }

    // Word boundary after
    if j < bytes.len() && bytes[j].is_ascii_alphanumeric() { return None; }
    Some(i..j)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Price ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_price_max_r_dollar() {
        let p = parse("tênis R$300");
        assert_eq!(p.filters.get("price_max").and_then(|v| v.as_f64()), Some(300.0));
    }

    #[test]
    fn test_price_max_ate() {
        let p = parse("tênis até 250,90");
        assert_eq!(p.filters.get("price_max").and_then(|v| v.as_f64()), Some(250.9));
    }

    #[test]
    fn test_price_max_menos_de() {
        let p = parse("calçado menos de R$150");
        assert_eq!(p.filters.get("price_max").and_then(|v| v.as_f64()), Some(150.0));
    }

    #[test]
    fn test_price_sanity_floor() {
        // 0.0 is below MIN_PRICE → ignored
        let p = parse("produto até R$0");
        assert!(p.filters.get("price_max").is_none());
    }

    #[test]
    fn test_price_sanity_ceiling() {
        // 1_000_001 is above MAX_PRICE → ignored
        let p = parse("produto até R$1000001");
        assert!(p.filters.get("price_max").is_none());
    }

    // ── Urgency ───────────────────────────────────────────────────────────────

    #[test]
    fn test_urgency_sets_in_stock() {
        let p = parse("tênis entrega hoje");
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
        assert!((p.availability_boost - AVAILABILITY_BOOST).abs() < f32::EPSILON);
    }

    #[test]
    fn test_urgency_amanha() {
        let p = parse("notebook amanha urgente");
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
    }

    // ── CEP ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_cep_hyphenated_removed() {
        let p = parse("entrega 01310-100 tênis");
        assert!(!p.q.contains("01310"), "CEP should be removed");
        assert!(p.q.contains("tênis"));
    }

    #[test]
    fn test_cep_raw_removed() {
        let p = parse("01310100 tênis running");
        assert!(!p.q.contains("01310100"));
        assert!(p.q.contains("tênis"));
    }

    // ── CPF ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_cpf_formatted_removed() {
        // 529.982.247-25 is a well-known valid test CPF
        let p = parse("compra 529.982.247-25 produto");
        assert!(!p.q.contains("529.982.247-25"), "CPF should be removed");
        assert!(p.q.contains("compra"));
        assert!(p.q.contains("produto"));
        assert!(p.applied.iter().any(|s| s == "removed:cpf"));
    }

    #[test]
    fn test_cpf_raw_removed() {
        let p = parse("52998224725 produto");
        assert!(!p.q.contains("52998224725"));
        assert!(p.q.contains("produto"));
    }

    #[test]
    fn test_invalid_cpf_not_removed() {
        // 111.111.111-11 passes digit count but fails checksum (all-same rejected)
        let p = parse("código 111.111.111-11 produto");
        assert!(p.q.contains("111.111.111-11"), "invalid CPF must not be removed");
    }

    #[test]
    fn test_random_11_digits_not_removed() {
        // Random sequence that does not pass CPF checksum
        let p = parse("código 12345678901 produto");
        assert!(p.q.contains("12345678901"), "non-CPF 11-digit sequence must not be removed");
    }

    // ── CNPJ ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_cnpj_formatted_removed() {
        // 11.222.333/0001-81 is a standard test CNPJ with valid checksum
        let p = parse("empresa 11.222.333/0001-81 produto");
        assert!(!p.q.contains("11.222.333/0001-81"), "CNPJ should be removed");
        assert!(p.q.contains("empresa"));
        assert!(p.applied.iter().any(|s| s == "removed:cnpj"));
    }

    #[test]
    fn test_cnpj_raw_removed() {
        let p = parse("11222333000181 produto");
        assert!(!p.q.contains("11222333000181"));
    }

    #[test]
    fn test_invalid_cnpj_not_removed() {
        let p = parse("código 11.111.111/1111-11 produto");
        assert!(p.q.contains("11.111.111/1111-11"), "invalid CNPJ must not be removed");
    }

    // ── Phone ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_mobile_phone_removed() {
        let p = parse("ligar (11) 99999-9999 produto");
        assert!(!p.q.contains("(11) 99999-9999"), "phone should be removed");
        assert!(p.q.contains("produto"));
        assert!(p.applied.iter().any(|s| s == "removed:phone"));
    }

    #[test]
    fn test_landline_phone_removed() {
        let p = parse("fax (21) 3333-4444 pedido");
        assert!(!p.q.contains("(21) 3333-4444"));
        assert!(p.q.contains("pedido"));
    }

    #[test]
    fn test_phone_no_space_removed() {
        let p = parse("(31)98765-4321 produto");
        assert!(!p.q.contains("(31)98765-4321"));
    }

    // ── Combined ──────────────────────────────────────────────────────────────

    #[test]
    fn test_plain_query_unchanged() {
        let p = parse("tênis nike running");
        assert_eq!(p.q, "tênis nike running");
        assert!(p.filters.is_empty());
        assert!((p.availability_boost - 1.0).abs() < f32::EPSILON);
        assert!(p.applied.is_empty());
    }

    #[test]
    fn test_combined_price_urgency_cep() {
        let p = parse("tênis até R$200 entrega amanhã 01310-100");
        assert_eq!(p.filters.get("price_max").and_then(|v| v.as_f64()), Some(200.0));
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
        assert!(!p.q.contains("01310"), "CEP removed");
        assert!(p.q.contains("tênis"));
    }
}
