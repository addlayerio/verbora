//! Verbora vs. a real, pinned third-party Rust competitor — normalizers.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.4 for the research dossier. Of the
//! six Rust candidates that research found for `remove_diacritics`, only one
//! is benchmarked here:
//!
//! * **`diacritics` 0.2.2 (YesSeri)** — the matrix's own pick as the closest
//!   semantic match: case-preserving, no forced lowercasing, a direct
//!   per-character table lookup (not NFD decomposition). Verified below (see
//!   `../tests/normalizers_correctness.rs`) to agree with Verbora on ASCII
//!   rejection, precomposed accented Latin, the `œ`/`Œ` ligature fold, the
//!   `ß`→`s`/`ẞ`→`S` fold, the documented `ſ`→`l` bug, the `İ`/`ı` Turkish
//!   dotted/dotless-I fold, and Cyrillic rejection — an unusually close match
//!   for two independently-written tables, which is unsurprising once you
//!   notice both plainly descend from the same long-circulated
//!   "remove-accents" reference table lineage that the reference's own
//!   `removeDiacritics` also descends from.
//! * **`unaccent` 0.1.1** — matrix: `Selected cases`/Partial, but
//!   deliberately not pinned here. It works by NFD-decomposing input and
//!   stripping combining marks, a different mechanism from Verbora's
//!   non-decomposing table lookup (see `remove_diacritics`'s own doc comment
//!   for why that distinction is not cosmetic: precomposed `é` and `e` +
//!   U+0301 fold identically under NFD-then-strip but differently under
//!   Verbora's direct lookup). Its crates.io license field is also flagged
//!   "non-standard" by the matrix — not worth the extra verification burden
//!   when a strictly closer-matching candidate (`diacritics`) already exists.
//! * **`secular` 1.0.1`** — matrix: `No`. Forces lowercasing as part of its
//!   only public API, so it does categorically different/additional work
//!   than Verbora's case-preserving function. Not benchmarked.
//! * **`deunicode`/`any_ascii`/`unidecode`, `unicode-normalization`** —
//!   matrix: `No` for both (full Unicode-to-ASCII transliterators, or a
//!   decomposition primitive that is not itself a diacritic-folder). Not
//!   benchmarked.
//!
//! # A real, narrow divergence — and why the benchmarked domain sidesteps it
//!
//! `diacritics` silently *drops* standalone Unicode combining marks (the
//! U+0300–U+036F "Combining Diacritical Marks" block and two extension
//! blocks): `remove_diacritics("e\u{0301}")` is `"e"` there and `"e\u{0301}"`
//! (unchanged — Verbora never decomposes) on Verbora's side. This is checked
//! explicitly, not just asserted in prose, in
//! `../tests/normalizers_correctness.rs`.
//!
//! Both benchmark groups below use only precomposed Latin text (`é`, not `e`
//! + U+0301) — the same `ascii_prose`/`accented_prose` generators
//!   `crates/verbora-normalizers/benches/normalizers.rs` already uses — so
//!   this divergence never
//!   triggers on the actual benchmarked input. This mirrors `distance.rs`'s own
//!   "kept to ASCII only... sidesteps that distinction entirely" pattern: a
//!   documented, narrowed, genuinely-fair input domain, not silence about the
//!   difference.
//!
//! # `normalize_ja` — zero competitors until this pass
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.4 selects two candidates for
//! `normalize_ja` (Japanese width/kana/symbol normalization, 17 individual
//! conversions plus the composed pipeline), both matrix `Selected cases`, and
//! neither had a bench group here until this pass — confirmed by grepping
//! this file for `normalize_ja`/`hiragana`/`katakana` before writing it, per
//! this round's own instruction to verify the gap rather than assume it:
//!
//! * **`unicode-jp` 0.4.0 (gemmarx)** — covers only 2 of the 17 conversions:
//!   `kana::hira2kata`/`kata2hira` (its published package name is
//!   `unicode-jp`, but its own `[lib] name = "kana"` — see its Cargo.toml —
//!   so Rust code below imports it as `kana::`). Reading
//!   `unicode-jp-0.4.0/src/kana.rs` directly: both are a bare `char` codepoint
//!   shift over a fixed range (`0x3041..=0x3096` / `0x30A1..=0x30F6`), with
//!   none of Verbora's `hiragana_to_katakana`/`katakana_to_hiragana` extra
//!   stages — halfwidth-katakana folding and the small-tsu-before-n-row /
//!   standalone-voiced-mark phonetic fixes (`fix_fullwidth_kana`). Narrowed
//!   to pure hiragana/katakana input with neither halfwidth characters nor a
//!   small tsu before an n-row consonant — real Japanese text (the Iroha
//!   pangram, いろは歌, all 47 base characters with no repeats) rather than a
//!   synthetic string, verified byte-exact on that domain in
//!   `../tests/normalizers_correctness.rs`, which also verifies the excluded
//!   divergence explicitly (`"まっなか"` and halfwidth input both produce
//!   different output on the two sides).
//! * **`kana-converter` 0.1.2** — narrower still, a single-purpose
//!   halfwidth-kana/ASCII-to-fullwidth converter with no reverse direction at
//!   all. Its `to_double_byte(_, KanaOnly)` composes halfwidth katakana with a
//!   following voiced/semi-voiced mark by adding a raw `+1`/`+2` codepoint
//!   offset to the mapped base character — no table of which pairs are real,
//!   unlike Verbora's `katakana_hf` (a literal two-character-key lookup, see
//!   `crates/verbora-normalizers/src/ja/tables.rs`'s `HF_KATAKANA.two`). That
//!   offset happens to land correctly for the ordinary gojuon rows (Unicode
//!   groups them adjacently in the katakana block) but not for `ｦ`/`ﾜ`
//!   (whose real voiced forms `ヺ`/`ヷ` were added later, non-adjacently) or
//!   for an orphan mark with no valid preceding base — all three verified as
//!   real, reproducible divergences in `../tests/normalizers_correctness.rs`,
//!   along with a fourth (`KanaOnly` also folds halfwidth CJK punctuation and
//!   space, a job Verbora keeps in the separate `pure_punctuation_hf`).
//!   Narrowed to halfwidth katakana using only the standard voiced/
//!   semi-voiced pairs, no punctuation, no space, no orphan marks, no
//!   `ｦ`/`ﾜ` + dakuten.
//!
//! # Shape
//!
//! Five groups — `remove_diacritics_ascii` (rejection: nothing to fold) and
//! `remove_diacritics_accented` (work: every fourth character folds), plus
//! `ja_hiragana_to_katakana`, `ja_katakana_to_hiragana` and
//! `ja_katakana_halfwidth_to_fullwidth` — each with
//! `BenchmarkId::new(competitor, repeats)` for `repeats` in [`SIZES`],
//! throughput in bytes. Every call's input and output is wrapped in
//! `black_box`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use verbora_normalizers::ja::converters::{
    hiragana_to_katakana, katakana_hf, katakana_to_hiragana,
};
use verbora_normalizers::remove_diacritics;

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

