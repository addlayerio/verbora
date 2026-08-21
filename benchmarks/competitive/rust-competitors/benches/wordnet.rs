//! Verbora vs. a real, pinned third-party Rust competitor — reading the
//! Princeton WordNet database (`wordnet-db` 0.1.3, johanneswd).
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.15 records **NO FAIR COMPETITOR FOUND
//! (Rust)** for this module, and that verdict was correct when it was written:
//! its candidates were `wordnet` (njaard) 0.1.2, dormant since 2017, and
//! `wordnet-ls`, archived. `wordnet-db` was first published after that pass —
//! §3's own competitor shortlist already calls it "the interesting one" — and
//! it reads the *same eight files*, from the *same directory*, and answers the
//! *same questions*. This file is that row, and §1.15's "no fair competitor"
//! line needs amending to name it. That is documentation debt this file
//! creates and cannot itself pay: `docs/` is outside this harness's ownership.
//!
//! # Why this pairing is fair, and what it is a pairing *of*
//!
//! The two crates are mechanically opposite, which is the reason to measure
//! them rather than an objection to it:
//!
//! | | `verbora-wordnet` | `wordnet-db` 0.1.3 |
//! |---|---|---|
//! | at open | reads bytes (or not — see `Storage`) | mmaps or reads **and eagerly parses every index line and every data record** into `HashMap`s |
//! | per query | binary-searches the index file, parses the record it lands on | one hash lookup, record already parsed |
//! | strings | owned `String`s in an owned `Synset` | `&str` borrowed from the mapped bytes |
//! | mmap | **cannot** — `unsafe_code = "deny"` forbids it; `Storage::LazyResident` is the declared stand-in | `LoadMode::Mmap`, the default |
//!
//! That is build-cost against query-cost: exactly the axis `benches/trie.rs`
//! already reports as separate `trie_build` and search groups, and the reason
//! this file publishes [`bench_open`] and [`bench_lookup`] **together**, plus
//! a combined one-shot [`bench_cold`]. A reader shown only one of the three
//! would draw the wrong conclusion from either.
//!
//! It also puts a number on `verbora-wordnet`'s no-`unsafe` policy. That trade
//! — no mmap, `LazyResident` instead — is today an assertion in `AGENTS.md`'s
//! "Archived Data and Memory Mapping" section with no figure attached. The
//! `verbora_lazy` and `wordnet_db_mmap` rows are what attach one.
//!
//! ## The comparable pairing, storage strategy by storage strategy
//!
//! `verbora-wordnet` offers four `Storage` strategies and `wordnet-db` two
//! `LoadMode`s, and they do not line up one for one. All six are measured, and
//! the two that answer the same question about the same mechanism are:
//!
//! * **`verbora_resident` ↔ `wordnet_db_owned`.** Both read all eight files
//!   into owned heap buffers at open, with no `unsafe` and no OS mapping. The
//!   only remaining difference is what each does with the bytes afterwards,
//!   which is the difference this file exists to measure. **This is the
//!   headline pair.**
//! * **`verbora_lazy` ↔ `wordnet_db_mmap`.** Not the same mechanism — one
//!   defers a `read`, the other defers a page fault — but they are the two
//!   crates' answers to the same question ("do not pay for what you do not
//!   touch"), and `wordnet_db_mmap` is `wordnet-db`'s *default*, so omitting
//!   it would measure a configuration its users do not run.
//! * `verbora_pread` and `verbora_indexed` have no `wordnet-db` counterpart at
//!   all and are carried for the Verbora-internal ranking, the same four rows
//!   `crates/verbora-wordnet/benches/wordnet.rs` reports in-workspace.
//!
//! ## The comparable query, and the allocation asymmetry inside it
//!
//! `wordnet-db` has no all-categories entry point: every query takes a `Pos`.
//! So the query benchmarked is **all senses of one lemma in one part of
//! speech**, which both crates express directly — `WordNet::senses(word,
//! PartOfSpeech::Noun)` on one side, `synsets_for_lemma(Pos::Noun, lemma)`
//! followed by `get_synset` for each id on the other. Verbora's
//! `WordNet::lookup` (all four categories) is deliberately *not* used: it
//! would search four index files against one hash probe, which is a
//! difference in the question asked, not in the answering.
//!
//! Both sides materialise a record per sense. They do not allocate the same
//! amount doing it — Verbora's `Synset` owns its `String`s, `wordnet-db`'s
//! `Synset<'a>` borrows `&str` out of the mapped buffer and allocates only the
//! `Vec<Lemma>`/`Vec<Pointer>` spines. That asymmetry is *intrinsic to the two
//! designs* (borrowing is what holding the whole file resident buys you), not
//! a shortcut handed to one side by this harness, so it stays in and is
//! reported here rather than adjusted away. The mirror-image asymmetry is on
//! the other side: `wordnet-db` allocates a `String` per query inside
//! `normalize_lemma`, where Verbora's `index_key` is the identity on an
//! already-legal key.
//!
//! A third group, [`bench_index_entry`], isolates the *search* from the
//! materialisation: `index_entry` on both sides returns the offsets and sense
//! counts without reading a data record at all. It is the most direct
//! binary-search-versus-hash row in the file.
//!
//! # A Verbora defect narrows this benchmark's domain
//!
//! Installing a real Princeton distribution to run this file turned up a bug
//! in `crates/verbora-wordnet`, **since fixed**. Recorded because the mechanism
//! is worth not rediscovering, and because it is why this file's probe list
//! looks the way it does:
//!
//! > `PointerSymbol::from_symbol` accepted the two-character domain pointers
//! > `;c`, `;r`, `;u`, `-c`, `-r`, `-u`, but **not** the bare `;` and `-`.
//! > Princeton's `index.*` files write the bare forms — the class letter
//! > appears only in `data.*` — so `WordNet::index_entry`, `senses`, `sense`
//! > and `lookup` all failed with
//! > `Error::MalformedIndexEntry { kind: InvalidField { field: "ptr_symbol",
//! > value: ";" } }` on any lemma whose pointer list includes one.
//!
//! Measured over the shipped files: **13,606 of 155,467 index entries in
//! WordNet 3.1 (8.8%)** and **13,488 of 155,287 in 3.0 (8.7%)** — including
//! `run`, `cat`, `light`, `water`, `computer`, `node`, `house`, `hand`,
//! `idea`, `music`, `river`, `bird` and `new_york`. `data.*` records are
//! unaffected: `verbora-wordnet`'s own
//! `every_synset_of_the_real_dictionary_parses` passes on both releases, and
//! only `every_entry_of_the_real_dictionary_is_reachable` fails — at the first
//! affected line, byte 1740 of `index.noun`. That test exists and is correct;
//! it is `#[ignore]`d for want of the separately-licensed database, so nothing
//! had ever run it against real data. `wordnet-db` parses these entries
//! without complaint, which is how the disagreement surfaced.
//!
//! Nothing here ever worked around it — no retry, no catch, no substitute
//! parser. While it was open, the probe list avoided the affected 8.8% and
//! `slip` (15 noun senses) stood in for `run` (16) as the highest-sense-count
//! probe, so the rows would have described only the 91.2% Verbora could read.
//! With the parser fixed, `run` is back and the domain is the whole dictionary.
//! `../tests/wordnet_correctness.rs`'s
//! `both_parsers_agree_on_the_formerly_unreadable_entries` holds it there, by
//! asserting the two parsers return the same sense counts for exactly the
//! lemmas that used to diverge.
//!
//! # The dictionary is separately licensed and is not vendored
//!
//! WordNet is Princeton University's, under its own licence (reproduced in
//! `crates/verbora-wordnet/LICENSE-WORDNET`); this repository ships none of
//! it, for the same reason `crates/verbora-wordnet` ships none of it. Run
//! `../../scripts/fetch-models.sh wordnet-en` once — it installs **3.1**, the
//! release `docs/COMPETITIVE_BENCHMARKS.md` §1.15 pins the reference side to —
//! or point `$WORDNET_DB_PATH` at an existing `dict` directory. 3.0 is
//! accepted too; synset offsets differ between the releases, so whichever is
//! found is used for both crates, never one each. Every group below skips
//! cleanly with a printed notice if it is absent — a missing licence-
//! restricted asset must never fail `cargo bench` for everyone else's groups.
//!
//! # Not benchmarked, and why
//!
//! * **`thesaurus` 0.5.2 (grantshandy) — synonym lookup.** §3's shortlist
//!   proposes it as the strongest WordNet-adjacent competitor by adoption, and
//!   the operation it proposes is right: a WordNet synset *is* a synonym set,
//!   so "give me the synonyms of `dog`" is a question both crates can answer.
//!   Reading `thesaurus-0.5.2/src/lib.rs` before writing the bench is what
//!   killed it. `synonyms(word)` calls `dict()`, and `dict()` deep-clones the
//!   entire `HashMap<String, Vec<String>>` — 125,701 keys averaging 3.4
//!   synonyms — on **every call**; without the `static` feature it
//!   re-decompresses and re-parses the embedded corpus instead. There is no
//!   borrowing accessor in the published API. A timing row would therefore
//!   measure `HashMap::clone` against a WordNet index search, which reports a
//!   defect in one crate as a speed difference between two. Hoisting `dict()`
//!   out of the timed region was considered and rejected on the `eddie`
//!   precedent (`../Cargo.toml`, and `../../README.md`'s "Resolved: `eddie`
//!   0.4.2 is unsound" section): a timing row calls the published API as
//!   published, and lifting a cost out of one side's loop is that same
//!   manoeuvre pointed the other way. The crate is not pinned at all, so the
//!   reasoning lives in `../Cargo.toml` beside where the pin would have gone.
//!   **This is a statement about 0.5.2's API, not about the comparison** — a
//!   release exposing a non-cloning lookup makes the synonym row fair
//!   immediately, and it should then be added.
//! * **Pointer traversal and closures** (`hypernym`, `closure`). `wordnet-db`
//!   parses pointers and hands back a `Vec<Pointer>`, but has no traversal
//!   entry point — no "follow this pointer", no transitive closure — so a
//!   `closure` row would have only a Verbora side. §1.15's own note already
//!   says as much of every candidate examined. It is measured without a
//!   competitor in `crates/verbora-wordnet/benches/wordnet.rs`'s `stages`
//!   group.
//! * **`wordnet` (njaard) 0.1.2** — excluded by §1.15 on maintenance and
//!   adoption grounds (dormant since 2017-10-22, 11 stars). Nothing found in
//!   this pass changes that.

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use verbora_wordnet::{Config, PartOfSpeech, Storage, WordNet};
use wordnet_db::{LoadMode, WordNet as DbWordNet};
use wordnet_types::Pos;

