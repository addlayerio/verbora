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
//! # Nine groups, four different fairness shapes
//!
//! 1. [`bench_whatlang_wrapper_overhead`] — Verbora's `WhatlangDetector`
//!    against the raw `whatlang::Detector` it wraps. **This is not a
//!    rival-algorithm comparison** — `WhatlangDetector::detect` literally
//!    constructs a `whatlang::Detector` and calls `.detect()` on it
//!    (`crates/verbora-language/src/whatlang_detector.rs`) — so this group
//!    exists only to show wrapper overhead is ~0, never reported as
//!    "Verbora beats/loses to whatlang" (matrix's own explicit instruction).
//! 2. [`bench_language_detection_by_length`],
//!    [`bench_language_detection_by_language`], and
//!    [`bench_language_detection_by_language_paragraph`] — the real
//!    three-way algorithm comparison: Verbora (via `WhatlangDetector`,
//!    n-gram + alphabet filter) vs. `lingua` (rule engine + 1-5-gram,
//!    restricted to the 21-language overlap via `from_languages()` — never
//!    its default 75-language configuration) vs. `whichlang` (hashed n-gram
//!    linear model, 13-language overlap, cannot abstain). Split into "by
//!    length" (one language, the full length ladder) and "by language" (13
//!    languages, one fixed tier — `sentence` and `paragraph` as two separate
//!    groups) rather than one combinatorial group, so each Criterion report
//!    reads as a clean one-dimensional sweep.
//! 3. [`bench_script_detection_by_length`],
//!    [`bench_script_detection_by_language`], and
//!    [`bench_script_detection_by_language_paragraph`] — Verbora's
//!    `detect_script` (one match per codepoint) against
//!    `whatlang::detect_script` (linear scan of up to 25 predicates per
//!    codepoint) — a real, legitimate algorithmic-complexity comparison per
//!    the matrix's own note, not a disqualified one.
//! 4. [`bench_transliteration_ja`] and [`bench_transliteration_ja_by_shape`]
//!    — Verbora's `transliterate_ja` against `wana_kana`'s `to_romaji`.
//!    **Throughput only.** `wana_kana` uses a doubled-vowel romanization
//!    convention (`"スーパー"` -> `"suupaa"`) while Verbora/the reference
//!    use modified Hepburn with macrons (`"tōkyō"`) — see
//!    `tests/transliteration_convention_diff.rs` for a real, executed proof
//!    the two outputs differ (long vowels, `を`, `ッ` before the d-row,
//!    punctuation, half-width katakana), not an assumed one. These groups
//!    never compare output values, only wall-clock time on byte-identical
//!    kana input.
//!
//! # The length ladder extends the dataset's tiers by whole-paragraph repetition
//!
//! The dataset's own four tiers top out at one ~700-byte paragraph, so every
//! "by length" group extends the grid with `paragraph.repeat(4/16/64)` —
//! derived from the same committed dataset, never a new source corpus. This
//! is fair to every implementation identically: each one reads the literal
//! same repeated bytes, and for the statistical detectors a repeated
//! paragraph has the *identical* n-gram distribution as the paragraph
//! itself, so the extension isolates per-byte scaling without changing the
//! classification problem (`tests/language_detection_correctness.rs`
//! verifies every detector still answers correctly on the repeated inputs
//! before these cells are trusted). The wrapper-overhead group stops at
//! `x16`: its claim ("wrapper overhead is ~0") is already resolved well
//! below that size, and its higher-rigor budget (see [`configure_precise`])
//! makes `x64` disproportionately expensive for no additional information.
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

use competitive_rust::language_support::{
    LanguageEntry, TIERS, lingua_restricted_languages, load_dataset,
};
use verbora_language::{LanguageDetector, WhatlangDetector, detect_script};
use verbora_transliterators::transliterate_ja;
use wana_kana::ConvertJapanese;

/// Kana-only prose (identical to `verbora-transliterators/benches/transliterators.rs`'s
/// own `kana_prose`) — every character is rewritten by some phase on the
/// Verbora side, and by `wana_kana`'s own kana table on the other.
fn kana_prose(repeats: usize) -> String {
    "とうきょうとっきょきょかきょくのぼーじょれーぬーゔぉー".repeat(repeats)
}

/// The by-length ladder for one language: the dataset's own four tiers,
/// extended past `paragraph` with whole-paragraph repetitions (see the
/// module doc comment's fairness note on why repetition is a legitimate,
/// distribution-preserving way to grow the grid from this committed
/// dataset). `max_reps` caps the extension per group — the wrapper-overhead
/// group stops at 16, the algorithm groups run to 64.
fn length_ladder(entry: &LanguageEntry, max_reps: usize) -> Vec<(String, String)> {
    let mut ladder: Vec<(String, String)> = TIERS
        .iter()
        .map(|&tier| (tier.to_owned(), entry.items.get(tier).to_owned()))
        .collect();
    let paragraph = entry.items.get("paragraph");
    for reps in [4usize, 16, 64] {
        if reps <= max_reps {
            ladder.push((format!("paragraph_x{reps}"), paragraph.repeat(reps)));
        }
    }
    ladder
}

