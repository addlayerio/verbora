//! `PhoneticIndex`: the crate's phonetic blocking structure.
//!
//! User-facing prose lives on the public items below, not here, because this
//! module is private and a `//!` comment on it would never reach docs.rs.

use std::cmp::Ordering;
use std::fmt;
use std::hash::Hash;

use crate::double_metaphone::DoubleMetaphone;
use crate::metaphone::Metaphone;
use crate::soundex::SoundEx;

// ---------------------------------------------------------------------------
// Compact inline codes
// ---------------------------------------------------------------------------

/// A phonetic code stored inline, up to `N` bytes, with no heap allocation.
///
/// `Eq`/`Hash`/`Ord` all compare only the first `len` bytes — the padding
/// beyond `len` is never read by a trait method, so its value does not
/// matter (it is left zeroed by [`InlineCode::new`] for `Debug`'s benefit,
/// not for correctness).
#[derive(Clone, Copy)]
pub struct InlineCode<const N: usize> {
    len: u8,
    bytes: [u8; N],
}

impl<const N: usize> InlineCode<N> {
    /// The largest code this type can hold, in bytes.
    pub const CAPACITY: usize = N;

    /// Stores `s` inline, or returns `None` if it does not fit.
    ///
    /// # Choosing the right API
    ///
    /// | | `new` | [`prefix_of`](Self::prefix_of) |
    /// |---|---|---|
    /// | Use case | a code whose length you know is bounded, and where a longer one is a bug | a blocking key over a code with no length bound |
    /// | Over-long input | `None` — you decide | the longest prefix that fits |
    /// | Trade-off | you handle the `None` | two distinct codes sharing a prefix become one key, so the bucket over-generates |
    /// | Recommendation | **the default.** Prefer it wherever the encoder's key length is fixed by its publication. | for [`Metaphone`], whose key length grows with the word |
    #[inline]
    #[must_use]
    pub fn new(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() > N {
            return None;
        }
        let mut bytes = [0u8; N];
        bytes[..b.len()].copy_from_slice(b);
        Some(Self {
            // `b.len() <= N <= u8::MAX` holds for every instantiation in this
            // crate; the widest is 128.
            len: b.len() as u8,
            bytes,
        })
    }

    /// Stores the longest prefix of `s` that fits, cut at a character
    /// boundary.
    ///
    /// Total: every `&str` produces a value. Where `s` already fits this is
    /// exactly [`new`](Self::new)'s result, so the truncation is observable
    /// only for a code longer than `N` bytes.
    #[inline]
    #[must_use]
    pub fn prefix_of(s: &str) -> Self {
        let mut end = s.len().min(N);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let b = &s.as_bytes()[..end];
        let mut bytes = [0u8; N];
        bytes[..end].copy_from_slice(b);
        Self {
            len: end as u8,
            bytes,
        }
    }

    /// The code's occupied bytes, with no UTF-8 validation.
    ///
    /// `Eq`/`Hash`/`Ord` use this, not [`InlineCode::as_str`], and that
    /// is a measured, not a stylistic, choice: `str`'s own `Ord` is
    /// already byte-lexicographic under the hood (UTF-8 preserves
    /// codepoint order byte-for-byte), so `as_str().cmp(...)` was paying
    /// for a `from_utf8` validation pass on both sides of *every single
    /// comparison* — on the hot path of every [`PhoneticIndex::bucket`]
    /// binary search — purely to immediately re-derive the byte slice
    /// this method returns directly. `benches/phonetic_index.rs`'s
    /// `neighbors` and `alt_designs_query` groups measured this costing
    /// 3-8x `encode()`'s own cost for `SoundexCode` and up to 45x for the
    /// wider `MetaphoneCode` (more bytes to validate per comparison) at a
    /// 100K-entry high-cardinality bucket, before this method existed —
    /// see that file's own module doc comment and `src/index.rs`'s for
    /// the exact before/after numbers.
    #[inline]
    #[must_use]
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The code as a string slice.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY-free: `bytes[..len]` was copied verbatim from a `&str` in
        // `new`, so it is valid UTF-8 by construction — no unsafe needed.
        std::str::from_utf8(self.as_bytes()).expect("InlineCode always copies from a valid &str")
    }
}

impl<const N: usize> PartialEq for InlineCode<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl<const N: usize> Eq for InlineCode<N> {}

impl<const N: usize> Hash for InlineCode<N> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `[u8]`'s own `Hash` impl folds in the length before the bytes
        // (see its blanket `impl<T: Hash> Hash for [T]`), so this remains
        // free of the kind of cross-length ambiguity a bare byte dump
        // could invite; it does not need to (and does not) match `str`'s
        // own `Hash` output, which nothing in this crate relies on.
        self.as_bytes().hash(state);
    }
}

