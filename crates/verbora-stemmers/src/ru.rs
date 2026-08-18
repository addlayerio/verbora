//! The Russian stemmer, ported from
//! The reference `porter_stemmer_ru`.
//!
//! # Regex shapes, and why they are hand-written
//!
//! Every rule is `pattern.test(token)` followed by `token.replace(pattern, repl)`
//! on an anchored alternation. Three shapes appear, and each reduces to a scan:
//!
//! | the reference | What it computes |
//! |---|---|
//! | `/(a\|bb\|ccc)$/ → ''` | delete the **longest** listed suffix |
//! | `/([ая])(ла\|на\|…)$/ → '$1'` | delete the longest listed suffix that is preceded by `а` or `я`, keeping that letter |
//! | `/(н)н/g → '$1'` | collapse every `нн` to `н`, left to right, non-overlapping |
//!
//! The first shape is *longest*, not first-listed, even though the alternation is
//! ordered: the engine takes the earliest start position at which some
//! alternative reaches `$`, and since every alternative is a distinct literal
//! anchored at the end, the earliest start is the longest suffix. Porting the
//! order as if it were first-match — the Italian convention — would give
//! `"важностию"` a different stem.
//!
//! `stem` computes the first two shapes with one [`crate::among`] binary
//! search per rule group instead of a linear scan per alternative
//! (`docs/PERFORMANCE_GAPS.md` entry 34): the longest listed suffix is what
//! `find_among_b` returns natively, and the preceded-by-`[ая]` shape is the
//! link-walk with a condition ([`crate::among::AmongTable::longest_where`]).
//! Every rule is a tail truncation of RV, so the whole pipeline runs as one
//! shrinking end cursor over a single buffer — the per-rule `to_vec()`
//! snapshots are gone. The pre-conversion implementation is kept in this
//! module's tests as the byte-exactness oracle.
//!
//! # `||` is falsy, not null-ish
//!
//! `reflexive(RV) || RV` and `superlative(x) || x` fall back when the rule
//! returns the **empty string** as well as when it returns `null`. Stemming
//! `"ся"` therefore returns `"ся"` rather than `""`, and a naive `??` port
//! changes it. Both sites are marked in the code.
//!
//! # `.` does not match a line terminator
//!
//! The region split is `/^(.*?[аеиоюяуыиэ])(.*)$/`. A token containing `\n`,
//! `\r`, U+2028 or U+2029 cannot match it at any offset, so the whole algorithm
//! is skipped and the lowercased, `ё`-folded token is returned unchanged.
//!
//! # One unreachable `TypeError`
//!
//! When R2's tail ends in `ост`/`ость` but the working string does not, the
//! reference assigns `null` to `derivationalResult` and then calls `.replace` on
//! it, which throws. No word in the reference's corpora, and none of 200,000
//! randomised Cyrillic probes, reaches it — the `ь` that `noun` always strips
//! from `ость` leaves `ост` behind, which matches. Rather than panic on input no
//! caller can construct, this port keeps the un-derivationalised string. See
//! `deviations` in the crate's parity report.

use std::borrow::Cow;
use std::sync::LazyLock;

use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_ru;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, slen, text, units};

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
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x0430 | 0x0435 | 0x0438 | 0x043E | 0x044E | 0x044F | 0x0443 | 0x044B | 0x044D
    )
}

// ---------------------------------------------------------------------------
// Shared scanners, also used by the Ukrainian stemmer
// ---------------------------------------------------------------------------

