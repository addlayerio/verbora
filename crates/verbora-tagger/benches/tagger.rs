// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the Brill tagger.
//!
//! **No campaign has been run against the post-migration implementation**, so no
//! figure from this file is published anywhere in the crate. What follows is the
//! measurement plan, not a result.
//!
//! Six things are worth measuring separately, because each has a different shape
//! and a different failure mode if the implementation drifts:
//!
//! * **cold start** — the first lexicon lookup in a fresh `Lexicon`. The bundled
//!   dictionaries are a packed index read in place, so this should be
//!   indistinguishable from a warm lookup; if it ever isn't, something has
//!   started parsing 4.6 MB of JSON at construction.
//! * **lookup** — the hot path, `Lexicon::tag_of`, on hits, misses, and words
//!   needing the lowercase retry.
//! * **tagging** — end to end, across document lengths, for both bundled
//!   languages. Dutch is the interesting one: 285 rules against English's 18,
//!   and transformation is O(positions × rules).
//! * **API variants** — `tag`, `tag_into` and `tag_stream` measured
//!   independently, since the crate's own documentation recommends between them.
//! * **rule parsing** — the text format, which a caller pays for once per rule
//!   set but which the trainer's round-trip pays for per model.
//! * **training** — one realistic corpus. Training is not parameterised by
//!   corpus size: it is O(iterations × corpus × templates), so a size sweep
//!   measures the product and little else.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_tagger::{
    BrillTagger, Corpus, Language, Lexicon, Rule, RuleSet, Tag, TaggedToken, Template, Trainer,
};

/// A document of `n` tokens drawn from ordinary English, cycled.
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

/// The first lookup a fresh `Lexicon` ever performs.
///
/// Criterion cannot give us a fresh process per iteration, so this measures the
/// next best thing: a lexicon constructed inside the timed region and used once.
/// With the packed index that is a header decode plus a binary search.
fn bench_cold_start(c: &mut Criterion) {
    let mut g = c.benchmark_group("lexicon/construct-and-lookup");
    g.bench_function("english", |b| {
        b.iter(|| {
            let lexicon = Lexicon::bundled(Language::English);
            black_box(lexicon.tag_of(black_box("flight")))
        });
    });
    g.bench_function("dutch", |b| {
        b.iter(|| {
            let lexicon = Lexicon::bundled(Language::Dutch);
            black_box(lexicon.tag_of(black_box("geldpers")))
        });
    });
    g.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let lexicon = Lexicon::bundled(Language::English);
    let mut g = c.benchmark_group("lexicon/tag_of");
    for (name, word) in [
        ("hit", "flight"),
        ("miss", "zzzznotawordanywhere"),
        ("lowercase-retry", "Jumps"),
        ("capitalised-default", "Zzzzznotaword"),
    ] {
        g.bench_function(name, |b| {
            b.iter(|| black_box(lexicon.tag_of(black_box(word))));
        });
    }
    g.finish();
}

fn bench_tag(c: &mut Criterion) {
    let mut g = c.benchmark_group("tag");
    for (name, language, make) in [
        (
            "english",
            Language::English,
            english_tokens as fn(usize) -> Vec<&'static str>,
        ),
        ("dutch", Language::Dutch, dutch_tokens),
    ] {
        let lexicon = Lexicon::bundled(language);
        let rules = RuleSet::bundled(language);
        let tagger = BrillTagger::new(&lexicon, &rules);
        for n in LENGTHS {
            let tokens = make(n);
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new(name, n), &tokens, |b, tokens| {
                b.iter(|| black_box(tagger.tag(black_box(tokens).iter().copied())));
            });
        }
    }
    g.finish();
}

/// The initial state alone, so the rule pass can be costed by subtraction.
fn bench_annotate(c: &mut Criterion) {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let mut g = c.benchmark_group("annotate/english");
    for n in LENGTHS {
        let tokens = english_tokens(n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &tokens, |b, tokens| {
            b.iter(|| black_box(tagger.annotate(black_box(tokens).iter().copied())));
        });
    }
    g.finish();
}

/// `tag` against `tag_into` against `tag_stream`, which is the comparison the
/// crate's "Choosing the right API" table rests on.
fn bench_api_variants(c: &mut Criterion) {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let tokens = english_tokens(4096);

    let mut g = c.benchmark_group("api-variants/english-4096tok");
    g.throughput(Throughput::Elements(tokens.len() as u64));
    g.bench_function("tag", |b| {
        b.iter(|| black_box(tagger.tag(black_box(&tokens).iter().copied())));
    });
    g.bench_function("tag_into", |b| {
        let mut buf: Vec<TaggedToken<'_>> = Vec::new();
        b.iter(|| {
            buf.clear();
            tagger.tag_into(black_box(&tokens).iter().copied(), &mut buf);
            black_box(buf.len())
        });
    });
    g.bench_function("tag_stream", |b| {
        b.iter(|| {
            black_box(
                tagger
                    .tag_stream(black_box(&tokens).iter().copied())
                    .count(),
            )
        });
    });
    g.finish();
}

/// A document far larger than any sensible buffer, to show `tag_stream`'s memory
/// is a function of the rule set rather than of the input.
fn bench_streaming(c: &mut Criterion) {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let tokens = english_tokens(100_000);
    let mut g = c.benchmark_group("tag_stream/english-100k");
    g.sample_size(20);
    g.throughput(Throughput::Elements(tokens.len() as u64));
    g.bench_function("collect-count", |b| {
        b.iter(|| {
            black_box(
                tagger
                    .tag_stream(black_box(&tokens).iter().copied())
                    .count(),
            )
        });
    });
    g.finish();
}

fn bench_rule_parsing(c: &mut Criterion) {
    let mut g = c.benchmark_group("rule-parsing");
    g.bench_function("one-english-rule", |b| {
        b.iter(|| black_box(black_box("VBD NN PREV-TAG DT").parse::<Rule>().unwrap()));
    });
    g.bench_function("dutch-rule-set-285", |b| {
        b.iter(|| black_box(RuleSet::bundled(Language::Dutch).len()));
    });
    let text = RuleSet::bundled(Language::Dutch).to_string();
    g.bench_function("dutch-rule-set-from-text", |b| {
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

fn bench_train(c: &mut Criterion) {
    let text = training_text(120);
    let corpus = Corpus::parse_brown(&text).expect("well-formed corpus");
    let mut lexicon = corpus
        .build_lexicon(Tag::new("NN").expect("NN is a tag"))
        .expect("corpus tokens are conforming");
    // Make the initial state wrong often enough that there is something to learn.
    for w in ["book", "running", "sleeping", "runs"] {
        lexicon
            .insert(w, vec![Tag::new("NN").expect("NN is a tag")])
            .expect("conforming key");
    }

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

fn bench_evaluate(c: &mut Criterion) {
    let text = training_text(120);
    let corpus = Corpus::parse_brown(&text).expect("well-formed corpus");
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let mut g = c.benchmark_group("evaluate");
    g.throughput(Throughput::Elements(corpus.token_count() as u64));
    g.bench_function("english", |b| {
        b.iter(|| black_box(tagger.evaluate(black_box(&corpus))));
    });
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_tag_batch(c: &mut Criterion) {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);

    let mut g = c.benchmark_group("tag_batch/english-512tok");
    for count in [1usize, 8, 64, 256] {
        let documents: Vec<Vec<&'static str>> = (0..count).map(|_| english_tokens(512)).collect();
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
    bench_cold_start,
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
