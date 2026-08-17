# How Verbora uses Rust

This is not a Rust tutorial. It is an account of the specific techniques Verbora
uses, where each one appears in the API, and what it means for the code you
write against it.

The organising claim is simple: **most of Verbora's speed advantage over a
widely-used JavaScript NLP library comes from not allocating**, not from
clever arithmetic. The algorithms themselves are the well-known, standard
ones — Levenshtein is Levenshtein, Jaro-Winkler is Jaro-Winkler. What differs
is the data that flows through them.

## The techniques, and where to find them

| Technique | Where it shows up in the API | Page |
|---|---|---|
| Borrowing | Thirteen tokenizers yield `&str` slices of your input | [Zero-copy](zero-copy.md) |
| `Cow` | Four of six normalizers; all 17 `ja::converters`; `Stemmer::stem`; three tokenizers | [Zero-copy](zero-copy.md) |
| Lazy iterators | `tokens()`, `ngrams_iter()`, `iter_keys_with_prefix()` | [Iterator vs `_into`](iterator-vs-into.md) |
| Caller-owned buffers | `tokenize_into()`, `pluralize_into()`, `stem_into()` | [Buffer reuse](buffer-reuse.md) |
| Choosing the smallest working set | Levenshtein's 2-row / 3-row / matrix modes | [Cache locality](cache-locality.md) |
| Struct-of-arrays | The Levenshtein search matrix | [Cache locality](cache-locality.md) |
| Flat arenas | `Trie`'s `Vec<Node>` addressed by `u32` | [Cache locality](cache-locality.md) |
| Inline small collections | `SmallVec` children per trie node | [Cache locality](cache-locality.md) |
| Stack buffers for small inputs | Jaro–Winkler's match flags (≤128 units) | [Allocation](allocation.md) |
| Cheaper hash keys | Dice hashes `(u16, u16)` instead of allocating a `String` per bigram | [Allocation](allocation.md) |
| Exact fast paths | ASCII `&[u8]` vs `Vec<u16>` promotion in distance and phonetics | [Zero-copy](zero-copy.md) |
| Monomorphised predicates | `CharClass` as a zero-sized type, so each tokenizer's scan inlines | [Cache locality](cache-locality.md) |

## The rule the project actually follows

```text
benchmark → profile → optimise → unit tests → full suite → benchmark again
```

Two consequences you can see in the code:

**No optimisation lands without the test suite re-run.** An optimisation that
breaks behaviour is not an optimisation. The full test suite must still pass.

**No optimisation is claimed without a measurement.** And the measurements
include the ones that went the wrong way. `verbora-distance`'s Jaro–Winkler
benchmark first came in at **0.6×** — Rust *slower* than a widely-used
JavaScript NLP library — because of two `vec![false; len]` allocations per call. Moving the match
flags to a stack buffer for inputs up to 128 units fixed it: the benchmark
measures **15.3 ns**, a **1.8×** speedup, with the test suite still green.
The story is
[on the benchmark page](../benchmarks/distance.md#a-measured-regression-and-its-fix),
kept because "it's Rust, so it's fast" would have shipped the regression.

The same discipline produced a *negative* result that is still in the source. The
character-class scanner's performance notes suggested replacing the range check
with a `[bool; 128]` / `u128` bitmask lookup for ASCII. It was implemented and
measured: **12% slower** — 11.2 µs against 9.8 µs on a 9.7 kB document — because
`rustc` already lowers a `matches!` over character ranges into a range check plus
a bit test, and the mask only adds a 128-bit shift. The simpler code stayed. You
can read the comment in `crates/verbora-tokenizers/src/scan.rs`.

## Read these in order

<div class="cards">

<a class="card" href="ergonomics-vs-throughput">
<span class="card-title">1. Ergonomics vs throughput →</span>
<span class="card-desc">When to reach for a performance-oriented API and — more often — when not to. Premature optimisation has a real cost here.</span>
</a>

<a class="card" href="iterator-vs-into">
<span class="card-title">2. Iterator vs reusable buffer →</span>
<span class="card-desc">The most-confused pair on the site. They solve different problems and neither replaces the other.</span>
</a>

<a class="card" href="buffer-reuse">
<span class="card-title">3. Buffer reuse →</span>
<span class="card-desc">What <code>clear()</code> does and does not free, the append-vs-clear conventions, and how to size a buffer up front.</span>
</a>

<a class="card" href="zero-copy">
<span class="card-title">4. Zero-copy and Cow →</span>
<span class="card-desc">Borrowed tokens, <code>Cow</code>-returning normalizers, and the ASCII fast paths that keep UTF-16 exactness free.</span>
</a>

<a class="card" href="allocation">
<span class="card-title">5. Allocation behaviour →</span>
<span class="card-desc">A per-API reference: what allocates, how much, and how often.</span>
</a>

<a class="card" href="batch-vs-streaming">
<span class="card-title">6. Batch vs streaming →</span>
<span class="card-desc">Bounded memory and early output against preallocation and shared setup.</span>
</a>

<a class="card" href="parallelism">
<span class="card-title">7. Parallelism →</span>
<span class="card-desc">Verbora ships none. How to add it, and the honest answer about when it helps.</span>
</a>

<a class="card" href="cache-locality">
<span class="card-title">8. Cache locality and data layout →</span>
<span class="card-desc">Where the big wins actually came from: working sets, arenas, struct-of-arrays.</span>
</a>

</div>

## What is not measured yet

<div class="callout callout-warn">
<strong>Memory is not instrumented.</strong> Allocation counts and peak RSS are
planned but not yet collected. Where this site describes allocation behaviour it
is describing <em>what the code does</em> — read from the source and stated
structurally — not a measurement. Anything presented as a number comes from
<a href="../benchmarks/">the benchmark page</a>, which covers
<code>verbora-distance</code> only.
</div>

Subsystems with Criterion benchmarks in-tree but no published cross-language
comparison yet: tokenizers, phonetics, n-grams, normalizers, inflectors, trie.
The benchmark files exist (`crates/*/benches/`) and run; the cross-language
tables have not been generated for them. Until they are, this guide is careful to
say "fewer allocations" rather than "N times faster".
