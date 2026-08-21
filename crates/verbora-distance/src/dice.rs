//! Sørensen–Dice coefficient: bigram sets, and the one call that intersects
//! them.
//!
//! **Internal notes.** The definition, the guarantees, the degenerate cases
//! and the consequences of taking bigrams over a *set* are published on
//! [`dice_coefficient`] and on the crate root. What follows is the record of
//! what this module used to do and what has not been measured since.
//!
//! # The removed preprocessing step
//!
//! Case and whitespace are significant here, which is a deliberate change from
//! Dice's earlier behaviour in this crate — and the widest **silent** change in
//! `docs/design/distance-contract.md`:
//! nothing fails to compile, but every input containing an upper-case letter
//! or any whitespace now scores differently. The removed step was an
//! unconditional `to_lowercase` plus a whitespace-run collapse and trim over
//! a whitespace set inherited from another language's regex `\s` class — a
//! policy no caller could turn off, absent from Dice (1945) and from the
//! bigram application alike, and the crate's last Unicode character database
//! dependency. See `docs/design/distance-contract.md` §3.5 and §4.6.
//!
//! # The rejected alternative to padding
//!
//! That a one-scalar operand yields no bigram at all is published on
//! [`dice_coefficient`]. Boundary markers (`^a$`) were considered as a way to
//! keep short-string gradation and rejected: they need an out-of-alphabet
//! sentinel scalar and would change *every* score, not just the degenerate
//! ones (`docs/design/distance-contract.md` §4.6).
//!
//! # What has not been measured
//!
//! `UNMEASURED` (`docs/design/distance-contract.md` §7, item 5): removing the
//! preprocessing step deleted two `String` and two `Vec` allocations per
//! call, and Dice is the one metric here with no ASCII fast path at all — it
//! never enters the crate's unit dispatch. Neither the saving nor the value
//! of adding such a path has been measured, and no number is published here
//! until a full-precision run exists.

use rustc_hash::FxHashSet;

