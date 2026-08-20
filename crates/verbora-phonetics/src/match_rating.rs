//! Match Rating Approach (Moore, Kuhns, Treffzs and Montgomery, 1977).

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
/// That publication states both halves of this type: three encoding rules
/// (rules 2-4 below) and a six-step comparison whose minimum-rating table is
/// printed in the paper. Everything else — the normalisation rule 1 performs,
/// which the paper presupposes by defining MRA over `A`-`Z`; how accented
/// letters reach that alphabet; what an empty code compares as; how the two
/// comparison passes interleave — is a Verbora specification decision, marked
/// as such where it appears.
///
/// MRA is unusual among the encoders here in that the publication defines a
/// *comparison*, not just a key: two names match when their codes agree
/// closely enough for their combined length. [`compare`](Self::compare) is
/// that decision, which is why it is **not** code equality.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Accented Latin letters are
///   folded to plain ASCII by the closed table this module documents, and
///   every scalar that is still not `A`–`Z` afterwards — digits, punctuation,
///   whitespace, Cyrillic, CJK, emoji, `Ø`, `Æ` — is skipped. Every code is
///   therefore pure ASCII, and every length the algorithm measures is a
///   character count and a byte count at once.
/// * A name whose *scalars*, after trimming whitespace, number zero or one
///   encodes to `""` and never matches anything. That gate is the
///   publication's ("a single initial is not a name"), applied to characters
///   rather than bytes so that `"é"` and `"e"` are treated alike.
/// * **Total**: no input panics, and there is no error type.
///
/// # Encoding
///
/// Rule 1 is the normalisation the publication presupposes; rules 2-4 are the
/// three it states, in the order it states them.
///
/// 1. Uppercase; fold the accented letters; drop everything that is not
///    `A`–`Z`.
/// 2. Delete the vowels `A E I O U` everywhere except the first position.
/// 3. Remove the second letter of every doubled consonant. The rule is
///    *pairwise and non-overlapping*, so a run of *n* keeps ⌈n/2⌉ and `BBB`
///    encodes `BB`. Vowel deletion happens first and its output is what rule 3
///    reads, so `SES` encodes `S`.
/// 4. If more than six letters remain, keep the first three and the last
///    three.
///
/// # Comparison
///
/// 1. Either side trimming to fewer than two scalars → not a match.
/// 2. Encode both. Code lengths differing by three or more → the comparison
///    is "obsolete": not a match.
/// 3. A left-to-right pass and a right-to-left pass blank out agreeing
///    characters, in place, so earlier blanks feed later comparisons.
/// 4. The rating is `6 −` the number of characters left unblanked in the
///    longer code; the names match when it reaches the minimum for the
///    combined code length: `≤4 → 5`, `5–7 → 4`, `8–11 → 3`, `12 → 2`,
///    `≥13 → 1`.
///
/// **Where the publication is ambiguous, and how Verbora resolves it.** The
/// paper gives the two passes as separate steps — "process the encoded strings
/// from left to right…", then "process the unmatched characters from right to
/// left…". Verbora specifies them as *interleaved*: one left-to-right
/// comparison and one right-to-left comparison per index, in that order, both
/// blanking in place. The two readings are not equivalent — `compare("BBBBB",
/// "CCCBBB")` is a match under the interleaved reading (rating 4, minimum 4)
/// and not a match under a strictly sequential one (rating 3) — so the choice
/// is stated here and pinned by test rather than left to the implementation.
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

/// Folds sixty accented Latin letters to their plain ASCII base; every other
/// scalar is returned unchanged.
///
/// **This table is Verbora's, not the paper's.** Moore et al. define MRA over
/// `A`–`Z` and say nothing about accented input, so an implementation must
/// choose. Dropping `é` outright would make `Séan` and `Sean` encode
/// differently, which defeats the purpose of a homophone key, so Verbora folds
/// instead. The sixty entries are the ones an MRA implementation conventionally
/// folds — the set distributed with Apache Commons Codec — adopted here as a
/// closed, enumerated specification rather than consulted as an oracle:
/// `accent_table_folds_exactly_sixty_letters` walks every scalar from `U+0000`
/// to `U+024F` and requires the table to be the identity outside those sixty.
///
/// Closed means closed: `ß`, `Ø`, `Æ`, `Ā`, `Ł` are *not* folded, and are
/// therefore dropped by the `A`–`Z` filter like any other non-letter.
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

/// The punctuation that actually turns up inside written personal names:
/// hyphens, ampersands, apostrophes, periods and commas.
///
/// This is an early reject, not a rule the encoding depends on. Rule 1 keeps
/// only `A`–`Z`, and none of these characters — nor any whitespace — can
/// become an `A`–`Z` letter under `to_uppercase` or the accent fold, so
/// removing them here changes nothing except how much work the rest of the
/// pass does. `punctuation_is_dropped_whether_or_not_it_is_trimmed_early`
/// pins that equivalence.
const fn is_trim_char(c: char) -> bool {
    matches!(c, '-' | '&' | '\'' | '.' | ',')
}

/// The twenty-one uppercase ASCII consonants rule 3 can double, `A E I O U`
/// being the vowels rule 2 has already removed. `H`, `W` and `Y` count as
/// consonants here: MRA never treats them as vowels.
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

/// Rule 1, fused into one pass: drop punctuation and whitespace, uppercase,
/// fold accents, keep only `A`–`Z`.
///
/// Uppercasing before folding is deliberate and observable: `Í` and `í` must
/// reach the fold table in the same case, and `ß` uppercases to `SS` — two
/// letters that rule 3 then sees as a doubled consonant, so `straße` encodes
/// `STRS`.
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

