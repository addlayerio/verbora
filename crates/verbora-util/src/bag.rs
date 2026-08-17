//! An append-only collection, ported from the reference `bag`.
//!
//! `Bag` is not on the reference's public surface — `util/index` does not
//! re-export it — but `index.d.ts` declares it and `EdgeWeightedDigraph` stores
//! every adjacency list in one, so its behaviour reaches the outside through
//! `getAdj` and `edges`. It is reproduced here rather than replaced by a `Vec`
//! because one of its methods is *backwards*, and code that reads
//! `bag.isEmpty()` has to keep getting the backwards answer.

/// An ordered, append-only collection.
///
/// # This type reproduces a bug on purpose
///
/// [`Bag::is_empty`] returns `true` when the bag is **not** empty. The reference
/// implements it as `return this.nElement > 0`, so `new Bag().isEmpty()` is
/// `false` and one `add` makes it `true`. Nothing in the reference calls it, which
/// is why the bug survived; it is preserved because the method is public and a
/// port that quietly inverts it is no longer the same library. Use
/// [`Bag::is_actually_empty`] when you want the ordinary meaning.
#[derive(Debug, Clone, Default)]
pub struct Bag<T> {
    dictionary: Vec<T>,
    n_element: usize,
}

impl<T> Bag<T> {
    /// Creates an empty bag.
    pub fn new() -> Self {
        Self {
            dictionary: Vec::new(),
            n_element: 0,
        }
    }

    /// Creates an empty bag with room for `capacity` elements.
    ///
    /// No counterpart in the reference — the reference arrays grow implicitly — but it
    /// lets a caller that knows the edge count avoid the reallocation chain.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            dictionary: Vec::with_capacity(capacity),
            n_element: 0,
        }
    }

    /// Appends `element`, returning `&mut self` so calls chain as they do in
    /// the reference (`bag.add(a).add(b)`).
    pub fn add(&mut self, element: T) -> &mut Self {
        self.dictionary.push(element);
        self.n_element += 1;
        self
    }

    /// **Returns `true` when the bag is NOT empty.**
    ///
    /// See the type-level documentation: this is `nElement > 0` in the
    /// reference, and the inversion is deliberate.
    #[doc(alias = "isEmpty")]
    pub fn is_empty(&self) -> bool {
        self.n_element > 0
    }

    /// Returns whether the bag really is empty.
    ///
    /// Not part of the reference surface; provided so Rust callers have a
    /// method that means what its name says.
    pub fn is_actually_empty(&self) -> bool {
        self.dictionary.is_empty()
    }

    /// The number of elements added, mirroring the reference's `nElement`.
    pub fn len(&self) -> usize {
        self.n_element
    }

    /// The elements, in insertion order.
    ///
    /// The reference's `unpack()` returns a fresh shallow copy each call; a shared
    /// slice is observationally identical for a bag nothing else can mutate, and
    /// costs nothing. [`Bag::unpack`] is the literal port.
    pub fn items(&self) -> &[T] {
        &self.dictionary
    }

    /// Iterates the elements in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.dictionary.iter()
    }
}

impl<T: PartialEq> Bag<T> {
    /// Returns whether the bag contains `item`.
    ///
    /// The reference uses `indexOf(item) >= 0`, i.e. strict equality — so a
    /// `NaN` element is never found, matching `PartialEq` for `f64` here.
    pub fn contains(&self, item: &T) -> bool {
        self.dictionary.iter().any(|e| e == item)
    }
}

impl<T: Clone> Bag<T> {
    /// Returns a fresh copy of the elements, in insertion order.
    ///
    /// The literal port of `unpack()`. Prefer [`Bag::items`] unless you need the
    /// copy.
    pub fn unpack(&self) -> Vec<T> {
        self.dictionary.clone()
    }
}

impl<'a, T> IntoIterator for &'a Bag<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_empty_is_inverted() {
        // A naive port would have written `self.dictionary.is_empty()`, which
        // gives the opposite answer on both lines below.
        let mut bag: Bag<i32> = Bag::new();
        assert!(!bag.is_empty());
        assert!(bag.is_actually_empty());
        bag.add(1);
        assert!(bag.is_empty());
        assert!(!bag.is_actually_empty());
    }

    #[test]
    fn add_chains_and_counts() {
        let mut bag = Bag::new();
        bag.add("x").add("x").add("y");
        assert_eq!(bag.len(), 3);
        assert_eq!(bag.items(), &["x", "x", "y"]);
    }

    #[test]
    fn unpack_is_a_copy() {
        let mut bag = Bag::new();
        bag.add(1).add(2);
        let mut copy = bag.unpack();
        copy.push(3);
        assert_eq!(bag.items(), &[1, 2]);
        assert_eq!(copy, vec![1, 2, 3]);
    }

    #[test]
    fn contains_uses_strict_equality() {
        let mut bag = Bag::new();
        bag.add(f64::NAN);
        // `indexOf` never finds NaN, and neither does `==` on f64.
        assert!(!bag.contains(&f64::NAN));
        bag.add(0.0);
        assert!(bag.contains(&-0.0));
    }

    #[test]
    fn empty_bag_iterates_nothing() {
        let bag: Bag<u8> = Bag::new();
        assert_eq!(bag.iter().count(), 0);
        assert!(bag.unpack().is_empty());
    }
}
