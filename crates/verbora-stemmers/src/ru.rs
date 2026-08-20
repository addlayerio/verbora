//! The Russian stemmer.
//!
//! # Rule shapes, and why they are hand-written scans
//!
//! Every rule is an anchored alternation, tested and then applied. Three shapes
//! appear, and each reduces to a scan:
//!
//! | Rule shape | What it computes |
//! |---|---|
//! | `/(a\|bb\|ccc)$/ → ''` | delete the **longest** listed suffix |
//! | `/([ая])(ла\|на\|…)$/ → '$1'` | delete the longest listed suffix that is preceded by `а` or `я`, keeping that letter |
//! | `/(н)н/g → '$1'` | collapse every `нн` to `н`, left to right, non-overlapping |
//!
//! The first shape is *longest*, not first-listed, even though the alternation
//! is ordered: a regex takes the earliest start position at which some
//! alternative reaches `$`, and since every alternative is a distinct literal
//! anchored at the end, the earliest start is the longest suffix. Reading the
//! order as first-match — the Italian convention — would give `"важностию"` a
//! different stem.
//!
//! `stem` computes the first two shapes with one [`crate::among`] binary
//! search per rule group instead of a linear scan per alternative
//! (`docs/PERFORMANCE_GAPS.md` entry 34): the longest listed suffix is what
//! `find_among_b` returns natively, and the preceded-by-`[ая]` shape is the
//! link-walk with a condition ([`crate::among::AmongTable::longest_where`]).
//! Every rule is a tail truncation of RV, so the whole pipeline runs as one
//! shrinking end cursor over a single buffer, with no per-rule `to_vec()`
//! snapshot. The linear-scan form is kept in this module's tests as a
//! differential oracle.
//!
//! # Two rules fall back on an empty result
//!
//! The reflexive and superlative rules fall back to their input when the rule
//! leaves the **empty string**, not only when it fails to match. Stemming
//! `"ся"` therefore returns `"ся"` rather than `""`. Both sites are marked in
//! the code.
//!
//! # `.` does not match a line terminator
//!
//! The region split is `/^(.*?[аеиоюяуыиэ])(.*)$/`. A token containing `\n`,
//! `\r`, U+2028 or U+2029 cannot match it at any offset, so the whole algorithm
//! is skipped and the lowercased, `ё`-folded token is returned unchanged.
//!
//! # The second derivational test is guarded on the original word
//!
//! The derivational rule is *selected* by looking at R2's tail of the word as
//! it was marked, and then *applied* to the shrunken working string. Those two
//! can disagree: R2's tail can end in `ост`/`ость` when the working string no
//! longer does. Verbora's contract for that case is to keep the
//! un-derivationalised string rather than fail — the alternative is an error
//! return on an input nobody can actually construct. It is not reachable in
//! practice: the `ь` that the noun rule always strips from `ость` leaves `ост`
//! behind, which matches, and 200,000 randomised Cyrillic probes find no word
//! that separates the two.
//!
//! # The text unit
//!
//! Every position here — `rv`, `end`, the region bounds, the arguments to
//! [`alt_suffix`] and [`av_shi`] — is an index in **Unicode scalar values**,
//! the unit [`crate::units`] states the crate's contract in. Russian is the
//! one language in this crate where the change of unit provably cannot move
//! an answer, and the reason is worth recording because it is *not* "Cyrillic
//! is on the Basic Multilingual Plane" (a caller may hand this stemmer any
//! text at all):
//!
//! **Every comparison this module makes against a constant is a comparison of
//! two positions in the same buffer.** `end - n >= rv + 2` is
//! `end - n - 2 >= rv`, the position of the `[ая]` against the start of RV;
//! `end - n > rv` and `end > rv` are the same shape; `full > r2s` compares two
//! marked regions. Re-indexing the buffer moves both sides of each of those by
//! the same amount, so every one of them answers alike. There is no absolute
//! length gate anywhere in the algorithm — no `if rv > 3`, no minimum word
//! length — which is exactly what the other Snowball stemmers here have and
//! Russian does not. `an_astral_character_cannot_move_a_russian_answer` enumerates
//! that claim rather than resting on it.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf};
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_ru;
use crate::stopwords::Language;
use crate::units::{ends_with, slen};