/// Rule 3 as a stream: remove the second letter of every doubled consonant,
/// scanning left to right and never reusing a letter that has already been
/// consumed as the second half of a pair.
///
/// A run of *n* equal consonants therefore keeps ⌈n/2⌉ of them — `armed` marks
/// an odd occurrence still waiting for its pair, so `BBB` emits, skips, emits.
/// Non-consonants reset the state and are never skipped; the only one that can
/// reach this stage is a leading vowel, which rule 2 keeps.
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

/// Rule 4: past six letters, keep the first three and the last three.
///
/// The cut is expressed in bytes because rules 1–3 can only have produced
/// `A`–`Z`, where a byte is a letter. The boundary check is what makes the
/// helper total rather than panicking, and is unreachable through
/// [`MatchRatingApproach::process`]; `first3_last3_is_unreachable_on_non_ascii`
/// states that reachability argument as a test.
fn first3_last3(mut value: String) -> String {
    let len = value.len();
    if len > 6 && value.is_char_boundary(3) && value.is_char_boundary(len - 3) {
        value.drain(3..len - 3);
    }
    value
}

/// The minimum rating a name pair must reach, by combined code length — the
/// table printed in Moore et al. (1977).
const fn minimum_rating(sum_length: usize) -> usize {
    match sum_length {
        0..=4 => 5,
        5..=7 => 4,
        8..=11 => 3,
        12 => 2,
        _ => 1,
    }
}

/// Characters left unblanked in a name after the two passes. Codes contain
/// only `A`–`Z`, so a blanked position is exactly a `' '`.
fn unmatched_len(name: &[char]) -> usize {
    name.iter().filter(|&&c| c != ' ').count()
}

