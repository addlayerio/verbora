//! Every key of every shipped vocabulary, walked through the documented
//! pipeline, must score the polarity it was shipped with.
//!
//! # Why this file enumerates instead of sampling
//!
//! The pipeline this crate documents is `WordTokenizer` → `get_sentiment`, and
//! the tokenizer is free to cut a key into pieces the analyzer then looks up
//! one at a time. When that happened silently, the 14,273 keys it cuts into two
//! or more pieces stopped being reachable and every test still passed, because
//! the handful of keys the tests named happened to be single words. Several of
//! the lost keys did not merely fall to zero — they inverted: `non-approved`
//! (-2) scored **+1** as `non` + `approved`.
//!
//! That 14,273 is counted by walking the tokenizer, in
//! `tests/key_derivation.rs`. Of those keys 2,510 contain a hyphen, 11,953
//! contain a space, 198 contain both, and 8 contain neither — cut at a slash,
//! a parenthesis, an acute accent, a curly quote or an asterisk instead. Those
//! last 8 are why counting the set with a `contains('-') || contains(' ')`
//! filter, as this number once was, gets a different answer.
//!
//! So the rule here is: walk *every* key of *every* table through the pipeline
//! and assert what it scores. The expected value is computed in this file from
//! the contract in [`verbora_sentiment`]'s own documentation — the lookup form
//! and the last-wins collision rule are re-implemented below rather than asked
//! of the crate, so a change to the crate's implementation cannot move the
//! target.

use std::collections::HashMap;

use verbora_sentiment::{
    Language, Polarity, SentimentAnalyzer, Stemmer, Vocabulary, VocabularyKind, supported_pairs,
};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// The contract's *pieces* of a string: its `WordTokenizer` word segments,
/// lowercased. May be empty — an emoji key has no word segment at all.
fn pieces(s: &str) -> Vec<String> {
    WordTokenizer.tokens(s).map(str::to_lowercase).collect()
}

/// The contract's *lookup form*: the pieces joined by one U+0020, or — for a
/// string with no pieces — the string lowercased.
fn form(s: &str) -> String {
    let p = pieces(s);
    if p.is_empty() {
        s.to_lowercase()
    } else {
        p.join(" ")
    }
}

/// The entry each lookup form resolves to: last key in file order wins.
fn winners(voca: &Vocabulary) -> HashMap<String, (String, f64)> {
    let mut out = HashMap::new();
    for (key, polarity) in voca.iter() {
        out.insert(form(key), (key.to_owned(), polarity.value()));
    }
    out
}

/// Every `(kind, language)` pair that has a table with at least one entry.
fn tables() -> impl Iterator<Item = (VocabularyKind, Language, &'static Vocabulary)> {
    supported_pairs().filter_map(|(kind, language)| {
        let voca = Vocabulary::shared(kind, language)?;
        (!voca.is_empty()).then_some((kind, language, voca))
    })
}

