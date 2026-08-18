//! Probabilistic spelling correction for Rust.
//!
//! ```
//! use verbora_spellcheck::Spellcheck;
//!
//! // The word list is a corpus: repeats are frequencies, and frequency is
//! // what orders the suggestions.
//! let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
//!
//! assert!(sc.is_correct("the"));
//! assert_eq!(sc.get_corrections("he", 1), ["the", "he", "she"]);
//! ```
//!
//! The algorithm is Norvig's: generate every string within `n` edits of the
//! input, discard the ones that are not words, and return what is left ordered
//! by corpus frequency — with the rule that a word reachable in `k` edits is
//! *infinitely* more probable than one needing `k + 1`.
//!
//! # What parity costs here
//!
//! Four properties of the reference are observable through its public API and
//! are reproduced exactly, none of which a fresh design would choose.
//!
//! ### 1. The order of the corrections is decided by an invalid comparator
//!
//! Each distance level is sorted with `(a, b) => a[1] > b[1] ? -1 : 1`, which
//! **never returns 0**. For finite frequencies that is harmless — TimSort only
//! ever tests a comparison against `< 0`, so the result is exactly a stable
//! descending sort — but a `NaN` frequency (which the reference really does
//! produce, see below) makes the relation non-transitive, and the output
//! becomes a property of the reference engine's specific sort algorithm. It is reproduced by
//! porting that algorithm; see `crate::comparator_sort`, validated against 1,015
//! permutations recorded from the reference engine itself.
//!
//! ### 2. The frequency table inherits from `Object.prototype`
//!
//! `word2frequency` is a plain `{}`. A word list containing `'constructor'`
//! once gives that word a frequency of `NaN`; containing it `n >= 2` times gives
//! `n - 1`, not `n`. `'__proto__'` never gets an own property at all. See
//! [`Spellcheck::frequency`].
//!
//! ### 3. Editing is UTF-16, so it can cut a character in half
//!
//! `edits` slices by code unit, so an astral character is edited as two
//! independent surrogates and 83 of the 238 candidates for `"a😀b"` are not
//! valid text. They can never be corrections themselves, but at
//! `max_distance >= 2` a second edit can repair them into ordinary words, so
//! they cannot be skipped. See [`edits_utf16`] and `crate::units`.
//!
//! ### 4. A word is one of its own edits
//!
//! At the end of the word the transposition degenerates and reproduces the
//! input, so `get_corrections` on a correctly spelled word normally returns
//! that word first. See [`Edits`].
//!
//! # How this port differs structurally
//!
//! The reference builds every edit level as a reference array before looking at
//! any of it — 270,000 strings for a nine-letter word at distance 2 — then
//! filters, maps to `[word, frequency]` pairs, sorts, maps back, concatenates
//! and finally de-duplicates in O(n²) with `indexOf`. This port:
//!
//! * makes [`Edits`] a **lazy** generator and streams the deepest level, so only
//!   the level that seeds the next one is ever materialised;
//! * hands the dedup inside `edits` to a hash set, which keeps first-occurrence
//!   order but drops the reference's quadratic `indexOf` scan;
//! * runs over `&[u8]` for ASCII input, where one byte *is* one code unit, so
//!   the dictionary lookup borrows a `&str` straight out of the candidate buffer
//!   and the hot path allocates nothing per candidate;
//! * de-duplicates the final result by word *index* rather than by string
//!   comparison, again in linear time;
//! * at `max_distance <= 2` skips candidate generation entirely: a lazily
//!   built SymSpell-style deletion index retrieves the handful of stored words
//!   sharing a deletion sequence with the input, and the reference's exact
//!   output order — distance grouping, frequency, *and* the generator-order
//!   tie-break — is reconstructed from closed forms for each word's position
//!   in the edit stream. Same bytes out, without walking the ~10⁴–10⁵
//!   candidate strings the reference walks at distance 2.
//!
//! # Divergences
//!
//! * **A negative or fractional `maxDistance` cannot be expressed.** In
//!   The reference `getCorrections('ca', -1)` never terminates: the recursion stops
//!   only on `distanceCounter === 0`, and each level multiplies the candidate
//!   count by ~500, so the process grows without bound until it is killed. The
//!   Rust signature takes a `u32`, so the hang is unreachable. `0` still means
//!   `1`, matching the reference's falsiness check.
//! * **Lone surrogates cannot be returned as `String`s.** [`edits`] renders a
//!   split surrogate pair as `U+FFFD`, which can collapse two distinct
//!   The reference results onto one Rust string. [`edits_utf16`] is exact, and is
//!   what [`Spellcheck`] uses internally, so corrections are unaffected.
//! * **[`Spellcheck::frequency`] returns `None`** where the reference would hand
//!   back an inherited `Object.prototype` method for a word that is not in the
//!   list. Unobservable through [`Spellcheck::get_corrections`], which only ever
//!   looks up words the trie has already accepted.
//!
//! # Fuzzy indexing (Verbora extension, not parity)
//!
//! [`FuzzyIndex`] and [`DeletionIndex`] both answer a related but different
//! question from [`Spellcheck::get_corrections`]: *which of many stored
//! words are within edit distance `k` of this query?*, pre-indexed once
//! instead of generating edits per call. Both are Verbora-native additions
//! with no reference equivalent and no `docs/MIGRATION_MATRIX.md` entry —
//! [`FuzzyIndex`] is a BK-tree (`max_distance` chosen per query, no
//! construction-time ceiling); [`DeletionIndex`] is a SymSpell-style
//! deletion index (`max_distance` fixed once at construction, faster
//! queries in exchange — see its own doc comment for the real, measured
//! trade-off between the two). Neither replaces the other; pick per call
//! site.

mod comparator_sort;
mod deletion_index;
mod edits;
mod fuzzy_index;
mod query_index;
mod spellcheck;
mod units;

pub use deletion_index::{DeletionIndex, DeletionIndexBuilder, DeletionNeighbors};
pub use edits::{
    ALPHABET, Edits, edits, edits_utf16, edits_with_max_distance, edits_with_max_distance_helper,
    edits_with_max_distance_utf16,
};
pub use fuzzy_index::{FuzzyIndex, FuzzyIndexBuilder, Neighbors};
pub use spellcheck::Spellcheck;
pub use units::EditUnit;

/// Orders `items` exactly as [`Spellcheck::get_corrections`] orders one
/// distance level: descending by `frequency`, ties keeping their original
/// order.
///
/// Exposed because that ordering is part of the observable contract and is
/// *not* reproducible with `sort_by`. The reference's comparator never returns
/// 0, which is harmless for finite frequencies — the result is then exactly a
/// stable descending sort — but not for `NaN`, where the induced relation is
/// non-transitive and the permutation becomes a property of the reference engine's TimSort. This
/// runs that algorithm.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::sort_by_frequency;
///
/// let mut ranked = [("a", 1.0), ("b", 3.0), ("c", 1.0), ("d", 3.0)];
/// sort_by_frequency(&mut ranked, |&(_, f)| f);
/// assert_eq!(ranked.map(|(w, _)| w), ["b", "d", "a", "c"]);
/// ```
pub fn sort_by_frequency<T: Copy>(items: &mut [T], frequency: impl Fn(&T) -> f64) {
    // `a[1] > b[1] ? -1 : 1`, verbatim.
    comparator_sort::sort_by(
        items,
        |a, b| {
            if frequency(a) > frequency(b) { -1 } else { 1 }
        },
    );
}
