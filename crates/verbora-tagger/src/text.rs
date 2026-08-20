//! The three token-shape tests Verbora's condition set defines.
//!
//! Each is stated here once, in full, because each is a *contract* rather than
//! an implementation detail: rules written against them must keep meaning the
//! same thing across versions.
//!
//! # The text unit
//!
//! This crate operates on whole tokens. Only two operations look inside one, and
//! both are defined on **Unicode scalar values**:
//!
//! * [`is_capitalized`] inspects the token's first scalar;
//! * the suffix test in
//!   [`Condition::CurrentWordEndsWith`](crate::Condition::CurrentWordEndsWith)
//!   is `str::ends_with`, a byte-level suffix match — which, because UTF-8 is
//!   self-synchronising, is exactly a scalar-sequence suffix match.
//!
//! Nothing here counts UTF-16 code units and nothing indexes a token by a
//! numeric position, so there is no unit for an astral scalar to be counted
//! twice in.

/// Whether a token is capitalised: its first Unicode scalar has the derived
/// `Uppercase` property.
///
/// The property is [UAX #44]'s `Uppercase` (`Lu` plus `Other_Uppercase`),
/// exposed by [`char::is_uppercase`]. Consequences worth stating, because a
/// looser or tighter test is easy to reach for by accident:
///
/// | Token | Capitalised | Why |
/// |---|---|---|
/// | `"Dog"` | yes | `D` is `Lu` |
/// | `"Ålesund"`, `"Москва"`, `"Ελλάς"` | yes | non-ASCII `Lu`; this is **not** an `A`–`Z` test |
/// | `"İstanbul"` | yes | U+0130 is `Lu` |
/// | `"ǅungla"` | no | U+01C5 is `Lt` (titlecase), which is not `Uppercase` |
/// | `"𝐀bc"` | yes | U+1D400 MATHEMATICAL BOLD CAPITAL A is `Lu` |
/// | `"5"`, `"."`, `"日本"`, `"😀"` | no | no case at all |
/// | `""` | no | there is no first scalar |
///
/// The empty token answers `false` rather than failing: a conforming token is
/// never empty (see [`crate::Lexicon`]), and a total predicate keeps the whole
/// tagging path infallible.
///
/// [UAX #44]: https://www.unicode.org/reports/tr44/#Uppercase
#[inline]
#[must_use]
pub(crate) fn is_capitalized(token: &str) -> bool {
    match token.as_bytes().first() {
        // ASCII fast path: one byte is one scalar, and `Uppercase` over ASCII is
        // exactly `A`–`Z`.
        Some(b) if b.is_ascii() => b.is_ascii_uppercase(),
        Some(_) => token.chars().next().is_some_and(char::is_uppercase),
        None => false,
    }
}

