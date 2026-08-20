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
//! # The unit
//!
//! R1's two bounds, `rest`'s length, the unexplained `slice(3)` above and every
//! cut below count **Unicode scalar values** — the unit [`crate::units`] states
//! for the whole crate. That matters more here than anywhere else, because
//! Swedish's cuts do *not* come from a matched suffix length: [`rebuild`] cuts
//! at `rest_len`, an arithmetic value derived from the region scan that no rule
//! table bounds. Only the buffer's own unit makes such a cut a character
//! boundary, and with the scalar unit it is one by construction rather than by
//! audit.
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

use crate::among::{AmongTable, Buf, UnionTable};
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::Language;
use crate::units::borrowed_prefix;

/// The Swedish stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerSv;
/// let s = PorterStemmerSv::new();
/// assert_eq!(s.stem("björks"), "björk");
/// assert_eq!(s.stem("jaktbössa"), "jaktböss");
/// assert_eq!(s.stem("BJÖRKS"), "björk");
/// ```
///
/// # `å`, `ä` and `ö` are letters, so nothing is folded
///
/// [`TokenizeAndStem::prepare`] is the identity for Swedish: text reaches the
/// tokenizer, the stop-word list and [`Self::stem`] spelled exactly as it was
/// written. No diacritic fold, no case fold, no rewrite of any kind. Three
/// independent reasons, and each on its own is decisive:
///
/// 1. **They are distinct letters, not accents.** Swedish has twenty-nine
///    letters; `å`, `ä` and `ö` are the last three and collate *after* `z`,
///    not next to `a` and `o`. Folding them merges words that are not the
///    same word: `för` ("for") becomes `for` (past tense of *fara*, "to
///    travel"), and `hår` ("hair") becomes `har` ("has").
/// 2. **Every rule below is stated over the un-folded alphabet.** The vowel
///    class is `[aeiouyäåö]`, R1's character class is `[a-zåäö]`, and step 3's
///    non-removable check is `(lös|full)t`. Fold first and `löst` can never
///    match — the rule is written for a spelling the stemmer would no longer
///    receive.
/// 3. **It emptied a third of the stop-word list.** 116 of the 428 Swedish
///    stop words are spelled with `å`, `ä` or `ö` and fold to strings that are
///    not themselves on the list, so a document-level fold silently deleted
///    them: `på`, `för`, `från`, `över`, `här`, `där`, `när` and 109 more
///    stopped being stop words while `is_stop_word` still answered `true` for
///    them.
///
/// Foreign accents in loanwords (`é`, `ç`, `ü`) are not folded either. Nothing
/// in the algorithm or the list distinguishes them, so folding only those
/// would be a new rule of Verbora's own invention with no algorithmic
/// consequence — and the alphabet argument does not reach them either way.
/// A caller who wants an accent-insensitive index should fold with
/// `verbora_normalizers::remove_diacritics` *around* this stemmer, where the
/// choice is theirs and visible.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerSv;

/// `[aeiouyäåö]` — lowercase only.
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'ä' | 'å' | 'ö')
}

