//! Every entry of every stop-word list, walked through the pipeline that is
//! supposed to filter it.
//!
//! # The failure this exists to catch
//!
//! A stop-word list is only worth what the pipeline can look up in it. Between
//! the text a caller passes and the string [`TokenizeAndStem::is_stop_word`]
//! finally sees there are two transforms — [`TokenizeAndStem::prepare`] and the
//! word walk in [`crate::base`] — and either one can make an entry
//! *unreachable*: still on the list, still returned by `contains`, and never
//! again matched by anything. Nothing fails. The list simply gets smaller
//! without saying so.
//!
//! It has already happened twice here. A whole-document diacritic fold in
//! Swedish `prepare` rewrote `för` to `for` before `is_stop_word` saw it and
//! killed 116 of 428 entries; the same fold killed 8 of Norwegian's 129. Both
//! survived a green suite, because the tests that covered stop words sampled a
//! handful of ASCII words. Sampling cannot find this class: the entries that
//! die are exactly the ones a spot check does not name.
//!
//! So this module does not sample. It enumerates **every entry of every list**
//! and requires each one to be either reachable or *provably* unreachable.
//!
//! # What "reachable" means, precisely
//!
//! An entry `e` is reachable when both of these hold:
//!
//! 1. `prepare(e) == e` — the rewrite the pipeline applies to the document
//!    before tokenizing does not rewrite the entry itself. This is the Swedish
//!    defect stated as a predicate.
//! 2. `e` is exactly **one whole token** of the pipeline's word walk: the
//!    walk's tokens, rejoined the way the walk joins them, reproduce `e`
//!    byte for byte. One token that is a *part* of `e` does not count — that
//!    is the case where a stray character rides along on an otherwise fine
//!    word, and it is invisible to a filtering test whenever the part is
//!    itself on the list.
//!
//! A reachable entry must then actually be dropped by
//! `tokenize_and_stem(e, false)`. An unreachable one must fall into one of two
//! structurally justified classes, each *checked* rather than asserted:
//!
//! | class | justification, verified per entry | example |
//! |---|---|---|
//! | `NO_WORD` | contains no alphanumeric scalar at all, so no word token can equal it | `"_"`, `"؟"` |
//! | `PHRASE` | contains whitespace, so it spans a boundary the walk always breaks at | `"может быть"` |
//!
//! Anything else — an entry that has a word in it and still is not one — is a
//! hard failure with no exemption available. That is the class `"je "` and
//! `"ei,"` belonged to.
//!
//! The two exempt sets are pinned by exact equality, per language, so an entry
//! cannot quietly join them: adding one to a list fails this test until it is
//! named here, and making an exempt entry reachable fails it too.

use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::Language;
use crate::{
    CarryStemmerFr, LancasterStemmer, PorterStemmer, PorterStemmerDe, PorterStemmerEs,
    PorterStemmerFa, PorterStemmerFr, PorterStemmerIt, PorterStemmerNl, PorterStemmerNo,
    PorterStemmerPt, PorterStemmerRu, PorterStemmerSv, PorterStemmerUk, StemmerId,
};

/// The entries of one list that the pipeline can never present to
/// `is_stop_word`, split by the reason.
///
/// Both fields are compared for exact equality against what the audit finds,
/// so this is a statement about the data rather than a filter over it.
struct Unreachable {
    /// Entries with no alphanumeric scalar anywhere: lone punctuation and
    /// symbols. A word token contains a letter or a digit by definition, so
    /// nothing the walk emits can equal these.
    no_word: &'static [&'static str],
    /// Entries containing whitespace: multi-word phrases. The walk breaks at
    /// whitespace unconditionally, so these arrive as two tokens or more and
    /// the phrase as spelled is never looked up.
    phrase: &'static [&'static str],
}

impl Unreachable {
    const NONE: Self = Self {
        no_word: &[],
        phrase: &[],
    };
}

/// Whether the whole of `entry` is a single token of `S`'s word walk.
///
/// The walk is [`WordTokenizer`] plus, for the one language that sets
/// [`TokenizeAndStem::HYPHEN_JOINS_LETTERS`], the tailoring that keeps
/// `U+002D` between two letters inside a word. Stating the tailoring as
/// "the tokens rejoined with `-` reproduce the entry" rather than re-deriving
/// the boundary rule keeps this from becoming a second copy of it, and
/// [`audit`] cross-checks the answer against the token count the real walk
/// reports.
pub(crate) fn is_one_whole_token(entry: &str, join_hyphens: bool) -> bool {
    let tokens: Vec<&str> = WordTokenizer.tokens(entry).collect();
    if tokens.is_empty() {
        return false;
    }
    if join_hyphens {
        return tokens.join("-") == entry;
    }
    tokens.len() == 1 && tokens[0] == entry
}

