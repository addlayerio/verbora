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
| `levenshtein`, unit cost | **bit-vector** | bit-parallel: `O(nm/64)` bitwise operations, one `u64` word per 64 units, no DP row at all |
| `levenshtein`, weighted costs | **1 row** | a cell needs only `up`, `left`, `diag` |
| `osa`, unit cost | **bit-vector** | the transposition extension of the same bit-parallel family |
| `osa`, weighted costs | **3 rows** | transposition reaches back to row − 2 |
| `damerau_levenshtein`, unit cost | **3 rolling rows + 1 saved-cell row** | Zhao–Sahni's linear-space algorithm: two remembered cells stand in for the arbitrarily-earlier row the textbook recurrence would read |
| `damerau_levenshtein`, weighted costs | **full matrix** | a weighted transposition reaches an arbitrary earlier row |
| `levenshtein_search`, unit cost | **two `Vec<u64>` of per-column deltas** | the parent of every cell is a pure function of its neighbours' costs, and unit-cost cell costs are recoverable from Myers' `Pv`/`Mv` words — so the backtrack recomputes them instead of storing a matrix |
| every other search | **full cost + parent matrix** | a transposition's parent depends on state cell costs cannot recover, and a weighted cell has no delta-bit form at all |

The cost difference between those rows is the point. On identical 64-unit
inputs:

| Call | Working set | Time † |
|---|---|--:|
| `levenshtein` | bit-vector | 166.1 ns |
| `osa` | bit-vector | 179.4 ns |
| `damerau_levenshtein` | 3 rolling rows | 7.75 µs |
| `levenshtein_search` | per-column bit-vector deltas | 12.79 µs |

† Every figure in this table is **pending re-measurement** and left as recorded
rather than replaced with a guess. The working-set ranking they illustrate is a
property of the algorithms, not of the run: a bit-vector kernel, three rolling
rows and a full cost-plus-parent matrix are three different amounts of memory to
touch per cell, in that order.

The ratio between the first and last rows — roughly **77×** as recorded, and
pending with them — is the practical point: asking only for a distance is far
cheaper than asking for a match position on the same input, because the answer
you asked for fits in a fundamentally smaller structure.

When the full matrix genuinely is required — the Damerau and OSA searches, and
every weighted search — it is stored as two flat vectors rather than one vector
of pairs, so the hot cost sweep never drags parent entries through cache; the
parents are paged in once, at the end, when the backtrace runs.

## 2. Flat arenas instead of one node per object

`verbora-trie` stores every node in a single `Vec<Node>` addressed by `u32`
indices, rather than allocating a reference-style object per node. Three
consequences you can observe through the API:

- **Building a trie is one arena allocation plus amortised growth**, not one
  allocation per node — one heap buffer holds the whole tree, alongside the hash
  membership set that answers `contains`.
- **`node_count()` is `O(1)`**, because it is the arena's own length rather than
  a walk that counts as it goes, and **`len()` is `O(1)` too**, because each node
  maintains the number of stored words in its subtree and the root's is the
  answer. Both are safe to call in a metrics loop.
- **A prefix count is `O(1)` after the descent.** The same maintained subtree
  counts make `iter_keys_with_prefix(p).count()` a descent plus one read, rather
  than a traversal of everything under `p`.
- **Traversal is iterative**, so a 100 kB input cannot overflow the stack the way
  per-scalar recursion does.

`u32` indices rather than pointers also halve the size of a link on 64-bit
targets, so more of the tree fits in each cache line. `Node` is 32 bytes, and the
subtree counter occupies bytes that were already padding — a test in the crate
pins that.

`FrozenTrie` takes the same idea one step further for an index that stops
changing: runs of single-child, non-word nodes collapse into one edge label, and
the words themselves are precomputed into one contiguous, enumeration-ordered
table, so a prefix query is a descent and a range rather than a walk.

## 3. Inline storage for small collections

Most trie nodes have one or two children, so child lists are held inline in a
`SmallVec` and only spill to the heap for nodes that genuinely branch. A bulk
load therefore does not make one heap allocation per node.

The same instinct appears in `verbora-distance`. Jaro–Winkler's scalar kernel —
the production path for operands of at most 16 units — keeps its two match-flag
arrays in `[bool; 128]` stack buffers rather than allocating them per call. The
bit-parallel kernels that take over above that pack the same flags one bit per
position, in stack arrays holding 16 `u64` words, so **the match flags stay off
the heap up to 1024 units per side**. What does reach the heap past one word is
the packed pattern-match table, as a single `Vec<u64>`.

## 4. Cheaper keys, not cheaper hashing alone

The Dice coefficient counts shared bigrams. Verbora hashes a `(char, char)` tuple
with `FxHashSet` rather than building a `String` per bigram: no allocation, a
smaller key, and a faster hash function.

| Benchmark | Input length | Time † |
|---|--:|--:|
| `dice/4` | 4 | 106.9 ns |
| `dice/16` | 16 | 308.1 ns |
| `dice/64` | 64 | 1.00 µs |
| `dice/256` | 256 | 3.17 µs |
| `dice/1024` | 1024 | 10.61 µs |

† Pending re-measurement, on the same terms as the table above. What holds
without a run is the shape: cost stays close to linear in input length, because
the per-bigram work is a tuple hash and a map probe with no allocator involved.

## 5. Monomorphised iterators

`BorrowingTokenizer::tokens` returns `impl Iterator` rather than a boxed trait
object, so each tokenizer's boundary scan inlines into the caller's loop with no
virtual call and no allocation. The same shape carries into `verbora-ngrams`,
whose `ngrams` returns `std::slice::Windows` directly — a struct the optimiser
already knows how to unroll.

## Applying this to your own code

**Process contiguous data.** A `Vec<String>` of documents scattered across the
heap is worse than one big `String` with offsets, if you control the ingest.

**Do not build what you will not read.** `ngrams` is lazy for this reason:
taking the first five windows of a long document should not materialise all of
them, and `.take(5)` is the whole difference.

**Pre-size.** `Vec::with_capacity`, `Trie::reserve`. Growth reallocation is
copying.

**Reuse.** One buffer across a loop keeps the same pages hot instead of returning
them to the allocator and getting different ones back. See
[Buffer reuse](buffer-reuse.md).

## Related

- [Allocation behaviour](allocation.md)
- [Benchmarks: string distance](../benchmarks/distance.md)
- [Trie](../features/trie.md) — the arena, in full.
