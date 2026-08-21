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
/// builder.insert_all(["bitten", "kitten", "sitting"]);
/// let index = builder.build();
///
/// let mut found: Vec<_> = index.neighbors("kitten", 3).collect();
/// // Ranking is the caller's: the index generates candidates, it does not
/// // rank. When the ranking wanted is the obvious one, it is this type's own.
/// found.sort();
/// // Nearest first, so the exact match leads even though "bitten" is
/// // alphabetically ahead of it.
/// assert_eq!(
///     found.iter().map(|n| (n.word, n.distance)).collect::<Vec<_>>(),
///     [("kitten", 0), ("bitten", 1), ("sitting", 3)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// [`Ord`] is written out rather than derived, and the two disagree, so
    /// this pins the difference on the pair that shows it: a nearer neighbour
    /// whose word sorts *after* a farther one. Comparing fields in declaration
    /// order — what `#[derive(Ord)]` would do — puts `"a"` first because `word`
    /// comes first in the struct, which is the ranking this type exists not to
    /// have.
    #[test]
    fn the_order_on_a_neighbor_is_the_documented_ranking() {
        let near = Neighbor {
            word: "b",
            distance: 1,
        };
        let far = Neighbor {
            word: "a",
            distance: 2,
        };
        assert!(near < far, "distance must outrank the word");

        let mut found = [far, near];
        found.sort();
        assert_eq!(
            found
                .iter()
                .map(|n| (n.word, n.distance))
                .collect::<Vec<_>>(),
            [("b", 1), ("a", 2)]
        );

        // Within one distance the word is the tiebreak, ascending.
        let mut tied = [
            Neighbor {
                word: "b",
                distance: 1,
            },
            Neighbor {
                word: "a",
                distance: 1,
            },
        ];
        tied.sort();
        assert_eq!(tied.iter().map(|n| n.word).collect::<Vec<_>>(), ["a", "b"]);

        // Total, and consistent with `Eq`: equal on both fields is `Equal`,
        // and every comparison agrees with `Ord`.
        assert_eq!(near.cmp(&near), std::cmp::Ordering::Equal);
        assert_eq!(near.partial_cmp(&far), Some(std::cmp::Ordering::Less));
        assert_eq!(far.partial_cmp(&near), Some(std::cmp::Ordering::Greater));
    }
}
