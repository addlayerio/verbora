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
//! # The unit
//!
//! Lengths and suffix boundaries here are **Unicode scalar values**, the unit
//! [`crate::units`] states for the whole crate. Carry was already the closest
//! of the sixteen stemmers to that reading — it slices at `char_indices`
//! offsets and counts transitions per `chars()` — so converting it was a
//! matter of saying so and of replacing the one remaining UTF-16 scan, the
//! gate. Both of the constructs that touched code units are shown below to
//! answer identically under either reading, so this file's stems did not
//! change: it is the reasoning that moved onto firm ground, not the behaviour.
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
///
/// The scan is per character, which is the unit this crate measures in. It was
/// already the answer under the code-unit reading too: Carry's vowel set tops
/// out at `œ` (`U+0153`), so an astral character is not a vowel, and neither is
/// either half of the surrogate pair it used to decompose into — and a run of
/// non-vowels contributes one transition however long it is.
fn word_size(word: &str) -> usize {
    let mut prev_vowel = false;
    let mut groups = 0;
    for c in word.chars() {
        let vowel = is_vowel(c);
        if !vowel && prev_vowel {
            groups += 1;
        }
        prev_vowel = vowel;
    }
    groups
}

/// Carry's vowel class, over a whole character.
///
/// [`is_carry_vowel`] is stated over BMP code points; nothing in the set
/// reaches `U+0154`, so anything above the Basic Multilingual Plane is a
/// consonant for Carry's purposes.
#[inline]
fn is_vowel(c: char) -> bool {
    (c as u32) < 0x1_0000 && is_carry_vowel(c as u16)
}

/// Whether `c` is one of the letters [`gate_fr`] accepts.
///
/// The gate is stated over BMP code points and nothing in it reaches `U+00FD`,
/// so scanning characters and scanning UTF-16 code units accept exactly the
/// same tokens: a BMP character *is* its own code unit, and an astral
/// character is neither in the set itself nor are the two surrogates it used
/// to be scanned as. The scan is per character because that is the crate's
/// unit, not because the answer moved.
#[inline]
fn is_french_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_fr(c as u16)
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
    // Character offsets, so a suffix boundary is a scalar-value boundary and
    // never splits a character.
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
        token.chars().any(is_french_letter)
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

    /// Every stem is a whole-character rewrite of the input: no cut can land
    /// inside a character, so no output can carry a replacement character the
    /// caller never supplied.
    #[test]
    fn stems_are_made_of_whole_characters() {
        for word in [
            "acteur",
            "action",
            "😀acteur",
            "acteur😀",
            "act😀eur",
            "𝟎action",
            "action𝟎",
            "chevaux😀",
            "😀",
            "😀😀",
            "a😀",
            "😀a",
            "naïve😀",
            "œufs😀",
        ] {
            let out = s(word);
            assert!(!out.contains('\u{FFFD}'), "stem({word:?}) = {out:?}");
            // Carry only ever replaces a trailing suffix, so the stem's own
            // characters all come from the input's character sequence.
            assert!(
                out.chars().count() <= word.chars().count() + 3,
                "stem({word:?}) = {out:?}"
            );
        }
    }

    /// The two constructs that used to be stated in UTF-16 code units answer
    /// identically over characters, for every scalar value there is.
    ///
    /// This is the whole justification for converting this file without
    /// changing a stem: `is_carry_vowel` tops out at `U+0153` and `gate_fr` at
    /// `U+00FC`, so neither an astral character nor either half of the
    /// surrogate pair it encodes to is ever admitted.
    #[test]
    fn the_character_scans_agree_with_the_code_unit_scans() {
        let mut buf = [0u16; 2];
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let units = c.encode_utf16(&mut buf);
            assert_eq!(
                is_vowel(c),
                units.iter().any(|u| is_carry_vowel(*u)),
                "U+{cp:04X} vowel"
            );
            assert_eq!(
                is_french_letter(c),
                units.iter().any(|u| gate_fr(*u)),
                "U+{cp:04X} gate"
            );
        }
    }

    #[test]
    fn the_gate_admits_french_letters_and_nothing_astral() {
        assert!(CarryStemmerFr::gate("étude"));
        assert!(CarryStemmerFr::gate("abc"));
        assert!(!CarryStemmerFr::gate("😀"));
        assert!(!CarryStemmerFr::gate("日本語"));
        assert!(!CarryStemmerFr::gate("---"));
        assert!(CarryStemmerFr::gate("😀a"));
    }

    /// A word size counts vowel-to-consonant transitions over characters, so
    /// an astral character contributes exactly one consonant, not two.
    #[test]
    fn word_size_counts_an_astral_character_once() {
        // `a` is a vowel, `😀` a consonant: one transition either way, but the
        // sequence `a😀a😀` has two under a per-character reading.
        assert_eq!(word_size("a😀"), 1);
        assert_eq!(word_size("a😀a😀"), 2);
        assert_eq!(word_size("😀😀"), 0);
        assert_eq!(word_size("a𝟎b"), 1, "one run of consonants, one transition");
    }
}