/// `[a-zåäö]`, the class R1's captured run is drawn from.
#[inline]
fn is_r1_char(c: char) -> bool {
    matches!(c, 'a'..='z' | 'å' | 'ä' | 'ö')
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

/// `getRegions`, freshly scanned. `stem` goes through [`RegionScan`]
/// instead; this remains as the definition the cached form is checked
/// against.
#[cfg(test)]
fn region_ix_uncached(t: &[char]) -> RegionIx {
    RegionScan::of(t).at_len(t.len())
}

/// A scan of `getRegions`' match, kept so the later steps can re-derive R1
/// after a truncation instead of rescanning the word.
///
/// # Why this is exact
///
/// The reference calls `getRegions(token)` afresh in every step, and the
/// steps below only ever *truncate* — except the paste arm of [`rebuild`],
/// which `stem` marks and rescans after. For a truncation the question is
/// what a rescan of a prefix would return. The match position is the first
/// `i` with `vowel(t[i]) && !vowel(t[i+1]) && r1char(t[i+2])`; truncating to
/// `len'` changes none of those three characters for any `i` with
/// `i + 2 < len'`, and removes exactly the positions with `i + 2 >= len'` from
/// consideration. So the same `i` is still the first match when
/// `i + 2 < len'`, and there is no match at all otherwise. The captured
/// run's end is the first non-R1 character at or after `i + 2`, or the
/// length; over a prefix that is the same index when it lies inside the
/// prefix and the prefix's own length when it does not — which is
/// `min(end, len')` in both cases, including the `index == 0` branch whose
/// end is the length by construction.
#[derive(Clone, Copy)]
struct RegionScan {
    index: Option<usize>,
    start: usize,
    end: usize,
}

impl RegionScan {
    fn of(t: &[char]) -> RegionScan {
        let Some(index) = (0..t.len().saturating_sub(2))
            .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))
        else {
            return RegionScan {
                index: None,
                start: 0,
                end: 0,
            };
        };
        if index == 0 {
            // The unexplained special case: `r1 = str.slice(3)`.
            return RegionScan {
                index: Some(0),
                start: 3,
                end: t.len(),
            };
        }
        let end = (index + 2..t.len())
            .find(|&i| !is_r1_char(t[i]))
            .unwrap_or(t.len());
        RegionScan {
            index: Some(index),
            start: index + 2,
            end,
        }
    }

    /// What `getRegions` would return for the same word truncated to `len`.
    #[inline]
    fn at_len(self, len: usize) -> RegionIx {
        let (r1s, r1e) = match self.index {
            Some(index) if index + 2 < len => (self.start, self.end.min(len)),
            _ => (0, 0),
        };
        RegionIx {
            rest_len: len - (r1e - r1s),
            r1s,
            r1e,
        }
    }
}

/// The sorted search tables, built once from the alternations below.
///
/// # Why step 2 is not among them
///
/// Its seven alternatives are all two units long, so the whole `find_among`
/// apparatus reduces to reading the last two units and switching on them —
/// see [`ends_consonant_pair`]. Step 3's two alternations *are* tables, but
/// merged into one: they interrogate the same region of the same word, so a
/// single search plus a link walk answers both (see
/// [`crate::among::UnionTable`]).
struct SvTables {
    step1a: AmongTable<char>,
    /// 0 = [`LOST_FULLT`] (`(lös|full)t`, checked first), 1 = [`LIG_IG_ELS`]
    /// (step 3's removable suffixes).
    step3: UnionTable<char>,
}

static TABLES: LazyLock<SvTables> = LazyLock::new(|| SvTables {
    step1a: AmongTable::build(STEP1A),
    step3: UnionTable::build(&[LOST_FULLT, LIG_IG_ELS]),
});

/// `(lös|full)t` — step 3's non-removable check.
static LOST_FULLT: &[&str] = &["löst", "fullt"];
/// `(lig|ig|els)` — step 3's removable suffixes.
static LIG_IG_ELS: &[&str] = &["lig", "ig", "els"];
/// `(dd|gd|nn|dt|gt|kt|tt)` — step 2's alternation, kept as the table the
/// hand-written [`ends_consonant_pair`] is checked against (and as the
/// oracle's input); `stem` itself never searches it.
#[cfg(test)]
static STEP2: &[&str] = &["dd", "gd", "nn", "dt", "gt", "kt", "tt"];

/// Whether `w[lb..cursor]` ends in one of `dd`, `gd`, `nn`, `dt`, `gt`,
/// `kt`, `tt` — step 2's alternation.
///
/// Every alternative is exactly two characters, so this is the same answer a
/// table search would give, reached by reading the two characters directly.
/// The arms are grouped by the *final* character because that is the one the
/// region guarantees is present once the length check passes.
#[inline]
fn ends_consonant_pair(w: &[char], cursor: usize, lb: usize) -> bool {
    if cursor < lb + 2 {
        return false;
    }
    let (first, last) = (w[cursor - 2], w[cursor - 1]);
    match last {
        'd' => matches!(first, 'd' | 'g'),             // dd, gd
        'n' => first == 'n',                           // nn
        't' => matches!(first, 'd' | 'g' | 'k' | 't'), // dt, gt, kt, tt
        _ => false,
    }
}

