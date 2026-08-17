//! Access to the bundled data, embedded in the binary at build time.
//!
//! `build.rs` packs the two lexicon JSON files into a compact index that this
//! module reads *in place*: no parsing, no allocation, and no `OnceLock`
//! initialisation of a 92,662-entry hash map. Startup cost is the cost of
//! slicing a byte array — see [`crate::lexicon`] for the measured comparison
//! against parsing the JSON.
//!
//! # Layout
//!
//! All integers little-endian; every section is a contiguous run of fixed-width
//! values so a lookup is arithmetic plus a binary search.
//!
//! ```text
//! 0   magic "LEX1"
//! 4   n_entries, n_tags, n_numeric
//! 16  section offsets: tag_off, tag_bytes, key_off, key_bytes, val_off, val_ids, sorted
//! 44  tag_off[n_tags+1]     u32  -> tag_bytes
//!     tag_bytes             u8   interned tag strings
//!     key_off[n_entries+1]  u32  -> key_bytes
//!     key_bytes             u8   keys, in the reference enumeration order
//!     val_off[n_entries+1]  u32  -> val_ids (in u16 units)
//!     val_ids               u16  tag indices
//!     sorted[n_entries]     u32  entry indices, in key byte order
//! ```
//!
//! Entries are stored in **the reference enumeration order** — array-index-like
//! keys first in ascending numeric order, then insertion order — because that
//! order reaches the output through `prettyPrint` and through the sequence of
//! `addWord` calls `Corpus.buildLexicon` makes. `n_numeric` is the length of the
//! hoisted prefix, so runtime additions can be spliced into it.
//!
//! The byte array is `align_of == 1`, so multi-byte values are read with
//! `from_le_bytes` rather than transmuted. A lookup performs ~17 of those reads;
//! they are not measurable next to the string comparisons.

include!(concat!(env!("OUT_DIR"), "/rules.rs"));

/// The packed English lexicon: 92,662 entries, 122 distinct tags.
static ENGLISH_LEXICON_BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/english.lex"));
/// The packed Dutch lexicon: 11,699 entries, 194 distinct tags.
static DUTCH_LEXICON_BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dutch.lex"));

/// A dictionary read directly out of the executable.
#[derive(Debug, Clone, Copy)]
pub struct StaticLexicon {
    blob: &'static [u8],
    n_entries: usize,
    n_numeric: usize,
    tag_off: usize,
    tag_bytes: usize,
    key_off: usize,
    key_bytes: usize,
    val_off: usize,
    val_ids: usize,
    sorted: usize,
}

#[inline]
fn u32_at(blob: &[u8], at: usize) -> usize {
    u32::from_le_bytes([blob[at], blob[at + 1], blob[at + 2], blob[at + 3]]) as usize
}

#[inline]
fn u16_at(blob: &[u8], at: usize) -> usize {
    u16::from_le_bytes([blob[at], blob[at + 1]]) as usize
}

impl StaticLexicon {
    const fn header(blob: &'static [u8]) -> Self {
        // `const fn` cannot call the helpers above, so the header is unrolled.
        macro_rules! word {
            ($i:expr) => {
                u32::from_le_bytes([
                    blob[$i * 4],
                    blob[$i * 4 + 1],
                    blob[$i * 4 + 2],
                    blob[$i * 4 + 3],
                ]) as usize
            };
        }
        assert!(
            blob[0] == b'L' && blob[1] == b'E' && blob[2] == b'X' && blob[3] == b'1',
            "packed lexicon has the wrong magic"
        );
        Self {
            blob,
            n_entries: word!(1),
            n_numeric: word!(3),
            tag_off: word!(4),
            tag_bytes: word!(5),
            key_off: word!(6),
            key_bytes: word!(7),
            val_off: word!(8),
            val_ids: word!(9),
            sorted: word!(10),
        }
    }

    /// The bundled English dictionary.
    #[must_use]
    pub const fn english() -> Self {
        Self::header(ENGLISH_LEXICON_BLOB)
    }

    /// The bundled Dutch dictionary.
    #[must_use]
    pub const fn dutch() -> Self {
        Self::header(DUTCH_LEXICON_BLOB)
    }

    /// Number of entries.
    #[inline]
    #[must_use]
    pub const fn len(self) -> usize {
        self.n_entries
    }

