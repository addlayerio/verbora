// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the Brill tagger.
//!
//! **No campaign has been run against this implementation**, so no figure from
//! this file is published anywhere in the crate. What follows is the measurement
//! plan, not a result.
//!
//! # The inputs are synthetic, and have to be
//!
//! This crate ships no dictionary (see `data/NOTICE.md`), so every benchmark
//! below builds its own lexicon and rule set. That is a change in what is being
//! measured, and it is worth being honest about: a generated 20,000-entry
//! lexicon has a different key-length distribution and a different hit rate from
//! a real one, so absolute numbers here are not comparable with any figure taken
//! before the dictionaries were removed. What the shapes still measure honestly
//! is *scaling* — against document length, against lexicon size, against rule
//! count — which is what these benchmarks were for.
//!
//! Five things are worth measuring separately, because each has a different
//! shape and a different failure mode if the implementation drifts:
//!
//! * **lookup** — the hot path, `Lexicon::tag_of`, on hits, misses, and words
//!   needing the lowercase retry, across lexicon sizes. The store is an ordered
//!   map, so this is expected to grow with `log n`; a jump would mean the
//!   lookup chain has started doing something per-call that it should not.
//! * **tagging** — end to end, across document lengths and rule counts.
//!   Transformation is `O(positions × rules)`, and the rule count is the
//!   multiplier a caller controls.
//! * **API variants** — `tag`, `tag_into` and `tag_stream` measured
//!   independently, since the crate's own documentation recommends between them.
//! * **rule parsing** — the text format, which a caller pays for once per rule
//!   set but which the trainer's round-trip pays for per model.
//! * **training** — one realistic corpus. Training is not parameterised by
//!   corpus size: it is `O(iterations × corpus × templates)`, so a size sweep
//!   measures the product and little else.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_tagger::{
    BrillTagger, Corpus, Lexicon, Rule, RuleSet, Tag, TaggedToken, Template, Trainer,
};

fn tag(s: &str) -> Tag {
    Tag::new(s.to_owned()).expect("a conforming tag")
}

/// The vocabulary the documents are drawn from, and the entries the lexicon is
/// seeded with.
///
/// Real words, so the lexicon hits and the rules have something to fire on; a
/// filler token like `"x"` would measure the miss path and the default category
/// instead of the work a document actually causes.
const VOCABULARY: &[(&str, &str)] = &[
    ("the", "DT"),
    ("quick", "JJ"),
    ("brown", "JJ"),
    ("fox", "NN"),
    ("jumps", "VBZ"),
    ("over", "IN"),
    ("a", "DT"),
    ("lazy", "JJ"),
    ("dog", "NN"),
    ("and", "CC"),
    ("would", "MD"),
    ("book", "NN"),
    ("flight", "NN"),
    ("to", "TO"),
    ("Paris", "NNP"),
    ("quickly", "RB"),
    ("www.example.com", "NN"),
    ("5", "NN"),
    ("cats", "NNS"),
    ("sees", "VBZ"),
];

/// A lexicon holding the vocabulary plus `filler` generated entries, so lookup
/// can be measured against dictionary size.
fn lexicon_of(filler: usize) -> Lexicon {
    let mut lexicon = Lexicon::new(tag("NN")).with_capitalized_default_tag(tag("NNP"));
    for (word, t) in VOCABULARY {
        lexicon
            .insert(word, vec![tag(t)])
            .expect("a conforming entry");
    }
    for i in 0..filler {
        lexicon
            .insert(&format!("zzfiller{i:06}"), vec![tag("NN"), tag("JJ")])
            .expect("a conforming entry");
    }
    lexicon
}

/// The three shape rules plus `count` contextual ones, so tagging can be
/// measured against rule-set size.
fn rules_of(count: usize) -> RuleSet {
    use std::fmt::Write as _;
    let mut text = String::from(
        "NN CD CURRENT-WORD-IS-NUMBER YES\n\
         NN URL CURRENT-WORD-IS-URL YES\n\
         NN NNS CURRENT-WORD-ENDS-WITH s\n",
    );
    for i in 0..count {
        writeln!(text, "NN Z{i} PREV-1-OR-2-TAG Q{i}").expect("writing to a String");
    }
    text.parse().expect("well-formed rules")
}

