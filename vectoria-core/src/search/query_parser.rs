use std::collections::HashMap;

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

// Longest prefix first to avoid partial matches (e.g. "menos de" before "de")
const PRICE_PREFIXES: &[&str] = &[
    "por até r$", "por ate r$",
    "menos de r$", "menos de ",
    "até r$", "até ",
    "ate r$", "ate ",
    "por r$",
    "r$",
];

pub struct ParsedQuery {
    /// Query with CEP noise removed. Price and urgency tokens are kept — they may
    /// contribute to BM25 matching and are cheap to ignore via filters.
    pub q: String,
    /// Auto-extracted filters. Merged with user-provided filters before retrieval;
    /// user filters override auto-extracted ones on key collision.
    pub filters: HashMap<String, serde_json::Value>,
    /// Multiplier applied to the availability ranking weight when delivery urgency
    /// is detected. 1.0 means no change.
    pub availability_boost: f32,
}

pub fn parse(raw: &str) -> ParsedQuery {
    let lower = raw.to_lowercase();
    let mut filters: HashMap<String, serde_json::Value> = HashMap::new();

    if let Some(price) = extract_price_max(&lower) {
        filters.insert("price_max".into(), serde_json::Value::from(price));
    }

    if URGENCY_TERMS.iter().any(|t| lower.contains(t)) {
        filters.insert("in_stock".into(), serde_json::Value::Bool(true));
    }

    let availability_boost = if filters.contains_key("in_stock") { 2.0 } else { 1.0 };
    let q = remove_cep(raw).split_whitespace().collect::<Vec<_>>().join(" ");

    ParsedQuery { q, filters, availability_boost }
}

/// Scans the lowercased query for price ceiling patterns and returns the value.
/// Handles: "R$300", "até R$300", "até 300", "menos de R$299,90", "por R$50".
fn extract_price_max(lower: &str) -> Option<f64> {
    for prefix in PRICE_PREFIXES {
        if let Some(pos) = lower.find(prefix) {
            let after = &lower[pos + prefix.len()..];
            if let Some(v) = parse_brl_number(after) {
                return Some(v);
            }
        }
    }
    None
}

/// Parse a Brazilian decimal number (comma or dot as decimal separator).
/// Consumes leading digits, optional comma/dot, trailing digits.
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

/// Remove Brazilian postal codes (CEP) from the query — they are structural noise
/// with no relevance to product text matching.
/// Matches NNNNN-NNN or NNNNNNNN at word boundaries.
fn remove_cep(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = cep_span(bytes, i) {
            i = end;
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Returns the byte-end of a CEP starting at `i`, or None if no CEP there.
fn cep_span(bytes: &[u8], i: usize) -> Option<usize> {
    // Must be at a word boundary
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
        return None;
    }
    if i + 5 > bytes.len() || !bytes[i..i + 5].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut j = i + 5;
    if j < bytes.len() && bytes[j] == b'-' {
        j += 1;
    }
    if j + 3 > bytes.len() || !bytes[j..j + 3].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let end = j + 3;
    if end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
        return None;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_urgency_sets_in_stock() {
        let p = parse("tênis entrega hoje");
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
        assert!((p.availability_boost - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_urgency_amanha() {
        let p = parse("notebook amanha urgente");
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn test_cep_removed_from_query() {
        let p = parse("entrega 01310-100 tênis");
        assert!(!p.q.contains("01310"), "CEP should be removed from query");
        assert!(p.q.contains("tênis"));
    }

    #[test]
    fn test_cep_no_hyphen_removed() {
        let p = parse("01310100 tênis running");
        assert!(!p.q.contains("01310100"));
        assert!(p.q.contains("tênis"));
    }

    #[test]
    fn test_plain_query_unchanged() {
        let p = parse("tênis nike running");
        assert_eq!(p.q, "tênis nike running");
        assert!(p.filters.is_empty());
        assert!((p.availability_boost - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_combined_price_and_urgency() {
        let p = parse("tênis até R$200 entrega amanhã");
        assert_eq!(p.filters.get("price_max").and_then(|v| v.as_f64()), Some(200.0));
        assert_eq!(p.filters.get("in_stock").and_then(|v| v.as_bool()), Some(true));
    }
}
