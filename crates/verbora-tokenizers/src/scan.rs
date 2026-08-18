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

/// Builds a [`CharClass::ASCII_LUT`] from a `const fn` predicate.
///
/// A macro rather than a `const fn` because a `const fn` cannot call through a
/// function pointer; expanding the loop per predicate keeps each table derived
/// from the *same* code that answers [`CharClass::is_word`], so the two can
/// never disagree.
macro_rules! ascii_lut {
    ($pred:path) => {{
        let mut lut = [0u64; 2];
        let mut b: u8 = 0;
        while b < 0x80 {
            if $pred(b as char) {
                lut[(b >> 6) as usize] |= 1u64 << (b & 63);
            }
            b += 1;
        }
        Some(lut)
    }};
}
pub(crate) use ascii_lut;

/// A word-character predicate, supplied as a type so it monomorphises away.
///
/// Every implementation delegates to a `const fn` built from the ranges in
/// [`crate::classes`]. A hand-rolled *per-character* `[bool; 128]` or `u128`
/// bitmask lookup for the ASCII half was implemented and measured: it came out
/// **12% slower** (11.2 µs against 9.8 µs on a 9.7 kB document), because
/// `rustc` already compiles a `matches!` over character ranges into a range
/// check plus a bit test, and the explicit mask only adds a 128-bit shift.
/// [`ASCII_LUT`](Self::ASCII_LUT) is a different shape: it feeds a *per-block*
/// scan in [`WordRuns`] that classifies 64 bytes into one `u64` branchlessly
/// and finds run boundaries with `trailing_zeros`, which measured 1.3–1.6x
/// faster than the per-character loop on the same documents.
pub trait CharClass {
    /// ASCII membership as a 128-bit set: bit `b & 63` of `ASCII_LUT[b >> 6]`
    /// is set exactly when `is_word(b as char)` holds for `b < 0x80`.
    ///
    /// `Some` enables [`WordRuns`]'s branchless 64-byte block scan; the default
    /// `None` keeps the per-character path, so implementations outside this
    /// crate keep their exact behaviour without writing a table. In-crate
    /// implementations derive it with the `ascii_lut!` macro from the same
    /// `const fn` that backs [`is_word`](Self::is_word), which is what makes
    /// the two representations provably consistent.
    const ASCII_LUT: Option<[u64; 2]> = None;

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
///
/// When `K` supplies an [`CharClass::ASCII_LUT`], the iterator scans ASCII
/// stretches 64 bytes at a time: each block is classified into a `u64` of
/// word/separator bits with no per-byte branching, and run boundaries fall out
/// of `trailing_zeros`. That block shape — not a per-character table lookup,
/// which was measured slower (see [`CharClass`]) — is what beats the scalar
/// loop on document-scale input. Non-ASCII bytes take the same per-character
/// decode-and-classify path as [`next_run`], entered per byte rather than per
/// block so heavily non-ASCII text pays at most one cheap 8×`u64` ASCII probe
/// per separator, not a wasted 64-byte classification per character.
#[derive(Debug, Clone)]
pub struct WordRuns<'a, K> {
    text: &'a str,
    pos: usize,
    /// Word-membership bits for the cached ASCII window: bit `i` describes
    /// byte `mask_base + i`. Bits at and above `mask_len` are always zero.
    mask: u64,
    /// Start of the cached window. `usize::MAX` is the "nothing cached"
    /// sentinel, chosen so [`Self::new`] can stay `const` — an empty window
    /// (`mask_len == 0`) at that base can never satisfy a lookup.
    mask_base: usize,
    /// Valid bit count of the window: 64 for a verified all-ASCII block,
    /// shorter when the text ends or a non-ASCII byte cuts the window off.
    mask_len: u32,
    class: std::marker::PhantomData<K>,
}

