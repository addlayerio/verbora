use std::sync::OnceLock;

use rustc_hash::FxHashMap;

/// Whether `key` is an array index, and therefore hoisted to the front of
/// the reference's own-property enumeration.
///
/// The check is the canonical-spelling one: the key must round-trip through
/// `u32` with no leading zeros, no sign and no separators, and must be at most
/// `2^32 - 2` (`4294967295` is the array *length* limit, not a valid index).
pub fn is_array_index(key: &str) -> bool {
    if key.is_empty() || key.len() > 10 {
        return false;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // "0" is an index; "00" and "01" are not, because they are not canonical.
    if key.len() > 1 && key.starts_with('0') {
        return false;
    }
    key.parse::<u32>().is_ok_and(|n| n != u32::MAX)
}

/// The enumeration order of one particular entry set, in both directions.
///
/// Both vectors are indexed the same way round: `by_slot[slot]` is the
/// position in `entries` that the reference enumerates at `slot`, and
/// `slot_of[position]` is its inverse. The inverse is what makes
/// [`OrderedMap::slot_of`] a lookup rather than a scan, which is the whole
/// point of keeping it — a feature vector is built by asking "which slot does
/// this token occupy?" once per *token*, not by asking "does this document
/// contain this feature?" once per *feature*.
#[derive(Debug, Clone, Default)]
struct Enumeration {
    by_slot: Vec<usize>,
    slot_of: Vec<usize>,
}

/// A string-keyed map with the reference's own-property enumeration order.
///
/// An insertion-ordered string map that enumerates like a reference object.
///
/// `Classifier.textToFeatures` builds its feature vector with
/// `for (const feature in this.features)`, so the **order in which the reference
/// enumerates own properties is the layout of the feature vector**, and every
/// model trained on it. That order is not insertion order and not lexicographic:
///
/// > Integer-index keys first, in ascending numeric order; then every other key
/// > in insertion order.
///
/// The consequence is a genuine bug that a port must reproduce. Add the token
/// `"99"` to a trained classifier and it does not land at the end of the feature
/// vector — it lands at the *front*, shifting every previously learned index by
/// one and silently corrupting the model. The reference's own recorded output
/// shows `classify(['alpha'])` flipping from `'A'` to `'B'` for exactly this
/// reason. A port with a stable insertion-ordered map produces a *correct*
/// model, and therefore the wrong answer.
///
/// An "integer index" is the canonical decimal spelling of an integer in
/// `0..=2^32-2`. `"0"`, `"42"` and `"4294967294"` qualify; `"01"`, `"-1"`,
/// `"1.5"`, `"1e3"`, `"4294967295"`, `" 1"` and the fullwidth `"０"` do not.
///
/// Cloning is deliberately cheap in behaviour, not in bytes: these maps are
/// small (one entry per distinct token) and are cloned only at snapshot points.
#[derive(Debug, Clone, Default)]
pub struct OrderedMap<V> {
    /// Entries in insertion order. Deletion preserves the order of the rest.
    entries: Vec<(String, V)>,
    index: FxHashMap<String, usize>,
    /// The enumeration order of the current `entries`, computed on first
    /// demand and dropped by every mutation that can reorder it.
    ///
    /// This is a memo, **not** a stable layout: `for...in` recomputes the
    /// order on every evaluation, and the whole point of [`OrderedMap`](crate::OrderedMap) is
    /// that adding an integer-like key reshuffles it. The memo is therefore
    /// invalidated by every structural change (a new key, a deletion) and
    /// only survives across calls that could not have changed the answer —
    /// which is exactly the situation `text_to_features` and `classify` are
    /// in, and where recomputing an O(n) partition-and-sort per call was pure
    /// waste. It is a `OnceLock` rather than a `Cell` because a trained
    /// classifier is shared across threads by `par_classify_batch`.
    order: OnceLock<Enumeration>,
}

impl<V> OrderedMap<V> {
    /// An empty map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
            order: OnceLock::new(),
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// A mutable reference to the value for `key`, if present.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        let i = *self.index.get(key)?;
        Some(&mut self.entries[i].1)
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// Inserts or overwrites, keeping the original position on overwrite —
    /// which is what assigning to an existing the reference property does.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        if let Some(&i) = self.index.get(&key) {
            // Overwriting keeps the position, so the enumeration is untouched
            // and the memo stays valid.
            self.entries[i].1 = value;
        } else {
            self.index.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
            self.order.take();
        }
    }

    /// `delete obj[key]`: removes the entry, leaving the rest in order.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let i = self.index.remove(key)?;
        let (_, v) = self.entries.remove(i);
        for slot in self.index.values_mut() {
            if *slot > i {
                *slot -= 1;
            }
        }
        self.order.take();
        Some(v)
    }

    /// Entries in insertion order — *not* the reference enumeration order.
    ///
    /// Useful when the caller only needs the contents; anything whose output
    /// order is observable must use [`Self::enumeration_order`].
    pub fn iter_insertion(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The enumeration of the current entries, computed once per entry set.
    ///
    /// Every mutation that can reorder the keys clears the memo, so this is
    /// never stale: it answers what a fresh `for...in` would answer *now*.
    fn enumeration(&self) -> &Enumeration {
        self.order.get_or_init(|| {
            let mut indices: Vec<(u32, usize)> = Vec::new();
            let mut rest: Vec<usize> = Vec::new();
            for (pos, (k, _)) in self.entries.iter().enumerate() {
                if is_array_index(k) {
                    indices.push((k.parse().expect("checked by is_array_index"), pos));
                } else {
                    rest.push(pos);
                }
            }
            indices.sort_unstable_by_key(|(n, _)| *n);
            let mut by_slot: Vec<usize> = indices.into_iter().map(|(_, pos)| pos).collect();
            by_slot.extend(rest);
            let mut slot_of = vec![0usize; by_slot.len()];
            for (slot, &pos) in by_slot.iter().enumerate() {
                slot_of[pos] = slot;
            }
            Enumeration { by_slot, slot_of }
        })
    }

    /// Keys in the reference's own-property enumeration order.
    ///
    /// The order is that of a fresh `for...in` over the map as it stands:
    /// remembering an order *across mutations* is what makes a naive port
    /// produce a *stable* feature layout, which is precisely the behaviour the
    /// reference does not have. The memo behind it is discarded by every
    /// mutation that can reorder the keys, so it never outlives the entry set
    /// it describes.
    pub fn enumeration_order(&self) -> Vec<&str> {
        self.enumeration()
            .by_slot
            .iter()
            .map(|&pos| self.entries[pos].0.as_str())
            .collect()
    }

    /// The position `key` occupies in [`Self::enumeration_order`], if present.
    ///
    /// The inverse direction of the same question, and the one a feature
    /// vector actually asks: a 0/1 observation is "slot *s* is set iff the
    /// document holds the key enumerated at *s*", which a caller can answer
    /// with one lookup per token instead of one membership test per key.
    /// Integer-like keys are hoisted here exactly as they are there — that is
    /// the same enumeration, read the other way round.
    pub fn slot_of(&self, key: &str) -> Option<usize> {
        let pos = *self.index.get(key)?;
        Some(self.enumeration().slot_of[pos])
    }

    /// Entries in the reference's own-property enumeration order.
    pub fn ordered_entries(&self) -> Vec<(&str, &V)> {
        self.enumeration()
            .by_slot
            .iter()
            .map(|&pos| {
                let (k, v) = &self.entries[pos];
                (k.as_str(), v)
            })
            .collect()
    }
}

