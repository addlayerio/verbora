//! Verbora vs. real, pinned third-party Rust competitors — tokenization.
//!
//! Reads the exact same `benches/data/words.json` that
//! `crates/verbora-tokenizers/benches/tokenizers.rs` (Verbora's own suite)
//! already reads, and rebuilds the identical `document(n)` helper that file
//! already defines (words cycled and joined with a single ASCII space) so
//! every implementation family tokenizes byte-identical text. See
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.1 for why `tantivy`, Hugging Face
//! `tokenizers`, `unicode-segmentation` and `segtok` were selected, and
//! `../../README.md` for why this crate lives outside the main workspace.
//!
//! # What the text-shaping migration did to this file
//!
//! `docs/design/text-shaping-contract.md` reduced `verbora-tokenizers` to
//! three tokenizers over one token shape — [`WordTokenizer`],
//! `SegmentTokenizer`, [`SentenceTokenizer`], every token a borrowed `&str`
//! substring of the input — and deleted everything else. Two of this file's
//! six groups measured APIs that are gone, and neither was re-pointed at a
//! lookalike so the group could keep its shape:
//!
//! * **`whitespace_tokenization` — deleted.** Its Verbora side was
//!   `RegexpTokenizer::new(Pattern::new(Regex::new(r"\s+")))`. `RegexpTokenizer`
//!   and `Pattern` are removed by contract §3.4, which also drops
//!   `verbora-tokenizers`' `regex` dependency outright: Verbora no longer
//!   performs regex or whitespace tokenization at *any* API, and a caller who
//!   wants it is told to use `regex` directly. Timing
//!   `tantivy::WhitespaceTokenizer` and Hugging Face `WhitespaceSplit`
//!   against nothing would measure nothing about Verbora, so the group and
//!   the `regex` dependency went together. **Coverage lost:** those two
//!   competitor rows have no Verbora counterpart in this harness any more.
//!   `docs/COMPETITIVE_BENCHMARKS.md` §1.1's "Whitespace tokenization
//!   (`RegexpTokenizer` configured with `\s+`)" row describes a capability
//!   Verbora does not have.
//! * **`aggressive_tokenization_en` — deleted.** `AggressiveTokenizer` and
//!   its fifteen language variants are removed by contract §3.4; §4.1 records
//!   why (the per-language content was nineteen hand-derived character
//!   classes with documented bugs, not linguistics). Nothing is lost
//!   measurably: that group timed `AggressiveTokenizer` against
//!   `unicode_words()`, and [`WordTokenizer`] — which now *is*
//!   `str::unicode_words()` — carries that comparison in
//!   [`bench_word_tokenization_wrapper_overhead`] below.
//!
//! Every published figure for both deleted groups measures code that no
//! longer exists, as do the surviving groups' Verbora rows: `WordTokenizer`
//! and `SentenceTokenizer` were both reimplemented on UAX #29. Per
//! `CLAUDE.md`, `results/raw/tokenizers-*` and the `tokenizers` rows of
//! `results/results.json` are stale, not approximately right, and must not be
//! republished from anything but a fresh run.
//!
//! # Rival comparisons and wrapper-overhead baselines are now different groups
//!
//! This is the migration's sharpest effect on fairness, and it is structural
//! rather than a caveat. `WordTokenizer::tokens` is *literally*
//! `text.unicode_words()` and `SentenceTokenizer::tokens` is built directly
//! on `text.split_sentence_bound_indices()`
//! (`crates/verbora-tokenizers/src/word.rs`, `src/sentence.rs`) — Verbora
//! delegates to `unicode-segmentation` rather than competing with it. Leaving
//! those rows inside `word_tokenization`/`sentence_tokenization` would report
//! Verbora-against-its-own-dependency as a competitive result, which
//! `AGENTS.md` § Cross-Implementation Benchmark Fairness forbids. They are
//! therefore split into explicit `*_wrapper_overhead` groups, following
//! `benches/language.rs`'s existing `whatlang_wrapper_overhead` precedent:
//! those numbers say what Verbora's wrapper costs over the primitive, and are
//! never to be reported as "Verbora beats/loses to unicode-segmentation".
//!
//! What remains a genuine rival comparison:
//!
//! * **`word_tokenization`** — `verbora`: [`WordTokenizer`]; `tantivy`:
//!   `tantivy::tokenizer::SimpleTokenizer` (splits on
//!   `char::is_alphanumeric()`); `huggingface`:
//!   `tokenizers::pre_tokenizers::whitespace::Whitespace` (pattern
//!   `\w+|[^\w\s]+`), called via `PreTokenizer::pre_tokenize` on a fresh
//!   `PreTokenizedString` — the isolated pre-tokenizer component, never
//!   `Tokenizer::encode`'s full BPE/WordPiece pipeline. Three independent
//!   implementations, and Verbora's now does categorically *more* than the
//!   other two: full UAX #29 WB1–WB999 word segmentation versus a character
//!   class test. On the narrowed domain below they draw identical boundaries
//!   (proved in `tests/tokenizers_correctness.rs`); off it they cannot, and
//!   a reader must not generalise this group into "Verbora tokenizes faster
//!   than tantivy" for arbitrary text.
//! * **`sentence_tokenization`** and its input-shape variant
//!   **`sentence_tokenization_boundary_density`** — `verbora`:
//!   [`SentenceTokenizer`]; `segtok`: `segtok::segmenter::split_single` with
//!   `SegmentConfig::default()`, a rule-based orthographic-feature segmenter
//!   and a genuinely different algorithm family. `segtok`'s own adoption
//!   signal is ambiguous — 452K/90d crates.io downloads against only 2 GitHub
//!   stars, most plausibly transitive rather than direct use
//!   (`docs/COMPETITIVE_BENCHMARKS.md` §2.1 records the same caveat) —
//!   flagged here rather than silently omitted.
//!
//! `WordPunctTokenizer` and `TreebankWordTokenizer` are still not benchmarked
//! here, and now for a second reason on top of the matrix's `NO FAIR
//! COMPETITOR FOUND` (§1.1): contract §3.4 deletes both, and §4.5 records why
//! re-deriving the Penn Treebank tokenizer was deferred rather than
//! half-done.
//!
//! # Both Verbora API shapes are timed, not just one
//!
//! `AGENTS.md` § Benchmark API Variants asks for each API to be measured
//! independently, and the pre-migration file did not: it timed Verbora
//! collecting a `Vec` against competitors that merely counted, quietly
//! charging Verbora an allocation nobody else paid. Every Verbora row below
//! is therefore two rows:
//!
//! * `verbora` — `tokenize_borrowed(text).len()`: one `Vec<&str>`, the
//!   correct default for most programs and the closest match to what
//!   `tantivy`'s `TokenStream` and Hugging Face's `PreTokenizedString`
//!   materialise internally.
//! * `verbora-lazy` — `tokens(text).count()`: the allocation-free primitive,
//!   the floor the collecting path is built on.
//!
//! # Why the word-boundary input domain is narrowed to punctuation-free ASCII words
//!
//! The implementations above are only `Partial` equivalents even within the
//! rows chosen — they use different character classes (Verbora: UAX #29
//! `Word_Break` plus an `Alphabetic`/`Nd`/`Nl`/`No` filter; tantivy
//! `SimpleTokenizer`: `char::is_alphanumeric()`; HF `Whitespace`: `\w`) and
//! disagree in practice on apostrophes, hyphens, decimal points and
//! underscores — the exact cases contract §3.1's own migration table lists.
//! None of those divergences is reachable from the shared `words.json`
//! corpus: every word is lowercase ASCII `[a-z]`, joined by exactly one
//! U+0020 space, with no digits, no underscores, no punctuation and no
//! non-ASCII whitespace anywhere in the input. On that narrowed domain every
//! one of these implementations draws token boundaries at exactly the same
//! byte offsets — proven by the `#[test]`s in
//! `tests/tokenizers_correctness.rs`, which run *before* any of the timings
//! below are trusted, per the project's `CORRECTNESS BEFORE PERFORMANCE`
//! rule.
//!
//! # Why the sentence-boundary input domain is narrowed to plain declarative sentences
//!
//! Verbora's `SentenceTokenizer` (UAX #29 §5 plus an optional abbreviation
//! tailoring, unused here) and `segtok`'s rule-based segmenter disagree on
//! abbreviations, URIs, decimal numbers, quotes, brackets, and (`segtok`
//! specifically) whether the sentence *following* a boundary starts with an
//! upper-case letter or number — none of which `sentence_prose` produces:
//! short all-lowercase-except-first-letter sentences, each ending in exactly
//! one `.`, separated by exactly one space, with no digits, quotes, brackets,
//! abbreviations or embedded newlines anywhere.
//!
//! One formatting difference is left, and the migration made it Verbora's:
//! contract §3.1 removed sentence trimming outright, because a tokenizer that
//! trims does not return substrings. A Verbora sentence therefore carries its
//! own trailing whitespace (so concatenation reproduces the input exactly),
//! `segtok` trims each sentence internally (confirmed by reading
//! `segtok-0.1.5/src/segmenter/mod.rs`'s `sentences()`,
//! `res.push(last.trim().to_string())`, directly), and
//! `unicode-segmentation` keeps the whitespace attached exactly as Verbora
//! now does. `tests/tokenizers_correctness.rs` proves the *boundaries*
//! coincide once that whitespace-attachment convention is normalized — a
//! documented, honest normalization, not a boundary disagreement being
//! papered over. The groups below count sentences, which the convention does
//! not affect.
//!
//! Every call's input and output is wrapped in `black_box`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use segtok::segmenter::{SegmentConfig, split_single};
use tantivy::tokenizer::{
    SimpleTokenizer, TokenStream as TantivyTokenStream, Tokenizer as TantivyTokenizer,
};
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::{OffsetReferential, OffsetType, PreTokenizedString, PreTokenizer};
use unicode_segmentation::UnicodeSegmentation;

