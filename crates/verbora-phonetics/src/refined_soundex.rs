//! Refined Soundex, pinned to rphonetic 3.0.6 / Apache commons-codec.
//!
//! **This is a Verbora-native extension, not a port of the JS reference** —
//! the reference `phonetics` module has no Refined Soundex. Per the crate's
//! extension pattern, behaviour is pinned to a canonical specification
//! instead: the [`RefinedSoundex`] encoder of **rphonetic 3.0.6** (the Rust
//! port of Apache commons-codec), which in turn transcribes commons-codec's
//! `org.apache.commons.codec.language.RefinedSoundex`. The lineage of the
//! algorithm itself is the Russell/Odell Soundex (Margaret K. Odell and
//! Robert C. Russell, U.S. patents 1,261,167 and 1,435,663), refined for
//! spell-checking by splitting the classic six consonant groups into ten and
//! removing the fixed four-character cap.
//!
//! # The algorithm
//!
//! 1. **Clean**: keep only the alphabetic characters of the input and
//!    uppercase them (full Unicode uppercasing, so `ß` becomes `SS`).
//! 2. **Emit**: the code is the first cleaned letter, followed by the group
//!    digit of *every* cleaned letter — the first letter included — with
//!    adjacent duplicate digits collapsed to one.
//! 3. There is **no truncation and no zero-padding**: the code grows with
//!    the word, and an input with no letters encodes to the empty string.
//!
//! The ten groups (digit → letters) of the US-English mapping:
//!
//! | digit | letters | | digit | letters |
//! |---|---|---|---|---|
//! | `0` | A E H I O U W Y | | `5` | Q X Z |
//! | `1` | B P | | `6` | D T |
//! | `2` | F V | | `7` | L |
//! | `3` | C K S | | `8` | M N |
//! | `4` | G J | | `9` | R |
//!
//! Unlike classic Soundex, vowels are not dropped — they encode as `0` and
//! therefore *separate* consonant runs: `bab` is `B101`, not `B11`.
//!
//! # Behavioural decisions
//!
//! * **US-English mapping only.** rphonetic offers `FromStr`/`TryFrom`
//!   constructors for custom 26-letter mappings; those are API affordances of
//!   that crate, not part of the algorithm, and commons-codec's shipped
//!   `US_ENGLISH` instance uses exactly the table above
//!   (`"01360240043788015936020505"`). This encoder pins that table.
//! * **`difference` is part of the surface.** commons-codec's
//!   `SoundexUtils.difference` (rphonetic's `SoundexCommons::difference`) is
//!   the standard similarity measure for this code — see
//!   [`RefinedSoundex::difference`].
//!
//! # Divergence from rphonetic (documented, deliberate)
//!
//! rphonetic's cleaning step keeps every `char::is_alphabetic()` character
//! and then indexes `mapping[ch as usize - 65]`, so any alphabetic character
//! whose Unicode uppercase form is not entirely `A`–`Z` sends it out of
//! bounds and **panics**. The exact input shapes affected: tokens containing
//! at least one alphabetic character `c` such that `c.to_uppercase()` yields
//! any character outside `A`–`Z` — e.g. `é` (→ `É`), `ñ`, Cyrillic
//! (`Москва`), CJK (`日本語`), `İ` (U+0130), `ʼn` (U+0149, → `ʼN`), or the
//! Kelvin sign (U+212A). This library must not panic on arbitrary text, so
//! such characters are instead **dropped as if absent from the input**
//! (they do not break a duplicate-digit run: `aéa` → `A0`). Inputs of these
//! shapes lie outside rphonetic's accepted domain and are excluded from the
//! benchmark comparison, per the crate's fairness pattern. Every input
//! rphonetic accepts without panicking encodes byte-identically here —
//! including uppercase expansions that stay inside `A`–`Z` (`ß` → `SS` →
//! `S3`, `ﬁ` → `FI`, `ſ` → `S`) and non-alphabetic Unicode such as emoji,
//! which both implementations simply filter out.

/// Group digit for each letter, indexed by `letter - b'A'`.
///
/// This is commons-codec's `US_ENGLISH_MAPPING_STRING`
/// (`"01360240043788015936020505"`), byte-for-byte the `ENGLISH_MAPPING`
/// table in rphonetic 3.0.6's `refined_soundex.rs`.
const MAPPING: [u8; 26] = *b"01360240043788015936020505";

