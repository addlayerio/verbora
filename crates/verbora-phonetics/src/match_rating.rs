//! Match Rating Approach (MRA) — the encoder *and* the genuine similarity test.
//!
//! The Match Rating Approach was developed at Western Airlines in 1977 by
//! Gwendolyn B. Moore, John L. Kuhns, Jeffrey L. Treffzs and Christian A.
//! Montgomery for indexing and comparing homophonous personal names
//! (*Accessing Individual Records from Personal Data Files Using Nonunique
//! Identifiers*, NBS Special Publication 500-2). It reached the Java
//! ecosystem as Apache commons-codec's `MatchRatingApproachEncoder`, and Rust
//! as `rphonetic`'s `MatchRatingApproach`.
//!
//! This is a **Verbora-native extension**: the JS reference the rest of this
//! crate ports has no MRA. Per the project's extension pattern, behavior is
//! pinned to a single canonical specification — **rphonetic 3.0.6**
//! (`src/match_rating_approach.rs`), the commons-codec lineage this encoder is
//! benchmarked against — and every decision is documented here. On rphonetic's
//! full accepted input domain (every input its code handles without
//! panicking), [`MatchRatingApproach::process`] and
//! [`MatchRatingApproach::compare`] are byte-identical / decision-identical to
//! rphonetic's `encode` and `is_encoded_equals`.
//!
//! # The algorithm
//!
//! **Encoding** ([`MatchRatingApproach::process`]):
//!
//! 1. Inputs whose trimmed form is empty or a single byte encode to `""`.
//! 2. Uppercase; drop `-`, `&`, `'`, `.`, `,` and all whitespace; fold the 60
//!    accented letters of the fixed table below to plain ASCII.
//! 3. Drop the vowels `A E I O U` everywhere except the first position.
//! 4. Collapse doubled ASCII consonants pairwise (`SS` → `S`).
//! 5. If more than six bytes remain, keep the first three and last three.
//!
//! **Comparison** ([`MatchRatingApproach::compare`]) is *not* code equality —
//! it is the published MRA match decision, exactly as rphonetic overrides
//! `is_encoded_equals`:
//!
//! 1. Either side trimmed-empty or trimmed to one byte → not a match.
//! 2. Raw string equality (before any encoding) → a match.
//! 3. Encode both. Encoded byte lengths differing by 3 or more → the
//!    comparison is "obsolete": not a match.
//! 4. A left-to-right pass and a right-to-left pass blank out agreeing
//!    characters (in-place, so earlier blanks feed later comparisons).
//! 5. The rating is `6 − max(unmatched bytes on either side)`; the names match
//!    when it reaches the minimum rating for the combined encoded length:
//!    `≤4 → 5`, `5–7 → 4`, `8–11 → 3`, `12 → 2`, `≥13 → 1`.
//!
//! # Behavioral decisions (all matching rphonetic 3.0.6)
//!
//! * **Byte lengths, not character counts.** The trimmed-length-one test, the
//!   6-byte truncation threshold, the length-difference-≥ 3 obsolescence test,
//!   the minimum-rating table input, and the unmatched-count are all in
//!   **bytes** (rphonetic uses `str::len` / `String::len` throughout, where
//!   commons-codec uses UTF-16 lengths). `"é"` trims to two bytes, so it
//!   encodes to `"E"` while `"e"` encodes to `""`.
//! * **The accent table is closed.** Only the 60 letters in rphonetic's
//!   `UNICODE` table fold; `ß` (uppercased to `SS` first), `Ø`, `Æ`, `Ā`,
//!   Cyrillic, CJK, emoji and digits all pass through untouched, are not
//!   vowels, and never collapse as doubles.
//! * **Double-consonant collapse is pairwise, not run-collapsing.** rphonetic
//!   applies one non-overlapping `replace("XX", "X")` per consonant, so a run
//!   of *n* becomes ⌈n/2⌉: `"BBB"` encodes to `"BB"`, not `"B"`.
//! * **Raw equality short-circuits `compare`** before encoding, so
//!   `compare("..", "..")` is `true` even though both sides encode to `""`.
//! * **Digits survive** into codes (`"1234567"` → `"123567"`) and are compared
//!   like any other character.
//!
//! # Divergences from rphonetic (excluded from the benchmark domain)
//!
//! rphonetic panics on two families of input; a text-processing library must
//! not, so this port substitutes defined behavior and documents it:
//!
//! 1. **Mid-character truncation.** When the cleaned string exceeds six bytes
//!    and byte offset `3` or `len − 3` falls inside a multi-byte character,
//!    rphonetic's `&value[0..3]` / `&value[len - 3..]` panic (e.g. `"Москва"`,
//!    `"😀😀"`, `"ABC日X"`). This port returns the cleaned string
//!    **untruncated** instead. Aligned non-ASCII input (e.g. `"日本語"` →
//!    `"日語"`) is unaffected and byte-identical.
//! 2. **Empty encodings inside `compare`.** An argument that trims to ≥ 2
//!    bytes of only removable characters (e.g. `".."`) encodes to `""`; the
//!    rating pass in rphonetic then computes `len() - 1` on an empty vec, so
//!    with overflow checks on (debug) it panics whenever **either** encoding
//!    is empty. In release the subtraction wraps and the outcome splits: when
//!    the *first* encoding is empty the blanking loop never runs and the
//!    rating is well defined, but when only the *second* is empty the loop
//!    indexes past it and panics (verified against rphonetic 3.0.6:
//!    `("..", "ab")` returns `false`, `("ab", "..")` panics). This port skips
//!    the blanking loop whenever either side is empty, which reproduces
//!    release rphonetic exactly wherever it does not panic and extends the
//!    same rule symmetrically: both-empty yields `true` (rating 6 ≥ minimum
//!    5), one-empty yields `6 − max(len)` rated against the table as usual —
//!    `compare("..", "ab")` and `compare("ab", "..")` are both `false`.

