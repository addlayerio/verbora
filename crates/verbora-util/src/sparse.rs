//! A reference sparse array, keyed by [`Vertex`].
//!
//! `EdgeWeightedDigraph.adj`, `ShortestPathTree.distTo` and
//! `ShortestPathTree.edgeTo` are all `[]` in the reference, written at arbitrary
//! vertex keys. Three properties of that are observable and none of them come
//! free in Rust:
//!
//! 1. **`length` is one past the largest integer index ever written**, not the
//!    number of entries. `EdgeWeightedDigraph#v()` returns it verbatim, so a
//!    graph consisting of the single edge `10 -> 3` reports eleven vertices.
//!    Writing a *string* key does not move it, so an all-string graph reports
//!    zero.
//! 2. **`for…in` order is integer keys ascending, then string keys in insertion
//!    order.** `edges()` is a `for…in` over `adj`, so this decides the global
//!    edge order, which decides `toString()` and — through `Topological` — the
//!    order every path tree relaxes in.
//! 3. **Holes are not entries.** A `for…in` skips them.
//!
//! # Storage
//!
//! Integer keys live in a dense `Vec<Option<T>>` while they stay near the front,
//! and spill into a `BTreeMap` beyond that. The dense half is what real graphs
//! use — contiguous small vertex numbers — and keeps lookups O(1); the spill
//! half means `add(4_000_000_000, 1, w)` costs one map entry rather than a 32 GB
//! allocation. Both halves iterate in ascending key order, and the invariant
//! that every spilled key is `>= dense.len()` makes chaining them the complete
//! ascending sequence.

use std::collections::BTreeMap;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::vertex::Vertex;

/// How far past the end of the dense region a write may reach before it spills.
///
/// Generous enough that ordinary graphs — including ones numbered from a
/// non-zero base, or with gaps where vertices were skipped — stay dense, small
/// enough that a single distant index cannot allocate unboundedly.
const DENSE_SLACK: usize = 1024;

/// A reference array addressed by [`Vertex`] keys.
#[derive(Debug, Clone)]
pub(crate) struct SparseArray<T> {
    /// Integer keys `0..dense.len()`, holes included as `None`.
    dense: Vec<Option<T>>,
    /// Integer keys at or beyond `dense.len()`.
    spilled: BTreeMap<u32, T>,
    /// String keys, in insertion order.
    ///
    /// The key is an `Rc<str>` rather than a `Box<str>` so a newly-named slot
    /// costs one allocation instead of two: `named` and `named_pos` both need
    /// an owned copy of the key, and a reference-counted clone is a pointer
    /// bump where a `Box<str>` clone would copy the bytes again.
    named: Vec<(Rc<str>, T)>,
    /// Position of each string key within `named`.
    named_pos: FxHashMap<Rc<str>, usize>,
    /// `Array.prototype.length`: one past the largest integer key ever written.
    length: u64,
}

impl<T> Default for SparseArray<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SparseArray<T> {
    /// Creates an empty array.
    pub(crate) fn new() -> Self {
        Self {
            dense: Vec::new(),
            spilled: BTreeMap::new(),
            named: Vec::new(),
            named_pos: FxHashMap::default(),
            length: 0,
        }
    }

    /// `array.length`.
    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    /// The number of live entries, holes excluded.
    ///
    /// Not observable in the reference; used to bound the `pathTo` walk, which the
    /// reference leaves unbounded because a DAG guarantees termination.
    pub(crate) fn entry_count(&self) -> usize {
        self.dense.iter().filter(|slot| slot.is_some()).count()
            + self.spilled.len()
            + self.named.len()
    }

    /// Reads the slot addressed by `v`, or `None` for a hole or absent key.
    pub(crate) fn get(&self, v: &Vertex) -> Option<&T> {
        match v.array_index() {
            Some(i) => self.get_index(i),
            None => self.get_named(&v.property_key()),
        }
    }

    fn get_index(&self, i: u32) -> Option<&T> {
        match self.dense.get(i as usize) {
            Some(slot) => slot.as_ref(),
            None => self.spilled.get(&i),
        }
    }

    fn get_named(&self, key: &str) -> Option<&T> {
        let pos = *self.named_pos.get(key)?;
        self.named.get(pos).map(|(_, value)| value)
    }

    /// Writes the slot addressed by `v`, extending `length` if `v` is an index.
    pub(crate) fn set(&mut self, v: &Vertex, value: T) {
        match v.array_index() {
            Some(i) => {
                self.length = self.length.max(u64::from(i) + 1);
                if self.grow_dense_to(i) {
                    self.dense[i as usize] = Some(value);
                } else {
                    self.spilled.insert(i, value);
                }
            }
            None => {
                let key = v.property_key();
                if let Some(&pos) = self.named_pos.get(key.as_ref()) {
                    self.named[pos].1 = value;
                } else {
                    let rc: Rc<str> = Rc::from(key.into_owned());
                    self.named_pos.insert(Rc::clone(&rc), self.named.len());
                    self.named.push((rc, value));
                }
            }
        }
    }

    /// Returns the slot addressed by `v`, inserting `make()` first if it is
    /// empty. Mirrors `if (!a[k]) a[k] = new Bag()` followed by a use of `a[k]`.
    pub(crate) fn get_or_insert_with(&mut self, v: &Vertex, make: impl FnOnce() -> T) -> &mut T {
        if self.get(v).is_none() {
            self.set(v, make());
        }
        self.get_mut(v).expect("just inserted")
    }

