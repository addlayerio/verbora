// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the Brill tagger.
//!
//! Four things are worth measuring separately, because each has a different
//! shape and a different failure mode if the implementation drifts:
//!
//! * **cold start** — the first lexicon lookup in a fresh process. The bundled
//!   dictionaries are a packed index read in place, so this should be
//!   indistinguishable from a warm lookup; if it ever isn't, something has
//!   started parsing 4.6 MB of JSON at startup.
//! * **lookup** — the hot path, `Lexicon::first_category`, on hits, misses, and
//!   words needing the lowercased retry.
//! * **tagging** — end to end, across sentence lengths, for both bundled
//!   languages. Dutch is the interesting one: 285 rules against English's 18, and
//!   `applyRules` is O(positions × rules).
//! * **streaming vs batch** — `tag_iter` holds seven words instead of the whole
//!   document; this shows what that costs (it should be nothing) and that it
//!   still terminates in constant memory on a 100k-token input.
//!
//! Training is deliberately *not* parameterised by corpus size: it is O(sites ×
//! rules) with rules growing with the corpus, so a size sweep measures the
//! quadratic and little else. One realistic corpus is benchmarked instead.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_tagger::{
    BrillPosTagger, BrillPosTester, BrillPosTrainer, Corpus, Language, Lexicon, RuleSet,
    RuleTemplate, Sentence, TaggedWord,
};

/// A sentence of `n` tokens drawn from ordinary English, cycled.
///
/// Real words, so the lexicon hits and the rules have something to fire on; a
/// filler token like `"x"` would measure the miss path and the default category
/// instead of the work a document actually causes.
fn english_tokens(n: usize) -> Vec<&'static str> {
    const WORDS: &[&str] = &[
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "a",
        "lazy",
        "dog",
        "and",
        "would",
        "book",
        "a",
        "flight",
        "to",
        "Paris",
        "quickly",
        "www.example.com",
        "5",
        "cats",
    ];
    WORDS.iter().copied().cycle().take(n).collect()
}

fn dutch_tokens(n: usize) -> Vec<&'static str> {
    const WORDS: &[&str] = &[
        "het",
        "is",
        "een",
        "mooie",
        "dag",
        "niet",
        "waar",
        "de",
        "Nederlandsche",
        "Bank",
        "moet",
        "geldpers",
        "aanzetten",
        "er",
        "aan",
        "hand",
        "van",
        "in",
        "met",
        "voor",
    ];
    WORDS.iter().copied().cycle().take(n).collect()
}

const LENGTHS: [usize; 4] = [8, 64, 512, 4096];

/// The first lookup a process ever performs.
///
/// `Criterion` cannot give us a fresh process per iteration, so this measures the
/// next best thing: a lexicon constructed inside the timed region, used once.
/// With the packed index that is a header decode plus a binary search; with a
/// JSON-parsing implementation it would be tens of milliseconds on iteration one
/// and microseconds thereafter, which shows up as an enormous variance.
fn bench_cold_start(c: &mut Criterion) {
    let mut g = c.benchmark_group("lexicon/construct-and-lookup");
    g.bench_function("english", |b| {
        b.iter(|| {
            let lexicon = Lexicon::detached(Some("EN"), Some("NN"), None);
            black_box(lexicon.first_category(black_box("flight")))
        });
    });
    g.bench_function("dutch", |b| {
        b.iter(|| {
            let lexicon = Lexicon::detached(Some("DU"), Some("N"), None);
            black_box(lexicon.first_category(black_box("geldpers")))
        });
    });
    g.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let mut g = c.benchmark_group("lexicon/first_category");
    for (label, word) in [
        ("hit-short", "the"),
        ("hit-long", "extraordinarily"),
        ("miss", "zzzznotawordatall"),
        // A capitalised word misses on the exact key and hits on the retry, so
        // this is the only case that allocates.
        ("lowercase-retry", "Flight"),
        ("non-ascii", "café"),
        ("empty", ""),
    ] {
        g.bench_function(label, |b| {
            b.iter(|| black_box(lexicon.first_category(black_box(word))));
        });
    }
    g.finish();
}

