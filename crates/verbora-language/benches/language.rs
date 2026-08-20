// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here.
#![allow(missing_docs)]

//! Criterion benchmarks for `verbora-language`, per `Fase 5 Language.md`
//! section 29's benchmarking brief. Run with:
//!
//! ```text
//! cargo bench -p verbora-language                           # groups A, C, E only
//! cargo bench -p verbora-language --all-features             # every group below
//! ```
//!
//! The crate always declares one `[[bench]] name = "language"` target
//! regardless of which features are active (see `Cargo.toml`), so this file
//! must compile and run something useful with zero features, not just under
//! `--all-features`. Every group that touches [`WhatlangDetector`] or
//! [`par_detect_batch`] is behind its own `#[cfg(feature = ...)]`, mirrored
//! in the `criterion_group!`/`criterion_main!` calls at the bottom of this
//! file, exactly the way `verbora-sentiment/benches/sentiment.rs` gates its
//! own `parallel`-only group.
//!
//! # Groups (section 29's lettering)
//!
//! * A. [`bench_script_detection`] — `detect_script` at a few input
//!   lengths. No feature flag.
//! * B. [`bench_language_detection`] — `WhatlangDetector::detect` at short
//!   text / paragraph / long document lengths. Behind `language-detection`.
//! * C. [`bench_strategy_lookup`] — `recommend(Language)` for every
//!   [`Language::ALL`] variant. No feature flag.
//! * D. [`bench_auto_end_to_end`] — `AutoPhoneticStrategy::detect_and_recommend`,
//!   same three lengths as B. Behind `language-detection`. Also carries E's
//!   required explicit comparison — see that function's own doc comment.
//! * E. [`bench_manual_path`] — `recommend(Language::German)` directly, the
//!   explicit-language path. No feature flag.
//! * F. [`bench_par_batch`] — sequential vs. [`par_detect_batch`] at a few
//!   batch sizes, mirroring `verbora-sentiment/benches/sentiment.rs`'s own
//!   `par_batch` group. Behind `language-detection` **and** `parallel`.
//! * G. [`bench_fast_language_detection`] — `HashedLinearDetector::detect`
//!   at four input lengths. Behind `fast-language-detection`.
//! * H. [`bench_fallback_composition`] — the `FallbackDetector` pairing of
//!   the two detectors, against both of its halves. Behind
//!   `fast-language-detection` **and** `language-detection`.
//!
//! # Memory (section 30) and lazy initialization (section 31)
//!
//! These are facts established by reading `whatlang 0.18.0`'s own source
//! (`~/.cargo/registry/.../whatlang-0.18.0/src/`) and by measuring with a
//! throwaway counting-allocator probe run as a *separate*, non-workspace
//! Cargo project — this workspace's `unsafe_code = "deny"` lint (see
//! `Cargo.toml`'s `[workspace.lints.rust]`) forbids writing a `GlobalAlloc`
//! impl inside this crate, the same constraint `verbora-phonetics/benches/
//! phonetic_index.rs`'s own `estimated_bytes` methods document hitting.
//! The probe is not part of this repository; only its findings are:
//!
//! **Detector model size.** `whatlang`'s language-frequency data
//! (`src/trigrams/profiles.rs`, 15,583 lines) is five `pub static LATIN_LANGS
//! /* ... */: LangProfileList`-shaped arrays — plain `&'static [Trigram]`
//! slices of compile-time constant data (confirmed at
//! `profiles.rs:11` `LATIN_LANGS`, `:11300` `CYRILLIC_LANGS`, `:13134`
//! `ARABIC_LANGS`, `:14053` `DEVANAGARI_LANGS`, `:14972` `HEBREW_LANGS`).
//! There is no runtime load step, no file I/O, and no deserialization —
//! this data is baked into the compiled binary's read-only data section the
//! same way any other `const`/`static` is, so "model size" here is a
//! compiled-binary-size question, not a runtime-memory one. `whatlang`'s own
//! per-character alphabet lookup (`src/alphabets/latin.rs`'s
//! `ALPHABET_LANG_MAP`) is the one piece of derived state it builds at
//! runtime — see "Lazy initialization" below for why that is already
//! handled.
//!
//! **Initialization cost.** [`WhatlangDetector::new`] is `pub const fn new()
//! -> Self { Self }` — a zero-sized, stateless unit struct, nothing to
//! construct. The probe confirms this is not just structurally cheap but
//! *actually* free: three repeated runs of `whatlang::Detector::new()`
//! (the type `WhatlangDetector::detect` constructs internally on every
//! call, per `src/whatlang_detector.rs`) each measured **0 heap
//! allocations, 0 bytes**, deterministically.
//!
//! **Allocations per detection.** The same probe measured
//! `whatlang::Detector::detect()` (`Method::Combined`, the crate default) at
//! **exactly 25 heap allocations/reallocations**, identical across all four
//! input lengths this file benchmarks (word/short_text/paragraph/
//! long_document, 6 to 10,460 bytes) — only the *byte total* scales with
//! input length (8,883 to 103,017 bytes measured), not the allocation
//! *count*. This matches what reading the call graph predicts: `detect()`
//! with the default `Method::Combined` runs both `alphabets::raw_detect`
//! and `trigrams::raw_detect` and merges them (`src/combined/mod.rs`) —
//! `LowercaseText::new` allocates one `String`
//! (`src/core/text.rs:12`, `original_text.to_lowercase()`); the alphabet
//! path (`src/alphabets/common.rs`'s `generic_alphabet_calculate_scores`)
//! builds `char_scores`, `lang_scores`, `raw_scores` and
//! `normalized_scores` (four `Vec`s); the trigram path
//! (`src/trigrams/utils.rs`'s `count`/`trigram_occurances_to_positions`)
//! builds one `HashMap` for trigram counts, one intermediate `Vec` to sort
//! it, and one more `HashMap` for the sorted positions, then
//! `src/trigrams/detection.rs`'s `calculate_scores_in_profiles` builds
//! `lang_distances` and collects `raw_scores` (five allocations total); and
//! `combined::raw_detect` builds its own `all_langs` and `scores` `Vec`s
//! (two more). That is twelve *logical* collections; the measured 25 raw
//! `alloc`/`realloc` calls is consistent with several of those (especially
//! the trigram `HashMap`s and the push-built `normalized_scores`/
//! `lang_distances`/`all_langs` vectors, none of which start with
//! `with_capacity` sized to their final length) growing through more than
//! one reallocation as they fill. [`WhatlangDetector::detect`] itself adds
//! at most one more allocation on top of that 25: a single-element `Vec` for
//! `LanguageDetection.candidates` when a candidate is found
//! (`src/whatlang_detector.rs`'s `vec![LanguageCandidate { .. }]`) — 26
//! total for a `Some` result, 25 when it returns `LanguageDetection::none()`
//! (`Vec::new()` does not allocate).
//!
//! **Lazy initialization: not needed, and not added.** Section 31 asks
//! whether `OnceLock`/`LazyLock` should guard model initialization. The
//! honest answer given the two findings above is no: there is no
//! runtime-loaded model on the `verbora-language` side to guard —
//! `WhatlangDetector` carries no state at all, and `whatlang`'s own compiled
//! trigram tables need no construction step whatsoever (see "Detector model
//! size" above). The one piece of `whatlang`-internal state that *is* built
//! at runtime — `ALPHABET_LANG_MAP`, an inverted char-to-language map built
//! once from `LATIN_ALPHABETS` — is already behind a `std::sync::LazyLock`
//! inside `whatlang` itself (`src/alphabets/latin.rs`), process-wide,
//! deferred until the first call that needs the Latin alphabet path. That
//! problem is already solved, by the dependency, one layer down from
//! anything `verbora-language` could add value by re-guarding. Wrapping
//! [`WhatlangDetector`] in another `OnceLock`/`LazyLock` in this crate would
//! add an atomic check to every [`WhatlangDetector::detect`] call to protect
//! a cache that would hold nothing — exactly the synchronization-for-its-
//! own-sake this workspace's anti-overengineering stance (`AGENTS.md`)
//! argues against. Verdict: no lazy-init machinery added here, on the
//! evidence above, not by default caution.
//!
//! **Thread safety (section 32).** [`WhatlangDetector`] is `Copy` and
//! carries no fields (`src/whatlang_detector.rs`), so it is `Send + Sync`
//! automatically — no `unsafe impl` needed, matching this workspace's
//! `unsafe_code = "deny"` lint. [`par_detect_batch`] (group F) is generic
//! over `D: LanguageDetector + Sync` and borrows one shared detector across
//! every Rayon thread rather than cloning per-thread state, which is only
//! sound because that detector has no per-thread-unsafe interior state to
//! begin with.