/// Match Rating Approach encoder and matcher, pinned to rphonetic 3.0.6.
///
/// [`process`](Self::process) produces the MRA personal-name code;
/// [`compare`](Self::compare) runs the full published MRA similarity
/// decision (not mere code equality).
///
/// ```
/// use verbora_phonetics::match_rating::MatchRatingApproach;
///
/// let mra = MatchRatingApproach::new();
/// assert_eq!(mra.process("Smith"), "SMTH");
/// assert_eq!(mra.process("Byrne"), "BYRN");
/// // A match:
/// assert!(mra.compare("Franciszek", "Frances"));
/// // Not a match:
/// assert!(!mra.compare("Karl", "Alessandro"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchRatingApproach;

/// Folds the 60 accented letters of rphonetic's `UNICODE`/`PLAIN_ASCII`
/// tables to plain ASCII; every other character is returned unchanged.
///
/// The table is deliberately closed: `ß`, `Ø`, `Æ`, `Ā` and the like are
/// *not* folded, matching rphonetic exactly.
const fn accent_fold(c: char) -> char {
    match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | '\u{0150}' => 'O',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | '\u{0151}' => 'o',
        'Ù' | 'Ú' | 'Û' | 'Ü' | '\u{0170}' => 'U',
        'ù' | 'ú' | 'û' | 'ü' | '\u{0171}' => 'u',
        'Ý' | '\u{0176}' | '\u{0178}' => 'Y',
        'ý' | '\u{0177}' | 'ÿ' => 'y',
        'Ñ' => 'N',
        'ñ' => 'n',
        'Ç' => 'C',
        'ç' => 'c',
        other => other,
    }
}

/// The characters rphonetic's `CHAR_TO_TRIM` removes (whitespace is removed
/// separately).
const fn is_trim_char(c: char) -> bool {
    matches!(c, '-' | '&' | '\'' | '.' | ',')
}

/// Uppercase ASCII consonants — exactly the 21 letters of rphonetic's
/// `DOUBLE_CONSONANT` table (includes `H`, `W`, `Y`).
const fn is_ascii_consonant(c: char) -> bool {
    matches!(
        c,
        'B' | 'C'
            | 'D'
            | 'F'
            | 'G'
            | 'H'
            | 'J'
            | 'K'
            | 'L'
            | 'M'
            | 'N'
            | 'P'
            | 'Q'
            | 'R'
            | 'S'
            | 'T'
            | 'V'
            | 'W'
            | 'X'
            | 'Y'
            | 'Z'
    )
}

/// rphonetic's `clean_name`, fused into one pass: filter punctuation and
/// whitespace, uppercase, fold accents.
///
/// rphonetic uppercases first and filters second; the order is
/// interchangeable because case mappings never produce (or consume)
/// punctuation or whitespace, and no uppercase expansion (`ß` → `SS`)
/// contains a filtered character.
fn clean_chars(token: &str) -> impl Iterator<Item = char> + '_ {
    token
        .chars()
        .filter(|&c| !c.is_whitespace() && !is_trim_char(c))
        .flat_map(char::to_uppercase)
        .map(accent_fold)
}

/// Streaming equivalent of rphonetic's `remove_double_consonants` (one
/// non-overlapping `replace("XX", "X")` per consonant).
///
/// A run of *n* equal consonants keeps ⌈n/2⌉ of them: `armed` marks an odd
/// occurrence still waiting for its pair, so `BBB` emits, skips, emits.
/// Non-consonants (vowels never reach this stage, but digits and non-ASCII
/// do) reset the state and are never skipped.
#[derive(Debug, Clone, Copy)]
struct PairCollapser {
    prev: char,
    armed: bool,
}