/// The four `verbora-wordnet` strategies, in the order
/// `crates/verbora-wordnet/benches/wordnet.rs` lists them, so the two suites'
/// tables read the same way round.
const VERBORA_STRATEGIES: &[(&str, Storage)] = &[
    ("verbora_pread", Storage::Pread),
    ("verbora_lazy", Storage::LazyResident),
    ("verbora_resident", Storage::Resident),
    ("verbora_indexed", Storage::Indexed),
];

/// `wordnet-db`'s two load modes. `Mmap` is its default and `Owned` is the
/// like-for-like partner of `Storage::Resident` — see the module doc comment's
/// pairing table.
const DB_MODES: &[(&str, LoadMode)] = &[
    ("wordnet_db_mmap", LoadMode::Mmap),
    ("wordnet_db_owned", LoadMode::Owned),
];

/// Probe lemmas, chosen for the shapes a noun lookup can take rather than at
/// random: a one-sense entry, a mid-sized one, the highest-sense-count lemma
/// **that `verbora-wordnet` can read at all**, and a miss.
///
/// `run` is the highest-sense-count noun at 16, and is the probe the
/// in-workspace bench uses. It was unusable here until `verbora-wordnet`
/// learned the bare `;` — one of the 13,606 entries it could not read — and
/// `slip` (15 senses) stood in for it. Both are kept: `slip` because the rows
/// measured against it stay comparable, `run` because it is the real ceiling.
///
/// Every probe here was checked against both WordNet 3.0's and 3.1's
/// `index.noun`: same sense counts, same pointer lists, no bare symbol in
/// either release, so the list does not silently depend on which one is
/// installed.
const PROBES: &[&str] = &["entity", "dog", "slip", "run", "zzzzz"];

