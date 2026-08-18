# WordNet

`verbora-wordnet` reads the Princeton WordNet lexical database and turns its
`index.*` / `data.*` files into synsets, definitions and relational pointers.

Two behaviours will surprise you if you assume otherwise: the index search is an
**incomplete** bisection that reports "not found" for some lemmas that are in the
file, and every traversal returns results in **reverse** of the order the files
list them. Both are pinned by the crate's test suite, and both are explained
below.

<div class="callout callout-spec">
<strong>Specification status.</strong> <code>lookup</code>, <code>get</code>,
<code>lookup_synonyms</code>, <code>get_synonyms</code>, the index bisection's
probe sequence and the reader's handling of CRLF line endings, missing gloss
separators and non-ASCII keys are all documented and test-pinned, with no
external data required. <code>cargo test -p verbora-wordnet</code> runs
<strong>54</strong> unit tests and <strong>14</strong> doctests.
</div>

## The database is separately licensed

`verbora` is MIT. **The WordNet database is not.** This crate ships **no
dictionary data at all** — none of the roughly 28 MB of index and data files
WordNet 3.0/3.1 consists of. It reads them at run time from a directory you
supply. The database is covered by Princeton University's own licence,
reproduced verbatim in `LICENSE-WORDNET` beside the crate; it requires the notice
to accompany all copies, including modifications, and forbids using Princeton's
name in advertising.

Download any WordNet 3.0 or 3.1 database from Princeton, then point
`WordNet::open` at the `dict` directory it contains — or set an environment
variable and use `WordNet::from_env`:

```text
export WORDNET_DB_PATH=/path/to/wordnet/dict
```