fn bench_tag(c: &mut Criterion) {
    let en_lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let en_rules = RuleSet::for_language(Language::English);
    let du_lexicon = Lexicon::detached(Some("DU"), Some("N"), Some("NP"));
    let du_rules = RuleSet::for_language(Language::Dutch);

    let mut g = c.benchmark_group("tag");
    for n in LENGTHS {
        let en = english_tokens(n);
        let tagger = BrillPosTagger::new(&en_lexicon, &en_rules);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("english", n), &n, |b, _| {
            b.iter(|| black_box(tagger.tag(black_box(en.iter().copied())).unwrap()));
        });

        let du = dutch_tokens(n);
        let tagger = BrillPosTagger::new(&du_lexicon, &du_rules);
        g.bench_with_input(BenchmarkId::new("dutch-285-rules", n), &n, |b, _| {
            b.iter(|| black_box(tagger.tag(black_box(du.iter().copied())).unwrap()));
        });
    }
    g.finish();
}

/// Lexicon pass alone, so the rule pass's share of `tag` is visible.
fn bench_tag_with_lexicon(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::empty();
    let tagger = BrillPosTagger::new(&lexicon, &rules);
    let mut g = c.benchmark_group("tag_with_lexicon");
    for n in LENGTHS {
        let tokens = english_tokens(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| black_box(tagger.tag_with_lexicon(black_box(tokens.iter().copied()))));
        });
    }
    g.finish();
}

fn bench_streaming(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::for_language(Language::English);
    let tagger = BrillPosTagger::new(&lexicon, &rules);
    let tokens = english_tokens(100_000);

    let mut g = c.benchmark_group("100k-tokens");
    g.throughput(Throughput::Elements(100_000));
    g.bench_function("batch", |b| {
        b.iter(|| black_box(tagger.tag(black_box(tokens.iter().copied())).unwrap()));
    });
    // Counting rather than collecting, so the comparison is the tagging work and
    // not the cost of materialising a 100,000-element Vec twice.
    g.bench_function("streaming", |b| {
        b.iter(|| {
            black_box(
                tagger
                    .tag_iter(black_box(tokens.iter().copied()))
                    .filter(|w| w.as_ref().is_ok_and(|w| w.tag().is_some()))
                    .count(),
            )
        });
    });
    g.finish();
}

fn bench_rule_parsing(c: &mut Criterion) {
    let mut g = c.benchmark_group("rules");
    g.bench_function("parse-one", |b| {
        b.iter(|| black_box(RuleSet::from_rule_strings(&["VBD NN PREV-TAG DT"]).unwrap()));
    });
    g.bench_function("load-english-18", |b| {
        b.iter(|| black_box(RuleSet::for_language(Language::English)));
    });
    g.bench_function("load-dutch-285", |b| {
        b.iter(|| black_box(RuleSet::for_language(Language::Dutch)));
    });
    g.finish();
}

/// A corpus of `sentences` ten-word sentences, gold-tagged by the lexicon so the
/// trainer has a realistic mixture of right and wrong initial guesses.
fn training_corpus(sentences: usize) -> Corpus<'static> {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::for_language(Language::English);
    let tagger = BrillPosTagger::new(&lexicon, &rules);
    // The rule-corrected tags become the gold standard, so the plain lexicon
    // tagging the trainer starts from disagrees with it exactly where the rules
    // fire — which is the signal the trainer is meant to learn.
    let words = english_tokens(sentences * 10);
    let mut corpus = Corpus::new();
    for chunk in words.chunks(10) {
        let tagged = tagger
            .tag(chunk.iter().copied())
            .expect("no failing predicate");
        corpus.sentences.push(Sentence::from_words(
            tagged
                .tagged_words
                .into_iter()
                .map(TaggedWord::into_owned)
                .collect(),
        ));
    }
    corpus.word_count = sentences * 10;
    corpus
}

