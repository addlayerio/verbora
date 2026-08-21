//! The corpus-wide term table.
//!
//! A term that appears in fifty documents is stored **once**, as a
//! `Box<str>` in one `Vec`, and referred to everywhere else by a [`TermId`].
//! That is what makes a document a `Vec<(TermId, u32)>` rather than a
//! `HashMap<String, u32>`, and what makes the corpus-wide document-frequency
//! table a plain `Vec<u32>` indexed by id rather than a second hash map.
//!
//! # Representation
//!
//! Ingestion calls [`Interner::intern`] once per *token* — tens of thousands of
//! times for one large document — so the table is shaped around that call
//! rather than around generality:
//!
//! * A term of at most eight bytes (the overwhelming majority of natural
//!   language tokens) is keyed by its bytes packed into a little-endian `u64`
//!   together with its length. The pair *is* the term: `key` fixes the first
//!   `len` bytes and zero-pads the rest, so two distinct terms can never share
//!   a `(key, len)` and a probe needs no verifying byte comparison. One
//!   multiply and one slot compare, with no string hashing and no pointer chase
//!   to the heap.
//! * A longer term goes through an `FxHash`-keyed table whose hits *are*
//!   verified by byte comparison against the stored name, exactly as a
//!   `HashMap` would.
//!
//! Both tables are open-addressed with linear probing at a load factor of at
//! most one half.
//!
//! # Queries never grow the table
//!
//! [`Interner::lookup`] answers without inserting, so probing a corpus with a
//! million distinct query terms cannot make it grow. Only ingestion interns.

use std::hash::Hasher;

/// An interned term, identified by its position in the corpus term table.
///
/// Ids are dense and are assigned in first-encounter order, starting at zero.
/// An id is only meaningful to the [`Interner`] that issued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TermId(u32);

impl TermId {
    /// The id as an array index.
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }

    /// The id as the `u32` it is stored as.
    ///
    /// Exists so callers that need the narrow form do not have to widen to
    /// `usize` and narrow back through a fallible conversion that could never
    /// fail — the round trip is lossless by type width alone, on every target.
    #[inline]
    pub(crate) fn as_u32(self) -> u32 {
        self.0
    }
}

/// One slot of the short-term table: the token's own bytes are the key.
#[derive(Debug, Clone, Copy, Default)]
struct ShortSlot {
    /// The token's bytes, little-endian, zero-padded to eight.
    key: u64,
    /// Byte length, `0..=8`.
    len: u8,
    /// Id plus one; `0` marks an empty slot.
    id1: u32,
}

/// One slot of the long-term table, verified by byte comparison on probe.
#[derive(Debug, Clone, Copy, Default)]
struct LongSlot {
    /// `FxHash` of the term's bytes.
    hash: u64,
    /// Id plus one; `0` marks an empty slot.
    id1: u32,
}

/// Maps term text to a compact id, shared by every document in one corpus.
#[derive(Debug, Clone)]
pub(crate) struct Interner {
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
    /// The longest term the short table can hold, in bytes.
    const SHORT_MAX: usize = 8;

    /// How many distinct terms have been interned.
    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// Packs at most eight bytes into the short-table key: the bytes
    /// little-endian, zero-padded to eight.
    #[inline]
    fn pack_short(bytes: &[u8]) -> u64 {
        let mut buf = [0u8; Self::SHORT_MAX];
        let n = bytes.len().min(Self::SHORT_MAX);
        buf[..n].copy_from_slice(&bytes[..n]);
        u64::from_le_bytes(buf)
    }

