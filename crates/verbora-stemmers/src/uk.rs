//! The Ukrainian stemmer, ported from
//! The reference `porter_stemmer_uk`.
//!
//! Structurally a copy of the Russian stemmer with Ukrainian tables, so the
//! scanners live in [`crate::ru`] and only the tables and one rule differ. Read
//! the Russian module documentation first — the longest-suffix reading of the
//! anchored alternations, the falsy `||`, and the line-terminator behaviour of
//! `.` all carry over unchanged. Ukrainian does **not** fold `ё`.
//!
//! # The one rule with a lookbehind
//!
//! `derivational` is
//!
//! ```text
//! /[^аеиоуюяіїє][аеиоуюяіїє]+[^аеиоуюяіїє]+[аеиоуюяіїє].*(?<=о)сть?$/
//! ```
//!
//! and the Rust `regex` crate has no lookbehind, so it is scanned by hand. Three
//! observations make the scan exact rather than approximate:
//!
//! * The match always ends at `$`, so `сть?`'s greedy `?` cannot change the
//!   *extent* of the match, only whether one exists. The condition reduces to
//!   "the string ends in `ост` or `ость`" — the lookbehind's `о` is the one
//!   already inside those literals.
//! * `[V]+` and `[^V]+` cannot be shortened: taking fewer vowels leaves a vowel
//!   where the following `[^V]+` needs a non-vowel. So both runs are maximal and
//!   the vowel that follows them is at a single determined index.
//! * `.*` is the only part that rejects line terminators, so a `\n` before the
//!   `ост` merely constrains how far left the preceding vowel may sit.
//!
//! Scanning left to right for the first start position that satisfies all three
//! reproduces the leftmost-match rule the engine applies.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_uk;
use crate::ru::{
    alt_suffix, av_shi, collapse_double, is_line_terminator, or_falsy, split_at_first_vowel,
    strip_final,
};
use crate::stopwords::Language;
use crate::units::{text, units};

/// The Ukrainian stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerUk;
/// let s = PorterStemmerUk::new();
/// assert_eq!(s.stem("важливий"), "важлив");
/// assert_eq!(s.stem("ВАЖЛИВИЙ"), "важлив");
/// assert_eq!(s.stem("мама"), "мам");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerUk;

/// `[аеиоуюяіїє]`.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x0430 | 0x0435 | 0x0438 | 0x043E | 0x0443 | 0x044E | 0x044F | 0x0456 | 0x0457 | 0x0454
    )
}

fn perfective_gerund(w: &[u16]) -> Option<Vec<u16>> {
    // `/[ая]в(ши|шись)$/` — identical to the Russian rule.
    if let Some(at) = av_shi(w, GERUND_AV_SHI) {
        return Some(w[..at].to_vec());
    }
    // Ukrainian drops the `ывши`/`ывшись`/`ыв` alternatives Russian carries.
    alt_suffix(w, GERUND).map(|at| w[..at].to_vec())
}

fn adjective(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, ADJECTIVE).map(|at| w[..at].to_vec())
}

fn participle(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, PARTICIPLE).map(|at| w[..at].to_vec())
}

fn adjectival(w: &[u16]) -> Option<Vec<u16>> {
    let result = adjective(w)?;
    Some(or_falsy(participle(&result), &result))
}

/// `/(с[яьи])$/` — a character class, so three alternatives.
fn reflexive(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, REFLEXIVE).map(|at| w[..at].to_vec())
}

fn verb(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, VERB).map(|at| w[..at].to_vec())
}

fn noun(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, NOUN).map(|at| w[..at].to_vec())
}

fn superlative(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, SUPERLATIVE).map(|at| w[..at].to_vec())
}

/// The hand-written lookbehind rule; see the module documentation.
///
/// Returns the string with the leftmost match — which always runs to the end —
/// removed, or `None` when no start position works.
fn derivational(w: &[u16]) -> Option<Vec<u16>> {
    let len = w.len();
    // `(?<=о)сть?$`: the index of the `с`.
    let c_pos = if len >= 4
        && w[len - 1] == 0x044C // ь
        && w[len - 2] == 0x0442 // т
        && w[len - 3] == 0x0441 // с
        && w[len - 4] == 0x043E
    // о
    {
        len - 3
    } else if len >= 3 && w[len - 1] == 0x0442 && w[len - 2] == 0x0441 && w[len - 3] == 0x043E {
        len - 2
    } else {
        return None;
    };

    // `.*` runs from the trailing vowel to just before the `с`, so no line
    // terminator may sit between them.
    let last_lt = (0..c_pos).rev().find(|&i| is_line_terminator(w[i]));

    for p in 0..len {
        if is_vowel(w[p]) {
            continue;
        }
        // `[V]+` and `[^V]+` are both forced to their maximal length.
        let vowels_end = (p + 1..len).find(|&i| !is_vowel(w[i])).unwrap_or(len);
        if vowels_end == p + 1 {
            continue;
        }
        let cons_end = (vowels_end..len).find(|&i| is_vowel(w[i])).unwrap_or(len);
        if cons_end == vowels_end || cons_end >= len {
            continue;
        }
        let j = cons_end; // the `[V]` after the consonant run
        if j >= c_pos || last_lt.is_some_and(|lt| lt >= j) {
            continue;
        }
        return Some(w[..p].to_vec());
    }
    None
}

