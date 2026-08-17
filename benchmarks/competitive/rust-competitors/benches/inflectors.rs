//! Verbora vs. real, pinned third-party Rust competitors — inflectors.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.5 for the research dossier.
//!
//! # `CountInflector`: two competitors now
//!
//! `ordinal` 0.4.0 (heaths/ordinal-rs) is the matrix's single `Yes` — full
//! algorithmic equivalence — row in the entire Inflectors group: "same narrow
//! job, actively maintained... best single match found." This pass adds a
//! second, genuinely different comparison rather than a redundant one:
//! `Inflector` 0.11.4's `numbers::ordinalize::ordinalize` operates on the
//! decimal **string** directly (`&str -> String`), the same shape as
//! `CountInflector::nth_str`, not `CountInflector::nth`'s `i64 -> String` —
//! so `bench_count_inflector_nth_str` is a distinct group from
//! `bench_count_inflector_nth`, not a second entry in the same one. Found
//! while implementing this pass, not in the matrix's original dossier:
//! `ordinalize` has **no** version of `ordinal`'s `% 20`/`% 100` bug (it
//! never computes a remainder at all — see `../tests/
//! inflectors_correctness.rs`'s own doc comment) and fully agrees with
//! `CountInflector::nth_str` over every non-negative `i64` in `0..2_000_000`,
//! exhaustively checked in that same file before this benchmark's numbers are
//! trusted.
//!
//! # `NounInflector`: two competitors, narrowed to a verified-agreeing domain
//!
//! `pluralizer` 0.5.0 and `Inflector` 0.11.4 (`string::{pluralize,
//! singularize}`) are both matrix `Partial` — same regex-suffix-rule-chain-
//! plus-irregulars strategy as Verbora's port, but independently maintained
//! rule tables that genuinely disagree with Verbora (and, on the round trip,
//! sometimes with each other) on contested irregulars — the matrix's own
//! headline example, `"octopus"`, is a real three-way split (`pluralizer`:
//! `"octopuses"`; Verbora and `Inflector` agree: `"octopi"`, but only
//! `Inflector`'s *forward* rule table happens to list `octop` at all — see
//! `../tests/inflectors_correctness.rs`'s `octopus_is_a_documented_
//! three_way_divergence`). Publishing a timing number for a comparison whose
//! sides can disagree on the answer is exactly the "different work" this
//! project's fairness rules forbid, so [`PAIRS`] below is not a hand-picked
//! convenient sample — it is the output of probing a much larger candidate
//! list (Verbora's own irregular/ambiguous-table entries plus ~120 common
//! regular and irregular English nouns spanning every rule class in
//! `crates/verbora-inflectors/src/data.rs`) and keeping only the words where
//! all three implementations unanimously agree on *both* `pluralize` and
//! `singularize`, verified in `../tests/inflectors_correctness.rs`'s
//! `benchmarked_pairs_agree_across_all_three_implementations`, which is kept
//! in sync with this file's `PAIRS` array by hand — drift would be caught by
//! that test failing. `inflector-plus` (matrix: `Selected cases`, a weaker
//! fork/continuation of `Inflector`) is deliberately not pinned: once
//! `Inflector` itself is benchmarked, `inflector-plus` would add a second,
//! strictly-inferior implementation of the identical comparison for no
//! additional evidence — the same "redundant once a stronger match already
//! exists" reasoning this file already applies to `ordinal`/`Inflector::
//! ordinalize` above.
//!
//! `PresentVerbInflector`, `NounInflectorFr`, `NounInflectorJa` remain matrix
//! `NO FAIR COMPETITOR FOUND` rows — no dedicated Rust crate exists for any
//! of the three. See `docs/PERFORMANCE.md`'s Inflectors section for the
//! honest reasoning on each.
//!
//! # Two real `CountInflector`/`ordinal` divergences — and the domain that
//! sidesteps both
//!
//! **Negative integers.** For *negative* integers not ending in `11`/`12`/
//! `13`, Verbora and `ordinal` genuinely disagree: `CountInflector::nth(-1)`
//! is `"-1th"` (the reference's `%` keeps the dividend's sign, so no negative
//! number ever matches the `1`/`2`/`3` cases — see `count.rs`'s own doc
//! comment), while `ordinal` takes the input's absolute value first, so
//! `(-1i64).to_ordinal_string()` is `"-1st"`. `Inflector::ordinalize` shares
//! this same shape of divergence, independently verified in
//! `../tests/inflectors_correctness.rs`.
//!
//! **A real bug in `ordinal` 0.4.0, found while verifying this benchmark, not
//! in the matrix's dossier.** Its teens-exception check uses `n % 20` where
//! the algorithm requires `n % 100` (`ordinal-0.4.0/src/lib.rs`'s
//! `impl_ordinal!`), so any non-negative integer whose last two digits fall in
//! `31..=33`, `51..=53`, `71..=73` or `91..=93` gets an incorrect `"th"`:
//! `31.to_ordinal_string()` is `"31th"`, not `"31st"`. Exhaustively counted
//! over `0..1000` in `../tests/inflectors_correctness.rs`:
//! **120 of 1000 (12%)** of non-negative integers are affected. This is
//! reported here in full, per the project's "no cherry-picking" rule, rather
//! than quietly worked around — see `docs/PERFORMANCE.md`'s Inflectors
//! section.
//!
//! Both divergences are verified explicitly, with assertions, not just
//! described, in `../tests/inflectors_correctness.rs`. This benchmark's
//! domain — the [`sample`] function's fixed, hand-picked value set — was
//! chosen *before* the bug above was found and, by chance, avoids every
//! affected value; `agrees_on_benchmarked_values` is the regression check
//! that keeps that true. The domain is non-negative (sidestepping the first
//! divergence deliberately: the overwhelming common real-world use of an
//! ordinal formatter — ranks, page numbers, "1st place" — is non-negative
//! anyway) and bug-range-free (sidestepping the second, now that it is
//! known) — exactly as `distance.rs` narrows to ASCII input and says so.
//!
//! # Shape
//!
//! Four groups, each with `BenchmarkId::new(competitor, n)` for `n` in
//! [`SIZES`], throughput in elements:
//!
//! - `count_inflector_nth` — `n` calls to `nth`/`to_ordinal_string` over the
//!   fixed non-negative range from [`sample`].
//! - `count_inflector_nth_str` — the same range, decimal-string-formatted
//!   first, `n` calls to `nth_str`/`ordinalize`.
//! - `noun_inflector_pluralize` / `noun_inflector_singularize` — `n` calls to
//!   `pluralize`/`singularize`, cycling through [`PAIRS`]'s verified-agreeing
//!   singular/plural forms.
//!
//! Every call's input and output is wrapped in `black_box`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use ordinal::ToOrdinal;
use verbora_inflectors::{CountInflector, NounInflector};

