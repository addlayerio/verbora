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
//! # Which matrix rows are benchmarked here, and why
//!
//! `tantivy`/`WhitespaceTokenizer` and Hugging Face `tokenizers`/
//! `WordTokenizer` (the file's original two groups):
//!
//! * **Whitespace tokenization** (§1.1, "Whitespace tokenization
//!   (`RegexpTokenizer` configured with `\s+`)") — `verbora`:
//!   `RegexpTokenizer::new(Pattern::new(Regex::new(r"\s+")))`; `tantivy`:
//!   `tantivy::tokenizer::WhitespaceTokenizer`; `huggingface`:
//!   `tokenizers::pre_tokenizers::whitespace::WhitespaceSplit`, called via
//!   `PreTokenizer::pre_tokenize` on a fresh `PreTokenizedString` — the
//!   isolated pre-tokenizer component, never `Tokenizer::encode`'s full
//!   BPE/WordPiece pipeline (which would do categorically more work — model
//!   lookup, merges, vocabulary — per the matrix's own caveat).
//! * **`WordTokenizer`** (§1.1) — `verbora`: `WordTokenizer` (splits on
//!   `[^A-Za-zА-Яа-я0-9_]+`); `tantivy`: `tantivy::tokenizer::SimpleTokenizer`
//!   (splits on `char::is_alphanumeric()`); `huggingface`:
//!   `tokenizers::pre_tokenizers::whitespace::Whitespace` (pattern
//!   `\w+|[^\w\s]+`), again called in isolation.
//!
//! Three further rows the matrix selected but that were never wired into
//! `Cargo.toml`/benchmarked in the first Fase 6 pass — this file's own
//! audit-round addition:
//!
//! * **`word_tokenization_unicode_segmentation`** (§1.1, `WordTokenizer` row,
//!   `unicode-segmentation` 1.13.3, "Selected cases") — `verbora`:
//!   `WordTokenizer::tokenize`; `unicode-words`:
//!   `str::unicode_words().count()`; `unicode-bounds`:
//!   `str::split_word_bounds()`, filtered to non-whitespace-only spans and
//!   counted — the extra filter is real, necessary work a caller of this
//!   lower-level "every run, word and separator alike" API must do to get a
//!   `WordTokenizer`-equivalent word list, so it is timed, not elided.
//! * **`aggressive_tokenization_en`** (§1.1, `AggressiveTokenizer` English
//!   variant row, `unicode-segmentation`, "Selected cases") — `verbora`:
//!   `AggressiveTokenizer::tokenize`; `unicode-words`: the same
//!   `unicode_words().count()` as above.
//! * **`sentence_tokenization`** (§1.1, `SentenceTokenizer` row,
//!   `unicode-segmentation` "Yes" + `segtok` 0.1.5 "Selected cases") —
//!   `verbora`: `SentenceTokenizer::tokenize`; `unicode-sentences`:
//!   `str::unicode_sentences().count()`; `unicode-bounds`:
//!   `str::split_sentence_bounds().count()`; `segtok`:
//!   `segtok::segmenter::split_single` with `SegmentConfig::default()`.
//!   `segtok`'s own adoption signal is ambiguous — 452K/90d crates.io
//!   downloads against only 2 GitHub stars, most plausibly transitive rather
//!   than direct use (`docs/COMPETITIVE_BENCHMARKS.md` §2.1 records the same
//!   caveat) — flagged here rather than silently omitted, per the matrix's
//!   own instruction not to hide a weak adoption signal just because a crate
//!   cleared the bar on algorithmic grounds.
//!
//! A later coverage-doubling round added **no new matrix rows** — it widened
//! the measurement grid over the rows already justified above: [`SIZES`] and
//! [`SENTENCE_COUNTS`] each grew from four points to eight (the original
//! four preserved unchanged, so existing figures stay comparable — see each
//! constant's own doc comment), and one input-*shape* group was added,
//! `sentence_tokenization_boundary_density` (same `SentenceTokenizer` row,
//! same four implementations, fixed word budget, words-per-sentence swept
//! 3→24 — see [`bench_sentence_tokenization_boundary_density`]'s doc comment
//! for why boundary density reveals a cost axis document *size* cannot).
//! Every new input is derived from the same shared `words.json` corpus by
//! the same `document`/`sentence_prose` builders and stays inside the same
//! narrowed domains documented below.
//!
//! `WordPunctTokenizer` and `TreebankWordTokenizer` are **not** benchmarked
//! here: the matrix records `NO FAIR COMPETITOR FOUND` for both on the Rust
//! side (§1.1) — every candidate either does less work (drops punctuation)
//! or produces a different token count on common input (groups punctuation
//! runs into one token), which is exactly the "comparing different things"
//! the project's fairness rules forbid.
//!
//! # Why the word-boundary input domain is narrowed to punctuation-free ASCII words
//!
//! The five word-boundary implementations above (verbora, tantivy, HF,
//! `unicode-segmentation` ×2) are only `Partial` equivalents even within the
//! rows chosen — they use different character classes (Verbora: ASCII +
//! Cyrillic; tantivy `SimpleTokenizer`/HF `Whitespace`: Unicode
//! alphanumeric/`\w`; `unicode-segmentation`: the full UAX#29 word-boundary
//! algorithm, which treats an internal apostrophe as part of a contraction
//! rather than a separator) and different whitespace definitions (Verbora/HF:
//! `char::is_whitespace`-equivalent; tantivy `WhitespaceTokenizer`: ASCII
//! whitespace only — `is_ascii_whitespace()` in the actual 0.26.1 source,
//! narrower than the matrix's own "`char::is_whitespace()`" note, confirmed
//! by reading `tantivy-0.26.1/src/tokenizer/whitespace_tokenizer.rs`
//! directly). None of those divergences are reachable from the shared
//! `words.json` corpus: every word is lowercase ASCII `[a-z]`, joined by
//! exactly one U+0020 space, with no digits, no underscores, no punctuation
//! (in particular no apostrophes or hyphens — the exact edge cases UAX#29
//! disagrees with Verbora's fixed regex class on, per the matrix's own note
//! on both the `WordTokenizer` and `AggressiveTokenizer`(en) rows) and no
//! non-ASCII whitespace anywhere in the input. On that narrowed domain every
//! one of these implementations draws token boundaries at exactly the same
//! byte offsets — proven by the `#[test]`s in
//! `tests/tokenizers_correctness.rs`, which run *before* any of the timings
//! below are trusted, per the project's `CORRECTNESS BEFORE PERFORMANCE`
//! rule.
//!
//! # Why the sentence-boundary input domain is narrowed to plain declarative sentences
//!
//! Verbora's `SentenceTokenizer` (placeholder-substitution + abbreviation
//! list), `unicode-segmentation`'s UAX#29 sentence-boundary algorithm, and
//! `segtok`'s rule-based orthographic-feature segmenter are three genuinely
//! different algorithm families that disagree on abbreviations, URIs,
//! decimal numbers, quotes, brackets, and (`segtok` specifically) whether the
//! sentence *following* a boundary starts with an upper-case letter or number
//! — none of that is exercised by `sentence_prose`'s output: short
//! all-lowercase-except-first-letter sentences, each ending in exactly one
//! `.`, separated by exactly one space, with no digits, quotes, brackets,
//! abbreviations or embedded newlines anywhere. On that domain, the three
//! implementations' sentence *boundaries* coincide exactly; the only
//! remaining difference is formatting, not disagreement — Verbora and
//! `segtok` both `trim()` each returned sentence (confirmed by reading
//! `segtok-0.1.5/src/segmenter/mod.rs`'s `sentences()`, `res.push(last.trim()
//! .to_string())`, directly), while `unicode-segmentation`'s
//! `unicode_sentences()`/`split_sentence_bounds()` keep the trailing
//! delimiter-adjacent whitespace attached to the preceding span by design (its
//! own doc example: `"Mr. Fox jumped..."` → `["Mr. ", "Fox jumped. ", ...]`).
//! `tests/tokenizers_correctness.rs` proves the four implementations agree
//! exactly once `unicode-segmentation`'s spans are trimmed for comparison —
//! a documented, honest normalization of a whitespace-attachment convention,
//! not a boundary disagreement being papered over.
//!
//! Every call's input and output is wrapped in `black_box`.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use segtok::segmenter::{SegmentConfig, split_single};
use tantivy::tokenizer::{
    SimpleTokenizer, TokenStream as TantivyTokenStream, Tokenizer as TantivyTokenizer,
    WhitespaceTokenizer,
};
use tokenizers::pre_tokenizers::whitespace::{Whitespace, WhitespaceSplit};
use tokenizers::{OffsetReferential, OffsetType, PreTokenizedString, PreTokenizer};
use unicode_segmentation::UnicodeSegmentation;