impl<'a, K: CharClass> WordRuns<'a, K> {
    /// Starts a scan over `text`.
    #[inline]
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            pos: 0,
            mask: 0,
            mask_base: usize::MAX,
            mask_len: 0,
            class: std::marker::PhantomData,
        }
    }

    /// Rebuilds the cached window to start at `pos`, which the caller has
    /// already checked holds an ASCII byte — so the window is never empty and
    /// the scan below it always makes progress.
    ///
    /// A full 64-byte block is probed for ASCII purity with eight `u64` loads
    /// first; only a clean block takes the fixed-trip branchless
    /// classification loop (which is what the optimiser vectorises). A mixed
    /// or short window instead classifies just its ASCII prefix with an
    /// early-exit loop, so the cost is proportional to bytes actually
    /// consumed, never to the block size.
    #[inline]
    fn refill(&mut self, lut: &[u64; 2], pos: usize) {
        let tail = &self.text.as_bytes()[pos..];
        self.mask_base = pos;
        if let Some(block) = tail.first_chunk::<64>() {
            let mut any = 0u64;
            for word in block.chunks_exact(8) {
                // The chunk is exactly 8 bytes, so the conversion cannot fail.
                any |= u64::from_le_bytes(word.try_into().expect("8-byte chunk"));
            }
            if any & 0x8080_8080_8080_8080 == 0 {
                let mut mask = 0u64;
                for (i, &b) in block.iter().enumerate() {
                    // `b < 0x80` here, so `b >> 6` indexes in bounds.
                    mask |= ((lut[(b >> 6) as usize] >> (b & 63)) & 1) << i;
                }
                self.mask = mask;
                self.mask_len = 64;
                return;
            }
        }
        let mut mask = 0u64;
        let mut n = 0u32;
        for &b in tail.iter().take(64) {
            if b >= 0x80 {
                break;
            }
            mask |= ((lut[(b >> 6) as usize] >> (b & 63)) & 1) << n;
            n += 1;
        }
        self.mask = mask;
        self.mask_len = n;
    }
}

