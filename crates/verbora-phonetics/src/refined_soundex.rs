//! Refined Soundex — the one encoder here whose reference is a distribution
//! rather than a paper.
//!
//! Every other algorithm in this crate cites a publication. Refined Soundex
//! has none: it exists as source code, shipped as
//! `org.apache.commons.codec.language.RefinedSoundex` in Apache Commons Codec,
//! and that distribution's twenty-six-character mapping table is the whole of
//! its definition. Verbora states the table and the emission rule in
//! [`RefinedSoundex`]'s own documentation and treats *that statement* as the
//! specification; the distribution is cited as the origin of the table, which
//! is a standards citation of the only standard this algorithm has.
//!
//! The distinction shows up in the tests below. They walk each input through
//! the documented mapping — letter by letter, digit by digit, with the walk
//! written out — instead of recording what any implementation returns. The
//! mapping table is short enough that enumeration is cheap, so every one of
//! the twenty-six letters is pinned individually and the table is rebuilt from
//! the ten documented letter groups and compared against the constant.

/// Group digit for each letter, indexed by `letter - b'A'`.
///
/// This is the reference distribution's `US_ENGLISH_MAPPING_STRING`,
/// `"01360240043788015936020505"` — see [`RefinedSoundex`] on why a
/// distribution, rather than a paper, is this encoder's citable basis.
const MAPPING: [u8; 26] = *b"01360240043788015936020505";

/// Refined Soundex — ten consonant groups, no truncation.
///
/// # Basis, and why it is not a publication
///
/// Refined Soundex has no paper. It is a variant distributed with **Apache
/// Commons Codec** as `org.apache.commons.codec.language.RefinedSoundex`,
/// which splits Russell and Odell's six consonant groups (U.S. patents
/// 1,261,167 and 1,435,663) into ten and drops the fixed four-character cap,
/// for spell-checking rather than census indexing. Verbora states the mapping
/// table and the emission rule below and treats *that* as the specification;
/// the Commons Codec distribution is cited as the origin of the table, not as
/// an oracle for behaviour.
///
/// Saying so plainly is deliberate. Manufacturing a citation for an algorithm
/// that has none would be worse than having none, and so would quietly
/// pinning this encoder against another implementation's test vectors: the
/// tests in this module derive every expected code by walking the table below,
/// so they would still be right if that implementation were wrong.
///
/// Prefer [`SoundEx`](crate::SoundEx) when you need the standard,
/// four-character census code; prefer this when you want a finer key that
/// grows with the word.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar**, and only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding. Every other scalar is
///   skipped, and skipping is transparent: a skipped scalar does not break a
///   duplicate-digit run, so `"a\u{e9}a"` and `"aa"` both encode `"A0"`.
/// * **No truncation and no padding**: the code grows with the word.
/// * A token with no `A`–`Z` letter encodes to `""`.
/// * **Total**: no input panics, and there is no error type.
///
/// # The algorithm
///
/// The code is the first letter, followed by the group digit of *every*
/// letter — the first included — with adjacent equal digits collapsed to one.
/// Unlike classic Soundex, vowels are not dropped: they encode as `0` and
/// therefore *separate* consonant runs, so `bab` is `B101`, not `B11`.
///
/// | digit | letters | | digit | letters |
/// |---|---|---|---|---|
/// | `0` | A E H I O U W Y | | `5` | Q X Z |
/// | `1` | B P | | `6` | D T |
/// | `2` | F V | | `7` | L |
/// | `3` | C K S | | `8` | M N |
/// | `4` | G J | | `9` | R |
///
/// [`RefinedSoundex::difference`] is the companion similarity measure the same
/// distribution ships alongside the encoder.
///
/// ```
/// use verbora_phonetics::RefinedSoundex;
///
/// let refined = RefinedSoundex::new();
/// assert_eq!(refined.process("jumped"), "J408106");
/// assert_eq!(refined.process("testing"), "T6036084");
/// assert!(refined.compare("Smith", "Smythe"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefinedSoundex;

