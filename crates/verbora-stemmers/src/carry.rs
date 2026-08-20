//! The Carry French stemmer, ported from the reference `Carry` module.
//!
//! Three passes over a suffix table. Each pass tries the **longest** suffix
//! first, and within a suffix length tries the "minimum radix 1" table before the
//! "minimum radix 2" one; a candidate is accepted only when its *word size* —
//! the number of vowel-to-consonant transitions — exceeds that minimum. If a
//! suffix is present in the first table but fails the size test, the second table
//! is tried for the **same** suffix before moving on to a shorter one. That is
//! why `acteur` stems to `ac` via `act` rather than directly: `wordSize("ac")` is
//! 1, which is not greater than 1.
//!
//! Two details a reader will not guess:
//!
//! * The suffix loop starts at `word.length - 1`, so the whole word is never a
//!   candidate suffix and a word of one character or less is never transformed.
//! * The input is **not** lowercased, so the all-lowercase tables never fire on
//!   `ÉTUDE` — while `getWordSize`'s vowel regex *does* carry `/i`, so uppercase
//!   letters still count as vowels. `stem("étude")` is `"étud"`; `stem("ÉTUDE")`
//!   is `"ÉTUDE"`.
//!
//! # Divergence: `Object.prototype`
//!
//! The reference looks suffixes up with `transformations[suffix]` on a plain
//! object, so twelve suffixes reach `Object.prototype` and return a function or
//! `Object.prototype` itself, which is then string-concatenated:
//! `stem("xxconstructor")` is `"xxfunction Object() { [native code] }"`.
//! The twelve are `constructor`, `__proto__`, `toString`, `valueOf`,
//! `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`, `toLocaleString`,
//! `__defineGetter__`, `__defineSetter__`, `__lookupGetter__` and
//! `__lookupSetter__`. The strings are the reference engine's `Function.prototype.toString` output
//! and are not reproducible portably, so this port returns the ordinary stem
//! instead (`"xxconstructo"`). The parity suite records the reference values and
//! asserts that this is the *only* place the two disagree.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::carry_tables::STEPS;
use crate::data::charsets::is_carry_vowel;
use crate::data::gates::gate_fr;
use crate::stopwords::Language;

/// The Carry French stemmer.
///
/// ```
/// use verbora_stemmers::CarryStemmerFr;
/// let s = CarryStemmerFr::new();
/// assert_eq!(s.stem("acteur"), "ac");
/// assert_eq!(s.stem("chevaux"), "cheval");
/// assert_eq!(s.stem("ÉTUDE"), "ÉTUDE"); // the tables are lowercase-only
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CarryStemmerFr;

/// `getWordSize` — the count of vowel-to-consonant transitions.
///
/// Reads the module-level `defaultConf.vowels`, never the instance's `conf`;
/// that latent bug is unobservable because only one instance is ever built.
fn word_size(word: &str) -> usize {
    let mut prev_vowel = false;
    let mut groups = 0;
    for c in word.chars() {
        // A non-BMP character is two code units in the reference, both surrogates
        // and both non-vowels; a run of non-vowels contributes one transition
        // however long it is, so counting characters gives the same answer.
        let vowel = (c as u32) < 0x1_0000 && is_carry_vowel(c as u16);
        if !vowel && prev_vowel {
            groups += 1;
        }
        prev_vowel = vowel;
    }
    groups
}

/// A sorted `(suffix, replacement)` table lookup.
fn lookup(table: &[(&str, &'static str)], suffix: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(suffix))
        .ok()
        .map(|i| table[i].1)
}

/// `tranform(word, stepConf)` — one of the three passes.
fn transform(word: &str, step: &[&[(&str, &'static str)]]) -> Option<String> {
    // Character offsets, so a suffix boundary never splits a code point.
    let offsets: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    let n = offsets.len();
    // `for (let suffixLength = word.length - 1; suffixLength > 0; …)`
    for suffix_len in (1..n).rev() {
        let cut = offsets[n - suffix_len];
        let suffix = &word[cut..];
        let base = &word[..cut];
        for (min_radix, table) in step.iter().enumerate() {
            let Some(replacement) = lookup(table, suffix) else {
                continue;
            };
            let candidate = format!("{base}{replacement}");
            if word_size(&candidate) > min_radix {
                return Some(candidate);
            }
        }
    }
    None
}

impl CarryStemmerFr {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let mut current: Option<String> = None;
        for step in STEPS {
            let input: &str = current.as_deref().unwrap_or(word);
            if let Some(next) = transform(input, step) {
                current = Some(next);
            }
        }
        current.map_or(Cow::Borrowed(word), Cow::Owned)
    }
}

impl TokenizeAndStem for CarryStemmerFr {
    // French is the only language that lowercases the token BEFORE consulting
    // the stop-word list.
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::Fr.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_fr)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for CarryStemmerFr {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        CarryStemmerFr::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("action", "ac"),
            ("acteur", "ac"),
            ("actrices", "ac"),
            ("volera", "vol"),
            ("petit", "pet"),
            ("manège", "manèg"),
            ("cheval", "cheval"),
            ("chevaux", "cheval"),
            ("beaux", "beal"),
            ("yeux", "yeux"),
            ("œuf", "œuv"),
            ("œufs", "œuv"),
            ("étude", "étud"),
            ("ÉTUDE", "ÉTUDE"),
            ("naïve", "naïv"),
            ("être", "êtr"),
            ("Dleyton", "Dleyton"),
            ("a", "a"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn short_words_are_never_emptied() {
        // The suffix loop stops one short of the whole word.
        for w in ["", "a", "e", "x", "y", "t"] {
            assert_eq!(s(w), w);
        }
    }

    #[test]
    fn word_size_counts_vowel_consonant_transitions() {
        assert_eq!(word_size(""), 0);
        assert_eq!(word_size("ac"), 1);
        assert_eq!(word_size("act"), 1);
        assert_eq!(word_size("acteur"), 2);
        assert_eq!(word_size("AEIOU"), 0);
        assert_eq!(word_size("yeux"), 1, "y is not a vowel here");
    }

    #[test]
    fn prototype_names_get_the_ordinary_stem() {
        // Documented divergence: the reference splices the reference engine's function source in here.
        assert_eq!(s("xxconstructor"), "xxconstructo");
        // No Carry suffix matches any tail of "__proto__", so the whole word
        // survives; the reference instead returns "xx[object Object]".
        assert_eq!(s("xx__proto__"), "xx__proto__");
        assert_eq!(s("xxvalueOf"), "xxvalueOv");
        // A bare property name never leaks even in the reference: the suffix loop
        // starts at `length - 1`, so the whole word is never looked up.
        assert_eq!(s("__proto__"), "__proto__");
        assert_eq!(s("valueOf"), "valueOv");
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("123"), "123");
        assert_eq!(s("a-b"), "a-b");
    }
}