impl<const N: usize> PartialOrd for InlineCode<N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<const N: usize> Ord for InlineCode<N> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Byte-lexicographic order on valid UTF-8 *is* codepoint order —
        // this is exactly what `str::cmp` does internally, minus the
        // `from_utf8` validation pass `as_str()` would otherwise repeat on
        // both operands of every comparison. See `as_bytes`'s own doc
        // comment for the measurement.
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl<const N: usize> fmt::Debug for InlineCode<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("InlineCode").field(&self.as_str()).finish()
    }
}

/// [`SoundEx`]'s code: one retained ASCII letter plus three digits, or empty.
///
/// Exactly 4 bytes, for every input, with no margin needed — [`SoundEx`]'s
/// contract fixes both the alphabet (`A`–`Z`, so one byte per character) and
/// the length (four characters, or none). `every_soundex_code_fits_its_inline_width`
/// enumerates the claim rather than trusting it.
pub type SoundexCode = InlineCode<4>;

/// [`Metaphone`]'s and [`DoubleMetaphone`]'s code.
///
/// [`DoubleMetaphone`] caps its keys at four characters, so it always fits
/// with room to spare. [`Metaphone`] does **not** cap its key — its length
/// grows with the word — so its [`PhoneticEncoder`] impl keys the index on the
/// first 128 characters of the key, via [`InlineCode::prefix_of`]. A key that
/// long needs a word of more than 64 letters; two such words sharing a
/// 128-character key prefix land in one bucket, which makes the bucket
/// over-generate candidates and never miss one. Candidate generation is
/// exactly the job [`PhoneticIndex`] does, so over-generation is sound where
/// under-generation would not be.
pub type MetaphoneCode = InlineCode<128>;

// ---------------------------------------------------------------------------
// One or two codes, without allocating
// ---------------------------------------------------------------------------

/// The code(s) [`PhoneticEncoder::encode`] produced for one input.
///
/// Every encoder wired into [`PhoneticIndex`] produces either exactly one code
/// ([`SoundEx`], [`Metaphone`], and [`DoubleMetaphone`] for a name with a
/// single pronunciation) or exactly two ([`DoubleMetaphone`] where the name
/// forks) — never a variable-length list — so this is a two-variant enum, not
/// a `Vec`. [`DaitchMokotoff`](crate::DaitchMokotoff) and
/// [`BeiderMorse`](crate::BeiderMorse), whose branch lists are genuinely
/// unbounded, deliberately do not implement [`PhoneticEncoder`]; see that
/// trait's own documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneticCodes<C> {
    /// A single-code encoder's output.
    One(C),
    /// A dual-code encoder's primary and alternate output. The two codes
    /// may be equal (some inputs metaphonically collapse to one spelling);
    /// [`PhoneticIndex::neighbors`] deduplicates the resulting buckets
    /// either way.
    Two(C, C),
}

impl<C: Copy> IntoIterator for PhoneticCodes<C> {
    type Item = C;
    type IntoIter = PhoneticCodesIter<C>;
    fn into_iter(self) -> Self::IntoIter {
        PhoneticCodesIter {
            codes: self,
            next: 0,
        }
    }
}

/// Iterator over a [`PhoneticCodes`]'s one or two codes. Never allocates.
#[derive(Debug, Clone)]
pub struct PhoneticCodesIter<C> {
    codes: PhoneticCodes<C>,
    next: u8,
}

impl<C: Copy> Iterator for PhoneticCodesIter<C> {
    type Item = C;
    fn next(&mut self) -> Option<C> {
        let item = match (self.codes, self.next) {
            (PhoneticCodes::One(a), 0) => Some(a),
            (PhoneticCodes::Two(a, _), 0) => Some(a),
            (PhoneticCodes::Two(_, b), 1) => Some(b),
            _ => None,
        };
        if item.is_some() {
            self.next += 1;
        }
        item
    }
}

// ---------------------------------------------------------------------------
// The encoder abstraction
// ---------------------------------------------------------------------------

/// A phonetic algorithm usable as a [`PhoneticIndex`] key.
///
/// Implemented for [`SoundEx`], [`Metaphone`] and [`DoubleMetaphone`] — the
/// three encoders whose key count is fixed at one or two. It is deliberately
/// *not* implemented for [`DaitchMokotoff`](crate::DaitchMokotoff) or
/// [`BeiderMorse`](crate::BeiderMorse), whose branch lists have no such bound:
/// squeezing them into [`PhoneticCodes`] would mean dropping every branch after
/// the second, and an index that silently drops keys **misses** candidates,
/// which is the one failure mode a blocking structure must not have. Build a
/// plain `HashMap<String, Vec<EntryId>>` over
/// [`DaitchMokotoff::codes`](crate::DaitchMokotoff::codes) if you need that.
///
/// Not implemented generically over
/// [`Phonetic`](verbora_core::Phonetic) either — each impl picks its own
/// compact [`InlineCode`] width, which that trait's `String` return type does
/// not carry.
///
/// # Implementing it
///
/// `encode` must be **total** and **pure**: the same input must always give
/// the same codes, and no input may panic. [`PhoneticIndex`] relies on both —
/// an entry is found in its own neighbour set precisely because build-time and
/// query-time encoding agree.
pub trait PhoneticEncoder {
    /// The compact code type this encoder produces. `Copy` and cheap to
    /// hash/compare by construction — see [`InlineCode`].
    type Code: Copy + Eq + Hash + Ord;

