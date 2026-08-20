// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the inflectors.
//!
//! The interesting axis is not input *size* — tokens are words — but which of
//! the pipeline's four stages resolves the call. A word on the invariant list
//! stops after a binary search; an irregular stops after two; a regular form
//! runs up to twenty-one regexes; and the case-restoration pass at the end costs
//! whatever the token's alphabet costs. Benchmarking one undifferentiated word
//! list would average all of that into a single number that no change could be
//! attributed to, so the corpora below are split by the path they take.
//!
//! Two further axes are measured because the crate makes a deliberate claim
//! about each:
//!
//! * **Construction.** The rule tables are compiled once per process and shared
//!   by every instance, so `new()` should be flat and independent of language —
//!   French has 595 lexical entries and English 49.
//! * **Allocation.** `pluralize` allocates its result; `pluralize_into` appends
//!   to a caller-owned buffer. The crate documentation says the difference is
//!   allocation and nothing else; this is where that is checked.
//!
//! The bulk corpus is `benches/data/words.json`, the shared word list every
//! other harness reads, so all numbers are measured on byte-identical input.

use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_inflectors::{
    Gender, NounInflector, NounInflectorFr, NounInflectorJa, OrdinalInflector, OrdinalInflectorFr,
    PresentVerbInflector, Rule, SingularPluralInflector,
};

/// Words that stop at the invariant list — one binary search, no regex.
const INVARIANT: &[&str] = &[
    "deer", "fish", "series", "sheep", "trout", "tuna", "salmon", "news", "money", "rice",
    "species", "swine", "corps", "graffiti",
];

/// Words that stop at the irregular table — a miss on the invariant list, then
/// a hit.
const IRREGULAR: &[&str] = &[
    "child",
    "person",
    "mouse",
    "louse",
    "ox",
    "foot",
    "tooth",
    "goose",
    "ephemeris",
];

/// Words that reach the regular rules, spread across early and late entries so
/// the first-match-wins scan is exercised at both ends.
const REGULAR: &[&str] = &[
    "party",
    "fly",
    "victory",
    "church",
    "box",
    "quiz",
    "tomato",
    "radius",
    "cactus",
    "matrix",
    "index",
    "workman",
    "woman",
    "knifelike",
    "antenna",
    "synopsis",
    "buffalo",
    "day",
    "journey",
    "criterion",
    "curriculum",
    "wolf",
    "knife",
];

/// Words no rule claims until the final append — the deepest path.
const FALLBACK: &[&str] = &[
    "hacker",
    "table",
    "window",
    "keyboard",
    "mountain",
    "river",
    "compiler",
    "benchmark",
    "allocation",
    "throughput",
];

/// The same words capitalised and upper-cased, which selects the other two
/// case-restoration modes and forces the case pass to rewrite every character.
fn case_shapes() -> Vec<String> {
    let mut out = Vec::new();
    for word in FALLBACK.iter().chain(REGULAR) {
        out.push((*word).to_owned());
        let mut capitalised = word.to_uppercase();
        capitalised.replace_range(1.., &word[1..]);
        out.push(capitalised);
        out.push(word.to_uppercase());
    }
    out
}

/// Non-ASCII input, which leaves every ASCII fast path in the crate: the
/// lowercase fold allocates, and case restoration goes through full Unicode
/// mapping.
const FRENCH: &[&str] = &[
    "cheval",
    "amiral",
    "bijou",
    "cadeau",
    "vitrail",
    "rhinocéros",
    "vérité",
    "orange",
    "œil",
    "landau",
    "pneu",
    "carnaval",
    "général",
    "manteau",
    "trou",
];

/// Japanese tokens, whose rules are all `^(.+)SUFFIX$` over multi-byte text.
const JAPANESE: &[&str] = &[
    "私",
    "人",
    "神",
    "友達",
    "わたし",
    "先生",
    "野郎",
    "人間",
    "圭一",
    "貴様",
    "かたち",
    "配達",
];

/// The shared bulk corpus, also read by every sibling harness.
fn bulk_words() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("words array")
        .iter()
        .take(4000)
        .map(|w| w.as_str().expect("string word").to_owned())
        .collect()
}