impl PairCollapser {
    const fn new() -> Self {
        Self {
            prev: '\0',
            armed: false,
        }
    }

    /// Returns `true` when `c` is the second half of a consonant pair and
    /// must be dropped. `armed` is only ever set for consonants, so no
    /// class check is needed on the skip path.
    fn skip(&mut self, c: char) -> bool {
        if self.armed && c == self.prev {
            self.armed = false;
            return true;
        }
        self.prev = c;
        self.armed = is_ascii_consonant(c);
        false
    }
}

/// rphonetic's `get_first3_last3`: byte-truncate to first three plus last
/// three when more than six bytes remain.
///
/// Where rphonetic would panic slicing through a multi-byte character (see
/// the module docs' divergence #1), the string is returned untruncated.
fn first3_last3(mut value: String) -> String {
    let len = value.len();
    if len > 6 && value.is_char_boundary(3) && value.is_char_boundary(len - 3) {
        value.drain(3..len - 3);
    }
    value
}

/// The minimum rating a name pair must reach, by combined encoded byte
/// length. Values from the 1977 paper, via commons-codec and rphonetic.
const fn minimum_rating(sum_length: usize) -> usize {
    match sum_length {
        0..=4 => 5,
        5..=7 => 4,
        8..=11 => 3,
        12 => 2,
        _ => 1,
    }
}

/// Unmatched bytes remaining in a blanked name: the byte length of every
/// character that was not blanked to `' '` (encoded names can never contain
/// a genuine space — whitespace is stripped during cleaning).
fn unmatched_len(name: &[char]) -> usize {
    name.iter()
        .filter(|&&c| c != ' ')
        .map(|c| c.len_utf8())
        .sum()
}

/// rphonetic's `left_to_right_then_right_to_left_processing`, ported
/// mutation-for-mutation: both passes run inside one loop, blanking agreeing
/// characters in place so earlier blanks feed later comparisons.
///
/// The one divergence (module docs, #2): when either name is empty the loop
/// is skipped instead of underflowing `len() - 1`.
fn rating_core(n1: &mut [char], n2: &mut [char]) -> usize {
    if !n1.is_empty() && !n2.is_empty() {
        let last1 = n1.len() - 1;
        let last2 = n2.len() - 1;
        for i in 0..n1.len() {
            if i > last2 {
                break;
            }
            if n1[i] == n2[i] {
                n1[i] = ' ';
                n2[i] = ' ';
            }
            if n1[last1 - i] == n2[last2 - i] {
                n1[last1 - i] = ' ';
                n2[last2 - i] = ' ';
            }
        }
    }
    6usize.abs_diff(unmatched_len(n1).max(unmatched_len(n2)))
}

/// Runs [`rating_core`] on stack buffers when both encoded names fit (they
/// always do on rphonetic's accepted domain: codes are at most six bytes),
/// falling back to heap vectors for the untruncated divergence path.
fn rating(name1: &str, name2: &str) -> usize {
    const STACK: usize = 8;
    // Byte length bounds character count, so `len() <= STACK` chars fit.
    if name1.len() <= STACK && name2.len() <= STACK {
        let mut b1 = ['\0'; STACK];
        let mut b2 = ['\0'; STACK];
        let n = fill(&mut b1, name1);
        let m = fill(&mut b2, name2);
        rating_core(&mut b1[..n], &mut b2[..m])
    } else {
        let mut v1: Vec<char> = name1.chars().collect();
        let mut v2: Vec<char> = name2.chars().collect();
        rating_core(&mut v1, &mut v2)
    }
}

/// Copies the characters of `s` into `buf`, returning how many were written.
/// Callers guarantee `s.len() <= buf.len()`.
fn fill<const N: usize>(buf: &mut [char; N], s: &str) -> usize {
    let mut n = 0;
    for c in s.chars() {
        buf[n] = c;
        n += 1;
    }
    n
}

impl MatchRatingApproach {
    /// Creates a Match Rating Approach encoder. It holds no state; the type
    /// exists to mirror rphonetic's `MatchRatingApproach`.
    ///
    /// ```
    /// use verbora_phonetics::match_rating::MatchRatingApproach;
    ///
    /// assert_eq!(MatchRatingApproach::new(), MatchRatingApproach::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as an MRA personal-name code.
    ///
    /// Inputs whose trimmed form is empty or a single **byte** encode to the
    /// empty string. See the module docs for the pipeline, the byte-length
    /// semantics, and the one divergence from rphonetic (mid-character
    /// truncation, where rphonetic panics and this returns the cleaned
    /// string untruncated).
    ///
    /// ```
    /// use verbora_phonetics::match_rating::MatchRatingApproach;
    ///
    /// let mra = MatchRatingApproach::new();
    /// assert_eq!(mra.process("HARPER"), "HRPR");
    /// assert_eq!(mra.process("Catherine"), "CTHRN");
    /// // First-3 + last-3 truncation past six bytes:
    /// assert_eq!(mra.process("Franciszek"), "FRNSZK");
    /// // Trimmed single byte:
    /// assert_eq!(mra.process("E"), "");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let trimmed = token.trim();
        if trimmed.is_empty() || trimmed.len() == 1 {
            return String::new();
        }

