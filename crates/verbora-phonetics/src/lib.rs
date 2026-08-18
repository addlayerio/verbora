//! Phonetic algorithms for Rust.
//!
//! Ports the reference `phonetics` module — four encoders that map a word to a key so
//! that similar-sounding words collide:
//!
//! | Type | Keys | Default length |
//! |---|---|---|
//! | [`SoundEx`] | one | 4 characters (initial + 3 digits) |
//! | [`Metaphone`] | one | 32 |
//! | [`DoubleMetaphone`] | **two** | 32 |
//! | [`SoundExDM`] (Daitch-Mokotoff) | one | 6 digits |
//!
//! ```
//! use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};
//!
//! assert_eq!(SoundEx::new().process("Robert"), "R163");
//! assert_eq!(Metaphone::new().process("phonetics"), "FNTKS");
//! assert_eq!(SoundExDM::new().process("ALPERT"), "087930");
//! assert_eq!(
//!     DoubleMetaphone::new().process("astromech"),
//!     ("ATRMX".to_owned(), "ATRMK".to_owned())
//! );
//! ```
//!
//! # These are transcriptions, not implementations
//!
//! Do not substitute a textbook algorithm or a third-party crate for any of
//! these: all four have documented, load-bearing deviations from the published
//! specifications, and the deviations are what callers of the reference actually
//! observe.
//!
//! * [`SoundEx`] condenses repeated digits *before* dropping non-digits, so
//!   `h`, `w` and vowels separate equal codes: `Ashcraft` is `A226`, not the
//!   textbook `A261`. Digits in the input survive into the code.
//! * [`Metaphone`] applies `transformX` after `cTransform`, so a word-initial
//!   `ch` that became `xh` is turned into `sh`: `chemical` is `SHMKL`, not
//!   `KMKL`. The reference's own test suite has that case commented out.
//! * [`DoubleMetaphone`] contains a family of the reference truthiness accidents —
//!   an `indexOf` used as a boolean, comparisons between slices and literals of
//!   different lengths, `pos++` in branches that consumed nothing — each of
//!   which changes the encoding of real words.
//! * [`SoundExDM`] declares the genuine Daitch-Mokotoff dual codes and never
//!   reads them, and leaks the string `"undefined"` into its output when the
//!   input contains the digit `0` after a coded letter.
//!
//! Every one of these is pinned by a unit test and by
//! `fixtures/phonetics.json`, recorded from the reference itself.
//!
//! # Unicode
//!
//! The reference indexes strings by UTF-16 code unit, and these algorithms index,
//! slice and truncate constantly. That is observable on astral-plane input — see
//! [`units`] for the mechanism and for the ASCII fast path that keeps exactness
//! free on ordinary text. Two encoders can produce an **unpaired surrogate**,
//! which a Rust `String` cannot hold; both offer a `…utf16` method that returns
//! the exact code units, and render `U+FFFD` otherwise.
//!
//! # Phonetic neighbor indexing (Verbora extension, not parity)
//!
//! [`PhoneticIndex`] answers "which stored words share a phonetic code with
//! this one?" over a whole dictionary — candidate generation, not a search
//! engine. This is a Verbora-native addition with no reference equivalent
//! and no `docs/MIGRATION_MATRIX.md` entry; see the [`index`] module for the
//! full design.
//!
//! # Beider-Morse Phonetic Matching (Verbora extension, not parity)
//!
//! The four encoders above are each tuned for one language's (mostly
//! English's) orthography — none solve the problem a genealogical name
//! index actually has: the *same* family name has different
//! phonetically-equivalent spellings depending on which country's
//! conventions transcribed it. [`BeiderMorse`] does, across 18 languages
//! (Generic), 10 (Ashkenazi), or 5 (Sephardic) — auto-detecting which one(s)
//! a name is plausibly spelled under, or restricting to one explicitly via
//! [`BeiderMorse::encode_language`]. Like [`PhoneticIndex`], this is a
//! Verbora-native addition with no the reference equivalent and no
//! `docs/MIGRATION_MATRIX.md` entry — see the [`beider_morse`] module for
//! the full design, including why its output ([`BeiderMorseCode`]'s
//! variable-length candidate list) is deliberately not [`PhoneticCodes`].
//!
//! ```
//! use verbora_phonetics::{BeiderMorse, NameType, RuleType};
//!
//! let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
//! let code = bm.encode("Renault");
//! assert!(code.spellings.iter().any(|s| s == "rinD"));
//! ```
//!
//! # Batch encoding, and when to parallelise it
//!
//! A single call to any encoder is cheap — tens to a couple hundred
//! nanoseconds — which means naively spawning one thread-pool task per word is
//! usually a net loss: the scheduling overhead is the same order of magnitude
//! as the work. Encoding *many* words, such as building a phonetic index over
//! a large dictionary, is a different story once the dictionary is large
//! enough. The optional `parallel` Cargo feature adds `par_encode_batch` and
//! `par_encode_double_batch`, which chunk the work across `rayon`'s thread
//! pool instead of dispatching per word; see the `parallel` module (built
//! only with that feature) for the full reasoning, the measured crossover,
//! and what it costs.

