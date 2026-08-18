//! Cologne phonetics (Kölner Phonetik) — the German-language analogue of
//! SoundEx.
//!
//! This is a **Verbora-native extension**: the algorithm is not in the JS
//! reference the rest of this crate ports. Its behavior is instead pinned,
//! byte for byte, to the `Cologne` encoder of `rphonetic` 3.0.6 (the Rust
//! port of Apache commons-codec's `ColognePhonetic`, and the crate this
//! encoder is benchmarked against) over rphonetic's full accepted input
//! domain. No divergence from rphonetic was found or introduced. The
//! algorithm itself is Hans Joachim Postel's: "Die Kölner Phonetik. Ein
//! Verfahren zur Identifizierung von Personennamen auf der Grundlage der
//! Gestaltanalyse", *IBM-Nachrichten* 19 (1969), pp. 925–931.
//!
//! Like SoundEx, the encoder maps a word to a string of digits so that
//! similar-sounding words collide, but it is tuned to German orthography:
//! the code is not truncated to a fixed length, vowels survive only at the
//! very front of the code, and `C`, `D`, `T`, `P` and `X` encode differently
//! depending on their neighbors.
//!
//! # The code table
//!
//! After uppercasing and umlaut folding, each letter contributes:
//!
//! | Letter | Context | Code |
//! |---|---|---|
//! | A, E, I, J, O, U, Y | — | `0` |
//! | B | — | `1` |
//! | P | not before H | `1` |
//! | D, T | not before C, S, Z | `2` |
//! | F, V, W | — | `3` |
//! | P | before H | `3` |
//! | G, K, Q | — | `4` |
//! | C | at code start, before A, H, K, L, O, Q, R, U, X | `4` |
//! | C | mid-word, not after S/Z, before A, H, K, O, Q, U, X | `4` |
//! | X | not after C, K, Q | `48` |
//! | L | — | `5` |
//! | M, N | — | `6` |
//! | R | — | `7` |
//! | S, Z | — | `8` |
//! | C | after S or Z, or in no `4` context above | `8` |
//! | D, T | before C, S, Z | `8` |
//! | X | after C, K, Q | `8` |
//! | H | — | nothing |
//!
//! "At code start" is literally *while the output buffer is still empty* —
//! a word-initial `H` produces nothing, so the `C` in `"hc"` still takes the
//! initial branch. This matches rphonetic exactly.
//!
//! # Behavioral decisions (each pinned to rphonetic 3.0.6 by a test below)
//!
//! * The input is Unicode-uppercased **first**, and only then are `Ä`, `Ö`,
//!   `Ü` folded to `A`, `O`, `U` (rphonetic uppercases the whole string and
//!   then runs three `replace` passes; folding each uppercased character is
//!   the same function, computed in one pass). `ß` uppercases to `SS` and
//!   encodes as `8`; the *capital* sharp s `ẞ` (U+1E9E) uppercases to
//!   itself, is not ASCII, and is therefore silently skipped — an rphonetic
//!   asymmetry this port reproduces.
//! * Only ASCII `A`–`Z` are encoded. Every other character — digits,
//!   punctuation, whitespace, accented Latin, Cyrillic, CJK, emoji — is
//!   skipped without panicking, but it is still *visible to the
//!   one-character lookahead* (`"p1h"` encodes `P` as `1`, not `3`, because
//!   the peeked character is `'1'`), and it does **not** update the
//!   preceding-letter context used by the `C` and `X` rules (in `"c-x"` the
//!   `X` still sees `C` as its predecessor).
//! * At the end of input the lookahead character is `'-'` — the same
//!   sentinel an *input* hyphen presents, so a trailing `C` and the `C` of
//!   `"c-"` encode identically (`8`). This quirk is rphonetic's.
//! * A vowel's `0` survives only while the output is still empty; elsewhere
//!   it is dropped — but it still separates equal digits, so `"sas"` is
//!   `"88"`, not `"8"`.
//! * `H` emits nothing but resets duplicate collapsing (its ignore-marker is
//!   recorded as the last emitted code), so `"shs"` is also `"88"`.
//! * Adjacent equal codes collapse to one, and characters that were skipped
//!   do not break the adjacency: `"Test test"` is `"28282"` — the two
//!   middle `T`s merge across the space.
//! * The encoder never errors and never panics. The empty string, and any
//!   input with no encodable letters, encode to `""`.
//!
//! ```
//! use verbora_phonetics::cologne::Cologne;
//!
//! let cologne = Cologne::new();
//! assert_eq!(cologne.process("Müller"), "657");
//! assert_eq!(cologne.process("Wikipedia"), "3412");
//! assert!(cologne.compare("Meyer", "Mayr"));
//! ```

