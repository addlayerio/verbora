# WordNet

`verbora-wordnet` reads the Princeton WordNet lexical database and turns its
`index.*` / `data.*` files into synsets, definitions and relational pointers.

WordNet groups English nouns, verbs, adjectives and adverbs into *synsets* — sets
of words that share one sense — and records the relations between them (Fellbaum,
*WordNet: An Electronic Lexical Database*, MIT Press, 1998). It ships as eight
plain-text files whose layout is specified by the `wndb(5WN)` and `wninput(5WN)`
manual pages. This crate reads those files and nothing else; every behaviour on
this page is derived from that published format, and where the format leaves
something open, Verbora's choice is stated and pinned by a test.

<div class="callout callout-spec">
<strong>Specification status.</strong> The lookup pipeline, the index bisection,
sense addressing, the reader's handling of malformed records and
<code>index_key</code>'s normalisation are all documented and test-pinned, with
no external data required. <code>cargo test -p verbora-wordnet</code> runs
<strong>86</strong> unit tests, <strong>4</strong> enumeration tests and
<strong>13</strong> doctests.
</div>

## The database is separately licensed

`verbora` is MIT. **The WordNet database is not.** This crate ships **no
dictionary data at all** — none of the roughly 28 MB of index and data files
WordNet 3.0/3.1 consists of. It reads them at run time from a directory you
supply. The database is covered by Princeton University's own licence, reproduced
verbatim in `LICENSE-WORDNET` beside the crate; it requires the notice to
accompany all copies, including modifications, and forbids using Princeton's name
in advertising.

Download any WordNet 3.0 or 3.1 database from Princeton, then point
`WordNet::open` at the `dict` directory it contains — or set an environment
variable and use `WordNet::from_env`:

```text
export VERBORA_WORDNET_DICT=/path/to/wordnet/dict
```