/// `(ши|шись)`, the alternation inside `/[ая]в(ши|шись)$/`; scanned by
/// [`crate::ru::av_shi`] rather than searched.
static GERUND_AV_SHI: &[&str] = &["ши", "шись"];
/// The unconditional perfective-gerund alternatives. Ukrainian drops the
/// `ывши`/`ывшись`/`ыв` alternatives Russian carries.
static GERUND: &[&str] = &["ив", "ивши", "ившись"];
/// `/(с[яьи])$/` — a character class, so three alternatives.
static REFLEXIVE: &[&str] = &["ся", "сь", "си"];
static SUPERLATIVE: &[&str] = &["ейш", "ейше"];
/// The adjectival endings.
///
/// Four of these tables shipped with a repeated entry — `ім` here, `ій` and
/// `их` in [`PARTICIPLE`], `ем` and `ю` in [`NOUN`]. Every search in this
/// module is longest-match, so the second copy of an entry can never fire
/// whatever the input; the repeats were removed and `data::table_audit`'s
/// `no_rule_table_lists_the_same_entry_twice` keeps them out.
static ADJECTIVE: &[&str] = &[
    "ими", "ій", "ий", "а", "е", "ова", "ове", "ів", "є", "їй", "єє", "еє", "я", "ім", "ем", "им",
    "их", "іх", "ою", "йми", "іми", "у", "ю", "ого", "ому", "ої",
];
static PARTICIPLE: &[&str] = &[
    "ий", "ого", "ому", "им", "ім", "а", "ій", "у", "ою", "і", "их", "йми",
];
static VERB: &[&str] = &[
    "сь", "ся", "ив", "ать", "ять", "у", "ю", "ав", "али", "учи", "ячи", "вши", "ши", "е", "ме",
    "ати", "яти", "є",
];
static NOUN: &[&str] = &[
    "а", "ев", "ов", "е", "ями", "ами", "еи", "и", "ей", "ой", "ий", "й", "иям", "ям", "ием", "ем",
    "ам", "ом", "о", "у", "ах", "иях", "ях", "ы", "ь", "ию", "ью", "ю", "ия", "ья", "я", "і",
    "ові", "ї", "ею", "єю", "ою", "є", "еві", "єм", "ів", "їв",
];

impl PorterStemmerUk {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let t = units(&token.to_lowercase());

        let Some((head_end, rv_start)) = split_at_first_vowel(&t, is_vowel) else {
            return Cow::Owned(text(&t));
        };
        let head = &t[..head_end];
        let rv = &t[rv_start..];
        let r2_tail = split_at_first_vowel(rv, is_vowel).map(|(_, s)| &rv[s..]);

        let mut result = match perfective_gerund(rv) {
            Some(r) => r,
            None => {
                let reflexed = or_falsy(reflexive(rv), rv);
                adjectival(&reflexed)
                    .or_else(|| verb(&reflexed))
                    .or_else(|| noun(&reflexed))
                    .unwrap_or(reflexed)
            }
        };
        strip_final(&mut result, 0x0438); // /и$/

        // `result` is not read again after this, so it can move into whichever
        // branch is taken instead of being cloned up front.
        let derived =
            if r2_tail.is_some_and(|tail| !tail.is_empty() && derivational(tail).is_some()) {
                // As in Russian, the reference throws when this second call is null.
                derivational(&result).unwrap_or(result)
            } else {
                result
            };

        let mut out = or_falsy(superlative(&derived), &derived);
        out = collapse_double(&out, 0x043D); // /(н)н/g
        strip_final(&mut out, 0x044C); // /ь$/

        let mut full = head.to_vec();
        full.extend_from_slice(&out);
        Cow::Owned(text(&full))
    }
}

impl TokenizeAndStem for PorterStemmerUk {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::Uk.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_uk)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("GERUND_AV_SHI", super::GERUND_AV_SHI),
        ("GERUND", super::GERUND),
        ("ADJECTIVE", super::ADJECTIVE),
        ("PARTICIPLE", super::PARTICIPLE),
        ("REFLEXIVE", super::REFLEXIVE),
        ("VERB", super::VERB),
        ("NOUN", super::NOUN),
        ("SUPERLATIVE", super::SUPERLATIVE),
    ];

    /// The prelude `stem` runs before any table is consulted: lowercasing,
    /// and nothing else. Ukrainian does not fold `ё`.
    pub(crate) fn prelude(token: &str) -> String {
        token.to_lowercase()
    }

    /// The prelude writes no marker unit.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
}

impl verbora_core::Stemmer for PorterStemmerUk {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerUk::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("важливий", "важлив"),
            ("ВАЖЛИВИЙ", "важлив"),
            ("мама", "мам"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    /// The cross-cutting battery every stemmer in this crate answers: empty,
    /// one character, uppercase, accented Latin, Greek, Cyrillic, CJK, an
    /// astral pair, punctuation, digits, a line terminator, and a very long
    /// word.
    ///
    /// The expectations are the *identity* in every row but the case fold,
    /// which is the whole point: none of these is a word of this language, so
    /// a stemmer that changes one is reaching outside its own alphabet.
    #[test]
    fn cross_script_battery() {
        for (input, want) in [
            ("", ""),
            ("a", "a"),
            ("A", "a"),
            ("Ä", "ä"),
            ("café", "café"),
            ("ΟΔΟΣ", "οδος"),
            ("Ω", "ω"),
            ("мама", "мам"),
            ("日本語", "日本語"),
            ("😀", "😀"),
            ("😀ab", "😀ab"),
            ("!?,.", "!?,."),
            ("123", "123"),
            ("\n", "\n"),
        ] {
            assert_eq!(s(input), want, "stem({input:?})");
        }
        assert_eq!(s(&"x".repeat(1000)).len(), 1000);
    }

    #[test]
    fn a_vowelless_word_is_returned_untouched() {
        assert_eq!(s("бвг"), "бвг");
    }
}