impl RefinedSoundex {
    /// Creates a Refined Soundex encoder. It holds no state; the type mirrors
    /// the crate's other encoders.
    ///
    /// ```
    /// use verbora_phonetics::RefinedSoundex;
    ///
    /// assert_eq!(RefinedSoundex::new(), RefinedSoundex::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` into its Refined Soundex code.
    ///
    /// Returns the empty string when the input contains no `A`–`Z` letter:
    /// `""`, `" "`, `"123"` and `"😀"` all encode to `""`.
    ///
    /// ```
    /// use verbora_phonetics::RefinedSoundex;
    ///
    /// let refined = RefinedSoundex::new();
    /// assert_eq!(refined.process("brown"), "B1908");
    /// assert_eq!(refined.process("quick"), "Q503");
    /// assert_eq!(refined.process("dogs"), "D6043");
    /// assert_eq!(refined.process("1-2-3"), "");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        // Output is at most one initial letter plus one digit per input
        // letter; `len + 1` therefore never reallocates on the ASCII path.
        let mut out = String::with_capacity(token.len() + 1);
        // Digits are `b'0'..=b'9'`, so 0 is a safe "no previous digit" value.
        let mut previous: u8 = 0;

        for letter in crate::letters::Letters::new(token) {
            if out.is_empty() {
                out.push(char::from(letter));
            }
            let code = MAPPING[usize::from(letter - b'A')];
            if code != previous {
                out.push(char::from(code));
                previous = code;
            }
        }

        out
    }

    /// Whether two strings share a Refined Soundex code.
    ///
    /// Note that two letter-free inputs both encode to `""` and therefore
    /// compare equal.
    ///
    /// ```
    /// use verbora_phonetics::RefinedSoundex;
    ///
    /// let refined = RefinedSoundex::new();
    /// assert!(refined.compare("Smithers", "Smythers"));
    /// assert!(!refined.compare("dogs", "dog"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }

    /// Number of positions at which the two codes carry the same character.
    ///
    /// This is the similarity measure the same distribution ships next to the
    /// encoder, modelled on SQL Server's `DIFFERENCE`. 0 means no similarity.
    /// Unlike classic Soundex (capped at 4 by its four-character codes),
    /// Refined Soundex codes are unbounded, so the difference can be
    /// arbitrarily large. Positions beyond the shorter code's length are not
    /// counted, and an empty code shares no position with anything.
    ///
    /// ```
    /// use verbora_phonetics::RefinedSoundex;
    ///
    /// let refined = RefinedSoundex::new();
    /// // Low similarity: only one position of M80940906 and A08690 agrees.
    /// assert_eq!(refined.difference("Margaret", "Andrew"), 1);
    /// // High similarity: S3806093 twice over.
    /// assert_eq!(refined.difference("Smithers", "Smythers"), 8);
    /// ```
    #[must_use]
    pub fn difference(&self, a: &str, b: &str) -> usize {
        let a = self.process(a);
        let b = self.process(b);
        // Codes are pure ASCII (an A–Z letter then digits), so comparing
        // bytes is comparing chars. Zipping stops at the shorter code, which
        // also makes an empty code score 0 without a special case.
        a.bytes().zip(b.bytes()).filter(|(x, y)| x == y).count()
    }
}

impl verbora_core::Phonetic for RefinedSoundex {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs() -> RefinedSoundex {
        RefinedSoundex::new()
    }

    // ------------------------------------------------------------------
    // The mapping table itself.
    //
    // The table *is* the specification for this encoder, so it is pinned two
    // ways that must agree: rebuilt from the ten letter groups the rustdoc
    // prints, and compared against the twenty-six-character string the
    // reference distribution publishes. If the documented groups and the
    // constant ever drift apart, the first assertion fails; if the constant
    // drifts from the distribution it cites, the second does.
    // ------------------------------------------------------------------

