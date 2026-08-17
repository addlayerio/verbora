//! Russell/NARA-style SoundEx, as the reference implements it.
//!
//! Ports the reference `soundex`. This is **not** textbook SoundEx —
//! see [`SoundEx::process`] for the three places it deviates, each of which is
//! reachable from ordinary input.

use std::borrow::Cow;

use crate::PhoneticError;
use crate::units::{clamp_take, coerce_or_default, utf16_len, utf16_to_lowercase};

/// Russell/NARA-style SoundEx.
///
/// ```
/// use verbora_phonetics::SoundEx;
///
/// let soundex = SoundEx::new();
/// assert_eq!(soundex.process("render"), "R536");
/// assert_eq!(soundex.process("phonetics"), "P532");
/// assert!(soundex.compare("ant", "and"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoundEx;

/// Maps a code unit to its SoundEx digit, or leaves it alone.
///
/// The reference composes six global regular expressions
/// (`transformR`, `transformHum`, `transformL`, `transformToungue`,
/// `transformThroats`, `transformLipps`). Their character classes are pairwise
/// disjoint and every output is a digit, so no later pass can re-match an
/// earlier pass's output: the composition is *exactly* one per-code-unit map,
/// which is what this table is. All six patterns are **lowercase-only** — there
/// is no `/i` flag — so uppercase input passes through untouched, and callers of
/// [`SoundEx::transform`] see that.
#[inline]
const fn soundex_digit(c: char) -> Option<u8> {
    match c {
        'b' | 'f' | 'p' | 'v' => Some(b'1'),
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => Some(b'2'),
        'd' | 't' => Some(b'3'),
        'l' => Some(b'4'),
        'm' | 'n' => Some(b'5'),
        'r' => Some(b'6'),
        _ => None,
    }
}

/// Whether a character is a digit *after* `transform` has run.
///
/// Two kinds of digit reach the `/\D/g` filter: the ones the class map produces,
/// and the ones that were already in the input. `transform` leaves the latter
/// alone, so `SoundEx::process("12345")` is `"1234"` — the input's own digits
/// become part of the code.
#[inline]
const fn transformed_digit(c: char) -> Option<u8> {
    match soundex_digit(c) {
        Some(d) => Some(d),
        None if c.is_ascii_digit() => Some(c as u8),
        None => None,
    }
}

/// What `transformed.replace(new RegExp('^' + transform(token.charAt(0))), '')`
/// does for a given first character.
///
/// The reference builds a regular expression **out of user input**, so the first
/// character is not merely compared — it is interpreted. This enum enumerates
/// every behaviour that interpretation can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Initial {
    /// `^<digit>`: strips one leading occurrence of that digit. This is the
    /// intended case — "deal with duplicate INITIAL consonant SOUNDS".
    Digit(u8),
    /// `^.`: strips *any* leading code unit. Only digit removal is observable,
    /// because the `\D` filter that follows would have dropped anything else.
    AnyChar,
    /// `^^`, `^$`, `^|`: the pattern matches the empty string, so the replace is
    /// a no-op.
    Nothing,
    /// A literal that is not a digit. It can only ever delete a character the
    /// `\D` filter deletes anyway, so it too is unobservable.
    NonDigitLiteral,
    /// `new RegExp` itself throws: `(`, `)`, `*`, `+`, `?`, `[`, `\`.
    Invalid,
}

impl Initial {
    /// Classifies the first character of the (already lowercased) token.
    fn classify(first: char) -> Self {
        if let Some(d) = soundex_digit(first) {
            return Self::Digit(d);
        }
        match first {
            // A literal digit survives `transform` unchanged and then anchors a
            // perfectly ordinary `^<digit>` pattern.
            '0'..='9' => Self::Digit(first as u8),
            '.' => Self::AnyChar,
            '^' | '$' | '|' => Self::Nothing,
            '(' | ')' | '*' | '+' | '?' | '[' | '\\' => Self::Invalid,
            // `{`, `}`, `]` and `-` are literals outside a character class under
            // the Annex B grammar every browser and Node implements.
            _ => Self::NonDigitLiteral,
        }
    }
}

