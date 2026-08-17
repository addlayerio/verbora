// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style is
// kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. real, pinned third-party Rust competitors — language
//! detection, script detection, and Japanese transliteration.
//!
//! Reads the exact same `datasets/language-accuracy/dataset.json` that
//! `examples/language_accuracy.rs` reads, so the timing numbers here and the
//! accuracy numbers there describe the same 13-language, four-tier text —
//! see `datasets/README.md` for its full sourcing (OHCHR's UDHR Translation
//! Project via the `eric-muller/udhr` "UDHR in XML" corpus) and extraction
//! methodology. See `docs/COMPETITIVE_BENCHMARKS.md` §1.9-1.11 for why
//! `whatlang`, `lingua`, `whichlang`, and `wana_kana` were selected, and
//! `../../README.md` for why this crate lives outside the main workspace.
//!
//! # Seven groups, four different fairness shapes
//!
//! 1. [`bench_whatlang_wrapper_overhead`] — Verbora's `WhatlangDetector`
//!    against the raw `whatlang::Detector` it wraps. **This is not a
//!    rival-algorithm comparison** — `WhatlangDetector::detect` literally
//!    constructs a `whatlang::Detector` and calls `.detect()` on it
//!    (`crates/verbora-language/src/whatlang_detector.rs`) — so this group
//!    exists only to show wrapper overhead is ~0, never reported as
//!    "Verbora beats/loses to whatlang" (matrix's own explicit instruction).
//! 2. [`bench_language_detection_by_length`] and
//!    [`bench_language_detection_by_language`] — the real three-way
//!    algorithm comparison: Verbora (via `WhatlangDetector`, n-gram +
//!    alphabet filter) vs. `lingua` (rule engine + 1-5-gram, restricted to
//!    the 21-language overlap via `from_languages()` — never its default
//!    75-language configuration) vs. `whichlang` (hashed n-gram linear
//!    model, 13-language overlap, cannot abstain). Split into "by length"
//!    (one language, all four tiers) and "by language" (13 languages, one
//!    fixed tier) rather than one combinatorial group, so each Criterion
//!    report reads as a clean one-dimensional sweep.
//! 3. [`bench_script_detection_by_length`] and
//!    [`bench_script_detection_by_language`] — Verbora's `detect_script`
//!    (one match per codepoint) against `whatlang::detect_script` (linear
//!    scan of up to 25 predicates per codepoint) — a real, legitimate
//!    algorithmic-complexity comparison per the matrix's own note, not a
//!    disqualified one.
//! 4. [`bench_transliteration_ja`] — Verbora's `transliterate_ja` against
//!    `wana_kana`'s `to_romaji`. **Throughput only.** `wana_kana` uses a
//!    doubled-vowel romanization convention (`"スーパー"` -> `"suupaa"`)
//!    while Verbora/the reference use modified Hepburn with macrons
//!    (`"tōkyō"`) — see `tests/transliteration_convention_diff.rs` for a
//!    real, executed proof the two outputs differ, not an assumed one. This
//!    group never compares output values, only wall-clock time on
//!    byte-identical kana input.
//!
//! # `lingua`'s per-call allocation is real, not a benchmarking artifact
//!
//! `LanguageDetector::detect_language_of<T: Into<String>>(&self, text: T)`
//! takes its input *by value*, converting a `&str` argument into an owned
//! `String` on every call (`lingua-1.8.0/src/detector.rs`). That allocation
//! is part of `lingua`'s real public API contract — any real caller pays
//! it — so it is included in the timed region here exactly as it would be
//! for a production caller, not special-cased away.
//!
//! Every implementation call below is wrapped in `black_box` on both input
//! and output, per the spec's `BLACK-BOX CHECKSUM` requirement.

use std::hint::black_box;

use criterion::measurement::Measurement;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};

use competitive_rust::language_support::{TIERS, lingua_restricted_languages, load_dataset};
use verbora_language::{LanguageDetector, WhatlangDetector, detect_script};
use verbora_transliterators::transliterate_ja;
use wana_kana::ConvertJapanese;