/// Refined Soundex encoder (commons-codec variant, US-English mapping).
///
/// Codes retain the initial letter, use ten consonant groups, collapse
/// adjacent duplicate digits, and are never truncated or padded. Byte-network
/// identical to rphonetic 3.0.6's `RefinedSoundex::default()` on every input
/// that crate accepts (see the module docs for the one divergence, on inputs
/// where rphonetic panics).
///
/// ```
/// use verbora_phonetics::refined_soundex::RefinedSoundex;
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
    /// use verbora_phonetics::refined_soundex::RefinedSoundex;
    ///
    /// assert_eq!(RefinedSoundex::new(), RefinedSoundex::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` into its Refined Soundex code.
    ///
    /// Returns the empty string when the input contains no letters (rphonetic
    /// does the same: `""`, `" "`, `"123"` and `"😀"` all encode to `""`).
    ///
    /// ```
    /// use verbora_phonetics::refined_soundex::RefinedSoundex;
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

        if token.is_ascii() {
            // Fast path: one forward scan over bytes, no intermediate
            // cleaned string (rphonetic allocates one and iterates chars).
            for &b in token.as_bytes() {
                if !(b | 0x20).is_ascii_lowercase() {
                    continue; // not a letter: cleaned away
                }
                let upper = b & !0x20;
                if out.is_empty() {
                    out.push(char::from(upper));
                }
                let code = MAPPING[(upper - b'A') as usize];
                if code != previous {
                    out.push(char::from(code));
                    previous = code;
                }
            }
        } else {
            // Slow path, still a single forward scan. Mirrors rphonetic's
            // clean (filter `is_alphabetic`, full uppercase) exactly, except
            // that uppercase characters outside A–Z are dropped where
            // rphonetic would panic — see the module docs.
            for c in token.chars() {
                if !c.is_alphabetic() {
                    continue;
                }
                for u in c.to_uppercase() {
                    if !u.is_ascii_uppercase() {
                        continue; // out of rphonetic's accepted domain
                    }
                    if out.is_empty() {
                        out.push(u);
                    }
                    let code = MAPPING[(u as u8 - b'A') as usize];
                    if code != previous {
                        out.push(char::from(code));
                        previous = code;
                    }
                }
            }
        }

        out
    }

    /// Whether two strings share a Refined Soundex code.
    ///
    /// Mirrors rphonetic's `Encoder::is_encoded_equals`. Note that two
    /// letter-free inputs both encode to `""` and therefore compare equal.
    ///
    /// ```
    /// use verbora_phonetics::refined_soundex::RefinedSoundex;
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
    /// commons-codec's `difference`, via rphonetic's
    /// `SoundexCommons::difference`.
    ///
    /// 0 means no similarity. Unlike classic Soundex (capped at 4 by its
    /// four-character codes), Refined Soundex codes are unbounded, so the
    /// difference can be arbitrarily large. Positions beyond the shorter
    /// code's length are not counted.
    ///
    /// ```
    /// use verbora_phonetics::refined_soundex::RefinedSoundex;
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
        // bytes is comparing chars. rphonetic early-returns 0 when either
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

    // Ported from rphonetic 3.0.6 `refined_soundex.rs::tests::test_encode`
    // (itself from commons-codec's RefinedSoundexTest). Every expected value
    // re-verified against rphonetic 3.0.6 directly.
    #[test]
    fn encodes_the_rphonetic_vectors() {
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

    // Ported from rphonetic 3.0.6 `refined_soundex.rs::tests::test_difference`
    // (itself from commons-codec's RefinedSoundexTest / the SQL Server
    // DIFFERENCE examples in commons-codec's javadoc).
    #[test]
    fn difference_matches_rphonetic() {
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

    // Digit of every letter, pinned one by one (verified against rphonetic
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

    // Verified against rphonetic 3.0.6.
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

    // Verified against rphonetic 3.0.6.
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

    // Verified against rphonetic 3.0.6.
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

    // Verified against rphonetic 3.0.6: these Unicode inputs are inside its
    // accepted domain (uppercase folds into A-Z, or the char is filtered).
    #[test]
    fn unicode_inside_rphonetic_domain_is_byte_identical() {
        let r = rs();
        assert_eq!(r.process("ß"), "S3"); // ß uppercases to SS
        assert_eq!(r.process("ﬁsh"), "F2030"); // ﬁ ligature uppercases to FI
        assert_eq!(r.process("ſmith"), "S38060"); // long s uppercases to S
        assert_eq!(r.process("ſmith"), r.process("Smith"));
        assert_eq!(r.process("a😀b"), "A01"); // emoji is not alphabetic
    }

    // These inputs make rphonetic 3.0.6 PANIC (out-of-bounds mapping index);
    // the module docs pin our documented divergence: such characters are
    // dropped as if absent. Excluded from the benchmark domain.
    #[test]
    fn out_of_domain_letters_are_dropped_without_panicking() {
        let r = rs();
        assert_eq!(r.process("café"), "C302"); // é dropped
        assert_eq!(r.process("naïve"), "N8020"); // ï dropped
        assert_eq!(r.process("Ñ"), ""); // nothing left
        assert_eq!(r.process("Москва"), "");
        assert_eq!(r.process("日本語"), "");
        assert_eq!(r.process("İstanbul"), "S3608107"); // İ dropped, S leads
        assert_eq!(r.process("ʼn"), "N8"); // uppercases to ʼN; ʼ dropped
        assert_eq!(r.process("a\u{212A}a"), "A0"); // Kelvin sign dropped...
        assert_eq!(r.process("aéa"), "A0"); // ...and drops don't break runs
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