    fn get_mut(&mut self, v: &Vertex) -> Option<&mut T> {
        match v.array_index() {
            Some(i) => match self.dense.get_mut(i as usize) {
                Some(slot) => slot.as_mut(),
                None => self.spilled.get_mut(&i),
            },
            None => {
                let pos = *self.named_pos.get(v.property_key().as_ref())?;
                self.named.get_mut(pos).map(|(_, value)| value)
            }
        }
    }

    /// Extends the dense region to cover `i`, returning whether it now does.
    ///
    /// Any spilled entries the extension swallows are migrated, which preserves
    /// the invariant that every spilled key is at or beyond `dense.len()` — and
    /// with it the correctness of both `get` and the iteration order.
    fn grow_dense_to(&mut self, i: u32) -> bool {
        let i = i as usize;
        if i < self.dense.len() {
            return true;
        }
        if i > self.dense.len() + DENSE_SLACK {
            return false;
        }
        self.dense.resize_with(i + 1, || None);
        if !self.spilled.is_empty() {
            let beyond = self.spilled.split_off(&(i as u32 + 1));
            let swallowed = std::mem::replace(&mut self.spilled, beyond);
            for (k, value) in swallowed {
                self.dense[k as usize] = Some(value);
            }
        }
        true
    }

    /// Iterates the live values in `for…in` order: integer keys ascending, then
    /// string keys in insertion order.
    pub(crate) fn iter_for_in(&self) -> impl Iterator<Item = &T> {
        self.dense
            .iter()
            .filter_map(Option::as_ref)
            .chain(self.spilled.values())
            .chain(self.named.iter().map(|(_, value)| value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: i32) -> Vertex {
        Vertex::from(n)
    }

    #[test]
    fn length_tracks_the_largest_integer_index() {
        let mut a = SparseArray::new();
        assert_eq!(a.length(), 0);
        a.set(&v(10), "x");
        assert_eq!(a.length(), 11);
        a.set(&v(3), "y");
        assert_eq!(a.length(), 11);
        // A string key does not move `length` — which is why an all-string
        // graph reports v() == 0.
        a.set(&Vertex::from("z"), "z");
        assert_eq!(a.length(), 11);
        // Nor does a fractional or negative one.
        a.set(&Vertex::Num(1.5), "f");
        a.set(&Vertex::Num(-1.0), "n");
        assert_eq!(a.length(), 11);
    }

    #[test]
    fn number_and_numeric_string_share_a_slot() {
        let mut a = SparseArray::new();
        a.set(&v(1), "num");
        assert_eq!(a.get(&Vertex::from("1")), Some(&"num"));
        a.set(&Vertex::from("1"), "str");
        assert_eq!(a.get(&v(1)), Some(&"str"));
    }

    #[test]
    fn for_in_order_hoists_integer_keys() {
        let mut a = SparseArray::new();
        a.set(&Vertex::from("b"), "b");
        a.set(&v(10), "ten");
        a.set(&Vertex::from("a"), "a");
        a.set(&v(2), "two");
        a.set(&Vertex::Num(1.5), "one-point-five");
        let seen: Vec<_> = a.iter_for_in().copied().collect();
        assert_eq!(seen, vec!["two", "ten", "b", "a", "one-point-five"]);
    }

    #[test]
    fn distant_index_spills_then_migrates() {
        let mut a = SparseArray::new();
        a.set(&v(0), "zero");
        // Far beyond the slack: spills.
        a.set(&Vertex::Num(1_000_000.0), "far");
        assert_eq!(a.get(&Vertex::Num(1_000_000.0)), Some(&"far"));
        assert_eq!(a.length(), 1_000_001);
        // Walking the dense region outward eventually swallows the spilled key;
        // it must still be found, exactly once, and in ascending order.
        for i in 1..=2000 {
            a.set(&Vertex::Num(f64::from(i)), "step");
        }
        assert_eq!(a.get(&Vertex::Num(1_000_000.0)), Some(&"far"));
        assert_eq!(a.entry_count(), 2002);
        let order: Vec<_> = a.iter_for_in().copied().collect();
        assert_eq!(order.first(), Some(&"zero"));
        assert_eq!(order.last(), Some(&"far"));
    }

    #[test]
    fn holes_are_skipped() {
        let mut a = SparseArray::new();
        a.set(&v(0), "a");
        a.set(&v(4), "b");
        assert_eq!(a.get(&v(2)), None);
        assert_eq!(a.iter_for_in().count(), 2);
        assert_eq!(a.entry_count(), 2);
    }

    #[test]
    fn nan_and_negative_zero_keys() {
        let mut a = SparseArray::new();
        // NaN is not an array index; its property key is "NaN".
        a.set(&Vertex::Num(f64::NAN), "nan");
        assert_eq!(a.get(&Vertex::Num(f64::NAN)), Some(&"nan"));
        assert_eq!(a.length(), 0);
        // -0 keys as "0", i.e. index 0.
        a.set(&Vertex::Num(-0.0), "zero");
        assert_eq!(a.get(&v(0)), Some(&"zero"));
        assert_eq!(a.length(), 1);
    }
}
