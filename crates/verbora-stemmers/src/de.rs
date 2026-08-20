//! The German Snowball stemmer, ported from
//! The reference `porter_stemmer_de`.
//!
//! # Case sensitivity is the whole story
//!
//! Nothing here lowercases. Every vowel class and every suffix literal is
//! lowercase, so an uppercase word flows through the region machinery matching
//! nothing: `stem("Häuser")` is `"Haus"` — the capital `H` survives — while
//! `stem("STRASSE")` and `stem("FRÖHLICH")` come back untouched. A `to_lowercase`
//! at the front would look like an obvious improvement and would change results
//! for every capitalised noun, which in German is most of them.
//!
//! # R2 is computed from the *unadjusted* R1
//!
//! The Snowball specification, quoted in the reference's own comment, says R1 is
//! adjusted so that at least three letters precede it. The code computes R1, then
//! R2 from the un-adjusted R1's substring, and only afterwards clamps R1 to 3.
//! Dutch, which is otherwise the same shape, adjusts first. Sharing one region
//! routine between the two breaks German — verified on `äckern`, where R1 becomes
//! 3 but R2 stays 5.
//!
//! # Suffix selection
//!
//! Each `word.search(/(a|b|c)$/)` returns the index where the match *starts*.
//! Because the alternatives are distinct literals anchored at `$`, the earliest
//! start is the longest suffix. The German code then does arithmetic on those
//! indices (`c1Index++`, `b2Index += 4`, `b3Index++`) and picks the smallest
//! with a **strict** `<`, so a tie keeps the earlier option letter — and the
//! option letter decides which follow-up rule runs.
//!
//! # The unit
//!
//! Every index above, both region bounds, the `r1 < 3` clamp and step 2's
//! `.{3}` lookbehind count **Unicode scalar values** — the unit
//! [`crate::units`] states for the whole crate, and the one the rules are
//! written in: R1 is *"the region after the first non-vowel following a
//! vowel"*, and a `.` in the reference's `/(.{3}[bdfghklmnt]st)$/` stands for
//! a letter. Nothing in this file is spelled in any other unit, so a cut can
//! only ever land on a character boundary.

use std::borrow::Cow;

use crate::among::Buf;
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_de;
use crate::stopwords::Language;
use crate::units::{ends_with, slen};

/// Options for [`PorterStemmerDe::stem_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerDeOptions {
    /// Keep `ä ö ü` instead of folding them to `a o u` in the postlude.
    pub preserve_umlauts: bool,
}

/// The German Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerDe;
/// let s = PorterStemmerDe::new();
/// assert_eq!(s.stem("bedürfnissen"), "bedurfnis");
/// assert_eq!(s.stem("Häuser"), "Haus");
/// assert_eq!(s.stem("STRASSE"), "STRASSE");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerDe;

/// `[aeiouyäöü]` — lowercase only, as written.
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'ä' | 'ö' | 'ü')
}

/// `[bdfghklmnrt]`, the valid s-ending.
#[inline]
fn is_s_ending(c: char) -> bool {
    matches!(
        c,
        'b' | 'd' | 'f' | 'g' | 'h' | 'k' | 'l' | 'm' | 'n' | 'r' | 't'
    )
}

/// `[bdfghklmnt]`, the valid st-ending (the s-ending without `r`).
#[inline]
fn is_st_ending(c: char) -> bool {
    matches!(c, 'b' | 'd' | 'f' | 'g' | 'h' | 'k' | 'l' | 'm' | 'n' | 't')
}

#[inline]
const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Whether `c` is one of the letters [`gate_de`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and its
/// highest member is `ẞ` (`U+1E9E`), so scanning characters and scanning
/// UTF-16 code units admit exactly the same tokens: a BMP character *is* its
/// own code unit, and an astral character is neither in the set itself nor are
/// the two surrogates it used to be scanned as. The scan is per character
/// because that is the crate's unit, not because the answer moved.
#[inline]
fn is_german_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_de(c as u16)
}

/// `word.search(/(alt|…)$/)`: the start index of the longest matching suffix.
fn search_suffix(w: &[char], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if ends_with(w, a) && best.is_none_or(|b| n > b) {
            best = Some(n);
        }
    }
    best.map(|n| w.len() - n)
}

