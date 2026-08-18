//! Documents, document keys, and the term interner they share.
//!
//! The reference stores a document as a plain the reference object mapping term to
//! count, with the document's key smuggled into the same map under `__key`.
//! Three consequences of that choice are observable and are reproduced here:
//!
//! * **`__proto__` is unstorable.** Assigning a number to it invokes the
//!   prototype setter, which ignores non-objects, so the token is silently
//!   dropped — `addDocument('__proto__ __proto__ alpha')` yields `{alpha: 1}`.
//! * **`__key` is just another key.** A token spelled `__key` is counted into
//!   the key slot with the reference `+`, so `('mykey', '__key __key alpha')`
//!   leaves the key as the string `"mykey11"`.
//! * **Key order is `for…in` order**, which hoists array-index-like keys to the
//!   front in ascending numeric order and leaves the rest in insertion order.
//!   `listTerms` and `JSON.stringify` both expose it.
//!
//! # Representation
//!
//! Terms are interned to `u32` ids **once per corpus**, so a term that appears
//! in fifty documents is allocated once rather than fifty times, and a document
//! is a `Vec<(TermId, f64)>` plus a hash index. That is a sparse, per-document
//! structure: nothing here ever materialises a `terms × documents` matrix.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::value::{DynValue, JsonValue, Proto, array_index, number_to_string, prototype_member};

/// An interned term identifier.
pub type TermId = u32;

/// The reserved key under which the reference stores a document's key.
pub const KEY_PROPERTY: &str = "__key";

/// The key whose assignment the reference silently discards.
pub const PROTO_PROPERTY: &str = "__proto__";

// ---------------------------------------------------------------------------
// Interner
// ---------------------------------------------------------------------------

/// Intrinsic flag: the term names an `Object.prototype` method, so issue
/// #119's zero-before-count rule applies to it.
pub(crate) const TERM_PROTO_METHOD: u8 = 1;
/// Intrinsic flag: the term is spelled `__key` and mutates the document key.
pub(crate) const TERM_KEY_PROPERTY: u8 = 1 << 1;
/// Intrinsic flag: the term is spelled `__proto__` and is silently dropped.
pub(crate) const TERM_PROTO_PROPERTY: u8 = 1 << 2;

/// Classifies a term by spelling alone.
///
/// These are the three per-token tests [`BuiltDocument::observe`] performs
/// that depend only on the term's text, never on mutable global state — which
/// is what lets the ingest fast path evaluate them once per *distinct* term
/// and cache the result in the interner's tables. The stop-word test is
/// deliberately excluded: the stop-word list is process-global and mutable,
/// so that answer is re-resolved per document instead (see `fast_build`).
fn term_flags(term: &str) -> u8 {
    if term == KEY_PROPERTY {
        return TERM_KEY_PROPERTY;
    }
    if term == PROTO_PROPERTY {
        return TERM_PROTO_PROPERTY;
    }
    if crate::value::OBJECT_PROTOTYPE_METHODS.contains(&term) {
        return TERM_PROTO_METHOD;
    }
    0
}

/// One slot of the short-term table: the token's own bytes are the key.
#[derive(Debug, Clone, Copy, Default)]
struct ShortSlot {
    /// The token's bytes, little-endian, zero-padded to eight.
    key: u64,
    /// Byte length (0..=8). `(key, len)` identifies the token *exactly*:
    /// `key` fixes the first `len` bytes and zero-pads the rest, so two
    /// distinct tokens can never collide and no verifying byte comparison is
    /// needed on a hit.
    len: u8,
    /// The term's [`term_flags`].
    flags: u8,
    /// Id plus one; `0` marks an empty slot.
    id1: u32,
}

/// One slot of the long-term table, verified by byte comparison on probe.
#[derive(Debug, Clone, Copy, Default)]
struct LongSlot {
    /// FxHash of the term's bytes.
    hash: u64,
    /// The term's [`term_flags`].
    flags: u8,
    /// Id plus one; `0` marks an empty slot.
    id1: u32,
}

