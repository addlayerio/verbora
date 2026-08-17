// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here.
#![allow(missing_docs)]

//! Criterion benchmarks for Beider-Morse (see
//! `src/beider_morse/mod.rs`'s own doc comment for what this encoder is and
//! why it's a separate type from [`verbora_phonetics::PhoneticCodes`]).
//!
//! Two costs matter here, and neither is shared with the other four
//! phonetic encoders this crate benchmarks in `phonetics.rs`:
//!
//! * **language auto-detection runs a full regex sweep before the real
//!   encode does any work** — `bench_guess_vs_explicit` compares
//!   [`BeiderMorse::encode`] (guesses first) against
//!   [`BeiderMorse::encode_language`] (caller already knows the language,
//!   skips the guesser) to show what that sweep costs on its own.
//! * **the candidate set is variable-length, not the fixed one-or-two codes
//!   every other encoder here returns** — cost scales with how many
//!   plausible spellings a name's rules actually produce, not with word
//!   length, so `bench_name_types` compares the same surname list's cost
//!   across all three `NameType`s. Measured, this is *not* simply "fewer
//!   languages is faster": `SURNAMES` is a Romance/Slavic/Greek list picked
//!   for Generic, and under Ashkenazi's own narrower 10-language pool most
//!   of it guesses ambiguously rather than to a single language, falling
//!   back to the wider `"any"` rule file and producing a larger candidate
//!   set than Generic's own (mostly single-language) guesses do — Ashkenazi
//!   measured *slower* than Generic here despite having 8 fewer languages,
//!   and Sephardic (5 languages, an even worse guess-match for this list)
//!   measured fastest of the three. The real driver is confirmed to be
//!   guess confidence and resulting candidate-set size, not raw language
//!   count — see `docs/PERFORMANCE_MATRIX.md`'s "Verbora-native extensions"
//!   section for the actual numbers this file produced and why that
//!   matters.
//!
//! Every group here runs [`warm_up`] first: rule tables compile lazily on
//! first use and are cached process-wide by `(NameType, RuleType, language
//! file)` key (`beider_morse::NameTypeData::table`), and that one-time
//! regex-compilation cost is real but is a poor fit for Criterion's
//! repeated-sampling model — there's no way to reset a process-wide cache
//! between samples, so a "cold" group would only ever measure its first
//! sample cold and every later one warm. What every group here reports is
//! steady-state, post-warm-up cost, the number that matters for the
//! realistic use (building an index of many names, not encoding a single
//! name once per process).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use verbora_phonetics::{BeiderMorse, NameType, RuleType};

/// Real surnames Beider-Morse was designed for, spanning the orthographic
/// traditions its rule corpus covers — the same role `phonetics.rs`'s own
/// `SURNAMES` plays for the other encoders, not reused directly since this
/// list is deliberately biased toward names the language guesser has a
/// confident single-language answer for (Slavic, Germanic, Romance, Greek),
/// rather than English-heavy.
const SURNAMES: [&str; 16] = [
    "Renault",
    "Mickiewicz",
    "Thompson",
    "Carvalho",
    "Klausewitz",
    "Giacometti",
    "Nagy",
    "Rodriguez",
    "Schwarz",
    "Kowalski",
    "Dvorak",
    "Popescu",
    "Papadopoulos",
    "Fernandez",
    "Nunes",
    "Toth",
];

/// One multi-word name per position `SURNAMES` doesn't otherwise cover —
/// `concat`'s default (`true`, see [`BeiderMorse::with_concat`]'s own doc
/// comment) fuses these into one lookup; the split (`concat: false`)
/// path is measured separately in `bench_concat`.
const MULTI_WORD_NAMES: [&str; 4] = ["Jean Paul", "von Neumann", "van Gogh", "de la Cruz"];

/// Forces every rule table this benchmark file touches to be compiled and
/// cached before a warm measurement starts, so `bench_name_types`,
/// `bench_guess_vs_explicit`, and `bench_concat` all measure steady-state
/// cost, not first-call compilation (see this file's own top doc comment
/// for why that one-time cost isn't its own benchmark group).
fn warm_up(bm: &BeiderMorse, words: &[&str]) {
    for w in words {
        black_box(bm.encode(w));
    }
}