/// Sørensen–Dice coefficient between two strings: a finite `f64` in
/// `0.0..=1.0`, higher meaning more similar.
///
/// `2 * |A ∩ B| / (|A| + |B|)` over the **sets** of adjacent scalar pairs of
/// the two operands, and `1.0`/`0.0` for equal/unequal operands too short to
/// have a bigram at all.
///
/// # Definition
///
/// Dice, L. R. (1945), *Measures of the amount of ecologic association between
/// species*, Ecology 26(3), 297–302. Dice's coincidence index for two samples
/// is `2C / (A + B)`, where `A` and `B` are the numbers of species in each
/// sample and `C` the number common to both. The string application takes the
/// "species" to be the distinct **bigrams** of a string: ordered pairs of
/// adjacent Unicode scalar values.
///
/// For a string of `n` scalars, `bigrams(s)` is the set
/// `{ (s[i], s[i+1]) : 0 <= i < n - 1 }`. It has at most `n - 1` elements and
/// is **empty** when `n < 2`. With `A = bigrams(s1)` and `B = bigrams(s2)`:
///
/// ```text
/// dice = 2 * |A ∩ B| / (|A| + |B|)     when |A| + |B| > 0
/// dice = 1.0                           when |A| + |B| == 0 and s1 == s2
/// dice = 0.0                           when |A| + |B| == 0 and s1 != s2
/// ```
///
/// The result is total, in range, an identity and symmetric, on the terms the
/// crate's similarities share — see
/// [*What the similarities guarantee*](crate#what-the-similarities-guarantee),
/// including the degenerate cases in the table there. Two of those guarantees
/// are worth spelling out for this metric in particular: the range holds
/// because `|A ∩ B| <= min(|A|, |B|)` bounds the numerator by the denominator
/// and both are exactly representable non-negative integers, and the symmetry
/// is bit-exact because both the intersection and the sum of the two set sizes
/// are symmetric.
///
/// # Examples
///
/// ```
/// use verbora_distance::dice_coefficient;
///
/// // {ni, ig, gh, ht} against {na, ac, ch, ht}: one shared bigram of 4 + 4.
/// assert_eq!(dice_coefficient("night", "nacht"), 2.0 * 1.0 / (4.0 + 4.0));
///
/// // Identical operands score 1.0 exactly — including two empty strings,
/// // which are identical rather than disjoint.
/// assert_eq!(dice_coefficient("abc", "abc"), 1.0);
/// assert_eq!(dice_coefficient("", ""), 1.0);
///
/// // Nothing in common scores 0.0 — and case is part of "in common".
/// assert_eq!(dice_coefficient("abc", "xyz"), 0.0);
/// assert_eq!(dice_coefficient("ABC", "abc"), 0.0);
/// assert_eq!(dice_coefficient(&"ABC".to_lowercase(), "abc"), 1.0);
/// ```
///
/// # No input rewriting: case and whitespace are significant
///
/// The operands are used exactly as given. Nothing is lower-cased, trimmed,
/// collapsed or normalised, and `' '` is an ordinary scalar forming ordinary
/// bigrams — so `dice_coefficient("ABC", "abc")` is `0.0`, not `1.0`, and
/// `dice_coefficient("Hello  World", "hello world")` is `14/21`, not `1.0`.
///
/// **Caseless, whitespace-insensitive matching is the caller's, and it is one
/// line:**
///
/// ```
/// use verbora_distance::dice_coefficient;
///
/// let a = "Hello  World";
/// let b = "hello world";
///
/// // Case-insensitive: fold once, at ingestion.
/// assert!(dice_coefficient(&a.to_lowercase(), &b.to_lowercase()) > dice_coefficient(a, b));
///
/// // Case- and whitespace-insensitive: collapse runs as well.
/// fn squash(s: &str) -> String {
///     s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
/// }
/// assert_eq!(dice_coefficient(&squash(a), &squash(b)), 1.0);
/// ```
///
/// Doing it at the call site is also strictly cheaper in a screening loop,
/// where a query folded inside the metric is re-folded against every
/// candidate. Note that `str::to_lowercase` is lowercasing, not UAX #21 case
/// folding; if that distinction matters, normalise with `verbora-normalizers`
/// first.
///
/// # The unit is the Unicode scalar value
///
/// A bigram is a pair of adjacent `char`s, so `"a😀b"` is three scalars and
/// yields two bigrams — `('a', '😀')` and `('😀', 'b')` — rather than
/// splitting the emoji across a pair. `"😀"` alone is a *single* scalar and so
/// has no bigram at all, landing in the same degenerate class as `"a"`.
///
/// # No padding
///
/// A one-scalar operand yields **no** bigram, so it shares none:
/// `dice_coefficient("a", "ab")` is `0.0`, and so is
/// `dice_coefficient("a", "a b")`. Padding a short operand with a space would
/// fabricate a bigram containing a scalar that is not in the input and
/// manufacture similarity out of it.
///
/// The consequence to know about is that Dice discriminates nothing below two
/// scalars: `dice_coefficient("a", "ab")` and `dice_coefficient("a", "zzz")`
/// are both `0.0`. For very short strings reach for
/// [`jaro_winkler`](fn@crate::jaro_winkler) or
/// [`levenshtein`](fn@crate::levenshtein) instead.
///
/// ```
/// use verbora_distance::dice_coefficient;
///
/// assert_eq!(dice_coefficient("a", "ab"), 0.0);
/// assert_eq!(dice_coefficient("a", "zzz"), 0.0);
/// assert_eq!(dice_coefficient("😀", "😀"), 1.0); // identical, not disjoint
/// ```
///
/// # Identity is not injective
///
/// `dice_coefficient(x, x) == 1.0` always, but `1.0` does **not** imply
/// equality: bigrams form a set, so `dice_coefficient("aaaa", "aa") == 1.0` —
/// both reduce to the single bigram `('a', 'a')`. This is inherent to the
/// set-based definition rather than a defect to patch, and a multiset variant
/// would not fix it either: `"aabab"` and `"abaab"` have identical bigram
/// multisets. Callers who need `score == 1.0` to imply equality compare the
/// strings.
///
/// ```
/// use verbora_distance::dice_coefficient;
///
/// assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);
/// assert!("aaaa" != "aa");
/// ```
///
/// # Allocation
///
/// Two hash sets of `(char, char)` per call, one per operand, each reserved
/// once to the operand's exact bigram bound (`n - 1`) so the table is never
/// grown mid-fill. Nothing else is allocated: the bigrams are `(char, char)`
/// keys built directly from a `chars()` walk, so no `String` is built per
/// bigram and no working `Vec` of scalars is materialised.
#[must_use]
pub fn dice_coefficient(s1: &str, s2: &str) -> f64 {
    let a = bigrams(s1);
    let b = bigrams(s2);

    let total = a.len() + b.len();
    if total == 0 {
        // Neither operand reaches two scalars, so Dice's `2C / (A + B)` is a
        // `0 / 0` with no value of its own. The singularity is removable and
        // consistency forces which value to insert: two operands that are
        // *identical* score 1.0 like every other identical pair, and two that
        // share no feature score 0.0 like every other disjoint pair.
        return if s1 == s2 { 1.0 } else { 0.0 };
    }

    // Probe the larger set with the smaller one. `|A ∩ B|` is a cardinality,
    // so which side is iterated changes nothing about the answer — only the
    // probe count, which this bounds by `min(|A|, |B|)`. Symmetry therefore
    // survives: the intersection and the sum of the two set sizes are both
    // symmetric, so swapping the arguments returns the identical bits.
    let (small, large) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let shared = small.iter().filter(|g| large.contains(*g)).count();

    (2 * shared) as f64 / total as f64
}

