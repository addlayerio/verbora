// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style is
// kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. real, pinned third-party Rust competitors — the trie.
//!
//! Reads the exact same `benches/data/words.json` that
//! `crates/verbora-trie/benches/trie.rs` (Verbora's own in-workspace bench)
//! already reads, so all implementations run on byte-identical input. See
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.18 for why `trie-rs`, `qp-trie` and
//! `fast_radix_trie` were selected, and `../../README.md` for why this crate
//! lives outside the main workspace. `fst` was added later, per
//! `benchmark.md`'s `FST — SPECIALIZED FROZEN COMPETITOR` directive, as a
//! specialized *frozen*-representation competitor for `Trie::contains` and
//! prefix enumeration — see the `fst` section below for its own
//! EXACT/NARROWED_EXACT/PARTIAL/TECHNIQUE/UNFAIR classification per
//! operation.
//!
//! # What is, and is not, benchmarked here
//!
//! The matrix marks the exact-semantics row `NO FAIR COMPETITOR FOUND (Rust)`
//! — no Rust crate replicates [`verbora_trie::Trie`]'s UTF-16-code-unit
//! keying, its the reference `for…in` child-enumeration order, or its
//! deliberately-preserved `keys_with_prefix` case-folding bug (all three
//! documented in `verbora_trie`'s own crate docs). Only the row the matrix
//! marks `Partial`/"Selected cases" is benchmarked: **generic
//! insert/contains/prefix-enumeration throughput on ASCII word lists.**
//!
//! **This file never asserts, measures, or implies that the five
//! implementations return results in the same order, or that their outputs
//! are interchangeable.** `trie-rs` is UTF-8-byte-keyed and returns keys in
//! byte-lexicographic order; `qp-trie` is a nybble-branching radix map with
//! its own internal order; `fast_radix_trie` is also UTF-8-byte-keyed and
//! also iterates in sorted byte-lexicographic order (its `iter`/`iter_prefix`
//! walk a byte-radix tree depth-first over ordered children — see its own
//! doc comments), so it happens to *agree* with `trie-rs` on order, but that
//! agreement is never relied on here either; `fst` is *also*
//! byte-lexicographic by construction (that ordering is the FST's own
//! invariant, not incidental), but its `Streamer`-based iteration is a
//! genuinely different API shape (keys decoded lazily per step, not a
//! standard `Iterator`) from every other competitor here; Verbora's order is
//! The reference `for…in` order. A `tests/trie_correctness.rs` integration test
//! in this crate checks that all five agree on the *set* of words each
//! operation reports (order-blind comparison, via `BTreeSet`) — that is the
//! only correctness claim made anywhere in this workbench, and it is checked
//! once, outside the timed code, per the project's `CORRECTNESS BEFORE
//! PERFORMANCE` rule.
//!
//! # `fast_radix_trie`'s `unsafe`, and what it buys
//!
//! `fast_radix_trie` uses `unsafe` internally: nodes are dynamically sized
//! (a node's label bytes and child pointers are packed inline into one
//! allocation, addressed through raw pointers) so that a node with *N*
//! children costs one allocation sized exactly for *N*, not a fixed
//! worst-case shape — its own README documents this trade explicitly and
//! states its test suite runs under `miri`. That is noted here, not to
//! disqualify it: this workbench is a separate, isolated Cargo workspace not
//! bound by the main workspace's `unsafe_code = "deny"` lint (see
//! `../../Cargo.toml`'s own comment on why). But it is architecturally
//! relevant to any timing difference below, because Verbora's own trie
//! (`crates/verbora-trie`) has zero `unsafe` anywhere — the comparison is
//! genuinely "arena of uniformly-sized, safe nodes" vs. "raw-pointer tree of
//! dynamically-sized, path-compressed nodes," not two implementations of the
//! same shape.
//!
//! # `fst`: a frozen finite-state transducer, not a trie
//!
//! `fst::Set` is architecturally nothing like the other four competitors in
//! this file: it is a finite-state *transducer* built once from
//! lexicographically **sorted** input via a streaming `SetBuilder`, then
//! queried through a `Streamer`-based API (`Streamer::next`, not
//! `Iterator::next` — keys are decoded lazily during the walk, not stored),
//! and it can never be mutated again once built. There is no incremental
//! `insert` into a live `Set` at all — in that specific respect it is closer
//! to `trie-rs`'s "compile once" LOUDS structure than to Verbora's, `qp-trie`'s,
//! or `fast_radix_trie`'s incrementally-mutable trees.
//!
//! Per-operation classification (EXACT/NARROWED_EXACT/PARTIAL/TECHNIQUE/
//! UNFAIR, `benchmark.md`'s own five-way bar):
//!
//! - **`build`/`contains_hit`/`contains_miss`** — **NARROWED_EXACT**.
//!   `Set::contains` and `Trie::contains` answer the identical yes/no
//!   membership question over the identical set of words, verified in
//!   `tests/trie_correctness.rs`. "Narrowed" because of one real, disclosed
//!   asymmetry: `Set::from_iter`/`SetBuilder::insert` require strictly
//!   increasing input (`Err(DuplicateKey)`/`Err(OutOfOrder)` otherwise; see
//!   `raw::Builder::check_last_key` in `fst`'s own source), where
//!   `Trie::add_strings` accepts any order, including duplicates, for free.
//!   `build_fst` performs the sort *and* dedup **inside** the timed `build`
//!   closure specifically so this asymmetry shows up as real measured
//!   nanoseconds in the `build` group rather than being hidden by
//!   pre-sorting outside the benchmark — the same rule this file already
//!   applies to `trie-rs`'s push-then-compile step.
//! - **`predictive_search`** — **NARROWED_EXACT**, same domain caveat as
//!   above. `fst` has no method literally named "prefix search"; the
//!   equivalent is composing the `Str` automaton with `.starts_with()` and
//!   running it through `Set::search`, or (for the empty-prefix case)
//!   `Set::stream()` directly. Verified set-equal to
//!   `Trie::keys_with_prefix` in `tests/trie_correctness.rs`.
//! - **`common_prefix_search`** — **TECHNIQUE** (not benchmarked). `fst` has
//!   no operation answering "which stored keys are prefixes of this query" —
//!   see the comment directly above `bench_common_prefix_search` for the
//!   full reasoning (same shape of exclusion as `qp-trie`'s).
//! - **Fuzzy/Levenshtein-automaton lookup** (the operation that would
//!   compare against `verbora_spellcheck::FuzzyIndex::neighbors`, per
//!   `benchmark.md`'s own directive to evaluate `fst` for this) — **not
//!   benchmarked in this file** (it is not a trie operation; there is
//!   nothing here to compare it against). This workspace's `Cargo.toml`
//!   now enables `fst`'s own `levenshtein` Cargo feature (`default = []` in
//!   `fst` 0.4.7's own `Cargo.toml` — this crate ships it off by default;
//!   `harper-core` 2.8.0's own `FstDictionary` composes plain `fst` with the
//!   *separate* `levenshtein_automata` crate instead, not this feature),
//!   and the comparison itself lives in its own dedicated file,
//!   `benches/fst_fuzzy.rs` (correctness pinned in
//!   `tests/fst_fuzzy_correctness.rs` first, per this workbench's
//!   `CORRECTNESS BEFORE PERFORMANCE` rule) — see that file's own doc
//!   comment for the NARROWED_EXACT classification and the real double
//!   crossover found (`docs/PERFORMANCE_GAPS.md` entry 37).
//!
//! `fst`'s mmap-backed mode (`Set::new(mmap)` over a file written by
//! `SetBuilder`) is a distinct architectural technique — relevant to this
//! workspace's "Archived Data and Memory Mapping" evaluation area — and is
//! deliberately not exercised here: every `fst::Set` in this file is built
//! and queried entirely in memory (`Set<Vec<u8>>`), never via
//! `unsafe { Mmap::map(...) }`, since this is a benchmarking question about
//! adopting `unsafe` deliberately, not something to slip in as a side effect
//! of a Criterion group.
//!
//! Five groups are benchmarked, one per matrix-eligible capability:
//!
//! - `build` — insert-all throughput, on both a "random" 20 000-word set and
//!   a "prefix_heavy" 32 000-entry set (short shared stems, many suffixes —
//!   see `prefix_heavy` below), matching the two datasets Verbora's own
//!   in-workspace `build` benchmark already uses. `fast_radix_trie`, like
//!   Verbora and `qp-trie`, is incrementally mutable — `build` is a plain
//!   per-word `GenericRadixMap::insert` loop, not a compile step (see the
//!   `trie-rs` section below for the one implementation where that is not
//!   true).
//! - `contains_hit` / `contains_miss` — exact-match lookup throughput, hits
//!   and misses measured separately since a miss exits the walk early and is
//!   the common case in real lookups (mirrors Verbora's own in-workspace
//!   bench).
//! - `common_prefix_search` — throughput of "which stored words are
//!   prefixes of this query" (`Trie::find_matches_on_path` /
//!   `trie_rs::Trie::common_prefix_search` /
//!   `fast_radix_trie::GenericRadixMap::common_prefixes`). **`qp-trie` is
//!   excluded from this group**: it has no native operation that answers
//!   this question — only `contains_key`/`get` (single key) and
//!   prefix-*enumeration* (`iter_prefix`, the *opposite* direction: "words
//!   that start with the query", not "stored words that are prefixes of the
//!   query"). Reproducing the operation on top of `qp-trie` by probing
//!   `contains_key` once per candidate prefix length would benchmark bespoke
//!   glue code, not a real `qp-trie` capability, so it is left out rather
//!   than faked. `fast_radix_trie` **is** included in this group — unlike
//!   `qp-trie` it ships exactly this operation natively: `common_prefixes`
//!   walks the query once and yields every ancestor on that path which is
//!   itself a stored key. That is also why `fast_radix_trie` is benchmarked
//!   here as `fast_radix_trie::StringRadixMap<()>` (a `GenericRadixMap` with
//!   a unit value) rather than as `StringRadixSet`: reading `set.rs` in the
//!   crate source shows `GenericRadixSet` re-exports only
//!   `get_longest_common_prefix` (the single-longest-match variant), never
//!   `common_prefixes` (the all-matching-prefixes iterator this group
//!   needs) — so the set wrapper cannot answer this question at all, and the
//!   map-with-`()`-value shape is used instead, exactly the pattern
//!   `qp_trie::Trie<BString, ()>` above already established in this file.
//! - `predictive_search` — throughput of prefix enumeration
//!   (`Trie::keys_with_prefix` / `trie_rs::Trie::predictive_search` /
//!   `qp_trie::Trie::iter_prefix_str` /
//!   `fast_radix_trie::GenericRadixMap::iter_prefix`), both for the full
//!   corpus (empty prefix) and for single-letter prefixes (the shape a real
//!   autocomplete issues).
//!
//! ## `fast_radix_trie` operations left out, and why
//!
//! `fast_radix_trie` also exposes `wildcard_iter` (glob-style `*`/`?`
//! pattern matching over stored keys) and `split_by_prefix` (destructively
//! partitions a tree in place into "matches this prefix" / "does not").
//! Neither is benchmarked here: Verbora has no glob-matching API at all, so
//! timing `wildcard_iter` alone would measure an unmatched capability, not a
//! competition between two implementations of the same operation; and
//! `split_by_prefix` mutates (and empties part of) the structure it is
//! called on, so it cannot be run as the repeated read-only Criterion
//! iteration every other operation in this file is, and no other trie
//! benchmarked here has a destructive-partition operation to compare it
//! against regardless.
//!
//! Every implementation's input and output is wrapped in `black_box`, per the
//! project's `BLACK-BOX EVERYTHING` rule.
//!
//! # Why `trie-rs`'s "build" is a compile step, not an insert loop
//!
//! `trie-rs` is LOUDS-based: entries are pushed into a `TrieBuilder` and then
//! compiled once via `.build()` into an immutable succinct structure — there
//! is no incremental single-word insert into an already-built `Trie`. That is
//! a real architectural difference from Verbora's (and `qp-trie`'s) mutable,
//! incrementally-insertable structure, and it is not hidden: `build` times
//! the whole push-then-compile sequence, which is the only way `trie-rs` can
//! be given this word list at all, and is exactly what any real caller would
//! do (build once from a fixed dictionary, then query many times).
//!
//! # Word list: synthetic ASCII strings, not a dictionary
//!
//! `benches/data/words.json` (`tools/bench-data/generate.py`) is a
//! deterministic-LCG-generated list of 20 000 lowercase ASCII strings, 3–14
//! characters, matching the shape ("ASCII English word lists") the matrix's
//! research dossier describes — this is the same corpus every other
//! benchmark in this workspace already treats as "the word list" (see
//! `crates/verbora-trie/benches/trie.rs`'s own `load_words`), not real
//! dictionary vocabulary. Since the alphabet is `a`–`z` only, this dataset
//! never exercises the ASCII-digit-first quirk of Verbora's `for…in`
//! ordering (documented in `verbora_trie::Trie::keys_with_prefix`) — that
//! quirk is a real, separately-tested property of the exact-semantics row
//! this file does not benchmark, not something silently dodged here.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fast_radix_trie::StringRadixMap;
use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Set, Streamer};
use qp_trie::Trie as QpTrie;
use qp_trie::wrapper::BString;
use trie_rs::{Trie as TrieRs, TrieBuilder};
use verbora_trie::{FrozenTrie, Trie};

