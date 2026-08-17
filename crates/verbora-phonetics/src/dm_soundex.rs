//! Daitch-Mokotoff Soundex, as the reference implements it.
//!
//! Ports the reference `dm_soundex`. Despite the name, this is the
//! **single-code** variant: the D-M dual codes for `CK`, `RS` and `RZ` are
//! declared in the table and never read, because the reference always takes
//! `legalState[0]`.

use std::fmt::Write as _;

use crate::PhoneticError;
use crate::dm_table::{Mapping, Node, root};
use crate::units::{Unit, clamp_slice_end, coerce_or_default, utf16_to_uppercase};

/// Daitch-Mokotoff Soundex.
///
/// ```
/// use verbora_phonetics::SoundExDM;
///
/// let dm = SoundExDM::new();
/// assert_eq!(dm.process("ALPERT"), "087930");
/// assert_eq!(dm.process("Ben Aron"), dm.process("BENARON"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoundExDM;

/// What a trie walk settled on.
///
/// Normally a code triple. It can also be a bare number, because `findRules`
/// indexes its nodes with characters taken straight from the input: the digit
/// `0` collides with the table's own legality marker, leaving `mapping` as a
/// number whose `[0]` is `undefined`. The reference then appends the *string*
/// `"undefined"` to its output, which is why `process("B0")` is `"undefi"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleMapping {
    /// `[start of word, before a vowel, any other situation]`; `-1` emits
    /// nothing.
    Triple(Mapping),
    /// The digit-collision case: indexing this yields `undefined`.
    Number(i32),
}

impl RuleMapping {
    /// `mapping[index]`, with `None` for the reference's `undefined`.
    #[must_use]
    pub fn get(self, index: usize) -> Option<i32> {
        match self {
            Self::Triple(t) => t.get(index).copied(),
            Self::Number(_) => None,
        }
    }
}

/// The result of [`SoundExDM::find_rules`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// How many code units the matched prefix consumed. Always at least 1, which
    /// is what guarantees the encoder makes progress.
    pub length: usize,
    /// The codes for that prefix.
    pub mapping: RuleMapping,
}

/// A position in the trie walk.
///
/// The reference's `state` variable is re-assigned from `state[str[offs]]`, and
/// that expression does not always yield a node: a digit key can produce a code
/// triple, and indexing *that* produces a number. Both are then indexed again on
/// the next iteration, so the walk has to model all three.
#[derive(Clone, Copy)]
enum State {
    /// A table node.
    Node(&'static Node),
    /// A code triple, reached by indexing a node with a digit.
    Mapping(Mapping),
    /// A bare number, reached by indexing a triple with a digit. Its value is
    /// deliberately not carried: a reference number has no properties, so both
    /// `state[key]` and `state[0]` are `undefined` from here on and the walk can
    /// only end.
    Number,
}

impl State {
    /// `state[key]`, folding the reference's falsy check into `None` — the walk
    /// breaks on absent *and* on zero, which is why a vowel's `[0, -1, -1]`
    /// never becomes a legal state.
    fn index(self, key: u8) -> Option<Self> {
        let digit = key.is_ascii_digit().then(|| usize::from(key - b'0'));
        match self {
            Self::Node(node) => match digit {
                Some(d) => node.indexed(d).map(Self::Mapping),
                None => node.child(key).map(Self::Node),
            },
            Self::Mapping(m) => match digit.and_then(|d| m.get(d)) {
                // `0` is falsy, so the walk stops there.
                Some(&0) | None => None,
                Some(_) => Some(Self::Number),
            },
            // Numbers have no properties.
            Self::Number => None,
        }
    }