/// `regions.rest + r1.slice(0, idx)`: a plain truncation when `rest` ends
/// exactly where R1 starts, the literal two-slice paste otherwise.
///
/// The paste arm's result is never longer than the buffer already is
/// (`rest_len + idx <= len` by construction), so it moves R1's kept prefix
/// down over the same buffer instead of building a second one. The two
/// ranges can overlap in either direction — `rest` and R1 need not be
/// adjacent — which is exactly what `copy_within`'s memmove semantics
/// handle.
/// Returns whether the paste arm ran — the caller's cached region scan is
/// only invalidated by that arm, the other being a plain truncation.
///
/// Both `keep` and `idx` are counts of characters, so neither the move nor the
/// truncation can land inside one. That is the guarantee the scalar unit buys
/// outright: these positions come from region arithmetic, not from a table
/// entry's length, so no audit of the tables could ever have bounded them.
fn rebuild(buf: &mut Buf<char>, r: &RegionIx, idx: usize) -> bool {
    if r.rest_len == r.r1s {
        buf.truncate(r.r1s + idx);
        return false;
    }
    let keep = r.rest_len.min(buf.len());
    buf.as_mut_slice().copy_within(r.r1s..r.r1s + idx, keep);
    buf.truncate(keep + idx);
    true
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
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let (mut buf, ascii_lower) = Buf::<char>::fill_lowercase_tracked(token);
        let mut rewrote = false;
        let t = buf.as_slice();
        // One scan of `getRegions`' match serves all three steps; see
        // `RegionScan` for why re-deriving it after a truncation is exact.
        let mut scan = RegionScan::of(t);

        // --- Step 1: 1a and 1b share ONE regions call; the shorter result
        // wins and a tie goes to 1b (strict `<`, see the module docs).
        let r = scan.at_len(t.len());
        let len = t.len();
        let mut idx_a: Option<usize> = None;
        if r.r1e > r.r1s {
            let n = tb.step1a.longest(t, r.r1e, r.r1s);
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
            && t[len - 1] == 's'
            && matches!(
                t[len - 2],
                'b' | 'c'
                    | 'd'
                    | 'f'
                    | 'g'
                    | 'h'
                    | 'j'
                    | 'k'
                    | 'l'
                    | 'm'
                    | 'n'
                    | 'o'
                    | 'p'
                    | 'r'
                    | 't'
                    | 'v'
                    | 'y'
            )
        {
            len_b = len - 1;
        }
        if len_a < len_b {
            let idx = idx_a.unwrap_or(0);
            if rebuild(&mut buf, &r, idx) {
                // The paste arm moves units rather than only dropping them,
                // so the cached scan no longer describes the word — and the
                // result is no longer a prefix of the input.
                scan = RegionScan::of(buf.as_slice());
                rewrote = true;
            }
        } else {
            buf.truncate(len_b);
        }

        // --- Step 2: drop a final unit when R1 ends in a listed pair -------
        let r = scan.at_len(buf.len());
        if r.r1e > r.r1s && ends_consonant_pair(buf.as_slice(), r.r1e, r.r1s) {
            let keep = buf.len().saturating_sub(1);
            buf.truncate(keep);
        }

        // --- Step 3 --------------------------------------------------------
        let r = scan.at_len(buf.len());
        if r.r1e > r.r1s {
            // One search over R1 answers both alternations; the link walk
            // visits matches longest-first, so the first hit per table id is
            // that table's own longest match.
            let mut best_lost = 0usize;
            let mut best_lig = 0usize;
            let mut i = tb.step3.find_longest_index(buf.as_slice(), r.r1e, r.r1s);
            while i >= 0 {
                let (n, link, tid) = tb.step3.entry(i);
                if tid == 0 {
                    if best_lost == 0 {
                        best_lost = n;
                    }
                } else if best_lig == 0 {
                    best_lig = n;
                }
                i = link;
            }
            // `/(lös|full)t$/` — the trailing `t` is outside the group, and
            // this branch wins outright when it fires.
            if best_lost > 0 {
                let keep = buf.len().saturating_sub(1);
                buf.truncate(keep);
            } else if best_lig > 0 {
                let idx = (r.r1e - r.r1s) - best_lig;
                // Last step: nothing reads the scan after this, but the
                // paste arm still rules out borrowing the input.
                rewrote |= rebuild(&mut buf, &r, idx);
            }
        }

        // Every step here either truncates or — in `rebuild`'s paste arm —
        // moves units down; only the former leaves a prefix of the input.
        // See `crate::units::borrowed_prefix`.
        if let Some(prefix) = borrowed_prefix(token, buf.len(), ascii_lower, rewrote) {
            return Cow::Borrowed(prefix);
        }
        Cow::Owned(buf.into_text())
    }

    /// Appends a stop word to the **process-global Swedish list**.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        Language::Sv.add(word);
    }

    /// Appends several stop words to the process-global Swedish list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Language::Sv.add_all(words);
    }
}