/// Maps term text to a compact id, shared by every document in one corpus.
///
/// Query terms are deliberately *not* interned: [`Self::lookup`] answers
/// without inserting, so probing a corpus with a million distinct queries
/// cannot grow the table.
///
/// # Representation
///
/// Ingestion calls [`Self::intern`] once per *token* — tens of thousands of
/// times for a large document — so the table is shaped around that call
/// rather than around generality:
///
/// * A term of at most eight bytes (the overwhelming majority of natural-
///   language tokens) is keyed by its bytes packed into a little-endian
///   `u64` plus its length. The pair *is* the term, so a probe is one
///   multiply and one 16-byte slot compare: no string hashing, no `memcmp`,
///   no pointer chase to the heap.
/// * A longer term goes through an FxHash-keyed table whose hits are
///   verified by byte comparison against the stored name, exactly as a
///   `HashMap` would.
///
/// Both tables are open-addressed with linear probing at a load factor of at
/// most one half. Each slot also caches the term's intrinsic [`term_flags`],
/// so the ingest fast path learns everything spelling-dependent about a
/// token from the same probe that resolves its id — see `fast_build` for why
/// that matters.
#[derive(Debug, Clone)]
pub struct Interner {
    names: Vec<Box<str>>,
    short: Vec<ShortSlot>,
    short_mask: usize,
    short_len: usize,
    long: Vec<LongSlot>,
    long_mask: usize,
    long_len: usize,
}

impl Default for Interner {
    /// Starts with small pre-sized tables (~1.3 KiB) so the hot probe loops
    /// never have to branch on emptiness.
    fn default() -> Self {
        Self {
            names: Vec::new(),
            short: vec![ShortSlot::default(); Self::SHORT_CAPACITY],
            short_mask: Self::SHORT_CAPACITY - 1,
            short_len: 0,
            long: vec![LongSlot::default(); Self::LONG_CAPACITY],
            long_mask: Self::LONG_CAPACITY - 1,
            long_len: 0,
        }
    }
}

impl Interner {
    /// Initial short-table capacity; must be a power of two.
    const SHORT_CAPACITY: usize = 64;
    /// Initial long-table capacity; must be a power of two.
    const LONG_CAPACITY: usize = 16;

    /// Packs at most eight bytes into the short-table key: the bytes
    /// little-endian, zero-padded to eight.
    #[inline]
    pub(crate) fn pack_short(bytes: &[u8]) -> u64 {
        debug_assert!(bytes.len() <= 8);
        let mut buf = [0u8; 8];
        buf[..bytes.len()].copy_from_slice(bytes);
        u64::from_le_bytes(buf)
    }

