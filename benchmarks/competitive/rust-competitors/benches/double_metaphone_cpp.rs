// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style
// is kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. a real, vendored C++ competitor — Double Metaphone.
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.6 already benchmarks
//! `verbora_phonetics::DoubleMetaphone` against `rphonetic` (Rust). This
//! file adds a second, independent Double Metaphone implementation:
//! [pixelglow/double_metaphone](https://github.com/pixelglow/double_metaphone),
//! a from-scratch C++11 transcription of Lawrence Philips' original
//! algorithm, vendored under `vendor/pixelglow-double_metaphone/` (see that
//! directory's own `README.md` for provenance and license) and compiled by
//! `build.rs` into a small `extern "C"` shim — the one non-Cargo,
//! non-Rust, compiled-from-source competitor in this whole workspace.
//!
//! # Why this is a fair comparison, not just a shared name
//!
//! Both sides implement the same published algorithm directly — Verbora's
//! own transcription (`crates/verbora-phonetics/src/double_metaphone.rs`,
//! ported from the reference implementation this crate's family targets)
//! and pixelglow's C++11 rewrite of Lawrence Philips' original C, both
//! returning a (primary key, secondary key) pair from one word.
//! `tests/double_metaphone_cpp_correctness.rs` pins the real, measured
//! result: **`Partial`, not byte-exact** — 584 of the 653 real English
//! surnames in `benches/data/names.json` (89.4%) agree exactly, the same
//! dataset `crates/verbora-phonetics/benches/phonetics.rs` and this crate's
//! own `benches/phonetics.rs` already use for phonetic-encoder benchmarks.
//! The confirmed, dominant divergence is a single rule difference in the
//! trailing-`S`-after-`[AI]` handling (see that test file's own module doc
//! comment for the full, source-cited account) — not a bug on either side,
//! and it does not disqualify the throughput comparison below: both sides
//! do the same *shape* of work, one left-to-right scan producing a
//! (primary, secondary) key pair. The correctness test also confirms
//! Verbora's default 32-character output cap never fires on this domain,
//! so the one real API difference between the two (Verbora caps by
//! default, the vendored library never does) never actually shows up in
//! the compared output.
//!
//! # What this does not measure
//!
//! **FFI call overhead is included, not isolated.** Every call below
//! crosses the Rust/C++ boundary once (one `CString` construction, one
//! `extern "C"` call, two bounded buffer copies, one UTF-8 validation on
//! the way back) — that is a real cost a Rust caller embedding this
//! library would actually pay, so it is measured as part of the number,
//! not subtracted out. See `src/double_metaphone_cpp.rs` for exactly what
//! crosses the boundary.
//!
//! **This is one C++ implementation, not "C++ in general."** No claim is
//! made about C++ Double Metaphone implementations generally — only about
//! this specific, vendored, frozen-since-2014 one.

use std::hint::black_box;

use competitive_rust::double_metaphone_cpp::double_metaphone_cpp;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use verbora_phonetics::DoubleMetaphone;

/// The shared name list, read from `benches/data/names.json` — the same
/// file `crates/verbora-phonetics/benches/phonetics.rs` and this crate's
/// own `benches/phonetics.rs` already use for phonetic-encoder benchmarks.
fn load_names() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/names.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["names"]
        .as_array()
        .expect("name list")
        .iter()
        .map(|n| n.as_str().expect("name").to_owned())
        .collect()
}

fn bench_double_metaphone(c: &mut Criterion) {
    let names = load_names();
    let dm = DoubleMetaphone::new();

    let mut g = c.benchmark_group("double_metaphone_cpp");
    g.throughput(Throughput::Elements(names.len() as u64));

    g.bench_function("verbora", |b| {
        b.iter(|| {
            names
                .iter()
                .map(|n| dm.process(black_box(n)))
                .map(|(p, s)| p.len() + s.len())
                .sum::<usize>()
        });
    });
    g.bench_function("pixelglow_cpp", |b| {
        b.iter(|| {
            names
                .iter()
                .map(|n| double_metaphone_cpp(black_box(n)))
                .map(|(p, s)| p.len() + s.len())
                .sum::<usize>()
        });
    });

    g.finish();
}

criterion_group!(benches, bench_double_metaphone);
criterion_main!(benches);