use verbora_tokenizers::{BorrowingTokenizer, SentenceTokenizer, WordTokenizer};

/// Document sizes, in words — a superset of
/// `crates/verbora-tokenizers/benches/tokenizers.rs`'s `scaling` grid
/// (`[16, 128, 1024, 8192]`): those four original sizes are preserved
/// unchanged so figures still line up across both files even though they run
/// independently. Each original ×8 gap is halved with a geometric midpoint
/// (64, 512, 4096) and the top extended by one step (32768 — the 20 000-word
/// shared corpus simply cycles, exactly as [`document`] already does for
/// every size), so scaling kinks between the original points are observable
/// rather than interpolated.
const SIZES: [usize; 8] = [16, 64, 128, 512, 1024, 4096, 8192, 32768];

/// Reads the shared word list, failing loudly if it has not been generated.
fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
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
        .expect("word list")
        .iter()
        .map(|w| w.as_str().expect("string").to_owned())
        .collect()
}

/// A plain document of `n` words, one ASCII space apart — the same shape
/// `crates/verbora-tokenizers/benches/tokenizers.rs`'s own `document` helper
/// builds. No punctuation, no digits, no non-ASCII whitespace: the narrowed
/// domain this file's module doc comment documents.
fn document(words: &[String], n: usize) -> String {
    words
        .iter()
        .cycle()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs a tantivy `Tokenizer` to completion and returns the token count.
///
/// Mirrors what `TextAnalyzer::token_stream(..).process(..)` does internally,
/// without the extra `TextAnalyzer`/`Box` indirection neither of the other
/// two implementations pays for either.
fn tantivy_tokenize(tok: &mut impl TantivyTokenizer, text: &str) -> usize {
    let mut stream = tok.token_stream(text);
    let mut n = 0usize;
    while stream.advance() {
        n += 1;
    }
    n
}

/// Runs a Hugging Face `PreTokenizer` in isolation and returns the split
/// count — never through `Tokenizer::encode`'s full BPE/WordPiece pipeline.
fn hf_pretokenize(pt: &impl PreTokenizer, text: &str) -> usize {
    let mut pretokenized = PreTokenizedString::from(text);
    pt.pre_tokenize(&mut pretokenized)
        .expect("Whitespace::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .len()
}

/// The rival word-boundary comparison: three independently written
/// implementations. See the module doc comment for the comparability limit
/// (Verbora runs full UAX #29 segmentation; the other two test a character
/// class) and for why both Verbora API shapes are timed.
fn bench_word_tokenization(c: &mut Criterion) {
    let words = words();
    let verbora = WordTokenizer;
    let mut g = c.benchmark_group("word_tokenization");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize_borrowed(black_box(d)).len()));
        });
        g.bench_with_input(BenchmarkId::new("verbora-lazy", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokens(black_box(d)).count()));
        });
        g.bench_with_input(BenchmarkId::new("tantivy", doc.len()), &doc, |b, d| {
            let mut tok = SimpleTokenizer::default();
            b.iter(|| black_box(tantivy_tokenize(&mut tok, black_box(d))));
        });
        g.bench_with_input(BenchmarkId::new("huggingface", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(hf_pretokenize(&Whitespace, black_box(d))));
        });
    }
    g.finish();
}

