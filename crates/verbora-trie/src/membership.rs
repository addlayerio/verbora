//! A hash membership index over the stored (already-folded) words, so that
//! `contains` costs one hash + one probe chain instead of one dependent arena
//! hop per Unicode scalar.
//!
//! # Why this exists
//!
//! `contains` on the arena is depth-bound: one dependent cache miss per scalar
//! over a multi-megabyte arena, and no layout change recovers that (an SoA/CSR
//! child layout and an ASCII byte path were both measured at zero gain — the
//! cost is the dependent misses, not decoding).
//! A flat membership set answers the same question in one hash of the query
//! bytes plus a short linear probe, roughly halving `contains` latency on the
//! benchmark corpus. The trade is paid at build time: maintaining the index
//! adds ~a third to a bulk load, a cost disclosed on
//! [`Trie::insert`](crate::Trie::insert).
//!
//! # Why it is exact
//!
//! `Trie::contains(s)` walks the Unicode scalars of the *folded* query and asks
//! whether the landing node is a word. That is precisely "is `fold(s)` one of
//! the folded strings that were stored": UTF-8 encoding is a bijection between
//! scalar sequences and well-formed byte sequences, so scalar-sequence
//! equality and byte equality coincide, and every stored word entered the trie
//! already folded. Storing the folded bytes in a set therefore answers
//! `contains` identically — differentially verified against the walk in this
//! crate's tests, including folding, astral scalars, duplicates, and the empty
//! string.
//!
//! # Design
//!
//! Dependency-free and fully safe: an open-addressed, linear-probing table
//! (load factor ≤ ½) over an append-only byte blob holding every key
//! back-to-back. Keys are never removed — the trie has no removal either —
//! which is what lets the blob be append-only and the table insert-only. The
//! hash is an FxHash-style multiply-and-rotate over 8-byte SWAR chunks; no
//! external hasher crate is pulled in for one internal set.
//!
//! # The first-byte gate
//!
//! Answering from a hash gave up the one thing the arena walk did well: it
//! rejected a query whose *first* code unit labels no root edge after a
//! single, always-cached child scan, without reading the rest of the query
//! at all. The hash cannot do that — it must digest every byte before it
//! knows where to look, and the slot it then reads is a random offset into a
//! table that outgrows L2 on any real word list, so the cheapest possible
//! miss cost the same as the most expensive one.
//!
//! [`HashIndex::first_byte`] buys that early exit back for 32 bytes: a
//! 256-bit map of the first byte of every stored key, embedded by value so
//! it shares a cache line with the fields around it and is warm for free.
//! Membership in the map is a *necessary* condition — every stored key's own
//! first byte is set — so clearing it is a sound rejection and setting it
//! only ever falls through to the unchanged probe. The gate is exact for the
//! same reason the table is: it is a predicate on the already-folded query
//! bytes, the same bytes the table stores, so no folding, surrogate or
//! empty-key case can slip past it (the empty key has no first byte and is
//! tracked by its own flag).

/// Hashes `b` by folding 8-byte little-endian chunks into a multiplicative
/// accumulator, seeded with the length so prefixes of one another ("ab",
/// "abc") diverge even before the tail chunk does.
#[inline]
pub(crate) fn hash_bytes(b: &[u8]) -> u64 {
    /// The multiplier `rustc-hash` (FxHash) uses; any odd constant with good
    /// bit dispersion works, this one is battle-tested.
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h: u64 = b.len() as u64;
    let mut it = b.chunks_exact(8);
    for c in it.by_ref() {
        // Infallible by `chunks_exact`'s own contract: it yields *only*
        // exactly-8-byte slices, and any short tail is excluded from the
        // iterator and reachable solely through `remainder()` below. The
        // conversion has no other failure mode.
        let v = u64::from_le_bytes(c.try_into().expect("chunks_exact yields 8 bytes"));
        h = (h.rotate_left(5) ^ v).wrapping_mul(K);
    }
    let rem = it.remainder();
    if !rem.is_empty() {
        let mut buf = [0u8; 8];
        buf[..rem.len()].copy_from_slice(rem);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(buf)).wrapping_mul(K);
    }
    h
}