/// `word.replace(/([V])x([V])/g, '$1X$2')` for one marked letter.
///
/// The `/g` replace is non-overlapping and scans left to right, so `"auau"`
/// becomes `"aUau"`: the second `u` is inside the region the first match already
/// consumed.
/// `token.replace(/ß/g, 'ss')`, in place.
///
/// The rewrite lengthens the word by one character per `ß`, so it counts first
/// and then copies backwards — no position is overwritten before it has been
/// read, and a word without `ß` (nearly all of them) never touches the buffer.
/// The filler pushed to make room is never read as text.
fn expand_sharp_s(buf: &mut Buf<char>) {
    let extra = buf.as_slice().iter().filter(|&&c| c == 'ß').count();
    if extra == 0 {
        return;
    }
    let old_len = buf.len();
    for _ in 0..extra {
        buf.push('\0');
    }
    let w = buf.as_mut_slice();
    let mut write = old_len + extra;
    for read in (0..old_len).rev() {
        if w[read] == 'ß' {
            write -= 2;
            w[write] = 's';
            w[write + 1] = 's';
        } else {
            write -= 1;
            w[write] = w[read];
        }
    }
}

fn mark_between_vowels(w: &mut [char], letter: char, marked: char) {
    let mut i = 0;
    while i + 2 < w.len() {
        if is_vowel(w[i]) && w[i + 1] == letter && is_vowel(w[i + 2]) {
            w[i + 1] = marked;
            i += 3;
        } else {
            i += 1;
        }
    }
}

/// The first index where a vowel is followed by a non-vowel.
fn region_scan(w: &[char], from: usize) -> Option<usize> {
    (from..w.len().saturating_sub(1)).find(|&i| is_vowel(w[i]) && !is_vowel(w[i + 1]))
}

/// The chosen option among several `(index, letter)` candidates.
///
/// `if (x !== -1 && x < index)` — a strict `<`, so the earlier letter wins ties.
fn choose(candidates: &[(Option<usize>, char)]) -> Option<(usize, char)> {
    let mut best: Option<(usize, char)> = None;
    for &(idx, letter) in candidates {
        if let Some(i) = idx
            && best.is_none_or(|(b, _)| i < b)
        {
            best = Some((i, letter));
        }
    }
    best
}