        let mut out = String::with_capacity(token.len());
        let mut collapser = PairCollapser::new();
        let mut first = true;
        for c in clean_chars(token) {
            if first {
                // The first cleaned character is kept even when it is a
                // vowel, and it seeds the double-consonant state (a fresh
                // collapser never skips).
                collapser.skip(c);
                out.push(c);
                first = false;
                continue;
            }
            if matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') {
                // Vowel removal runs before double-consonant collapse, so a
                // removed vowel does NOT reset the pair state: "SES" → "S".
                continue;
            }
            if !collapser.skip(c) {
                out.push(c);
            }
        }
        first3_last3(out)
    }

    /// The genuine MRA similarity decision — **not** code equality.
    ///
    /// Mirrors rphonetic's `is_encoded_equals` exactly: trimmed-empty or
    /// trimmed-single-byte arguments never match; *raw* equal strings match
    /// before any encoding; encoded lengths differing by three or more bytes
    /// make the comparison obsolete; otherwise both codes are blanked
    /// left-to-right and right-to-left and the residue is rated against the
    /// minimum-rating table. See the module docs for the empty-encoding
    /// divergence.
    ///
    /// ```
    /// use verbora_phonetics::match_rating::MatchRatingApproach;
    ///
    /// let mra = MatchRatingApproach::new();
    /// assert!(mra.compare("Burns", "Bourne"));
    /// assert!(mra.compare("smith", "smyth"));
    /// assert!(!mra.compare("Sean", "Pete"));
    /// // Same code ("SMTH" vs "SMTH") is necessary but not sufficient
    /// // elsewhere; here the decision is the full MRA rating.
    /// assert!(!mra.compare("Karl", "C")); // single byte never matches
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        let ta = a.trim();
        let tb = b.trim();
        if ta.is_empty() || tb.is_empty() {
            return false;
        }
        if ta.len() == 1 || tb.len() == 1 {
            return false;
        }
        // Raw equality, before encoding — rphonetic compares the original
        // arguments, so ".." == ".." matches even though both encode to "".
        if a == b {
            return true;
        }

        let name1 = self.process(a);
        let name2 = self.process(b);

        if name1.len().abs_diff(name2.len()) >= 3 {
            return false;
        }

        let minimum = minimum_rating(name1.len() + name2.len());
        rating(&name1, &name2) >= minimum
    }
}

impl verbora_core::Phonetic for MatchRatingApproach {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    /// Overridden: MRA's `compare` is the published similarity decision, not
    /// the trait's default key equality — mirroring rphonetic, whose
    /// `Encoder::is_encoded_equals` is likewise overridden.
    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mra() -> MatchRatingApproach {
        MatchRatingApproach::new()
    }

    /// Test-side wrapper over the production accent map, mirroring
    /// rphonetic's `remove_accent(String)`.
    fn fold(s: &str) -> String {
        s.chars().map(accent_fold).collect()
    }

    /// Test-side wrapper over the production pair collapser, mirroring
    /// rphonetic's `remove_double_consonants(String)` (fixture inputs are
    /// already uppercase, so the `to_uppercase` it performs is a no-op).
    fn collapse(s: &str) -> String {
        let mut state = PairCollapser::new();
        s.chars().filter(|&c| !state.skip(c)).collect()
    }

    // ------------------------------------------------------------------
    // Fixtures ported from rphonetic 3.0.6 `src/match_rating_approach.rs`
    // `mod tests` (itself derived from commons-codec's
    // MatchRatingApproachEncoderTest). Every fixture in that suite appears
    // below; helper-level fixtures run against the equivalent private
    // helper here, and two (`remove_vowels` on ALESSANDRA, `clean_name`)
    // are additionally pinned end-to-end because this port fuses those
    // stages into one scan.
    // ------------------------------------------------------------------

