//! Refined Soundex (Apache Commons Codec's US-English mapping).

/// Group digit for each letter, indexed by `letter - b'A'`.
///
/// Apache Commons Codec's `US_ENGLISH_MAPPING_STRING`,
/// `"01360240043788015936020505"` — see [`RefinedSoundex`] on why that
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
/// [`RefinedSoundex::difference`] is Commons Codec's companion similarity
/// measure for this code.
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
    /// Returns the empty string when the input contains no letters (the
    /// does the same: `""`, `" "`, `"123"` and `"😀"` all encode to `""`).
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

    /// Number of positions at which the two codes carry the same character —
    /// commons-codec's `difference`, via Commons Codec's
    /// `SoundexCommons::difference`.
    ///
    /// 0 means no similarity. Unlike classic Soundex (capped at 4 by its
    /// four-character codes), Refined Soundex codes are unbounded, so the
    /// difference can be arbitrarily large. Positions beyond the shorter
    /// code's length are not counted.
    ///
    /// ```
    /// use verbora_phonetics::RefinedSoundex;
    ///
    /// let refined = RefinedSoundex::new();
    /// // Low similarity
    /// assert_eq!(refined.difference("Margaret", "Andrew"), 1);
    /// // High similarity
    /// assert_eq!(refined.difference("Smithers", "Smythers"), 8);
    /// ```
    #[must_use]
    pub fn difference(&self, a: &str, b: &str) -> usize {
        let a = self.process(a);
        let b = self.process(b);
        // Codes are pure ASCII (an A–Z letter then digits), so comparing
        // bytes is comparing chars. Commons Codec early-returns 0 when either
        // code is empty; an empty zip counts 0 all the same.
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

    // From Apache Commons Codec's `RefinedSoundexTest`, the distribution
    // this mapping table comes from. (was: Apache Commons Codec `refined_soundex.rs::tests::test_encode`
    // (itself from commons-codec's RefinedSoundexTest). Every expected value
    // re-verified against Apache Commons Codec directly.
    #[test]
    fn encodes_the_commons_codec_vectors() {
        let r = rs();
        for (input, want) in [
            ("testing", "T6036084"),
            ("TESTING", "T6036084"),
            ("The", "T60"),
            ("quick", "Q503"),
            ("brown", "B1908"),
            ("fox", "F205"),
            ("jumped", "J408106"),
            ("over", "O0209"),
            ("the", "T60"),
            ("lazy", "L7050"),
            ("dogs", "D6043"),
        ] {
            assert_eq!(r.process(input), want, "for {input:?}");
        }
    }

    // Ported from Apache Commons Codec `refined_soundex.rs::tests::test_difference`
    // (itself from commons-codec's RefinedSoundexTest / the SQL Server
    // DIFFERENCE examples in commons-codec's javadoc).
    #[test]
    fn difference_matches_the_commons_codec_measure() {
        let r = rs();
        assert_eq!(r.difference("", ""), 0);
        assert_eq!(r.difference(" ", " "), 0);
        assert_eq!(r.difference("Smith", "Smythe"), 6);
        assert_eq!(r.difference("Ann", "Andrew"), 3);
        assert_eq!(r.difference("Margaret", "Andrew"), 1);
        assert_eq!(r.difference("Janet", "Margaret"), 1);
        assert_eq!(r.difference("Green", "Greene"), 5);
        assert_eq!(r.difference("Blotchet-Halls", "Greene"), 1);
        assert_eq!(r.difference("Smith", "Smythe"), 6);
        assert_eq!(r.difference("Smithers", "Smythers"), 8);
        assert_eq!(r.difference("Anothers", "Brothers"), 5);
    }

    #[test]
    fn difference_extremes() {
        let r = rs();
        // Identical input: every position matches (8-character code).
        assert_eq!(r.difference("testing", "testing"), 8);
        // A shared prefix counts up to the shorter code.
        assert_eq!(r.difference("dogs", "dog"), 4); // D6043 vs D604
        // One empty side contributes nothing.
        assert_eq!(r.difference("Smith", ""), 0);
        assert_eq!(r.difference("", "Smith"), 0);
    }

    // Digit of every letter, pinned one by one (verified against Commons Codec
    // 3.0.6: encode of each single letter). This is the whole mapping table.
    #[test]
    fn alphabet_sweep_pins_every_group_digit() {
        let r = rs();
        for (letter, want) in [
            ("A", "A0"),
            ("B", "B1"),
            ("C", "C3"),
            ("D", "D6"),
            ("E", "E0"),
            ("F", "F2"),
            ("G", "G4"),
            ("H", "H0"),
            ("I", "I0"),
            ("J", "J4"),
            ("K", "K3"),
            ("L", "L7"),
            ("M", "M8"),
            ("N", "N8"),
            ("O", "O0"),
            ("P", "P1"),
            ("Q", "Q5"),
            ("R", "R9"),
            ("S", "S3"),
            ("T", "T6"),
            ("U", "U0"),
            ("V", "V2"),
            ("W", "W0"),
            ("X", "X5"),
            ("Y", "Y0"),
            ("Z", "Z5"),
        ] {
            assert_eq!(r.process(letter), want, "for {letter:?}");
            // And the lowercase form encodes identically.
            assert_eq!(
                r.process(&letter.to_lowercase()),
                want,
                "for lowercase {letter:?}"
            );
        }
    }

    #[test]
    fn mapping_table_is_the_commons_codec_us_english_string() {
        assert_eq!(&MAPPING, b"01360240043788015936020505");
    }

    #[test]
    fn letter_free_input_encodes_to_the_empty_string() {
        let r = rs();
        for input in ["", " ", "  ", "\t\n", "123", "!!!", "1-2-3", "___", "😀"] {
            assert_eq!(r.process(input), "", "for {input:?}");
        }
    }

    // Verified against Apache Commons Codec.
    #[test]
    fn non_letters_are_cleaned_away_not_separators() {
        let r = rs();
        assert_eq!(r.process("b4t"), "B16"); // digit vanishes between letters
        assert_eq!(r.process("the quick"), "T60503"); // space vanishes
        assert_eq!(r.process("a-b"), "A01");
        assert_eq!(r.process("x y"), "X50");
        assert_eq!(r.process("O'Brien"), "O01908");
        assert_eq!(r.process("don't"), "D6086");
        assert_eq!(r.process("d'Artagnan"), "D609604808");
        assert_eq!(r.process("McDonald"), "M83608076");
        assert_eq!(r.process("van der Berg"), "V2086091094");
        assert_eq!(r.process("Blotchet-Halls"), "B1706306073");
        // Cleaning happens before duplicate collapse, so a non-letter between
        // two same-group letters does NOT keep both digits: "a-b" above, and
        assert_eq!(r.process("b b"), "B1");
    }

    #[test]
    fn mixed_case_is_case_insensitive() {
        let r = rs();
        assert_eq!(r.process("TeStInG"), "T6036084");
        assert_eq!(r.process("SMITH"), r.process("smith"));
        assert_eq!(r.process("McDonald"), r.process("mcdonald"));
    }

    // Verified against Apache Commons Codec.
    #[test]
    fn adjacent_duplicate_digits_collapse() {
        let r = rs();
        assert_eq!(r.process("bb"), "B1");
        assert_eq!(r.process("aaa"), "A0");
        assert_eq!(r.process("mn"), "M8"); // same group, different letters
        assert_eq!(r.process("bpbp"), "B1"); // a whole run of group 1
        assert_eq!(r.process("bab"), "B101"); // vowel separates the run
        // Vowels/H/W encode as 0 rather than being dropped, so unlike
        // classic Soundex they break consonant runs but also run together
        // themselves: Ashcraft = A,s,h,c,r,a,f,t = 0,3,0,3,9,0,2,6.
        assert_eq!(r.process("Ashcraft"), "A03039026");
    }

    // Verified against Apache Commons Codec.
    #[test]
    fn no_truncation_and_no_padding() {
        let r = rs();
        // Single letters give two-character codes; nothing pads them out.
        assert_eq!(r.process("a"), "A0");
        // Long words keep growing.
        assert_eq!(
            r.process("supercalifragilisticexpialidocious"),
            "S3010930702904070360305107060303"
        );
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
        // Every code is ASCII.
        for input in ["caf\u{e9}", "\u{65e5}\u{672c}\u{8a9e}", "jumped"] {
            assert!(r.process(input).is_ascii(), "for {input:?}");
        }
    }

    #[test]
    fn compare_is_code_equality() {
        let r = rs();
        assert!(r.compare("Smith", "Smythe"));
        assert!(r.compare("Smithers", "Smythers"));
        assert!(r.compare("bpbp", "b"));
        assert!(!r.compare("dogs", "dog"));
        assert!(!r.compare("Green", "Greene"));
        // Letter-free inputs all share the empty code.
        assert!(r.compare("", "123"));
        assert!(r.compare("!!!", " "));
        assert!(!r.compare("a", ""));
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
