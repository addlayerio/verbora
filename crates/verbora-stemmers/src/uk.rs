//! The Ukrainian stemmer.
//!
//! Structurally the Russian stemmer with Ukrainian tables, so the scanners live
//! in [`crate::ru`] and only the tables and one rule differ. Read the Russian
//! module documentation first — the longest-suffix reading of the anchored
//! alternations, the empty-result fallback, and the line-terminator behaviour
//! of `.` all carry over unchanged. Ukrainian does **not** fold `ё`.
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
//! Scanning left to right for the first start position that satisfies all
//! three is therefore exactly the leftmost match the regex specifies.
//!
//! # The text unit, and the one rule it moves
//!
//! Positions here are indices in **Unicode scalar values** — see
//! [`crate::units`] for why an algorithm published over letters is measured
//! that way. Ukrainian is the only stemmer in this group whose output the unit
//! actually changes, and [`derivational`] is the whole of it.
//!
//! Every *other* rule in this module compares two positions in the same
//! buffer, so re-indexing moves both sides alike (see [`crate::ru`]'s note).
//! [`derivational`] does not: it walks the word looking for the first position
//! where a non-vowel is followed by a run of vowels. Under a UTF-16 reading a
//! single astral character is **two** non-vowel positions — a high surrogate
//! whose successor is also a non-vowel, and a low surrogate — so the scan would
//! skip the first, match at the second, and return a prefix ending *between the
//! halves of one character*. Decoding that replaces the orphan with `U+FFFD`,
//! which the caller never supplied: `stem("ео𝟎етост")` is `"ео"`, where
//! counting code units would give `"ео\u{FFFD}"`. A `char` buffer has no such
//! state to be in.
//!
//! The two absolute lengths in [`derivational`] — `len >= 4` and `len >= 3` —
//! are counts of characters. Neither is observable: each guards a test
//! that four (or three) named Cyrillic characters sit at the end of the word,
//! which already implies the length, so the guard is a bounds check rather
//! than a rule.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_uk;
use crate::ru::{
    alt_suffix, av_shi, collapse_double, is_line_terminator, or_falsy, split_at_first_vowel,
    strip_final,
};
use crate::stopwords::Language;
use crate::units::text;

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
fn is_vowel(c: char) -> bool {
    matches!(c, 'а' | 'е' | 'и' | 'о' | 'у' | 'ю' | 'я' | 'і' | 'ї' | 'є')
}

/// Whether `c` is one of the characters [`gate_uk`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and nothing in
/// it reaches `U+1D80`, so an astral character is never a Ukrainian letter:
/// neither the character itself nor either half of the surrogate pair encoding
/// it is in the set.
#[inline]
fn is_ukrainian_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_uk(c as u16)
}

fn perfective_gerund(w: &[char]) -> Option<Vec<char>> {
    // `/[ая]в(ши|шись)$/` — identical to the Russian rule.
    if let Some(at) = av_shi(w, GERUND_AV_SHI) {
        return Some(w[..at].to_vec());
    }
    // Ukrainian drops the `ывши`/`ывшись`/`ыв` alternatives Russian carries.
    alt_suffix(w, GERUND).map(|at| w[..at].to_vec())
}

fn adjective(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, ADJECTIVE).map(|at| w[..at].to_vec())
}

fn participle(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, PARTICIPLE).map(|at| w[..at].to_vec())
}

fn adjectival(w: &[char]) -> Option<Vec<char>> {
    let result = adjective(w)?;
    Some(or_falsy(participle(&result), &result))
}

/// `/(с[яьи])$/` — a character class, so three alternatives.
fn reflexive(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, REFLEXIVE).map(|at| w[..at].to_vec())
}

fn verb(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, VERB).map(|at| w[..at].to_vec())
}

fn noun(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, NOUN).map(|at| w[..at].to_vec())
}

fn superlative(w: &[char]) -> Option<Vec<char>> {
    alt_suffix(w, SUPERLATIVE).map(|at| w[..at].to_vec())
}

