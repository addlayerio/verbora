//! A hash membership index over the stored (already-folded) words, so that
//! `contains` costs one hash + one probe chain instead of one dependent arena
//! hop per UTF-16 code unit.
//!
//! # Why this exists
//!
//! `contains` on the arena is depth-bound: ~8.5 dependent cache misses over a
//! multi-megabyte arena for a natural-language word, and no layout change
//! recovers that (an SoA/CSR child layout and an ASCII byte path were both
//! measured at zero gain — the cost is the dependent misses, not decoding).
//! A flat membership set answers the same question in one hash of the query
//! bytes plus a short linear probe, roughly halving `contains` latency on the
//! benchmark corpus. The trade is paid at build time: maintaining the index
//! adds ~a third to a bulk load, a cost disclosed on
//! [`Trie::add_string`](crate::Trie::add_string).
//!
//! # Why it is exact
//!
//! `Trie::contains(s)` walks the UTF-16 units of the *folded* query and asks
//! whether the landing node is a word. That is precisely "is `fold(s)` one of
//! the folded strings that were stored": `encode_utf16` is injective on
//! well-formed strings, so unit-sequence equality and byte equality coincide,
//! and every stored word entered the trie already folded. Storing the folded
//! bytes in a set therefore answers `contains` identically — differentially
//! verified against the walk in this crate's tests, including folding,
//! surrogate pairs, duplicates, and the empty string.
//!
//! # Design
//!
//! Dependency-free and fully safe: an open-addressed, linear-probing table
//! (load factor ≤ ½) over an append-only byte blob holding every key
//! back-to-back. Keys are never removed — the trie has no removal either —
//! which is what lets the blob be append-only and the table insert-only. The
//! hash is an FxHash-style multiply-and-rotate over 8-byte SWAR chunks; no
//! external hasher crate is pulled in for one internal set.

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
    /// `slots.len() - 1`; `slots.len()` is always a power of two (or zero
    /// before the first insert — the empty state allocates nothing, so a
    /// fresh [`Trie`](crate::Trie) stays two allocations, not three).
    mask: usize,
    slots: Vec<u32>,
    hashes: Vec<u64>,
    blob: Vec<u8>,
    /// `len + 1` entries once non-empty: `offs[0] == 0`, then one end offset
    /// per key.
    offs: Vec<u32>,
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
    pub(crate) fn reserve(&mut self, keys: usize, bytes: usize) {
        let want = ((self.len + keys) * 2 + 1).next_power_of_two().max(64);
        if want > self.slots.len() {
            self.rebuild_slots(want);
        }
        self.hashes.reserve(keys);
        self.blob.reserve(bytes);
        if self.offs.is_empty() {
            self.offs.reserve(keys + 1);
            self.offs.push(0);
        } else {
            self.offs.reserve(keys);
        }
    }

    /// Bytes of key `k`.
    #[inline]
    fn key_bytes(&self, k: usize) -> &[u8] {
        &self.blob[self.offs[k] as usize..self.offs[k + 1] as usize]
    }

    /// Whether `b` — which must already be folded — is a stored key.
    #[inline]
    pub(crate) fn contains_folded(&self, b: &[u8]) -> bool {
        if self.len == 0 {
            return false;
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
    /// was **already** present, matching
    /// [`Trie::add_string`](crate::Trie::add_string)'s convention.
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
        self.blob.extend_from_slice(b);
        self.offs
            .push(u32::try_from(self.blob.len()).expect("membership blob exceeds 4 GiB"));
        self.hashes.push(h);
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
        let mask = new_cap - 1;
        let mut slots = vec![0u32; new_cap];
        for (k, &h) in self.hashes.iter().enumerate() {
            let mut i = (h as usize) & mask;
            while slots[i] != 0 {
                i = (i + 1) & mask;
            }
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
    fn insert_reports_prior_presence_like_add_string() {
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