/// The Russian stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerRu;
/// let s = PorterStemmerRu::new();
/// assert_eq!(s.stem("важнейшими"), "важн");
/// assert_eq!(s.stem("ёлка"), "елк");
/// assert_eq!(s.stem("ВАЖНАЯ"), "важн");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerRu;

/// `[аеиоюяуыиэ]` — the class the region split uses (`и` appears twice in it).
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(c, 'а' | 'е' | 'и' | 'о' | 'ю' | 'я' | 'у' | 'ы' | 'э')
}

/// Whether `c` is one of the characters [`gate_ru`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and nothing in
/// it reaches `U+1D80`, so an astral character is never a Russian letter:
/// neither the character itself nor either half of the surrogate pair encoding
/// it is in the set. See `crate::data::gates`' own "Unit independence" note.
#[inline]
fn is_russian_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_ru(c as u16)
}

// ---------------------------------------------------------------------------
// Shared scanners, also used by the Ukrainian stemmer
// ---------------------------------------------------------------------------

/// The four code points a regex `.` refuses to match.
///
/// The region split is specified as a regular expression, and this restriction
/// comes with it: a token containing one of these characters does not match the
/// split at any offset, so the algorithm is skipped. Snowball's own Russian
/// knows nothing about line terminators; Verbora keeps the restriction because
/// the split it ships is the regex one and the two must agree. See
/// [`split_at_first_vowel`].
#[inline]
pub(crate) fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/(a|bb|ccc)$/`: the start index of the longest listed suffix of `w`.
pub(crate) fn alt_suffix(w: &[char], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        if ends_with(w, a) {
            let start = w.len() - slen(a);
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/[ая]в(ши|шись)$/`: the start index of the whole match.
///
/// `alts` is the caller's own copy of the alternation — [`SHI`] here and
/// `uk::GERUND_AV_SHI` there — rather than a literal written a second time
/// inside this function, so each language's table stays the single place its
/// own suffixes are spelled and `data::table_audit` can enumerate them.
pub(crate) fn av_shi(w: &[char], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if n + 2 <= w.len()
            && ends_with(w, a)
            && w[w.len() - n - 1] == 'в'
            && matches!(w[w.len() - n - 2], 'а' | 'я')
        {
            let start = w.len() - n - 2;
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/(н)н/g → '$1'`: collapse doubled `н`, non-overlapping, left to right.
pub(crate) fn collapse_double(w: &[char], letter: char) -> Vec<char> {
    let mut out = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if w[i] == letter && w.get(i + 1) == Some(&letter) {
            out.push(letter);
            i += 2;
        } else {
            out.push(w[i]);
            i += 1;
        }
    }
    out
}

/// `/^(.*?[V])(.*)$/`: `(head, tail)`, or `None` when the pattern cannot match.
pub(crate) fn split_at_first_vowel(w: &[char], vowel: fn(char) -> bool) -> Option<(usize, usize)> {
    if w.iter().copied().any(is_line_terminator) {
        // `.` cannot cross a line terminator, and neither `.*?` nor `.*` may skip
        // one, so no offset can produce a match.
        return None;
    }
    let i = w.iter().position(|&c| vowel(c))?;
    Some((i + 1, i + 1))
}

/// Removes the last character when it is `c`; `/c$/` with an empty replacement.
pub(crate) fn strip_final(w: &mut Vec<char>, c: char) {
    if w.last() == Some(&c) {
        w.pop();
    }
}

/// A rule result with the empty string treated as "no result".
///
/// The rule chain falls back to its input both when a rule does not match and
/// when it matches but leaves nothing behind, which is why `stem("ся")` is
/// `"ся"` rather than `""`.
pub(crate) fn or_falsy(value: Option<Vec<char>>, fallback: &[char]) -> Vec<char> {
    match value {
        Some(v) if !v.is_empty() => v,
        _ => fallback.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// The rule groups, as cursor arithmetic over one buffer
// ---------------------------------------------------------------------------
//
// Every rule reads `t[rv..end]` and, on a match, moves `end` left. The
// `Option<usize>` shape keeps the two outcomes the fallbacks distinguish
// apart: `None` is "the rule's pattern did not match", `Some(rv)` is "it
// matched and left the empty string".

/// The sorted search tables, built once from the rule tables below.
struct RuTables {
    /// `(ши|шись)` — the alternation inside `/[ая]в(ши|шись)$/`.
    shi: AmongTable<char>,
    /// The unconditional perfective-gerund alternatives.
    gerund2: AmongTable<char>,
    reflexive: AmongTable<char>,
    adjective: AmongTable<char>,
    /// The `[ая]`-conditioned participle alternatives.
    part1: AmongTable<char>,
    part2: AmongTable<char>,
    /// The `[ая]`-conditioned verb alternatives.
    verb1: AmongTable<char>,
    verb2: AmongTable<char>,
    noun: AmongTable<char>,
    superlative: AmongTable<char>,
    derivational: AmongTable<char>,
}

static TABLES: LazyLock<RuTables> = LazyLock::new(|| RuTables {
    shi: AmongTable::build(SHI),
    gerund2: AmongTable::build(GERUND2),
    reflexive: AmongTable::build(REFLEXIVE),
    adjective: AmongTable::build(ADJECTIVE),
    part1: AmongTable::build(PART1),
    part2: AmongTable::build(PART2),
    verb1: AmongTable::build(VERB1),
    verb2: AmongTable::build(VERB2),
    noun: AmongTable::build(NOUN),
    superlative: AmongTable::build(SUPERLATIVE),
    derivational: AmongTable::build(DERIVATIONAL),
});

/// The perfective-gerund rule over `t[rv..end]`: the new end, or `None`.
fn perfective_gerund_end(t: &[char], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    // `/[ая]в(ши|шись)$/` first, unconditionally preferred over the plain
    // alternatives when it matches: it is the first rule of the group.
    //
    // `end - n >= rv + 2` is `end - n - 2 >= rv`: the position of the `[ая]`
    // must lie inside RV. Both sides are positions in `t`, so the test reads
    // the same whatever the buffer is indexed in.
    let n = tb.shi.longest_where(t, end, rv, |n| {
        end - n >= rv + 2 && t[end - n - 1] == 'в' && matches!(t[end - n - 2], 'а' | 'я')
    });
    if n > 0 {
        return Some(end - n - 2);
    }
    let m = tb.gerund2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

/// `participle`: the `[ая]`-conditioned list keeps the captured letter.
fn participle_end(t: &[char], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let n = tb.part1.longest_where(t, end, rv, |n| {
        end - n > rv && matches!(t[end - n - 1], 'а' | 'я')
    });
    if n > 0 {
        return Some(end - n);
    }
    let m = tb.part2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

/// `adjectival`: adjective, then participle with the falsy fallback.
fn adjectival_end(t: &[char], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let a = tb.adjective.longest(t, end, rv);
    if a == 0 {
        return None;
    }
    let e1 = end - a;
    // `result = pariticipleResult || result` — falsy fallback again.
    Some(match participle_end(t, rv, e1, tb) {
        Some(pe) if pe > rv => pe,
        _ => e1,
    })
}

/// `verb`: same two-list shape as `participle`.
fn verb_end(t: &[char], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let n = tb.verb1.longest_where(t, end, rv, |n| {
        end - n > rv && matches!(t[end - n - 1], 'а' | 'я')
    });
    if n > 0 {
        return Some(end - n);
    }
    let m = tb.verb2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

fn noun_end(t: &[char], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let m = tb.noun.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

/// `(ши|шись)` — the alternation inside `/[ая]в(ши|шись)$/`.
pub(crate) static SHI: &[&str] = &["ши", "шись"];
/// The unconditional perfective-gerund alternatives.
static GERUND2: &[&str] = &["ив", "ивши", "ившись", "ывши", "ывшись", "ыв"];
static REFLEXIVE: &[&str] = &["ся", "сь"];
/// The `[ая]`-conditioned participle alternatives.
static PART1: &[&str] = &["ем", "нн", "вш", "ющ", "щ"];
static PART2: &[&str] = &["ивш", "ывш", "ующ"];
static SUPERLATIVE: &[&str] = &["ейш", "ейше"];
static DERIVATIONAL: &[&str] = &["ост", "ость"];
static ADJECTIVE: &[&str] = &[
    "ее", "ие", "ые", "ое", "ими", "ыми", "ей", "ий", "ый", "ой", "ем", "им", "ым", "ом", "его",
    "ого", "ему", "ому", "их", "ых", "ую", "юю", "ая", "яя", "ою", "ею",
];
static VERB1: &[&str] = &[
    "ла", "на", "ете", "йте", "ли", "й", "л", "ем", "н", "ло", "но", "ет", "ют", "ны", "ть", "ешь",
    "нно",
];
static VERB2: &[&str] = &[
    "ила", "ыла", "ена", "ейте", "уйте", "ите", "или", "ыли", "ей", "уй", "ил", "ыл", "им", "ым",
    "ен", "ило", "ыло", "ено", "ят", "ует", "ит", "ыт", "ены", "ить", "ыть", "ишь", "ую", "ю",
];
static NOUN: &[&str] = &[
    "а", "ев", "ов", "ие", "ье", "е", "иями", "ями", "ами", "еи", "ии", "и", "ией", "ей", "ой",
    "ий", "й", "иям", "ям", "ием", "ем", "ам", "ом", "о", "у", "ах", "иях", "ях", "ы", "ь", "ию",
    "ью", "ю", "ия", "ья", "я",
];

impl PorterStemmerRu {
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
        let tb = &*TABLES;
        // Lowercased, with every `ё` folded to `е` — every occurrence, not
        // just the first.
        // The lowered characters go straight into a stack buffer: Cyrillic
        // lowercasing through `String` was measured at 82 µs per 1024 bench
        // words, over a third of this stemmer's total.
        let mut b: Buf<char> = Buf::fill_lowercase(token);
        for c in b.as_mut_slice() {
            if *c == 'ё' {
                *c = 'е';
            }
        }
        let t = b.as_slice();

        let Some((_, rv)) = split_at_first_vowel(t, is_vowel) else {
            return Cow::Owned(b.into_text());
        };
        let full = t.len();
        // R2 is the same split applied to the original RV; only its tail is
        // ever read, and only through the guard below.
        let r2 = split_at_first_vowel(&t[rv..], is_vowel).map(|(_, after)| rv + after);

        let mut end = match perfective_gerund_end(t, rv, full, tb) {
            Some(e) => e,
            None => {
                // `reflexive(RV) || RV` — the falsy fallback.
                let refl = tb.reflexive.longest(t, full, rv);
                let reflexed = if refl > 0 && full - refl > rv {
                    full - refl
                } else {
                    full
                };
                adjectival_end(t, rv, reflexed, tb)
                    .or_else(|| verb_end(t, rv, reflexed, tb))
                    .or_else(|| noun_end(t, rv, reflexed, tb))
                    .unwrap_or(reflexed)
            }
        };
        // /и$/ on the working string (the RV region).
        if end > rv && t[end - 1] == 'и' {
            end -= 1;
        }

        // The guard reads R2's tail *of the original word*, not the shrunken
        // working string — the regions were marked once. When the guard passes
        // but this second search misses, the string is kept as it stands (see
        // the module docs).
        if r2.is_some_and(|r2s| full > r2s && tb.derivational.longest(t, full, r2s) > 0) {
            let d = tb.derivational.longest(t, end, rv);
            if d > 0 {
                end -= d;
            }
        }

        // `superlative(x) || x` — falsy fallback.
        let sup = tb.superlative.longest(t, end, rv);
        if sup > 0 && end - sup > rv {
            end -= sup;
        }

        // `/(н)н/g → '$1'` over the RV region, compacted in place.
        {
            let t = b.as_mut_slice();
            let mut write = rv;
            let mut read = rv;
            while read < end {
                if t[read] == 'н' && read + 1 < end && t[read + 1] == 'н' {
                    t[write] = 'н';
                    write += 1;
                    read += 2;
                } else {
                    t[write] = t[read];
                    write += 1;
                    read += 1;
                }
            }
            end = write;
        }
        // /ь$/.
        if end > rv && b.as_slice()[end - 1] == 'ь' {
            end -= 1;
        }

        b.truncate(end);
        Cow::Owned(b.into_text())
    }
}

impl TokenizeAndStem for PorterStemmerRu {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::Ru.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.chars().any(is_russian_letter)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    use crate::among::Buf;

    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("SHI", super::SHI),
        ("GERUND2", super::GERUND2),
        ("REFLEXIVE", super::REFLEXIVE),
        ("ADJECTIVE", super::ADJECTIVE),
        ("PART1", super::PART1),
        ("PART2", super::PART2),
        ("VERB1", super::VERB1),
        ("VERB2", super::VERB2),
        ("NOUN", super::NOUN),
        ("SUPERLATIVE", super::SUPERLATIVE),
        ("DERIVATIONAL", super::DERIVATIONAL),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation:
    /// lowercase, then fold every `ё` to `е`.
    pub(crate) fn prelude(token: &str) -> String {
        let mut b: Buf<char> = Buf::fill_lowercase(token);
        for c in b.as_mut_slice() {
            if *c == 'ё' {
                *c = 'е';
            }
        }
        b.into_text()
    }

    /// The prelude writes no marker character: it only folds one letter onto
    /// another that is already in the alphabet.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
}

impl verbora_core::Stemmer for PorterStemmerRu {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerRu::new().stem(t).into_owned()
    }

    /// A working buffer, the way this stemmer builds one: one position per
    /// Unicode scalar value.
    fn scalars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("важнейшими", "важн"),
            ("важностию", "важност"),
            ("валандался", "валанда"),
            ("вагоном", "вагон"),
            ("ёлка", "елк"),
            ("ВАЖНАЯ", "важн"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn a_line_terminator_disables_the_whole_algorithm() {
        assert_eq!(s("аб\nв"), "аб\nв");
    }

    #[test]
    fn doubled_n_collapses_left_to_right() {
        assert_eq!(collapse_double(&scalars("нннн"), 'н'), scalars("нн"));
        // The word itself is vowel-free, so the split fails and it is returned
        // untouched — the collapse never runs.
        assert_eq!(s("нннн"), "нннн");
        assert_eq!(collapse_double(&scalars("ннн"), 'н'), scalars("нн"));
        assert_eq!(collapse_double(&scalars("н"), 'н'), scalars("н"));
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
    // Differential oracle: the same rules written as linear alternation scans.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::text;

        /// `/([ая])(ла|на|…)$/`: the index just after the kept `[ая]`.
        fn alt_suffix_after(w: &[char], keep: &[char], alts: &[&str]) -> Option<usize> {
            let mut best: Option<usize> = None;
            for a in alts {
                let n = slen(a);
                if n < w.len() && ends_with(w, a) && keep.contains(&w[w.len() - n - 1]) {
                    let start = w.len() - n;
                    if best.is_none_or(|b| start < b) {
                        best = Some(start);
                    }
                }
            }
            best
        }

        fn perfective_gerund(w: &[char]) -> Option<Vec<char>> {
            if let Some(at) = av_shi(w, SHI) {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, GERUND2).map(|at| w[..at].to_vec())
        }

        fn adjective(w: &[char]) -> Option<Vec<char>> {
            alt_suffix(w, ADJECTIVE).map(|at| w[..at].to_vec())
        }

        fn participle(w: &[char]) -> Option<Vec<char>> {
            if let Some(at) = alt_suffix_after(w, &['а', 'я'], PART1) {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, PART2).map(|at| w[..at].to_vec())
        }

        fn adjectival(w: &[char]) -> Option<Vec<char>> {
            let result = adjective(w)?;
            Some(or_falsy(participle(&result), &result))
        }

        fn reflexive(w: &[char]) -> Option<Vec<char>> {
            alt_suffix(w, REFLEXIVE).map(|at| w[..at].to_vec())
        }

        fn verb(w: &[char]) -> Option<Vec<char>> {
            if let Some(at) = alt_suffix_after(w, &['а', 'я'], VERB1) {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, VERB2).map(|at| w[..at].to_vec())
        }

        fn noun(w: &[char]) -> Option<Vec<char>> {
            alt_suffix(w, NOUN).map(|at| w[..at].to_vec())
        }

        fn superlative(w: &[char]) -> Option<Vec<char>> {
            alt_suffix(w, SUPERLATIVE).map(|at| w[..at].to_vec())
        }

        fn derivational(w: &[char]) -> Option<Vec<char>> {
            alt_suffix(w, DERIVATIONAL).map(|at| w[..at].to_vec())
        }

        pub(super) fn stem(token: &str) -> String {
            let mut t: Vec<char> = token.to_lowercase().chars().collect();
            for c in &mut t {
                if *c == 'ё' {
                    *c = 'е';
                }
            }

            let Some((head_end, rv_start)) = split_at_first_vowel(&t, is_vowel) else {
                return text(&t);
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

            let derived = if r2_tail
                .is_some_and(|tail| !tail.is_empty() && alt_suffix(tail, DERIVATIONAL).is_some())
            {
                derivational(&result).unwrap_or(result)
            } else {
                result
            };

            let mut out = or_falsy(superlative(&derived), &derived);
            out = collapse_double(&out, 'н'); // /(н)н/g
            strip_final(&mut out, 'ь'); // /ь$/

            let mut full = head.to_vec();
            full.extend_from_slice(&out);
            text(&full)
        }
    }

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

    /// Cyrillic stems crossed with real table suffixes (stacked up to two
    /// deep), plus ё, uppercase, line-terminator, astral and digit noise —
    /// every special path the module documents.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
            'т', 'у', 'ч', 'ш', 'щ', 'ы', 'ь', 'э', 'ю', 'я', 'ё',
        ];
        const SUFFIXES: &[&str] = &[
            "авшись",
            "явши",
            "ывшись",
            "ившись",
            "ив",
            "ыв",
            "ими",
            "ыми",
            "его",
            "ому",
            "ая",
            "яя",
            "ся",
            "сь",
            "ла",
            "на",
            "ете",
            "ешь",
            "нно",
            "ила",
            "ейте",
            "уйте",
            "ишь",
            "ует",
            "иями",
            "ями",
            "ией",
            "иях",
            "ью",
            "ья",
            "ость",
            "ост",
            "ейше",
            "ейш",
            "нн",
            "н",
            "и",
            "ь",
            "авши",
            "яв",
            "ав",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(8) {
            s.push(ALPHA[rng.below(ALPHA.len())]);
        }
        if rng.below(10) < 7 {
            s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            if rng.below(4) == 0 {
                s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            }
        }
        match rng.below(40) {
            0 => s = s.to_uppercase(),
            1 => s.push('😀'),
            2 => s.insert(0, '日'),
            3 => s.push_str("123"),
            4 => s.push('\n'),
            5 => s.insert(0, '\u{2028}'),
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_linear_scan_oracle() {
        let stemmer = PorterStemmerRu::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("ru") {
            check(&w);
        }
        for w in [
            "",
            "ся",
            "сь",
            "нннн",
            "бвг",
            "аб\nв",
            "важнейшими",
            "важностию",
            "валандался",
            "ёёёё",
            "ость",
            "остью",
            "радость",
            "ЁЛКА",
        ] {
            check(w);
        }
        let mut rng = Rng(0xA5A5_1234_5678_9ABC);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }

    // -----------------------------------------------------------------------
    // The text unit
    // -----------------------------------------------------------------------

    /// A character outside the Basic Multilingual Plane, and a character
    /// inside it that is its exact equal for every question this module asks.
    ///
    /// `U+1D7CE` (MATHEMATICAL BOLD DIGIT ZERO) and `U+4E2D` are both outside
    /// [`is_vowel`], outside [`is_line_terminator`], neither `ё` nor `е` nor
    /// `н` nor `ь` nor `и`, absent from every rule table (the highest code
    /// point in any of them is `U+044F`), fixed points of `str::to_lowercase`,
    /// and rejected by [`is_russian_letter`]. Under the crate's unit each is
    /// exactly **one** position of the working buffer.
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

    /// An astral character is one position, so replacing it with an inert
    /// Basic Multilingual Plane character cannot change a stem.
    ///
    /// # Russian is the one language here the unit provably cannot move
    ///
    /// Unlike Ukrainian's `derivational`, every comparison this module makes
    /// against a constant compares **two positions in the same buffer** —
    /// `end - n >= rv + 2` is `end - n - 2 >= rv`, `end > rv` and `full > r2s`
    /// are the same shape — and there is no absolute length gate anywhere in
    /// the algorithm. Re-indexing the buffer moves both sides of each of those
    /// by the same amount. This test therefore passed before the conversion as
    /// well as after, and it is here as the *certification* of that argument
    /// rather than as its red-to-green gate: the gate for this group is
    /// `crate::uk`'s, where the same change does move answers.
    #[test]
    fn an_astral_character_cannot_move_a_russian_answer() {
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

    /// One character in, one character out: no stem may contain a character
    /// the caller did not supply.
    ///
    /// A `char` working buffer cannot hold half a character, so no cut this
    /// module makes — a table match, the `[ая]в(ши|шись)` lookbehind, the `нн`
    /// compaction, or a region bound — can produce one.
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

    /// The size of [`astral_corpus`], derived from its own seeds rather than
    /// recorded: a seed of `n` characters has `n + 1` insertion points.
    fn placements() -> usize {
        astral_seeds().iter().map(|s| s.chars().count() + 1).sum()
    }

    /// What the enumerations walk: **every** Russian stop word, **every** rule
    /// table entry, the bench corpus, and a seeded corpus of Russian shapes.
    ///
    /// The composition is arithmetic, not a sample of convenience: 137 shipped
    /// stop words, the 129 entries of the eleven rule tables (2, 6, 2, 26, 5,
    /// 3, 17, 28, 36, 2 and 2), the 12 bench words, and 40,000 seeded words.
    /// The seeds stay on the Basic Multilingual Plane so that the one astral
    /// character inserted below is the only one in play.
    fn astral_seeds() -> Vec<String> {
        const ALPHA: &[char] = &[
            'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
            'т', 'у', 'ч', 'ш', 'щ', 'ы', 'ь', 'э', 'ю', 'я', 'ё',
        ];
        const SUFFIXES: &[&str] = &[
            "авшись",
            "явши",
            "ывшись",
            "ившись",
            "ив",
            "ыв",
            "ими",
            "ыми",
            "его",
            "ому",
            "ая",
            "яя",
            "ся",
            "сь",
            "ла",
            "на",
            "ете",
            "ешь",
            "нно",
            "ила",
            "ейте",
            "уйте",
            "ишь",
            "ует",
            "иями",
            "ями",
            "ией",
            "иях",
            "ью",
            "ья",
            "ость",
            "ост",
            "ейше",
            "ейш",
            "нн",
            "н",
            "и",
            "ь",
        ];

        let mut seeds: Vec<String> = Language::Ru
            .defaults()
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        for (_, table) in audit::TABLES {
            seeds.extend(table.iter().map(|e| (*e).to_owned()));
        }
        seeds.extend(crate::test_support::bench_words("ru"));
        let mut rng = Rng(0x0BAD_C0DE_1234_5678);
        for _ in 0..40_000 {
            let mut w = String::new();
            for _ in 0..1 + rng.below(7) {
                w.push(ALPHA[rng.below(ALPHA.len())]);
            }
            if rng.below(10) < 8 {
                w.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            }
            // The paths the module documents: an uppercase word, a line
            // terminator that disables the algorithm outright, and digits.
            match rng.below(24) {
                0 => w = w.to_uppercase(),
                1 => w.push('\n'),
                2 => w.insert(0, '\u{2028}'),
                3 => w.push_str("123"),
                _ => {}
            }
            seeds.push(w);
        }
        assert_eq!(seeds.len(), 137 + 129 + 12 + 40_000);
        seeds
    }

    /// [`astral_seeds`] with [`ASTRAL`] inserted at every position of every
    /// seed. Nothing here is sampled.
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
    /// the old way. Russian's pipeline is `prepare` (the identity here), then
    /// UAX #29 word segmentation, then a lookup on the **raw** token
    /// (`FILTER_ON = Casing::Raw`) — the `ё` fold and the lowercase both
    /// happen after the list has already been consulted, so no transform can
    /// get in front of it.
    ///
    /// # Four entries that cannot survive
    ///
    /// `может быть`, `все еще`, `с кем` and `хотел бы` are *phrases*. A space
    /// is a word boundary under UAX #29 with no tailoring available to change
    /// it, so the tokenizer hands `is_stop_word` two tokens and neither is on
    /// the list. Those four are unreachable through
    /// [`TokenizeAndStem::tokenize_and_stem`] and always were — a property of
    /// the shipped list, not of the text unit. They are pinned here as
    /// *exactly* the space-bearing entries, so a fifth one appearing is a test
    /// failure rather than a silent loss.
    #[test]
    fn every_single_token_stop_word_survives_the_pipeline() {
        let st = PorterStemmerRu::new();
        let words = Language::Ru.defaults();
        assert_eq!(words.len(), 137);
        let unfiltered: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| !st.tokenize_and_stem(w, false).is_empty())
            .collect();
        let phrases: Vec<&str> = words.iter().copied().filter(|w| w.contains(' ')).collect();
        assert_eq!(unfiltered, phrases);
        assert_eq!(phrases.len(), 4);
    }

    /// Every rule table entry measures the same as text and as buffer, and a
    /// cut by its own length lands where the entry starts.
    ///
    /// The tables are `&'static str` and are never re-encoded, so the unit
    /// they are *measured* in is the only thing the migration could have
    /// moved. All 129 entries are Basic Multilingual Plane text, which is
    /// asserted here rather than assumed: that premise is what lets a buffer
    /// length have a table entry's length subtracted from it at all.
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
                let probe: Vec<char> = format!("во{entry}").chars().collect();
                let n = slen(entry);
                assert_eq!(n, entry.chars().count(), "{name} {entry:?}");
                assert!(
                    ends_with(&probe, entry),
                    "{name} {entry:?} is not found at the end of its own probe"
                );
                assert_eq!(
                    crate::units::text(&probe[..probe.len() - n]),
                    "во",
                    "{name} {entry:?} cuts in the wrong place"
                );
            }
        }
        assert_eq!(entries, 129);
    }
}
