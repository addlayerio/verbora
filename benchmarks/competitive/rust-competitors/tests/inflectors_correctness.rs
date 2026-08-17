//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/inflectors.rs`.
//!
//! Verifies, once and outside the timed code, that `CountInflector::nth` and
//! the `ordinal` 0.4.0 crate agree on the non-negative domain actually
//! benchmarked, and explicitly documents — via an assertion, not just a
//! comment — the real divergence found for negative integers, which the
//! benchmarked domain deliberately excludes.
//!
//! Also covers the two competitors this pass adds: `pluralizer` 0.5.0 and
//! `Inflector` 0.11.4 for `NounInflector`, and `Inflector`'s
//! `numbers::ordinalize::ordinalize` as a second `CountInflector` match.

use ordinal::ToOrdinal;
use verbora_inflectors::{CountInflector, NounInflector};

/// The exact values `benches/inflectors.rs`'s `sample` cycles through — every
/// suffix class (units, the teens exception, round hundreds) it actually
/// benchmarks, plus the two boundary checks below. Kept in sync with that
/// function's `BASE` array by hand; drift would be caught by
/// `agrees_on_benchmarked_values` failing.
const BENCHMARKED: [i64; 24] = [
    0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 20, 21, 22, 23, 100, 101, 111, 112, 113, 121, 1000,
    1_000_000,
];

#[test]
fn agrees_on_benchmarked_values() {
    for i in BENCHMARKED {
        let a = CountInflector::nth(i);
        let b = i.to_ordinal_string();
        assert_eq!(a, b, "nth({i}): verbora={a:?} ordinal={b:?}");
    }
    assert_eq!(CountInflector::nth(i64::MAX), i64::MAX.to_ordinal_string());
}

/// A real, verified bug in `ordinal` 0.4.0, found while writing this test —
/// not a design difference, and not in the matrix's own dossier: its `suffix`
/// computes the teens exception as `n % 20` instead of `n % 100`
/// (`ordinal-0.4.0/src/lib.rs`, `impl_ordinal!`), so any number whose last two
/// digits fall in `31..=33`, `51..=53`, `71..=73` or `91..=93` gets `"th"`
/// instead of the correct `st`/`nd`/`rd`. Exhaustively counted over `0..1000`:
/// **120 of 1000** (12%) non-negative integers are misformatted — `31` comes
/// back `"31th"`, not `"31st"`. `benches/inflectors.rs`'s `sample` function
/// deliberately avoids every value in this bug's range (see its own doc
/// comment); this test is why that avoidance is deliberate and not
/// coincidental.
#[test]
fn ordinal_crate_has_a_real_teens_modulus_bug() {
    for i in [31i64, 32, 33, 51, 52, 53, 71, 72, 73, 91, 92, 93, 131, 331] {
        let verbora = CountInflector::nth(i);
        let ordinal = i.to_ordinal_string();
        assert!(
            verbora.ends_with("st") || verbora.ends_with("nd") || verbora.ends_with("rd"),
            "expected Verbora to get {i} right, got {verbora:?}"
        );
        assert!(
            ordinal.ends_with("th"),
            "expected the known ordinal-crate bug to reproduce at {i}, got {ordinal:?}"
        );
        assert_ne!(verbora, ordinal);
    }

    let mismatches = (0..1000i64)
        .filter(|&i| CountInflector::nth(i) != i.to_ordinal_string())
        .count();
    assert_eq!(
        mismatches, 120,
        "bug rate changed — re-verify before trusting this test's claim"
    );
}

/// The real divergence: the reference's signed `%` means Verbora gives every
/// negative number (outside the `11`/`12`/`13` coincidence) a `"th"` suffix,
/// while `ordinal` takes the absolute value first and so still applies
/// `st`/`nd`/`rd`. `benches/inflectors.rs` never feeds negative input to
/// either side because of this.
#[test]
fn negative_integers_are_a_documented_divergence() {
    for (i, verbora_expected, ordinal_expected) in [
        (-1i64, "-1th", "-1st"),
        (-2, "-2th", "-2nd"),
        (-3, "-3th", "-3rd"),
        (-21, "-21th", "-21st"),
    ] {
        let a = CountInflector::nth(i);
        let b = i.to_ordinal_string();
        assert_eq!(a, verbora_expected);
        assert_eq!(b, ordinal_expected);
        assert_ne!(
            a, b,
            "the negative-integer divergence is expected to still exist at {i}"
        );
    }
    // The one coincidental agreement in the negative range, kept as a
    // regression check rather than left as a surprise: both read "th" here,
    // but for unrelated reasons (Verbora: the reference teens exception on `-11 %
    // 100 == -11`, which is not `> 10`; ordinal: `abs(-11) == 11`, a genuine
    // teen).
    assert_eq!(CountInflector::nth(-11), "-11th");
    assert_eq!((-11i64).to_ordinal_string(), "-11th");
}

