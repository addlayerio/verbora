//! Verbora vs. real, pinned third-party Rust competitors — string distance.
//!
//! Reads the exact same `benches/data/distance-pairs.json` that
//! `crates/verbora-distance/benches/distance.rs` (Verbora) already reads, so
//! every implementation — Verbora and the Rust competitors below — runs on
//! byte-identical input. See `docs/COMPETITIVE_BENCHMARKS.md` §1.8
//! for why each competitor was selected (widely-adopted or, for the
//! byte-level trio, the widest-download ASCII-fast alternative found) and
//! `../../README.md` for why this crate lives outside the main workspace.
//!
//! # Which rows are benchmarked, and why
//!
//! **Char-indexed, matrix `Yes` (full algorithmic equivalence):**
//! `strsim`, `rapidfuzz` — Levenshtein, Damerau-Levenshtein (unrestricted
//! and restricted/OSA), Jaro, Jaro-Winkler, Hamming. `stringmetrics` joins
//! for Levenshtein and Hamming only (also matrix `Yes`) — its
//! `damerau_levenshtein` is **deliberately never called**: reading
//! `stringmetrics-2.2.2/src/algorithms.rs` and `algorithms/damerau_impl.rs`
//! directly shows the whole `damerau` module is commented out of both `mod`
//! and `pub use` (not merely an internal stub reachable through the public
//! API — it does not compile into the crate at all in this version), and
//! the function that would back it is two lines that always `return 0`. See
//! `manifests/competitors.json`'s own `stringmetrics` entry for the exact
//! source lines.
//!
//! `eddie` joins for Jaro and Jaro-Winkler only (matrix `Yes`) — abandoned
//! since 2020, so `tests/distance_correctness.rs` re-verifies its output
//! against Verbora's on the shared corpus plus hand-picked edge cases before
//! any number below is trusted, per the matrix's own "re-verify against
//! fresh vectors" flag.
//!
//! **Byte-level, ASCII-only fair (matrix `Selected cases`/`Partial`):**
//! `triple_accel` and `editdistancek` operate on raw `&[u8]`, not
//! chars/UTF-16 units — genuinely different from every char-indexed
//! competitor above on non-ASCII input, but numerically identical to it on
//! ASCII, which is all `load_ascii_pairs` ever supplies. `triple_accel`
//! covers Levenshtein, restricted-only Damerau (`rdamerau` — benchmarked
//! only in the `restricted_osa` group, never `unrestricted`, since
//! `triple_accel` has no unrestricted variant), Hamming, and fuzzy
//! substring search (`levenshtein_search`, a genuinely different problem
//! shape from Verbora's — see `bench_fuzzy_substring_search` below).
//! `editdistancek` covers Levenshtein only, fixed unit costs, no Damerau.
//!
//! Sørensen-Dice is deliberately excluded from every competitor — the
//! matrix documents strsim's and rapidfuzz-adjacent crates' Dice as a
//! different variant (multiset vs. set bigrams, no whitespace-collapse), so
//! publishing a timing number for it here would be exactly the "comparing
//! different things" the project's own fairness rules forbid.
//!
//! # Input shapes: random, near-identical and Levenshtein edge shapes
//!
//! Every group runs two input shapes over the same 4..1024 size sweep, both
//! derived from the one shared `distance-pairs.json` corpus — no second
//! dataset exists that only this benchmark sees:
//!
//! - **random** (`BenchmarkId` parameter = `n`, unchanged from the first
//!   round): the corpus pairs as-is. Independent random lowercase-ASCII
//!   strings land at edit distance ≈ 0.9·n (measured: 4/4, 16/16, 59/64,
//!   229/256 on this exact corpus) — the worst case for every
//!   distance-bounded fast path.
//! - **near-identical** (`parameter = "<n>-near"`): the corpus's own `a`
//!   string against a copy of itself with one substituted midpoint
//!   character — edit distance exactly 1 in every metric family here (one
//!   substitution is one edit for Levenshtein, OSA, unrestricted Damerau,
//!   and Hamming alike). This is the shape where `editdistancek`'s
//!   O(n·d)-style banded search, `triple_accel`'s band-doubling, and
//!   `rapidfuzz`'s common-affix trimming engage, and a plain O(n·m) DP
//!   cannot follow — a difference no amount of extra sizes on the random
//!   shape would reveal, which is why the second shape was preferred over a
//!   denser size grid. Sizes stay the corpus's own five so each
//!   implementation's near-vs-random curves overlay per size. Fairness:
//!   both sides of every near-identical row still receive the literal same
//!   two strings; the derivation is deterministic (midpoint byte, fixed
//!   substitute) and happens once, outside all timed loops.
//! - **planted** (`parameter = "<n>-planted"`, `fuzzy_substring_search`
//!   only): see `bench_fuzzy_substring_search`'s own doc comment — the
//!   search group's shape axis is needle-present vs. needle-absent, not
//!   pair similarity, so it derives its second shape accordingly.
//!
//! `Throughput` for the derived rows follows each group's existing
//! convention (`n·n` elements for the quadratic-DP groups, `n` for the
//! linear-ish ones): it is the *nominal* problem area of the row's inputs,
//! never any one implementation's shortcut, so it stays comparable across
//! shapes exactly as it already is across competitors.
//!
//! Every function call below is wrapped in `black_box` on input and output,
//! per the spec's `BLACK-BOX CHECKSUM` requirement — nothing here is cheap
//! enough for LLVM to plausibly constant-fold across a whole benchmark
//! iteration, but the discipline is applied uniformly rather than assumed.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use eddie::{Jaro as EddieJaro, JaroWinkler as EddieJaroWinkler};