    /// Encodes `input`, producing one code or two.
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code>;
}

impl PhoneticEncoder for SoundEx {
    type Code = SoundexCode;

    /// Always one code, of exactly four bytes or none — see [`SoundexCode`].
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        let code = self.process(input);
        // Four bytes at most, by SoundEx's own contract; `prefix_of` keeps
        // `encode` total without a branch a caller could not reach anyway.
        PhoneticCodes::One(SoundexCode::prefix_of(&code))
    }
}

impl PhoneticEncoder for Metaphone {
    type Code = MetaphoneCode;

    /// Always one code, keyed on the first 128 characters of the (uncapped)
    /// Metaphone key — see [`MetaphoneCode`].
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        PhoneticCodes::One(MetaphoneCode::prefix_of(&self.process(input)))
    }
}

impl PhoneticEncoder for DoubleMetaphone {
    type Code = MetaphoneCode;

    /// Two codes for a name whose pronunciation forks, one for a name whose
    /// does not — so a name with a single pronunciation occupies one bucket
    /// rather than the same bucket twice.
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        let code = self.process(input);
        match code.alternate() {
            Some(alternate) => PhoneticCodes::Two(
                MetaphoneCode::prefix_of(code.primary()),
                MetaphoneCode::prefix_of(alternate),
            ),
            None => PhoneticCodes::One(MetaphoneCode::prefix_of(code.primary())),
        }
    }
}

// ---------------------------------------------------------------------------
// Entries and ids
// ---------------------------------------------------------------------------

/// An opaque handle to a stored entry, valid for the [`PhoneticIndex`] that
/// produced it.
///
/// `u32`-addressed: a dictionary with more than 4 billion entries needs a
/// different design, and nothing in this workspace approaches that scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EntryId(u32);

impl EntryId {
    #[inline]
    fn index(self) -> usize {
        self.0 as usize
    }
}

// ---------------------------------------------------------------------------
// Builder — the mutable, convenient side
// ---------------------------------------------------------------------------

/// Accumulates entries and their phonetic codes. `build()` freezes it.
///
/// # Example
///
/// The Build → Freeze → Query lifecycle this module's own doc comment
/// describes, end to end:
///
/// ```
/// use verbora_phonetics::{PhoneticIndexBuilder, SoundEx};
///
/// // BUILD: insert as many entries as you like, in any order.
/// let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
/// builder.insert("Smith");
/// builder.insert("Smyth");
/// builder.insert("Johnson");
///
/// // FREEZE: compact, immutable, Send + Sync -- share it behind an Arc.
/// let index = builder.build();
///
/// // QUERY: lazy, allocation-free beyond encoding the query itself.
/// let neighbors: Vec<&str> = index.neighbors("Smith").collect();
/// assert!(neighbors.contains(&"Smith"));
/// assert!(neighbors.contains(&"Smyth")); // "Smith" and "Smyth" share a SoundEx code
/// assert!(!neighbors.contains(&"Johnson"));
/// ```
///
/// # Duplicates
///
/// `insert("Smith")` twice creates two distinct entries with two distinct
/// [`EntryId`]s, both present in [`PhoneticIndex::neighbors`]'s output.
/// Duplicates are **preserved**, not merged — a caller indexing records
/// that happen to share a name (different people, same phonebook entry
/// text) needs both ids back to recover which record actually matched, and
/// only the caller knows whether "same text" should mean "same entry" for
/// their own data. Deduplicate by text yourself, after querying, if that is
/// what you want.
pub struct PhoneticIndexBuilder<E: PhoneticEncoder> {
    encoder: E,
    entries: Vec<Box<str>>,
    // One (code, EntryId) row per code an entry produced — two rows for a
    // dual-code encoder's entry, one for a single-code encoder's.
    rows: Vec<(E::Code, EntryId)>,
}