    /// The ten groups, exactly as [`RefinedSoundex`]'s documentation prints
    /// them.
    const GROUPS: [(u8, &str); 10] = [
        (b'0', "AEHIOUWY"),
        (b'1', "BP"),
        (b'2', "FV"),
        (b'3', "CKS"),
        (b'4', "GJ"),
        (b'5', "QXZ"),
        (b'6', "DT"),
        (b'7', "L"),
        (b'8', "MN"),
        (b'9', "R"),
    ];

    #[test]
    fn the_documented_groups_rebuild_the_mapping_table() {
        // Every letter appears in exactly one group, and the twenty-six of
        // them cover the alphabet.
        let mut rebuilt = [0u8; 26];
        let mut seen = [false; 26];
        for (digit, letters) in GROUPS {
            for letter in letters.bytes() {
                let i = usize::from(letter - b'A');
                assert!(!seen[i], "{} appears in two groups", char::from(letter));
                seen[i] = true;
                rebuilt[i] = digit;
            }
        }
        assert!(seen.iter().all(|&s| s), "a letter is in no group at all");
        assert_eq!(rebuilt, MAPPING, "the documented groups and MAPPING differ");
    }

    #[test]
    fn the_mapping_table_is_the_one_the_reference_distribution_publishes() {
        assert_eq!(&MAPPING, b"01360240043788015936020505");
    }

