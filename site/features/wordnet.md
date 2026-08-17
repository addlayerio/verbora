# WordNet

`verbora-wordnet` reads the Princeton WordNet lexical database and turns its
`index.*` / `data.*` files into synsets, definitions and relational pointers.
It keeps two long-established behaviours that are, strictly, wrong: the index search is
an **incomplete** binary search that reports "not found" for lemmas that are
unquestionably in the file, and every relation traversal comes back in
**reverse** order because the reference drains its work lists with `pop()`.
Both are deliberate, both are covered by the test fixture, and both are
explained below.

<div class="callout callout-spec">
<strong>Specification status.</strong> <code>lookup</code>, <code>get</code>,
<code>lookup_synonyms</code>, <code>get_synonyms</code>, the index bisection's
probe sequence and the database reader's handling of CRLF line endings,
missing gloss separators and non-ASCII keys are all documented and
test-pinned, with no external data required.
<code>cargo test -p verbora-wordnet</code> runs <strong>54</strong> unit tests
and <strong>14</strong> doctests.
</div>

## WordNet is separately licensed

`verbora` is MIT. **The WordNet database is not.** This crate ships **no
dictionary data at all** — no `index.noun`, no `data.verb`, none of the roughly
28 MB of index and data files WordNet 3.0/3.1 consists of. It reads them at run
time from a directory you supply, which is what keeps 34 MB of separately
licensed content out of this repository and out of your dependency tree. The
database is covered by Princeton University's own licence, reproduced verbatim
in `LICENSE-WORDNET` beside the crate; it requires the notice to accompany all
copies, including modifications, and forbids using Princeton's name in
advertising.

Get the database with either of these, then point `WordNet::open` or
`WordNet::from_env` at the `dict` directory it produces:

```text
npm install wordnet-db          # WordNet 3.1, ~10 MB packed
export WORDNET_DB_PATH="$PWD/node_modules/wordnet-db/dict"
```