impl SoundEx {
    /// Creates a SoundEx encoder. It holds no state; the type exists to mirror
    /// The reference `SoundEx`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` with the default code length of four characters.
    ///
    /// # Divergence from the reference
    ///
    /// When the first character is `(`, `)`, `*`, `+`, `?`, `[` or `\`, the
    /// reference throws a `SyntaxError` from the regular expression it builds
    /// out of that character. This method instead performs no initial-sound
    /// strip and returns a code, because a text-processing library should not
    /// fail on punctuation. [`SoundEx::try_process`] reproduces the throw
    /// exactly.
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        self.process_with(token, None)
    }

    /// Encodes `token`, honouring the reference's `maxLength` coercion.
    ///
    /// `max_length` is `Option<f64>` rather than `Option<usize>` because the
    /// argument is a reference number whose *non*-integral and negative values
    /// are observable: The reference computes `(maxLength && maxLength - 1) || 3`
    /// and feeds it to `String#substr`. So `Some(1.0)` behaves like the default
    /// (`1 && 0` is falsy), `Some(-1.0)` yields just the initial letter, and
    /// `Some(1.5)` yields the initial letter too (`substr(0, 0.5)` truncates to
    /// zero).
    ///
    /// See [`Self::process`] for the one divergence.
    #[must_use]
    pub fn process_with(&self, token: &str, max_length: Option<f64>) -> String {
        self.encode(token, max_length, false)
            .unwrap_or_else(|_| unreachable!("lenient mode never fails"))
            .into_string()
    }

    /// Encodes `token` with exact the reference semantics, including the
    /// `SyntaxError` it throws for tokens whose first character is a regular
    /// expression metacharacter.
    ///
    /// # Errors
    ///
    /// Returns [`PhoneticError::InvalidInitialPattern`] for a token beginning
    /// with `(`, `)`, `*`, `+`, `?`, `[` or `\`.
    pub fn try_process(
        &self,
        token: &str,
        max_length: Option<f64>,
    ) -> Result<String, PhoneticError> {
        Ok(self.encode(token, max_length, true)?.into_string())
    }

    /// Like [`Self::try_process`], but returns UTF-16 code units.
    ///
    /// The reference keeps `token.charAt(0)` — the first **code unit** — so a
    /// token starting with an astral character produces a code that begins with
    /// an unpaired high surrogate (`SoundEx.process("😀")` is `"\u{D83D}000"`).
    /// A Rust `String` cannot hold that; [`Self::try_process`] therefore
    /// substitutes `U+FFFD`, and this method exists for callers that need the
    /// exact sequence.
    ///
    /// # Errors
    ///
    /// As [`Self::try_process`].
    pub fn try_process_utf16(
        &self,
        token: &str,
        max_length: Option<f64>,
    ) -> Result<Vec<u16>, PhoneticError> {
        Ok(self.encode(token, max_length, true)?.into_utf16())
    }

    /// Whether two strings share a SoundEx code, at the default length.
    ///
    /// Mirrors the inherited `Phonetic#compare`, and therefore uses the lenient
    /// [`Self::process`].
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }

    /// The whole pipeline, once, shared by every entry point.
    fn encode(
        &self,
        token: &str,
        max_length: Option<f64>,
        strict: bool,
    ) -> Result<Encoded, PhoneticError> {
        let lower = utf16_to_lowercase(token);

        let mut chars = lower.chars();
        let Some(first) = chars.next() else {
            // `''.charAt(0)` is `''`, so the code is the padding alone: three
            // characters, with no initial letter. Every other input yields four.
            return Ok(Encoded {
                head: Head::Empty,
                tail: pad_and_take(Vec::new(), max_length),
            });
        };
        // `substr(1, …)` drops one CODE UNIT. For an astral first character that
        // leaves its low surrogate at the front of the remainder — a non-digit,
        // so it cannot affect condensation or the digit filter, and dropping the
        // whole character here is observationally identical.
        let rest = chars.as_str();

        let initial = Initial::classify(first);
        if strict && initial == Initial::Invalid {
            return Err(PhoneticError::InvalidInitialPattern(first));
        }

        // `condense(transform(rest))` runs BEFORE non-digits are stripped, so
        // vowels, `h` and `w` separate two equal digits instead of merging with
        // them. This is why `Ashcraft` encodes as `A226` here and `A261` in the
        // textbook algorithm.
        let mut digits: Vec<u8> = Vec::with_capacity(rest.len().min(16));
        let mut prev_digit: Option<u8> = None;
        let mut first_emitted: Option<Option<u8>> = None;
        for c in rest.chars() {
            match transformed_digit(c) {
                Some(d) => {
                    if prev_digit == Some(d) {
                        continue; // collapsed by `/(\d)?\1+/g`
                    }
                    prev_digit = Some(d);
                    first_emitted.get_or_insert(Some(d));
                    digits.push(d);
                }
                None => {
                    prev_digit = None;
                    first_emitted.get_or_insert(None);
                }
            }
        }

        // The initial-strip removes at most the first code unit, and only a
        // digit removal survives the `\D` filter that follows.
        let leading_digit = first_emitted.flatten();
        let strips = match (initial, leading_digit) {
            (Initial::Digit(d), Some(l)) => d == l,
            (Initial::AnyChar, Some(_)) => true,
            _ => false,
        };
        if strips {
            digits.remove(0);
        }

        Ok(Encoded {
            head: Head::of(first),
            tail: pad_and_take(digits, max_length),
        })
    }

    // -- prototype methods, exposed because the reference exposes them ----------

    /// `token.replace(/[bfpv]/g, '1')`. Lowercase-only, like the reference.
    #[must_use]
    pub fn transform_lipps<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(token, |c| matches!(c, 'b' | 'f' | 'p' | 'v'), b'1')
    }

    /// `token.replace(/[cgjkqsxz]/g, '2')`. Lowercase-only.
    #[must_use]
    pub fn transform_throats<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(
            token,
            |c| matches!(c, 'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z'),
            b'2',
        )
    }

    /// `token.replace(/[dt]/g, '3')`. Lowercase-only.
    ///
    /// The reference spells this `transformToungue`; the misspelling is part of
    /// its public API, but is not reproduced here.
    #[must_use]
    pub fn transform_toungue<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(token, |c| matches!(c, 'd' | 't'), b'3')
    }

    /// `token.replace(/l/g, '4')`. Lowercase-only.
    #[must_use]
    pub fn transform_l<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(token, |c| c == 'l', b'4')
    }

    /// `token.replace(/[mn]/g, '5')`. Lowercase-only.
    #[must_use]
    pub fn transform_hum<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(token, |c| matches!(c, 'm' | 'n'), b'5')
    }

    /// `token.replace(/r/g, '6')`. Lowercase-only.
    #[must_use]
    pub fn transform_r<'a>(&self, token: &'a str) -> Cow<'a, str> {
        map_class(token, |c| c == 'r', b'6')
    }

    /// The composition of the six class transforms, in one pass.
    ///
    /// See `soundex_digit` for why collapsing six sequential regular
    /// expressions into a single map is exact rather than merely equivalent.
    #[must_use]
    pub fn transform<'a>(&self, token: &'a str) -> Cow<'a, str> {
        if !token.chars().any(|c| soundex_digit(c).is_some()) {
            return Cow::Borrowed(token);
        }
        let mut out = String::with_capacity(token.len());
        for c in token.chars() {
            match soundex_digit(c) {
                Some(d) => out.push(char::from(d)),
                None => out.push(c),
            }
        }
        Cow::Owned(out)
    }

    /// Collapses each maximal run of the same digit to one occurrence.
    ///
    /// The reference writes this as `token.replace(/(\d)?\1+/g, '$1')` — an
    /// optional capture group with a **backreference**, which the Rust `regex`
    /// crate cannot express at all. The pattern also matches the empty string at
    /// every position (the group does not participate, so `\1` matches empty and
    /// `$1` expands to nothing); those matches are no-ops, which is why a
    /// literal translation is both impossible and unnecessary. Non-digits are
    /// never collapsed: `condense("aabbcc") == "aabbcc"`.
    #[must_use]
    pub fn condense<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let mut prev: Option<char> = None;
        let has_run = token.chars().any(|c| {
            let run = c.is_ascii_digit() && prev == Some(c);
            prev = Some(c);
            run
        });
        if !has_run {
            return Cow::Borrowed(token);
        }

        let mut out = String::with_capacity(token.len());
        let mut prev: Option<char> = None;
        for c in token.chars() {
            if c.is_ascii_digit() && prev == Some(c) {
                continue;
            }
            prev = Some(c);
            out.push(c);
        }
        Cow::Owned(out)
    }

    /// Pads a code to three characters, and only to three.
    ///
    /// `Array(4 - n).join('0')` yields `3 - n` zeros, so anything shorter than
    /// four characters becomes exactly three and anything else is returned
    /// unchanged. The length measured is the reference's, in UTF-16 code units.
    #[must_use]
    pub fn pad_right0<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let len = utf16_len(token);
        if len >= 3 {
            return Cow::Borrowed(token);
        }
        let mut out = String::with_capacity(token.len() + 3 - len);
        out.push_str(token);
        for _ in len..3 {
            out.push('0');
        }
        Cow::Owned(out)
    }
}