    /// Table index hash for a short key: one XOR and one multiply. The key
    /// already *is* the token, so no byte iteration happens at all.
    #[inline(always)]
    fn short_index(key: u64, len: usize) -> usize {
        let h = (key ^ ((len as u64) << 56).rotate_left(17)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (h >> 32) as usize
    }

    /// FxHash of a long term's bytes. This is a private table, so exact
    /// identity with `FxHashMap`'s string hashing is irrelevant; collisions
    /// are resolved by the byte comparison in [`Self::intern_long`].
    #[inline]
    fn long_hash(bytes: &[u8]) -> u64 {
        use std::hash::Hasher;
        let mut h = rustc_hash::FxHasher::default();
        h.write(bytes);
        h.finish()
    }

    /// Allocates the next id. Ids are dense and their assignment order is
    /// observable through [`BuiltDocument::entries`], so both fast and slow
    /// ingest paths must intern every token in encounter order.
    fn next_id(&self) -> TermId {
        let id = TermId::try_from(self.names.len()).expect("more than u32::MAX distinct terms");
        // `id + 1` must also fit: the tables store ids offset by one so `0`
        // can mark an empty slot.
        assert!(id < TermId::MAX, "more than u32::MAX distinct terms");
        id
    }

    /// Returns the id for `term`, allocating one if it is new.
    ///
    /// # Panics
    ///
    /// Panics if more than `u32::MAX - 1` distinct terms are interned.
    pub fn intern(&mut self, term: &str) -> TermId {
        if term.len() <= 8 {
            self.intern_short(Self::pack_short(term.as_bytes()), term.len())
                .0
        } else {
            self.intern_long(term).0
        }
    }

    /// Interns a term of at most eight bytes by its packed key, returning its
    /// id and its [`term_flags`].
    ///
    /// `key` must be [`Self::pack_short`] of exactly `len` bytes that form
    /// complete UTF-8 sequences (any `&str` subslice on char boundaries
    /// qualifies) — the insert path reconstructs the term's text from the key
    /// alone.
    #[inline]
    pub(crate) fn intern_short(&mut self, key: u64, len: usize) -> (TermId, u8) {
        let mut i = Self::short_index(key, len) & self.short_mask;
        loop {
            let slot = self.short[i];
            if slot.id1 == 0 {
                return self.insert_short(i, key, len);
            }
            if slot.key == key && usize::from(slot.len) == len {
                return (slot.id1 - 1, slot.flags);
            }
            i = (i + 1) & self.short_mask;
        }
    }

    #[cold]
    fn insert_short(&mut self, i: usize, key: u64, len: usize) -> (TermId, u8) {
        let bytes = key.to_le_bytes();
        let term =
            std::str::from_utf8(&bytes[..len]).expect("short keys are packed from &str slices");
        let flags = term_flags(term);
        let id = self.next_id();
        self.names.push(Box::from(term));
        self.short[i] = ShortSlot {
            key,
            len: len as u8,
            flags,
            id1: id + 1,
        };
        self.short_len += 1;
        if self.short_len * 2 > self.short.len() {
            self.grow_short();
        }
        (id, flags)
    }

    #[cold]
    fn grow_short(&mut self) {
        let new_len = self.short.len() * 2;
        let mask = new_len - 1;
        let mut table = vec![ShortSlot::default(); new_len];
        for slot in self.short.iter().filter(|s| s.id1 != 0) {
            let mut i = Self::short_index(slot.key, usize::from(slot.len)) & mask;
            while table[i].id1 != 0 {
                i = (i + 1) & mask;
            }
            table[i] = *slot;
        }
        self.short = table;
        self.short_mask = mask;
    }

    /// Interns a term longer than eight bytes, returning its id and its
    /// [`term_flags`].
    pub(crate) fn intern_long(&mut self, term: &str) -> (TermId, u8) {
        let hash = Self::long_hash(term.as_bytes());
        let mut i = (hash as usize) & self.long_mask;
        loop {
            let slot = self.long[i];
            if slot.id1 == 0 {
                return self.insert_long(i, hash, term);
            }
            if slot.hash == hash
                && self.names[(slot.id1 - 1) as usize].as_bytes() == term.as_bytes()
            {
                return (slot.id1 - 1, slot.flags);
            }
            i = (i + 1) & self.long_mask;
        }
    }

    #[cold]
    fn insert_long(&mut self, i: usize, hash: u64, term: &str) -> (TermId, u8) {
        let flags = term_flags(term);
        let id = self.next_id();
        self.names.push(Box::from(term));
        self.long[i] = LongSlot {
            hash,
            flags,
            id1: id + 1,
        };
        self.long_len += 1;
        if self.long_len * 2 > self.long.len() {
            self.grow_long();
        }
        (id, flags)
    }

    #[cold]
    fn grow_long(&mut self) {
        let new_len = self.long.len() * 2;
        let mask = new_len - 1;
        let mut table = vec![LongSlot::default(); new_len];
        for slot in self.long.iter().filter(|s| s.id1 != 0) {
            let mut i = (slot.hash as usize) & mask;
            while table[i].id1 != 0 {
                i = (i + 1) & mask;
            }
            table[i] = *slot;
        }
        self.long = table;
        self.long_mask = mask;
    }

    /// Returns the id for `term` if it has been interned.
    pub fn lookup(&self, term: &str) -> Option<TermId> {
        if term.len() <= 8 {
            let key = Self::pack_short(term.as_bytes());
            let mut i = Self::short_index(key, term.len()) & self.short_mask;
            loop {
                let slot = self.short[i];
                if slot.id1 == 0 {
                    return None;
                }
                if slot.key == key && usize::from(slot.len) == term.len() {
                    return Some(slot.id1 - 1);
                }
                i = (i + 1) & self.short_mask;
            }
        }
        let hash = Self::long_hash(term.as_bytes());
        let mut i = (hash as usize) & self.long_mask;
        loop {
            let slot = self.long[i];
            if slot.id1 == 0 {
                return None;
            }
            if slot.hash == hash
                && self.names[(slot.id1 - 1) as usize].as_bytes() == term.as_bytes()
            {
                return Some(slot.id1 - 1);
            }
            i = (i + 1) & self.long_mask;
        }
    }

    /// Returns the text behind an id.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not produced by this interner.
    pub fn name(&self, id: TermId) -> &str {
        &self.names[id as usize]
    }
}

// ---------------------------------------------------------------------------
// Document keys
// ---------------------------------------------------------------------------

/// A document key: any the reference value.
///
/// `removeDocument` compares with `===`, so this type's equality is
/// the reference's strict equality — including `NaN !== NaN`, `-0 === 0`, and
/// **reference identity for objects**, which is why [`Self::Object`] holds an
/// `Arc` and compares by pointer. Two structurally identical keys built
/// separately are different keys, exactly as in the reference.
#[derive(Debug, Clone, Default)]
pub enum DocKey {
    /// No key was passed. This is the default, and `removeDocument(undefined)`
    /// matches it.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Num(f64),
    /// A string.
    Str(Arc<str>),
    /// An object or array key, compared by identity.
    Object(Arc<JsonValue>),
}

impl DocKey {
    /// Builds a string key.
    pub fn string(s: impl AsRef<str>) -> Self {
        Self::Str(Arc::from(s.as_ref()))
    }