    /// Whether the dictionary is empty. Never true for the bundled data.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.n_entries == 0
    }

    /// How many leading entries have array-index-like keys.
    ///
    /// The Dutch lexicon has 122 (`"1"`…`"2000"`); the English one has none.
    #[inline]
    #[must_use]
    pub const fn numeric_prefix_len(self) -> usize {
        self.n_numeric
    }

    /// The key of entry `i`, in enumeration order.
    ///
    /// # Panics
    ///
    /// Panics if `i >= len()`.
    #[inline]
    #[must_use]
    pub fn key(self, i: usize) -> &'static str {
        // Written from `&str` in build.rs, so the bytes are valid UTF-8.
        std::str::from_utf8(self.key_raw(i)).expect("packed keys are UTF-8")
    }

    /// The key of entry `i` as raw bytes, skipping UTF-8 validation.
    ///
    /// A lookup makes ~17 probes and only the *last* one is ever converted to a
    /// `&str`. Validating the other sixteen costs `O(key length)` each for an
    /// answer the packer already guaranteed, and it showed up as roughly a third
    /// of `Lexicon::first_category` in `benches/tagger.rs`. Byte comparison is
    /// also the correct ordering here: the permutation table is sorted by bytes,
    /// and for UTF-8 that is the same order as by code point.
    #[inline]
    #[must_use]
    pub fn key_raw(self, i: usize) -> &'static [u8] {
        let lo = self.key_bytes + u32_at(self.blob, self.key_off + i * 4);
        let hi = self.key_bytes + u32_at(self.blob, self.key_off + (i + 1) * 4);
        &self.blob[lo..hi]
    }

    /// The tags of entry `i`.
    ///
    /// # Panics
    ///
    /// Panics if `i >= len()`.
    #[inline]
    #[must_use]
    pub fn tags(self, i: usize) -> StaticTags {
        StaticTags {
            lex: self,
            at: u32_at(self.blob, self.val_off + i * 4),
            end: u32_at(self.blob, self.val_off + (i + 1) * 4),
        }
    }

    /// The `n`-th interned tag string.
    #[inline]
    fn tag(self, n: usize) -> &'static str {
        let lo = self.tag_bytes + u32_at(self.blob, self.tag_off + n * 4);
        let hi = self.tag_bytes + u32_at(self.blob, self.tag_off + (n + 1) * 4);
        std::str::from_utf8(&self.blob[lo..hi]).expect("packed tags are UTF-8")
    }

    /// The enumeration-order index of `word`, or `None`.
    #[must_use]
    pub fn find(self, word: &str) -> Option<usize> {
        let (mut lo, mut hi) = (0usize, self.n_entries);
        let needle = word.as_bytes();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let entry = u32_at(self.blob, self.sorted + mid * 4);
            match self.key_raw(entry).cmp(needle) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(entry),
            }
        }
        None
    }

    /// The first tag of `word`, if the word is present.
    ///
    /// Returns `Some(None)` for a word present with an **empty** tag list — the
    /// English key `""` is exactly that, and the distinction from "absent"
    /// decides whether the default category applies.
    #[inline]
    #[must_use]
    pub fn first_tag(self, word: &str) -> Option<Option<&'static str>> {
        let i = self.find(word)?;
        Some(self.tags(i).next())
    }
}

/// Iterator over one entry's tags.
#[derive(Debug, Clone)]
pub struct StaticTags {
    lex: StaticLexicon,
    at: usize,
    end: usize,
}

impl Iterator for StaticTags {
    type Item = &'static str;

    #[inline]
    fn next(&mut self) -> Option<&'static str> {
        if self.at >= self.end {
            return None;
        }
        let n = u16_at(self.lex.blob, self.lex.val_ids + self.at * 2);
        self.at += 1;
        Some(self.lex.tag(n))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.end - self.at;
        (n, Some(n))
    }
}

impl ExactSizeIterator for StaticTags {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_matches_the_recorded_shape() {
        let en = StaticLexicon::english();
        assert_eq!(en.len(), 92_662);
        assert_eq!(en.numeric_prefix_len(), 0);
        assert_eq!(en.key(0), "'");
        assert_eq!(en.key(2), "Ranavan");
        assert_eq!(en.tags(2).collect::<Vec<_>>(), ["NNP"]);
    }

    #[test]
    fn the_empty_key_maps_to_an_empty_tag_list() {
        // Present, but with no tags: `tagWord('')` returns `[]`, so the word
        // survives untagged instead of taking the default category.
        let en = StaticLexicon::english();
        assert_eq!(en.first_tag(""), Some(None));
        assert_eq!(en.first_tag("no-such-word-anywhere"), None);
    }

    #[test]
    fn words_that_look_like_reference_internals_are_real_entries() {
        let en = StaticLexicon::english();
        assert_eq!(
            en.tags(en.find("undefined").unwrap()).collect::<Vec<_>>(),
            ["JJ"]
        );
        assert_eq!(
            en.tags(en.find("null").unwrap()).collect::<Vec<_>>(),
            ["JJ", "NN"]
        );
        assert_eq!(
            en.tags(en.find("length").unwrap()).collect::<Vec<_>>(),
            ["NN"]
        );
        // ...whereas these are not in the dictionary at all.
        assert!(en.find("__proto__").is_none());
        assert!(en.find("toString").is_none());
    }

    #[test]
    fn dutch_hoists_its_numeric_keys() {
        let du = StaticLexicon::dutch();
        assert_eq!(du.len(), 11_699);
        assert_eq!(du.numeric_prefix_len(), 122);
        assert_eq!(du.key(0), "1");
        assert_eq!(du.key(9), "10");
        assert_eq!(du.key(122), "nijptangen");
    }

    #[test]
    fn every_key_is_findable() {
        for lex in [StaticLexicon::english(), StaticLexicon::dutch()] {
            for i in 0..lex.len() {
                assert_eq!(lex.find(lex.key(i)), Some(i), "{:?}", lex.key(i));
            }
        }
    }

    #[test]
    fn rule_tables_have_the_recorded_sizes() {
        assert_eq!(ENGLISH_RULES.len(), 18);
        assert_eq!(DUTCH_RULES.len(), 285);
        assert_eq!(BRILL_PAPER_RULES.len(), 10);
        assert_eq!(ENGLISH_RULES[12], "* RB CURRENT-WORD-ENDS-WITH ly");
    }
}