/// The whole point of the file: every key, every table, through the pipeline.
///
/// Each key is classified into exactly one of the contract's four cases and
/// asserted against the case's rule. The classification counts are printed and
/// the totals asserted, so a key silently changing class is a failure too.
#[test]
fn every_key_of_every_table_is_reachable_through_the_tokenizer() {
    let mut total_scored = 0usize;
    let mut total_shadowed = 0usize;
    let mut total_negation = 0usize;
    let mut total_unsegmentable = 0usize;

    for (kind, language, voca) in tables() {
        let analyzer = SentimentAnalyzer::without_stemmer(language, kind)
            .unwrap_or_else(|e| panic!("{kind:?}/{language}: {e}"));
        let winners = winners(voca);
        let negations = analyzer.negations();

        for (key, polarity) in voca.iter() {
            let key_form = form(key);
            let (winner, expected) = winners
                .get(&key_form)
                .unwrap_or_else(|| panic!("{kind:?}/{language}: no winner for {key:?}"));
            if winner != key {
                total_shadowed += 1;
            }

            let key_pieces = pieces(key);
            if key_pieces.is_empty() {
                // No word segment at all — an emoji, a circled letter, a bare
                // symbol. `WordTokenizer` yields nothing, so the pipeline
                // cannot reach it; the key is still reachable from a token
                // spelled exactly like it, which is what a tokenizer that does
                // emit symbol segments would hand over.
                total_unsegmentable += 1;
                assert_eq!(
                    WordTokenizer.tokens(key).count(),
                    0,
                    "{kind:?}/{language}: {key:?}"
                );
                let direct = analyzer.score([key]);
                assert_eq!(direct.count, 1, "{kind:?}/{language}: {key:?}");
                assert_eq!(direct.sum, *expected, "{kind:?}/{language}: {key:?}");
                continue;
            }

            if key_pieces.len() == 1 && negations.contains(&key_pieces[0].as_str()) {
                // The negation branch runs before the single-token lookup, so a
                // word in both lists contributes nothing at all. Documented,
                // deliberate, and asserted here rather than silently skipped.
                total_negation += 1;
                let scored = analyzer.score(WordTokenizer.tokens(key));
                assert_eq!(scored.count, 1, "{kind:?}/{language}: {key:?}");
                assert_eq!(scored.sum, 0.0, "{kind:?}/{language}: {key:?}");
                continue;
            }

            total_scored += 1;
            let scored = analyzer.score(WordTokenizer.tokens(key));
            assert_eq!(
                scored.count,
                1,
                "{kind:?}/{language}: {key:?} ({} pieces) must score as ONE unit, \
                 not {} — a key cut into pieces divides its own polarity",
                key_pieces.len(),
                scored.count
            );
            assert_eq!(
                scored.sum, *expected,
                "{kind:?}/{language}: {key:?} (shipped {polarity}) scored {} \
                 through WordTokenizer; the winning entry for lookup form \
                 {key_form:?} is {winner:?} at {expected}",
                scored.sum
            );
        }
    }

    // Every key of every table landed in exactly one of the three classes.
    // (Shadowing is orthogonal — a shadowed key is still scored, negated or
    // unsegmentable — so it is counted separately and not added in.)
    let total: usize = tables().map(|(_, _, v)| v.len()).sum();
    assert_eq!(
        total_scored + total_negation + total_unsegmentable,
        total,
        "scored={total_scored} negation={total_negation} \
         unsegmentable={total_unsegmentable} (shadowed={total_shadowed})"
    );
    // 1,488 emoji and circled-letter keys in the Spanish and Portuguese AFINN
    // tables have no word segment; `WordTokenizer` filters symbol runs out, so
    // reaching them needs a tokenizer that emits them. 102 keys are shadowed
    // by a later key with the same lookup form. Both numbers are small, known,
    // and would move if either rule changed.
    assert_eq!(total_unsegmentable, 1488);
    assert_eq!(total_shadowed, 102);
    assert_eq!(total_negation, 6);
}