use verbora_distance::{
    hamming, jaro, jaro_winkler,
    levenshtein::{Options as LevOptions, damerau_levenshtein, levenshtein, levenshtein_search},
};

/// The shared ASCII pairs, read from the same file every sibling benchmark
/// (Verbora's own Criterion suite, the reference harness) reads. Kept to
/// ASCII only in this file: `rapidfuzz`'s and `strsim`'s Levenshtein/
/// Damerau/Jaro family are `char`-indexed like Verbora's UTF-16-unit
/// indexing only up to the Basic Multilingual Plane — ASCII input sidesteps
/// that distinction entirely, keeping this specific comparison unambiguous.
fn load_ascii_pairs() -> Vec<(usize, String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/distance-pairs.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    let mut v: Vec<(usize, String, String)> = json["pairs"]["ascii"]
        .as_object()
        .expect("pair map")
        .iter()
        .map(|(k, pair)| {
            (
                k.parse().expect("numeric key"),
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    v.sort_by_key(|(n, _, _)| *n);
    v
}

/// The deterministic near-identical derivation from the module doc comment:
/// the pair's own `a` against a copy of itself with the one midpoint byte
/// substituted for a fixed distinct letter (`q`, or `x` when the midpoint
/// already is `q`) — edit distance exactly 1 under every metric family in
/// this file, same length, still pure lowercase ASCII. Runs once per group,
/// outside every timed loop.
fn near_identical_of(a: &str) -> String {
    let mut bytes = a.as_bytes().to_vec();
    let mid = bytes.len() / 2;
    bytes[mid] = if bytes[mid] == b'q' { b'x' } else { b'q' };
    String::from_utf8(bytes).expect("ASCII in, ASCII out")
}

/// The two pair shapes from the module doc comment, interleaved per size:
/// each corpus row as-is (`BenchmarkId` parameter `"<n>"`, byte-identical to
/// the first round) followed by its near-identical derivation
/// (`"<n>-near"`). Both shapes are built here, once, outside all timed
/// loops — both sides of every row still receive the literal same strings.
fn shaped_pairs() -> Vec<(String, usize, String, String)> {
    let mut rows = Vec::new();
    for (n, a, b) in load_ascii_pairs() {
        let near = near_identical_of(&a);
        rows.push((n.to_string(), n, a.clone(), b));
        rows.push((format!("{n}-near"), n, a, near));
    }
    rows
}

/// Extra Levenshtein-only shapes that make the current fast paths observable
/// in a fair cross-library run. All are lowercase ASCII and unit-cost, so the
/// char- and byte-indexed implementations below have identical semantics.
/// Constructed once, before timing:
///
/// - `near/1024`: one central substitution, exercising common-affix trim;
/// - `disjoint/1024`: no shared symbols, exercising the disjoint-alphabet
///   early return;
/// - `late-overlap/65x10000`: a short pattern whose only overlap occurs at
///   the end of the long target, ensuring that the disjoint probe is not
///   accidentally reported as free work.
fn levenshtein_edge_shapes() -> Vec<(&'static str, String, String)> {
    let near_a = "a".repeat(512) + &"b".repeat(512);
    let mut near_b = near_a.clone().into_bytes();
    near_b[512] = b'c';
    vec![
        (
            "near/1024",
            near_a,
            String::from_utf8(near_b).expect("ASCII input"),
        ),
        ("disjoint/1024", "a".repeat(1024), "b".repeat(1024)),
        (
            "late-overlap/65x10000",
            "z".repeat(65),
            "a".repeat(9_998) + "zb",
        ),
    ]
}

fn bench_levenshtein(c: &mut Criterion) {
    let inputs = shaped_pairs();
    let opts = LevOptions::default();
    let mut g = c.benchmark_group("levenshtein");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(levenshtein(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::levenshtein(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::levenshtein::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
        g.bench_with_input(
            BenchmarkId::new("stringmetrics", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| black_box(stringmetrics::levenshtein(black_box(a), black_box(b))));
            },
        );
        // Byte-level (`&[u8]`), ASCII-only fair — see this file's own module
        // doc comment.
        g.bench_with_input(
            BenchmarkId::new("triple_accel", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(triple_accel::levenshtein(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("editdistancek", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(editdistancek::edit_distance(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
    }
    g.finish();
}

fn bench_levenshtein_edge_shapes(c: &mut Criterion) {
    let inputs = levenshtein_edge_shapes();
    let opts = LevOptions::default();
    let mut g = c.benchmark_group("levenshtein_edge_shapes");
    for (id, a, b) in &inputs {
        // Nominal DP area, independent of any implementation shortcut.
        g.throughput(Throughput::Elements((a.len() * b.len()) as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(levenshtein(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::levenshtein(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::levenshtein::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
        g.bench_with_input(
            BenchmarkId::new("stringmetrics", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| black_box(stringmetrics::levenshtein(black_box(a), black_box(b))));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("triple_accel", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(triple_accel::levenshtein(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("editdistancek", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(editdistancek::edit_distance(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
    }
    g.finish();
}

fn bench_damerau_levenshtein_unrestricted(c: &mut Criterion) {
    let inputs = shaped_pairs();
    let opts = LevOptions {
        restricted: false,
        ..Default::default()
    };
    let mut g = c.benchmark_group("damerau_levenshtein_unrestricted");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(damerau_levenshtein(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::damerau_levenshtein(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::damerau_levenshtein::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
    }
    g.finish();
}

fn bench_damerau_levenshtein_restricted_osa(c: &mut Criterion) {
    let inputs = shaped_pairs();
    let opts = LevOptions {
        restricted: true,
        ..Default::default()
    };
    let mut g = c.benchmark_group("damerau_levenshtein_restricted_osa");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(damerau_levenshtein(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::osa_distance(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::osa::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
        // `rdamerau` is triple_accel's ONLY Damerau variant — restricted
        // (OSA) only, never unrestricted, per the matrix; it never appears
        // in `bench_damerau_levenshtein_unrestricted` above. Byte-level,
        // ASCII-only fair — see this file's own module doc comment.
        g.bench_with_input(
            BenchmarkId::new("triple_accel", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(triple_accel::rdamerau(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
    }
    g.finish();
}

fn bench_jaro(c: &mut Criterion) {
    let inputs = shaped_pairs();
    // `eddie::Jaro` holds reusable internal match-flag buffers (see its own
    // doc comment) — constructed once and reused across calls, exactly like
    // `strsim`'s/`rapidfuzz`'s stateless calls need no such setup and
    // `LevOptions`/`jaro_winkler::Options` above are built once outside the
    // timed loop.
    let ejaro = EddieJaro::new();
    let mut g = c.benchmark_group("jaro");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements(*n as u64));
        // No the reference row here: its own Jaro is an unexported private
        // helper (see docs/COMPETITIVE_BENCHMARKS.md §1.8) — Verbora's own
        // `jaro` is real and public, so it is included.
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(jaro(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::jaro(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::jaro::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
        // Crate abandoned since 2020 — output re-verified against Verbora's
        // in `tests/distance_correctness.rs` before this number is trusted.
        g.bench_with_input(BenchmarkId::new("eddie", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(ejaro.similarity(black_box(a), black_box(b))));
        });
    }
    g.finish();
}

fn bench_jaro_winkler(c: &mut Criterion) {
    let inputs = shaped_pairs();
    let opts = verbora_distance::jaro_winkler::Options::default();
    let ejarwin = EddieJaroWinkler::new();
    let mut g = c.benchmark_group("jaro_winkler");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements(*n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(jaro_winkler(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::jaro_winkler(black_box(a), black_box(b))));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(rapidfuzz::distance::jaro_winkler::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ))
            });
        });
        // Same abandoned-since-2020 caveat as `bench_jaro` above.
        g.bench_with_input(BenchmarkId::new("eddie", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(ejarwin.similarity(black_box(a), black_box(b))));
        });
    }
    g.finish();
}

fn bench_hamming(c: &mut Criterion) {
    // Hamming requires equal-length input; every pair sampled here already
    // is, since both come from the same-length ASCII generator.
    let inputs = shaped_pairs();
    let mut g = c.benchmark_group("hamming");
    for (id, n, a, b) in &inputs {
        g.throughput(Throughput::Elements(*n as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(hamming(black_box(a), black_box(b), false)));
        });
        g.bench_with_input(BenchmarkId::new("strsim", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(strsim::hamming(black_box(a), black_box(b)).unwrap()));
        });
        g.bench_with_input(BenchmarkId::new("rapidfuzz", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| {
                black_box(
                    rapidfuzz::distance::hamming::distance(
                        black_box(a.chars()),
                        black_box(b.chars()),
                    )
                    .unwrap(),
                )
            });
        });
        g.bench_with_input(
            BenchmarkId::new("stringmetrics", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| black_box(stringmetrics::hamming(black_box(a), black_box(b)).unwrap()));
            },
        );
        // Byte-level, ASCII-only fair — see this file's own module doc
        // comment.
        g.bench_with_input(
            BenchmarkId::new("triple_accel", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(triple_accel::hamming(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    ))
                });
            },
        );
    }
    g.finish();
}

/// Fuzzy substring search: `levenshtein_search`/`damerau_levenshtein_search`
/// in the matrix. Verbora returns one best-matching substring with a full
/// backtrace (`SearchResult { substring, distance, offset }`); triple_accel
/// returns an iterator over every match within a bounded edit distance `k`
/// (default `ceil(needle.len() / 2)`) — a genuinely different problem shape,
/// which is exactly why the matrix marks this row `Selected cases`, never
/// `Yes`. Both sides are still real, comparable WORK on the same input (a
/// full edit-distance search of `target`/`haystack` for the closest region
/// to `source`/`needle`), so a timing comparison is fair; an output-value
/// comparison would not be, and none is made here or claimed anywhere in
/// this file — see `tests/distance_correctness.rs` for the one thing that
/// *is* checked (both sides run to completion without panicking, offsets
/// stay in bounds).
///
/// `a` is the needle/pattern, `b` the haystack/target — the same convention
/// `crates/verbora-distance/benches/distance.rs`'s own `search_matrix` group
/// already uses for this identical shared corpus.
///
/// # The planted shape (`parameter = "<n>-planted"`)
///
/// This group's shape axis is needle-present vs. needle-absent, not pair
/// similarity (module doc comment). The random rows are the needle-absent
/// side: independent random strings put the closest region of the haystack
/// at edit distance ≈ 0.9·n from the needle. The planted rows splice the
/// needle verbatim into the midpoint of the same corpus haystack
/// (`b[..n/2] + a + b[n/2..]`, length `2n`) — needle-present at edit
/// distance 0, the shape where triple_accel's bounded-`k` scan terminates
/// early while Verbora's best-match backtrace still walks the whole
/// haystack. Derived from the one shared corpus, deterministically, outside
/// all timed loops. `Throughput` is the nominal search area
/// `needle.len() · haystack.len()` — exactly the existing `n·n` for the
/// random rows (both strings are length `n`) and `2n²` for the planted
/// ones, never any one implementation's early-exit shortcut.
fn plant_needle(a: &str, b: &str) -> String {
    let mid = b.len() / 2;
    format!("{}{}{}", &b[..mid], a, &b[mid..])
}

fn bench_fuzzy_substring_search(c: &mut Criterion) {
    let corpus = load_ascii_pairs();
    let opts = LevOptions::default();
    // Both shapes built once, outside all timed loops — see `plant_needle`'s
    // own doc comment.
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for (n, a, b) in &corpus {
        rows.push((n.to_string(), a.clone(), b.clone()));
        rows.push((format!("{n}-planted"), a.clone(), plant_needle(a, b)));
    }
    let mut g = c.benchmark_group("fuzzy_substring_search");
    for (id, a, b) in &rows {
        g.throughput(Throughput::Elements((a.len() * b.len()) as u64));
        g.bench_with_input(BenchmarkId::new("verbora", id), &(a, b), |bch, (a, b)| {
            bch.iter(|| black_box(levenshtein_search(black_box(a), black_box(b), &opts)));
        });
        g.bench_with_input(
            BenchmarkId::new("triple_accel", id),
            &(a, b),
            |bch, (a, b)| {
                bch.iter(|| {
                    black_box(
                        triple_accel::levenshtein_search(
                            black_box(a.as_bytes()),
                            black_box(b.as_bytes()),
                        )
                        .collect::<Vec<_>>(),
                    )
                });
            },
        );
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_levenshtein,
    bench_levenshtein_edge_shapes,
    bench_damerau_levenshtein_unrestricted,
    bench_damerau_levenshtein_restricted_osa,
    bench_jaro,
    bench_jaro_winkler,
    bench_hamming,
    bench_fuzzy_substring_search,
);
criterion_main!(benches);
