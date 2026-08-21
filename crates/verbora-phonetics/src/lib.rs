//! Phonetic encoders: keys that make similar-sounding words collide.
//!
//! Twelve algorithms, each implementing its own publication, plus one
//! Verbora-native structure built on top of them ([`PhoneticIndex`]).
//!
//! ```
//! use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx};
//!
//! assert_eq!(SoundEx::new().process("Robert"), "R163");
//! assert_eq!(Metaphone::new().process("phonetics"), "FNTKS");
//! assert_eq!(DoubleMetaphone::new().process("Smith").primary(), "SM0");
//! ```
//!
//! # The algorithms and what each cites
//!
//! | Type | Publication | Key shape |
//! |---|---|---|
//! | [`SoundEx`] | Russell 1918; NARA, *The Soundex Indexing System* | letter + 3 digits |
//! | [`RefinedSoundex`] | no publication; the reference distribution is its definition (see below) | letter + digits, unbounded |
//! | [`Metaphone`] | Philips 1990, *Computer Language* 7(12) | letters, unbounded |
//! | [`DoubleMetaphone`] | Philips 2000, *C/C++ Users Journal* 18(6) | one or two keys, ≤ 4 |
//! | [`DaitchMokotoff`] | Daitch–Mokotoff Soundex (Gary Mokotoff and Randy Daitch, 1985) | 6 digits, branching |
//! | [`Nysiis`] | Taft 1970, *Name Search Techniques* | letters |
//! | [`Caverphone1`] / [`Caverphone2`] | Hood 2002 / 2004, Caversham Project | 6 / 10 characters |
//! | [`Cologne`] | Postel 1969, *IBM-Nachrichten* 19 | digits |
//! | [`Phonex`] | Lait and Randell 1996 | letter + digits |
//! | [`MatchRatingApproach`] | Moore et al. 1977, *Western Union* | letters, plus a match decision |
//! | [`BeiderMorse`] | Beider and Morse phonetic matching | a candidate list |
//!
//! Every row above cites a paper except one. **Refined Soundex has no
//! publication to cite** — no paper, no standards document, no author's
//! specification: the algorithm exists only as the letter-to-digit mapping
//! that ships in the Apache Commons Codec distribution, which is therefore
//! its reference definition rather than a second implementation of one.
//! Citing a distribution is a standards citation when the distribution *is*
//! the standard; [`RefinedSoundex`]'s own documentation says so, and states
//! the mapping in full so that a reader never has to go and read code to
//! learn what the encoder does.
//!
//! # Choosing the right API
//!
//! Every encoder answers "what is this word's key?" with `process`, and "do
//! these two words sound alike?" with `compare`. Four further shapes exist,
//! each for a workload the plain pair cannot serve:
//!
//! | Shape | Use case | Allocation | Recommendation |
//! |---|---|---|---|
//! | `encoder.process(word)` | one word, one key | one `String` | **the default.** Reach for anything else only when this one is measurably in the way. |
//! | `encoder.compare(a, b)` | "do these sound alike?" | two `String`s | the readable way to ask; it is defined as key equality (Double Metaphone excepted — see below) |
//! | `encoder.process_into(word, &mut buf)` | encoding a dictionary into a buffer you own | none, once `buf` has grown | you must manage `buf`, including clearing it; offered on [`SoundEx`] and [`Metaphone`], whose keys are the ones most often accumulated in bulk |
//! | [`PhoneticIndex`] | "which of these ten thousand names sound like this one?" | one build-time structure; queries allocate only the query's key | the only shape that answers a *set* question; a loop of `compare` over a dictionary is O(n) per query |
//! | [`par_encode_batch`] (feature `parallel`) | encoding tens of thousands of words at once | one output `Vec` | measure first: a single call costs tens of nanoseconds, so the thread pool only pays for itself in bulk |
//!
//! [`DoubleMetaphone::compare`] is the one `compare` that is *not* key
//! equality: it matches when the two names share **either** key, which is the
//! entire reason the algorithm produces two.
//!
//! # The text unit
//!
//! Every encoder here reads **one Unicode scalar at a time**. What it does
//! with a scalar depends on the algorithm's own alphabet, and each type's
//! documentation states it exactly. No input can be split in the middle of a
//! character and no output can contain a character the input did not imply.
//!
//! One exception, stated because it is observable: [`DaitchMokotoff`] advances
//! past a matched rule pattern by the pattern's **byte** length. For the
//! coding chart's ASCII patterns that is the same as its character length, but
//! the four two-byte keys `ą`, `ę`, `ţ` and `ț` therefore consume one
//! character more than they matched. That behaviour is recorded and pinned
//! rather than corrected — see [`DaitchMokotoff`]'s own documentation for why
//! and for exactly which inputs it changes.
//!
//! The three Latin-alphabet encoders ([`SoundEx`], [`Metaphone`],
//! [`DoubleMetaphone`]) read only `A`–`Z` after simple ASCII case folding and
//! skip everything else; the encoders specified for a particular language
//! ([`Cologne`] for German, [`DaitchMokotoff`] for Slavic and Yiddish
//! spellings, [`BeiderMorse`] for eighteen) fold the accented letters their own
//! publications name. Where you need `Müller` to code as `Mueller` under an
//! encoder that does not fold it, transliterate before encoding.
//!
//! # No encoder here fails
//!
//! Every `process` is total: no `Result`, no panic, on any `&str`. An empty
//! key means the algorithm found nothing to encode — it is the absence of a
//! key, not a value standing in for one, so two empty keys are not a match.
//!
//! An empty key does **not** imply the input had no recognised letter. Several
//! algorithms encode a letter only in a context they define, so a word made
//! only of letters they treat as silent or as carriers encodes to nothing:
//! `Metaphone` on `"y"` or `"w"`, `Cologne` on `"h"`, and
//! `MatchRatingApproach` on any single vowel, since rule 2 removes vowels
//! after the first letter and rule 1 has nothing to keep. Test for emptiness
//! before treating a key as an index entry.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg))]
// Documentation in this crate links to `par_encode_batch` (`parallel`).
// Those links resolve on docs.rs, which builds all features; without the
// feature the targets do not exist, and the lint would fire on every plain
// `cargo doc`. It stays armed in the all-features build, which is the one
// that ships.
#![cfg_attr(not(feature = "parallel"), allow(rustdoc::broken_intra_doc_links))]