/// The same enumeration, with a stemmer installed.
///
/// A stemmer rebuilds the table, which is the exact shape of defect this file
/// exists for: the keys are rewritten and the scoring loop must still find
/// them. The expected value is the stem-collision winner, computed here.
#[test]
fn every_key_stays_reachable_when_a_stemmer_rebuilds_the_table() {
    fn check<S: Stemmer + Clone>(kind: VocabularyKind, language: Language, stemmer: S) {
        let base = Vocabulary::shared(kind, language).expect("shipped table");
        let analyzer = SentimentAnalyzer::with_stemmer(language, kind, stemmer.clone())
            .expect("shipped table");
        let negations = analyzer.negations();

        // The contract has two halves. Each piece of a key is stemmed and the
        // stemmed forms collide last-wins in file order — that is the table.
        // Scoring then looks a token up *unstemmed first and stemmed only on a
        // miss*, so a token whose own spelling happens to be some other word's
        // stem lands on that other word: `Habgier` lowercases to `habgier`,
        // which is what `habgierig` stems to. Both halves are modelled here.
        let stem_form = |key: &str| -> String {
            let p = pieces(key);
            if p.is_empty() {
                return key.to_lowercase();
            }
            p.iter()
                .map(|piece| {
                    let stem = stemmer.stem(piece);
                    if stem.is_empty() {
                        piece.clone()
                    } else {
                        stem.into_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut winners: HashMap<String, f64> = HashMap::new();
        for (key, polarity) in base.iter() {
            winners.insert(stem_form(key), polarity.value());
        }

        for (key, _) in base.iter() {
            let key_pieces = pieces(key);
            if key_pieces.is_empty() {
                continue;
            }
            if key_pieces.len() == 1 && negations.contains(&key_pieces[0].as_str()) {
                continue;
            }
            // Unstemmed lookup first, stemmed on a miss. The stemmed form is
            // always present, so every key resolves to something.
            let expected = winners
                .get(&form(key))
                .or_else(|| winners.get(&stem_form(key)))
                .copied()
                .expect("a key's own stem form is always in the table");
            let scored = analyzer.score(WordTokenizer.tokens(key));
            assert_eq!(scored.count, 1, "{kind:?}/{language}: {key:?}");
            assert_eq!(
                scored.sum,
                expected,
                "{kind:?}/{language}: {key:?} scored {} through the stemmed \
                 table; lookup form {:?}, stem form {:?}",
                scored.sum,
                form(key),
                stem_form(key),
            );
        }
    }

    check(
        VocabularyKind::Pattern,
        Language::German,
        verbora_stemmers::PorterStemmerDe::new(),
    );
    check(
        VocabularyKind::Afinn,
        Language::English,
        verbora_stemmers::PorterStemmer::new(),
    );
    check(
        VocabularyKind::Pattern,
        Language::English,
        verbora_stemmers::PorterStemmer::new(),
    );
}

/// The capitalised German pattern entries, which no bridged stemmer folds.
///
/// 1,234 of the 3,465 `pattern`/German keys are capitalised. The scoring loop
/// looks a token up by its lowercase, so every one of them used to be
/// unreachable — and installing `PorterStemmerDe` did not fix it: 699 stayed
/// unreachable, 232 scored their own polarity and 303 scored some *other*
/// entry's, because a stem of a capitalised key is still capitalised. Keys are
/// now indexed by their lookup form, which is lowercased.
#[test]
fn capitalised_keys_are_reachable_without_a_stemmer() {
    let voca = Vocabulary::shared(VocabularyKind::Pattern, Language::German).unwrap();
    let analyzer =
        SentimentAnalyzer::without_stemmer(Language::German, VocabularyKind::Pattern).unwrap();
    let winners = winners(voca);

    let mut capitalised = 0usize;
    for (key, polarity) in voca.iter() {
        if key.chars().all(|c| !c.is_uppercase()) {
            continue;
        }
        capitalised += 1;
        let (_, expected) = winners[&form(key)];
        assert_eq!(
            analyzer.score(WordTokenizer.tokens(key)).sum,
            expected,
            "{key:?} is worth {polarity}"
        );
    }
    assert_eq!(capitalised, 1234);

    // The two named in the defect report, spelled as they ship.
    assert_eq!(analyzer.get_sentiment(["Abfall"]), Some(-0.0048));
    assert_eq!(analyzer.get_sentiment(["abfall"]), Some(-0.0048));
}

/// The hyphenated English keys the tokenizer split, including the three that
/// inverted rather than merely vanishing.
#[test]
fn hyphenated_keys_score_their_own_polarity_not_their_halves() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    let score = |text: &str| a.score(WordTokenizer.tokens(text));

    // Sign flip: `non` is not a key, `approved` is worth +2.
    assert_eq!(
        a.vocabulary().get("non-approved").map(Polarity::value),
        Some(-2.0)
    );
    let s = score("non-approved");
    assert_eq!(s.count, 1);
    assert_eq!(s.sum, -2.0);

    // Silently zero: neither `cover` nor `up` is a key.
    assert_eq!(
        a.vocabulary().get("cover-up").map(Polarity::value),
        Some(-3.0)
    );
    let s = score("cover-up");
    assert_eq!(s.count, 1);
    assert_eq!(s.sum, -3.0);

    // Diluted: four tokens divided a -5 into -1.25.
    assert_eq!(
        a.vocabulary().get("son-of-a-bitch").map(Polarity::value),
        Some(-5.0)
    );
    let s = score("son-of-a-bitch");
    assert_eq!(s.count, 1);
    assert_eq!(s.sum, -5.0);
    assert_eq!(
        a.get_sentiment(WordTokenizer.tokens("son-of-a-bitch")),
        Some(-5.0)
    );

    // The space-spelled key is the same lookup form as the hyphenated one.
    assert_eq!(
        a.get_sentiment(WordTokenizer.tokens("son of a bitch")),
        Some(-5.0)
    );

    // Sticky negation still applies to a span.
    assert_eq!(
        a.get_sentiment(WordTokenizer.tokens("not a cover-up")),
        Some(1.0)
    );
}

/// A phrase key is one lexical unit, so it is one unit of the denominator.
#[test]
fn a_matched_span_counts_once_toward_the_denominator() {
    let a =
        SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Senticon).unwrap();
    // `pitch-black` and `pitch black` are both shipped keys and share a lookup
    // form; the later one wins, and both spellings reach it.
    let hyphen = a.score(WordTokenizer.tokens("pitch-black"));
    let space = a.score(WordTokenizer.tokens("pitch black"));
    assert_eq!(hyphen.count, 1);
    assert_eq!(space.count, 1);
    assert_eq!(hyphen.sum, space.sum);

    // Longest match wins: `good` alone is a key too, but the span is taken
    // first, so the pair contributes one addend and one unit.
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    let two = a.score(WordTokenizer.tokens("cover-up good"));
    assert_eq!(two.count, 2);
    assert_eq!(two.sum, 0.0);
}

/// Public lookup answers for every spelling of every key.
#[test]
fn the_public_lookup_accepts_every_shipped_key() {
    for (kind, language, voca) in tables() {
        let winners = winners(voca);
        for (key, _) in voca.iter() {
            let (_, expected) = winners[&form(key)];
            assert_eq!(
                voca.get(key).map(Polarity::value),
                Some(expected),
                "{kind:?}/{language}: {key:?}"
            );
            // The scoring loop looks a token up by its lowercase, so every key
            // must be reachable that way too.
            assert_eq!(
                voca.get(&key.to_lowercase()).map(Polarity::value),
                Some(expected),
                "{kind:?}/{language}: {key:?} lowercased"
            );
        }
    }
}

/// No public entry point can hand a caller a `NaN`.
///
/// This is the enumeration behind the crate's "no result is `NaN`" claim, and
/// it is here rather than in a unit test because it must walk every shipped
/// vocabulary rather than the one English table the unit tests reach for.
///
/// Before [`SentimentAnalyzer::get_sentiment`] and [`Score`] became `Option`,
/// this found **80** escape sites across the fourteen pairs: `0.0 / 0.0` for
/// an empty document (14 pairs × 2 entry points) and `sum / 0` for an explicit
/// zero denominator (14 pairs × 4 documents). Every one of them is now `None`.
#[test]
fn no_public_entry_point_can_answer_with_a_nan() {
    let documents: [Vec<&str>; 6] = [
        vec![],
        vec![""],
        vec!["", "", ""],
        vec!["good"],
        vec!["not", "good", "good"],
        vec!["😂", "\u{FFFD}", "\u{1F600}\u{0301}"],
    ];
    let mut checked = 0usize;
    for (kind, language) in supported_pairs() {
        let a = SentimentAnalyzer::without_stemmer(language, kind)
            .unwrap_or_else(|e| panic!("{kind:?}/{language}: {e}"));
        for document in &documents {
            let scored = a.score(document);
            assert!(scored.sum.is_finite(), "{kind:?}/{language} {document:?}");

            // The mean exists exactly when something was scored, and is finite
            // when it exists.
            assert_eq!(
                scored.mean().is_some(),
                scored.count > 0,
                "{kind:?}/{language} {document:?}"
            );
            for value in [
                a.get_sentiment(document),
                a.get_sentiment_over(document, 0),
                a.get_sentiment_over(document, 3),
                scored.mean(),
            ] {
                assert!(
                    value.is_none_or(f64::is_finite),
                    "{kind:?}/{language} {document:?} answered {value:?}"
                );
                checked += 1;
            }
            // A denominator of zero has no quotient; every other one does.
            assert_eq!(a.get_sentiment_over(document, 0), None);
            assert!(a.get_sentiment_over(document, 3).is_some());
        }
    }
    assert_eq!(checked, 14 * 6 * 4, "every pair and document was walked");
}

/// Every one of the 75,803 shipped polarities is a finite decimal, and the
/// text a family publishes parses back to the number it is scored with.
///
/// This is what makes [`Polarity::value`] total: the fields are private and the
/// only way a `Polarity` comes into existence is this table, so proving the
/// table finite proves the accessor can never answer `NaN`.
#[test]
fn every_shipped_polarity_is_a_finite_decimal_matching_its_own_text() {
    let mut entries = 0usize;
    let mut with_text = 0usize;
    for (kind, language) in supported_pairs() {
        let Some(voca) = Vocabulary::shared(kind, language) else {
            continue;
        };
        for (key, polarity) in voca.iter() {
            entries += 1;
            assert!(
                polarity.value().is_finite(),
                "{kind:?}/{language}: {key:?} is {}",
                polarity.value()
            );
            match polarity.as_written() {
                Some(text) => {
                    with_text += 1;
                    assert_eq!(
                        text.parse::<f64>(),
                        Ok(polarity.value()),
                        "{kind:?}/{language}: {key:?} publishes {text:?}"
                    );
                }
                // AFINN publishes integers, and an integer polarity round-trips
                // through `f64` exactly.
                None => assert_eq!(
                    polarity.value().fract(),
                    0.0,
                    "{kind:?}/{language}: {key:?}"
                ),
            }
            assert_eq!(polarity.to_string().parse::<f64>(), Ok(polarity.value()));
        }
    }
    assert_eq!(entries, 75_803);
    // senticon and pattern publish decimal strings; the three AFINN tables do
    // not, and they hold 3382 + 1653 + 1644 = 6679 entries.
    assert_eq!(entries - with_text, 6_679);
}
