# verbora-wordnet

Read the WordNet lexical database from Rust. WordNet groups English nouns,
verbs, adjectives and adverbs into *synsets* — sets of words sharing one sense —
and records the relations between them (Fellbaum, *WordNet: An Electronic
Lexical Database*, MIT Press, 1998). This crate parses the eight `index.*` and
`data.*` files whose layout the `wndb(5WN)` and `wninput(5WN)` manual pages
specify, and gives you senses, glosses, pointers and breadth-first relation
closures over them.

**This crate ships no dictionary data.** WordNet is separately licensed by
Princeton University, so the files are read at run time from a directory you
supply: download a WordNet 3.0 or 3.1 distribution, then point `WordNet::open`
at the `dict` directory inside it, or set `WORDNET_DB_PATH` and call
`WordNet::from_env`. Princeton's licence is reproduced verbatim in
`LICENSE-WORDNET` beside this crate and must accompany all copies of the
database.

## What it guarantees

Index searching is **byte-wise**, because `wndb(5WN)` specifies index files as
sorted in the ASCII collating sequence and comparing decoded scalars would
silently disagree on a corrupt file; content comes back as `&str`, decoded from
UTF-8 with invalid bytes replaced by `U+FFFD`, so one bad byte costs one
character of one gloss rather than the whole record. Everything is yielded in
the order the files list it — categories noun, verb, adjective, adverb; senses in
the index line's own most-frequently-tagged-first order. **A word with no entry
is `None` or an empty `Vec`, never an error and never a sentinel**; a missing
file, a bad offset or a record that does not match the documented format is an
`Error`, reported rather than papered over. No public function panics on any
input. Normalisation is named rather than hidden: turning what a user typed into
the lower-case, underscore-joined spelling the index is keyed on is `index_key`,
and each entry point documents whether it applies it or takes its argument
verbatim. There is no morphological reduction here — this crate looks up the
lemma you give it.

### Choosing a storage strategy

`Storage` decides how bytes reach the parser. All four answer every query
identically; picking the wrong one is the main way to be disappointed by this
crate.

| `Storage` | Startup | Per query | Resident memory | Choose it for |
|---|---|---|---|---|
| `Pread` | none | positioned reads, a handful of syscalls | none | a short-lived process asking one or two questions |
| `LazyResident` | none | in memory once the file is first touched | grows to the files you actually use | startup latency matters *and* steady state does |
| `Resident` *(default)* | reads the dictionary | in memory | the whole dictionary | a long-lived process answering many queries |
| `Indexed` | + one newline scan | line starts by `partition_point` | + four bytes per line | high query rates; the line table can be persisted with `PrebuiltIndex` |

Start with the default, `Resident`. **There is no memory-mapped backend and
there will not be one**: `mmap` needs either `unsafe` or a dependency that uses
it, and this workspace sets `unsafe_code = "deny"`. `LazyResident` is the
declared stand-in — it is the closest safe analogue, paying for a file the first
time it is touched instead of at open.

These are qualitative descriptions, not measurements; no timing figures are
published because none have been taken against this implementation.

`WordNet` is immutable after construction and `Send + Sync`, so one instance can
be queried concurrently from many threads with no lock and no per-query cache.

## Example

```rust,no_run
use verbora_wordnet::{PartOfSpeech, PointerSymbol, WordNet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The `dict` directory of a WordNet distribution you downloaded.
    let wn = WordNet::open("/usr/share/wordnet")?;

    // Every sense of "node", in the order WordNet numbers them.
    for (i, synset) in wn.senses("node", PartOfSpeech::Noun)?.iter().enumerate() {
        println!("node#n#{}: {}", i + 1, synset.gloss.definition);
    }

    // Walk up the hypernym chain from one numbered sense.
    let node = wn.sense(&"node#n#1".parse()?)?.expect("node has senses");
    for parent in wn.closure(&node, PointerSymbol::Hypernym).take(5) {
        println!("^ {}", parent?.lemma());
    }

    // A word with no entry is absence, not an error.
    assert!(wn.lookup("zzzznotaword")?.is_empty());
    Ok(())
}
```

## See also

- Full documentation: <https://verbora.dev/features/wordnet>
- [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers) — reducing
  *running* to *run* before you look it up; this crate deliberately does not,
  so a lookup never quietly answers about a different word.
- [`verbora-inflectors`](https://crates.io/crates/verbora-inflectors) — going the
  other way, from a lemma to its inflected forms.
- [`verbora-trie`](https://crates.io/crates/verbora-trie) — if what you wanted
  was prefix search over your own word list rather than a lexical database.
