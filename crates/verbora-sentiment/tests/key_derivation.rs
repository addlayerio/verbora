//! The table's keys and the scoring loop's probes are one derivation, and this
//! file enumerates that: every key of every shipped vocabulary, through both
//! sides, for every stemmer that can be installed with it.
//!
//! # Why this file exists
//!
//! A stemmed table is built by stemming the pieces of every key and rejoining
//! them; the scoring loop matches it by stemming the pieces of every token and
//! rejoining them. Those are the same sentence twice, and for a while they were
//! two pieces of code: the builder segmented the stemmer's output a second time
//! before filing it, and the loop probed with the stemmer's output verbatim. So
//! `ofendre's` — Porter-stemmed to `ofendre'` — was filed under `ofendre`,
//! where it answered for the unrelated `ofendre` key it had displaced, and the
//! probe `ofendre'` reached nothing at all.
//!
//! That is the same defect the span-matching work was written to remove, one
//! layer down: an index built on one spelling and consulted with another. It
//! survived a test that checked three `(vocabulary, stemmer)` pairs, because
//! all three happened to contain no key whose stem carries a character the
//! tokenizer strips. Across the fourteen vocabularies and the sixteen
//! installable stemmers, 228 keys scored some other entry's polarity or none.
//!
//! So the rule here is the one `reachability.rs` already follows and is
//! extended to the second axis: walk *every* key of *every* table through
//! *every* stemmer, and compute the expected answer in this file from the
//! contract in [`verbora_sentiment`]'s own documentation rather than asking the
//! crate for it.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use verbora_sentiment::{
    Language, NoStemmer, Polarity, SentimentAnalyzer, Stemmer, Vocabulary, VocabularyKind,
    supported_pairs,
};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

// ---------------------------------------------------------------------------
// The contract, re-implemented here so the crate cannot move its own target.
// ---------------------------------------------------------------------------

/// The **pieces** of a string: its `WordTokenizer` word segments, lowercased —
/// or, for a string with no word segment at all, the string lowercased, as one
/// piece.
///
/// The one-piece fallback is the same rule on both sides of the pipeline: it is
/// how `😂` is indexed, and it is what a caller must hand over as a token to
/// reach that entry, since `WordTokenizer` yields no segment for it.
fn pieces(text: &str) -> Vec<String> {
    let segments: Vec<String> = WordTokenizer.tokens(text).map(str::to_lowercase).collect();
    if segments.is_empty() {
        vec![text.to_lowercase()]
    } else {
        segments
    }
}

/// Those pieces after a stemmer sees them: one stem per piece, and a piece
/// whose stem is empty keeps its own spelling.
fn stemmed_pieces<S: Stemmer>(pieces: &[String], stemmer: &S) -> Vec<String> {
    pieces
        .iter()
        .map(|piece| {
            let stem = stemmer.stem(piece);
            if stem.is_empty() {
                piece.clone()
            } else {
                stem.into_owned()
            }
        })
        .collect()
}

/// The **lookup form** of a run of pieces: the pieces joined by one U+0020.
fn form(pieces: &[String]) -> String {
    pieces.join(" ")
}