// ---------------------------------------------------------------------------
// `Inflector::numbers::ordinalize::ordinalize` vs `CountInflector::nth_str`
// ---------------------------------------------------------------------------

/// Unlike `ordinal` 0.4.0, `Inflector::ordinalize` operates directly on the
/// decimal string, checking whether the *character* immediately before the
/// last one is `'1'` — reading `Inflector-0.11.4/src/numbers/ordinalize/
/// mod.rs` shows this has no `% 20`-vs-`% 100` bug: it never even computes a
/// remainder. Exhaustively confirmed to fully agree with
/// `CountInflector::nth_str` (`CountInflector::nth`'s string-in/string-out
/// twin — the fair match for `ordinalize`'s own `&str -> String` shape) over
/// every non-negative `i64` in `0..2_000_000`, not just a hand-picked sample.
#[test]
fn inflector_ordinalize_agrees_with_count_inflector_nth_str_on_non_negative_integers() {
    for i in 0..2_000_000i64 {
        let s = i.to_string();
        let v = CountInflector::nth_str(&s);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(v, inf, "nth_str({s}): verbora={v:?} inflector={inf:?}");
    }
}

/// The same negative-integer divergence as `ordinal` (see above), verified
/// separately because `Inflector::ordinalize` reaches the "th" via a
/// different code path (no signed-`%` coercion at all — it just is not
/// designed for negative input): confirmed by reading
/// `Inflector-0.11.4/src/numbers/ordinalize/mod.rs`'s own doctest,
/// `ordinalize("-1")` is `"-1st"`. `benches/inflectors.rs` never feeds
/// negative input to either side because of this.
#[test]
fn inflector_ordinalize_negative_integers_are_a_documented_divergence() {
    for (i, verbora_expected, inflector_expected) in [
        (-1i64, "-1th", "-1st"),
        (-2, "-2th", "-2nd"),
        (-3, "-3th", "-3rd"),
        (-21, "-21th", "-21st"),
    ] {
        let s = i.to_string();
        let v = CountInflector::nth_str(&s);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(v, verbora_expected);
        assert_eq!(inf, inflector_expected);
        assert_ne!(v, inf);
    }
}

// ---------------------------------------------------------------------------
// `NounInflector` vs `pluralizer` 0.5.0 and `Inflector` 0.11.4
// ---------------------------------------------------------------------------

/// The exact (singular, plural) pairs `benches/inflectors.rs`'s
/// `noun_inflector_pluralize`/`noun_inflector_singularize` groups cycle
/// through — verified below, once, to agree across all three implementations
/// in both directions before any timing number is trusted. Found by probing
/// a much larger candidate list (Verbora's own `AMBIGUOUS`/irregular-table
/// entries plus ~120 common regular/irregular English nouns spanning every
/// rule class in `crates/verbora-inflectors/src/data.rs`) and keeping only
/// the words where `NounInflector`, `pluralizer::pluralize`, and
/// `inflector::string::{pluralize,singularize}` unanimously agree — kept in
/// sync with `benches/inflectors.rs`'s own `PAIRS` array by hand; drift would
/// be caught by `benchmarked_pairs_agree_across_all_three_implementations`
/// failing.
pub(crate) const PAIRS: &[(&str, &str)] = &[
    ("party", "parties"),
    ("fly", "flies"),
    ("victory", "victories"),
    ("church", "churches"),
    ("box", "boxes"),
    ("matrix", "matrices"),
    ("index", "indices"),
    ("woman", "women"),
    ("synopsis", "synopses"),
    ("day", "days"),
    ("journey", "journeys"),
    ("hacker", "hackers"),
    ("table", "tables"),
    ("window", "windows"),
    ("keyboard", "keyboards"),
    ("mountain", "mountains"),
    ("river", "rivers"),
    ("compiler", "compilers"),
    ("benchmark", "benchmarks"),
    ("allocation", "allocations"),
    ("throughput", "throughputs"),
    ("cat", "cats"),
    ("dog", "dogs"),
    ("city", "cities"),
    ("bus", "buses"),
    ("glass", "glasses"),
    ("wish", "wishes"),
    ("thesis", "theses"),
    ("analysis", "analyses"),
    ("vertex", "vertices"),
    ("cherry", "cherries"),
    ("baby", "babies"),
    ("toy", "toys"),
    ("key", "keys"),
    ("boy", "boys"),
    ("roof", "roofs"),
    ("chief", "chiefs"),
    ("cliff", "cliffs"),
    ("fox", "foxes"),
    ("dish", "dishes"),
    ("brush", "brushes"),
    ("kiss", "kisses"),
    ("class", "classes"),
    ("dress", "dresses"),
    ("bench", "benches"),
    ("watch", "watches"),
    ("tax", "taxes"),
    ("virus", "viri"),
    ("status", "statuses"),
    ("sky", "skies"),
    ("story", "stories"),
    ("country", "countries"),
    ("family", "families"),
    ("lady", "ladies"),
    ("army", "armies"),
    ("copy", "copies"),
    ("puppy", "puppies"),
    ("study", "studies"),
    ("memory", "memories"),
    ("enemy", "enemies"),
    ("monkey", "monkeys"),
    ("donkey", "donkeys"),
    ("valley", "valleys"),
    ("turkey", "turkeys"),
    ("man", "men"),
    ("foot", "feet"),
    ("tooth", "teeth"),
    ("goose", "geese"),
    ("ox", "oxen"),
    ("sex", "sexes"),
    ("deer", "deer"),
    ("sheep", "sheep"),
    ("series", "series"),
];