/// The Iroha pangram (いろは歌): all 47 base hiragana, no repeats,
/// historically used as a Japanese font/encoding test string for exactly the
/// property this benchmark needs — entirely inside U+3041..=U+3096, no
/// iteration marks, no small tsu, no halfwidth. Repeated for larger sizes,
/// identical to `../tests/normalizers_correctness.rs`'s own `IROHA_HIRAGANA`.
fn iroha_hiragana(repeats: usize) -> String {
    "いろはにほへとちりぬるをわかよたれそつねならむうゐのおくやまけふこえてあさきゆめみしゑひもせす"
        .repeat(repeats)
}

/// The same pangram in katakana — entirely inside U+30A1..=U+30F6.
fn iroha_katakana(repeats: usize) -> String {
    "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス"
        .repeat(repeats)
}

/// Halfwidth katakana using only the voiced/semi-voiced pairs both sides
/// agree on (no punctuation, no space, no orphan marks, no `ｦ`/`ﾜ` +
/// dakuten) — identical to `../tests/normalizers_correctness.rs`'s own
/// `HALFWIDTH_KATAKANA`.
fn halfwidth_katakana(repeats: usize) -> String {
    "ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅｶﾀｶﾅｶﾞｷﾞｸﾞｹﾞｺﾞｻﾞｼﾞｽﾞｾﾞｿﾞﾀﾞﾁﾞﾂﾞﾃﾞﾄﾞﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ".repeat(repeats)
}

fn bench_ja_hiragana_to_katakana(c: &mut Criterion) {
    let mut g = c.benchmark_group("ja_hiragana_to_katakana");
    for n in SIZES {
        let s = iroha_hiragana(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(hiragana_to_katakana(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("unicode-jp", n), &s, |bch, s| {
            bch.iter(|| black_box(kana::hira2kata(black_box(s))));
        });
    }
    g.finish();
}

fn bench_ja_katakana_to_hiragana(c: &mut Criterion) {
    let mut g = c.benchmark_group("ja_katakana_to_hiragana");
    for n in SIZES {
        let s = iroha_katakana(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(katakana_to_hiragana(black_box(s))));
        });
        g.bench_with_input(BenchmarkId::new("unicode-jp", n), &s, |bch, s| {
            bch.iter(|| black_box(kana::kata2hira(black_box(s))));
        });
    }
    g.finish();
}

fn bench_ja_katakana_halfwidth_to_fullwidth(c: &mut Criterion) {
    let mut g = c.benchmark_group("ja_katakana_halfwidth_to_fullwidth");
    for n in SIZES {
        let s = halfwidth_katakana(n);
        g.throughput(Throughput::Bytes(s.len() as u64));
        g.bench_with_input(BenchmarkId::new("verbora", n), &s, |bch, s| {
            bch.iter(|| black_box(katakana_hf(black_box(s))));
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
    bench_ja_hiragana_to_katakana,
    bench_ja_katakana_to_hiragana,
    bench_ja_katakana_halfwidth_to_fullwidth,
);
criterion_main!(benches);