    /// Table index hash for a short key: one XOR and one multiply. The key
    /// already *is* the token, so no byte iteration happens at all.
    #[inline]
    fn short_index(key: u64, len: usize) -> usize {
        let h = (key ^ ((len as u64) << 56).rotate_left(17)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (h >> 32) as usize
    }

    /// `FxHash` of a long term's bytes. This is a private table, so exact
    /// identity with `FxHashMap`'s string hashing is irrelevant; collisions are
    /// resolved by the byte comparison in [`Self::intern_long`].
    #[inline]
    fn long_hash(bytes: &[u8]) -> u64 {
        let mut h = rustc_hash::FxHasher::default();
        h.write(bytes);
        h.finish()
    }

    /// Allocates the next id.
    ///
    /// # Panics
    ///
    /// Panics once `u32::MAX - 1` distinct terms have been interned. A corpus
    /// that large would need more than 34 GiB for its term names alone, so the
    /// limit is a statement about the id width rather than a reachable failure
    /// mode; it is checked rather than wrapped because a wrapped id would alias
    /// two different terms silently.
    fn next_id(&self) -> TermId {
        // The assertion, not a narrowing, is what enforces the limit: the
        // tables store ids offset by one so `0` can mark an empty slot, so
        // `u32::MAX` itself is unavailable and the bound is one tighter than
        // `u32::try_from` would give. `names` grows by one per successful call,
        // so this fires before the vector could reach `u32::MAX + 1` — which is
        // why the `try_from` that used to precede it was unreachable.
        assert!(
            self.names.len() < u32::MAX as usize,
            "more than u32::MAX distinct terms"
        );
        TermId(self.names.len() as u32)
    }

    /// Returns the id for `term`, allocating one if it is new.
    ///
    /// # Panics
    ///
    /// See [`Self::next_id`].
    #[inline]
    pub(crate) fn intern(&mut self, term: &str) -> TermId {
        if term.len() <= Self::SHORT_MAX {
            self.intern_short(Self::pack_short(term.as_bytes()), term)
        } else {
            self.intern_long(term)
        }
    }

    /// Interns a term of at most eight bytes, given its packed key.
    ///
    /// `key` must be [`Self::pack_short`] of `term`'s bytes; the caller passes
    /// it separately only because the ingest loop can compute it more cheaply
    /// than a length-dependent copy can.
    #[inline]
    fn intern_short(&mut self, key: u64, term: &str) -> TermId {
        debug_assert_eq!(key, Self::pack_short(term.as_bytes()));
        let len = term.len();
        let mut i = Self::short_index(key, len) & self.short_mask;
        loop {
            let slot = self.short[i];
            if slot.id1 == 0 {
                return self.insert_short(i, key, term);
            }
            if slot.key == key && usize::from(slot.len) == len {
                return TermId(slot.id1 - 1);
            }
            i = (i + 1) & self.short_mask;
        }
    }

    #[cold]
    fn insert_short(&mut self, i: usize, key: u64, term: &str) -> TermId {
        let id = self.next_id();
        self.names.push(Box::from(term));
        self.short[i] = ShortSlot {
            key,
            // Checked by the caller's `term.len() <= SHORT_MAX`.
            len: u8::try_from(term.len()).unwrap_or(u8::MAX),
            id1: id.0 + 1,
        };
        self.short_len += 1;
        if self.short_len * 2 > self.short.len() {
            self.grow_short();
        }
        id
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

    /// Interns a term longer than eight bytes.
    #[inline]
    fn intern_long(&mut self, term: &str) -> TermId {
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
                return TermId(slot.id1 - 1);
            }
            i = (i + 1) & self.long_mask;
        }
    }