/// The dictionary directory, or `None` when it has not been installed.
///
/// Checks the two variables `crates/verbora-wordnet/benches/wordnet.rs`
/// already honours, then this harness's own fetched-asset location, so a
/// developer who has set either up for the in-workspace suite needs no second
/// setup for this one.
fn dict_dir() -> Option<PathBuf> {
    for var in ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            let dir = PathBuf::from(value);
            if dir.join("index.noun").is_file() {
                return Some(dir);
            }
        }
    }
    // `scripts/fetch-models.sh wordnet-en` installs 3.1 (the release
    // `docs/COMPETITIVE_BENCHMARKS.md` §1.15 pins the reference side to);
    // 3.0 is accepted so an existing install is not made useless by the
    // default changing. Synset offsets differ between the two releases, so
    // whichever is found is used for BOTH crates — never one each.
    let models = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models");
    for release in ["wordnet-3.1", "wordnet-3.0"] {
        let fetched = models.join(release).join("dict");
        if fetched.join("index.noun").is_file() {
            return Some(fetched);
        }
    }
    None
}

fn skip_notice() {
    eprintln!(
        "wordnet benches skipped: no WordNet dictionary found.\n\
         It is separately licensed (Princeton University) and not vendored.\n\
         Fetch it with: benchmarks/competitive/scripts/fetch-models.sh wordnet-en\n\
         or point $WORDNET_DB_PATH at a directory holding `index.noun` and its\n\
         seven siblings."
    );
}