impl TokenizeAndStem for PorterStemmerSv {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    // `prepare` is deliberately *not* overridden: the trait's identity default
    // is the specified behaviour. See `PorterStemmerSv`'s own documentation,
    // "`å`, `ä` and `ö` are letters, so nothing is folded", for the reasoning
    // and for the 116 stop words a fold here used to delete.

    fn is_stop_word(word: &str) -> bool {
        Language::Sv.contains(word)
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
        ("STEP1A", super::STEP1A),
        ("STEP2", super::STEP2),
        ("LOST_FULLT", super::LOST_FULLT),
        ("LIG_IG_ELS", super::LIG_IG_ELS),
    ];

    /// The prelude `stem` runs before any table is consulted: lowercasing,
    /// and nothing else. `å`, `ä` and `ö` are letters and are not folded.
    pub(crate) fn prelude(token: &str) -> String {
        token.to_lowercase()
    }

    /// The prelude writes no marker unit.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
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

    /// A working buffer, the way `stem` builds one.
    fn scalars(t: &str) -> Vec<char> {
        t.chars().collect()
    }

    /// **Every** entry of the Swedish stop-word list must still be recognised
    /// through the documented pipeline, not a spot check of a few.
    ///
    /// A whole-document diacritic fold in `prepare` rewrote `för` to `for`
    /// before `is_stop_word` ever saw it, so 116 of the 428 entries — every
    /// one spelled with `å`, `ä` or `ö` whose folded form is not itself on the
    /// list — silently stopped being stop words. A handful of ASCII spot
    /// checks passes that unchanged, which is why this enumerates the list.
    #[test]
    fn every_stop_word_is_filtered_by_the_pipeline() {
        let st = PorterStemmerSv::new();
        let defaults = Language::Sv.defaults();
        let mut unreachable = Vec::new();
        for word in defaults {
            // The entry must be one word to the tokenizer, or no `prepare`
            // could ever present it to `is_stop_word` in the first place.
            assert_eq!(
                st.tokenize_and_stem(word, true).len(),
                1,
                "{word:?} is not a single token"
            );
            if !st.tokenize_and_stem(word, false).is_empty() {
                unreachable.push(*word);
            }
        }
        assert!(
            unreachable.is_empty(),
            "{} of {} Swedish stop words are unreachable through the pipeline: {unreachable:?}",
            unreachable.len(),
            defaults.len()
        );
    }