// ---------------------------------------------------------------------------
// Shared input
// ---------------------------------------------------------------------------

/// The shared word list, read from `benches/data/words.json` — the same file
/// `crates/verbora-trie/benches/trie.rs` reads.
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

/// A dictionary-shaped variant: short shared stems with many suffixes.
/// Identical construction to `crates/verbora-trie/benches/trie.rs`'s own
/// `prefix_heavy`, so the two "build" benchmarks (in-workspace vs.
/// competitive) describe the same dataset.
fn prefix_heavy(words: &[String]) -> Vec<String> {
    const SUFFIXES: [&str; 8] = ["", "s", "ed", "ing", "er", "est", "ly", "ness"];
    words
        .iter()
        .take(4000)
        .flat_map(|w| SUFFIXES.iter().map(move |s| format!("{w}{s}")))
        .collect()
}

/// Words that are *not* in the trie: the reference's own convention
/// (`crates/verbora-trie/benches/trie.rs::misses`) — no generated word
/// contains an uppercase letter, so appending one guarantees a miss.
fn misses(words: &[String]) -> Vec<String> {
    words.iter().map(|w| format!("{w}QQ")).collect()
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn build_verbora(words: &[String]) -> Trie {
    let mut t = Trie::new();
    t.add_strings(words.iter().map(String::as_str));
    t
}

/// `Trie::freeze`'s path-compressed representation, built from the same
/// corpus — Verbora's own answer to `fast_radix_trie`'s path compression,
/// added after this file's own `predictive_search` group first measured
/// Verbora losing to it (`docs/PERFORMANCE_GAPS.md` entry 32). `build` here
/// is intentionally not timed as a `build` group entry: the "build" this
/// file measures is a single `add_strings` bulk load, which every other
/// implementation in this group must also do from scratch; `freeze()` is a
/// SEPARATE, additional step only `verbora_frozen`'s own rows pay for, timed
/// on its own in `crates/verbora-trie/benches/trie.rs`'s `freeze` group
/// instead of blended into this file's numbers.
fn build_verbora_frozen(words: &[String]) -> FrozenTrie {
    build_verbora(words).freeze()
}

fn build_trie_rs(words: &[String]) -> TrieRs<u8> {
    let mut b = TrieBuilder::new();
    for w in words {
        b.push(w.as_str());
    }
    b.build()
}

fn build_qp_trie(words: &[String]) -> QpTrie<BString, ()> {
    let mut t = QpTrie::new();
    for w in words {
        t.insert_str(w.as_str(), ());
    }
    t
}

/// `StringRadixMap<()>` (a `GenericRadixMap` with a unit value), not
/// `StringRadixSet` — see the module doc comment's `common_prefix_search`
/// paragraph for why the set wrapper cannot be used here.
fn build_fast_radix_trie(words: &[String]) -> StringRadixMap<()> {
    let mut t = StringRadixMap::new();
    for w in words {
        t.insert(w.as_str(), ());
    }
    t
}

/// Sorts and deduplicates `words`, then streams the result into an
/// `fst::Set`. `fst::SetBuilder`/`Set::from_iter` requires strictly
/// increasing input — a real, disclosed asymmetry against Verbora's
/// arbitrary-order `add_strings`, see the module doc comment's `fst`
/// section. The sort (and dedup — `fst` errors on an exact duplicate key,
/// the same reason `verbora_spellcheck::FuzzyIndexBuilder`'s own bench
/// dedupes this corpus) happens **inside** this function, and is therefore
/// included in the timed `build` benchmark below rather than hoisted out of
/// it: the same "measure the real cost, don't hide it" rule `trie-rs`'s own
/// push-then-compile `build` step already follows in this file.
fn build_fst(words: &[String]) -> Set<Vec<u8>> {
    let mut sorted: Vec<&str> = words.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    Set::from_iter(sorted).expect("sorted, deduplicated input builds an fst::Set")
}

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

fn bench_build(c: &mut Criterion) {
    let words = load_words();
    let heavy = prefix_heavy(&words);
    let mut g = c.benchmark_group("build");

    for (label, set) in [("random", &words), ("prefix_heavy", &heavy)] {
        g.throughput(Throughput::Elements(set.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", label), set, |b, set| {
            b.iter(|| black_box(build_verbora(black_box(set)).get_size()));
        });
        g.bench_with_input(BenchmarkId::new("trie_rs", label), set, |b, set| {
            b.iter(|| black_box(build_trie_rs(black_box(set))));
        });
        g.bench_with_input(BenchmarkId::new("qp_trie", label), set, |b, set| {
            b.iter(|| black_box(build_qp_trie(black_box(set)).count()));
        });
        g.bench_with_input(BenchmarkId::new("fast_radix_trie", label), set, |b, set| {
            b.iter(|| black_box(build_fast_radix_trie(black_box(set)).len()));
        });
        // Includes the sort + dedup `build_fst` performs internally — see
        // that function's own doc comment for why this is not hidden.
        g.bench_with_input(BenchmarkId::new("fst", label), set, |b, set| {
            b.iter(|| black_box(build_fst(black_box(set))));
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// contains_hit / contains_miss
// ---------------------------------------------------------------------------

fn bench_contains(c: &mut Criterion) {
    let words = load_words();
    let absent = misses(&words);

    let verbora = build_verbora(&words);
    // Included here for a complete, honest picture, not because this group
    // is `Trie::freeze`'s target: `crates/verbora-trie/benches/trie.rs`'s own
    // `contains_hit`/`contains_miss` groups already found a real regression
    // (~1.5x-1.6x slower than the mutable arena) — the extra `units` buffer
    // indirection and multi-unit slice comparison per edge cost more than
    // they save on operations that only ever compress AWAY node-hop count
    // along a single path, not across a whole subtree the way enumeration
    // does. See `docs/PERFORMANCE_GAPS.md` entry 32's update.
    let verbora_frozen = build_verbora_frozen(&words);
    let trie_rs = build_trie_rs(&words);
    let qp_trie = build_qp_trie(&words);
    let fast_radix_trie = build_fast_radix_trie(&words);
    // Built once, outside the timed `contains` loop — the sort/dedup cost is
    // charged only to the `build` group above, matching how every other
    // structure in this function is built once before its query benchmarks.
    let fst_set = build_fst(&words);

    for (group_name, probes) in [("contains_hit", &words), ("contains_miss", &absent)] {
        let mut g = c.benchmark_group(group_name);
        g.throughput(Throughput::Elements(probes.len() as u64));

        g.bench_with_input(BenchmarkId::new("verbora", "words"), probes, |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| verbora.contains(black_box(w)))
                    .count()
            });
        });
        g.bench_with_input(
            BenchmarkId::new("verbora_frozen", "words"),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| verbora_frozen.contains(black_box(w)))
                        .count()
                });
            },
        );
        g.bench_with_input(BenchmarkId::new("trie_rs", "words"), probes, |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| trie_rs.exact_match(black_box(w.as_str())))
                    .count()
            });
        });
        g.bench_with_input(BenchmarkId::new("qp_trie", "words"), probes, |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| qp_trie.contains_key_str(black_box(w.as_str())))
                    .count()
            });
        });
        g.bench_with_input(
            BenchmarkId::new("fast_radix_trie", "words"),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| fast_radix_trie.contains_key(black_box(w.as_str())))
                        .count()
                });
            },
        );
        g.bench_with_input(BenchmarkId::new("fst", "words"), probes, |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| fst_set.contains(black_box(w.as_str())))
                    .count()
            });
        });

        g.finish();
    }
}