/// `unicode-segmentation`'s `split_word_bounds()` yields *every* run — words
/// and separators alike (see the crate's own doc example:
/// `"The quick (\"brown\")  fox".split_word_bounds()` →
/// `["The", " ", "quick", ...]`). Filtering to non-whitespace-only spans is
/// the real work a caller must do to get a `WordTokenizer`-equivalent word
/// list from this lower-level API, so it is included in the timed region,
/// not elided.
fn unicode_split_word_bounds_word_count(text: &str) -> usize {
    text.split_word_bounds()
        .filter(|s| !s.chars().all(char::is_whitespace))
        .count()
}

/// **Not a rival-algorithm comparison.** `WordTokenizer::tokens` is literally
/// `text.unicode_words()` (`crates/verbora-tokenizers/src/word.rs`), so the
/// `unicode-words` row below is Verbora's own implementation measured
/// directly; the difference between it and `verbora` is the cost of
/// collecting a `Vec<&str>`, and the difference between it and `verbora-lazy`
/// should be approximately nothing. The `unicode-bounds` row is the
/// lower-level `split_word_bounds()` API plus the filter a caller must write
/// to get an equivalent word list — the same relationship `SegmentTokenizer`
/// bears to `WordTokenizer` inside Verbora.
///
/// This group exists so that overhead is *visible*, exactly as
/// `benches/language.rs`'s `whatlang_wrapper_overhead` does for
/// `WhatlangDetector`. Its numbers must never be reported as Verbora winning
/// or losing against `unicode-segmentation`.
fn bench_word_tokenization_wrapper_overhead(c: &mut Criterion) {
    let words = words();
    let verbora = WordTokenizer;
    let mut g = c.benchmark_group("word_tokenization_wrapper_overhead");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize_borrowed(black_box(d)).len()));
        });
        g.bench_with_input(BenchmarkId::new("verbora-lazy", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokens(black_box(d)).count()));
        });
        g.bench_with_input(
            BenchmarkId::new("unicode-words", doc.len()),
            &doc,
            |b, d| {
                b.iter(|| black_box(black_box(d).unicode_words().count()));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("unicode-bounds", doc.len()),
            &doc,
            |b, d| {
                b.iter(|| black_box(unicode_split_word_bounds_word_count(black_box(d))));
            },
        );
    }
    g.finish();
}

