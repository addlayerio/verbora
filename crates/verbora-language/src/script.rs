//! Script detection — the block table, the vote, and the tie rule.
//!
//! The user-facing contract lives on [`Script`] and [`detect_script`]
//! themselves: this module is private, so anything written here as `//!`
//! never reaches docs.rs. What stays here is implementation reasoning.

use std::fmt;

/// A writing system, classified from a scalar's Unicode block.
///
/// Script detection is more reliable than language detection on short
/// input: knowing a word is written in Cyrillic does not tell you whether
/// it is Russian or Ukrainian, but it rules out every Latin-script language
/// at zero cost, before spending anything on a statistical model. This is a
/// majority vote over Unicode blocks, not a model — no training data, no
/// crate dependency, no allocation.
///
/// The enumeration is deliberately coarse: it names exactly the scripts
/// this crate's [`Language`](crate::Language) list is written in, plus
/// [`Script::Other`] for everything else. It is not, and will not become, a
/// re-encoding of the UCD `Script` property's ~160 values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Script {
    /// Covers every Latin-script language this crate's [`Language`](crate::Language)
    /// enumerates (English, Spanish, French, German, …), diacritics
    /// included.
    Latin,
    /// Russian, Ukrainian, and other Cyrillic-script languages.
    Cyrillic,
    /// Modern Greek.
    Greek,
    /// Arabic and other Arabic-script languages.
    Arabic,
    /// Hebrew.
    Hebrew,
    /// Han (CJK ideographs) — Chinese, and the kanji portion of Japanese.
    Han,
    /// Japanese hiragana.
    Hiragana,
    /// Japanese katakana.
    Katakana,
    /// Korean Hangul.
    Hangul,
    /// Hindi and other Devanagari-script languages.
    Devanagari,
    /// A letter in a script this classifier has no dedicated variant for
    /// (Thai, Armenian, Ethiopic, …).
    ///
    /// Not an error, and **not** the same answer as `None`: `Other` means
    /// "there are letters here, written in a script this crate models no
    /// language for", while `None` from [`detect_script`] means "there are
    /// no letters here at all". A caller routing to a language-specific
    /// pipeline wants to distinguish the two.
    Other,
}

impl Script {
    /// The script one Unicode scalar votes for, or `None` when it is
    /// script-neutral.
    ///
    /// The rule, in full:
    ///
    /// 1. A scalar without the Unicode `Alphabetic` property
    ///    ([`char::is_alphabetic`], UAX #44's derived property) is
    ///    script-neutral and votes for nothing. Digits, punctuation,
    ///    whitespace, symbols, emoji and non-alphabetic combining marks are
    ///    all in this class — including the ones that sit *inside* a
    ///    script's block, such as `×` (U+00D7, inside Latin-1 Supplement),
    ///    the Greek ano teleia `·` (U+0387) and the katakana middle dot `・`
    ///    (U+30FB).
    /// 2. An alphabetic scalar in one of the blocks below votes for that
    ///    block's script.
    /// 3. Any other alphabetic scalar votes for [`Script::Other`].
    ///
    /// | Script | Blocks |
    /// |---|---|
    /// | [`Script::Latin`] | `U+0041..=U+005A`, `U+0061..=U+007A`, `U+00C0..=U+02AF`, `U+1E00..=U+1EFF` |
    /// | [`Script::Greek`] | `U+0370..=U+03FF`, `U+1F00..=U+1FFF` |
    /// | [`Script::Cyrillic`] | `U+0400..=U+052F` |
    /// | [`Script::Hebrew`] | `U+0590..=U+05FF` |
    /// | [`Script::Arabic`] | `U+0600..=U+06FF`, `U+0750..=U+077F` |
    /// | [`Script::Devanagari`] | `U+0900..=U+097F` |
    /// | [`Script::Hiragana`] | `U+3040..=U+309F` |
    /// | [`Script::Katakana`] | `U+30A0..=U+30FF` |
    /// | [`Script::Hangul`] | `U+AC00..=U+D7AF` |
    /// | [`Script::Han`] | `U+4E00..=U+9FFF`, `U+3400..=U+4DBF`, `U+F900..=U+FAFF` |
    ///
    /// This is a block approximation of UAX #24's `Script` property, not
    /// the property itself: the property would need the full UCD table, and
    /// this crate ships no Unicode data. The approximation is exact for
    /// every letter of the ten scripts above and errs only by answering
    /// [`Script::Other`] for letters outside their blocks (Latin letters in
    /// Latin Extended-C, for instance).
    ///
    /// ```
    /// use verbora_language::Script;
    ///
    /// assert_eq!(Script::of('a'), Some(Script::Latin));
    /// assert_eq!(Script::of('Ж'), Some(Script::Cyrillic));
    /// assert_eq!(Script::of('ก'), Some(Script::Other)); // Thai: a letter, no variant
    /// assert_eq!(Script::of('7'), None); // a digit votes for nothing
    /// assert_eq!(Script::of('×'), None); // a symbol, despite its Latin-1 block
    /// ```
    #[must_use]
    pub fn of(c: char) -> Option<Self> {
        classify_index(c).map(Self::from_index)
    }

