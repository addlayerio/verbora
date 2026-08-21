//! Russell Soundex, as codified by the U.S. National Archives.

use crate::letters::Letters;

/// Russell Soundex — one retained letter followed by three digits.
///
/// # Publication
///
/// Robert C. Russell, *Index*, U.S. Patent 1,261,167 (filed 1917, granted
/// 1918) and U.S. Patent 1,435,663 (1922). The coding rules Verbora
/// implements are the ones the U.S. National Archives and Records
/// Administration publishes for the federal census indexes, *The Soundex
/// Indexing System* — the only widely-cited normative statement of the
/// algorithm, and the source of the eleven worked examples pinned in this
/// crate's tests.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Every scalar of the input is
///   examined once, left to right.
/// * **Only the twenty-six Latin letters `A`–`Z` are coded**, matched after
///   simple ASCII case folding. Every other scalar — an accented letter, a
///   digit, a space, a hyphen, an apostrophe, a CJK ideograph, an emoji — is
///   *skipped*: it neither contributes a digit nor separates two equal
///   digits. Soundex is defined over the Roman alphabet of an English-language
///   surname index and has no code for anything else; inventing one would be
///   behaviour with no citable basis. Transliterate first (`ö` → `oe`) if you
///   want accented names to code as their Latin spelling.
/// * **The code is exactly four characters, or empty.** A token containing at
///   least one `A`–`Z` letter yields the retained first letter plus three
///   digits, zero-padded. A token containing none — `""`, `"…"`, `"日本語"` —
///   yields `""`, because there is no letter to retain. That empty string is
///   the absence of a code, not a sentinel standing in for one: no non-empty
///   input can produce it.
/// * **Total.** No input panics, and there is no error type.
///
/// # The coding rules
///
/// | Letters | Digit |
/// |---|---|
/// | `B` `F` `P` `V` | `1` |
/// | `C` `G` `J` `K` `Q` `S` `X` `Z` | `2` |
/// | `D` `T` | `3` |
/// | `L` | `4` |
/// | `M` `N` | `5` |
/// | `R` | `6` |
/// | `A` `E` `I` `O` `U` `Y` `H` `W` | *(not coded)* |
///
/// 1. The first letter is retained, uppercased, and is **not** re-emitted as a
///    digit — but its own digit still primes rule 2, which is why `Pfister` is
///    `P236` and not `P123`.
/// 2. Two letters with the same digit that are adjacent, or separated only by
///    `H` or `W`, are coded once (`Ashcraft` → `A261`: the `S` and the `C`
///    across the `H` are one `2`).
/// 3. `A` `E` `I` `O` `U` `Y` separate: two same-digit letters on either side
///    of one are coded twice (`Tymczak` → `T522`).
/// 4. The digits are truncated to three, or padded with `0` to three.
///
/// # Examples
///
/// ```
/// use verbora_phonetics::SoundEx;
///
/// let soundex = SoundEx::new();
/// assert_eq!(soundex.process("Robert"), "R163");
/// assert_eq!(soundex.process("Rupert"), "R163");
/// assert_eq!(soundex.process("Ashcraft"), "A261");
/// assert!(soundex.compare("Robert", "Rupert"));
///
/// // No Latin letter, so no code at all.
/// assert_eq!(soundex.process("日本語"), "");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SoundEx;

/// The Soundex digit for an uppercase ASCII letter, or `None` when the letter
/// is one of the eight the table leaves uncoded.
#[inline]
const fn digit(letter: u8) -> Option<u8> {
    match letter {
        b'B' | b'F' | b'P' | b'V' => Some(b'1'),
        b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => Some(b'2'),
        b'D' | b'T' => Some(b'3'),
        b'L' => Some(b'4'),
        b'M' | b'N' => Some(b'5'),
        b'R' => Some(b'6'),
        _ => None,
    }
}

/// How a letter affects the "same digit already seen" state of rule 2.
enum Step {
    /// A coded letter: emit its digit unless the previous coded letter, across
    /// any run of `H`/`W`, carried the same one.
    Coded(u8),
    /// `H` or `W`: transparent. Rule 2 explicitly reaches across it.
    Transparent,
    /// A vowel (`A` `E` `I` `O` `U` `Y`): rule 3's separator. It clears the
    /// memory of the previous digit, so the same digit may be emitted again.
    Separator,
}

#[inline]
const fn classify(letter: u8) -> Step {
    match digit(letter) {
        Some(d) => Step::Coded(d),
        None if letter == b'H' || letter == b'W' => Step::Transparent,
        None => Step::Separator,
    }
}

