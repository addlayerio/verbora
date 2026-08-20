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
/// // Ranking is the caller's: the index generates candidates, it does not
/// // rank. When the ranking wanted is the obvious one, it is this type's own.
/// found.sort();
/// assert_eq!(
///     found.iter().map(|n| (n.word, n.distance)).collect::<Vec<_>>(),
///     [("kitten", 0), ("mitten", 1), ("sitting", 3)]
/// );
/// ```
///
/// # Order
///
/// [`Ord`] is *nearest first*, then ascending word — the ranking a caller
/// sorting neighbours actually wants. It is written out rather than derived
/// because the derived order compares fields in declaration order, which puts
/// `word` first and would rank an alphabetically early word ahead of an exact
/// match. The order is total and consistent with `Eq`: two neighbours agreeing
/// on word and distance agree on every field there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Neighbor<'a> {
    /// The indexed word, borrowed from the index.
    pub word: &'a str,
    /// Its exact edit distance from the query, under the crate's metric
    /// ([`verbora_distance::damerau_levenshtein`], counted in Unicode
    /// scalars). Always `<= max_distance`.
    pub distance: u32,
}

impl Ord for Neighbor<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .cmp(&other.distance)
            .then_with(|| self.word.cmp(other.word))
    }
}

impl PartialOrd for Neighbor<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
