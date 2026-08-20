//! Verbora vs. real, pinned third-party Rust competitors — normalizers.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.4 for the research dossier, and
//! `docs/design/text-shaping-contract.md` §3.2 for the contract this file was
//! re-pointed at.
//!
//! # What the text-shaping migration did to this file
//!
//! `verbora-normalizers` now exposes six functions — `nfd`, `nfc`, `nfkd`,
//! `nfkc`, `remove_diacritics`, `par_remove_diacritics_batch` — and nothing
//! else. Three of this file's five groups measured APIs that no longer exist,
//! and they were **not** re-pointed at a lookalike so the group could keep its
//! shape:
//!
//! * `ja_hiragana_to_katakana` / `ja_katakana_to_hiragana` — **deleted**.
//!   `verbora_normalizers::ja::converters` is gone in full, and hiragana ↔
//!   katakana conversion is not a Unicode normalization: the contract (§3.2,
//!   "Cut: the Japanese normalizers") records it as a *transliteration* that
//!   belongs to `verbora-transliterators`, which today ships only kana →
//!   romaji (`transliterate_ja`). There is no Verbora side left to time, so
//!   timing `unicode-jp`'s `hira2kata`/`kata2hira` alone would measure
//!   nothing about Verbora. The `unicode-jp` dependency went with the groups.
//! * `ja_katakana_halfwidth_to_fullwidth` — **re-pointed**, not deleted. The
//!   capability genuinely survives: NFKC's compatibility decomposition maps
//!   halfwidth katakana to its fullwidth form and decomposes the halfwidth
//!   voiced sound mark `U+FF9E` to the combining `U+3099`, which canonical
//!   composition then recombines, so `nfkc("ｶﾞ") == "ガ"` — the same
//!   user-visible operation `katakana_hf` performed. See
//!   [`bench_nfkc_halfwidth_katakana`] for the comparability limit this
//!   creates and how the input domain is narrowed to respect it.
//!
//! Every published figure for the two deleted groups, and for the re-pointed
//! one, measures code that no longer exists. `results/raw/normalizers-ja_*`
//! and the `normalizers` rows of `results/results.json` are stale for that
//! reason and must not be republished — per `CLAUDE.md`, a number whose code
//! has changed is stale, not approximately right.
//!
//! # `remove_diacritics` — same function, opposite mechanism
//!
//! `remove_diacritics` survives under the same name, but its definition
//! changed from a 820-entry precomposed-scalar table to (contract §3.2):
//!
//! > `s` under Canonical Decomposition (NFD), with every scalar whose
//! > `Canonical_Combining_Class` is non-zero removed, under Canonical
//! > Composition (NFC).
//!
//! That inverts the reasoning the previous revision of this comment used to
//! pick a competitor, and the inversion is recorded rather than quietly
//! dropped:
//!
//! * **`diacritics` 0.2.2 (YesSeri)** — still the pinned competitor, still a
//!   case-preserving direct per-character table lookup. It is now the
//!   *mechanically opposite* implementation of the same user-facing task
//!   (fold the accents off Latin text), which is a perfectly fair thing to
//!   benchmark — but it is no longer the "closest semantic match", and the
//!   two now disagree on far more of Unicode than they did (105 codepoints in
//!   `U+00C0..=U+024F` alone, up from 7). `../tests/normalizers_correctness.rs`
//!   characterises that divergence set by rule instead of by list, and proves
//!   the two still agree byte-for-byte on everything the two groups below
//!   actually feed them.
//! * **`unaccent` 0.1.1** — matrix `Selected cases`/Partial. Previously
//!   rejected *because* it decomposes and Verbora did not; that objection is
//!   now void, and `unaccent` has become the mechanism-matched competitor.
//!   It is still not pinned here: the matrix's other objection (crates.io
//!   flags its license field "non-standard") is unresolved, and adding a
//!   dependency needs the licence review `AGENTS.md` § Licensing requires.
//!   Recorded as an open follow-up, not silently skipped.
//! * **`secular` 1.0.1** — matrix `No`. Forces lowercasing in its only public
//!   API, so it does categorically different work. Unchanged verdict.
//! * **`deunicode`/`any_ascii`/`unidecode`** — matrix `No` (full
//!   Unicode-to-ASCII transliterators). Unchanged verdict.
//! * **`unicode-normalization`** — no longer classifiable as a competitor at
//!   all: it is now the crate `verbora-normalizers` is *implemented on*.
//!   Benchmarking Verbora against it would be a wrapper-overhead measurement,
//!   not a rival-implementation one, and this file does not pretend otherwise
//!   by including it.
//!
//! # The divergence that used to be here is gone; a much larger one replaced it
//!
//! The previous revision documented one narrow divergence — `diacritics`
//! strips standalone combining marks and Verbora did not — and narrowed both
//! groups to precomposed Latin to sidestep it. Verbora now decomposes first,
//! so *that* divergence no longer exists: both sides fold `"e\u{0301}"` to
//! `"e"`. What replaced it is broader and runs the other way: Verbora leaves
//! every letter whose "accent" is part of its identity untouched (`ø`, `æ`,
//! `ß`, `đ`, `ł`, `ſ`, `ı` — no canonical decomposition, so nothing to
//! remove), where `diacritics`' table folds each to a bare ASCII letter.
//!
//! Both groups below are still built from `ascii_prose`/`accented_prose`, the
//! same generators `crates/verbora-normalizers/benches/normalizers.rs` uses,
//! and every character they produce is either plain ASCII or a precomposed
//! Latin letter that canonically decomposes to an ASCII base plus marks —
//! squarely inside the verified-agreeing domain. This is the same
//! documented-narrowing discipline `distance.rs` applies, not silence about
//! the difference.
//!
//! # Shape
//!
//! Three groups — `remove_diacritics_ascii` (rejection: nothing to fold),
//! `remove_diacritics_accented` (work: every fourth character folds), and
//! `nfkc_halfwidth_katakana` — each with `BenchmarkId::new(competitor,
//! repeats)` for `repeats` in [`SIZES`], throughput in bytes. Every call's
//! input and output is wrapped in `black_box`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use verbora_normalizers::{nfkc, remove_diacritics};

