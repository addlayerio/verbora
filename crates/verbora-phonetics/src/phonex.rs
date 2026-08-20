//! Phonex (Lait and Randell, 1996).

/// Phonex — a Soundex refinement for British surnames.
///
/// # Publication
///
/// A. J. Lait and Brian Randell, "An Assessment of Name Matching Algorithms",
/// Technical Report Series, University of Newcastle upon Tyne Department of
/// Computing Science, 1996. Phonex adds a preprocessing stage (trailing-`S`
/// removal, leading-pair and leading-letter substitutions) and
/// context-sensitive digit rules to Soundex, tuned to reduce false negatives
/// on British surname data.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar**, and only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding. Every other scalar is
///   skipped. Phonex, like the Soundex it refines, is stated over the Roman
///   alphabet.
/// * Because every key character is ASCII, the configured maximum length is a
///   character count and a byte count at once — there is no input for which
///   they differ.
/// * A token with no `A`–`Z` letter — and a token whose letters are all
///   trailing `S`, which the preprocessing removes — encodes to all-`0`
///   padding at the configured length.
/// * **Total**: no input panics, and there is no error type.
///
/// # The algorithm
///
/// 1. **Preprocess** the letter sequence:
///    * remove every trailing `S` (`JONES` → `JONE`, `SSS` → empty);
///    * rewrite a leading pair by replacing only its **first** letter:
///      `KN…` → `NN…`, `PH…` → `FH…`, `WR…` → `RR…`;
///    * remove one leading `H` (`HARRINGTON` → `ARRINGTON`; a second `H` is
///      not removed: `HHART` → `HART`);
///    * substitute the (new) first letter: `E I O U Y` → `A`, `P` → `B`,
///      `V` → `F`, `K Q` → `C`, `J` → `G`, `Z` → `S`.
/// 2. **Transcode**: emit the first preprocessed character verbatim, then walk
///    the rest against the digit table (`BPFV`→1, `CSKGJQXZ`→2, `DT`→3,
///    `L`→4, `MN`→5, `R`→6, other→0) with three context rules: `D`/`T` before
///    `C` is silent; `L` and `R` code only before a vowel (`AEIOUY`, so `Y`
///    counts) or at word end; `M`/`N` swallow a following `D` or `G`. A digit
///    equal to the previously pushed code is suppressed; `0` is never pushed.
/// 3. **Pad** with `0` to the configured length (default 4).
///
/// The only configuration is that maximum length
/// ([`Phonex::with_max_code_length`]); [`Phonex::new`] and
/// [`Phonex::default`] use the conventional 4.
///
/// ```
/// use verbora_phonetics::Phonex;
///
/// let phonex = Phonex::new();
/// assert_eq!(phonex.process("KNUTH"), "N300");
/// assert_eq!(phonex.process("Wright"), "R623");
/// assert!(phonex.compare("Schmidt", "Schmit"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Phonex {
    max_code_length: usize,
}

/// The conventional Phonex code length: one letter plus three digits.
const DEFAULT_MAX_CODE_LENGTH: usize = 4;

