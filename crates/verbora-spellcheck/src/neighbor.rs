//! [`Neighbor`]: what both fuzzy indexes yield.

/// One indexed word that is within the requested edit distance of a query,
/// together with how far away it actually is.
///
/// The distance is not an extra: both indexes compute it to decide whether the
/// word is a match at all, so returning it costs nothing and saves the caller
/// recomputing it to rank the results.
///
/// ```
/// use verbora_spellcheck::FuzzyIndexBuilder;
///
/// let mut builder = FuzzyIndexBuilder::new();
/// builder.insert_all(["kitten", "sitting", "mitten"]);
/// let index = builder.build();
///
/// let mut found: Vec<_> = index.neighbors("kitten", 3).collect();
/// // Ranking is the caller's: the index generates candidates, it does not rank.
/// found.sort_by_key(|n| (n.distance, n.word));
/// assert_eq!(
///     found.iter().map(|n| (n.word, n.distance)).collect::<Vec<_>>(),
///     [("kitten", 0), ("mitten", 1), ("sitting", 3)]
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Neighbor<'a> {
    /// The indexed word, borrowed from the index.
    pub word: &'a str,
    /// Its exact edit distance from the query, under the crate's metric
    /// ([`verbora_distance::damerau_levenshtein`], counted in Unicode
    /// scalars). Always `<= max_distance`.
    pub distance: u32,
}
