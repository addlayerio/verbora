//! The reference string and regular-expression semantics that Rust does not share.
//!
//! Each item here marks a place where the obvious Rust equivalent silently
//! disagrees with the reference implementation. They are small; they are also
//! where most of a naive port's divergences come from.

use std::borrow::Cow;

pub use verbora_core::whitespace::is_whitespace;

/// The reference language `\s` class, spelled out for use inside `regex` patterns.
///
/// Rust's `\s` is Unicode `White_Space`, which **excludes** U+FEFF and
/// **includes** U+0085 — both differences from the reference language, and both reachable
/// from real text (a byte-order mark that survived a decode, and NEL in
/// EBCDIC-converted documents). Patterns that split or trim on `\s` must use
/// this instead.
pub const SPACE_CLASS: &str = r"[\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";

/// The negation of [`SPACE_CLASS`] — the reference language `\S`.
pub const NON_SPACE_CLASS: &str = r"[^\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";

/// Whether the UTF-16 code unit `u` is the reference language `\s`.
///
/// Every whitespace code point is in the BMP, so a code unit comparison is exact
/// — a surrogate half is never whitespace.
#[inline]
pub const fn is_whitespace_unit(u: u16) -> bool {
    matches!(u,
        0x0009..=0x000D
        | 0x0020
        | 0x00A0
        | 0x1680
        | 0x2000..=0x200A
        | 0x2028
        | 0x2029
        | 0x202F
        | 0x205F
        | 0x3000
        | 0xFEFF
    )
}