pub mod beider_morse;
pub mod caverphone;
pub mod cologne;
pub mod daitch_mokotoff;
pub mod dm_soundex;
pub mod double_metaphone;
pub mod index;
pub mod match_rating;
pub mod metaphone;
pub mod nysiis;
#[cfg(feature = "parallel")]
pub mod parallel;
pub mod phonex;
pub mod refined_soundex;
pub mod soundex;
pub mod units;

mod dm_table;
mod pipe;

pub use beider_morse::{BeiderMorse, BeiderMorseCode, Language, LanguageSet, NameType, RuleType};
pub use caverphone::{Caverphone1, Caverphone2};
pub use cologne::Cologne;
pub use daitch_mokotoff::DaitchMokotoff;
pub use dm_soundex::{RuleMapping, Rules, SoundExDM};
pub use double_metaphone::DoubleMetaphone;
pub use index::{
    DaitchMokotoffCode, EntryId, InlineCode, MetaphoneCode, Neighbors, PhoneticCodes,
    PhoneticCodesIter, PhoneticEncoder, PhoneticIndex, PhoneticIndexBuilder, SoundexCode,
};
pub use match_rating::MatchRatingApproach;
pub use metaphone::Metaphone;
pub use nysiis::Nysiis;
#[cfg(feature = "parallel")]
pub use parallel::{DEFAULT_CHUNK_SIZE, par_encode_batch, par_encode_double_batch};
pub use phonex::Phonex;
pub use refined_soundex::RefinedSoundex;
pub use soundex::SoundEx;

use std::fmt;

/// A condition under which the reference throws.
///
/// Both variants are reachable from ordinary arguments, so they are surfaced as
/// errors rather than panics. The infallible entry points ([`SoundEx::process`],
/// [`SoundExDM::process`]) cannot produce either.
#[derive(Debug, Clone, PartialEq)]
pub enum PhoneticError {
    /// [`SoundEx::try_process`] was given a token whose first character is a
    /// regular-expression metacharacter.
    ///
    /// The reference builds `new RegExp('^' + transform(token.charAt(0)))` out
    /// of that character, so `(`, `)`, `*`, `+`, `?`, `[` and `\` make the
    /// constructor throw a `SyntaxError`. Real token streams contain
    /// punctuation, so this is not a theoretical case.
    InvalidInitialPattern(char),
    /// [`SoundExDM::try_process`] was given a `codeLength` that makes the
    /// reference's `new Array(n).join('0')` padding throw a `RangeError`,
    /// because `n` is not a non-negative integer below 2³².
    InvalidArrayLength(f64),
}

impl fmt::Display for PhoneticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInitialPattern(c) => write!(
                f,
                "SoundEx builds a regular expression from the first character, and {c:?} is a metacharacter"
            ),
            Self::InvalidArrayLength(n) => {
                write!(f, "invalid array length {n} while padding the code")
            }
        }
    }
}

impl std::error::Error for PhoneticError {}

/// Encodes an already-tokenized stream, dropping stop words.
///
/// This is the reusable half of `Phonetic#tokenizeAndPhoneticize(str,
/// processor, keepStops)`. The reference splits the string itself with a shared
/// module-level `AggressiveTokenizer`; that tokenizer lives in another crate, so
/// the string-input wrapper is left to the caller and this function takes the
/// tokens directly.
///
/// Filtering follows the reference, which is *case-sensitive* and tests the
/// **raw** token: `the` is dropped but `The` is not, and a token like `"it's"`,
/// `co-op` or `/path` is encoded verbatim, punctuation and all.
///
/// The list consulted is the reference's process-wide mutable one, which
/// `addStopWord` on any stemmer can change at runtime. Use
/// [`phoneticize_tokens_with`] to pass an explicit list instead.
///
/// ```
/// use verbora_phonetics::{Metaphone, phoneticize_tokens};
///
/// let metaphone = Metaphone::new();
/// let keys = phoneticize_tokens(["the", "quick", "brown", "fox"], false, |t| {
///     metaphone.process(t)
/// });
/// assert_eq!(keys, ["KK", "BRN", "FKS"]);
/// ```
pub fn phoneticize_tokens<'a, T, O>(
    tokens: T,
    keep_stops: bool,
    process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where
    T: IntoIterator<Item = &'a str>,
{
    filter_and_map(
        tokens,
        |t| keep_stops || !verbora_core::stopwords::is_default_stopword(t),
        process,
    )
}