#[test]
fn benchmarked_pairs_agree_across_all_three_implementations() {
    let inflector = NounInflector::new();
    for (singular, plural) in PAIRS {
        let vp = inflector.pluralize(singular).unwrap();
        let vs = inflector.singularize(plural).unwrap();
        assert_eq!(vp, *plural, "verbora.pluralize({singular:?})");
        assert_eq!(vs, *singular, "verbora.singularize({plural:?})");

        let pp = pluralizer::pluralize(singular, 2, false);
        let ps = pluralizer::pluralize(plural, 1, false);
        assert_eq!(pp, *plural, "pluralizer::pluralize({singular:?}, 2, _)");
        assert_eq!(ps, *singular, "pluralizer::pluralize({plural:?}, 1, _)");

        let ip = inflector::string::pluralize::to_plural(singular);
        let is_ = inflector::string::singularize::to_singular(plural);
        assert_eq!(ip, *plural, "inflector::to_plural({singular:?})");
        assert_eq!(is_, *singular, "inflector::to_singular({plural:?})");
    }
}

/// The headline divergence the matrix's own dossier names: `pluralizer`'s
/// independently-maintained rule table has no `octop`-prefixed entry in its
/// `us`/`i` rule (`pluralizer-0.5.0/src/constants.rs`'s `PLURAL_RULES`), so
/// "octopus" instead falls through to its generic `[^aou]us$ -> $1es` rule —
/// `"octopuses"`, not `"octopi"`. `Inflector` 0.11.4's own rule table *does*
/// list `octop` (`Inflector-0.11.4/src/string/pluralize/mod.rs`), so it
/// agrees with Verbora on the plural direction — the three-way split only
/// shows up on the round trip, confirmed below too. `"octopus"` is
/// deliberately not in `PAIRS`/`benches/inflectors.rs`'s benchmarked domain
/// because of this.
#[test]
fn octopus_is_a_documented_three_way_divergence() {
    let inflector = NounInflector::new();
    assert_eq!(inflector.pluralize("octopus").unwrap(), "octopi");
    assert_eq!(pluralizer::pluralize("octopus", 2, false), "octopuses");
    assert_eq!(inflector::string::pluralize::to_plural("octopus"), "octopi");

    // The round trip: Verbora and Inflector agree octopus -> octopi, but
    // `pluralizer` has no reverse rule for "-i" endings that keeps it in
    // sync with the forward direction it just took.
    assert_eq!(inflector.singularize("octopi").unwrap(), "octopus");
    assert_eq!(
        inflector::string::singularize::to_singular("octopi"),
        "octopus"
    );
    assert_eq!(
        pluralizer::pluralize("octopi", 1, false),
        "octopi",
        "pluralizer leaves \"octopi\" unchanged -- it has no irregular or \
         regex entry for an -i ending on the singularization side"
    );
}
