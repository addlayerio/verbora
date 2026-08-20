//! Cologne phonetics / Kölner Phonetik (Postel, 1969).

/// The ignore-marker code. `H` emits it: nothing is appended, but it becomes
/// the last-emitted code and thereby resets duplicate collapsing. It doubles
/// as the end-of-input lookahead sentinel: no rule looks ahead for it, so
/// a word-final letter takes the same branch as one before a hyphen.
const IGNORE: u8 = b'-';

/// Appends `code` to `out` under the Cologne output rules, and records it as
/// the last emitted code either way.
///
/// A code is appended unless it is the ignore-marker, equal to the previous
/// code (adjacent duplicates collapse), or a `0` past the front of the code
/// (vowels survive only while the output is still empty).
#[inline]
fn emit(out: &mut String, last: &mut u8, code: u8) {
    if code != IGNORE && *last != code && (code != b'0' || out.is_empty()) {
        out.push(char::from(code));
    }
    *last = code;
}

/// German orthography's own transliteration of the letters Postel's table
/// does not list: the umlauts lose their diaeresis and `ß` is written `ss`.
///
/// Applied after uppercasing, so it sees `Ä Ö Ü` and (via
/// [`char::to_uppercase`]) the `SS` that lowercase `ß` already became. The
/// capital sharp s `ẞ` (U+1E9E) uppercases to itself, so it is folded here
/// explicitly rather than being silently skipped — `ß` and `ẞ` must encode
/// alike.
#[inline]
fn fold_german(c: char) -> Folded {
    match c {
        'Ä' => Folded::Many("A".chars()),
        'Ö' => Folded::Many("O".chars()),
        'Ü' => Folded::Many("U".chars()),
        'ẞ' => Folded::Many("SS".chars()),
        _ => Folded::One(Some(c)),
    }
}

/// One character, or the several a German fold expands it into.
enum Folded {
    /// The character itself, unfolded.
    One(Option<char>),
    /// The fold's replacement text.
    Many(std::str::Chars<'static>),
}

impl Iterator for Folded {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        match self {
            Self::One(c) => c.take(),
            Self::Many(chars) => chars.next(),
        }
    }
}