    #[test]
    fn accent_removal_matches_rphonetic_fixtures() {
        // test_accent_removal_all_lower_successfully_removed
        assert_eq!(fold("áéíóú"), "aeiou");
        // test_accent_removal_with_spaces_successfully_removed_and_spaces_invariant
        assert_eq!(fold("áé íó  ú"), "ae io  u");
        // test_accent_removal_upper_and_lower_successfully_removed_and_case_invariant
        assert_eq!(fold("ÁeíÓuu"), "AeiOuu");
        // test_accent_removal_mixed_with_unusual_chars_...
        assert_eq!(fold("Á-e'í.,ó&ú"), "A-e'i.,o&u");
        // test_accent_removal_ger_span_fren_mix_successfully_removed
        // (ß is not in the table and passes through)
        assert_eq!(fold("äëöüßÄËÖÜñÑà"), "aeoußAEOUnNa");
        // test_accent_removal_comprehensive_accent_mix_all_successfully_removed
        assert_eq!(
            fold("È,É,Ê,Ë,Û,Ù,Ï,Î,À,Â,Ô,è,é,ê,ë,û,ù,ï,î,à,â,ô,ç"),
            "E,E,E,E,U,U,I,I,A,A,O,e,e,e,e,u,u,i,i,a,a,o,c"
        );
        // test_accent_removal_normal_string_no_change
        assert_eq!(
            fold("Colorless green ideas sleep furiously"),
            "Colorless green ideas sleep furiously"
        );
        // test_accent_removal_nino_no_change
        assert_eq!(fold(""), "");
    }

    #[test]
    fn double_consonant_removal_matches_rphonetic_fixtures() {
        // test_remove_single_double_consonants_buble_removed_successfully
        assert_eq!(collapse("BUBBLE"), "BUBLE");
        // test_remove_double_consonants_mississippi_removed_successfully
        assert_eq!(collapse("MISSISSIPPI"), "MISISIPI");
        // test_remove_double_double_vowel_beetle_not_removed
        assert_eq!(collapse("BEETLE"), "BEETLE");
    }

    #[test]
    fn vowel_removal_matches_rphonetic_fixtures() {
        // test_remove_vowel_aidan_returns_adn — no doubles, so the encoding
        // equals the remove_vowels fixture directly.
        assert_eq!(mra().process("AIDAN"), "ADN");
        // test_remove_vowel_declan_returns_dcln — likewise.
        assert_eq!(mra().process("DECLAN"), "DCLN");
        // test_remove_vowel_alessandra_returns_alssndr — the fixture value
        // "ALSSNDR" is the intermediate BEFORE double-consonant collapse;
        // end-to-end the SS then collapses: ALSSNDR → ALSNDR (6 bytes, no
        // truncation).
        assert_eq!(mra().process("ALESSANDRA"), "ALSNDR");
    }

    #[test]
    fn first3_last3_matches_rphonetic_fixtures() {
        // test_get_first3_last3_alexander_returns_aleder
        assert_eq!(first3_last3("Alexzander".to_string()), "Aleder");
        // test_get_first3_last3_pete_returns_pete
        assert_eq!(first3_last3("PETE".to_string()), "PETE");
    }

    #[test]
    fn left_right_processing_matches_rphonetic_fixtures() {
        // test_left_to_right_then_right_to_left_alexander_alexandra_returns_4
        assert_eq!(rating("ALEXANDER", "ALEXANDRA"), 4);
        // test_left_to_right_then_right_to_left_einstein_michaela_returns_0
        assert_eq!(rating("EINSTEIN", "MICHAELA"), 0);
    }

    #[test]
    fn minimum_rating_matches_rphonetic_fixtures() {
        // test_get_min_rating_{1,2,5,6,7,8,10,13}_...
        assert_eq!(minimum_rating(1), 5);
        assert_eq!(minimum_rating(2), 5);
        assert_eq!(minimum_rating(5), 4);
        assert_eq!(minimum_rating(6), 4);
        assert_eq!(minimum_rating(7), 4);
        assert_eq!(minimum_rating(8), 3);
        assert_eq!(minimum_rating(10), 3);
        assert_eq!(minimum_rating(13), 1);
    }

    #[test]
    fn clean_name_matches_rphonetic_fixture() {
        // test_clean_name_successfully_clean
        let cleaned: String = clean_chars("This-ís   a t.,es &t").collect();
        assert_eq!(cleaned, "THISISATEST");
        // ...and pinned end-to-end: THISISATEST → vowels → THSSTST →
        // doubles → THSTST (6 bytes, no truncation).
        assert_eq!(mra().process("This-ís   a t.,es &t"), "THSTST");
    }

    #[test]
    fn encode_matches_rphonetic_fixtures() {
        let m = mra();
        // test_get_encoding_harper_hrpr
        assert_eq!(m.process("HARPER"), "HRPR");
        // test_get_encoding_smith_to_smth
        assert_eq!(m.process("Smith"), "SMTH");
        // test_get_encoding_smyth_to_smyth (Y is a consonant here)
        assert_eq!(m.process("Smyth"), "SMYTH");
        // test_get_encoding_space_to_nothing
        assert_eq!(m.process(" "), "");
        // test_get_encoding_no_space_to_nothing
        assert_eq!(m.process(""), "");
        // test_get_encoding_one_letter_to_nothing
        assert_eq!(m.process("E"), "");
    }