    /// Every letter, encoded on its own, upper and lower case. A one-letter
    /// token yields the letter plus its own group digit, so this walks the
    /// whole table through the public API.
    #[test]
    fn every_letter_encodes_to_its_documented_group_digit() {
        let r = rs();
        for (digit, letters) in GROUPS {
            for letter in letters.chars() {
                let want = format!("{letter}{}", char::from(digit));
                assert_eq!(r.process(&letter.to_string()), want, "for {letter:?}");
                assert_eq!(
                    r.process(&letter.to_lowercase().to_string()),
                    want,
                    "for lowercase {letter:?}"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Encoding, derived one letter at a time.
    // ------------------------------------------------------------------

    /// Each row spells out the digit of every letter, before collapsing, so
    /// the expected code can be checked against the table rather than trusted.
    /// The `|` marks a digit dropped because it repeats the one before it.
    #[test]
    fn encoding_walks_every_letter_through_the_mapping() {
        let r = rs();
        const DERIVATIONS: &[(&str, &str, &str)] = &[
            // input, per-letter digits, code
            ("testing", "t6 e0 s3 t6 i0 n8 g4", "T6036084"),
            ("TESTING", "T6 E0 S3 T6 I0 N8 G4", "T6036084"),
            // h and e are both group 0, so the second collapses.
            ("The", "T6 h0 e|0", "T60"),
            // u and i collapse into one 0; c and k both into one 3.
            ("quick", "q5 u0 i|0 c3 k|3", "Q503"),
            ("brown", "b1 r9 o0 w|0 n8", "B1908"),
            ("fox", "f2 o0 x5", "F205"),
            ("jumped", "j4 u0 m8 p1 e0 d6", "J408106"),
            ("over", "o0 v2 e0 r9", "O0209"),
            ("lazy", "l7 a0 z5 y0", "L7050"),
            ("dogs", "d6 o0 g4 s3", "D6043"),
            // Vowels are digits, not deletions: they separate consonant runs
            // instead of joining them.
            ("bab", "b1 a0 b1", "B101"),
            ("bb", "b1 b|1", "B1"),
            ("bpbp", "b1 p|1 b|1 p|1", "B1"),
            ("mn", "m8 n|8", "M8"),
            ("aaa", "a0 a|0 a|0", "A0"),
            (
                "Ashcraft",
                "A0 s3 h0 c3 r9 a0 f2 t6",
                // Nothing collapses: 0,3,0,3,9,0,2,6 has no adjacent pair.
                "A03039026",
            ),
        ];

        for &(input, digits, want) in DERIVATIONS {
            assert_eq!(r.process(input), want, "for {input:?}");
            // The derivation column is not decoration: check that its
            // uncollapsed digit sequence really is what the table says, and
            // that collapsing it really is the code.
            let mut expected = String::new();
            let mut previous = 0u8;
            for step in digits.split_whitespace() {
                let mut bytes = step.bytes();
                let letter = bytes.next().expect("a letter").to_ascii_uppercase();
                let rest: Vec<u8> = bytes.collect();
                let (collapsed, digit) = match rest.as_slice() {
                    [b'|', d] => (true, *d),
                    [d] => (false, *d),
                    _ => panic!("malformed derivation step {step:?}"),
                };
                assert_eq!(
                    MAPPING[usize::from(letter - b'A')],
                    digit,
                    "{step:?} claims the wrong group digit"
                );
                if expected.is_empty() {
                    expected.push(char::from(letter));
                }
                assert_eq!(
                    collapsed,
                    digit == previous,
                    "{step:?} disagrees with the collapse rule"
                );
                if !collapsed {
                    expected.push(char::from(digit));
                    previous = digit;
                }
            }
            assert_eq!(expected, want, "the derivation of {input:?}");
        }
    }

    /// Long words keep growing: the code is never truncated and never padded.
    ///
    /// `supercalifragilisticexpialidocious` walks to
    /// `3 0 1 0 9 3 0 7 0 2 9 0 4 0 7 0 3 6 0 3 0 5 1 0 0 7 0 6 0 3 0 0 0 3`;
    /// the four adjacent repeats (`i`/`a` in `pial`, and `o`/`u` in `cious`)
    /// collapse, leaving thirty-one digits behind the initial `S`.
    #[test]
    fn no_truncation_and_no_padding() {
        let r = rs();
        // A single letter gives a two-character code; nothing pads it out.
        assert_eq!(r.process("a"), "A0");
        let long = r.process("supercalifragilisticexpialidocious");
        assert_eq!(long, "S3010930702904070360305107060303");
        assert_eq!(long.len(), 1 + 31);
        // 100 letters of alternating groups: 100 digits survive.
        let expected = {
            let mut s = String::from("A");
            for _ in 0..50 {
                s.push_str("01");
            }
            s
        };
        assert_eq!(r.process(&"ab".repeat(50)), expected);
    }

    #[test]
    fn very_long_input() {
        let r = rs();
        assert_eq!(r.process(&"a".repeat(10_000)), "A0");
        let long = "jumped".repeat(2_000);
        let code = r.process(&long);
        assert!(code.starts_with("J408106"));
        // Every repetition contributes its six digits "408106": the "d"/"j"
        // seam is 6 then 4, so nothing collapses across the boundary.
        assert_eq!(code.len(), 1 + 6 * 2_000);
    }

    // ------------------------------------------------------------------
    // The text unit.
    // ------------------------------------------------------------------

    #[test]
    fn letter_free_input_encodes_to_the_empty_string() {
        let r = rs();
        for input in ["", " ", "  ", "\t\n", "123", "!!!", "1-2-3", "___", "😀"] {
            assert_eq!(r.process(input), "", "for {input:?}");
        }
    }

    /// A non-letter is *skipped*, not treated as a separator — so it cannot
    /// keep two same-group letters from collapsing into one digit.
    #[test]
    fn non_letters_are_skipped_rather_than_separating() {
        let r = rs();
        const DERIVATIONS: &[(&str, &str, &str)] = &[
            ("b4t", "b1 t6", "B16"),
            ("the quick", "t6 h|0 e|0 q5 u|0 i|0 c3 k|3", "T60503"),
            ("a-b", "a0 b1", "A01"),
            ("x y", "x5 y0", "X50"),
            ("O'Brien", "O0 B1 r9 i0 e|0 n8", "O01908"),
            ("don't", "d6 o0 n8 t6", "D6086"),
            ("d'Artagnan", "d6 A0 r9 t6 a0 g4 n8 a0 n8", "D609604808"),
            ("McDonald", "M8 c3 D6 o0 n8 a0 l7 d6", "M83608076"),
            (
                "van der Berg",
                "v2 a0 n8 d6 e0 r9 B1 e0 r9 g4",
                "V2086091094",
            ),
            (
                "Blotchet-Halls",
                "B1 l7 o0 t6 c3 h0 e|0 t6 H0 a|0 l7 l|7 s3",
                "B1706306073",
            ),
            // The whole point: the space does not stop the two b's colliding.
            ("b b", "b1 b|1", "B1"),
        ];
        for &(input, digits, want) in DERIVATIONS {
            assert_eq!(r.process(input), want, "for {input:?}");
            // The digit column must name exactly the letters of the input.
            let letters: String = input.chars().filter(char::is_ascii_alphabetic).collect();
            let named: String = digits
                .split_whitespace()
                .map(|s| s.chars().next().expect("a letter"))
                .collect();
            assert_eq!(named, letters, "the derivation of {input:?}");
        }
    }

    #[test]
    fn mixed_case_is_case_insensitive() {
        let r = rs();
        assert_eq!(r.process("TeStInG"), "T6036084");
        assert_eq!(r.process("SMITH"), r.process("smith"));
        assert_eq!(r.process("McDonald"), r.process("mcdonald"));
    }

    /// The text unit, enumerated over one scalar of every class. A scalar
    /// outside `A`-`Z` is skipped, and the skip is *transparent*: it does not
    /// break a duplicate-digit run, or `"a\u{e9}a"` would encode differently
    /// from `"aa"`.
    #[test]
    fn only_ascii_letters_are_read() {
        let r = rs();
        for input in [
            "",
            " ",
            "12345",
            "...",
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{1F600}",
            "\u{041c}\u{043e}",
            "\u{d1}",
        ] {
            assert_eq!(r.process(input), "", "for {input:?}");
        }
        assert_eq!(r.process("caf\u{e9}"), r.process("caf"));
        assert_eq!(r.process("na\u{ef}ve"), r.process("nave"));
        assert_eq!(r.process("\u{df}"), ""); // not an A-Z letter, so skipped
        assert_eq!(r.process("stra\u{df}e"), r.process("strae"));
        assert_eq!(r.process("\u{130}stanbul"), r.process("stanbul"));
        assert_eq!(r.process("a\u{1F600}b"), "A01");
        // A skipped scalar is transparent to the duplicate-digit collapse.
        assert_eq!(r.process("a\u{e9}a"), "A0");
        assert_eq!(r.process("a\u{212A}a"), "A0");
        // Every code is ASCII: an A-Z initial followed by digits.
        for input in crate::corpus::NON_ASCII_NAMES
            .iter()
            .chain(crate::corpus::PATHOLOGICAL.iter())
        {
            let code = r.process(input);
            assert!(
                code.is_empty()
                    || (code.as_bytes()[0].is_ascii_uppercase()
                        && code.bytes().skip(1).all(|b| b.is_ascii_digit())),
                "for {input:?}: {code:?}"
            );
        }
    }

    // ------------------------------------------------------------------
    // `difference`, and `compare`.
    // ------------------------------------------------------------------

    /// Each row carries both codes, so the count can be checked by lining
    /// them up rather than believed.
    #[test]
    fn difference_counts_agreeing_positions_of_the_two_codes() {
        let r = rs();
        const PAIRS: &[(&str, &str, &str, &str, usize)] = &[
            // a, b, code a, code b, agreeing positions
            ("", "", "", "", 0),
            (" ", " ", "", "", 0),
            ("Smith", "", "S38060", "", 0),
            ("", "Smith", "", "S38060", 0),
            // Identical codes agree everywhere they overlap.
            ("Smith", "Smythe", "S38060", "S38060", 6),
            ("Smithers", "Smythers", "S3806093", "S3806093", 8),
            ("testing", "testing", "T6036084", "T6036084", 8),
            // A08 against A08690: the shorter code stops the count at three.
            ("Ann", "Andrew", "A08", "A08690", 3),
            ("dogs", "dog", "D6043", "D604", 4),
            // G4908 against G49080: five overlapping positions, all equal.
            ("Green", "Greene", "G4908", "G49080", 5),
            // M80940906 against A08690 agrees only at index 5 (0 against 0).
            ("Margaret", "Andrew", "M80940906", "A08690", 1),
            // J40806 against M80940906 agrees only at index 2.
            ("Janet", "Margaret", "J40806", "M80940906", 1),
            // B1706306073 against G49080 agrees only at index 3.
            ("Blotchet-Halls", "Greene", "B1706306073", "G49080", 1),
            // A0806093 against B1906093 agrees on the last five positions.
            ("Anothers", "Brothers", "A0806093", "B1906093", 5),
        ];

        for &(a, b, code_a, code_b, want) in PAIRS {
            assert_eq!(r.process(a), code_a, "code of {a:?}");
            assert_eq!(r.process(b), code_b, "code of {b:?}");
            // The count, recomputed from the two literal codes rather than
            // from the encoder.
            let counted = code_a
                .chars()
                .zip(code_b.chars())
                .filter(|(x, y)| x == y)
                .count();
            assert_eq!(counted, want, "the row's own arithmetic for {a:?}/{b:?}");
            assert_eq!(r.difference(a, b), want, "difference {a:?}/{b:?}");
            // Symmetric: position agreement does not depend on argument order.
            assert_eq!(r.difference(b, a), want, "difference {b:?}/{a:?}");
        }
    }

    /// `difference` is bounded by the shorter code and reaches that bound
    /// exactly when one code is a prefix of the other.
    #[test]
    fn difference_is_bounded_by_the_shorter_code() {
        let r = rs();
        for a in [
            "testing",
            "Smith",
            "dogs",
            "",
            "123",
            "supercalifragilistic",
        ] {
            for b in ["testing", "Smythe", "dog", "", "!!!", "Margaret"] {
                let (ca, cb) = (r.process(a), r.process(b));
                let d = r.difference(a, b);
                assert!(d <= ca.len().min(cb.len()), "{a:?}/{b:?}");
                assert_eq!(
                    d == ca.len().min(cb.len()),
                    ca.starts_with(&cb) || cb.starts_with(&ca),
                    "{a:?}/{b:?}: {ca:?} {cb:?}"
                );
            }
        }
    }

    #[test]
    fn compare_is_code_equality() {
        let r = rs();
        assert!(r.compare("Smith", "Smythe"));
        assert!(r.compare("Smithers", "Smythers"));
        // "bpbp" is B1 and so is "b": every letter of both is group 1.
        assert!(r.compare("bpbp", "b"));
        // "dogs" is D6043 and "dog" is D604 — a prefix is not equality.
        assert!(!r.compare("dogs", "dog"));
        assert!(!r.compare("Green", "Greene"));
        // Letter-free inputs all share the empty code.
        assert!(r.compare("", "123"));
        assert!(r.compare("!!!", " "));
        assert!(!r.compare("a", ""));
        // ... and `compare` agrees with `difference` reaching both lengths.
        for (a, b) in [("Smith", "Smythe"), ("dogs", "dog"), ("", "123")] {
            let (ca, cb) = (r.process(a), r.process(b));
            assert_eq!(
                r.compare(a, b),
                ca.len() == cb.len() && r.difference(a, b) == ca.len()
            );
        }
    }

    #[test]
    fn usable_through_the_phonetic_trait() {
        fn encode_with(p: &impl verbora_core::Phonetic, token: &str) -> String {
            p.process(token)
        }
        let r = rs();
        assert_eq!(encode_with(&r, "jumped"), "J408106");
        assert!(verbora_core::Phonetic::compare(&r, "Smith", "Smythe"));
    }
}
