//! An insertion-ordered string map that enumerates like a reference object.
//!
//! `Classifier.textToFeatures` builds its feature vector with
//! `for (const feature in this.features)`, so the **order in which the reference
//! enumerates own properties is the layout of the feature vector**, and every
//! model trained on it. That order is not insertion order and not lexicographic:
//!
//! > Integer-index keys first, in ascending numeric order; then every other key
//! > in insertion order.
//!
//! The consequence is a genuine bug that a port must reproduce. Add the token
//! `"99"` to a trained classifier and it does not land at the end of the feature
//! vector — it lands at the *front*, shifting every previously learned index by
//! one and silently corrupting the model. The reference's own recorded output
//! shows `classify(['alpha'])` flipping from `'A'` to `'B'` for exactly this
//! reason. A port with a stable insertion-ordered map produces a *correct*
//! model, and therefore the wrong answer.
//!
//! An "integer index" is the canonical decimal spelling of an integer in
//! `0..=2^32-2`. `"0"`, `"42"` and `"4294967294"` qualify; `"01"`, `"-1"`,
//! `"1.5"`, `"1e3"`, `"4294967295"`, `" 1"` and the fullwidth `"０"` do not.

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

/// A string-keyed map with the reference's own-property enumeration order.
///
/// Cloning is deliberately cheap in behaviour, not in bytes: these maps are
/// small (one entry per distinct token) and are cloned only at snapshot points.
#[derive(Debug, Clone, Default)]
pub struct OrderedMap<V> {
    /// Entries in insertion order. Deletion preserves the order of the rest.
    entries: Vec<(String, V)>,
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

    /// Inserts or overwrites, keeping the original position on overwrite —
    /// which is what assigning to an existing the reference property does.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        if let Some(&i) = self.index.get(&key) {
            self.entries[i].1 = value;
        } else {
            self.index.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
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
        Some(v)
    }

    /// Entries in insertion order — *not* the reference enumeration order.
    ///
    /// Useful when the caller only needs the contents; anything whose output
    /// order is observable must use [`Self::enumeration_order`].
    pub fn iter_insertion(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Keys in the reference's own-property enumeration order.
    ///
    /// Recomputed on every call, exactly as `for...in` is: caching the order
    /// across mutations is what makes a naive port produce a *stable* feature
    /// layout, which is precisely the behaviour the reference does not have.
    pub fn enumeration_order(&self) -> Vec<&str> {
        let mut indices: Vec<(u32, &str)> = Vec::new();
        let mut rest: Vec<&str> = Vec::new();
        for (k, _) in &self.entries {
            if is_array_index(k) {
                indices.push((k.parse().expect("checked by is_array_index"), k.as_str()));
            } else {
                rest.push(k.as_str());
            }
        }
        indices.sort_unstable_by_key(|(n, _)| *n);
        let mut out: Vec<&str> = indices.into_iter().map(|(_, k)| k).collect();
        out.extend(rest);
        out
    }

    /// Entries in the reference's own-property enumeration order.
    pub fn ordered_entries(&self) -> Vec<(&str, &V)> {
        self.enumeration_order()
            .into_iter()
            .map(|k| (k, self.get(k).expect("key came from this map")))
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
        // Recorded from the reference engine: addDocument('zebra 42 apple 7') yields this order.
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
