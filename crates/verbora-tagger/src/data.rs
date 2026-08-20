//! The bundled Brill data, embedded in the binary at build time.
//!
//! `build.rs` packs the two lexicon JSON files into a compact index that this
//! module reads *in place*: no parsing, no allocation, and no lazily-initialised
//! 104,237-entry hash map. Start-up cost is the cost of slicing a byte array.
//!
//! Entries are stored sorted by key bytes, which for well-formed UTF-8 is the
//! same order as by Unicode scalar value, so a lookup is a binary search over
//! bytes and iteration is in a documented, deterministic order.

include!(concat!(env!("OUT_DIR"), "/generated.rs"));

/// The packed English lexicon.
static ENGLISH_LEXICON_BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/english.lex"));
/// The packed Dutch lexicon.
static DUTCH_LEXICON_BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dutch.lex"));

/// A dictionary read directly out of the executable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StaticLexicon {
    blob: &'static [u8],
    n_entries: usize,
    tag_off: usize,
    tag_bytes: usize,
    key_off: usize,
    key_bytes: usize,
    val_off: usize,
    val_ids: usize,
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
            blob[0] == b'L' && blob[1] == b'E' && blob[2] == b'X' && blob[3] == b'2',
            "packed lexicon has the wrong magic"
        );
        Self {
            blob,
            n_entries: word!(1),
            tag_off: word!(3),
            tag_bytes: word!(4),
            key_off: word!(5),
            key_bytes: word!(6),
            val_off: word!(7),
            val_ids: word!(8),
        }
    }

    /// The bundled English dictionary.
    pub(crate) const fn english() -> Self {
        Self::header(ENGLISH_LEXICON_BLOB)
    }

    /// The bundled Dutch dictionary.
    pub(crate) const fn dutch() -> Self {
        Self::header(DUTCH_LEXICON_BLOB)
    }

    /// Number of entries.
    #[inline]
    pub(crate) const fn len(self) -> usize {
        self.n_entries
    }

    /// The key of entry `i`, in ascending byte order.
    #[inline]
    pub(crate) fn key(self, i: usize) -> &'static str {
        // Written from `&str` in build.rs, so the bytes are valid UTF-8.
        std::str::from_utf8(self.key_raw(i)).expect("packed keys are UTF-8")
    }

    /// The key of entry `i` as raw bytes, skipping UTF-8 validation.
    ///
    /// A lookup makes ~17 probes and only the last can ever be handed back as a
    /// `&str`. Validating the other sixteen costs `O(key length)` each for an
    /// answer the packer already guaranteed. Byte comparison is also the correct
    /// ordering: the table is sorted by bytes, and for UTF-8 that is the same
    /// order as by scalar value.
    #[inline]
    pub(crate) fn key_raw(self, i: usize) -> &'static [u8] {
        let lo = self.key_bytes + u32_at(self.blob, self.key_off + i * 4);
        let hi = self.key_bytes + u32_at(self.blob, self.key_off + (i + 1) * 4);
        &self.blob[lo..hi]
    }

    /// The tags of entry `i`. Never empty: `build.rs` rejects entries with none.
    #[inline]
    pub(crate) fn tags(self, i: usize) -> StaticTags {
        StaticTags {
            lex: self,
            at: u32_at(self.blob, self.val_off + i * 4),
            end: u32_at(self.blob, self.val_off + (i + 1) * 4),
        }
    }

    /// The `n`-th interned tag string.
    ///
    /// UTF-8 for the same reason [`StaticLexicon::key`] is: `build.rs` writes
    /// `tag_bytes` from `&str` tags with an offset table of `n_tags + 1`
    /// entries, so `[lo, hi)` is exactly one tag and never spans a boundary.
    /// `n` is never caller-supplied either — it is read out of `val_ids`, which
    /// `build.rs` populates only from its own `tag_ids` map, so every stored id
    /// indexes a tag that exists. `every_packed_entry_satisfies_the_contract`
    /// drains this for all 104,237 shipped entries.
    #[inline]
    fn tag(self, n: usize) -> &'static str {
        let lo = self.tag_bytes + u32_at(self.blob, self.tag_off + n * 4);
        let hi = self.tag_bytes + u32_at(self.blob, self.tag_off + (n + 1) * 4);
        std::str::from_utf8(&self.blob[lo..hi]).expect("packed tags are UTF-8")
    }

    /// The index of `word`, or `None`.
    pub(crate) fn find(self, word: &str) -> Option<usize> {
        let (mut lo, mut hi) = (0usize, self.n_entries);
        let needle = word.as_bytes();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key_raw(mid).cmp(needle) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// The most frequent tag of `word`, if the word is present.
    #[inline]
    pub(crate) fn primary_tag(self, word: &str) -> Option<&'static str> {
        let i = self.find(word)?;
        self.tags(i).next()
    }
}

/// Iterator over one entry's tags.
#[derive(Debug, Clone)]
pub(crate) struct StaticTags {
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