// ---------------------------------------------------------------------------
// common_prefix_search (Verbora: find_matches_on_path) -- verbora + trie_rs +
// fast_radix_trie; see the module doc comment for why qp-trie has no native
// equivalent and fast_radix_trie is benchmarked as a `StringRadixMap<()>`
// rather than a `StringRadixSet`. `fst` is excluded for the same reason as
// qp-trie: it has no operation answering "which stored keys are prefixes of
// this query" -- only `Set::contains` (single key) and prefix
// *enumeration* via `search(Str::new(p).starts_with())`, the opposite
// direction ("keys that start with the query"). Reproducing this by probing
// `contains` once per candidate prefix length would benchmark bespoke glue
// code layered on top of `fst`, not a real `fst` capability, so it is left
// out rather than faked -- see the module doc comment's `fst` section.
// ---------------------------------------------------------------------------

fn bench_common_prefix_search(c: &mut Criterion) {
    let words = load_words();
    let probes: Vec<&str> = words.iter().take(4000).map(String::as_str).collect();

    let verbora = build_verbora(&words);
    let trie_rs = build_trie_rs(&words);
    let fast_radix_trie = build_fast_radix_trie(&words);

    let mut g = c.benchmark_group("common_prefix_search");
    g.throughput(Throughput::Elements(probes.len() as u64));

    g.bench_with_input(
        BenchmarkId::new("verbora", "probes"),
        &probes,
        |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .map(|w| verbora.iter_matches_on_path(black_box(w)).count())
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("trie_rs", "probes"),
        &probes,
        |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .map(|w| {
                        trie_rs
                            .common_prefix_search::<String, _>(black_box(w.as_bytes()))
                            .count()
                    })
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("fast_radix_trie", "probes"),
        &probes,
        |b, probes| {
            b.iter(|| {
                probes
                    .iter()
                    .map(|w| fast_radix_trie.common_prefixes(black_box(w)).count())
                    .sum::<usize>()
            });
        },
    );

    g.finish();
}

