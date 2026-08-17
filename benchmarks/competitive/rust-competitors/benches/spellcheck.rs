// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style is
// kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. real, pinned third-party Rust competitors — spellchecking.
//!
//! Reads the exact same `benches/data/words.json` that
//! `crates/verbora-spellcheck/benches/spellcheck.rs` (Verbora's own
//! in-workspace bench) already reads. See `docs/COMPETITIVE_BENCHMARKS.md` §1.17 for why
//! `symspell`, `harper-core` and `spellbook` were selected, and
//! `../../README.md` for why this crate lives outside the main workspace.
//!
//! # Four competitors, four different fairness shapes
//!
//! - **`symspell`** (0.5.2) — same task shape as Verbora's
//!   `get_corrections`: `SymSpell::lookup(word, Verbosity, max_edit_distance)`
//!   against a loaded frequency dictionary. Loaded with the **exact same
//!   `words.json` corpus** Verbora is (same words, same per-word
//!   frequencies, same `max_distance`) — the matrix's explicit "load both
//!   engines with the same frequency dictionary" instruction. `Verbosity::All`
//!   is used because it matches Verbora's own accumulate-every-level-up-to-
//!   `max_distance` behaviour (`Spellcheck::corrections_over` loops
//!   `depth in 1..=distance` and appends every level's hits, not just the
//!   closest non-empty one — see that function's source) more closely than
//!   `Verbosity::Closest`, which would stop at the first non-empty level.
//!   The algorithm itself (delete-precomputation, O(1)-average lookup) is
//!   still a genuinely different mechanism from Verbora's combinatorial edit
//!   generation (matrix: `Partial`) — this file never asserts the two
//!   return the same suggestion *set*, only that they are given the same
//!   corpus and asked the same `max_distance` question.
//! - **`harper-core`** (2.8.0, `default-features = false` — see
//!   `Cargo.toml`) — `spell::FstDictionary::new` accepts an arbitrary word
//!   list, so it is **also** loaded with Verbora's own `words.json` corpus
//!   (via `Dictionary::fuzzy_match_str`, the closest analogue to
//!   `get_corrections`) rather than `FstDictionary::curated()` — the
//!   matrix's "load a comparable dictionary" branch, not the "label the
//!   comparison explicitly" one, because harper-core's constructor happens
//!   to make the first branch possible.
//! - **`fast_symspell`** (0.1.10) — a *second* SymSpell-family competitor.
//!   Reading its real source directly (this crate registers no repository
//!   URL on crates.io, so `cargo doc`/the resolved dependency's own
//!   registry-cache source — the literal contents of the published `.crate`
//!   tarball crates.io always hosts, `.crate`-file-equivalent, read here
//!   rather than guessed at from dependency names) shows `symspell.rs` is a
//!   near-verbatim fork of `symspell` 0.5.2's own `SymSpell::lookup`,
//!   `create_dictionary_entry` and `edits_prefix` — same deletion-index
//!   algorithm, confirmed line-for-line, not merely "SymSpell-family" by
//!   name. Three real deltas earn it a separate row rather than folding it
//!   into the `symspell` one above:
//!   1. `ahash::RandomState` replaces `DefaultHasher` for the `deletes`
//!      map's hash.
//!   2. `triple_accel::levenshtein::rdamerau_exp` (SIMD-accelerated
//!      restricted Damerau-Levenshtein) replaces `strsim::damerau_levenshtein`
//!      as the post-deletion-lookup verification pass. **This dependency
//!      has a real, reproducible bug** (`triple_accel` 0.4.0, last commit
//!      2023-03-13): it over-counts the distance for a single-character
//!      insertion that duplicates a nearby character (`"tac"` →
//!      `"tatc"` comes back as 2, not the correct 1 — confirmed against
//!      `strsim::damerau_levenshtein` and against `triple_accel`'s own
//!      plain-Levenshtein function, which can only be smaller-or-equal).
//!      Found and pinned down with minimal counterexamples in
//!      `tests/spellcheck_fast_symspell_correctness.rs`'s
//!      `triple_accel_rdamerau_exp_overcounts_on_a_reproducible_input_shape`
//!      — real, silently missed corrections on an ordinary
//!      doubled-letter-typo shape, not a synthetic corner case.
//!   3. `rkyv` `Archive`/`Serialize` derives plus a new `ArchivedSymSpell`
//!      type add a **second construction path**: serialize a built
//!      dictionary once with `SymSpell::save`, then reconstitute it via
//!      `ArchivedSymSpell::from_bytes` without re-running
//!      `create_dictionary_entry` for every word — see
//!      [`bench_fast_symspell_archived_load`].
//!
//!   `fast_symspell::SymSpellBuilder` also silently changes one default
//!   from `symspell`'s own: `count_threshold` is `100` here, `1` there. See
//!   [`build_fast_symspell`]'s own doc comment and
//!   `tests/spellcheck_fast_symspell_correctness.rs` for why that is not a
//!   cosmetic difference for this corpus specifically.
//! - **`spellbook`** (0.4.2) — Hunspell affix-rule morphological correction.
//!   Its `.aff`/`.dic` format cannot represent Verbora's flat frequency
//!   corpus (there is no such thing as a "frequency" in a Hunspell
//!   dictionary, and affix rules generate word forms Verbora's corpus never
//!   contains), so per the matrix this is a **matched-workload TIMING
//!   comparison only**, run in its own `spellcheck_spellbook_*` groups on a
//!   real `en_US` dictionary — never Verbora's corpus, and never presented
//!   next to the `words.json`-based numbers as if the inputs matched. See
//!   [`bench_spellbook`]'s own doc comment.
//!
//! # Fetched assets, and graceful skipping
//!
//! `spellbook` needs a real Hunspell `.aff`/`.dic` pair, which this
//! repository does not vendor (a separately-licensed multi-hundred-KB
//! dictionary — same reasoning as the WordNet database, see
//! `crates/verbora-wordnet/benches/wordnet.rs`). Run
//! `../../scripts/fetch-models.sh hunspell-en-us` once; the `spellbook`
//! groups skip cleanly with a printed notice if it is absent. `symspell` and
//! `harper-core` need no external asset — both build their dictionaries from
//! `benches/data/words.json`, already required by every other spellcheck
//! bench in this repository.
//!
//! # `fast_symspell`: re-investigated, not taken on faith
//!
//! `docs/COMPETITIVE_BENCHMARKS.md`'s §1.17 matrix already carries a row for
//! `fast_symspell` from an earlier research pass: `Equivalent? = No`,
//! `Benchmarkable? = No`, reasoned as "No repository URL registered on
//! crates.io — unverifiable provenance; download curve consistent with
//! abandonment." Both factual claims underneath that verdict were
//! re-checked directly, not re-asserted:
//!
//! - **"Unverifiable provenance"** — true that crates.io lists no
//!   repository/homepage/documentation link, but false that this makes the
//!   code unreadable: crates.io always hosts the raw source tarball for
//!   every published version regardless of repository metadata, and once
//!   `fast_symspell` is a resolved dependency, `cargo`'s own registry cache
//!   holds that exact tarball's extracted contents locally — read directly
//!   for this pass (see the module doc comment above), not guessed at from
//!   its `Cargo.toml` dependency list. The source is ordinary, readable,
//!   substantially a fork of the already-vetted `symspell` 0.5.2.
//! - **"Download curve consistent with abandonment"** — crates.io's own API
//!   shows 22,760 total downloads and a **90-day recent-download window**
//!   (not an unspecified one) of 129 — low, but a real, steady 1–9
//!   downloads/day spread across essentially every day of that window
//!   (checked via `/api/v1/crates/fast_symspell/downloads`), not a cliff to
//!   zero. `daibo83`'s last publish was 2025-01-17 — genuinely stale
//!   *development* (no release in well over a year) — but that is a
//!   different claim from "abandonment" in the sense of "nobody downloads
//!   this anymore," which the steady 90-day curve does not support.
//!
//! Given real, working source (confirmed by building it, loading it with
//! this workspace's own corpus after fixing the `count_threshold` trap
//! below, and getting it to answer real spelling-correction queries
//! correctly — see `tests/spellcheck_fast_symspell_correctness.rs`), this
//! file **overturns** the prior `No`/`No` verdict: `fast_symspell` is
//! `Partial`/`Yes`, the same classification `symspell` itself already
//! carries and for the same reason (same task shape, same deletion-index
//! family, genuinely different implementation details worth measuring —
//! see the module doc comment above). The prior pass's own bar (an
//! unregistered repository) was a reasonable heuristic that happened to be
//! wrong here once someone actually read the code sitting behind it;
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.17's row should be updated to match
//! (out of scope for this file to edit directly, flagged for whoever owns
//! that document next).
//!
//! # Correctness
//!
//! No implementation here is claimed to produce Verbora's exact suggestion
//! list (the matrix marks every row `Partial`, never `Yes` — genuinely
//! different algorithms throughout). What *is* checked, in
//! `tests/spellcheck_smoke.rs`, is that every implementation actually
//! reports a real corpus word as correctly spelled and a real corpus typo as
//! incorrectly spelled — a sanity check that the benchmark measures live,
//! working calls, not a degenerate no-op. `fast_symspell` gets a second,
//! dedicated file — `tests/spellcheck_fast_symspell_correctness.rs` — for
//! the three fast_symspell-specific traps this module doc comment and
//! [`build_fast_symspell`] already flag: the `count_threshold` default that
//! would silently empty this corpus, the restricted-Damerau vs. plain
//! Levenshtein distance-semantics divergence between `fast_symspell` and
//! [`FuzzyIndex`](verbora_spellcheck::FuzzyIndex), and the narrower domain
//! where the two genuinely agree.