`WordNet::from_env` checks `$WORDNET_DB_PATH`, then `$VERBORA_WORDNET_DICT`
(this crate's own override), then one conventional relative path, and fails with
`Error::Io` naming every candidate it tried.

Snippets on this page that need the real database are fenced `no_run`. The
runnable ones build a tiny, hand-written dictionary in the WordNet text format —
the same trick the crate's own unit tests use.

## When to use it

- **Synonym, definition and relation lookup for English words**, when you already
  have (or can install) the database and do not need it embedded in your binary.
- **Walking the hypernym/hyponym/meronym graph.** `relation` gives you one hop;
  `closure` walks the whole transitive chain lazily, with cycle protection.
- **A long-lived process serving concurrent lookups.** `WordNet` is immutable
  after construction and `Send + Sync`; share one `Arc<WordNet>` across threads
  with no locking. See [Concurrency](#concurrency).

## When not to use it

- **You cannot ship or install the database.** There is no bundled fallback; every
  method that needs a missing file returns `Error::Io` at open, not later.
- **You need every WordNet entry to be reachable.** The index search is not
  merely slow, it is **incomplete** — see
  [The index search is incomplete](#the-index-search-is-incomplete). A word
  genuinely in the database can still come back as a miss.
- **You want results in dictionary or alphabetical order.** Every traversal comes
  back in `pop()`-driven reverse order. See
  [Result order is reversed](#result-order-is-reversed).
- **You want stemming, POS tagging, or a general thesaurus API.** This crate is
  the lexical database reader only. Pair it with [Inflectors](./inflectors) or
  your own normalisation if the input is not already a WordNet headword.

## Quick example

Two synsets, `alpha` and `beta`, with `alpha` pointing at `beta` as its hypernym.
No download required.

```rust
use verbora_wordnet::{WordNet, pointer};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-quick-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let index = "aaa n 1 0 1 0 00000000  \nbbb n 2 1 @ 2 0 00000000 00000083  \nccc n 1 0 1 0 00000083  \n";
    let data = "00000000 06 n 01 alpha 0 001 @ 00000083 n 0000 | the first letter; \"as in alpha\"  \n00000083 06 n 02 beta 0 second 1 000 | the second letter  \n";
    for pos in ["noun", "verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), index).unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), data).unwrap();
    }
    dir
}

fn main() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();

    let alpha = wn.get(0.0, "n").unwrap();
    assert_eq!(alpha.lemma.as_deref(), Some("alpha"));
    assert_eq!(alpha.def, "the first letter");
    assert_eq!(alpha.exp, ["as in alpha"]);

    // One pointer hop from alpha reaches beta.
    let synonyms = wn.get_synonyms_of(&alpha).unwrap();
    assert_eq!(synonyms[0].lemma.as_deref(), Some("beta"));

    assert_eq!(wn.relation(&alpha, pointer::HYPERNYM).count(), 1);
    assert_eq!(wn.relation(&alpha, pointer::HYPONYM).count(), 0);

    std::fs::remove_dir_all(&dir).ok();
}
```

Against the real database:

```rust no_run
use verbora_wordnet::{WordNet, pointer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("/path/to/wordnet/dict")?;

    for synset in wn.lookup("bank")? {
        println!("{:?}  {}", synset.lemma, synset.def);
    }

    // Walk up the hypernym chain from one specific noun sense.
    let sense = wn.get(3_832_647.0, "n")?;
    for parent in wn.closure(&sense, pointer::HYPERNYM).take(5) {
        println!("^ {}", parent?.def);
    }
    Ok(())
}
```

## Choosing a `Storage` strategy

The four strategies change only how the dictionary's bytes physically arrive.
None changes a single answer — the crate's own tests assert that the same lookup
against the same dictionary agrees across all four — which is what lets `Storage`
be a runtime choice rather than a type parameter.

| `Storage` | Startup | Per query | Resident memory | Pick it when |
|---|---|---|---|---|
| `Pread` | none | a handful of positioned syscalls | none | short-lived process, one or two lookups (a CLI tool) |
| `LazyResident` | none | in-memory, once a file is first touched | grows to whichever files were used | long-lived process that may never touch some of the eight files |
| `Resident` *(default)* | reads ~28 MB | in-memory scan | ~28 MB | long-lived process querying broadly, where predictable per-query latency beats a fast first request |
| `Indexed` | + one `memchr` pass over the resident bytes | `partition_point` over a `u32` line-start table | + ~4 bytes per line (~470 KiB for `index.noun`) | hot path with many repeated lookups, where the bisection shows up in a profile |

```rust no_run
use verbora_wordnet::{Config, Storage, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::from_env()?;
    let indexed = WordNet::open_with("/path/to/wordnet/dict", &Config::new(Storage::Indexed))?;
    // Same answers either way.
    assert_eq!(wn.lookup("entity")?, indexed.lookup("entity")?);
    Ok(())
}
```

There is deliberately no memory-mapped fifth strategy: `mmap` cannot be reached
from safe Rust without an extra dependency or `unsafe`, and this crate admits
neither. `LazyResident` covers the case `mmap` is usually reached for — near-zero
startup, in-memory cost once a file is touched — at the honest cost of paying for
the whole file the first time any part of it is read.

### The `PrebuiltIndex` sidecar

`Storage::Indexed` normally builds its line-start tables with one `memchr` pass
at open time. `PrebuiltIndex` persists those line offsets to a file once, so every
later open loads them instead of re-scanning ~28 MB for newlines.

The dictionary text files remain the only source of truth: the sidecar carries no
lemmas, glosses or content-derived offsets — only each line's byte position, plus
the length of the file it was built from. `PrebuiltIndex::source_for` refuses an
entry whose file has since changed size, because the bisection's probe positions
are a function of file length. The builder is a pure function of the eight
dictionary files — same bytes in, same sidecar out, no timestamps — so it can be
built once in CI and shipped alongside the dictionary.

```rust no_run
use verbora_wordnet::{Config, PrebuiltIndex, Storage, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = "/path/to/wordnet/dict";
    let sidecar = PrebuiltIndex::default_path(dir);
    PrebuiltIndex::build(dir)?.save(&sidecar)?;

    // Same answers, whether the tables were scanned just now or loaded from disk.
    let scanned = WordNet::open_with(dir, &Config::new(Storage::Indexed))?;
    let loaded = WordNet::open_with(dir, &Config::default().with_prebuilt(&sidecar))?;
    assert_eq!(scanned.lookup("entity")?, loaded.lookup("entity")?);
    Ok(())
}
```

## Traversal: eager vs lazy

Every method performs strictly sequential, synchronous I/O and returns its result
directly.

| API | Answers | Lazy | Output | Allocates |
|---|---|:--:|---|---|
| `lookup` | every synset for a word, all parts of speech | ❌ | `Vec<DataRecord>` | one `Vec`, one `DataRecord` per synset |
| `lookup_iter` | the same synsets | ✅ | `LookupIter` → `Result<DataRecord>` | one `DataRecord` per synset actually read |
| `par_lookup_batch` | `lookup`, fanned out over many words | ❌ | `Vec<Result<Vec<DataRecord>>>` | one outer `Vec`, plus `lookup`'s own per word |
| `pointers` | every synset one hop from a record | ✅ | `Pointers` → `Result<DataRecord>` | one `DataRecord` per hop followed |
| `relation` | `pointers`, filtered to one relation symbol | ✅ | `Pointers` → `Result<DataRecord>` | as `pointers`, minus the hops skipped |
| `closure` | the whole transitive chain of one relation | ✅ | `Closure` → `Result<DataRecord>` | one `DataRecord` per synset visited, plus a `VecDeque` queue and a seen-offsets `Vec` |
| `get_synonyms_of` | `pointers`, eager and **re-read from disk** | ❌ | `Vec<DataRecord>` | as `pointers`, plus the re-read |

Three things are easy to miss:

- **`lookup` is exactly `lookup_iter(word).collect()`.** `"run"` has 57 senses in
  the real database, so collecting all of them to take the first two is 55 line
  reads you did not need. `lookup_iter`'s errors are sticky: once one lookup
  fails, later `next()` calls return `None` rather than re-reporting forever.
- **`get_synonyms_of` re-reads the starting synset from disk** by offset and tag
  before following its pointers, so a record you mutated after reading is not
  what gets followed. Use `pointers` when you want the pointers already in hand.
- **`closure` is the only one that walks more than one hop.** It is
  breadth-first, never yields the starting record, and is cycle-safe: an offset
  already emitted is never queued again, tracked as the bit pattern of the `f64`
  offset so a `NaN` offset compares by identity rather than by IEEE 754's "`NaN`
  never equals anything", which would otherwise defeat the cycle check.

`get_synonyms(offset, tag)` and `get_synonyms_of(&record)` are the same operation
from two starting points, given two names rather than one overloaded function, so
an offset of `0.0` is never mistaken for "no offset given".

```rust no_run
use verbora_wordnet::{WordNet, pointer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("/path/to/wordnet/dict")?;
    let sense = wn.get(3_832_647.0, "n")?;

    // One hop of one relation.
    for parent in wn.relation(&sense, pointer::HYPERNYM) {
        println!("^ {}", parent?.def);
    }
    // The whole chain, lazily, one read at a time.
    for ancestor in wn.closure(&sense, pointer::HYPERNYM) {
        println!("^^ {}", ancestor?.def);
    }
    Ok(())
}
```

### `par_lookup_batch`

Behind the `parallel` Cargo feature, `par_lookup_batch` is exactly
`words.par_iter().map(|w| self.lookup(w)).collect()` — a thin fan-out over the
same sequential `lookup`, input order preserved, each element carrying its own
`Result`. `WordNet` is already immutable and `Send + Sync` with nothing cached or
locked per query, so it needed no new synchronization.

Measured on `benches/wordnet.rs`'s `par_lookup_batch` group (`Storage::Resident`,
32 hardware threads, the same 16-word mix repeated out to each size):

| Batch size | Sequential | Parallel | Speedup |
|--:|--:|--:|--:|
| 16 | 633.5 µs | 600.6 µs | ~1.05× (noise-level) |
| 160 | 7.03 ms | 2.10 ms | ~3.3× |
| 1600 | 100.2 ms | 24.95 ms | ~4.0× |

A batch of 16 common words sits close to break-even: a `rayon` task costs about a
microsecond to schedule, comparable to a single lookup's own cost (~5.7–6.9 µs
for a common entry like `entity`, up to ~150–206 µs for a high-sense-count word
like `run`). Prefer a plain `.iter().map(WordNet::lookup)` loop at that scale;
from a few hundred words up, the win is real. See
[Parallelism](../performance/parallelism).

```rust  ignore
let results = wn.par_lookup_batch(&["run", "entity", "zzzzz"]);
for r in results {
    match r {
        Ok(synsets) => println!("{} senses", synsets.len()),
        Err(e) => println!("lookup failed: {e}"),
    }
}
```

## Result order is reversed

Every traversal drains its work list from the back, so results arrive reversed at
two levels: parts of speech are consulted **adv, adj, verb, noun**, and within one
part of speech the index line's offsets are visited **last to first**.
`lookup("fast")` therefore yields `r:86892 r:86488 s:324771 … n:1071904` against
the real database. The order is pinned by value in the crate's test suite.

```rust
use verbora_wordnet::WordNet;

fn multi_pos_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-order-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let noun_a = "00000000 06 n 01 x 0 000 | noun sense A  \n".to_string();
    let noun_b_offset = noun_a.len();
    let noun_b = format!("{noun_b_offset:08} 06 n 01 x 0 000 | noun sense B  \n");
    std::fs::write(dir.join("data.noun"), format!("{noun_a}{noun_b}")).unwrap();
    std::fs::write(
        dir.join("index.noun"),
        format!("x n 2 0 2 0 00000000 {noun_b_offset:08}  \n"),
    )
    .unwrap();

    std::fs::write(dir.join("data.verb"), "00000000 06 v 01 x 0 000 | verb sense  \n").unwrap();
    std::fs::write(dir.join("index.verb"), "x v 1 0 1 0 00000000  \n").unwrap();

    for pos in ["adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), "").unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), "").unwrap();
    }
    dir
}

fn main() {
    let dir = multi_pos_dict();
    let wn = WordNet::open(&dir).unwrap();

    // Verb before noun, and within noun, sense B (the SECOND offset on the
    // index line) before sense A.
    let defs: Vec<String> = wn
        .lookup("x")
        .unwrap()
        .into_iter()
        .map(|r| r.def.trim_end().to_owned())
        .collect();
    assert_eq!(defs, ["verb sense", "noun sense B", "noun sense A"]);

    // lookup_iter is lazy: the FIRST result costs exactly one line read, and it
    // is the verb sense — reading it never touches data.noun at all.
    let first: Vec<String> = wn
        .lookup_iter("x")
        .take(1)
        .map(|r| r.unwrap().def.trim_end().to_owned())
        .collect();
    assert_eq!(first, ["verb sense"]);

    // Sense NUMBERS run forwards over the index line, the opposite direction.
    assert_eq!(wn.find_sense("x#n#1").unwrap().unwrap().def.trim_end(), "noun sense A");
    let senses: Vec<String> =
        wn.query_sense("x#n").unwrap().iter().map(|s| s.to_string()).collect();
    assert_eq!(senses, ["x#n#1", "x#n#2"]);

    std::fs::remove_dir_all(&dir).ok();
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Sense numbers run <strong>forwards</strong> over the
index line's own order — sense 1 is the first offset written on the line — which
is the <strong>opposite</strong> of the order <code>lookup</code> yields for that
part of speech. Numbering results as <code>lookup</code> hands them to you would
number every word backwards. Use <code>query_sense</code> /
<code>find_sense</code>, which parse and resolve a <code>lemma#pos[#n]</code>
string, rather than the position a synset arrives in.
</div>

## The index search is incomplete

`IndexFile::find` bisects **byte positions**, not lines. Each probe snaps
backwards to the start of the line it landed in, compares that line's first token
to the search key, and halves the step — and because the snap-back changes which
line a position denotes, the invariant a binary search needs does not hold.
Measured against the shipped WordNet 3.1 database:

| File | Lemmas | Reported missing |
|---|---:|---:|
| `index.adv` | 4,475 | 20 |
| `index.verb` | 11,540 | 183 |
| `index.adj` | 21,499 | 117 |
| `index.noun` | 117,953 | 624 |

`index.verb` misses the **entire head of the file** — `aah`, `abandon`, `abase`,
`abate` are all reported missing — and `awful`, `safely`, `such`, `bitter` and
`firm` are adverbs this search cannot find. The probe sequence is pinned by the
test suite, so which lemmas are reachable is stable and reproducible. Treat a miss
as "this search could not reach the word", never as "the word is absent"; if you
need guaranteed coverage, scan the index file yourself or keep your own lemma
list.

## Concurrency

`WordNet`, `IndexFile` and `DataFile` are immutable after construction and
`Send + Sync`. Nothing is cached per query and nothing is locked, so any number
of threads can query one shared dictionary concurrently with no coordination:

```rust no_run
use std::sync::Arc;
use verbora_wordnet::WordNet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = Arc::new(WordNet::open("/path/to/wordnet/dict")?);

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let wn = Arc::clone(&wn);
            std::thread::spawn(move || wn.lookup("entity").unwrap().len())
        })
        .collect();
    for h in handles {
        println!("{} senses", h.join().unwrap());
    }
    Ok(())
}
```

This is the same "share one `Arc`, fan out read-only queries" pattern
[Parallelism](../performance/parallelism) describes: open the dictionary once,
wrap it in an `Arc`, and drive queries from as many threads as you like — with
your own `rayon` iterator over any traversal above, or with
[`par_lookup_batch`](#par-lookup-batch) for the one that ships a built-in
fan-out.

## Error behaviour <span class="badge badge-fallible">FALLIBLE</span>

Every method that reaches malformed or unusual on-disk data returns a specific
`Error` variant instead of panicking, hanging, or looping unboundedly:

| Condition | Result |
|---|---|
| An unopenable dictionary file | `Error::Io`, at open rather than at the first query |
| An offset that lands past EOF | `Error::UnterminatedLine` |
| A record with no `'\| '` gloss separator | `Error::MissingGloss` |
| An absurdly large word or pointer count | `Error::CountTooLarge`, refused beyond `numfmt::MAX_COUNT` |
| A part-of-speech tag that is not `n`/`v`/`a`/`s`/`r` | `Error::UnknownPos` — case-sensitive, no `"noun"`, no default |
| A negative probe position | `Error::NegativeProbe` — verified unreachable across all 147,580 keys the fixture exercises, but reported rather than silently clamped |

A file's length is recorded once at open, so a file that changes size mid-session
does not change the search path; descriptors are held for the process lifetime
(or not at all, for `Storage::Resident`) rather than opened per operation.

## Allocation behaviour

- **At open.** `Resident` and `Indexed` each read the whole file into one
  `Box<[u8]>` per file (eight files); `Indexed` adds one `u32` per line. `Pread`
  and `LazyResident` allocate nothing beyond the `File` handles —
  `LazyResident`'s buffer is allocated the first time that file is queried.
- **Per query, eager.** `get` and `lookup` return owned `DataRecord`s: one
  `String` per textual field, one `Vec` each for `synonyms`, `ptrs` and `exp`.
- **Per query, borrowed** <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>**.**
  `DataFile::with_record` hands a `DataRecordRef` to a closure instead: every
  string field except the cleaned examples is a subslice of the line being
  parsed, so reading a synset allocates only for examples containing a quote or a
  whitespace run needing cleanup. This is the primitive `DataFile::get` is built
  on.
- **Traversal.** `pointers` and `relation` allocate nothing beyond the
  `DataRecord` each hop reads. `closure` adds a `VecDeque<Pointer>` queue and a
  `Vec<u64>` of visited offsets, both bounded by the reachable subgraph.
- **`IndexFile::find`.** Each probe allocates one `String` for the line it reads
  (or reuses a scratch buffer on the positioned-read backends); nothing is
  retained but the winning line.

There is no `_into` variant and no caller-supplied output buffer anywhere in this
crate. See [Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Performance characteristics

`crates/verbora-wordnet/benches/wordnet.rs` is a Criterion suite comparing the
four `Storage` strategies across five dimensions: `open` (startup cost), `cold`
(open plus one lookup), `lookup` (steady-state per-query latency), `repeat`
(throughput over a realistic word list) and `footprint` (resident bytes). These
benches need the real database and skip cleanly when `$WORDNET_DB_PATH` is unset.
Reproduce with `cargo bench -p verbora-wordnet`; see
[Benchmarks](../benchmarks/index) for results across the workspace.

## Unicode and language notes

- **String comparison during the bisection is UTF-16 code-unit order**
  <span class="badge badge-utf16">UTF-16</span> (`whitespace::value_lt`), not
  Rust's UTF-8 byte `Ord`. The two disagree for supplementary-plane characters,
  which decides which way a probe turns.
- **Lookup normalisation is full Unicode lowercasing**, not
  `str::to_ascii_lowercase`: `'İSTANBUL'` lowercases to nine code units from
  eight, and `'ΟΔΟΣ'` produces a final sigma. Whitespace runs — a set that
  excludes U+0085 and includes U+FEFF — collapse to a single `_`, so
  `"  entity  "` becomes `"_entity_"`, which then misses.
- **Line splitting on `/\s+/` keeps empty edge fields**, which is what makes
  `find("")` a hit on a licence-header line: the header starts with two spaces,
  so its first token is `""`.
- **Decoding is lossy, not fallible.** A line's bytes are converted with
  `String::from_utf8_lossy`, substituting U+FFFD for invalid bytes.
- **Reported paths are normalised.** `IndexFile::file_path` and
  `DataFile::file_path` collapse `.`, `..` and duplicate separators, which plain
  [`Path::join`](https://doc.rust-lang.org/std/path/struct.Path.html#method.join)
  does not.

## Common mistakes

- **Assuming the database ships with the crate.** It does not, ever.
  `WordNet::open` on a missing directory fails immediately with `Error::Io` — no
  partial dictionary, no silent stall.
- **Passing a `Pos` name instead of its one-letter tag.** `get` and
  `get_synonyms` take `"n"`, `"v"`, `"a"`, `"s"` or `"r"`; `"noun"` and `"N"`
  both return `Error::UnknownPos`. `get_at` takes a typed `Pos` and cannot fail
  to resolve one.
- **Treating a miss as proof the word is not in the database.** The bisection is
  incomplete by construction — see
  [The index search is incomplete](#the-index-search-is-incomplete).
- **Expecting `lookup` or `get_synonyms` to follow the index line's order.** Both
  reverse it; number senses with `query_sense`, not by arrival position.
- **Expecting `get_synonyms_of` to reuse your record's own pointers.** It re-reads
  the synset from disk. Use `pointers` for the value in hand.
- **Mixing up `get`'s two error paths.** `Error::UnknownPos` means the *tag* was
  wrong; `Error::MissingGloss` and `Error::UnterminatedLine` mean the tag resolved
  but the *offset* landed somewhere that is not a well-formed record start.

```rust
use verbora_wordnet::{Error, WordNet};