/// The ignore-marker code. `H` emits it: nothing is appended, but it becomes
/// the last-emitted code and thereby resets duplicate collapsing. It doubles
/// as the end-of-input lookahead sentinel, exactly as in rphonetic.
const IGNORE: u8 = b'-';

/// Appends `code` to `out` under the Cologne output rules, and records it as
/// the last emitted code either way.
///
/// A code is appended unless it is the ignore-marker, equal to the previous
/// code (adjacent duplicates collapse), or a `0` past the front of the code
/// (vowels survive only while the output is still empty). This is a faithful
/// transcription of rphonetic's `CologneOutput::push`.
#[inline]
fn emit(out: &mut String, last: &mut u8, code: u8) {
    if code != IGNORE && *last != code && (code != b'0' || out.is_empty()) {
        out.push(char::from(code));
    }
    *last = code;
}

/// Folds the three uppercase umlauts to their base vowels.
///
/// rphonetic runs `replace('Ä', "A")` (and `Ö`, `Ü`) over the whole
/// uppercased string; since `str::to_uppercase` maps characters
/// independently, folding each uppercased character is the identical
/// function without the three intermediate `String`s.
#[inline]
const fn fold_umlaut(c: char) -> char {
    match c {
        'Ä' => 'A',
        'Ö' => 'O',
        'Ü' => 'U',
        _ => c,
    }
}

/// The whole encoder, over an already uppercased-and-folded character
/// stream.
///
/// One forward pass with a single character of lookahead — the same shape as
/// rphonetic's `peekable` loop, minus its four intermediate `String`
/// allocations. `capacity` seeds the one `String` this call allocates.
fn encode_stream(mut chars: impl Iterator<Item = char>, capacity: usize) -> String {
    let mut out = String::with_capacity(capacity);
    // rphonetic seeds its output's last-char with '/'; any non-code byte
    // works, it only has to differ from every real code and from IGNORE.
    let mut last_code: u8 = b'/';
    // The previously *encoded* letter (context for the C and X rules).
    // Skipped characters do not touch it; rphonetic starts it at '-'.
    let mut prev: char = '-';

    let mut cur = chars.next();
    while let Some(ch) = cur {
        let next = chars.next();
        if ch.is_ascii_uppercase() {
            // rphonetic: `iterator.peek().unwrap_or(&CHAR_IGNORE)` — the raw
            // next character of the normalized string, letter or not.
            let peek = next.unwrap_or('-');
            match ch {
                'A' | 'E' | 'I' | 'J' | 'O' | 'U' | 'Y' => emit(&mut out, &mut last_code, b'0'),
                'B' => emit(&mut out, &mut last_code, b'1'),
                'P' => {
                    let code = if peek == 'H' { b'3' } else { b'1' };
                    emit(&mut out, &mut last_code, code);
                }
                'D' | 'T' => {
                    let code = if matches!(peek, 'C' | 'S' | 'Z') {
                        b'8'
                    } else {
                        b'2'
                    };
                    emit(&mut out, &mut last_code, code);
                }
                'F' | 'V' | 'W' => emit(&mut out, &mut last_code, b'3'),
                'G' | 'K' | 'Q' => emit(&mut out, &mut last_code, b'4'),
                'X' => {
                    if matches!(prev, 'C' | 'K' | 'Q') {
                        emit(&mut out, &mut last_code, b'8');
                    } else {
                        emit(&mut out, &mut last_code, b'4');
                        emit(&mut out, &mut last_code, b'8');
                    }
                }
                'S' | 'Z' => emit(&mut out, &mut last_code, b'8'),
                'C' => {
                    let code = if out.is_empty() {
                        // "Code start" is an EMPTY OUTPUT, not position zero:
                        // letters before this C may all have been H.
                        if matches!(peek, 'A' | 'H' | 'K' | 'L' | 'O' | 'Q' | 'R' | 'U' | 'X') {
                            b'4'
                        } else {
                            b'8'
                        }
                    } else if matches!(prev, 'S' | 'Z')
                        || !matches!(peek, 'A' | 'H' | 'K' | 'O' | 'Q' | 'U' | 'X')
                    {
                        b'8'
                    } else {
                        b'4'
                    };
                    emit(&mut out, &mut last_code, code);
                }
                'R' => emit(&mut out, &mut last_code, b'7'),
                'L' => emit(&mut out, &mut last_code, b'5'),
                'M' | 'N' => emit(&mut out, &mut last_code, b'6'),
                'H' => emit(&mut out, &mut last_code, IGNORE),
                // The arms above cover all of A–Z, and `ch` passed
                // `is_ascii_uppercase`.
                _ => unreachable!("every ASCII uppercase letter has a Cologne rule"),
            }
            prev = ch;
        }
        cur = next;
    }
    out
}

