//! The Swedish stemmer, ported from
//! The reference `porter_stemmer_sv`.
//!
//! # Rebuilt from `rest`, not truncated
//!
//! Steps 1a and 3 do not cut the token; they return `regions.rest +
//! r1.slice(0, match.index)`. `rest` is `str.slice(0, str.length - r1.length)`,
//! which is only the same thing as "the token minus R1" when R1 really is a
//! suffix — and it need not be, because the capture class `[a-zåäö]` excludes
//! digits, `-`, `ü` and every uppercase letter, so R1 stops early on
//! `"björk-1"`. The reference's arithmetic is reproduced literally rather than
//! simplified into a truncation: `stem` takes the truncation fast path exactly
//! when `rest` ends where R1 starts (the common case), and materialises the
//! two-slice paste otherwise.
//!
//! # `getRegions` has a comment admitting it is unexplained
//!
//! `if (match.index + 2 < 3) r1 = str.slice(3)` carries the note *"Not clear why
//! we need this! Algorithm does not describe this part!"*. It fires exactly when
//! the match starts at index 0, and it is kept.
//!
//! # Step 1 keeps the shorter of 1a and 1b
//!
//! Like Norwegian, and with the same strict `<`: on a tie step 1b wins. Unlike
//! Norwegian, both branches share one `getRegions` call, so the regions are those
//! of the *input*; steps 2 and 3 recompute them from their own argument through
//! The reference's default-parameter evaluation.
//!
//! # Longest suffix, via `find_among`
//!
//! Each alternation takes the longest listed suffix of R1 (the earliest match
//! start at which some alternative reaches `$`). `stem` computes it with one
//! [`crate::among`] binary search per step instead of a linear scan per
//! alternative (`docs/PERFORMANCE_GAPS.md` entry 34), passing R1 as
//! `(lb, cursor)` limits so no slice is snapshotted. The pre-conversion
//! implementation is kept verbatim in this module's tests as the
//! byte-exactness oracle.

use std::borrow::Cow;
use std::sync::LazyLock;

use verbora_normalizers::normalize_sv;
use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};
use crate::units::{text, units};

/// The Swedish stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerSv;
/// let s = PorterStemmerSv::new();
/// assert_eq!(s.stem("björks"), "björk");
/// assert_eq!(s.stem("jaktbössa"), "jaktböss");
/// assert_eq!(s.stem("BJÖRKS"), "björk");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerSv;

/// `[aeiouyäåö]` — lowercase only.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0x79 | 0xE4 | 0xE5 | 0xF6
    )
}

/// `[a-zåäö]`, the class R1's captured run is drawn from.
#[inline]
fn is_r1_char(c: u16) -> bool {
    matches!(c, 0x61..=0x7A | 0xE5 | 0xE4 | 0xF6)
}

/// `getRegions`, as indices into `t`: R1 is `t[r1s..r1e]` (empty when the
/// pattern does not match), and `rest_len` is the reference's
/// `str.length - r1.length` — a *length*, not R1's start, which is the whole
/// point of the "rebuilt from `rest`" note in the module docs.
struct RegionIx {
    r1s: usize,
    r1e: usize,
    rest_len: usize,
}

fn region_ix(t: &[u16]) -> RegionIx {
    let (mut r1s, mut r1e) = (0usize, 0usize);
    if let Some(index) = (0..t.len().saturating_sub(2))
        .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))
    {
        r1s = index + 2;
        r1e = (index + 2..t.len())
            .find(|&i| !is_r1_char(t[i]))
            .unwrap_or(t.len());
        if index == 0 {
            // The unexplained special case: `r1 = str.slice(3)`.
            r1s = 3;
            r1e = t.len();
        }
    }
    RegionIx {
        rest_len: t.len() - (r1e - r1s),
        r1s,
        r1e,
    }
}

/// The sorted search tables, built once from the alternations below.
struct SvTables {
    step1a: AmongTable,
    /// `(dd|gd|nn|dt|gt|kt|tt)` — step 2's alternation.
    step2: AmongTable,
    /// `(lös|full)t` — checked before the removable step-3 suffixes.
    lost_fullt: AmongTable,
    /// `(lig|ig|els)` — step 3's removable suffixes.
    lig_ig_els: AmongTable,
}

static TABLES: LazyLock<SvTables> = LazyLock::new(|| SvTables {
    step1a: AmongTable::build(STEP1A),
    step2: AmongTable::build(&["dd", "gd", "nn", "dt", "gt", "kt", "tt"]),
    lost_fullt: AmongTable::build(&["löst", "fullt"]),
    lig_ig_els: AmongTable::build(&["lig", "ig", "els"]),
});

