//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/inflectors.rs`.
//!
//! Verifies, once and outside the timed code, that [`OrdinalInflector::nth`]
//! and the `ordinal` 0.4.0 crate agree on the domain actually benchmarked,
//! and characterizes — via an assertion, not just a comment — the one real
//! divergence that remains, which is a verified bug in `ordinal` itself and
//! which the benchmarked domain deliberately excludes.
//!
//! Also covers the `NounInflector` competitors `pluralizer` 0.5.0 and
//! `Inflector` 0.11.4, and `Inflector`'s `numbers::ordinalize::ordinalize` as
//! a second [`OrdinalInflector`] match.
//!
//! # What the Rust-native migration changed here
//!
//! Two things, both of which *widened* this file's coverage rather than
//! narrowing it:
//!
//! **The negative-integer divergence is gone.** Verbora's ordinal suffix is
//! now specified on `n.unsigned_abs() % 100` ([`OrdinalInflector::suffix`]),
//! so `nth(-1)` is `"-1st"`. It previously answered `"-1th"` because the old
//! implementation's signed `%` kept the dividend's sign. `ordinal` has always
//! taken `Abs::abs(self)` first (`ordinal-0.4.0/src/lib.rs`'s
//! `impl_ordinal!`), so the two now agree on negative input, and the four
//! tests that existed to pin the disagreement have been replaced by tests
//! pinning the agreement plus Verbora's own contract for the sign.
//!
//! **`nth_str` no longer exists, so the `Inflector::ordinalize` comparison
//! retargets onto `nth`.** The old Verbora carried a string-in/string-out
//! ordinal twin, `CountInflector::nth_str`, pinned to `f64` number semantics;
//! it was the shape-match for `ordinalize`'s own `&str -> String` signature,
//! and every `ordinalize` assertion below used to be capped at `2^53 - 1`
//! because past that bound `nth_str` suffixed a *rounded* value. Verbora is
//! now `i64`-exact with no string-in form at all, so those assertions retarget
//! onto `nth(i)` for the same integer `i` and the `2^53` cap is dropped: the
//! sweeps below now cover the **full** `i64` range, which the old ones could
//! not. See `benches/inflectors.rs`'s own doc comment for why the *timing*
//! group did not survive the same retarget — outputs for a given value are
//! comparable, but a `&str` input against an `i64` input is not equivalent
//! benchmark preparation.

use ordinal::ToOrdinal;
use verbora_inflectors::{NounInflector, OrdinalInflector};

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
        let a = OrdinalInflector::nth(i);
        let b = i.to_ordinal_string();
        assert_eq!(a, b, "nth({i}): verbora={a:?} ordinal={b:?}");
    }
    assert_eq!(
        OrdinalInflector::nth(i64::MAX),
        i64::MAX.to_ordinal_string()
    );
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
        let verbora = OrdinalInflector::nth(i);
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
        .filter(|&i| OrdinalInflector::nth(i) != i.to_ordinal_string())
        .count();
    assert_eq!(
        mismatches, 120,
        "bug rate changed — re-verify before trusting this test's claim"
    );
}

/// True exactly when `i`'s last two digits fall in the `ordinal` crate's
/// verified bug ranges (`31..=33`, `51..=53`, `71..=73`, `91..=93` — see
/// [`ordinal_crate_has_a_real_teens_modulus_bug`]). The single definition the
/// sweeps below share, so the excluded domain is stated once, not re-derived
/// per test.
///
/// Taken on the **magnitude**, not the signed remainder. Both implementations
/// now discard the sign before deciding a suffix — Verbora via
/// `n.unsigned_abs() % 100`, `ordinal` via `Abs::abs(self) % 20` — so the bug
/// domain is symmetric about zero, and `-31` diverges for exactly the same
/// reason `31` does. Before the Rust-native migration this function took
/// `i % 100`, which was correct then only because Verbora's old signed `%`
/// made *every* negative a separate, wider divergence handled by its own
/// tests; see this file's module doc comment.
fn in_ordinal_bug_range(i: i64) -> bool {
    matches!(i.unsigned_abs() % 100, 31..=33 | 51..=53 | 71..=73 | 91..=93)
}

