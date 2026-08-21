// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the clause analyzer.
//!
//! Sentence lengths span 8 to 2 048 words so that scaling — the pipeline is a
//! constant number of linear passes — is visible rather than inferred. Tokens come from
//! `benches/data/words.json`, the shared corpus the rest of the workspace's
//! benchmarks use, so every harness describes byte-identical input.
//!
//! No figures measured from this harness are published anywhere in the crate:
//! the pipeline was rewritten and nothing has been measured against it.

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use verbora_analyzers::{TaggedWord, Terminator, analyze, analyze_with_terminator};

/// Sentence lengths, in words, excluding the terminator.
const SIZES: [usize; 5] = [8, 32, 128, 512, 2048];

/// The Penn Treebank tags assigned cyclically to the corpus words.
///
/// Chosen so every rule is exercised at a realistic frequency: `IN` opens a
/// prepositional phrase, `NN`/`NNS` close one, `VBD` ends the subject, `PRP` is
/// the tag-question pronoun. `DT` comes first so the sentence is never
/// imperative, which keeps the subject/predicate split the thing being
/// measured.
///
/// Every entry is a real Penn Treebank tag; an ambiguity class such as `NN|IN`
/// would measure the `TagClass::Other` path instead and is deliberately absent.
const TAG_CYCLE: [&str; 10] = [
    "DT", "JJ", "NN", "VBD", "RB", "IN", "NNS", "PRP", "VBG", "CD",
];

/// The shared word list, read from `benches/data/words.json`.
fn load_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
        .expect("a word list")
        .iter()
        .map(|w| w.as_str().expect("a word").to_owned())
        .collect()
}

/// Builds an `n`-word sentence plus a full stop, borrowing every token.
fn sentence<'a>(words: &'a [String], n: usize) -> Vec<TaggedWord<'a>> {
    let mut out: Vec<TaggedWord<'a>> = (0..n)
        .map(|i| {
            TaggedWord::new(
                words[i % words.len()].as_str(),
                TAG_CYCLE[i % TAG_CYCLE.len()],
            )
        })
        .collect();
    out.push(TaggedWord::new(".", "."));
    out
}

fn bench_analyze(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("analyze");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // The input is borrowed and never modified, so one sentence can be
            // analysed over and over without rebuilding it.
            bench.iter(|| black_box(analyze(&tags)));
        });
    }
    g.finish();
}

/// The out-of-band form, which analyses one more word (no terminator is split
/// off) and skips the terminator lookup.
fn bench_analyze_with_terminator(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("analyze_with_terminator");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            bench.iter(|| black_box(analyze_with_terminator(&tags, Some(Terminator::FullStop))));
        });
    }
    g.finish();
}

/// The two rendering shapes: lazy iteration vs. one owned `String`.
fn bench_render(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("render");
    for n in SIZES {
        let tags = sentence(&words, n);
        let analysis = analyze(&tags);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("subject_tokens", n), &n, |bench, _| {
            bench.iter(|| black_box(analysis.subject_tokens().count()));
        });
        g.bench_with_input(BenchmarkId::new("subject_to_string", n), &n, |bench, _| {
            bench.iter(|| black_box(analysis.subject_to_string()));
        });
    }
    g.finish();
}

/// Building the input, which a caller pays before calling anything here.
fn bench_build(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("build_sentence");
    for n in SIZES {
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // Tokens are borrowed, so this measures one `Vec` growth and
            // nothing else — the payoff for `Cow` fields over `String` ones.
            bench.iter_batched(
                || (),
                |()| black_box(sentence(&words, n)),
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

/// Sequential `analyze` loop vs. `par_analyze_batch`, at a few batch sizes.
/// Requires the `parallel` feature; a no-op group otherwise, so
/// `criterion_group!` below stays a single unconditional list.
fn bench_par_analyze_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        use verbora_analyzers::par_analyze_batch;

        let words = load_words();
        // A mid-range sentence length from `SIZES`, repeated out to a small
        // batch near rayon's scheduling break-even point and to larger ones
        // where the fan-out should win.
        const WORDS_PER_SENTENCE: usize = 32;
        let mut g = c.benchmark_group("par_analyze_batch");
        for &n in &[16usize, 256, 4096] {
            let sentences: Vec<Vec<TaggedWord<'_>>> = (0..n)
                .map(|_| sentence(&words, WORDS_PER_SENTENCE))
                .collect();
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new("sequential", n), &n, |bench, _| {
                bench.iter(|| {
                    let out: Vec<_> = sentences.iter().map(|s| analyze(s)).collect();
                    black_box(out)
                });
            });
            g.bench_with_input(BenchmarkId::new("parallel", n), &n, |bench, _| {
                bench.iter(|| black_box(par_analyze_batch(&sentences)));
            });
        }
        g.finish();
    }
}

criterion_group!(
    benches,
    bench_analyze,
    bench_analyze_with_terminator,
    bench_render,
    bench_build,
    bench_par_analyze_batch
);
criterion_main!(benches);