/// Insert-only membership set over folded key bytes.
///
/// `slots` holds `key index + 1` (0 marks an empty slot); key `k`'s bytes
/// live at `blob[offs[k]..offs[k + 1]]`. Per-key hashes are kept so growing
/// re-slots without re-reading (and re-hashing) the blob.
#[derive(Clone, Default)]
pub(crate) struct HashIndex {
    /// Which bytes occur as the *first* byte of some stored key, bit `b & 63`
    /// of word `b >> 6`. See the module doc comment's "first-byte gate": a
    /// clear bit rejects the query before it is hashed or a slot is touched.
    /// First field so it lands at the start of the enclosing `Trie` rather
    /// than behind the `Vec`s.
    first_byte: [u64; 4],
    /// Whether the empty key is stored. It has no first byte, so
    /// [`HashIndex::first_byte`] cannot represent it and the gate needs this
    /// one bit to stay a *necessary* condition rather than a wrong one.
    has_empty: bool,
    /// `slots.len() - 1`; `slots.len()` is always a power of two (or zero
    /// before the first insert — the empty state allocates nothing, so a
    /// fresh [`Trie`](crate::Trie) stays two allocations, not three).
    mask: usize,
    slots: Vec<u32>,
    hashes: Vec<u64>,
    blob: Vec<u8>,
    /// `len + 1` entries once non-empty: `offs[0] == 0`, then one end offset
    /// per key. `usize`, not `u32`: a narrower offset would make a blob larger
    /// than 4 GiB a panic, and a dictionary that large is a legitimate input.
    offs: Vec<usize>,
    len: usize,
}