    /// Builds an object key with a fresh identity.
    pub fn object(value: JsonValue) -> Self {
        Self::Object(Arc::new(value))
    }

    /// The reference `===`.
    pub fn strict_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            // `==` on f64 is exactly `===` on numbers: NaN differs from itself
            // and -0 equals 0.
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The reference truthiness, as `document[term] ? … : 1` tests it.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Object(_) => true,
        }
    }

    /// The reference `key + 1`, which a token spelled `__key` performs.
    ///
    /// A naive port would increment a counter. The reference concatenates when
    /// the key is a string and stringifies objects through `ToPrimitive`, so
    /// `'mykey'` becomes `'mykey1'` and `{a: 1}` becomes `'[object Object]1'`.
    pub fn plus_one(&self) -> Self {
        match self {
            Self::Num(n) => Self::Num(n + 1.0),
            Self::Bool(b) => Self::Num(f64::from(u8::from(*b)) + 1.0),
            Self::Null => Self::Num(1.0),
            Self::Undefined => Self::Num(f64::NAN),
            Self::Str(s) => Self::Str(Arc::from(format!("{s}1"))),
            Self::Object(v) => Self::Str(Arc::from(format!("{}1", v.to_text()))),
        }
    }

    /// The key as the value a property read would produce.
    pub fn as_value(&self) -> DynValue {
        match self {
            Self::Undefined => DynValue::Undefined,
            Self::Null => DynValue::Null,
            Self::Bool(b) => DynValue::Bool(*b),
            Self::Num(n) => DynValue::Num(*n),
            Self::Str(s) => DynValue::Str(Arc::clone(s)),
            Self::Object(v) => DynValue::Json(Arc::clone(v)),
        }
    }

    /// Appends the key's `JSON.stringify` rendering, or returns `false` if the
    /// key is `undefined` — which the serializer **omits entirely**.
    pub fn write_json(&self, out: &mut String) -> bool {
        match self {
            Self::Undefined => return false,
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Num(n) => {
                if n.is_finite() {
                    out.push_str(&number_to_string(*n));
                } else {
                    out.push_str("null");
                }
            }
            Self::Str(s) => crate::value::write_json_string(s, out),
            Self::Object(v) => v.write_json(out),
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Built documents
// ---------------------------------------------------------------------------

/// A term-count map produced by `buildDocument` from a string or a token array.
///
/// Counts are `f64` because they are the reference numbers, and because the
/// issue-#119 fix can store a literal `0`: a token that names an
/// `Object.prototype` method is reset to `0` *before* the stop-word test, so a
/// stop-worded `toString` leaves a zero-valued own property behind.
#[derive(Debug, Clone, Default)]
pub struct BuiltDocument {
    key: DocKey,
    entries: Vec<(TermId, f64)>,
    index: FxHashMap<TermId, u32>,
}

impl BuiltDocument {
    /// An empty document with the given key — the `{ __key: key }` accumulator.
    pub fn new(key: DocKey) -> Self {
        Self {
            key,
            entries: Vec::new(),
            index: FxHashMap::default(),
        }
    }

    /// Assembles a document the `fast_build` path produced.
    ///
    /// `entries` must be in insertion order with at most one entry per id —
    /// exactly what a sequence of [`Self::observe`] calls builds; the
    /// differential tests in `tfidf.rs` hold the fast path to that. The hash
    /// index is derived here, once per document, instead of once per token.
    pub(crate) fn from_parts(key: DocKey, entries: Vec<(TermId, f64)>) -> Self {
        let mut index: FxHashMap<TermId, u32> =
            FxHashMap::with_capacity_and_hasher(entries.len(), Default::default());
        for (i, (id, _)) in entries.iter().enumerate() {
            index.insert(
                *id,
                u32::try_from(i).expect("documents hold < u32::MAX terms"),
            );
        }
        Self {
            key,
            entries,
            index,
        }
    }

    /// The document's key.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The stored count for a term id, if any.
    pub fn count(&self, id: TermId) -> Option<f64> {
        self.index.get(&id).map(|i| self.entries[*i as usize].1)
    }

    /// The entries in insertion order.
    pub fn entries(&self) -> &[(TermId, f64)] {
        &self.entries
    }

    /// Applies one term occurrence, reproducing `buildDocument`'s reduce body.
    ///
    /// `filtered` is the result of the stop-word test; the caller performs it
    /// because the reset that precedes it happens **regardless**.
    pub fn observe(&mut self, id: TermId, term: &str, filtered: bool, interner: &Interner) {
        debug_assert_eq!(interner.name(id), term);

        if term == PROTO_PROPERTY {
            // `document.__proto__ = <number|string>` is a no-op: the accessor
            // ignores anything that is not an object or null. A HashMap-backed
            // port would happily store a count here and diverge.
            return;
        }
        if term == KEY_PROPERTY {
            if !filtered {
                self.key = if self.key.is_truthy() {
                    self.key.plus_one()
                } else {
                    DocKey::Num(1.0)
                };
            }
            return;
        }

        // Issue #119: an inherited method shadows the count, so the reference
        // zeroes the slot first. This runs even when the term is stop-worded.
        let inherited_function =
            !self.index.contains_key(&id) && crate::value::OBJECT_PROTOTYPE_METHODS.contains(&term);
        if inherited_function {
            self.push(id, 0.0);
        }
        if filtered {
            return;
        }
        match self.index.get(&id).copied() {
            Some(i) => {
                let slot = &mut self.entries[i as usize];
                // `document[term] ? document[term] + 1 : 1` — a zeroed slot
                // restarts at 1 rather than becoming 1 by increment.
                slot.1 = if slot.1 == 0.0 || slot.1.is_nan() {
                    1.0
                } else {
                    slot.1 + 1.0
                };
            }
            None => self.push(id, 1.0),
        }
    }

    fn push(&mut self, id: TermId, value: f64) {
        let i = u32::try_from(self.entries.len()).expect("documents hold < u32::MAX terms");
        self.entries.push((id, value));
        self.index.insert(id, i);
    }

    /// The document's properties in the reference `for…in` order.
    ///
    /// Array-index-like keys come first in ascending numeric order, then every
    /// other key in insertion order — and since `{ __key: key }` is the
    /// accumulator's seed, `__key` leads the second group.
    pub fn ordered_slots(&self, interner: &Interner) -> Vec<Slot> {
        let mut indexed: Vec<(u32, u32)> = Vec::new();
        let mut rest: Vec<u32> = Vec::new();
        for (i, (id, _)) in self.entries.iter().enumerate() {
            let i = u32::try_from(i).expect("documents hold < u32::MAX terms");
            match array_index(interner.name(*id)) {
                Some(n) => indexed.push((n, i)),
                None => rest.push(i),
            }
        }
        indexed.sort_unstable();

        let mut slots = Vec::with_capacity(indexed.len() + rest.len() + 1);
        slots.extend(indexed.into_iter().map(|(_, i)| Slot::Term(i)));
        slots.push(Slot::Key);
        slots.extend(rest.into_iter().map(Slot::Term));
        slots
    }
}

/// One property position in a document's `for…in` walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The `__key` property, which `listTerms` skips.
    Key,
    /// A term, identified by its index in [`BuiltDocument::entries`].
    Term(u32),
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// One slot in the corpus.
///
/// `buildDocument` returns anything that is neither a string nor an array
/// **unchanged**, so an object, a number or even `null` can occupy a corpus
/// slot. Such a document never matches a term but still counts towards
/// `documents.length`, and therefore towards every idf — the reference spec relies on
/// exactly that when it asserts `1 + ln(6/5)` for a six-document corpus in which
/// two documents are not strings.
#[derive(Debug, Clone)]
pub enum Document {
    /// Built from a string or a token array.
    Built(BuiltDocument),
    /// Stored verbatim.
    Raw(RawDocument),
}

/// A value `buildDocument` returned unchanged, plus a lookup index.
///
/// The index exists because the reference's document is a reference **object**,
/// and property access on one is O(1). A `Vec<(String, JsonValue)>` alone
/// preserves the key order `listTerms` needs but makes every `idf` an
/// O(documents × terms) scan; on a 256-document corpus deserialized from JSON
/// that measured 3.4 ms for a single `idf`, against 18 ns for the built path.
/// The `Vec` stays authoritative for order; the map only answers lookups.
#[derive(Debug, Clone)]
pub struct RawDocument {
    value: Arc<JsonValue>,
    /// Own-property index. `None` for non-object shapes, which have no own
    /// string keys to index.
    index: Option<FxHashMap<Box<str>, u32>>,
}

impl RawDocument {
    /// Wraps a value, indexing it if it is an object.
    pub fn new(value: JsonValue) -> Self {
        Self::from_arc(Arc::new(value))
    }

    /// Wraps an already-shared value.
    pub fn from_arc(value: Arc<JsonValue>) -> Self {
        let index = match &*value {
            JsonValue::Obj(fields) => Some(
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, (k, _))| {
                        (Box::from(k.as_str()), u32::try_from(i).unwrap_or(u32::MAX))
                    })
                    .collect(),
            ),
            _ => None,
        };
        Self { value, index }
    }

    /// The stored value.
    pub fn value(&self) -> &JsonValue {
        &self.value
    }

    /// The shared handle, for callers that need to keep it alive cheaply.
    pub fn arc(&self) -> &Arc<JsonValue> {
        &self.value
    }

    /// An own property, in O(1) for objects.
    fn own(&self, key: &str) -> Option<&JsonValue> {
        match (&*self.value, &self.index) {
            (JsonValue::Obj(fields), Some(index)) => index.get(key).map(|i| &fields[*i as usize].1),
            _ => self.value.own(key),
        }
    }
}

