//! Allocation-count and RSS report for `docs/COMPETITIVE_BENCHMARKS.md`
//! §1.8's Distances module — the memory dimension `benches/distance.rs`
//! (timing only) and every other file in this suite never measured before
//! `rust-competitors/src/memory.rs` existed (see this repo's own
//! `PARTIAL` finding on that point).
//!
//! Per `Fase 6 Benchmark.md`'s `MEMORY BENCHMARKS` section ("Performance NO
//! significa solamente tiempo"), a competitor that wins on latency but
//! allocates far more per call is not simply "faster" without that context.
//! This file makes that measurement for every (group, size, implementation)
//! triple `benches/distance.rs` benchmarks for speed, using
//! [`memory::measure`] — single-threaded, one clean call per cell, no
//! batching/statistics (see that module's own doc comment for why one call
//! is the real number here, unlike Criterion's repeated sampling).
//!
//! Every implementation is set up identically to its `benches/distance.rs`
//! counterpart: any struct state is built once outside the measured closure,
//! so what is measured is the same "one distance call" both files' timing and
//! memory numbers describe — not construction cost. Verbora's own entry
//! points take no configuration, so there is nothing to hoist on that side.
//!
//! **No `eddie` row, by the same rule `benches/distance.rs` states.** `eddie`
//! 0.4.2's published `str` API executes undefined behaviour on every call, so
//! neither a time nor a memory figure derived from it is reportable; its
//! sound `slice` API survives only as a correctness oracle in
//! `tests/distance_correctness.rs`. Every `"eddie"` row already present in
//! `../results/distance-memory.json` predates that finding and is retired —
//! it will disappear on the next run of this example. See
//! `tests/distance_correctness.rs`'s `eddie_slice` module.
//!
//! Run with: `cargo run --release --example distance_memory`
//!
//! Writes `../results/distance-memory.json` (every number in this file's
//! stdout table traces back to that JSON) and prints a human-readable
//! table to stdout.

use std::fs;
use std::hint::black_box;
use std::path::Path;

use competitive_rust::memory;
use serde::Serialize;
use verbora_distance::{
    damerau_levenshtein, hamming, jaro, jaro_winkler, levenshtein, levenshtein_search,
    osa as verbora_osa,
};

#[derive(Debug, Clone, Serialize)]
struct MemoryRow {
    group: &'static str,
    size: usize,
    implementation: &'static str,
    allocations: u64,
    bytes_allocated: u64,
    deallocations: u64,
    bytes_deallocated: u64,
    rss_kb_after: Option<u64>,
}

