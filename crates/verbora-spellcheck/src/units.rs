//! Text-unit dispatch: the reference's UTF-16 semantics without paying for them on
//! ordinary input.
//!
//! # Why the edit generator cannot use `char`
//!
//! `Spellcheck.edits` slices with `word.slice(i, j)` and counts with
//! `word.length`, both of which are UTF-16 **code unit** operations. Editing a
//! word that contains an astral character therefore cuts *between the halves of
//! a surrogate pair*:
//!
//! ```text
//! edits("a😀b")   reference units : 238 candidates, 83 of them containing a lone
//!                                    surrogate (e.g. "a" + U+DE00 + "b")
//!                 Rust chars      : 186 candidates, none of them
//! ```
//!
//! Those malformed candidates are never dictionary words, so they cannot
//! *themselves* be corrections — but at `maxDistance >= 2` they are edited
//! again, and a second edit can repair them into perfectly ordinary words. Drop
//! them and the distance-2 candidate set is wrong. The generator must therefore
//! run over `u16`, not `char`.
//!
//! # Why it does not *always* run over `u16`
//!
//! For an ASCII word, one byte is one code unit, and every edit of an ASCII word
//! is ASCII again (the insert/replace alphabet is `a`–`z`). The `u8`
//! representation is then not an approximation but the same computation on a
//! narrower type — and it lets the dictionary lookup borrow a `&str` straight
//! out of the candidate buffer instead of decoding into a scratch `String`.
//!
//! Since ASCII is closed under the operation, the choice is made once, at the
//! entry point, and holds for every level of the search. See
//! [`Spellcheck::get_corrections`](crate::Spellcheck::get_corrections).

use std::hash::Hash;

mod sealed {
    /// Prevents outside implementations, so the two representations stay
    /// provably equivalent to UTF-16.
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
}

/// A unit the edit generator can operate on: an ASCII byte or a UTF-16 code
/// unit.
///
/// The trait is sealed. Both implementations index identically to the reference
/// for the inputs they are chosen for, which is the only property the rest of
/// the crate relies on.
pub trait EditUnit: Copy + Eq + Hash + 'static + sealed::Sealed {
    /// Widens an ASCII byte (always one of `a`–`z` here) into this unit type.
    fn from_ascii(byte: u8) -> Self;

    /// Views `units` as a `&str`, using `buf` as scratch only when it must.
    ///
    /// Returns `None` when `units` is not well-formed text — which for `u16`
    /// means it contains an unpaired surrogate. Such a candidate can never be a
    /// dictionary word, because every word in the list came from a `&str`.
    fn as_str<'b>(units: &'b [Self], buf: &'b mut String) -> Option<&'b str>;

    /// Renders `units` as a `String`, replacing anything malformed with
    /// `U+FFFD`.
    ///
    /// Used only by the convenience APIs that hand back `String`s; the parity
    /// path goes through [`EditUnit::as_str`], which never guesses.
    fn to_lossy_string(units: &[Self]) -> String;
}

impl EditUnit for u8 {
    #[inline]
    fn from_ascii(byte: u8) -> Self {
        byte
    }

    #[inline]
    fn as_str<'b>(units: &'b [Self], _buf: &'b mut String) -> Option<&'b str> {
        // Every value here originates from an ASCII `&str` or the `a`–`z`
        // alphabet, so this validation is a vectorised no-op in practice — and
        // it borrows, so the hot path allocates nothing at all.
        std::str::from_utf8(units).ok()
    }

    fn to_lossy_string(units: &[Self]) -> String {
        String::from_utf8_lossy(units).into_owned()
    }
}

impl EditUnit for u16 {
    #[inline]
    fn from_ascii(byte: u8) -> Self {
        u16::from(byte)
    }

    fn as_str<'b>(units: &'b [Self], buf: &'b mut String) -> Option<&'b str> {
        buf.clear();
        for decoded in char::decode_utf16(units.iter().copied()) {
            buf.push(decoded.ok()?);
        }
        Some(buf)
    }

    fn to_lossy_string(units: &[Self]) -> String {
        char::decode_utf16(units.iter().copied())
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }
}

/// Encodes `word` as UTF-16 code units.
pub(crate) fn to_utf16(word: &str) -> Vec<u16> {
    word.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_bytes_borrow_rather_than_copy() {
        let mut buf = String::new();
        let units = b"cat";
        let s = u8::as_str(units, &mut buf).expect("ASCII is always valid");
        assert_eq!(s, "cat");
        // The `&str` points into the candidate buffer, so the scratch is
        // untouched — that is the whole point of the `u8` path.
        assert!(buf.is_empty());
        assert_eq!(u8::as_str(b"", &mut buf), Some(""));
    }

    #[test]
    fn utf16_decodes_every_plane() {
        let mut buf = String::new();
        for word in [
            "",
            "a",
            "A",
            "café",
            "Москва",
            "Ελλάδα",
            "日本語",
            "a😀b",
            "…!?",
            "123",
        ] {
            let units = to_utf16(word);
            assert_eq!(u16::as_str(&units, &mut buf), Some(word), "{word:?}");
            assert_eq!(u16::to_lossy_string(&units), word, "{word:?}");
        }
    }

    /// An edit that cuts a surrogate pair in half is not text. It must be
    /// rejected by `as_str` — a candidate that is not text can never be a
    /// dictionary word — but still *survive* as units, because a second edit can
    /// repair it.
    #[test]
    fn lone_surrogates_are_rejected_not_guessed() {
        let mut buf = String::new();
        // "a😀b" with the high surrogate deleted.
        let broken: Vec<u16> = vec![u16::from(b'a'), 0xde00, u16::from(b'b')];
        assert_eq!(u16::as_str(&broken, &mut buf), None);
        assert_eq!(u16::to_lossy_string(&broken), "a\u{fffd}b");

        // A trailing high surrogate is equally broken.
        let dangling: Vec<u16> = vec![u16::from(b'a'), 0xd83d];
        assert_eq!(u16::as_str(&dangling, &mut buf), None);
        assert_eq!(u16::to_lossy_string(&dangling), "a\u{fffd}");
    }

    #[test]
    fn from_ascii_widens_the_alphabet_identically() {
        for byte in b'a'..=b'z' {
            // Fully qualified: `from_ascii` is a name the standard library may
            // yet claim on the integer types.
            assert_eq!(<u8 as EditUnit>::from_ascii(byte), byte);
            assert_eq!(<u16 as EditUnit>::from_ascii(byte), u16::from(byte));
        }
    }

    #[test]
    fn very_long_input_round_trips() {
        let long = "é".repeat(5000);
        let units = to_utf16(&long);
        assert_eq!(units.len(), 5000);
        let mut buf = String::new();
        assert_eq!(u16::as_str(&units, &mut buf), Some(long.as_str()));
    }
}