/// A document of `n_sentences` short declarative sentences: each sentence is
/// `words_per_sentence` lowercase words from the shared corpus, its first
/// letter capitalized, ending in exactly one `.`; sentences are joined by
/// exactly one ASCII space. No digits, quotes, brackets, abbreviations or
/// newlines anywhere — the narrowed sentence-boundary domain this file's own
/// module doc comment documents, empirically confirmed (not merely assumed)
/// to be where Verbora's `SentenceTokenizer` and `segtok`'s rule-based
/// segmenter agree, by `tests/tokenizers_correctness.rs`.
fn sentence_prose(words: &[String], n_sentences: usize, words_per_sentence: usize) -> String {
    let mut out = String::new();
    let mut pool = words.iter().cycle();
    for s in 0..n_sentences {
        if s > 0 {
            out.push(' ');
        }
        for w in 0..words_per_sentence {
            let word = pool.next().expect("words() is non-empty");
            if w == 0 {
                let mut chars = word.chars();
                if let Some(first) = chars.next() {
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                }
            } else {
                out.push(' ');
                out.push_str(word);
            }
        }
        out.push('.');
    }
    out
}

/// Sentence counts for `sentence_prose`, scaled similarly to `SIZES` above
/// but in sentences rather than words (each sentence here is a fixed 6
/// words). Like [`SIZES`], the original four counts (`[4, 32, 256, 2048]`)
/// are unchanged, each ×8 gap halved with a geometric midpoint (16, 128,
/// 1024) plus one larger step (8192).
const SENTENCE_COUNTS: [usize; 8] = [4, 16, 32, 128, 256, 1024, 2048, 8192];