/// Kana-only prose (identical to `verbora-transliterators/benches/transliterators.rs`'s
/// own `kana_prose`) — every character is rewritten by some phase on the
/// Verbora side, and by `wana_kana`'s own kana table on the other.
fn kana_prose(repeats: usize) -> String {
    "とうきょうとっきょきょかきょくのぼーじょれーぬーゔぉー".repeat(repeats)
}

/// A reduced sampling budget applied to every group in this file — the same
/// values `crates/verbora-language/benches/language.rs` and
/// `crates/verbora-phonetics/benches/phonetic_index.rs` use for the same
/// reason: this file has 6 groups spanning up to 39 `(id, tier/language)`
/// combinations each (`language_detection_by_language`'s 13 languages x 3
/// detectors), so Criterion's un-tuned defaults (3 s warm-up + 5 s
/// measurement per point) would push total run time past a reasonable
/// budget without changing what is actually being measured.
/// `sample_size(30)` is Criterion's own documented minimum recommended
/// size, not an arbitrary cut, and is applied identically to every
/// implementation in every group — never tuned per-competitor.
fn configure<M: Measurement>(g: &mut BenchmarkGroup<'_, M>) {
    g.sample_size(30);
    g.measurement_time(std::time::Duration::from_millis(1500));
    g.warm_up_time(std::time::Duration::from_millis(400));
}

/// A higher-rigor budget for [`bench_whatlang_wrapper_overhead`] only — it
/// has just 8 `(id, tier)` points (vs. up to 39 elsewhere in this file), so
/// affording Criterion's own default-ish sample size here is cheap, and
/// this specific comparison needs it: an initial run at [`configure`]'s
/// reduced budget produced a "verbora_wrapper" number that disagreed with
/// the *same* `WhatlangDetector::detect` call's number in
/// `language_detection_by_length` by up to 2x on identical input — a
/// measurement-noise artifact of the reduced sample size, not a real
/// semantic difference (both call sites run the literal same function).
/// This group's specific claim ("wrapper overhead is small") deserves a
/// number solid enough to trust, not the same noise floor as the other
/// five groups' broader sweeps.
fn configure_precise<M: Measurement>(g: &mut BenchmarkGroup<'_, M>) {
    g.sample_size(100);
    g.measurement_time(std::time::Duration::from_secs(3));
    g.warm_up_time(std::time::Duration::from_secs(1));
}

// ---------------------------------------------------------------------------
// 1. Wrapper overhead — NOT a rival-algorithm comparison.
// ---------------------------------------------------------------------------