use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fast_symspell::{
    ArchivedSymSpell, AsciiStringStrategy as FastAsciiStringStrategy, SymSpell as FastSymSpell,
    SymSpellBuilder as FastSymSpellBuilder, Verbosity as FastVerbosity,
};
use harper_core::spell::{Dictionary as HarperDictionary, FstDictionary};
use harper_core::{CharString, DictWordMetadata};
use spellbook::Dictionary as SpellbookDictionary;
use symspell::{AsciiStringStrategy, SymSpell, SymSpellBuilder, Verbosity};
use verbora_spellcheck::{FuzzyIndexBuilder, Spellcheck};

/// Corpus sizes, in words — identical to
/// `crates/verbora-spellcheck/benches/spellcheck.rs`'s own `CORPUS_SIZES`.
const CORPUS_SIZES: [usize; 4] = [100, 1_000, 10_000, 20_000];

/// Reads the shared word list. Fails loudly, matching every sibling bench
/// that reads this same file.
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
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

/// The first word of exactly `len` characters — a rule, not an index, so
/// every sibling bench (Verbora's own, the reference's) can reproduce the same
/// choice without hard-coding a string.
fn word_of_length(words: &[String], len: usize) -> String {
    words
        .iter()
        .find(|w| w.chars().count() == len)
        .unwrap_or_else(|| panic!("no {len}-character word in the corpus"))
        .clone()
}

