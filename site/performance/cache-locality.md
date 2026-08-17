# Cache locality and data layout

Row-reduction and struct-of-arrays layout are not algorithmic improvements —
they are the same algorithm operating on a different shape of memory. The
single largest win in this repository, 3307.7× on `levenshtein/ascii/1024`
(documented in [String distance results](../benchmarks/distance.md)), comes
from a genuinely faster *algorithm* — a bit-vector Levenshtein — not from
layout, so it is not covered here. The layout techniques below are still real
and still worth applying, but bit-parallel and snapshot kernels cover most of
the distance modes too, so the purest remaining demonstration is search mode,
the one corner of the distance code that still fills the full matrix. This
page collects the layout decisions Verbora makes and what each one is worth.

## 1. Pick the smallest working set that answers the question

The reference's Levenshtein always materialises a full `(n+1) × (m+1)` matrix of
heap-allocated cell objects, each holding a cost and a parent coordinate — even
when the caller wants only the final number. That is `O(nm)` allocations of
`O(nm)` pointer-chased objects.

Verbora picks the cheapest structure that can answer the question asked — and,
where a genuinely cheaper algorithm exists, that instead:

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** | not a row-reduction — Myers'/Hyyrö's bit-parallel algorithm, `O(nm/64)` bitwise ops per call. See [String distance results](../benchmarks/distance.md) |
| distance, no Damerau (fallback: weighted costs) | **2 rows** | a cell needs only `up`, `left`, `diag` |
| distance, restricted Damerau, unit cost | **bit-vector** | Hyyrö's transposition extension of the same bit-parallel family |
| distance, restricted Damerau (fallback: weighted costs) | **3 rows** | transposition reaches back to row − 2 |
| distance, unrestricted Damerau | **2 rows + per-symbol snapshots** | transposition reaches an arbitrary earlier row, so the kernel snapshots the last row where each symbol occurred instead of keeping the whole matrix |
| search, any variant | full matrix | the match start is recovered by walking parents |