/// Walks every entry of `entries` through `stemmer`'s pipeline.
///
/// `code` names the list in failure messages. `unreachable` is the exact set of
/// entries the pipeline cannot reach, which the audit reproduces and compares.
fn audit<S: TokenizeAndStem>(stemmer: &S, code: &str, entries: &[&str], unreachable: &Unreachable) {
    let mut no_word = Vec::new();
    let mut phrase = Vec::new();
    let mut mangled_by_prepare = Vec::new();
    let mut stray = Vec::new();
    let mut not_filtered = Vec::new();

    for &entry in entries {
        // (1) The document rewrite must leave the entry alone. Swedish `för`
        // failed exactly here.
        if S::prepare(entry) != entry {
            mangled_by_prepare.push(entry);
            continue;
        }

        // The real walk's token count, taken from the pipeline itself rather
        // than recomputed: with `keep_stops` there is one stem per token.
        let produced = stemmer.tokenize_and_stem(entry, true).len();
        if is_one_whole_token(entry, S::HYPHEN_JOINS_LETTERS) {
            // (2) Reachable, so the pipeline owes us one token and an empty
            // result. The token count comes from the real walk, so the two
            // ways of asking cross-check each other here.
            assert_eq!(
                produced, 1,
                "{code}: {entry:?} is one whole token by the wholeness rule but \
                 the walk produced {produced}"
            );
            if !stemmer.tokenize_and_stem(entry, false).is_empty() {
                not_filtered.push(entry);
            }
            continue;
        }

        // Unreachable. It must be provably so, not merely observed to be, and
        // the proof is the walk's own token count together with what the entry
        // is made of.
        if produced == 0 {
            assert!(
                !entry.chars().any(char::is_alphanumeric),
                "{code}: {entry:?} holds a word but the walk found no token in it"
            );
            no_word.push(entry);
        } else if produced > 1 && entry.chars().any(char::is_whitespace) {
            phrase.push(entry);
        } else {
            stray.push(entry);
        }
    }

    assert!(
        mangled_by_prepare.is_empty(),
        "{code}: prepare() rewrites {} of its own stop words before is_stop_word \
         can see them: {mangled_by_prepare:?}",
        mangled_by_prepare.len()
    );
    assert!(
        not_filtered.is_empty(),
        "{code}: {} entries are single tokens the pipeline does not drop: \
         {not_filtered:?}",
        not_filtered.len()
    );
    assert!(
        stray.is_empty(),
        "{code}: {} entries carry a word plus characters no token can include, \
         so they can never be matched: {stray:?}. Correct the spelling — there \
         is no exemption for this class.",
        stray.len()
    );
    assert_eq!(
        no_word, unreachable.no_word,
        "{code}: the set of entries holding no word at all changed"
    );
    assert_eq!(
        phrase, unreachable.phrase,
        "{code}: the set of multi-word entries changed"
    );

    // Every unreachable entry is still on the list it came from: this audit
    // reports dead weight, it does not license the list and the membership
    // test to disagree.
    for &entry in no_word.iter().chain(phrase.iter()) {
        assert!(
            S::is_stop_word(entry),
            "{code}: {entry:?} is not even on the list it was read from"
        );
    }
}