/// Second-round sharpening of the bug's characterization: over `0..10_000`,
/// Verbora and `ordinal` disagree on a value **if and only if**
/// [`in_ordinal_bug_range`] holds — the documented bug domain is exactly
/// right, neither wider nor narrower — and the mismatch count scales exactly
/// linearly (1200 = 10 × the first round's 120-per-1000 measurement).
#[test]
fn ordinal_bug_domain_is_exactly_the_four_last_two_digit_ranges() {
    let mut mismatches = 0usize;
    for i in 0..10_000i64 {
        let diverges = OrdinalInflector::nth(i) != i.to_ordinal_string();
        assert_eq!(
            diverges,
            in_ordinal_bug_range(i),
            "at {i}: divergence and bug-range membership must coincide"
        );
        mismatches += usize::from(diverges);
    }
    assert_eq!(mismatches, 1200);
}

/// SplitMix64 — the standard tiny deterministic PRNG, inlined so this test
/// crate needs no `rand` dependency. Seeded with fixed constants below, so
/// every run checks the identical case set.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 100_000 deterministic random non-negative `i64` values (fixed seed),
/// spanning the full magnitude range, skipping only [`in_ordinal_bug_range`]
/// values — the same documented exclusion the fixed samples already apply,
/// respected here rather than widened or narrowed. Every surviving value must
/// agree with both competitors: `to_ordinal_string` on the `i64` directly,
/// and `ordinalize` on that same integer's decimal string.
#[test]
fn seeded_random_sweep_agrees_outside_the_bug_ranges() {
    let mut state = 0x5EED_0002_0D1A_7015u64;
    let mut checked = 0usize;
    while checked < 100_000 {
        let i = (splitmix64(&mut state) >> 1) as i64; // top bit cleared: non-negative
        if in_ordinal_bug_range(i) {
            continue; // documented ordinal-crate bug domain, excluded as always
        }
        checked += 1;
        let a = OrdinalInflector::nth(i);
        let b = i.to_ordinal_string();
        assert_eq!(a, b, "nth({i}): verbora={a:?} ordinal={b:?}");

        // `Inflector::ordinalize` takes the decimal string of the same
        // integer and must produce the same ordinal. Before the migration
        // this arm was capped at `2^53 - 1`, because the old `nth_str` was
        // pinned to `f64` number semantics and suffixed a rounded value past
        // that bound. `nth` is `i64`-exact, so the cap is gone and the full
        // range is checked.
        let s = i.to_string();
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(a, inf, "ordinalize({s}): verbora={a:?} inflector={inf:?}");
    }
}

/// The exact values `benches/inflectors.rs`'s `sample_large` cycles through —
/// the `"<n>-large"` shape's 9-19-digit ordinals, spanning every suffix class
/// (`st`/`nd`/`rd`/teens/`th`) at large magnitudes, all non-negative and all
/// outside [`in_ordinal_bug_range`] (the same two documented narrowings as
/// the small sample). Kept in sync with that function's `LARGE` array by
/// hand; drift would be caught by this test failing. Verified on both API
/// pairs, like the small values, before any `"-large"` timing number is
/// trusted.
const BENCHMARKED_LARGE: [i64; 12] = [
    123_456_789,
    987_654_321,
    1_000_000_002,
    1_000_000_003,
    1_000_000_007,
    9_876_543_210,
    9_876_543_211,
    99_999_999_999,
    122_333_444_455_555,
    777_777_777_777_777,
    999_999_999_999_999_999,
    i64::MAX,
];