impl Phonex {
    /// Creates a Phonex encoder with the conventional maximum code length
    /// of 4.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::new().process("Sinatra"), "S536");
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_code_length: DEFAULT_MAX_CODE_LENGTH,
        }
    }

    /// Creates a Phonex encoder with a custom maximum code length, mirroring
    /// the code length, in characters.
    ///
    /// The length is measured in **bytes** and codes shorter than it are
    /// zero-padded up to it — see the type
    /// documentation for the byte-length quirks this implies on non-ASCII
    /// input.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::with_max_code_length(6).process("Sinatra"), "S53600");
    /// assert_eq!(Phonex::with_max_code_length(2).process("Sinatra"), "S5");
    /// assert_eq!(Phonex::with_max_code_length(1).process("Sinatra"), "S");
    /// assert_eq!(Phonex::with_max_code_length(0).process("Sinatra"), "");
    /// ```
    #[must_use]
    pub const fn with_max_code_length(max_code_length: usize) -> Self {
        Self { max_code_length }
    }

    /// The maximum code length this encoder pads and truncates to, in bytes.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::new().max_code_length(), 4);
    /// assert_eq!(Phonex::with_max_code_length(10).max_code_length(), 10);
    /// ```
    #[must_use]
    pub const fn max_code_length(&self) -> usize {
        self.max_code_length
    }

    /// Encodes `token`, returning its Phonex code.
    ///
    /// The code, at this encoder's configured length, for every `&str`
    /// input. Never panics; input that cleans to nothing (empty strings,
    /// digits, punctuation, emoji, all-`S` words) encodes to all zeros.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// let phonex = Phonex::new();
    /// assert_eq!(phonex.process("Phonex"), "F520");
    /// assert_eq!(phonex.process("Ashcraft"), "A261");
    /// assert_eq!(phonex.process("Meyer-Lansky"), "M452");
    /// assert_eq!(phonex.process(""), "0000");
    /// assert_eq!(phonex.process("12345"), "0000");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let mut rest = PreChars::new(token);
        let mut curr = rest.next();
        let mut next = rest.next();

        // The transcode loop, including the index bookkeeping the `M`/`N`
        // swallow rule needs: a swallow advances past the swallowed letter,
        // so an iteration that started as the first stops counting as one.
        // Only "is this the first iteration" is ever observable, which
        // `first_iter`/`treat_as_first` carry.
        let mut result = String::with_capacity(self.max_code_length.clamp(4, 16));
        let mut code = '0';
        let mut last = '0';
        let mut last_push = '0';
        let mut first_iter = true;

        while let Some(c) = curr {
            // Every key character is ASCII and the code grows one character
            // at a time, so equality here is the same test as `>=`.
            if result.len() == self.max_code_length {
                break;
            }

            // The first preprocessed character is always emitted verbatim.
            if first_iter {
                result.push(c);
                last_push = c;
            }

            let (new_code, skip_next) = transcode(c, next, next.is_none());
            if let Some(new_code) = new_code {
                code = new_code;
            }

            // The swallow advances past the D/G, so an iteration that
            // started as the first stops counting as it.
            let treat_as_first = first_iter && !skip_next;
            if skip_next {
                next = rest.next();
            }

            if last != code && code != '0' && !treat_as_first {
                result.push(code);
                last_push = code;
            }

            // `last` rewinds to the last *pushed* character — possibly the
            // head letter — after every iteration, except that the first
            // iteration records its own code instead.
            last = last_push;
            if treat_as_first {
                last = code;
            }

            curr = next;
            next = rest.next();
            first_iter = false;
        }

        // Pad to the configured length. The code is ASCII, so this is a
        // character count.
        while result.len() < self.max_code_length {
            result.push('0');
        }

        result
    }

    /// Whether two strings share a Phonex code at this encoder's length.
    ///
    /// Both inputs are encoded and the codes compared for equality.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// let phonex = Phonex::new();
    /// assert!(phonex.compare("Knuth", "Nuth"));
    /// assert!(phonex.compare("Dalitz", "Duhlitz"));
    /// assert!(!phonex.compare("Wilson", "Worms"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl Default for Phonex {
    /// Equivalent to [`Phonex::new`]: maximum code length 4, matching
    /// the conventional length of 4.
    fn default() -> Self {
        Self::new()
    }
}