/// Every `(kind, language)` pair whose table has at least one entry.
fn tables() -> Vec<(VocabularyKind, Language, &'static Vocabulary)> {
    supported_pairs()
        .filter_map(|(kind, language)| {
            let voca = Vocabulary::shared(kind, language)?;
            (!voca.is_empty()).then_some((kind, language, voca))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The enumeration.
// ---------------------------------------------------------------------------

/// One stemmer against all thirteen non-empty tables.
///
/// Three things are asserted per table, and they are three different claims:
///
/// 1. **The table's keys are exactly the strings the scoring loop produces.**
///    Set equality both ways — no stored key the loop can never spell, and no
///    probe the table does not hold. This is the property that failed.
/// 2. **Each of those keys holds the last-wins polarity**, so the collision
///    rule survived the rebuild.
/// 3. **Scoring the key through the analyzer finds it**, as one unit, at that
///    polarity — the end-to-end statement the first two imply and this checks.
fn check<S: Stemmer + Clone>(label: &str, stemmer: S) {
    for (kind, language, base) in tables() {
        let analyzer = SentimentAnalyzer::with_stemmer(language, kind, stemmer.clone())
            .unwrap_or_else(|e| panic!("{label} {kind:?}/{language}: {e}"));
        let rebuilt = analyzer.vocabulary();
        let negations = analyzer.negations();

        // The probe the scoring loop builds for each key, and the entry that
        // wins it: last in file order, as the contract says for every
        // collision this crate has.
        let mut winners: HashMap<String, f64> = HashMap::new();
        let mut probes: Vec<(String, String)> = Vec::with_capacity(base.len());
        for (key, polarity) in base.iter() {
            let probe = form(&stemmed_pieces(&pieces(key), &stemmer));
            winners.insert(probe.clone(), polarity.value());
            probes.push((key.to_owned(), probe));
        }

        // 1. The stored keys are exactly the probes, both ways.
        let stored: HashMap<&str, f64> = rebuilt.iter().map(|(k, p)| (k, p.value())).collect();
        assert_eq!(
            stored.len(),
            rebuilt.len(),
            "{label} {kind:?}/{language}: a rebuilt table must hold each lookup \
             form once"
        );
        let stored_keys: HashSet<&str> = stored.keys().copied().collect();
        let probe_forms: HashSet<&str> = winners.keys().map(String::as_str).collect();
        let unreachable: Vec<&&str> = stored_keys.difference(&probe_forms).take(8).collect();
        assert!(
            unreachable.is_empty(),
            "{label} {kind:?}/{language}: {} stored keys are spellings the \
             scoring loop never produces, e.g. {unreachable:?}",
            stored_keys.difference(&probe_forms).count()
        );
        let missing: Vec<&&str> = probe_forms.difference(&stored_keys).take(8).collect();
        assert!(
            missing.is_empty(),
            "{label} {kind:?}/{language}: {} probes reach no entry, e.g. \
             {missing:?}",
            probe_forms.difference(&stored_keys).count()
        );

        for (key, probe) in &probes {
            // 2. …holding the last-wins polarity.
            let expected = winners[probe];
            assert_eq!(
                stored.get(probe.as_str()),
                Some(&expected),
                "{label} {kind:?}/{language}: {key:?} probes {probe:?}"
            );

            // 3. …and the analyzer finds it, as one unit.
            //
            // Two documented cases score something else and are stated rather
            // than skipped: a key with no word segment is not reachable through
            // `WordTokenizer` at all, and a single-piece key that is also a
            // negation word takes the negation branch, which runs first and
            // scores nothing.
            let key_pieces = pieces(key);
            let segmented = WordTokenizer.tokens(key).count() > 0;
            if !segmented {
                continue;
            }
            if key_pieces.len() == 1 && negations.contains(&key_pieces[0].as_str()) {
                let scored = analyzer.score(WordTokenizer.tokens(key));
                assert_eq!(scored.count, 1, "{label} {kind:?}/{language}: {key:?}");
                assert_eq!(scored.sum, 0.0, "{label} {kind:?}/{language}: {key:?}");
                continue;
            }
            // The loop looks a token up unstemmed *before* it stems it, so a
            // key whose own lookup form is some other key's stem lands on that
            // other key — the documented crosstalk a stemmer buys.
            let unstemmed = form(&key_pieces);
            let expected = winners.get(&unstemmed).copied().unwrap_or(expected);
            let scored = analyzer.score(WordTokenizer.tokens(key));
            assert_eq!(
                scored.count, 1,
                "{label} {kind:?}/{language}: {key:?} must score as one unit"
            );
            assert_eq!(
                scored.sum, expected,
                "{label} {kind:?}/{language}: {key:?} scored {} through the \
                 rebuilt table; lookup form {unstemmed:?}, probe {probe:?}",
                scored.sum
            );
        }
    }
}

/// Every vocabulary against every stemmer that can be installed with it.
///
/// The list below is every type `verbora_sentiment::stemmer`'s `bridge!` macro
/// implements [`Stemmer`] for, plus [`NoStemmer`]. Any stemmer at all can be
/// installed — [`Stemmer`] is a public trait — so this is the shipped closure
/// of it, and a new bridged stemmer belongs here in the same change.
#[test]
fn probe_and_table_agree_for_every_key_of_every_vocabulary_and_stemmer() {
    check("NoStemmer", NoStemmer);
    check("CarryStemmerFr", verbora_stemmers::CarryStemmerFr::new());
    check(
        "LancasterStemmer",
        verbora_stemmers::LancasterStemmer::new(),
    );
    check("PorterStemmer", verbora_stemmers::PorterStemmer::new());
    check("PorterStemmerDe", verbora_stemmers::PorterStemmerDe::new());
    check("PorterStemmerEs", verbora_stemmers::PorterStemmerEs::new());
    check("PorterStemmerFa", verbora_stemmers::PorterStemmerFa::new());
    check("PorterStemmerFr", verbora_stemmers::PorterStemmerFr::new());
    check("PorterStemmerIt", verbora_stemmers::PorterStemmerIt::new());
    check("PorterStemmerNl", verbora_stemmers::PorterStemmerNl::new());
    check("PorterStemmerNo", verbora_stemmers::PorterStemmerNo::new());
    check("PorterStemmerPt", verbora_stemmers::PorterStemmerPt::new());
    check("PorterStemmerRu", verbora_stemmers::PorterStemmerRu::new());
    check("PorterStemmerSv", verbora_stemmers::PorterStemmerSv::new());
    check("PorterStemmerUk", verbora_stemmers::PorterStemmerUk::new());
    check("StemmerJa", verbora_stemmers::StemmerJa::new());
}

/// The four keys the old two-path derivation got wrong, named so a regression
/// is legible rather than a count.
///
/// Each is a key whose stem carries a character `WordTokenizer` does not put in
/// a word segment, so re-segmenting the stem — which is what the builder used
/// to do before filing it — produced a spelling the scoring loop never builds.
#[test]
fn a_stem_that_ends_in_punctuation_is_filed_and_probed_the_same_way() {
    let porter = verbora_stemmers::PorterStemmer::new();

    // The one that inverted rather than vanishing: `ofendre's` stems to
    // `ofendre'`, which re-segments to `ofendre` — the lookup form of a
    // *different* shipped key with a different polarity.
    let ca = Vocabulary::shared(VocabularyKind::Senticon, Language::Catalan).unwrap();
    let analyzer =
        SentimentAnalyzer::with_stemmer(Language::Catalan, VocabularyKind::Senticon, porter)
            .unwrap();
    assert_eq!(ca.get("ofendre").map(Polarity::value), Some(-0.594));
    assert_eq!(ca.get("ofendre's").map(Polarity::value), Some(-0.375));
    assert_eq!(porter.stem("ofendre"), "ofendr");
    assert_eq!(porter.stem("ofendre's"), "ofendre'");
    // `ofendre` stems to `ofendr`; the token `ofendre` is looked up unstemmed
    // first, misses, and then finds its own stem. It must not find `ofendre's`.
    assert_eq!(analyzer.get_sentiment(["ofendre"]), Some(-0.594));

    // The ones that fell to zero: a possessive phrase key, four tokens or two.
    let en = SentimentAnalyzer::with_stemmer(Language::English, VocabularyKind::Senticon, porter)
        .unwrap();
    let base = Vocabulary::shared(VocabularyKind::Senticon, Language::English).unwrap();
    for key in ["dog's breakfast", "writer's block", "ne'er-do-well"] {
        let expected = base
            .get(key)
            .map(Polarity::value)
            .unwrap_or_else(|| panic!("{key} ships"));
        let scored = en.score(WordTokenizer.tokens(key));
        assert_eq!(scored.count, 1, "{key}");
        assert_eq!(scored.sum, expected, "{key}");
    }
}

/// The rebuild costs one `stem` call per piece and not one more.
///
/// This is a correctness assertion, not a performance one: an extra derivation
/// — stemming the whole key alongside its pieces, say — shows up here as extra
/// calls, and it is exactly how a third spelling of a key would get into the
/// index. It also pins the count `Vocabulary::stemmed` publishes.
#[test]
fn the_rebuild_makes_one_stem_call_per_piece() {
    struct Counting<'c, S> {
        calls: &'c Cell<usize>,
        inner: S,
    }

    impl<S: Stemmer> Stemmer for Counting<'_, S> {
        fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str> {
            self.calls.set(self.calls.get() + 1);
            self.inner.stem(word)
        }
    }

    for (kind, language, expected_entries, expected_calls) in [
        (
            VocabularyKind::Senticon,
            Language::English,
            24_839usize,
            33_874usize,
        ),
        (VocabularyKind::Afinn, Language::English, 3_382, 3_443),
    ] {
        let base = Vocabulary::shared(kind, language).unwrap();
        assert_eq!(base.len(), expected_entries);
        let by_hand: usize = base.keys().map(|key| pieces(key).len()).sum();
        assert_eq!(
            by_hand, expected_calls,
            "{kind:?}/{language}: piece count changed"
        );

        let calls = Cell::new(0);
        let counting = Counting {
            calls: &calls,
            inner: verbora_stemmers::PorterStemmer::new(),
        };
        let _rebuilt = base.stemmed(&counting);
        assert_eq!(
            calls.get(),
            expected_calls,
            "{kind:?}/{language}: the rebuild stems something other than the \
             pieces of each key"
        );
    }
}

/// What a Porter rebuild costs English AFINN, enumerated rather than quoted.
#[test]
fn english_afinn_loses_121_polarities_to_stem_collisions() {
    let base = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
    let stemmer = verbora_stemmers::PorterStemmer::new();
    let rebuilt = base.stemmed(&stemmer);

    let mut winners: HashMap<String, f64> = HashMap::new();
    for (key, polarity) in base.iter() {
        winners.insert(
            form(&stemmed_pieces(&pieces(key), &stemmer)),
            polarity.value(),
        );
    }
    let changed = base
        .iter()
        .filter(|(key, polarity)| {
            winners[&form(&stemmed_pieces(&pieces(key), &stemmer))] != polarity.value()
        })
        .count();

    assert_eq!(base.len(), 3382);
    assert_eq!(winners.len(), 1967, "distinct stem forms");
    assert_eq!(rebuilt.len(), 1967);
    assert_eq!(changed, 121, "entries that no longer answer with their own");
}

/// The headline "N of the 75,803 shipped entries" figure, by enumeration.
///
/// The number answers "how many entries does a token stream need span matching
/// to reach", which is a question about where `WordTokenizer` cuts — so it is
/// counted by cutting. The `contains('-') || contains(' ')` filter it used to
/// be derived with is wrong in both directions at once, and both directions are
/// asserted here so the shortcut is not reintroduced as an optimisation.
#[test]
fn the_multi_piece_count_is_derived_by_walking_the_tokenizer() {
    let mut entries = 0usize;
    let mut multi_piece = 0usize;
    let mut string_filter = 0usize;
    let (mut hyphen, mut space, mut both) = (0usize, 0usize, 0usize);
    let mut missed_by_filter: Vec<String> = Vec::new();
    let mut over_claimed_by_filter: Vec<String> = Vec::new();

    for (kind, language) in supported_pairs() {
        let Some(voca) = Vocabulary::shared(kind, language) else {
            continue;
        };
        for key in voca.keys() {
            entries += 1;
            let cut = WordTokenizer.tokens(key).count() >= 2;
            let guessed = key.contains('-') || key.contains(' ');
            if cut {
                multi_piece += 1;
                if key.contains('-') {
                    hyphen += 1;
                }
                if key.contains(' ') {
                    space += 1;
                }
                if key.contains('-') && key.contains(' ') {
                    both += 1;
                }
            }
            if guessed {
                string_filter += 1;
            }
            match (cut, guessed) {
                (true, false) => missed_by_filter.push(key.to_owned()),
                (false, true) => over_claimed_by_filter.push(key.to_owned()),
                _ => {}
            }
        }
    }

    assert_eq!(entries, 75_803);
    assert_eq!(multi_piece, 14_273);
    assert_eq!(string_filter, 14_268);
    // The breakdown `reachability.rs` quotes, so it cannot drift either.
    assert_eq!((hyphen, space, both), (2_510, 11_953, 198));
    assert_eq!(hyphen + space - both + missed_by_filter.len(), multi_piece);
    assert_eq!(
        missed_by_filter,
        [
            "encender(se)",
            "s/n",
            "señal/ruido",
            "signal/noise",
            "cal”ligrafia",
            "rompre´s",
            "passarel”la",
            "f*cking",
        ]
    );
    assert_eq!(
        over_claimed_by_filter,
        ["ultratge ", "herri-", "azkartasun "]
    );
}