    /// [`SCRIPTS`]`[index]`, with [`OTHER`] (and any out-of-range index)
    /// answering [`Script::Other`]. Total by construction: no panic, no
    /// `expect`.
    fn from_index(index: u8) -> Self {
        SCRIPTS
            .get(index as usize)
            .copied()
            .unwrap_or(Script::Other)
    }
}

impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Latin => "Latin",
            Self::Cyrillic => "Cyrillic",
            Self::Greek => "Greek",
            Self::Arabic => "Arabic",
            Self::Hebrew => "Hebrew",
            Self::Han => "Han",
            Self::Hiragana => "Hiragana",
            Self::Katakana => "Katakana",
            Self::Hangul => "Hangul",
            Self::Devanagari => "Devanagari",
            Self::Other => "Other",
        })
    }
}

/// The scripts [`detect_script`] keeps a vote count for, in count-array
/// index order. [`Script::Other`] is deliberately absent: it is counted
/// separately because it loses every tie (see [`detect_script`]).
///
/// A fixed-size array beats a `HashMap` here — `Copy`, no hashing, no
/// allocation, and the whole table fits in one cache line.
const SCRIPTS: [Script; 10] = [
    Script::Latin,
    Script::Cyrillic,
    Script::Greek,
    Script::Arabic,
    Script::Hebrew,
    Script::Han,
    Script::Hiragana,
    Script::Katakana,
    Script::Hangul,
    Script::Devanagari,
];

/// The count-array index [`classify_index`] reports for an alphabetic
/// scalar in none of [`SCRIPTS`]' blocks — one past the last real index, so
/// it shares one `Option<u8>` return with them without a second enum, and
/// so [`Script::from_index`] maps it to [`Script::Other`] by falling off
/// the end of [`SCRIPTS`].
const OTHER: u8 = 10;

/// [`Script::Latin`]'s index, named because the ASCII fast path bumps that
/// counter directly instead of going through [`classify_index`]
/// (`latin_index_matches_scripts` pins the two together).
const LATIN: usize = 0;

/// The block table [`Script::of`] documents, as `(first, last, index)`.
///
/// This is the **single source of truth** for which block belongs to which
/// script: [`classify_index`] reads it, and so does [`detect_script`]'s
/// cached-range shortcut. There is no second copy of these bounds anywhere.
///
/// The rows are disjoint (`script_ranges_are_disjoint`), which is what
/// makes "check the block that matched the previous scalar first" a pure
/// reordering — same answer, one comparison instead of a walk down the
/// table. Real text is overwhelmingly single-script, so that one-entry
/// cache hits nearly every time.
const SCRIPT_RANGES: [(u32, u32, u8); 17] = [
    (0x0041, 0x005A, 0), // Latin
    (0x0061, 0x007A, 0),
    (0x00C0, 0x02AF, 0),
    (0x1E00, 0x1EFF, 0),
    (0x0370, 0x03FF, 2), // Greek
    (0x1F00, 0x1FFF, 2),
    (0x0400, 0x052F, 1), // Cyrillic
    (0x0590, 0x05FF, 4), // Hebrew
    (0x0600, 0x06FF, 3), // Arabic
    (0x0750, 0x077F, 3),
    (0x0900, 0x097F, 9), // Devanagari
    (0x3040, 0x309F, 6), // Hiragana
    (0x30A0, 0x30FF, 7), // Katakana
    (0xAC00, 0xD7AF, 8), // Hangul
    (0x4E00, 0x9FFF, 5), // Han
    (0x3400, 0x4DBF, 5),
    (0xF900, 0xFAFF, 5),
];