impl PorterStemmerDe {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token with default options.
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        self.stem_with(word, PorterStemmerDeOptions::default())
    }

    /// Stems one token, optionally keeping the umlauts.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn stem_with<'a>(&self, word: &'a str, options: PorterStemmerDeOptions) -> Cow<'a, str> {
        let mut buf: Buf<char> = Buf::fill(word);

        // --- Prelude -------------------------------------------------------
        // `u` and `y` between vowels are marked so they stop counting as vowels.
        // The commented-out ae/oe/ue mappings in the reference stay omitted:
        // they cause trouble with diphthongs, as its comment says.
        mark_between_vowels(buf.as_mut_slice(), 'u', 'U');
        mark_between_vowels(buf.as_mut_slice(), 'y', 'Y');
        expand_sharp_s(&mut buf);
        let w = buf.as_mut_slice();

        // --- Regions -------------------------------------------------------
        let mut r1_index = region_scan(w, 0).map(|i| i + 2);
        // R2 comes from the UNADJUSTED R1. Reordering these two blocks is the
        // single most tempting "cleanup" in this file, and it changes results.
        let r2_index = r1_index.and_then(|r1| region_scan(&w[r1..], 0).map(|i| i + 2 + r1));
        if let Some(r1) = r1_index
            && r1 < 3
        {
            r1_index = Some(3);
        }

        // --- Step 1 --------------------------------------------------------
        let a1 = search_suffix(w, STEP1_A);
        let b1 = search_suffix(w, STEP1_B);
        let c1 = (w.len() >= 2 && w[w.len() - 1] == 's' && is_s_ending(w[w.len() - 2]))
            .then(|| w.len() - 1);
        if let Some((index1, option1)) = choose(&[(a1, 'a'), (b1, 'b'), (c1, 'c')])
            && let Some(r1) = r1_index
            && index1 >= r1
        {
            buf.truncate(index1);
            if option1 == 'b' && ends_with(buf.as_slice(), NISS[0]) {
                buf.truncate(buf.len() - 1);
            }
        }

        // --- Step 2 --------------------------------------------------------
        let w = buf.as_slice();
        let a2 = search_suffix(w, STEP2_A);
        // `/(.{3}[bdfghklmnt]st)$/` then `+= 4`: three characters `.` can match
        // (so no line terminators), a valid st-ending, then `st`.
        let b2 = (w.len() >= 6
            && ends_with(w, "st")
            && is_st_ending(w[w.len() - 3])
            && !w[w.len() - 6..w.len() - 3]
                .iter()
                .copied()
                .any(is_line_terminator))
        .then(|| w.len() - 2);
        if let Some((index2, _)) = choose(&[(a2, 'a'), (b2, 'b')])
            && let Some(r1) = r1_index
            && index2 >= r1
        {
            buf.truncate(index2);
        }

        // --- Step 3 --------------------------------------------------------
        let w = buf.as_slice();
        let a3 = search_suffix(w, STEP3_A);
        let b3 = Self::search_non_e_prefixed(w, STEP3_B).map(|i| i + 1);
        let c3 = search_suffix(w, STEP3_C);
        let d3 = search_suffix(w, STEP3_D);
        if let Some((index3, option3)) = choose(&[(a3, 'a'), (b3, 'b'), (c3, 'c'), (d3, 'd')])
            && let Some(r2) = r2_index
            && index3 >= r2
        {
            buf.truncate(index3);
            match option3 {
                'a' => {
                    if let Some(o) =
                        Self::search_non_e_prefixed(buf.as_slice(), STEP3_A_IG).map(|i| i + 1)
                        && o >= r2
                    {
                        buf.truncate(o);
                    }
                }
                'c' => {
                    if let Some(o) = search_suffix(buf.as_slice(), STEP3_C_ER_EN)
                        && r1_index.is_some_and(|r1| o >= r1)
                    {
                        buf.truncate(o);
                    }
                }
                'd' => {
                    if let Some(o) = search_suffix(buf.as_slice(), STEP3_D_LICH_IG)
                        && o >= r2
                    {
                        buf.truncate(o);
                    }
                }
                _ => {}
            }
        }

        // --- Postlude ------------------------------------------------------
        for c in buf.as_mut_slice() {
            match *c {
                'U' => *c = 'u',
                'Y' => *c = 'y',
                _ => {}
            }
        }
        if !options.preserve_umlauts {
            for c in buf.as_mut_slice() {
                match *c {
                    'ä' => *c = 'a',
                    'ö' => *c = 'o',
                    'ü' => *c = 'u',
                    _ => {}
                }
            }
        }
        Cow::Owned(buf.into_text())
    }

    /// `word.search(/[^e](alt|…)$/)`: the index of the guard character.
    ///
    /// The guard is "any character that is not `e`" — a negated class, so it
    /// matches line terminators too, unlike `.`.
    fn search_non_e_prefixed(w: &[char], alts: &[&str]) -> Option<usize> {
        let mut best: Option<usize> = None;
        for a in alts {
            let n = slen(a);
            if w.len() > n && ends_with(w, a) && w[w.len() - n - 1] != 'e' {
                let start = w.len() - n - 1;
                if best.is_none_or(|b| start < b) {
                    best = Some(start);
                }
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// Rule tables
// ---------------------------------------------------------------------------
//
// Every suffix literal the algorithm compares against is named here rather
// than written inline at its call site, so `data::table_audit` can walk all of
// them through the prelude that guards them. An inline literal is a table no
// audit can enumerate.

/// Step 1, option `a`: the endings deleted in R1 outright.
static STEP1_A: &[&str] = &["em", "ern", "er"];
/// Step 1, option `b`: the endings whose deletion may expose `niss`.
static STEP1_B: &[&str] = &["e", "en", "es"];
/// The residue option `b` checks for, to undouble `nis`.
static NISS: &[&str] = &["niss"];
/// Step 2, option `a`.
static STEP2_A: &[&str] = &["en", "er", "est"];
/// Step 3, option `a`.
static STEP3_A: &[&str] = &["end", "ung"];
/// Step 3, option `b` — each must be preceded by a character other than `e`.
static STEP3_B: &[&str] = &["isch", "ig", "ik"];
/// Step 3, option `c`.
static STEP3_C: &[&str] = &["lich", "heit"];
/// Step 3, option `d`.
static STEP3_D: &[&str] = &["keit"];
/// The follow-up to option `a`.
static STEP3_A_IG: &[&str] = &["ig"];
/// The follow-up to option `c`, checked against R1 rather than R2.
static STEP3_C_ER_EN: &[&str] = &["er", "en"];
/// The follow-up to option `d`.
static STEP3_D_LICH_IG: &[&str] = &["lich", "ig"];

impl TokenizeAndStem for PorterStemmerDe {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::De.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.chars().any(is_german_letter)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerDe {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    use crate::among::Buf;

    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("STEP1_A", super::STEP1_A),
        ("STEP1_B", super::STEP1_B),
        ("NISS", super::NISS),
        ("STEP2_A", super::STEP2_A),
        ("STEP3_A", super::STEP3_A),
        ("STEP3_B", super::STEP3_B),
        ("STEP3_C", super::STEP3_C),
        ("STEP3_D", super::STEP3_D),
        ("STEP3_A_IG", super::STEP3_A_IG),
        ("STEP3_C_ER_EN", super::STEP3_C_ER_EN),
        ("STEP3_D_LICH_IG", super::STEP3_D_LICH_IG),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation.
    pub(crate) fn prelude(token: &str) -> String {
        let mut buf: Buf<char> = Buf::fill(token);
        super::mark_between_vowels(buf.as_mut_slice(), 'u', 'U');
        super::mark_between_vowels(buf.as_mut_slice(), 'y', 'Y');
        super::expand_sharp_s(&mut buf);
        buf.into_text()
    }

    /// The units the prelude writes, paired with what it writes them for.
    pub(crate) static MARKERS: &[(&str, &str)] = &[("U", "u"), ("Y", "y"), ("ss", "ß")];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerDe::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("bedürfnissen", "bedurfnis"),
            ("äckern", "ack"),
            ("ackers", "ack"),
            ("armes", "arm"),
            ("derbsten", "derb"),
            ("straße", "strass"),
            ("", ""),
            ("123", "123"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn case_is_preserved_because_nothing_folds() {
        assert_eq!(s("Häuser"), "Haus");
        assert_eq!(s("STRASSE"), "STRASSE");
        assert_eq!(s("FRÖHLICH"), "FRÖHLICH");
        assert_eq!(s("fröhlich"), "frohlich");
    }

    #[test]
    fn umlauts_can_be_kept() {
        let opts = PorterStemmerDeOptions {
            preserve_umlauts: true,
        };
        assert_eq!(
            PorterStemmerDe::new().stem_with("fröhlich", opts),
            "fröhlich"
        );
        assert_eq!(PorterStemmerDe::new().stem_with("äckern", opts), "äck");
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("a"), "a");
        assert_eq!(s("ab"), "ab");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
    }

    /// The two characters this file cannot tell apart.
    ///
    /// `U+1F600` is one Unicode scalar value and two UTF-16 code units;
    /// `U+4E2D` is one of each. Nothing in this file distinguishes them: the
    /// vowel class is `[aeiouyäöü]`, the s-ending and st-ending classes are
    /// `[bdfghklmnrt]` and `[bdfghklmnt]`, the line-terminator class is
    /// `\n \r U+2028 U+2029`, the prelude looks only for `u`, `y` and `ß`, the
    /// postlude only for `U`, `Y`, `ä`, `ö` and `ü`, every rule table is ASCII,
    /// and `str::to_lowercase` leaves both alone. Neither is in any of those
    /// sets, so the *only* thing about them either can influence is a position
    /// or a length — which is exactly the unit under test.
    const ASTRAL: char = '😀';
    /// The Basic Multilingual Plane twin of [`ASTRAL`]; see there.
    const BMP_TWIN: char = '中';

    /// Every entry of the German stop-word list and of every rule table, with
    /// one inert character inserted at every character position, paired with
    /// the same insertion of its BMP twin.
    fn inert_placements() -> Vec<(String, String)> {
        let mut corpus: Vec<&str> = Language::De.defaults().to_vec();
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
    /// Because [`ASTRAL`] and [`BMP_TWIN`] are indistinguishable to every
    /// predicate, every table and the case fold, the two stems must be the
    /// same string up to the substitution. Anything else is a position or a
    /// length counted in a unit that is not a character.
    #[test]
    fn an_astral_character_occupies_one_position() {
        let st = PorterStemmerDe::new();
        let cases = inert_placements();
        let twin = BMP_TWIN.to_string();
        // Pinned so an enumeration that quietly walked nothing cannot pass:
        // 620 stop words and 23 rule-table entries, each probed at every one of its
        // `len + 1` character positions.
        assert_eq!(cases.len(), 3978, "the enumerated corpus changed size");
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

    /// The German gate still admits the letters German is written with and
    /// still rejects the Spanish ones it was once a copy of — enumerated over
    /// the whole stop-word list, and through the public `gate` rather than the
    /// raw predicate, because the scan over that predicate is what changed.
    #[test]
    fn the_gate_survives_the_character_scan() {
        // The four letters the corrected set added, and the six Spanish ones
        // it dropped. `.any()` makes an omission invisible in any word that
        // also holds an a-z letter, so each is asked about on its own.
        for c in "äöüßÄÖÜẞ".chars() {
            assert!(
                PorterStemmerDe::gate(&c.to_string()),
                "the German gate rejects {c:?}, a letter German is written with"
            );
        }
        for c in "áéíñóúÁÉÍÑÓÚ".chars() {
            assert!(
                !PorterStemmerDe::gate(&c.to_string()),
                "the German gate admits {c:?}, which is Spanish"
            );
        }
        // Nothing outside the Basic Multilingual Plane is a German letter, and
        // scanning characters must not admit one through a surrogate half.
        assert!(!PorterStemmerDe::gate("😀"));
        assert!(!PorterStemmerDe::gate("日本語"));
        assert!(PorterStemmerDe::gate("😀a"));
        // Every entry of the list, through the gate the pipeline applies.
        let rejected: Vec<&str> = Language::De
            .defaults()
            .iter()
            .copied()
            .filter(|w| !PorterStemmerDe::gate(&w.to_lowercase()))
            .collect();
        assert!(
            rejected.is_empty(),
            "{} of {} German stop words are rejected by the German gate: {rejected:?}",
            rejected.len(),
            Language::De.defaults().len()
        );
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-`Buf` implementation, verbatim — an owned
    // `Vec<char>` working buffer and a freshly allocated one for the `ß`
    // expansion. The conversion above is meant to change nothing but where
    // those characters live.
    // -----------------------------------------------------------------------
    fn oracle_stem(word: &str, options: PorterStemmerDeOptions) -> String {
        use crate::units::text;
        let mut w: Vec<char> = word.chars().collect();

        // --- Prelude -------------------------------------------------------
        // `u` and `y` between vowels are marked so they stop counting as vowels.
        // The commented-out ae/oe/ue mappings in the reference stay omitted:
        // they cause trouble with diphthongs, as its comment says.
        mark_between_vowels(&mut w, 'u', 'U');
        mark_between_vowels(&mut w, 'y', 'Y');
        if w.contains(&'ß') {
            let mut expanded = Vec::with_capacity(w.len() + 2);
            for c in &w {
                if *c == 'ß' {
                    expanded.extend("ss".chars());
                } else {
                    expanded.push(*c);
                }
            }
            w = expanded;
        }

        // --- Regions -------------------------------------------------------
        let mut r1_index = region_scan(&w, 0).map(|i| i + 2);
        // R2 comes from the UNADJUSTED R1. Reordering these two blocks is the
        // single most tempting "cleanup" in this file, and it changes results.
        let r2_index = r1_index.and_then(|r1| region_scan(&w[r1..], 0).map(|i| i + 2 + r1));
        if let Some(r1) = r1_index
            && r1 < 3
        {
            r1_index = Some(3);
        }

        // --- Step 1 --------------------------------------------------------
        let a1 = search_suffix(&w, STEP1_A);
        let b1 = search_suffix(&w, STEP1_B);
        let c1 = (w.len() >= 2 && w[w.len() - 1] == 's' && is_s_ending(w[w.len() - 2]))
            .then(|| w.len() - 1);
        if let Some((index1, option1)) = choose(&[(a1, 'a'), (b1, 'b'), (c1, 'c')])
            && let Some(r1) = r1_index
            && index1 >= r1
        {
            w.truncate(index1);
            if option1 == 'b' && ends_with(&w, NISS[0]) {
                w.truncate(w.len() - 1);
            }
        }

        // --- Step 2 --------------------------------------------------------
        let a2 = search_suffix(&w, STEP2_A);
        // `/(.{3}[bdfghklmnt]st)$/` then `+= 4`: three characters `.` can match
        // (so no line terminators), a valid st-ending, then `st`.
        let b2 = (w.len() >= 6
            && ends_with(&w, "st")
            && is_st_ending(w[w.len() - 3])
            && !w[w.len() - 6..w.len() - 3]
                .iter()
                .copied()
                .any(is_line_terminator))
        .then(|| w.len() - 2);
        if let Some((index2, _)) = choose(&[(a2, 'a'), (b2, 'b')])
            && let Some(r1) = r1_index
            && index2 >= r1
        {
            w.truncate(index2);
        }

        // --- Step 3 --------------------------------------------------------
        let a3 = search_suffix(&w, STEP3_A);
        let b3 = PorterStemmerDe::search_non_e_prefixed(&w, STEP3_B).map(|i| i + 1);
        let c3 = search_suffix(&w, STEP3_C);
        let d3 = search_suffix(&w, STEP3_D);
        if let Some((index3, option3)) = choose(&[(a3, 'a'), (b3, 'b'), (c3, 'c'), (d3, 'd')])
            && let Some(r2) = r2_index
            && index3 >= r2
        {
            w.truncate(index3);
            match option3 {
                'a' => {
                    if let Some(o) =
                        PorterStemmerDe::search_non_e_prefixed(&w, STEP3_A_IG).map(|i| i + 1)
                        && o >= r2
                    {
                        w.truncate(o);
                    }
                }
                'c' => {
                    if let Some(o) = search_suffix(&w, STEP3_C_ER_EN)
                        && r1_index.is_some_and(|r1| o >= r1)
                    {
                        w.truncate(o);
                    }
                }
                'd' => {
                    if let Some(o) = search_suffix(&w, STEP3_D_LICH_IG)
                        && o >= r2
                    {
                        w.truncate(o);
                    }
                }
                _ => {}
            }
        }

        // --- Postlude ------------------------------------------------------
        for c in &mut w {
            match *c {
                'U' => *c = 'u',
                'Y' => *c = 'y',
                _ => {}
            }
        }
        if !options.preserve_umlauts {
            for c in &mut w {
                match *c {
                    'ä' => *c = 'a',
                    'ö' => *c = 'o',
                    'ü' => *c = 'u',
                    _ => {}
                }
            }
        }
        text(&w)
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

    /// German stems crossed with the real rule suffixes, plus umlaut, `ß`,
    /// case, astral and digit noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'k', 'l', 'm', 'n', 'o', 'r', 's', 't',
            'u', 'y', 'z', 'ä', 'ö', 'ü', 'ß',
        ];
        const SUFFIXES: &[&str] = &[
            "em", "ern", "er", "e", "en", "es", "est", "st", "end", "ung", "isch", "ig", "ik",
            "lich", "heit", "keit", "niss", "nis", "ß", "sse", "auen", "eien",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(9) {
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
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_pre_buf_oracle() {
        let stemmer = PorterStemmerDe::new();
        let check = |input: &str| {
            for options in [
                PorterStemmerDeOptions::default(),
                PorterStemmerDeOptions {
                    preserve_umlauts: true,
                },
            ] {
                assert_eq!(
                    stemmer.stem_with(input, options).as_ref(),
                    oracle_stem(input, options),
                    "stem_with({input:?}, {options:?})"
                );
            }
        };
        for w in crate::test_support::bench_words("de") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "ab",
            "123",
            "😀",
            "日本語",
            "bedürfnissen",
            "äckern",
            "ackers",
            "armes",
            "derbsten",
            "straße",
            "STRASSE",
            "FRÖHLICH",
            "fröhlich",
            "Häuser",
            "ßßßß",
            "bauernhaus",
            "seiend",
            "heiterkeit",
        ] {
            check(w);
        }
        let long = "bedürfnissen".repeat(6); // exercises the `Buf` heap spill
        check(&long);
        let mut rng = Rng(0x243F_6A88_85A3_08D3);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