/// The reason a property read failed, mirroring the reference engine's `TypeError` text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTarget {
    /// `documents[d]` was out of range.
    Undefined,
    /// The document is `null`.
    Null,
}

impl Document {
    /// Reads a property, walking the prototype chain the way the reference does.
    ///
    /// # Errors
    ///
    /// Returns the target kind when the document is `null`, which throws.
    pub fn get(&self, term: &str, interner: &Interner) -> Result<DynValue, ReadTarget> {
        match self {
            Self::Built(doc) => {
                if term == KEY_PROPERTY {
                    return Ok(doc.key.as_value());
                }
                if let Some(id) = interner.lookup(term)
                    && let Some(count) = doc.count(id)
                {
                    return Ok(DynValue::Num(count));
                }
                Ok(prototype_member(Proto::Object, term).unwrap_or(DynValue::Undefined))
            }
            Self::Raw(raw) => raw_get(raw, term),
        }
    }

    /// The document's `__key` property, as `tfidfs` reads it.
    ///
    /// # Errors
    ///
    /// Returns [`ReadTarget::Null`] when the document is `null`.
    pub fn key_value(&self, interner: &Interner) -> Result<DynValue, ReadTarget> {
        self.get(KEY_PROPERTY, interner)
    }

    /// The `DocKey` `removeDocument` compares against, or `undefined` for a
    /// document that has no `__key` at all.
    ///
    /// # Errors
    ///
    /// Returns [`ReadTarget::Null`] when the document is `null`, which is what
    /// `document.__key` does inside `findIndex`.
    pub fn remove_key(&self) -> Result<DocKey, ReadTarget> {
        match self {
            Self::Built(doc) => Ok(doc.key.clone()),
            Self::Raw(raw) => match raw.value() {
                JsonValue::Null => Err(ReadTarget::Null),
                _ => Ok(raw.own(KEY_PROPERTY).map_or(DocKey::Undefined, json_to_key)),
            },
        }
    }