`WordNet::from_env` checks `$VERBORA_WORDNET_DICT` (this crate's own override),
then `$WORDNET_DB_PATH` (the variable WordNet distributions conventionally set),
then `./dict`. A candidate counts only if it actually contains `index.noun`, so a
stale variable falls through to the next one instead of failing the whole call;
if none matches, `Error::DictionaryNotFound` names every candidate that was
tried.

Snippets on this page that need the real database are fenced `no_run`. The
runnable ones build a tiny, hand-written dictionary in the WordNet text format —
the same trick the crate's own unit tests use.

## When to use it

- **Synonym, definition and relation lookup for English words**, when you already
  have (or can install) the database and do not need it embedded in your binary.
  A synset's own `words` *are* its synonyms — the set of words sharing that
  sense — so `senses(word, pos)` and `Synset::words` answer the synonym question
  directly, with no separate call.
- **Walking the hypernym/hyponym/meronym graph.** `related` gives you one hop;
  `closure` walks the whole transitive chain lazily, breadth first, visiting each
  synset at most once.
- **A long-lived process serving concurrent lookups.** `WordNet` is immutable
  after construction and `Send + Sync`; share one `Arc<WordNet>` across threads
  with no locking. See [Concurrency](#concurrency).

## When not to use it

- **You cannot ship or install the database.** There is no bundled fallback; every
  method that needs a missing file returns `Error::Io` at open, not later.
- **You need morphological reduction.** This crate looks up the lemma you give it.
  Reducing *running* to *run* belongs to [Stemmers](./stemmers) or
  [Inflectors](./inflectors); keeping it there is what stops a lookup from quietly
  answering about a different word.
- **You want a general thesaurus API, POS tagging, or the rest of a WordNet
  distribution.** Only the eight `index.*`/`data.*` files are read —
  `index.sense`, the morphological exception lists, `cntlist` and the
  lexicographer sources are not.

## Quick example

Two synsets, `alpha` and `beta`, with `alpha` pointing at `beta` as its hypernym.
No download required.

```rust
use verbora_wordnet::{PartOfSpeech, PointerSymbol, Sense, SynsetOffset, WordNet};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-quick-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // `wndb(5WN)` requires the copyright header lines to start with two spaces,
    // so their first field is empty and sorts below every real lemma. This one
    // is 65 bytes, which is where the first data record begins.
    let header = "  1 Copyright notice, two leading spaces, as the format requires\n";
    let alpha =
        "00000065 06 n 01 alpha 0 001 @ 00000148 n 0000 | the first letter; \"as in alpha\"  \n";
    let beta = "00000148 06 n 02 beta 0 second 1 000 | the second letter  \n";

    std::fs::write(dir.join("data.noun"), format!("{header}{alpha}{beta}")).unwrap();
    std::fs::write(
        dir.join("index.noun"),
        format!(
            "{header}\
             aaa n 1 0 1 0 00000065  \n\
             bbb n 2 1 @ 2 0 00000065 00000148  \n\
             ccc n 1 0 1 0 00000148  \n"
        ),
    )
    .unwrap();
    // A dictionary is all eight files; the other three categories are empty here.
    for suffix in ["verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{suffix}")), "").unwrap();
        std::fs::write(dir.join(format!("data.{suffix}")), "").unwrap();
    }
    dir
}

fn main() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();

    // Senses come back in sense order: element 0 is sense 1.
    let senses = wn.senses("bbb", PartOfSpeech::Noun).unwrap();
    assert_eq!(
        senses.iter().map(|s| s.offset).collect::<Vec<_>>(),
        [SynsetOffset::new(65), SynsetOffset::new(148)]
    );

    let alpha = &senses[0];
    assert_eq!(alpha.lemma(), "alpha");
    assert_eq!(alpha.gloss.definition, "the first letter");
    assert_eq!(alpha.gloss.examples, ["as in alpha"]);

    // A synset's own words are its synonyms — the words sharing that sense.
    let beta = &senses[1];
    let synonyms: Vec<&str> = beta.words.iter().map(|w| w.lemma.as_str()).collect();
    assert_eq!(synonyms, ["beta", "second"]);

    // One pointer hop from alpha reaches beta.
    assert_eq!(wn.related(alpha, PointerSymbol::Hypernym).count(), 1);
    assert_eq!(wn.related(alpha, PointerSymbol::Hyponym).count(), 0);
    let parent = wn.related(alpha, PointerSymbol::Hypernym).next().unwrap().unwrap();
    assert_eq!(parent.lemma(), "beta");

    // The same synset, addressed as a numbered sense.
    let first: Sense = "bbb#n#1".parse().unwrap();
    assert_eq!(wn.sense(&first).unwrap().unwrap().offset, SynsetOffset::new(65));
    // Past the end is absence, not an error.
    assert!(wn.sense(&"bbb#n#3".parse().unwrap()).unwrap().is_none());

    std::fs::remove_dir_all(&dir).ok();
}
```

Against the real database:

```rust no_run
use verbora_wordnet::{PartOfSpeech, PointerSymbol, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("/path/to/wordnet/dict")?;

    // Every sense of "bank", nouns first, then verbs, adjectives and adverbs.
    for synset in wn.lookup("bank")? {
        println!("{}  {}", synset.lemma(), synset.gloss.definition);
    }

    // Just the nouns, numbered as WordNet numbers them.
    for (i, synset) in wn.senses("bank", PartOfSpeech::Noun)?.iter().enumerate() {
        println!("bank#n#{}: {}", i + 1, synset.gloss.definition);
    }

    // Walk up the hypernym chain from one specific noun sense.
    let node = wn.sense(&"node#n#8".parse()?)?.expect("node has eight senses");
    for parent in wn.closure(&node, PointerSymbol::Hypernym).take(5) {
        println!("^ {}", parent?.gloss.definition);
    }
    Ok(())
}
```

## Normalisation is named, never hidden

WordNet's index files are keyed on a specific spelling: lower-case ASCII, with the
words of a collocation joined by `_`. Turning a word a user typed into that
spelling is a real transform, so it has a name — `index_key` — and every entry
point says which side of the line it is on. Entry points that take a **word**
(`lookup`, `senses`, `index_entry`, `sense`) apply it first; entry points that
take a **key** (`IndexFile::entry`) use the argument verbatim.

```rust
use verbora_wordnet::index_key;

fn main() {
    assert_eq!(index_key("entity"), "entity");
    assert_eq!(index_key("New York"), "new_york");
    assert_eq!(index_key("new  york"), "new_york");
    assert_eq!(index_key("  entity  "), "entity");
    assert_eq!(index_key("U.S.A."), "u.s.a.");

    // Non-ASCII is left exactly as it is, and is therefore simply absent from
    // an ASCII index. Mangling it into something that matched would be a guess.
    assert_eq!(index_key("CAFÉ"), "cafÉ");
}
```

The transform is idempotent and is the identity on every string that is already a
legal index key, which is what makes every lemma reachable through it.
`tests/enumeration.rs` proves that by walking **every** entry of every index file
it is given rather than sampling: 5,956 generated keys spanning the lemma
alphabet, reachable under all four `Storage` strategies, plus 12,372 alternative
spellings (upper-cased, whitespace-padded) resolving to the same entries and 55
deliberately absent keys producing no false hit.

## Choosing a `Storage` strategy

The four strategies change only how the dictionary's bytes physically arrive.
None changes a single answer — the crate's own tests assert that the same lookup
against the same dictionary agrees across all four — which is what lets `Storage`
be a runtime choice rather than a type parameter.

| `Storage` | Startup | Per query | Resident memory | Pick it when |
|---|---|---|---|---|
| `Pread` | none | a handful of positioned reads | none | short-lived process, one or two lookups (a CLI tool) |
| `LazyResident` | none | in memory, once a file is first touched | grows to whichever files were used | long-lived process that may never touch some of the eight files |
| `Resident` *(default)* | reads the dictionary | in memory | the whole dictionary | long-lived process querying broadly, where predictable per-query latency beats a fast first request |
| `Indexed` | + one newline scan | line starts by `partition_point` | + four bytes per line | hot path with many repeated lookups, where the backwards line scan shows up in a profile |

**These are qualitative descriptions, not measurements.** The crate's Criterion
suite measures all four; no figures are published because none have been taken
against this implementation.

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

`Storage::Indexed` normally builds its line-start tables with one newline scan at
open time. `PrebuiltIndex` persists those line offsets to a file once, so every
later open loads them instead of re-scanning the dictionary for newlines.

The dictionary text files remain the only source of truth: the sidecar carries no
lemmas, glosses or content-derived offsets — only each line's byte position, plus
the length of the file it was built from. Opening against a sidecar refuses any
entry whose file has since changed size, with `Error::Prebuilt` naming the file
and both lengths, because the bisection's probe positions are a function of file
length. The builder is a pure function of the eight dictionary files — same bytes
in, same sidecar out, no timestamps, every integer little-endian — so it can be
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

## Order

Everything comes out in the order the files list it.

- `lookup` consults categories in the order **noun, verb, adjective, adverb**.
- Within a category, senses come out in **sense order** — the order the index line
  lists its offsets, which `wndb(5WN)` defines as most-frequently-tagged first.
  Element `0` is sense 1, so numbering results by arrival position and numbering
  them with `Sense` agree.
- `pointers` yields pointers in the order the data record writes them.
- `closure` is breadth first and yields each reachable synset at most once.

There is no deduplication across categories: a lemma that exists as both a noun
and a verb yields the senses of both.

## Traversal: eager vs lazy

Every method performs strictly sequential, synchronous I/O and returns its result
directly.

| API | Answers | Lazy | Output | Allocates |
|---|---|:--:|---|---|
| `lookup` | every sense of a word, all four categories | ❌ | `Result<Vec<Synset>>` | one `Vec`, one `Synset` per sense |
| `lookup_iter` | the same senses | ✅ | `LookupIter` → `Result<Synset>` | one `Synset` per sense actually read |
| `senses` | one category's senses | ❌ | `Result<Vec<Synset>>` | as `lookup`, over one index file |
| `sense` | one numbered sense | ❌ | `Result<Option<Synset>>` | one index line, one record |
| `index_entry` | the index line only — offsets, counts, relation symbols | ❌ | `Result<Option<IndexEntry>>` | one `IndexEntry`; no data record is read |
| `par_lookup_batch` | `lookup`, fanned out over many words | ❌ | `Vec<Result<Vec<Synset>>>` | one outer `Vec`, plus `lookup`'s own per word |
| `pointers` | every synset one hop from a record | ✅ | `Pointers` → `Result<Synset>` | one `Synset` per hop followed |
| `related` | `pointers`, filtered to one relation | ✅ | `Pointers` → `Result<Synset>` | as `pointers`, minus the hops skipped |
| `closure` | the whole transitive chain of one relation | ✅ | `Closure` → `Result<Synset>` | one `Synset` per synset visited, plus a queue and a visited set |

Three things are easy to miss:

- **`lookup` is exactly `lookup_iter(word).collect()`.** A word with many senses
  costs one line read each, so collecting all of them to take the first two is
  work you did not need. `lookup_iter` stops after yielding its first `Err`:
  once a read has failed, continuing would report the same failure repeatedly.
- **`senses` searches one index file, `lookup` searches four.** When you know the
  category, saying so skips three bisections.
- **`closure` is the only one that walks more than one hop.** It never yields the
  starting synset, and its visited set is keyed on `(category, offset)` — an
  offset alone is ambiguous across the four data files. Following `Hyponym` from a
  general synset can reach tens of thousands of descendants, so prefer `.take(n)`
  or a filter unless you want all of them.

```rust no_run
use verbora_wordnet::{PartOfSpeech, PointerSymbol, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("/path/to/wordnet/dict")?;

    for synset in wn.senses("node", PartOfSpeech::Noun)? {
        // One hop of one relation.
        for parent in wn.related(&synset, PointerSymbol::Hypernym) {
            println!("^ {}", parent?.lemma());
        }
        // The whole chain, lazily, one read at a time.
        for ancestor in wn.closure(&synset, PointerSymbol::Hypernym) {
            println!("^^ {}", ancestor?.lemma());
        }
    }
    Ok(())
}
```

### `par_lookup_batch`

Behind the `parallel` Cargo feature, `par_lookup_batch` is exactly
`words.par_iter().map(|w| self.lookup(w)).collect()` — a thin fan-out over the
same sequential `lookup`, input order preserved, each element carrying its own
`Result`, so one word's failure does not abort the others. `WordNet` is already
immutable and `Send + Sync` with nothing cached or locked per query, so it needed
no new synchronization, and it uses whichever global `rayon` pool is installed
rather than building one of its own.

Reach for it when the *batch* is the unit of work — resolving every distinct token
of a corpus as an offline step, for example. A single lookup is cheap enough that
a small batch can be dominated by the cost of scheduling the tasks; prefer a plain
`.iter().map(...)` loop for a handful of words.

**The crossover point is unmeasured for this implementation**, so no speedup
figure is quoted here. See [Parallelism](../performance/parallelism).

```rust ignore
let results = wn.par_lookup_batch(&["run", "entity", "zzzzz"]);
for r in results {
    match r {
        Ok(synsets) => println!("{} senses", synsets.len()),
        Err(e) => println!("lookup failed: {e}"),
    }
}
```

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

A word with no entry is `None` or an empty `Vec` — never an error and never a
sentinel. A file that cannot be read, or a record that does not match the
documented format, is an `Error`, including the cases a lenient reader would paper
over. No public function panics on any input.

| Condition | Result |
|---|---|
| A dictionary file is missing or unreadable | `Error::Io`, at open rather than at the first query |
| `from_env` found no dictionary | `Error::DictionaryNotFound`, naming every candidate tried |
| An offset lies at or past the end of its data file | `Error::OffsetOutOfRange`, with the offset and the file's recorded length |
| An offset does not point at the start of a record | `Error::MalformedSynset` with `RecordError::OffsetMismatch` |
| A record read from the wrong category's file | `Error::MalformedSynset` with an `ss_type` complaint |
| A record has no `\|` gloss delimiter | `Error::MalformedSynset` with `RecordError::MissingGloss` |
| A numeric field is not a number in its documented radix | `Error::MalformedSynset` / `Error::MalformedIndexEntry` with `RecordError::InvalidField` |
| An index line's two sense counts disagree | `RecordError::SenseCountMismatch` — guessing which to believe would drop or invent senses |
| A dictionary file is 4 GiB or larger, under any strategy that holds it in memory | `Error::FileTooLarge`, naming the file, its length and the limit |
| A prebuilt sidecar no longer matches the files | `Error::Prebuilt` |

Every variant names the file it concerns, and the two record variants also name
the exact byte position of the record that failed, so a malformed dictionary can
be inspected with `dd`/`sed` without guesswork. A file's length is recorded once
at open, so a file that changes size mid-session does not change the search path.

## Allocation behaviour

- **At open.** `Resident` and `Indexed` each read the whole file into one buffer
  per file (eight files); `Indexed` adds four bytes per line. `Pread` and
  `LazyResident` allocate nothing beyond the `File` handles — `LazyResident`'s
  buffer is allocated the first time that file is queried.
- **The 4 GiB ceiling.** Every strategy that makes a file resident — `Resident`,
  `Indexed` and `LazyResident` — refuses a file of 4 GiB or more with
  `Error::FileTooLarge` instead of handing its length to an allocator. A
  dictionary path is caller-supplied and so is its size, so the length is
  checked, not trusted: `LazyResident` reports it at open rather than at the
  first query, and the read is capped one byte past the limit so a file that
  grows between the metadata call and the read is refused on what was actually
  read. Only `Pread`, which holds one line at a time, has no ceiling. WordNet
  3.1's largest file is about 16 MB, four thousand times under the limit, which
  is why respecting it costs nothing.
- **Per query, eager.** `synset`, `senses` and `lookup` return owned `Synset`s:
  one `String` per textual field, one `Vec` each for `words`, `pointers` and the
  gloss's examples.
- **Per query, borrowed** <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>**.**
  `with_synset` hands a `SynsetRef` to a closure instead: every string field
  points into the line being parsed, so reading a synset allocates only where the
  gloss needs cleaning up. This is the primitive the owned form is built on —
  `synset` is one `SynsetRef::to_synset` on top of it, so the two cannot diverge.
- **Traversal.** `pointers` and `related` allocate nothing beyond the `Synset`
  each hop reads. `closure` adds a queue of pending pointers and a visited set,
  both bounded by the reachable subgraph.
- **The bisection.** Each probe reads one line into a scratch buffer that is
  reused across probes; nothing is retained but the winning line.

There is no `_into` variant and no caller-supplied output buffer anywhere in this
crate. See [Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Performance characteristics

`crates/verbora-wordnet/benches/wordnet.rs` is a Criterion suite comparing the
four `Storage` strategies across startup, cold, warm and batch dimensions. These
benches need the real database and skip cleanly when no dictionary is configured.
Reproduce with `cargo bench -p verbora-wordnet`; see
[Benchmarks](../benchmarks/index) for results across the workspace.

**No timing figure is published for this crate**: no measurement describes the
implementation as it now stands, and measurement is pending.

## Unicode and language notes

- **Searching is byte-wise.** `wndb(5WN)` specifies that index files are sorted in
  the ASCII collating sequence so they can be binary searched, so `IndexFile::entry`
  compares the raw bytes of a line's first field against the raw bytes of the key.
  Comparing decoded scalar values would agree for every legal file — the format's
  alphabet is ASCII — and would silently disagree for a corrupt one, which is the
  case where being wrong matters.
- **Content is Unicode scalar values.** A gloss is handed back as `&str`, decoded
  from UTF-8 with invalid bytes replaced by `U+FFFD` rather than failing the whole
  read: one corrupt byte costs one character of one definition, not the record.
- **`index_key` works in scalars for whitespace and in ASCII for case.**
  Whitespace uses Unicode's own definition, because a non-breaking space between
  two words is still a word boundary. Case is folded only for ASCII, because
  `wndb(5WN)` defines index lemmas as lower-case ASCII: folding `İ` would produce
  a string no index contains, which is a guess dressed as a lookup.
- **The copyright header is not an entry.** Header lines begin with two spaces, so
  their first field is empty and sorts below every real lemma; an empty key answers
  `None` without touching the file.

## Common mistakes

- **Assuming the database ships with the crate.** It does not, ever.
  `WordNet::open` on a missing directory fails immediately with `Error::Io` — no
  partial dictionary, no silent stall.
- **Passing a raw offset without a category.** The same byte position names a
  different synset in each of the four data files, so `synset` takes a
  `SynsetOffset` *and* a `PartOfSpeech`. Use `target` to follow a `Pointer`, which
  already carries both.
- **Expecting a lookup to reduce morphology.** `lookup("running")` looks for
  *running*. Stem first if that is what you meant.
- **Passing a word where a key belongs.** `IndexFile::entry` uses its argument
  verbatim; `WordNet::lookup` and friends apply `index_key` first.
- **Treating an unknown word as an error.** It is an empty `Vec` or a `None`.
  Errors are reserved for files that cannot be read or records that do not parse.
- **Assuming `#s` means something separate from `#a`.** The adjective-satellite
  tag routes to the adjective files, because that is where satellites live.

```rust
use verbora_wordnet::{Error, WordNet};

fn main() {
    assert!(matches!(WordNet::open("/no/such/dir"), Err(Error::Io { .. })));
}
```

## Related

- [Stemmers](./stemmers) and [Inflectors](./inflectors) — reduce a word to a
  headword before looking it up here.
- [Iterator vs. `_into`](../performance/iterator-vs-into) — the lazy/eager
  distinction behind `lookup` vs. `lookup_iter` and `related` vs. `closure`.
- [Parallelism](../performance/parallelism) — the shared, `Arc`-wrapped,
  read-only-query pattern this page reuses.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy) — what "borrowed" means for `SynsetRef`.
- [Choosing an API](../choosing/index), [Core traits](./core),
  [Benchmarks](../benchmarks/index), [Recipes](../recipes/index).

## API reference

```rust ignore
// verbora_wordnet
pub struct Config { pub storage: Storage, pub prebuilt: Option<PathBuf> }
pub enum Storage { Pread, LazyResident, Resident /* default */, Indexed }
pub enum PartOfSpeech { Noun, Verb, Adjective, Adverb }
pub enum SynsetType { Noun, Verb, Adjective, AdjectiveSatellite, Adverb }

impl WordNet {
    pub fn open(dict_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn open_with(dict_dir: impl AsRef<Path>, config: &Config) -> Result<Self>;
    pub fn from_env() -> Result<Self>;
    pub fn from_env_with(config: &Config) -> Result<Self>;

    pub fn dict_dir(&self) -> &Path;
    pub fn index_file(&self, pos: PartOfSpeech) -> &IndexFile;
    pub fn data_file(&self, pos: PartOfSpeech) -> &DataFile;

    pub fn index_entry(&self, word: &str, pos: PartOfSpeech) -> Result<Option<IndexEntry>>;
    pub fn senses(&self, word: &str, pos: PartOfSpeech) -> Result<Vec<Synset>>;
    pub fn sense(&self, sense: &Sense) -> Result<Option<Synset>>;
    pub fn lookup(&self, word: &str) -> Result<Vec<Synset>>;
    pub fn lookup_iter<'a>(&'a self, word: &str) -> LookupIter<'a>;

    pub fn synset(&self, offset: SynsetOffset, pos: PartOfSpeech) -> Result<Synset>;
    pub fn with_synset<R>(&self, offset: SynsetOffset, pos: PartOfSpeech,
        f: impl FnOnce(&SynsetRef<'_>) -> R) -> Result<R>;
    pub fn target(&self, pointer: &Pointer) -> Result<Synset>;

    pub fn pointers<'a>(&'a self, synset: &'a Synset) -> Pointers<'a>;
    pub fn related<'a>(&'a self, synset: &'a Synset, symbol: PointerSymbol) -> Pointers<'a>;
    pub fn closure<'a>(&'a self, synset: &Synset, symbol: PointerSymbol) -> Closure<'a>;

    // requires the `parallel` Cargo feature
    pub fn par_lookup_batch(&self, words: &[&str]) -> Vec<Result<Vec<Synset>>>;
}

impl PartOfSpeech {
    pub const ALL: [Self; 4];
    pub fn from_tag(tag: &str) -> Option<Self>;   // "n"/"v"/"a"/"s"/"r"; "s" -> Adjective
    pub fn tag(self) -> &'static str;
    pub fn file_suffix(self) -> &'static str;     // "noun"/"verb"/"adj"/"adv"
    pub fn name(self) -> &'static str;
}

impl Iterator for LookupIter<'_> { type Item = Result<Synset>; }
impl Iterator for Pointers<'_>   { type Item = Result<Synset>; }
impl Iterator for Closure<'_>    { type Item = Result<Synset>; }

// records
pub struct SynsetOffset(u32);          // new(u32), get() -> u32
pub struct Synset { pub offset: SynsetOffset, pub lex_filenum: u8,
    pub synset_type: SynsetType, pub words: Vec<Word>, pub pointers: Vec<Pointer>,
    pub gloss: Gloss }
pub struct SynsetRef<'a> { /* borrowed mirror of Synset; to_synset() -> Synset */ }
pub struct Word { pub lemma: String, pub lex_id: u8, pub marker: Option<SyntacticMarker> }
pub struct Gloss { pub definition: String, pub examples: Vec<String> }
pub struct Pointer { pub symbol: PointerSymbol, pub offset: SynsetOffset,
    pub synset_type: SynsetType, pub scope: PointerScope }
pub enum PointerScope { Semantic, Lexical { source_word: NonZeroU8, target_word: NonZeroU8 } }
pub struct IndexEntry { pub lemma: String, pub pos: PartOfSpeech,
    pub pointer_symbols: Vec<PointerSymbol>, pub tagged_sense_count: u32,
    pub synset_offsets: Vec<SynsetOffset> }

impl Synset {
    pub fn lemma(&self) -> &str;
    pub fn part_of_speech(&self) -> PartOfSpeech;
    pub fn pointers_with(&self, symbol: PointerSymbol) -> impl Iterator<Item = &Pointer>;
}

impl DataFile {
    pub fn open(path: impl AsRef<Path>, pos: PartOfSpeech, storage: Storage) -> Result<Self>;
    pub fn path(&self) -> &Path;
    pub fn part_of_speech(&self) -> PartOfSpeech;
    pub fn len_bytes(&self) -> u64;
    pub fn synset(&self, offset: SynsetOffset) -> Result<Synset>;
    pub fn with_synset<R>(&self, offset: SynsetOffset,
        f: impl FnOnce(&SynsetRef<'_>) -> R) -> Result<R>;
    pub fn synsets(&self) -> Synsets<'_>;
}
impl IndexFile {
    pub fn open(path: impl AsRef<Path>, pos: PartOfSpeech, storage: Storage) -> Result<Self>;
    pub fn path(&self) -> &Path;
    pub fn part_of_speech(&self) -> PartOfSpeech;
    pub fn len_bytes(&self) -> u64;
    pub fn entry(&self, key: &str) -> Result<Option<IndexEntry>>;
    pub fn entries(&self) -> Entries<'_>;
}
pub fn index_key(word: &str) -> Cow<'_, str>;

// prebuilt
impl PrebuiltIndex {
    pub fn build(dict_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()>;
    pub fn load(path: impl AsRef<Path>) -> Result<Self>;
    pub fn to_bytes(&self) -> Vec<u8>;
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self>;
    pub fn names(&self) -> Vec<&str>;
    pub fn line_count(&self) -> usize;
    pub fn byte_size(&self) -> usize;
    pub fn default_path(dict_dir: impl AsRef<Path>) -> PathBuf;
}

// sense addressing — `lemma#pos#number`, all three parts required
pub struct SenseNumber(NonZeroU16);    // FIRST, new, from_u16, get, as_usize
pub struct Sense { pub lemma: String, pub pos: PartOfSpeech, pub number: SenseNumber }
impl std::str::FromStr for Sense { type Err = ParseSenseError; }
impl std::fmt::Display for Sense { /* "lemma#pos#number" */ }

// pointer — 26 relations: Antonym, Hypernym, InstanceHypernym, Hyponym,
// InstanceHyponym, MemberHolonym, SubstanceHolonym, PartHolonym, MemberMeronym,
// SubstanceMeronym, PartMeronym, Attribute, DerivationallyRelatedForm,
// DomainOfTopic, MemberOfTopic, DomainOfRegion, MemberOfRegion, DomainOfUsage,
// MemberOfUsage, Entailment, Cause, AlsoSee, VerbGroup, SimilarTo,
// ParticipleOfVerb, Pertainym
impl PointerSymbol {
    pub fn from_symbol(symbol: &str) -> Option<Self>;
    pub fn symbol(self) -> &'static str;
    pub fn name(self, pos: PartOfSpeech) -> &'static str;
}

// error
pub enum Error {
    Io { path: PathBuf, source: std::io::Error },
    DictionaryNotFound { tried: Vec<PathBuf> },
    MalformedIndexEntry { path: PathBuf, line_start: u64, kind: RecordError },
    MalformedSynset { path: PathBuf, offset: SynsetOffset, kind: RecordError },
    OffsetOutOfRange { path: PathBuf, offset: SynsetOffset, file_len: u64 },
    FileTooLarge { path: PathBuf, len: u64, limit: u64 },
    Prebuilt { path: PathBuf, reason: String },
    // #[non_exhaustive]
}
pub enum RecordError {
    MissingField { field: &'static str },
    InvalidField { field: &'static str, value: String },
    MissingGloss,
    SenseCountMismatch { synset_cnt: u32, sense_cnt: u32 },
    OffsetMismatch { requested: SynsetOffset, found: SynsetOffset },
    // #[non_exhaustive]
}
pub type Result<T> = std::result::Result<T, Error>;
```

No `unsafe`, no global mutable state. `WordNet`, `IndexFile` and `DataFile` are
`Send + Sync`; nothing depends on what was looked up before.
`par_lookup_batch` is the crate's only parallel entry point, gated behind the
`parallel` Cargo feature and off by default.
