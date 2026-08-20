use rustc_hash::FxHashMap;

/// A string-keyed map that iterates in insertion order.
///
/// The order is the contract, not an implementation detail: a key takes the
/// next free slot the first time it is inserted and keeps that slot for as long
/// as it is present. Overwriting an existing key writes through to its value
/// and leaves its slot alone. The only operation that moves a slot is
/// [`Self::remove`], which closes the gap it leaves — see there.
///
/// [`Classifier`](crate::Classifier) is why the order is written down. A
/// feature's identity in an observation vector *is* its slot in this map, and a
/// trained model's weights are indexed by that slot, so the order has to be one
/// that a widening vocabulary cannot disturb. Under this contract, adding the
/// token `"99"` to a trained classifier appends it after every feature already
/// known and leaves every previously learned index exactly where it was; the
/// model stays valid, and only the new slot is untrained.
///
/// # History
///
/// Verbora's classifiers began as a port, and this type used to reproduce the
/// source runtime's own-property enumeration order: integer-index keys first in
/// ascending numeric order, then everything else in insertion order. That is a
/// defect, not a feature — under it, adding `"99"` to a trained classifier
/// hoisted it to slot 0 and shifted every learned index by one, silently
/// scrambling the model. Verbora specifies insertion order instead.
///
/// Models saved before the change are unaffected, and no compatibility stamp
/// bump was needed: they were serialised in the order their own slots were in,
/// and a restore replays that order as insertion order, which reproduces the
/// same slots. See [`SCHEMA`](crate::stamp::SCHEMA).
///
/// Cloning is deliberately cheap in behaviour, not in bytes: these maps are
/// small (one entry per distinct token) and are cloned only at snapshot points.
#[derive(Debug, Clone, Default)]
pub struct OrderedMap<V> {
    /// Entries in insertion order. `entries[slot]` is the entry at `slot`.
    entries: Vec<(String, V)>,
    /// Key to slot. Kept in step with `entries` by every mutation, which is
    /// what makes [`Self::slot_of`] a hash lookup rather than a scan — the
    /// question a feature vector asks once per *token*, not once per feature.
    index: FxHashMap<String, usize>,
}

impl<V> OrderedMap<V> {
    /// An empty map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
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

    /// Inserts or overwrites, keeping the original slot on overwrite.
    ///
    /// A new key is appended, so it takes the next slot and disturbs nothing
    /// already in the map.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        if let Some(&i) = self.index.get(&key) {
            self.entries[i].1 = value;
        } else {
            self.index.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
        }
    }

    /// Removes the entry, leaving the rest in order.
    ///
    /// This is the one mutation that moves slots: closing the gap shifts every
    /// key after the removed one down by one. A caller holding slot indices
    /// derived from this map — a trained model, above all — must treat them as
    /// invalidated past the removal point.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let i = self.index.remove(key)?;
        let (_, v) = self.entries.remove(i);
        for slot in self.index.values_mut() {
            if *slot > i {
                *slot -= 1;
            }
        }
        Some(v)
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Keys in insertion order — slot 0 first.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// The slot `key` occupies, if present.
    ///
    /// The inverse of [`Self::keys`], and the direction a feature vector
    /// actually asks in: a 0/1 observation is "slot *s* is set iff the document
    /// holds the key at *s*", which a caller answers with one lookup per token
    /// instead of one membership test per key.
    pub fn slot_of(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
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

    fn keys<V>(m: &OrderedMap<V>) -> Vec<&str> {
        m.keys().collect()
    }

    /// The rule the whole type exists to state: a new key goes at the end,
    /// whatever it is spelled like.
    ///
    /// `"42"` and `"7"` are the keys a reference-order map would have hoisted
    /// ahead of `"zebra"`, in ascending numeric order. They do not move here.
    #[test]
    fn integer_like_keys_take_the_next_slot_like_any_other() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        m.insert("zebra", 1);
        m.insert("42", 1);
        m.insert("appl", 1);
        m.insert("7", 1);
        assert_eq!(keys(&m), vec!["zebra", "42", "appl", "7"]);
        for (slot, key) in keys(&m).iter().enumerate() {
            assert_eq!(m.slot_of(key), Some(slot), "{key}");
        }
    }

    /// Adding a key — integer-like or not — must not move any slot already
    /// handed out. This is the invariant a trained model depends on.
    #[test]
    fn adding_a_key_never_moves_an_existing_slot() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for k in ["alpha", "beta", "gamma"] {
            m.insert(k, 1);
        }
        let before: Vec<(String, usize)> = m
            .keys()
            .map(|k| (k.to_owned(), m.slot_of(k).expect("present")))
            .collect();
        for k in ["99", "0", "4294967294", "delta"] {
            m.insert(k, 1);
            for (key, slot) in &before {
                assert_eq!(m.slot_of(key), Some(*slot), "{key} moved when {k} arrived");
            }
        }
        assert_eq!(
            keys(&m),
            vec!["alpha", "beta", "gamma", "99", "0", "4294967294", "delta"]
        );
    }

    #[test]
    fn insertion_order_is_kept() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for k in ["c", "a", "b"] {
            m.insert(k, 0);
        }
        assert_eq!(keys(&m), vec!["c", "a", "b"]);
    }

    #[test]
    fn overwrite_keeps_position() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        m.insert("a", 1);
        m.insert("b", 2);
        m.insert("a", 3);
        assert_eq!(keys(&m), vec!["a", "b"]);
        assert_eq!(m.slot_of("a"), Some(0));
        assert_eq!(m.get("a"), Some(&3));
    }

    #[test]
    fn slot_of_is_the_inverse_of_keys() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for k in ["zebra", "42", "appl", "7", "0"] {
            m.insert(k, 1);
        }
        let order = keys(&m);
        assert_eq!(order, vec!["zebra", "42", "appl", "7", "0"]);
        for (slot, key) in order.iter().enumerate() {
            assert_eq!(m.slot_of(key), Some(slot), "{key}");
        }
        assert_eq!(m.slot_of("absent"), None);
    }

    /// Removal closes the gap, so slots *after* the hole move down by one and
    /// slots before it do not.
    #[test]
    fn removal_preserves_the_rest_and_shifts_only_what_followed() {
        let mut m: OrderedMap<u32> = OrderedMap::new();
        for (i, k) in ["a", "b", "c", "d"].iter().enumerate() {
            m.insert(*k, i as u32);
        }
        assert_eq!(m.remove("b"), Some(1));
        assert_eq!(keys(&m), vec!["a", "c", "d"]);
        assert_eq!(m.slot_of("a"), Some(0));
        assert_eq!(m.slot_of("c"), Some(1));
        assert_eq!(m.slot_of("d"), Some(2));
        assert_eq!(m.slot_of("b"), None);
        assert_eq!(m.get("d"), Some(&3));
        assert_eq!(m.remove("b"), None);
    }

    #[test]
    fn iter_yields_entries_in_slot_order() {
        let m: OrderedMap<u32> = [("b".to_owned(), 2u32), ("a".to_owned(), 1)]
            .into_iter()
            .collect();
        assert_eq!(m.iter().collect::<Vec<_>>(), vec![("b", &2), ("a", &1)]);
        assert_eq!(m.len(), 2);
        assert!(!m.is_empty());
        assert!(m.contains_key("a"));
    }
}