fn main() {
    assert!(matches!(WordNet::open("/no/such/dir"), Err(Error::Io { .. })));
}
```

## Related

- [Inflectors](./inflectors) — normalise a word to a headword before looking it
  up here.
- [Iterator vs. `_into`](../performance/iterator-vs-into) — the lazy/eager
  distinction behind `lookup` vs. `lookup_iter` and `relation` vs. `closure`.
- [Parallelism](../performance/parallelism) — the shared, `Arc`-wrapped,
  read-only-query pattern this page reuses.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy) — what "borrowed" means for
  `DataRecordRef`.
- [Choosing an API](../choosing/index), [Core traits](./core),
  [Benchmarks](../benchmarks/index), [Recipes](../recipes/index).

## API reference

```rust ignore
// verbora_wordnet
pub struct Config { pub storage: Storage, pub prebuilt: Option<PathBuf> }
pub enum Storage { Pread, LazyResident, Resident /* default */, Indexed }
pub enum Pos { Noun, Verb, Adj, Adv }
pub struct FilePair<'a> { pub index: &'a IndexFile, pub data: &'a DataFile }

impl WordNet {
    pub fn open(dict_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_with(dict_dir: impl AsRef<Path>, config: &Config) -> Result<Self>;
    pub fn from_env() -> Result<Self>;
    pub fn from_env_with(config: &Config) -> Result<Self>;

    pub fn dict_dir(&self) -> &Path;
    pub fn index_file(&self, pos: Pos) -> &IndexFile;
    pub fn data_file_for(&self, pos: Pos) -> &DataFile;
    pub fn data_file(&self, tag: &str) -> Option<&DataFile>;
    pub fn file_pairs(&self) -> [FilePair<'_>; 4];
    pub fn pair(&self, pos: Pos) -> FilePair<'_>;

    pub fn lookup(&self, word: &str) -> Result<Vec<DataRecord>>;
    pub fn lookup_iter<'a>(&'a self, word: &str) -> LookupIter<'a>;

    pub fn get(&self, synset_offset: f64, tag: &str) -> Result<DataRecord>;
    pub fn get_at(&self, synset_offset: f64, pos: Pos) -> Result<DataRecord>;

    pub fn lookup_synonyms(&self, word: &str) -> Result<Vec<DataRecord>>;
    pub fn get_synonyms(&self, synset_offset: f64, tag: &str) -> Result<Vec<DataRecord>>;
    pub fn get_synonyms_of(&self, record: &DataRecord) -> Result<Vec<DataRecord>>;

    pub fn pointers<'a>(&'a self, record: &'a DataRecord) -> Pointers<'a>;
    pub fn relation<'a>(&'a self, record: &'a DataRecord, symbol: &'a str) -> Pointers<'a>;
    pub fn closure<'a>(&'a self, record: &DataRecord, symbol: &'a str) -> Closure<'a>;