impl SoundEx {
    /// Creates a Soundex encoder.
    ///
    /// The encoder is stateless and zero-sized; the type exists so that
    /// [`verbora_core::Phonetic`] can be implemented for it and so that
    /// call sites read as `soundex.process(word)`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as its four-character Soundex code.
    ///
    /// Returns `""` — and only then — when `token` contains no `A`–`Z`
    /// letter. See the [type documentation](Self) for the coding rules and
    /// the text unit.
    ///
    /// ```
    /// use verbora_phonetics::SoundEx;
    ///
    /// let soundex = SoundEx::new();
    /// // NARA's own worked examples.
    /// assert_eq!(soundex.process("Washington"), "W252");
    /// assert_eq!(soundex.process("Lee"), "L000");
    /// assert_eq!(soundex.process("Gutierrez"), "G362");
    /// assert_eq!(soundex.process("Pfister"), "P236");
    /// assert_eq!(soundex.process("Tymczak"), "T522");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let mut out = String::with_capacity(4);
        self.process_into(token, &mut out);
        out
    }

    /// Appends `token`'s Soundex code to `out`.
    ///
    /// Appends nothing when `token` has no `A`–`Z` letter, exactly as
    /// [`SoundEx::process`] returns `""` for the same input. `out` is never
    /// cleared, so a caller accumulating many codes into one buffer keeps what
    /// is already there.
    ///
    /// # Choosing the right API
    ///
    /// | | [`process`](Self::process) | `process_into` |
    /// |---|---|---|
    /// | Use case | one word, one code | encoding a dictionary into a buffer you already own |
    /// | Allocation | one `String` per call | none, once `out` has grown |
    /// | Trade-off | none | you manage `out`, including clearing it when you want one code at a time |
    /// | Recommendation | **the default** | reach for it only when a profile shows the per-call `String` matters |
    pub fn process_into(&self, token: &str, out: &mut String) {
        let mut letters = Letters::new(token);
        let Some(first) = letters.next() else {
            return;
        };
        out.push(char::from(first));

        // Rule 1: the retained letter's own digit primes rule 2 without being
        // emitted. `Pfister` -> P236, not P123.
        let mut previous = digit(first);
        let mut digits = 0;
        for letter in letters {
            if digits == 3 {
                break;
            }
            match classify(letter) {
                Step::Coded(d) => {
                    if previous != Some(d) {
                        out.push(char::from(d));
                        digits += 1;
                    }
                    previous = Some(d);
                }
                // Rule 2 reaches across `H` and `W`, so they change nothing.
                Step::Transparent => {}
                // Rule 3: a vowel breaks the run, so the same digit may repeat.
                Step::Separator => previous = None,
            }
        }

        // Rule 4: pad to three digits.
        for _ in digits..3 {
            out.push('0');
        }
    }

    /// Whether `a` and `b` share a Soundex code.
    ///
    /// Two tokens with no `A`–`Z` letter both encode to `""` and therefore
    /// compare equal; that is the honest reading of "same code", since neither
    /// carries a name Soundex can index.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl verbora_core::Phonetic for SoundEx {
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

    /// Every worked example NARA publishes with *The Soundex Indexing System*,
    /// transcribed from the standard rather than from this implementation.
    ///
    /// Each one exercises a different clause: `Washington` rule 4's
    /// truncation, `Lee` rule 4's padding, `Pfister` rule 1's priming,
    /// `Ashcraft` rule 2's reach across `H`, `Tymczak` rule 3's separation.
    #[test]
    fn nara_worked_examples() {
        let s = SoundEx::new();
        for (input, want) in [
            ("Washington", "W252"),
            ("Lee", "L000"),
            ("Gutierrez", "G362"),
            ("Pfister", "P236"),
            ("Jackson", "J250"),
            ("Tymczak", "T522"),
            ("VanDeusen", "V532"),
            ("Ashcraft", "A261"),
            ("Robert", "R163"),
            ("Rupert", "R163"),
            ("Honeyman", "H555"),
        ] {
            assert_eq!(s.process(input), want, "for {input:?}");
        }
    }

    /// Rule 2 is stated as "adjacent, **or separated only by `H` or `W`**".
    /// Both halves need their own witness, because an implementation that
    /// dropped `H`/`W` before deduplicating would pass the adjacent case and
    /// fail this one — that is exactly the difference between `A261` and the
    /// `A226` a "condense first, filter later" order produces.
    #[test]
    fn rule_two_reaches_across_h_and_w() {
        let s = SoundEx::new();
        // S(2) H C(2): one 2.  R(6). A separates. F(1). T truncated.
        assert_eq!(s.process("Ashcraft"), "A261");
        // S(2) W C(2): one 2, same as across an H.
        assert_eq!(s.process("Aswcraft"), "A261");
        // Adjacent, no separator at all: S(2) C(2) -> one 2.
        assert_eq!(s.process("Ascraft"), "A261");
        // With a vowel between them rule 3 wins and both are coded: 2, 2, 6.
        assert_eq!(s.process("Asocraft"), "A226");
    }

    /// Rule 3's separators are the five vowels **and `Y`**; `H` and `W` are
    /// not separators. `Honeyman` is NARA's own witness for `Y`: N(5), Y
    /// separates, M(5), A separates, N(5) — three fives, `H555`.
    #[test]
    fn rule_three_separators_are_the_vowels_and_y() {
        let s = SoundEx::new();
        assert_eq!(s.process("Honeyman"), "H555");
        // Without the Y the two 5s would merge: N(5) M(5) adjacent -> one 5.
        assert_eq!(s.process("Honman"), "H550");
    }

    /// Rule 1: the retained letter primes the duplicate check but is not
    /// itself a digit. `Pfister` is NARA's witness; the negative control is a
    /// first letter whose digit differs from the second letter's.
    #[test]
    fn rule_one_primes_the_duplicate_check() {
        let s = SoundEx::new();
        // P(1) F(1) -> the F is swallowed. S(2) T(3) E R(6) -> 236.
        assert_eq!(s.process("Pfister"), "P236");
        // B(1) L(4): different digits, so the L is coded. A C(2) K(2) -> 42.
        assert_eq!(s.process("Black"), "B420");
    }

    /// Rule 4 in both directions, and the boundary where truncation begins.
    #[test]
    fn rule_four_pads_and_truncates_to_three_digits() {
        let s = SoundEx::new();
        assert_eq!(s.process("Lee"), "L000"); // zero digits
        assert_eq!(s.process("Ely"), "E400"); // one
        assert_eq!(s.process("Elm"), "E450"); // two
        assert_eq!(s.process("Elms"), "E452"); // exactly three
        assert_eq!(s.process("Elmset"), "E452"); // four, truncated
        assert_eq!(s.process(&"b".repeat(500)), "B000");
    }

    /// The text unit and the skip rule, enumerated over one scalar of every
    /// class the contract names. A skipped scalar is *transparent*: it may not
    /// act as a rule-3 separator, or `"a-b"` would differ from `"ab"`.
    #[test]
    fn only_ascii_letters_are_coded_and_everything_else_is_skipped() {
        let s = SoundEx::new();

        // No A-Z letter anywhere: no code at all.
        for empty in ["", " ", "...", "1234", "日本語", "😀", "Москва", "\u{301}"] {
            assert_eq!(s.process(empty), "", "for {empty:?}");
        }

        // A skipped scalar neither codes nor separates.
        assert_eq!(s.process("a-b"), s.process("ab"));
        assert_eq!(s.process("O'Brien"), s.process("OBrien"));
        assert_eq!(s.process("caf\u{e9}"), s.process("caf")); // é is skipped
        assert_eq!(s.process("na\u{ef}ve"), s.process("nave"));
        // An astral scalar is one unit and is skipped like any other non-letter.
        assert_eq!(s.process("R\u{1F600}obert"), "R163");
        // A digit is not a letter, and does not leak into the code.
        assert_eq!(s.process("12345"), "");
        assert_eq!(s.process("R2D2"), "R300");
    }

    /// Case folding is simple ASCII folding, so the retained letter is always
    /// one uppercase ASCII byte and the code is always four bytes.
    #[test]
    fn case_folding_is_ascii_and_the_code_is_always_four_bytes() {
        let s = SoundEx::new();
        assert_eq!(s.process("BLACKBERRY"), s.process("blackberry"));
        assert_eq!(s.process("BlAcKbErRy"), "B421");
        // `ß` uppercases to `SS` in Unicode; it is not an A-Z letter here, so
        // it is skipped outright and cannot lengthen the code.
        assert_eq!(s.process("\u{df}"), "");
        for word in ["Robert", "\u{df}x", "a", "Zzzzzz"] {
            let code = s.process(word);
            assert!(code.is_empty() || code.len() == 4, "for {word:?}: {code:?}");
        }
    }

    #[test]
    fn single_letter_inputs() {
        let s = SoundEx::new();
        assert_eq!(s.process("a"), "A000");
        assert_eq!(s.process("Z"), "Z000");
        assert_eq!(s.process("h"), "H000");
    }

    #[test]
    fn compare_is_code_equality() {
        let s = SoundEx::new();
        assert!(s.compare("Robert", "Rupert"));
        assert!(s.compare("ant", "and"));
        assert!(!s.compare("ant", "anne"));
        // Two codeless tokens share the codeless "code".
        assert!(s.compare("", "日本語"));
        assert!(!s.compare("", "a"));
    }

    #[test]
    fn process_into_appends_and_never_clears() {
        let s = SoundEx::new();
        let mut buf = String::from("keep:");
        s.process_into("Robert", &mut buf);
        s.process_into("日本語", &mut buf); // appends nothing
        s.process_into("Rupert", &mut buf);
        assert_eq!(buf, "keep:R163R163");
    }
}
