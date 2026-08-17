// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the trie, and the measurement that justifies its
//! storage layout.
//!
//! # What is being compared
//!
//! The reference gives every node its own the reference object holding its own hash
//! map, so a lookup is a hash plus a pointer chase and building an *n*-node trie
//! costs *n* allocations. `SipTrie`/`FxTrie` below are the closest faithful Rust
//! analogues of that shape — one `HashMap<u16, Box<Node>>` per node — in two
//! flavours, `std`'s SipHash and `rustc-hash`'s FxHash, so the arena is not
//! being flattered by a slow hasher.
//!
//! The shipped [`verbora_trie::Trie`] instead keeps all nodes in one `Vec` and
//! each node's children in a two-element inline array scanned linearly.
//!
//! ## Measured (20 000-word list, 32 000 for `prefix_heavy`)
//!
//! | Benchmark | arena | hashmap (Fx) | hashmap (Sip) |
//! |---|--:|--:|--:|
//! | `build/random` | **1.48 ms** | 25.1 ms | 41.9 ms |
//! | `build/prefix_heavy` | **2.18 ms** | 11.5 ms | 15.2 ms |
//! | `contains_hit` | **1.13 ms** | 1.20 ms | 3.15 ms |
//! | `contains_miss` | **1.26 ms** | 1.94 ms | 3.51 ms |
//! | `get_size` | **0.23 ns** | 1.87 ms | — |
//!
//! The arena wins everywhere, and the margin says *why*: build is 17× faster
//! than the fastest hash baseline because the arena makes no per-node
//! allocation, while `contains_hit` is a near tie with FxHash — at one or two
//! children per node, scanning an inline array costs about what hashing a `u16`
//! does. So the layout is chosen for its build cost and its memory behaviour,
//! not for a lookup advantage that does not exist. `get_size` is the outlier
//! only because the arena's length *is* the answer.
//!
//! Against the reference implementation the same inputs run 5.5–15× faster,
//! and `get_size` moves from a full traversal to a field read.
//!
//! Inputs come from `benches/data/words.json`, which every sibling harness
//! reads too, so all of them are measured on byte-identical data.

use std::collections::HashMap;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use rustc_hash::FxHashMap;
use verbora_trie::{FrozenTrie, Trie};

// ---------------------------------------------------------------------------
// Baseline: one hash map per node, mirroring the reference's object-per-node
// ---------------------------------------------------------------------------

/// Generates a trie in the shape the reference uses: one node per code unit, each
/// owning its own hash map, reached by a pointer chase.
///
/// A macro rather than a generic because the node type is recursive through the
/// map, which no single type parameter can express cleanly. Two instantiations
/// follow, so the arena is measured against both the default hasher and the
/// fastest one available.
macro_rules! hash_trie {
    ($node:ident, $trie:ident, $map:ident) => {
        #[derive(Default)]
        struct $node {
            children: $map<u16, Box<$node>>,
            is_word: bool,
        }

        #[derive(Default)]
        struct $trie {
            root: $node,
        }

        impl $trie {
            fn add_string(&mut self, s: &str) -> bool {
                let mut node = &mut self.root;
                for unit in s.encode_utf16() {
                    node = node.children.entry(unit).or_default();
                }
                std::mem::replace(&mut node.is_word, true)
            }

            fn contains(&self, s: &str) -> bool {
                let mut node = &self.root;
                for unit in s.encode_utf16() {
                    match node.children.get(&unit) {
                        Some(next) => node = next,
                        None => return false,
                    }
                }
                node.is_word
            }

            /// The reference's `getSize`: a full traversal, because a per-node
            /// structure has nowhere to keep a running total.
            fn get_size(&self) -> usize {
                fn walk(n: &$node) -> usize {
                    1 + n.children.values().map(|c| walk(c)).sum::<usize>()
                }
                walk(&self.root)
            }

            fn from_words(words: &[String]) -> Self {
                let mut t = Self::default();
                for w in words {
                    t.add_string(w);
                }
                t
            }
        }
    };
}

hash_trie!(SipNode, SipTrie, HashMap);
hash_trie!(FxNode, FxTrie, FxHashMap);

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// The shared word list, read from `benches/data/words.json`.
fn load_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
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

/// A dictionary-shaped variant of the same words: short shared stems with many
/// suffixes, which is what real vocabularies look like and where a trie's node
/// sharing actually pays off. Random words share almost nothing below depth 2.
fn prefix_heavy(words: &[String]) -> Vec<String> {
    const SUFFIXES: [&str; 8] = ["", "s", "ed", "ing", "er", "est", "ly", "ness"];
    words
        .iter()
        .take(4000)
        .flat_map(|w| SUFFIXES.iter().map(move |s| format!("{w}{s}")))
        .collect()
}