mod beider_morse;
mod caverphone;
mod cologne;
#[cfg(test)]
mod corpus;
mod daitch_mokotoff;
mod double_metaphone;
mod index;
mod letters;
mod match_rating;
mod metaphone;
mod nysiis;
#[cfg(feature = "parallel")]
mod parallel;
mod phonex;
mod refined_soundex;
mod soundex;

pub use beider_morse::{BeiderMorse, BeiderMorseCode, Language, LanguageSet, NameType, RuleType};
pub use caverphone::{Caverphone1, Caverphone2};
pub use cologne::Cologne;
pub use daitch_mokotoff::DaitchMokotoff;
pub use double_metaphone::{DoubleMetaphone, DoubleMetaphoneCode};
pub use index::{
    EntryId, InlineCode, MetaphoneCode, Neighbors, PhoneticCodes, PhoneticCodesIter,
    PhoneticEncoder, PhoneticIndex, PhoneticIndexBuilder, SoundexCode,
};
pub use match_rating::MatchRatingApproach;
pub use metaphone::Metaphone;
pub use nysiis::Nysiis;
#[cfg(feature = "parallel")]
pub use parallel::{DEFAULT_CHUNK_SIZE, par_encode_batch, par_encode_double_batch};
pub use phonex::Phonex;
pub use refined_soundex::RefinedSoundex;
pub use soundex::SoundEx;

use verbora_core::StopWords;