use std::hint::black_box;

use criterion::measurement::Measurement;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};

#[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
use verbora_language::FallbackDetector;
#[cfg(feature = "fast-language-detection")]
use verbora_language::HashedLinearDetector;
#[cfg(all(
    feature = "fast-language-detection",
    not(feature = "language-detection")
))]
use verbora_language::LanguageDetector;
#[cfg(all(feature = "language-detection", feature = "parallel"))]
use verbora_language::par_detect_batch;
#[cfg(feature = "language-detection")]
use verbora_language::{AutoPhoneticStrategy, Confidence};
use verbora_language::{Language, detect_script, recommend};
#[cfg(feature = "language-detection")]
use verbora_language::{LanguageDetector, WhatlangDetector};

/// A single ambiguous word (the crate-level doc comment's own example) —
/// the smallest input tier. Used only for script detection
/// ([`bench_script_detection`]): script classification is still meaningful
/// at word length, but statistical language detection is not (see this
/// file's own module doc comment and `src/lib.rs`'s "Short inputs are
/// genuinely ambiguous" section), which is why [`bench_language_detection`]
/// and [`bench_auto_end_to_end`] deliberately do not include a word-length
/// tier — benchmarking its *latency* would invite mistaking "this ran fast"
/// for "this was confident," which it is not.
const WORD: &str = "Müller";

