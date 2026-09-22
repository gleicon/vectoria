//! # Query Understanding — pt-BR Phonetic Normalization
//!
//! Normalizes Portuguese text tokens so phonetically equivalent spellings
//! produce the same BM25 tokens. Applied symmetrically at index time and
//! query time so matches are consistent.
//!
//! SymSpell's `AsciiStringStrategy` already handles diacritics for spell
//! correction; this module covers the BM25 path which has no built-in
//! Portuguese normalization.
//!
//! ## Rules (applied in order)
//!
//! | Rule                 | Example                         |
//! |----------------------|---------------------------------|
//! | Lowercase            | "Tênis" → "tênis"               |
//! | Diacritic fold       | "ção" → "cao", "ê" → "e"        |
//! | `ph` → `f`           | "pharmacia" → "farmacia"        |
//! | Word-final `y` → `i` | "suely" → "sueli"               |
//!
//! Double-consonant collapse (ss→s, rr→r) is intentionally omitted: in Portuguese,
//! "rr" and "r" are distinct phonemes ("carro" ≠ "caro") and SymSpell already handles
//! the common "impressão"/"impresão" spelling variant via edit-distance.
//!
//! ## Tunable points
//!
//! | API                    | Effect                                           |
//! |------------------------|--------------------------------------------------|
//! | `normalize(text)`      | Apply all rules to every whitespace-delimited    |
//! |                        | purely-alphabetic token; mixed tokens pass through|

/// Normalize a text string for pt-BR phonetic equivalence.
/// Applies the rule table in module docs to every whitespace-delimited token.
/// Purely-alphabetic tokens are normalized; mixed tokens (product codes, numbers) pass through.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            if token.chars().all(|c| c.is_alphabetic()) {
                normalize_token(token)
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_token(token: &str) -> String {
    let folded: String = token.chars().map(|c| fold_char(c.to_lowercase().next().unwrap_or(c))).collect();
    let folded = folded.replace("ph", "f");
    if folded.ends_with('y') {
        let mut s = folded[..folded.len() - 1].to_string();
        s.push('i');
        s
    } else {
        folded
    }
}

/// Map accented characters to their ASCII base form.
fn fold_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diacritic_fold() {
        assert_eq!(normalize("tênis"), "tenis");
        assert_eq!(normalize("calçado"), "calcado");
        assert_eq!(normalize("óculos"), "oculos");
        assert_eq!(normalize("ação"), "acao");
    }

    #[test]
    fn test_name_variant_y_to_i() {
        assert_eq!(normalize("suely"), "sueli");
        assert_eq!(normalize("daisy"), "daisi");
        // Mid-word 'y' is not touched — only word-final position is substituted.
        assert_eq!(normalize("rayon"), "rayon");
    }

    #[test]
    fn test_y_to_i_only_on_alpha_tokens() {
        // Mixed token (e.g. product code "Y-50") — passes through unchanged
        assert_eq!(normalize("Y-50"), "Y-50");
    }

    #[test]
    fn test_ph_to_f() {
        assert_eq!(normalize("pharmacia"), "farmacia");
        assert_eq!(normalize("pharma"), "farma");
    }

    #[test]
    fn test_phonetic_equivalence_sueli() {
        assert_eq!(normalize("sueli"), normalize("suely"));
    }

    #[test]
    fn test_multiword() {
        assert_eq!(normalize("tênis Adidas corrida"), "tenis adidas corrida");
    }

    #[test]
    fn test_non_alpha_passthrough() {
        // Numbers and product codes must not be mangled
        assert_eq!(normalize("42"), "42");
        assert_eq!(normalize("R$150"), "R$150");
    }
}