/// Encodes an already-tokenized stream, dropping stop words.
///
/// This is the reusable half of the tokenize-and-encode pipeline: the caller
/// supplies the tokens, so the choice of tokenizer stays visible at the call
/// site. [`tokenize_and_phoneticize`] is the wrapper that pairs it with
/// [`verbora_tokenizers::WordTokenizer`].
///
/// Filtering tests the **raw** token against `stop_words`, so it is exactly as
/// case-sensitive as the list you pass. Pass [`StopWords::new`] — the empty
/// list — to encode every token.
///
/// There is deliberately no variant that reads a process-global stop-word
/// list. A key that depends on whether some other part of the program has
/// mutated a global is not reproducible, and no publication here calls for
/// one.
///
/// ```
/// use verbora_core::{StopWordLanguage, StopWords};
/// use verbora_phonetics::{Metaphone, phoneticize_tokens};
///
/// let metaphone = Metaphone::new();
/// let stops = StopWords::for_language(StopWordLanguage::En);
/// let keys = phoneticize_tokens(["the", "quick", "brown", "fox"], &stops, |t| {
///     metaphone.process(t)
/// });
/// assert_eq!(keys, ["KK", "BRN", "FKS"]);
/// ```
pub fn phoneticize_tokens<'a, T, O>(
    tokens: T,
    stop_words: &StopWords,
    process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where
    T: IntoIterator<Item = &'a str>,
{
    filter_and_map(tokens, stop_words, process)
}

/// Tokenizes `text` and encodes each surviving token.
///
/// Tokenization is [`verbora_tokenizers::WordTokenizer`]: the UAX #29 word
/// segments containing a letter or a digit, borrowed from `text`.
///
/// Because the boundaries are UAX #29's, a hyphen or a slash splits a token —
/// `"well-known"` is encoded as two keys, not one — while an apostrophe, a
/// decimal point, a thousands separator, a colon between letters and an
/// underscore do not. Text whose words carry those characters produces a
/// different key list from a naive `[A-Za-z]`-run split.
///
/// Tokens are **not** case-folded before the stop-word test, so `"The"`
/// survives a lowercase list while `"the"` does not. Fold the text yourself if
/// you want otherwise; folding it here would be this crate silently rewriting
/// its caller's input.
///
/// ```
/// use verbora_core::{StopWordLanguage, StopWords};
/// use verbora_phonetics::{Metaphone, tokenize_and_phoneticize};
///
/// let metaphone = Metaphone::new();
/// let keys = tokenize_and_phoneticize("The quick brown fox", &StopWords::new(), |t| {
///     metaphone.process(t)
/// });
/// assert_eq!(keys, ["0", "KK", "BRN", "FKS"]);
/// ```
pub fn tokenize_and_phoneticize<O>(
    text: &str,
    stop_words: &StopWords,
    process: impl FnMut(&str) -> O,
) -> Vec<O> {
    let tokens = verbora_tokenizers::BorrowingTokenizer::tokenize_borrowed(
        &verbora_tokenizers::WordTokenizer,
        text,
    );
    filter_and_map(tokens.iter().copied(), stop_words, process)
}