/// A short, unambiguous sentence (~68 bytes) — the "short text" tier
/// `Fase 5 Language.md` section 29.B asks for.
const SHORT_TEXT: &str = "This is a fairly long English sentence, written to be unambiguous.";

/// A short paragraph (~520 bytes, four sentences) — the "paragraph" tier.
const PARAGRAPH: &str = "Natural language processing systems often need to determine which \
    language a piece of text is written in before applying any language-specific \
    processing. Detecting the language correctly matters for tokenization, stemming, \
    and phonetic encoding, since each of these steps makes different assumptions about \
    the underlying grammar and alphabet. Statistical detectors compare character and \
    word frequency patterns against known language profiles, producing a confidence \
    score alongside the guess rather than a bare assertion.";

/// The "long document" tier: [`PARAGRAPH`] repeated 20x (~10.4 KB) — long
/// enough to be a genuinely different regime from a single paragraph, small
/// enough that Criterion's own sampling stays fast.
fn long_document() -> String {
    PARAGRAPH.repeat(20)
}

/// Applies a uniform, reduced sampling budget to every group in this file,
/// the same values `verbora-phonetics/benches/phonetic_index.rs` uses for
/// the same reason: real, reproducible orders of magnitude across many
/// benchmark points without an unreasonable total run time.
/// `sample_size(30)` is Criterion's own documented minimum recommended size,
/// not an arbitrary cut.
fn configure<M: Measurement>(g: &mut BenchmarkGroup<'_, M>) {
    g.sample_size(30);
    g.measurement_time(std::time::Duration::from_secs(2));
    g.warm_up_time(std::time::Duration::from_millis(500));
}

// ---------------------------------------------------------------------------
// A. Script detection — no feature flag.
// ---------------------------------------------------------------------------