/// A misspelling of `word`: the middle character deleted. Identical rule to
/// `crates/verbora-spellcheck/benches/spellcheck.rs`'s `typo`.
fn typo(word: &str) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    if chars.len() > 1 {
        chars.remove(chars.len() / 2);
    }
    chars.into_iter().collect()
}

/// Word frequencies as [`Spellcheck::new`] computes them: occurrence counts
/// in the slice, in first-occurrence order. `symspell::load_dictionary_line`
/// takes the same "word count" shape, so both engines are handed the
/// identical (word, frequency) pairs.
fn frequencies(words: &[String]) -> Vec<(String, i64)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for w in words {
        *counts.entry(w.as_str()).or_insert_with(|| {
            order.push(w.clone());
            0
        }) += 1;
    }
    order
        .into_iter()
        .map(|w| {
            let c = counts[w.as_str()];
            (w, c)
        })
        .collect()
}

fn build_symspell(words: &[String], max_distance: i64) -> SymSpell<AsciiStringStrategy> {
    let mut sc: SymSpell<AsciiStringStrategy> = SymSpellBuilder::default()
        .max_dictionary_edit_distance(max_distance)
        .build()
        .expect("valid SymSpell configuration");
    for (word, count) in frequencies(words) {
        sc.load_dictionary_line(&format!("{word} {count}"), 0, 1, " ");
    }
    sc
}

fn build_harper(words: &[String]) -> FstDictionary {
    let entries: Vec<(CharString, DictWordMetadata)> = words
        .iter()
        .map(|w| {
            (
                w.chars().collect::<CharString>(),
                DictWordMetadata::default(),
            )
        })
        .collect();
    FstDictionary::new(entries)
}

