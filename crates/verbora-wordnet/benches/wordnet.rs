// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for WordNet access.
//!
//! This module is the project's headline data-access case: The reference reads
//! **one byte per `fs.read` call** and re-opens a descriptor for every operation,
//! so the interesting question is not "is Rust faster" but "which storage
//! strategy should the crate default to". Four are measured, on identical work:
//!
//! * `Storage::Pread` — positioned reads, nothing preloaded;
//! * `Storage::LazyResident` — whole file, loaded on first touch;
//! * `Storage::Resident` — whole file, loaded at open;
//! * `Storage::Indexed` — resident plus a line-start table, with and without a
//!   prebuilt sidecar.
//!
//! Four dimensions, because they rank the strategies differently:
//!
//! | Group | Question |
//! |---|---|
//! | `open` | startup cost — what a one-shot process pays before its first answer |
//! | `cold` | open + one lookup — the honest cost of a single query |
//! | `lookup` | steady-state per-query latency on a warm dictionary |
//! | `repeat` | throughput over a realistic word list |
//! | `footprint` | resident bytes, reported as a `Throughput` so it lands in the report |
//!
//! The dictionary is separately licensed and not vendored. Set
//! `$WORDNET_DB_PATH`; the benches skip cleanly if it is absent, because a
//! missing licence-restricted asset should not fail a build.

use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_wordnet::{Config, Pos, PrebuiltIndex, Storage, WordNet, pointer};

