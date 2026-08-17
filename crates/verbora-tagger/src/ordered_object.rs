//! A plain the reference object, for the places where key order reaches the output.
//!
//! `Corpus.analyse` builds `tagFrequencies` and `posTags` as plain objects, and
//! `Corpus.buildLexicon` then iterates `Object.keys(tagFrequencies)` to decide
//! the order of its `addWord` calls. That order is **not** insertion order:
//! The reference engine enumerates array-index-like keys first, in ascending numeric order, and
//! only then the rest in insertion order. A corpus containing the tokens
//! `2, a, a, a, 1` enumerates as `["1", "2", "a"]`.
//!
//! Neither `HashMap` (arbitrary) nor a `Vec` of pairs (pure insertion order)
//! reproduces that, so this type keeps insertion order and applies the hoisting
//! rule when the keys are read out.

use rustc_hash::FxHashMap;

/// Whether `key` is a reference language array index: the canonical decimal form of an
/// integer in `0 ..= 2^32 - 2`. `"01"`, `"-1"` and `"1.0"` are not.
#[must_use]
pub fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    (n < u64::from(u32::MAX)).then_some(n as u32)
}

/// An insertion-ordered string map that enumerates like a reference object.
#[derive(Debug, Clone)]
pub struct OrderedObject<V> {
    entries: Vec<(Box<str>, V)>,
    index: FxHashMap<Box<str>, usize>,
}

impl<V> Default for OrderedObject<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
        }
    }
}

impl<V> OrderedObject<V> {
    /// An empty object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value for `key`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|i| &self.entries[*i].1)
    }

    /// Mutable access to the value for `key`.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        let i = *self.index.get(key)?;
        Some(&mut self.entries[i].1)
    }

    /// Inserts or replaces. An existing key keeps its position.
    pub fn insert(&mut self, key: &str, value: V) {
        if let Some(i) = self.index.get(key) {
            self.entries[*i].1 = value;
            return;
        }
        self.index.insert(key.into(), self.entries.len());
        self.entries.push((key.into(), value));
    }

    /// Returns the value for `key`, inserting `default` first if it is absent.
    pub fn entry_or_insert(&mut self, key: &str, default: V) -> &mut V {
        if !self.index.contains_key(key) {
            self.insert(key, default);
        }
        self.get_mut(key).expect("just inserted")
    }

    /// Number of keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries in insertion order, ignoring the hoisting rule.
    pub fn in_insertion_order(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (&**k, v))
    }

    /// The entries in `Object.keys` order: array indices first, ascending, then
    /// the rest in insertion order.
    #[must_use]
    pub fn in_key_order(&self) -> Vec<(&str, &V)> {
        let mut numeric: Vec<(u32, usize)> = Vec::new();
        let mut plain: Vec<usize> = Vec::new();
        for (i, (k, _)) in self.entries.iter().enumerate() {
            match array_index(k) {
                Some(n) => numeric.push((n, i)),
                None => plain.push(i),
            }
        }
        // Sorted, not stable-sorted-by-insertion: numeric order wins outright,
        // and duplicate keys cannot occur.
        numeric.sort_unstable();
        numeric
            .into_iter()
            .map(|(_, i)| i)
            .chain(plain)
            .map(|i| {
                let (k, v) = &self.entries[i];
                (&**k, v)
            })
            .collect()
    }

    /// The keys in `Object.keys` order.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.in_key_order().into_iter().map(|(k, _)| k).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_index_recognition() {
        assert_eq!(array_index("0"), Some(0));
        assert_eq!(array_index("2000"), Some(2000));
        assert_eq!(array_index("01"), None);
        assert_eq!(array_index(""), None);
        assert_eq!(array_index("-1"), None);
        assert_eq!(array_index("1.0"), None);
        assert_eq!(array_index("4294967294"), Some(4_294_967_294));
        assert_eq!(array_index("4294967295"), None, "2^32-1 is not an index");
        assert_eq!(array_index("99999999999"), None);
    }

    #[test]
    fn numeric_keys_are_hoisted_and_sorted() {
        let mut o = OrderedObject::new();
        for k in ["2", "a", "a", "1"] {
            o.insert(k, k.len());
        }
        assert_eq!(o.keys(), ["1", "2", "a"]);
        let mut o = OrderedObject::new();
        for k in ["10", "9", "b", "0", "a"] {
            o.insert(k, 0);
        }
        assert_eq!(o.keys(), ["0", "9", "10", "b", "a"]);
    }

    #[test]
    fn reinsertion_keeps_position() {
        let mut o = OrderedObject::new();
        o.insert("a", 1);
        o.insert("b", 2);
        o.insert("a", 3);
        assert_eq!(o.keys(), ["a", "b"]);
        assert_eq!(o.get("a"), Some(&3));
    }
}