use verbora_tokenizers::{
    AggressiveTokenizer, Pattern, RegexpTokenizer, SentenceTokenizer, Tokenize, WordTokenizer,
};

/// Document sizes, in words — a superset of
/// `crates/verbora-tokenizers/benches/tokenizers.rs`'s `scaling` grid
/// (`[16, 128, 1024, 8192]`): those four original sizes are preserved
/// unchanged so figures still line up across both files even though they run
/// independently. The coverage-doubling round then halves each original ×8
/// gap with a geometric midpoint (64, 512, 4096) and extends the top by one
/// step (32768 — the 20 000-word shared corpus simply cycles, exactly as
/// [`document`] already does for every size), so scaling kinks between the
/// original points are observable rather than interpolated.
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
        .expect("Whitespace/WhitespaceSplit::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .len()
}

fn bench_whitespace_tokenization(c: &mut Criterion) {
    let words = words();
    let verbora = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"\s+").unwrap()));
    let mut g = c.benchmark_group("whitespace_tokenization");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).map(|v| v.len())));
        });
        g.bench_with_input(BenchmarkId::new("tantivy", doc.len()), &doc, |b, d| {
            let mut tok = WhitespaceTokenizer::default();
            b.iter(|| black_box(tantivy_tokenize(&mut tok, black_box(d))));
        });
        g.bench_with_input(BenchmarkId::new("huggingface", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(hf_pretokenize(&WhitespaceSplit, black_box(d))));
        });
    }
    g.finish();
}