/// Repeat counts for the prose generators below (not byte lengths — see the
/// module doc comment). Matches `distance.rs`'s `SIZES` so
/// `scripts/collect-results.py`'s hardcoded size set finds every directory.
const SIZES: [usize; 9] = [4, 8, 16, 32, 64, 128, 256, 512, 1024];

/// English prose with no diacritics: pure rejection. Identical generator to
/// `crates/verbora-normalizers/benches/normalizers.rs`'s `ascii_prose`.
fn ascii_prose(words: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .repeat(words.div_ceil(9))
        .trim_end()
        .to_owned()
}

/// Latin text where roughly every fourth character folds. Identical
/// generator to the sibling benches named above.
fn accented_prose(repeats: usize) -> String {
    "crème brûlée à la française, naïve résumé of Ångström ".repeat(repeats)
}

fn bench_ascii(c: &mut Criterion) {
    let mut g = c.benchmark_group("remove_diacritics_ascii");
    for n in SIZES {
        let s = ascii_prose(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(remove_diacritics(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("diacritics-crate", n), &s, |bch, s| {
            bch.iter(|| black_box(diacritics::remove_diacritics(black_box(s))));
        });
    }
    g.finish();
}

fn bench_accented(c: &mut Criterion) {
    let mut g = c.benchmark_group("remove_diacritics_accented");
    for n in SIZES {
        let s = accented_prose(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(remove_diacritics(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("diacritics-crate", n), &s, |bch, s| {
            bch.iter(|| black_box(diacritics::remove_diacritics(black_box(s))));
        });
    }
    g.finish();
}

/// Halfwidth katakana using only the voiced/semi-voiced pairs both sides
/// agree on (no punctuation, no space, no orphan marks, no `ｦ`/`ﾜ` +
/// dakuten) — identical to `../tests/normalizers_correctness.rs`'s own
/// `HALFWIDTH_KATAKANA`.
fn halfwidth_katakana(repeats: usize) -> String {
    "ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅｶﾀｶﾅｶﾞｷﾞｸﾞｹﾞｺﾞｻﾞｼﾞｽﾞｾﾞｿﾞﾀﾞﾁﾞﾂﾞﾃﾞﾄﾞﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ".repeat(repeats)
}

/// The re-pointed halfwidth-katakana group: `verbora_normalizers::nfkc`
/// against `kana_converter::to_double_byte(_, KanaOnly)`.
///
/// # The comparability limit, stated rather than assumed
///
/// `AGENTS.md` § Cross-Implementation Benchmark Fairness requires both sides
/// to do the same work, and here they do not do *exactly* the same work:
/// `nfkc` is the general UAX #15 Normalization Form KC over arbitrary Unicode
/// — it must consult compatibility decompositions, canonical combining
/// classes and composition exclusions for every scalar it sees — while
/// `kana-converter`'s `KanaOnly` mode is a purpose-built halfwidth-kana table
/// plus `+1`/`+2` codepoint arithmetic that knows about nothing else. On this
/// input they produce byte-identical output (proved per character over the
/// whole of `U+FF66..=U+FF9D`, and over both fixtures, in
/// `../tests/normalizers_correctness.rs`), so the comparison is fair *for
/// this workload*; a reader must not generalise it into "NFKC costs X" or
/// "kana-converter is X faster at normalization", because the two are not
/// substitutable outside the narrowed domain.
///
/// The domain is narrowed exactly as it was for `katakana_hf`, and for the
/// same reasons — no halfwidth punctuation or ideographic space (which
/// `KanaOnly` folds and which NFKC also folds, but to *different* targets:
/// `U+FF9F`-adjacent punctuation aside, `kana-converter` maps halfwidth space
/// to `U+3000` where NFKC maps it to `U+0020`), no orphan voiced marks, and
/// no `ｦﾞ`/`ﾜﾞ` (where NFKC correctly composes `ヺ`/`ヷ` and `kana-converter`'s
/// blind offset arithmetic lands on the unrelated `ン`/`ヰ`). Each of those
/// exclusions is asserted as a real, reproduced divergence in the correctness
/// test rather than described in prose alone.
fn bench_nfkc_halfwidth_katakana(c: &mut Criterion) {
    let mut g = c.benchmark_group("nfkc_halfwidth_katakana");
    for n in SIZES {
        let s = halfwidth_katakana(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(nfkc(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("kana-converter", n), &s, |bch, s| {
            bch.iter(|| {
                black_box(kana_converter::to_double_byte(
                    black_box(s),
                    kana_converter::ConvertMode::KanaOnly,
                ))
            });
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_ascii,
    bench_accented,
    bench_nfkc_halfwidth_katakana,
);
criterion_main!(benches);