/// Builds a `fast_symspell::SymSpell` from the exact same `(word, count)`
/// pairs [`build_symspell`] loads — same corpus, same frequencies, same
/// `max_distance` — with one deliberate, load-bearing deviation from "match
/// every constructor argument": `count_threshold` is forced to `0`.
///
/// `fast_symspell::SymSpellBuilder` defaults `count_threshold` to **100**;
/// `symspell::SymSpellBuilder` (used unmodified by [`build_symspell`])
/// defaults it to **1**. `benches/data/words.json`'s own per-word
/// frequencies never exceed 3 (checked directly against the generated
/// corpus), so building with `fast_symspell`'s untouched default would make
/// `create_dictionary_entry` reject *every* word — an empty dictionary, and
/// every `fast_symspell` benchmark number in this file would measure
/// nothing but "how fast is a lookup against zero entries."
/// `tests/spellcheck_fast_symspell_correctness.rs`'s
/// `demonstrates_default_count_threshold_would_empty_this_corpus` is the
/// regression test that would fail loudly if this override were ever
/// dropped. `0` is the closest available match to `symspell`'s own
/// behaviour here, not a genuinely different fairness stance: every count
/// [`frequencies`] produces is `>= 1`, so both crates' *original* defaults
/// would let the whole corpus through if `fast_symspell` had not
/// independently chosen a much higher one.
fn build_fast_symspell(
    words: &[String],
    max_distance: i64,
) -> FastSymSpell<FastAsciiStringStrategy> {
    let mut sc: FastSymSpell<FastAsciiStringStrategy> = FastSymSpellBuilder::default()
        .max_dictionary_edit_distance(max_distance)
        .count_threshold(0)
        .build()
        .expect("valid fast_symspell configuration");
    for (word, count) in frequencies(words) {
        sc.load_dictionary_line(&format!("{word} {count}"), 0, 1, " ");
    }
    sc
}

/// The first `n` *distinct* words drawn from `words` — mirrors
/// `crates/verbora-spellcheck/benches/fuzzy_index.rs`'s own
/// `distinct_words`, needed for the same reason here: both
/// [`FuzzyIndexBuilder`] and `fast_symspell`'s internal `words:
/// HashMap<String, i64>` collapse duplicate dictionary entries down to one,
/// so a corpus slice that double-counts repeats would understate `n`'s real
/// effect on either structure's shape.
fn distinct_words(words: &[String], n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words
        .iter()
        .filter(|w| seen.insert((*w).clone()))
        .take(n)
        .cloned()
        .collect()
}

/// `benchmarks/competitive/models/hunspell-en_US/`, or `$HUNSPELL_EN_US_DIR`.
fn hunspell_en_us_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("HUNSPELL_EN_US_DIR") {
        let p = PathBuf::from(v);
        if p.join("en_US.aff").is_file() {
            return Some(p);
        }
    }
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models/hunspell-en_US");
    vendored.join("en_US.aff").is_file().then_some(vendored)
}

fn build_spellbook(dir: &Path) -> SpellbookDictionary {
    let aff = std::fs::read_to_string(dir.join("en_US.aff")).expect("en_US.aff reads");
    let dic = std::fs::read_to_string(dir.join("en_US.dic")).expect("en_US.dic reads");
    SpellbookDictionary::new(&aff, &dic).expect("en_US.aff/.dic parse")
}

fn bench_new(c: &mut Criterion) {
    let ascii = words();
    let mut g = c.benchmark_group("spellcheck_new");
    for n in CORPUS_SIZES {
        let slice = &ascii[..n];
        g.throughput(Throughput::Elements(n as u64));

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| black_box(Spellcheck::new(black_box(slice))));
        });
        g.bench_with_input(BenchmarkId::new("symspell", n), &n, |b, _| {
            b.iter(|| black_box(build_symspell(black_box(slice), 2)));
        });
        g.bench_with_input(BenchmarkId::new("harper_core", n), &n, |b, _| {
            b.iter(|| black_box(build_harper(black_box(slice))));
        });
        g.bench_with_input(BenchmarkId::new("fast_symspell", n), &n, |b, _| {
            b.iter(|| black_box(build_fast_symspell(black_box(slice), 2)));
        });
    }
    g.finish();
}