impl<E: PhoneticEncoder> PhoneticIndexBuilder<E> {
    /// Starts an empty index over `encoder`.
    pub fn new(encoder: E) -> Self {
        Self {
            encoder,
            entries: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Reserves capacity for at least `additional` more entries.
    pub fn reserve(&mut self, additional: usize) {
        self.entries.reserve(additional);
        self.rows.reserve(additional * 2);
    }

    /// Encodes and stores `entry`, returning its id.
    pub fn insert(&mut self, entry: &str) -> EntryId {
        let id = EntryId(self.entries.len() as u32);
        self.entries.push(entry.into());
        for code in self.encoder.encode(entry) {
            self.rows.push((code, id));
        }
        id
    }

    /// Inserts every item of `entries` in order. A thin loop over
    /// [`PhoneticIndexBuilder::insert`] — offered because building from a
    /// whole dictionary at once is the common case, not because it does
    /// anything a caller's own loop could not.
    pub fn extend<'a, I: IntoIterator<Item = &'a str>>(&mut self, entries: I) {
        let iter = entries.into_iter();
        let (lower, _) = iter.size_hint();
        self.reserve(lower);
        for entry in iter {
            self.insert(entry);
        }
    }

    /// The number of entries inserted so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no entries have been inserted yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Freezes the index: sorts and compacts the accumulated codes into the
    /// read-only, lock-free representation [`PhoneticIndex`] queries.
    #[must_use]
    pub fn build(mut self) -> PhoneticIndex<E> {
        // Sort by (code, id) so every code's rows land contiguously, in
        // ascending EntryId order — the ascending order is what lets
        // `Neighbors` merge up to two buckets without allocating.
        self.rows.sort_unstable();

        let mut codes: Vec<E::Code> = Vec::new();
        let mut offsets: Vec<u32> = Vec::with_capacity(self.rows.len() + 1);
        let mut ids: Vec<EntryId> = Vec::with_capacity(self.rows.len());

        for (code, id) in self.rows {
            if codes.last() != Some(&code) {
                codes.push(code);
                offsets.push(ids.len() as u32);
            }
            ids.push(id);
        }
        offsets.push(ids.len() as u32);

        PhoneticIndex {
            encoder: self.encoder,
            entries: self.entries.into_boxed_slice(),
            codes: codes.into_boxed_slice(),
            offsets: offsets.into_boxed_slice(),
            ids: ids.into_boxed_slice(),
        }
    }
}

// ---------------------------------------------------------------------------
// The frozen index — the query side
// ---------------------------------------------------------------------------

/// A compact, immutable phonetic index. Build one with
/// [`PhoneticIndexBuilder`].
///
/// `codes` is sorted, so [`PhoneticIndex::bucket`] binary-searches it;
/// `offsets[i]..offsets[i + 1]` is `codes[i]`'s slice of `ids`. This is a
/// compressed-sparse-row layout — the same shape a sparse matrix or a CSR
/// graph uses — chosen over a frozen `HashMap<Code, Box<[EntryId]>>` and
/// over a per-code direct table after benchmarking both; see
/// `benches/phonetic_index.rs` for the numbers and which one actually won,
/// per encoder.
///
/// `PhoneticIndex<E>` is `Send + Sync` whenever `E` is (every encoder in
/// this crate is a zero-sized, stateless type, so this is always true for
/// them) — share it behind `Arc` for concurrent, lock-free queries.
pub struct PhoneticIndex<E: PhoneticEncoder> {
    encoder: E,
    entries: Box<[Box<str>]>,
    codes: Box<[E::Code]>,
    offsets: Box<[u32]>,
    ids: Box<[EntryId]>,
}

impl<E: PhoneticEncoder> PhoneticIndex<E> {
    /// The number of entries in the index (including duplicates).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The text stored under `id`.
    #[must_use]
    pub fn get(&self, id: EntryId) -> &str {
        &self.entries[id.index()]
    }

    /// The encoder this index was built with.
    #[must_use]
    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    /// `code`'s bucket: every id whose entry produced exactly this code, in
    /// ascending `EntryId` order. Empty if no entry produced it. Allocates
    /// nothing — this is a binary search plus a slice.
    #[must_use]
    pub fn bucket(&self, code: E::Code) -> &[EntryId] {
        match self.codes.binary_search(&code) {
            Ok(i) => {
                let (start, end) = (self.offsets[i] as usize, self.offsets[i + 1] as usize);
                &self.ids[start..end]
            }
            Err(_) => &[],
        }
    }

    /// The entries phonetically similar to `query`: every stored entry that
    /// shares at least one code with `query`, deduplicated, in no
    /// particular order beyond "ascending `EntryId` within a merge run".
    ///
    /// Lazy: encoding `query` is the only allocation on this path (see this
    /// module's own doc comment for why), and iterating — including
    /// `.take(n)` — allocates nothing further.
    #[must_use]
    pub fn neighbors<'a>(&'a self, query: &str) -> Neighbors<'a, E> {
        let (a, b) = match self.encoder.encode(query) {
            PhoneticCodes::One(code) => (self.bucket(code), &[][..]),
            PhoneticCodes::Two(x, y) => (self.bucket(x), self.bucket(y)),
        };
        Neighbors {
            index: self,
            a,
            b,
            last_yielded: None,
        }
    }
}