impl verbora_core::Phonetic for Phonex {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

/// One step of the digit table, from Lait and Randell's
/// `Phonex::transcode`.
///
/// Returns the digit for `curr` (or `None` when a context rule silences it —
/// in which case the caller *keeps its previous code* — a carried-context rule
/// the encode loop depends on) and whether the following character must be
/// swallowed (`M`/`N` before `D`/`G`).
#[inline]
fn transcode(curr: char, next: Option<char>, is_last_char: bool) -> (Option<char>, bool) {
    match curr {
        'B' | 'P' | 'F' | 'V' => (Some('1'), false),
        'C' | 'S' | 'K' | 'G' | 'J' | 'Q' | 'X' | 'Z' => (Some('2'), false),
        'D' | 'T' => match next {
            Some('C') => (None, false),
            _ => (Some('3'), false),
        },
        'L' => {
            if is_vowel(next) || is_last_char {
                (Some('4'), false)
            } else {
                (None, false)
            }
        }
        'M' | 'N' => (Some('5'), matches!(next, Some('D') | Some('G'))),
        'R' => {
            if is_vowel(next) || is_last_char {
                (Some('6'), false)
            } else {
                (None, false)
            }
        }
        _ => (Some('0'), false),
    }
}

/// Vowel test for the `L`/`R` context rules:
/// ASCII-lowercase the character, then match `a e i o u` **and `y`**
/// (`Y` counts here — `Ellery` → `A460`
/// depends on it). Non-ASCII letters are never vowels here, exactly as in
/// the reference suite.
#[inline]
fn is_vowel(c: Option<char>) -> bool {
    match c {
        Some(c) => matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y'),
        None => false,
    }
}

/// Streaming equivalent of the clean + trailing-`S`
/// removal: yields the input's alphabetic characters uppercased (full
/// Unicode case mapping, so one input char may yield several), with every
/// maximal run of `S` that touches the end of the stream dropped.
///
/// Trailing-`S` removal normally needs the whole string; here an `S` run is
/// only *counted* (`pending_s`) until a later non-`S` letter proves it was
/// not trailing, at which point the run is replayed. Memory use is O(1)
/// regardless of input length.
struct CleanChars<'a> {
    inner: std::str::Chars<'a>,
    /// `S` characters seen but not yet known to be non-trailing.
    pending_s: usize,
    /// The non-`S` character that proved a pending `S` run non-trailing;
    /// emitted after the run is replayed.
    held: Option<char>,
}

impl<'a> CleanChars<'a> {
    fn new(token: &'a str) -> Self {
        Self {
            inner: token.chars(),
            pending_s: 0,
            held: None,
        }
    }

    /// The next `A`-`Z` letter, uppercased. Every other scalar is skipped.
    fn next_upper(&mut self) -> Option<char> {
        self.inner
            .find(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase())
    }
}

impl Iterator for CleanChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if self.held.is_some() {
                if self.pending_s > 0 {
                    self.pending_s -= 1;
                    return Some('S');
                }
                return self.held.take();
            }
            match self.next_upper() {
                // End of input: any pending `S` run was trailing — drop it.
                None => return None,
                Some('S') => self.pending_s += 1,
                Some(c) => {
                    if self.pending_s > 0 {
                        self.held = Some(c);
                        self.pending_s -= 1;
                        return Some('S');
                    }
                    return Some(c);
                }
            }
        }
    }
}

/// The full preprocessed character stream: [`CleanChars`] with the leading
/// transformations applied to the first one or two characters, in order:
///
/// 1. leading pair (`KN`→`NN`, `PH`→`FH`, `WR`→`RR` — only the first letter
///    is replaced);
/// 2. one leading `H` removed (checked once, after the pair rewrite);
/// 3. leading-letter substitution (`EIOUY`→`A`, `P`→`B`, `V`→`F`, `KQ`→`C`,
///    `J`→`G`, `Z`→`S`) — applied to the character exposed by the `H`
///    removal, which therefore never gets the *pair* treatment
///    (`HKNUTH` → `CNUTH`, not `NNUTH`).
struct PreChars<'a> {
    clean: CleanChars<'a>,
    first: Option<char>,
    second: Option<char>,
}

impl<'a> PreChars<'a> {
    fn new(token: &'a str) -> Self {
        let mut clean = CleanChars::new(token);
        let mut first = clean.next();
        let mut second = clean.next();

        // Leading pair: replace only the first character.
        match (first, second) {
            (Some('K'), Some('N')) => first = Some('N'),
            (Some('P'), Some('H')) => first = Some('F'),
            (Some('W'), Some('R')) => first = Some('R'),
            _ => {}
        }

        // One leading `H` is dropped. (`second` can only be `None` here when
        // the stream is exhausted, so the shift below cannot lose a char.)
        if first == Some('H') {
            first = second;
            second = clean.next();
        }

        // Leading-letter substitution.
        first = match first {
            Some('E' | 'I' | 'O' | 'U' | 'Y') => Some('A'),
            Some('P') => Some('B'),
            Some('V') => Some('F'),
            Some('K' | 'Q') => Some('C'),
            Some('J') => Some('G'),
            Some('Z') => Some('S'),
            other => other,
        };

        Self {
            clean,
            first,
            second,
        }
    }
}