/// `padRight0(digits).substr(0, (maxLength && maxLength - 1) || 3)`, fused.
fn pad_and_take(mut digits: Vec<u8>, max_length: Option<f64>) -> Vec<u8> {
    // `1` is falsy after the decrement, so it selects the default rather than
    // producing a one-character code — a genuine quirk, not an off-by-one.
    let want = match max_length {
        Some(m) if m != 0.0 && !m.is_nan() => coerce_or_default(Some(m - 1.0), 3.0),
        _ => 3.0,
    };
    if digits.len() < 4 {
        digits.resize(3, b'0');
    }
    digits.truncate(clamp_take(want, digits.len()));
    digits
}

/// `token.charAt(0).toUpperCase()` — the first **code unit**, uppercased.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Head {
    /// The token was empty, so `charAt(0)` returned `''`.
    Empty,
    /// A BMP character's uppercase form. It can be longer than one character:
    /// `ß` uppercases to `SS`, giving a five-character code.
    Text(String),
    /// The high surrogate of an astral first character, orphaned by `charAt(0)`.
    Surrogate(u16),
}

impl Head {
    fn of(first: char) -> Self {
        if first.len_utf16() == 2 {
            let mut buf = [0u16; 2];
            Self::Surrogate(first.encode_utf16(&mut buf)[0])
        } else {
            Self::Text(first.to_uppercase().collect())
        }
    }
}

