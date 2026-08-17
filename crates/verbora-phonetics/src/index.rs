//! Phonetic neighbor indexing: build once, query many, for candidate retrieval.
//!
//! This is a Verbora-native extension, **not** part of the reference parity
//! surface — no `docs/MIGRATION_MATRIX.md` entry references it, and it must
//! never be reported as one. It exists because every phonetic encoder in this
//! crate answers "do these two words sound alike?" one pair at a time, and a
//! caller with a dictionary of thousands of names has no efficient way to ask
//! "which stored words sound like this one?" without an index.
//!
//! # What this is, and what it deliberately is not
//!
//! [`PhoneticIndex::neighbors`] answers exactly one question: *which stored
//! entries share a phonetic code with this query?* It is phonetic **candidate
//! generation** — a blocking step — not a search engine. It does not rank,
//! does not apply an edit-distance threshold, and does not accept a query
//! language. Compose it with [`verbora_distance`](https://docs.rs/verbora-distance)
//! at the call site when you need that:
//!
//! ```ignore
//! index
//!     .neighbors("Smith")
//!     .map(|candidate| (candidate, jaro_winkler("Smith", candidate)))
//! ```
//!
//! # Build → Freeze → Query
//!
//! [`PhoneticIndexBuilder`] is the mutable, convenient side: call
//! [`PhoneticIndexBuilder::insert`] as many times as you like, in any order.
//! [`PhoneticIndexBuilder::build`] freezes it into a [`PhoneticIndex`] — a
//! compact, immutable, `Send + Sync` structure with no locks and no further
//! allocation on the query path once a query has been encoded. Share it with
//! `Arc<PhoneticIndex<E>>`; nothing about querying it needs `&mut`.
//!
//! # Single- and multi-code encoders
//!
//! [`Metaphone`] and [`SoundEx`] each produce one code per word.
//! [`DoubleMetaphone`] and [`SoundExDM`] produce two — a primary and an
//! alternate spelling of the same sound. [`PhoneticEncoder::Codes`] models
//! this as [`PhoneticCodes`], an inline `One`/`Two` enum: an entry with two
//! codes occupies two buckets, and [`PhoneticIndex::neighbors`] unions and
//! deduplicates across however many codes the query itself produces — no
//! `Vec` and no `HashSet` are allocated to do it, since at most two sorted
//! bucket slices are ever being merged.
//!
//! # Compact codes, and why encoding itself is not zero-allocation (yet)
//!
//! [`InlineCode`] stores a phonetic code inline — `Copy`, no heap allocation,
//! sized per encoder ([`SoundexCode`] is 17 bytes, [`MetaphoneCode`] is 129 —
//! see each type alias's own doc comment for why the margin is that
//! generous, not the tighter ASCII-only bound a first guess would pick).
//! Storing entries this way instead of `String` means the index's own bucket
//! storage never allocates per code.
//!
//! Producing a code from a query string, however, currently goes through
//! each encoder's existing [`Phonetic::process`]/[`DoubleKeyPhonetic::process_double`]
//! method, which returns an owned `String` — [`PhoneticEncoder::encode`]
//! copies its bytes into an `InlineCode` and drops the `String`. That
//! `String` is a real, measured allocation on every `encode()` call,
//! including once per [`PhoneticIndex::neighbors`] query. Giving every
//! encoder a second, allocation-free `process_into` code path was evaluated
//! and deliberately deferred: it would touch four already-parity-verified
//! encoders' internals for a benefit that only matters once bucket lookup
//! itself is the bottleneck, which the benchmarks in
//! `benches/phonetic_index.rs` show it is not at realistic dictionary sizes.
//! See that file's own doc comment for the numbers. Bucket lookup and
//! neighbor iteration *after* encoding are allocation-free, verified by the
//! same benchmarks.
//!
//! # Persistence
//!
//! Not implemented here, on purpose: no caller in this workspace needs it
//! yet. `verbora-spellcheck` — the one crate whose "look up candidates for
//! a misspelled word" shape resembles this one — does not depend on
//! `verbora-phonetics` at all, and nothing outside this crate's own tests
//! and benchmarks constructs a [`PhoneticIndex`]. Per this feature's own
//! spec, that absence of a concrete use case is reason enough to ship no
//! `to_json`/`from_json`, and `Cargo.toml` carries no `serde` dependency.
//!
//! The design was still checked against that spec's other half — not
//! making persistence *impossible* later — by actually compiling a mirror
//! of this module's generic shape against `serde` 1.0.229 (the version
//! this workspace's `Cargo.lock` already pins), not just reasoning about
//! it. Result: [`PhoneticIndex`]'s five frozen fields derive cleanly, and
//! nothing about them, [`EntryId`], or the [`PhoneticEncoder`] trait's own
//! bound would need to change. Two small, additive things would need to
//! exist first, neither of which is a redesign:
//!
//! 1. A **manual** (not derived) `Serialize`/`Deserialize` for
//!    [`InlineCode`]. Deriving directly does not compile —
//!    `#[derive(Serialize)]` on `InlineCode<const N: usize>` fails with
//!    "the trait `Serialize` is not implemented for `[u8; N]`", because
//!    `serde`'s derive only has array support for *literal* lengths
//!    0..=32, not a generic `const N`. This costs nothing real: the
//!    hand-written impl serializes through [`InlineCode::as_str`]
//!    (`"R163"`), which is more compact and more portable than a derived
//!    fixed-width byte array would have been anyway.
//! 2. One `#[serde(bound(...))]` attribute on `PhoneticIndex<E>`.
//!    `#[derive(Serialize)]`'s automatic bound inference adds `E:
//!    Serialize` but cannot see that `codes: Box<[E::Code]>` also needs
//!    `E::Code: Serialize` — `E::Code` is a projection through the trait,
//!    not a bare generic parameter the derive macro can find. This is a
//!    one-line, well-documented `serde` workaround, not a structural
//!    change.
//!
//! Neither step touches [`PhoneticEncoder::Code`]'s existing bound
//! (`Copy + Eq + Hash + Ord` stays exactly as it is), so asking a
//! *hypothetical* external [`PhoneticEncoder`] implementor for
//! `Self::Code: Serialize` is reasonable precisely because it would never
//! be a bound on the trait itself — only on a conditional
//! `impl<E: PhoneticEncoder> Serialize for
//! PhoneticIndex<E> where E: Serialize, E::Code: Serialize` added
//! alongside `to_json`. An implementor who never touches anything
//! serde-shaped changes nothing and pays nothing. [`SoundEx`],
//! [`Metaphone`], [`SoundExDM`] and [`DoubleMetaphone`] are all zero-field
//! unit structs, so deriving `Serialize`/`Deserialize` on them (and on the
//! `u32`-newtype [`EntryId`]) is free, confirmed the same way.
//!
//! The one real caution for whoever adds this later, not a reason to
//! change anything about the fields today: `codes`/`offsets`/`ids` carry
//! an invariant [`PhoneticIndexBuilder::build`] establishes and
//! [`PhoneticIndex::bucket`] trusts without re-checking (`codes`
//! ascending, `offsets.len() == codes.len() + 1`, `offsets` monotonic). A
//! bare `#[derive(Deserialize)]` placed directly on `PhoneticIndex` would
//! accept any wire-provided `codes`/`offsets`/`ids` combination, including
//! one that breaks it — a bad `codes` order gives silently wrong `bucket`
//! results, and a short `offsets` panics on out-of-bounds indexing.
//! `Deserialize` support should be routed through a validating
//! constructor instead of a plain derive, the same way any CSR-shaped
//! structure's deserializer has to be.
//!
//! `rkyv` + `memmap2` — this feature's spec's other named option, for a
//! "high-performance internal archive" rather than portable interchange —
//! was not evaluated here: it raises separate, zero-copy-validation
//! questions that closing out the `serde` question above did not require
//! answering.
//!
//! [`Metaphone`]: crate::Metaphone
//! [`SoundEx`]: crate::SoundEx
//! [`DoubleMetaphone`]: crate::DoubleMetaphone
//! [`SoundExDM`]: crate::SoundExDM
//! [`Phonetic::process`]: verbora_core::Phonetic::process
//! [`DoubleKeyPhonetic::process_double`]: verbora_core::DoubleKeyPhonetic::process_double

