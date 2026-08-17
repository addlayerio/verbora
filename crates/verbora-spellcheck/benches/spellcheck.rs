// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the spellchecker.
//!
//! Five things are measured, because they answer five different questions:
//!
//! * **Construction** — building the trie and the frequency table over corpora
//!   spanning three orders of magnitude. This is paid once per instance and is
//!   linear in the corpus, so it is reported as throughput.
//! * **`is_correct`** — the trie walk, on hits and on misses. Misses matter more
//!   than hits: the correction search performs tens of thousands of them and
//!   almost all of them fail, and a near miss (a long shared prefix) costs far
//!   more than one that dies on the first character.
//! * **`get_corrections`** — the whole pipeline, at distance 1 and 2, over
//!   corpora of different sizes. Distance 2 examines several hundred times more
//!   candidates than distance 1, and the point of measuring both is that the
//!   *ratio* is what decides whether a caller can afford it.
//! * **The edit generator's API shape** — `next_edit` (borrowing), `Iterator`
//!   (owning `Vec<u16>`), and [`edits`] (owning `String`) on identical input.
//!   This is the crate's central design claim: the convenience forms are built
//!   on the lazy primitive, so the primitive must be the cheapest of the three.
//! * **The sort** — [`sort_by_frequency`], the port of the reference engine's `Array#sort`, across
//!   lengths that select the short-array path, a single merge and a deep merge
//!   stack, with and without the NaN frequencies the port exists for. NaN is not
//!   a curiosity here: it makes the comparison non-transitive, which changes
//!   which branches of the merge are taken.
//! * **`par_get_corrections_batch` vs. a sequential loop** (`parallel` feature
//!   only) — the same `get_corrections` calls, at the same distance and over
//!   the same corpus, run through `Spellcheck::par_get_corrections_batch`
//!   instead of a `.iter().map(...)` loop, at a few batch sizes. This is what
//!   answers whether the `rayon` fan-out is worth its scheduling overhead at a
//!   given batch size, not just that it compiles.
//!
//! ASCII and Cyrillic corpora are benchmarked side by side. They are the same
//! shape and size — the transliteration is a bijection on `a`–`z`, so the tries
//! branch identically — and the difference between them is therefore exactly the
//! cost of leaving the `&[u8]` fast path for `Vec<u16>`, where every dictionary
//! probe has to decode into a scratch `String` instead of borrowing out of the
//! candidate buffer.
//!
//! Inputs come from `benches/data/words.json`, the shared word list every
//! spellcheck harness in the repo reads. Every input is derived from it by a
//! stated rule (first word of a given length, first 4,000 words, …) rather than
//! by an index into a shuffled list, so each harness is provably measuring the
//! same work.

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use verbora_spellcheck::{Edits, Spellcheck, edits, edits_utf16, sort_by_frequency};

/// Corpus sizes, in words. The corpus decides both the trie's depth and how
/// often a candidate is a hit, so it is the parameter that matters most.
const CORPUS_SIZES: [usize; 4] = [100, 1_000, 10_000, 20_000];

/// How many probes each membership benchmark walks.
///
/// One `is_correct` call is ~15 ns, which is below the resolution the reference
/// baseline's timer can resolve honestly. Both sides therefore measure a fixed
/// batch, and the throughput annotation divides it back out.
const PROBE_COUNT: usize = 4_000;

/// Reads the shared word list, failing loudly if it has not been generated.
fn words() -> Vec<String> {
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
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

/// The same words transliterated into Cyrillic, one code unit per letter.
///
/// The map is a bijection on `a`–`z`, so the corpus keeps its exact branching
/// structure and word lengths. The only thing that changes is that the ASCII
/// fast path can no longer be taken.
fn cyrillic(words: &[String]) -> Vec<String> {
    const RU: [char; 26] = [
        'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
        'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ',
    ];
    words
        .iter()
        .map(|w| {
            w.chars()
                .map(|c| {
                    if c.is_ascii_lowercase() {
                        RU[(c as usize) - ('a' as usize)]
                    } else {
                        c
                    }
                })
                .collect()
        })
        .collect()
}

/// The first word of exactly `len` characters.
///
/// A rule rather than an index, so the reference baseline can reproduce the
/// choice without either side having to hard-code a string.
fn word_of_length(words: &[String], len: usize) -> String {
    words
        .iter()
        .find(|w| w.chars().count() == len)
        .unwrap_or_else(|| panic!("no {len}-character word in the corpus"))
        .clone()
}

/// A misspelling of `word`: the middle character deleted.
///
/// A realistic probe. Feeding `get_corrections` a word that is already correct
/// measures something else (the dictionary hits it immediately), and feeding it
/// noise measures the empty case.
fn typo(word: &str) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    if chars.len() > 1 {
        chars.remove(chars.len() / 2);
    }
    chars.into_iter().collect()
}