/// A finished code, held in the form that keeps every representation exact.
struct Encoded {
    head: Head,
    tail: Vec<u8>,
}

impl Encoded {
    /// Lossy rendering: an orphaned surrogate becomes `U+FFFD`.
    fn into_string(self) -> String {
        // The tail is nothing but ASCII digits.
        let tail = std::str::from_utf8(&self.tail).expect("digits are ASCII");
        match self.head {
            Head::Empty => tail.to_owned(),
            Head::Text(mut s) => {
                s.push_str(tail);
                s
            }
            Head::Surrogate(u) => {
                let mut units = vec![u];
                units.extend(self.tail.iter().map(|&b| u16::from(b)));
                String::from_utf16_lossy(&units)
            }
        }
    }

    /// Exact rendering, code unit for code unit.
    fn into_utf16(self) -> Vec<u16> {
        let mut units: Vec<u16> = match &self.head {
            Head::Empty => Vec::with_capacity(self.tail.len()),
            Head::Text(s) => s.encode_utf16().collect(),
            Head::Surrogate(u) => vec![*u],
        };
        units.extend(self.tail.iter().map(|&b| u16::from(b)));
        units
    }
}

/// `token.replace(/[class]/g, digit)` for one of the six class transforms.
fn map_class<'a>(token: &'a str, in_class: impl Fn(char) -> bool, digit: u8) -> Cow<'a, str> {
    if !token.chars().any(&in_class) {
        return Cow::Borrowed(token);
    }
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        if in_class(c) {
            out.push(char::from(digit));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
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

    fn sx() -> SoundEx {
        SoundEx::new()
    }

    #[test]
    fn encodes_the_reference_vectors() {
        let s = sx();
        for (input, want) in [
            ("render", "R536"),
            ("super", "S160"),
            ("but", "B300"),
            ("butt", "B300"),
            ("blackberry", "B421"),
            ("BLACKBERRY", "B421"),
            ("calculate", "C424"),
            ("fox", "F200"),
            ("jump", "J510"),
            ("phonetics", "P532"),
            ("Lloyd", "L300"),
            ("Pfister", "P236"),
            ("manhattan", "M535"),
            ("Lukasiewicz", "L222"),
            ("Gauss", "G200"),
            ("Tymczak", "T522"),
            ("Jackson", "J250"),
            ("Robert", "R163"),
            ("Rupert", "R163"),
            ("Honeyman", "H555"),
        ] {
            assert_eq!(s.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn condensation_before_filtering_separates_equal_codes() {
        // Textbook SoundEx gives A261: `h` and `w` are supposed to be removed
        // BEFORE adjacent duplicates are merged. The reference merges first.
        assert_eq!(sx().process("Ashcraft"), "A226");
    }

    #[test]
    fn digits_in_the_input_survive() {
        assert_eq!(sx().process("12345"), "1234");
    }

    #[test]
    fn empty_input_has_no_initial_letter() {
        assert_eq!(sx().process(""), "000");
    }

    #[test]
    fn single_character_inputs() {
        assert_eq!(sx().process("a"), "A000");
        assert_eq!(sx().process("Z"), "Z000");
        assert_eq!(sx().process("'"), "'000");
    }

    #[test]
    fn all_uppercase_matches_all_lowercase() {
        let s = sx();
        assert_eq!(s.process("BLACKBERRY"), s.process("blackberry"));
    }

    #[test]
    fn accented_and_non_latin_letters_pass_through() {
        let s = sx();
        assert_eq!(s.process("café"), "C100");
        assert_eq!(s.process("naïve"), "N100");
        assert_eq!(s.process("ÉCOLE"), "É240");
        assert_eq!(s.process("ç"), "Ç000");
        // Cyrillic and CJK have no SoundEx class at all.
        assert_eq!(s.process("Москва"), "М000");
        assert_eq!(s.process("日本語"), "日000");
    }

    #[test]
    fn case_mapping_can_lengthen_the_code() {
        // charAt(0).toUpperCase() of 'ß' is "SS", so the code is five long.
        assert_eq!(sx().process("ß"), "SS000");
    }

    #[test]
    fn astral_first_character_orphans_a_surrogate() {
        let s = sx();
        // Exact: the high surrogate, then the padding.
        assert_eq!(
            s.try_process_utf16("😀", None).unwrap(),
            vec![0xD83D, 0x30, 0x30, 0x30]
        );
        // Lossy: a Rust String cannot hold the orphan.
        assert_eq!(s.process("😀"), "\u{FFFD}000");
    }

    #[test]
    fn punctuation_and_whitespace() {
        let s = sx();
        assert_eq!(s.process("  "), " 000");
        assert_eq!(s.process("x y"), "X000");
        assert_eq!(s.process("a-b"), "A100");
    }

    #[test]
    fn very_long_input() {
        assert_eq!(sx().process("supercalifragilisticexpialidocious"), "S162");
        assert_eq!(sx().process(&"a".repeat(500)), "A000");
    }

    #[test]
    fn max_length_coercion() {
        let s = sx();
        assert_eq!(s.process_with("phonetics", Some(1.0)), "P532"); // 1 && 0 -> falsy
        assert_eq!(s.process_with("phonetics", Some(2.0)), "P5");
        assert_eq!(s.process_with("phonetics", Some(0.0)), "P532"); // falsy
        assert_eq!(s.process_with("phonetics", Some(f64::NAN)), "P532");
        assert_eq!(s.process_with("phonetics", Some(-1.0)), "P");
        assert_eq!(s.process_with("phonetics", Some(1.5)), "P");
        assert_eq!(s.process_with("jump", Some(8.0)), "J510");
        assert_eq!(
            s.process_with("supercalifragilisticexpialidocious", Some(8.0)),
            "S1624162"
        );
    }

    #[test]
    fn regex_metacharacters_throw_only_in_strict_mode() {
        let s = sx();
        assert_eq!(
            s.try_process("(abc", None),
            Err(PhoneticError::InvalidInitialPattern('('))
        );
        assert_eq!(s.process("(abc"), "(120");
        // `.` really is a wildcard: it eats the first transformed character.
        assert_eq!(s.try_process(".bcd", None).unwrap(), ".230");
        // `^`, `$` and `|` match the empty string, so nothing is stripped.
        assert_eq!(s.try_process("^bcd", None).unwrap(), "^123");
        assert_eq!(s.try_process("|bcd", None).unwrap(), "|123");
    }

    #[test]
    fn helper_methods_match_the_reference() {
        let s = sx();
        assert_eq!(s.transform_lipps("bopper"), "1o11er");
        assert_eq!(s.transform_throats("cgjkqsxz"), "22222222");
        assert_eq!(s.transform_toungue("dat"), "3a3");
        assert_eq!(s.transform_l("lala"), "4a4a");
        assert_eq!(s.transform_hum("mummification"), "5u55ificatio5");
        assert_eq!(s.transform_r("render"), "6ende6");
        assert_eq!(s.transform("render"), "6e53e6");
        // Uppercase input is untouched: none of the patterns carry /i.
        assert_eq!(s.transform("RENDER"), "RENDER");
    }

    #[test]
    fn condense_collapses_only_digit_runs() {
        let s = sx();
        assert_eq!(s.condense("11222556"), "1256");
        assert_eq!(s.condense("1112223"), "123");
        assert_eq!(s.condense("aabbcc"), "aabbcc");
        assert_eq!(s.condense("1a1"), "1a1");
        assert_eq!(s.condense("123321"), "12321");
        assert_eq!(s.condense(""), "");
    }

    #[test]
    fn pad_right0_pads_to_exactly_three() {
        let s = sx();
        assert_eq!(s.pad_right0(""), "000");
        assert_eq!(s.pad_right0("1"), "100");
        assert_eq!(s.pad_right0("12"), "120");
        assert_eq!(s.pad_right0("123"), "123");
        assert_eq!(s.pad_right0("1234"), "1234");
        assert_eq!(s.pad_right0("12345"), "12345");
    }

    #[test]
    fn compare_uses_the_default_length() {
        let s = sx();
        assert!(s.compare("ant", "and"));
        assert!(!s.compare("ant", "anne"));
        assert!(s.compare("band", "bant"));
        assert!(!s.compare("band", "gand"));
    }
}