use std::cmp::Ordering;
use std::fmt;
use std::hash::Hash;

use verbora_core::DoubleKeyPhonetic;

use crate::dm_soundex::SoundExDM;
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
    /// Stores `s` inline.
    ///
    /// # Panics
    ///
    /// Panics if `s` is longer than `N` bytes. Every encoder in this crate
    /// caps its output well under the `N` its [`PhoneticEncoder`] impl
    /// declares (see each impl's own doc comment for the bound), so this
    /// is an internal invariant violation, not a condition a caller's input
    /// can trigger.
    #[inline]
    #[must_use]
    pub fn new(s: &str) -> Self {
        let b = s.as_bytes();
        assert!(
            b.len() <= N,
            "phonetic code {b:?} ({} bytes) exceeds the {N}-byte inline capacity",
            b.len()
        );
        let mut bytes = [0u8; N];
        bytes[..b.len()].copy_from_slice(b);
        Self {
            len: b.len() as u8,
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

/// [`SoundEx`]'s code: one initial "letter" plus up to three digits.
///
/// For ASCII input this is always exactly 4 bytes (`process("render")` is
/// `"R536"`). It is **not** capped at 4 bytes in general: The reference
/// passes non-Latin and accented initial characters through unchanged
/// (`process("日本語")` is `"日000"`, 6 bytes), and `.toUpperCase()`-style
/// case mapping can itself expand one character into several
/// (`process("ß")` is `"SS000"`). 16 bytes is a deliberately generous bound
/// covering every case-expansion Unicode defines (at most a few codepoints,
/// each at most 4 UTF-8 bytes) plus the 3-digit suffix — see
/// `tests::soundex_code_never_overflows_on_unicode_stress_input` for the
/// property test backing that margin, not just an unverified guess.
pub type SoundexCode = InlineCode<16>;
/// [`SoundExDM`]'s code: six digits, always exactly 6 bytes.
pub type DaitchMokotoffCode = InlineCode<6>;
/// [`Metaphone`]'s and [`DoubleMetaphone`]'s code: capped at the default
/// maximum length of 32 — but 32 **UTF-16 code units** (the reference's own `.length`
/// semantics), not 32 bytes. For CJK and other non-BMP-adjacent scripts one
/// unit can cost up to 4 UTF-8 bytes, so the real worst case is
/// `32 * 4 = 128` bytes, not 32 — an earlier, wrong assumption that
/// `tests::soundex_code_never_overflows_on_unicode_stress_input`'s sibling
/// stress coverage caught directly (a 50-character CJK repeat produced a
/// 75-byte Metaphone code against a 32-byte cap). See
/// [`Metaphone::process_with`](crate::Metaphone::process_with) for the
/// uncapped form, which [`PhoneticEncoder`] does not use.
pub type MetaphoneCode = InlineCode<128>;

// ---------------------------------------------------------------------------
// One or two codes, without allocating
// ---------------------------------------------------------------------------

/// The code(s) [`PhoneticEncoder::encode`] produced for one input.
///
/// Every encoder in this crate produces either exactly one code
/// ([`SoundEx`], [`Metaphone`], [`SoundExDM`]) or exactly two
/// ([`DoubleMetaphone`]) — never a variable-length list — so this is a
/// two-variant enum, not a `Vec`.
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
/// Implemented for every encoder in this crate. Not implemented generically
/// over [`Phonetic`]/[`DoubleKeyPhonetic`] — each impl picks its own compact
/// [`InlineCode`] width, which those traits' `String`/`(String, String)`
/// return types do not carry.
pub trait PhoneticEncoder {
    /// The compact code type this encoder produces. `Copy` and cheap to
    /// hash/compare by construction — see [`InlineCode`].
    type Code: Copy + Eq + Hash + Ord;

    /// Encodes `input`, producing one code or two.
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code>;
}

impl PhoneticEncoder for SoundEx {
    type Code = SoundexCode;
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        PhoneticCodes::One(SoundexCode::new(&self.process(input)))
    }
}

impl PhoneticEncoder for Metaphone {
    type Code = MetaphoneCode;
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        PhoneticCodes::One(MetaphoneCode::new(&self.process(input)))
    }
}