    // requires the `parallel` Cargo feature
    pub fn par_lookup_batch(&self, words: &[&str]) -> Vec<Result<Vec<DataRecord>>>;

    pub fn query_sense(&self, spec: &str) -> Result<Vec<Sense>>;
    pub fn find_sense(&self, spec: &str) -> Result<Option<DataRecord>>;
}

impl Pos {
    pub fn from_tag(tag: &str) -> Option<Self>;   // "n"/"v"/"a"/"s"/"r"; "s" -> Adj
    pub fn suffix(self) -> &'static str;          // "noun"/"verb"/"adj"/"adv"
    pub fn tag(self) -> &'static str;             // "n"/"v"/"a"/"r"
    pub fn all() -> [Self; 4];
}

impl Iterator for LookupIter<'_> { type Item = Result<DataRecord>; }
impl Iterator for Pointers<'_>   { type Item = Result<DataRecord>; }
impl Iterator for Closure<'_>    { type Item = Result<DataRecord>; }

// records
pub struct DataRecord { pub synset_offset: f64, pub lex_filenum: f64, pub pos: Option<String>,
    pub w_cnt: f64, pub lemma: Option<String>, pub synonyms: Vec<Option<String>>,
    pub lex_id: Option<String>, pub ptrs: Vec<Pointer>, pub gloss: String, pub def: String,
    pub exp: Vec<String> }