fn bench_whatlang_wrapper_overhead(c: &mut Criterion) {
    let dataset = load_dataset();
    let english = dataset
        .iter()
        .find(|l| l.iso639_1 == "en")
        .expect("english is in the dataset");

    let mut g = c.benchmark_group("whatlang_wrapper_overhead");
    configure_precise(&mut g);
    let verbora = WhatlangDetector::new();
    for tier in TIERS {
        let text = english.items.get(tier);
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora_wrapper", tier),
            &text,
            |b, &text| {
                b.iter(|| black_box(verbora.detect(black_box(text))));
            },
        );
        // Exactly what WhatlangDetector::detect does internally: construct
        // a fresh whatlang::Detector and call .detect() on it (its own
        // `new()` is documented as allocation-free — see
        // crates/verbora-language/benches/language.rs's own doc comment).
        g.bench_with_input(BenchmarkId::new("raw_whatlang", tier), &text, |b, &text| {
            b.iter(|| black_box(whatlang::Detector::new().detect(black_box(text))));
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// 2. The real three-way algorithm comparison.
// ---------------------------------------------------------------------------

/// By length: one language (English — inside every detector's unrestricted
/// operating range), all four tiers.
fn bench_language_detection_by_length(c: &mut Criterion) {
    let dataset = load_dataset();
    let english = dataset
        .iter()
        .find(|l| l.iso639_1 == "en")
        .expect("english is in the dataset");
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut g = c.benchmark_group("language_detection_by_length");
    configure(&mut g);
    for tier in TIERS {
        let text = english.items.get(tier);
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", tier), &text, |b, &text| {
            b.iter(|| black_box(verbora.detect(black_box(text))));
        });
        g.bench_with_input(BenchmarkId::new("lingua", tier), &text, |b, &text| {
            b.iter(|| black_box(lingua_detector.detect_language_of(black_box(text))));
        });
        g.bench_with_input(BenchmarkId::new("whichlang", tier), &text, |b, &text| {
            b.iter(|| black_box(whichlang::detect_language(black_box(text))));
        });
    }
    g.finish();
}

/// By language: all 13 languages in the triple overlap, fixed at the
/// `sentence` tier — long enough that none of the three detectors is
/// operating outside the length range its own documentation targets.
fn bench_language_detection_by_language(c: &mut Criterion) {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut g = c.benchmark_group("language_detection_by_language");
    configure(&mut g);
    for entry in &dataset {
        let text = entry.items.get("sentence");
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora", &entry.iso639_1),
            &text,
            |b, &text| b.iter(|| black_box(verbora.detect(black_box(text)))),
        );
        g.bench_with_input(
            BenchmarkId::new("lingua", &entry.iso639_1),
            &text,
            |b, &text| b.iter(|| black_box(lingua_detector.detect_language_of(black_box(text)))),
        );
        g.bench_with_input(
            BenchmarkId::new("whichlang", &entry.iso639_1),
            &text,
            |b, &text| b.iter(|| black_box(whichlang::detect_language(black_box(text)))),
        );
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// 3. Script detection — a legitimate complexity comparison.
// ---------------------------------------------------------------------------

/// By length: English (Latin script) across all four tiers.
fn bench_script_detection_by_length(c: &mut Criterion) {
    let dataset = load_dataset();
    let english = dataset
        .iter()
        .find(|l| l.iso639_1 == "en")
        .expect("english is in the dataset");

    let mut g = c.benchmark_group("script_detection_by_length");
    configure(&mut g);
    for tier in TIERS {
        let text = english.items.get(tier);
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", tier), &text, |b, &text| {
            b.iter(|| black_box(detect_script(black_box(text))));
        });
        g.bench_with_input(BenchmarkId::new("whatlang", tier), &text, |b, &text| {
            b.iter(|| black_box(whatlang::detect_script(black_box(text))));
        });
    }
    g.finish();
}

/// By language/script: all 13 languages at the `sentence` tier — Latin
/// (en/es/fr/de/it/pt/nl/sv), Cyrillic (ru), Devanagari (hi), Han (zh), and
/// a genuine Hiragana/Katakana/Han mix (ja).
fn bench_script_detection_by_language(c: &mut Criterion) {
    let dataset = load_dataset();

    let mut g = c.benchmark_group("script_detection_by_language");
    configure(&mut g);
    for entry in &dataset {
        let text = entry.items.get("sentence");
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora", &entry.iso639_1),
            &text,
            |b, &text| b.iter(|| black_box(detect_script(black_box(text)))),
        );
        g.bench_with_input(
            BenchmarkId::new("whatlang", &entry.iso639_1),
            &text,
            |b, &text| b.iter(|| black_box(whatlang::detect_script(black_box(text)))),
        );
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// 4. Transliteration — throughput only, never output correctness.
// ---------------------------------------------------------------------------

fn bench_transliteration_ja(c: &mut Criterion) {
    let mut g = c.benchmark_group("transliteration_ja");
    configure(&mut g);
    for reps in [1usize, 16, 256] {
        let input = kana_prose(reps);
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", reps), &input, |b, s| {
            b.iter(|| black_box(transliterate_ja(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("wana_kana", reps), &input, |b, s| {
            b.iter(|| black_box(black_box(s.as_str()).to_romaji()));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_whatlang_wrapper_overhead,
    bench_language_detection_by_length,
    bench_language_detection_by_language,
    bench_script_detection_by_length,
    bench_script_detection_by_language,
    bench_transliteration_ja,
);
criterion_main!(benches);