fn bench_is_correct(c: &mut Criterion) {
    let ascii = words();
    let sc = Spellcheck::new(&ascii);
    let symspell = build_symspell(&ascii, 2);
    let harper = build_harper(&ascii);
    let fast_symspell = build_fast_symspell(&ascii, 2);

    const PROBE_COUNT: usize = 4_000;
    let hits: Vec<&str> = ascii[..PROBE_COUNT].iter().map(String::as_str).collect();
    let near_owned: Vec<String> = ascii[..PROBE_COUNT].iter().map(|w| typo(w)).collect();
    let near: Vec<&str> = near_owned.iter().map(String::as_str).collect();
    let far_owned: Vec<String> = ascii[..PROBE_COUNT]
        .iter()
        .map(|w| format!("Q{w}"))
        .collect();
    let far: Vec<&str> = far_owned.iter().map(String::as_str).collect();

    let mut g = c.benchmark_group("spellcheck_is_correct");
    g.throughput(Throughput::Elements(PROBE_COUNT as u64));

    // `BenchmarkId::new(id, probe_kind)`, not `bench_function("id/probe_kind",
    // ...)` — the latter's `/` is sanitised into the on-disk directory name
    // rather than nested into it (Criterion flattens it to
    // e.g. `verbora_hit/`), which `scripts/collect-results.py`'s
    // `group/id/size` directory-discovery cannot see. Every group in this
    // file uses the `BenchmarkId::new` shape for exactly this reason.
    for (probe_kind, probes) in [("hit", &hits), ("miss_near", &near), ("miss_far", &far)] {
        g.bench_with_input(
            BenchmarkId::new("verbora", probe_kind),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| sc.is_correct(black_box(w)))
                        .count()
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("symspell", probe_kind),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| !symspell.lookup(black_box(w), Verbosity::Top, 0).is_empty())
                        .count()
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("harper_core", probe_kind),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| harper.contains_word_str(black_box(w)))
                        .count()
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("fast_symspell", probe_kind),
            probes,
            |b, probes| {
                b.iter(|| {
                    probes
                        .iter()
                        .filter(|w| {
                            !fast_symspell
                                .lookup(black_box(w), FastVerbosity::Top, 0)
                                .is_empty()
                        })
                        .count()
                });
            },
        );
    }

    g.finish();
}

fn bench_get_corrections(c: &mut Criterion) {
    let ascii = words();

    // Distance 1 — every corpus size, matching Verbora's own bench.
    let probe = typo(&word_of_length(&ascii, 8));
    let mut g = c.benchmark_group("spellcheck_get_corrections_d1");
    for n in CORPUS_SIZES {
        let slice = &ascii[..n];
        let sc = Spellcheck::new(slice);
        let symspell = build_symspell(slice, 1);
        let harper = build_harper(slice);
        let fast_symspell = build_fast_symspell(slice, 1);

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| black_box(sc.get_corrections(black_box(&probe), 1)));
        });
        g.bench_with_input(BenchmarkId::new("symspell", n), &n, |b, _| {
            b.iter(|| black_box(symspell.lookup(black_box(&probe), Verbosity::All, 1)));
        });
        g.bench_with_input(BenchmarkId::new("harper_core", n), &n, |b, _| {
            b.iter(|| black_box(harper.fuzzy_match_str(black_box(&probe), 1, usize::MAX)));
        });
        g.bench_with_input(BenchmarkId::new("fast_symspell", n), &n, |b, _| {
            b.iter(|| black_box(fast_symspell.lookup(black_box(&probe), FastVerbosity::All, 1)));
        });
    }
    g.finish();

    // Distance 2 — the two sizes Verbora's own bench uses (see that file's
    // doc comment for why: candidate count squares, so a full sweep across
    // all four corpus sizes is not worth the wall-clock time).
    let probe = typo(&word_of_length(&ascii, 6));
    let mut g = c.benchmark_group("spellcheck_get_corrections_d2");
    g.sample_size(20);
    for n in [1_000usize, 20_000] {
        let slice = &ascii[..n];
        let sc = Spellcheck::new(slice);
        let symspell = build_symspell(slice, 2);
        let harper = build_harper(slice);
        let fast_symspell = build_fast_symspell(slice, 2);

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| black_box(sc.get_corrections(black_box(&probe), 2)));
        });
        g.bench_with_input(BenchmarkId::new("symspell", n), &n, |b, _| {
            b.iter(|| black_box(symspell.lookup(black_box(&probe), Verbosity::All, 2)));
        });
        g.bench_with_input(BenchmarkId::new("harper_core", n), &n, |b, _| {
            b.iter(|| black_box(harper.fuzzy_match_str(black_box(&probe), 2, usize::MAX)));
        });
        g.bench_with_input(BenchmarkId::new("fast_symspell", n), &n, |b, _| {
            b.iter(|| black_box(fast_symspell.lookup(black_box(&probe), FastVerbosity::All, 2)));
        });
    }
    g.finish();
}