    /// `state[0]`, which is both the legality test and the value `findRules`
    /// finally returns.
    fn zero(self) -> Option<RuleMapping> {
        match self {
            Self::Node(node) => node.zero().map(RuleMapping::Triple),
            Self::Mapping(m) => (m[0] != 0).then_some(RuleMapping::Number(m[0])),
            Self::Number => None,
        }
    }
}

/// The letters whose start-of-word code is `0`, which is exactly the test the
/// reference performs to decide "before a vowel".
///
/// It writes that test as a second `findRules` call on a one-character string
/// and compares `mapping[0]` with `0`; only `A`, `E`, `I`, `O` and `U` qualify.
/// `Y` (code 1) and `J` (code 4) do **not**, despite being vowels in the D-M
/// documentation.
#[inline]
fn is_dm_vowel<U: Unit>(u: U) -> bool {
    matches!(u.to_ascii(), Some(b'A' | b'E' | b'I' | b'O' | b'U'))
}

impl SoundExDM {
    /// Creates a D-M encoder. It holds no state; the type exists to mirror
    /// The reference `SoundExDM`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `word` to the standard six digits.
    #[must_use]
    pub fn process(&self, word: &str) -> String {
        self.try_process(word, None)
            .unwrap_or_else(|_| unreachable!("the default code length is an integer"))
    }

    /// Encodes `word` to `code_length` digits.
    ///
    /// `code_length` is `Option<f64>` because the argument is a reference
    /// number and its odd values are observable: `0`, `NaN` and `None` select
    /// the default of six, while `Some(-1.0)` reaches `String#slice` and drops
    /// the last digit with no padding.
    ///
    /// # Errors
    ///
    /// Returns [`PhoneticError::InvalidArrayLength`] when padding is required
    /// and `code_length` is not a non-negative integer below 2³². The reference
    /// pads with `new Array(n).join('0')`, and `new Array` throws `RangeError`
    /// for such an `n` — `process("ALPERT", 6.5)` really does throw.
    pub fn try_process(
        &self,
        word: &str,
        code_length: Option<f64>,
    ) -> Result<String, PhoneticError> {
        let upper = utf16_to_uppercase(word);
        let output = if upper.is_ascii() {
            encode(upper.as_bytes())
        } else {
            encode(&upper.encode_utf16().collect::<Vec<u16>>())
        };
        normalize(output, code_length)
    }

    /// Whether two words share a D-M code, at the default length.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }

    /// The longest legal prefix of `str`, and its codes.
    ///
    /// The walk continues *through* non-legal intermediate nodes, which is how
    /// clusters like `SCHTSCH` are found even though `SCHTS` is not itself a
    /// legal state. An unknown first character yields `length: 1` with the
    /// mapping `[-1, -1, -1]`, so the encoder always advances.
    #[must_use]
    pub fn find_rules(&self, str: &str) -> Rules {
        if str.is_ascii() {
            find_rules(str.as_bytes())
        } else {
            find_rules(&str.encode_utf16().collect::<Vec<u16>>())
        }
    }

