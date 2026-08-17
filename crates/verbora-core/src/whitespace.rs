//! The reference string semantics that differ from Rust's defaults.
//!
//! Small helpers, but load-bearing: each one marks a place where the obvious
//! Rust equivalent silently disagrees with the reference implementation.

/// Whether `c` is matched by the reference's regular-expression `\s` class.
///
/// # Why not `char::is_whitespace`
///
/// The two sets are close but **not** equal, and both differences are reachable
/// from ordinary text:
///
/// | Character | the reference `\s` | Rust `is_whitespace` |
/// |-----------|---------|----------------------|
/// | `U+0085` NEXT LINE          | no  | **yes** |
/// | `U+FEFF` ZERO WIDTH NBSP    | **yes** | no  |
///
/// The reference language defines `\s` as `WhiteSpace | LineTerminator`, where `WhiteSpace`
/// is TAB, VT, FF, SP, NBSP, ZWNBSP and the `Zs` category, and `LineTerminator`
/// is LF, CR, LS and PS. `U+0085` is category `Cc`, so the reference does not match
/// it; Rust's `White_Space` property does. `U+FEFF` is the mirror image.
///
/// Any port of a reference routine that splits or trims on `\s` must use this
/// function, or it will tokenise text containing either character differently.
/// The exact set, for reference:
///
/// | Range / code point | Name |
/// |---|---|
/// | `U+0009`–`U+000D` | TAB, LF, VT, FF, CR |
/// | `U+0020` | SPACE |
/// | `U+00A0` | NO-BREAK SPACE |
/// | `U+1680` | OGHAM SPACE MARK |
/// | `U+2000`–`U+200A` | EN QUAD … HAIR SPACE |
/// | `U+2028`, `U+2029` | LINE / PARAGRAPH SEPARATOR |
/// | `U+202F` | NARROW NO-BREAK SPACE |
/// | `U+205F` | MEDIUM MATHEMATICAL SPACE |
/// | `U+3000` | IDEOGRAPHIC SPACE |
/// | `U+FEFF` | ZERO WIDTH NO-BREAK SPACE |
#[inline]
pub const fn is_whitespace(c: char) -> bool {
    matches!(c,
        '\u{0009}'..='\u{000D}'
        | '\u{0020}'
        | '\u{00A0}'
        | '\u{1680}'
        | '\u{2000}'..='\u{200A}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{202F}'
        | '\u{205F}'
        | '\u{3000}'
        | '\u{FEFF}'
    )
}

/// Collapses every run of the reference whitespace to a single space and trims the
/// ends — the `replace(/\s+/g, ' ').replace(/^\s+|\s+$/g, '')` idiom.
///
/// Returns the input unchanged (borrowed, no allocation) when it already has no
/// whitespace to normalise, which is the common case for single words.
pub fn collapse_whitespace(s: &str) -> std::borrow::Cow<'_, str> {
    let needs_work = s.chars().any(is_whitespace);
    if !needs_work {
        return std::borrow::Cow::Borrowed(s);
    }

    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if is_whitespace(c) {
            in_ws = true;
        } else {
            if in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = false;
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differs_from_rust_whitespace_where_it_must() {
        // NEL: Rust says whitespace, the reference does not.
        assert!('\u{0085}'.is_whitespace());
        assert!(!is_whitespace('\u{0085}'));

        // ZWNBSP: the reference says whitespace, Rust does not.
        assert!(!'\u{FEFF}'.is_whitespace());
        assert!(is_whitespace('\u{FEFF}'));
    }

    #[test]
    fn covers_the_common_cases() {
        for c in [
            ' ', '\t', '\n', '\r', '\u{000B}', '\u{000C}', '\u{00A0}', '\u{3000}',
        ] {
            assert!(is_whitespace(c), "{c:?}");
        }
        for c in ['a', '0', '-', '\u{200B}' /* ZWSP is not \s */] {
            assert!(!is_whitespace(c), "{c:?}");
        }
    }

    #[test]
    fn collapse_matches_the_reference_idiom() {
        assert_eq!(collapse_whitespace("  a   b  "), "a b");
        assert_eq!(collapse_whitespace("abc"), "abc");
        assert_eq!(collapse_whitespace(""), "");
        assert_eq!(collapse_whitespace("   "), "");
        assert_eq!(collapse_whitespace("a\t\n b"), "a b");
    }

    #[test]
    fn collapse_borrows_when_there_is_nothing_to_do() {
        assert!(matches!(
            collapse_whitespace("nowhitespace"),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