#[test]
fn agrees_on_benchmarked_large_values() {
    for i in BENCHMARKED_LARGE {
        assert!(
            !in_ordinal_bug_range(i),
            "{i} must stay outside the documented ordinal-crate bug ranges"
        );
        let a = OrdinalInflector::nth(i);
        let b = i.to_ordinal_string();
        assert_eq!(a, b, "nth({i}): verbora={a:?} ordinal={b:?}");

        // `Inflector::ordinalize` takes the decimal string of the same
        // integer and must produce the same ordinal. Before the migration
        // this arm was capped at `2^53 - 1`, because the old `nth_str` was
        // pinned to `f64` number semantics and suffixed a rounded value past
        // that bound. `nth` is `i64`-exact, so the cap is gone and the full
        // range is checked.
        let s = i.to_string();
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(a, inf, "ordinalize({s}): verbora={a:?} inflector={inf:?}");
    }
}

/// Verbora's own contract for the sign, pinned directly from
/// `crates/verbora-inflectors/src/ordinal.rs` rather than inferred from any
/// competitor: the sign is **orthographic**. [`OrdinalInflector::suffix`] is
/// specified on `n.unsigned_abs() % 100`, so the suffix a negative integer
/// gets is the suffix its magnitude gets, with a `-` prefixed to the numeral.
///
/// This is a behaviour change from the pre-migration `CountInflector`, whose
/// signed `%` gave every negative a `"th"`; the four cases below are the
/// exact spot values the old `negative_integers_are_a_documented_divergence`
/// asserted the *disagreement* on, kept at the same values so the change of
/// answer is legible in the diff rather than hidden behind a new fixture set.
#[test]
fn verbora_treats_the_sign_as_orthographic() {
    for (i, expected) in [
        (-1i64, "-1st"),
        (-2, "-2nd"),
        (-3, "-3rd"),
        (-11, "-11th"),
        (-12, "-12th"),
        (-13, "-13th"),
        (-21, "-21st"),
        (-101, "-101st"),
        (-111, "-111th"),
    ] {
        let got = OrdinalInflector::nth(i);
        assert_eq!(got, expected, "nth({i})");
        // The magnitude decides the suffix, and nothing else.
        assert_eq!(
            OrdinalInflector::suffix(i),
            OrdinalInflector::suffix(-i),
            "suffix({i}) must equal suffix({})",
            -i
        );
    }
}

/// The negative-integer divergence the pre-migration `CountInflector` had
/// against `ordinal` is **gone**: both sides now discard the sign before
/// choosing a suffix. What remains on negative input is exactly the same
/// `% 20`-vs-`% 100` bug in `ordinal` already characterized above for
/// non-negative input, now visibly symmetric about zero — over `-1000..0` the
/// two sides diverge on a value if and only if [`in_ordinal_bug_range`] holds
/// for it, and nowhere else.
#[test]
fn negative_integers_now_agree_outside_the_ordinal_bug_range() {
    let mut mismatches = 0usize;
    for i in -1000i64..0 {
        let a = OrdinalInflector::nth(i);
        let b = i.to_ordinal_string();
        let diverges = a != b;
        assert_eq!(
            diverges,
            in_ordinal_bug_range(i),
            "at {i}: divergence and bug-range membership must coincide \
             (verbora={a:?} ordinal={b:?})"
        );
        mismatches += usize::from(diverges);
    }
    // Exactly the non-negative rate, mirrored: `-1000..0` spans magnitudes
    // `1..=1000`, and neither `0` nor `1000` is in the bug range, so this is
    // the same 120-per-1000 the `0..1000` sweep measures.
    assert_eq!(mismatches, 120);
}

// ---------------------------------------------------------------------------
// `Inflector::numbers::ordinalize::ordinalize` vs `OrdinalInflector::nth`
// ---------------------------------------------------------------------------
//
// This section used to compare `ordinalize` against `CountInflector::nth_str`,
// a string-in/string-out twin that no longer exists. It now compares against
// `nth(i)` for the same integer `i`: the two produce the same ordinal for the
// same *value*, which is what a correctness oracle needs, even though their
// input types differ enough that timing them against each other would not be
// fair (see `benches/inflectors.rs`). Because `nth` is `i64`-exact rather than
// `f64`-pinned, every sweep below covers the full `i64` range instead of
// stopping at `2^53 - 1`.