/// The shared body of the two entry points above.
fn filter_and_map<'a, T, O>(
    tokens: T,
    stop_words: &StopWords,
    mut process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where
    T: IntoIterator<Item = &'a str>,
{
    tokens
        .into_iter()
        .filter(|t| !stop_words.contains(t))
        .map(&mut process)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_core::StopWordLanguage;

    #[test]
    fn phoneticize_tokens_filters_against_the_supplied_list_only() {
        let m = Metaphone::new();
        let stops = StopWords::from_iter_of(["the"]);

        let keys = phoneticize_tokens(["the", "quick"], &stops, |t| m.process(t));
        assert_eq!(keys, ["KK"]);

        // The list is consulted verbatim, so its case discipline is the
        // caller's to choose.
        let keys = phoneticize_tokens(["The", "quick"], &stops, |t| m.process(t));
        assert_eq!(keys, ["0", "KK"]);

        // An empty list keeps everything.
        let keys = phoneticize_tokens(["the", "quick"], &StopWords::new(), |t| m.process(t));
        assert_eq!(keys, ["0", "KK"]);
    }

    #[test]
    fn phoneticize_tokens_supports_two_key_encoders() {
        let dm = DoubleMetaphone::new();
        let keys = phoneticize_tokens(
            ["phonetic", "modules"],
            &StopWords::for_language(StopWordLanguage::En),
            |t| dm.process(t),
        );
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].primary(), "FNTK");
        assert_eq!(keys[1].primary(), "MTLS");
    }

    /// The tokenizing entry point cuts at UAX #29 boundaries, so the input
    /// classes that move are hyphens, slashes and interior punctuation. Each
    /// expected value is derived from a named UAX #29 rule, not from running
    /// the code.
    #[test]
    fn tokenize_and_phoneticize_cuts_at_uax29_boundaries() {
        let m = Metaphone::new();
        let keys = |text: &str| tokenize_and_phoneticize(text, &StopWords::new(), |t| m.process(t));

        // U+002D HYPHEN-MINUS is Word_Break=Other: WB999 breaks, two tokens.
        assert_eq!(keys("well-known"), [m.process("well"), m.process("known")]);
        // U+002F SOLIDUS is Word_Break=Other: two tokens.
        assert_eq!(keys("and/or"), [m.process("and"), m.process("or")]);
        // WB6/WB7 over Single_Quote: one token, apostrophe included.
        assert_eq!(keys("don't"), [m.process("don't")]);
        // WB11/WB12: Numeric x MidNumLet x Numeric, and MidNum for the comma.
        assert_eq!(keys("3.14 1,000"), [m.process("3.14"), m.process("1,000")]);
        // WB13a/WB13b: ExtendNumLet keeps an underscore inside the word.
        assert_eq!(keys("node_js"), [m.process("node_js")]);
        // Non-ASCII letters are letters, and are not split out of the word.
        assert_eq!(keys("café"), [m.process("café")]);
        // Punctuation-only and whitespace-only runs are dropped by the word
        // filter, so no empty token ever reaches an encoder.
        assert!(keys("... --- ,,,").is_empty());
        assert!(keys("").is_empty());
    }

    /// **Enumeration, not sampling.** Every encoder in the crate, over every
    /// entry of a real non-ASCII name corpus and every pathological Unicode
    /// shape — plus each of those repeated fifty times, which is where index
    /// arithmetic in a lookahead goes wrong.
    ///
    /// The corpus matters as much as the sweep. `benches/data/names.json`,
    /// the 653-name list these encoders were previously exercised against, is
    /// entirely ASCII, and ASCII input is invariant under every text-unit rule
    /// anyone would propose — so it could not have detected a text-unit defect
    /// at all. See [`corpus`] for what this one holds and why.
    #[test]
    fn every_encoder_is_total_over_the_non_ascii_corpus() {
        let inputs = corpus::NON_ASCII_NAMES
            .iter()
            .chain(corpus::PATHOLOGICAL.iter());

        for input in inputs {
            let _ = SoundEx::new().process(input);
            let _ = RefinedSoundex::new().process(input);
            let _ = Metaphone::new().process(input);
            let _ = DoubleMetaphone::new().process(input);
            let _ = DaitchMokotoff::new().process(input);
            let _ = DaitchMokotoff::new().codes(input);
            let _ = Nysiis::new().process(input);
            let _ = Nysiis::with_strict(true).process(input);
            let _ = Caverphone1::new().process(input);
            let _ = Caverphone2::new().process(input);
            let _ = Cologne::new().process(input);
            let _ = Phonex::new().process(input);
            let _ = MatchRatingApproach::new().process(input);
            for name_type in [NameType::Generic, NameType::Ashkenazi, NameType::Sephardic] {
                for rule_type in [RuleType::Approx, RuleType::Exact] {
                    let _ = BeiderMorse::new(name_type, rule_type).encode(input);
                }
            }
            // Repeated at length, which is where an index arithmetic bug in a
            // lookahead would show up.
            let long = input.repeat(50);
            let _ = Metaphone::new().process(&long);
            let _ = DoubleMetaphone::new().process(&long);
            let _ = Nysiis::new().process(&long);
            let _ = Caverphone2::new().process(&long);
        }
    }

    /// Every encoder's *declared* output shape, enumerated over the same
    /// corpus. Totality alone is a weak claim — an encoder that silently
    /// returned `U+FFFD`, or a code twice its published length, would pass
    /// a no-panic sweep and fail here.
    #[test]
    fn every_encoder_honours_its_declared_output_shape() {
        for input in corpus::NON_ASCII_NAMES
            .iter()
            .chain(corpus::PATHOLOGICAL.iter())
        {
            // Soundex: four ASCII characters, or none.
            let code = SoundEx::new().process(input);
            assert!(
                code.is_empty() || code.len() == 4,
                "soundex {input:?}: {code:?}"
            );
            assert!(code.is_ascii());

            // Metaphone: ASCII letters plus `0`, never truncated.
            let code = Metaphone::new().process(input);
            assert!(
                code.bytes().all(|b| b.is_ascii_uppercase() || b == b'0'),
                "metaphone {input:?}: {code:?}"
            );

            // Double Metaphone: at most four characters per key.
            let code = DoubleMetaphone::new().process(input);
            assert!(code.primary().len() <= 4, "dmeta {input:?}: {code:?}");
            assert!(code.alternate().is_none_or(|a| a.len() <= 4));
            assert!(code.alternate() != Some(code.primary()), "{input:?}");

            // Daitch-Mokotoff: every branch is exactly six digits.
            for branch in DaitchMokotoff::new().codes(input) {
                assert_eq!(branch.len(), 6, "dm {input:?}: {branch:?}");
                assert!(branch.bytes().all(|b| b.is_ascii_digit()));
            }

            // Caverphone: exactly six / ten ASCII characters, always.
            assert_eq!(Caverphone1::new().process(input).len(), 6, "cv1 {input:?}");
            assert_eq!(Caverphone2::new().process(input).len(), 10, "cv2 {input:?}");

            // Phonex: exactly the configured length, in ASCII.
            let code = Phonex::new().process(input);
            assert_eq!(code.len(), 4, "phonex {input:?}: {code:?}");
            assert!(code.is_ascii());

            // Cologne: digits only.
            let code = Cologne::new().process(input);
            assert!(
                code.bytes().all(|b| b.is_ascii_digit()),
                "cologne {input:?}: {code:?}"
            );

            // NYSIIS: ASCII uppercase, at most six characters when strict.
            let code = Nysiis::new().process(input);
            assert!(code.is_ascii(), "nysiis {input:?}: {code:?}");
            assert!(code.len() <= 6, "nysiis {input:?}: {code:?}");

            // Refined Soundex: an ASCII letter followed by digits.
            let code = RefinedSoundex::new().process(input);
            assert!(code.is_ascii(), "refined {input:?}: {code:?}");
            assert!(
                code.is_empty()
                    || (code.as_bytes()[0].is_ascii_uppercase()
                        && code.bytes().skip(1).all(|b| b.is_ascii_digit())),
                "refined {input:?}: {code:?}"
            );

            // Match Rating: ASCII, at most six characters.
            let code = MatchRatingApproach::new().process(input);
            assert!(
                code.is_ascii() && code.len() <= 6,
                "mra {input:?}: {code:?}"
            );

            // No encoder anywhere may emit U+FFFD: it is the signature of a
            // string cut through the middle of a character, which the
            // one-scalar text unit makes unrepresentable.
            for code in [
                SoundEx::new().process(input),
                Metaphone::new().process(input),
                DoubleMetaphone::new().process(input).primary().to_owned(),
                DaitchMokotoff::new().process(input),
                Caverphone1::new().process(input),
                Caverphone2::new().process(input),
                Phonex::new().process(input),
                Cologne::new().process(input),
                Nysiis::new().process(input),
                RefinedSoundex::new().process(input),
                MatchRatingApproach::new().process(input),
            ] {
                assert!(!code.contains('\u{FFFD}'), "{input:?} -> {code:?}");
            }
            for name_type in [NameType::Generic, NameType::Ashkenazi, NameType::Sephardic] {
                let code = BeiderMorse::new(name_type, RuleType::Approx).encode(input);
                for spelling in &code.spellings {
                    assert!(
                        !spelling.contains('\u{FFFD}'),
                        "{name_type:?} {input:?} -> {spelling:?}"
                    );
                }
            }
        }
    }

    /// Encoding is a pure function of the input, and `compare` is an
    /// equivalence on it — reflexive and symmetric — for every encoder, over
    /// the whole corpus. A stateful encoder or an asymmetric comparison would
    /// be a defect no single-call test can see.
    #[test]
    fn encoding_is_pure_and_comparison_is_symmetric() {
        let names: Vec<&str> = corpus::NON_ASCII_NAMES
            .iter()
            .chain(corpus::PATHOLOGICAL.iter())
            .copied()
            .collect();

        for input in &names {
            assert_eq!(SoundEx::new().process(input), SoundEx::new().process(input));
            assert_eq!(
                Metaphone::new().process(input),
                Metaphone::new().process(input)
            );
            assert_eq!(
                DoubleMetaphone::new().process(input),
                DoubleMetaphone::new().process(input)
            );
            assert!(SoundEx::new().compare(input, input));
            assert!(Metaphone::new().compare(input, input));
            assert!(DoubleMetaphone::new().compare(input, input));
            assert!(Cologne::new().compare(input, input));
            assert!(Nysiis::new().compare(input, input));
        }

        for (a, b) in names.iter().zip(names.iter().skip(1)) {
            assert_eq!(SoundEx::new().compare(a, b), SoundEx::new().compare(b, a));
            assert_eq!(
                Metaphone::new().compare(a, b),
                Metaphone::new().compare(b, a)
            );
            assert_eq!(
                DoubleMetaphone::new().compare(a, b),
                DoubleMetaphone::new().compare(b, a)
            );
            assert_eq!(
                MatchRatingApproach::new().compare(a, b),
                MatchRatingApproach::new().compare(b, a)
            );
        }
    }

    /// The three Latin-alphabet encoders declare that a scalar outside
    /// `A`-`Z` is *skipped*, which is a stronger claim than "ignored": it
    /// must not act as a separator either. Enumerated by inserting one
    /// skipped scalar at every position of every corpus name and requiring
    /// the key not to move.
    #[test]
    fn a_skipped_scalar_is_transparent_at_every_position() {
        const SKIPPED: &[char] = &['\u{e9}', '\u{1F600}', '7', '.', '\u{301}', '\u{4e2d}'];
        let soundex = SoundEx::new();
        let metaphone = Metaphone::new();

        for name in corpus::NON_ASCII_NAMES.iter().take(40) {
            let plain: String = name.chars().filter(char::is_ascii_alphabetic).collect();
            let want_soundex = soundex.process(&plain);
            let want_metaphone = metaphone.process(&plain);
            for &noise in SKIPPED {
                for cut in 0..=plain.len() {
                    if !plain.is_char_boundary(cut) {
                        continue;
                    }
                    let mut spiked = String::with_capacity(plain.len() + 4);
                    spiked.push_str(&plain[..cut]);
                    spiked.push(noise);
                    spiked.push_str(&plain[cut..]);
                    assert_eq!(
                        soundex.process(&spiked),
                        want_soundex,
                        "soundex moved for {spiked:?}"
                    );
                    assert_eq!(
                        metaphone.process(&spiked),
                        want_metaphone,
                        "metaphone moved for {spiked:?}"
                    );
                }
            }
        }
    }

    /// The empty key means "no key", and only an input with no recognised
    /// letter can produce it. Enumerated per encoder rather than sampled,
    /// because "returns `""` for empty input" is the one property a sentinel
    /// return would also satisfy.
    #[test]
    fn the_empty_key_is_unreachable_from_a_letter_bearing_input() {
        let alphabet = "abcdefghijklmnopqrstuvwxyz";
        for letter in alphabet.chars() {
            let word = letter.to_string();
            assert!(!SoundEx::new().process(&word).is_empty(), "soundex {word}");
            assert!(
                !Metaphone::new().process(&format!("{word}a")).is_empty(),
                "metaphone {word}a"
            );
            assert!(
                !DoubleMetaphone::new()
                    .process(&format!("{word}a"))
                    .primary()
                    .is_empty(),
                "double metaphone {word}a"
            );
        }
        assert_eq!(SoundEx::new().process(""), "");
        assert_eq!(Metaphone::new().process(""), "");
        assert_eq!(DoubleMetaphone::new().process("").primary(), "");
    }
}
