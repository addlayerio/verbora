//! Match Rating Approach (Moore et al., 1977).

/// Match Rating Approach — a name code **and** a similarity decision.
///
/// # Publication
///
/// Gwendolyn B. Moore, John L. Kuhns, Jeffrey L. Treffzs and Christian A.
/// Montgomery, *Accessing Individual Records from Personal Data Files Using
/// Nonunique Identifiers*, NBS Special Publication 500-2, U.S. National Bureau
/// of Standards, 1977 — developed at Western Airlines for indexing and
/// comparing homophonous personal names.
///
/// MRA is unusual among the encoders here in that the publication defines a
/// *comparison*, not just a key: two names match when their codes agree
/// closely enough for their combined length. [`compare`](Self::compare) is
/// that decision, which is why it is **not** code equality.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Accented Latin letters are
///   folded to plain ASCII by the closed table below, and every scalar that
///   is still not `A`–`Z` afterwards — digits, punctuation, whitespace,
///   Cyrillic, CJK, emoji, `Ø`, `Æ` — is skipped. Every code is therefore
///   pure ASCII, and every length the algorithm measures is a character count
///   and a byte count at once.
/// * A name whose *scalars*, after trimming, number zero or one encodes to
///   `""` and never matches anything. That gate is the publication's ("a
///   single initial is not a name"), applied to characters rather than bytes
///   so that `"é"` and `"e"` are treated alike.
/// * **Total**: no input panics, and there is no error type.
///
/// # Encoding
///
/// 1. Uppercase; fold the accented letters; drop everything that is not
///    `A`–`Z`.
/// 2. Drop the vowels `A E I O U` everywhere except the first position.
/// 3. Collapse doubled consonants **pairwise**, not by run: a run of *n*
///    keeps ⌈n/2⌉, so `BBB` encodes `BB`. Vowel removal happens first and
///    does not reset the pair state, so `SES` encodes `S`.
/// 4. If more than six characters remain, keep the first three and the last
///    three.
///
/// # Comparison
///
/// 1. Either side trimming to fewer than two scalars → not a match.
/// 2. Raw string equality, before any encoding → a match.
/// 3. Encode both. Code lengths differing by three or more → the comparison
///    is "obsolete": not a match.
/// 4. A left-to-right pass and a right-to-left pass blank out agreeing
///    characters, in place, so earlier blanks feed later comparisons.
/// 5. The rating is `6 − max(unmatched characters on either side)`; the names
///    match when it reaches the minimum for the combined code length:
///    `≤4 → 5`, `5–7 → 4`, `8–11 → 3`, `12 → 2`, `≥13 → 1`.
///
/// ```
/// use verbora_phonetics::MatchRatingApproach;
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

/// Folds the 60 accented letters of Commons Codec's `UNICODE`/`PLAIN_ASCII`
/// tables to plain ASCII; every other character is returned unchanged.
///
/// The table is deliberately closed: `ß`, `Ø`, `Æ`, `Ā` and the like are
/// *not* folded, matching Commons Codec exactly.
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

/// The characters Commons Codec's `CHAR_TO_TRIM` removes (whitespace is removed
/// separately).
const fn is_trim_char(c: char) -> bool {
    matches!(c, '-' | '&' | '\'' | '.' | ',')
}

/// Uppercase ASCII consonants — exactly the 21 letters of Commons Codec's
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

/// Commons Codec's `clean_name`, fused into one pass: filter punctuation and
/// whitespace, uppercase, fold accents.
///
/// Commons Codec uppercases first and filters second; the order is
/// interchangeable because case mappings never produce (or consume)
/// punctuation or whitespace, and no uppercase expansion (`ß` → `SS`)
/// contains a filtered character.
fn clean_chars(token: &str) -> impl Iterator<Item = char> + '_ {
    token
        .chars()
        .filter(|&c| !c.is_whitespace() && !is_trim_char(c))
        .flat_map(char::to_uppercase)
        .map(accent_fold)
        // After folding, anything still outside A-Z has no MRA rule and is
        // dropped. This is what keeps every code pure ASCII, and therefore
        // every length the algorithm measures a character count.
        .filter(char::is_ascii_uppercase)
}

/// Streaming equivalent of Commons Codec's `remove_double_consonants` (one
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

/// Commons Codec's `get_first3_last3`: byte-truncate to first three plus last
/// three when more than six bytes remain.
///
/// Where Commons Codec would panic slicing through a multi-byte character (see
/// the module docs' divergence #1), the string is returned untruncated.
fn first3_last3(mut value: String) -> String {
    let len = value.len();
    if len > 6 && value.is_char_boundary(3) && value.is_char_boundary(len - 3) {
        value.drain(3..len - 3);
    }
    value
}

/// The minimum rating a name pair must reach, by combined encoded byte
/// length. Values from the 1977 paper, via commons-codec and Commons Codec.
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