/// A document of `n` tokens drawn from the vocabulary, cycled.
fn tokens(n: usize) -> Vec<&'static str> {
    VOCABULARY.iter().map(|(w, _)| *w).cycle().take(n).collect()
}

const LENGTHS: [usize; 4] = [8, 64, 512, 4096];
const LEXICON_SIZES: [usize; 3] = [0, 1_000, 20_000];

fn bench_lookup(c: &mut Criterion) {
    let mut g = c.benchmark_group("lexicon/tag_of");
    for size in LEXICON_SIZES {
        let lexicon = lexicon_of(size);
        for (name, word) in [
            ("hit", "flight"),
            ("miss", "zzzznotawordanywhere"),
            ("lowercase-retry", "Jumps"),
            ("capitalised-default", "Zzzzznotaword"),
        ] {
            g.bench_with_input(BenchmarkId::new(name, size), &lexicon, |b, lexicon| {
                b.iter(|| black_box(lexicon.tag_of(black_box(word))));
            });
        }
    }
    g.finish();
}

fn bench_tag(c: &mut Criterion) {
    let lexicon = lexicon_of(20_000);
    let mut g = c.benchmark_group("tag");
    for rule_count in [0usize, 16, 256] {
        let rules = rules_of(rule_count);
        let tagger = BrillTagger::new(&lexicon, &rules);
        for n in LENGTHS {
            let document = tokens(n);
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(
                BenchmarkId::new(format!("{}rules", rules.len()), n),
                &document,
                |b, document| {
                    b.iter(|| black_box(tagger.tag(black_box(document).iter().copied())));
                },
            );
        }
    }
    g.finish();
}

/// The initial state alone, so the rule pass can be costed by subtraction.
fn bench_annotate(c: &mut Criterion) {
    let lexicon = lexicon_of(20_000);
    let rules = rules_of(16);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let mut g = c.benchmark_group("annotate");
    for n in LENGTHS {
        let document = tokens(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &document, |b, document| {
            b.iter(|| black_box(tagger.annotate(black_box(document).iter().copied())));
        });
    }
    g.finish();
}

/// `tag` against `tag_into` against `tag_stream`, which is the comparison the
/// crate's "Choosing the right API" table rests on.
fn bench_api_variants(c: &mut Criterion) {
    let lexicon = lexicon_of(20_000);
    let rules = rules_of(16);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let document = tokens(4096);

    let mut g = c.benchmark_group("api-variants/4096tok");
    g.throughput(Throughput::Elements(document.len() as u64));
    g.bench_function("tag", |b| {
        b.iter(|| black_box(tagger.tag(black_box(&document).iter().copied())));
    });
    g.bench_function("tag_into", |b| {
        let mut buf: Vec<TaggedToken<'_>> = Vec::new();
        b.iter(|| {
            buf.clear();
            tagger.tag_into(black_box(&document).iter().copied(), &mut buf);
            black_box(buf.len())
        });
    });
    g.bench_function("tag_stream", |b| {
        b.iter(|| {
            black_box(
                tagger
                    .tag_stream(black_box(&document).iter().copied())
                    .count(),
            )
        });
    });
    g.finish();
}

/// A document far larger than any sensible buffer, to show `tag_stream`'s memory
/// is a function of the rule set rather than of the input.
fn bench_streaming(c: &mut Criterion) {
    let lexicon = lexicon_of(20_000);
    let rules = rules_of(16);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let document = tokens(100_000);
    let mut g = c.benchmark_group("tag_stream/100k");
    g.sample_size(20);
    g.throughput(Throughput::Elements(document.len() as u64));
    g.bench_function("collect-count", |b| {
        b.iter(|| {
            black_box(
                tagger
                    .tag_stream(black_box(&document).iter().copied())
                    .count(),
            )
        });
    });
    g.finish();
}

fn bench_rule_parsing(c: &mut Criterion) {
    let mut g = c.benchmark_group("rule-parsing");
    g.bench_function("one-rule", |b| {
        b.iter(|| black_box(black_box("VBD NN PREV-TAG DT").parse::<Rule>().unwrap()));
    });
    g.bench_function("brill-1992-set", |b| {
        b.iter(|| black_box(RuleSet::brill_1992().len()));
    });
    let text = rules_of(256).to_string();
    g.bench_function("259-rule-set-from-text", |b| {
        b.iter(|| black_box(black_box(&text).parse::<RuleSet>().unwrap().len()));
    });
    g.finish();
}

