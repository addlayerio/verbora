# Phonetic neighbors

`PhoneticIndex` answers one question over a whole dictionary at once: *which
stored entries share a phonetic code with this query?* Every encoder in
[Phonetics](phonetics.md) can already tell you whether two specific words sound
alike; this type builds an index over thousands of them so a caller can ask
that question of the whole dictionary in roughly the time one query's own
encoding takes.

## When to use it

- **Blocking a fuzzy-match pipeline over a real dictionary**, not just a
  handful of strings. [`SoundEx::compare`](phonetics.md#compare-versus-comparing-two-process-results)-style
  pairwise checks are fine for a few dozen names; once the dictionary reaches
  thousands of entries, build one `PhoneticIndex` and query it repeatedly
  instead of re-encoding the whole dictionary per lookup.
- **Read-heavy, build-once workloads.** `PhoneticIndex` is immutable, lock-free
  and `Send + Sync` — share it behind `Arc` across threads or requests without
  any synchronisation.
- **Feeding a ranking step.** `neighbors()` produces candidates; composing the
  output with a string metric from
  [`verbora-distance`](https://docs.rs/verbora-distance) is the intended next
  step — see [Ranking the candidates](#ranking-the-candidates-with-verbora-distance)
  below.

## When not to use it

- **A search engine.** `neighbors()` does not rank, does not apply an
  edit-distance threshold, and does not accept a query language. It returns
  every entry that shares a code with the query, in no particular order beyond
  "ascending id within a merge run" — nothing more.
- **A handful of comparisons.** Building an index has a real, measured cost
  (see [Performance characteristics](#performance-characteristics)); for a
  one-off "do these two words sound alike?" call, use the encoder's own
  `process`/`compare` directly.
- **Non-English text**, for the same reasons the underlying encoders don't fit
  it — see [Phonetics § Unicode and language notes](phonetics.md#unicode-and-language-notes).
  Indexing doesn't change what the encoders themselves understand.
- **Persistence.** There is no `to_json`/`from_json` today — see
  [Persistence](#persistence) below.

## Quick example

The full Build → Freeze → Query lifecycle, with `SoundEx`:

```rust
use verbora_phonetics::{PhoneticIndexBuilder, SoundEx};

fn main() {
    // BUILD: insert as many entries as you like, in any order.
    let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
    builder.insert("Smith");
    builder.insert("Smyth");
    builder.insert("Johnson");

    // FREEZE: compact, immutable, Send + Sync — share it behind an Arc.
    let index = builder.build();

    // QUERY: lazy, allocation-free beyond encoding the query itself.
    let neighbors: Vec<&str> = index.neighbors("Smith").collect();
    assert!(neighbors.contains(&"Smith"));
    assert!(neighbors.contains(&"Smyth")); // "Smith" and "Smyth" share a SoundEx code
    assert!(!neighbors.contains(&"Johnson"));
}
```

## The Build → Freeze → Query shape

The same pattern Verbora uses everywhere a structure is built once and queried
many times (`verbora-tagger`'s lexicons, `verbora-wordnet`'s `PrebuiltIndex`):
a mutable builder on one side, a compact immutable structure on the other.

| Stage | Type | Mutability | Cost model |
|---|---|---|---|
| Build | `PhoneticIndexBuilder<E>` | `&mut self`, insert in any order | One `Box<str>` per entry, plus growable `Vec`s |
| Freeze | `PhoneticIndexBuilder::build(self)` | consumes the builder | One sort of every `(code, id)` row, then three fixed-size arrays |
| Query | `PhoneticIndex<E>` | `&self` only, no interior mutability | Binary search + lazy iteration, no locks |

Freezing buys two things. **Concurrency:** the builder holds growable `Vec`s,
which need a lock to share; `PhoneticIndex` holds only `Box<[T]>`, so
`Arc<PhoneticIndex<E>>` gives every reader lock-free access. **Lookup shape:**
`build()` pays one `sort_unstable()` so every later `bucket()` is a binary
search instead of a linear scan. Every method on the frozen side takes `&self`,
which is what makes the `Arc` sharing sound.

```rust
use std::sync::Arc;
use std::thread;

use verbora_phonetics::{PhoneticIndexBuilder, SoundEx};

fn main() {
    let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
    for name in ["Robert", "Rupert", "Smith", "Smythe"] {
        builder.insert(name);
    }
    let index = Arc::new(builder.build()); // FREEZE, then share

    let mut handles = Vec::new();
    for _ in 0..4 {
        let index = Arc::clone(&index);
        handles.push(thread::spawn(move || index.neighbors("Robert").count()));
    }
    for h in handles {
        assert_eq!(h.join().unwrap(), 2); // Robert, Rupert — no lock needed
    }
}
```

## Choosing the right API

Two independent choices: **which encoder** to index with, and **which query
method** — `neighbors()` or `bucket()`.

### Which encoder

`PhoneticEncoder` is implemented for three encoders, keyed by
how many codes each one produces per entry:

| Encoder | `PhoneticEncoder::Code` | Codes per entry | Buckets an entry occupies |
|---|---|:--:|:--:|
| `SoundEx` | `SoundexCode` (`InlineCode<4>`) | 1 | 1 |
| `Metaphone` | `MetaphoneCode` (`InlineCode<128>`) | 1 | 1 |
| `DoubleMetaphone` | `MetaphoneCode` (`InlineCode<128>`) | **2** | 2 |

The other encoders in [`verbora-phonetics`](phonetics.md) implement
`verbora_core::Phonetic` but not `PhoneticEncoder`, so they answer
"what is this word's key?" without plugging into the dictionary index. To
block a dictionary under one of them, group by `process()` yourself.

The general trade-offs between encoders — coarser vs. tighter buckets, which
languages each one fits — don't change when you index rather than call
`process` directly; see
[Phonetics § Decision tree](phonetics.md#decision-tree-—-which-encoder). The one
index-specific consideration is `DoubleMetaphone`'s two codes: every entry it
encodes occupies **two** buckets, and `neighbors()` unions and deduplicates
across them automatically, so indexing under it costs roughly double the rows
for meaningfully wider recall (matches multi-origin names other encoders
would put in different buckets entirely).

```rust
use verbora_phonetics::{DoubleMetaphone, PhoneticIndexBuilder};

fn main() {
    let mut builder = PhoneticIndexBuilder::new(DoubleMetaphone::new());
    builder.insert("Smith"); // primary "SM0", alternate "XMT": two distinct codes
    let index = builder.build();

    // Matches via EITHER code, and appears exactly once even though it
    // occupies two buckets.
    let hits: Vec<&str> = index.neighbors("Smith").collect();
    assert_eq!(hits, vec!["Smith"]);

    // "Schmidt"'s primary key is "Smith"'s alternate, so it finds it.
    let hits: Vec<&str> = index.neighbors("Schmidt").collect();
    assert_eq!(hits, vec!["Smith"]);
}
```

### `neighbors()` versus `bucket()`

| | `neighbors(query)` | `bucket(code)` |
|---|---|---|
| Input | a query `&str` (encodes it for you) | an already-computed `E::Code` |
| Output | `impl Iterator<Item = &str>` (lazy) | `&[EntryId]` (a raw slice) |
| Handles a dual-code encoder's union + dedup | ✅ | ❌ — one code only |
| Resolves ids back to text | ✅, via `PhoneticIndex::get` internally | ❌ — caller calls `get` per id |
| Length known without iterating | ❌ (would have to drain) | ✅, `O(1)` `.len()` |
| Allocation | one `String` (encoding the query) | none — `code` must already exist |
| Best for | the default: correct for every encoder, including `DoubleMetaphone` | you already hold a code (e.g. reusing one you computed elsewhere) and want the raw ids without the merge machinery |

`bucket()` is the lower-level primitive `neighbors()` is built from — for a
single-code encoder it is `neighbors()` minus the encoding step and minus the
id-to-`&str` resolution, at the cost of doing both yourself:

```rust
use verbora_phonetics::{PhoneticCodes, PhoneticEncoder, PhoneticIndexBuilder, SoundEx};

fn main() {
    let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
    builder.insert("Robert");
    builder.insert("Rupert");
    builder.insert("Smith");
    let index = builder.build();

    let soundex = SoundEx::new();
    let PhoneticCodes::One(code) = soundex.encode("Robert") else {
        unreachable!("SoundEx is single-code")
    };

    let ids = index.bucket(code); // O(1)-length raw slice, no allocation
    assert_eq!(ids.len(), 2); // Robert, Rupert
    let names: Vec<&str> = ids.iter().map(|&id| index.get(id)).collect();
    assert!(names.contains(&"Robert"));
    assert!(names.contains(&"Rupert"));
}
```

`neighbors()` behaves the same for every encoder, and that uniformity has a
price: it always runs the two-bucket union-and-dedup machinery
`DoubleMetaphone` requires, even when only one bucket is ever in play, and it
has to drain its iterator to produce a count. Reach for `bucket()` when you
already hold a single code and want raw ids — it skips the encoding, the merge,
and the drain, giving an `O(1)` `.len()` instead.

## Advanced usage

### Ranking the candidates with `verbora-distance`

This is the exact composition this feature exists for — candidate generation
feeding a scoring step, not a replacement for one:

```rust
use verbora_distance::jaro_winkler;
use verbora_phonetics::{PhoneticIndexBuilder, SoundEx};

fn main() {
    let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
    for name in ["Smith", "Smyth", "Smithe", "Jones", "Johnson"] {
        builder.insert(name);
    }
    let index = builder.build();

    // neighbors() generates candidates; verbora-distance scores them.
    // Composed at the call site, exactly as the module's own documentation
    // describes it — PhoneticIndex never calls into verbora-distance itself.
    let mut ranked: Vec<(&str, f64)> = index
        .neighbors("Smith")
        .map(|candidate| (candidate, jaro_winkler("Smith", candidate)))
        .collect();
    // The score is always a finite f64, so `total_cmp` orders it outright.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));

    assert_eq!(ranked[0].0, "Smith"); // exact match ranks first
    assert!(ranked.iter().any(|&(name, _)| name == "Smyth"));
    assert!(!ranked.iter().any(|&(name, _)| name == "Jones")); // different code, never a candidate
}
```

`PhoneticIndex` deliberately has no `rank`/`search` method of its own — see
[Phonetics § Ranking](phonetics.md#when-not-to-use-it) for why a phonetic key
is "a yes/no bucket, not a score." The [Fuzzy name matching](../recipes/fuzzy-matching.md)
recipe walks through the same three-step shape (bucket → rank → threshold) in
more depth, including how to pick a distance metric for the ranking step.

### Persistence

`PhoneticIndex` has no `to_json`/`from_json`, and `verbora-phonetics` carries
no `serde` dependency. Rebuild the index at startup from your own stored
dictionary — at realistic sizes that costs milliseconds, not seconds (see
[Build cost](#build-cost)).

## Performance characteristics

<div class="callout callout-warn">
<strong>Pending re-measurement.</strong> Every number below was recorded before
0.2.0, and no run has been made against the code as it now stands. They are
retained only until a fresh full-precision run replaces them — no number here
should be quoted as the library's present performance, and the shape of each
table, not the values, is the part worth reading.
</div>

<div class="callout callout-note">
<strong>Machine-dependent.</strong> Every number below came from
<code>cargo bench -p verbora-phonetics --bench phonetic_index</code> on one
development machine. Ratios and orders of magnitude should hold; exact
nanosecond figures will not reproduce identically on different hardware. See
that file's own module documentation for the full methodology.
</div>

### Build cost

Time to insert every entry and freeze, `SoundEx` (single-code) and
`DoubleMetaphone` (dual-code, so roughly twice the rows to sort):

| Entries | `SoundEx` | Per entry | `DoubleMetaphone` | Per entry |
|---:|---:|---:|---:|---:|
| 1,000 | 130.7 µs | 131 ns | 356.8 µs | 357 ns |
| 10,000 | 1.531 ms | 153 ns | 4.156 ms | 416 ns |
| 100,000 | 17.64 ms | 176 ns | 53.33 ms | 533 ns |

Per-entry cost grows slowly with dictionary size because `build()` sorts every
accumulated `(code, id)` row — `O(n log n)` — before compacting; this is a
one-time cost paid once per index, not per query.

### Query latency

`neighbors()`, fully drained (`.count()`), at three scenarios: a query that
hits an entry once, a query that misses entirely, and a query that lands in a
wide bucket sized to scale with the dictionary (1% of it, floored at 50
matches):

| Entries | Scenario | Matches | `SoundEx` | `DoubleMetaphone` |
|---:|---|---:|---:|---:|
| 1,000 | hit | 1 | 106 ns | 162 ns |
| 1,000 | miss | 0 | 101 ns | 127 ns |
| 1,000 | wide bucket | 50 | 156 ns | 234 ns |
| 10,000 | hit | 1 | 126 ns | 202 ns |
| 10,000 | miss | 0 | 111 ns | 171 ns |
| 10,000 | wide bucket | 100 | 266 ns | 399 ns |
| 100,000 | hit | 1 | 138 ns | 218 ns |
| 100,000 | miss | 0 | 116 ns | 189 ns |
| 100,000 | wide bucket | 1,000 | 1.63 µs | 2.37 µs |

For comparison, encoding the query alone — independent of dictionary size,
since encoding never touches the index — costs 40–58 ns for `SoundEx` and
49–81 ns for `DoubleMetaphone`, depending on the input.

**Encoding dominates for a hit or a miss.** At every dictionary size, the hit
and miss rows above sit within roughly 1–2× encoding's own cost: the binary
search plus a one-or-zero-item merge adds tens of nanoseconds on top of an
encode step that already costs tens of nanoseconds. In the common blocking
scenario — a query matching a handful of entries at most — lookup is
effectively free next to `encode()`.

**Iteration cost scales with the number of matches, not with dictionary
size.** The wide-bucket row is the exception: at 100,000 entries the query
that matches 1,000 entries costs roughly 10× the hit row, not because lookup
got slower but because draining 1,000 items through `Neighbors`' merge-and-dedup
logic costs about 1.5 ns per item — unavoidable, since every match returned
has to be visited to be returned. A caller that only needs the *count*, not
the matched text, can skip this by working with `bucket()` directly for a
single-code encoder — see below.

### Memory footprint

The index stores codes, bucket offsets and entry ids as three flat arrays — a
compressed-sparse-row shape — plus one `Box<str>` per entry. At 100,000
`SoundEx` entries that is **2,899,788 bytes, or 29.0 bytes per entry** — the
smallest of the four layouts benchmarked for this structure, against 31.1
bytes for a frozen `HashMap<Code, Box<[EntryId]>>`, 32.3 for a dense
perfect-hash array and 39.8 for a `String`-keyed `HashMap`.

Codes compare as raw bytes, not as validated `&str`s. UTF-8 validity is
guaranteed at construction, so re-checking it on every comparison would be
pure overhead — and not a small one: at a 100,000-entry wide bucket it costs
3–8× `encode()`'s own cost for a `SoundexCode`, and up to 45× for the wider
`MetaphoneCode`.

Bucket lookup is a binary search over a sorted array (110–134 ns for a hit or a
miss at 100,000 entries) rather than a hash probe — a little lookup latency
traded for the memory footprint above and for lock-free sharing.

Reproduce all of the above with:

```bash
cargo bench -p verbora-phonetics --bench phonetic_index
```

## Allocation behaviour

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>
applies to querying a built index — not to building one, and not to encoding
a query string.

| Operation | Allocates |
|---|---|
| `PhoneticIndexBuilder::insert` | One `Box<str>` for the entry's text, plus amortised `Vec` growth for `entries` and `rows` |
| `PhoneticIndexBuilder::build` | Three `Vec`s (`codes`, `offsets`, `ids`), each converted to a fixed `Box<[_]>` once — no further growth after this point |
| `PhoneticIndex::bucket` | Nothing — a binary search plus a slice |
| `PhoneticIndex::neighbors` | One `String`, from the underlying encoder's `process`/`process_double` — copied into an `InlineCode` and dropped inside `encode()`. Draining the returned iterator, including `.take(n)`, allocates nothing further. |

That one `String` per `neighbors()` call comes from the encoder itself:
`process`/`process_double` returns an owned `String`, and
`PhoneticEncoder::encode` copies its bytes into an `InlineCode` and drops it.
There is no allocation-free encoding path; at realistic dictionary sizes the
encode step, not the bucket lookup, is the cost that matters.

## Common mistakes

**Treating `neighbors()`'s order as a ranking.** It is "ascending id within a
merge run," an artifact of the sorted-merge implementation, not a similarity
order. Rank at the call site — see
[Ranking the candidates](#ranking-the-candidates-with-verbora-distance).

**Forgetting duplicates are preserved.** `insert("Smith")` twice creates two
distinct entries with two distinct `EntryId`s, and both come back from
`neighbors("Smith")`. Deduplicate by text yourself, after querying, if that is
what your data needs — the index can't know whether two identical strings
represent the same real-world record.

**Comparing `EntryId`s across two different indexes.** An `EntryId` is only
meaningful against the specific `PhoneticIndex` that produced it — it's a
plain index into that index's own `entries` array, not a globally unique
identifier.

**Reaching for `bucket()` with a dual-code encoder and only checking one
code.** `bucket()` takes one already-computed code and returns only that
code's matches; for `DoubleMetaphone`, that silently misses every entry that
matches solely via the other code. Use `neighbors()` unless you specifically
want one code's raw bucket.

**Rebuilding the index per query.** `PhoneticIndexBuilder::build()` costs real,
measured milliseconds at realistic dictionary sizes (see
[Build cost](#build-cost)) — build once, share the frozen `PhoneticIndex`
behind `Arc`, and query it as many times as you need.

## Related

- [Phonetics](phonetics.md) — the three encoders this index can be built
  from (`SoundEx`, `Metaphone`, `DoubleMetaphone`), the nine it cannot, and
  how to choose between them, including
  [by language](phonetics.md#choosing-a-phonetic-algorithm).
- [Language](language.md) — one layer up the pipeline: detects script and
  language, then recommends an encoder — which may be one of the three above,
  or `DaitchMokotoff`, `Cologne` or `BeiderMorse`, none of which this index
  accepts. Useful when you don't yet know which encoder you want.
- [String distance](distance.md) — the scoring step that runs on
  `neighbors()`'s output.
- [Fuzzy name matching](../recipes/fuzzy-matching.md) — the bucket → rank →
  threshold recipe this index implements the first step of.
- [Allocation](../performance/allocation.md) — the allocation-free query path
  in more general terms.

## API reference

### Types

| Item | Description |
|---|---|
| `PhoneticIndexBuilder<E>` | The mutable build side. `new(encoder)`, `insert`, `extend`, `reserve`, `len`, `is_empty`, `build(self) -> PhoneticIndex<E>` |
| `PhoneticIndex<E>` | The frozen query side. `len`, `is_empty`, `get(EntryId) -> &str`, `encoder`, `bucket`, `neighbors`. `Send + Sync` whenever `E` is |
| `Neighbors<'a, E>` | The lazy iterator `neighbors()` returns. `Item = &'a str` |
| `PhoneticEncoder` | Trait implemented for `SoundEx`, `Metaphone` and `DoubleMetaphone`. `type Code: Copy + Eq + Hash + Ord`; `encode(&self, &str) -> PhoneticCodes<Self::Code>` |
| `PhoneticCodes<C>` | `One(C)` or `Two(C, C)` — what `encode()` produced. `IntoIterator<Item = C>` |
| `PhoneticCodesIter<C>` | The iterator `PhoneticCodes::into_iter` returns |
| `InlineCode<const N: usize>` | A stack-stored code, up to `N` bytes, `Copy`. `new(&str) -> Option<Self>` (`None` past `N` bytes), `prefix_of(&str) -> Self` (keeps the longest prefix that fits, cut at a character boundary), `as_str(&self) -> &str` |
| `SoundexCode` | `InlineCode<4>` — `SoundEx`'s code type |
| `MetaphoneCode` | `InlineCode<128>` — `Metaphone`'s and `DoubleMetaphone`'s code type |
| `EntryId` | An opaque, `u32`-addressed handle into one `PhoneticIndex`'s entries |

### Methods

| Method | Signature |
|---|---|
| `PhoneticIndexBuilder::new` | `(encoder: E) -> Self` |
| `PhoneticIndexBuilder::insert` | `(&mut self, entry: &str) -> EntryId` |
| `PhoneticIndexBuilder::extend` | `<'a, I: IntoIterator<Item = &'a str>>(&mut self, entries: I)` |
| `PhoneticIndexBuilder::reserve` | `(&mut self, additional: usize)` |
| `PhoneticIndexBuilder::build` | `(self) -> PhoneticIndex<E>` |
| `PhoneticIndex::len` / `is_empty` | `(&self) -> usize` / `-> bool` |
| `PhoneticIndex::get` | `(&self, id: EntryId) -> &str` |
| `PhoneticIndex::encoder` | `(&self) -> &E` |
| `PhoneticIndex::bucket` | `(&self, code: E::Code) -> &[EntryId]` |
| `PhoneticIndex::neighbors` | `<'a>(&'a self, query: &str) -> Neighbors<'a, E>` |
| `PhoneticEncoder::encode` | `(&self, input: &str) -> PhoneticCodes<Self::Code>` |