/// `regions.rest + r1.slice(0, idx)`: a plain truncation when `rest` ends
/// exactly where R1 starts, the literal two-slice paste otherwise.
fn rebuild(t: &mut Vec<u16>, r: &RegionIx, idx: usize) {
    if r.rest_len == r.r1s {
        t.truncate(r.r1s + idx);
    } else {
        let mut out = t[..r.rest_len.min(t.len())].to_vec();
        out.extend_from_slice(&t[r.r1s..r.r1s + idx]);
        *t = out;
    }
}

/// The step-1a alternation, in source order.
static STEP1A: &[&str] = &[
    "heterna", "hetens", "anden", "andes", "andet", "arens", "arnas", "ernas", "heten", "heter",
    "ornas", "ande", "ades", "aren", "arna", "arne", "aste", "erna", "erns", "orna", "ade", "are",
    "ast", "ens", "ern", "het", "ad", "ar", "as", "at", "en", "er", "es", "or", "a", "e",
];

impl PorterStemmerSv {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token: lowercase, then step 1, step 2, step 3.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let lower = token.to_lowercase();
        let mut t = units(&lower);

        // --- Step 1: 1a and 1b share ONE regions call; the shorter result
        // wins and a tie goes to 1b (strict `<`, see the module docs).
        let r = region_ix(&t);
        let len = t.len();
        let mut idx_a: Option<usize> = None;
        if r.r1e > r.r1s {
            let n = tb.step1a.longest(&t, r.r1e, r.r1s);
            if n > 0 {
                idx_a = Some((r.r1e - r.r1s) - n);
            }
        }
        let len_a = idx_a.map_or(len, |idx| r.rest_len + idx);
        // 1b: `/(b|c|d|f|g|h|j|k|l|m|n|o|p|r|t|v|y)s$/` — `k` is in this list
        // where the Norwegian one omits it, and the vowel `o` is included.
        let mut len_b = len;
        if r.r1e > r.r1s
            && len >= 2
            && t[len - 1] == 0x73
            && matches!(
                t[len - 2],
                0x62 | 0x63
                    | 0x64
                    | 0x66
                    | 0x67
                    | 0x68
                    | 0x6A
                    | 0x6B
                    | 0x6C
                    | 0x6D
                    | 0x6E
                    | 0x6F
                    | 0x70
                    | 0x72
                    | 0x74
                    | 0x76
                    | 0x79
            )
        {
            len_b = len - 1;
        }
        if len_a < len_b {
            let idx = idx_a.unwrap_or(0);
            rebuild(&mut t, &r, idx);
        } else {
            t.truncate(len_b);
        }

        // --- Step 2: drop a final unit when R1 ends in a listed pair -------
        let r = region_ix(&t);
        if r.r1e > r.r1s && tb.step2.longest(&t, r.r1e, r.r1s) > 0 {
            let keep = t.len().saturating_sub(1);
            t.truncate(keep);
        }

        // --- Step 3 --------------------------------------------------------
        let r = region_ix(&t);
        if r.r1e > r.r1s {
            // `/(lös|full)t$/` — the trailing `t` is outside the group.
            if tb.lost_fullt.longest(&t, r.r1e, r.r1s) > 0 {
                let keep = t.len().saturating_sub(1);
                t.truncate(keep);
            } else {
                let n = tb.lig_ig_els.longest(&t, r.r1e, r.r1s);
                if n > 0 {
                    let idx = (r.r1e - r.r1s) - n;
                    rebuild(&mut t, &r, idx);
                }
            }
        }

        Cow::Owned(text(&t))
    }

    /// Appends a stop word to the **process-global Swedish list**.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        stopwords::add(Language::Sv, word);
    }

    /// Appends several stop words to the process-global Swedish list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        stopwords::add_all(Language::Sv, words);
    }
}