or download any WordNet 3.0 or 3.1 database directory directly from Princeton
and point the same variable at it. `WordNet::from_env` checks
`$WORDNET_DB_PATH`, then `$VERBORA_WORDNET_DICT` (this crate's own override),
then `./node_modules/wordnet-db/dict`, and fails with `Error::Io` naming every
candidate it tried rather than silently doing nothing.

Every snippet on this page that needs the real database is fenced `no_run` and
says so. The runnable ones build a tiny, hand-written dictionary in the WordNet
text format — a few lines of `index.*` and `data.*` content — the same trick
the crate's own unit tests use so they never need the 34 MB database either.

## When to use it

- **Porting the reference that called the reference's `WordNet`.** Every entry point
  maps onto a Rust method with the same argument shape (see
  [The core API surface](#the-core-api-surface)), and results — definitions,
  synonym lists, pointer traversal, even the incomplete search's false
  misses — are byte-identical to the reference.
- **Synonym, definition and relation lookup for English words**, when you
  already have (or can install) the database and do not need it embedded in
  your binary.
- **Walking the hypernym/hyponym/meronym graph.** `WordNet::relation` gives
  you one hop; `WordNet::closure` walks the whole transitive chain lazily,
  with cycle protection.
- **A long-lived process serving concurrent lookups.** `WordNet` is
  immutable after construction and `Send + Sync`; share one `Arc<WordNet>`
  across threads with no locking. See [Concurrency](#concurrency).

## When not to use it

- **You cannot ship or install the database.** There is no bundled fallback and
  no partial dictionary; every method that needs a file the caller has not
  provided returns `Error::Io` at open, not later.
- **You need every WordNet entry to be reachable.** The reference's index
  search is not merely slow, it is **incomplete** — see
  [The index search is deliberately incomplete](#the-index-search-is-deliberately-incomplete).
  A word that is genuinely in the database can still come back as a miss.
- **You want results in dictionary or alphabetical order.** Every traversal —
  `lookup`, `getSynonyms`, pointer walks — comes back in the reference's
  `pop()`-driven reverse order. See
  [Result order is defined by a bug](#result-order-is-defined-by-a-bug).
- **You want stemming, POS tagging, or a general thesaurus API.** This crate is
  the lexical database reader only; nothing here inflects a word to find its
  base form first. Pair it with [Inflectors](./inflectors) or your own
  normalisation if the input is not already a WordNet headword.

## Quick example

This uses the same tiny, hand-built dictionary format the crate's own tests
use — two synsets, `alpha` and `beta`, with `alpha` pointing at `beta` as its
hypernym. No download required.

```rust
use verbora_wordnet::{WordNet, pointer};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-quick-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Two index lines: alpha (one sense) and beta (one sense, pointed at by alpha).
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
    assert_eq!(synonyms.len(), 1);
    assert_eq!(synonyms[0].lemma.as_deref(), Some("beta"));

    assert_eq!(wn.relation(&alpha, pointer::HYPERNYM).count(), 1);
    assert_eq!(wn.relation(&alpha, pointer::HYPONYM).count(), 0);

    std::fs::remove_dir_all(&dir).ok();
}
```

Against the real database the same shape looks like this — fenced `no_run`
because it needs a `dict/` directory this page cannot provide:

```rust no_run
use verbora_wordnet::{WordNet, pointer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("node_modules/wordnet-db/dict")?;

    for synset in wn.lookup("node")? {
        println!("{:?}  {}", synset.lemma, synset.def);
    }

    // Walk up the hypernym chain from "node" (the network sense).
    let node = wn.get(3_832_647.0, "n")?;
    for parent in wn.closure(&node, pointer::HYPERNYM).take(5) {
        println!("^ {}", parent?.def);
    }
    Ok(())
}
```

## Choosing the right API

Two decisions sit on top of each other here, and both are the site's usual
"more than one way to do the same conceptual thing": **which byte-access
strategy backs the dictionary**, and **which traversal shape to call** for a
given question. Neither has a universally correct answer.

### Choosing a WordNet loading strategy

The reference reads **one byte per `fs.read` call** — 1,098 syscalls for a
single `find('entity')` on `index.noun`, and 61 open/close pairs for
`lookup('run')`. This crate reproduces the probe *positions* that search makes
exactly, because they are observable through the false misses, and nothing
else about how the bytes physically arrive. Four `Storage` strategies are
offered:

| `Storage` | Startup | Per query | Resident memory |
|---|---|---|---|
| `Storage::Pread` | none | a handful of positioned syscalls | none |
| `Storage::LazyResident` | none | in-memory, once a file is first touched | grows to whichever files were used |
| `Storage::Resident` *(default)* | reads ~28 MB | in-memory scan | ~28 MB |
| `Storage::Indexed` | + one `memchr` pass over the resident bytes | `partition_point` over a `u32` line-start table | + about 4 bytes per line (~470 KiB for `index.noun`'s 118k lines) |

No strategy changes a single answer — only how the bytes arrive — which is
exactly what lets `Storage` be a runtime choice rather than a type parameter.
The crate's own unit tests assert this directly: the same lookup against the
same tiny dictionary agrees across all four backends.

```rust
use verbora_wordnet::{Config, Storage, WordNet};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-storage-{}-{:?}",
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
    let reference = WordNet::open_with(&dir, &Config::new(Storage::Resident))
        .unwrap()
        .lookup("bbb")
        .unwrap();

    for storage in [Storage::Pread, Storage::LazyResident, Storage::Indexed] {
        let wn = WordNet::open_with(&dir, &Config::new(storage)).unwrap();
        assert_eq!(wn.lookup("bbb").unwrap(), reference, "{storage:?}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

Choosing between them is a genuine judgment call — nothing here is right for
every deployment:

- **`Storage::Pread`** — closest in spirit to the reference, minus the
  one-byte-at-a-time pathology (a backward scan reads 512-byte blocks, a
  forward scan reads 4 KiB blocks). Pick this for a **CLI tool run once**: no
  startup cost, and the process exits before residency would have paid for
  itself.
- **`Storage::LazyResident`** — free startup, and the first query against a
  given file pays to read the whole thing; every query after that is
  in-memory. Pick this for a **long-lived process that might not touch every
  one of the eight files** — a service that only ever looks up nouns pays
  nothing for `data.verb`.
- **`Storage::Resident`** *(the default)* — pays ~28 MB up front so every
  query afterwards is uniformly in-memory. Pick this for a **long-lived
  process that queries broadly**, where predictable per-query latency matters
  more than a fast first request.
- **`Storage::Indexed`** — `Resident` plus a line-start table, so
  `findPrevEOL` becomes a `partition_point` instead of a backward byte scan.
  Pick this for a **hot path with many repeated lookups** where the bisection
  itself shows up in a profile.

```text
Which Storage strategy?
│
├── Short-lived process, one or two lookups
│      └── Storage::Pread
│
├── Long-lived process, but this dictionary may go untouched
│      └── Storage::LazyResident
│
├── Long-lived process that queries broadly across the dictionary
│      └── Storage::Resident            (the default)
│
└── Hot path with many repeated lookups
       ├── Startup can pay for the memchr scan once
       │      └── Storage::Indexed
       └── Startup must be near-zero too, across many process restarts
              └── Storage::Indexed + a PrebuiltIndex sidecar
```

#### The `PrebuiltIndex` sidecar

`Storage::Indexed` normally builds its line-start tables with one `memchr`
pass over the resident bytes at open time. `PrebuiltIndex` trades that scan
for a smaller one: it persists the derived line offsets to a file once, and
every later open loads that file instead of re-scanning ~28 MB for newlines.

The dictionary text files remain the **only** source of truth. The sidecar
carries no lemmas, no glosses, and no offsets drawn from the dictionary's
content — only the byte position of every line, plus the length of the file it
was built from. `PrebuiltIndex::source_for` refuses an entry whose file has
since changed size, because the index bisection's probe positions are a
function of file length: a sidecar built against a different dictionary would
silently describe a different search if it were trusted.

```rust
use verbora_wordnet::{Config, PrebuiltIndex, Storage, WordNet};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-prebuilt-{}-{:?}",
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
    let sidecar = dir.join("wordnet.nrsidx");
    PrebuiltIndex::build(&dir).unwrap().save(&sidecar).unwrap();

    // Same answers, whether the tables were scanned just now or loaded from disk.
    let scanned = WordNet::open_with(&dir, &Config::new(Storage::Indexed)).unwrap();
    let loaded = WordNet::open_with(&dir, &Config::default().with_prebuilt(&sidecar)).unwrap();
    for word in ["aaa", "bbb", "ccc", "zzz"] {
        assert_eq!(scanned.lookup(word).unwrap(), loaded.lookup(word).unwrap());
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

The builder is a pure function of the eight dictionary files — same bytes in,
same sidecar out, on any machine, with no timestamps — so it can be built once
in CI or a deploy step and shipped alongside the dictionary.

### The core API surface

Every reference entry point takes a callback and returns `undefined`; within
one operation its I/O is strictly sequential, so a synchronous port is order
equivalent and callbacks become return values:

| Reference | `verbora-wordnet` |
|---|---|
| `wn.lookup(w, cb)` | `WordNet::lookup` → `Result<Vec<DataRecord>>`, and the lazy `WordNet::lookup_iter` |
| `wn.get(off, pos, cb)` | `WordNet::get` |
| `wn.getDataFile(pos)` | `WordNet::data_file` → `Option`, for the reference's `undefined` |
| `wn.lookupSynonyms(w, cb)` | `WordNet::lookup_synonyms` |
| `wn.getSynonyms(off, pos, cb)` | `WordNet::get_synonyms` |
| `wn.getSynonyms(record, cb)` | `WordNet::get_synonyms_of` |

**`WordNet::open` / `open_with` / `from_env`.** `open` takes the directory
holding `index.noun` and its seven siblings — the path
`require('wordnet-db').path` returns in Node — and fails immediately with
`Error::Io` if a file is missing, where the reference stalls silently at the
first lookup instead. `open_with` takes an explicit `Config` (storage
strategy, optional prebuilt sidecar); `from_env` locates the directory for
you, checking `$WORDNET_DB_PATH`, `$VERBORA_WORDNET_DICT`, then
`./node_modules/wordnet-db/dict`.

```rust no_run
use verbora_wordnet::{Config, Storage, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::from_env()?;
    let indexed = WordNet::open_with("node_modules/wordnet-db/dict", &Config::new(Storage::Indexed))?;
    let _ = (wn, indexed);
    Ok(())
}
```

**`WordNet::lookup` and `lookup_iter`.** `lookup` normalises `word` — lowercase,
whitespace runs collapsed to a single `_` — then searches all four parts of
speech and returns every synset found, in the reference's order. `lookup_iter`
is the lazy primitive it is built on: reading one synset costs one line read,
so `.take(n)` genuinely avoids the rest. See
[Eager vs lazy relation traversal](#eager-vs-lazy-relation-and-lookup-traversal).

**`WordNet::get` and `get_at`.** `get(offset, tag)` reads the synset at a raw
byte offset in the data file for a part-of-speech *tag* (`"n"`, `"v"`, `"a"`,
`"s"`, `"r"`), returning `Error::UnknownPos` for anything else — where the
reference throws a `TypeError` reading `'get'` on `undefined`. `get_at` takes a
typed `Pos` instead and cannot fail to resolve one.

**`get_synonyms` vs `get_synonyms_of` — a divergence forced by types.** The
reference's `getSynonyms` is one the reference function dispatching on
`arguments` and **truthiness**: called with `(offset, pos, cb)` it looks up a
fresh synset by offset and tag; called with `(record, cb)` it re-reads the
synset the record already describes. Because the dispatch is truthy/falsy
rather than type-based, a falsy `pos` (`0`, `''`) silently promotes the
callback into the `pos` slot, and a falsy `synsetOffset` (`0`) promotes the
record object itself into the offset slot — four distinct failure shapes, all
recorded in the fixture. Rust has no `arguments` object, so the two intended
call shapes get their own names instead of a numeric offset of `0` becoming an
accident:

```rust
use verbora_wordnet::WordNet;

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-getsyn-{}-{:?}",
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

    // Same answer, reached two ways: by offset and tag, or from a record already in hand.
    let by_pair = wn.get_synonyms(0.0, "n").unwrap();
    let by_record = wn.get_synonyms_of(&alpha).unwrap();
    assert_eq!(by_pair, by_record);
    std::fs::remove_dir_all(&dir).ok();
}
```

**`WordNet::pointers`, `relation` and `closure`.** Covered in full under
[Eager vs lazy relation and lookup traversal](#eager-vs-lazy-relation-and-lookup-traversal).

**`Sense`, `find_sense` and `query_sense` — extensions with no reference
counterpart.** The reference WordNet module exports exactly the `WordNet`
constructor and nine methods; there is no `findSense` and no `querySense`
anywhere in the reference tree, verified by grep across the whole
the reference source and its specs. `Sense` parses a `lemma#pos[#n]`
string (`"entity#n#1"`), `WordNet::query_sense` lists every sense of a lemma
numbered from 1, and `WordNet::find_sense` resolves one numbered sense
straight to its synset:

```rust
use verbora_wordnet::WordNet;

fn multi_pos_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-sense-{}-{:?}",
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
    for pos in ["verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), "").unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), "").unwrap();
    }
    dir
}

fn main() {
    let dir = multi_pos_dict();
    let wn = WordNet::open(&dir).unwrap();

    let senses: Vec<String> = wn
        .query_sense("x#n")
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(senses, ["x#n#1", "x#n#2"]);

    assert_eq!(wn.find_sense("x#n#1").unwrap().unwrap().def.trim_end(), "noun sense A");
    assert_eq!(wn.find_sense("x#n#2").unwrap().unwrap().def.trim_end(), "noun sense B");
    std::fs::remove_dir_all(&dir).ok();
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Sense numbers run <strong>forwards</strong> over the
index line's own order — sense 1 is the first offset written on the line. That
is the <strong>opposite</strong> of the order <code>lookup</code> yields for
that part of speech, because <code>lookup</code> drains the same offsets from
the back. In the example above, <code>wn.lookup("x")</code> yields
<code>["noun sense B", "noun sense A"]</code> — sense 2 before sense 1 — while
<code>find_sense("x#n#1")</code> correctly answers sense 1. Numbering results
as <code>lookup</code> hands them to you would number every word backwards;
that is precisely why <code>find_sense</code>/<code>query_sense</code> exist
rather than inviting every caller to rediscover this.
</div>

## Eager vs lazy relation and lookup traversal

This is the site's usual thesis — lazy iterator vs. eager `Vec` — applied to a
graph instead of a token stream, plus one more axis specific to WordNet: how
many hops a traversal walks.

### Comparison table

| API | Answers | Lazy | Output | Allocates |
|---|---|:--:|---|---|
| `WordNet::lookup` | every synset for a word, all parts of speech | ❌ | `Vec<DataRecord>` | one `Vec`, one `DataRecord` per synset |
| `WordNet::lookup_iter` | the same synsets | ✅ | `LookupIter` → `Result<DataRecord>` | one `DataRecord` per synset actually read |
| `WordNet::par_lookup_batch` | `lookup`, fanned out over many words at once | ❌ | `Vec<Result<Vec<DataRecord>>>` | one outer `Vec`, plus `lookup`'s own allocations per word |
| `WordNet::pointers` | every synset one hop from a record | ✅ | `Pointers` → `Result<DataRecord>` | one `DataRecord` per hop actually followed |
| `WordNet::relation` | `pointers`, filtered to one relation symbol | ✅ | `Pointers` → `Result<DataRecord>` | as `pointers`, minus the hops skipped |
| `WordNet::closure` | the whole transitive chain of one relation | ✅ | `Closure` → `Result<DataRecord>` | one `DataRecord` per synset visited, plus a `VecDeque` queue and a seen-offsets `Vec` |
| `WordNet::get_synonyms_of` | `pointers`, eager and **re-read from disk** | ❌ | `Vec<DataRecord>` | as `pointers`, plus the re-read of the starting record |

Two things are easy to miss:

- **`get_synonyms_of` does not reuse the pointers already on your record.** The
  reference re-reads the synset from disk by offset and tag before following
  its pointers, so the caller's in-memory record is left untouched — this port
  does the same, on purpose, which is why it takes `&DataRecord` rather than
  consuming it. `pointers` is the same relation, made lazy and without the
  re-read, because it works from the pointers already in hand.
- **`closure` is the only one of these that walks more than one hop.** It is
  breadth-first, cycle-safe (each synset offset is visited at most once,
  tracked by the bit pattern of its `f64` offset so `NaN` compares by
  identity), and the starting record itself is never yielded.
- **`par_lookup_batch` is the only one of these with a built-in parallel
  sibling.** It fans `lookup` out across threads, one word per `rayon` task —
  see [`par_lookup_batch`](#par-lookup-batch) below.

### Decision tree

```text
I have a DataRecord and a relation to follow
│
├── "Every synset for this word, across every part of speech"
│      ├── I want them all, and I'll hold onto the result
│      │      └── WordNet::lookup()          → Vec<DataRecord>
│      ├── I might stop early, or read one at a time
│      │      └── WordNet::lookup_iter()      → lazy, .take(n) saves real I/O
│      └── I have MANY words to look up at once
│             └── WordNet::par_lookup_batch() → lookup, fanned out (parallel feature)
│
├── "Everything one hop away, of ANY relation"
│      └── WordNet::pointers()                → lazy
│
├── "Everything one hop away, of ONE relation (hypernym, hyponym, …)"
│      └── WordNet::relation(record, symbol)  → lazy, filtered
│
└── "The WHOLE chain — keep following this relation until it stops"
       └── WordNet::closure(record, symbol)   → lazy, breadth-first, cycle-safe
```

### `lookup` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — reads every offset on all four index lines before returning</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;DataRecord&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, plus one <code>DataRecord</code> (several <code>String</code>s and <code>Vec</code>s each) per synset found</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — no caller-supplied buffer</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">A word you expect to have few senses, or whose whole result set you want to keep</span></div>
</div>

Exactly `self.lookup_iter(word).collect()`. `"run"` has 57 senses in the real
database; collecting all of them to then take the first two is 55 line reads
you did not need.

### `par_lookup_batch`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>words.par_iter().map(|w| self.lookup(w)).collect()</code> over Rayon's global thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;Result&lt;Vec&lt;DataRecord&gt;&gt;&gt;</code>, input order preserved</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code> sized to <code>words.len()</code>, plus whatever <code>lookup</code> itself allocates per word</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — behind the <code>parallel</code> Cargo feature</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">A few hundred words or more, resolved as one offline batch</span></div>
</div>

`WordNet` is immutable after construction and already `Send + Sync` with
nothing cached or locked per query (see [Concurrency](#concurrency) below), so
fanning `lookup` out across threads needed no new synchronization at all —
`par_lookup_batch` is exactly `words.par_iter().map(|w| self.lookup(w)).collect()`,
a thin wrapper over the same sequential `lookup`, not a second search
implementation. Enable it with the crate's `parallel` Cargo feature.

Measured on `benches/wordnet.rs`'s own `par_lookup_batch` group
(`Storage::Resident`, 32 hardware threads, the same 16-word mix
`bench_repeat` uses, repeated out to each size), sequential vs. parallel:

| Batch size | Sequential | Parallel | Speedup |
|--:|--:|--:|--:|
| 16 | 633.5 µs | 600.6 µs | ~1.05× (noise-level) |
| 160 | 7.03 ms | 2.10 ms | ~3.3× |
| 1600 | 100.2 ms | 24.95 ms | ~4.0× |

A batch of 16 common words sits close to the break-even point: a `rayon` task
costs on the order of a microsecond to schedule, which is comparable to a
single lookup's own cost (~5.7–6.9 µs for a common entry like `entity`, up to
~150–206 µs for a high-sense-count word like `run`). Prefer a plain
`.iter().map(WordNet::lookup)` loop at that scale; from a few hundred words up
the scheduling cost amortises and the win is real — see
[Parallelism](../performance/parallelism) for the same reasoning applied
workspace-wide.

```rust  ignore
use verbora_wordnet::WordNet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("node_modules/wordnet-db/dict")?;
    let results = wn.par_lookup_batch(&["run", "entity", "zzzzz"]);
    for r in results {
        match r {
            Ok(synsets) => println!("{} senses", synsets.len()),
            Err(e) => println!("lookup failed: {e}"),
        }
    }
    Ok(())
}
```

### `lookup_iter` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — one index probe and one data-file line read per item actually yielded</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>LookupIter&lt;'_&gt;</code> → <code>Result&lt;DataRecord&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>DataRecord</code> per synset actually read; nothing for the parts of speech that come back empty or are never reached</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v"><code>.take(n)</code>, an existence check, or any early exit</span></div>
</div>

An error is sticky: once one lookup fails, the iterator returns `None` on
every later `next()` rather than re-reporting the same failure forever.

### `pointers` / `relation` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — one data-file read per pointer actually followed, draining the pointer list from the back</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Pointers&lt;'_&gt;</code> → <code>Result&lt;DataRecord&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>DataRecord</code> per hop followed; <code>relation</code>'s filtering costs a symbol comparison, not an allocation, for every hop it skips</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">One hop of the graph, using the pointers already on a record you have in hand</span></div>
</div>

### `closure` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy, breadth-first — one data-file read per synset actually visited</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Closure&lt;'_&gt;</code> → <code>Result&lt;DataRecord&gt;</code>; the starting record itself is never yielded</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>DataRecord</code> per synset visited, plus a <code>VecDeque&lt;Pointer&gt;</code> work queue and a <code>Vec&lt;u64&gt;</code> of visited offsets, both bounded by the reachable subgraph</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Climbing a hypernym chain to its root, or any question that needs the whole transitive closure of one relation</span></div>
</div>

Cycle-safe: an offset already emitted is never queued again, tracked as the
bit pattern of the `f64` offset so a `NaN` offset — reachable from a malformed
record — compares by identity rather than by IEEE 754's "`NaN` never equals
anything", which would otherwise defeat the cycle check.

### `get_synonyms_of` <a class="badge badge-owned" href="../performance/allocation">OWNED</a> <span class="badge badge-fallible">FALLIBLE</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — re-reads the starting synset from disk by offset and tag, then follows every pointer on <strong>that</strong> read</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;DataRecord&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One re-read <code>DataRecord</code> plus one per pointer followed</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Reproducing <code>getSynonyms(record, cb)</code> exactly, re-read included</span></div>
</div>

The re-read is not an oversight to optimise away — it is what the reference
does, so a record you mutated after reading it is not what gets followed.
Reach for `pointers` instead when you want the pointers already in hand.

### Result order is defined by a bug

Every recursive helper in the reference walks its work list with `pop()`, so
results come back backwards at two levels: the parts of speech are consulted
in the order **adv, adj, verb, noun** — the reverse of the `[noun, verb, adj,
adv]` array literal that produces them — and within one part of speech, the
index line's offsets are visited **last to first**. `lookup("fast")` therefore
yields `r:86892 r:86488 s:324771 … n:1071904` against the real database, and
`io_spec/wordnet_spec` asserts this order by value, so it is not incidental
— the reference's own test suite depends on it.

The following builds a small two-part-of-speech dictionary — one verb sense
and two noun senses sharing the lemma `"x"` — to make both reversals visible
without the real database:

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

    // Verb before noun (the reversed [noun, verb, adj, adv] order), and within
    // noun, sense B (the SECOND offset on the index line) before sense A.
    let defs: Vec<String> = wn
        .lookup("x")
        .unwrap()
        .into_iter()
        .map(|r| r.def.trim_end().to_owned())
        .collect();
    assert_eq!(defs, ["verb sense", "noun sense B", "noun sense A"]);

    // lookup_iter is lazy: the FIRST result costs exactly one line read, and
    // it is the verb sense — reading it never touches data.noun at all.
    let first: Vec<String> = wn
        .lookup_iter("x")
        .take(1)
        .map(|r| r.unwrap().def.trim_end().to_owned())
        .collect();
    assert_eq!(first, ["verb sense"]);

    std::fs::remove_dir_all(&dir).ok();
}
```

### `closure` walks past what `relation` can see

```rust
use verbora_wordnet::{WordNet, pointer};

fn chain_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-closure-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // alpha --hypernym--> beta --hypernym--> gamma
    let rec_gamma = "00000000 06 n 01 gamma 0 000 | top  \n".to_string();
    let beta_off = rec_gamma.len();
    let rec_beta = format!("{beta_off:08} 06 n 01 beta 0 001 @ 00000000 n 0000 | middle  \n");
    let alpha_off = rec_gamma.len() + rec_beta.len();
    let rec_alpha =
        format!("{alpha_off:08} 06 n 01 alpha 0 001 @ {beta_off:08} n 0000 | bottom  \n");
    let data = format!("{rec_gamma}{rec_beta}{rec_alpha}");
    let index = format!(
        "alpha n 1 0 1 0 {alpha_off:08}  \nbeta n 1 0 1 0 {beta_off:08}  \ngamma n 1 0 1 0 00000000  \n"
    );

    for pos in ["noun", "verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), &index).unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), &data).unwrap();
    }
    dir
}

fn main() {
    let dir = chain_dict();
    let wn = WordNet::open(&dir).unwrap();
    let alpha_offset = wn.lookup("alpha").unwrap()[0].synset_offset;
    let alpha = wn.get(alpha_offset, "n").unwrap();

    // One hop: only "middle".
    let one_hop: Vec<String> = wn
        .relation(&alpha, pointer::HYPERNYM)
        .map(|r| r.unwrap().def.trim_end().to_owned())
        .collect();
    assert_eq!(one_hop, ["middle"]);

    // The whole chain: "middle" AND "top", walked lazily, one read at a time.
    let whole_chain: Vec<String> = wn
        .closure(&alpha, pointer::HYPERNYM)
        .map(|r| r.unwrap().def.trim_end().to_owned())
        .collect();
    assert_eq!(whole_chain, ["middle", "top"]);

    std::fs::remove_dir_all(&dir).ok();
}
```

Against the real database, this is exactly the shape of climbing from a
specific noun sense up to `entity` — shown in the crate's own doctest, `no_run`
here because it needs the real dictionary:

```rust no_run
use verbora_wordnet::{WordNet, pointer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wn = WordNet::open("node_modules/wordnet-db/dict")?;
    let node = wn.get(3_832_647.0, "n")?;
    for parent in wn.closure(&node, pointer::HYPERNYM).take(5) {
        println!("^ {}", parent?.def);
    }
    Ok(())
}
```

## Advanced usage

### Concurrency

`WordNet`, `IndexFile` and `DataFile` are immutable after construction
and `Send + Sync`. Nothing is cached per query and nothing is locked — the
reference is the same in substance, since it opens and closes a descriptor per
operation and caches nothing either — so any number of threads can query one
shared dictionary concurrently with no coordination:

```rust
use verbora_wordnet::WordNet;
use std::sync::Arc;

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-concurrency-{}-{:?}",
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
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WordNet>();

    let dir = tiny_dict();
    let wn = Arc::new(WordNet::open(&dir).unwrap());
    let expected = wn.lookup("bbb").unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let wn = Arc::clone(&wn);
            let expected = expected.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    assert_eq!(wn.lookup("bbb").unwrap(), expected);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    std::fs::remove_dir_all(&dir).ok();
}
```

This is the same "share one `Arc`, fan out read-only queries" pattern
[Parallelism](../performance/parallelism) describes for `Trie`: build or open
the dictionary once, wrap it in an `Arc`, and drive queries from as many
threads as you like — with your own `rayon` parallel iterator over any
traversal shown above, or with [`par_lookup_batch`](#par-lookup-batch) for the
one traversal (`lookup`) that ships a built-in fan-out behind the `parallel`
Cargo feature.

### Why there is no `mmap` backend

`Storage` offers four strategies and deliberately no fifth, memory-mapped
one. `mmap` cannot be reached from safe Rust without an extra dependency
(`memmap2`) or `unsafe` code, and this workspace admits neither — no
`unsafe_code` anywhere in the crate, and no dependency outside the curated
`[workspace.dependencies]` list. `Storage::LazyResident` is offered
specifically to cover the case `mmap` is usually reached for: near-zero
startup, in-memory cost once a file is touched — at the honest cost of paying
for the whole file the first time any part of it is read, rather than the
kernel paging it in on demand.

## Deliberate divergences <span class="badge badge-fallible">FALLIBLE</span>

Every variant below corresponds to a case where the reference does not
*return* anything at all — it hangs, or throws from inside an `fs` callback
where no `try`/`catch` at the call site can see it. Reproducing those literally
would make the crate unusable, so each becomes an `Error` instead, and each
is exercised by the fixture, which records the reference's own outcome so the
mapping cannot drift silently. This is the same "faithful, not flattering"
honesty this site applies everywhere correctness and safety pull in different
directions — | | Reference | Here |
|---|---|---|
| **W1** | unopenable file: logs a message, callback never fires | `Error::Io`, reported at open rather than at the first query |
| **W2** | offset past EOF: the line-reader recurses forever, doubling its buffer until the process dies | `Error::UnterminatedLine` |
| **W3** | a record with no `'\| '` gloss separator: async `TypeError` reading `'split'` on `undefined` | `Error::MissingGloss` |
| **W4** | an absurd word or pointer count: loops that many times, pushing `undefined` until the process runs out of memory | `Error::CountTooLarge`, refused beyond `numfmt::MAX_COUNT` |
| **W5** | `fs.statSync` on every `find`, so a file changing size mid-session changes the search path | the length is recorded once, at open |
| **W6** | a descriptor opened and closed per operation — 61 for `lookup('run')` | one handle (or none, for `Storage::Resident`) for the process lifetime |
| **W7** | a negative probe position: `fs.read` throws `ERR_OUT_OF_RANGE`, nothing is delivered | `Error::NegativeProbe` — verified unreachable across all 147,580 keys the fixture exercises, but reported rather than silently clamped |

Two more divergences are forced by the type system rather than chosen:

- **`lookup(123)` cannot be written.** The reference's `WordNet#lookup(123,
  cb)` throws `word.toLowerCase is not a function` at call time; here the
  parameter is `&str`, so the equivalent mistake is a compile error instead of
  a runtime one.
- **`getSynonyms`'s truthiness dispatch becomes two named methods** — see
  [`get_synonyms` vs `get_synonyms_of`](#the-core-api-surface) above.

### The index search is deliberately incomplete

`IndexFile::find` bisects **byte positions**, not lines. Each probe snaps
backwards to the start of the line it landed in, compares that line's first
token to the search key, and halves the step — and because the snap-back
changes which line a position denotes, the invariant a binary search needs does
not hold. Measured against the shipped WordNet 3.1 database:

| File | Lemmas | Reported missing |
|---|---:|---:|
| `index.adv` | 4,475 | 20 |
| `index.verb` | 11,540 | 183 |
| `index.adj` | 21,499 | 117 |
| `index.noun` | 117,953 | 624 |

`index.verb` misses the **entire head of the file** — `aah`, `abandon`,
`abase`, `abate` are all reported missing — and `awful`, `safely`, `such`,
`bitter` and `firm` are adverbs the reference cannot find. This is not a
subtle inefficiency to route around; a search that finds every lemma is a
*different program*, and the reference's own specs assert results that depend
on the incomplete one. Reproducing it exactly is the single highest-value
correctness requirement in this crate.

## Performance characteristics

`crates/verbora-wordnet/benches/wordnet.rs` is a Criterion suite comparing the
four `Storage` strategies against each other across five dimensions:

| Group | Question |
|---|---|
| `open` | startup cost — what a one-shot process pays before its first answer |
| `cold` | open plus one lookup — the honest cost of a single query |
| `lookup` | steady-state per-query latency on a warm dictionary |
| `repeat` | throughput over a realistic word list |
| `footprint` | resident bytes per strategy, reported as a `Throughput` so it lands in the report |

These benches need the real, separately licensed database and skip cleanly
when `$WORDNET_DB_PATH` is unset, rather than failing a build over a missing
licensed asset.

<div class="callout callout-note">
<strong>Not yet benchmarked against the reference.</strong> Unlike
<code>verbora-distance</code>, there is currently no recorded reference
baseline or joined comparison table for WordNet access. See
<a href="../benchmarks/index">Benchmarks</a> for what has and has not been
measured across the workspace, and reproduce the in-tree numbers yourself with
<code>cargo bench -p verbora-wordnet</code> against an installed dictionary.
</div>

## Allocation behaviour

**At open.** `Storage::Resident` and `Storage::Indexed` each read the whole
file into one `Box<[u8]>` per file (eight files total); `Storage::Indexed`
additionally allocates one `u32` per line. `Storage::Pread` and
`Storage::LazyResident` allocate nothing at open beyond the `File` handles
themselves — `LazyResident`'s `Box<[u8]>` is allocated lazily, the first time
that file is actually queried.

**Per query, on the eager path.** `WordNet::get` and `WordNet::lookup`
return owned `DataRecord`s: one `String` (or `Option<String>`) per textual
field, one `Vec` for `synonyms`, one for `ptrs`, one for `exp`. A synset with
many synonyms and pointers — `run` has senses with a dozen or more of each —
allocates proportionally.

**Per query, on the borrowed path** <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>**.**
`DataFile::with_record` hands a
`DataRecordRef` to a closure instead: every string field except the cleaned
examples is a subslice of the line being parsed, so reading a synset allocates
**only** for the examples that actually contain a quote or a whitespace run
needing cleanup. This is the primitive `DataFile::get` is built on.

**Relation traversal.** `WordNet::pointers` and `relation` allocate nothing
of their own beyond the `DataRecord` each hop reads — they borrow the pointer
slice already on your record. `WordNet::closure` additionally carries a
`VecDeque<Pointer>` work queue and a `Vec<u64>` of visited offsets, both
growing with the size of the reachable subgraph rather than the whole
dictionary.

**`IndexFile::find`.** Each probe allocates one `String` for the line it reads
(or reuses a `scratch: Vec<u8>` on the positioned-read backends); nothing is
retained once the bisection stops, aside from the winning line.

There is no `_into` variant and no caller-supplied output buffer anywhere in
this crate. See [Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Unicode and language notes

- **String comparison during the bisection is UTF-16 code-unit order**
  <span class="badge badge-utf16">UTF-16</span>
  (`whitespace::value_lt`), matching the reference's `<` rather than Rust's UTF-8 byte
  `Ord`. The two disagree for supplementary-plane characters, which decides
  which way a probe turns. - **Lookup normalisation is full Unicode lowercasing**, not
  `str::to_ascii_lowercase`: `'İSTANBUL'` lowercases to nine code units from
  eight, and `'ΟΔΟΣ'` produces a final sigma. Whitespace runs — the reference's
  `\s` set, which is not Rust's `char::is_whitespace` (it excludes U+0085 and
  includes U+FEFF) — collapse to a single `_`, so `"  entity  "` becomes
  `"_entity_"`, which then misses.
- **Line splitting on `/\s+/` keeps empty edge fields**, which is what makes
  `find("")` a hit on a WordNet licence-header line: the header starts with
  two spaces, so its first token is `""`.
- **Decoding is lossy, not fallible.** A line's bytes are converted with
  `String::from_utf8_lossy`, substituting U+FFFD for invalid bytes, matching
  the reference's own `buff.toString('UTF-8')` rather than erroring.
- **Node's `path.join` semantics are reproduced** for the paths this crate
  reports back (`IndexFile::file_path`, `DataFile::file_path`), including
  normalising `.`, `..` and duplicate separators — plain
  [`Path::join`](https://doc.rust-lang.org/std/path/struct.Path.html#method.join)
  does not.

## Common mistakes

**Assuming the database ships with the crate, or with any Verbora crate.** It
does not, ever. `WordNet::open` on a missing directory fails immediately with
`Error::Io` — no partial dictionary, no silent stall:

```rust
use verbora_wordnet::{Error, WordNet};

fn main() {
    assert!(matches!(WordNet::open("/no/such/dir"), Err(Error::Io { .. })));
}
```

**Passing a `Pos` name instead of its one-letter tag.** `WordNet::get` and
`WordNet::get_synonyms` take `"n"`, `"v"`, `"a"`, `"s"` or `"r"` — matching
is case-sensitive and there is no `"noun"`, `"N"`, or default:

```rust
use verbora_wordnet::{Error, WordNet};

fn tiny_dict() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-docs-postag-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let index = "aaa n 1 0 1 0 00000000  \n";
    let data = "00000000 06 n 01 alpha 0 000 | the first letter  \n";
    for pos in ["noun", "verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), index).unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), data).unwrap();
    }
    dir
}

fn main() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    assert!(matches!(wn.get(0.0, "noun"), Err(Error::UnknownPos(_))));
    assert!(matches!(wn.get(0.0, "N"), Err(Error::UnknownPos(_))));
    assert!(wn.get(0.0, "n").is_ok());
    std::fs::remove_dir_all(&dir).ok();
}
```

**Treating a "miss" from `lookup`/`find` as proof the word is not in the
database.** The bisection is incomplete by construction — see
[The index search is deliberately incomplete](#the-index-search-is-deliberately-incomplete).
A miss means only that this search could not reach the word, which is not the
same claim as "not present".

**Expecting `lookup` or `get_synonyms` to come back in the order the index
line lists.** Both reverse it — see
[Result order is defined by a bug](#result-order-is-defined-by-a-bug). Number
senses with `WordNet::query_sense` instead of the position a synset arrives
in from `lookup`.

**Calling `get_synonyms_of` and expecting it to reuse your record's own
pointers.** It re-reads the synset from disk by offset and tag, exactly as the
reference does, so a record you built or mutated yourself is not what gets
followed. Use `WordNet::pointers` when you want to follow the pointers
already on the value in hand.

**Mixing up `WordNet::get`'s two error paths.** `Error::UnknownPos` means the
*tag* was not one of `n`/`v`/`a`/`s`/`r`; `Error::MissingGloss` and
`Error::UnterminatedLine` mean the tag resolved fine but the *offset* landed
somewhere that is not a well-formed record start. Both are reachable from
ordinary-looking mistakes — a hand-typed offset, or an offset copied from the
wrong file's dictionary.

## Related

- [Choosing an API](../choosing/index) — the cross-crate version of the
  decision trees on this page.
- [Parallelism](../performance/parallelism) — the shared, `Arc`-wrapped,
  read-only-query pattern this page reuses from `Trie`.
- [Iterator vs. `_into`](../performance/iterator-vs-into) — the lazy/eager
  distinction behind `lookup` vs. `lookup_iter` and `relation` vs. `closure`.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy) — what "borrowed" means for
  `DataRecordRef`.
  way it does.
- [Benchmarks](../benchmarks/index) — what has and has not been measured.
- [Inflectors](./inflectors) — normalise a word to a headword before looking
  it up here.
- [Core traits](./core) — the shared vocabulary the rest of the workspace uses.
- [Recipes](../recipes/index) — end-to-end pipelines.

## API reference

Everything the crate exports:

```rust ignore
// verbora_wordnet
pub struct WordNet { /* private */ }
pub struct Config { pub storage: Storage, pub prebuilt: Option<PathBuf> }
pub struct FilePair<'a> { pub index: &'a IndexFile, pub data: &'a DataFile }
pub enum Pos { Noun, Verb, Adj, Adv }

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
    pub fn lookup_from_files(&self, files: &mut Vec<FilePair<'_>>, results: &mut Vec<DataRecord>, word: &str) -> Result<()>;
    pub fn push_results(&self, data: &DataFile, results: &mut Vec<DataRecord>, offsets: &mut Vec<f64>) -> Result<()>;

    pub fn get(&self, synset_offset: f64, tag: &str) -> Result<DataRecord>;
    pub fn get_at(&self, synset_offset: f64, pos: Pos) -> Result<DataRecord>;

    pub fn lookup_synonyms(&self, word: &str) -> Result<Vec<DataRecord>>;
    pub fn get_synonyms(&self, synset_offset: f64, tag: &str) -> Result<Vec<DataRecord>>;
    pub fn get_synonyms_of(&self, record: &DataRecord) -> Result<Vec<DataRecord>>;
    pub fn load_synonyms(&self, synonyms: &mut Vec<DataRecord>, results: &mut Vec<DataRecord>, ptrs: &mut Vec<Pointer>) -> Result<()>;
    pub fn load_result_synonyms(&self, synonyms: &mut Vec<DataRecord>, results: &mut Vec<DataRecord>) -> Result<()>;

    pub fn pointers<'a>(&'a self, record: &'a DataRecord) -> Pointers<'a>;
    pub fn relation<'a>(&'a self, record: &'a DataRecord, symbol: &'a str) -> Pointers<'a>;
    pub fn closure<'a>(&'a self, record: &DataRecord, symbol: &'a str) -> Closure<'a>;

    // requires the `parallel` Cargo feature
    pub fn par_lookup_batch(&self, words: &[&str]) -> Vec<Result<Vec<DataRecord>>>;

    // sense.rs — extensions with no reference counterpart
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

// data_file
pub struct DataFile { /* private */ }
pub struct DataRecord { pub synset_offset: f64, pub lex_filenum: f64, pub pos: Option<String>,
    pub w_cnt: f64, pub lemma: Option<String>, pub synonyms: Vec<Option<String>>,
    pub lex_id: Option<String>, pub ptrs: Vec<Pointer>, pub gloss: String, pub def: String,
    pub exp: Vec<String> }
pub struct DataRecordRef<'a> { /* borrowed mirror of DataRecord */ }
pub struct Pointer { pub pointer_symbol: Option<String>, pub synset_offset: f64,
    pub pos: Option<String>, pub source_target: Option<String> }
pub struct PointerRef<'a> { /* borrowed mirror of Pointer */ }

impl DataFile {
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self>;
    pub fn file_path(&self) -> &str;
    pub fn path(&self) -> PathBuf;
    pub fn source(&self) -> &Source;
    pub fn with_record<R>(&self, offset: f64, f: impl FnOnce(&DataRecordRef<'_>) -> R) -> Result<R>;
    pub fn get(&self, offset: f64) -> Result<DataRecord>;
}
pub fn parse_data_line(line: &str) -> std::result::Result<DataRecordRef<'_>, ParseError>;

// index_file
pub struct IndexFile { /* private */ }
pub enum Find { Hit(IndexHit), Miss }
pub struct IndexHit { /* private */ }
pub struct IndexRecord { pub lemma: Option<String>, pub pos: Option<String>,
    pub ptr_symbol: Vec<Option<String>>, pub sense_cnt: f64, pub tagsense_cnt: f64,
    pub synset_offset: Vec<f64> }
pub struct Probe { pub position: i64, pub adjustment: i64, pub key: String }
pub struct Probes<'a> { /* private */ }

impl IndexFile {
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self>;
    pub fn file_path(&self) -> &str;
    pub fn path(&self) -> PathBuf;
    pub fn source(&self) -> &Source;
    pub fn probes<'a>(&'a self, search_key: &'a str) -> Probes<'a>;
    pub fn find(&self, search_key: &str) -> Result<Find>;
    pub fn lookup(&self, word: &str) -> Result<Option<IndexRecord>>;
}
pub fn parse_index_line(line: &str) -> Result<IndexRecord>;

// source
pub enum Storage { Pread, LazyResident, Resident /* default */, Indexed }
pub struct Source { /* private */ }
impl Source {
    pub fn open(path: &Path, storage: Storage) -> Result<Self>;
    pub fn len(&self) -> u64;
    pub fn is_empty(&self) -> bool;
    pub fn path(&self) -> &Path;
    pub fn line_starts(&self) -> Option<&[u32]>;
    pub fn prev_eol(&self, pos: i64) -> Result<u64>;
    pub fn line_at<'a>(&'a self, start: u64, scratch: &'a mut Vec<u8>) -> Result<&'a [u8]>;
}
pub fn build_line_starts(bytes: &[u8]) -> Box<[u32]>;

// prebuilt
pub struct PrebuiltIndex { /* private */ }
impl PrebuiltIndex {
    pub fn build(dict_dir: impl AsRef<Path>) -> Result<Self>;
    pub fn to_bytes(&self) -> Vec<u8>;
    pub fn from_bytes(bytes: &[u8]) -> Result<Self>;
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()>;
    pub fn load(path: impl AsRef<Path>) -> Result<Self>;
    pub fn source_for(&self, name: &str, path: &Path) -> Result<Source>;
    pub fn names(&self) -> Vec<&str>;
    pub fn line_count(&self) -> usize;
    pub fn byte_size(&self) -> usize;
    pub fn default_path(dict_dir: impl AsRef<Path>) -> PathBuf;
}

// sense
pub struct Sense { pub lemma: String, pub pos: Pos, pub number: Option<usize> }
pub struct ParseSenseError(/* private */);
impl std::str::FromStr for Sense { type Err = ParseSenseError; }
impl std::fmt::Display for Sense { /* "lemma#pos[#n]" */ }

// pointer — relation-symbol constants and their descriptions
pub const ANTONYM: &str; pub const HYPERNYM: &str; pub const INSTANCE_HYPERNYM: &str;
pub const HYPONYM: &str; pub const INSTANCE_HYPONYM: &str; pub const MEMBER_HOLONYM: &str;
pub const SUBSTANCE_HOLONYM: &str; pub const PART_HOLONYM: &str; pub const MEMBER_MERONYM: &str;
pub const SUBSTANCE_MERONYM: &str; pub const PART_MERONYM: &str; pub const ATTRIBUTE: &str;
pub const DERIVATIONALLY_RELATED: &str; pub const DOMAIN_TOPIC: &str; pub const MEMBER_TOPIC: &str;
pub const DOMAIN_REGION: &str; pub const MEMBER_REGION: &str; pub const DOMAIN_USAGE: &str;
pub const MEMBER_USAGE: &str; pub const ENTAILMENT: &str; pub const CAUSE: &str;
pub const ALSO_SEE: &str; pub const VERB_GROUP: &str; pub const SIMILAR_TO: &str;
pub const PARTICIPLE: &str; pub const PERTAINYM: &str;
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
`Send + Sync`; nothing here depends on what was looked up before.
`WordNet::par_lookup_batch` is the crate's only parallel entry point, gated
behind the `parallel` Cargo feature and off by default — see
[`par_lookup_batch`](#par-lookup-batch) above and
[Parallelism](../performance/parallelism).