/// The thirteen lists this crate carries, each through its own stemmer.
///
/// French appears twice because two stemmers consult it, and they disagree
/// about casing ([`PorterStemmerFr`] and [`CarryStemmerFr`] both filter on
/// [`Casing::Lower`], but that is a fact to check rather than assume).
#[test]
fn every_stop_word_is_reachable_through_its_own_pipeline() {
    audit(
        &PorterStemmerDe::new(),
        "de",
        Language::De.defaults(),
        &Unreachable::NONE,
    );
    audit(
        &PorterStemmerEs::new(),
        "es",
        Language::Es.defaults(),
        &Unreachable {
            no_word: &["_"],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerFa::new(),
        "fa",
        Language::Fa.defaults(),
        &Unreachable {
            no_word: &["؟", "!", "٪", ".", "،", "؛", ":", ";", ","],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerFr::new(),
        "fr",
        Language::Fr.defaults(),
        &Unreachable::NONE,
    );
    audit(
        &CarryStemmerFr::new(),
        "fr (Carry)",
        Language::Fr.defaults(),
        &Unreachable::NONE,
    );
    audit(
        &StemmerId::new(),
        "id",
        Language::Id.defaults(),
        &Unreachable::NONE,
    );
    audit(
        &PorterStemmerIt::new(),
        "it",
        Language::It.defaults(),
        &Unreachable {
            no_word: &["_"],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerNl::new(),
        "nl",
        Language::Nl.defaults(),
        &Unreachable {
            no_word: &["$", "_", "-"],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerNo::new(),
        "no",
        Language::No.defaults(),
        &Unreachable {
            no_word: &["_"],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerPt::new(),
        "pt",
        Language::Pt.defaults(),
        &Unreachable {
            no_word: &["_"],
            phrase: &[],
        },
    );
    audit(
        &PorterStemmerRu::new(),
        "ru",
        Language::Ru.defaults(),
        &Unreachable {
            no_word: &["$", "_"],
            phrase: &["может быть", "все еще", "с кем", "хотел бы"],
        },
    );
    audit(
        &PorterStemmerSv::new(),
        "sv",
        Language::Sv.defaults(),
        &Unreachable::NONE,
    );
    audit(
        &PorterStemmerUk::new(),
        "uk",
        Language::Uk.defaults(),
        &Unreachable {
            no_word: &["$", "_"],
            phrase: &["може бути", "все ще", "хотів би"],
        },
    );
}

/// The two entries the audit above found dead, at the level a caller sees
/// them.
///
/// Both were single characters of punctuation attached to an ordinary word —
/// Dutch `"je "` with a trailing space, German `"ei,"` with a trailing comma —
/// and both had been in the shipped data from the beginning. Neither could
/// ever match: the walk yields `je` and `ei`, and neither equals the entry.
///
/// `"ei,"` shows why a filtering test alone is not enough to find this class.
/// German also lists `"ei"`, so `tokenize_and_stem("ei,", false)` was already
/// empty and every "is this entry filtered?" check passed — the dead entry was
/// hidden behind a live one. Only asking whether the entry *is* the token
/// exposes it.
#[test]
fn the_entries_that_were_dead_now_filter() {
    let nl = PorterStemmerNl::new();
    assert!(PorterStemmerNl::is_stop_word("je"));
    assert!(!PorterStemmerNl::is_stop_word("je "));
    // `je`, `en`, `ik`, `heb` and `mijn` are all on the Dutch list; `hebt` and
    // `boek` are not. Before the fix `je` came through twice while everything
    // else around it filtered — the shape that makes this class so quiet.
    assert_eq!(
        nl.tokenize_and_stem("je hebt je boek en ik heb mijn boek", false),
        ["hebt", "boek", "boek"],
        "je is the commonest Dutch pronoun on the list; it must not survive"
    );

    let de = PorterStemmerDe::new();
    assert!(PorterStemmerDe::is_stop_word("ei"));
    assert!(!PorterStemmerDe::is_stop_word("ei,"));
    assert!(de.tokenize_and_stem("ei, ei", false).is_empty());
}

/// Japanese is the fourteenth list and has no pipeline, so it gets the half of
/// the audit that still applies.
///
/// [`crate::StemmerJa`] stems one token and does not tokenize — UAX #29's
/// default word rules do not segment Japanese, so the caller supplies the
/// segmentation — which means there is no `prepare` and no walk to check the
/// list against. What remains is the part that holds for *any* caller-supplied
/// segmentation: an entry with whitespace in it, or with no word in it, could
/// not be a token under any segmentation of Japanese text either.
#[test]
fn the_japanese_list_holds_only_things_a_segmenter_could_produce() {
    for &entry in Language::Ja.defaults() {
        assert!(!entry.is_empty(), "the Japanese list holds an empty entry");
        assert!(
            !entry.chars().any(char::is_whitespace),
            "{entry:?} carries whitespace, so no segment can equal it"
        );
        assert!(
            entry.chars().all(char::is_alphanumeric),
            "{entry:?} is not made of letters and digits"
        );
        assert!(
            crate::Language::Ja.contains(entry),
            "{entry:?} is not found by the membership test of the list it came from"
        );
    }
}

/// English, whose list lives in `verbora-core` rather than here.
///
/// Both English pipelines in this crate — [`PorterStemmer`] and
/// [`LancasterStemmer`] — filter against the process-global list, so the
/// same audit applies to them. The exempt set is deliberately *not* pinned by
/// equality the way the thirteen above are: `crates/verbora-core` owns that
/// data, and pinning it here would turn an improvement made there into a
/// failure here. What is pinned is the part that is a defect under any
/// ownership — a rewrite that eats an entry, a stray character riding on a
/// word, or a whole token the pipeline declines to drop.
#[test]
fn the_english_list_is_reachable_through_both_of_its_pipelines() {
    for (code, unreachable) in [
        (
            "en (Porter)",
            audit_permissive(
                &PorterStemmer::new(),
                verbora_core::StopWordLanguage::En.stopwords(),
            ),
        ),
        (
            "en (Lancaster)",
            audit_permissive(
                &LancasterStemmer::new(),
                verbora_core::StopWordLanguage::En.stopwords(),
            ),
        ),
    ] {
        // Recorded, not asserted, and the set is now empty: `$` and `_` held
        // no word, so no token could equal them, and `verbora-core` has since
        // retired both. The shape of this check is deliberately unchanged —
        // the exempt set stays pinned where the data lives, so a later repair
        // there cannot turn into a failure here.
        assert!(
            unreachable
                .iter()
                .all(|e| !e.chars().any(char::is_alphanumeric)),
            "{code}: an entry with a word in it is unreachable: {unreachable:?}"
        );
    }
}

/// [`audit`] without the exact-set comparison, returning the unreachable
/// entries instead of checking them against a pinned list.
fn audit_permissive<'a, S: TokenizeAndStem>(stemmer: &S, entries: &[&'a str]) -> Vec<&'a str> {
    let mut unreachable = Vec::new();
    for &entry in entries {
        assert_eq!(
            S::prepare(entry),
            entry,
            "prepare() rewrites the stop word {entry:?} before is_stop_word sees it"
        );
        if is_one_whole_token(entry, S::HYPHEN_JOINS_LETTERS) {
            assert!(
                stemmer.tokenize_and_stem(entry, false).is_empty(),
                "{entry:?} is a whole token the pipeline does not drop"
            );
        } else {
            unreachable.push(entry);
        }
    }
    unreachable
}

/// The casing each list is filtered on, which decides whether a capitalised
/// occurrence is dropped.
///
/// Not a defect and not a preference — a recorded asymmetry that
/// [`crate::base`] documents and that this audit depends on: `audit` compares
/// entries against the filter string, so it would be measuring the wrong thing
/// if a language changed which casing it filters on without saying so.
#[test]
fn the_filter_casing_of_every_language_is_the_documented_one() {
    assert_eq!(PorterStemmerDe::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerEs::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerFa::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerFr::FILTER_ON, Casing::Lower);
    assert_eq!(CarryStemmerFr::FILTER_ON, Casing::Lower);
    assert_eq!(StemmerId::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerIt::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerNl::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerNo::FILTER_ON, Casing::Lower);
    assert_eq!(PorterStemmerPt::FILTER_ON, Casing::Lower);
    assert_eq!(PorterStemmerRu::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmerSv::FILTER_ON, Casing::Lower);
    assert_eq!(PorterStemmerUk::FILTER_ON, Casing::Raw);
    assert_eq!(PorterStemmer::FILTER_ON, Casing::Raw);
    assert_eq!(LancasterStemmer::FILTER_ON, Casing::Raw);
    // Every list is lower-case throughout, which is why filtering on the raw
    // token still drops the lower-case occurrences of a word.
    for lang in Language::ALL {
        for entry in lang.defaults() {
            assert_eq!(
                *entry,
                entry.to_lowercase(),
                "{lang:?}: {entry:?} is not lower-case, so the raw-token filter would miss it"
            );
        }
    }
}

/// Reads one `… NAME: &[&str] = &[...]` literal out of Rust source.
///
/// `header` is the whole declaration up to and including the opening bracket,
/// so the same reader serves both the `pub static` data tables above and the
/// `const` word fixtures `benches/stemmers.rs` declares — which is how
/// [`super::gates`]'s German audit reaches the bench corpus without copying it
/// into a test and letting the two drift.
///
/// Deliberately small: the files it reads hold no escape sequences, no raw
/// strings and no comments inside a literal — a property this checks — so a
/// scan for balanced brackets and quoted runs is exact.
pub(super) fn str_slice_literal(source: &str, header: &str) -> Vec<String> {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("no declaration matching {header:?}"))
        + header.len();
    let mut depth = 1usize;
    let mut end = None;
    for (offset, byte) in source[start..].bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &source[start..end.expect("unterminated slice literal")];
    assert!(
        !body.contains('\\'),
        "{header:?} gained an escape sequence; this reader cannot see through one"
    );
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let close = after.find('"').expect("unterminated string literal");
        out.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    out
}