    /// `prepare` rewrites nothing and borrows everything.
    ///
    /// This is the decision recorded on [`PorterStemmerSv`]: `å`, `ä` and `ö`
    /// are letters of the Swedish alphabet, so a diacritic fold here merges
    /// distinct words, makes the stemmer's own `(lös|full)t` rule unmatchable,
    /// and deletes 116 stop words. Foreign accents are left alone for the same
    /// reason the fold is: nothing downstream distinguishes them.
    #[test]
    fn prepare_is_the_identity_and_never_allocates() {
        for text in [
            "förälder",
            "Åsa",
            "âçêîñóôûš",
            "ààà ééé",
            "stiftelsen",
            "körsbärsträdgårdarna",
            "",
        ] {
            assert!(
                matches!(PorterStemmerSv::prepare(text), Cow::Borrowed(t) if t == text),
                "prepare rewrote {text:?}"
            );
        }
    }

    /// The three-letter argument, at the level a caller sees it: folding would
    /// collapse pairs that Swedish spells differently because they *are*
    /// different words.
    #[test]
    fn folding_would_merge_distinct_words() {
        let st = PorterStemmerSv::new();
        // `för` ("for") vs `for` (past tense of `fara`); `hår` vs `har`.
        assert_ne!(
            st.tokenize_and_stem("för", true),
            st.tokenize_and_stem("for", true)
        );
        assert_ne!(
            st.tokenize_and_stem("hår", true),
            st.tokenize_and_stem("har", true)
        );
        // Step 3's `(lös|full)t` check is spelled with `ö`, so it is only
        // reachable at all because nothing folded the token first.
        assert_eq!(s("löst"), "löst");
    }

    /// The hand-written step-2 matcher must answer exactly what a search of
    /// [`STEP2`] would, for every region and every pair of characters in the
    /// Latin-1 range the alternation lives in — otherwise the table it
    /// replaced would still be the specification and this would be a
    /// divergence rather than an optimisation.
    #[test]
    fn the_consonant_pair_matcher_agrees_with_its_table() {
        let table: AmongTable<char> = AmongTable::build(STEP2);
        for a in 0..=0xFFu32 {
            for b in 0..=0xFFu32 {
                let (ca, cb) = (
                    char::from_u32(a).expect("U+0000-U+00FF are all scalar values"),
                    char::from_u32(b).expect("U+0000-U+00FF are all scalar values"),
                );
                let w = ['x', ca, cb];
                for lb in 0..=3usize {
                    for cursor in lb..=3usize {
                        assert_eq!(
                            ends_consonant_pair(&w, cursor, lb),
                            table.longest(&w, cursor, lb) > 0,
                            "characters {a:#06X},{b:#06X} region {lb}..{cursor}"
                        );
                    }
                }
            }
        }
        // The pair is read as whole characters, so an astral one is one
        // position and cannot half-match either alternative.
        assert!(!ends_consonant_pair(&scalars("a😀t"), 3, 0));
        assert!(ends_consonant_pair(&scalars("😀dt"), 3, 0));
    }

    /// `RegionScan::at_len` must agree with a fresh `getRegions` of the
    /// truncated word at every truncation point, which is what lets `stem`
    /// scan once.
    #[test]
    fn a_cached_scan_matches_a_rescan_of_every_prefix() {
        let mut rng = Rng(0xB7E1_5162_8AED_2A6B);
        for _ in 0..20_000 {
            let word = random_word(&mut rng).to_lowercase();
            let t = scalars(&word);
            let scan = RegionScan::of(&t);
            for len in 0..=t.len() {
                let got = scan.at_len(len);
                let want = region_ix_uncached(&t[..len]);
                assert_eq!(
                    (got.r1s, got.r1e, got.rest_len),
                    (want.r1s, want.r1e, want.rest_len),
                    "{word:?} truncated to {len}"
                );
            }
        }
    }