    #[test]
    fn compare_corner_cases_match_rphonetic_fixtures() {
        let m = mra();
        // test_is_encode_equals_corner_case_second_name_nothing_returns_false
        assert!(!m.compare("test", ""));
        // test_is_encode_equals_corner_case_first_name_nothing_returns_false
        assert!(!m.compare("", "test"));
        // test_is_encode_equals_corner_case_second_name_just_space_returns_false
        assert!(!m.compare("test", " "));
        // test_is_encode_equals_corner_case_first_name_just_space_returns_false
        assert!(!m.compare(" ", "test"));
        // test_is_encode_equals_corner_case_first_name_just_1_letter_returns_false
        assert!(!m.compare("t", "test"));
        // test_is_encode_equals_second_name_just_1_letter_returns_false
        assert!(!m.compare("test", "t"));
    }

    #[test]
    fn compare_matches_rphonetic_match_fixtures() {
        let m = mra();
        // test_compare_name_same_names_returns_false_successfully (sic — it
        // asserts a match, via the raw-equality shortcut)
        assert!(m.compare("John", "John"));
        assert!(m.compare("smith", "smyth"));
        assert!(m.compare("Burns", "Bourne"));
        assert!(m.compare("Catherine", "Kathryn"));
        assert!(m.compare("Brian", "Bryan"));
        assert!(m.compare("Séan", "Shaun"));
        assert!(m.compare("Cólm", "C-olín"));
        assert!(m.compare("Stephen", "Steven"));
        assert!(m.compare("Steven", "Stefan"));
        assert!(m.compare("Stephen", "Stefan"));
        assert!(m.compare("Sam", "Samuel"));
        assert!(m.compare("Micky", "Michael"));
        assert!(m.compare("Oona", "Oonagh"));
        assert!(m.compare("Sophie", "Sofia"));
        assert!(m.compare("Franciszek", "Frances"));
        assert!(m.compare("Tomasz", "tom"));
        // test_compare_small_input_cark_kl_successfully_matched
        assert!(m.compare("Kl", "Karl"));
        assert!(m.compare("Zach", "Zacharia"));
        assert!(m.compare("O'Sullivan", "Ó ' Súilleabháin"));
        assert!(m.compare("o'muireadhaigh", "Ó 'Muircheartaigh "));
        assert!(m.compare("Cooper-Flynn", "Super-Lyn"));
        assert!(m.compare("Hailey", "Halley"));
        assert!(m.compare("Auerbach", "Uhrbach"));
        assert!(m.compare("Moskowitz", "Moskovitz"));
        assert!(m.compare("LIPSHITZ", "LIPPSZYC"));
        assert!(m.compare("LEWINSKY", "LEVINSKI"));
        assert!(m.compare("SZLAMAWICZ", "SHLAMOVITZ"));
        assert!(m.compare("R o s o ch o w a c ie c", " R o s o k ho v a ts e ts"));
        assert!(m.compare(" P rz e m y s l", " P sh e m e sh i l"));
        assert!(m.compare("Peterson", "Peters"));
        assert!(m.compare("McGowan", "Mc Geoghegan"));
        assert!(m.compare("Sean", "John"));
    }

    #[test]
    fn compare_matches_rphonetic_non_match_fixtures() {
        let m = mra();
        // test_compare_short_names_al_ed_works_but_no_match
        assert!(!m.compare("Al", "Ed"));
        // test_compare_name_to_single_letter_karl_c_does_not_match
        assert!(!m.compare("Karl", "C"));
        // test_compare_karl_alessandro_does_not_match (length diff >= 3)
        assert!(!m.compare("Karl", "Alessandro"));
        // test_compare_forenames_una_oonagh_should_successfully_match_but_does_not
        assert!(!m.compare("Úna", "Oonagh"));
        // test_compare_long_surnames_moriarty_omuircheartaigh_does_not_successful_match
        assert!(!m.compare("Moriarty", "OMuircheartaigh"));
        // test_compare_surnames_corner_case_murphy_space_no_match
        assert!(!m.compare("Murphy", " "));
        // test_compare_surnames_corner_case_murphy_no_space_no_match
        assert!(!m.compare("Murphy", ""));
        // test_compare_surnames_murphy_lynch_no_match_expected
        assert!(!m.compare("Murphy", "Lynch"));
        // test_compare_forenames_sean_pete_no_match_expected
        assert!(!m.compare("Sean", "Pete"));
    }

    // ------------------------------------------------------------------
    // Hand-written edge cases and adversarial shapes (Verbora additions).
    // ------------------------------------------------------------------