/// Lazy iterator over [`PhoneticIndex::neighbors`]'s results.
///
/// Merges the (at most two) sorted bucket slices the query's codes selected,
/// skipping an id that would otherwise be yielded twice — the classic
/// sorted-merge dedup, needing only "the last id yielded" as state, not a
/// `HashSet`.
pub struct Neighbors<'a, E: PhoneticEncoder> {
    index: &'a PhoneticIndex<E>,
    a: &'a [EntryId],
    b: &'a [EntryId],
    last_yielded: Option<EntryId>,
}

impl<'a, E: PhoneticEncoder> Iterator for Neighbors<'a, E> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        loop {
            let id = match (self.a.first(), self.b.first()) {
                (Some(&x), Some(&y)) => {
                    if x <= y {
                        self.a = &self.a[1..];
                        x
                    } else {
                        self.b = &self.b[1..];
                        y
                    }
                }
                (Some(&x), None) => {
                    self.a = &self.a[1..];
                    x
                }
                (None, Some(&y)) => {
                    self.b = &self.b[1..];
                    y
                }
                (None, None) => return None,
            };
            if self.last_yielded == Some(id) {
                continue;
            }
            self.last_yielded = Some(id);
            return Some(self.index.get(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_soundex(words: &[&str]) -> PhoneticIndex<SoundEx> {
        let mut b = PhoneticIndexBuilder::new(SoundEx::new());
        b.extend(words.iter().copied());
        b.build()
    }

    #[test]
    fn exact_match_is_a_neighbor() {
        let idx = build_soundex(&["Robert", "Rupert", "Smith"]);
        let names: Vec<&str> = idx.neighbors("Robert").collect();
        assert!(names.contains(&"Robert"));
        assert!(names.contains(&"Rupert")); // both encode to R163
        assert!(!names.contains(&"Smith"));
    }

    #[test]
    fn no_match_is_empty() {
        let idx = build_soundex(&["Robert"]);
        assert_eq!(idx.neighbors("Zzz").count(), 0);
    }

    #[test]
    fn empty_index_is_empty() {
        let idx = build_soundex(&[]);
        assert!(idx.is_empty());
        assert_eq!(idx.neighbors("anything").count(), 0);
    }

    #[test]
    fn duplicates_are_preserved_and_both_returned() {
        let idx = build_soundex(&["Smith", "Smith"]);
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.neighbors("Smith").count(), 2);
    }

    #[test]
    fn unicode_input_does_not_panic() {
        let idx = build_soundex(&["Müller", "日本語", "😀emoji", ""]);
        for probe in ["Müller", "日本語", "😀emoji", "", "unrelated"] {
            let _: Vec<&str> = idx.neighbors(probe).collect();
        }
    }

    /// The inline widths are asserted, not assumed. `SoundexCode` claims
    /// exactly four bytes and `MetaphoneCode` claims that Double Metaphone
    /// always fits; both are enumerated here over the Unicode shapes that
    /// historically broke fixed-capacity assumptions in this crate —
    /// astral-plane scalars, combining marks, case mappings that expand
    /// (`ß` -> `SS`, `ﬃ` -> `FFI`), right-to-left scripts and CJK — plus long
    /// repeats of each.
    #[test]
    fn every_soundex_code_fits_its_inline_width() {
        let stress_inputs = [
            "日本語",
            "Москва",
            "ß",
            "ﬃ",
            "İstanbul",
            "café",
            "naïve",
            "😀😀😀😀😀",
            "a\u{0301}\u{0301}\u{0301}",
            "\u{1F600}\u{1F601}",
            "\u{FFFD}",
            "עברית",
            "العربية",
            "",
            " ",
            "0",
            "000000000",
            "Robert",
            "Mac Gregor",
        ];
        let long_repeats: Vec<String> = ["日", "ß", "😀", "Ω", "Robert", "x"]
            .iter()
            .map(|c| c.repeat(50))
            .collect();

        let soundex = SoundEx::new();
        let dmeta = DoubleMetaphone::new();

        for input in stress_inputs
            .iter()
            .copied()
            .chain(long_repeats.iter().map(String::as_str))
        {
            // SoundEx: the key round-trips through the inline width with no
            // truncation at all, which is the claim `SoundexCode` makes.
            let key = soundex.process(input);
            assert!(
                SoundexCode::new(&key).is_some(),
                "SoundEx key {key:?} for {input:?} does not fit 4 bytes"
            );
            let PhoneticCodes::One(code) = PhoneticEncoder::encode(&soundex, input) else {
                panic!("SoundEx must produce exactly one code");
            };
            assert_eq!(code.as_str(), key, "SoundEx key truncated for {input:?}");

            // Double Metaphone: both keys fit, so `prefix_of` never truncates.
            let code = dmeta.process(input);
            assert!(MetaphoneCode::new(code.primary()).is_some());
            if let Some(alternate) = code.alternate() {
                assert!(MetaphoneCode::new(alternate).is_some());
            }

            // Metaphone's key is uncapped, so only the prefix relation holds.
            let key = Metaphone::new().process(input);
            let PhoneticCodes::One(code) = PhoneticEncoder::encode(&Metaphone::new(), input) else {
                panic!("Metaphone must produce exactly one code");
            };
            assert!(
                key.starts_with(code.as_str()),
                "Metaphone bucket key {:?} is not a prefix of {key:?}",
                code.as_str()
            );
        }
    }

    #[test]
    fn iterator_supports_take_without_materializing_everything() {
        let words: Vec<String> = (0..1000).map(|i| format!("Robert{i}")).collect();
        let mut b = PhoneticIndexBuilder::new(SoundEx::new());
        for w in &words {
            b.insert(w);
        }
        let idx = b.build();
        let first_three: Vec<&str> = idx.neighbors("Robert").take(3).collect();
        assert_eq!(first_three.len(), 3);
    }

    #[test]
    fn insertion_order_does_not_affect_neighbor_set() {
        let forward = build_soundex(&["Alice", "Bob", "Carol"]);
        let backward = build_soundex(&["Carol", "Bob", "Alice"]);
        let mut a: Vec<&str> = forward.neighbors("Alice").collect();
        let mut b: Vec<&str> = backward.neighbors("Alice").collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }

    /// Deterministically generates `n` pseudo-realistic, name-like words for
    /// the large-scale tests below, combining a small pool of prefixes,
    /// middles, and suffixes. No `rand`/`proptest`/fuzzing dependency —
    /// see this module's own doc comment ("Compact codes, and why encoding
    /// itself is not zero-allocation (yet)") and this crate's established
    /// hand-written-stress-test convention (e.g.
    /// `soundex_code_never_overflows_on_unicode_stress_input` above) for why
    /// this workspace stays with hand-rolled generation instead. Combinations
    /// repeat past `PREFIXES.len() * MIDDLES.len() * SUFFIXES.len()` (5,824);
    /// that is fine for the tests using this, which only need realistic
    /// *shape*, not global uniqueness.
    fn generate_words(n: usize) -> Vec<String> {
        const PREFIXES: &[&str] = &[
            "Al", "Ash", "Bar", "Bel", "Cal", "Dan", "El", "Fen", "Gar", "Hal", "Ib", "Jen", "Kal",
            "Lor", "Mar", "Nor", "Os", "Pel", "Quen", "Ral", "Sil", "Tor", "Ul", "Val", "Wes",
            "Xan", "Yor", "Zel",
        ];
        const MIDDLES: &[&str] = &[
            "a", "an", "ar", "el", "en", "ia", "in", "o", "on", "or", "u", "um", "un",
        ];
        const SUFFIXES: &[&str] = &[
            "son", "ton", "ville", "burg", "ford", "land", "man", "wood", "field", "ridge",
            "stone", "worth", "ley", "more", "dale", "gate",
        ];
        let per_prefix = MIDDLES.len() * SUFFIXES.len();
        (0..n)
            .map(|i| {
                let combo = i % (PREFIXES.len() * per_prefix);
                let prefix = PREFIXES[combo / per_prefix];
                let middle = MIDDLES[(combo / SUFFIXES.len()) % MIDDLES.len()];
                let suffix = SUFFIXES[combo % SUFFIXES.len()];
                format!("{prefix}{middle}{suffix}")
            })
            .collect()
    }

    /// Beyond `insertion_order_does_not_affect_neighbor_set`'s 3-word check:
    /// a realistic, hundreds-of-entries dictionary, built three ways —
    /// `PhoneticIndexBuilder::extend` once, individual `insert()` calls in
    /// the same order, and individual `insert()` calls in reverse order —
    /// must answer every probe's `neighbors()` query identically. `build()`
    /// sorts `rows` by `(code, id)` before compacting, so this is really a
    /// property of that sort, not of `extend` specifically; testing it at
    /// hundreds-of-entries scale exercises far more distinct codes and
    /// bucket sizes than the 3-word version above can.
    #[test]
    fn batch_extend_matches_individual_inserts_forward_and_reverse() {
        let words = generate_words(500);

        let mut via_extend = PhoneticIndexBuilder::new(SoundEx::new());
        via_extend.extend(words.iter().map(String::as_str));
        let via_extend = via_extend.build();

        let mut via_forward = PhoneticIndexBuilder::new(SoundEx::new());
        for w in &words {
            via_forward.insert(w);
        }
        let via_forward = via_forward.build();

        let mut via_backward = PhoneticIndexBuilder::new(SoundEx::new());
        for w in words.iter().rev() {
            via_backward.insert(w);
        }
        let via_backward = via_backward.build();

        for probe in &words {
            let mut a: Vec<&str> = via_extend.neighbors(probe).collect();
            let mut b: Vec<&str> = via_forward.neighbors(probe).collect();
            let mut c: Vec<&str> = via_backward.neighbors(probe).collect();
            a.sort_unstable();
            b.sort_unstable();
            c.sort_unstable();
            assert_eq!(a, b, "extend vs. forward insert disagree for {probe:?}");
            assert_eq!(a, c, "extend vs. reverse insert disagree for {probe:?}");
        }
    }

    /// A forking name occupies two buckets and both match the query, so the
    /// merge must surface it once. "Smith" -> SM0 / XMT, per
    /// `double_metaphone.rs`'s own pinned keys.
    #[test]
    fn double_metaphone_unions_and_dedups_primary_and_alternate_buckets() {
        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        b.insert("Smith");
        let idx = b.build();
        let hits: Vec<&str> = idx.neighbors("Smith").collect();
        assert_eq!(hits, vec!["Smith"]);
    }

    /// A name with one pronunciation produces `PhoneticCodes::One`, so it
    /// occupies exactly one bucket -- the `(String, String)` shape this
    /// replaced could only express "two codes that happen to be equal", which
    /// filed a second, redundant row for every such entry.
    #[test]
    fn a_name_with_one_pronunciation_occupies_one_bucket() {
        let codes = PhoneticEncoder::encode(&DoubleMetaphone::new(), "Thompson");
        assert!(
            matches!(codes, PhoneticCodes::One(_)),
            "Thompson has no alternate pronunciation: {codes:?}"
        );

        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        b.insert("Thompson");
        b.insert("unrelated");
        let idx = b.build();
        assert_eq!(
            idx.neighbors("Thompson").collect::<Vec<_>>(),
            vec!["Thompson"]
        );
    }

    /// A neighbour reachable only through the alternate key, looked up
    /// directly so the merge is not what is under test.
    #[test]
    fn double_metaphone_finds_a_neighbor_matching_only_the_alternate_code() {
        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        b.insert("Smith");
        b.insert("unrelated");
        let idx = b.build();
        let PhoneticCodes::Two(primary, alternate) =
            PhoneticEncoder::encode(&DoubleMetaphone::new(), "Smith")
        else {
            panic!("Smith forks, so it must produce two codes");
        };
        assert_ne!(primary, alternate, "the fixture assumes distinct codes");
        assert!(
            idx.bucket(alternate)
                .iter()
                .any(|&id| idx.get(id) == "Smith")
        );
        // "Schmidt"'s primary key IS "Smith"'s alternate, so querying one
        // finds the other.
        assert!(idx.neighbors("Schmidt").any(|n| n == "Smith"));
    }

    #[test]
    fn concurrent_reads_on_a_shared_index() {
        use std::sync::Arc;
        let idx = Arc::new(build_soundex(&["Robert", "Rupert", "Smith", "Smythe"]));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let idx = Arc::clone(&idx);
            handles.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    let _: Vec<&str> = idx.neighbors("Robert").collect();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn entry_ids_returned_by_insert_are_stable_and_match_get() {
        let words = generate_words(500);
        let mut b = PhoneticIndexBuilder::new(SoundEx::new());
        let ids: Vec<EntryId> = words.iter().map(|w| b.insert(w)).collect();
        let idx = b.build();
        for (word, id) in words.iter().zip(ids.iter()) {
            assert_eq!(idx.get(*id), word.as_str());
        }
    }

    /// Strong, cheap-to-check invariant across a whole index, not just a
    /// handful of hand-picked examples: every entry must appear in its own
    /// neighbor set when queried with its own text, because `encode()` is a
    /// pure function of the input string — the code(s) computed for an entry
    /// at build time are exactly the code(s) `neighbors()` computes for that
    /// same text at query time, so the entry can never miss its own bucket.
    /// Checked at a few-thousand-word scale, and for both a single-code
    /// encoder (SoundEx) and a multi-code one (DoubleMetaphone), so the
    /// dedup/merge path in [`Neighbors`] is under the same scrutiny as the
    /// single-bucket path.
    #[test]
    fn every_entry_is_its_own_neighbor_across_a_large_generated_dictionary() {
        let words = generate_words(3_000);

        let mut soundex_builder = PhoneticIndexBuilder::new(SoundEx::new());
        soundex_builder.extend(words.iter().map(String::as_str));
        let soundex_idx = soundex_builder.build();
        for w in &words {
            assert!(
                soundex_idx.neighbors(w).any(|n| n == w),
                "{w:?} was not found in its own SoundEx neighbor set"
            );
        }

        let mut dm_builder = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        dm_builder.extend(words.iter().map(String::as_str));
        let dm_idx = dm_builder.build();
        for w in &words {
            assert!(
                dm_idx.neighbors(w).any(|n| n == w),
                "{w:?} was not found in its own DoubleMetaphone neighbor set"
            );
        }
    }

    /// `new` is total: an over-long code is `None`, not a panic. `prefix_of`
    /// is total the other way, cutting at a character boundary so the stored
    /// bytes are always valid UTF-8 even when the cut lands mid-character.
    #[test]
    fn inline_code_construction_is_total_in_both_directions() {
        assert_eq!(InlineCode::<4>::new("toolong"), None);
        assert_eq!(
            InlineCode::<4>::new("R163").map(|c| c.as_str().to_owned()),
            Some("R163".to_owned())
        );
        assert_eq!(
            InlineCode::<4>::new("").map(|c| c.as_str().to_owned()),
            Some(String::new())
        );

        assert_eq!(InlineCode::<4>::prefix_of("toolong").as_str(), "tool");
        assert_eq!(InlineCode::<4>::prefix_of("R163").as_str(), "R163");
        // "日" is three bytes, so a 4-byte budget holds one and must not cut
        // the second in half.
        assert_eq!(InlineCode::<4>::prefix_of("日本").as_str(), "日");
        assert_eq!(InlineCode::<2>::prefix_of("日").as_str(), "");
    }

    #[test]
    fn inline_code_equality_hash_and_order_match_the_underlying_string() {
        let a = InlineCode::<8>::prefix_of("R163");
        let b = InlineCode::<8>::prefix_of("R163");
        let c = InlineCode::<8>::prefix_of("S530");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.cmp(&c), "R163".cmp("S530"));
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    /// A hand-controlled encoder for auditing [`Neighbors`]' merge/dedup
    /// logic directly, independent of any real encoder's output. Every
    /// existing multi-code test in this module drives the merge through
    /// `DoubleMetaphone`'s actual codes, which only ever exercises "the
    /// query's entry shares a code with itself" shapes. This encoder instead
    /// lets a test wire up the merge's genuinely interesting shape by hand:
    /// one id appearing in *both* of a two-distinct-code query's buckets,
    /// interleaved against other ids that appear in only one of the two —
    /// exactly the case a "skip if equal to the last id yielded" dedup
    /// scheme could get wrong if duplicate occurrences of a value were ever
    /// non-adjacent in the merged output.
    struct AuditMergeEncoder;
    impl PhoneticEncoder for AuditMergeEncoder {
        type Code = InlineCode<8>;
        fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
            match input {
                "x" | "query" => {
                    PhoneticCodes::Two(InlineCode::prefix_of("AAA"), InlineCode::prefix_of("BBB"))
                }
                "only_a" => PhoneticCodes::One(InlineCode::prefix_of("AAA")),
                "only_b" => PhoneticCodes::One(InlineCode::prefix_of("BBB")),
                "neither" => PhoneticCodes::One(InlineCode::prefix_of("ZZZ")),
                // A total fallback: the trait requires `encode` never to panic.
                _ => PhoneticCodes::One(InlineCode::prefix_of("???")),
            }
        }
    }

    /// Hand-verified independently of the existing test suite (see
    /// [`AuditMergeEncoder`]): with `x` -> codes `{AAA, BBB}`, `only_a` ->
    /// `{AAA}` alone and `only_b` -> `{BBB}` alone, `bucket(AAA)` is
    /// `[x, only_a]` and `bucket(BBB)` is `[x, only_b]` (ids in insertion
    /// order 0, 1, 2). Querying with a word that also encodes to
    /// `{AAA, BBB}` merges those two lists; `x`'s id appears once in each
    /// list, at the *front* of both, so the merge must visit it from one
    /// list, then skip the duplicate arriving from the other list, then
    /// continue on to yield both singleton entries — not stop early, and
    /// not double-count `x`.
    #[test]
    fn neighbors_merges_and_dedups_across_two_distinct_code_buckets() {
        let mut b = PhoneticIndexBuilder::new(AuditMergeEncoder);
        let x = b.insert("x");
        let only_a = b.insert("only_a");
        let only_b = b.insert("only_b");
        let _neither = b.insert("neither");
        let idx = b.build();

        assert_eq!(idx.bucket(InlineCode::prefix_of("AAA")), &[x, only_a]);
        assert_eq!(idx.bucket(InlineCode::prefix_of("BBB")), &[x, only_b]);

        let mut hits: Vec<&str> = idx.neighbors("query").collect();
        hits.sort_unstable();
        assert_eq!(
            hits,
            vec!["only_a", "only_b", "x"],
            "x must appear exactly once despite being the head of both buckets"
        );

        // Same invariant holds when the query index. is asked for `x` itself,
        // covering the case where the query's own entry is what produces
        // the shared id at the front of both lists.
        let mut hits_from_x: Vec<&str> = idx.neighbors("x").collect();
        hits_from_x.sort_unstable();
        assert_eq!(hits_from_x, vec!["only_a", "only_b", "x"]);

        assert_eq!(idx.neighbors("neither").count(), 1); // only itself
    }
}
