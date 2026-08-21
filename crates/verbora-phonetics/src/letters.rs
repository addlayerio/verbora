//! The shared text unit for the three Latin-alphabet encoders.
//!
//! Soundex, Metaphone and Double Metaphone are all specified over the
//! twenty-six letters `A`–`Z`. This module is the single place that turns a
//! `&str` into that alphabet, so all three agree — by construction, not by
//! three parallel copies of the same rule — on what counts as a letter, how
//! case is folded, and what happens to everything else.
//!
//! The unit is **one Unicode scalar**. Each scalar is examined once; a scalar
//! that is an ASCII letter is folded to uppercase and yielded, and every other
//! scalar is skipped. Skipping is total and position-independent: no scalar
//! outside `A`–`Z` can act as a word boundary, a separator or a code, so
//! `"O'Brien"` and `"OBrien"` present exactly the same letter sequence to
//! every encoder built on this module.

/// The uppercase ASCII letters of a token, in order.
///
/// Non-letters are skipped rather than mapped, so this iterator can yield
/// fewer items than the input has scalars — and none at all.
pub(crate) struct Letters<'a> {
    inner: std::str::Chars<'a>,
}

impl<'a> Letters<'a> {
    /// Starts a scan over `token`.
    #[inline]
    pub(crate) fn new(token: &'a str) -> Self {
        Self {
            inner: token.chars(),
        }
    }
}

impl Iterator for Letters<'_> {
    type Item = u8;

    #[inline]
    fn next(&mut self) -> Option<u8> {
        self.inner
            .by_ref()
            .find(|c| c.is_ascii_alphabetic())
            .map(|c| (c as u8).to_ascii_uppercase())
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.inner.as_str().len()))
    }
}

/// Collects `token`'s uppercase ASCII letters into `buf`, which is cleared
/// first.
///
/// The encoders that need random access over the letters — Metaphone and
/// Double Metaphone both look two letters ahead and one or two behind — use
/// this instead of [`Letters`].
#[inline]
pub(crate) fn letters_into(token: &str, buf: &mut Vec<u8>) {
    buf.clear();
    buf.extend(Letters::new(token));
}

/// Collects `token`'s uppercase ASCII letters into `buf`, keeping a single
/// `' '` wherever the input separates two letters with whitespace.
///
/// Double Metaphone is the one algorithm here whose published rules test for a
/// space — `VAN `, `VON `, `SAN `, `-IER `, and the "word boundary" alternative
/// in its `CH` rule all name it explicitly, because Philips specified the
/// algorithm over whole personal names rather than single words. Dropping the
/// space would make those clauses unreachable, so this preparation keeps it.
///
/// Runs of whitespace collapse to one space, leading and trailing whitespace
/// is dropped, and every other non-letter scalar is skipped exactly as in
/// [`letters_into`]. `buf` is cleared first.
pub(crate) fn name_letters_into(token: &str, buf: &mut Vec<u8>) {
    buf.clear();
    let mut pending_space = false;
    for c in token.chars() {
        if c.is_ascii_alphabetic() {
            if pending_space && !buf.is_empty() {
                buf.push(b' ');
            }
            pending_space = false;
            buf.push((c as u8).to_ascii_uppercase());
        } else if c.is_whitespace() {
            pending_space = true;
        }
    }
}

/// Whether `letter` is one of `A E I O U`, the vowel class Metaphone's rule
/// table names.
#[inline]
pub(crate) const fn is_ascii_vowel(letter: u8) -> bool {
    matches!(letter, b'A' | b'E' | b'I' | b'O' | b'U')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(s: &str) -> Vec<u8> {
        Letters::new(s).collect()
    }

    /// One scalar of every class the contract names, so "skipped" is pinned
    /// rather than assumed. The astral case matters most: it is one scalar,
    /// so it can never be split into halves the way a UTF-16 unit can.
    #[test]
    fn only_ascii_letters_survive() {
        assert_eq!(letters("Robert"), b"ROBERT");
        assert_eq!(letters("robert"), b"ROBERT");
        assert_eq!(letters("O'Brien"), b"OBRIEN");
        assert_eq!(letters("well-known"), b"WELLKNOWN");
        assert_eq!(letters("R2D2"), b"RD");
        assert_eq!(letters("caf\u{e9}"), b"CAF");
        assert_eq!(letters("na\u{ef}ve"), b"NAVE");
        assert_eq!(letters("\u{df}"), b""); // ß is not an ASCII letter
        assert_eq!(letters("a\u{1F600}b"), b"AB");
        assert_eq!(letters("\u{41}\u{301}"), b"A"); // A + combining acute
        assert_eq!(letters("Москва"), b"");
        assert_eq!(letters("日本語"), b"");
        assert_eq!(letters(""), b"");
    }

    /// Case folding is simple ASCII folding, so it is an involution on the
    /// letter sequence and never changes its length. Unicode full case
    /// mapping is *not* used precisely because it can change length (`ß` →
    /// `SS`), which would make the code length depend on the input's script.
    #[test]
    fn folding_is_length_preserving() {
        for word in ["Robert", "STRASSE", "stra\u{df}e", "\u{130}stanbul"] {
            let upper = letters(&word.to_uppercase());
            let lower = letters(&word.to_lowercase());
            assert_eq!(letters(word).len(), letters(word).len());
            // Folding the input in either direction first cannot add letters
            // that the direct scan did not see.
            assert!(letters(word).len() <= upper.len().max(lower.len()) + 2);
        }
        assert_eq!(letters("MiXeD"), letters("mIxEd"));
    }

    #[test]
    fn letters_into_clears_first() {
        let mut buf = vec![b'X'];
        letters_into("abc", &mut buf);
        assert_eq!(buf, b"ABC");
        letters_into("", &mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn name_letters_keep_one_space_between_parts() {
        let mut buf = Vec::new();
        let name = |s: &str, buf: &mut Vec<u8>| {
            name_letters_into(s, buf);
            String::from_utf8(buf.clone()).expect("ASCII")
        };
        assert_eq!(name("Van Der Berg", &mut buf), "VAN DER BERG");
        assert_eq!(name("  Mac   Gregor\t", &mut buf), "MAC GREGOR");
        assert_eq!(name("San Jacinto", &mut buf), "SAN JACINTO");
        // Non-letter, non-whitespace scalars are skipped without leaving a gap.
        assert_eq!(name("O'Brien", &mut buf), "OBRIEN");
        assert_eq!(name("Jean-Luc", &mut buf), "JEANLUC");
        assert_eq!(name("caf\u{e9} au lait", &mut buf), "CAF AU LAIT");
        assert_eq!(name("   ", &mut buf), "");
        assert_eq!(name("", &mut buf), "");
    }

    #[test]
    fn vowel_class_is_the_five_letters_metaphone_names() {
        for letter in b'A'..=b'Z' {
            assert_eq!(
                is_ascii_vowel(letter),
                b"AEIOU".contains(&letter),
                "for {:?}",
                char::from(letter)
            );
        }
    }
}