    #[test]
    fn published_byrne_vectors() {
        // Canonical vectors from the 1977 paper / Wikipedia's MRA article.
        let m = mra();
        assert_eq!(m.process("Byrne"), "BYRN");
        assert_eq!(m.process("Boern"), "BRN");
        assert!(m.compare("Byrne", "Boern"));
    }

    #[test]
    fn trimmed_single_byte_versus_single_char() {
        let m = mra();
        // The one-letter test is in BYTES: "é" trims to two bytes and
        // encodes; "e" trims to one and does not. Matches rphonetic.
        assert_eq!(m.process("e"), "");
        assert_eq!(m.process("é"), "E");
        assert_eq!(m.process("É"), "E");
        assert_eq!(m.process("€"), "€");
        // Two raw chars, one of them removable, still encode: "a." → "A"…
        assert_eq!(m.process("a."), "A");
        // …while bare "a" does not.
        assert_eq!(m.process("a"), "");
        assert_eq!(m.process(" a "), "");
    }

    #[test]
    fn whitespace_and_punctuation_only_inputs() {
        let m = mra();
        assert_eq!(m.process("\t\n  "), "");
        assert_eq!(m.process(".."), "");
        assert_eq!(m.process("-&',."), "");
        // Non-removable punctuation passes straight through.
        assert_eq!(m.process("!?"), "!?");
    }

    #[test]
    fn digits_survive_into_codes() {
        let m = mra();
        assert_eq!(m.process("42"), "42");
        assert_eq!(m.process("1234567"), "123567");
        // Digit "doubles" never collapse — only the 21 consonants do.
        assert_eq!(m.process("1122"), "1122");
        assert_eq!(m.process("Ab1"), "AB1");
        assert!(m.compare("123456789", "123456780"));
        assert!(!m.compare("12", "345"));
    }

    #[test]
    fn mixed_case_input() {
        let m = mra();
        assert_eq!(m.process("sMiTh"), "SMTH");
        assert_eq!(m.process("HaRpEr"), m.process("harper"));
    }

    #[test]
    fn vowel_handling_quirks() {
        let m = mra();
        // The first cleaned character is kept even when it is a vowel.
        assert_eq!(m.process("AEIOU"), "A");
        assert_eq!(m.process("AA"), "A");
        // Removed vowels do not shield consonant pairs: BAAAB → BB → B.
        assert_eq!(m.process("BAAAB"), "B");
        assert_eq!(m.process("SES"), "S");
        // Accented vowels fold BEFORE vowel removal: Ídá → IDA → ID.
        assert_eq!(m.process("Ídá"), "ID");
    }

    #[test]
    fn double_consonant_runs_collapse_pairwise_not_fully() {
        // rphonetic's per-pair non-overlapping replace keeps ceil(n/2) of a
        // run of n — a documented quirk, reproduced exactly.
        let m = mra();
        assert_eq!(m.process("BB"), "B");
        assert_eq!(m.process("BBB"), "BB");
        assert_eq!(m.process("BBBB"), "BB");
        assert_eq!(m.process("BBBBB"), "BBB");
        assert_eq!(m.process("MISSISSIPPI"), "MSSP");
    }

    #[test]
    fn sharp_s_uppercases_before_folding() {
        // String uppercasing expands ß to SS, which then collapses:
        // straße → STRASSE → STRSS → STRS. Matches rphonetic.
        assert_eq!(mra().process("straße"), "STRS");
    }

    #[test]
    fn non_ascii_aligned_input_matches_rphonetic() {
        let m = mra();
        // 9 bytes, boundaries at 3 and 6 — rphonetic slices this fine.
        assert_eq!(m.process("日本語"), "日語");
        // 6 bytes — under the truncation threshold.
        assert_eq!(m.process("日本"), "日本");
        // Single emoji: trims to 4 bytes, passes through.
        assert_eq!(m.process("😀"), "😀");
        // Comparison of identical CJK via encoding (not raw equality).
        assert!(m.compare("日本語 ", "日本語"));
    }

    #[test]
    fn divergence_mid_char_truncation_returns_untruncated() {
        // rphonetic PANICS on all of these (byte offset 3 or len-3 falls
        // inside a multi-byte character); we return the cleaned string
        // untruncated. Documented divergence #1 — excluded from the
        // benchmark domain.
        let m = mra();
        assert_eq!(m.process("Москва"), "МОСКВА"); // 2-byte chars, offset 3 splits О
        assert_eq!(m.process("😀😀"), "😀😀"); // 4-byte chars, offset 3 splits
        assert_eq!(m.process("ABC日X"), "ABC日X"); // len-3 = 4 splits 日
        assert_eq!(m.process("ĀĀĀĀ"), "ĀĀĀĀ"); // 2-byte chars, offset 3 splits
    }