impl<V> FromIterator<(String, V)> for OrderedMap<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_index_recognition_is_canonical() {
        for k in ["0", "1", "2", "42", "4294967294"] {
            assert!(is_array_index(k), "{k} should be an index");
        }
        for k in [
            "",
            "b",
            "01",
            "00",
            "-1",
            "1.5",
            "1e3",
            "4294967295",
            "4294967296",
            "+1",
            " 1",
            "1 ",
            "٣",
            "０",
            "9007199254740993",
        ] {
            assert!(!is_array_index(k), "{k} should not be an index");
        }
    }

    #[test]
    fn enumeration_hoists_integer_keys() {
        // Derived from the enumeration rule this type documents, not recorded:
        // integer-index keys first in ascending numeric order, then the rest in
        // insertion order. Of the four keys inserted, "42" and "7" are integer
        // indices and sort 7 < 42; "zebra" and "appl" are not, and keep the
        // order they were inserted in.
        let mut m: OrderedMap<u32> = OrderedMap::new();
        m.insert("zebra", 1);
        m.insert("42", 1);
        m.insert("appl", 1);
        m.insert("7", 1);
        assert_eq!(m.enumeration_order(), vec!["7", "42", "zebra", "appl"]);
    }

    #[test]
    fn insertion_order_is_kept_for_non_indices() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for k in ["c", "a", "b"] {
            m.insert(k, 0);
        }
        assert_eq!(m.enumeration_order(), vec!["c", "a", "b"]);
    }

    #[test]
    fn overwrite_keeps_position() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        m.insert("a", 1);
        m.insert("b", 2);
        m.insert("a", 3);
        assert_eq!(m.enumeration_order(), vec!["a", "b"]);
        assert_eq!(m.get("a"), Some(&3));
    }

    #[test]
    fn slot_of_is_the_inverse_of_enumeration_order() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for k in ["zebra", "42", "appl", "7", "0"] {
            m.insert(k, 1);
        }
        let order = m.enumeration_order();
        assert_eq!(order, vec!["0", "7", "42", "zebra", "appl"]);
        for (slot, key) in order.iter().enumerate() {
            assert_eq!(m.slot_of(key), Some(slot), "{key}");
        }
        assert_eq!(m.slot_of("absent"), None);
    }

    /// The memo may not survive anything that can reorder the keys — that is
    /// the whole quirk `textToFeatures` inherits from `for...in`.
    #[test]
    fn the_memo_is_dropped_by_every_reordering_mutation() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        m.insert("alpha", 1);
        m.insert("beta", 1);
        // Warm the memo before each mutation, so a stale one would show.
        assert_eq!(m.enumeration_order(), vec!["alpha", "beta"]);
        assert_eq!(m.slot_of("alpha"), Some(0));

        m.insert("99", 1);
        assert_eq!(m.enumeration_order(), vec!["99", "alpha", "beta"]);
        assert_eq!(m.slot_of("alpha"), Some(1));

        // An overwrite keeps the position: the memo is still correct, and the
        // answer must not change.
        m.insert("alpha", 7);
        assert_eq!(m.enumeration_order(), vec!["99", "alpha", "beta"]);
        assert_eq!(m.slot_of("alpha"), Some(1));
        assert_eq!(m.get("alpha"), Some(&7));

        m.remove("99");
        assert_eq!(m.enumeration_order(), vec!["alpha", "beta"]);
        assert_eq!(m.slot_of("alpha"), Some(0));
        assert_eq!(m.slot_of("99"), None);
    }

    #[test]
    fn removal_preserves_the_rest() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for (i, k) in ["a", "b", "c", "d"].iter().enumerate() {
            m.insert(*k, i as u32);
        }
        assert_eq!(m.remove("b"), Some(1));
        assert_eq!(m.enumeration_order(), vec!["a", "c", "d"]);
        assert_eq!(m.get("d"), Some(&3));
        assert_eq!(m.remove("b"), None);
    }
}