    /// The property names a `for…in` over this document visits, in order.
    ///
    /// A `for…in` over `null`, a number or a boolean visits nothing; over a
    /// string or an array it visits the indices. That is why `listTerms` never
    /// throws by itself even on a corpus full of junk.
    pub fn for_in_keys(&self, interner: &Interner) -> Vec<String> {
        match self {
            Self::Built(doc) => doc
                .ordered_slots(interner)
                .into_iter()
                .map(|slot| match slot {
                    Slot::Key => KEY_PROPERTY.to_owned(),
                    Slot::Term(i) => interner.name(doc.entries()[i as usize].0).to_owned(),
                })
                .collect(),
            Self::Raw(raw) => match raw.value() {
                JsonValue::Obj(fields) => {
                    let mut indexed: Vec<(u32, usize)> = Vec::new();
                    let mut rest: Vec<usize> = Vec::new();
                    for (i, (k, _)) in fields.iter().enumerate() {
                        match array_index(k) {
                            Some(n) => indexed.push((n, i)),
                            None => rest.push(i),
                        }
                    }
                    indexed.sort_unstable();
                    indexed
                        .into_iter()
                        .map(|(_, i)| i)
                        .chain(rest)
                        .map(|i| fields[i].0.clone())
                        .collect()
                }
                JsonValue::Arr(items) => (0..items.len()).map(|i| i.to_string()).collect(),
                // `for (x in 'abc')` walks the UTF-16 indices of the string.
                JsonValue::Str(s) => (0..s.encode_utf16().count())
                    .map(|i| i.to_string())
                    .collect(),
                JsonValue::Null | JsonValue::Bool(_) | JsonValue::Num(_) => Vec::new(),
            },
        }
    }
}