/// Words per sentence `sentence_prose` builds with, fixed across every size
/// in [`SENTENCE_COUNTS`] so only the sentence *count* scales.
const WORDS_PER_SENTENCE: usize = 6;

/// The rival sentence-boundary comparison: Verbora's UAX #29 §5 segmentation
/// against `segtok`'s rule-based orthographic-feature segmenter, a genuinely
/// different algorithm family. `unicode-segmentation` is not here — Verbora
/// is built on it; see [`bench_sentence_tokenization_wrapper_overhead`].
fn bench_sentence_tokenization(c: &mut Criterion) {
    let words = words();
    let verbora = SentenceTokenizer::new();
    let mut g = c.benchmark_group("sentence_tokenization");
    for n in SENTENCE_COUNTS {
        let text = sentence_prose(&words, n, WORDS_PER_SENTENCE);
        g.throughput(Throughput::Bytes(text.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", text.len()), &text, |b, d| {
            b.iter(|| black_box(verbora.tokenize_borrowed(black_box(d)).len()));
        });
        g.bench_with_input(
            BenchmarkId::new("verbora-lazy", text.len()),
            &text,
            |b, d| {
                b.iter(|| black_box(verbora.tokens(black_box(d)).count()));
            },
        );
        g.bench_with_input(BenchmarkId::new("segtok", text.len()), &text, |b, d| {
            b.iter(|| black_box(split_single(black_box(d), SegmentConfig::default()).len()));
        });
    }
    g.finish();
}