/// A Brown-format corpus with a realistic error rate for the trainer to work on.
fn training_text(sentences: usize) -> String {
    const LINES: &[&str] = &[
        "The_AT dog_NN runs_VBZ quickly_RB",
        "to_TO book_VB a_AT flight_NN",
        "a_AT book_NN is_BEZ good_JJ",
        "the_AT running_VBG dogs_NNS bark_VB",
        "he_PPS would_MD book_VB it_PPO",
        "The_AT cats_NNS were_BED sleeping_VBG",
    ];
    LINES
        .iter()
        .copied()
        .cycle()
        .take(sentences)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A lexicon built from `corpus` that is wrong often enough for the trainer to
/// have something to learn.
fn training_lexicon(corpus: &Corpus<'_>) -> Lexicon {
    let mut lexicon = corpus
        .build_lexicon(tag("NN"))
        .expect("corpus tokens are conforming");
    for w in ["book", "running", "sleeping", "runs"] {
        lexicon
            .insert(w, vec![tag("NN")])
            .expect("a conforming entry");
    }
    lexicon
}

fn bench_train(c: &mut Criterion) {
    let text = training_text(120);
    let corpus = Corpus::parse_brown(&text).expect("well-formed corpus");
    let lexicon = training_lexicon(&corpus);

    let mut g = c.benchmark_group("train");
    g.sample_size(10);
    g.bench_function("contextual-templates", |b| {
        b.iter(|| {
            black_box(
                Trainer::new()
                    .with_templates(Template::CONTEXTUAL)
                    .train(black_box(&corpus), black_box(&lexicon))
                    .rules()
                    .len(),
            )
        });
    });
    g.bench_function("all-templates", |b| {
        b.iter(|| {
            black_box(
                Trainer::new()
                    .train(black_box(&corpus), black_box(&lexicon))
                    .rules()
                    .len(),
            )
        });
    });
    g.finish();
}

/// Evaluation against the Brill 1992 rules, which are written in the same Brown
/// tag set the corpus above uses — the one place a bundled rule set and a
/// caller's data can legitimately meet.
fn bench_evaluate(c: &mut Criterion) {
    let text = training_text(120);
    let corpus = Corpus::parse_brown(&text).expect("well-formed corpus");
    let lexicon = training_lexicon(&corpus);
    let rules = RuleSet::brill_1992();
    let tagger = BrillTagger::new(&lexicon, &rules);
    let mut g = c.benchmark_group("evaluate");
    g.throughput(Throughput::Elements(corpus.token_count() as u64));
    g.bench_function("brill-1992", |b| {
        b.iter(|| black_box(tagger.evaluate(black_box(&corpus))));
    });
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_tag_batch(c: &mut Criterion) {
    let lexicon = lexicon_of(20_000);
    let rules = rules_of(16);
    let tagger = BrillTagger::new(&lexicon, &rules);

    let mut g = c.benchmark_group("tag_batch/512tok");
    for count in [1usize, 8, 64, 256] {
        let documents: Vec<Vec<&'static str>> = (0..count).map(|_| tokens(512)).collect();
        g.throughput(Throughput::Elements((count * 512) as u64));
        g.bench_with_input(
            BenchmarkId::new("sequential", count),
            &documents,
            |b, docs| {
                b.iter(|| {
                    let out: Vec<_> = docs.iter().map(|d| tagger.tag(d.iter().copied())).collect();
                    black_box(out.len())
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("parallel", count),
            &documents,
            |b, docs| {
                b.iter(|| black_box(tagger.par_tag_batch(black_box(docs)).len()));
            },
        );
    }
    g.finish();
}

#[cfg(not(feature = "parallel"))]
fn bench_par_tag_batch(_: &mut Criterion) {}

criterion_group!(
    benches,
    bench_lookup,
    bench_tag,
    bench_annotate,
    bench_api_variants,
    bench_streaming,
    bench_rule_parsing,
    bench_train,
    bench_evaluate,
    bench_par_tag_batch,
);
criterion_main!(benches);