/// Whether a token is a Verbora **numeral**.
///
/// Verbora defines the grammar rather than delegating to a language runtime's
/// string-to-number coercion, so that the answer does not depend on anything
/// outside this crate:
///
/// ```text
/// numeral  = sign? mantissa exponent?
/// sign     = "+" | "-"
/// mantissa = grouped ( "." digits? )?  |  "." digits
/// grouped  = digits | digit{1,3} ( "," digit{3} )+
/// digits   = digit+
/// digit    = "0".."9"                       (ASCII only, U+0030..U+0039)
/// exponent = ("e" | "E") sign? digits
/// ```
///
/// The whole token must match. Worked examples:
///
/// | Token | Numeral | Note |
/// |---|---|---|
/// | `"5"`, `"-2"`, `"+5"`, `"3.14"`, `".5"`, `"5."` | yes | |
/// | `"1e5"`, `"1E-5"` | yes | complete exponent |
/// | `"1,000"`, `"12,345,678"` | yes | groups of exactly three after the first |
/// | `""` | **no** | an empty token is not a numeral |
/// | `"1,0000"`, `"10,00"`, `",100"` | no | malformed grouping |
/// | `"5abc"`, `"1_000"`, `"1e"`, `"."`, `"-"` | no | trailing or incomplete text |
/// | `"0x10"`, `"Infinity"`, `"NaN"` | no | not decimal literals |
/// | `"٣"`, `"１２３"` | no | ASCII digits only, by definition above |
///
/// The four rows that most often differ from a runtime coercion are `""`
/// (which several languages call zero), `"5abc"` (which a prefix parser accepts),
/// `"0x10"` and `"Infinity"`.
#[must_use]
pub(crate) fn is_numeral(token: &str) -> bool {
    let b = token.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_len = grouped_digits(b, &mut i);
    let mut frac_digits = 0;
    let has_point = b.get(i) == Some(&b'.');
    if has_point {
        i += 1;
        frac_digits = plain_digits(b, &mut i);
    }
    if int_len == 0 && frac_digits == 0 {
        // Neither "123" nor ".5": a bare sign, point or empty string.
        return false;
    }
    if int_len == 0 && !has_point {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if plain_digits(b, &mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// Consumes `digit+`, returning how many were consumed.
fn plain_digits(b: &[u8], i: &mut usize) -> usize {
    let start = *i;
    while matches!(b.get(*i), Some(d) if d.is_ascii_digit()) {
        *i += 1;
    }
    *i - start
}

/// Consumes `digits` or `digit{1,3} ("," digit{3})+`, returning the byte length
/// consumed (0 when there is no integer part).
fn grouped_digits(b: &[u8], i: &mut usize) -> usize {
    let start = *i;
    let first = plain_digits(b, i);
    if first == 0 {
        return 0;
    }
    if b.get(*i) != Some(&b',') {
        return *i - start;
    }
    // A grouped numeral: the first group is 1..=3 digits, every later group is
    // exactly 3. Anything else is not a numeral at all, so back the cursor out.
    if first > 3 {
        *i = start + first;
        return first;
    }
    let mut cursor = *i;
    while b.get(cursor) == Some(&b',') {
        let mut j = cursor + 1;
        if plain_digits(b, &mut j) != 3 {
            // Malformed group: keep only the digits already consumed, which then
            // leaves a `,` in the token and fails the whole-token check.
            *i = start + first;
            return first;
        }
        cursor = j;
    }
    *i = cursor;
    cursor - start
}

/// Whether a token *looks like* a URL under Verbora's deliberately small
/// heuristic.
///
/// The token must contain a `U+002E FULL STOP` that is neither its first nor its
/// last scalar, **and** two consecutive ASCII letters somewhere. That is the
/// whole rule; it is a Verbora heuristic, not an implementation of [RFC 3986],
/// and it is stated here so that the bundled `NN URL CURRENT-WORD-IS-URL YES`
/// rule has a definition to point at.
///
/// | Token | URL-like | Why |
/// |---|---|---|
/// | `"www.example.com"`, `"example.org"` | yes | interior dot, `ww`/`ex` |
/// | `"3.14"` | no | no two adjacent ASCII letters |
/// | `"e.g."` | no | trailing dot, and no adjacent letters |
/// | `"A.A.U."` | no | no two adjacent letters |
/// | `".com"`, `"com."` | no | the dot is not interior |
/// | `"日本.語"` | no | the letters are not ASCII |
///
/// [RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986
#[must_use]
pub(crate) fn looks_like_url(token: &str) -> bool {
    let interior_dot = match (token.find('.'), token.rfind('.')) {
        (Some(first), Some(last)) => first > 0 && last + 1 < token.len(),
        _ => false,
    };
    interior_dot
        && token
            .as_bytes()
            .windows(2)
            .any(|w| w[0].is_ascii_alphabetic() && w[1].is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalisation_is_the_unicode_uppercase_property() {
        for yes in [
            "Dog",
            "Ålesund",
            "Москва",
            "Ελλάς",
            "ΟΔΟΣ",
            "İstanbul",
            "Z",
            "𝐀bc",
        ] {
            assert!(is_capitalized(yes), "{yes:?}");
        }
        for no in [
            "dog",
            "ålesund",
            "москва",
            "ǅungla",
            "5",
            ".",
            "日本",
            "😀",
            "",
            "ß",
        ] {
            assert!(!is_capitalized(no), "{no:?}");
        }
    }

    /// An astral scalar is read whole, never as a lone surrogate.
    ///
    /// U+1D400 MATHEMATICAL BOLD CAPITAL A has `General_Category=Lu`, so it *is*
    /// capitalised — while U+1F600 GRINNING FACE has no case at all and is not.
    /// A UTF-16-indexed implementation reads the high surrogate U+D83D for the
    /// second of those and would answer differently.
    #[test]
    fn astral_scalars_are_read_whole() {
        assert!('\u{1d400}'.is_uppercase(), "U+1D400 is Lu");
        assert!(is_capitalized("𝐀bc"));
        assert!(!is_capitalized("😀"));
        assert!(is_capitalized("Ǆungla"), "U+01C4 is Lu");
    }

    #[test]
    fn numeral_grammar() {
        for yes in [
            "5",
            "-2",
            "+5",
            "0",
            "3.14",
            ".5",
            "5.",
            "-0.0",
            "1e5",
            "1E-5",
            "1e+5",
            "1,000",
            "12,345,678",
            "999",
            "1.5e10",
        ] {
            assert!(is_numeral(yes), "{yes:?} should be a numeral");
        }
        for no in [
            "",
            " ",
            ".",
            "-",
            "+",
            "e5",
            "1e",
            "1e+",
            "5abc",
            "0abc",
            "1_000",
            "0x10",
            "0b101",
            "Infinity",
            "-Infinity",
            "NaN",
            "abc",
            "٣",
            "１２３",
            "1,0000",
            "10,00",
            ",100",
            "1,",
            "1..2",
            "--5",
        ] {
            assert!(!is_numeral(no), "{no:?} should not be a numeral");
        }
    }

    #[test]
    fn url_heuristic() {
        for yes in ["www.example.com", "example.org", "a.bc.d", "naïve.com"] {
            assert!(looks_like_url(yes), "{yes:?}");
        }
        for no in [
            "3.14",
            "e.g.",
            "A.A.U.",
            ".com",
            "com.",
            "日本.語",
            "",
            "abc",
            "a.b",
            "1.2",
        ] {
            assert!(!looks_like_url(no), "{no:?}");
        }
    }
}