/// **Not a rival-algorithm comparison**, for the same reason
/// [`bench_word_tokenization_wrapper_overhead`] is not:
/// `SentenceTokenizer::tokens` iterates `text.split_sentence_bound_indices()`
/// and re-joins segments across suppressed boundaries
/// (`crates/verbora-tokenizers/src/sentence.rs`). With no abbreviations
/// configured — the configuration benchmarked here and everywhere in this
/// file — no boundary is ever suppressed, so Verbora emits exactly the
/// `unicode-bounds` segmentation and this group measures the cost of the
/// suppression check and the `Vec`, nothing more.
///
/// `unicode-sentences` is the same crate's higher-level API, which drops
/// segments containing no alphanumeric content; it is included because it,
/// not `split_sentence_bounds`, is what a caller reaching for
/// "unicode-segmentation sentences" typically writes.
fn bench_sentence_tokenization_wrapper_overhead(c: &mut Criterion) {
    let words = words();
    let verbora = SentenceTokenizer::new();
    let mut g = c.benchmark_group("sentence_tokenization_wrapper_overhead");
    for n in SENTENCE_COUNTS {
        let text = sentence_prose(&words, n, WORDS_PER_SENTENCE);
        g.throughput(Throughput::Bytes(text.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", text.len()), &text, |b, d| {
            b.iter(|| black_box(verbora.tokenize_borrowed(black_box(d)).len()));
        });
        g.bench_with_input(
            BenchmarkId::new("verbora-lazy", text.len()),
            &text,
            |b, d| {
                b.iter(|| black_box(verbora.tokens(black_box(d)).count()));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("unicode-sentences", text.len()),
            &text,
            |b, d| {
                b.iter(|| black_box(black_box(d).unicode_sentences().count()));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("unicode-bounds", text.len()),
            &text,
            |b, d| {
                b.iter(|| black_box(black_box(d).split_sentence_bounds().count()));
            },
        );
    }
    g.finish();
}

/// Words-per-sentence shapes for the boundary-density group below: from
/// boundary-dense three-word sentences to boundary-sparse 24-word ones, an
/// 8× spread in sentence-boundary count over a near-constant byte budget.
const DENSITY_WORDS_PER_SENTENCE: [usize; 4] = [3, 6, 12, 24];

/// Total word budget held fixed across every shape in
/// [`DENSITY_WORDS_PER_SENTENCE`] (each of 3, 6, 12 and 24 divides it), so
/// only boundary *density* varies, never the amount of word text.
const DENSITY_TOTAL_WORDS: usize = 1536;

/// The same rival comparison as [`bench_sentence_tokenization`] — an
/// input-*shape* variant, not a new matrix row. That group scales the
/// sentence count at a fixed 6 words per sentence, which can never separate
/// per-byte scanning cost from per-boundary cost (Verbora's per-boundary
/// suppression check and re-slice, `segtok`'s per-span join rules); this
/// group holds the word budget fixed at [`DENSITY_TOTAL_WORDS`] and sweeps
/// [`DENSITY_WORDS_PER_SENTENCE`] instead, varying the boundary count 8×
/// while the byte count stays within ~3% (`sentence_prose` draws the same
/// [`DENSITY_TOTAL_WORDS`] words from the cycled corpus for every shape;
/// only the `.`-plus-joining-space overhead differs, by `n_sentences - 1`
/// bytes). The `BenchmarkId` parameter is therefore the words-per-sentence
/// shape — the axis that varies — not the near-constant byte length. Every
/// document stays inside the same narrowed declarative-sentence domain the
/// module doc comment documents and `tests/tokenizers_correctness.rs` proves
/// agreement on (including at these exact densities).
fn bench_sentence_tokenization_boundary_density(c: &mut Criterion) {
    let words = words();
    let verbora = SentenceTokenizer::new();
    let mut g = c.benchmark_group("sentence_tokenization_boundary_density");
    for wps in DENSITY_WORDS_PER_SENTENCE {
        let text = sentence_prose(&words, DENSITY_TOTAL_WORDS / wps, wps);
        g.throughput(Throughput::Bytes(text.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", wps), &text, |b, d| {
            b.iter(|| black_box(verbora.tokenize_borrowed(black_box(d)).len()));
        });
        g.bench_with_input(BenchmarkId::new("verbora-lazy", wps), &text, |b, d| {
            b.iter(|| black_box(verbora.tokens(black_box(d)).count()));
        });
        g.bench_with_input(BenchmarkId::new("segtok", wps), &text, |b, d| {
            b.iter(|| black_box(split_single(black_box(d), SegmentConfig::default()).len()));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_word_tokenization,
    bench_word_tokenization_wrapper_overhead,
    bench_sentence_tokenization,
    bench_sentence_tokenization_wrapper_overhead,
    bench_sentence_tokenization_boundary_density
);
criterion_main!(benches);
