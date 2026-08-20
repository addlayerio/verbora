# How Verbora uses Rust

Verbora is fast for an unglamorous reason: **it does not allocate much.** The
algorithms are the standard ones — Levenshtein is Levenshtein, Jaro–Winkler is
Jaro–Winkler. What differs is the data that flows through them, and how often it
has to be copied.

This section covers the techniques that produce that, where each one surfaces in
the API, and what it means for the code you write.

## The techniques, and where to find them

| Technique | Where it shows up in the API | Page |
|---|---|---|
| Borrowing | Every tokenizer yields `&str` slices of your input; every n-gram window borrows your slice | [Zero-copy](zero-copy.md) |
| `Cow` | All five normalizers, guaranteed borrowed when nothing changed; `Stemmer::stem` | [Zero-copy](zero-copy.md) |
| Lazy iterators | `tokens()`, `ngrams()`, `char_ngrams()`, `iter_keys_with_prefix()` | [Iterator vs `_into`](iterator-vs-into.md) |
| Caller-owned buffers | `tokenize_borrowed_into()`, `pluralize_into()`, `stem_into()` | [Buffer reuse](buffer-reuse.md) |
| Choosing the smallest working set | Levenshtein's bit-vector / row / matrix modes | [Cache locality](cache-locality.md) |
| Struct-of-arrays | The Levenshtein search matrix | [Cache locality](cache-locality.md) |
| Flat arenas | `Trie`'s `Vec<Node>` addressed by `u32` | [Cache locality](cache-locality.md) |
| Inline small collections | `SmallVec` children per trie node | [Cache locality](cache-locality.md) |
| Stack buffers for small inputs | Jaro–Winkler's match flags | [Allocation](allocation.md) |
| Cheaper hash keys | Dice hashes `(char, char)` instead of a `String` per bigram | [Allocation](allocation.md) |
| Exact fast paths | ASCII `&[u8]` vs promotion to a decoded `Vec<char>`, in distance | [Zero-copy](zero-copy.md) |
| Monomorphised iterators | `tokens()` returns `impl Iterator`, not a boxed trait object, so each boundary scan inlines | [Cache locality](cache-locality.md) |

## Read these in order

<div class="cards">

<a class="card" href="ergonomics-vs-throughput">
<span class="card-title">1. Ergonomics vs throughput →</span>
<span class="card-desc">When to reach for a performance-oriented API and — more often — when not to.</span>
</a>

<a class="card" href="iterator-vs-into">
<span class="card-title">2. Iterator vs reusable buffer →</span>
<span class="card-desc">Two shapes people assume are alternatives. They solve different problems.</span>
</a>

<a class="card" href="buffer-reuse">
<span class="card-title">3. Buffer reuse →</span>
<span class="card-desc">What <code>clear()</code> does and does not free, the append-vs-clear conventions, and how to size a buffer up front.</span>
</a>

<a class="card" href="zero-copy">
<span class="card-title">4. Zero-copy and Cow →</span>
<span class="card-desc">Borrowed tokens, <code>Cow</code>-returning normalizers, and the ASCII fast paths that keep exact Unicode indexing free.</span>
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
<span class="card-desc">The fourteen opt-in <code>par_*</code> APIs, and how to parallelise everything else yourself.</span>
</a>

<a class="card" href="cache-locality">
<span class="card-title">8. Cache locality and data layout →</span>
<span class="card-desc">Working sets, arenas, struct-of-arrays — where the big wins actually came from.</span>
</a>

</div>

## How to read the numbers here

<div class="callout callout-warn">
<strong>Timings are measured; allocation counts are not.</strong> Published
timings come from <a href="../benchmarks/">the benchmark pages</a>, and today
they cover <code>verbora-distance</code>. Criterion benchmarks for tokenizers,
phonetics, n-grams, normalizers, inflectors and the trie exist in-tree
(<code>crates/*/benches/</code>) but their tables are not published yet, so this
section says "fewer allocations" rather than quoting a speed figure for those
subsystems. Where a page describes allocation behaviour it describes
<em>what the code does</em>, read from the source — allocation counting and
peak-RSS instrumentation are not in the repository.
</div>