/// Runs one corpus through both directions of one inflector.
fn bench_corpus<I>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    label: &str,
    inflector: &I,
    corpus: &[String],
) where
    I: SingularPluralInflector,
{
    group.throughput(Throughput::Elements(corpus.len() as u64));
    group.bench_with_input(BenchmarkId::new("pluralize", label), corpus, |b, words| {
        b.iter(|| {
            for word in words {
                black_box(inflector.pluralize(black_box(word)));
            }
        });
    });
    group.bench_with_input(
        BenchmarkId::new("singularize", label),
        corpus,
        |b, words| {
            b.iter(|| {
                for word in words {
                    black_box(inflector.singularize(black_box(word)));
                }
            });
        },
    );
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// English nouns, split by which stage of the pipeline resolves the call.
fn bench_english_paths(c: &mut Criterion) {
    let inflector = NounInflector::new();
    let mut group = c.benchmark_group("noun-en/by-path");

    for (label, corpus) in [
        ("invariant", owned(INVARIANT)),
        ("irregular", owned(IRREGULAR)),
        ("regular", owned(REGULAR)),
        ("fallback", owned(FALLBACK)),
        ("case-shapes", case_shapes()),
    ] {
        bench_corpus(&mut group, label, &inflector, &corpus);
    }

    group.finish();
}

/// The other three inflectors, on the input each is written for.
fn bench_languages(c: &mut Criterion) {
    let mut group = c.benchmark_group("by-language");

    bench_corpus(&mut group, "en", &NounInflector::new(), &owned(FALLBACK));
    bench_corpus(&mut group, "fr", &NounInflectorFr::new(), &owned(FRENCH));
    bench_corpus(&mut group, "ja", &NounInflectorJa::new(), &owned(JAPANESE));
    bench_corpus(
        &mut group,
        "verb",
        &PresentVerbInflector::new(),
        &owned(&[
            "catch", "go", "cash", "annex", "buzz", "claim", "drink", "fly", "try", "pass",
        ]),
    );

    group.finish();
}

/// Bulk throughput over the shared corpus, and the one comparison the
/// documentation's "Choosing the right API" table rests on.
fn bench_bulk(c: &mut Criterion) {
    let words = bulk_words();
    let inflector = NounInflector::new();
    let mut group = c.benchmark_group("noun-en/bulk");
    group.throughput(Throughput::Elements(words.len() as u64));

    group.bench_function("pluralize", |b| {
        b.iter(|| {
            for word in &words {
                black_box(inflector.pluralize(black_box(word)));
            }
        });
    });

    // The buffer form: one allocation for the whole corpus rather than one per
    // word. This is the only reason `*_into` exists.
    group.bench_function("pluralize_into", |b| {
        let mut buffer = String::with_capacity(64);
        b.iter(|| {
            for word in &words {
                buffer.clear();
                inflector.pluralize_into(black_box(word), &mut buffer);
                black_box(buffer.as_str());
            }
        });
    });

    group.finish();
}

/// Construction, which sharing the compiled tables is supposed to make free.
///
/// French is the case to watch: its lexical list is 595 entries against
/// English's 49. Flat across languages means the sharing works.
fn bench_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("construct");
    group.bench_function("NounInflector", |b| {
        b.iter(|| black_box(NounInflector::new()));
    });
    group.bench_function("NounInflectorFr", |b| {
        b.iter(|| black_box(NounInflectorFr::new()));
    });
    group.bench_function("NounInflectorJa", |b| {
        b.iter(|| black_box(NounInflectorJa::new()));
    });
    group.bench_function("PresentVerbInflector", |b| {
        b.iter(|| black_box(PresentVerbInflector::new()));
    });
    group.finish();
}

/// Caller-added rules, which are consulted before every built-in table and so
/// sit on the hot path for every call once present.
fn bench_custom_rules(c: &mut Criterion) {
    let mut inflector = NounInflector::new();
    for pattern in ["(?i)(code|ware)$", "(?i)^gizmo$", "(?i)(widget)$"] {
        inflector.add_plural(Rule::new(pattern, "${0}z").expect("valid pattern"));
    }
    let corpus = owned(FALLBACK);

    let mut group = c.benchmark_group("custom-rules");
    group.throughput(Throughput::Elements(corpus.len() as u64));
    group.bench_function("three-rules-all-missing", |b| {
        b.iter(|| {
            for word in &corpus {
                black_box(inflector.pluralize(black_box(word)));
            }
        });
    });
    group.bench_function("compile", |b| {
        b.iter(|| {
            black_box(Rule::new("(?i)^(bij|caill|ch|gen|hib)oux$", "${1}ou").ok());
        });
    });
    group.finish();
}

/// The ordinal inflectors: the allocating form, the buffer form and the
/// suffix-only form, which is the whole reason all three exist.
fn bench_ordinals(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordinal");

    group.bench_function("nth", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(OrdinalInflector::nth(black_box(i)));
            }
        });
    });
    group.bench_function("nth_into", |b| {
        let mut buffer = String::with_capacity(32);
        b.iter(|| {
            for i in 0..100i64 {
                buffer.clear();
                OrdinalInflector::nth_into(black_box(i), &mut buffer);
                black_box(buffer.as_str());
            }
        });
    });
    group.bench_function("suffix", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(OrdinalInflector::suffix(black_box(i)));
            }
        });
    });
    group.bench_function("fr/nth", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(OrdinalInflectorFr::nth(black_box(i), Gender::Masculine));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_english_paths,
    bench_languages,
    bench_bulk,
    bench_construction,
    bench_custom_rules,
    bench_ordinals
);
criterion_main!(benches);