/// `fast_symspell`'s second construction path: [`ArchivedSymSpell::from_bytes`]
/// over an `rkyv`-serialized dictionary, vs. building the same dictionary
/// from scratch via [`build_fast_symspell`] — isolating exactly the
/// "precomputed index + zero-copy load" mechanism this file's own module
/// doc comment and the `Cargo.toml` pin comment speculate about, from the
/// deletion-index *algorithm* itself (already covered by
/// `spellcheck_new`/`spellcheck_get_corrections_*` above). Run once, at the
/// corpus's largest size, since this is a process-startup-cost question,
/// not a per-word one.
///
/// Two deliberate implementation choices, both worth spelling out:
///
/// - **[`ArchivedSymSpell::from_file`] is not benchmarked.** Reading its
///   source (`src/archived.rs`) shows it backs its `Mmap` with a
///   process-global `static OnceLock<Mmap>` — the *second* call in one
///   process, which is exactly what a Criterion `b.iter()` loop would make
///   on every sample after the first, returns the already-mapped memory for
///   free. That would benchmark "how fast is a no-op after warmup," not
///   loading cost. Calling [`ArchivedSymSpell::from_bytes`] directly against
///   a byte buffer read once, outside the timed closure, sidesteps that
///   singleton and measures the real, repeatable cost: interpreting an
///   already-available archive into a live `ArchivedSymSpell`.
/// - **`std::fs::read`, not `memmap2::Mmap`, supplies those bytes.**
///   `memmap2` is a transitive dependency of `fast_symspell`, not a direct
///   one of this crate — using it here would mean adding it to
///   `Cargo.toml`, out of scope for this pass (see this file's own
///   `Cargo.toml` pin comment). This changes *where* the archive's bytes
///   live before `from_bytes` sees them (a heap `Vec<u8>` vs. mapped pages)
///   but not what `from_bytes` itself measures: `rkyv`'s zero-copy
///   archived-struct interpretation — a pointer-cast-and-validate over
///   already-resident bytes, not a deserialization pass — which is the
///   actual mechanism in question, independent of the I/O path that put the
///   bytes in memory.
fn bench_fast_symspell_archived_load(c: &mut Criterion) {
    let ascii = words();
    let n = 20_000;
    let slice = &ascii[..n];

    let mut g = c.benchmark_group("spellcheck_fast_symspell_archived_load");
    g.throughput(Throughput::Elements(n as u64));

    g.bench_with_input(
        BenchmarkId::new("fast_symspell", "build_from_scratch"),
        &n,
        |b, _| {
            b.iter(|| black_box(build_fast_symspell(black_box(slice), 2)));
        },
    );

    let built = build_fast_symspell(slice, 2);
    let archive_path = std::env::temp_dir().join("verbora_bench_fast_symspell_20000.bin");
    built
        .save(archive_path.to_str().expect("temp path is valid UTF-8"))
        .expect("fast_symspell archive saves");
    let bytes = std::fs::read(&archive_path).expect("archive reads back");
    let _ = std::fs::remove_file(&archive_path);

    g.bench_with_input(
        BenchmarkId::new("fast_symspell", "load_archived_bytes"),
        &n,
        |b, _| {
            b.iter(|| black_box(ArchivedSymSpell::from_bytes(black_box(&bytes))));
        },
    );

    g.finish();
}

/// A *batch* of independent single-word corrections — the shape a caller
/// spell-checking a whole document or a column of free-text input actually
/// runs, as opposed to `spellcheck_get_corrections_d1`/`_d2`'s one-word-at-
/// a-time numbers above. Sequential on every side, deliberately: `Spellcheck`
/// does expose a `rayon`-parallel `par_get_corrections_batch`, but it sits
/// behind the `parallel` feature, which this workspace's `Cargo.toml` does
/// not enable for its `verbora-spellcheck` dependency (`[dependencies]
/// verbora-spellcheck = { path = "../../../crates/verbora-spellcheck" }`, no
/// `features = [...]`) — turning it on would mean editing `Cargo.toml`
/// while another process is concurrently editing that same file for other
/// competitors this session, out of scope here and flagged rather than
/// worked around. A sequential `words.iter().map(...)` loop is still the
/// fair baseline every competitor in this file can run without a feature
/// flag of its own, at both a "small" and a "large" dictionary size.
fn bench_batch_correction(c: &mut Criterion) {
    let ascii = words();
    const BATCH: usize = 200;

    let mut g = c.benchmark_group("spellcheck_batch_correction");
    g.sample_size(20);
    for n in [1_000usize, 20_000] {
        let slice = &ascii[..n];
        let sc = Spellcheck::new(slice);
        let symspell = build_symspell(slice, 1);
        let harper = build_harper(slice);
        let fast_symspell = build_fast_symspell(slice, 1);

        let probes: Vec<String> = slice.iter().take(BATCH).map(|w| typo(w)).collect();
        g.throughput(Throughput::Elements(BATCH as u64));

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| {
                for w in &probes {
                    black_box(sc.get_corrections(black_box(w), 1));
                }
            });
        });
        g.bench_with_input(BenchmarkId::new("symspell", n), &n, |b, _| {
            b.iter(|| {
                for w in &probes {
                    black_box(symspell.lookup(black_box(w), Verbosity::All, 1));
                }
            });
        });
        g.bench_with_input(BenchmarkId::new("harper_core", n), &n, |b, _| {
            b.iter(|| {
                for w in &probes {
                    black_box(harper.fuzzy_match_str(black_box(w), 1, usize::MAX));
                }
            });
        });
        g.bench_with_input(BenchmarkId::new("fast_symspell", n), &n, |b, _| {
            b.iter(|| {
                for w in &probes {
                    black_box(fast_symspell.lookup(black_box(w), FastVerbosity::All, 1));
                }
            });
        });
    }
    g.finish();
}