fn bench_word_tokenization(c: &mut Criterion) {
    let words = words();
    let verbora = WordTokenizer::new();
    let mut g = c.benchmark_group("word_tokenization");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).map(|v| v.len())));
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

/// A document of `n_sentences` short declarative sentences: each sentence is
/// `words_per_sentence` lowercase words from the shared corpus, its first
/// letter capitalized, ending in exactly one `.`; sentences are joined by
/// exactly one ASCII space. No digits, quotes, brackets, abbreviations or
/// newlines anywhere — the narrowed sentence-boundary domain this file's own
/// module doc comment documents, empirically confirmed (not merely assumed)
/// to be where Verbora's `SentenceTokenizer`, `unicode-segmentation`'s UAX#29
/// splitter and `segtok`'s rule-based segmenter all agree, by
/// `tests/tokenizers_correctness.rs`.
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

/// §1.1's `WordTokenizer` row, `unicode-segmentation` competitor —
/// `unicode_words()`/`split_word_bounds()`, "Selected cases". Reuses the same
/// `document`/`SIZES` as `bench_word_tokenization` above so the numbers are
/// directly comparable to the `tantivy`/`huggingface` rows in that group.
fn bench_word_tokenization_unicode_segmentation(c: &mut Criterion) {
    let words = words();
    let verbora = WordTokenizer::new();
    let mut g = c.benchmark_group("word_tokenization_unicode_segmentation");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).map(|v| v.len())));
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

/// §1.1's `AggressiveTokenizer` (English variant) row, `unicode-segmentation`
/// competitor — `unicode_words()` only, "Selected cases" (only the English
/// variant is plausibly comparable per the matrix).
fn bench_aggressive_tokenization_en(c: &mut Criterion) {
    let words = words();
    let verbora = AggressiveTokenizer::new();
    let mut g = c.benchmark_group("aggressive_tokenization_en");
    for n in SIZES {
        let doc = document(&words, n);
        g.throughput(Throughput::Bytes(doc.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", doc.len()), &doc, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).len()));
        });
        g.bench_with_input(
            BenchmarkId::new("unicode-words", doc.len()),
            &doc,
            |b, d| {
                b.iter(|| black_box(black_box(d).unicode_words().count()));
            },
        );
    }
    g.finish();
}

