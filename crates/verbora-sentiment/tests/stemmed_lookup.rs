//! [`Vocabulary::get`] must find every key of every table, including a table
//! rebuilt through a stemmer.
//!
//! # The defect this file pins
//!
//! A rebuilt table's keys *are* lookup forms — `Vocabulary::stemmed` files each
//! entry under the pieces of its key, stemmed and rejoined, and indexes that
//! string verbatim because that is what the scoring loop probes with. But
//! `get` derived a lookup form from its argument before probing, so it handed
//! the index a form derived *from a form*: the one thing the module
//! documentation says nothing here may do.
//!
//! For a stem that carries a character `WordTokenizer` does not put in a word
//! segment, the two disagree. With `PorterStemmerFr` on English senticon, one
//! key stems to `ne'`; `get("ne'")` re-segmented that to `ne`, which is a
//! *different* entry of the same table with the opposite sign — `+0.25` where
//! the key is worth `-0.25`. Another 352 forms across the sixteen stemmers and
//! thirteen non-empty tables missed entirely.
//!
//! The scoring loop was never affected: it probes verbatim. Only the public
//! lookup was, which is why the existing enumerations — which score through the
//! analyzer — passed.

use std::collections::HashMap;

use verbora_sentiment::{
    Language, NoStemmer, Polarity, SentimentAnalyzer, Stemmer, Vocabulary, VocabularyKind,
    supported_pairs,
};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// The contract's pieces, re-implemented so the crate cannot move its target.
fn pieces(text: &str) -> Vec<String> {
    let segments: Vec<String> = WordTokenizer.tokens(text).map(str::to_lowercase).collect();
    if segments.is_empty() {
        vec![text.to_lowercase()]
    } else {
        segments
    }
}

/// Every key of every rebuilt table, looked up by the form it is stored under.
fn check<S: Stemmer + Clone>(label: &str, stemmer: S, totals: &mut (usize, usize)) {
    for (kind, language) in supported_pairs() {
        let Some(base) = Vocabulary::shared(kind, language) else {
            continue;
        };
        if base.is_empty() {
            continue;
        }
        let analyzer = SentimentAnalyzer::with_stemmer(language, kind, stemmer.clone())
            .unwrap_or_else(|e| panic!("{label} {kind:?}/{language}: {e}"));
        let rebuilt = analyzer.vocabulary();

        // The winner of each stemmed form: last entry in file order.
        let mut winners: HashMap<String, f64> = HashMap::new();
        for (key, polarity) in base.iter() {
            let form = pieces(key)
                .iter()
                .map(|piece| {
                    let stem = stemmer.stem(piece);
                    if stem.is_empty() {
                        piece.clone()
                    } else {
                        stem.into_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            winners.insert(form, polarity.value());
        }

        totals.0 += winners.len();
        for (form, expected) in &winners {
            assert_eq!(
                rebuilt.get(form).map(Polarity::value),
                Some(*expected),
                "{label} {kind:?}/{language}: get({form:?}) does not find the entry \
                 stored under that very form"
            );
            totals.1 += 1;
        }
    }
}

/// All sixteen installable stemmers against all thirteen non-empty tables.
#[test]
fn every_stored_form_is_found_by_the_public_lookup() {
    let mut totals = (0usize, 0usize);
    check("NoStemmer", NoStemmer, &mut totals);
    check(
        "CarryStemmerFr",
        verbora_stemmers::CarryStemmerFr::new(),
        &mut totals,
    );
    check(
        "LancasterStemmer",
        verbora_stemmers::LancasterStemmer::new(),
        &mut totals,
    );
    check(
        "PorterStemmer",
        verbora_stemmers::PorterStemmer::new(),
        &mut totals,
    );
    check(
        "PorterStemmerDe",
        verbora_stemmers::PorterStemmerDe::new(),
        &mut totals,
    );
    check(
        "PorterStemmerEs",
        verbora_stemmers::PorterStemmerEs::new(),
        &mut totals,
    );
    check(
        "PorterStemmerFa",
        verbora_stemmers::PorterStemmerFa::new(),
        &mut totals,
    );
    check(
        "PorterStemmerFr",
        verbora_stemmers::PorterStemmerFr::new(),
        &mut totals,
    );
    check(
        "PorterStemmerIt",
        verbora_stemmers::PorterStemmerIt::new(),
        &mut totals,
    );
    check(
        "PorterStemmerNl",
        verbora_stemmers::PorterStemmerNl::new(),
        &mut totals,
    );
    check(
        "PorterStemmerNo",
        verbora_stemmers::PorterStemmerNo::new(),
        &mut totals,
    );
    check(
        "PorterStemmerPt",
        verbora_stemmers::PorterStemmerPt::new(),
        &mut totals,
    );
    check(
        "PorterStemmerRu",
        verbora_stemmers::PorterStemmerRu::new(),
        &mut totals,
    );
    check(
        "PorterStemmerSv",
        verbora_stemmers::PorterStemmerSv::new(),
        &mut totals,
    );
    check(
        "PorterStemmerUk",
        verbora_stemmers::PorterStemmerUk::new(),
        &mut totals,
    );
    check("StemmerJa", verbora_stemmers::StemmerJa::new(), &mut totals);
    assert_eq!(totals.0, totals.1);
    assert_eq!(
        totals.0, 1_151_809,
        "the enumeration walked every stored form"
    );
}

/// The one form that answered another entry, named so a regression is legible
/// rather than a count.
#[test]
fn a_stem_ending_in_punctuation_finds_its_own_entry_and_not_its_prefix() {
    let stemmer = verbora_stemmers::PorterStemmerFr::new();
    let analyzer =
        SentimentAnalyzer::with_stemmer(Language::English, VocabularyKind::Senticon, stemmer)
            .unwrap();
    let rebuilt = analyzer.vocabulary();

    // Two distinct entries of the rebuilt table, one a re-segmentation of the
    // other, with opposite signs.
    let apostrophe = rebuilt.get("ne'").map(Polarity::value);
    let bare = rebuilt.get("ne").map(Polarity::value);
    assert_eq!(apostrophe, Some(-0.25));
    assert_eq!(bare, Some(0.25));
    assert_ne!(apostrophe, bare);

    // The scoring loop and the public lookup agree, which is the whole point.
    assert_eq!(analyzer.score(["ne'"]).sum, -0.25);
    assert_eq!(analyzer.score(["ne"]).sum, 0.25);

    // …and the Catalan case that merely vanished.
    let ca = SentimentAnalyzer::with_stemmer(
        Language::Catalan,
        VocabularyKind::Senticon,
        verbora_stemmers::PorterStemmer::new(),
    )
    .unwrap();
    assert_eq!(
        ca.vocabulary().get("ofendre'").map(Polarity::value),
        Some(-0.375)
    );
    assert_eq!(
        ca.vocabulary().get("ofendr").map(Polarity::value),
        Some(-0.594)
    );
}

/// Text spellings still reach a rebuilt phrase key, so the fix adds hits rather
/// than trading one set for another.
#[test]
fn a_rebuilt_phrase_key_is_still_reachable_from_its_text_spellings() {
    let base = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
    let stemmed = base.stemmed(&verbora_stemmers::PorterStemmer::new());
    for spelling in ["bad luck", "bad-luck", "Bad Luck", "BAD-LUCK"] {
        assert_eq!(
            stemmed.get(spelling).map(Polarity::value),
            Some(-2.0),
            "{spelling}"
        );
    }
}