The two-row path is exactly what a weighted-cost 1024-character comparison
runs — turning roughly a million heap cells into two 8 KiB rows that stay in
L1 — but `levenshtein/ascii/1024` measures unit-cost distance instead. Unit-
cost distance calls — plain and restricted Damerau alike, at any length —
take the bit-vector rows above, with the kernel's character-mask tables held
in plain arrays rather than a `HashMap`, which is why that benchmark posts
**3307.7×**. Of the three `levenshtein_variants` benchmarks, only one still
isolates the row-reduction effect on its own; the other two run faster
algorithms instead of a row-reduced matrix (their benchmark names describe
the structures being compared against on the reference side, not what
Verbora's side runs):

| Benchmark | Speedup | What it measures now |
|---|--:|---|
| `levenshtein_variants/search_matrix` | 13.8× | the same full matrix on both sides — the win is layout: flat struct-of-arrays vs per-cell heap objects |
| `levenshtein_variants/damerau_unrestricted_matrix` | 39.2× | despite the name, not a full matrix: two rows plus per-symbol snapshots against the reference's full matrix |
| `levenshtein_variants/damerau_restricted_3row` | 1059.5× | despite the name, not three rows: the bit-parallel OSA kernel — an algorithm win, not a layout win |

The lesson generalises: *before* optimising a loop, ask whether it needs the
data structure it is filling — and whether a fundamentally cheaper algorithm
exists before reaching for a fundamentally cheaper structure.

## 2. Struct-of-arrays where the matrix is unavoidable

When the full matrix genuinely is required — search mode has to backtrack — it is
stored as two flat vectors rather than one vector of pairs:

```text
Array of structs                  Struct of arrays

[cost|parent][cost|parent]…       [cost][cost][cost][cost]…
                                  [parent][parent][parent]…

the cost sweep drags every        the cost sweep touches only costs;
parent through cache              parents stay cold until backtracking
```

The hot inner loop reads and writes costs only. Keeping parents in a separate
allocation means they never occupy a cache line during the sweep, and are paged
in exactly once, at the end, when the backtrace runs.

## 3. Flat arenas instead of one node per object

`verbora-trie` stores every node in a single `Vec<Node>` addressed by `u32`
indices, rather than allocating a reference-style object per node:

```text
Reference implementation          Verbora

node ──▶ {} ──▶ {} ──▶ {}         [n0][n1][n2][n3][n4][n5]…
          │      │      │          ▲    ▲
          ▼      ▼      ▼          └────┴─ u32 indices into one Vec
         {}     {}     {}

one heap object per node          one allocation for the whole trie
one hash map per node             children inline in a SmallVec
size = a full traversal           size = O(1), the arena's own length
```

Three consequences you can observe through the API:

- **Building a trie is one allocation plus amortised growth**, not one
  allocation per node. `Trie` is `{ nodes: Vec<Node>, folds: bool }` — a single
  heap buffer for the whole tree.
- **`get_size()` is O(1)**, because it is `self.nodes.len()` — the arena's own
  length — rather than a walk that counts as it goes.
- **Traversal is iterative**, so a 100 kB input cannot overflow the stack the way
  per-code-unit recursion does.

`u32` indices rather than pointers also halve the size of a link on 64-bit
targets, so more of the tree fits in each cache line.

## 4. Inline storage for small collections

Most trie nodes have one or two children. A `HashMap` per node would allocate for
every one of them; a `SmallVec` holds the common cases inline, in the node
itself, and only spills to the heap for nodes that genuinely branch. The
`Cargo.toml` records the reason:

> Child lists are one or two entries for the overwhelming majority of nodes;
> inline storage keeps a bulk load from making one heap allocation per node.

The same idea appears in `verbora-distance`: Jaro–Winkler's scalar kernel keeps
its two match-flag arrays in `[bool; 128]` stack buffers rather than allocating
them per call. Words are short, so the stack path is the common path — not a
micro-optimisation for a rare case. (Since the bit-parallel match-flagging
kernels landed, that scalar kernel serves inputs up to 16 code units and doubles
as the oracle the bit-parallel path is differentially tested against; longer
inputs keep their match flags packed into `u64` words instead — the same
instinct, taken further.)

## 5. Cheaper keys, not cheaper hashing alone

The Dice coefficient counts shared bigrams. The reference allocates a `String`
per bigram and hashes it. Verbora hashes a `(u16, u16)` tuple with `FxHashMap`
instead — no allocation, a smaller key, and a faster hash:

| Input length | Speedup |
|--:|--:|
| 4 | 3.3× |
| 16 | 4.0× |
| 64 | 4.5× |
| 256 | 5.7× |
| 1024 | **7.4×** |

The ratio grows with input size because the allocation pressure it removes grows
with input size. A win that *scales* like this is usually an allocation win, not
a constant-factor one.

## 6. Monomorphised predicates

Twelve of the thirteen character-class tokenizers share one scanner (`WordRuns`),
generic over a zero-sized `CharClass` type rather than taking a function
pointer, so each tokenizer's predicate inlines into its own copy of the loop and
the ASCII branch stays predictable. The thirteenth, `AggressiveTokenizerFa`,
has its own hand-written iterator instead, because Persian's rules need a
bracket-aware pre-pass the shared scanner has no hook for.

This is also where the repository keeps its best *negative* result. The scanner's
notes suggested replacing the range check with a `[bool; 128]` or `u128` bitmask
for the ASCII half. It was implemented and measured:

> **12% slower** — 11.2 µs against 9.8 µs on a 9.7 kB document — because `rustc`
> already compiles a `matches!` over character ranges into a range check plus a
> bit test, and the explicit mask only adds a 128-bit shift.

The simpler code stayed, and the measurement is in the source so nobody tries it
again.

## Applying this to your own code

**Process contiguous data.** A `Vec<String>` of documents scattered across the
heap is worse than one big `String` with offsets, if you control the ingest.

**Do not build what you will not read.** The n-gram engine has both
`ngrams_iter` and `ngrams` for this reason: taking the first five windows of a
long document should not materialise all of them.

**Pre-size.** `Vec::with_capacity`, `Trie::reserve`. Growth reallocation is
copying.

**Reuse.** One buffer across a loop keeps the same pages hot instead of returning
them to the allocator and getting different ones back. See
[Buffer reuse](buffer-reuse.md).

**Measure, including the negative results.** Two of the anecdotes on this page
are optimisations that made things worse. They were only found by benchmarking.

## Related

- [Allocation behaviour](allocation.md)
- [Benchmarks: string distance](../benchmarks/distance.md)
- [Trie](../features/trie.md) — the arena, in full.