/// As [`phoneticize_tokens`], but against a caller-supplied stop-word list.
///
/// Prefer this where reproducible behaviour matters: it touches no global state.
///
/// ```
/// use verbora_core::StopWords;
/// use verbora_phonetics::{SoundEx, phoneticize_tokens_with};
///
/// let soundex = SoundEx::new();
/// let stops = StopWords::from_iter_of(["the"]);
/// let keys = phoneticize_tokens_with(["the", "fox"], &stops, false, |t| soundex.process(t));
/// assert_eq!(keys, ["F200"]);
/// ```
pub fn phoneticize_tokens_with<'a, T, O>(
    tokens: T,
    stop_words: &verbora_core::StopWords,
    keep_stops: bool,
    process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where
    T: IntoIterator<Item = &'a str>,
{
    filter_and_map(tokens, |t| keep_stops || !stop_words.contains(t), process)
}

/// Tokenizes `text` and encodes each surviving token.
///
/// This is the complete `Phonetic#tokenizeAndPhoneticize(str, processor,
/// keepStops)`. The reference splits with a module-level `AggressiveTokenizer`
/// shared by all four phonetic algorithms, so this uses the same tokenizer.
///
/// Note the asymmetry with the stemmers, which lower-case before tokenizing:
/// this path does **not**, so `"The"` survives the stop-word filter (which is
/// case-sensitive) while `"the"` is dropped. That is the reference's behaviour,
/// not an oversight here.
///
/// ```
/// use verbora_phonetics::{Metaphone, tokenize_and_phoneticize};
///
/// let metaphone = Metaphone::new();
/// let keys = tokenize_and_phoneticize("The quick brown fox", false, |t| {
///     metaphone.process(t)
/// });
/// // "The" is capitalised, so it is not the stop word "the" and survives.
/// assert_eq!(keys, ["0", "KK", "BRN", "FKS"]);
/// ```
pub fn tokenize_and_phoneticize<O>(
    text: &str,
    keep_stops: bool,
    process: impl FnMut(&str) -> O,
) -> Vec<O> {
    let tokenizer = verbora_tokenizers::AggressiveTokenizer::new();
    let tokens = verbora_tokenizers::Tokenize::tokenize(&tokenizer, text);
    filter_and_map(
        tokens.iter().copied(),
        |t| keep_stops || !verbora_core::stopwords::is_default_stopword(t),
        process,
    )
}

/// As [`tokenize_and_phoneticize`], but against a caller-supplied stop-word list.
///
/// Prefer this where reproducible behaviour matters: it touches no global state.
pub fn tokenize_and_phoneticize_with<O>(
    text: &str,
    stop_words: &verbora_core::StopWords,
    keep_stops: bool,
    process: impl FnMut(&str) -> O,
) -> Vec<O> {
    let tokenizer = verbora_tokenizers::AggressiveTokenizer::new();
    let tokens = verbora_tokenizers::Tokenize::tokenize(&tokenizer, text);
    filter_and_map(
        tokens.iter().copied(),
        |t| keep_stops || !stop_words.contains(t),
        process,
    )
}

/// The shared body of the two `phoneticize_tokens` entry points.
fn filter_and_map<'a, T, O>(
    tokens: T,
    keep: impl Fn(&str) -> bool,
    mut process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where
    T: IntoIterator<Item = &'a str>,
{
    tokens
        .into_iter()
        .filter(|t| keep(t))
        .map(&mut process)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_core::StopWords;

    #[test]
    fn phoneticize_tokens_filters_case_sensitively() {
        let m = Metaphone::new();

        let keys = phoneticize_tokens(["the", "quick"], false, |t| m.process(t));
        assert_eq!(keys, ["KK"]);

        // Capitalised stop words are NOT filtered: The reference tests the raw
        // token against a lowercase list.
        let keys = phoneticize_tokens(["The", "quick"], false, |t| m.process(t));
        assert_eq!(keys, ["0", "KK"]);

        let keys = phoneticize_tokens(["the", "quick"], true, |t| m.process(t));
        assert_eq!(keys, ["0", "KK"]);
    }

    #[test]
    fn phoneticize_tokens_supports_two_key_encoders() {
        let dm = DoubleMetaphone::new();
        let stops = StopWords::english();
        let keys =
            phoneticize_tokens_with(["phonetic", "modules"], &stops, false, |t| dm.process(t));
        assert_eq!(
            keys,
            [
                ("FNTK".to_owned(), "FNTK".to_owned()),
                ("MTLS".to_owned(), "MTLS".to_owned())
            ]
        );
    }

    #[test]
    fn errors_display_usefully() {
        assert!(
            PhoneticError::InvalidInitialPattern('(')
                .to_string()
                .contains("metacharacter")
        );
        assert!(
            PhoneticError::InvalidArrayLength(1.5)
                .to_string()
                .contains("1.5")
        );
    }
}