/// `wordnet-db`'s loader is fallible on a real dictionary in ways this harness
/// cannot check ahead of time (a `?` on any malformed record aborts the whole
/// load). A failure is reported and the group is skipped, never `unwrap`ped:
/// one competitor that cannot load its data must not take down a campaign that
/// has already measured the other rows.
fn load_db(dict: &Path, mode: LoadMode) -> Option<DbWordNet> {
    match DbWordNet::load_with_mode(dict, mode) {
        Ok(db) => Some(db),
        Err(e) => {
            eprintln!("wordnet benches: wordnet-db {mode:?} failed to load the dictionary: {e:#}");
            None
        }
    }
}

/// All noun senses of `lemma`, as records — `wordnet-db`'s side of the
/// [`bench_lookup`] query.
///
/// Two calls rather than one because the crate splits them: ids first, then a
/// record per id. Returning the count keeps the parsed views alive to the end
/// of the closure without materialising a `Vec` that Verbora's side does not
/// build either.
fn db_senses(db: &DbWordNet, lemma: &str) -> usize {
    db.synsets_for_lemma(Pos::Noun, lemma)
        .iter()
        .filter_map(|id| db.get_synset(*id))
        .count()
}

/// Startup: what a process pays before it can answer anything.
///
/// `sample_size(10)`, not Criterion's default 100. `wordnet-db`'s `load`
/// parses the whole database on every iteration, so the default would spend
/// the better part of an hour in this one group — and per `CLAUDE.md`'s
/// benchmarking rule, a campaign's wall-clock budget is the binding
/// constraint. Ten samples of a multi-hundred-millisecond operation still
/// gives Criterion a usable distribution; ten samples of a nanosecond one
/// would not, which is why only this group and [`bench_cold`] are reduced.
fn bench_open(c: &mut Criterion) {
    let Some(dict) = dict_dir() else {
        skip_notice();
        return;
    };
    let mut g = c.benchmark_group("wordnet_open");
    g.sample_size(10);
    for (name, storage) in VERBORA_STRATEGIES {
        g.bench_function(*name, |b| {
            b.iter(|| WordNet::open_with(black_box(&dict), &Config::new(*storage)).unwrap());
        });
    }
    for (name, mode) in DB_MODES {
        if load_db(&dict, *mode).is_none() {
            continue;
        }
        g.bench_function(*name, |b| {
            b.iter(|| DbWordNet::load_with_mode(black_box(&dict), *mode).unwrap());
        });
    }
    g.finish();
}