fn bench_train(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let templates: Vec<RuleTemplate> = ["NEXT-TAG", "PREV-TAG"]
        .iter()
        .map(|n| RuleTemplate::named(n).expect("a real template"))
        .collect();

    let mut g = c.benchmark_group("train");
    g.sample_size(20);
    for sentences in [8usize, 32, 128] {
        let corpus = training_corpus(sentences);
        g.throughput(Throughput::Elements((sentences * 10) as u64));
        g.bench_with_input(
            BenchmarkId::from_parameter(sentences),
            &sentences,
            |b, _| {
                b.iter(|| {
                    // Training rewrites the corpus, so each iteration needs its own.
                    let mut fresh = corpus.clone();
                    black_box(
                        BrillPosTrainer::new(1)
                            .train(&mut fresh, black_box(&templates), &lexicon)
                            .unwrap(),
                    )
                });
            },
        );
    }
    g.finish();
}

/// Sequential `documents.iter().map(tag).collect()` versus
/// [`BrillPosTagger::par_tag_batch`], at a few document counts, holding each
/// document at a realistic 512 tokens (the size the crate's own `tag/english`
/// benchmark already uses).
///
/// This is the number that decides whether `par_tag_batch` is worth reaching
/// for: per-document tagging is ~200us at this length (near-linear up to
/// 4096 tokens per `bench_tag`), so the question is purely how many
/// documents it takes to amortise Rayon's thread hand-off.
#[cfg(feature = "parallel")]
fn bench_par_tag_batch(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::for_language(Language::English);
    let tagger = BrillPosTagger::new(&lexicon, &rules);

    const DOC_TOKENS: usize = 512;
    let doc = english_tokens(DOC_TOKENS);

    let mut g = c.benchmark_group("tag_batch/english-512tok");
    for doc_count in [1usize, 8, 64, 256] {
        let documents: Vec<Vec<&str>> = std::iter::repeat_n(doc.clone(), doc_count).collect();
        g.throughput(Throughput::Elements((doc_count * DOC_TOKENS) as u64));

        g.bench_with_input(
            BenchmarkId::new("sequential", doc_count),
            &doc_count,
            |b, _| {
                b.iter(|| {
                    let out: Vec<_> = documents
                        .iter()
                        .map(|d| tagger.tag(d.iter().copied()))
                        .collect();
                    black_box(out)
                });
            },
        );

        g.bench_with_input(
            BenchmarkId::new("parallel", doc_count),
            &doc_count,
            |b, _| {
                b.iter(|| black_box(tagger.par_tag_batch(black_box(&documents))));
            },
        );
    }
    g.finish();
}

fn bench_test(c: &mut Criterion) {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::for_language(Language::English);
    let tagger = BrillPosTagger::new(&lexicon, &rules);
    let corpus = training_corpus(128);
    let mut g = c.benchmark_group("test");
    g.throughput(Throughput::Elements(1280));
    g.bench_function("1280-words", |b| {
        b.iter(|| {
            black_box(
                BrillPosTester::new()
                    .test(black_box(&corpus), &tagger)
                    .unwrap(),
            )
        });
    });
    g.finish();
}

#[cfg(not(feature = "parallel"))]
criterion_group!(
    benches,
    bench_cold_start,
    bench_lookup,
    bench_tag,
    bench_tag_with_lexicon,
    bench_streaming,
    bench_rule_parsing,
    bench_train,
    bench_test
);

#[cfg(feature = "parallel")]
criterion_group!(
    benches,
    bench_cold_start,
    bench_lookup,
    bench_tag,
    bench_tag_with_lexicon,
    bench_streaming,
    bench_rule_parsing,
    bench_train,
    bench_test,
    bench_par_tag_batch
);

criterion_main!(benches);
