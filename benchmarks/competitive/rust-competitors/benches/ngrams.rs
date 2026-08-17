// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style is
// kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. a real, pinned third-party Rust competitor — n-grams.
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.2 originally recorded `NO FAIR
//! COMPETITOR FOUND (Rust)` for this module: `ngrammatic`'s headline feature
//! is fuzzy corpus search (`Corpus`/`CorpusBuilder`/`search`), a genuinely
//! different problem from plain n-gram generation, and every other Rust
//! n-gram crate found was either abandoned or solved a different problem
//! again. That verdict for the *fuzzy-search* feature stands — this file
//! benchmarks neither `Corpus` nor `search`, and makes no claim about them.
//!
//! What changed: `ngrammatic::Ngram`/`NgramBuilder` — the character n-gram
//! generator `Corpus` is itself built on — turns out to be a genuinely fair,
//! comparable primitive. Read directly from `ngrammatic` 0.7.0's own source
//! (`src/ngram.rs`), `NgramBuilder::new(text).arity(n).finish()` pads `text`
//! with `n - 1` characters on each side (space, by default — `Pad::Auto`),
//! slides a window of size `n` across the padded sequence, and returns a
//! `HashMap<SmolStr, usize>` of gram → count. That is exactly what a caller
//! building character n-grams with frequency counts over Verbora would do:
//! `verbora_ngrams::ngrams(&chars, n, Some(' '), Some(' '))` folded into a
//! `HashMap<String, usize>` — see [`verbora_char_grams`] below.
//!
//! # What is, and is not, claimed
//!
//! **Verbora has no dedicated character-n-gram, string-in convenience
//! function.** `verbora_ngrams`'s string-input helpers (`ngrams_str` and
//! friends, in `crates/verbora-ngrams/src/text.rs`) tokenize into *words*
//! first — a different granularity than `ngrammatic`'s character-level
//! grams, and not what this file compares. The character-level path used
//! here — `text.chars().collect::<Vec<char>>()` then the generic
//! `ngrams()` engine — is the same generic primitive every other Verbora
//! n-gram entry point is built on (`crates/verbora-ngrams/src/engine.rs`),
//! just called directly rather than through a string-specific wrapper. That
//! architectural difference (a dedicated `NgramBuilder` vs. a generic engine
//! called with `T = char`) is disclosed, not hidden — and it is timed
//! identically on both sides: the `char` collection and the map-folding are
//! inside the benchmarked closure for Verbora, exactly as `NgramBuilder`'s
//! own `String` allocation, padding and windowing are for `ngrammatic`.
//!
//! **Output equivalence is verified, not assumed.** Both sides pad with
//! `arity - 1` copies of the same character and slide the same window —
//! `tests/ngrams_correctness.rs` checks the two produce byte-identical
//! `(gram, count)` sets over every word in `benches/data/words.json`, for
//! both arities benchmarked here. The one known, disclosed divergence
//! (inputs shorter than `arity - 1`, where Verbora's padding follows the
//! reference's own negative-slice re-anchoring quirk and `ngrammatic` does
//! not) is also pinned by that test, and does not affect this file: the
//! shortest word in the benchmarked list is 3 characters, longer than either
//! arity tested minus one.
//!
//! Two groups are benchmarked, each building a gram → count map for every
//! word in the shared word list:
//!
//! - `bigrams` (arity 2)
//! - `trigrams` (arity 3)

use std::collections::HashMap;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ngrammatic::{NgramBuilder, Pad};
use verbora_ngrams::ngrams;

/// The shared word list, read from `benches/data/words.json` — the same file
/// `benches/trie.rs` and every other file in this crate reads.
fn load_words() -> Vec<String> {
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
        .map(|w| w.as_str().expect("word").to_owned())
        .collect()
}

/// Character n-grams of `word`, padded with `arity - 1` spaces on each side
/// and folded into a `(gram, count)` map — the fair comparison point against
/// `ngrammatic::Ngram::grams`, per this file's own module doc comment.
/// `tests/ngrams_correctness.rs` verifies the two agree exactly.
fn verbora_char_grams(word: &str, arity: usize) -> HashMap<String, usize> {
    let chars: Vec<char> = word.chars().collect();
    let mut map: HashMap<String, usize> = HashMap::new();
    for gram in ngrams(&chars, arity, Some(' '), Some(' ')) {
        *map.entry(gram.iter().collect()).or_insert(0) += 1;
    }
    map
}

fn bench_arity(c: &mut Criterion, group_name: &str, arity: usize) {
    let words = load_words();
    let mut g = c.benchmark_group(group_name);
    g.throughput(Throughput::Elements(words.len() as u64));

    g.bench_function("verbora", |b| {
        b.iter(|| {
            words
                .iter()
                .map(|w| verbora_char_grams(black_box(w), arity).len())
                .sum::<usize>()
        });
    });
    g.bench_function("ngrammatic", |b| {
        b.iter(|| {
            words
                .iter()
                .map(|w| {
                    NgramBuilder::new(black_box(w))
                        .arity(arity)
                        .pad_full(Pad::Auto)
                        .finish()
                        .grams
                        .len()
                })
                .sum::<usize>()
        });
    });

    g.finish();
}

fn bench_bigrams(c: &mut Criterion) {
    bench_arity(c, "bigrams", 2);
}

fn bench_trigrams(c: &mut Criterion) {
    bench_arity(c, "trigrams", 3);
}

criterion_group!(benches, bench_bigrams, bench_trigrams);
criterion_main!(benches);