/// The same words transliterated into Cyrillic, to measure the non-ASCII path:
/// two UTF-8 bytes but still one code unit, so the node count is unchanged while
/// folding and slicing get more expensive.
fn cyrillic(words: &[String]) -> Vec<String> {
    const ALPHABET: [char; 26] = [
        'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
        'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ',
    ];
    words
        .iter()
        .map(|w| {
            w.bytes()
                .map(|b| ALPHABET[usize::from(b - b'a') % ALPHABET.len()])
                .collect()
        })
        .collect()
}

/// Words that are *not* in the trie, for the miss path: misses take a different
/// route (they exit early) and are the common case in real lookups.
fn misses(words: &[String]) -> Vec<String> {
    words.iter().map(|w| format!("{w}QQ")).collect()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_build(c: &mut Criterion) {
    let words = load_words();
    let heavy = prefix_heavy(&words);
    let mut g = c.benchmark_group("build");

    for (label, set) in [("random", &words), ("prefix_heavy", &heavy)] {
        g.throughput(Throughput::Elements(set.len() as u64));

        g.bench_with_input(BenchmarkId::new("arena", label), set, |b, set| {
            b.iter(|| {
                let mut t = Trie::new();
                t.add_strings(set.iter().map(String::as_str));
                black_box(t.get_size())
            });
        });

        g.bench_with_input(BenchmarkId::new("hashmap_sip", label), set, |b, set| {
            b.iter(|| black_box(SipTrie::from_words(set).get_size()));
        });

        g.bench_with_input(BenchmarkId::new("hashmap_fx", label), set, |b, set| {
            b.iter(|| black_box(FxTrie::from_words(set).get_size()));
        });
    }

    g.finish();
}

fn bench_contains(c: &mut Criterion) {
    let words = load_words();
    let absent = misses(&words);

    let arena: Trie = words.iter().collect();
    let frozen: FrozenTrie = arena.freeze();
    let sip = SipTrie::from_words(&words);
    let fx = FxTrie::from_words(&words);

    for (name, probes) in [("contains_hit", &words), ("contains_miss", &absent)] {
        let mut g = c.benchmark_group(name);
        g.throughput(Throughput::Elements(probes.len() as u64));

        g.bench_function("arena", |b| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| arena.contains(black_box(w)))
                    .count()
            });
        });
        // The path-compressed frozen representation — see `bench_freeze` and
        // `bench_enumeration`'s `frozen_*` functions for where this is meant
        // to win; `contains` is here mainly to confirm it does not regress.
        g.bench_function("frozen", |b| {
            b.iter(|| {
                probes
                    .iter()
                    .filter(|w| frozen.contains(black_box(w)))
                    .count()
            });
        });
        g.bench_function("hashmap_sip", |b| {
            b.iter(|| probes.iter().filter(|w| sip.contains(black_box(w))).count());
        });
        g.bench_function("hashmap_fx", |b| {
            b.iter(|| probes.iter().filter(|w| fx.contains(black_box(w))).count());
        });

        g.finish();
    }
}

/// The one-time cost `Trie::freeze` pays, kept as its own group per the
/// Build → Freeze → Query pattern's own convention of measuring every stage
/// separately rather than blending it into a query number.
fn bench_freeze(c: &mut Criterion) {
    let words = load_words();
    let arena: Trie = words.iter().collect();

    let mut g = c.benchmark_group("freeze");
    g.throughput(Throughput::Elements(words.len() as u64));
    g.bench_function("arena", |b| b.iter(|| black_box(arena.freeze())));
    g.finish();
}

fn bench_queries(c: &mut Criterion) {
    let words = load_words();
    let arena: Trie = words.iter().collect();
    let probes: Vec<&str> = words.iter().take(4000).map(String::as_str).collect();

    let mut g = c.benchmark_group("queries");
    g.throughput(Throughput::Elements(probes.len() as u64));

    g.bench_function("find_matches_on_path", |b| {
        b.iter(|| {
            probes
                .iter()
                .map(|w| arena.iter_matches_on_path(black_box(w)).count())
                .sum::<usize>()
        });
    });

    // The allocation-free variant: callers who only need the split point never
    // have to materialise either half.
    g.bench_function("find_prefix_lengths", |b| {
        b.iter(|| {
            probes
                .iter()
                .map(|w| arena.find_prefix_lengths(black_box(w)).1)
                .sum::<usize>()
        });
    });

    g.bench_function("find_prefix", |b| {
        b.iter(|| {
            probes
                .iter()
                .map(|w| arena.find_prefix(black_box(w)).1.len())
                .sum::<usize>()
        });
    });

    g.finish();
}

