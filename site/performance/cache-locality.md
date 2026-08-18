# Cache locality and data layout

Row reduction, flat arenas and struct-of-arrays are not algorithmic
improvements — they are the same algorithm operating on a different shape of
memory. Modern CPUs are fast at arithmetic and slow at waiting for memory, so
the shape usually matters more than the instruction count.

This page collects the layout decisions that are visible through the API, what
each one is worth, and how to apply the same reasoning to your own code.

## 1. Ask for the narrowest result you need

The Levenshtein family answers each question with the smallest structure that
can produce the answer asked for:

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** | bit-parallel: `O(nm/64)` bitwise operations, one `u64` word per 64 units, no DP row at all |
| distance, no Damerau, weighted costs | **1 row** | a cell needs only `up`, `left`, `diag` |
| distance, restricted Damerau, unit cost | **bit-vector** | the transposition extension of the same bit-parallel family |
| distance, restricted Damerau, weighted costs | **3 rows** | transposition reaches back to row − 2 |
| distance, unrestricted Damerau | **2 rows + per-symbol snapshots** | transposition reaches an arbitrary earlier row, so the kernel snapshots the last row where each symbol occurred |
| search, any variant | **full matrix** | the match start is recovered by walking parents |

The cost difference between those rows is the point. On identical 64-unit
inputs:

| Call | Working set | Time |
|---|---|--:|
| `levenshtein` | bit-vector | 166.1 ns |
| `damerau_levenshtein` (restricted) | bit-vector | 179.4 ns |
| `damerau_levenshtein` (unrestricted) | 2 rows + snapshots | 7.75 µs |
| `levenshtein_search` | full cost + parent matrix | 12.79 µs |

Asking only for a distance is roughly **77× cheaper** than asking for a match
position on the same input, because the answer you asked for fits in a
fundamentally smaller structure.

When the full matrix genuinely is required — search mode has to backtrack — it
is stored as two flat vectors rather than one vector of pairs, so the hot cost
sweep never drags parent entries through cache; the parents are paged in once,
at the end, when the backtrace runs.

## 2. Flat arenas instead of one node per object

`verbora-trie` stores every node in a single `Vec<Node>` addressed by `u32`
indices, rather than allocating a reference-style object per node. Three
consequences you can observe through the API:

- **Building a trie is one allocation plus amortised growth**, not one
  allocation per node. `Trie` is `{ nodes: Vec<Node>, folds: bool }` — a single
  heap buffer for the whole tree.
- **`get_size()` is `O(1)`**, because it is the arena's own length rather than a
  walk that counts as it goes. It is safe to call in a metrics loop.
- **Traversal is iterative**, so a 100 kB input cannot overflow the stack the way
  per-code-unit recursion does.

`u32` indices rather than pointers also halve the size of a link on 64-bit
targets, so more of the tree fits in each cache line.

## 3. Inline storage for small collections

Most trie nodes have one or two children, so child lists are held inline in a
`SmallVec` and only spill to the heap for nodes that genuinely branch. A bulk
load therefore does not make one heap allocation per node.

The same instinct appears in `verbora-distance`. Jaro–Winkler's scalar kernel
keeps its two match-flag arrays in `[bool; 128]` stack buffers rather than
allocating them per call, so **`jaro` and `jaro_winkler` allocate nothing at all
for inputs up to 128 code units** — and words are short, so that is the common
path. Longer inputs go further: the bit-parallel kernels pack match flags into
`u64` words.

## 4. Cheaper keys, not cheaper hashing alone

The Dice coefficient counts shared bigrams. Verbora hashes a `(u16, u16)` tuple
with `FxHashMap` rather than building a `String` per bigram: no allocation, a
smaller key, and a faster hash function.

| Benchmark | Input length | Time |
|---|--:|--:|
| `dice/4` | 4 | 106.9 ns |
| `dice/16` | 16 | 308.1 ns |
| `dice/64` | 64 | 1.00 µs |
| `dice/256` | 256 | 3.17 µs |
| `dice/1024` | 1024 | 10.61 µs |

Cost stays close to linear in input length, because the per-bigram work is a
tuple hash and a map probe with no allocator involved.

## 5. Monomorphised predicates

Twelve of the thirteen character-class tokenizers share one scanner (`WordRuns`),
generic over a zero-sized `CharClass` type rather than taking a function
pointer, so each tokenizer's predicate inlines into its own copy of the loop and
the ASCII branch stays predictable. The thirteenth, `AggressiveTokenizerFa`, has
its own hand-written iterator, because Persian's rules need a bracket-aware
pre-pass the shared scanner has no hook for.

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

## Related

- [Allocation behaviour](allocation.md)
- [Benchmarks: string distance](../benchmarks/distance.md)
- [Trie](../features/trie.md) — the arena, in full.