/// Same loader, same file, as `benches/distance.rs` and
/// `tests/distance_correctness.rs`.
fn load_ascii_pairs() -> Vec<(usize, String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/distance-pairs.json");
    let body = fs::read_to_string(&path).unwrap_or_else(|e| {
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

fn main() {
    let pairs = load_ascii_pairs();
    let mut rows = Vec::new();

    for (n, a, b) in &pairs {
        let n = *n;

        // --- levenshtein ---
        record(&mut rows, "levenshtein", n, "verbora", || {
            black_box(levenshtein(black_box(a), black_box(b)));
        });
        record(&mut rows, "levenshtein", n, "strsim", || {
            black_box(strsim::levenshtein(black_box(a), black_box(b)));
        });
        record(&mut rows, "levenshtein", n, "rapidfuzz", || {
            black_box(rapidfuzz::distance::levenshtein::distance(
                black_box(a.chars()),
                black_box(b.chars()),
            ));
        });
        record(&mut rows, "levenshtein", n, "stringmetrics", || {
            black_box(stringmetrics::levenshtein(black_box(a), black_box(b)));
        });
        record(&mut rows, "levenshtein", n, "triple_accel", || {
            black_box(triple_accel::levenshtein(
                black_box(a.as_bytes()),
                black_box(b.as_bytes()),
            ));
        });
        record(&mut rows, "levenshtein", n, "editdistancek", || {
            black_box(editdistancek::edit_distance(
                black_box(a.as_bytes()),
                black_box(b.as_bytes()),
            ));
        });

        // --- damerau_levenshtein_unrestricted ---
        record(
            &mut rows,
            "damerau_levenshtein_unrestricted",
            n,
            "verbora",
            || {
                black_box(damerau_levenshtein(black_box(a), black_box(b)));
            },
        );
        record(
            &mut rows,
            "damerau_levenshtein_unrestricted",
            n,
            "strsim",
            || {
                black_box(strsim::damerau_levenshtein(black_box(a), black_box(b)));
            },
        );
        record(
            &mut rows,
            "damerau_levenshtein_unrestricted",
            n,
            "rapidfuzz",
            || {
                black_box(rapidfuzz::distance::damerau_levenshtein::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ));
            },
        );

        // --- damerau_levenshtein_restricted_osa ---
        record(
            &mut rows,
            "damerau_levenshtein_restricted_osa",
            n,
            "verbora",
            || {
                black_box(verbora_osa(black_box(a), black_box(b)));
            },
        );
        record(
            &mut rows,
            "damerau_levenshtein_restricted_osa",
            n,
            "strsim",
            || {
                black_box(strsim::osa_distance(black_box(a), black_box(b)));
            },
        );
        record(
            &mut rows,
            "damerau_levenshtein_restricted_osa",
            n,
            "rapidfuzz",
            || {
                black_box(rapidfuzz::distance::osa::distance(
                    black_box(a.chars()),
                    black_box(b.chars()),
                ));
            },
        );
        record(
            &mut rows,
            "damerau_levenshtein_restricted_osa",
            n,
            "triple_accel",
            || {
                black_box(triple_accel::rdamerau(
                    black_box(a.as_bytes()),
                    black_box(b.as_bytes()),
                ));
            },
        );

        // --- jaro ---
        record(&mut rows, "jaro", n, "verbora", || {
            black_box(jaro(black_box(a), black_box(b)));
        });
        record(&mut rows, "jaro", n, "strsim", || {
            black_box(strsim::jaro(black_box(a), black_box(b)));
        });
        record(&mut rows, "jaro", n, "rapidfuzz", || {
            black_box(rapidfuzz::distance::jaro::distance(
                black_box(a.chars()),
                black_box(b.chars()),
            ));
        });

        // --- jaro_winkler ---
        record(&mut rows, "jaro_winkler", n, "verbora", || {
            black_box(jaro_winkler(black_box(a), black_box(b)));
        });
        record(&mut rows, "jaro_winkler", n, "strsim", || {
            black_box(strsim::jaro_winkler(black_box(a), black_box(b)));
        });
        record(&mut rows, "jaro_winkler", n, "rapidfuzz", || {
            black_box(rapidfuzz::distance::jaro_winkler::distance(
                black_box(a.chars()),
                black_box(b.chars()),
            ));
        });

        // --- hamming ---
        record(&mut rows, "hamming", n, "verbora", || {
            black_box(hamming(black_box(a), black_box(b)));
        });
        record(&mut rows, "hamming", n, "strsim", || {
            black_box(strsim::hamming(black_box(a), black_box(b)).unwrap());
        });
        record(&mut rows, "hamming", n, "rapidfuzz", || {
            black_box(
                rapidfuzz::distance::hamming::distance(black_box(a.chars()), black_box(b.chars()))
                    .unwrap(),
            );
        });
        record(&mut rows, "hamming", n, "stringmetrics", || {
            black_box(stringmetrics::hamming(black_box(a), black_box(b)).unwrap());
        });
        record(&mut rows, "hamming", n, "triple_accel", || {
            black_box(triple_accel::hamming(
                black_box(a.as_bytes()),
                black_box(b.as_bytes()),
            ));
        });

        // --- fuzzy_substring_search ---
        record(&mut rows, "fuzzy_substring_search", n, "verbora", || {
            black_box(levenshtein_search(black_box(a), black_box(b)));
        });
        record(
            &mut rows,
            "fuzzy_substring_search",
            n,
            "triple_accel",
            || {
                black_box(
                    triple_accel::levenshtein_search(
                        black_box(a.as_bytes()),
                        black_box(b.as_bytes()),
                    )
                    .collect::<Vec<_>>(),
                );
            },
        );
    }

    print_report(&rows);

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .expect("rust-competitors/ has a parent")
        .join("results/distance-memory.json");
    let json = serde_json::to_string_pretty(&rows).expect("serializable rows");
    fs::write(&out_path, json)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_path.display()));
    println!("\nwrote {}", out_path.display());
}

/// Runs `f` once under [`memory::measure`] and appends the resulting row.
fn record(
    rows: &mut Vec<MemoryRow>,
    group: &'static str,
    size: usize,
    implementation: &'static str,
    f: impl Fn(),
) {
    // One identical, unmeasured warm-up call first — same convention
    // `docs/PERFORMANCE_GAPS.md`'s entry 18 (`whatlang`/`whichlang`
    // per-call memory) already established for this suite. It matters for
    // any implementation holding grow-on-demand state reused across calls:
    // measuring the very first call at each new, larger size in this sweep
    // would report a one-time buffer-growth cost, not the steady-state
    // per-call cost every other implementation here already reports.
    f();
    let (_, report) = memory::measure(f);
    rows.push(MemoryRow {
        group,
        size,
        implementation,
        allocations: report.allocations,
        bytes_allocated: report.bytes_allocated,
        deallocations: report.deallocations,
        bytes_deallocated: report.bytes_deallocated,
        rss_kb_after: report.rss_kb_after,
    });
}

fn print_report(rows: &[MemoryRow]) {
    println!(
        "{:<36} {:>6} {:<14} {:>6} {:>12} {:>6} {:>14}",
        "group", "size", "impl", "allocs", "bytes_alloc", "deallocs", "bytes_dealloc"
    );
    for r in rows {
        println!(
            "{:<36} {:>6} {:<14} {:>6} {:>12} {:>6} {:>14}",
            r.group,
            r.size,
            r.implementation,
            r.allocations,
            r.bytes_allocated,
            r.deallocations,
            r.bytes_deallocated
        );
    }
}