/// The dictionary, or `None` when it has not been installed.
fn dict_dir() -> Option<PathBuf> {
    for var in ["WORDNET_DB_PATH", "VERBORA_WORDNET_DICT"] {
        if let Some(v) = std::env::var_os(var) {
            let p = PathBuf::from(v);
            if p.join("index.noun").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Words spanning the shapes a lookup can take.
///
/// `run` has 57 senses across four parts of speech and is the worst case in the
/// database; `awful` and `abandon` are present but reported missing by the
/// reference's incomplete bisection, so they exercise the full probe sequence
/// without any data reads at all.
const WORDS: &[&str] = &[
    "entity",
    "node",
    "fast",
    "run",
    "dog",
    "cat",
    "good",
    "light",
    "water",
    "computer",
    "awful",
    "abandon",
    "quickly",
    "beautiful",
    "new_york",
    "zzzzz",
];

const STRATEGIES: &[(&str, Storage)] = &[
    ("pread", Storage::Pread),
    ("lazy", Storage::LazyResident),
    ("resident", Storage::Resident),
    ("indexed", Storage::Indexed),
];

fn skip_notice() {
    eprintln!(
        "verbora-wordnet benches skipped: no WordNet dictionary found.\n\
         It is separately licensed (Princeton University) and not vendored.\n\
         Point $WORDNET_DB_PATH at a directory holding `index.noun` and its\n\
         seven siblings."
    );
}

/// Startup: what a process pays before it can answer anything.
fn bench_open(c: &mut Criterion) {
    let Some(dict) = dict_dir() else {
        skip_notice();
        return;
    };
    let mut g = c.benchmark_group("open");
    // Long enough to be meaningful without spending a minute reading 28 MB
    // hundreds of times.
    g.sample_size(20);
    for (name, storage) in STRATEGIES {
        g.bench_function(*name, |b| {
            b.iter(|| WordNet::open_with(black_box(&dict), &Config::new(*storage)).unwrap());
        });
    }

    // The prebuilt sidecar exists to remove the newline scan from startup; this
    // is the measurement that justifies it.
    let sidecar = std::env::temp_dir().join("verbora-wordnet-bench.nrsidx");
    PrebuiltIndex::build(&dict).unwrap().save(&sidecar).unwrap();
    g.bench_function("indexed_prebuilt", |b| {
        b.iter(|| {
            WordNet::open_with(black_box(&dict), &Config::default().with_prebuilt(&sidecar))
                .unwrap()
        });
    });
    g.finish();
}

/// Open plus one lookup: the honest cost for a one-shot process.
fn bench_cold(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("cold");
    g.sample_size(20);
    for (name, storage) in STRATEGIES {
        g.bench_function(*name, |b| {
            b.iter(|| {
                let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
                wn.lookup(black_box("entity")).unwrap()
            });
        });
    }
    g.finish();
}

/// Steady-state per-query latency on a warm dictionary.
fn bench_lookup(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("lookup");
    for (name, storage) in STRATEGIES {
        let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
        // Warm the lazy backend so this measures steady state, not first touch.
        for w in WORDS {
            let _ = wn.lookup(w);
        }
        for word in ["entity", "run", "awful", "zzzzz"] {
            g.bench_with_input(BenchmarkId::new(*name, word), word, |b, w| {
                b.iter(|| wn.lookup(black_box(w)).unwrap());
            });
        }
    }
    g.finish();
}

/// Throughput over a word list — what a batch job actually sees.
fn bench_repeat(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("repeat");
    g.throughput(Throughput::Elements(WORDS.len() as u64));
    for (name, storage) in STRATEGIES {
        let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
        for w in WORDS {
            let _ = wn.lookup(w);
        }
        g.bench_function(*name, |b| {
            b.iter(|| {
                let mut n = 0usize;
                for w in WORDS {
                    n += wn.lookup(black_box(w)).unwrap().len();
                }
                n
            });
        });
    }
    g.finish();
}

/// The pieces of a lookup, so a regression can be attributed.
fn bench_stages(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let wn = WordNet::open_with(&dict, &Config::new(Storage::Resident)).unwrap();
    let index = wn.index_file(Pos::Noun);
    let data = wn.data_file_for(Pos::Noun);
    let mut g = c.benchmark_group("stages");

    // The bisection alone: ~18 probes over index.noun, no data reads.
    g.bench_function("index_find_hit", |b| {
        b.iter(|| index.find(black_box("entity")).unwrap());
    });
    g.bench_function("index_find_miss", |b| {
        b.iter(|| index.find(black_box("zzzzzzzz")).unwrap());
    });
    // A false miss: present in the file, but the search cannot reach it.
    g.bench_function("index_find_false_miss", |b| {
        b.iter(|| index.find(black_box("assembling")).unwrap());
    });
    // One synset: line read plus the full parse.
    g.bench_function("data_get_owned", |b| {
        b.iter(|| data.get(black_box(3_832_647.0)).unwrap());
    });
    // The same synset with no allocation beyond the examples.
    g.bench_function("data_get_borrowed", |b| {
        b.iter(|| {
            data.with_record(black_box(3_832_647.0), |r| r.ptrs.len())
                .unwrap()
        });
    });
    // Pointer traversal.
    let node = wn.get(3_832_647.0, "n").unwrap();
    g.bench_function("pointers", |b| {
        b.iter(|| wn.pointers(black_box(&node)).count());
    });
    g.bench_function("hypernym_closure", |b| {
        b.iter(|| wn.closure(black_box(&node), pointer::HYPERNYM).count());
    });
    g.finish();
}

/// Resident memory per strategy, reported through Criterion so it shows up
/// beside the timings rather than in a separate note nobody reads.
fn bench_footprint(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("footprint");
    g.sample_size(10);

    let dict_bytes: u64 = ["index", "data"]
        .iter()
        .flat_map(|k| ["noun", "verb", "adj", "adv"].map(move |p| format!("{k}.{p}")))
        .map(|f| {
            std::fs::metadata(dict.join(f))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum();
    let prebuilt = PrebuiltIndex::build(&dict).unwrap();

    eprintln!(
        "footprint: dictionary {:.1} MB; line-start tables {:.1} MB ({} lines)\n\
         \tPread          ~0 resident\n\
         \tLazyResident   grows to the files touched (index.noun alone is {:.1} MB)\n\
         \tResident       {:.1} MB\n\
         \tIndexed        {:.1} MB",
        dict_bytes as f64 / 1e6,
        prebuilt.byte_size() as f64 / 1e6,
        prebuilt.line_count(),
        std::fs::metadata(dict.join("index.noun")).unwrap().len() as f64 / 1e6,
        dict_bytes as f64 / 1e6,
        (dict_bytes as usize + prebuilt.byte_size()) as f64 / 1e6,
    );

    // Building the tables is the cost the prebuilt sidecar removes from startup.
    g.throughput(Throughput::Bytes(dict_bytes));
    g.bench_function("build_line_starts", |b| {
        b.iter(|| PrebuiltIndex::build(black_box(&dict)).unwrap().line_count());
    });
    g.finish();
}

/// Sequential [`WordNet::lookup`] vs. [`WordNet::par_lookup_batch`], at a few
/// realistic batch sizes. Requires the `parallel` feature; a no-op group
/// otherwise, so `criterion_group!` below stays a single, unconditional list.
fn bench_par_lookup_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        let Some(dict) = dict_dir() else {
            skip_notice();
            return;
        };
        let wn = WordNet::open_with(&dict, &Config::new(Storage::Resident)).unwrap();
        // Warm the dictionary, matching `bench_repeat`.
        for w in WORDS {
            let _ = wn.lookup(w);
        }

        let mut g = c.benchmark_group("par_lookup_batch");
        // `WORDS` (16 entries) repeated out to a few realistic batch sizes: a
        // small batch close to rayon's scheduling break-even point, and two
        // larger ones where the worst case (`run`, 57 senses) recurs often
        // enough to matter.
        for &n in &[16usize, 160, 1600] {
            let words: Vec<&str> = WORDS.iter().copied().cycle().take(n).collect();
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new("sequential", n), &words, |b, words| {
                b.iter(|| {
                    let mut total = 0usize;
                    for w in words {
                        total += wn.lookup(black_box(w)).unwrap().len();
                    }
                    total
                });
            });
            g.bench_with_input(BenchmarkId::new("parallel", n), &words, |b, words| {
                b.iter(|| wn.par_lookup_batch(black_box(words)));
            });
        }
        g.finish();
    }
}

criterion_group!(
    benches,
    bench_open,
    bench_cold,
    bench_lookup,
    bench_repeat,
    bench_stages,
    bench_footprint,
    bench_par_lookup_batch
);
criterion_main!(benches);
