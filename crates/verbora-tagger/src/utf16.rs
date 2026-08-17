//! UTF-16 string semantics, with an ASCII fast path.
//!
//! Three Brill predicates index strings the way the reference does — by UTF-16 code
//! unit — and a byte-indexed port silently disagrees on any non-ASCII token:
//!
//! * `currentWordEndsWith` compares `word.indexOf(p)` against
//!   `word.length - p.length`, so both must be code-unit counts;
//! * `currentWordIsCap` / `nextWordIsCap` / `prevWordIsCap` read `word[0]`,
//!   which for an astral character is a **lone high surrogate**;
//! * `Lexicon.tagWordWithDefaults` tests `/[A-Z]/` against that same `word[0]`.
//!
//! For ASCII input one byte *is* one code unit, so the fast paths here are exact
//! rather than approximate — the same dispatch `verbora_distance::units` uses.

/// Number of UTF-16 code units in `s` — the reference's `String.prototype.length`.
///
/// Astral characters count as two.
#[inline]
#[must_use]
pub fn len(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().map(char::len_utf16).sum()
    }
}

/// `haystack.indexOf(needle)`, in UTF-16 code units, or `None` when absent.
///
/// Both arguments are well-formed UTF-8, and UTF-8 is self-synchronising, so the
/// first byte-aligned match is also the first code-unit-aligned match: the two
/// search orders cannot disagree. Only the *reported index* needs converting.
#[inline]
#[must_use]
pub fn index_of(haystack: &str, needle: &str) -> Option<usize> {
    let at = haystack.find(needle)?;
    Some(if haystack.is_ascii() {
        at
    } else {
        len(&haystack[..at])
    })
}

/// `word[0] === word[0].toUpperCase()` — the reference's capitalisation test.
///
/// # What this is not
///
/// It is not "starts with an uppercase letter". Any character whose uppercase
/// mapping is itself passes, so digits and punctuation are "capitalised":
/// `'5'` and `'.'` both return `true`. Characters with a multi-character
/// uppercase mapping fail (`'ß'` → `"SS"`, `'ﬁ'` → `"FI"`), as do titlecase
/// letters (`'ǅ'` U+01C5 uppercases to U+01C4).
///
/// For a character outside the BMP, the reference's `word[0]` is the lone high
/// surrogate, whose `toUpperCase` is itself — so `"𝐀bc"` and `"😀x"` are both
/// reported as capitalised. Rust sees the whole scalar value instead, but
/// `char::to_uppercase` is also the identity on those, so the answers agree.
///
/// # Errors
///
/// Returns `None` for the empty string, where the reference throws
/// `Cannot read properties of undefined (reading 'toUpperCase')`.
#[inline]
#[must_use]
pub fn first_char_is_own_uppercase(s: &str) -> Option<bool> {
    let c = s.chars().next()?;
    if c.is_ascii() {
        // Fast path: an ASCII character uppercases to itself unless it is a-z.
        return Some(!c.is_ascii_lowercase());
    }
    let mut up = c.to_uppercase();
    Some(up.next() == Some(c) && up.next().is_none())
}

/// `/[A-Z]/.test(word[0])` — ASCII-only, against the first UTF-16 code unit.
///
/// The empty string is `false`: `''[0]` is `undefined`, which the regex coerces
/// to the *string* `"undefined"`, which contains no `A`–`Z`. Accented capitals
/// (`'Å'`) and non-Latin capitals (`'Μ'`) are `false` too — this really is the
/// ASCII range and nothing more.
#[inline]
#[must_use]
pub fn first_is_ascii_upper(s: &str) -> bool {
    matches!(s.as_bytes().first(), Some(b) if b.is_ascii_uppercase())
}

/// Whether `token` contains two consecutive ASCII letters.
///
/// This is `/[a-zA-Z]{2}/.test(token)`: unanchored, ASCII-only, and with no
/// relationship to the `.` that `currentWordIsURL` also requires. Widening it to
/// `char::is_alphabetic` would make `"日本.語"` a URL, which it is not in
/// the reference.
#[inline]
#[must_use]
pub fn has_two_adjacent_ascii_letters(token: &str) -> bool {
    token
        .as_bytes()
        .windows(2)
        .any(|w| w[0].is_ascii_alphabetic() && w[1].is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_counts_code_units() {
        assert_eq!(len(""), 0);
        assert_eq!(len("abc"), 3);
        assert_eq!(len("café"), 4);
        assert_eq!(len("😀"), 2);
        assert_eq!(len("a😀b"), 4);
        assert_eq!(len("𝕳𝖊𝖑𝖑𝖔"), 10);
        assert_eq!(len("日本語"), 3);
        assert_eq!(len("Москва"), 6);
    }

    #[test]
    fn index_of_reports_code_unit_positions() {
        assert_eq!(index_of("cats", "s"), Some(3));
        assert_eq!(index_of("sees", "s"), Some(0));
        assert_eq!(index_of("a😀b", "b"), Some(3));
        assert_eq!(index_of("😀s", "s"), Some(2));
        assert_eq!(index_of("abc", "z"), None);
        assert_eq!(index_of("abc", ""), Some(0));
    }

    #[test]
    fn capitalisation_matches_the_reference() {
        assert_eq!(first_char_is_own_uppercase("Abc"), Some(true));
        assert_eq!(first_char_is_own_uppercase("abc"), Some(false));
        assert_eq!(first_char_is_own_uppercase("5"), Some(true));
        assert_eq!(first_char_is_own_uppercase("."), Some(true));
        assert_eq!(first_char_is_own_uppercase("ß"), Some(false));
        assert_eq!(first_char_is_own_uppercase("Ålesund"), Some(true));
        assert_eq!(first_char_is_own_uppercase("ǅungla"), Some(false));
        assert_eq!(first_char_is_own_uppercase("İ"), Some(true));
        assert_eq!(first_char_is_own_uppercase("ﬁle"), Some(false));
        assert_eq!(first_char_is_own_uppercase("𝐀bc"), Some(true));
        assert_eq!(first_char_is_own_uppercase("😀x"), Some(true));
        assert_eq!(first_char_is_own_uppercase("日本"), Some(true));
        assert_eq!(first_char_is_own_uppercase("Москва"), Some(true));
        assert_eq!(first_char_is_own_uppercase("москва"), Some(false));
        assert_eq!(first_char_is_own_uppercase(""), None);
    }

    #[test]
    fn ascii_upper_is_ascii_only() {
        assert!(first_is_ascii_upper("Abc"));
        assert!(!first_is_ascii_upper("abc"));
        assert!(!first_is_ascii_upper("Ålesund"));
        assert!(!first_is_ascii_upper("Москва"));
        assert!(!first_is_ascii_upper(""));
        assert!(!first_is_ascii_upper("😀"));
    }

    #[test]
    fn two_letter_scan() {
        assert!(has_two_adjacent_ascii_letters("www.example.com"));
        assert!(has_two_adjacent_ascii_letters("ab."));
        assert!(has_two_adjacent_ascii_letters(".ab"));
        assert!(has_two_adjacent_ascii_letters("AB."));
        assert!(has_two_adjacent_ascii_letters("naïve.com"));
        assert!(!has_two_adjacent_ascii_letters("a.b"));
        assert!(!has_two_adjacent_ascii_letters("e.g"));
        assert!(!has_two_adjacent_ascii_letters("1.2"));
        assert!(!has_two_adjacent_ascii_letters("日本語"));
        assert!(!has_two_adjacent_ascii_letters(""));
    }
}
