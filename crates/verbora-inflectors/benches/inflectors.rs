// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the inflectors.
//!
//! The interesting axis is not input *size* — tokens are words — but which of
//! `ize`'s four stages resolves the call. A word on the ambiguous list stops
//! after a binary search; an irregular stops after two; a regular form runs up
//! to twelve translated regexes; and the case-restoration pass at the end costs
//! whatever the token's alphabet costs. Benchmarking one undifferentiated word
//! list would average all of that into a single number that no change could be
//! attributed to, so the corpora below are split by the path they take.
//!
//! Two further axes are measured because the port deliberately departs from the
//! reference on both:
//!
//! * **Construction.** The reference rebuilds every rule, both `FormSet`s and the
//!   ambiguous array in each constructor — 744 entries for French. Here the
//!   tables are compiled once per process and shared, so `new()` should be flat
//!   and independent of language.
//! * **Allocation.** `pluralize` allocates its result; `pluralize_into` appends
//!   to a caller-owned buffer, which is the shape a batch job wants.
//!
//! The bulk corpus is `benches/data/words.json`, the shared word list every
//! other harness reads, so all numbers are measured on byte-identical input.

use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_inflectors::{
    CountInflector, CountInflectorFr, NounInflector, NounInflectorFr, NounInflectorJa,
    PresentVerbInflector, Rule,
};

/// Words that stop at the ambiguous list — one binary search, no regex.
const AMBIGUOUS: &[&str] = &[
    "deer", "fish", "series", "sheep", "trout", "tuna", "salmon", "news", "money", "rice",
    "species", "swine", "corps", "graffiti",
];

/// Words that stop at the irregular table — a search plus a miss on the
/// ambiguous list.
const IRREGULAR: &[&str] = &[
    "child", "man", "person", "mouse", "ox", "foot", "tooth", "goose", "leaf", "wolf", "knife",
    "life", "wife", "shelf", "thief", "hero", "torso",
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
];

/// Words no rule claims until the final `(.*)` catch-all — the deepest path.
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
/// `restoreCase` branches and forces the case pass to rewrite every character.
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
/// lowercase fold allocates, and the case-restoration pass goes through full
/// Unicode mapping.
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
    I: verbora_inflectors::SingularPluralInflector,
{
    group.throughput(Throughput::Elements(corpus.len() as u64));
    group.bench_with_input(BenchmarkId::new("pluralize", label), corpus, |b, words| {
        b.iter(|| {
            for word in words {
                black_box(inflector.pluralize(black_box(word)).ok());
            }
        });
    });
    group.bench_with_input(
        BenchmarkId::new("singularize", label),
        corpus,
        |b, words| {
            b.iter(|| {
                for word in words {
                    black_box(inflector.singularize(black_box(word)).ok());
                }
            });
        },
    );
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// English nouns, split by which stage of `ize` resolves the call.
fn bench_english_paths(c: &mut Criterion) {
    let inflector = NounInflector::new();
    let mut group = c.benchmark_group("noun-en/by-path");

    for (label, corpus) in [
        ("ambiguous", owned(AMBIGUOUS)),
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

/// Bulk throughput over the shared corpus, for comparison against the reference.
fn bench_bulk(c: &mut Criterion) {
    let words = bulk_words();
    let inflector = NounInflector::new();
    let mut group = c.benchmark_group("noun-en/bulk");
    group.throughput(Throughput::Elements(words.len() as u64));

    group.bench_function("pluralize", |b| {
        b.iter(|| {
            for word in &words {
                black_box(inflector.pluralize(black_box(word)).ok());
            }
        });
    });

    // The buffer form: one allocation for the whole corpus rather than one per
    // word. This is the reason `*_into` exists.
    group.bench_function("pluralize_into", |b| {
        let mut buffer = String::with_capacity(64);
        b.iter(|| {
            for word in &words {
                buffer.clear();
                inflector
                    .pluralize_into(black_box(word), &mut buffer)
                    .expect("corpus has no empty words");
                black_box(buffer.as_str());
            }
        });
    });

    group.finish();
}

/// Construction, which the port makes free by sharing the compiled tables.
///
/// The French case is the one to watch: The reference rebuilds a 744-entry array
/// and ten regexes per instance, so a per-instance port would show French an
/// order of magnitude above English here. Flat means the sharing works.
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
    for pattern in ["(code|ware)", "^gizmo$", "widget"] {
        inflector.add_plural(Rule::from_pattern(pattern, true, "$1z").expect("valid pattern"));
    }
    let corpus = owned(FALLBACK);

    let mut group = c.benchmark_group("custom-rules");
    group.throughput(Throughput::Elements(corpus.len() as u64));
    group.bench_function("three-rules-all-missing", |b| {
        b.iter(|| {
            for word in &corpus {
                black_box(inflector.pluralize(black_box(word)).ok());
            }
        });
    });
    group.bench_function("translate-and-compile", |b| {
        b.iter(|| {
            black_box(Rule::from_pattern("^(bij|caill|ch|gen|hib)oux$", true, "$1ou").ok());
        });
    });
    group.finish();
}

/// The ordinal inflectors, which allocate exactly one string per call.
fn bench_count(c: &mut Criterion) {
    let mut group = c.benchmark_group("count");

    group.bench_function("nth/i64", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(CountInflector::nth(black_box(i)));
            }
        });
    });
    group.bench_function("nth_form/i64", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(CountInflector::nth_form(black_box(i)));
            }
        });
    });
    // The float path pays for the reference's `Number::toString` layout rules.
    group.bench_function("nth_f64", |b| {
        b.iter(|| {
            for i in 0..100 {
                black_box(CountInflector::nth_f64(black_box(f64::from(i) + 0.5)));
            }
        });
    });
    // The string path additionally runs the reference's `ToNumber` grammar.
    group.bench_function("nth_str", |b| {
        let inputs: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        b.iter(|| {
            for s in &inputs {
                black_box(CountInflector::nth_str(black_box(s)));
            }
        });
    });
    group.bench_function("fr/nth/i64", |b| {
        b.iter(|| {
            for i in 0..100i64 {
                black_box(CountInflectorFr::nth(black_box(i)));
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
    bench_count
);
criterion_main!(benches);