/// The whole encoder, over an already uppercased-and-folded character
/// stream.
///
/// One forward pass with a single character of lookahead. `capacity` seeds
/// the one `String` this call allocates.
fn encode_stream(mut chars: impl Iterator<Item = char>, capacity: usize) -> String {
    let mut out = String::with_capacity(capacity);
    // Any non-code byte works as the "nothing emitted yet" marker; it only
    // has to differ from every real code and from IGNORE.
    let mut last_code: u8 = b'/';
    // The previously *encoded* letter (context for the C and X rules).
    // Skipped characters do not touch it.
    let mut prev: char = '-';

    let mut cur = chars.next();
    while let Some(ch) = cur {
        let next = chars.next();
        if ch.is_ascii_uppercase() {
            // The raw next character of the normalized string, letter or
            // not; the end of input presents the same sentinel a hyphen does.
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

/// Cologne phonetics (Kölner Phonetik) — the German-language analogue of
/// Soundex.
///
/// # Publication
///
/// Hans Joachim Postel, "Die Kölner Phonetik. Ein Verfahren zur
/// Identifizierung von Personennamen auf der Grundlage der Gestaltanalyse",
/// *IBM-Nachrichten* 19 (1969), pp. 925–931.
///
/// Like Soundex it maps a word to digits so that similar-sounding words
/// collide, but it is tuned to German orthography: the code is not truncated
/// to a fixed length, vowels survive only at the very front, and `C`, `D`,
/// `T`, `P` and `X` encode differently depending on their neighbours.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Only `A`–`Z` are encoded, with
///   German orthography's own folds applied first: `Ä Ö Ü ä ö ü` become
///   `A O U`, and `ß`/`ẞ` become `SS`. Every other scalar — digits,
///   punctuation, whitespace, other accented Latin, Cyrillic, CJK, emoji — is
///   skipped.
/// * A skipped scalar is **visible to the one-character lookahead** but does
///   **not** update the preceding-letter context the `C` and `X` rules read.
///   So `"p1h"` codes its `P` as `1`, not `3` (the peeked character is `1`,
///   not `H`), while in `"c-x"` the `X` still sees `C` as its predecessor.
/// * A vowel's `0` survives only while the code is still empty; elsewhere it
///   is dropped — but it still separates equal digits, so `"sas"` is `"88"`,
///   not `"8"`. `H` emits nothing and likewise separates, so `"shs"` is
///   `"88"` too.
/// * Adjacent equal codes collapse, and skipped scalars do not break that
///   adjacency: `"Test test"` is `"28282"`, the two middle `T`s merging
///   across the space.
/// * A token with no encodable letter encodes to `""`.
/// * **Total**: no input panics, and there is no error type.
///
/// # The code table
///
/// | Letter | Context | Code |
/// |---|---|---|
/// | A, E, I, J, O, U, Y | — | `0` |
/// | B | — | `1` |
/// | P | not before H | `1` |
/// | D, T | not before C, S, Z | `2` |
/// | F, V, W | — | `3` |
/// | P | before H | `3` |
/// | G, K, Q | — | `4` |
/// | C | at code start, before A, H, K, L, O, Q, R, U, X | `4` |
/// | C | mid-word, not after S/Z, before A, H, K, O, Q, U, X | `4` |
/// | X | not after C, K, Q | `48` |
/// | L | — | `5` |
/// | M, N | — | `6` |
/// | R | — | `7` |
/// | S, Z | — | `8` |
/// | C | after S or Z, or in no `4` context above | `8` |
/// | D, T | before C, S, Z | `8` |
/// | X | after C, K, Q | `8` |
/// | H | — | nothing |
///
/// "At code start" is literally *while the code is still empty* — a
/// word-initial `H` emits nothing, so the `C` in `"hc"` still takes the
/// initial branch. At the end of the word the lookahead is no letter at all,
/// so a word-final `C` takes the same branch as a `C` before a hyphen.
///
/// ```
/// use verbora_phonetics::Cologne;
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
    /// use verbora_phonetics::Cologne;
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
    /// use verbora_phonetics::Cologne;
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
            // (ß → "SS"), then the German fold.
            encode_stream(
                token
                    .chars()
                    .flat_map(char::to_uppercase)
                    .flat_map(fold_german),
                token.len(),
            )
        }
    }

    /// Whether two strings share a Cologne code.
    ///
    /// ```
    /// use verbora_phonetics::Cologne;
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

// Where every expected value in this module comes from.
//
// Each code below is derived by walking its input through the substitution
// table Hans Joachim Postel published in "Die Kölner Phonetik. Ein Verfahren
// zur Identifizierung von Personennamen auf der Grundlage der Gestaltanalyse",
// IBM-Nachrichten 19 (1969), pp. 925–931 — the table reproduced in full in
// this module's own rustdoc — and then applying the two output rules the
// paper states: adjacent equal codes collapse, and a `0` survives only at
// the front of the code.
//
// The derivations are written out beside the fixtures so a reader can check
// the arithmetic instead of trusting it. In a trace, every letter carries the
// code the table gives it in that position; `·` is H, whose code is "nothing";
// and a code in [brackets] is one the two output rules then discard — either a
// `0` past the front of the code, or a code equal to the one before it. So
//
//     schmidt: S8 C[8] H· M6 I[0] D2 T[2] = 862
//
// reads: S is 8; C follows S and is therefore 8, which collapses into it;
// H emits nothing; M is 6; I's 0 is dropped for not being at the front; D is
// not before C/S/Z and is 2; T is likewise 2 and collapses into D's.
//
// Postel's table leaves exactly three things to the implementation, and the
// rustdoc above fixes all three as part of Verbora's contract. Traces that
// turn on one of them say which:
//
//   * "im Anlaut" (at the start) is read as *while the emitted code is still
//     empty*, so a leading H — which emits nothing — does not end it;
//   * at the end of the word the lookahead is "no letter at all", the same
//     thing a non-letter presents;
//   * a skipped scalar is visible to the lookahead but does not become the
//     preceding letter that the C and X rules read.
//
// Nothing here is transcribed from another implementation's test suite.
#[cfg(test)]
mod tests {
    use super::*;

    fn cologne() -> Cologne {
        Cologne::new()
    }

    // ------------------------------------------------------------------
    // The table itself, row by row
    // ------------------------------------------------------------------

    /// **Enumeration, not sampling.** Every row of Postel's table gets a
    /// witness word whose code that row alone decides, so a row silently
    /// changing meaning fails here rather than in whichever name happened to
    /// contain the letter.
    ///
    /// Context-dependent rows get *both* sides: the pair `("aca", "04")` and
    /// `("acl", "085")` is what proves interior C's follower set excludes L,
    /// and `("aca", "04")` against `("asca", "08")` is what proves an S before
    /// the C overrides that follower set. Neither assertion alone can.
    #[test]
    fn every_row_of_the_published_table_has_a_witness() {
        let rows: &[(&str, &str, &str)] = &[
            // (row of the table, witness, code)
            ("A E I J O U Y -> 0", "a", "0"),
            ("A E I J O U Y -> 0", "e", "0"),
            ("A E I J O U Y -> 0", "i", "0"),
            ("A E I J O U Y -> 0", "j", "0"),
            ("A E I J O U Y -> 0", "o", "0"),
            ("A E I J O U Y -> 0", "u", "0"),
            ("A E I J O U Y -> 0", "y", "0"),
            ("H -> nothing", "h", ""),
            ("B -> 1", "b", "1"),
            ("P not before H -> 1", "p", "1"),
            ("D T not before C S Z -> 2", "d", "2"),
            ("D T not before C S Z -> 2", "t", "2"),
            ("F V W -> 3", "f", "3"),
            ("F V W -> 3", "v", "3"),
            ("F V W -> 3", "w", "3"),
            ("P before H -> 3", "ph", "3"),
            ("G K Q -> 4", "g", "4"),
            ("G K Q -> 4", "k", "4"),
            ("G K Q -> 4", "q", "4"),
            // C at the start, before one of A H K L O Q R U X -> 4. Nine
            // witnesses, one per follower; the follower's own code trails.
            ("C at start before A H K L O Q R U X -> 4", "ca", "4"),
            ("C at start before A H K L O Q R U X -> 4", "ch", "4"),
            ("C at start before A H K L O Q R U X -> 4", "ck", "4"),
            ("C at start before A H K L O Q R U X -> 4", "cl", "45"),
            ("C at start before A H K L O Q R U X -> 4", "co", "4"),
            ("C at start before A H K L O Q R U X -> 4", "cq", "4"),
            ("C at start before A H K L O Q R U X -> 4", "cr", "47"),
            ("C at start before A H K L O Q R U X -> 4", "cu", "4"),
            ("C at start before A H K L O Q R U X -> 4", "cx", "48"),
            // C before A H K O Q U X except after S/Z -> 4. The same nine
            // followers minus L and R, which the interior row drops.
            ("C before A H K O Q U X, not after S Z -> 4", "aca", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "ach", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "ack", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "aco", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "acq", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "acu", "04"),
            ("C before A H K O Q U X, not after S Z -> 4", "acx", "048"),
            ("X not after C K Q -> 48", "x", "48"),
            ("L -> 5", "l", "5"),
            ("M N -> 6", "m", "6"),
            ("M N -> 6", "n", "6"),
            ("R -> 7", "r", "7"),
            ("S Z -> 8", "s", "8"),
            ("S Z -> 8", "z", "8"),
            // C after S or Z -> 8. The 8 collapses into the S's own 8, so the
            // row is observable only against the counterfactual "aca" = 04
            // three rows up: same followers, no S, code 4.
            ("C after S Z -> 8", "asca", "08"),
            ("C after S Z -> 8", "azca", "08"),
            ("C at start, not before A H K L O Q R U X -> 8", "ce", "8"),
            ("C not before A H K O Q U X -> 8", "ace", "08"),
            ("C not before A H K O Q U X -> 8", "acl", "085"),
            ("C not before A H K O Q U X -> 8", "acr", "087"),
            ("D T before C S Z -> 8", "dc", "8"),
            ("D T before C S Z -> 8", "ds", "8"),
            ("D T before C S Z -> 8", "dz", "8"),
            ("D T before C S Z -> 8", "tc", "8"),
            ("D T before C S Z -> 8", "ts", "8"),
            ("D T before C S Z -> 8", "tz", "8"),
            // X after C K Q -> 8, against "rx" = 748 where the preceding code
            // is not itself a 4 and the two-digit 48 is therefore visible.
            ("X after C K Q -> 8", "cx", "48"),
            ("X after C K Q -> 8", "kx", "48"),
            ("X after C K Q -> 8", "qx", "48"),
            ("X not after C K Q -> 48", "rx", "748"),
        ];
        for &(row, witness, want) in rows {
            assert_eq!(cologne().process(witness), want, "{row} / {witness:?}");
        }
        // Every letter of the alphabet is claimed by some row above.
        let covered: String = rows
            .iter()
            .flat_map(|&(_, witness, _)| witness.chars())
            .collect();
        for letter in 'a'..='z' {
            assert!(covered.contains(letter), "no witness exercises {letter:?}");
        }
    }

    #[test]
    fn single_letters_cover_the_full_alphabet() {
        // Each letter alone, so the lookahead is the end of the word: P is
        // "not before H" and codes 1, D and T are "not before C/S/Z" and code
        // 2, and C is at the start but not before one of A H K L O Q R U X,
        // so it codes 8. H alone emits nothing at all.
        let want = [
            ("a", "0"),
            ("b", "1"),
            ("c", "8"),
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

    // ------------------------------------------------------------------
    // Short words: one or two table rows each, fully traced
    // ------------------------------------------------------------------

    #[test]
    fn one_and_two_letter_words() {
        let data: [(&str, &str); 31] = [
            ("a", "0"),        // A0
            ("e", "0"),        // E0
            ("i", "0"),        // I0
            ("o", "0"),        // O0
            ("u", "0"),        // U0
            ("\u{00E4}", "0"), // ä folds to A: A0
            ("\u{00F6}", "0"), // ö folds to O: O0
            ("\u{00FC}", "0"), // ü folds to U: U0
            ("\u{00DF}", "8"), // ß is written ss: S8 S[8]
            ("aa", "0"),       // A0 A[0]
            ("ha", "0"),       // H· A0 — the code is still empty, so the 0 lands
            ("h", ""),         // H·
            ("aha", "0"),      // A0 H· A[0] — the second 0 is past the front
            ("b", "1"),        // B1
            ("p", "1"),        // P1, the lookahead being no letter rather than H
            ("ph", "3"),       // P3 H·
            ("f", "3"),        // F3
            ("v", "3"),        // V3
            ("w", "3"),        // W3
            ("g", "4"),        // G4
            ("k", "4"),        // K4
            ("q", "4"),        // Q4
            ("x", "48"),       // X48, no C/K/Q before it
            ("ax", "048"),     // A0 X48
            ("cx", "48"),      // C4 (before X) X8 (after C)
            ("l", "5"),        // L5
            ("cl", "45"),      // C4 (at the start, before L) L5
            ("acl", "085"),    // A0 C8 L5 — interior C's follower set has no L
            ("mn", "6"),       // M6 N[6]
            ("{mn}", "6"),     // braces are not letters: M6 N[6]
            ("r", "7"),        // R7
        ];
        for (input, want) in data {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // German words and names, each traced through the table
    // ------------------------------------------------------------------

    #[test]
    fn german_words_and_names() {
        let data: [(&str, &str); 34] = [
            // M6 U[0] L5 L[5] E[0] R7
            ("m\u{00DC}ller", "657"),
            ("m\u{00FC}ller", "657"),
            // S8 C[8] H· M6 I[0] D2 T[2]
            ("schmidt", "862"),
            // S8 C[8] H· N6 E[0] I[0] D2 E[0] R7
            ("schneider", "8627"),
            // F3 I[0] S8 C[8] H· E[0] R7
            ("fischer", "387"),
            // W3 E[0] B1 E[0] R7
            ("weber", "317"),
            // W3 A[0] G4 N6 E[0] R7
            ("wagner", "3467"),
            // B1 E[0] C4 K[4] E[0] R7 — interior C before K
            ("becker", "147"),
            // H· O0 F3 F[3] M6 A[0] N6 N[6]
            ("hoffmann", "0366"),
            // S8 C[8] H· A[0] F3 E[0] R7 (Ä folds to A)
            ("sch\u{00C4}fer", "837"),
            ("sch\u{00E4}fer", "837"),
            // B1 R7 E[0] S8 C[8] H· N6 E[0] W3
            ("Breschnew", "17863"),
            // W3 I[0] K4 I[0] P1 E[0] D2 I[0] A[0]
            ("Wikipedia", "3412"),
            // P1 E[0] T2 E[0] R7
            ("peter", "127"),
            // P3 H· A[0] R7 M6 A[0] — P before H
            ("pharma", "376"),
            // M6 O[0] N6 C4 H· E[0] N6 G4 L5 A[0] D2 B1 A[0] C4 H·
            ("m\u{00F6}nchengladbach", "664645214"),
            // D2 E[0] U[0] T8 S[8] C[8] H· — T before S
            ("deutsch", "28"),
            // D2 E[0] U[0] T8 Z[8] — T before Z
            ("deutz", "28"),
            // H· A0 M6 B1 U[0] R7 G4
            ("hamburg", "06174"),
            // H· A0 N6 N[6] O[0] V3 E[0] R7
            ("hannover", "0637"),
            // C4 H· R7 I[0] S8 T[8] S[8] T2 O[0] L5 L[5] E[0] N6 — the first
            // T is before S and codes 8, the second is before O and codes 2
            ("christstollen", "478256"),
            // X48 A[0] N6 T2 H· I[0] P1 P[1] E[0]
            ("Xanthippe", "48621"),
            // Z8 A[0] C4 H· A[0] R7 I[0] A[0] S8
            ("Zacharias", "8478"),
            // H· O0 L5 Z8 B1 A[0] U[0]
            ("Holzbau", "0581"),
            // M6 A[0] T8 S[8] C[8] H·
            ("matsch", "68"),
            // M6 A[0] T8 Z[8]
            ("matz", "68"),
            // A0 R7 B1 E[0] I[0] T8 S[8] A[0] M6 T2
            ("Arbeitsamt", "071862"),
            // E0 B1 E[0] R7 H· A[0] R7 D2 — H separates the two R7s, so the
            // second is not collapsed away
            ("Eberhard", "01772"),
            // ... and the extra T collapses into the D
            ("Eberhardt", "01772"),
            // C8 E[0] L5 S8 I[0] U[0] S8 — C at the start before E
            ("Celsius", "8588"),
            // A0 C8 E[0]
            ("Ace", "08"),
            // S8 H· C4 H· — H resets the preceding letter, so the C is not
            // "after S" and takes its before-H branch instead
            ("shch", "84"),
            // X48 C4 H· — X is not after C/K/Q here, the C is before H
            ("xch", "484"),
            // H· E0 I[0] T2 H· A[0] B1 U[0]
            ("heithabu", "021"),
        ];
        for (input, want) in data {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
    }

    /// Three names in which a single letter's context decides the code, so
    /// each is a one-fixture check on one branch of the table.
    #[test]
    fn names_whose_code_turns_on_one_letters_context() {
        let c = cologne();
        // A0 A[0] B1 J[0] O[0] E[0] — a leading vowel run keeps exactly one
        // 0, and the J and the trailing vowels are all past the front.
        assert_eq!(c.process("Aabjoe"), "01");
        // A0 A[0] C8 L5 A[0] N6 — the C is interior (the code already holds a
        // 0), and L is in the word-initial follower set but *not* the
        // interior one, so this C codes 8 where "clan" alone would give 4.
        assert_eq!(c.process("Aaclan"), "0856");
        assert_eq!(c.process("clan"), "456"); // C4 L5 A[0] N6, for contrast
        // A0 Y[0] C4 H· L5 M6 A[0] J[0] R7 — an interior C before H, which
        // *is* in the interior follower set, so this one codes 4.
        assert_eq!(c.process("Aychlmajr"), "04567");
    }

    #[test]
    fn hyphenated_names() {
        // B1 E[0] R7 G4 I[0] S8 C[8] H· G4 L5 A[0] D2 B1 A[0] C4 H· — the
        // hyphen is skipped and the second G opens no new word: the code runs
        // straight on.
        assert_eq!(cologne().process("bergisch-gladbach"), "174845214");
        // M6 U[0] L5 L[5] E[0] R7 L5 U[0] D2 E[0] N6 S8 C[8] H· E[0] I[0] D2 T[2]
        assert_eq!(
            cologne().process("M\u{00FC}ller-L\u{00FC}denscheidt"),
            "65752682"
        );
    }

    #[test]
    fn spelling_variants_of_one_name_collide() {
        // The point of the algorithm: six spellings of the same name, all 65.
        //   mella  M6 E[0] L5 L[5] A[0]
        //   milah  M6 I[0] L5 A[0] H·
        //   moulla M6 O[0] U[0] L5 L[5] A[0]
        //   mellah M6 E[0] L5 L[5] A[0] H·
        //   muehle M6 U[0] E[0] H· L5 E[0]
        //   mule   M6 U[0] L5 E[0]
        for input in ["mella", "milah", "moulla", "mellah", "muehle", "mule"] {
            assert_eq!(cologne().process(input), "65", "for {input:?}");
        }
        // Seven spellings of Meyer, all 67. Each is M6, then a run of vowels
        // (E, A, I, Y and J all code 0, and every one of them is past the
        // front of the code, so all are dropped), then R7.
        for input in ["Meier", "Maier", "Mair", "Meyer", "Meyr", "Mejer", "Major"] {
            assert_eq!(cologne().process(input), "67", "for {input:?}");
        }
    }

    #[test]
    fn compare_is_code_equality() {
        // Each pair, with the code both sides derive to.
        let data: [(&str, &str, &str); 8] = [
            ("Muller", "M\u{00FC}ller", "657"), // ü folds to u
            ("Meyer", "Mayr", "67"),
            ("house", "house", "08"),
            ("House", "house", "08"),
            ("Haus", "house", "08"), // H· vowel-run S8 on both sides
            ("ganz", "Gans", "468"), // Z and S both code 8
            ("ganz", "G\u{00E4}nse", "468"),
            ("Miyagi", "Miyako", "64"), // G and K both code 4
        ];
        for (a, b, code) in data {
            assert_eq!(cologne().process(a), code, "for {a:?}");
            assert_eq!(cologne().process(b), code, "for {b:?}");
            assert!(cologne().compare(a, b), "{a:?} vs {b:?}");
        }
        // ... and unequal codes do not compare equal.
        assert!(!cologne().compare("Meyer", "M\u{00FC}ller")); // 67 vs 657
    }

    // ------------------------------------------------------------------
    // The three points Postel's table leaves to the implementation
    // ------------------------------------------------------------------

    #[test]
    fn special_chars_between_same_letters() {
        // A skipped scalar does not break the collapsing of adjacent equal
        // codes: T2 E[0] S8 T2 T[2] E[0] S8 T2 in every one of these, the
        // fourth and fifth letters' 2s merging across whatever sits between
        // them.
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

    #[test]
    fn skipped_characters_are_visible_to_lookahead_only() {
        let c = cologne();
        // P before a literal H is 3; before anything else — including a
        // skipped scalar that itself precedes an H — it is 1.
        assert_eq!(c.process("ph"), "3"); // P3 H·
        assert_eq!(c.process("p1h"), "1"); // P1 H·  (the lookahead is '1')
        assert_eq!(c.process("p h"), "1"); // P1 H·
        // D before S is 8; with a skipped scalar between them it is 2.
        assert_eq!(c.process("ds"), "8"); // D8 S[8]
        assert_eq!(c.process("d-s"), "28"); // D2 S8
        // But a skipped scalar does *not* become the preceding letter: the X
        // in "c-x" still reads C as its predecessor and codes 8, which then
        // collapses into the C's own 8.
        assert_eq!(c.process("cx"), "48"); // C4 X8
        assert_eq!(c.process("c-x"), "8"); // C8 X[8]
    }

    #[test]
    fn end_of_word_presents_the_same_lookahead_as_a_non_letter() {
        let c = cologne();
        // A word-final P is "not before H" and codes 1; a word-final D is
        // "not before C/S/Z" and codes 2; a word-final C is "not before
        // A H K L O Q R U X" and codes 8.
        assert_eq!(c.process("p"), "1");
        assert_eq!(c.process("d"), "2");
        assert_eq!(c.process("hc"), "8"); // H· C8
        // A trailing hyphen presents the same "no letter" to the lookahead.
        assert_eq!(c.process("c-"), "8"); // C8
        assert_eq!(c.process("p-h"), "1"); // P1 H·
    }

    #[test]
    fn at_the_start_means_while_the_code_is_still_empty() {
        let c = cologne();
        // Word-initial C takes the nine-follower set, which includes L and R.
        assert_eq!(c.process("ca"), "4"); // C4 A[0]
        assert_eq!(c.process("cl"), "45"); // C4 L5
        assert_eq!(c.process("cr"), "47"); // C4 R7
        assert_eq!(c.process("ce"), "8"); // C8 E[0]
        // Interior C takes the seven-follower set, which does not.
        assert_eq!(c.process("acl"), "085"); // A0 C8 L5
        assert_eq!(c.process("acr"), "087"); // A0 C8 R7
        assert_eq!(c.process("ach"), "04"); // A0 C4 H·
        // After S or Z the follower set is irrelevant — always 8 — and it
        // collapses into the S/Z's own 8.
        assert_eq!(c.process("sca"), "8"); // S8 C[8] A[0]
        assert_eq!(c.process("szca"), "8"); // S8 Z[8] C[8] A[0]
        // Leading Hs emit nothing, so the code is still empty when the C is
        // reached and it takes the word-initial branch — the reading of "im
        // Anlaut" this crate's contract fixes.
        assert_eq!(c.process("hca"), "4"); // H· C4 A[0]
        assert_eq!(c.process("hce"), "8"); // H· C8 E[0]
        assert_eq!(c.process("hhhc"), "8"); // H· H· H· C8
    }

    #[test]
    fn c_after_s_is_always_8() {
        let c = cologne();
        assert_eq!(c.process("sc"), "8"); // S8 C[8]
        assert_eq!(c.process("sch"), "8"); // S8 C[8] H·
        assert_eq!(c.process("schh"), "8"); // S8 C[8] H· H·
        // With an H between them the C is no longer after S: its preceding
        // letter is H, and its lookahead is H too, so it takes the 4 branch.
        assert_eq!(c.process("shch"), "84"); // S8 H· C4 H·
    }

    #[test]
    fn x_rule() {
        let c = cologne();
        // After C, K or Q: a plain 8. Otherwise 48.
        assert_eq!(c.process("kx"), "48"); // K4 X8
        assert_eq!(c.process("qx"), "48"); // Q4 X8
        assert_eq!(c.process("rx"), "748"); // R7 X48
        assert_eq!(c.process("ax"), "048"); // A0 X48
        // A run of X: the previous letter is an X, not a C/K/Q, so each one
        // re-emits the full 48.
        assert_eq!(c.process("xx"), "4848"); // X48 X48
        assert_eq!(c.process("xxxx"), "48484848");
        // G is not in the C/K/Q set, so this X emits 48 — but its 4 collapses
        // into the G's, making the result identical to "kx" by a different
        // route.
        assert_eq!(c.process("gx"), "48"); // G4 X[4]8
    }

    #[test]
    fn vowel_zero_survives_only_at_the_front_but_still_separates() {
        let c = cologne();
        assert_eq!(c.process("aei"), "0"); // A0 E[0] I[0]
        // A dropped interior vowel still breaks the collapsing of the codes
        // around it.
        assert_eq!(c.process("sas"), "88"); // S8 A[0] S8
        assert_eq!(c.process("ss"), "8"); // S8 S[8]
        // H does the same while emitting nothing at all.
        assert_eq!(c.process("shs"), "88"); // S8 H· S8
        // A vowel after a word-initial H: the code is still empty, so its 0
        // is kept.
        assert_eq!(c.process("ha"), "0"); // H· A0
        assert_eq!(c.process("hah"), "0"); // H· A0 H·
    }

    /// Chains where the preceding-letter context, the lookahead and H's
    /// empty code interlock across several letters at once. Every one is
    /// traced through the table; none is recorded from anything.
    #[test]
    fn context_chains() {
        let cases: &[(&str, &str)] = &[
            // X48 C4 X8 — the C is before X (a 4-follower) and the second X
            // is after C.
            ("xcx", "4848"),
            // C4 X8 C[8] — the last C's lookahead is no letter, so it is 8,
            // which collapses into the X's.
            ("cxc", "48"),
            // P1 P3 H· — the first P looks ahead to a P, the second to an H.
            ("pph", "13"),
            ("phh", "3"), // P3 H· H·
            // D2 T8 Z[8] D[8] C[8] — T is before Z, D is before C, and the
            // whole 8-run collapses to one digit.
            ("dtzdc", "28"),
            ("szsz", "8"), // S8 Z[8] S[8] Z[8]
            // H· X48 H· — H is the preceding letter of the X, not a C/K/Q,
            // so the X emits both digits.
            ("hxh", "48"),
        ];
        for &(input, want) in cases {
            assert_eq!(cologne().process(input), want, "for {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // Unicode, folding and totality
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
    fn mixed_case_is_case_insensitive() {
        let c = cologne();
        assert_eq!(c.process("MüLLeR"), "657");
        assert_eq!(c.process("ScHmIdT"), "862");
        assert_eq!(c.process("WIKIPEDIA"), c.process("wikipedia"));
    }

    /// German writes `ß` as `ss`, so it codes as one collapsed `8` — and the
    /// capital sharp s `ẞ` (U+1E9E), which uppercases to *itself* and would
    /// otherwise fall through as "not an A-Z letter", is folded explicitly so
    /// that the two cases of the same letter cannot disagree.
    #[test]
    fn both_cases_of_sharp_s_fold_to_ss() {
        let c = cologne();
        assert_eq!(c.process("ß"), "8"); // S8 S[8]
        assert_eq!(c.process("ẞ"), "8");
        assert_eq!(c.process("ß"), c.process("ss"));
        assert_eq!(c.process("ẞ"), c.process("SS"));
        // S8 T2 R7 A[0] S8 S[8] E[0]
        assert_eq!(c.process("Straße"), "8278");
        assert_eq!(c.process("Strasse"), "8278");
        assert_eq!(c.process("STRAẞE"), "8278");
        assert_eq!(c.process("ẞßẞ"), "8"); // six S, five of them collapsed
    }

    #[test]
    fn umlauts_fold_to_plain_vowels() {
        let c = cologne();
        assert_eq!(c.process("Ä"), "0");
        assert_eq!(c.process("Ö"), "0");
        assert_eq!(c.process("Ü"), "0");
        // A folded umlaut is also what the lookahead sees: the C in "cä" reads
        // an A and takes the 4 branch, and the A's own 0 is then dropped for
        // not being at the front.
        assert_eq!(c.process("cä"), "4"); // C4 A[0]
        // Other accented Latin letters are NOT folded — é is skipped, so the
        // C's lookahead is A and the F is reached with nothing between.
        assert_eq!(c.process("café"), "43"); // C4 A[0] F3
        assert_eq!(c.process("naïve"), "63"); // N6 A[0] V3 E[0]
    }

    #[test]
    fn digits_in_input_are_skipped_not_encoded() {
        let c = cologne();
        assert_eq!(c.process("m1ller"), "657"); // M6 L5 L[5] E[0] R7
        assert_eq!(c.process("42müller42"), "657");
    }

    #[test]
    fn very_long_input() {
        let c = cologne();
        // One leading 0, and every later A's 0 dropped.
        assert_eq!(c.process(&"a".repeat(10_000)), "0");
        // Each interior A is dropped but still separates the Bs' 1s, so the
        // code is a leading 0 followed by one 1 per B.
        assert_eq!(
            c.process(&"ab".repeat(5_000)),
            format!("0{}", "1".repeat(5_000))
        );
        // Repeating a whole word repeats its code: the trailing H of
        // "...bach" and the leading M of "mönchen..." emit an empty code and
        // a 6, which neither collapse nor drop.
        let long = "mönchengladbach ".repeat(500);
        let want = "664645214".repeat(500);
        assert_eq!(c.process(&long), want);
    }

    #[test]
    fn multi_char_uppercase_expansions() {
        let c = cologne();
        // U+FB01 LATIN SMALL LIGATURE FI uppercases to "FI": F3 I[0].
        assert_eq!(c.process("\u{FB01}"), "3");
        assert_eq!(c.process("a\u{FB01}n"), "036"); // A0 F3 I[0] N6
        // U+0149 ʼn uppercases to "ʼN" — the apostrophe is skipped, the N codes.
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