/// Open plus one query: the honest total for a one-shot process, and the row
/// where an eager full parse is charged for what it bought.
///
/// Not derivable by adding [`bench_open`] and [`bench_lookup`]: those measure
/// a *warm* dictionary, and `Storage::Pread`/`LazyResident` do their first
/// real work here rather than at open.
fn bench_cold(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("wordnet_cold");
    g.sample_size(10);
    for (name, storage) in VERBORA_STRATEGIES {
        g.bench_function(*name, |b| {
            b.iter(|| {
                let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
                wn.senses(black_box("entity"), PartOfSpeech::Noun).unwrap()
            });
        });
    }
    for (name, mode) in DB_MODES {
        if load_db(&dict, *mode).is_none() {
            continue;
        }
        g.bench_function(*name, |b| {
            b.iter(|| {
                let db = DbWordNet::load_with_mode(&dict, *mode).unwrap();
                db_senses(&db, black_box("entity"))
            });
        });
    }
    g.finish();
}

/// Steady-state per-query latency on a warm dictionary: all noun senses of one
/// lemma, materialised as records.
///
/// Every implementation is warmed over [`PROBES`] before it is measured, so
/// `Storage::LazyResident`'s first touch lands outside the timed region and
/// this group measures steady state for all six rows rather than steady state
/// for five and a first read for one.
fn bench_lookup(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("wordnet_lookup");
    for (name, storage) in VERBORA_STRATEGIES {
        let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
        for w in PROBES {
            let _ = wn.senses(w, PartOfSpeech::Noun);
        }
        for word in PROBES {
            g.bench_with_input(BenchmarkId::new(*name, word), word, |b, w| {
                b.iter(|| wn.senses(black_box(w), PartOfSpeech::Noun).unwrap());
            });
        }
    }
    for (name, mode) in DB_MODES {
        let Some(db) = load_db(&dict, *mode) else {
            continue;
        };
        for w in PROBES {
            let _ = db_senses(&db, w);
        }
        for word in PROBES {
            g.bench_with_input(BenchmarkId::new(*name, word), word, |b, w| {
                b.iter(|| db_senses(&db, black_box(w)));
            });
        }
    }
    g.finish();
}

/// The search alone: offsets and sense counts, no data record read.
///
/// The most direct binary-search-versus-hash row in this file — Verbora walks
/// the sorted index file the way `wndb(5WN)` specifies it may be walked;
/// `wordnet-db` probes a `HashMap` it built at open. Both entry points are
/// called `index_entry` and both return the same information, which is as
/// close to a like-for-like pair as two independently written crates get.
fn bench_index_entry(c: &mut Criterion) {
    let Some(dict) = dict_dir() else { return };
    let mut g = c.benchmark_group("wordnet_index_entry");
    for (name, storage) in VERBORA_STRATEGIES {
        let wn = WordNet::open_with(&dict, &Config::new(*storage)).unwrap();
        for w in PROBES {
            let _ = wn.index_entry(w, PartOfSpeech::Noun);
        }
        for word in PROBES {
            g.bench_with_input(BenchmarkId::new(*name, word), word, |b, w| {
                b.iter(|| wn.index_entry(black_box(w), PartOfSpeech::Noun).unwrap());
            });
        }
    }
    for (name, mode) in DB_MODES {
        let Some(db) = load_db(&dict, *mode) else {
            continue;
        };
        for w in PROBES {
            let _ = db.index_entry(Pos::Noun, w);
        }
        for word in PROBES {
            g.bench_with_input(BenchmarkId::new(*name, word), word, |b, w| {
                b.iter(|| db.index_entry(Pos::Noun, black_box(w)));
            });
        }
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_open,
    bench_cold,
    bench_lookup,
    bench_index_entry
);
criterion_main!(benches);

// CORRECTNESS BEFORE PERFORMANCE: see `../tests/wordnet_correctness.rs`, not a
// `#[cfg(test)] mod` in this file — a Criterion `[[bench]]` target compiles
// with `harness = false`, so an in-file `#[test]` would be dead code `cargo
// test` never invokes. That file asserts the two crates agree on what they
// find in the dictionary before any timing number from this file is trusted,
// and skips (rather than fails) when the dictionary is not installed.