/// Repeats `base` whole-string until the result is at least `target_bytes`
/// long — used by [`bench_transliteration_ja_by_shape`] so its four shapes
/// are compared at one comparable byte budget instead of four accidental
/// ones, while every shape stays a pure repetition of text derived from
/// data this workspace already commits.
fn repeat_to_bytes(base: &str, target_bytes: usize) -> String {
    assert!(!base.is_empty(), "cannot repeat an empty base string");
    base.repeat(target_bytes.div_ceil(base.len()))
}

/// A reduced sampling budget applied to every group in this file — the same
/// values `crates/verbora-language/benches/language.rs` and
/// `crates/verbora-phonetics/benches/phonetic_index.rs` use for the same
/// reason: this file has 9 groups spanning up to 39 `(id, tier/language)`
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
/// has just 12 `(id, tier)` points (vs. up to 39 elsewhere in this file), so
/// affording Criterion's own default-ish sample size here is cheap, and
/// this specific comparison needs it: an initial run at [`configure`]'s
/// reduced budget produced a "verbora_wrapper" number that disagreed with
/// the *same* `WhatlangDetector::detect` call's number in
/// `language_detection_by_length` by up to 2x on identical input — a
/// measurement-noise artifact of the reduced sample size, not a real
/// semantic difference (both call sites run the literal same function).
/// This group's specific claim ("wrapper overhead is small") deserves a
/// number solid enough to trust, not the same noise floor as the other
/// groups' broader sweeps.
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
    for (label, text) in length_ladder(english, 16) {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora_wrapper", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(verbora.detect(black_box(text))));
            },
        );
        // Exactly what WhatlangDetector::detect does internally: construct
        // a fresh whatlang::Detector and call .detect() on it (its own
        // `new()` is documented as allocation-free — see
        // crates/verbora-language/benches/language.rs's own doc comment).
        g.bench_with_input(
            BenchmarkId::new("raw_whatlang", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(whatlang::Detector::new().detect(black_box(text))));
            },
        );
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// 2. The real three-way algorithm comparison.
// ---------------------------------------------------------------------------

/// By length: one language (English — inside every detector's unrestricted
/// operating range), the full length ladder (four dataset tiers plus
/// paragraph x4/x16/x64 — see the module doc comment).
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
    for (label, text) in length_ladder(english, 64) {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(verbora.detect(black_box(text))));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("lingua", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(lingua_detector.detect_language_of(black_box(text))));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("whichlang", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(whichlang::detect_language(black_box(text))));
            },
        );
    }
    g.finish();
}

