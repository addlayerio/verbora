//! Train-side scoring vs. shipped inference, end to end.
//!
//! `langdetect_train::Model::score`/`predict` duplicate the inference
//! formula (they must — training scores with weights that are not
//! compiled statics yet). This test closes the loop: rebuild the
//! train-side `Model` from the *compiled-in* generated weights and assert
//! its verdict matches the real `HashedLinearDetector` on real sentences.
//! Combined with `tokenizer_differential.rs` (same feature stream) this
//! pins every numeric step the trainer shares with inference.

use langdetect_train::{Model, featurize, featurize_cyrillic};
use verbora_language::train_support::{CYRILLIC_CLASSES, LATIN_CLASSES};
use verbora_language::{HashedLinearDetector, Language, LanguageDetector, Script, detect_script};

/// The generated weights are `pub` only inside a private module of
/// `verbora-language`, deliberately — so this test re-parses the
/// generated source file instead of importing it. Crude but honest: it
/// checks the exact artifact the crate compiles.
fn parse_generated_table(source: &str, name: &str) -> Vec<f32> {
    let start = source
        .find(&format!("pub static {name}:"))
        .unwrap_or_else(|| panic!("{name} not found in generated weights"));
    let open = source[start..].find('[').unwrap() + start;
    let open = source[open + 1..].find('[').unwrap() + open + 1; // second '[' opens the literal
    let close = source[open..].find(']').unwrap() + open;
    source[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f32>().unwrap())
        .collect()
}

fn model_from_generated(weights_name: &str, intercepts_name: &str, n: usize) -> Model {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/verbora-language/src/hashed_linear_weights.rs"
    ))
    .expect("generated weights file must exist");
    let weights = parse_generated_table(&source, weights_name);
    let intercepts = parse_generated_table(&source, intercepts_name);
    assert_eq!(weights.len(), 4096 * n);
    assert_eq!(intercepts.len(), n);
    Model {
        weights,
        intercepts,
        n_classes: n,
    }
}

#[test]
fn latin_model_agrees_with_shipped_detector() {
    let model = model_from_generated("LATIN_WEIGHTS", "LATIN_INTERCEPTS", 16);
    let detector = HashedLinearDetector::new();
    let sentences = [
        "The weather is beautiful today and the children are playing outside.",
        "El tiempo es hermoso hoy y los niños juegan afuera en el parque.",
        "Le temps est magnifique aujourd'hui et les enfants jouent dehors.",
        "Das Wetter ist heute schön und die Kinder spielen draußen im Park.",
        "O tempo está bonito hoje e as crianças brincam lá fora no parque.",
        "Il tempo è bello oggi e i bambini giocano fuori nel parco.",
        "Het weer is vandaag mooi en de kinderen spelen buiten in het park.",
        "Vädret är vackert idag och barnen leker ute i parken tillsammans.",
    ];
    for text in sentences {
        assert_eq!(
            detect_script(text),
            Some(Script::Latin),
            "fixture must be Latin"
        );
        let Some(candidate) = detector.detect(text).best().copied() else {
            // Shipped detector abstained (margin below threshold) — the
            // train-side margin must then also sit below the compiled
            // threshold; skip the language comparison.
            continue;
        };
        let (class, _margin) = model
            .predict(&featurize(text))
            .expect("sentence has features");
        assert_eq!(
            LATIN_CLASSES[class], candidate.language,
            "train-side scoring disagrees with inference on {text:?}"
        );
    }
}

#[test]
fn cyrillic_model_agrees_with_shipped_detector() {
    let model = model_from_generated("CYRILLIC_WEIGHTS", "CYRILLIC_INTERCEPTS", 2);
    let detector = HashedLinearDetector::new();
    for text in [
        "Сегодня прекрасная погода, и дети играют на улице в парке.",
        "Сьогодні чудова погода, і діти граються надворі в парку.",
    ] {
        let Some(candidate) = detector.detect(text).best().copied() else {
            continue;
        };
        let (class, _margin) = model
            .predict(&featurize_cyrillic(text))
            .expect("sentence has features");
        assert_eq!(
            CYRILLIC_CLASSES[class], candidate.language,
            "train-side scoring disagrees with inference on {text:?}"
        );
    }
}

#[test]
fn class_orders_match_the_language_enum_declaration_order() {
    // The corpus table in src/main.rs, the class arrays re-exported by the
    // crate, and Language::ALL must agree on ordering — a silent
    // permutation here would train correct models whose columns mean the
    // wrong language.
    let latin_from_all: Vec<Language> = Language::ALL
        .into_iter()
        .filter(|l| LATIN_CLASSES.contains(l))
        .collect();
    assert_eq!(latin_from_all, LATIN_CLASSES.to_vec());
    let cyrillic_from_all: Vec<Language> = Language::ALL
        .into_iter()
        .filter(|l| CYRILLIC_CLASSES.contains(l))
        .collect();
    assert_eq!(cyrillic_from_all, CYRILLIC_CLASSES.to_vec());
}