impl HashIndex {
    /// An empty index. Allocation is deferred to the first insert so that
    /// constructing a `Trie` that never stores anything stays free.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Pre-sizes for `keys` more insertions totalling about `bytes` of key
    /// data, so a bulk load pays no incremental rehashes or blob doublings.
    ///
    /// Every step is best-effort. `keys` reaches here from
    /// [`Trie::insert_all`](crate::Trie::insert_all), whose only source for it
    /// is an iterator's `size_hint` — a number any safe iterator may overstate,
    /// up to `usize::MAX`. This table is a pure accelerator over the arena, so
    /// a reservation that cannot be satisfied is dropped: the load then pays
    /// the rehashes it would have paid without this call, which is the cost of
    /// an optimisation not applying, not a failure.
    pub(crate) fn reserve(&mut self, keys: usize, bytes: usize) {
        // `insert_folded` refuses to store more than `u32::MAX` keys, so
        // headroom above that can never be filled and pre-sizing for it is
        // waste by construction.
        let keys = keys.min((u32::MAX as usize).saturating_sub(self.len));
        let want = (self.len + keys)
            .checked_mul(2)
            .and_then(|n| n.checked_add(1))
            .and_then(usize::checked_next_power_of_two)
            .map_or(0, |n| n.max(64));
        if want > self.slots.len() {
            // Sized fallibly rather than through `rebuild_slots`: the table it
            // would build for a load of billions of keys is tens of gigabytes,
            // and failing to widen the table only lengthens probe chains.
            #[expect(
                clippy::slow_vector_initialization,
                reason = "`vec![0; want]` is the infallible form this is avoiding: \
                          `want` derives from an untrusted `size_hint`, so the allocation \
                          has to be able to fail without aborting"
            )]
            let mut slots = Vec::new();
            if slots.try_reserve_exact(want).is_ok() {
                slots.resize(want, 0);
                self.reslot_into(slots);
            }
        }
        let _ = self.hashes.try_reserve(keys);
        let _ = self.blob.try_reserve(bytes);
        if self.offs.is_empty() {
            let _ = self.offs.try_reserve(keys.saturating_add(1));
            self.offs.push(0);
        } else {
            let _ = self.offs.try_reserve(keys);
        }
    }

    /// Bytes of key `k`.
    #[inline]
    fn key_bytes(&self, k: usize) -> &[u8] {
        &self.blob[self.offs[k]..self.offs[k + 1]]
    }

    /// Whether some stored key starts with the byte `b0`.
    #[inline]
    fn first_byte_seen(&self, b0: u8) -> bool {
        self.first_byte[usize::from(b0 >> 6)] & (1u64 << (b0 & 63)) != 0
    }

    /// Records `b`'s first byte (or its emptiness) in the gate.
    #[inline]
    fn note_first_byte(&mut self, b: &[u8]) {
        match b.first() {
            Some(&b0) => self.first_byte[usize::from(b0 >> 6)] |= 1u64 << (b0 & 63),
            None => self.has_empty = true,
        }
    }

    /// Whether `b` — which must already be folded — is a stored key.
    #[inline]
    pub(crate) fn contains_folded(&self, b: &[u8]) -> bool {
        // The first-byte gate, before the hash: a byte no stored key starts
        // with settles the question out of one 32-byte map that never leaves
        // cache, which is the arena walk's root-level early exit restored.
        // An empty index has every bit clear and `has_empty` unset, so this
        // also subsumes the `len == 0` case the probe below would otherwise
        // have to guard (`slots` is empty then, and indexing it would panic).
        match b.first() {
            Some(&b0) => {
                if !self.first_byte_seen(b0) {
                    return false;
                }
            }
            None => {
                if !self.has_empty {
                    return false;
                }
            }
        }
        let h = hash_bytes(b);
        let mut i = (h as usize) & self.mask;
        loop {
            let s = self.slots[i];
            if s == 0 {
                return false;
            }
            if self.key_bytes((s - 1) as usize) == b {
                return true;
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Inserts `b` — which must already be folded — returning `true` when it
    /// was **already** present.
    pub(crate) fn insert_folded(&mut self, b: &[u8]) -> bool {
        if self.slots.is_empty() {
            self.reserve(1, b.len());
        }
        let h = hash_bytes(b);
        let mut i = (h as usize) & self.mask;
        loop {
            let s = self.slots[i];
            if s == 0 {
                break;
            }
            if self.key_bytes((s - 1) as usize) == b {
                return true;
            }
            i = (i + 1) & self.mask;
        }
        let k = self.len;
        // Only on a genuine insert: the gate must admit every stored key, and
        // a duplicate returned above without reaching here has already been
        // recorded.
        self.note_first_byte(b);
        self.blob.extend_from_slice(b);
        self.offs.push(self.blob.len());
        self.hashes.push(h);
        // The one remaining capacity bound, documented on `Trie` itself: more
        // than `u32::MAX` distinct keys needs tens of gigabytes of live text.
        self.slots[i] = u32::try_from(k + 1).expect("membership index exceeds 2^32 keys");
        self.len += 1;
        // Growing at load ½ keeps probe chains short; the check runs after
        // the insert so `i` above was found in a table that still had room.
        if self.len * 2 >= self.slots.len() {
            self.rebuild_slots(self.slots.len() * 2);
        }
        false
    }

    /// Re-slots every stored key into a table of `new_cap` slots (a power of
    /// two ≥ the current size), using the remembered hashes.
    fn rebuild_slots(&mut self, new_cap: usize) {
        self.reslot_into(vec![0u32; new_cap]);
    }

    /// [`HashIndex::rebuild_slots`] over an already-allocated, all-zero table,
    /// so a caller that needs the allocation to be fallible can make it so and
    /// still share the re-slotting.
    fn reslot_into(&mut self, mut slots: Vec<u32>) {
        let mask = slots.len() - 1;
        for (k, &h) in self.hashes.iter().enumerate() {
            let mut i = (h as usize) & mask;
            while slots[i] != 0 {
                i = (i + 1) & mask;
            }
            // Lossless, and checked in exactly one place rather than two:
            // `k` indexes `self.hashes`, whose length is `self.len`, and
            // `insert_folded` refuses to grow past `u32::MAX` keys before a
            // hash is ever pushed. Re-checking here would suggest the bound
            // lives in two places; it does not.
            slots[i] = (k + 1) as u32;
        }
        self.slots = slots;
        self.mask = mask;
    }
}

impl std::fmt::Debug for HashIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HashIndex")
            .field("len", &self.len)
            .field("slots", &self.slots.len())
            .field("bytes", &self.blob.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_allocates_nothing_and_contains_nothing() {
        let idx = HashIndex::new();
        assert_eq!(idx.len, 0);
        assert_eq!(idx.slots.capacity(), 0);
        assert_eq!(idx.blob.capacity(), 0);
        assert!(!idx.contains_folded(b""));
        assert!(!idx.contains_folded(b"anything"));
    }

    #[test]
    fn insert_reports_prior_presence() {
        let mut idx = HashIndex::new();
        assert!(!idx.insert_folded(b"test"));
        assert!(idx.insert_folded(b"test"));
        assert!(idx.contains_folded(b"test"));
        assert_eq!(idx.len, 1, "a duplicate stores nothing");
    }

    #[test]
    fn the_empty_key_is_a_key_like_any_other() {
        let mut idx = HashIndex::new();
        assert!(!idx.contains_folded(b""));
        assert!(!idx.insert_folded(b""));
        assert!(idx.insert_folded(b""));
        assert!(idx.contains_folded(b""));
        assert!(!idx.contains_folded(b"x"));
    }

    #[test]
    fn prefixes_of_stored_keys_are_not_members() {
        // The length-seeded hash and the byte comparison must both keep
        // "ab" and "abc" apart — the same property the trie's is_word flag
        // provides structurally.
        let mut idx = HashIndex::new();
        idx.insert_folded(b"abc");
        assert!(idx.contains_folded(b"abc"));
        assert!(!idx.contains_folded(b"ab"));
        assert!(!idx.contains_folded(b"abcd"));
    }

    #[test]
    fn growth_preserves_every_key() {
        // Far past several load-factor doublings, every inserted key must
        // still probe to its slot and every absent key must still miss.
        let mut idx = HashIndex::new();
        let keys: Vec<String> = (0..2_000).map(|i| format!("key-{i}")).collect();
        for k in &keys {
            assert!(!idx.insert_folded(k.as_bytes()));
        }
        assert_eq!(idx.len, keys.len());
        for k in &keys {
            assert!(idx.contains_folded(k.as_bytes()), "{k} lost in growth");
            assert!(idx.insert_folded(k.as_bytes()), "{k} re-insertable");
        }
        for i in 2_000..2_100 {
            assert!(!idx.contains_folded(format!("key-{i}").as_bytes()));
        }
    }

    #[test]
    fn reserve_pre_sizes_without_changing_answers() {
        let mut reserved = HashIndex::new();
        reserved.reserve(500, 500 * 8);
        let slots_before = reserved.slots.len();
        let mut plain = HashIndex::new();
        for i in 0..500 {
            let k = format!("{i:08}");
            assert_eq!(
                reserved.insert_folded(k.as_bytes()),
                plain.insert_folded(k.as_bytes())
            );
        }
        assert_eq!(
            reserved.slots.len(),
            slots_before,
            "a covered load must not rehash"
        );
        for i in 0..600 {
            let k = format!("{i:08}");
            assert_eq!(
                reserved.contains_folded(k.as_bytes()),
                plain.contains_folded(k.as_bytes())
            );
        }
    }

    #[test]
    fn non_ascii_keys_round_trip() {
        let mut idx = HashIndex::new();
        for w in ["über", "straße", "日本語", "😀👍", "i\u{307}"] {
            assert!(!idx.insert_folded(w.as_bytes()));
        }
        for w in ["über", "straße", "日本語", "😀👍", "i\u{307}"] {
            assert!(idx.contains_folded(w.as_bytes()));
        }
        assert!(!idx.contains_folded("uber".as_bytes()));
        assert!(!idx.contains_folded("😀".as_bytes()));
    }
}