impl PhoneticEncoder for SoundExDM {
    type Code = DaitchMokotoffCode;
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        PhoneticCodes::One(DaitchMokotoffCode::new(&self.process(input)))
    }
}

impl PhoneticEncoder for DoubleMetaphone {
    type Code = MetaphoneCode;
    fn encode(&self, input: &str) -> PhoneticCodes<Self::Code> {
        let (primary, alternate) = self.process_double(input);
        PhoneticCodes::Two(MetaphoneCode::new(&primary), MetaphoneCode::new(&alternate))
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

    /// `SoundexCode`'s 16-byte bound is a deliberate margin, not a proven
    /// tight one (see its own doc comment) — this stress-tests it against a
    /// wide spread of real Unicode shapes known to be awkward for exactly
    /// this kind of fixed-capacity assumption: astral-plane characters,
    /// combining marks, characters whose `.toUpperCase()` expands (German
    /// ß, several Cyrillic/Greek special-casing rules), right-to-left
    /// scripts, and CJK. Every encoder in this crate is exercised, not just
    /// SoundEx, since Metaphone/DoubleMetaphone/SoundExDM share the same
    /// class of risk against their own 32/6-byte bounds.
    #[test]
    fn soundex_code_never_overflows_on_unicode_stress_input() {
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
            "', repeated 200 times",
            "",
            " ",
            "0",
            "000000000",
        ];
        let long_repeats: Vec<String> = ["日", "ß", "😀", "Ω"]
            .iter()
            .map(|c| c.repeat(50))
            .collect();

        let soundex = SoundEx::new();
        let metaphone = Metaphone::new();
        let dm = SoundExDM::new();
        let dmeta = DoubleMetaphone::new();

        for input in stress_inputs
            .iter()
            .copied()
            .chain(long_repeats.iter().map(String::as_str))
        {
            // Each encode() call itself must not panic (this is what
            // InlineCode::new's assert would trip if a bound were too
            // tight) -- that is the property under test.
            let _ = PhoneticEncoder::encode(&soundex, input);
            let _ = PhoneticEncoder::encode(&metaphone, input);
            let _ = PhoneticEncoder::encode(&dm, input);
            let _ = PhoneticEncoder::encode(&dmeta, input);
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

    #[test]
    fn double_metaphone_unions_and_dedups_primary_and_secondary_buckets() {
        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        // "astromech" -> ("ATRMX", "ATRMK") per this crate's own doctest.
        b.insert("astromech");
        let idx = b.build();
        let hits: Vec<&str> = idx.neighbors("astromech").collect();
        // Must appear exactly once, not twice, even though it occupies two
        // buckets and both match the query.
        assert_eq!(hits, vec!["astromech"]);
    }

    #[test]
    fn double_metaphone_entry_with_identical_primary_and_secondary_codes_is_not_double_counted() {
        // Unlike "astromech" above (ATRMX vs ATRMK, genuinely distinct), the
        // fixture here needs primary == secondary so the *other* branch of
        // `PhoneticCodes::Two` gets exercised: `encode()` still returns two
        // rows for this entry (see `PhoneticCodesIter`, which yields both
        // positions unconditionally), so this checks that colliding onto the
        // very same bucket twice still surfaces the entry exactly once.
        // "Matrix" -> ("MTRKS", "MTRKS") per `double_metaphone.rs`'s own
        // doctest.
        let PhoneticCodes::Two(primary, secondary) = DoubleMetaphone::new().encode("Matrix") else {
            unreachable!("DoubleMetaphone always produces two codes")
        };
        assert_eq!(
            primary, secondary,
            "fixture assumes identical primary/secondary codes"
        );

        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        b.insert("Matrix");
        b.insert("unrelated");
        let idx = b.build();
        let hits: Vec<&str> = idx.neighbors("Matrix").collect();
        // Must appear exactly once, not twice, even though both of its codes
        // are the same code and both match the query.
        assert_eq!(hits, vec!["Matrix"]);
    }

    #[test]
    fn double_metaphone_finds_a_neighbor_matching_only_the_secondary_code() {
        let mut b = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        b.insert("astromech");
        b.insert("unrelated");
        let idx = b.build();
        let PhoneticCodes::Two(primary, secondary) = DoubleMetaphone::new().encode("astromech")
        else {
            unreachable!("DoubleMetaphone always produces two codes")
        };
        assert_ne!(primary, secondary, "the fixture assumes distinct codes");
        // A synthetic query matching only via a manual bucket lookup on the
        // secondary code, bypassing encode() entirely.
        assert!(
            idx.bucket(secondary)
                .iter()
                .any(|&id| idx.get(id) == "astromech")
        );
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

    #[test]
    fn inline_code_new_panics_on_oversized_input() {
        let result = std::panic::catch_unwind(|| InlineCode::<4>::new("toolong"));
        assert!(result.is_err());
    }

    #[test]
    fn inline_code_equality_hash_and_order_match_the_underlying_string() {
        let a = InlineCode::<8>::new("R163");
        let b = InlineCode::<8>::new("R163");
        let c = InlineCode::<8>::new("S530");
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
                "x" | "query" => PhoneticCodes::Two(InlineCode::new("AAA"), InlineCode::new("BBB")),
                "only_a" => PhoneticCodes::One(InlineCode::new("AAA")),
                "only_b" => PhoneticCodes::One(InlineCode::new("BBB")),
                "neither" => PhoneticCodes::One(InlineCode::new("ZZZ")),
                other => panic!("AuditMergeEncoder has no mapping for {other:?}"),
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

        assert_eq!(idx.bucket(InlineCode::new("AAA")), &[x, only_a]);
        assert_eq!(idx.bucket(InlineCode::new("BBB")), &[x, only_b]);

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
