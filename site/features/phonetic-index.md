# Phonetic neighbors

`PhoneticIndex` answers one question over a whole dictionary at once: *which
stored entries share a phonetic code with this query?* Every encoder in
[Phonetics](phonetics.md) can already tell you whether two specific words sound
alike; this type builds an index over thousands of them so a caller can ask
that question of the whole dictionary in roughly the time one query's own
encoding takes.

<div class="callout callout-note">
<strong>Verbora-native extension — not a ported feature.</strong>
<code>PhoneticIndex</code> has no counterpart in the reference. What follows is
backed by this workspace's own evidence: 18 unit tests and
2 doctests in <code>crates/verbora-phonetics/src/index.rs</code>, and the
Criterion benchmarks in <code>crates/verbora-phonetics/benches/phonetic_index.rs</code>.
See <a href="phonetics">Phonetics</a> for the four tested encoders
this index is built from.
</div>

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

This is the same pattern this workspace uses everywhere a structure is built
once and queried many times (`verbora-tagger`'s lexicons, `verbora-wordnet`'s
`PrebuiltIndex`): a mutable, convenient builder on one side, and a compact,
immutable, lock-free structure on the other.

| Stage | Type | Mutability | Cost model |
|---|---|---|---|
| Build | `PhoneticIndexBuilder<E>` | `&mut self`, insert in any order | One `Box<str>` per entry, plus growable `Vec`s |
| Freeze | `PhoneticIndexBuilder::build(self)` | consumes the builder | One sort of every `(code, id)` row, then three fixed-size arrays |
| Query | `PhoneticIndex<E>` | `&self` only, no interior mutability | Binary search + lazy iteration, no locks |

**Why freeze at all, instead of querying the builder directly?** Two reasons,
both load-bearing:

1. **Concurrency.** `PhoneticIndexBuilder` holds growable `Vec`s — mutating
   them from multiple threads needs a lock. `PhoneticIndex` holds only
   `Box<[T]>` — nothing to grow, nothing to lock. `Arc<PhoneticIndex<E>>` gives
   every reader the same lock-free access `RwLock<PhoneticIndexBuilder<E>>`
   would need synchronisation for.
2. **Lookup shape.** The builder's rows are insertion-ordered; a query needs
   "every id for this code," which means either a scan or a sorted structure.
   `build()` pays the sort once (`rows.sort_unstable()`), so every subsequent
   `bucket()` call is a binary search instead of a linear scan over the whole
   dictionary.

`PhoneticIndex::bucket` and `PhoneticIndex::neighbors` both take `&self` — no
method on the frozen side needs `&mut`, which is exactly what makes sharing it
behind `Arc` sound.

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

`PhoneticEncoder` is implemented for all four encoders in this crate, keyed by
how many codes each one produces per entry:

| Encoder | `PhoneticEncoder::Code` | Codes per entry | Buckets an entry occupies |
|---|---|:--:|:--:|
| `SoundEx` | `SoundexCode` (`InlineCode<16>`) | 1 | 1 |
| `Metaphone` | `MetaphoneCode` (`InlineCode<128>`) | 1 | 1 |
| `SoundExDM` | `DaitchMokotoffCode` (`InlineCode<6>`) | 1 | 1 |
| `DoubleMetaphone` | `MetaphoneCode` (`InlineCode<128>`) | **2** | 2 |

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
    builder.insert("astromech"); // -> ("ATRMX", "ATRMK"), two distinct codes
    let index = builder.build();

    // Matches via EITHER code, and appears exactly once even though it
    // occupies two buckets.
    let hits: Vec<&str> = index.neighbors("astromech").collect();
    assert_eq!(hits, vec!["astromech"]);
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