impl<'a, K: CharClass> Iterator for WordRuns<'a, K> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        // Classes without a table keep the per-character scan, bit for bit.
        let Some(lut) = K::ASCII_LUT else {
            let (a, b) = next_run::<K>(self.text, self.pos)?;
            self.pos = b;
            return Some(&self.text[a..b]);
        };
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        let mut pos = self.pos;

        // Skip separators to the first word character. `wrapping_sub` folds
        // the "before the window" and "past the window" checks into one
        // compare, and makes the `usize::MAX` sentinel base harmless.
        let start = loop {
            if pos >= len {
                self.pos = pos;
                return None;
            }
            let rel = pos.wrapping_sub(self.mask_base);
            if rel < self.mask_len as usize {
                // Bits at and above `mask_len` are zero, so any set bit in
                // `words` is inside the window.
                let words = self.mask >> rel;
                let skip = words.trailing_zeros() as usize;
                if rel + skip < self.mask_len as usize {
                    break pos + skip;
                }
                pos = self.mask_base + self.mask_len as usize;
            } else if bytes[pos] >= 0x80 {
                let c = char_at(self.text, pos);
                if K::is_word(c) {
                    break pos;
                }
                pos += c.len_utf8();
            } else {
                self.refill(&lut, pos);
            }
        };

        // Extend the run to the next separator. Bit `mask_len` of `!mask` is
        // always set for a partial window, so `run` never overshoots the
        // window's valid bits.
        pos = start;
        loop {
            if pos >= len {
                break;
            }
            let rel = pos.wrapping_sub(self.mask_base);
            if rel < self.mask_len as usize {
                let seps = !self.mask >> rel;
                let run = seps.trailing_zeros() as usize;
                if rel + run < self.mask_len as usize {
                    pos += run;
                    break;
                }
                pos = self.mask_base + self.mask_len as usize;
            } else if bytes[pos] >= 0x80 {
                let c = char_at(self.text, pos);
                if !K::is_word(c) {
                    break;
                }
                pos += c.len_utf8();
            } else {
                self.refill(&lut, pos);
            }
        }
        self.pos = pos;
        Some(&self.text[start..pos])
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

    /// The English aggressive class, with the block fast path enabled — the
    /// ASCII-heavy shape the block scan exists for.
    struct En;
    impl CharClass for En {
        const ASCII_LUT: Option<[u64; 2]> = ascii_lut!(crate::classes::is_word_en);
        fn is_word(c: char) -> bool {
            crate::classes::is_word_en(c)
        }
    }

    /// The WordTokenizer class, whose word set *includes* Cyrillic — so word
    /// runs themselves cross in and out of the ASCII window.
    struct Wd;
    impl CharClass for Wd {
        const ASCII_LUT: Option<[u64; 2]> = ascii_lut!(crate::classes::is_word_word);
        fn is_word(c: char) -> bool {
            crate::classes::is_word_word(c)
        }
    }

    /// The per-character scanner, kept as the oracle the block scan is
    /// differentially tested against: [`next_run`] never consults the LUT.
    fn scalar_runs<K: CharClass>(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some((a, b)) = next_run::<K>(text, pos) {
            out.push(&text[a..b]);
            pos = b;
        }
        out
    }

    fn assert_block_matches_scalar<K: CharClass>(s: &str) {
        assert_eq!(
            WordRuns::<K>::new(s).collect::<Vec<_>>(),
            scalar_runs::<K>(s),
            "block scan diverged from the scalar oracle on {s:?}"
        );
    }

    /// Randomized differential test over mixed ASCII / Cyrillic / CJK / emoji
    /// text, including characters that straddle 64-byte block boundaries. The
    /// scalar path is the shipped pre-block-scan behaviour, so agreement here
    /// is agreement with the reference semantics it was verified against.
    #[test]
    fn block_scan_matches_scalar_on_randomized_corpora() {
        let mut state = 0x243F_6A88_85A3_08D3_u64;
        let mut next = move || {
            // xorshift64: tiny, deterministic, and dependency-free.
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet: Vec<char> = "abcXYZ019_'-/ \t\n.!?,;яЖё日é😀кий".chars().collect();
        for _ in 0..4000 {
            let len = (next() % 300) as usize;
            let s: String = (0..len)
                .map(|_| alphabet[next() as usize % alphabet.len()])
                .collect();
            assert_block_matches_scalar::<En>(&s);
            assert_block_matches_scalar::<Wd>(&s);
        }
    }

    /// Deterministic block-boundary edges: runs that span whole blocks, and a
    /// separator or multi-byte character landing on every offset around the
    /// 64-byte seams the randomized test could only hit by luck.
    #[test]
    fn block_scan_handles_block_boundaries() {
        for len in [1usize, 63, 64, 65, 127, 128, 129, 200] {
            let s = "a".repeat(len);
            assert_eq!(WordRuns::<En>::new(&s).collect::<Vec<_>>(), [s.as_str()]);
        }
        for cut in 60..70usize {
            // A separator at every offset around the first block seam.
            let mut s = "a".repeat(130);
            s.replace_range(cut..=cut, " ");
            assert_block_matches_scalar::<En>(&s);
            // A two-byte word character (Cyrillic я, in `Wd` but not `En`).
            let s = format!("{}я{}", "a".repeat(cut), "b".repeat(70));
            assert_block_matches_scalar::<En>(&s);
            assert_block_matches_scalar::<Wd>(&s);
            // A four-byte separator (emoji) at the seam.
            let s = format!("{}😀{}", "a".repeat(cut), "b".repeat(70));
            assert_block_matches_scalar::<Wd>(&s);
        }
        assert_block_matches_scalar::<Wd>(&"я".repeat(100));
        assert_block_matches_scalar::<Wd>(&"я ".repeat(64));
    }
}