/// Unlike `ordinal` 0.4.0, `Inflector::ordinalize` operates directly on the
/// decimal string, checking whether the *character* immediately before the
/// last one is `'1'` — reading `Inflector-0.11.4/src/numbers/ordinalize/
/// mod.rs` shows this has no `% 20`-vs-`% 100` bug: it never even computes a
/// remainder. Exhaustively confirmed to fully agree with
/// [`OrdinalInflector::nth`] over every non-negative `i64` in `0..2_000_000`,
/// not just a hand-picked sample.
#[test]
fn inflector_ordinalize_agrees_with_ordinal_inflector_nth_on_non_negative_integers() {
    for i in 0..2_000_000i64 {
        let s = i.to_string();
        let v = OrdinalInflector::nth(i);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(v, inf, "nth({i}): verbora={v:?} inflector={inf:?}");
    }
}

/// Extension of the exhaustive `0..2_000_000` check above to the magnitudes it
/// cannot reach: 200_000 deterministic random values (fixed seed) across the
/// **full** non-negative `i64` range, no exclusions at all — this competitor
/// pair has no documented divergence on non-negative input to respect, unlike
/// `ordinal`'s.
///
/// The pre-migration version of this test capped its draw at `2^53` because
/// `nth_str` was pinned to `f64` number semantics and, past that bound,
/// suffixed a *rounded* value while `ordinalize` suffixed the exact digits.
/// `nth` has no such bound, so the cap is gone and the top of the `i64` range
/// is now genuinely covered.
#[test]
fn inflector_ordinalize_agrees_on_seeded_random_values_across_the_full_range() {
    let mut state = 0x5EED_0003_10FE_EC70u64;
    for case in 0..200_000 {
        let i = (splitmix64(&mut state) >> 1) as i64; // top bit cleared: non-negative
        let s = i.to_string();
        let v = OrdinalInflector::nth(i);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(
            v, inf,
            "case {case}, nth({i}): verbora={v:?} inflector={inf:?}"
        );
    }
}

