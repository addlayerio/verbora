//! The shared scanner behind the character-class tokenizers.
//!
//! Thirteen of the reference's sixteen "aggressive" tokenizers reduce to the same
//! operation once their pipelines are unfolded: **emit every maximal non-empty
//! run of word characters**. They get there by different routes —
//! `split(/[^…]+/)` then `trim`, or `replace(/[^…]/g,' ')` then collapse-split
//! then filter, or (Portuguese) a `+`-less split whose interior empties a second
//! filter removes — but all three compose to the same output, which is why one
//! scanner can serve them all. See each tokenizer's documentation for the
//! equivalence argument that licenses it.
//!
//! The scanner is generic over a zero-sized [`CharClass`] rather than taking a
//! function pointer, so each tokenizer's predicate inlines into its own loop and
//! the ASCII fast path stays branch-predictable.

use std::borrow::Cow;

/// A word-character predicate, supplied as a type so it monomorphises away.
///
/// Every implementation delegates to a `const fn` built from the ranges in
/// [`crate::classes`]. A hand-rolled `[bool; 128]` or `u128` bitmask lookup
/// for the ASCII half — which the cluster's performance notes suggest — was
/// implemented and measured: it came out **12% slower** (11.2 µs against 9.8 µs
/// on a 9.7 kB document), because `rustc` already compiles a `matches!` over
/// character ranges into a range check plus a bit test, and the explicit mask
/// only adds a 128-bit shift. The straightforward code is both faster and
/// simpler here, which is why it is what remains.
pub trait CharClass {
    /// Whether `c` belongs to a token.
    fn is_word(c: char) -> bool;
}

/// Finds the next maximal run of word characters at or after `pos`.
///
/// Returns the run's byte range, or `None` when the input is exhausted.
#[inline]
fn next_run<K: CharClass>(text: &str, mut pos: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Skip separators. ASCII is handled without decoding, which is the whole
    // input for most Latin-script text.
    while pos < len {
        let b = bytes[pos];
        if b < 0x80 {
            if K::is_word(b as char) {
                break;
            }
            pos += 1;
        } else {
            let c = char_at(text, pos);
            if K::is_word(c) {
                break;
            }
            pos += c.len_utf8();
        }
    }
    if pos >= len {
        return None;
    }

    let start = pos;
    while pos < len {
        let b = bytes[pos];
        if b < 0x80 {
            if !K::is_word(b as char) {
                break;
            }
            pos += 1;
        } else {
            let c = char_at(text, pos);
            if !K::is_word(c) {
                break;
            }
            pos += c.len_utf8();
        }
    }
    Some((start, pos))
}

/// The character beginning at byte offset `pos`, which must be a boundary.
#[inline]
fn char_at(text: &str, pos: usize) -> char {
    // Safe indexing: callers only ever pass boundaries they walked to.
    text[pos..].chars().next().unwrap_or('\u{fffd}')
}

/// Lazily yields every maximal run of `K` characters as a borrowed slice.
#[derive(Debug, Clone)]
pub struct WordRuns<'a, K> {
    text: &'a str,
    pos: usize,
    class: std::marker::PhantomData<K>,
}

impl<'a, K: CharClass> WordRuns<'a, K> {
    /// Starts a scan over `text`.
    #[inline]
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            pos: 0,
            class: std::marker::PhantomData,
        }
    }
}

impl<'a, K: CharClass> Iterator for WordRuns<'a, K> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        let (a, b) = next_run::<K>(self.text, self.pos)?;
        self.pos = b;
        Some(&self.text[a..b])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // One token needs at least one byte, plus a separator between any two.
        (
            0,
            Some(self.text.len().saturating_sub(self.pos).div_ceil(2) + 1),
        )
    }
}

/// Text a tokenizer scans: the caller's input, or a rewrite of it.
///
/// Tokenizers that must transform before splitting (Norwegian and Swedish strip
/// diacritics, Hindi deletes punctuation) cannot always borrow — but they can
/// when the transformation was a no-op, which is the common case. Keeping the
/// distinction in the data lets those tokenizers stay zero-copy on the inputs
/// where zero-copy is possible instead of allocating unconditionally.
#[derive(Debug, Clone)]
pub enum Source<'a> {
    /// The input, unchanged.
    Borrowed(&'a str),
    /// A rewritten copy.
    Owned(String),
}

impl<'a> Source<'a> {
    /// The text to scan.
    #[inline]
    pub fn text(&self) -> &str {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(s) => s,
        }
    }

    /// A slice of the text, borrowing from the input where possible.
    #[inline]
    pub fn slice(&self, start: usize, end: usize) -> Cow<'a, str> {
        match self {
            Self::Borrowed(s) => Cow::Borrowed(&s[start..end]),
            Self::Owned(s) => Cow::Owned(s[start..end].to_owned()),
        }
    }
}

impl<'a> From<Cow<'a, str>> for Source<'a> {
    fn from(c: Cow<'a, str>) -> Self {
        match c {
            Cow::Borrowed(s) => Self::Borrowed(s),
            Cow::Owned(s) => Self::Owned(s),
        }
    }
}

/// [`WordRuns`] over a [`Source`], yielding `Cow` because the text may be a
/// rewrite that the tokens cannot outlive.
#[derive(Debug, Clone)]
pub struct SourceRuns<'a, K> {
    src: Source<'a>,
    pos: usize,
    class: std::marker::PhantomData<K>,
}

impl<'a, K: CharClass> SourceRuns<'a, K> {
    /// Starts a scan over `src`.
    #[inline]
    pub const fn new(src: Source<'a>) -> Self {
        Self {
            src,
            pos: 0,
            class: std::marker::PhantomData,
        }
    }
}

impl<'a, K: CharClass> Iterator for SourceRuns<'a, K> {
    type Item = Cow<'a, str>;

    #[inline]
    fn next(&mut self) -> Option<Cow<'a, str>> {
        let (a, b) = next_run::<K>(self.src.text(), self.pos)?;
        self.pos = b;
        Some(self.src.slice(a, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ascii;
    impl CharClass for Ascii {
        fn is_word(c: char) -> bool {
            c.is_ascii_alphanumeric()
        }
    }

    #[test]
    fn yields_maximal_runs_only() {
        let v: Vec<&str> = WordRuns::<Ascii>::new("  ab!!cd  ").collect();
        assert_eq!(v, ["ab", "cd"]);
    }

    #[test]
    fn empty_and_all_separator_inputs_yield_nothing() {
        assert_eq!(WordRuns::<Ascii>::new("").count(), 0);
        assert_eq!(WordRuns::<Ascii>::new("...").count(), 0);
    }

    #[test]
    fn multibyte_separators_do_not_split_mid_character() {
        let v: Vec<&str> = WordRuns::<Ascii>::new("a日本b").collect();
        assert_eq!(v, ["a", "b"]);
        let v: Vec<&str> = WordRuns::<Ascii>::new("a😀b").collect();
        assert_eq!(v, ["a", "b"]);
    }

    #[test]
    fn source_runs_borrow_when_the_source_did() {
        let src = Source::Borrowed("ab cd");
        let v: Vec<Cow<'_, str>> = SourceRuns::<Ascii>::new(src).collect();
        assert!(matches!(v[0], Cow::Borrowed(_)));
        assert_eq!(v, ["ab", "cd"]);
    }
}