    #[test]
    fn divergence_empty_encodings_in_compare() {
        // Both sides encode to "": rphonetic panics in debug and returns
        // true in release; we always return true (rating 6 >= minimum 5).
        // Documented divergence #2.
        let m = mra();
        assert!(m.compare("..", ",,"));
        // First side encodes to "": release rphonetic returns false here
        // (its loop never runs) and we match it byte-for-byte; debug
        // rphonetic panics. Rating: 6 - 2 = 4 < minimum 5.
        assert!(!m.compare("..", "ab"));
        // Second side encodes to "": rphonetic panics in BOTH profiles;
        // we apply the same fully-unmatched rating symmetrically.
        assert!(!m.compare("ab", ".."));
    }

    #[test]
    fn raw_equality_short_circuits_before_encoding() {
        let m = mra();
        // Identical raw strings match even when both encode to "".
        assert!(m.compare("..", ".."));
        assert!(m.compare("!?", "!?"));
        assert!(m.compare("Al", "Al"));
        // Whitespace differences defeat the shortcut but not the rating.
        assert!(m.compare("  John  ", "John"));
    }

    #[test]
    fn obsolete_comparison_length_difference_of_three() {
        let m = mra();
        // "KRL" (3) vs "ALSNDR" (6): diff exactly 3 → obsolete → false,
        // even though shorter pairs of these letters could rate.
        assert_eq!(m.process("Karl"), "KRL");
        assert_eq!(m.process("Alessandro"), "ALSNDR");
        assert!(!m.compare("Karl", "Alessandro"));
        // Diff of 2 still compares: "Kl" vs "Karl" matches (fixture above).
        assert!(m.compare("Oona", "Oonagh")); // 2 vs 4
    }

    #[test]
    fn very_long_input() {
        let m = mra();
        // 1000 Bs → pairwise collapse to 500 → first3+last3.
        assert_eq!(m.process(&"B".repeat(1000)), "BBBBBB");
        // Vowels beyond the first all drop regardless of length.
        assert_eq!(m.process(&"a".repeat(1000)), "A");
        assert_eq!(
            m.process("supercalifragilisticexpialidocious"),
            // SPRCLFRGLSTCXPLDCS → first 3 + last 3
            "SPRDCS"
        );
    }

    #[test]
    fn rating_internals_on_empty_and_uneven_names() {
        // The divergence path, pinned at the helper level.
        assert_eq!(rating("", ""), 6);
        assert_eq!(rating("", "AB"), 4);
        assert_eq!(rating("AB", ""), 4);
        // Multi-byte characters count their UTF-8 length, as rphonetic's
        // String::len does.
        assert_eq!(rating("日X", "日Y"), 6usize.abs_diff(1));
        assert_eq!(rating("日X", "本Y"), 6usize.abs_diff(4));
    }

    /// The rating buffers are `[char; 8]` selected by *byte* length: names of
    /// exactly 8 bytes (the boundary) must take the stack path without
    /// overflow, and longer untruncated divergence-path names must fall back
    /// to the heap. Both paths ride through `compare` here.
    #[test]
    fn rating_stack_boundary_and_heap_fallback() {
        let m = mra();
        // "Моск" cleans to "МОСК": 8 bytes (the exact stack capacity), 4
        // chars, untruncated because byte 3 splits О (divergence #1).
        assert_eq!(m.process("Моск"), "МОСК");
        assert_eq!(m.process("Моск").len(), 8);
        // Raw strings differ (leading space) so the equality shortcut does
        // not fire; both encode to the same 8-byte name; the stack-path
        // rating blanks everything: a match. rphonetic panics on this pair.
        assert!(m.compare("Моск", " Моск"));
        // 8-byte versus 6-byte name ("ĀĀĀA" drops its trailing vowel A):
        // rating leaves one Ā (2 bytes) unmatched, 6 - 2 = 4 >= minimum 1.
        assert_eq!(m.process("ĀĀĀA"), "ĀĀĀ");
        assert!(m.compare("ĀĀĀĀ", "ĀĀĀA"));
        // 12-byte untruncated names take the heap fallback.
        assert_eq!(m.process("Москва").len(), 12);
        assert!(m.compare("Москва", " Москва"));
        // Stack path meets heap path: 8 vs 12 bytes differ by >= 3 — the
        // obsolescence rule fires before any rating buffer is touched.
        assert!(!m.compare("Моск", "Москва"));
    }

    #[test]
    fn phonetic_trait_delegates_to_inherent_methods() {
        fn through_trait<P: verbora_core::Phonetic>(p: &P) -> (String, bool, bool) {
            (
                p.process("Smith"),
                p.compare("Burns", "Bourne"),
                p.compare("Murphy", "Lynch"),
            )
        }
        let (code, matched, unmatched) = through_trait(&MatchRatingApproach::new());
        assert_eq!(code, "SMTH");
        assert!(matched);
        assert!(!unmatched);
    }
}