/// [`FuzzyIndex`](verbora_spellcheck::FuzzyIndex)'s `neighbors` vs.
/// `fast_symspell::SymSpell::lookup` — the comparison this file's own
/// module doc comment and the `Cargo.toml` pin comment both call out as the
/// one that matters: `crates/verbora-spellcheck/src/fuzzy_index.rs`'s
/// BK-tree was "weighed against [a SymSpell-style deletion index] on paper
/// but never benchmarked against a real implementation until now" (that
/// type's own doc comment). Construction and query are two independent
/// groups, deliberately mirroring
/// `crates/verbora-spellcheck/benches/fuzzy_index.rs`'s own two-group shape
/// (`fuzzy_index_construction` / `fuzzy_index_query_vs_brute_force`): same
/// `CORPUS_SIZES`, same `QUERY_COUNT`, same `MAX_DISTANCE`, same "corpus
/// words are their own queries" choice, same [`distinct_words`] dedup
/// (both structures collapse duplicate dictionary entries, so a slice that
/// double-counts repeats would understate `n`'s real effect on either).
///
/// **Not the same output shape, so only counts are compared here.**
/// `fast_symspell::Suggestion` carries a verified restricted-Damerau
/// distance and a frequency count; `FuzzyIndex::neighbors` is deliberately
/// unranked candidate generation with no verification pass of its own (see
/// that type's own doc comment). Set-level agreement between the two is
/// checked separately, once, outside any timed code, in
/// `tests/spellcheck_fast_symspell_correctness.rs` —
/// `fuzzyindex_and_fast_symspell_agree_on_deletion_typos` — and only over
/// the restricted domain where the two engines' distance metrics provably
/// coincide (see that test's own doc comment for why transposition-typos
/// are excluded from that particular check).
fn bench_fuzzyindex_vs_fast_symspell(c: &mut Criterion) {
    const MAX_DISTANCE: u32 = 2;
    const QUERY_COUNT: usize = 200;

    let all_words = words();

    let mut construction = c.benchmark_group("spellcheck_fuzzyindex_construction");
    for &n in &CORPUS_SIZES {
        let corpus = distinct_words(&all_words, n);
        construction.throughput(Throughput::Elements(corpus.len() as u64));

        construction.bench_with_input(BenchmarkId::new("fuzzy_index", n), &corpus, |b, corpus| {
            b.iter(|| {
                let mut builder = FuzzyIndexBuilder::new();
                for w in black_box(corpus) {
                    builder.insert(w);
                }
                builder.build()
            });
        });
        construction.bench_with_input(
            BenchmarkId::new("fast_symspell", n),
            &corpus,
            |b, corpus| {
                b.iter(|| black_box(build_fast_symspell(black_box(corpus), MAX_DISTANCE as i64)));
            },
        );
    }
    construction.finish();

    let mut query = c.benchmark_group("spellcheck_fuzzyindex_query");
    for &n in &CORPUS_SIZES {
        let corpus = distinct_words(&all_words, n);
        let mut builder = FuzzyIndexBuilder::new();
        for w in &corpus {
            builder.insert(w);
        }
        let index = builder.build();
        let fast_symspell = build_fast_symspell(&corpus, MAX_DISTANCE as i64);

        let queries: Vec<&str> = corpus
            .iter()
            .take(QUERY_COUNT)
            .map(String::as_str)
            .collect();
        query.throughput(Throughput::Elements(queries.len() as u64));

        query.bench_with_input(
            BenchmarkId::new("fuzzy_index", n),
            &queries,
            |b, queries| {
                b.iter(|| {
                    let mut total = 0usize;
                    for &q in queries {
                        total += index.neighbors(black_box(q), MAX_DISTANCE).count();
                    }
                    total
                });
            },
        );
        query.bench_with_input(
            BenchmarkId::new("fast_symspell", n),
            &queries,
            |b, queries| {
                b.iter(|| {
                    let mut total = 0usize;
                    for &q in queries {
                        total += fast_symspell
                            .lookup(black_box(q), FastVerbosity::All, MAX_DISTANCE as i64)
                            .len();
                    }
                    total
                });
            },
        );
    }
    query.finish();
}