    /// Every source entry the crate does not ship, counted by reason.
    ///
    /// The English source holds 92,662 entries and the crate ships 92,538. The
    /// 124 that do not survive are, exactly:
    ///
    /// | Reason | English | Dutch |
    /// |---|---:|---:|
    /// | key was corpus markup, not a token | 122 | 0 |
    /// | key decoded onto a key already held (`\*` onto `*`) | 1 | 0 |
    /// | entry contract (the key `""`, whose tag list is also empty) | 1 | 0 |
    ///
    /// The 122 markup keys are 86 with a `\` escaping nothing, 36 carrying the
    /// corpus's own word/tag separator and the tag after it; `build.rs`'s
    /// `decode_key` documents both shapes.
    #[test]
    fn every_dropped_source_entry_is_accounted_for() {
        assert_eq!(ENGLISH_SOURCE_ENTRIES, 92_662);
        assert_eq!(ENGLISH_KEYS_NOT_TOKENS, 122);
        assert_eq!(ENGLISH_KEYS_MERGED, 1);
        assert_eq!(ENGLISH_ENTRIES_REJECTED, 1);
        assert_eq!(
            StaticLexicon::english().len(),
            ENGLISH_SOURCE_ENTRIES
                - ENGLISH_KEYS_NOT_TOKENS
                - ENGLISH_KEYS_MERGED
                - ENGLISH_ENTRIES_REJECTED
        );
        assert_eq!(StaticLexicon::english().len(), 92_538);

        assert_eq!(DUTCH_SOURCE_ENTRIES, 11_699);
        assert_eq!(DUTCH_KEYS_NOT_TOKENS, 0);
        assert_eq!(DUTCH_KEYS_MERGED, 0);
        assert_eq!(DUTCH_ENTRIES_REJECTED, 0);
        assert_eq!(StaticLexicon::dutch().len(), 11_699);
    }

    /// Every packed key and every packed tag satisfies the literal contract —
    /// including the one restriction only a tag carries, that it is not the
    /// wildcard `*` — and every entry carries at least one tag. Enumerated, not
    /// sampled.
    #[test]
    fn every_packed_entry_satisfies_the_contract() {
        for lex in [StaticLexicon::english(), StaticLexicon::dutch()] {
            for i in 0..lex.len() {
                let key = lex.key(i);
                assert!(!key.is_empty(), "empty key at {i}");
                assert!(
                    !key.chars().any(char::is_whitespace),
                    "key {key:?} contains whitespace"
                );
                let tags: Vec<&str> = lex.tags(i).collect();
                assert!(!tags.is_empty(), "entry {key:?} has no tags");
                for t in tags {
                    assert!(!t.is_empty(), "empty tag on {key:?}");
                    assert!(
                        !t.chars().any(char::is_whitespace),
                        "tag {t:?} on {key:?} contains whitespace"
                    );
                    assert_ne!(t, "*", "the wildcard is a tag on {key:?}");
                }
            }
        }
    }

    /// Keys are stored in strictly ascending byte order, and every one of them
    /// is findable at its own index. Enumerated over all 104,237 entries.
    #[test]
    fn keys_are_sorted_and_every_key_is_findable() {
        for lex in [StaticLexicon::english(), StaticLexicon::dutch()] {
            for i in 0..lex.len() {
                if i > 0 {
                    assert!(
                        lex.key_raw(i - 1) < lex.key_raw(i),
                        "keys out of order at {i}"
                    );
                }
                assert_eq!(lex.find(lex.key(i)), Some(i), "{:?}", lex.key(i));
            }
        }
    }

    /// No packed key still carries a corpus escape.
    ///
    /// `\` is not a character any of these corpora put inside a token — it is
    /// only ever the escape marker — so one surviving in a packed key means a
    /// key that no conforming token can equal. A `/` *is* legitimate token text
    /// once decoded (`Asia/Pacific`), so it is not part of this check; the keys
    /// that carried a bare separator are counted instead, by
    /// `every_dropped_source_entry_is_accounted_for`.
    #[test]
    fn no_packed_key_carries_corpus_markup() {
        for lex in [StaticLexicon::english(), StaticLexicon::dutch()] {
            for i in 0..lex.len() {
                let key = lex.key(i);
                assert!(
                    !key.contains('\\'),
                    "key {key:?} still carries a corpus escape"
                );
            }
        }
        let en = StaticLexicon::english();
        for markup in ["Asia\\/Pacific", "Asia\\", "Vale\\", "me/PRP", "W/NNP.R.G."] {
            assert_eq!(en.find(markup), None, "{markup:?} is still a key");
        }
        // ...and the tokens those entries were spelled from are reachable.
        assert_eq!(en.primary_tag("Asia/Pacific"), Some("JJ"));
        assert_eq!(en.primary_tag("M*A*S*H"), Some("NNP"));
    }

    #[test]
    fn the_empty_key_is_gone() {
        assert_eq!(StaticLexicon::english().find(""), None);
        assert_eq!(StaticLexicon::english().primary_tag(""), None);
    }

    #[test]
    fn rule_tables_have_the_recorded_sizes() {
        assert_eq!(ENGLISH_RULES.len(), 18);
        assert_eq!(DUTCH_RULES.len(), 274);
        assert_eq!(BRILL_PAPER_RULES.len(), 10);
        assert_eq!(ENGLISH_RULES[12], "* RB CURRENT-WORD-ENDS-WITH ly");
    }
}