/// Whether `c` is a reference language *LineTerminator*, i.e. one of the four
/// characters a bare `.` refuses to match.
///
/// Rust's `.` excludes only `\n`, so a pattern translated character-for-character
/// would match `\r`, U+2028 and U+2029 where the reference does not.
#[inline]
pub const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Whether the UTF-16 code unit `u` is a reference language *LineTerminator*.
#[inline]
pub const fn is_line_terminator_unit(u: u16) -> bool {
    matches!(u, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// `String.prototype.trim` — strips the reference language whitespace from both ends.
///
/// Differs from [`str::trim`] on U+FEFF (stripped here, kept by Rust) and
/// U+0085 (kept here, stripped by Rust).
pub fn trim_units(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// The reference language `Canonicalize` for a non-Unicode-mode `/i` regular expression.
///
/// Case-insensitive matching in the reference is **not** Unicode case folding. Two
/// rules make it narrower, and both are observable:
///
/// * a character whose uppercase form is more than one code unit is left alone,
///   so `ß` never matches `SS`;
/// * a **non-ASCII** character whose uppercase form is ASCII is left alone, so
///   `ſ` (U+017F) does not match `s` and `K` (U+212A) does not match `k`.
///
/// Rust's `(?i)` applies full Unicode simple folding and would match all three.
pub fn canonicalize(c: char) -> char {
    let mut upper = c.to_uppercase();
    let Some(u) = upper.next() else { return c };
    // "If u does not consist of a single code unit, return ch."
    if upper.next().is_some() || u.len_utf16() != 1 {
        return c;
    }
    if !c.is_ascii() && u.is_ascii() { c } else { u }
}

/// Whether `haystack` starts, at `at`, with `needle` compared case-insensitively
/// under [`canonicalize`]. Returns the byte length consumed in `haystack`.
///
/// The two strings can consume different numbers of *bytes* for the same number
/// of characters (`ß` vs `ß` is trivial, but `İ` vs `i̇` is not), so the consumed
/// length is returned rather than assumed.
pub fn ci_match_at(haystack: &str, at: usize, needle: &str) -> Option<usize> {
    let rest = haystack.get(at..)?;
    let mut h = rest.chars();
    let mut consumed = 0;
    for n in needle.chars() {
        let c = h.next()?;
        if canonicalize(c) != canonicalize(n) {
            return None;
        }
        consumed += c.len_utf8();
    }
    Some(consumed)
}

/// The reference language `GetSubstitution` for a string replacement with **no** capture
/// groups.
///
/// `String.prototype.replace` interprets `$` sequences inside the replacement
/// *text*, not just inside a literal written by the programmer. The reference's
/// sentence tokenizer passes captured input as the replacement, so a URL
/// containing `$&` or ``$` `` genuinely rewrites the output — a quirk that must
/// be reproduced, not fixed.
///
/// Rust's `Regex::replace` understands `$1` and `${name}` but not `$&`,
/// ``$` `` or `$'`, and treats an unknown `$name` as an empty expansion rather
/// than as literal text. Hence this manual implementation.
///
/// * `matched` — the substring the pattern matched
/// * `position` — byte offset of `matched` within `subject`
/// * `subject` — the full string being searched
/// * `template` — the replacement text
pub fn get_substitution<'t>(
    matched: &str,
    position: usize,
    subject: &str,
    template: &'t str,
) -> Cow<'t, str> {
    if !template.contains('$') {
        return Cow::Borrowed(template);
    }
    let mut out = String::with_capacity(template.len());
    let mut it = template.char_indices();
    while let Some((i, c)) = it.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match template[i + 1..].chars().next() {
            Some('$') => {
                out.push('$');
                it.next();
            }
            Some('&') => {
                out.push_str(matched);
                it.next();
            }
            Some('`') => {
                out.push_str(&subject[..position]);
                it.next();
            }
            Some('\'') => {
                out.push_str(&subject[position + matched.len()..]);
                it.next();
            }
            // `$1`..`$99` and `$<name>` need capture groups to expand. With none,
            // the reference language leaves the whole sequence as literal text.
            _ => out.push('$'),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_does_not_fold_non_ascii_onto_ascii() {
        assert_eq!(canonicalize('a'), 'A');
        assert_eq!(canonicalize('A'), 'A');
        // ß uppercases to "SS" — two units — so it stays put.
        assert_eq!(canonicalize('ß'), 'ß');
        // The two characters Rust's `(?i)` would wrongly fold onto ASCII.
        assert_eq!(canonicalize('\u{17f}'), '\u{17f}');
        assert_eq!(canonicalize('\u{212a}'), '\u{212a}');
        assert_eq!(canonicalize('é'), 'É');
    }

    #[test]
    fn ci_match_is_case_insensitive_but_not_folding() {
        assert_eq!(ci_match_at("Dr. Smith", 0, "dr."), Some(3));
        assert_eq!(ci_match_at("DR. Smith", 0, "dr."), Some(3));
        assert_eq!(ci_match_at("xdr.", 1, "DR."), Some(3));
        assert_eq!(ci_match_at("ſmith", 0, "smith"), None);
        assert_eq!(ci_match_at("ab", 0, "abc"), None);
    }

    #[test]
    fn substitution_expands_the_four_dollar_forms() {
        // subject = "xxAyy", match "A" at 2
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$&]"), "[A]");
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$`]"), "[xx]");
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$']"), "[yy]");
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$$]"), "[$]");
        // No capture groups, so `$1` and `$<n>` stay literal.
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$1]"), "[$1]");
        assert_eq!(get_substitution("A", 2, "xxAyy", "[$<n>]"), "[$<n>]");
        assert_eq!(get_substitution("A", 2, "xxAyy", "plain"), "plain");
    }

    #[test]
    fn trim_units_differs_from_rust_trim_where_it_must() {
        assert_eq!(trim_units("\u{feff}a\u{feff}"), "a");
        assert_eq!(trim_units("\u{85}a\u{85}"), "\u{85}a\u{85}");
        assert_eq!(trim_units("  a b  "), "a b");
        assert_eq!(trim_units(""), "");
    }

    #[test]
    fn whitespace_unit_matches_the_char_predicate() {
        for u in 0u16..=0xffff {
            if let Some(c) = char::from_u32(u32::from(u)) {
                assert_eq!(is_whitespace_unit(u), is_whitespace(c), "U+{u:04X}");
            }
        }
    }
}