/// The hand-written lookbehind rule; see the module documentation.
///
/// Returns the string with the leftmost match — which always runs to the end —
/// removed, or `None` when no start position works.
///
/// # The scan is per character
///
/// `p` is the `[^аеиоуюяіїє]` the match opens on, and `w[..p]` is what
/// survives, so `p` is a cut position and every character of the word is one
/// position of `w`. That is the whole of the unit correction in this module:
/// under the old UTF-16 reading an astral character occupied two positions and
/// the scan could stop between them, cutting one character in half. See the
/// module documentation.
fn derivational(w: &[char]) -> Option<Vec<char>> {
    let len = w.len();
    // `(?<=о)сть?$`: the index of the `с`. The `len >= 4`/`len >= 3` guards
    // are bounds checks rather than rules — four (three) named characters at
    // the end already imply the length.
    let c_pos = if len >= 4
        && w[len - 1] == 'ь'
        && w[len - 2] == 'т'
        && w[len - 3] == 'с'
        && w[len - 4] == 'о'
    {
        len - 3
    } else if len >= 3 && w[len - 1] == 'т' && w[len - 2] == 'с' && w[len - 3] == 'о' {
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
        let t: Vec<char> = token.to_lowercase().chars().collect();

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
        strip_final(&mut result, 'и'); // /и$/

        // `result` is not read again after this, so it can move into whichever
        // branch is taken instead of being cloned up front.
        let derived =
            if r2_tail.is_some_and(|tail| !tail.is_empty() && derivational(tail).is_some()) {
                // As in Russian, the guard passing does not guarantee this
                // second search hits; the string is kept as it stands.
                derivational(&result).unwrap_or(result)
            } else {
                result
            };

        let mut out = or_falsy(superlative(&derived), &derived);
        out = collapse_double(&out, 'н'); // /(н)н/g
        strip_final(&mut out, 'ь'); // /ь$/

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
        token.chars().any(is_ukrainian_letter)
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

    // -----------------------------------------------------------------------
    // The text unit
    // -----------------------------------------------------------------------

    /// A character outside the Basic Multilingual Plane, and a character
    /// inside it that is its exact equal for every question this module asks.
    ///
    /// `U+1D7CE` (MATHEMATICAL BOLD DIGIT ZERO) and `U+4E2D` are both outside
    /// [`is_vowel`], outside [`is_line_terminator`], absent from every rule
    /// table (the highest code point in any of them is `U+0457`), fixed points
    /// of `str::to_lowercase`, and rejected by [`is_ukrainian_letter`]. Under
    /// the crate's unit each is exactly **one** position of the working
    /// buffer, so substituting one for the other cannot change an answer.
    ///
    /// That is the whole content of the tests below, and it is a real claim
    /// rather than a tautology: under the UTF-16 reading this stemmer replaced
    /// [`ASTRAL`] with *two* positions, and the substitution did change
    /// answers — 159 of the 286,326 placements the enumeration below walks.
    const ASTRAL: char = '\u{1D7CE}';
    /// See [`ASTRAL`].
    const BMP_TWIN: char = '\u{4E2D}';

    /// `word` with `c` inserted before its `i`th character.
    fn insert_at(word: &str, i: usize, c: char) -> String {
        let cs: Vec<char> = word.chars().collect();
        let mut out: String = cs[..i].iter().collect();
        out.push(c);
        out.extend(&cs[i..]);
        out
    }

    /// `derivational` cuts at the character its match opened on, never between
    /// the halves of one.
    ///
    /// The expected value is worked from the rule, not recorded from the code.
    /// The rule (see the module documentation) is
    ///
    /// ```text
    /// /[^аеиоуюяіїє][аеиоуюяіїє]+[^аеиоуюяіїє]+[аеиоуюяіїє].*(?<=о)сть?$/
    /// ```
    ///
    /// and `stem("ео𝟎етост")` runs like this:
    ///
    /// * `е` at position 0 is a vowel, so the region split gives
    ///   `head = "е"` and `RV = "о𝟎етост"` — seven characters, `о` `𝟎` `е` `т`
    ///   `о` `с` `т`.
    /// * `…ост` ends in none of the perfective-gerund, reflexive, adjectival,
    ///   verb or noun tables, so the working string is `RV` unchanged.
    /// * R2's tail is `"𝟎етост"`, which the rule *does* match — `𝟎` is the
    ///   opening non-vowel, `е` the vowel run, `т` the consonant run, `о` the
    ///   vowel after it, and the string ends in `ост` — so the rule is applied
    ///   to the working string as well.
    /// * In `"о𝟎етост"` position 0 is the vowel `о` and cannot open the match.
    ///   Position 1 is `𝟎`, followed by the vowel `е`, the consonant `т` and
    ///   the vowel `о`, all before the `с` of `ост` at position 5. So the
    ///   leftmost match opens at position 1, everything from there is removed,
    ///   and `RV[..1]` is `"о"`.
    /// * `superlative` misses, there is no `нн` and no final `ь`, so the answer
    ///   is `head + "о"`.
    ///
    /// Position 1 is `𝟎`'s **only** position, because a position is one
    /// Unicode scalar value. Counting it as two — the UTF-16 reading — moved
    /// the match one place to the right and cut the character in half, and the
    /// decode then rendered the orphaned surrogate as `U+FFFD`: this same call
    /// answered `"ео\u{FFFD}"`, a character the caller never supplied.
    #[test]
    fn derivational_cuts_where_the_rule_matched() {
        assert_eq!(s("ео𝟎етост"), "ео");
        // The same word with an inert Basic Multilingual Plane character in
        // place of the astral one. It is one position under either reading, so
        // it is what the astral case has to agree with.
        assert_eq!(s("ео中етост"), "ео");
    }

    /// One character in, one character out: no stem may contain a character
    /// the caller did not supply.
    ///
    /// A `char` working buffer cannot hold half a character, so no cut this
    /// module makes — a table match, a region bound, or [`derivational`]'s
    /// hand-written scan, which relates to neither — can produce one. The
    /// UTF-16 buffer could, and did.
    #[test]
    fn no_stem_invents_a_replacement_character() {
        let corpus = astral_corpus();
        for word in &corpus {
            let out = s(word);
            assert!(
                !out.contains('\u{FFFD}'),
                "stem({word:?}) returned {out:?}, which the caller never supplied"
            );
        }
        assert_eq!(corpus.len(), placements());
    }

    /// Every entry of the Ukrainian stop-word list and of every rule table,
    /// with one inert character inserted at **every** position — as an astral
    /// character and as its Basic-Multilingual-Plane twin — must stem alike.
    ///
    /// This is the enumeration, not a sample: 124 stop words, all 143 rule
    /// table entries across the eight tables, and 40,000 seeded words built
    /// from the Ukrainian alphabet crossed with the real suffixes, each walked
    /// at every one of its insertion points. 286,326 placements in all.
    #[test]
    fn an_astral_character_counts_as_one_position() {
        let twin = BMP_TWIN.to_string();
        let astral = ASTRAL.to_string();
        let corpus = astral_corpus();
        for word in &corpus {
            let bmp = word.replace(ASTRAL, &twin);
            assert_eq!(
                s(word),
                s(&bmp).replace(BMP_TWIN, &astral),
                "stem({word:?}) does not agree with its BMP twin {bmp:?}"
            );
        }
        assert_eq!(corpus.len(), placements());
    }

    /// The size of [`astral_corpus`], derived from its own seeds rather than
    /// recorded: a seed of `n` characters has `n + 1` insertion points.
    fn placements() -> usize {
        astral_seeds().iter().map(|s| s.chars().count() + 1).sum()
    }

    /// What the enumerations walk: **every** Ukrainian stop word, **every**
    /// rule table entry, and a seeded corpus of Ukrainian shapes.
    ///
    /// The composition is arithmetic, not a sample of convenience: 124 shipped
    /// stop words, the 108 entries of the eight rule tables, and 40,000 seeded
    /// words built from the Ukrainian alphabet crossed with the real endings —
    /// `ост`/`ость` among them, since that is the only shape [`derivational`]
    /// can fire on at all, and [`derivational`] is the only rule the text unit
    /// reaches.
    fn astral_seeds() -> Vec<String> {
        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x
            }
            fn below(&mut self, n: usize) -> usize {
                (self.next() % n as u64) as usize
            }
        }
        const ALPHA: &[char] = &[
            'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
            'т', 'у', 'ч', 'ш', 'щ', 'ь', 'ю', 'я', 'і', 'ї', 'є',
        ];
        const SUFFIXES: &[&str] = &[
            "ость",
            "ост",
            "остю",
            "ості",
            "авши",
            "явши",
            "ившись",
            "ими",
            "ого",
            "ому",
            "ої",
            "ся",
            "сь",
            "си",
            "ать",
            "ять",
            "учи",
            "ячи",
            "вши",
            "ши",
            "ями",
            "ами",
            "ей",
            "ий",
            "ій",
            "ею",
            "єю",
            "ою",
            "єм",
            "ів",
            "їв",
            "ейш",
            "ейше",
            "нн",
            "н",
            "и",
            "ь",
        ];

        let mut seeds: Vec<String> = Language::Uk
            .defaults()
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        for (_, table) in audit::TABLES {
            seeds.extend(table.iter().map(|e| (*e).to_owned()));
        }
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for _ in 0..40_000 {
            let mut w = String::new();
            for _ in 0..1 + rng.below(7) {
                w.push(ALPHA[rng.below(ALPHA.len())]);
            }
            if rng.below(10) < 8 {
                w.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            }
            seeds.push(w);
        }
        assert_eq!(seeds.len(), 124 + 108 + 40_000);
        seeds
    }

    /// [`astral_seeds`] with [`ASTRAL`] inserted at every position of every
    /// seed. Nothing here is sampled: every seed contributes every one of its
    /// insertion points.
    fn astral_corpus() -> Vec<String> {
        let mut out = Vec::new();
        for seed in &astral_seeds() {
            for i in 0..=seed.chars().count() {
                out.push(insert_at(seed, i, ASTRAL));
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // The tables, walked through the documented pipeline
    // -----------------------------------------------------------------------

    /// Every shipped stop word that *is* one token still reaches
    /// [`TokenizeAndStem::is_stop_word`] spelled the way the list spells it.
    ///
    /// This is the failure this migration has produced eight times: a stage
    /// transforms the text before a later stage looks it up in a table spelled
    /// the old way. Ukrainian's pipeline is `prepare` (the identity here),
    /// then UAX #29 word segmentation, then a lookup on the **raw** token
    /// (`FILTER_ON = Casing::Raw`), so every entry has to come back out of the
    /// tokenizer whole.
    ///
    /// # Three entries that cannot
    ///
    /// `може бути`, `все ще` and `хотів би` are *phrases*. A space is a word
    /// boundary under UAX #29 with no tailoring available to change it, so the
    /// tokenizer hands `is_stop_word` two tokens and neither is on the list —
    /// those three entries are unreachable through
    /// [`TokenizeAndStem::tokenize_and_stem`] and always were. That is a
    /// property of the shipped list rather than of the text unit, and it is
    /// pinned here as *exactly* the space-bearing entries so that a fourth one
    /// appearing is a test failure rather than a silent loss.
    #[test]
    fn every_single_token_stop_word_survives_the_pipeline() {
        let st = PorterStemmerUk::new();
        let words = Language::Uk.defaults();
        assert_eq!(words.len(), 124);
        let unfiltered: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| !st.tokenize_and_stem(w, false).is_empty())
            .collect();
        let phrases: Vec<&str> = words.iter().copied().filter(|w| w.contains(' ')).collect();
        assert_eq!(unfiltered, phrases);
        assert_eq!(phrases.len(), 3);
    }

    /// Every rule table entry measures the same as text and as buffer, and a
    /// cut by its own length lands where the entry starts.
    ///
    /// The tables are `&'static str` and are never re-encoded, so the unit
    /// they are *measured* in is the only thing the migration could have
    /// moved. There are 108 entries across the eight tables — 2 + 3 + 3 + 2 +
    /// 26 + 12 + 18 + 42 — and none of them is astral (the highest code point
    /// in any is `U+0457`), which is asserted here rather than assumed: that
    /// premise is what lets a buffer length have a table entry's length
    /// subtracted from it at all.
    #[test]
    fn every_rule_table_entry_measures_the_same_as_the_buffer() {
        let mut entries = 0usize;
        for (name, table) in audit::TABLES {
            for entry in *table {
                entries += 1;
                assert!(
                    entry.chars().all(|c| (c as u32) < 0x1_0000),
                    "{name} carries the astral entry {entry:?}"
                );
                // A word the entry is the suffix of. `slen` is the entry's
                // length in the crate's unit; the buffer's own count of it
                // must be the same number, and cutting by it must leave the
                // prefix whole.
                let probe: Vec<char> = format!("бо{entry}").chars().collect();
                let n = crate::units::slen(entry);
                assert_eq!(n, entry.chars().count(), "{name} {entry:?}");
                assert!(
                    crate::units::ends_with(&probe, entry),
                    "{name} {entry:?} is not found at the end of its own probe"
                );
                assert_eq!(
                    text(&probe[..probe.len() - n]),
                    "бо",
                    "{name} {entry:?} cuts in the wrong place"
                );
            }
        }
        assert_eq!(entries, 108);
    }
}