    /// Pads with `'0'` or cuts to fit `length` characters.
    ///
    /// # Errors
    ///
    /// As [`Self::try_process`].
    pub fn normalize_length(
        &self,
        token: &str,
        length: Option<f64>,
    ) -> Result<String, PhoneticError> {
        normalize(token.to_owned(), length)
    }
}

/// [`SoundExDM::normalize_length`], taking ownership so the encoder's buffer is
/// padded in place instead of copied.
fn normalize(mut out: String, length: Option<f64>) -> Result<String, PhoneticError> {
    let want = coerce_or_default(length, 6.0);
    let len = out.len(); // the encoder only ever emits ASCII
    if (len as f64) < want {
        // `new Array(want - len + 1).join('0')`, which throws unless its
        // argument is a valid array length.
        let n = want - len as f64;
        if n.fract() != 0.0 || n + 1.0 >= f64::from(u32::MAX) {
            return Err(PhoneticError::InvalidArrayLength(n + 1.0));
        }
        out.extend(std::iter::repeat_n('0', n as usize));
    }
    // `slice`, not `substring`: a negative end counts back from the end.
    out.truncate(clamp_slice_end(want, out.len()));
    Ok(out)
}

/// `findRules(str)`
fn find_rules<U: Unit>(str: &[U]) -> Rules {
    let first = str.first().and_then(|u| u.to_ascii()).and_then(root);
    let mut state = first.map(State::Node);
    // `legalState = state || [[-1, -1, -1]]`. Every one of the 26 first-letter
    // entries is a legal state, so the fallback below is only reached when the
    // first character is not a table key at all.
    let mut mapping = state
        .and_then(State::zero)
        .unwrap_or(RuleMapping::Triple([-1, -1, -1]));
    let mut chars_involved = 1;

    for (offs, unit) in str.iter().enumerate().skip(1) {
        // A non-ASCII code unit is not a key in the table, so `state[str[offs]]`
        // is `undefined` and the walk stops — same as an unknown letter.
        let Some(next) = state.zip(unit.to_ascii()).and_then(|(s, key)| s.index(key)) else {
            break;
        };
        state = Some(next);
        if let Some(m) = next.zero() {
            mapping = m;
            chars_involved = offs + 1;
        }
    }

    Rules {
        length: chars_involved,
        mapping,
    }
}

/// The encoding loop, before length normalisation.
fn encode<U: Unit>(word: &[U]) -> String {
    let mut output = String::with_capacity(8);
    let mut pos = 0;
    // `lastCode` starts at the number -1, not at `undefined`.
    let mut last_code = Some(-1);

    while pos < word.len() {
        let substr = &word[pos..];
        let rules = find_rules(substr);

        let code = if pos == 0 {
            rules.mapping.get(0)
        } else if substr.get(rules.length).copied().is_some_and(is_dm_vowel) {
            rules.mapping.get(1)
        } else {
            rules.mapping.get(2)
        };

        // Codes are compared as whole numbers, so 6 and 66 are distinct and a
        // two-digit code contributes two characters. `undefined !== undefined`
        // is false in the reference, so consecutive `undefined` codes collapse —
        // which `Option`'s own equality reproduces.
        if code != Some(-1) && code != last_code {
            match code {
                // Every code in the table is one or two digits; the formatter is
                // kept only so a hand-built `Rules` cannot silently misprint.
                Some(n @ 0..=99) => {
                    if n >= 10 {
                        output.push(char::from(b'0' + (n / 10) as u8));
                    }
                    output.push(char::from(b'0' + (n % 10) as u8));
                }
                Some(n) => {
                    let _ = write!(output, "{n}");
                }
                None => output.push_str("undefined"),
            }
        }
        last_code = code;
        pos += rules.length;
    }

    output
}

impl verbora_core::Phonetic for SoundExDM {
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

    fn dm() -> SoundExDM {
        SoundExDM::new()
    }