/// `encode`'s steady-state cost across all three `NameType`s, on the same
/// surname list — the spread between them is the "how many languages does
/// this name type's rule corpus have to consider" effect described in this
/// file's own doc comment.
fn bench_name_types(c: &mut Criterion) {
    let mut g = c.benchmark_group("beider_morse/name_types");
    g.throughput(Throughput::Elements(SURNAMES.len() as u64));

    for name_type in [NameType::Generic, NameType::Ashkenazi, NameType::Sephardic] {
        let label = match name_type {
            NameType::Generic => "generic",
            NameType::Ashkenazi => "ashkenazi",
            NameType::Sephardic => "sephardic",
        };
        let bm = BeiderMorse::new(name_type, RuleType::Approx);
        warm_up(&bm, &SURNAMES);
        g.bench_function(label, |b| {
            b.iter(|| {
                let mut n = 0;
                for w in black_box(&SURNAMES) {
                    n += bm.encode(w).spellings.len();
                }
                n
            });
        });
    }

    g.finish();
}

/// `RuleType::Approx` (the wide net) vs `RuleType::Exact` (the narrower
/// refinement pass) — both run the same Rules pass, so the gap is purely
/// the final pass's own cost and candidate-set size.
fn bench_rule_types(c: &mut Criterion) {
    let mut g = c.benchmark_group("beider_morse/rule_types");
    g.throughput(Throughput::Elements(SURNAMES.len() as u64));

    for rule_type in [RuleType::Approx, RuleType::Exact] {
        let label = match rule_type {
            RuleType::Approx => "approx",
            RuleType::Exact => "exact",
        };
        let bm = BeiderMorse::new(NameType::Generic, rule_type);
        warm_up(&bm, &SURNAMES);
        g.bench_function(label, |b| {
            b.iter(|| {
                let mut n = 0;
                for w in black_box(&SURNAMES) {
                    n += bm.encode(w).spellings.len();
                }
                n
            });
        });
    }

    g.finish();
}

/// The language guesser's own cost: `encode` (guesses, then encodes) vs
/// `encode_language` (caller already knows it, skips straight to the same
/// per-word encode this file's own doc comment describes).
fn bench_guess_vs_explicit(c: &mut Criterion) {
    let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
    warm_up(&bm, &SURNAMES);
    // Also warm the "french" table specifically, since `encode_language`
    // below never touches "any" at all.
    for w in &SURNAMES {
        black_box(bm.encode_language(w, "french"));
    }

    let mut g = c.benchmark_group("beider_morse/guess_vs_explicit");
    g.throughput(Throughput::Elements(SURNAMES.len() as u64));

    g.bench_function("guess", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&SURNAMES) {
                n += bm.encode(w).spellings.len();
            }
            n
        });
    });
    g.bench_function("explicit_language", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&SURNAMES) {
                n += bm
                    .encode_language(w, "french")
                    .map_or(0, |c| c.spellings.len());
            }
            n
        });
    });

    g.finish();
}

/// Multi-word names: `concat: true` (the default — one fused lookup) vs
/// `concat: false` (each word encoded independently and hyphen-joined,
/// roughly the cost of two single-word encodes plus a join).
fn bench_concat(c: &mut Criterion) {
    let fused = BeiderMorse::new(NameType::Generic, RuleType::Approx);
    let split = fused.with_concat(false);
    warm_up(&fused, &MULTI_WORD_NAMES);
    warm_up(&split, &MULTI_WORD_NAMES);

    let mut g = c.benchmark_group("beider_morse/concat");
    g.throughput(Throughput::Elements(MULTI_WORD_NAMES.len() as u64));

    g.bench_function("fused", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&MULTI_WORD_NAMES) {
                n += fused.encode(w).spellings.len();
            }
            n
        });
    });
    g.bench_function("split", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&MULTI_WORD_NAMES) {
                n += split.encode(w).spellings.len();
            }
            n
        });
    });

    g.finish();
}

criterion_group!(
    beider_morse_benches,
    bench_name_types,
    bench_rule_types,
    bench_guess_vs_explicit,
    bench_concat
);
criterion_main!(beider_morse_benches);