/// `spellbook`, alone, on a real `en_US` Hunspell dictionary — **never**
/// Verbora's `words.json` corpus (the `.aff`/`.dic` format cannot represent
/// it; see this file's module doc comment). Verbora is included in the same
/// group for scale, running on its *own* corpus and its *own* realistic
/// probes — the two columns answer "how long does each library take to
/// finish its own realistic task", not "which is faster at the identical
/// task", and `docs/PERFORMANCE.md`'s Spellcheck section says so explicitly
/// wherever this group's numbers are quoted.
fn bench_spellbook(c: &mut Criterion) {
    let Some(dir) = hunspell_en_us_dir() else {
        eprintln!(
            "spellcheck benches: spellbook groups skipped — en_US dictionary not found.\n\
             Fetch it with: benchmarks/competitive/scripts/fetch-models.sh hunspell-en-us"
        );
        return;
    };
    let dict = build_spellbook(&dir);
    let ascii = words();
    let sc = Spellcheck::new(&ascii[..20_000]);

    // Real English words/misspellings for spellbook's own dictionary; the
    // same shape of probe (hit / near-miss typo / far miss) as
    // `bench_is_correct`, but drawn from English vocabulary the Hunspell
    // dictionary can actually recognise.
    const REAL_WORDS: &[&str] = &[
        "hello",
        "world",
        "spelling",
        "correction",
        "example",
        "language",
        "computer",
        "keyboard",
        "algorithm",
        "dictionary",
        "sentence",
        "paragraph",
        "internet",
        "software",
        "engineer",
        "benchmark",
        "performance",
        "quality",
        "reliable",
        "accurate",
    ];
    let real_typos: Vec<String> = REAL_WORDS.iter().map(|w| typo(w)).collect();
    let real_typo_refs: Vec<&str> = real_typos.iter().map(String::as_str).collect();

    let mut g = c.benchmark_group("spellcheck_spellbook_is_correct");
    // `BenchmarkId::new`, not `bench_function("id/kind", ...)` — see
    // `bench_is_correct`'s own comment on why the latter defeats
    // `collect-results.py`'s directory discovery.
    g.bench_with_input(
        BenchmarkId::new("spellbook", "hit"),
        REAL_WORDS,
        |b, words| {
            b.iter(|| words.iter().filter(|w| dict.check(black_box(w))).count());
        },
    );
    g.bench_with_input(
        BenchmarkId::new("spellbook", "miss_near"),
        &real_typo_refs,
        |b, words| {
            b.iter(|| words.iter().filter(|w| dict.check(black_box(w))).count());
        },
    );
    // Reference column: Verbora on its own corpus, its own probes.
    let verbora_hits: Vec<&str> = ascii[..1_000].iter().map(String::as_str).collect();
    g.bench_with_input(
        BenchmarkId::new("verbora_own_corpus", "hit"),
        &verbora_hits,
        |b, words| {
            b.iter(|| words.iter().filter(|w| sc.is_correct(black_box(w))).count());
        },
    );
    g.finish();

    let mut g = c.benchmark_group("spellcheck_spellbook_suggest");
    for word in ["helo", "wrold", "korrect", "beleive"] {
        g.bench_with_input(BenchmarkId::new("spellbook", word), word, |b, w| {
            let mut out = Vec::new();
            b.iter(|| {
                out.clear();
                dict.suggest(black_box(w), &mut out);
                black_box(out.len())
            });
        });
    }
    // Reference column: Verbora's own corpus, one representative typo.
    let verbora_probe = typo(&word_of_length(&ascii, 8));
    g.bench_with_input(
        BenchmarkId::new("verbora_own_corpus", "typo8"),
        &verbora_probe,
        |b, w| {
            b.iter(|| black_box(sc.get_corrections(black_box(w), 1)));
        },
    );
    g.finish();
}

criterion_group!(
    benches,
    bench_new,
    bench_is_correct,
    bench_get_corrections,
    bench_fast_symspell_archived_load,
    bench_batch_correction,
    bench_fuzzyindex_vs_fast_symspell,
    bench_spellbook
);
criterion_main!(benches);
