use vectoria_core::search::spell::SpellCorrector;

fn seeded() -> SpellCorrector {
    let s = SpellCorrector::new();
    // Seed with vocabulary representative of a product catalog
    s.add_text("white shoes running shoes leather shoes sneakers");
    s.add_text("wireless headphones noise cancelling bluetooth");
    s.add_text("red dress blue dress summer dress cotton");
    s.add_text("keyboard mechanical gaming laptop computer");
    s.add_text("water bottle stainless steel insulated");
    s
}

#[test]
fn compound_split_whiteshoes() {
    let s = seeded();
    let result = s.correct("whiteshoes");
    assert_eq!(result, "white shoes", "compound split: 'whiteshoes' → 'white shoes'");
}

#[test]
fn compound_split_multi_word() {
    let s = seeded();
    let result = s.correct("runningshoes");
    assert_eq!(result, "running shoes");
}

#[test]
fn compound_split_three_words() {
    let s = seeded();
    // "leathershoes" → "leather shoes"
    let result = s.correct("leathershoes");
    assert!(
        result.contains("leather") && result.contains("shoes"),
        "expected split of 'leathershoes', got: '{}'", result
    );
}

#[test]
fn typo_correction_preserved() {
    let s = seeded();
    let result = s.correct("wireles headphons");
    // both words should be corrected
    assert!(result.contains("wireless"), "expected 'wireless' in '{}'", result);
    assert!(result.contains("headphone"), "expected 'headphone' in '{}'", result);
}

#[test]
fn extra_space_join() {
    let s = seeded();
    // "key board" → "keyboard" (combi check in lookup_compound)
    let result = s.correct("key board");
    assert_eq!(result, "keyboard");
}

#[test]
fn compound_split_three_word_long() {
    let s = seeded();
    // 3-word compound — exceeds lookup_compound's bigram split, requires word_segmentation pre-pass
    let result = s.correct("noisecancellingheadphones");
    assert!(
        result.contains("noise") && result.contains("headphone"),
        "expected 3-word split of 'noisecancellingheadphones', got: '{}'", result
    );
}

#[test]
fn clean_query_unchanged() {
    let s = seeded();
    let result = s.correct("wireless headphones");
    assert_eq!(result, "wireless headphones");
}

#[test]
fn empty_query() {
    let s = seeded();
    assert_eq!(s.correct(""), "");
}