/// Commons Codec's `left_to_right_then_right_to_left_processing`, ported
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
/// always do on Commons Codec's accepted domain: codes are at most six bytes),
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
    /// exists to mirror Commons Codec's `MatchRatingApproach`.
    ///
    /// ```
    /// use verbora_phonetics::MatchRatingApproach;
    ///
    /// assert_eq!(MatchRatingApproach::new(), MatchRatingApproach::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as an MRA personal-name code.
    ///
    /// A token trimming to fewer than two scalars encodes to `""`. See the
    /// [type documentation](Self) for the pipeline.
    ///
    /// ```
    /// use verbora_phonetics::MatchRatingApproach;
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
        // Scalars, not bytes: "a single initial is not a name" is a claim
        // about characters.
        if token.trim().chars().nth(1).is_none() {
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

    /// The published MRA similarity decision — **not** code equality.
    ///
    /// See the [type documentation](Self) for the five steps.
    ///
    /// ```
    /// use verbora_phonetics::MatchRatingApproach;
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
        if a.trim().chars().nth(1).is_none() || b.trim().chars().nth(1).is_none() {
            return false;
        }
        // Raw equality, before encoding — Commons Codec compares the original
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
    /// the trait's default key equality.
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
    /// Commons Codec's `remove_accent(String)`.
    fn fold(s: &str) -> String {
        s.chars().map(accent_fold).collect()
    }

    /// Test-side wrapper over the production pair collapser, mirroring
    /// Commons Codec's `remove_double_consonants(String)` (fixture inputs are
    /// already uppercase, so the `to_uppercase` it performs is a no-op).
    fn collapse(s: &str) -> String {
        let mut state = PairCollapser::new();
        s.chars().filter(|&c| !state.skip(c)).collect()
    }

    // ------------------------------------------------------------------
    // Fixtures ported from Apache Commons Codec `src/match_rating_approach.rs`
    // `mod tests` (itself derived from commons-codec's
    // MatchRatingApproachEncoderTest). Every fixture in that suite appears
    // below; helper-level fixtures run against the equivalent private
    // helper here, and two (`remove_vowels` on ALESSANDRA, `clean_name`)
    // are additionally pinned end-to-end because this port fuses those
    // stages into one scan.
    // ------------------------------------------------------------------

    #[test]
    fn accent_removal_matches_commons_codec_fixtures() {
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
    fn double_consonant_removal_matches_commons_codec_fixtures() {
        // test_remove_single_double_consonants_buble_removed_successfully
        assert_eq!(collapse("BUBBLE"), "BUBLE");
        // test_remove_double_consonants_mississippi_removed_successfully
        assert_eq!(collapse("MISSISSIPPI"), "MISISIPI");
        // test_remove_double_double_vowel_beetle_not_removed
        assert_eq!(collapse("BEETLE"), "BEETLE");
    }

    #[test]
    fn vowel_removal_matches_commons_codec_fixtures() {
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
    fn first3_last3_matches_commons_codec_fixtures() {
        // test_get_first3_last3_alexander_returns_aleder
        assert_eq!(first3_last3("Alexzander".to_string()), "Aleder");
        // test_get_first3_last3_pete_returns_pete
        assert_eq!(first3_last3("PETE".to_string()), "PETE");
    }

    #[test]
    fn left_right_processing_matches_commons_codec_fixtures() {
        // test_left_to_right_then_right_to_left_alexander_alexandra_returns_4
        assert_eq!(rating("ALEXANDER", "ALEXANDRA"), 4);
        // test_left_to_right_then_right_to_left_einstein_michaela_returns_0
        assert_eq!(rating("EINSTEIN", "MICHAELA"), 0);
    }

    #[test]
    fn minimum_rating_matches_commons_codec_fixtures() {
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
    fn clean_name_matches_commons_codec_fixture() {
        // test_clean_name_successfully_clean
        let cleaned: String = clean_chars("This-ís   a t.,es &t").collect();
        assert_eq!(cleaned, "THISISATEST");
        // ...and pinned end-to-end: THISISATEST → vowels → THSSTST →
        // doubles → THSTST (6 bytes, no truncation).
        assert_eq!(mra().process("This-ís   a t.,es &t"), "THSTST");
    }

    #[test]
    fn encode_matches_commons_codec_fixtures() {
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
    fn compare_corner_cases_match_commons_codec_fixtures() {
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
    fn compare_matches_commons_codec_match_fixtures() {
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
    fn compare_matches_commons_codec_non_match_fixtures() {
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
    /// The "a single initial is not a name" gate counts scalars, not bytes,
    /// so an accented single letter is treated exactly like a plain one.
    #[test]
    fn a_single_scalar_is_not_a_name() {
        let m = mra();
        assert_eq!(m.process("e"), "");
        assert_eq!(m.process("\u{e9}"), "");
        assert_eq!(m.process("\u{c9}"), "");
        assert_eq!(m.process("\u{20ac}"), "");
        assert_eq!(m.process("\u{1F600}"), "");
        // Two raw scalars, one of them removable, still pass the gate.
        assert_eq!(m.process("a."), "A");
        assert_eq!(m.process("a"), "");
        assert_eq!(m.process(" a "), "");
        // ... and never match anything.
        assert!(!m.compare("e", "e"));
        assert!(!m.compare("\u{e9}", "\u{e9}"));
    }
    #[test]
    fn whitespace_and_punctuation_only_inputs() {
        let m = mra();
        assert_eq!(m.process("\t\n  "), "");
        assert_eq!(m.process(".."), "");
        assert_eq!(m.process("-&',."), "");
        // Non-removable punctuation is not an A-Z letter either.
        assert_eq!(m.process("!?"), "");
    }
    /// A digit is not a letter, and MRA is a *name* algorithm: digits are
    /// dropped like every other non-`A`-`Z` scalar.
    #[test]
    fn digits_are_dropped() {
        let m = mra();
        assert_eq!(m.process("42"), "");
        assert_eq!(m.process("1234567"), "");
        assert_eq!(m.process("Ab1"), m.process("Ab"));
        assert_eq!(m.process("O'Brien"), m.process("OBrien"));
        assert_eq!(m.process("Jean-Luc"), m.process("JeanLuc"));
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
        // Commons Codec's per-pair non-overlapping replace keeps ceil(n/2) of a
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
        // straße → STRASSE → STRSS → STRS. Matches Commons Codec.
        assert_eq!(mra().process("straße"), "STRS");
    }
    /// The text unit: after the accent fold, a scalar that is still not
    /// `A`-`Z` is dropped, so every code is pure ASCII and the first-3/last-3
    /// cut can never land inside a character.
    #[test]
    fn every_code_is_ascii() {
        let m = mra();
        for input in [
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{41c}\u{43e}\u{441}\u{43a}\u{432}\u{430}",
            "\u{1F600}\u{1F600}",
            "ABC\u{65e5}X",
            "\u{100}\u{100}\u{100}\u{100}",
            "Franciszek",
            "caf\u{e9}",
        ] {
            let code = m.process(input);
            assert!(code.is_ascii(), "{input:?} -> {code:?}");
            assert!(code.len() <= 6, "{input:?} -> {code:?}");
        }
        // A name written only in a non-Latin script has no MRA code.
        assert_eq!(m.process("\u{65e5}\u{672c}\u{8a9e}"), "");
        assert_eq!(m.process("\u{41c}\u{43e}\u{441}\u{43a}\u{432}\u{430}"), "");
        // Accented Latin folds rather than disappearing.
        assert_eq!(m.process("caf\u{e9}"), m.process("cafe"));
        assert_eq!(m.process("Fran\u{e7}ois"), m.process("Francois"));
    }
    /// Two names that both encode to `""` (all their scalars were removable)
    /// have nothing to disagree about: the rating is 6, which clears every
    /// minimum, so they match. The blanking pass is skipped rather than
    /// indexing an empty buffer.
    #[test]
    fn empty_encodings_in_compare() {
        let m = mra();
        assert!(m.compare("..", ",,"));
        // First side encodes to "": release Commons Codec returns false here
        // (its loop never runs) and we match it byte-for-byte; debug
        // Commons Codec panics. Rating: 6 - 2 = 4 < minimum 5.
        assert!(!m.compare("..", "ab"));
        // Second side encodes to "": Commons Codec panics in BOTH profiles;
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
        // Multi-byte characters count their UTF-8 length, as Commons Codec's
        // String::len does.
        assert_eq!(rating("日X", "日Y"), 6usize.abs_diff(1));
        assert_eq!(rating("日X", "本Y"), 6usize.abs_diff(4));
    }

    /// The rating buffers are `[char; 8]` with a heap fallback beyond that.
    /// Every code is ASCII, so the boundary is a plain character count: an
    /// exactly-6-character code pair must take the stack path and a pair of
    /// longer *raw* names must still rate correctly after truncation.
    #[test]
    fn rating_stack_boundary_and_heap_fallback() {
        let m = mra();
        assert_eq!(m.process("Franciszek"), "FRNSZK");
        assert_eq!(m.process("Franciszek").len(), 6);
        assert!(m.compare("Franciszek", "Frances"));
        // Codes of 6 and 4: the length gap is 2, under the obsolescence
        // threshold of 3, so the rating actually runs.
        assert_eq!(m.process("Smith"), "SMTH");
        assert!(m.compare("Smith", "Smyth"));
        // A long input still truncates to six, so the stack path always
        // suffices for MRA codes.
        assert!(m.process(&"abcdefghij".repeat(50)).len() <= 6);
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