pub struct DataRecordRef<'a> { /* borrowed mirror of DataRecord */ }
pub struct Pointer { pub pointer_symbol: Option<String>, pub synset_offset: f64,
    pub pos: Option<String>, pub source_target: Option<String> }
pub struct IndexRecord { pub lemma: Option<String>, pub pos: Option<String>,
    pub ptr_symbol: Vec<Option<String>>, pub sense_cnt: f64, pub tagsense_cnt: f64,
    pub synset_offset: Vec<f64> }
pub enum Find { Hit(IndexHit), Miss }

impl DataFile {
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self>;
    pub fn file_path(&self) -> &str;
    pub fn path(&self) -> PathBuf;
    pub fn with_record<R>(&self, offset: f64, f: impl FnOnce(&DataRecordRef<'_>) -> R) -> Result<R>;
    pub fn get(&self, offset: f64) -> Result<DataRecord>;
}
impl IndexFile {
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self>;
    pub fn file_path(&self) -> &str;
    pub fn path(&self) -> PathBuf;
    pub fn probes<'a>(&'a self, search_key: &'a str) -> Probes<'a>;
    pub fn find(&self, search_key: &str) -> Result<Find>;
    pub fn lookup(&self, word: &str) -> Result<Option<IndexRecord>>;
}
pub fn parse_data_line(line: &str) -> std::result::Result<DataRecordRef<'_>, ParseError>;
pub fn parse_index_line(line: &str) -> Result<IndexRecord>;

// prebuilt
impl PrebuiltIndex {
    pub fn build(dict_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()>;
    pub fn load(path: impl AsRef<Path>) -> Result<Self>;
    pub fn to_bytes(&self) -> Vec<u8>;
    pub fn from_bytes(bytes: &[u8]) -> Result<Self>;
    pub fn source_for(&self, name: &str, path: &Path) -> Result<Source>;
    pub fn names(&self) -> Vec<&str>;
    pub fn line_count(&self) -> usize;
    pub fn byte_size(&self) -> usize;
    pub fn default_path(dict_dir: impl AsRef<Path>) -> PathBuf;
}

// sense
pub struct Sense { pub lemma: String, pub pos: Pos, pub number: Option<usize> }
impl std::str::FromStr for Sense { type Err = ParseSenseError; }
impl std::fmt::Display for Sense { /* "lemma#pos[#n]" */ }

// pointer — relation-symbol constants: ANTONYM, HYPERNYM, INSTANCE_HYPERNYM,
// HYPONYM, INSTANCE_HYPONYM, MEMBER_HOLONYM, SUBSTANCE_HOLONYM, PART_HOLONYM,
// MEMBER_MERONYM, SUBSTANCE_MERONYM, PART_MERONYM, ATTRIBUTE,
// DERIVATIONALLY_RELATED, DOMAIN_TOPIC, MEMBER_TOPIC, DOMAIN_REGION,
// MEMBER_REGION, DOMAIN_USAGE, MEMBER_USAGE, ENTAILMENT, CAUSE, ALSO_SEE,
// VERB_GROUP, SIMILAR_TO, PARTICIPLE, PERTAINYM
pub fn describe(symbol: &str) -> Option<&'static str>;

// error
pub enum Error {
    Io { path: PathBuf, source: std::io::Error },
    UnterminatedLine { path: PathBuf, offset: u64 },
    MissingGloss { path: PathBuf, offset: u64 },
    InvalidOffset { path: PathBuf, offset: f64 },
    UnknownPos(String),
    NegativeProbe { path: PathBuf, position: i64 },
    CountTooLarge { field: &'static str, value: f64 },
    Prebuilt(String),
    // #[non_exhaustive]
}
pub type Result<T> = std::result::Result<T, Error>;
```

No `unsafe`, no global mutable state. `WordNet`, `IndexFile` and `DataFile` are
`Send + Sync`; nothing depends on what was looked up before.
`par_lookup_batch` is the crate's only parallel entry point, gated behind the
`parallel` Cargo feature and off by default.