/// Shared body for the two by-language groups: all 13 languages in the
/// triple overlap, one fixed tier per group. `sentence` is long enough that
/// none of the three detectors is operating outside the length range its
/// own documentation targets; `paragraph` (the second group) is every
/// detector's *easiest* regime — `tests/language_detection_correctness.rs`
/// proves all three answer correctly on every one of its 13 texts — so its
/// numbers compare the algorithms' per-byte cost with accuracy held at
/// 100% for all of them, a dimension the sentence tier alone cannot pin.
fn bench_language_detection_at_tier(c: &mut Criterion, group_name: &str, tier: &str) {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut g = c.benchmark_group(group_name);
    configure(&mut g);
    for entry in &dataset {
        let text = entry.items.get(tier);
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

/// By language at the `sentence` tier.
fn bench_language_detection_by_language(c: &mut Criterion) {
    bench_language_detection_at_tier(c, "language_detection_by_language", "sentence");
}

/// By language at the `paragraph` tier — see
/// [`bench_language_detection_at_tier`] for why this second fixed tier
/// earns its own group.
fn bench_language_detection_by_language_paragraph(c: &mut Criterion) {
    bench_language_detection_at_tier(c, "language_detection_by_language_paragraph", "paragraph");
}

// ---------------------------------------------------------------------------
// 3. Script detection — a legitimate complexity comparison.
// ---------------------------------------------------------------------------

/// By length: English (Latin script) across the full length ladder (four
/// dataset tiers plus paragraph x4/x16/x64) — both implementations are
/// linear per-codepoint scans, so the extended sizes are exactly where
/// their per-codepoint constant factors separate cleanly from call
/// overhead.
fn bench_script_detection_by_length(c: &mut Criterion) {
    let dataset = load_dataset();
    let english = dataset
        .iter()
        .find(|l| l.iso639_1 == "en")
        .expect("english is in the dataset");

    let mut g = c.benchmark_group("script_detection_by_length");
    configure(&mut g);
    for (label, text) in length_ladder(english, 64) {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("verbora", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(detect_script(black_box(text))));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("whatlang", &label),
            &text.as_str(),
            |b, &text| {
                b.iter(|| black_box(whatlang::detect_script(black_box(text))));
            },
        );
    }
    g.finish();
}

/// Shared body for the two by-language/script groups: all 13 languages at
/// one fixed tier — Latin (en/es/fr/de/it/pt/nl/sv), Cyrillic (ru),
/// Devanagari (hi), Han (zh), and a genuine Hiragana/Katakana/Han mix (ja).
/// The `paragraph` tier group exists because the two implementations'
/// relative cost is script-dependent (whatlang's linear predicate scan
/// tries Latin-family predicates first), and the per-language byte budgets
/// at the `sentence` tier are small enough that fixed overhead can mask
/// that — the paragraph tier gives each script ~10x more bytes to make the
/// per-codepoint difference visible.
fn bench_script_detection_at_tier(c: &mut Criterion, group_name: &str, tier: &str) {
    let dataset = load_dataset();

    let mut g = c.benchmark_group(group_name);
    configure(&mut g);
    for entry in &dataset {
        let text = entry.items.get(tier);
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

/// By language/script at the `sentence` tier.
fn bench_script_detection_by_language(c: &mut Criterion) {
    bench_script_detection_at_tier(c, "script_detection_by_language", "sentence");
}

/// By language/script at the `paragraph` tier — see
/// [`bench_script_detection_at_tier`] for why this second fixed tier earns
/// its own group.
fn bench_script_detection_by_language_paragraph(c: &mut Criterion) {
    bench_script_detection_at_tier(c, "script_detection_by_language_paragraph", "paragraph");
}

// ---------------------------------------------------------------------------
// 4. Transliteration — throughput only, never output correctness.
// ---------------------------------------------------------------------------

fn bench_transliteration_ja(c: &mut Criterion) {
    let mut g = c.benchmark_group("transliteration_ja");
    configure(&mut g);
    for reps in [1usize, 4, 16, 64, 256, 1024] {
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

/// Input-shape sweep at one fixed ~4 KiB byte budget — the size dimension
/// is [`bench_transliteration_ja`]'s job; this group varies what the bytes
/// *are*, because the two implementations' work per byte is
/// shape-dependent in ways a single prose shape cannot reveal:
///
/// - `kana_prose` — the baseline hiragana+chōonpu prose the size sweep
///   uses (every character rewritten, long-vowel handling hot).
/// - `hiragana_pure` — the hiragana characters extracted from the
///   dataset's own Japanese paragraph (U+3041-U+3096), no chōonpu at all:
///   pure kana-table throughput with the long-vowel machinery never
///   triggered.
/// - `katakana_choonpu` — the baseline prose mechanically shifted to
///   katakana (each hiragana codepoint +0x60, the exact hiragana→katakana
///   block offset): the same rewrite density through each side's katakana
///   tables, chōonpu handling hottest here.
/// - `kanji_mixed` — the dataset's own Japanese `paragraph` tier verbatim:
///   realistic prose where a large share of characters are kanji both
///   implementations only *pass through* (plus fullwidth punctuation,
///   which Verbora passes through and `wana_kana` rewrites — a documented
///   convention divergence, see `tests/transliteration_convention_diff.rs`;
///   the byte counts differ on output, never on the timed input).
///
/// Every shape is derived from data this file already uses (the shared
/// `kana_prose` literal and `dataset.json`'s Japanese entry) by
/// filter/shift/repeat — no new source corpus. Both implementations read
/// the literal same bytes per shape, and `Throughput::Bytes` makes the
/// four shapes comparable as per-byte rates despite small length
/// differences from whole-string repetition.
fn bench_transliteration_ja_by_shape(c: &mut Criterion) {
    let dataset = load_dataset();
    let ja = dataset
        .iter()
        .find(|l| l.iso639_1 == "ja")
        .expect("japanese is in the dataset");
    let paragraph = ja.items.get("paragraph");
    let hiragana_pure: String = paragraph
        .chars()
        .filter(|c| ('\u{3041}'..='\u{3096}').contains(c))
        .collect();
    let katakana_choonpu: String = kana_prose(1)
        .chars()
        .map(|c| {
            if ('\u{3041}'..='\u{3096}').contains(&c) {
                char::from_u32(c as u32 + 0x60).expect("hiragana +0x60 is always valid katakana")
            } else {
                c
            }
        })
        .collect();

    const TARGET_BYTES: usize = 4096;
    let shapes = [
        ("kana_prose", repeat_to_bytes(&kana_prose(1), TARGET_BYTES)),
        (
            "hiragana_pure",
            repeat_to_bytes(&hiragana_pure, TARGET_BYTES),
        ),
        (
            "katakana_choonpu",
            repeat_to_bytes(&katakana_choonpu, TARGET_BYTES),
        ),
        ("kanji_mixed", repeat_to_bytes(paragraph, TARGET_BYTES)),
    ];

    let mut g = c.benchmark_group("transliteration_ja_by_shape");
    configure(&mut g);
    for (label, input) in &shapes {
        g.throughput(Throughput::Bytes(input.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", label), input, |b, s| {
            b.iter(|| black_box(transliterate_ja(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("wana_kana", label), input, |b, s| {
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
    bench_language_detection_by_language_paragraph,
    bench_script_detection_by_length,
    bench_script_detection_by_language,
    bench_script_detection_by_language_paragraph,
    bench_transliteration_ja,
    bench_transliteration_ja_by_shape,
);
criterion_main!(benches);