/// A. [`detect_script`] at a few input lengths.
///
/// No feature flag: `detect_script` is pure Unicode-range classification —
/// no crate dependency, no allocation (see `src/script.rs`'s own module doc
/// comment). This group's numbers back the crate-level doc comment's claim
/// that script detection is "cheap enough to always run first," ahead of
/// statistical language detection.
fn bench_script_detection(c: &mut Criterion) {
    let long_doc = long_document();
    let mut g = c.benchmark_group("script_detection");
    configure(&mut g);

    for (label, text) in [
        ("word", WORD),
        ("short_text", SHORT_TEXT),
        ("paragraph", PARAGRAPH),
        ("long_document", long_doc.as_str()),
    ] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &text, |b, &text| {
            b.iter(|| detect_script(black_box(text)));
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// C. Strategy lookup — no feature flag.
// ---------------------------------------------------------------------------

/// C. [`recommend`] for every [`Language::ALL`] variant.
///
/// `recommend` is a closed `match` over 22 arms (`src/strategy.rs`) — no
/// statistical process, no I/O, and the only allocation on any arm is the
/// short `Vec<PhoneticRecommendation>` built for `alternatives` (one to
/// three elements). This group turns "the manual path must be extremely
/// cheap" (section 29's own words) into a number for every language, not
/// just the one E singles out, so a caller can see that no language costs
/// meaningfully more than another.
fn bench_strategy_lookup(c: &mut Criterion) {
    let mut g = c.benchmark_group("strategy_lookup");
    configure(&mut g);

    for lang in Language::ALL {
        g.bench_with_input(
            BenchmarkId::from_parameter(lang.iso639_1()),
            &lang,
            |b, &lang| b.iter(|| recommend(black_box(lang))),
        );
    }

    // The whole table in one pass: 22 lookups back to back, the cost a
    // caller would pay for (hypothetically) building a per-language cache
    // eagerly at startup — which nothing in this crate does, precisely
    // because a single lookup is already this cheap.
    g.bench_function("all_languages", |b| {
        b.iter(|| {
            for lang in Language::ALL {
                black_box(recommend(black_box(lang)));
            }
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// E. Manual path — no feature flag.
// ---------------------------------------------------------------------------

/// E. `recommend(Language::German)` directly — the explicit-language path a
/// caller who already knows the language takes (see the crate-level doc
/// comment: "A caller who already knows the language calls `recommend`
/// directly and pays nothing for detection").
///
/// Isolated in its own group, rather than folded into
/// [`bench_strategy_lookup`]'s per-language loop, so this exact benchmark
/// can be repeated verbatim inside [`bench_auto_end_to_end`] (behind
/// `language-detection`) for the explicit manual-vs-auto comparison section
/// 29.E requires — see that function's own doc comment for why it is
/// repeated rather than cross-referenced.
///
/// German specifically because it is section 29.E's own example, and
/// because `recommend`'s `German` arm sits in the crate's largest match-arm
/// group (`German | Dutch | Swedish | Norwegian | Finnish`) rather than the
/// single-language arms — this is a representative number, not a
/// best-case one.
fn bench_manual_path(c: &mut Criterion) {
    let mut g = c.benchmark_group("manual_path");
    configure(&mut g);

    g.bench_function("recommend_german", |b| {
        b.iter(|| recommend(black_box(Language::German)));
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// B. Language detection — behind `language-detection`.
// ---------------------------------------------------------------------------

/// B. [`WhatlangDetector::detect`] at short text / paragraph / long document
/// lengths — the three tiers section 29.B asks for. Word-length is
/// deliberately excluded; see [`WORD`]'s own doc comment for why measuring
/// single-word detection *latency* would invite mistaking speed for
/// confidence.
///
/// Only compiled with `--features language-detection`.
#[cfg(feature = "language-detection")]
fn bench_language_detection(c: &mut Criterion) {
    let long_doc = long_document();
    let detector = WhatlangDetector::new();

    let mut g = c.benchmark_group("language_detection");
    configure(&mut g);

    for (label, text) in [
        ("short_text", SHORT_TEXT),
        ("paragraph", PARAGRAPH),
        ("long_document", long_doc.as_str()),
    ] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &text, |b, &text| {
            b.iter(|| detector.detect(black_box(text)));
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// D. End-to-end auto selection (+ E's explicit comparison) — behind
// `language-detection`.
// ---------------------------------------------------------------------------

/// D. [`AutoPhoneticStrategy::detect_and_recommend`] at the same three
/// tiers as [`bench_language_detection`]: text -> detect -> recommend, in
/// one call.
///
/// Also carries E's required explicit comparison: `manual_german` is
/// [`bench_manual_path`]'s exact benchmark, repeated in *this* group so
/// Criterion's own report places the manual and auto numbers side by side
/// in one table, rather than requiring a reader to cross-reference a
/// separate, feature-gated-differently section — per section 29.E, "your
/// benchmark must make this comparison explicit against the auto path's
/// cost."
///
/// The confidence threshold (0.5) is a benchmarking choice, not a
/// recommended default — [`AutoPhoneticStrategy::new`]'s own doc comment
/// explains why this crate does not ship one, and the threshold does not
/// change `detect_and_recommend`'s cost either way (the comparison against
/// it happens after detection, not before).
///
/// Only compiled with `--features language-detection`.
#[cfg(feature = "language-detection")]
fn bench_auto_end_to_end(c: &mut Criterion) {
    let long_doc = long_document();
    let auto = AutoPhoneticStrategy::new(
        WhatlangDetector::new(),
        Confidence::new(0.5).expect("0.5 is in range"),
    );

    let mut g = c.benchmark_group("auto_end_to_end");
    configure(&mut g);

    for (label, text) in [
        ("auto_short_text", SHORT_TEXT),
        ("auto_paragraph", PARAGRAPH),
        ("auto_long_document", long_doc.as_str()),
    ] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &text, |b, &text| {
            b.iter(|| auto.detect_and_recommend(black_box(text)));
        });
    }

    g.bench_function("manual_german", |b| {
        b.iter(|| recommend(black_box(Language::German)));
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// G. Fast hashed-linear detection — behind `fast-language-detection`.
// ---------------------------------------------------------------------------

/// G. [`HashedLinearDetector::detect`] at four input lengths.
///
/// Unlike group B this group *does* include the word tier: the fast
/// detector's short-input latency is the design target its decomposition
/// analysis measured (whatlang's per-profile scan gives it a ~20 µs floor
/// on a single word; the hashed-linear path has no such floor), and its
/// word-length *behavior* is abstention or a low, `best_above`-rejectable
/// confidence rather than a confident guess — so measuring the latency
/// does not invite the speed-for-confidence confusion B's exclusion
/// guards against. No accuracy claim is attached to any tier here; see
/// `src/hashed_linear.rs`'s own doc comment for what this detector does
/// and does not claim.
///
/// Only compiled with `--features fast-language-detection`.
#[cfg(feature = "fast-language-detection")]
fn bench_fast_language_detection(c: &mut Criterion) {
    let long_doc = long_document();
    let detector = HashedLinearDetector::new();

    let mut g = c.benchmark_group("fast_language_detection");
    configure(&mut g);

    for (label, text) in [
        ("word", WORD),
        ("short_text", SHORT_TEXT),
        ("paragraph", PARAGRAPH),
        ("long_document", long_doc.as_str()),
    ] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &text, |b, &text| {
            b.iter(|| detector.detect(black_box(text)));
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// H. Fallback composition — behind `fast-language-detection` AND
// `language-detection`.
// ---------------------------------------------------------------------------

/// H. `FallbackDetector<HashedLinearDetector, WhatlangDetector>` against
/// both of its own halves, at the same four tiers as G.
///
/// This group exists to make one cost model visible rather than asserted:
/// the composition pays the fast detector's cost when the fast detector
/// commits, and *both* detectors' cost when it abstains. So its curve
/// should sit on top of `fast` from `short_text` upward and on top of
/// `whatlang` at the `word` tier — which is the whole trade, since the
/// composition is what recovers the abstentions the fast detector's
/// accuracy loses on short input (see `src/fallback.rs`'s measured table).
/// Running all three in one group means those three numbers come from one
/// machine state, not three.
///
/// Only compiled with `--features fast-language-detection,language-detection`.
#[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
fn bench_fallback_composition(c: &mut Criterion) {
    let long_doc = long_document();
    let fast = HashedLinearDetector::new();
    let whatlang = WhatlangDetector::new();
    let composed = FallbackDetector::new(HashedLinearDetector::new(), WhatlangDetector::new());

    let mut g = c.benchmark_group("fallback_composition");
    configure(&mut g);

    for (label, text) in [
        ("word", WORD),
        ("short_text", SHORT_TEXT),
        ("paragraph", PARAGRAPH),
        ("long_document", long_doc.as_str()),
    ] {
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::new("fast", label), &text, |b, &text| {
            b.iter(|| fast.detect(black_box(text)));
        });
        g.bench_with_input(BenchmarkId::new("whatlang", label), &text, |b, &text| {
            b.iter(|| whatlang.detect(black_box(text)));
        });
        g.bench_with_input(BenchmarkId::new("fallback", label), &text, |b, &text| {
            b.iter(|| composed.detect(black_box(text)));
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// F. Sequential vs. parallel batch — behind `language-detection` AND
// `parallel`.
// ---------------------------------------------------------------------------

/// F. Sequential vs. [`par_detect_batch`] at a few batch sizes, mirroring
/// `verbora-sentiment/benches/sentiment.rs`'s own `par_batch` group (see
/// that file for the pattern this one follows) and answering
/// `src/parallel.rs`'s own module doc comment, which points here for "the
/// real crossover this project measured."
///
/// Batch members are [`SHORT_TEXT`]-length — a realistic single document,
/// not a single word — matching `src/parallel.rs`'s own framing of what
/// `par_detect_batch` is for: "a caller holding a large, independent
/// corpus... where the per-text cost repeats thousands of times," not
/// per-word detection (which section 33 of the spec explicitly says Rayon
/// must never back).
///
/// Only compiled with `--features language-detection,parallel`.
#[cfg(all(feature = "language-detection", feature = "parallel"))]
fn bench_par_batch(c: &mut Criterion) {
    let detector = WhatlangDetector::new();

    let mut g = c.benchmark_group("par_batch");
    configure(&mut g);

    for batch_size in [16_usize, 64, 256, 1_024, 4_096] {
        let texts: Vec<&str> = std::iter::repeat_n(SHORT_TEXT, batch_size).collect();
        g.throughput(Throughput::Elements(batch_size as u64));

        g.bench_with_input(
            BenchmarkId::new("sequential", batch_size),
            &texts,
            |b, texts| {
                b.iter(|| texts.iter().map(|t| detector.detect(t)).collect::<Vec<_>>());
            },
        );
        g.bench_with_input(
            BenchmarkId::new("parallel", batch_size),
            &texts,
            |b, texts| b.iter(|| par_detect_batch(&detector, texts)),
        );
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// Harness wiring — mirrors verbora-sentiment/benches/sentiment.rs's own
// feature-gated criterion_group! pattern, extended with a third
// (language-detection-only) combination since this crate has two
// independent optional features rather than one.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "language-detection"))]
criterion_group!(
    benches,
    bench_script_detection,
    bench_strategy_lookup,
    bench_manual_path
);

#[cfg(all(feature = "language-detection", not(feature = "parallel")))]
criterion_group!(
    benches,
    bench_script_detection,
    bench_strategy_lookup,
    bench_manual_path,
    bench_language_detection,
    bench_auto_end_to_end
);

#[cfg(all(feature = "language-detection", feature = "parallel"))]
criterion_group!(
    benches,
    bench_script_detection,
    bench_strategy_lookup,
    bench_manual_path,
    bench_language_detection,
    bench_auto_end_to_end,
    bench_par_batch
);

// G is independent of the language-detection × parallel matrix above, so
// it gets its own group rather than doubling that matrix to eight cfg
// combinations; when the feature is off, a no-op function stands in
// (criterion_main! only needs a callable per group).
#[cfg(all(
    feature = "fast-language-detection",
    not(feature = "language-detection")
))]
criterion_group!(fast_benches, bench_fast_language_detection);
// H needs both detectors, so it joins G's group only when both features
// are on — the composition literally cannot be constructed otherwise.
#[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
criterion_group!(
    fast_benches,
    bench_fast_language_detection,
    bench_fallback_composition
);
#[cfg(not(feature = "fast-language-detection"))]
fn fast_benches() {}

criterion_main!(benches, fast_benches);