    #[cold]
    fn insert_long(&mut self, i: usize, hash: u64, term: &str) -> TermId {
        let id = self.next_id();
        self.names.push(Box::from(term));
        self.long[i] = LongSlot {
            hash,
            id1: id.0 + 1,
        };
        self.long_len += 1;
        if self.long_len * 2 > self.long.len() {
            self.grow_long();
        }
        id
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

    /// Returns the id for `term` if it has been interned, without inserting.
    pub(crate) fn lookup(&self, term: &str) -> Option<TermId> {
        if term.len() <= Self::SHORT_MAX {
            let key = Self::pack_short(term.as_bytes());
            let mut i = Self::short_index(key, term.len()) & self.short_mask;
            loop {
                let slot = self.short[i];
                if slot.id1 == 0 {
                    return None;
                }
                if slot.key == key && usize::from(slot.len) == term.len() {
                    return Some(TermId(slot.id1 - 1));
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
                return Some(TermId(slot.id1 - 1));
            }
            i = (i + 1) & self.long_mask;
        }
    }

    /// The text behind an id, or `None` if `id` did not come from this
    /// interner.
    pub(crate) fn name(&self, id: TermId) -> Option<&str> {
        self.names.get(id.index()).map(|n| &**n)
    }

    /// The text behind an id issued by this interner.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this interner. Every caller inside this
    /// crate holds an id read out of a structure this same `Interner` owns, so
    /// the precondition is structural; [`Self::name`] is the checked form.
    pub(crate) fn name_of(&self, id: TermId) -> &str {
        // `TermId` is `pub(crate)` and never escapes the crate, and every call
        // site reads its ids out of `self.documents[..].entries()` — all
        // interned by this very interner. `names` is append-only (removing a
        // document decrements document frequencies and never touches the
        // interner), so an id, once issued, stays resolvable for the corpus's
        // whole life. Two corpora cannot be crossed either: no public method
        // accepts a `Document`.
        self.name(id).expect("id issued by this interner")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_dense_and_stable() {
        let mut i = Interner::default();
        let a = i.intern("alpha");
        let b = i.intern("beta");
        let c = i.intern("a-term-longer-than-eight-bytes");
        assert_eq!([a.index(), b.index(), c.index()], [0, 1, 2]);
        assert_eq!(i.len(), 3);
        // Re-interning answers the same id without allocating a new one.
        assert_eq!(i.intern("beta"), b);
        assert_eq!(i.intern("a-term-longer-than-eight-bytes"), c);
        assert_eq!(i.len(), 3);
        assert_eq!(i.lookup("alpha"), Some(a));
        assert_eq!(i.lookup("absent"), None);
        assert_eq!(i.lookup("absent-but-longer-than-eight"), None);
        assert_eq!(i.name(c), Some("a-term-longer-than-eight-bytes"));
    }

    #[test]
    fn lookup_does_not_intern() {
        let mut i = Interner::default();
        i.intern("present");
        for probe in ["absent", "also-absent-and-rather-long", ""] {
            assert_eq!(i.lookup(probe), None);
        }
        assert_eq!(i.len(), 1);
    }

    #[test]
    fn padding_is_distinguished_from_content() {
        // The short table pads keys with zero bytes, so a term that *ends* in
        // real NUL bytes must still be distinct from its trimmed spelling.
        let mut i = Interner::default();
        let plain = i.intern("a");
        let nul = i.intern("a\0");
        let empty = i.intern("");
        let just_nul = i.intern("\0");
        assert_ne!(plain, nul);
        assert_ne!(empty, just_nul);
        assert_eq!(i.len(), 4);
        assert_eq!(i.lookup("a\0"), Some(nul));
        assert_eq!(i.lookup(""), Some(empty));
        assert_eq!(i.name(nul), Some("a\0"));
    }

    #[test]
    fn the_short_long_boundary_is_eight_bytes() {
        let mut i = Interner::default();
        let eight = i.intern("exactly8");
        let nine = i.intern("exactly8b");
        assert_ne!(eight, nine);
        assert_eq!(i.lookup("exactly8"), Some(eight));
        assert_eq!(i.lookup("exactly8b"), Some(nine));
        assert_eq!(i.name(eight), Some("exactly8"));
        assert_eq!(i.name(nine), Some("exactly8b"));
        // Eight *bytes*, not eight characters: four two-byte scalars are short,
        // three three-byte scalars are long.
        let short_utf8 = i.intern("ёлка");
        let long_utf8 = i.intern("日本語");
        assert_eq!(i.name(short_utf8), Some("ёлка"));
        assert_eq!(i.name(long_utf8), Some("日本語"));
        assert_eq!(i.lookup("ёлка"), Some(short_utf8));
        assert_eq!(i.lookup("日本語"), Some(long_utf8));
    }

    #[test]
    fn growth_preserves_every_id_and_name() {
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
            assert_eq!(i.name(pair[0]), Some(s.as_str()));
            assert_eq!(i.name(pair[1]), Some(l.as_str()));
        }
        // Ids stayed dense and in first-encounter order.
        let mut sorted: Vec<usize> = ids.iter().map(|id| id.index()).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..1000).collect::<Vec<_>>());
    }

    #[test]
    fn name_of_an_unissued_id_is_none() {
        let mut i = Interner::default();
        let a = i.intern("alpha");
        assert_eq!(i.name(a), Some("alpha"));
        assert_eq!(i.name(TermId(1)), None);
        assert_eq!(i.name(TermId(u32::MAX)), None);
    }
}