/// Comparison steps 3 and 4: the left-to-right and right-to-left passes,
/// interleaved one index at a time (see [`MatchRatingApproach`] on why that
/// reading, and what the alternative would decide differently), blanking
/// agreeing characters in place so earlier blanks feed later comparisons.
///
/// Blanking always writes both sides at once, and a code can never contain a
/// genuine `' '`, so the two sides always lose the same number of characters:
/// `unmatched(n1) - unmatched(n2) == len(n1) - len(n2)`. Taking the larger of
/// the two counts is therefore literally the paper's "unmatched characters in
/// the longer string", not an approximation of it.
///
/// A rating cannot go below zero. On the reachable domain it never would —
/// codes are at most six characters, so at most six can go unmatched — but
/// saturating rather than wrapping keeps the helper total on any input, and
/// keeps a hopeless comparison from folding back into a passing score.
fn rating_core(n1: &mut [char], n2: &mut [char]) -> usize {
    if !n1.is_empty() && !n2.is_empty() {
        let last1 = n1.len() - 1;
        let last2 = n2.len() - 1;
        for i in 0..n1.len().min(n2.len()) {
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
    6usize.saturating_sub(unmatched_len(n1).max(unmatched_len(n2)))
}

/// Runs [`rating_core`] on stack buffers when both codes fit — they always do
/// on the reachable domain, where rule 4 caps a code at six characters —
/// falling back to heap vectors so the helper stays total for anything else.
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
    /// exists so MRA is reached the same way as every other encoder here.
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
    /// [type documentation](Self) for the four rules.
    ///
    /// ```
    /// use verbora_phonetics::MatchRatingApproach;
    ///
    /// let mra = MatchRatingApproach::new();
    /// assert_eq!(mra.process("HARPER"), "HRPR");
    /// assert_eq!(mra.process("Catherine"), "CTHRN");
    /// // First-3 + last-3 truncation past six letters:
    /// assert_eq!(mra.process("Franciszek"), "FRNSZK");
    /// // A single initial is not a name:
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
                // Rule 2 keeps the first letter even when it is a vowel, and
                // that letter seeds rule 3's pair state (a fresh collapser
                // never skips).
                collapser.skip(c);
                out.push(c);
                first = false;
                continue;
            }
            if matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') {
                // Rule 2 runs before rule 3 and rule 3 reads rule 2's output,
                // so a deleted vowel does NOT separate a consonant pair:
                // "SES" -> "SS" -> "S".
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
    /// See the [type documentation](Self) for the steps.
    ///
    /// ```
    /// use verbora_phonetics::MatchRatingApproach;
    ///
    /// let mra = MatchRatingApproach::new();
    /// assert!(mra.compare("Burns", "Bourne"));
    /// assert!(mra.compare("smith", "smyth"));
    /// assert!(!mra.compare("Sean", "Pete"));
    /// // Sharing a code is not what is being asked: the decision is the full
    /// // MRA rating against the minimum for the combined length.
    /// assert!(!mra.compare("Karl", "C")); // a single initial never matches
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        if a.trim().chars().nth(1).is_none() || b.trim().chars().nth(1).is_none() {
            return false;
        }
        // Pure short-circuit, not a rule: identical inputs encode identically,
        // every position blanks in the left-to-right pass, and a rating of 6
        // clears every minimum in the table. `the_raw_equality_shortcut_cannot
        // _change_a_decision` proves it against a reference that omits it.
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

    /// The published procedure, written a second time and on purpose
    /// differently: four separate string-rewriting stages, one per published
    /// rule, in the published order, instead of the production encoder's
    /// single fused scan.
    ///
    /// Nothing here is transcribed from another implementation, and nothing
    /// here is allowed to consult the production code except [`accent_fold`],
    /// which is a Verbora specification table rather than an algorithm step
    /// and is enumerated entry-by-entry by its own test below. Because the two
    /// bodies share no control flow, a mistake in fusing rule 2 into rule 3 —
    /// exactly the kind of mistake the fused scan invites — shows up as a
    /// disagreement rather than as two copies of the same bug.
    ///
    /// The reference also deliberately omits `compare`'s raw-equality
    /// short-circuit, so the cross-check below is what establishes that the
    /// short-circuit is an optimisation and not a rule.
    mod reference {
        use super::accent_fold;

        /// Rule 1. Note the absence of a punctuation filter: the reference
        /// keeps only `A`-`Z` and lets that do all the work, which is the
        /// claim `is_trim_char`'s documentation makes.
        fn clean(name: &str) -> String {
            name.chars()
                .flat_map(char::to_uppercase)
                .map(accent_fold)
                .filter(char::is_ascii_uppercase)
                .collect()
        }

        /// Rule 2: delete all vowels unless the vowel begins the word.
        fn remove_vowels(s: &str) -> String {
            s.chars()
                .enumerate()
                .filter(|&(i, c)| i == 0 || !matches!(c, 'A' | 'E' | 'I' | 'O' | 'U'))
                .map(|(_, c)| c)
                .collect()
        }

        /// Rule 3: remove the second consonant of any double consonant.
        /// `str::replace` scans left to right and does not overlap, which is
        /// the rule's "pairwise" reading spelled out by the standard library.
        fn remove_double_consonants(s: &str) -> String {
            let mut out = s.to_owned();
            for c in "BCDFGHJKLMNPQRSTVWXYZ".chars() {
                out = out.replace(&format!("{c}{c}"), &c.to_string());
            }
            out
        }

        /// Rule 4: reduce to six by joining the first three and last three.
        fn first3_last3(s: &str) -> String {
            let letters: Vec<char> = s.chars().collect();
            if letters.len() <= 6 {
                return s.to_owned();
            }
            letters[..3]
                .iter()
                .chain(&letters[letters.len() - 3..])
                .collect()
        }

        pub(super) fn encode(name: &str) -> String {
            if name.trim().chars().count() < 2 {
                return String::new();
            }
            first3_last3(&remove_double_consonants(&remove_vowels(&clean(name))))
        }

        /// The minimum-rating table, written as the paper's inequalities
        /// rather than as a `match` over ranges.
        pub(super) fn minimum_rating(sum: usize) -> usize {
            if sum <= 4 {
                5
            } else if sum <= 7 {
                4
            } else if sum <= 11 {
                3
            } else if sum == 12 {
                2
            } else {
                1
            }
        }

        /// Comparison steps 3-5, on `Vec<char>` with no stack-buffer
        /// specialisation and no early `break`.
        pub(super) fn rating(a: &str, b: &str) -> usize {
            let mut n1: Vec<char> = a.chars().collect();
            let mut n2: Vec<char> = b.chars().collect();
            if !n1.is_empty() && !n2.is_empty() {
                let (last1, last2) = (n1.len() - 1, n2.len() - 1);
                for i in 0..n1.len().min(n2.len()) {
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
            let unmatched = |n: &[char]| n.iter().filter(|&&c| c != ' ').count();
            6usize.saturating_sub(unmatched(&n1).max(unmatched(&n2)))
        }

        /// Comparison steps 1-6. No raw-equality short-circuit.
        pub(super) fn compare(a: &str, b: &str) -> bool {
            if a.trim().chars().count() < 2 || b.trim().chars().count() < 2 {
                return false;
            }
            let (c1, c2) = (encode(a), encode(b));
            let (l1, l2) = (c1.chars().count(), c2.chars().count());
            if l1.abs_diff(l2) >= 3 {
                return false;
            }
            rating(&c1, &c2) >= minimum_rating(l1 + l2)
        }
    }

    // ------------------------------------------------------------------
    // Rule 1: cleaning and the accent table.
    // ------------------------------------------------------------------

    /// The fold table is a closed Verbora specification, so it is enumerated
    /// rather than sampled: every entry is listed, and every other scalar in
    /// the range where precomposed Latin letters live must be untouched.
    ///
    /// The closure half is the half that matters. `Ā` (U+0100) is an `A` with
    /// a diacritic and is *not* folded; if the table quietly grew to cover it,
    /// a sampling test would never notice, and `Āb` would silently start
    /// encoding as `AB` instead of `""`.
    #[test]
    fn accent_table_folds_exactly_sixty_letters() {
        const GROUPS: &[(&str, char)] = &[
            ("ÀÁÂÃÄÅ", 'A'),
            ("àáâãäå", 'a'),
            ("ÈÉÊË", 'E'),
            ("èéêë", 'e'),
            ("ÌÍÎÏ", 'I'),
            ("ìíîï", 'i'),
            ("ÒÓÔÕÖŐ", 'O'),
            ("òóôõöő", 'o'),
            ("ÙÚÛÜŰ", 'U'),
            ("ùúûüű", 'u'),
            ("ÝŶŸ", 'Y'),
            ("ýŷÿ", 'y'),
            ("Ñ", 'N'),
            ("ñ", 'n'),
            ("Ç", 'C'),
            ("ç", 'c'),
        ];

        let mut folded = 0usize;
        for &(letters, base) in GROUPS {
            for c in letters.chars() {
                assert_eq!(accent_fold(c), base, "fold({c:?})");
                folded += 1;
            }
        }
        assert_eq!(folded, 60, "the table is documented as sixty letters");

        // Closure: nothing else in Basic Latin, Latin-1 Supplement, Latin
        // Extended-A or Latin Extended-B moves.
        let listed: Vec<char> = GROUPS.iter().flat_map(|(l, _)| l.chars()).collect();
        for scalar in 0u32..=0x024F {
            let Some(c) = char::from_u32(scalar) else {
                continue;
            };
            let moved = accent_fold(c) != c;
            assert_eq!(
                moved,
                listed.contains(&c),
                "accent_fold moved {c:?} (U+{scalar:04X}) but it is not in the table"
            );
        }

        // The named exclusions, spelled out because they are the ones a reader
        // will expect to be folded.
        for c in ['ß', 'Ø', 'ø', 'Æ', 'æ', 'Ā', 'ā', 'Ł', 'ł', 'Đ', 'Þ'] {
            assert_eq!(accent_fold(c), c, "{c:?} must not fold");
        }
    }

    /// Rule 1 keeps only `A`-`Z`, so the early punctuation filter cannot be
    /// load-bearing: a character it removes and a character it does not remove
    /// must both vanish, and the code must not move either way.
    #[test]
    fn punctuation_is_dropped_whether_or_not_it_is_trimmed_early() {
        let m = mra();
        let trimmed = ['-', '&', '\'', '.', ','];
        let untrimmed = ['!', '?', '/', '(', '3', '\u{2014}'];
        for c in trimmed.into_iter().chain(untrimmed) {
            assert!(
                is_trim_char(c) || !"-&'.,".contains(c),
                "the two sets must not overlap"
            );
            let spiked = format!("Jean{c}Luc");
            assert_eq!(m.process(&spiked), m.process("JeanLuc"), "for {c:?}");
        }
        // Whitespace likewise: it is filtered early and would be filtered late.
        assert_eq!(m.process("Mc Gowan"), m.process("McGowan"));
        // Only A-Z ever survives rule 1.
        let cleaned: String = clean_chars("Ó ' Súilleabháin -42-").collect();
        assert_eq!(cleaned, "OSUILLEABHAIN");
        assert!(cleaned.bytes().all(|b| b.is_ascii_uppercase()));
    }

    /// Uppercasing runs before the fold and before rule 2, and `ß` uppercases
    /// to two letters, so rule 3 sees a doubled consonant that was one scalar
    /// in the input.
    ///
    /// Derivation: `straße` → rule 1 → `STRASSE` → rule 2 → `STRSS` → rule 3
    /// → `STRS` → rule 4 (4 ≤ 6, no cut) → `STRS`.
    #[test]
    fn case_mapping_can_lengthen_a_name_before_rule_three() {
        assert_eq!(mra().process("straße"), "STRS");
        assert_eq!(mra().process("STRASSE"), "STRS");
    }

    // ------------------------------------------------------------------
    // Rules 2, 3 and 4, derived one name at a time.
    // ------------------------------------------------------------------

    /// Each row carries its own derivation, so the expected code can be
    /// checked against the published rules instead of taken on trust.
    #[test]
    fn encoding_follows_the_four_published_rules() {
        let m = mra();
        // (input, after rule 1, after rule 2, after rule 3, final)
        const DERIVATIONS: &[(&str, &str, &str, &str, &str)] = &[
            // No vowels after the first, no doubles, six or fewer: rules 2-4
            // are the identity beyond the vowel cut.
            ("HARPER", "HARPER", "HRPR", "HRPR", "HRPR"),
            ("Smith", "SMITH", "SMTH", "SMTH", "SMTH"),
            // Y is a consonant to MRA, so it survives rule 2.
            ("Smyth", "SMYTH", "SMYTH", "SMYTH", "SMYTH"),
            ("Byrne", "BYRNE", "BYRN", "BYRN", "BYRN"),
            ("Boern", "BOERN", "BRN", "BRN", "BRN"),
            ("Catherine", "CATHERINE", "CTHRN", "CTHRN", "CTHRN"),
            ("AIDAN", "AIDAN", "ADN", "ADN", "ADN"),
            ("DECLAN", "DECLAN", "DCLN", "DCLN", "DCLN"),
            // Rule 3 bites: the SS left by rule 2 becomes S.
            ("ALESSANDRA", "ALESSANDRA", "ALSSNDR", "ALSNDR", "ALSNDR"),
            ("BUBBLE", "BUBBLE", "BBBL", "BBL", "BBL"),
            // Rule 4 bites: seven letters become first three plus last three.
            ("Franciszek", "FRANCISZEK", "FRNCSZK", "FRNCSZK", "FRNSZK"),
            ("Alexzander", "ALEXZANDER", "ALXZNDR", "ALXZNDR", "ALXNDR"),
            // Both rules bite. MISSISSIPPI: rule 2 leaves M SS SS PP, rule 3
            // halves each pair, rule 4 has nothing left to cut.
            ("MISSISSIPPI", "MISSISSIPPI", "MSSSSPP", "MSSP", "MSSP"),
            // Punctuation and spacing vanish in rule 1; the accented I folds
            // to a plain I and is then deleted as a vowel.
            (
                "This-ís   a t.,es &t",
                "THISISATEST",
                "THSSTST",
                "THSTST",
                "THSTST",
            ),
            // A vowel deleted by rule 2 does not separate a pair for rule 3:
            // SES becomes SS, which becomes S.
            ("SES", "SES", "SS", "S", "S"),
            ("BAAAB", "BAAAB", "BB", "B", "B"),
            // Rule 2 keeps the first letter even when it is a vowel.
            ("AEIOU", "AEIOU", "A", "A", "A"),
            ("Ídá", "IDA", "ID", "ID", "ID"),
        ];

        for &(input, rule1, rule2, rule3, want) in DERIVATIONS {
            assert_eq!(m.process(input), want, "encoding {input:?}");
            // The intermediate stages, checked against the same derivation.
            let cleaned: String = clean_chars(input).collect();
            assert_eq!(cleaned, rule1, "rule 1 on {input:?}");
            assert_eq!(
                collapse_pairs(&drop_vowels_after_first(&cleaned)),
                rule3,
                "rule 3 on {input:?}"
            );
            assert_eq!(
                drop_vowels_after_first(&cleaned),
                rule2,
                "rule 2 on {input:?}"
            );
            assert_eq!(first3_last3(rule3.to_owned()), want, "rule 4 on {input:?}");
        }
    }

    /// Rule 2, applied to an already-cleaned string.
    fn drop_vowels_after_first(cleaned: &str) -> String {
        cleaned
            .chars()
            .enumerate()
            .filter(|&(i, c)| i == 0 || !matches!(c, 'A' | 'E' | 'I' | 'O' | 'U'))
            .map(|(_, c)| c)
            .collect()
    }

    /// Rule 3, driven through the production collapser one character at a
    /// time — the same state machine `process` uses, exercised in isolation.
    fn collapse_pairs(s: &str) -> String {
        let mut state = PairCollapser::new();
        s.chars().filter(|&c| !state.skip(c)).collect()
    }

    /// "Remove the second letter of every doubled consonant" is pairwise, so
    /// a run of *n* keeps ⌈n/2⌉ rather than collapsing to one. Enumerated for
    /// runs 1 through 9 instead of spot-checked.
    #[test]
    fn rule_three_keeps_half_of_every_run_rounded_up() {
        let m = mra();
        // From two, because a lone letter is stopped by the single-initial
        // gate before any rule runs.
        for n in 2..=9usize {
            let want = "B".repeat(n.div_ceil(2));
            assert_eq!(m.process(&"B".repeat(n)), want, "a run of {n}");
        }
        // Doubled vowels are not doubled consonants: rule 2 has already
        // deleted them, so rule 3 never sees them.
        assert_eq!(collapse_pairs("BEETLE"), "BEETLE");
        assert_eq!(collapse_pairs("MISSISSIPPI"), "MISISIPI");
    }

    /// Rule 4 cuts at six, and only past six.
    #[test]
    fn rule_four_joins_the_first_three_and_the_last_three() {
        // Six or fewer is returned unchanged, at every length.
        for s in ["", "P", "PE", "PET", "PETE", "PETER", "PETERS"] {
            assert_eq!(first3_last3(s.to_owned()), s, "for {s:?}");
        }
        // Seven is the first length that cuts: A-l-e + d-e-r.
        assert_eq!(first3_last3("Alexander".to_owned()), "Aleder");
        assert_eq!(first3_last3("ALXZNDR".to_owned()), "ALXNDR");
        // And it always yields exactly six.
        assert_eq!(first3_last3("ABCDEFGHIJKLMNOP".to_owned()).len(), 6);
    }

    /// Rule 4's cut is expressed in bytes; the reachability argument that
    /// makes that safe is that rules 1-3 emit only `A`-`Z`. Stated as a test
    /// so the argument fails loudly if rule 1 ever stops filtering.
    #[test]
    fn first3_last3_is_unreachable_on_non_ascii() {
        let m = mra();
        for input in crate::corpus::NON_ASCII_NAMES
            .iter()
            .chain(crate::corpus::PATHOLOGICAL.iter())
        {
            let code = m.process(input);
            assert!(code.is_ascii(), "{input:?} -> {code:?}");
            assert!(code.len() <= 6, "{input:?} -> {code:?}");
        }
        // The guard itself: a string whose byte cut would land mid-character
        // is returned untouched rather than panicking. "ab日本語" puts byte 3
        // inside the first CJK character, so neither end of the cut is a
        // boundary.
        assert_eq!(first3_last3("ab日本語".to_owned()), "ab日本語");
    }

    // ------------------------------------------------------------------
    // The comparison: the minimum table, the rating passes, the decision.
    // ------------------------------------------------------------------

    /// The minimum-rating table as the paper prints it, enumerated over every
    /// combined length a pair of codes can actually have (0 through 12) plus
    /// the open-ended tail.
    #[test]
    fn minimum_rating_is_the_published_table() {
        for sum in 0..=4 {
            assert_eq!(minimum_rating(sum), 5, "sum {sum}");
        }
        for sum in 5..=7 {
            assert_eq!(minimum_rating(sum), 4, "sum {sum}");
        }
        for sum in 8..=11 {
            assert_eq!(minimum_rating(sum), 3, "sum {sum}");
        }
        assert_eq!(minimum_rating(12), 2);
        for sum in 13..=64 {
            assert_eq!(minimum_rating(sum), 1, "sum {sum}");
        }
        // Rule 4 caps each code at six, so 12 is the largest sum a real pair
        // reaches and the `>= 13` row is unreachable through `compare`.
        assert_eq!(minimum_rating(6 + 6), 2);
    }

    /// The two passes, worked by hand.
    ///
    /// `ALEXANDER` / `ALEXANDRA`, both nine characters. The left-to-right
    /// half blanks indices 0-4 (`ALEXA`); the right-to-left half blanks
    /// indices 6 and 5 (`D`, `N`) and then meets already-blank positions.
    /// `E`,`R` survive on the left and `R`,`A` on the right: two unmatched
    /// each, so the rating is `6 - 2 = 4`.
    ///
    /// `EINSTEIN` / `MICHAELA`, both eight. Only index 1 (`I`) agrees
    /// left-to-right and only index 5 (`E`) agrees right-to-left, leaving six
    /// unmatched on each side: `6 - 6 = 0`.
    #[test]
    fn the_two_passes_rate_by_hand() {
        assert_eq!(rating("ALEXANDER", "ALEXANDRA"), 4);
        assert_eq!(rating("EINSTEIN", "MICHAELA"), 0);
        // Identical codes blank completely, which is the maximum rating.
        assert_eq!(rating("SMTH", "SMTH"), 6);
        // And a rating never goes below zero, however hopeless the pair.
        assert_eq!(rating("ABCDEFGH", "IJKLMNOP"), 0);
    }

    /// Blanking always writes both sides, so the two unmatched counts differ
    /// by exactly the length difference — which is why taking the maximum is
    /// literally the paper's "unmatched characters in the longer string".
    /// Enumerated over every pair of codes the fixtures below produce.
    #[test]
    fn the_longer_code_always_holds_the_unmatched_maximum() {
        let m = mra();
        let codes: Vec<String> = DECIDED_BY_RATING
            .iter()
            .flat_map(|&(a, b, ..)| [m.process(a), m.process(b)])
            .collect();
        for a in &codes {
            for b in &codes {
                let (mut n1, mut n2): (Vec<char>, Vec<char>) =
                    (a.chars().collect(), b.chars().collect());
                let scored = rating_core(&mut n1, &mut n2);
                let (u1, u2) = (unmatched_len(&n1), unmatched_len(&n2));
                assert_eq!(u1.abs_diff(u2), a.len().abs_diff(b.len()), "{a:?} vs {b:?}");
                let longer = if a.len() >= b.len() { u1 } else { u2 };
                assert_eq!(scored, 6usize.saturating_sub(longer), "{a:?} vs {b:?}");
            }
        }
    }

    /// The publication states the two passes as consecutive steps; Verbora
    /// specifies them interleaved. The readings are not equivalent, and this
    /// is the pair that shows it.
    ///
    /// `BBBBB` → rule 3 keeps ⌈5/2⌉ = 3 → `BBB`. `CCCBBB` → `CCBB`.
    /// Combined length 7, so the minimum is 4.
    ///
    /// *Interleaved* (Verbora): i=0 blanks the last `B` of each; i=1 blanks
    /// the next `B` of each; i=2 finds `B` against `C`. One `B` is left on the
    /// short side and `CC` on the long side → `6 - 2 = 4` → **match**.
    ///
    /// *Consecutive*: the whole left-to-right pass runs first and only the
    /// final `B`/`B` agrees, leaving the right-to-left pass nothing adjacent
    /// to work with → `BB` and `CCB` unmatched → `6 - 3 = 3` → **no match**.
    #[test]
    fn the_two_passes_are_interleaved_not_consecutive() {
        let m = mra();
        assert_eq!(m.process("BBBBB"), "BBB");
        assert_eq!(m.process("CCCBBB"), "CCBB");
        assert_eq!(minimum_rating("BBB".len() + "CCBB".len()), 4);
        assert_eq!(rating("BBB", "CCBB"), 4);
        assert!(m.compare("BBBBB", "CCCBBB"));

        // The consecutive reading, written out here so the difference is
        // visible rather than asserted.
        fn consecutive(a: &str, b: &str) -> usize {
            let mut n1: Vec<char> = a.chars().collect();
            let mut n2: Vec<char> = b.chars().collect();
            let (last1, last2) = (n1.len() - 1, n2.len() - 1);
            for i in 0..n1.len().min(n2.len()) {
                if n1[i] == n2[i] {
                    n1[i] = ' ';
                    n2[i] = ' ';
                }
            }
            for i in 0..n1.len().min(n2.len()) {
                if n1[last1 - i] == n2[last2 - i] {
                    n1[last1 - i] = ' ';
                    n2[last2 - i] = ' ';
                }
            }
            let unmatched = |n: &[char]| n.iter().filter(|&&c| c != ' ').count();
            6usize.saturating_sub(unmatched(&n1).max(unmatched(&n2)))
        }
        assert_eq!(consecutive("BBB", "CCBB"), 3);
    }

    /// Pairs whose verdict the rating decides, each carrying the two codes,
    /// the rating and the minimum so that every step of §Comparison can be
    /// re-derived from the published rules rather than believed.
    ///
    /// The names are drawn from the MRA literature and from the awkward
    /// shapes real name data contains — Irish `Ó`, hyphens, interior spaces,
    /// Slavic transliterations. Their *expected values* are computed here from
    /// the rules, not recorded from any implementation.
    #[allow(clippy::type_complexity)]
    const DECIDED_BY_RATING: &[(&str, &str, &str, &str, usize, usize, bool)] = &[
        // name a, name b, code a, code b, rating, minimum, match?
        ("John", "John", "JHN", "JHN", 6, 4, true),
        ("Byrne", "Boern", "BYRN", "BRN", 5, 4, true),
        ("smith", "smyth", "SMTH", "SMYTH", 5, 3, true),
        ("Burns", "Bourne", "BRNS", "BRN", 5, 4, true),
        ("Catherine", "Kathryn", "CTHRN", "KTHRYN", 4, 3, true),
        ("Brian", "Bryan", "BRN", "BRYN", 5, 4, true),
        ("Séan", "Shaun", "SN", "SHN", 5, 4, true),
        ("Cólm", "C-olín", "CLM", "CLN", 5, 4, true),
        ("Stephen", "Steven", "STPHN", "STVN", 4, 3, true),
        ("Steven", "Stefan", "STVN", "STFN", 5, 3, true),
        ("Stephen", "Stefan", "STPHN", "STFN", 4, 3, true),
        ("Sam", "Samuel", "SM", "SML", 5, 4, true),
        ("Micky", "Michael", "MCKY", "MCHL", 4, 3, true),
        ("Oona", "Oonagh", "ON", "ONGH", 4, 4, true),
        ("Sophie", "Sofia", "SPH", "SF", 4, 4, true),
        ("Franciszek", "Frances", "FRNSZK", "FRNCS", 3, 3, true),
        ("Tomasz", "tom", "TMSZ", "TM", 4, 4, true),
        ("Kl", "Karl", "KL", "KRL", 5, 4, true),
        ("Zach", "Zacharia", "ZCH", "ZCHR", 5, 4, true),
        (
            "O'Sullivan",
            "Ó ' Súilleabháin",
            "OSLVN",
            "OSLBHN",
            4,
            3,
            true,
        ),
        (
            "o'muireadhaigh",
            "Ó 'Muircheartaigh ",
            "OMRHGH",
            "OMRTGH",
            5,
            2,
            true,
        ),
        ("Cooper-Flynn", "Super-Lyn", "CPRLYN", "SPRLYN", 5, 2, true),
        ("Hailey", "Halley", "HLY", "HLY", 6, 4, true),
        ("Auerbach", "Uhrbach", "ARBCH", "UHRBCH", 4, 3, true),
        ("Moskowitz", "Moskovitz", "MSKWTZ", "MSKVTZ", 5, 2, true),
        ("LIPSHITZ", "LIPPSZYC", "LPSHTZ", "LPSZYC", 3, 2, true),
        ("LEWINSKY", "LEVINSKI", "LWNSKY", "LVNSK", 4, 3, true),
        ("SZLAMAWICZ", "SHLAMOVITZ", "SZLWCZ", "SHLVTZ", 3, 2, true),
        (
            "R o s o ch o w a c ie c",
            " R o s o k ho v a ts e ts",
            "RSCHWC",
            "RSKSTS",
            2,
            2,
            true,
        ),
        (
            " P rz e m y s l",
            " P sh e m e sh i l",
            "PRZYSL",
            "PSHSHL",
            2,
            2,
            true,
        ),
        ("Peterson", "Peters", "PTRSN", "PTRS", 5, 3, true),
        ("McGowan", "Mc Geoghegan", "MCGWN", "MCGHGN", 4, 3, true),
        // Famously generous: two names that share nothing but their length
        // still clear a minimum of 4 once the trailing N blanks.
        ("Sean", "John", "SN", "JHN", 4, 4, true),
        // Short codes need a rating of 5, which two-letter codes with nothing
        // in common cannot reach.
        ("Al", "Ed", "AL", "ED", 4, 5, false),
        ("Sean", "Pete", "SN", "PT", 4, 5, false),
        // ...and famously ungenerous: Úna and Oonagh are the same name.
        ("Úna", "Oonagh", "UN", "ONGH", 3, 4, false),
        ("Moriarty", "OMuircheartaigh", "MRTY", "OMRTGH", 0, 3, false),
        ("Murphy", "Lynch", "MRPHY", "LYNCH", 1, 3, false),
    ];

    /// Pairs rejected before the rating ever runs, and by which of the two
    /// gates.
    const REJECTED_BEFORE_RATING: &[(&str, &str, Gate)] = &[
        ("Karl", "C", Gate::SingleInitial),
        ("Murphy", " ", Gate::SingleInitial),
        ("Murphy", "", Gate::SingleInitial),
        ("test", "", Gate::SingleInitial),
        ("", "test", Gate::SingleInitial),
        ("test", " ", Gate::SingleInitial),
        (" ", "test", Gate::SingleInitial),
        ("t", "test", Gate::SingleInitial),
        ("test", "t", Gate::SingleInitial),
        // KRL is three letters, ALSNDR is six: a difference of exactly three
        // makes the comparison obsolete, whatever the letters are.
        ("Karl", "Alessandro", Gate::Obsolete),
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Gate {
        SingleInitial,
        Obsolete,
    }

    #[test]
    fn every_decided_pair_re_derives_from_the_published_steps() {
        let m = mra();
        for &(a, b, code_a, code_b, want_rating, want_minimum, want_match) in DECIDED_BY_RATING {
            assert_eq!(m.process(a), code_a, "code of {a:?}");
            assert_eq!(m.process(b), code_b, "code of {b:?}");
            assert!(
                code_a.len().abs_diff(code_b.len()) < 3,
                "{a:?}/{b:?} would be obsolete, not rated"
            );
            assert_eq!(
                minimum_rating(code_a.len() + code_b.len()),
                want_minimum,
                "minimum for {a:?}/{b:?}"
            );
            assert_eq!(rating(code_a, code_b), want_rating, "rating {a:?}/{b:?}");
            assert_eq!(
                want_rating >= want_minimum,
                want_match,
                "the table's own arithmetic disagrees with its verdict for {a:?}/{b:?}"
            );
            assert_eq!(m.compare(a, b), want_match, "compare {a:?}/{b:?}");
            // The decision is symmetric: nothing in the procedure privileges
            // an argument position.
            assert_eq!(m.compare(b, a), want_match, "compare {b:?}/{a:?}");
        }
    }

    #[test]
    fn every_rejected_pair_is_rejected_by_the_gate_it_names() {
        let m = mra();
        for &(a, b, gate) in REJECTED_BEFORE_RATING {
            let single = a.trim().chars().nth(1).is_none() || b.trim().chars().nth(1).is_none();
            match gate {
                Gate::SingleInitial => assert!(single, "{a:?}/{b:?} is not single-initial"),
                Gate::Obsolete => {
                    assert!(!single, "{a:?}/{b:?} is gated earlier than claimed");
                    assert!(
                        m.process(a).len().abs_diff(m.process(b).len()) >= 3,
                        "{a:?}/{b:?} is not obsolete"
                    );
                }
            }
            assert!(!m.compare(a, b), "compare {a:?}/{b:?}");
            assert!(!m.compare(b, a), "compare {b:?}/{a:?}");
        }
        // A difference of two still rates: KL against KRL is 2 against 3.
        assert!(mra().compare("Kl", "Karl"));
    }

    // ------------------------------------------------------------------
    // The reference cross-check.
    // ------------------------------------------------------------------

    /// Every corpus name and every fixture name, through both encoders.
    fn cross_check_inputs() -> Vec<&'static str> {
        let mut inputs: Vec<&'static str> = crate::corpus::NON_ASCII_NAMES
            .iter()
            .chain(crate::corpus::PATHOLOGICAL.iter())
            .copied()
            .collect();
        for &(a, b, ..) in DECIDED_BY_RATING {
            inputs.push(a);
            inputs.push(b);
        }
        for &(a, b, _) in REJECTED_BEFORE_RATING {
            inputs.push(a);
            inputs.push(b);
        }
        inputs.extend([
            "",
            " ",
            "  \t\n",
            "..",
            ",,",
            "-&',.",
            "!?",
            "42",
            "1234567",
            "a",
            "a.",
            "E",
            "AA",
            "BB",
            "BBB",
            "BBBB",
            "BBBBB",
            "CCCBBB",
            "SES",
            "AEIOU",
            "straße",
            "supercalifragilisticexpialidocious",
            "MISSISSIPPI",
            "Ídá",
            "café",
            "naïve",
            "İstanbul",
        ]);
        inputs
    }

    /// The fused production scan against the four-stage transcription of the
    /// published rules, over every input the crate has.
    #[test]
    fn encoding_agrees_with_the_published_rules_transcribed_separately() {
        let m = mra();
        for input in cross_check_inputs() {
            assert_eq!(
                m.process(input),
                reference::encode(input),
                "encoding {input:?}"
            );
            // Repeated at length, which is where a fused scan's state machine
            // goes wrong if it goes wrong at all.
            let long = input.repeat(20);
            assert_eq!(
                m.process(&long),
                reference::encode(&long),
                "encoding {input:?} x20"
            );
        }
    }

    /// The production decision against the transcribed one, over every
    /// ordered pair of those inputs.
    #[test]
    fn the_decision_agrees_with_the_published_steps_over_every_pair() {
        let m = mra();
        let inputs = cross_check_inputs();
        for a in &inputs {
            for b in &inputs {
                assert_eq!(m.compare(a, b), reference::compare(a, b), "{a:?} vs {b:?}");
            }
        }
    }

    /// The reference omits `compare`'s raw-equality short-circuit, so the
    /// pair-wise agreement above already proves the short-circuit is inert.
    /// This states the claim directly for the inputs where it is least
    /// obvious: names that encode to nothing at all.
    #[test]
    fn the_raw_equality_shortcut_cannot_change_a_decision() {
        let m = mra();
        for s in ["..", "!?", "Al", "  John  ", "日本語", "😀😀"] {
            assert_eq!(m.compare(s, s), reference::compare(s, s), "{s:?}");
        }
        // Identical inputs share a code, so every position blanks and the
        // rating is 6 — above every minimum in the table.
        assert_eq!(m.process(".."), "");
        assert_eq!(rating("", ""), 6);
        assert!(m.compare("..", ".."));
        // Which is why two *different* letterless names match as well.
        assert!(m.compare("..", ",,"));
        // A single initial is still rejected first, identical or not.
        assert!(!m.compare("e", "e"));
    }

    // ------------------------------------------------------------------
    // The text unit, totality and the trait.
    // ------------------------------------------------------------------

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
        // Two raw scalars, one of them punctuation, still pass the gate.
        assert_eq!(m.process("a."), "A");
        assert_eq!(m.process("a"), "");
        assert_eq!(m.process(" a "), "");
        // ... and a single initial never matches anything.
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

    /// The text unit: after the accent fold, a scalar that is still not
    /// `A`-`Z` is dropped, so every code is pure ASCII and rule 4's cut can
    /// never land inside a character.
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

    /// Two names that both encode to `""` have nothing to disagree about: the
    /// rating is 6, which clears every minimum, so they match. One empty side
    /// against a real name does not: the real name is entirely unmatched.
    #[test]
    fn empty_encodings_in_compare() {
        let m = mra();
        assert!(m.compare("..", ",,"));
        // "" against "AB": two unmatched, rating 4, minimum 5.
        assert_eq!(rating("", "AB"), 4);
        assert_eq!(minimum_rating(2), 5);
        assert!(!m.compare("..", "ab"));
        // Symmetric, argument order included.
        assert_eq!(rating("AB", ""), 4);
        assert!(!m.compare("ab", ".."));
    }

    #[test]
    fn very_long_input() {
        let m = mra();
        // 1000 Bs -> rule 3 keeps 500 -> rule 4 keeps six.
        assert_eq!(m.process(&"B".repeat(1000)), "BBBBBB");
        // Vowels beyond the first all drop regardless of length.
        assert_eq!(m.process(&"a".repeat(1000)), "A");
        assert_eq!(
            m.process("supercalifragilisticexpialidocious"),
            // SPRCLFRGLSTCXPLDCS -> first three plus last three
            "SPRDCS"
        );
    }

    /// The rating buffers are `[char; 8]` with a heap fallback beyond that.
    /// Every code is ASCII and at most six characters, so `compare` always
    /// takes the stack path; the fallback exists to keep the helper total.
    #[test]
    fn rating_stack_boundary_and_heap_fallback() {
        let m = mra();
        assert_eq!(m.process("Franciszek").len(), 6);
        assert!(m.compare("Franciszek", "Frances"));
        assert!(m.compare("Smith", "Smyth"));
        // A long input still truncates to six, so the stack path suffices.
        assert!(m.process(&"abcdefghij".repeat(50)).len() <= 6);
        // The fallback, driven directly: nine characters on each side.
        assert_eq!(rating("ALEXANDER", "ALEXANDRA"), 4);
        // And it is not reachable from `compare`, because rule 4 caps codes.
        for input in cross_check_inputs() {
            assert!(m.process(input).len() <= 6, "{input:?}");
        }
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