/// Cologne phonetics (Kölner Phonetik) encoder.
///
/// Byte-identical to rphonetic 3.0.6's `Cologne` on every input; see the
/// [module documentation](self) for the rules and provenance.
///
/// ```
/// use verbora_phonetics::cologne::Cologne;
///
/// let cologne = Cologne::new();
/// assert_eq!(cologne.process("Müller"), "657");
/// assert_eq!(cologne.process("schmidt"), "862");
/// assert_eq!(cologne.process("Breschnew"), "17863");
/// assert!(cologne.compare("ganz", "Gans"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cologne;

impl Cologne {
    /// Creates a Cologne encoder. It holds no state; the algorithm has no
    /// parameters (no maximum code length — Cologne codes are unbounded).
    ///
    /// ```
    /// use verbora_phonetics::cologne::Cologne;
    ///
    /// let cologne = Cologne::new();
    /// assert_eq!(cologne.process("schneider"), "8627");
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as its Cologne phonetic code.
    ///
    /// Any input is accepted; characters with no Cologne rule are skipped
    /// (they never panic and never emit), so an input with no encodable
    /// letter yields `""`. At most one `String` — the returned code — is
    /// allocated per call.
    ///
    /// ```
    /// use verbora_phonetics::cologne::Cologne;
    ///
    /// let cologne = Cologne::new();
    /// assert_eq!(cologne.process("Wikipedia"), "3412");
    /// assert_eq!(cologne.process("Xanthippe"), "48621");
    /// assert_eq!(cologne.process("bergisch-gladbach"), "174845214");
    /// assert_eq!(cologne.process(""), "");
    /// assert_eq!(cologne.process("東京 123 🙂"), "");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        if token.is_ascii() {
            // ASCII fast path: uppercase byte-by-byte, no umlauts possible.
            encode_stream(
                token.bytes().map(|b| char::from(b.to_ascii_uppercase())),
                token.len(),
            )
        } else {
            // Full Unicode path: per-character uppercasing can expand
            // (ß → "SS"), then the umlaut fold. Identical, character for
            // character, to rphonetic's `to_uppercase()` + three `replace`s.
            encode_stream(
                token.chars().flat_map(char::to_uppercase).map(fold_umlaut),
                token.len(),
            )
        }
    }

    /// Whether two strings share a Cologne code.
    ///
    /// Mirrors rphonetic's `is_encoded_equals` (code equality on
    /// [`Self::process`]).
    ///
    /// ```
    /// use verbora_phonetics::cologne::Cologne;
    ///
    /// let cologne = Cologne::new();
    /// assert!(cologne.compare("Meyer", "Mayr"));
    /// assert!(cologne.compare("Haus", "house"));
    /// assert!(!cologne.compare("Meyer", "Müller"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl verbora_core::Phonetic for Cologne {
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

    fn cologne() -> Cologne {
        Cologne::new()
    }

    // ------------------------------------------------------------------
    // Fixtures ported verbatim from rphonetic 3.0.6, src/cologne.rs tests
    // (themselves derived from commons-codec's ColognePhoneticTest).
    // ------------------------------------------------------------------

    #[test]
    fn aabjoe() {
        // rphonetic `test_aabjoe`.
        assert_eq!(cologne().process("Aabjoe"), "01");
    }

    #[test]
    fn aaclan() {
        // rphonetic `test_aaclan`.
        assert_eq!(cologne().process("Aaclan"), "0856");
    }

    #[test]
    fn aychlmajr_for_codec122() {
        // rphonetic `test_aychlmajr_for_codec122` (commons-codec CODEC-122).
        assert_eq!(cologne().process("Aychlmajr"), "04567");
    }

    #[test]
    fn edge_cases() {
        // rphonetic `test_edge_cases`, complete.
        let data: [(&str, &str); 31] = [
            ("a", "0"),
            ("e", "0"),
            ("i", "0"),
            ("o", "0"),
            ("u", "0"),
            ("\u{00E4}", "0"), // ä
            ("\u{00F6}", "0"), // ö
            ("\u{00FC}", "0"), // ü
            ("\u{00DF}", "8"), // ß (uppercases to SS, collapses to one 8)
            ("aa", "0"),
            ("ha", "0"),
            ("h", ""),
            ("aha", "0"),
            ("b", "1"),
            ("p", "1"),
            ("ph", "3"),
            ("f", "3"),
            ("v", "3"),
            ("w", "3"),
            ("g", "4"),
            ("k", "4"),
            ("q", "4"),
            ("x", "48"),
            ("ax", "048"),
            ("cx", "48"),
            ("l", "5"),
            ("cl", "45"),
            ("acl", "085"),
            ("mn", "6"),
            ("{mn}", "6"), // characters above 'Z' are skipped
            ("r", "7"),
        ];
        for (input, want) in data {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn examples() {
        // rphonetic `test_examples`, complete (includes commons-codec
        // CODEC-254 "shch" and CODEC-255 "xch").
        let data: [(&str, &str); 33] = [
            ("m\u{00DC}ller", "657"), // mÜller
            ("m\u{00FC}ller", "657"), // müller
            ("schmidt", "862"),
            ("schneider", "8627"),
            ("fischer", "387"),
            ("weber", "317"),
            ("wagner", "3467"),
            ("becker", "147"),
            ("hoffmann", "0366"),
            ("sch\u{00C4}fer", "837"), // schÄfer
            ("sch\u{00E4}fer", "837"), // schäfer
            ("Breschnew", "17863"),
            ("Wikipedia", "3412"),
            ("peter", "127"),
            ("pharma", "376"),
            ("m\u{00F6}nchengladbach", "664645214"), // mönchengladbach
            ("deutsch", "28"),
            ("deutz", "28"),
            ("hamburg", "06174"),
            ("hannover", "0637"),
            ("christstollen", "478256"),
            ("Xanthippe", "48621"),
            ("Zacharias", "8478"),
            ("Holzbau", "0581"),
            ("matsch", "68"),
            ("matz", "68"),
            ("Arbeitsamt", "071862"),
            ("Eberhard", "01772"),
            ("Eberhardt", "01772"),
            ("Celsius", "8588"),
            ("Ace", "08"),
            ("shch", "84"), // CODEC-254
            ("xch", "484"), // CODEC-255
        ];
        for (input, want) in data {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
        // rphonetic lists "heithabu" in the same table.
        assert_eq!(cologne().process("heithabu"), "021");
    }

    #[test]
    fn hyphenated_names() {
        // rphonetic `test_hyphen`.
        assert_eq!(cologne().process("bergisch-gladbach"), "174845214");
        assert_eq!(
            cologne().process("M\u{00FC}ller-L\u{00FC}denscheidt"),
            "65752682"
        );
    }

    #[test]
    fn compare_equal_codes() {
        // rphonetic `test_is_encode_equals`, via our `compare`.
        let data: [(&str, &str); 8] = [
            ("Muller", "M\u{00FC}ller"), // Müller
            ("Meyer", "Mayr"),
            ("house", "house"),
            ("House", "house"),
            ("Haus", "house"),
            ("ganz", "Gans"),
            ("ganz", "G\u{00E4}nse"), // Gänse
            ("Miyagi", "Miyako"),
        ];
        for (a, b) in data {
            assert!(cologne().compare(a, b), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn variations_mella() {
        // rphonetic `test_variations_mella`.
        for input in ["mella", "milah", "moulla", "mellah", "muehle", "mule"] {
            assert_eq!(cologne().process(input), "65", "for {input:?}");
        }
    }

    #[test]
    fn variations_meyer() {
        // rphonetic `test_variations_meyer`.
        for input in ["Meier", "Maier", "Mair", "Meyer", "Meyr", "Mejer", "Major"] {
            assert_eq!(cologne().process(input), "67", "for {input:?}");
        }
    }

    #[test]
    fn special_chars_between_same_letters() {
        // rphonetic `test_special_chars_between_same_letters`: skipped
        // characters do not break duplicate collapsing.
        for input in [
            "Test test",
            "Testtest",
            "Test-test",
            "TesT#Test",
            "TesT?test",
        ] {
            assert_eq!(cologne().process(input), "28282", "for {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // Hand-written edge cases (verified against rphonetic 3.0.6 by a
    // differential run over these exact inputs).
    // ------------------------------------------------------------------

    #[test]
    fn empty_and_letterless_inputs() {
        let c = cologne();
        assert_eq!(c.process(""), "");
        assert_eq!(c.process(" "), "");
        assert_eq!(c.process("   \t\n"), "");
        assert_eq!(c.process("12345"), "");
        assert_eq!(c.process("!?#--__"), "");
        assert_eq!(c.process("東京"), "");
        assert_eq!(c.process("Москва́"), ""); // Cyrillic never maps
        assert_eq!(c.process("🙂🎉"), "");
    }

    #[test]
    fn single_letters_cover_the_full_alphabet() {
        // Every letter's context-free code (H is the empty code).
        let want = [
            ("a", "0"),
            ("b", "1"),
            ("c", "8"), // lookahead is the end sentinel '-', not in AHKLOQRUX
            ("d", "2"),
            ("e", "0"),
            ("f", "3"),
            ("g", "4"),
            ("h", ""),
            ("i", "0"),
            ("j", "0"),
            ("k", "4"),
            ("l", "5"),
            ("m", "6"),
            ("n", "6"),
            ("o", "0"),
            ("p", "1"),
            ("q", "4"),
            ("r", "7"),
            ("s", "8"),
            ("t", "2"),
            ("u", "0"),
            ("v", "3"),
            ("w", "3"),
            ("x", "48"),
            ("y", "0"),
            ("z", "8"),
        ];
        for (input, code) in want {
            assert_eq!(cologne().process(input), code, "for {input:?}");
            let upper = input.to_uppercase();
            assert_eq!(cologne().process(&upper), code, "for {upper:?}");
        }
    }

    #[test]
    fn mixed_case_is_case_insensitive() {
        let c = cologne();
        assert_eq!(c.process("MüLLeR"), "657");
        assert_eq!(c.process("ScHmIdT"), "862");
        assert_eq!(c.process("WIKIPEDIA"), c.process("wikipedia"));
    }

    #[test]
    fn sharp_s_asymmetry() {
        let c = cologne();
        // ß uppercases to "SS" → one collapsed 8.
        assert_eq!(c.process("ß"), "8");
        assert_eq!(c.process("Straße"), "8278");
        assert_eq!(c.process("Strasse"), "8278");
        // Capital ẞ (U+1E9E) uppercases to itself, is not ASCII, and is
        // skipped — rphonetic behaves identically.
        assert_eq!(c.process("ẞ"), "");
        assert_eq!(c.process("ẞßẞ"), "8");
    }

    #[test]
    fn umlauts_fold_to_plain_vowels() {
        let c = cologne();
        assert_eq!(c.process("Ä"), "0");
        assert_eq!(c.process("Ö"), "0");
        assert_eq!(c.process("Ü"), "0");
        // A folded umlaut also serves as lookahead: C before ä sees 'A' and
        // takes 4 (the ä's own 0 is then dropped mid-word).
        assert_eq!(c.process("cä"), "4");
        // Other accented Latin letters are NOT folded — é is skipped.
        assert_eq!(c.process("café"), "43");
        assert_eq!(c.process("naïve"), "63");
    }

    #[test]
    fn skipped_characters_are_visible_to_lookahead_only() {
        let c = cologne();
        // P before literal 'h' is 3; before anything else (even a skipped
        // digit or space that precedes an h) it is 1.
        assert_eq!(c.process("ph"), "3");
        assert_eq!(c.process("p1h"), "1");
        assert_eq!(c.process("p h"), "1");
        // D before a skipped character is 2 even if an s follows it.
        assert_eq!(c.process("ds"), "8");
        assert_eq!(c.process("d-s"), "28");
        // But skipped characters do not update the preceding-letter context:
        // the X in "c-x" still sees C and encodes 8 (collapsing with C's 8).
        assert_eq!(c.process("cx"), "48");
        assert_eq!(c.process("c-x"), "8");
    }

    #[test]
    fn x_rule() {
        let c = cologne();
        // After C, K, Q: plain 8. Otherwise: 48.
        assert_eq!(c.process("kx"), "48"); // K→4, X→8
        assert_eq!(c.process("qx"), "48");
        assert_eq!(c.process("rx"), "748");
        assert_eq!(c.process("ax"), "048");
        // A run of X: each one re-emits "48" (prev X is not C/K/Q).
        assert_eq!(c.process("xx"), "4848");
        assert_eq!(c.process("xxxx"), "48484848");
        // G is NOT in the C/K/Q set, but its 4 collapses with X's 4.
        assert_eq!(c.process("gx"), "48");
    }

    #[test]
    fn c_rule_initial_versus_interior() {
        let c = cologne();
        // Word-initial C: 4 before A,H,K,L,O,Q,R,U,X — note L and R.
        assert_eq!(c.process("ca"), "4");
        assert_eq!(c.process("cl"), "45");
        assert_eq!(c.process("cr"), "47");
        assert_eq!(c.process("ce"), "8");
        // Interior C: the follower set loses L and R.
        assert_eq!(c.process("acl"), "085");
        assert_eq!(c.process("acr"), "087");
        assert_eq!(c.process("ach"), "04");
        // After S or Z the follower set is irrelevant: always 8, and it
        // collapses into the S/Z's own 8; the A's 0 is dropped mid-word.
        assert_eq!(c.process("sca"), "8");
        assert_eq!(c.process("szca"), "8");
        // "Code start" means EMPTY OUTPUT: a leading H emits nothing, so
        // the C in "hc..." still takes the word-initial branch.
        assert_eq!(c.process("hca"), "4");
        assert_eq!(c.process("hce"), "8");
    }

    #[test]
    fn c_after_s_is_always_8() {
        let c = cologne();
        // sc → 8 then 8 → collapsed "8"; the A then re-opens nothing (0 is
        // dropped past the front).
        assert_eq!(c.process("sc"), "8");
        assert_eq!(c.process("sch"), "8");
        assert_eq!(c.process("schh"), "8");
        // CODEC-254 shape: the H before C resets prev to H, so C is 4.
        assert_eq!(c.process("shch"), "84");
    }

    #[test]
    fn vowel_zero_survives_only_at_the_front_but_still_separates() {
        let c = cologne();
        assert_eq!(c.process("aei"), "0");
        // Dropped interior vowels still break duplicate collapsing.
        assert_eq!(c.process("sas"), "88");
        assert_eq!(c.process("ss"), "8");
        // H also breaks duplicate collapsing while emitting nothing.
        assert_eq!(c.process("shs"), "88");
        // A vowel after H at the very front: output still empty, so 0 lands.
        assert_eq!(c.process("ha"), "0");
        assert_eq!(c.process("hah"), "0");
    }

    #[test]
    fn trailing_h_and_lookahead_sentinel() {
        let c = cologne();
        // At end of input the lookahead is the '-' sentinel: a final P is 1,
        // a final D/T is 2, a final C is 8.
        assert_eq!(c.process("p"), "1");
        assert_eq!(c.process("d"), "2");
        assert_eq!(c.process("hc"), "8");
        // An input hyphen presents the same '-' to the lookahead.
        assert_eq!(c.process("c-"), "8");
        assert_eq!(c.process("p-h"), "1");
    }

    /// Chains where the `prev`-letter context, the lookahead, and the
    /// H-ignore marker interlock across several letters. Recorded from
    /// rphonetic 3.0.6.
    #[test]
    fn recorded_context_chains() {
        let cases: &[(&str, &str)] = &[
            // Three leading Hs emit nothing, so C still takes the
            // empty-output branch with the end sentinel as lookahead.
            ("hhhc", "8"),
            // X→48, then C sees peek X (in AHKOQUX) → 4, then X after C → 8.
            ("xcx", "4848"),
            // C(peek X)→4, X after C→8, final C (prev X, peek sentinel)→8.
            ("cxc", "48"),
            // First P peeks P → 1; second P peeks H → 3; H emits nothing.
            ("pph", "13"),
            ("phh", "3"),
            // D(peek T)→2, T(peek Z)→8, Z→8, D(peek C)→8, C(prev D)→8: all
            // the 8s collapse.
            ("dtzdc", "28"),
            // A whole S/Z run with no separator collapses to a single 8.
            ("szsz", "8"),
            // H updates `prev` (to H) without emitting, so X after H is 48.
            ("hxh", "48"),
        ];
        for &(input, want) in cases {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn digits_in_input_are_skipped_not_encoded() {
        let c = cologne();
        assert_eq!(c.process("m1ller"), "657");
        assert_eq!(c.process("42müller42"), "657");
    }

    #[test]
    fn very_long_input() {
        let c = cologne();
        assert_eq!(c.process(&"a".repeat(10_000)), "0");
        // Each interior `a` is dropped but still separates the `b`s' 1s.
        assert_eq!(
            c.process(&"ab".repeat(5_000)),
            format!("0{}", "1".repeat(5_000))
        );
        let long = "mönchengladbach ".repeat(500);
        let mut want = String::from("664645214");
        // Between repeats: "...bach mönchen..." — H then M; the space skips.
        for _ in 1..500 {
            want.push_str("664645214");
        }
        assert_eq!(c.process(&long), want);
    }

    #[test]
    fn multi_char_uppercase_expansions() {
        let c = cologne();
        // U+FB01 LATIN SMALL LIGATURE FI uppercases to "FI".
        assert_eq!(c.process("\u{FB01}"), "3");
        assert_eq!(c.process("a\u{FB01}n"), "036");
        // U+0149 ʼn uppercases to "ʼN" — the apostrophe is skipped.
        assert_eq!(c.process("\u{0149}"), "6");
    }

    #[test]
    fn trait_agrees_with_inherent_methods() {
        use verbora_core::Phonetic;
        let c = Cologne::new();
        assert_eq!(Phonetic::process(&c, "Müller"), "657");
        assert!(Phonetic::compare(&c, "Meyer", "Mayr"));
    }
}