/// Batch sizes: `nth` calls per measured iteration. Matches `distance.rs`'s
/// `SIZES` so `scripts/collect-results.py`'s hardcoded size set finds every
/// directory.
const SIZES: [usize; 5] = [4, 16, 64, 256, 1024];

/// `n` non-negative integers spanning every suffix class (units, teens
/// exception, round hundreds/thousands), cycled rather than sequential so
/// every batch size exercises the same distribution of `1st`/`2nd`/`3rd`/`th`
/// outcomes.
fn sample(n: usize) -> Vec<i64> {
    const BASE: [i64; 24] = [
        0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 20, 21, 22, 23, 100, 101, 111, 112, 113, 121,
        1000, 1_000_000,
    ];
    BASE.iter().cycle().take(n).copied().collect()
}

fn bench_count_inflector(c: &mut Criterion) {
    let mut g = c.benchmark_group("count_inflector_nth");
    for n in SIZES {
        let nums = sample(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &nums, |bch, nums| {
            bch.iter(|| {
                nums.iter()
                    .map(|&i| CountInflector::nth(black_box(i)).len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("ordinal", n), &nums, |bch, nums| {
            bch.iter(|| {
                nums.iter()
                    .map(|&i| black_box(i).to_ordinal_string().len())
                    .sum::<usize>()
            });
        });
    }
    g.finish();
}

/// `count_inflector_nth_str`: the same [`sample`] range, pre-formatted as
/// decimal strings (formatting happens once, outside the timed closure, so
/// both sides measure only the ordinal-suffix decision, not `i64::to_string`)
/// — `Inflector::ordinalize`'s real API shape is `&str -> String`, not
/// `i64 -> String`, so `nth_str` (`CountInflector::nth`'s own string-in
/// twin) is the fair Verbora counterpart, not `nth`.
fn bench_count_inflector_nth_str(c: &mut Criterion) {
    let mut g = c.benchmark_group("count_inflector_nth_str");
    for n in SIZES {
        let strs: Vec<String> = sample(n).iter().map(i64::to_string).collect();
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &strs, |bch, strs| {
            bch.iter(|| {
                strs.iter()
                    .map(|s| CountInflector::nth_str(black_box(s)).len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("inflector", n), &strs, |bch, strs| {
            bch.iter(|| {
                strs.iter()
                    .map(|s| inflector::numbers::ordinalize::ordinalize(black_box(s)).len())
                    .sum::<usize>()
            });
        });
    }
    g.finish();
}

/// Verified-agreeing (singular, plural) pairs — see this file's own module
/// doc comment and `../tests/inflectors_correctness.rs`'s
/// `benchmarked_pairs_agree_across_all_three_implementations`, which is kept
/// in sync with this array by hand.
const PAIRS: &[(&str, &str)] = &[
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

/// `n` singular forms from [`PAIRS`], cycled.
fn plural_sample(n: usize) -> Vec<&'static str> {
    PAIRS.iter().map(|(s, _)| *s).cycle().take(n).collect()
}

/// `n` plural forms from [`PAIRS`], cycled in the same order as
/// [`plural_sample`] so both sample functions exercise the same distribution
/// of rule classes at every size.
fn singular_sample(n: usize) -> Vec<&'static str> {
    PAIRS.iter().map(|(_, p)| *p).cycle().take(n).collect()
}

fn bench_noun_inflector_pluralize(c: &mut Criterion) {
    let verbora = NounInflector::new();
    let mut g = c.benchmark_group("noun_inflector_pluralize");
    for n in SIZES {
        let words = plural_sample(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| verbora.pluralize(black_box(w)).unwrap().len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("pluralizer", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| pluralizer::pluralize(black_box(w), 2, false).len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("inflector", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| inflector::string::pluralize::to_plural(black_box(w)).len())
                    .sum::<usize>()
            });
        });
    }
    g.finish();
}

fn bench_noun_inflector_singularize(c: &mut Criterion) {
    let verbora = NounInflector::new();
    let mut g = c.benchmark_group("noun_inflector_singularize");
    for n in SIZES {
        let words = singular_sample(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| verbora.singularize(black_box(w)).unwrap().len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("pluralizer", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| pluralizer::pluralize(black_box(w), 1, false).len())
                    .sum::<usize>()
            });
        });
        g.bench_with_input(BenchmarkId::new("inflector", n), &words, |bch, words| {
            bch.iter(|| {
                words
                    .iter()
                    .map(|w| inflector::string::singularize::to_singular(black_box(w)).len())
                    .sum::<usize>()
            });
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_count_inflector,
    bench_count_inflector_nth_str,
    bench_noun_inflector_pluralize,
    bench_noun_inflector_singularize,
);
criterion_main!(benches);