Reach for `bucket()` only when you already have a code in hand and specifically
want the `O(1)`-length slice it returns — see
[the honest trade-off](#the-honest-trade-off-uniform-neighbors-vs-a-raw-slice)
below for what that buys you and what it costs.

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
        .map(|candidate| (candidate, jaro_winkler("Smith", candidate, &Default::default())))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

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

Not implemented. `verbora-phonetics`'s `Cargo.toml` carries no `serde`
dependency, and `PhoneticIndex` has no `to_json`/`from_json` — no crate in this
workspace constructs a `PhoneticIndex` outside this crate's own tests and
benchmarks, and `AGENTS.md`'s Data Structures and Archived Data policies both
call for a concrete caller before shipping persistence, not a speculative one.

That absence was checked against the other half of the same question — not
making persistence *impossible* to add later — by compiling a mirror of this
module's generic shape against `serde`, not just reasoning about it. The
result: nothing about `PhoneticIndex`'s fields, `EntryId`, or the
`PhoneticEncoder` trait's bound would need to change. Two small, additive
pieces would: a manual (not derived) `Serialize`/`Deserialize` for
`InlineCode<N>` (its derive fails on a generic `const N` array), and one
`#[serde(bound(...))]` attribute on `PhoneticIndex<E>` (the derive's automatic
bound inference can't see that `E::Code` needs `Serialize` too). Whoever adds
this later should route `Deserialize` through a validating constructor rather
than a bare derive — the frozen `codes`/`offsets`/`ids` arrays carry an
invariant (`codes` ascending, `offsets` monotonic,
`offsets.len() == codes.len() + 1`) that `bucket()` trusts without
re-checking, and a bare derive would accept wire data that violates it.

## Performance characteristics

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
and miss rows above sit within roughly 1–2× encoding's own cost — the binary
search plus a one-or-zero-item merge adds tens of nanoseconds on top of an
encode step that already costs tens of nanoseconds. This is the situation
[the module's own documentation](https://docs.rs/verbora-phonetics) describes
as "bucket lookup and neighbor iteration cost approximately nothing next to
`encode()` itself" — true for the common blocking scenario, where a query
matches a handful of entries at most.

**Iteration cost scales with the number of matches, not with dictionary
size.** The wide-bucket row is the exception: at 100,000 entries the query
that matches 1,000 entries costs roughly 10× the hit row, not because lookup
got slower but because draining 1,000 items through `Neighbors`' merge-and-dedup
logic costs about 1.5 ns per item — unavoidable, since every match returned
has to be visited to be returned. A caller that only needs the *count*, not
the matched text, can skip this by working with `bucket()` directly for a
single-code encoder — see below.

### The InlineCode design, and its real measured cost

`InlineCode<N>`'s `Eq`, `Ord` and `Hash` compare the code's raw bytes
(`as_bytes`), not its validated `&str` form (`as_str`). This looks like a
micro-optimisation; it measured as a real one. Before this method existed,
every comparison went through `as_str().cmp(...)`, which re-validates UTF-8 on
both operands of *every* comparison — including every step of the binary
search `bucket()` performs, and every comparison the merge in `Neighbors`
does. `benches/phonetic_index.rs`'s `neighbors` and `alt_designs_query` groups
measured that validation costing **3–8×** `encode()`'s own cost for the narrow
`SoundexCode`, and **up to 45×** for the wider `MetaphoneCode` (more bytes to
validate per comparison), at a 100,000-entry wide bucket — this is why the
comparison traits go through raw bytes instead: UTF-8 validity is already
guaranteed at construction (`InlineCode::new` only ever copies from a `&str`),
so re-validating it on every comparison was pure waste.

### The honest trade-off: uniform `neighbors()` vs. a raw slice

The struct-of-arrays layout `codes`/`offsets`/`ids` — a compressed-sparse-row
shape — was benchmarked at 100,000 `SoundEx` entries against three
alternatives that don't ship: a plain `String`-keyed `HashMap`, a frozen
`HashMap<SoundexCode, Box<[EntryId]>>`, and a dense array indexed by a perfect
hash of the code shape. Memory is where the shipped design wins clearly:

| Design | Bytes at 100,000 entries | Bytes per entry | Relative to shipped |
|---|---:|---:|---:|
| `InlineCode` + CSR (shipped) | 2,899,788 | 29.00 | 1.00× |
| Frozen `HashMap<Code, Box<[EntryId]>>` | 3,105,157 | 31.05 | 1.07× |
| Dense perfect-hash array | 3,227,269 | 32.27 | 1.11× |
| `String`-keyed `HashMap` | 3,979,361 | 39.79 | 1.37× |

Query latency is where the comparison is more nuanced than "the shipped design
won." `bucket()`/`neighbors()` on the shipped index cost roughly 2× the
hash-based alternatives' raw lookup for a hit or a miss at 100,000 entries
(around 110–134 ns versus 43–66 ns) — the price of a binary search over a
sorted array instead of a hash probe. At the wide bucket the gap looks larger
still (1.61 µs versus 45–367 ns), but that comparison isn't apples to apples:
the frozen-`HashMap` and dense-array alternatives return a raw `&[u32]` and the
benchmark takes its `O(1)` `.len()`, while the shipped `neighbors()` must
actually drain its iterator to produce a count, because unlike those two
throwaway designs it also has to support `DoubleMetaphone`'s two-bucket
union-and-dedup — machinery every query pays for even when, as with `SoundEx`,
only one bucket is ever in play. `PhoneticIndex::bucket()` gives that same
`O(1)`-length raw-slice access when a caller has a single code in hand and
doesn't need the union; see
[`neighbors()` versus `bucket()`](#neighbors-versus-bucket) above.

<div class="callout callout-good">
<strong>Why this is on the site instead of just in a commit message.</strong>
Publishing "the shipped design's memory footprint is the best of the four, and
its query latency is competitive but not universally fastest, for reasons X
and Y" is more useful than either silence or an unqualified "faster" claim
would be — and it is the same standard <a href="../benchmarks/distance">the
string-distance benchmarks</a> hold themselves to.
</div>

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

That one `String` per `neighbors()` call is real and currently unavoidable:
every encoder's existing `process`/`process_double` method returns an owned
`String`, and `PhoneticEncoder::encode` copies its bytes into an `InlineCode`
and drops it. Giving each encoder a second, allocation-free `process_into`
path was evaluated and deliberately deferred — it would touch four
already-verified encoders' internals for a benefit that, per the numbers
above, only matters once bucket lookup itself is the bottleneck, which it is
not at realistic dictionary sizes.

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

- [Phonetics](phonetics.md) — the four tested encoders this index is
  built from, and how to choose between them, including
  [by language](phonetics.md#choosing-a-phonetic-algorithm).
- [Language](language.md) — another Verbora-native extension, one layer up
  the pipeline: detects script and language, then recommends which of the
  four encoders above actually fits. Useful when you don't yet know which
  encoder to build this index with.
- [String distance](distance.md) — the scoring step that runs on
  `neighbors()`'s output.
- [Fuzzy name matching](../recipes/fuzzy-matching.md) — the bucket → rank →
  threshold recipe this index implements the first step of.
- [Build → Freeze → Query](#the-build-→-freeze-→-query-shape) — the same pattern
  used by `verbora-tagger`'s lexicons and `verbora-wordnet`'s `PrebuiltIndex`.
- [Allocation](../performance/allocation.md) — the allocation-free query path
  in more general terms.

## API reference

### Types

| Item | Description |
|---|---|
| `PhoneticIndexBuilder<E>` | The mutable build side. `new(encoder)`, `insert`, `extend`, `reserve`, `len`, `is_empty`, `build(self) -> PhoneticIndex<E>` |
| `PhoneticIndex<E>` | The frozen query side. `len`, `is_empty`, `get(EntryId) -> &str`, `encoder`, `bucket`, `neighbors`. `Send + Sync` whenever `E` is |
| `Neighbors<'a, E>` | The lazy iterator `neighbors()` returns. `Item = &'a str` |
| `PhoneticEncoder` | Trait implemented for all four encoders. `type Code: Copy + Eq + Hash + Ord`; `encode(&self, &str) -> PhoneticCodes<Self::Code>` |
| `PhoneticCodes<C>` | `One(C)` or `Two(C, C)` — what `encode()` produced. `IntoIterator<Item = C>` |
| `PhoneticCodesIter<C>` | The iterator `PhoneticCodes::into_iter` returns |
| `InlineCode<const N: usize>` | A stack-stored code, up to `N` bytes, `Copy`. `new(&str) -> Self` (panics past `N` bytes), `as_str(&self) -> &str` |
| `SoundexCode` | `InlineCode<16>` — `SoundEx`'s code type |
| `DaitchMokotoffCode` | `InlineCode<6>` — `SoundExDM`'s code type |
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