/// [`Script::of`] as a [`SCRIPTS`] index (or [`OTHER`]), which is what the
/// counting loop actually needs.
///
/// The `Alphabetic` test comes first because it is the rule's first clause,
/// not merely because it is cheap: a scalar that is not a letter must not
/// reach the block table at all, or `×` would vote Latin.
#[inline]
fn classify_index(c: char) -> Option<u8> {
    if !c.is_alphabetic() {
        return None;
    }
    let cp = c as u32;
    match SCRIPT_RANGES
        .iter()
        .find(|&&(lo, hi, _)| cp >= lo && cp <= hi)
    {
        Some(&(_, _, index)) => Some(index),
        None => Some(OTHER),
    }
}

/// The dominant script in `input`, or `None` if it holds no letters at all
/// (empty input, or nothing but digits, punctuation, whitespace and
/// symbols).
///
/// # The unit, and what counts as a vote
///
/// **The text unit is one Unicode scalar value.** Each scalar votes for at
/// most one script, under [`Script::of`]'s rule: non-alphabetic scalars are
/// script-neutral and vote for nothing, and every alphabetic scalar votes,
/// for a named script or for [`Script::Other`]. No grapheme clustering
/// happens — a base letter and a following combining mark are two scalars,
/// and the mark votes only if it is itself alphabetic (Devanagari matras
/// are; the Japanese voiced-sound marks U+3099/U+309A are not).
///
/// The script with the most votes wins. [`Script::Other`] is counted
/// separately and needs a **strict** majority over every named script to
/// win, because it is not a script: it is the residual class, and two
/// letters in it may not even share a writing system. When a named script
/// and `Other` tie, the named script is the more specific — and more
/// useful — answer, so it takes it.
///
/// # Ties
///
/// **A tie between named scripts goes to whichever of the tied scripts the
/// text opens with.**
///
/// ```
/// use verbora_language::{Script, detect_script};
///
/// assert_eq!(detect_script("aЖ"), Some(Script::Latin));    // 1-1, Latin first
/// assert_eq!(detect_script("Жa"), Some(Script::Cyrillic)); // 1-1, Cyrillic first
/// assert_eq!(detect_script("aaaaaЖ"), Some(Script::Latin)); // 5-1, no tie at all
/// ```
///
/// This rule is a property of the text, which is the point. The obvious
/// alternative — break ties by a fixed order over the [`Script`] variants —
/// would make the answer depend on the order the enum happens to be
/// declared in, so adding a variant or sorting the list alphabetically
/// would silently change results for real input while no test that names a
/// script could see it coming. "The script the text opens with" cannot
/// drift that way, is total (no two scripts share a first occurrence), and
/// reads as a rule a caller can predict without consulting this crate's
/// source.
///
/// It is *not* a claim that the opening script is the true one. A tie is a
/// statement that the evidence is balanced; a caller who cannot act on a
/// coin-flip should count the scripts themselves rather than ask this
/// function to invent a preference.
///
/// # Determinism
///
/// Pure, allocation-free and deterministic: the same `&str` produces the
/// same answer on every call, every thread and every platform. No hashing,
/// no iteration-order dependence, no floating point.
///
/// # Cost
///
/// One pass, plus a second pass over the input only when a tie actually
/// occurs. ASCII bytes are counted straight off the byte slice with no
/// UTF-8 decoding — the only ASCII scalars that vote are `A`–`Z`/`a`–`z`,
/// so a byte test settles them — and non-ASCII runs are walked with one
/// `chars()` iterator per run, each scalar re-testing the block that
/// matched the previous one before falling back to a walk over the whole
/// block table.
#[must_use]
pub fn detect_script(input: &str) -> Option<Script> {
    let mut counts = [0u32; 10];
    let mut other = 0u32;
    // Any SCRIPT_RANGES row is a valid starting guess; Latin-1..IPA is the
    // one non-ASCII block most likely to appear in this crate's inputs.
    let mut cached_range = SCRIPT_RANGES[2];
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let byte = bytes[i];
        if byte < 0x80 {
            if byte.is_ascii_alphabetic() {
                counts[LATIN] += 1;
            }
            i += 1;
            continue;
        }
        // A non-ASCII run: `i` is a character boundary (every ASCII byte
        // consumed above is one whole character, and the loop below stops
        // on a whole-character boundary), so slicing here cannot panic.
        let mut consumed = 0usize;
        for c in input[i..].chars() {
            if c.is_ascii() {
                break;
            }
            consumed += c.len_utf8();
            if !c.is_alphabetic() {
                continue;
            }
            let cp = c as u32;
            let (lo, hi, index) = cached_range;
            if cp.wrapping_sub(lo) <= hi - lo {
                counts[index as usize] += 1;
                continue;
            }
            // Cache miss: find the scalar's block and re-arm the cache with
            // it. A letter in no block at all is Script::Other.
            match SCRIPT_RANGES
                .iter()
                .find(|&&(lo, hi, _)| cp >= lo && cp <= hi)
            {
                Some(&row) => {
                    cached_range = row;
                    counts[row.2 as usize] += 1;
                }
                None => other += 1,
            }
        }
        i += consumed;
    }

    let mut best_count = 0u32;
    let mut winner = 0usize;
    let mut tied = 0usize;
    for (index, &count) in counts.iter().enumerate() {
        if count > best_count {
            best_count = count;
            winner = index;
            tied = 1;
        } else if count == best_count {
            tied += 1;
        }
    }

    if best_count == 0 {
        return if other > 0 { Some(Script::Other) } else { None };
    }
    // Other is the residual class, not a script: it takes a strict
    // majority, and loses every tie.
    if other > best_count {
        return Some(Script::Other);
    }
    if tied > 1 {
        // A second pass, paid only on a genuine tie: the first scalar
        // belonging to any of the tied scripts decides. It always finds
        // one — every tied script has at least `best_count >= 1` scalars in
        // `input` — so `winner` keeps its lowest-index value only in the
        // impossible case, never as a silent fallback for real input.
        for c in input.chars() {
            match classify_index(c) {
                Some(index) if index != OTHER && counts[index as usize] == best_count => {
                    winner = index as usize;
                    break;
                }
                _ => {}
            }
        }
    }
    Some(Script::from_index(winner as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A literal transcription of the contract in [`detect_script`]'s own
    /// doc comment, written the obvious way: one `chars()` pass, one
    /// `Script::of` per scalar, then the documented tie rule. It is the
    /// parity oracle — the optimized [`detect_script`] is only allowed to
    /// be faster, never different — so it must stay a *transcription of the
    /// contract*, not a second design.
    fn reference_detect_script(input: &str) -> Option<Script> {
        let mut counts = [0u32; 10];
        let mut other = 0u32;

        for c in input.chars() {
            match Script::of(c) {
                Some(Script::Other) => other += 1,
                Some(s) => {
                    let index = SCRIPTS
                        .iter()
                        .position(|&x| x == s)
                        .expect("Script::of returns Other or a SCRIPTS member");
                    counts[index] += 1;
                }
                None => {}
            }
        }

        let best_count = counts.iter().copied().fold(0, u32::max);
        if best_count == 0 {
            return if other > 0 { Some(Script::Other) } else { None };
        }
        if other > best_count {
            return Some(Script::Other);
        }
        // "Whichever tied script the text opens with" — for a single
        // maximum this is that script's own first scalar, so one branch
        // covers both cases.
        for c in input.chars() {
            if let Some(s) = Script::of(c)
                && s != Script::Other
                && let Some(index) = SCRIPTS.iter().position(|&x| x == s)
                && counts[index] == best_count
            {
                return Some(s);
            }
        }
        unreachable!("a script with a positive count has a scalar in the input")
    }

    /// xorshift64* — a deterministic PRNG written out here so the
    /// randomized corpora below are reproducible from the seed alone,
    /// with no dev-dependency (this crate ships none for randomness).
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Characters drawn from every classification outcome the traversal
    /// has a separate path for: ASCII letters (the byte fast path), ASCII
    /// neutrals, each script's own block (including both halves of the
    /// multi-block scripts), non-ASCII neutrals (punctuation that ends a
    /// run without being counted), non-alphabetic scalars *inside* script
    /// blocks, `Script::Other` letters, and astral scalars.
    /// Written as a string (not a `[char]` array) so the whole alphabet
    /// stays readable in a few lines; the sampler below iterates its
    /// characters.
    const ALPHABET: &str = "aZq \t\n09.,!-\u{0}éüñßǎʃếộЖяїѐωΑἀאבݐकह।ひゟカヿ。、日本㐂﨟한국😀🎉\u{10FFFF}กᐃᚠ\u{2028}\u{00A0}·€×÷\u{0387}\u{30FB}\u{3099}";

    fn random_text(rng: &mut Rng, max_chars: usize) -> String {
        let alphabet: Vec<char> = ALPHABET.chars().collect();
        let len = rng.below(max_chars + 1);
        (0..len)
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect()
    }

    #[test]
    fn detect_script_matches_the_reference_implementation() {
        // Randomized differential parity: the optimized traversal (ASCII
        // byte runs + cached block) must agree with the straightforward
        // transcription of the contract on every input, including the ones
        // that stress its seams — script switches mid-run, ASCII embedded
        // in non-ASCII, ties.
        let mut rng = Rng(0x5EED_1234_ABCD_0001);
        for _ in 0..20_000 {
            let text = random_text(&mut rng, 24);
            assert_eq!(
                detect_script(&text),
                reference_detect_script(&text),
                "diverged on {text:?}"
            );
        }
        for _ in 0..2_000 {
            let text = random_text(&mut rng, 400);
            assert_eq!(
                detect_script(&text),
                reference_detect_script(&text),
                "diverged on a long input: {text:?}"
            );
        }
    }

    #[test]
    fn detect_script_matches_the_reference_on_fixtures_and_edge_cases() {
        // The hand-picked half of the parity check: real prose per script,
        // exact ties (where the tie rule is the only thing that decides the
        // answer), Other-vs-script majorities, and inputs that are all
        // boundary (empty, one character, ASCII/non-ASCII alternation, a
        // lone astral character).
        let ties = [
            "aЖ", "Жa", "abЖж", "日a", "a日", "한a", "אa", "क a", "ひカ", "カひ",
        ];
        let corpora = [
            "hello world",
            "café müller",
            "Приветствую вас, друзья",
            "こんにちは世界",
            "日本語テキスト",
            "中文文本",
            "हिन्दी में लिखा",
            "العربية نص",
            "עברית טקסט",
            "ελληνικά κείμενο",
            "한국어 문장",
            "Tiếng Việt có dấu",
            "ก ข ค ง",       // Script::Other letters
            "ก ข ค hello",   // Other vs Latin
            "ก ข ค ง hello", // Other majority
            "",
            " ",
            "a",
            "é",
            "😀",
            "\u{10FFFF}",
            "a😀b😀c",
            "aéaéaéaé",
            "12345 !!! ...",
            "\u{0}\u{0}\u{0}",
            "2 × 3 ÷ 4",
        ];
        for input in ties.iter().chain(&corpora) {
            assert_eq!(
                detect_script(input),
                reference_detect_script(input),
                "diverged on {input:?}"
            );
        }
    }

    #[test]
    fn detect_script_agrees_with_script_of_on_every_single_scalar() {
        // Enumeration, not a sample: for a one-scalar input the vote has
        // exactly one voter, so `detect_script` must answer precisely what
        // `Script::of` says — for all 1,112,064 scalars. This is the check
        // that keeps the byte fast path, the cached-block shortcut and the
        // documented per-scalar rule from drifting apart.
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let mut buf = [0u8; 4];
            let text: &str = c.encode_utf8(&mut buf);
            assert_eq!(
                detect_script(text),
                Script::of(c),
                "detect_script and Script::of disagree on U+{cp:04X}"
            );
        }
    }

    #[test]
    fn script_of_is_alphabetic_and_then_the_block_table() {
        // The documented rule, enumerated over all of Unicode against its
        // two clauses read literally: `Alphabetic` gates everything, and
        // the block table decides which script an alphabetic scalar votes
        // for. Catches a typo'd bound and a missing `is_alphabetic` alike.
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let expected = if !c.is_alphabetic() {
                None
            } else {
                Some(
                    SCRIPT_RANGES
                        .iter()
                        .find(|&&(lo, hi, _)| cp >= lo && cp <= hi)
                        .map_or(Script::Other, |&(_, _, index)| Script::from_index(index)),
                )
            };
            assert_eq!(Script::of(c), expected, "U+{cp:04X}");
        }
    }

    #[test]
    fn from_index_maps_other_and_every_real_index() {
        // OTHER is defined as "one past the last SCRIPTS index", and
        // `from_index` relies on that to answer Script::Other by falling
        // off the end rather than by a second match arm.
        assert_eq!(OTHER as usize, SCRIPTS.len());
        for (index, &script) in SCRIPTS.iter().enumerate() {
            assert_eq!(Script::from_index(index as u8), script);
        }
        assert_eq!(Script::from_index(OTHER), Script::Other);
        assert_eq!(Script::from_index(u8::MAX), Script::Other);
    }

    #[test]
    fn latin_index_matches_scripts() {
        // The ASCII fast path writes counts[LATIN] without consulting
        // `classify_index`, so LATIN has to be Latin's real slot — a
        // reordering of SCRIPTS would otherwise silently count every ASCII
        // letter as some other script.
        assert_eq!(SCRIPTS[LATIN], Script::Latin);
        assert_eq!(classify_index('a'), Some(LATIN as u8));
        assert_eq!(classify_index('Z'), Some(LATIN as u8));
    }

    #[test]
    fn script_ranges_are_disjoint() {
        // The cached-block shortcut is only a valid reordering because no
        // scalar is in two rows: if two rows overlapped, which one happened
        // to be cached would change the answer.
        for (i, &(lo_a, hi_a, _)) in SCRIPT_RANGES.iter().enumerate() {
            assert!(lo_a <= hi_a, "row {i} is inverted");
            for &(lo_b, hi_b, _) in &SCRIPT_RANGES[i + 1..] {
                assert!(
                    hi_a < lo_b || hi_b < lo_a,
                    "rows overlap: {lo_a:#X}..={hi_a:#X} and {lo_b:#X}..={hi_b:#X}"
                );
            }
        }
    }

    /// One alphabetic scalar per [`SCRIPTS`] entry, in index order.
    const SAMPLE_LETTERS: [char; 10] = ['a', 'б', 'γ', 'د', 'ש', '漢', 'ひ', 'カ', '한', 'क'];

    #[test]
    fn a_tie_goes_to_the_script_that_opens_the_text() {
        // Every ordered pair of the ten scripts, not a sample: 90 two-letter
        // inputs, each a perfect 1-1 tie, each of which must answer with the
        // script that comes first in the *text*. Enumerating both orders is
        // what makes this a test of the rule rather than of one example —
        // a fixed-order tiebreak passes half of these and fails the other
        // half.
        for (i, &first) in SAMPLE_LETTERS.iter().enumerate() {
            for (j, &second) in SAMPLE_LETTERS.iter().enumerate() {
                if i == j {
                    continue;
                }
                let text: String = [first, second].into_iter().collect();
                assert_eq!(
                    detect_script(&text),
                    Some(SCRIPTS[i]),
                    "{text:?}: a 1-1 tie must resolve to the opening script"
                );
            }
        }
    }

    #[test]
    fn a_deeper_tie_still_goes_to_the_opening_script() {
        // The rule is about the *first* scalar of a tied script, not about
        // one-character inputs: 3-3 ties, and a tie among three scripts at
        // once, resolve the same way.
        assert_eq!(detect_script("aaaббб"), Some(Script::Latin));
        assert_eq!(detect_script("бббaaa"), Some(Script::Cyrillic));
        assert_eq!(detect_script("бaγaбγ"), Some(Script::Cyrillic));
        // A losing script appearing first changes nothing: Greek has one
        // vote against Latin's two.
        assert_eq!(detect_script("γaa"), Some(Script::Latin));
    }

    #[test]
    fn other_never_wins_a_tie_and_needs_a_strict_majority() {
        // `Other` is the residual class, not a script (see `detect_script`).
        // 1 Thai letter vs 1 Latin letter is a tie it must lose, in either
        // text order; 2 vs 1 is the strict majority it needs to win.
        assert_eq!(detect_script("กa"), Some(Script::Latin));
        assert_eq!(detect_script("aก"), Some(Script::Latin));
        assert_eq!(detect_script("กขa"), Some(Script::Other));
        // Tied against two different scripts at once: still loses.
        assert_eq!(detect_script("กaб"), Some(Script::Latin));
    }

    #[test]
    fn non_alphabetic_scalars_inside_a_scripts_block_do_not_vote() {
        // The first clause of `Script::of`'s rule, on the scalars that make
        // it load-bearing: each of these sits inside a block this table
        // maps to a script, and none of them is a letter.
        for c in ['\u{00D7}', '\u{00F7}', '\u{0387}', '\u{30FB}', '\u{3099}'] {
            assert_eq!(
                Script::of(c),
                None,
                "U+{:04X} is not a letter and must not vote for a script",
                c as u32
            );
            assert_eq!(detect_script(&c.to_string()), None, "U+{:04X}", c as u32);
        }
        // And they cannot tip a vote either.
        assert_eq!(detect_script("×Ж"), Some(Script::Cyrillic));
    }

    #[test]
    fn latin_text() {
        assert_eq!(detect_script("hello world"), Some(Script::Latin));
        assert_eq!(detect_script("café müller"), Some(Script::Latin));
    }

    #[test]
    fn cyrillic_text() {
        assert_eq!(detect_script("Москва"), Some(Script::Cyrillic));
    }

    #[test]
    fn japanese_mixed_hiragana_and_han_picks_the_majority() {
        // "日本語" is 3 Han characters; a 1-character hiragana tiebreak
        // should not override a clear Han majority.
        assert_eq!(detect_script("日本語"), Some(Script::Han));
    }

    #[test]
    fn pure_hiragana() {
        assert_eq!(detect_script("ひらがな"), Some(Script::Hiragana));
    }

    #[test]
    fn pure_katakana() {
        assert_eq!(detect_script("カタカナ"), Some(Script::Katakana));
    }

    #[test]
    fn korean_text() {
        assert_eq!(detect_script("한국어"), Some(Script::Hangul));
    }

    #[test]
    fn arabic_text() {
        assert_eq!(detect_script("العربية"), Some(Script::Arabic));
    }

    #[test]
    fn hebrew_text() {
        assert_eq!(detect_script("עברית"), Some(Script::Hebrew));
    }

    #[test]
    fn devanagari_text() {
        assert_eq!(detect_script("हिन्दी"), Some(Script::Devanagari));
    }

    #[test]
    fn greek_text() {
        assert_eq!(detect_script("ελληνικά"), Some(Script::Greek));
    }

    #[test]
    fn empty_and_punctuation_only_are_none() {
        assert_eq!(detect_script(""), None);
        assert_eq!(detect_script("123 !@# ..."), None);
    }

    #[test]
    fn does_not_panic_on_astral_plane_or_emoji() {
        for input in ["😀😀😀", "\u{1F600}", "a😀b", "🎉🎊", &"😀".repeat(200)] {
            let _ = detect_script(input);
        }
    }

    #[test]
    fn mixed_script_picks_the_majority_not_the_first() {
        // 5 Latin vs 1 Cyrillic: Latin wins on count, so the tie rule never
        // runs — this specifically checks the *count*, not text order.
        assert_eq!(detect_script("aaaaaЖ"), Some(Script::Latin));
        assert_eq!(detect_script("Жaaaaa"), Some(Script::Latin));
    }

    #[test]
    fn detect_script_is_deterministic_across_repeated_calls() {
        // No hidden state, no hashing, no iteration-order dependence — the
        // same input must produce the exact same answer every time.
        for input in ["hello world", "日本語", "aaaaaЖ", "aЖ", "", "123 !@#"] {
            let first = detect_script(input);
            for _ in 0..5 {
                assert_eq!(
                    detect_script(input),
                    first,
                    "detect_script({input:?}) returned a different result across repeated calls"
                );
            }
        }
    }
}