// ---------------------------------------------------------------------------
// predictive_search (Verbora: keys_with_prefix)
// ---------------------------------------------------------------------------

fn bench_predictive_search(c: &mut Criterion) {
    let words = load_words();
    let letters: Vec<String> = (b'a'..=b'z').map(|c| (c as char).to_string()).collect();

    let verbora = build_verbora(&words);
    let verbora_frozen = build_verbora_frozen(&words);
    let trie_rs = build_trie_rs(&words);
    let qp_trie = build_qp_trie(&words);
    let fast_radix_trie = build_fast_radix_trie(&words);
    let fst_set = build_fst(&words);

    let mut g = c.benchmark_group("predictive_search");

    // Full enumeration: every stored word, empty prefix. `bench_with_input`
    // throughout (rather than `bench_function`) for one uniform call shape
    // across every group in this file, matching `benches/distance.rs`'s own
    // convention; the `&()` input is unused since each closure already
    // captures its own trie by reference.
    g.throughput(Throughput::Elements(words.len() as u64));
    g.bench_with_input(BenchmarkId::new("verbora", "all"), &(), |b, ()| {
        b.iter(|| black_box(verbora.keys_with_prefix("")).len());
    });
    g.bench_with_input(BenchmarkId::new("verbora_frozen", "all"), &(), |b, ()| {
        b.iter(|| black_box(verbora_frozen.keys_with_prefix("")).len());
    });
    g.bench_with_input(BenchmarkId::new("trie_rs", "all"), &(), |b, ()| {
        b.iter(|| {
            trie_rs
                .predictive_search::<String, _>(black_box(""))
                .count()
        });
    });
    g.bench_with_input(BenchmarkId::new("qp_trie", "all"), &(), |b, ()| {
        b.iter(|| qp_trie.iter_prefix_str(black_box("")).count());
    });
    g.bench_with_input(BenchmarkId::new("fast_radix_trie", "all"), &(), |b, ()| {
        b.iter(|| fast_radix_trie.iter_prefix(black_box("")).count());
    });
    // Empty-prefix enumeration is exactly `Set::stream()`, `fst`'s plain
    // lexicographic walk of every key -- equivalent to, but cheaper to write
    // than, `search(Str::new("").starts_with())`.
    g.bench_with_input(BenchmarkId::new("fst", "all"), &(), |b, ()| {
        b.iter(|| {
            let mut stream = fst_set.stream();
            let mut n = 0usize;
            while stream.next().is_some() {
                n += 1;
            }
            black_box(n)
        });
    });

    // One-letter prefixes: the shape a real autocomplete issues.
    g.throughput(Throughput::Elements(letters.len() as u64));
    g.bench_with_input(
        BenchmarkId::new("verbora", "1char"),
        &letters,
        |b, letters| {
            b.iter(|| {
                letters
                    .iter()
                    .map(|c| verbora.iter_keys_with_prefix(black_box(c)).count())
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("verbora_frozen", "1char"),
        &letters,
        |b, letters| {
            b.iter(|| {
                letters
                    .iter()
                    .map(|c| verbora_frozen.iter_keys_with_prefix(black_box(c)).count())
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("trie_rs", "1char"),
        &letters,
        |b, letters| {
            b.iter(|| {
                letters
                    .iter()
                    .map(|c| {
                        trie_rs
                            .predictive_search::<String, _>(black_box(c.as_str()))
                            .count()
                    })
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("qp_trie", "1char"),
        &letters,
        |b, letters| {
            b.iter(|| {
                letters
                    .iter()
                    .map(|c| qp_trie.iter_prefix_str(black_box(c.as_str())).count())
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(
        BenchmarkId::new("fast_radix_trie", "1char"),
        &letters,
        |b, letters| {
            b.iter(|| {
                letters
                    .iter()
                    .map(|c| fast_radix_trie.iter_prefix(black_box(c.as_str())).count())
                    .sum::<usize>()
            });
        },
    );
    g.bench_with_input(BenchmarkId::new("fst", "1char"), &letters, |b, letters| {
        b.iter(|| {
            letters
                .iter()
                .map(|c| {
                    let automaton = Str::new(black_box(c)).starts_with();
                    let mut stream = fst_set.search(automaton).into_stream();
                    let mut n = 0usize;
                    while stream.next().is_some() {
                        n += 1;
                    }
                    n
                })
                .sum::<usize>()
        });
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_contains,
    bench_common_prefix_search,
    bench_predictive_search,
);
criterion_main!(benches);