impl Iterator for PreChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if let Some(c) = self.first.take() {
            return Some(c);
        }
        if let Some(c) = self.second.take() {
            return Some(c);
        }
        self.clean.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects the preprocessed stream, i.e. what the private
    /// `Phonex::preprocess` returns as a `String`.
    fn preprocessed(token: &str) -> String {
        PreChars::new(token).collect()
    }

    fn assert_encodes(cases: &[(&str, &str)]) {
        let phonex = Phonex::new();
        for &(input, expected) in cases {
            assert_eq!(phonex.process(input), expected, "encoding {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // Fixtures from the Lait and Randell paper's own worked examples and
    // from Apache Commons Codec's Phonex suite, which is the same lineage.
    // A few, marked below, are change detectors rather than paper-derived.
    // ------------------------------------------------------------------

    /// Commons Codec `test_preprocess` (12 fixtures).
    #[test]
    fn commons_codec_preprocess_fixtures() {
        for (input, expected) in [
            ("TESTSSS", "TEST"),
            ("SSS", ""),
            ("KNUTH", "NNUTH"),
            ("PHONETIC", "FHONETIC"),
            ("WRIGHT", "RRIGHT"),
            ("HARRINGTON", "ARRINGTON"),
            ("EIGER", "AIGER"),
            ("PERCIVAL", "BERCIVAL"),
            ("VERTIGAN", "FERTIGAN"),
            ("KELVIN", "CELVIN"),
            ("JONES", "GONE"),
            ("ZEPHYR", "SEPHYR"),
        ] {
            assert_eq!(preprocessed(input), expected, "preprocessing {input:?}");
        }
    }

    /// Commons Codec `test_transcode` (25 fixtures).
    #[test]
    fn commons_codec_transcode_fixtures() {
        for (curr, next, is_last_char, code, skip_next_char) in [
            ('B', None, false, Some('1'), false),
            ('P', None, false, Some('1'), false),
            ('F', None, false, Some('1'), false),
            ('V', None, false, Some('1'), false),
            ('C', None, false, Some('2'), false),
            ('S', None, false, Some('2'), false),
            ('K', None, false, Some('2'), false),
            ('G', None, false, Some('2'), false),
            ('J', None, false, Some('2'), false),
            ('Q', None, false, Some('2'), false),
            ('X', None, false, Some('2'), false),
            ('Z', None, false, Some('2'), false),
            ('D', None, false, Some('3'), false),
            ('T', None, false, Some('3'), false),
            ('D', Some('C'), false, None, false),
            ('T', Some('C'), false, None, false),
            ('L', Some('A'), false, Some('4'), false),
            ('L', Some('B'), true, Some('4'), false),
            ('L', Some('B'), false, None, false),
            ('M', None, false, Some('5'), false),
            ('N', None, false, Some('5'), false),
            ('M', Some('D'), false, Some('5'), true),
            ('M', Some('G'), false, Some('5'), true),
            ('R', Some('A'), false, Some('6'), false),
            ('R', None, true, Some('6'), false),
        ] {
            assert_eq!(
                transcode(curr, next, is_last_char),
                (code, skip_next_char),
                "transcoding {curr:?} before {next:?} (last: {is_last_char})"
            );
        }
    }

    /// Commons Codec `test_encode` (54 fixtures).
    #[test]
    fn commons_codec_encode_fixtures() {
        assert_encodes(&[
            ("123 testsss", "T230"),
            ("24/7 test", "T230"),
            ("A", "A000"),
            ("Ashcraft", "A261"),
            ("Lee", "L000"),
            ("Kuhne", "C500"),
            ("Meyer-Lansky", "M452"),
            ("Oepping", "A150"),
            ("Daley", "D400"),
            ("Dalitz", "D432"),
            ("Duhlitz", "D432"),
            ("Dull", "D400"),
            ("De Ledes", "D430"),
            ("Sandemann", "S500"),
            ("Schmidt", "S530"),
            ("Sinatra", "S536"),
            ("Heinrich", "A562"),
            ("Hammerschlag", "A524"),
            ("Williams", "W450"),
            ("Wilms", "W500"),
            ("Wilson", "W250"),
            ("Worms", "W500"),
            ("Zedlitz", "S343"),
            ("Zotteldecke", "S320"),
            ("ZYX test", "S232"),
            ("Scherman", "S500"),
            ("Schurman", "S500"),
            ("Sherman", "S500"),
            ("Shermansss", "S500"),
            ("Shireman", "S650"),
            ("Shurman", "S500"),
            ("Euler", "A460"),
            ("Ellery", "A460"),
            ("Hilbert", "A130"),
            ("Heilbronn", "A165"),
            ("Gauss", "G000"),
            ("Ghosh", "G200"),
            ("Knuth", "N300"),
            ("Kant", "C530"),
            ("Lloyd", "L430"),
            ("Ladd", "L300"),
            ("Lukasiewicz", "L200"),
            ("Lissajous", "L200"),
            ("Philip", "F410"),
            ("Fripp", "F610"),
            ("Czarkowska", "C200"),
            ("Hornblower", "A514"),
            ("Looser", "L260"),
            ("Wright", "R623"),
            ("Phonic", "F520"),
            ("Quickening", "C250"),
            ("Kuickening", "C250"),
            ("Joben", "G150"),
            ("Zelda", "S300"),
        ]);
    }

    /// Commons Codec `test_encode_number` and `test_encode_empty_string`.
    #[test]
    fn commons_codec_number_and_empty_fixtures() {
        assert_encodes(&[("123456789", "0000"), ("", "0000")]);
    }

    // ------------------------------------------------------------------
    // Verbora edge cases. Every expectation below was derived by tracing
    // Commons Codec's code path by hand; none diverges from it.
    // ------------------------------------------------------------------

    #[test]
    fn single_letters() {
        assert_encodes(&[
            ("A", "A000"),
            ("B", "B000"),
            // Preprocessing consumes the whole input.
            ("H", "0000"), // leading-H removal
            ("S", "0000"), // trailing-S removal
            ("s", "0000"),
            // Leading-letter substitution changes the emitted head.
            ("E", "A000"),
            ("Y", "A000"),
            ("P", "B000"),
            ("V", "F000"),
            ("K", "C000"),
            ("Q", "C000"),
            ("J", "G000"),
            ("Z", "S000"),
            // L and R are word-final here, so their digit is computed (but
            // never pushed for a first character).
            ("L", "L000"),
            ("R", "R000"),
            ("X", "X000"),
        ]);
    }

    #[test]
    fn non_letters_only() {
        assert_encodes(&[
            ("   ", "0000"),
            ("!!!", "0000"),
            ("-'\u{2019}", "0000"),
            ("\t\n", "0000"),
            ("0", "0000"),
            ("42", "0000"),
            ("😀🚀", "0000"),
        ]);
    }

    #[test]
    fn mixed_case_and_embedded_noise() {
        let phonex = Phonex::new();
        assert_eq!(phonex.process("knuth"), "N300");
        assert_eq!(phonex.process("KnUtH"), "N300");
        assert_eq!(phonex.process("k n u t h"), "N300");
        assert_eq!(phonex.process("K9N-U_T.H!"), "N300");
        assert_eq!(phonex.process("wright"), phonex.process("WRIGHT"));
    }

    /// The text unit, enumerated over one scalar of every class. A scalar
    /// outside `A`-`Z` is skipped, so a name codes exactly as its
    /// ASCII-letters-only spelling and every code is pure ASCII of the
    /// configured length.
    #[test]
    fn only_ascii_letters_are_read() {
        let phonex = Phonex::new();
        for input in [
            "",
            " ",
            "12345",
            "...",
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{1F600}",
            "\u{041c}\u{043e}",
        ] {
            assert_eq!(phonex.process(input), "0000", "for {input:?}");
        }
        assert_eq!(phonex.process("caf\u{e9}"), phonex.process("caf"));
        assert_eq!(phonex.process("\u{e4}hnlich"), phonex.process("hnlich"));
        assert_eq!(phonex.process("stra\u{df}e"), phonex.process("strae"));
        assert_eq!(phonex.process("\u{10428}"), "0000");
        // The code is always exactly `max_code_length` ASCII characters, so
        // the byte length and the character length can never disagree.
        for input in ["caf\u{e9}", "\u{65e5}ba", "Sinatra", "", "\u{10428}x"] {
            for len in [2usize, 4, 6] {
                let code = Phonex::with_max_code_length(len).process(input);
                assert!(code.is_ascii(), "{input:?} at {len}: {code:?}");
                assert_eq!(code.len(), len, "{input:?} at {len}: {code:?}");
                assert_eq!(code.chars().count(), len);
            }
        }
    }

    /// `\u{df}` is not an `A`-`Z` letter, so it is skipped rather than case-expanded
    /// into a pair of `S`es that the trailing-`S` strip would then eat.
    #[test]
    fn sharp_s_is_skipped_not_expanded() {
        let phonex = Phonex::new();
        assert_eq!(preprocessed("\u{df}"), "");
        assert_eq!(phonex.process("\u{df}"), "0000");
        assert_eq!(phonex.process("Stra\u{df}e"), phonex.process("Strae"));
        assert_ne!(phonex.process("Stra\u{df}e"), phonex.process("Strasse"));
        assert_eq!(phonex.process("a\u{df}"), "A000");
    }

    /// Trailing-`S` removal strips runs of any length, but only at the end.
    #[test]
    fn trailing_s_runs() {
        let phonex = Phonex::new();
        assert_eq!(preprocessed("ASAS"), "ASA");
        assert_eq!(phonex.process("asa s"), "A200");
        // A long trailing run exercises the streaming counter.
        let long_tail = format!("T{}", "s".repeat(4096));
        assert_eq!(phonex.process(&long_tail), "T000");
        assert_eq!(phonex.process(&"s".repeat(4096)), "0000");
        // Interior runs are kept.
        assert_eq!(preprocessed("ASSSSA"), "ASSSSA");
    }

    /// The M/N swallow in the very first position bumps the reference suite's loop
    /// index, so the first iteration pushes its digit — where the same
    /// letter before anything else pushes nothing.
    #[test]
    fn leading_nasal_swallow_quirk() {
        assert_encodes(&[
            ("Ng", "N500"),
            ("Nd", "N500"),
            ("Mg", "M500"),
            ("Md", "M500"),
            // Control group: no swallow, no digit from the head.
            ("Na", "N000"),
            ("Ma", "M000"),
            ("Nt", "N300"),
        ]);
    }

    /// The suppressed-duplicate reset: after a non-pushing iteration `last`
    /// rewinds to the head letter, so a *carried* code (here the `2` kept
    /// through `R`'s silenced context) gets pushed after all.
    #[test]
    fn duplicate_suppression_reset_quirk() {
        assert_encodes(&[
            ("Czarkowska", "C200"),
            // D/T-before-C silence carries the previous code the same way.
            ("Sandemann", "S500"),
        ]);
    }

    /// Leading-`H` removal happens once, and the letter it exposes gets the
    /// substitution table but never the pair table.
    #[test]
    fn leading_h_cases() {
        let phonex = Phonex::new();
        assert_eq!(preprocessed("HHART"), "HART");
        assert_eq!(phonex.process("Hhart"), "H300");
        assert_eq!(phonex.process("Hh"), "H000");
        assert_eq!(phonex.process("Hhh"), "H000");
        // H-removal exposes K, which is *substituted* (K→C), not
        // pair-rewritten (KN→NN).
        assert_eq!(preprocessed("HKNUTH"), "CNUTH");
        assert_eq!(phonex.process("Hknuth"), "C530");
    }

    /// Leading pairs replace only their first letter, and interact with the
    /// trailing-S strip that runs before them.
    #[test]
    fn leading_pair_cases() {
        let phonex = Phonex::new();
        assert_eq!(preprocessed("KNS"), "NN");
        assert_eq!(phonex.process("kns"), "N000");
        assert_eq!(preprocessed("PHS"), "FH");
        assert_eq!(phonex.process("phs"), "F000");
        assert_eq!(preprocessed("WRS"), "RR");
        assert_eq!(phonex.process("wrs"), "R600");
        // Pair letters *not* at the head are ordinary.
        assert_eq!(phonex.process("Akn"), "A250");
    }

    /// `Y` counts as a vowel for the L/R context rules (the reference suite passes
    /// `include_y = true`): `R` before `Y` codes (`Ary`), `R` before a
    /// consonant does not (`Arb`).
    #[test]
    fn y_is_a_context_vowel() {
        assert_encodes(&[("Ellery", "A460"), ("Ary", "A600"), ("Arb", "A100")]);
    }

    /// Longer chains where the three context rules and the index bookkeeping
    /// interlock — repeated nasal swallows, D/T-before-C silences feeding the
    /// duplicate-suppression reset, and L/R context flips. All values
    /// recorded from Commons Codec.
    #[test]
    fn recorded_context_rule_chains() {
        assert_encodes(&[
            // Repeated M/N+D/G swallows, incl. the first-position quirk.
            ("ndgndgndg", "N525"),
            ("knknkn", "N252"),
            ("ngng", "N500"),
            ("mgmg", "M500"),
            // Swallow after other letters, with dedup reset in between.
            ("sandgmann", "S525"),
            ("mndgl", "M240"),
            // D-before-C silence carrying the previous code.
            ("dcdcdc", "D200"),
            ("tctc", "T200"),
            ("dcl", "D240"),
            ("ldc", "L200"),
            // L/R context: silenced (before consonant) versus coded (before
            // vowel/Y or at word end).
            ("rlrlrl", "R400"),
            ("lrlrlr", "L600"),
            ("rylr", "R600"),
            ("arlb", "A100"),
            // A pure same-digit run: the reset lets the THIRD S push the
            // digit the second suppressed (same mechanism as Czarkowska).
            ("ssssb", "S210"),
        ]);
    }

    #[test]
    fn configurable_length() {
        assert_eq!(Phonex::with_max_code_length(0).process("Sinatra"), "");
        assert_eq!(Phonex::with_max_code_length(0).process(""), "");
        assert_eq!(Phonex::with_max_code_length(1).process("Sinatra"), "S");
        assert_eq!(Phonex::with_max_code_length(1).process(""), "0");
        assert_eq!(Phonex::with_max_code_length(2).process("Sinatra"), "S5");
        assert_eq!(Phonex::with_max_code_length(6).process("Sinatra"), "S53600");
        assert_eq!(
            Phonex::with_max_code_length(10).process("Sinatra"),
            "S536000000"
        );
        assert_eq!(Phonex::with_max_code_length(6).process(""), "000000");
    }

    /// The configured length is honoured exactly, for every input: with an
    /// all-ASCII code there is no scalar that can overshoot it.
    #[test]
    fn the_configured_length_is_never_overshot() {
        for len in [1usize, 2, 3, 4, 8] {
            let phonex = Phonex::with_max_code_length(len);
            for input in [
                "\u{65e5}ba",
                "\u{65e5}\u{672c}\u{8a9e}",
                "Sinatra",
                "",
                "caf\u{e9}",
            ] {
                assert_eq!(phonex.process(input).len(), len, "{input:?} at {len}");
            }
        }
        assert_eq!(Phonex::with_max_code_length(2).process("\u{65e5}ba"), "B0");
    }

    #[test]
    fn very_long_input() {
        let phonex = Phonex::new();
        assert_eq!(phonex.process(&"a".repeat(10_000)), "A000");
        assert_eq!(phonex.process(&"ab".repeat(5_000)), "A100");
        assert_eq!(phonex.process(&"Czarkowska".repeat(1_000)), "C200");
    }

    #[test]
    fn compare_matches_code_equality() {
        let phonex = Phonex::new();
        assert!(phonex.compare("Knuth", "Nuth"));
        assert!(phonex.compare("Schmidt", "Schmit"));
        assert!(phonex.compare("Dalitz", "Duhlitz"));
        assert!(phonex.compare("", "123"));
        assert!(!phonex.compare("Wilson", "Worms"));
        // Length changes what collides.
        assert!(Phonex::with_max_code_length(1).compare("Sinatra", "Sherman"));
        assert!(!Phonex::new().compare("Sinatra", "Sherman"));
    }

    #[test]
    fn constructors_and_getters() {
        assert_eq!(Phonex::default(), Phonex::new());
        assert_eq!(Phonex::new().max_code_length(), 4);
        assert_eq!(Phonex::with_max_code_length(4), Phonex::new());
        assert_eq!(Phonex::with_max_code_length(7).max_code_length(), 7);
    }

    #[test]
    fn phonetic_trait_delegates() {
        let phonex: &dyn verbora_core::Phonetic = &Phonex::new();
        assert_eq!(phonex.process("Knuth"), "N300");
        assert!(phonex.compare("Knuth", "Nuth"));
    }
}