/// The reference's line terminators, the four code points `.` refuses to match.
#[inline]
pub(crate) fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// `/(a|bb|ccc)$/`: the start index of the longest listed suffix of `w`.
pub(crate) fn alt_suffix(w: &[u16], alts: &[&str]) -> Option<usize> {
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
pub(crate) fn av_shi(w: &[u16]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in ["ши", "шись"] {
        let n = slen(a);
        if n + 2 <= w.len()
            && ends_with(w, a)
            && w[w.len() - n - 1] == 0x0432 // в
            && matches!(w[w.len() - n - 2], 0x0430 | 0x044F)
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
pub(crate) fn collapse_double(w: &[u16], letter: u16) -> Vec<u16> {
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
pub(crate) fn split_at_first_vowel(w: &[u16], vowel: fn(u16) -> bool) -> Option<(usize, usize)> {
    if w.iter().copied().any(is_line_terminator) {
        // `.` cannot cross a line terminator, and neither `.*?` nor `.*` may skip
        // one, so no offset can produce a match.
        return None;
    }
    let i = w.iter().position(|&c| vowel(c))?;
    Some((i + 1, i + 1))
}

/// Removes the last code unit when it is `c`; `/c$/` with an empty replacement.
pub(crate) fn strip_final(w: &mut Vec<u16>, c: u16) {
    if w.last() == Some(&c) {
        w.pop();
    }
}

/// The reference's `x || fallback` for a rule result: the empty string is falsy.
pub(crate) fn or_falsy(value: Option<Vec<u16>>, fallback: &[u16]) -> Vec<u16> {
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
// `Option<usize>`/`or_falsy` shapes of the reference are preserved exactly:
// `None` is "the rule's regex did not match", `Some(rv)` is "it matched and
// left the empty string" — the two outcomes the falsy fallbacks distinguish.

/// The sorted search tables, built once from the rule tables below.
struct RuTables {
    /// `(ши|шись)` — the alternation inside `/[ая]в(ши|шись)$/`.
    shi: AmongTable,
    /// The unconditional perfective-gerund alternatives.
    gerund2: AmongTable,
    reflexive: AmongTable,
    adjective: AmongTable,
    /// The `[ая]`-conditioned participle alternatives.
    part1: AmongTable,
    part2: AmongTable,
    /// The `[ая]`-conditioned verb alternatives.
    verb1: AmongTable,
    verb2: AmongTable,
    noun: AmongTable,
    superlative: AmongTable,
    derivational: AmongTable,
}

static TABLES: LazyLock<RuTables> = LazyLock::new(|| RuTables {
    shi: AmongTable::build(&["ши", "шись"]),
    gerund2: AmongTable::build(&["ив", "ивши", "ившись", "ывши", "ывшись", "ыв"]),
    reflexive: AmongTable::build(&["ся", "сь"]),
    adjective: AmongTable::build(ADJECTIVE),
    part1: AmongTable::build(&["ем", "нн", "вш", "ющ", "щ"]),
    part2: AmongTable::build(&["ивш", "ывш", "ующ"]),
    verb1: AmongTable::build(VERB1),
    verb2: AmongTable::build(VERB2),
    noun: AmongTable::build(NOUN),
    superlative: AmongTable::build(&["ейш", "ейше"]),
    derivational: AmongTable::build(&["ост", "ость"]),
});

/// `perfectiveGerund` over `t[rv..end]`: the new end, or `None`.
fn perfective_gerund_end(t: &[u16], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    // `/[ая]в(ши|шись)$/` first, unconditionally preferred over the plain
    // alternatives when it matches — the reference tests it first.
    let n = tb.shi.longest_where(t, end, rv, |n| {
        end - n >= rv + 2 && t[end - n - 1] == 0x0432 && matches!(t[end - n - 2], 0x0430 | 0x044F)
    });
    if n > 0 {
        return Some(end - n - 2);
    }
    let m = tb.gerund2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

/// `participle`: the `[ая]`-conditioned list keeps the captured letter.
fn participle_end(t: &[u16], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let n = tb.part1.longest_where(t, end, rv, |n| {
        end - n > rv && matches!(t[end - n - 1], 0x0430 | 0x044F)
    });
    if n > 0 {
        return Some(end - n);
    }
    let m = tb.part2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

/// `adjectival`: adjective, then participle with the falsy fallback.
fn adjectival_end(t: &[u16], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
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
fn verb_end(t: &[u16], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let n = tb.verb1.longest_where(t, end, rv, |n| {
        end - n > rv && matches!(t[end - n - 1], 0x0430 | 0x044F)
    });
    if n > 0 {
        return Some(end - n);
    }
    let m = tb.verb2.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

fn noun_end(t: &[u16], rv: usize, end: usize, tb: &RuTables) -> Option<usize> {
    let m = tb.noun.longest(t, end, rv);
    if m > 0 { Some(end - m) } else { None }
}

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
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        // `.toLowerCase().replace(/ё/g, 'е')` — global, so every ё folds.
        let mut t = units(&token.to_lowercase());
        for c in &mut t {
            if *c == 0x0451 {
                *c = 0x0435;
            }
        }

        let Some((_, rv)) = split_at_first_vowel(&t, is_vowel) else {
            return Cow::Owned(text(&t));
        };
        let full = t.len();
        // R2 is the same split applied to the original RV; only its tail is
        // ever read, and only through the guard below.
        let r2 = split_at_first_vowel(&t[rv..], is_vowel).map(|(_, after)| rv + after);

        let mut end = match perfective_gerund_end(&t, rv, full, tb) {
            Some(e) => e,
            None => {
                // `reflexive(RV) || RV` — the falsy fallback.
                let refl = tb.reflexive.longest(&t, full, rv);
                let reflexed = if refl > 0 && full - refl > rv {
                    full - refl
                } else {
                    full
                };
                adjectival_end(&t, rv, reflexed, tb)
                    .or_else(|| verb_end(&t, rv, reflexed, tb))
                    .or_else(|| noun_end(&t, rv, reflexed, tb))
                    .unwrap_or(reflexed)
            }
        };
        // /и$/ on the working string (the RV region).
        if end > rv && t[end - 1] == 0x0438 {
            end -= 1;
        }

        // The guard reads R2's tail *of the original word*, not the shrunken
        // working string — the regions were marked once. The reference throws
        // when this second `derivational` misses; keeping the string as-is is
        // the documented divergence (see the module docs).
        if r2.is_some_and(|r2s| full > r2s && tb.derivational.longest(&t, full, r2s) > 0) {
            let d = tb.derivational.longest(&t, end, rv);
            if d > 0 {
                end -= d;
            }
        }

        // `superlative(x) || x` — falsy fallback.
        let sup = tb.superlative.longest(&t, end, rv);
        if sup > 0 && end - sup > rv {
            end -= sup;
        }

        // `/(н)н/g → '$1'` over the RV region, compacted in place.
        {
            let mut write = rv;
            let mut read = rv;
            while read < end {
                if t[read] == 0x043D && read + 1 < end && t[read + 1] == 0x043D {
                    t[write] = 0x043D;
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
        if end > rv && t[end - 1] == 0x044C {
            end -= 1;
        }

        t.truncate(end);
        Cow::Owned(text(&t))
    }
}

impl TokenizeAndStem for PorterStemmerRu {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_ru(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Ru, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_ru)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
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
        assert_eq!(collapse_double(&units("нннн"), 0x043D), units("нн"));
        // The word itself is vowel-free, so the split fails and it is returned
        // untouched — the collapse never runs.
        assert_eq!(s("нннн"), "нннн");
        assert_eq!(collapse_double(&units("ннн"), 0x043D), units("нн"));
        assert_eq!(collapse_double(&units("н"), 0x043D), units("н"));
    }

    /// The cross-cutting battery from `docs/PARITY.md`: empty, one character,
    /// uppercase, accented Latin, Greek, Cyrillic, CJK, an astral pair,
    /// punctuation, digits, a line terminator, and a very long word. Every
    /// expectation below was read off the reference with `node`.
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
    // Differential oracle: the pre-find_among implementation, verbatim.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;

        /// `/([ая])(ла|на|…)$/`: the index just after the kept `[ая]`.
        fn alt_suffix_after(w: &[u16], keep: &[u16], alts: &[&str]) -> Option<usize> {
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

        fn perfective_gerund(w: &[u16]) -> Option<Vec<u16>> {
            if let Some(at) = av_shi(w) {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, &["ив", "ивши", "ившись", "ывши", "ывшись", "ыв"])
                .map(|at| w[..at].to_vec())
        }

        fn adjective(w: &[u16]) -> Option<Vec<u16>> {
            alt_suffix(w, ADJECTIVE).map(|at| w[..at].to_vec())
        }

        fn participle(w: &[u16]) -> Option<Vec<u16>> {
            if let Some(at) = alt_suffix_after(w, &[0x0430, 0x044F], &["ем", "нн", "вш", "ющ", "щ"])
            {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, &["ивш", "ывш", "ующ"]).map(|at| w[..at].to_vec())
        }

        fn adjectival(w: &[u16]) -> Option<Vec<u16>> {
            let result = adjective(w)?;
            Some(or_falsy(participle(&result), &result))
        }

        fn reflexive(w: &[u16]) -> Option<Vec<u16>> {
            alt_suffix(w, &["ся", "сь"]).map(|at| w[..at].to_vec())
        }

        fn verb(w: &[u16]) -> Option<Vec<u16>> {
            if let Some(at) = alt_suffix_after(w, &[0x0430, 0x044F], VERB1) {
                return Some(w[..at].to_vec());
            }
            alt_suffix(w, VERB2).map(|at| w[..at].to_vec())
        }

        fn noun(w: &[u16]) -> Option<Vec<u16>> {
            alt_suffix(w, NOUN).map(|at| w[..at].to_vec())
        }

        fn superlative(w: &[u16]) -> Option<Vec<u16>> {
            alt_suffix(w, &["ейш", "ейше"]).map(|at| w[..at].to_vec())
        }

        fn derivational(w: &[u16]) -> Option<Vec<u16>> {
            alt_suffix(w, &["ост", "ость"]).map(|at| w[..at].to_vec())
        }

        pub(super) fn stem(token: &str) -> String {
            let mut t = units(&token.to_lowercase());
            for c in &mut t {
                if *c == 0x0451 {
                    *c = 0x0435;
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
            strip_final(&mut result, 0x0438); // /и$/

            let derived = if r2_tail.is_some_and(|tail| {
                !tail.is_empty() && alt_suffix(tail, &["ост", "ость"]).is_some()
            }) {
                derivational(&result).unwrap_or(result)
            } else {
                result
            };

            let mut out = or_falsy(superlative(&derived), &derived);
            out = collapse_double(&out, 0x043D); // /(н)н/g
            strip_final(&mut out, 0x044C); // /ь$/

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
}