/// Converts a deserialized `__key` back into a comparable key.
fn json_to_key(v: &JsonValue) -> DocKey {
    match v {
        JsonValue::Null => DocKey::Null,
        JsonValue::Bool(b) => DocKey::Bool(*b),
        JsonValue::Num(n) => DocKey::Num(*n),
        JsonValue::Str(s) => DocKey::string(s),
        other => DocKey::object(other.clone()),
    }
}

/// A property read against a value `buildDocument` stored unchanged.
fn raw_get(raw: &RawDocument, term: &str) -> Result<DynValue, ReadTarget> {
    match raw.value() {
        JsonValue::Null => Err(ReadTarget::Null),
        JsonValue::Obj(_) => Ok(raw.own(term).map_or_else(
            || prototype_member(Proto::Object, term).unwrap_or(DynValue::Undefined),
            |found| DynValue::Json(Arc::new(found.clone())),
        )),
        JsonValue::Arr(items) => {
            if let Some(n) = array_index(term)
                && let Some(found) = items.get(n as usize)
            {
                return Ok(DynValue::Json(Arc::new(found.clone())));
            }
            if term == "length" {
                #[expect(clippy::cast_precision_loss, reason = "array lengths fit in f64")]
                return Ok(DynValue::Num(items.len() as f64));
            }
            Ok(prototype_member(Proto::Array, term).unwrap_or(DynValue::Undefined))
        }
        JsonValue::Str(s) => {
            let units: Vec<u16> = s.encode_utf16().collect();
            if let Some(n) = array_index(term)
                && let Some(unit) = units.get(n as usize)
            {
                return Ok(DynValue::Str(Arc::from(String::from_utf16_lossy(
                    std::slice::from_ref(unit),
                ))));
            }
            if term == "length" {
                #[expect(clippy::cast_precision_loss, reason = "string lengths fit in f64")]
                return Ok(DynValue::Num(units.len() as f64));
            }
            Ok(prototype_member(Proto::String, term).unwrap_or(DynValue::Undefined))
        }
        JsonValue::Num(_) => {
            Ok(prototype_member(Proto::Number, term).unwrap_or(DynValue::Undefined))
        }
        JsonValue::Bool(_) => {
            Ok(prototype_member(Proto::Boolean, term).unwrap_or(DynValue::Undefined))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(key: DocKey, tokens: &[&str]) -> (BuiltDocument, Interner) {
        let mut interner = Interner::default();
        let mut doc = BuiltDocument::new(key);
        for t in tokens {
            let id = interner.intern(t);
            doc.observe(id, t, false, &interner);
        }
        (doc, interner)
    }

    #[test]
    fn proto_token_is_dropped() {
        let (doc, i) = build(DocKey::Undefined, &["__proto__", "__proto__", "alpha"]);
        assert_eq!(doc.entries().len(), 1);
        assert_eq!(i.name(doc.entries()[0].0), "alpha");
    }

    #[test]
    fn key_token_corrupts_a_string_key() {
        let (doc, _) = build(DocKey::string("mykey"), &["__key", "__key", "alpha"]);
        assert!(doc.key().strict_eq(&DocKey::string("mykey11")));
    }

    #[test]
    fn key_token_on_an_absent_key_counts_from_one() {
        let (doc, _) = build(DocKey::Undefined, &["__key", "__key", "alpha"]);
        assert!(doc.key().strict_eq(&DocKey::Num(2.0)));
    }

    #[test]
    fn integer_keys_hoist_in_for_in_order() {
        let (doc, i) = build(
            DocKey::string("k"),
            &["zeta", "2020", "alpha", "10", "beta"],
        );
        let names: Vec<String> = doc
            .ordered_slots(&i)
            .into_iter()
            .map(|s| match s {
                Slot::Key => KEY_PROPERTY.to_owned(),
                Slot::Term(idx) => i.name(doc.entries()[idx as usize].0).to_owned(),
            })
            .collect();
        assert_eq!(names, ["10", "2020", "__key", "zeta", "alpha", "beta"]);
    }

    #[test]
    fn object_keys_compare_by_identity() {
        let a = DocKey::object(JsonValue::Obj(vec![("n".into(), JsonValue::Num(0.0))]));
        let b = DocKey::object(JsonValue::Obj(vec![("n".into(), JsonValue::Num(0.0))]));
        assert!(a.strict_eq(&a.clone()));
        assert!(!a.strict_eq(&b));
    }

    #[test]
    fn nan_keys_never_match_themselves() {
        let n = DocKey::Num(f64::NAN);
        assert!(!n.strict_eq(&n));
        assert!(DocKey::Num(-0.0).strict_eq(&DocKey::Num(0.0)));
    }

    #[test]
    fn inherited_methods_are_zeroed_before_counting() {
        let (doc, i) = build(DocKey::Undefined, &["toString", "toString"]);
        assert_eq!(doc.count(i.lookup("toString").unwrap()), Some(2.0));
    }

    #[test]
    fn a_stopworded_method_name_still_leaves_a_zero() {
        let mut interner = Interner::default();
        let mut doc = BuiltDocument::new(DocKey::Undefined);
        let id = interner.intern("toString");
        doc.observe(id, "toString", true, &interner);
        assert_eq!(doc.count(id), Some(0.0));
    }

    // -- interner ------------------------------------------------------------

    #[test]
    fn interner_ids_are_dense_and_stable() {
        let mut i = Interner::default();
        let a = i.intern("alpha");
        let b = i.intern("beta");
        let c = i.intern("a-term-longer-than-eight-bytes");
        assert_eq!([a, b, c], [0, 1, 2]);
        // Re-interning answers the same id without allocating a new one.
        assert_eq!(i.intern("beta"), b);
        assert_eq!(i.intern("a-term-longer-than-eight-bytes"), c);
        assert_eq!(i.lookup("alpha"), Some(a));
        assert_eq!(i.lookup("absent"), None);
        assert_eq!(i.lookup("absent-but-longer-than-eight"), None);
        assert_eq!(i.name(c), "a-term-longer-than-eight-bytes");
    }

    #[test]
    fn interner_distinguishes_padding_from_content() {
        // The short table pads keys with zero bytes, so a term that *ends* in
        // real NUL bytes must still be distinct from its trimmed spelling.
        let mut i = Interner::default();
        let plain = i.intern("a");
        let nul = i.intern("a\0");
        let empty = i.intern("");
        let just_nul = i.intern("\0");
        assert_eq!(4, [plain, nul, empty, just_nul].len());
        assert_ne!(plain, nul);
        assert_ne!(empty, just_nul);
        assert_eq!(i.lookup("a\0"), Some(nul));
        assert_eq!(i.lookup(""), Some(empty));
        assert_eq!(i.name(nul), "a\0");
    }

    #[test]
    fn interner_short_long_boundary() {
        let mut i = Interner::default();
        let eight = i.intern("exactly8");
        let nine = i.intern("exactly8b");
        assert_ne!(eight, nine);
        assert_eq!(i.lookup("exactly8"), Some(eight));
        assert_eq!(i.lookup("exactly8b"), Some(nine));
        assert_eq!(i.name(eight), "exactly8");
        assert_eq!(i.name(nine), "exactly8b");
    }

    #[test]
    fn interner_growth_preserves_ids() {
        // Enough distinct terms to force several rehashes of both tables.
        let mut i = Interner::default();
        let short: Vec<String> = (0..500).map(|n| format!("s{n}")).collect();
        let long: Vec<String> = (0..500).map(|n| format!("long-term-number-{n}")).collect();
        let mut ids = Vec::new();
        for (s, l) in short.iter().zip(&long) {
            ids.push(i.intern(s));
            ids.push(i.intern(l));
        }
        for ((s, l), pair) in short.iter().zip(&long).zip(ids.chunks(2)) {
            assert_eq!(i.lookup(s), Some(pair[0]), "{s} moved");
            assert_eq!(i.lookup(l), Some(pair[1]), "{l} moved");
            assert_eq!(i.name(pair[0]), s.as_str());
            assert_eq!(i.name(pair[1]), l.as_str());
        }
        // Ids stayed dense and in first-encounter order.
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..1000).collect::<Vec<_>>());
    }

    #[test]
    fn interner_handles_non_ascii_terms() {
        let mut i = Interner::default();
        // "ёлка" is 8 bytes in UTF-8 (short table); "Ленинград" is long.
        let a = i.intern("ёлка");
        let b = i.intern("ленинград");
        assert_eq!(i.lookup("ёлка"), Some(a));
        assert_eq!(i.lookup("ленинград"), Some(b));
        assert_eq!(i.name(a), "ёлка");
        assert_eq!(i.name(b), "ленинград");
    }
}