/// Sentence counts for `sentence_prose`, scaled similarly to `SIZES` above
/// but in sentences rather than words (each sentence here is a fixed 6
/// words). Like [`SIZES`], the coverage-doubling round kept the original
/// four counts (`[4, 32, 256, 2048]`) unchanged and halved each ×8 gap with
/// a geometric midpoint (16, 128, 1024) plus one larger step (8192).
const SENTENCE_COUNTS: [usize; 8] = [4, 16, 32, 128, 256, 1024, 2048, 8192];

/// Words per sentence `sentence_prose` builds with, fixed across every size
/// in [`SENTENCE_COUNTS`] so only the sentence *count* scales.
const WORDS_PER_SENTENCE: usize = 6;

/// §1.1's `SentenceTokenizer` row — `unicode-segmentation`
/// (`UnicodeSentences`/`USentenceBounds`, "Yes") and `segtok`
/// (`split_single`, "Selected cases").
fn bench_sentence_tokenization(c: &mut Criterion) {
    let words = words();
    let verbora = SentenceTokenizer::new();
    let mut g = c.benchmark_group("sentence_tokenization");
    for n in SENTENCE_COUNTS {
        let text = sentence_prose(&words, n, WORDS_PER_SENTENCE);
        g.throughput(Throughput::Bytes(text.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", text.len()), &text, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).len()));
        });
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
        g.bench_with_input(BenchmarkId::new("segtok", text.len()), &text, |b, d| {
            b.iter(|| black_box(split_single(black_box(d), SegmentConfig::default()).len()));
        });
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

/// The same §1.1 `SentenceTokenizer` row and the same four implementations
/// as [`bench_sentence_tokenization`] — an input-*shape* variant of that
/// group, not a new matrix row. [`bench_sentence_tokenization`] scales the
/// sentence count at a fixed 6 words per sentence, which can never separate
/// per-byte scanning cost from per-boundary cost (Verbora's per-delimiter
/// placeholder substitution, `segtok`'s per-span join rules); this group
/// holds the word budget fixed at [`DENSITY_TOTAL_WORDS`] and sweeps
/// [`DENSITY_WORDS_PER_SENTENCE`] instead, varying the boundary count 8×
/// while the byte count stays within ~3% (`sentence_prose` draws the same
/// [`DENSITY_TOTAL_WORDS`] words from the cycled corpus for every shape;
/// only the `.`-plus-joining-space overhead differs, by `n_sentences - 1`
/// bytes). The `BenchmarkId` parameter is therefore the words-per-sentence
/// shape — the axis that varies — not the near-constant byte length. Every
/// document stays inside the same narrowed declarative-sentence domain the
/// module doc comment documents and `tests/tokenizers_correctness.rs`
/// proves agreement on (including at these exact densities).
fn bench_sentence_tokenization_boundary_density(c: &mut Criterion) {
    let words = words();
    let verbora = SentenceTokenizer::new();
    let mut g = c.benchmark_group("sentence_tokenization_boundary_density");
    for wps in DENSITY_WORDS_PER_SENTENCE {
        let text = sentence_prose(&words, DENSITY_TOTAL_WORDS / wps, wps);
        g.throughput(Throughput::Bytes(text.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", wps), &text, |b, d| {
            b.iter(|| black_box(verbora.tokenize(black_box(d)).len()));
        });
        g.bench_with_input(BenchmarkId::new("unicode-sentences", wps), &text, |b, d| {
            b.iter(|| black_box(black_box(d).unicode_sentences().count()));
        });
        g.bench_with_input(BenchmarkId::new("unicode-bounds", wps), &text, |b, d| {
            b.iter(|| black_box(black_box(d).split_sentence_bounds().count()));
        });
        g.bench_with_input(BenchmarkId::new("segtok", wps), &text, |b, d| {
            b.iter(|| black_box(split_single(black_box(d), SegmentConfig::default()).len()));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_whitespace_tokenization,
    bench_word_tokenization,
    bench_word_tokenization_unicode_segmentation,
    bench_aggressive_tokenization_en,
    bench_sentence_tokenization,
    bench_sentence_tokenization_boundary_density
);
criterion_main!(benches);