fn bench_construction(c: &mut Criterion) {
    let ascii = words();
    let ru = cyrillic(&ascii);
    let mut g = c.benchmark_group("spellcheck_new");
    for (tag, corpus) in [("ascii", &ascii), ("cyrillic", &ru)] {
        for n in CORPUS_SIZES {
            let slice = &corpus[..n];
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| Spellcheck::new(black_box(slice)));
            });
        }
    }
    g.finish();
}

fn bench_is_correct(c: &mut Criterion) {
    let words = words();
    let sc = Spellcheck::new(&words);
    let hits: Vec<&str> = words[..PROBE_COUNT].iter().map(String::as_str).collect();
    // Shares a long prefix with a real word but is not one: the expensive miss,
    // and the one the correction search actually generates.
    let near: Vec<String> = words[..PROBE_COUNT].iter().map(|w| typo(w)).collect();
    // Dies on the first character.
    let far: Vec<String> = words[..PROBE_COUNT]
        .iter()
        .map(|w| format!("Q{w}"))
        .collect();

    let mut g = c.benchmark_group("spellcheck_is_correct");
    g.throughput(Throughput::Elements(PROBE_COUNT as u64));
    g.bench_function("hit", |b| {
        b.iter(|| hits.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.bench_function("miss_near", |b| {
        b.iter(|| near.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.bench_function("miss_far", |b| {
        b.iter(|| far.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.finish();
}

fn bench_get_corrections(c: &mut Criterion) {
    let ascii = words();
    let ru = cyrillic(&ascii);
    // Length is the dominant cost — the candidate count is 26 + 54n per level —
    // so the probes are pinned to a length rather than to a position.
    let probe = typo(&word_of_length(&ascii, 8));
    let ru_probe = typo(&word_of_length(&ru, 8));

    let mut g = c.benchmark_group("spellcheck_get_corrections_d1");
    for (tag, corpus, probe) in [("ascii", &ascii, &probe), ("cyrillic", &ru, &ru_probe)] {
        for n in CORPUS_SIZES {
            let sc = Spellcheck::new(&corpus[..n]);
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| sc.get_corrections(black_box(probe), 1));
            });
        }
    }
    g.finish();

    // Distance 2 squares the candidate count, so it gets its own group, a
    // deliberately short probe, and only the two corpus sizes a caller might
    // plausibly pay for. A five-character probe is ~87,000 candidates; the
    // eight-character one used above would be ~380,000, which is past the point
    // where the reference baseline can be measured in reasonable time.
    let probe = typo(&word_of_length(&ascii, 6));
    let ru_probe = typo(&word_of_length(&ru, 6));
    let mut g = c.benchmark_group("spellcheck_get_corrections_d2");
    g.sample_size(20);
    for (tag, corpus, probe) in [("ascii", &ascii, &probe), ("cyrillic", &ru, &ru_probe)] {
        for n in [1_000usize, 20_000] {
            let sc = Spellcheck::new(&corpus[..n]);
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| sc.get_corrections(black_box(probe), 2));
            });
        }
    }
    g.finish();
}

/// Sequential vs. `rayon`-parallel batch corrections, at the sizes a caller
/// batching a whole document's misspellings might actually hit.
///
/// The probes are one typo per corpus word, taken in corpus order — the same
/// construction `bench_is_correct`'s `near` uses — rather than one word
/// repeated, so the batch mixes short and long inputs the way a real document
/// would, instead of measuring `n` copies of a single cost.
#[cfg(feature = "parallel")]
fn bench_par_get_corrections_batch(c: &mut Criterion) {
    let ascii = words();
    let sc = Spellcheck::new(&ascii[..20_000]);
    let typos: Vec<String> = ascii[..512].iter().map(|w| typo(w)).collect();
    let probes: Vec<&str> = typos.iter().map(String::as_str).collect();

    // Distance 2, matching `bench_get_corrections`'s d2 group: this is the
    // expensive case (3.9-5.5 ms/call on a single word, see that group's
    // results) where a batch fan-out has something to amortise. Sample size
    // is lowered for the same reason it is there.
    let mut g = c.benchmark_group("spellcheck_par_get_corrections_batch_d2");
    g.sample_size(10);
    for n in [8usize, 64, 512] {
        let batch = &probes[..n];
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("sequential", n), &n, |b, _| {
            b.iter(|| {
                batch
                    .iter()
                    .map(|w| sc.get_corrections(black_box(w), 2))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), &n, |b, _| {
            b.iter(|| sc.par_get_corrections_batch(black_box(batch), 2));
        });
    }
    g.finish();
}

fn bench_edits(c: &mut Criterion) {
    let mut g = c.benchmark_group("spellcheck_edits");
    for word in ["ab", "cat", "something", "abcdefghijklmnop"] {
        let units: Vec<u16> = word.encode_utf16().collect();
        // 26 + 54n raw candidates before dedup: the honest denominator for a
        // per-candidate cost.
        g.throughput(Throughput::Elements((26 + 54 * word.len()) as u64));

        // The lazy primitive: nothing is copied out per candidate.
        g.bench_with_input(BenchmarkId::new("next_edit", word), word, |b, _| {
            b.iter(|| {
                let mut it = Edits::new(black_box(word.as_bytes()));
                let mut n = 0usize;
                while let Some(e) = it.next_edit() {
                    n += e.len();
                }
                n
            });
        });
        // The owning iterator, one `Vec<u16>` per candidate.
        g.bench_with_input(BenchmarkId::new("collect_utf16", word), word, |b, _| {
            b.iter(|| Edits::new(black_box(units.as_slice())).count());
        });
        // The convenience API, one `String` per candidate — the shape the
        // reference always pays for.
        g.bench_with_input(BenchmarkId::new("edits_string", word), word, |b, _| {
            b.iter(|| edits(black_box(word)).len());
        });
    }

    // Non-ASCII, where the generator cannot use `&[u8]` at all.
    for word in ["café", "Москва", "a😀b"] {
        g.throughput(Throughput::Elements(
            (26 + 54 * word.encode_utf16().count()) as u64,
        ));
        g.bench_with_input(BenchmarkId::new("utf16", word), word, |b, _| {
            b.iter(|| edits_utf16(black_box(word)).len());
        });
    }
    g.finish();
}

fn bench_sort(c: &mut Criterion) {
    /// A deterministic PRNG, so every run sorts the same permutation. The
    /// the reference baseline uses the same recurrence and the same seed.
    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    let mut g = c.benchmark_group("spellcheck_sort_by_frequency");
    // 4 takes the short-array path, 64 the first TimSort length, 500 a single
    // merge, 20000 a deep merge stack with galloping.
    for n in [4usize, 64, 500, 20_000] {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let finite: Vec<(u32, f64)> = (0..n)
            .map(|i| {
                (
                    u32::try_from(i).expect("index fits in u32"),
                    (xorshift(&mut state) % 64) as f64,
                )
            })
            .collect();
        // One frequency in eight is NaN — the shape a corpus containing
        // `'constructor'` or `'toString'` exactly once actually produces.
        let with_nan: Vec<(u32, f64)> = finite
            .iter()
            .map(|&(i, f)| if i % 8 == 0 { (i, f64::NAN) } else { (i, f) })
            .collect();

        g.throughput(Throughput::Elements(n as u64));
        for (tag, data) in [("finite", &finite), ("with_nan", &with_nan)] {
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter_batched_ref(
                    || data.clone(),
                    |v| sort_by_frequency(black_box(v), |p| p.1),
                    BatchSize::SmallInput,
                );
            });
        }
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_construction,
    bench_is_correct,
    bench_get_corrections,
    bench_edits,
    bench_sort
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_par_get_corrections_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);

#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