    #[test]
    fn encodes_the_reference_vectors() {
        let d = dm();
        for (input, want) in [
            ("ALPERT", "087930"),
            ("GOLDEN", "583600"),
            ("BREUER", "791900"),
            ("FREUD", "793000"),
            ("HABER", "579000"),
            ("MANHEIM", "665600"),
            ("MINTZ", "664000"),
            ("TOPF", "370000"),
            ("KLEINMAN", "586660"),
            ("LIPSHITZ", "874400"),
            ("LIPPSZYC", "874500"),
            ("LEWINSKY", "816450"),
            ("LEVINSKI", "876450"),
            ("SZLAMAWICZ", "486740"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn spellings_that_should_agree_do() {
        let d = dm();
        assert_eq!(d.process("Ben Aron"), d.process("BENARON"));
        assert_eq!(d.process("PERES"), d.process("PERETZ"));
        assert_eq!(d.process("MOSKOWITZ"), d.process("MOSKOVITZ"));
        assert_eq!(d.process("AUERBACH"), d.process("OHRBACH"));
        assert_eq!(d.process("ZELENKO"), d.process("ZIELONKA"));
    }

    #[test]
    fn two_digit_codes_are_whole_numbers() {
        let d = dm();
        // MN and NM are coded separately (66), not merged into one 6.
        assert_eq!(d.process("MN"), "660000");
        assert_eq!(d.process("NM"), "660000");
        assert_eq!(d.process("RS"), "940000");
        assert_eq!(d.process("AXE"), "054000");
        assert_eq!(d.process("ASDA"), "043000");
    }

    #[test]
    fn multi_letter_clusters_win() {
        let d = dm();
        assert_eq!(d.process("SCHTSCH"), "200000");
        assert_eq!(d.process("ZDZ"), "200000");
        assert_eq!(d.process("SD"), "200000");
        assert_eq!(d.process("CZ"), "400000");
        assert_eq!(d.process("CK"), "500000");
    }

    #[test]
    fn edge_and_unicode_inputs() {
        let d = dm();
        assert_eq!(d.process(""), "000000");
        assert_eq!(d.process("a"), "000000");
        assert_eq!(d.process("X"), "500000");
        assert_eq!(d.process("Y"), "100000");
        assert_eq!(d.process("Q"), "500000");
        assert_eq!(d.process("JOHN"), "460000");
        assert_eq!(d.process("john"), "460000");
        assert_eq!(d.process("123"), "000000");
        assert_eq!(d.process("ç"), "000000");
        assert_eq!(d.process("é"), "000000");
        assert_eq!(d.process("😀"), "000000");
        assert_eq!(d.process("a-b"), "070000");
        // toUpperCase turns ß into SS before the walk begins.
        assert_eq!(d.process("ß"), "400000");
        assert_eq!(d.process(&"a".repeat(500)), "000000");
    }

    #[test]
    fn the_digit_zero_collides_with_the_legality_marker() {
        let d = dm();
        // `state['0']` finds the code triple, whose `[0]` is a number, whose
        // `[0]` is undefined — and `output += undefined` appends "undefined".
        assert_eq!(d.process("B0"), "undefi");
        assert_eq!(d.process("C0"), "undefi");
        assert_eq!(d.process("MOSKOWITZ0"), "6457un");
        // Safe only because A's start-of-word code is the falsy number 0.
        assert_eq!(d.process("A0"), "000000");
    }

    #[test]
    fn find_rules_walks_through_illegal_states() {
        let d = dm();
        assert_eq!(
            d.find_rules("SCHTSCH"),
            Rules {
                length: 7,
                mapping: RuleMapping::Triple([2, 4, 4])
            }
        );
        assert_eq!(
            d.find_rules(""),
            Rules {
                length: 1,
                mapping: RuleMapping::Triple([-1, -1, -1])
            }
        );
        assert_eq!(
            d.find_rules("B0"),
            Rules {
                length: 2,
                mapping: RuleMapping::Number(7)
            }
        );
        // Walking into the unused dual code via the array index `1`.
        assert_eq!(
            d.find_rules("CK1"),
            Rules {
                length: 3,
                mapping: RuleMapping::Number(45)
            }
        );
    }

    #[test]
    fn code_length_coercion() {
        let d = dm();
        assert_eq!(d.try_process("ALPERT", Some(10.0)).unwrap(), "0879300000");
        assert_eq!(d.try_process("ALPERT", Some(3.0)).unwrap(), "087");
        assert_eq!(d.try_process("ALPERT", Some(0.0)).unwrap(), "087930");
        assert_eq!(d.try_process("ALPERT", Some(f64::NAN)).unwrap(), "087930");
        assert_eq!(d.try_process("ALPERT", None).unwrap(), "087930");
        // slice(0, -1) drops the last character and pads nothing.
        assert_eq!(d.try_process("ALPERT", Some(-1.0)).unwrap(), "0879");
        assert_eq!(d.try_process("LONGWORDXYZ", Some(3.0)).unwrap(), "865");
    }

    #[test]
    fn fractional_code_length_throws_when_padding_is_needed() {
        let d = dm();
        assert_eq!(
            d.try_process("ALPERT", Some(6.5)),
            Err(PhoneticError::InvalidArrayLength(2.5))
        );
        // No padding needed, so `new Array` is never called.
        assert_eq!(d.normalize_length("1234567", Some(6.5)).unwrap(), "123456");
    }

    #[test]
    fn normalize_length_vectors() {
        let d = dm();
        assert_eq!(d.normalize_length("12", Some(6.0)).unwrap(), "120000");
        assert_eq!(d.normalize_length("1234567", Some(6.0)).unwrap(), "123456");
        assert_eq!(d.normalize_length("12", None).unwrap(), "120000");
        assert_eq!(d.normalize_length("12", Some(0.0)).unwrap(), "120000");
        assert_eq!(d.normalize_length("12", Some(-1.0)).unwrap(), "1");
    }

    #[test]
    fn compare_uses_the_default_length() {
        let d = dm();
        assert!(d.compare("PERES", "PERETZ"));
        assert!(!d.compare("ALPERT", "GOLDEN"));
    }
}