    /// The Swedish counterpart of Norwegian's borrow path: an already-lower
    /// ASCII word that was only truncated comes back as a slice of the
    /// input. Swedish reaches it less often because `å ä ö` are not ASCII.
    #[test]
    fn an_unrewritten_ascii_word_is_returned_borrowed() {
        let st = PorterStemmerSv::new();
        assert!(matches!(st.stem("klockorna"), Cow::Borrowed("klock")));
        assert!(matches!(st.stem("stiftelsen"), Cow::Borrowed("stift")));
        assert!(matches!(st.stem("xyz"), Cow::Borrowed("xyz")));
        // Uppercase folds, so the buffer is no longer the input's bytes.
        assert!(matches!(st.stem("BJORKS"), Cow::Owned(_)));
        // Non-ASCII keeps the owned path.
        assert!(matches!(st.stem("björks"), Cow::Owned(_)));
        assert_eq!(st.stem("björks"), "björk");
        assert_eq!(st.stem("BJÖRKS"), "björk");
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

    /// The two characters this file cannot tell apart.
    ///
    /// `U+1F600` is one Unicode scalar value and two UTF-16 code units;
    /// `U+4E2D` is one of each. Nothing in this file distinguishes them: the
    /// vowel class is `[aeiouyäåö]`, R1's capture class is `[a-zåäö]`, step
    /// 1b's consonant list and step 2's pairs are ASCII, every table entry is
    /// drawn from `a`–`z` plus `ö`, and `str::to_lowercase` leaves both alone.
    /// Neither is in any of those sets, so the only thing about either that
    /// can influence the result is a position or a length — which is the unit
    /// under test.
    const ASTRAL: char = '😀';
    /// The Basic Multilingual Plane twin of [`ASTRAL`]; see there.
    const BMP_TWIN: char = '中';

    /// Every entry of the Swedish stop-word list and of every rule table, with
    /// one inert character inserted at every character position, paired with
    /// the same insertion of its BMP twin.
    fn inert_placements() -> Vec<(String, String)> {
        let mut corpus: Vec<&str> = Language::Sv.defaults().to_vec();
        for (_, table) in audit::TABLES {
            corpus.extend_from_slice(table);
        }
        let mut out = Vec::new();
        for w in corpus {
            for at in w.char_indices().map(|(i, _)| i).chain([w.len()]) {
                let (mut astral, mut bmp) = (w.to_owned(), w.to_owned());
                astral.insert(at, ASTRAL);
                bmp.insert(at, BMP_TWIN);
                out.push((astral, bmp));
            }
        }
        out
    }

    /// An inert character occupies **one** position, whichever plane it lives
    /// on — enumerated over the whole stop-word list and every rule table
    /// rather than sampled.
    ///
    /// Swedish is the language this matters most for. Its cuts do not come
    /// from a matched suffix length but from region arithmetic — `RegionScan`'s
    /// `start: 3`, and [`rebuild`]'s `rest_len` — which no table audit bounds,
    /// so a buffer whose positions were not characters could be cut anywhere
    /// at all.
    #[test]
    fn an_astral_character_occupies_one_position() {
        let st = PorterStemmerSv::new();
        let cases = inert_placements();
        let twin = BMP_TWIN.to_string();
        // Pinned so an enumeration that quietly walked nothing cannot pass:
        // 428 stop words and 48 rule-table entries, each probed at every one of its
        // `len + 1` character positions.
        assert_eq!(cases.len(), 2745, "the enumerated corpus changed size");
        let mut diverged: Vec<(String, String, String)> = Vec::new();
        let mut invented: Vec<String> = Vec::new();
        for (astral, bmp) in &cases {
            let got = st.stem(astral).into_owned();
            if got.contains('\u{FFFD}') {
                invented.push(astral.clone());
            }
            let want = st.stem(bmp).into_owned();
            if got.replace(ASTRAL, &twin) != want {
                diverged.push((astral.clone(), got, want));
            }
        }
        // One assertion for both defects, so a failing run reports both counts
        // rather than stopping at the first.
        assert!(
            invented.is_empty() && diverged.is_empty(),
            "of {} placements, {} come back carrying a replacement character \
             the caller never supplied ({:?}) and {} measure an astral \
             character as more than one position ({:?})",
            cases.len(),
            invented.len(),
            &invented[..invented.len().min(3)],
            diverged.len(),
            &diverged[..diverged.len().min(3)]
        );
    }

    /// Every character of a stem is a character of the input, so no cut can
    /// invent one.
    ///
    /// This is the property a character buffer gives by construction and a
    /// code-unit buffer could not: [`rebuild`]'s paste arm moves positions
    /// with `copy_within` at `rest_len`, a length derived from the region scan
    /// with no relation to any table entry, and truncates at `keep + idx`. The
    /// corpus below is built to reach that arm — R1's capture class excludes
    /// `-`, `ü` and digits, so those make `rest` and R1's start disagree.
    #[test]
    fn no_cut_can_split_a_character() {
        let st = PorterStemmerSv::new();
        let mut corpus: Vec<String> = Vec::new();
        for stem in ["klok", "hus", "het", "björk", "stiftels", "gost"] {
            for tail in ["a", "e", "en", "ens", "heten", "hetens", "ade", "ig", "els"] {
                for filler in ["", "-", "ü", "1"] {
                    corpus.push(format!("{stem}{filler}{tail}"));
                }
            }
        }
        let mut probes = 0usize;
        for word in &corpus {
            for at in word.char_indices().map(|(i, _)| i).chain([word.len()]) {
                let mut probe = word.clone();
                probe.insert(at, ASTRAL);
                probes += 1;
                let got = st.stem(&probe).into_owned();
                assert!(
                    !got.contains('\u{FFFD}'),
                    "stem({probe:?}) = {got:?} — a character the caller never supplied"
                );
                // Every step here either truncates or moves characters down;
                // none writes a character of its own. So every character of
                // the stem is a character of the input — which is the strongest
                // form of "nothing was invented", and the exact claim a cut
                // between the halves of a surrogate pair breaks.
                assert!(
                    got.chars().all(|c| probe.contains(c)),
                    "stem({probe:?}) = {got:?} holds a character the input does not"
                );
            }
        }
        // 6 stems x 9 tails x 4 fillers = 216 words, each probed at every one
        // of its `len + 1` positions: 36*27 + 24*26 + 54*3 + 216 = 1974.
        assert_eq!(corpus.len(), 6 * 9 * 4);
        assert_eq!(probes, 1_974, "the probe corpus changed size");
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::text;
        use crate::units::{ends_with, slen};

        struct Regions<'t> {
            r1: &'t [char],
            rest_len: usize,
        }

        fn regions(t: &[char]) -> Regions<'_> {
            let mut r1: &[char] = &[];
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

        fn listed_suffix(w: &[char], alternatives: &[&str]) -> Option<usize> {
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

        fn step1a(t: &[char], r: &Regions<'_>) -> Vec<char> {
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

        fn step1b(t: &[char], r: &Regions<'_>) -> Vec<char> {
            if !r.r1.is_empty()
                && t.len() >= 2
                && t[t.len() - 1] == 's'
                && matches!(
                    t[t.len() - 2],
                    'b' | 'c'
                        | 'd'
                        | 'f'
                        | 'g'
                        | 'h'
                        | 'j'
                        | 'k'
                        | 'l'
                        | 'm'
                        | 'n'
                        | 'o'
                        | 'p'
                        | 'r'
                        | 't'
                        | 'v'
                        | 'y'
                )
            {
                return t[..t.len() - 1].to_vec();
            }
            t.to_vec()
        }

        fn step1(t: &[char]) -> Vec<char> {
            let r = regions(t);
            let a = step1a(t, &r);
            let b = step1b(t, &r);
            if a.len() < b.len() { a } else { b }
        }

        fn step2(t: &[char]) -> Vec<char> {
            let r = regions(t);
            if !r.r1.is_empty() && listed_suffix(r.r1, STEP2).is_some() {
                return t[..t.len().saturating_sub(1)].to_vec();
            }
            t.to_vec()
        }

        fn step3(t: &[char]) -> Vec<char> {
            let r = regions(t);
            if r.r1.is_empty() {
                return t.to_vec();
            }
            if listed_suffix(r.r1, LOST_FULLT).is_some() {
                return t[..t.len().saturating_sub(1)].to_vec();
            }
            match listed_suffix(r.r1, LIG_IG_ELS) {
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
            let t: Vec<char> = lower.chars().collect();
            text(&step3(&step2(&step1(&t))))
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