/// The set of ordered pairs of adjacent Unicode scalar values in `s`.
///
/// Empty when `s` has fewer than two scalars: a bigram needs two, and no
/// padding scalar is invented to manufacture one.
fn bigrams(s: &str) -> FxHashSet<(char, char)> {
    let mut set = FxHashSet::default();

    let mut chars = s.chars();
    let Some(mut prev) = chars.next() else {
        return set;
    };

    // At most `n - 1` distinct pairs, so sizing the table up front means the
    // fill below never rehashes. Counting scalars is one cheap linear walk
    // next to hashing every pair, and unlike the byte length it does not
    // over-reserve by up to 4x on non-ASCII input.
    set.reserve(s.chars().count() - 1);

    // A pair of scalars is 8 bytes and `Copy`; keying the set on the tuple
    // means no `String` is allocated per bigram.
    for c in chars {
        set.insert((prev, c));
        prev = c;
    }
    set
}

/// [`dice_coefficient`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// # Why this exists
///
/// `dice_coefficient` is a pure function over two borrowed `&str`s with no
/// shared state, so scoring many independent pairs is embarrassingly
/// parallel with zero coordination cost between pairs. This function is
/// exactly `pairs.par_iter().map(|(a, b)| dice_coefficient(a, b)).collect()`
/// — a thin fan-out over the existing sequential primitive, not a second
/// implementation of it. The bigram/intersect pipeline inside
/// `dice_coefficient` is untouched.
///
/// # When to reach for it vs. the sequential loop
///
/// `dice_coefficient` is dominated by hashing two bigram sets, while a
/// `rayon` task costs on the order of a microsecond to schedule
/// (`site/performance/parallelism.md`) — so a plain
/// `pairs.iter().map(|(a, b)| dice_coefficient(a, b)).collect()` loop wins
/// outright for small batches. This function pays off once the batch is large
/// enough that the per-pair hashing dominates scheduling; confirm it on your
/// own data.
///
/// `UNMEASURED`: the crossover table this documentation used to carry was
/// measured before the preprocessing step and the padding were deleted, so it
/// described a different algorithm and is removed rather than adjusted. It is
/// restored only from a fresh full-precision run of
/// `cargo bench -p verbora-distance --features parallel -- par_dice`
/// (`docs/design/distance-contract.md` §7, items 5 and 8).
///
/// # Allocation behaviour
///
/// One `Vec<f64>` sized to `pairs.len()` for the output, plus whatever
/// [`dice_coefficient`] itself allocates per pair (two bigram sets — see its
/// own `Allocation behaviour` section). No additional buffering, no locking,
/// no per-call thread-pool construction — this uses whichever global `rayon`
/// pool is already installed (or `rayon`'s default one), so pool
/// configuration remains the caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] ==
/// dice_coefficient(pairs[i].0, pairs[i].1)` — via `rayon`'s order-preserving
/// `map` + `collect`. `dice_coefficient` never errors and never returns a
/// non-finite value, so every element is a finite `f64` in `0.0..=1.0`.
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| dice_coefficient(a, b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SplitMix64 PRNG, used by the randomized property test below.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    /// The arithmetic of `2 * |A ∩ B| / (|A| + |B|)`, worked out from the
    /// definition for each pair rather than recorded from a run.
    ///
    /// - `("night", "nacht")`: `{ni, ig, gh, ht}` and `{na, ac, ch, ht}`
    ///   share `ht` — `2 · 1 / (4 + 4)`.
    /// - `("abc", "abd")`: `{ab, bc}` and `{ab, bd}` share `ab` —
    ///   `2 · 1 / (2 + 2)`.
    /// - `("ab", "abc")`: `{ab}` and `{ab, bc}` share `ab` —
    ///   `2 · 1 / (1 + 2)`.
    /// - `("abc", "xyz")`: disjoint — `2 · 0 / (2 + 2)`.
    /// - `("Hello  World", "hello world")`: 12 scalars give 11 distinct
    ///   bigrams `{He, el, ll, lo, o␠, ␠␠, ␠W, Wo, or, rl, ld}`; 11 scalars
    ///   give 10, `{he, el, ll, lo, o␠, ␠w, wo, or, rl, ld}`. Shared:
    ///   `el, ll, lo, o␠, or, rl, ld` — seven. `2 · 7 / (11 + 10) = 14/21`.
    /// - `("  padded  ", "padded")`: 10 scalars give the 9 pairs
    ///   `␠␠, ␠p, pa, ad, dd, de, ed, d␠, ␠␠`, of which `␠␠` repeats, so the
    ///   *set* has 8; `"padded"` gives `{pa, ad, dd, de, ed}`, 5. Shared: all
    ///   5. `2 · 5 / (8 + 5) = 10/13`.
    #[test]
    fn arithmetic_follows_the_definition() {
        assert_eq!(dice_coefficient("night", "nacht"), 2.0 * 1.0 / (4.0 + 4.0));
        assert_eq!(dice_coefficient("abc", "abd"), 2.0 * 1.0 / (2.0 + 2.0));
        assert_eq!(dice_coefficient("ab", "abc"), 2.0 * 1.0 / (1.0 + 2.0));
        assert_eq!(dice_coefficient("abc", "xyz"), 0.0);
        assert_eq!(
            dice_coefficient("Hello  World", "hello world"),
            2.0 * 7.0 / (11.0 + 10.0)
        );
        assert_eq!(
            dice_coefficient("  padded  ", "padded"),
            2.0 * 5.0 / (8.0 + 5.0)
        );

        // The bigram counts the arithmetic above claims, asserted directly so
        // a wrong denominator cannot hide inside a matching quotient.
        assert_eq!(bigrams("night").len(), 4);
        assert_eq!(bigrams("nacht").len(), 4);
        assert_eq!(bigrams("Hello  World").len(), 11);
        assert_eq!(bigrams("hello world").len(), 10);
        assert_eq!(bigrams("  padded  ").len(), 8);
        assert_eq!(bigrams("padded").len(), 5);
    }

    #[test]
    fn identical_strings_score_one() {
        for x in ["abc", "a", "", " ", "  ", "😀", "a😀b", "Москва", "क्षि"] {
            assert_eq!(
                dice_coefficient(x, x).to_bits(),
                1.0f64.to_bits(),
                "dice_coefficient({x:?}, {x:?})"
            );
        }
    }

    /// The `|A| + |B| == 0` branch, which replaces the `0 / 0` this function
    /// used to return as `NaN`. Both operands are shorter than two scalars,
    /// so neither has a bigram; identical operands score `1.0` and everything
    /// else `0.0`.
    #[test]
    fn degenerate_pairs_are_total() {
        for (a, b, want) in [
            ("", "", 1.0f64),
            ("a", "a", 1.0),
            ("a", "b", 0.0),
            ("", "a", 0.0),
            ("a", "", 0.0),
            (" ", " ", 1.0),
            (" ", "\t", 0.0),
            ("\u{FEFF}", " ", 0.0),
            // One set empty, the other not: `2 · 0 / (0 + 1)` divides
            // normally and is 0.0 without needing the branch.
            ("a", "ab", 0.0),
        ] {
            let got = dice_coefficient(a, b);
            assert_eq!(got.to_bits(), want.to_bits(), "dice({a:?}, {b:?}) = {got}");
            assert!(got.is_finite(), "dice({a:?}, {b:?}) is not finite");
        }
    }

    #[test]
    fn empty_against_nonempty_is_zero() {
        assert_eq!(dice_coefficient("", "abc"), 0.0);
        assert_eq!(dice_coefficient("abc", ""), 0.0);
    }

    /// Every result over the cross product of the degenerate corpus is
    /// finite, in `[0, 1]`, and bit-identical under argument swap — including
    /// the whitespace scalars the deleted preprocessing step used to erase
    /// (U+0085 and U+FEFF are in that step's foreign `\s` class but not in
    /// Unicode `White_Space`, and both used to reduce to the empty string).
    #[test]
    fn totality_over_the_degenerate_corpus() {
        const CORPUS: &[&str] = &["", " ", "\t", "\n", "\u{FEFF}", "\u{0085}", "a", "😀", "  "];
        for a in CORPUS {
            for b in CORPUS {
                let v = dice_coefficient(a, b);
                assert!(v.is_finite(), "dice({a:?}, {b:?}) = {v} is not finite");
                assert!(
                    (0.0..=1.0).contains(&v),
                    "dice({a:?}, {b:?}) = {v} is outside [0, 1]"
                );
                assert_eq!(
                    v.to_bits(),
                    dice_coefficient(b, a).to_bits(),
                    "dice is not symmetric on ({a:?}, {b:?})"
                );
                assert_eq!(v == 1.0, a == b, "dice({a:?}, {b:?}) = {v}");
            }
        }
    }

    /// A one-scalar operand has no bigram, and none is fabricated for it by
    /// padding with a space.
    #[test]
    fn one_scalar_operands_are_not_padded() {
        assert!(bigrams("a").is_empty());
        assert!(bigrams("😀").is_empty());

        // With padding, `"a"` would carry `('a', ' ')` and share it with both
        // of these; without it there is nothing to share.
        //   A = bigrams("a")   = {}                   |A| = 0
        //   B = bigrams("a b") = {(a,␠), (␠,b)}       |B| = 2
        //   2 * 0 / (0 + 2) = 0.0
        assert_eq!(dice_coefficient("a", "a b"), 0.0);
        assert_eq!(dice_coefficient("a", "a a"), 0.0);
        assert_eq!(dice_coefficient("a", "b"), 0.0);
    }

    /// Case, whitespace and format characters are ordinary scalars: nothing
    /// is folded, trimmed or collapsed before the bigrams are formed.
    #[test]
    fn operands_are_not_rewritten() {
        // {AB, BC} against {ab, bc} is disjoint.
        assert_eq!(dice_coefficient("ABC", "abc"), 0.0);
        // ...and folding at the call site is what recovers the match.
        assert_eq!(dice_coefficient(&"ABC".to_lowercase(), "abc"), 1.0);

        // U+FEFF was in the deleted step's whitespace class, so it used to be
        // erased from both operands and these two scored 1.0 together.
        assert_eq!(dice_coefficient("a\u{FEFF}b", "a\u{FEFF}b"), 1.0);
        assert!(dice_coefficient("a\u{FEFF}b", "ab") < 1.0);

        // Whitespace runs are significant: 11 and 10 bigrams, 7 shared.
        assert!(dice_coefficient("Hello  World", "hello world") < 1.0);
        assert!(dice_coefficient("  padded  ", "padded") < 1.0);
    }

    /// `1.0` does not imply equality, because the bigrams form a set.
    #[test]
    fn identity_is_not_injective() {
        assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);
        assert_eq!(bigrams("aaaa"), bigrams("aa"));
    }

    #[test]
    fn partial_overlap_is_between_zero_and_one() {
        let d = dice_coefficient("night", "nacht");
        assert!(d > 0.0 && d < 1.0, "got {d}");
    }

    /// A bigram is a pair of adjacent *scalars*, so an astral character is
    /// one element of a pair rather than two.
    ///
    /// `docs/design/distance-contract.md` §2.5: two astral scalars share no
    /// bigram with one of them. Under the UTF-16 unit the shared high
    /// surrogate manufactured a `0.5` here.
    ///   A = bigrams("😀😁") = {(😀,😁)}    |A| = 1
    ///   B = bigrams("😀")   = {}           |B| = 0
    ///   2 * 0 / (1 + 0) = 0.0
    #[test]
    fn astral_characters_use_scalar_pairs() {
        assert_eq!("😀".chars().count(), 1);
        assert_eq!(bigrams("😀😁"), [('😀', '😁')].into_iter().collect());
        assert_eq!(dice_coefficient("😀😁", "😀"), 0.0);
        assert_eq!(dice_coefficient("😀", "😀"), 1.0);

        // Three scalars, two bigrams — the emoji is never split across a pair.
        assert_eq!(
            bigrams("a😀b"),
            [('a', '😀'), ('😀', 'b')].into_iter().collect()
        );
    }

    /// Range, totality and symmetry over 51,000 randomized pairs, lengths
    /// `0..=300`, across six alphabets: a single repeated scalar (which makes
    /// every equal-length pair *equal*, exercising the identity half
    /// constantly, and collapses every bigram set to one element), a
    /// whitespace-heavy alphabet including the two format characters the
    /// deleted preprocessing step used to erase, mixed case (the class this
    /// step changed), the Latin alphabet, a BMP alphabet and an astral one.
    #[test]
    fn guarantees_hold_over_a_large_randomized_corpus() {
        const ALPHABETS: [&[char]; 6] = [
            &['a'],
            &[' ', '\t', '\n', '\u{FEFF}', '\u{0085}', 'a', 'b'],
            &['a', 'A', 'b', 'B', ' '],
            &[
                'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
                'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
            ],
            &['\u{430}', '\u{431}', '\u{4e2d}', '\u{f1}', '\u{3b1}'],
            &['\u{1F600}', '\u{1D518}', '\u{1F389}', '\u{2070E}'],
        ];

        let mut rng = SplitMix64(0x0D1C_E0D1_2026_0819);
        let draw = |rng: &mut SplitMix64, alphabet: usize, len: usize| -> String {
            let a = ALPHABETS[alphabet];
            (0..len).map(|_| a[rng.next_range(a.len())]).collect()
        };

        for round in 0..51_000u32 {
            let alphabet = (round % 6) as usize;
            // Lengths 0..=300, with the empty and single-scalar classes drawn
            // often enough that the degenerate branch is genuinely covered.
            let len1 = if round % 401 == 0 {
                round as usize % 2
            } else {
                rng.next_range(301)
            };
            let len2 = if round % 401 == 200 {
                round as usize % 2
            } else if round % 7 == 0 {
                rng.next_range(3)
            } else {
                rng.next_range(301)
            };
            let a = draw(&mut rng, alphabet, len1);
            let b = draw(&mut rng, alphabet, len2);

            let v = dice_coefficient(&a, &b);
            assert!(v.is_finite(), "dice({a:?}, {b:?}) = {v} is not finite");
            assert!(
                (0.0..=1.0).contains(&v),
                "dice({a:?}, {b:?}) = {v} is outside [0, 1]"
            );
            assert_eq!(
                v.to_bits(),
                dice_coefficient(&b, &a).to_bits(),
                "dice is not symmetric on ({a:?}, {b:?})"
            );
            if a == b {
                assert_eq!(
                    v.to_bits(),
                    1.0f64.to_bits(),
                    "dice({a:?}, {a:?}) is not exactly 1.0"
                );
            }
        }
    }
}