fn bench_enumeration(c: &mut Criterion) {
    let words = load_words();
    let arena: Trie = words.iter().collect();
    let frozen: FrozenTrie = arena.freeze();

    let mut g = c.benchmark_group("enumeration");
    g.throughput(Throughput::Elements(words.len() as u64));

    // Streaming avoids the result `Vec` but still allocates one `String` per key
    // — a stored word exists nowhere contiguously, so it must be spelled out.
    g.bench_function("keys_stream", |b| {
        b.iter(|| arena.keys().map(|k| k.len()).sum::<usize>());
    });
    g.bench_function("keys_collect", |b| {
        b.iter(|| black_box(arena.keys_with_prefix("")).len());
    });
    // One-letter prefixes are the shape an autocomplete actually issues.
    g.bench_function("keys_with_prefix_1char", |b| {
        b.iter(|| {
            (b'a'..=b'z')
                .map(|c| {
                    arena
                        .iter_keys_with_prefix(black_box(&(c as char).to_string()))
                        .count()
                })
                .sum::<usize>()
        });
    });

    // `FrozenTrie` counterparts of the three groups above — the operation
    // `Trie::freeze`'s own doc comment (and `docs/PERFORMANCE_GAPS.md` entry
    // 32) identify as this crate's one measured loss against a competitor,
    // and the reason path compression exists at all.
    g.bench_function("frozen_keys_stream", |b| {
        b.iter(|| frozen.keys().map(|k| k.len()).sum::<usize>());
    });
    g.bench_function("frozen_keys_collect", |b| {
        b.iter(|| black_box(frozen.keys_with_prefix("")).len());
    });
    g.bench_function("frozen_keys_with_prefix_1char", |b| {
        b.iter(|| {
            (b'a'..=b'z')
                .map(|c| {
                    frozen
                        .iter_keys_with_prefix(black_box(&(c as char).to_string()))
                        .count()
                })
                .sum::<usize>()
        });
    });

    g.finish();
}

fn bench_unicode_and_folding(c: &mut Criterion) {
    let words = load_words();
    let cyr = cyrillic(&words);

    let ascii: Trie = words.iter().collect();
    let cyr_trie: Trie = cyr.iter().collect();

    let mut folding = Trie::case_insensitive();
    folding.add_strings(words.iter().map(String::as_str));
    let upper: Vec<String> = words.iter().map(|w| w.to_uppercase()).collect();

    let mut g = c.benchmark_group("unicode_and_folding");
    g.throughput(Throughput::Elements(words.len() as u64));

    g.bench_function("contains_ascii", |b| {
        b.iter(|| {
            words
                .iter()
                .filter(|w| ascii.contains(black_box(w)))
                .count()
        });
    });
    g.bench_function("contains_cyrillic", |b| {
        b.iter(|| {
            cyr.iter()
                .filter(|w| cyr_trie.contains(black_box(w)))
                .count()
        });
    });
    // Already-lowercase input takes the borrowing path through `fold`; the
    // upper-case probes pay for the copy.
    g.bench_function("contains_folding_nochange", |b| {
        b.iter(|| {
            words
                .iter()
                .filter(|w| folding.contains(black_box(w)))
                .count()
        });
    });
    g.bench_function("contains_folding_uppercase", |b| {
        b.iter(|| {
            upper
                .iter()
                .filter(|w| folding.contains(black_box(w)))
                .count()
        });
    });

    g.finish();
}

fn bench_get_size(c: &mut Criterion) {
    let words = load_words();
    let arena: Trie = words.iter().collect();
    let fx = FxTrie::from_words(&words);

    // The reference warns that `getSize` "may not be good for frequent use"
    // because it traverses everything. In the arena it is a `Vec::len`.
    let mut g = c.benchmark_group("get_size");
    g.bench_function("arena", |b| b.iter(|| black_box(arena.get_size())));
    g.bench_function("hashmap_fx", |b| b.iter(|| black_box(fx.get_size())));
    g.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_contains,
    bench_freeze,
    bench_queries,
    bench_enumeration,
    bench_unicode_and_folding,
    bench_get_size
);
criterion_main!(benches);
