//! Spelling correction and edit-distance candidate retrieval.
//!
//! ```
//! use verbora_spellcheck::Spellcheck;
//!
//! // The word list is a corpus: repeats are frequencies, and frequency
//! // breaks ties within an edit distance.
//! let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
//!
//! assert!(sc.is_correct("the"));
//! assert_eq!(sc.correction_words("he", 1), ["he", "the", "she"]);
//! ```
//!
//! # One metric, one unit
//!
//! Everything in this crate measures distance the same way:
//! [`verbora_distance::damerau_levenshtein`] — unrestricted
//! Damerau–Levenshtein, unit cost, counted in **Unicode scalar values**
//! (`docs/design/distance-contract.md` §2).
//!
//! * **Damerau–Levenshtein**, not plain Levenshtein, because a transposed
//!   adjacent pair is the commonest typo class there is and should cost one
//!   edit, not two.
//! * **Unrestricted**, not optimal-string-alignment, because it is a true
//!   metric — symmetric, obeying the triangle inequality — which
//!   [`FuzzyIndex`]'s BK-tree pruning requires to be correct at all.
//! * **Unicode scalars**, because generation and verification must agree on
//!   what one unit is. A generator that counts in code units against a metric
//!   that counts in scalars silently returns fewer matches with nothing failing
//!   to compile — see `crate::deletions` for the completeness lemma this rests
//!   on, and `tests/completeness.rs` for its exhaustive verification.
//!
//! There is no cost parameter anywhere, and no configurable alphabet. A
//! correction is defined by the metric, so it is never restricted to words
//! spelled with some fixed letter set: `"cafe"` corrects to `"café"` and
//! `"Мсква"` to `"Москва"`, in any script, with no configuration.
//!
//! # Three structures, three questions
//!
//! | Type | Answers | `max_distance` is fixed | Built for |
//! |---|---|---|---|
//! | [`Spellcheck`] | *what should this misspelling have been?* | per query | one corpus with frequencies; ranks its answers |
//! | [`FuzzyIndex`] | *which stored words are within `k` of this one?* | per query | repeated queries at varying `k` |
//! | [`DeletionIndex`] | the same question | at **build** time | repeated queries at one small, known `k` |
//!
//! [`Spellcheck`] is the one that ranks. The two indexes are **candidate
//! generation** — a blocking step, not a search engine: they do not rank, do
//! not pick a best match, and do not accept a query language, the same boundary
//! [`verbora_phonetics::PhoneticIndex`](https://docs.rs/verbora-phonetics)
//! draws. Each neighbour comes back with its exact distance already computed,
//! so ranking at the call site is one sort and no recomputation.
//!
//! ```
//! use verbora_spellcheck::{DeletionIndexBuilder, FuzzyIndexBuilder};
//!
//! let mut fuzzy = FuzzyIndexBuilder::new();
//! fuzzy.insert_all(["kitten", "sitting", "mitten"]);
//! let fuzzy = fuzzy.build();
//!
//! let mut deletion = DeletionIndexBuilder::new(2);
//! deletion.insert_all(["kitten", "sitting", "mitten"]);
//! let deletion = deletion.build();
//!
//! // The same set, reached two ways.
//! let mut a: Vec<&str> = fuzzy.neighbors("kitten", 2).map(|n| n.word).collect();
//! let mut b: Vec<&str> = deletion
//!     .neighbors("kitten", 2)
//!     .expect("2 is the index's ceiling")
//!     .map(|n| n.word)
//!     .collect();
//! a.sort_unstable();
//! b.sort_unstable();
//! assert_eq!(a, b);
//!
//! // Only the BK-tree can be asked for more after the fact.
//! assert_eq!(fuzzy.neighbors("kitten", 3).count(), 3);
//! assert!(deletion.neighbors("kitten", 3).is_err());
//! ```
//!
//! # Choosing the Right API
//!
//! **Which type?**
//!
//! ```text
//! do you have per-word frequencies and want ranked suggestions?
//!  └── yes ── Spellcheck
//!  └── no  ── will every query use the same small k, fixed up front?
//!              ├── yes ── DeletionIndex   (cheaper queries, k fixed at build)
//!              └── no  ── FuzzyIndex      (k chosen per query)
//! ```
//!
//! Neither index replaces the other and neither replaces [`Spellcheck`]:
//! `Spellcheck` carries the corpus frequencies and the ranking rule, the
//! indexes carry neither. `Spellcheck` maintains its own retrieval structure
//! internally, so pairing it with an index of the same words would index the
//! corpus twice.
//!
//! **Which [`Spellcheck`] method?** See that type's own "Choosing the Right
//! API" section — `is_correct` for membership, `best_correction` for one
//! suggestion, [`Spellcheck::corrections`] for a ranked list with the numbers
//! behind the ranking, [`Spellcheck::correction_words`] for owned words alone,
//! and `par_corrections_batch` (feature `parallel`) for document-sized batches.
//!
//! No speed figure appears in this crate's documentation. The contract changed
//! with this crate's migration, so every previously published number is stale;
//! what is documented instead is the work each path does and what it allocates.

mod deletion_index;
mod deletions;
mod fuzzy_index;
mod neighbor;
mod spellcheck;

pub use deletion_index::{
    DeletionIndex, DeletionIndexBuilder, DeletionNeighbors, DistanceBeyondIndex,
};
pub use fuzzy_index::{FuzzyIndex, FuzzyIndexBuilder, Neighbors};
pub use neighbor::Neighbor;
pub use spellcheck::{CorpusTooLarge, Correction, MAX_DISTINCT_WORDS, Spellcheck};
