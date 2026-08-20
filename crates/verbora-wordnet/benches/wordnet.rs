// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for WordNet access.
//!
//! The question this suite exists to answer is not "is Rust fast" but "which
//! storage strategy should the crate default to". Four are measured on
//! identical work:
//!
//! * `Storage::Pread` — positioned reads, nothing preloaded;
//! * `Storage::LazyResident` — whole file, read on first touch;
//! * `Storage::Resident` — whole file, read at open;
//! * `Storage::Indexed` — resident plus a line-start table, with and without a
//!   prebuilt sidecar.
//!
//! Five dimensions, because they rank the strategies differently:
//!
//! | Group | Question |
//! |---|---|
//! | `open` | startup cost — what a one-shot process pays before its first answer |
//! | `cold` | open + one lookup — the honest cost of a single query |
//! | `lookup` | steady-state per-query latency on a warm dictionary |
//! | `repeat` | throughput over a realistic word list |
//! | `stages` | the pieces of a lookup, so a regression can be attributed |
//! | `footprint` | resident bytes, reported through Criterion so it lands in the report |
//!
//! The dictionary is separately licensed and not vendored. Set
//! `$WORDNET_DB_PATH`; the benches skip cleanly if it is absent, because a
//! missing licence-restricted asset should not fail a build.

use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_wordnet::{
    Config, PartOfSpeech, PointerSymbol, PrebuiltIndex, Sense, Storage, SynsetOffset, WordNet,
};

/// The dictionary, or `None` when it has not been installed.
fn dict_dir() -> Option<PathBuf> {
    for var in ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            let dir = PathBuf::from(value);
            if dir.join("index.noun").is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// Words spanning the shapes a lookup can take: common entries, the
/// highest-sense-count word in the database (`run`), a collocation, and a miss.
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
    // Long enough to be meaningful without reading the whole dictionary
    // hundreds of times.
    g.sample_size(20);
    for (name, storage) in STRATEGIES {
        g.bench_function(*name, |b| {
            b.iter(|| WordNet::open_with(black_box(&dict), &Config::new(*storage)).unwrap());
        });
    }

    // The prebuilt sidecar exists to remove the newline scan from startup; this
    // is the measurement that justifies it.
    let sidecar = std::env::temp_dir().join("verbora-wordnet-bench.vbwnix");
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
    let index = wn.index_file(PartOfSpeech::Noun);
    let data = wn.data_file(PartOfSpeech::Noun);
    let mut g = c.benchmark_group("stages");

    // The binary search alone, no data reads.
    g.bench_function("index_entry_hit", |b| {
        b.iter(|| index.entry(black_box("entity")).unwrap());
    });
    g.bench_function("index_entry_miss", |b| {
        b.iter(|| index.entry(black_box("zzzzzzzz")).unwrap());
    });
    // A key that sorts before every lemma and after every header line: the
    // longest probe sequence the search can take.
    g.bench_function("index_entry_first_lemma", |b| {
        b.iter(|| index.entry(black_box("'hood")).unwrap());
    });

    // One synset, resolved through the index so the benchmark does not depend
    // on a hard-coded offset that differs between WordNet 3.0 and 3.1.
    let entity = wn
        .index_entry("entity", PartOfSpeech::Noun)
        .unwrap()
        .expect("`entity` is in every WordNet release");
    let offset: SynsetOffset = entity.synset_offsets[0];

    g.bench_function("synset_owned", |b| {
        b.iter(|| data.synset(black_box(offset)).unwrap());
    });
    g.bench_function("synset_borrowed", |b| {
        b.iter(|| {
            data.with_synset(black_box(offset), |r| r.pointers.len())
                .unwrap()
        });
    });

    // Pointer traversal, from a synset with a real hypernym chain.
    let sense: Sense = "node#n#1".parse().unwrap();
    if let Ok(Some(node)) = wn.sense(&sense) {
        g.bench_function("pointers", |b| {
            b.iter(|| wn.pointers(black_box(&node)).count());
        });
        g.bench_function("hypernym_closure", |b| {
            b.iter(|| {
                wn.closure(black_box(&node), PointerSymbol::Hypernym)
                    .count()
            });
        });
    }
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
        .flat_map(|kind| PartOfSpeech::ALL.map(move |p| format!("{kind}.{}", p.file_suffix())))
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

/// Sequential [`WordNet::lookup`] vs. `WordNet::par_lookup_batch`, at a few
/// realistic batch sizes. Requires the `parallel` feature; a no-op group
/// otherwise, so `criterion_group!` below stays a single unconditional list.
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
        for w in WORDS {
            let _ = wn.lookup(w);
        }

        let mut g = c.benchmark_group("par_lookup_batch");
        // `WORDS` repeated out to a small batch near rayon's scheduling
        // break-even point and two larger ones where the worst case recurs
        // often enough to matter.
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