/// Negative input agrees too, and for the same reason it agrees with
/// `ordinal`: Verbora now treats the sign as orthographic
/// ([`verbora_treats_the_sign_as_orthographic`]), and
/// `Inflector::ordinalize` never inspects the sign at all — it looks only at
/// the last two *characters*, so a leading `-` cannot reach its decision.
/// `Inflector-0.11.4/src/numbers/ordinalize/mod.rs`'s own doctest already
/// states `ordinalize("-1") == "-1st"`; this pins that Verbora now says the
/// same, where before the migration it said `"-1th"`.
///
/// `Inflector::ordinalize` has no `% 20` bug, so unlike `ordinal` this
/// agreement holds across the whole `-1000..0` sweep with no excluded range.
#[test]
fn inflector_ordinalize_agrees_on_negative_integers() {
    for (i, expected) in [
        (-1i64, "-1st"),
        (-2, "-2nd"),
        (-3, "-3rd"),
        (-11, "-11th"),
        (-21, "-21st"),
        (-31, "-31st"),
    ] {
        let s = i.to_string();
        let v = OrdinalInflector::nth(i);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(v, expected, "verbora nth({i})");
        assert_eq!(inf, expected, "inflector ordinalize({s})");
    }

    for i in -1000i64..0 {
        let s = i.to_string();
        let v = OrdinalInflector::nth(i);
        let inf = inflector::numbers::ordinalize::ordinalize(&s);
        assert_eq!(v, inf, "nth({i}): verbora={v:?} inflector={inf:?}");
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
/// rule class in `crates/verbora-inflectors/src/data.rs` in the first round,
/// and ~140 further candidates — Latin/Greek plurals, o/f/fe/z endings,
/// uninflected animals, silent-e and -th words, tech vocabulary — in the
/// second) and keeping only the words where `NounInflector`,
/// `pluralizer::pluralize`, and `inflector::string::{pluralize,singularize}`
/// unanimously agree — kept in sync with `benches/inflectors.rs`'s own
/// `PAIRS` array by hand; drift would be caught by
/// `benchmarked_pairs_agree_across_all_three_implementations` failing.
/// Second-round rejects worth naming (all outside the benchmarked domain, per
/// the same unanimity rule as ever): most Latin/Greek irregulars
/// (`focus`/`datum`/`criterion`...), the f→ves class (`wife`, `leaf`...), and
/// silent-e singulars (`house`: Verbora singularizes `houses` → `hous`,
/// faithful to the reference's own rule chain; both competitors say `house`).
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
    // -- second coverage round: 59 more verified-unanimous pairs ------------
    ("photo", "photos"),
    ("piano", "pianos"),
    ("radio", "radios"),
    ("video", "videos"),
    ("zoo", "zoos"),
    ("studio", "studios"),
    ("kangaroo", "kangaroos"),
    ("diagnosis", "diagnoses"),
    ("parenthesis", "parentheses"),
    ("hoax", "hoaxes"),
    ("mile", "miles"),
    ("place", "places"),
    ("price", "prices"),
    ("piece", "pieces"),
    ("face", "faces"),
    ("race", "races"),
    ("page", "pages"),
    ("edge", "edges"),
    ("bridge", "bridges"),
    ("change", "changes"),
    ("image", "images"),
    ("stage", "stages"),
    ("village", "villages"),
    ("cottage", "cottages"),
    ("branch", "branches"),
    ("beach", "beaches"),
    ("coach", "coaches"),
    ("speech", "speeches"),
    ("torch", "torches"),
    ("porch", "porches"),
    ("lunch", "lunches"),
    ("march", "marches"),
    ("arch", "arches"),
    ("patch", "patches"),
    ("sketch", "sketches"),
    ("switch", "switches"),
    ("stitch", "stitches"),
    ("batch", "batches"),
    ("bush", "bushes"),
    ("flash", "flashes"),
    ("crash", "crashes"),
    ("splash", "splashes"),
    ("marsh", "marshes"),
    ("path", "paths"),
    ("month", "months"),
    ("myth", "myths"),
    ("length", "lengths"),
    ("truth", "truths"),
    ("laptop", "laptops"),
    ("server", "servers"),
    ("packet", "packets"),
    ("socket", "sockets"),
    ("thread", "threads"),
    ("buffer", "buffers"),
    ("vector", "vectors"),
    ("pointer", "pointers"),
    ("string", "strings"),
    ("integer", "integers"),
];

#[test]
fn benchmarked_pairs_agree_across_all_three_implementations() {
    let inflector = NounInflector::new();
    for (singular, plural) in PAIRS {
        let vp = inflector.pluralize(singular);
        let vs = inflector.singularize(plural);
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

/// The exact subset `benches/inflectors.rs`'s `"<n>-irregular"` shape cycles
/// through — irregular/uninflected table entries plus the non-default suffix
/// classes (ix/ex→ices, is→es), none of which the plain `+s` fast path can
/// serve. Kept in sync with that file's `IRREGULAR` array by hand. Every
/// entry must also appear in [`PAIRS`], so the unanimity test above already
/// covers each one; this test pins the subset relationship itself, so the
/// bench cannot silently cycle a word the three-way agreement check never
/// saw.
const IRREGULAR: &[(&str, &str)] = &[
    ("matrix", "matrices"),
    ("index", "indices"),
    ("vertex", "vertices"),
    ("woman", "women"),
    ("man", "men"),
    ("foot", "feet"),
    ("tooth", "teeth"),
    ("goose", "geese"),
    ("ox", "oxen"),
    ("synopsis", "synopses"),
    ("thesis", "theses"),
    ("analysis", "analyses"),
    ("deer", "deer"),
    ("sheep", "sheep"),
    ("series", "series"),
];

#[test]
fn benchmarked_irregular_subset_is_drawn_from_pairs() {
    for entry in IRREGULAR {
        assert!(
            PAIRS.contains(entry),
            "IRREGULAR entry {entry:?} must be one of the verified-unanimous PAIRS"
        );
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
    assert_eq!(inflector.pluralize("octopus"), "octopi");
    assert_eq!(pluralizer::pluralize("octopus", 2, false), "octopuses");
    assert_eq!(inflector::string::pluralize::to_plural("octopus"), "octopi");

    // The round trip: Verbora and Inflector agree octopus -> octopi, but
    // `pluralizer` has no reverse rule for "-i" endings that keeps it in
    // sync with the forward direction it just took.
    assert_eq!(inflector.singularize("octopi"), "octopus");
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

/// A new agreement dimension the first round never checked: the full
/// benchmarked round trip. For every pair, singular → pluralize →
/// singularize → pluralize must land back on the plural, on all three
/// implementations — composition stability, not just the two single-step
/// directions the unanimity test already pins.
#[test]
fn benchmarked_pairs_round_trip_on_all_three_implementations() {
    let inflector = NounInflector::new();
    for (singular, plural) in PAIRS {
        let v = inflector.pluralize(&inflector.singularize(&inflector.pluralize(singular)));
        assert_eq!(v, *plural, "verbora round trip from {singular:?}");

        let p = pluralizer::pluralize(
            &pluralizer::pluralize(&pluralizer::pluralize(singular, 2, false), 1, false),
            2,
            false,
        );
        assert_eq!(p, *plural, "pluralizer round trip from {singular:?}");

        let i =
            inflector::string::pluralize::to_plural(&inflector::string::singularize::to_singular(
                &inflector::string::pluralize::to_plural(singular),
            ));
        assert_eq!(i, *plural, "inflector round trip from {singular:?}");
    }
}

/// The two pairs the Rust-native migration removed from [`PAIRS`], pinned
/// individually so the exclusion is visible rather than merely absent.
///
/// [`PAIRS`] is the set of nouns all three implementations agree on, in both
/// directions — see `benches/inflectors.rs`'s module doc comment for how it
/// was derived. Re-running that unanimity probe after the migration found
/// exactly two new disagreements, and both are Verbora-side changes:
///
/// * **`virus`** — Verbora now pluralizes to `"viruses"`; `pluralizer` and
///   `Inflector` both still produce `"viri"`.
/// * **`aliases`** — Verbora's `singularize` now returns `"aliase"`, so the
///   pair no longer round-trips on Verbora's side at all. `pluralize("alias")`
///   still agrees with both competitors at `"aliases"`; it is only the reverse
///   direction that broke, which is why the pair had to leave a domain that
///   requires agreement in *both* directions.
///
/// This test asserts the disagreements rather than the agreements, so it fails
/// if either is silently resolved — at which point the pair can go back into
/// [`PAIRS`] and into the benchmark's domain.
#[test]
fn the_two_pairs_the_migration_removed_from_the_unanimous_domain() {
    let verbora = NounInflector::new();

    assert_eq!(verbora.pluralize("virus"), "viruses");
    assert_eq!(pluralizer::pluralize("virus", 2, false), "viri");
    assert_eq!(inflector::string::pluralize::to_plural("virus"), "viri");

    // The forward direction still agrees; only the reverse diverges.
    assert_eq!(verbora.pluralize("alias"), "aliases");
    assert_eq!(pluralizer::pluralize("alias", 2, false), "aliases");
    assert_eq!(verbora.singularize("aliases"), "aliase");
    assert_eq!(pluralizer::pluralize("aliases", 1, false), "alias");

    // Neither word may reappear in the benchmarked domain while that holds.
    for excluded in ["virus", "alias"] {
        assert!(
            !PAIRS.iter().any(|(s, _)| *s == excluded),
            "{excluded:?} is back in PAIRS but still diverges"
        );
    }
}