impl TokenizeAndStem for PorterStemmerSv {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_sv(c)
    }

    /// `AggressiveTokenizerSv` folds `à á è é` — and only their first occurrence
    /// each — before splitting.
    fn prepare(text: &str) -> Cow<'_, str> {
        normalize_sv(text)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Sv, word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerSv {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerSv::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("björks", "björk"),
            ("BJÖRKS", "björk"),
            ("jaktbössa", "jaktböss"),
            ("klockorna", "klock"),
            ("flickornas", "flick"),
            ("stiftelsen", "stift"),
            ("frihetens", "frihet"),
            ("härligt", "här"),
            ("körsbärsträdgårdarna", "körsbärsträdgård"),
            // R1 of "fullt" is "llt", which does not end in "fullt", so the
            // `(lös|full)t` rule cannot fire on the word it was written for.
            ("fullt", "fullt"),
            ("löst", "löst"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
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
            ("мама", "мама"),
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

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{ends_with, slen};

        struct Regions<'t> {
            r1: &'t [u16],
            rest_len: usize,
        }

        fn regions(t: &[u16]) -> Regions<'_> {
            let mut r1: &[u16] = &[];
            if let Some(index) = (0..t.len().saturating_sub(2))
                .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))
            {
                let end = (index + 2..t.len())
                    .find(|&i| !is_r1_char(t[i]))
                    .unwrap_or(t.len());
                r1 = &t[index + 2..end];
                if index == 0 {
                    r1 = t.get(3..).unwrap_or_default();
                }
            }
            Regions {
                rest_len: t.len().saturating_sub(r1.len()),
                r1,
            }
        }

        fn listed_suffix(w: &[u16], alternatives: &[&str]) -> Option<usize> {
            let mut best: Option<usize> = None;
            for a in alternatives {
                if ends_with(w, a) {
                    let start = w.len() - slen(a);
                    if best.is_none_or(|b| start < b) {
                        best = Some(start);
                    }
                }
            }
            best
        }

        fn step1a(t: &[u16], r: &Regions<'_>) -> Vec<u16> {
            if r.r1.is_empty() {
                return t.to_vec();
            }
            match listed_suffix(r.r1, STEP1A) {
                Some(idx) => {
                    let mut out = t[..r.rest_len.min(t.len())].to_vec();
                    out.extend_from_slice(&r.r1[..idx]);
                    out
                }
                None => t.to_vec(),
            }
        }

        fn step1b(t: &[u16], r: &Regions<'_>) -> Vec<u16> {
            if !r.r1.is_empty()
                && t.len() >= 2
                && t[t.len() - 1] == 0x73
                && matches!(
                    t[t.len() - 2],
                    0x62 | 0x63
                        | 0x64
                        | 0x66
                        | 0x67
                        | 0x68
                        | 0x6A
                        | 0x6B
                        | 0x6C
                        | 0x6D
                        | 0x6E
                        | 0x6F
                        | 0x70
                        | 0x72
                        | 0x74
                        | 0x76
                        | 0x79
                )
            {
                return t[..t.len() - 1].to_vec();
            }
            t.to_vec()
        }

        fn step1(t: &[u16]) -> Vec<u16> {
            let r = regions(t);
            let a = step1a(t, &r);
            let b = step1b(t, &r);
            if a.len() < b.len() { a } else { b }
        }

        fn step2(t: &[u16]) -> Vec<u16> {
            let r = regions(t);
            if !r.r1.is_empty()
                && listed_suffix(r.r1, &["dd", "gd", "nn", "dt", "gt", "kt", "tt"]).is_some()
            {
                return t[..t.len().saturating_sub(1)].to_vec();
            }
            t.to_vec()
        }

        fn step3(t: &[u16]) -> Vec<u16> {
            let r = regions(t);
            if r.r1.is_empty() {
                return t.to_vec();
            }
            if listed_suffix(r.r1, &["löst", "fullt"]).is_some() {
                return t[..t.len().saturating_sub(1)].to_vec();
            }
            match listed_suffix(r.r1, &["lig", "ig", "els"]) {
                Some(idx) => {
                    let mut out = t[..r.rest_len.min(t.len())].to_vec();
                    out.extend_from_slice(&r.r1[..idx]);
                    out
                }
                None => t.to_vec(),
            }
        }

        pub(super) fn stem(token: &str) -> String {
            let lower = token.to_lowercase();
            text(&step3(&step2(&step1(&units(&lower)))))
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

    /// Swedish stems crossed with real table suffixes (stacked up to two
    /// deep) plus the characters that stop R1 early (`-`, `ü`, digits) —
    /// the paste-not-truncate path — and case/astral/CJK noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'r', 's',
            't', 'u', 'v', 'y', 'å', 'ä', 'ö', 'ü',
        ];
        const SUFFIXES: &[&str] = &[
            "heterna", "hetens", "anden", "andes", "arens", "ornas", "ande", "ades", "aren",
            "arna", "aste", "erna", "orna", "ade", "are", "ast", "ens", "ern", "het", "ad", "ar",
            "as", "at", "en", "er", "es", "or", "a", "e", "dd", "gd", "nn", "dt", "gt", "kt", "tt",
            "löst", "fullt", "lig", "ig", "els", "ks", "s",
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
        match rng.below(30) {
            0 => s = s.to_uppercase(),
            1 => s.push('😀'),
            2 => s.insert(0, '日'),
            3 => s.push_str("123"),
            4 => {
                let at = rng.below(s.len().max(1)).min(s.len());
                if s.is_char_boundary(at) {
                    s.insert(at, '-');
                }
            }
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_linear_scan_oracle() {
        let stemmer = PorterStemmerSv::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("sv") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "björks",
            "björk-1",
            "fullt",
            "löst",
            "härligt",
            "stiftelsen",
            "körsbärsträdgårdarna",
            "abc",
            "ett",
            "ös",
        ] {
            check(w);
        }
        let mut rng = Rng(0x5EED_CAFE_F00D_D00D);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
