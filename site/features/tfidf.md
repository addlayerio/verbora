# TF-IDF

`verbora-tfidf` is tested against the reference TF-IDF implementation (246 lines) — a
corpus of documents, an inverse-document-frequency cache, and the handful of
queries (`tf`, `idf`, `tfidf`, `tfidfs`, `listTerms`) built on top of them. The
reference stores each document as a reference object mapping term to count,
with the document's own key smuggled into the same object under `__key`. That
single design choice is why this crate is not a `HashMap<String, f64>`
wrapper: a term spelled `__proto__` is silently dropped, a term spelled
`toString` shadows a real method until it is explicitly zeroed, key order is
the reference's `for…in` order rather than insertion order, and the cache that
backs `idf` is sometimes `{}` and sometimes `Object.create(null)` in ways that
change what `idf("toString")` returns. All of it is reproduced, not smoothed
over, and explained below.

<div class="callout callout-spec">
<strong>Specification status.</strong> The full stateful surface — corpus
construction, serialization and deserialization, the idf cache, stop-word and
tokenizer configuration, term ordering and the empty-corpus case — is
documented and test-pinned, with no external data required.
<code>cargo test -p verbora-tfidf</code> runs <strong>52</strong> unit tests
and <strong>2</strong> doctests.
</div>

## When to use it

- **Porting the reference that called the reference's `TfIdf`.** Every entry point —
  `addDocument`, `addFileSync`, `removeDocument`, `tf`, `idf`, `tfidf`,
  `tfidfs`, `listTerms`, `setTokenizer`, `setStopwords` — maps onto a Rust
  method or free function with the same argument shape, and results are
  byte-identical to the reference, including the ones that look like bugs.
- **Ranking documents against a query, or ranking a document's own terms**,
  for a corpus that fits comfortably in memory and is built once and queried
  many times.
- **A corpus that grows incrementally.** `add_document` and `remove_document`
  keep an incremental document-frequency table, so `idf` on a built corpus
  stays cheap (see [Performance characteristics](#performance-characteristics))
  as documents are added and removed one at a time.
- **Ingesting a large batch of text documents in one call.**
  [`par_add_documents_batch`](#adding-many-documents-at-once-par-add-documents-batch),
  behind the `parallel` Cargo feature, parallelizes the tokenizing of a
  `DocumentInput::Text` batch ahead of a still-sequential ingest — a real but
  modest win (5–13% measured), worth it for a genuinely large batch, not for
  a handful of documents.

## When not to use it

- **You want a `terms × documents` matrix, sparse vectors, or cosine
  similarity between documents.** Nothing here builds one; a document is a
  `Vec<(TermId, f64)>`, and the only cross-document operation is `idf`'s
  document-frequency count. If you need vector search, this is the wrong
  layer to build it on top of without writing that layer yourself.
- **You need the reference-object semantics to just go away.** They do not:
  `__proto__` terms are still dropped, `toString`-named terms still divergently
  short-circuit `idf` on a fresh or `addFileSync`-loaded instance, and
  `list_terms`'s tie-break order is still the reference engine's, not a lexicographic one. If
  none of that is acceptable for your use case, this crate's exactness is
  working against you, not for you.
- **You are counting term frequency for something other than ranking**, e.g.
  a straightforward bag-of-words histogram with no idf weighting. A
  `HashMap<&str, u32>` over your own tokenizer output is simpler and carries
  none of the reference-object baggage.

## Quick example

```rust
use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};

fn main() {
    let mut tfidf = TfIdf::new();
    for text in [
        "this document is about node.",
        "this document is about ruby.",
        "this document is about ruby and node.",
        "this document is about node. it has node examples",
    ] {
        tfidf
            .add_document(DocumentInput::Text(text), DocKey::Undefined, false)
            .unwrap();
    }

    // "node" is in every document, so its idf is 1 + ln(4/4) = 1.
    assert_eq!(tfidf.idf("node").unwrap(), 1.0 + (4.0f64 / 4.0).ln());
    assert_eq!(tfidf.tfidfs(Terms::Text("node")).unwrap(), [1.0, 0.0, 1.0, 2.0]);

    let ranked = tfidf.list_terms(3).unwrap();
    assert_eq!(ranked[0].term, "node");
}
```

## Choosing the right API

This crate has more axes of choice than most on this site: which constructor
builds the corpus, which shape `add_document` was handed, whether to spend
now (`restore_cache`) or later on the idf cache, whether to add documents one
at a time or as one parallel batch, which of three query shapes answers your
question, and — the one every corpus shares whether it likes it or not — a
pair of **process-global** settings. None of these is a stylistic preference;
each takes a materially different code path with a materially different cost
or a materially different result.

### Comparison table

| Decision | Options | What actually differs |
|---|---|---|
| Building the corpus | [`TfIdf::new`](#tfidf-new-vs-tfidf-from-json) / [`TfIdf::from_json`](#tfidf-new-vs-tfidf-from-json) | which internal representation every document gets — count table or scanned value |
| Feeding one document | [`DocumentInput::Text`](#the-three-shapes-of-add-document) / `::Tokens` / `::Raw` | lowercasing, stop-word filtering, and whether the slot can ever match a query |
| After mutating the corpus | [`restore_cache: bool`](#the-idf-cache-and-restore-cache) | discard every cached idf vs. recompute every cached idf in place, now |
| Adding many text documents | `add_document` in a loop / [`par_add_documents_batch`](#adding-many-documents-at-once-par-add-documents-batch) | a still-sequential ingest vs. parallel tokenizing ahead of a still-sequential replay — `Text` only, modest measured win |
| Reading a score | [`tfidf`](#tfidf-vs-tfidfs-vs-list-terms) / `tfidfs` / `list_terms` | one term × one document, one term × every document, or every term × one document, ranked |
| Tokenizing a string document | the [process-global tokenizer](#the-process-global-tokenizer-and-stop-word-list) | affects every `TfIdf` in the process, including ones built earlier |

### Decision tree

```text
I have a corpus question
│
├── "Build a corpus from text/tokens I have in hand"
│      └── TfIdf::new()                    → count-table fast path
│
├── "Restore a corpus a previous process saved"
│      └── TfIdf::from_json(&s)            → scanning path, O(1) lookup still
│
├── "Add one document"
│      ├── Plain text, want lowercasing + stop-word filtering
│      │      └── DocumentInput::Text(s)
│      ├── Already tokenized, want it stored EXACTLY as given
│      │      └── DocumentInput::Tokens(&tokens)
│      └── Some other JSON shape, occupies a slot but matches nothing
│             └── DocumentInput::Raw(value)
│
├── "Add MANY text documents at once, and it's a large batch"
│      └── par_add_documents_batch(&pairs)  → Text only, parallel feature,
│                                              modest win — see below
│
├── "Score a term against ONE document"
│      └── tfidf(terms, d)
│
├── "Score a term against EVERY document"
│      └── tfidfs(terms)                   → tfidf(terms, d) for every d, in order
│
└── "Rank every term OF one document"
       └── list_terms(d)                   → for…in order, then the reference engine-stable-sorted
```

### `TfIdf::new` vs `TfIdf::from_json`

The crate's own module documentation states the representation choice
directly: nothing here ever materialises a `terms × documents` matrix. A
document built through `add_document` is a `Vec<(TermId, f64)>` plus a hash
index — `BuiltDocument` — and terms are interned to a `u32` `TermId` **once
per corpus**, so a term repeated across fifty documents costs one allocation,
not fifty. `TfIdf::new()` starts every corpus on this path.

`TfIdf::from_json` cannot use it. A JSON-deserialized document can hold values
a count table has no room for — a string, a literal `0`, a negative number —
so a corpus restored this way keeps each document as a `RawDocument`:
the original `JsonValue`, indexed by an `FxHashMap<Box<str>, u32>` so a
property lookup stays O(1) even though the value itself is not a plain count.

The two disagree about **one** thing: how expensive an `idf` cache miss is.

<div class="perf">
<div class="perf-row"><span class="perf-k">Corpus built via <code>TfIdf::new</code> + <code>add_document</code></span></div>
<div class="perf-row"><span class="perf-k">Cache-miss cost</span><span class="perf-v">O(1) — an incremental document-frequency table, not a rescan</span></div>
<div class="perf-row"><span class="perf-k">Measured</span><span class="perf-v">flat at ~18 ns from 1 document to 256 (<code>idf_cold/built</code>, <code>benches/tfidf.rs</code>)</span></div>
</div>

<div class="perf">
<div class="perf-row"><span class="perf-k">Corpus restored via <code>TfIdf::from_json</code></span></div>
<div class="perf-row"><span class="perf-k">Cache-miss cost</span><span class="perf-v">O(documents) — every document is scanned, same as the reference</span></div>
<div class="perf-row"><span class="perf-k">Measured</span><span class="perf-v">24 ns at 1 document, 2.6 µs at 256 (<code>idf_cold/deserialized</code>) — linear, like the reference</span></div>
</div>

That 2.6 µs is itself the *fixed* version: `RawDocument`'s doc comment records
that before it grew its own lookup index, a single `idf` on a 256-document
deserialized corpus cost **3.4 ms** — an O(documents × terms) scan of `Vec<(String,
JsonValue)>` pairs, found by exactly this kind of benchmarking rather than
assumed. Both numbers are from `crates/verbora-tfidf/benches/tfidf.rs`; see
[Performance characteristics](#performance-characteristics).

`from_json` never gets the O(1) path back, even for documents that happen to
look like plain counts — the representation is decided once, at
deserialization, not per document:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut built = TfIdf::new();
    built
        .add_document(DocumentInput::Text("shared term"), DocKey::string("a"), false)
        .unwrap();
    built
        .add_document(DocumentInput::Text("shared term"), DocKey::string("b"), false)
        .unwrap();

    let json = built.to_json();
    let mut restored = TfIdf::from_json(&json).unwrap();

    // Same corpus, same answer -- from_json changes HOW the answer is found,
    // not what it is.
    assert_eq!(built.idf("shared").unwrap(), restored.idf("shared").unwrap());
}
```

### The three shapes of `add_document`

`DocumentInput` is not a stylistic wrapper around one `buildDocument` code
path; the reference's `buildDocument` genuinely branches on the reference
type of what it was handed, and each branch is observable:

| Variant | Lowercased | Stop-word filtered | Tokenizer used | Can it match a query? |
|---|:--:|:--:|---|:--:|
| `DocumentInput::Text(&str)` | ✅ | ✅ | the process-global tokenizer | ✅ |
| `DocumentInput::Tokens(&[&str])` | ❌ | ❌ | none — used verbatim | ✅ (exact strings only) |
| `DocumentInput::Raw(JsonValue)` | — | — | none | ❌ — never matches, but still counts toward every `idf`'s denominator |

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut from_text = TfIdf::new();
    from_text
        .add_document(DocumentInput::Text("The The THE node"), DocKey::Undefined, false)
        .unwrap();

    let mut from_tokens = TfIdf::new();
    from_tokens
        .add_document(
            DocumentInput::Tokens(&["The", "The", "THE", "node"]),
            DocKey::Undefined,
            false,
        )
        .unwrap();

    // Text: lowercased, and "the" is a default stop word, so it never lands
    // in the document at all.
    assert_eq!(from_text.tf_at("the", 0).unwrap().to_number(), 0.0);
    assert_eq!(from_text.tf_at("node", 0).unwrap().to_number(), 1.0);

    // Tokens: used exactly as given. Three different terms, none filtered.
    assert_eq!(from_tokens.tf_at("The", 0).unwrap().to_number(), 2.0);
    assert_eq!(from_tokens.tf_at("THE", 0).unwrap().to_number(), 1.0);
    assert_eq!(from_tokens.tf_at("the", 0).unwrap().to_number(), 0.0);
}
```

`DocumentInput::Raw` is for reproducing a corpus that (in the reference) held
something other than a string or an array in a document slot — an object, a
number, even `null`. The reference's `buildDocument` returns such a value
**unchanged**, so it occupies a slot, is never a term match for anything, and
still counts toward `documents.length` — which is the denominator of every
`idf`:

```rust
use verbora_tfidf::{DocKey, DocumentInput, JsonValue, TfIdf};

fn main() {
    let mut t = TfIdf::new();
    t.add_document(DocumentInput::Text("node"), DocKey::Undefined, false).unwrap();
    t.add_document(DocumentInput::Text("node"), DocKey::Undefined, false).unwrap();
    t.add_document(
        DocumentInput::Raw(JsonValue::Obj(vec![(
            "text".into(),
            JsonValue::Str("node".into()),
        )])),
        DocKey::string("K"),
        false,
    )
    .unwrap();

    // Three documents in the denominator, but the term "node" as a PROPERTY
    // NAME only appears on the two built ones -- the raw object's own "text"
    // property is not the same thing as a term called "node".
    assert_eq!(t.idf("node").unwrap(), 1.0 + (3.0f64 / 3.0).ln());

    // A raw document gets no __key at all, so a key that was assigned to it
    // at add_document time never matches it on removal...
    assert!(!t.remove_document(&DocKey::string("K")).unwrap());
    // ...only the reference's own DocKey::Undefined does.
    assert!(t.remove_document(&DocKey::Undefined).unwrap());
}
```

### The idf cache and `restore_cache`

Every `idf` result is cached by term. `add_document` and `remove_document`
take a `restore_cache: bool` that decides what happens to that cache after
the corpus changes:

- **`restore_cache: false`** (the common case) — the cache is thrown away
  outright and replaced with an empty one. Every term becomes cold; the next
  read of each recomputes it lazily.
- **`restore_cache: true`** — nothing is thrown away. Instead, every term
  **currently in the cache** is recomputed immediately, in place —
  `for (const term in this._idfCache) this.idf(term, true)`, reproduced
  literally as `TfIdf::idf_value` called with `force: true` over
  `TfIdf::idf_cache`'s key order.

In the reference, `restoreCache` exists because the alternative — leaving stale
values in the cache — would be silently wrong, and warming everything back up
immediately is preferable to a corpus-wide rescan happening unpredictably
later. In this port that motivation only partly carries over: an `idf` cache
miss is already O(1) on the built-document fast path (see the previous
section), so paying to eagerly refresh N cached terms costs roughly what N
lazy misses would have cost anyway. `restore_cache` is still the right choice
when you want every cached value to be correct **immediately**, or when your
corpus contains `RawDocument`s (from `from_json`), where a miss is still
O(documents) and refreshing eagerly genuinely front-loads real work.

There is also a second, easy-to-miss effect: `restore_cache: false` does not
just empty the cache, it **replaces its identity** — from whatever backed it
before to `Object.create(null)`. That distinction is the subject of the next
section.

### Adding many documents at once: `par_add_documents_batch`

Behind this crate's `parallel` Cargo feature,
`TfIdf::par_add_documents_batch(&mut self, documents: &[(&str, DocKey)]) ->
Result<(), TfIdfError>` adds many **text** documents in one call, tokenizing
them in parallel ahead of a still-sequential ingestion pass. Read this
section before reaching for it: of the thirteen `par_*` APIs across the
workspace (see [Parallelism](../performance/parallelism)), this is the one
that is **not** a plain fan-out over its sequential sibling, and the one with
the most modest measured win — both for the same underlying reason.

#### Why this cannot be `docs.par_iter().for_each(add_document)`

[`add_document`](#the-three-shapes-of-add-document) takes `&mut self` and
mutates three pieces of *shared* corpus state on every call: the
[`Interner`], the incremental document-frequency table, and the idf cache's
own identity (see [above](#the-idf-cache-and-restore-cache)). A naive
`docs.par_iter().for_each(|d| corpus.add_document(d, ..))` does not compile
against that; wrapping the corpus in a `Mutex` would compile, but it would
serialize exactly the work a parallel version exists to speed up, for a net
loss once lock contention is added on top. A map-reduce redesign — build `N`
partial corpora, merge — was considered and rejected too: merging interners
would need a new algorithm with no analogue in the sequential code, which is
exactly the "second implementation" this workspace's Rayon policy exists to
prevent (see the callout on [Parallelism](../performance/parallelism)).

What actually is parallelizable is narrower than "the whole call": lowercasing
and tokenizing a **text** document is a pure function of that document's own
text and the (thread-safely-read) global tokenizer, touching no corpus state
at all. `par_add_documents_batch` fans out exactly that part, in two phases:

1. **Parallel.** Every `(text, key)` pair's text is lowercased and tokenized
   independently — the *exact same* [`lowercase_units`] and process-global
   tokenizer ([`globals::tokenize_global`]) calls `add_document`'s own `Text`
   branch already makes, just computed ahead of time instead of inline.
2. **Sequential.** The tokenized documents are replayed, in original order,
   through the exact per-token loop `add_document`'s `Text` branch already
   runs — [`globals::is_stopword`], [`Interner::intern`],
   [`BuiltDocument::observe`], then the push. This is the part that touches
   the shared interner and document-frequency table, and it is left
   untouched on purpose: same calls, same order, same result as calling
   `add_document` once per document.

Because step 2 replays step 1's output through unmodified sequential
primitives in the original order, this method is provably equivalent to
calling `add_document(DocumentInput::Text(text), key, false)` once per pair —
verified by `tests/parallel.rs`'s sequential-vs-parallel test suite over
every pathological input `src/tfidf.rs`'s own test suite already knows about.

#### Scope: `Text` only, and always `restore_cache: false`

Narrower than every other `par_*` API on this site:

- **Only [`DocumentInput::Text`](#the-three-shapes-of-add-document).**
  `DocumentInput::Tokens` has no tokenizing to parallelize (it is used
  verbatim) and `DocumentInput::Raw` does no processing at all. Add those
  with the ordinary sequential `add_document`, before or after a batch call.
- **No `restore_cache` parameter.** This method always behaves like
  `restore_cache: false` — discarding the idf cache and reinstalling
  `Object.create(null)` once, after every document in the batch has been
  pushed, matching what `N` sequential `add_document(.., false)` calls leave
  behind. `restore_cache: true`'s eager-refresh semantics have no equivalent
  here; call `add_document` in a loop if you need them.

#### The measured win is real but modest — Amdahl's law, not a bug

`benches/tfidf.rs`'s `parallel_batch` group (`--features parallel`) measures
two corpus shapes on one 32-core machine:

| Shape | N | Sequential | `par_add_documents_batch` | Δ |
|---|---:|---:|---:|---:|
| `few_large` (≈167 kB docs) | 8 | 18.4 ms | 17.5 ms | ~5% faster |
| `few_large` | 64 | 141.1 ms | 130.9 ms | ~7% faster |
| `few_large` | 256 | 560.5 ms | 514.4 ms | ~8% faster |
| `many_small` (≈1.2 kB docs) | 128 | 3.46 ms | 3.69 ms | ~7% **slower** |
| `many_small` | 1,024 | 25.6 ms | 23.5 ms | ~8% faster |
| `many_small` | 8,192 | 211.6 ms | 183.2 ms | ~13% faster |

If the sequential replay phase were negligible next to tokenizing, 32 cores
tokenizing in parallel would show a far larger win than the 5–13% measured
here. It is not negligible: the interner and `BuiltDocument` index lookups
this method deliberately leaves sequential are a real, non-trivial share of
total ingestion time at these document sizes — this is Amdahl's law made
concrete, not an implementation shortfall. Below roughly a thousand small
documents (or a few dozen large ones), the sequential loop can win outright —
`many_small/128` is measurably *slower* in parallel, the fork-join cost not
yet amortized. Reach for this method when ingesting a genuinely large batch
in one call; for a handful of documents, or documents arriving one at a time,
`add_document` in a loop is simpler and at least as fast.
[Parallelism](../performance/parallelism) shows this same table beside
`verbora-spellcheck`'s much larger win, deliberately, as the two ends of what
a measured, honestly-reported `par_*` API looks like on this site.

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Parallel tokenizing phase, then a sequential interning/counting replay — not a plain fan-out over <code>add_document</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Result&lt;(), TfIdfError&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec&lt;Vec&lt;String&gt;&gt;</code> sized to <code>documents.len()</code> for the tokenized intermediate — one <code>String</code> per token, even on the default tokenizer's normally allocation-free path, because tokens must outlive the parallel closure that produced them</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes, partially — behind the <code>parallel</code> Cargo feature; only the tokenizing phase runs on more than one thread</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Ingesting a genuinely large batch of text documents in one call, not a handful</span></div>
</div>

```rust  ignore
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let docs = [
        ("this document is about node.", DocKey::Num(0.0)),
        ("this document is about ruby.", DocKey::Num(1.0)),
    ];

    let mut parallel = TfIdf::new();
    parallel.par_add_documents_batch(&docs).unwrap();

    let mut sequential = TfIdf::new();
    for (text, key) in docs {
        sequential
            .add_document(DocumentInput::Text(text), key, false)
            .unwrap();
    }

    // Same corpus either way -- this method changes HOW the documents are
    // ingested, not what the resulting corpus is.
    assert_eq!(parallel.to_json(), sequential.to_json());
}
```

### `tfidf` vs `tfidfs` vs `list_terms`

| API | Answers | Shape | Terms come from |
|---|---|---|---|
| `tfidf(terms, d)` | one score | `f64` | your query, against document `d` |
| `tfidfs(terms)` | one score per document | `Vec<f64>`, index = document order | your query, against every document |
| `tfidfs_with(terms, cb)` | same, plus a callback | `cb(index, score, key)` per document, synchronously | your query, against every document |
| `list_terms(d)` | every term of one document, ranked | `Vec<TermScore>`, descending by score | document `d`'s own `for…in` keys |

`tfidf` and `tfidfs` take a query you supply — `Terms::Text` or
`Terms::Tokens`, the same string-vs-array distinction as `add_document`,
**except that `Terms::Text` is never stop-word filtered**: querying `"the"`
computes and caches an idf for a term a string-built document could never
contain in the first place. `list_terms` takes no query at all; its terms are
whatever `for…in` visits on the document itself, in that order, before being
sorted by score — see
[`for…in` order in `list_terms`](#for-in-order-in-list-terms) for what that
order actually is.

```rust
use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};

fn main() {
    let mut tfidf = TfIdf::new();
    for text in [
        "this document is about node.",
        "this document is about ruby.",
        "this document is about ruby and node.",
        "this document is about node. it has node examples",
    ] {
        tfidf
            .add_document(DocumentInput::Text(text), DocKey::Undefined, false)
            .unwrap();
    }

    // One term, one document.
    assert_eq!(tfidf.tfidf(Terms::Text("document"), 3).unwrap(), 0.776_856_448_685_790_3);

    // The same term, against every document -- "document" is in all four,
    // with the same idf and the same tf, so every score is identical.
    assert_eq!(
        tfidf.tfidfs(Terms::Text("document")).unwrap(),
        [0.776_856_448_685_790_3; 4],
    );

    // Every term OF document 3, ranked descending. "node" appears twice in
    // that document and nowhere else it doesn't, so it wins.
    let ranked = tfidf.list_terms(3).unwrap();
    let names: Vec<&str> = ranked.iter().map(|t| t.term.as_str()).collect();
    assert_eq!(names, ["node", "examples", "document"]);
}
```

### The process-global tokenizer and stop-word list

<a class="badge badge-global" href="../performance/parallelism">GLOBAL STATE</a>

The reference TF-IDF implementation opens with two **module-level** `let`s:

```text
let tokenizer = new Tokenizer()                       // module scope
let stopwords = require('../util/stopwords').words     // module scope
```

`setTokenizer` and `setStopwords` read as instance methods but assign to
those, so calling either on *one* `TfIdf` changes how *every* `TfIdf` in the
process tokenizes its next string document — including instances created
before the call. This is the exact hazard `verbora_ngrams::set_tokenizer`
documents on [Ngrams](./ngrams) and that
[Parallelism](../performance/parallelism) generalises across the workspace;
read those first if the shape (`RwLock` + `AtomicBool`, an explicit-argument
escape hatch) is new to you. `verbora-tfidf` reuses the same shape for its own
two globals, with two differences worth knowing precisely rather than
assuming:

- **Its own statics.** `verbora_tfidf::globals` owns a private
  `RwLock<Option<Arc<dyn TfIdfTokenizer>>>` and a private
  `RwLock<Option<Arc<StopwordSet>>>` — this crate's tokenizer swap is
  independent of `verbora_ngrams`'s (a different crate, a different global);
  they cannot interfere with each other.
- **Stricter atomics.** The "has this ever been overridden" flags use
  `Ordering::Release` on write and `Ordering::Acquire` on read. The
  established precedent in `verbora_ngrams::tokenizer` and
  `verbora_core::stopwords` uses `Ordering::Relaxed` throughout, on the
  reasoning that a torn read only ever falls back to the correct default
  behaviour rather than corrupting anything. `verbora-tfidf`'s stricter
  ordering is not documented as a deliberate divergence in its own source; it
  is simply what the code does, noted here rather than assumed.
- **No `..._with` sibling for the tokenizer.** `verbora_ngrams` offers
  `ngrams_str_with(&tokenizer, …)`, an explicit-argument escape hatch that
  reads no global. `verbora-tfidf` has no equivalent for `add_document`,
  `tfidf` or `tfidfs`. Its escape hatch is a different API entirely:
  `DocumentInput::Tokens` / `Terms::Tokens`, which tokenizes **outside** the
  crate — do your own tokenizing however you like, then hand over the
  `&[&str]` — and reads no global by construction.

The stop-word list has one more layer than the tokenizer does. Until
`TfIdf::set_stopwords` is called, `is_stopword` does not consult an
empty-until-set slot of its own — it **aliases**
`verbora_core::stopwords`'s process-global list, the same one every
stemmer's `addStopWord` mutates and [Phonetics](./phonetics) reads. Only after
`set_stopwords` is called does `verbora-tfidf` switch to its own,
independent list:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    verbora_tfidf::globals::reset_stopwords();

    // "mango" becomes a stop word for EVERY consumer of the shared list --
    // stemmers, phonetics, and any TfIdf that hasn't called set_stopwords.
    verbora_core::stopwords::add_global_stopword("mango");

    let mut t = TfIdf::new();
    t.add_document(DocumentInput::Text("node and mango"), DocKey::Undefined, false)
        .unwrap();
    let names: Vec<String> = t.list_terms(0).unwrap().into_iter().map(|s| s.term).collect();
    assert_eq!(names, ["node"]);

    verbora_core::stopwords::remove_global_stopword("mango");
}
```

Installing a custom tokenizer affects instances that already exist, exactly
as the reference's spec suite relies on:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};
use std::sync::Arc;

fn main() {
    let mut a = TfIdf::new();
    a.add_document(DocumentInput::Text("this isn't node"), DocKey::string("a"), false)
        .unwrap();
    let default_terms: Vec<String> =
        a.list_terms(0).unwrap().into_iter().map(|t| t.term).collect();
    assert_eq!(default_terms, ["isn", "node"]);

    // Installed through a DIFFERENT call, after `a` already exists.
    TfIdf::set_tokenizer(Arc::new(verbora_tokenizers::TreebankWordTokenizer::new()));

    // The OLD instance `a` is affected by a global installed after it was built.
    a.add_document(DocumentInput::Text("this isn't node"), DocKey::string("b"), false)
        .unwrap();
    let treebank_terms: Vec<String> =
        a.list_terms(1).unwrap().into_iter().map(|t| t.term).collect();
    assert_eq!(treebank_terms, ["n't", "node"]);

    // Undo it -- the reference has no such call; this exists so tests (and this
    // page's later snippets) can isolate themselves.
    verbora_tfidf::globals::reset_tokenizer();
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> If you call <code>TfIdf::set_tokenizer</code> or
<code>TfIdf::set_stopwords</code> from your own tests, reset them afterwards
(<code>verbora_tfidf::globals::reset_tokenizer</code> /
<code>reset_stopwords</code>) or isolate the tests that touch them behind a
mutex, the same way the crate's own tests do.
Rust test binaries run on multiple threads in one process by default, so an
unguarded test can observe another one's tokenizer mid-flight.
</div>

## Advanced usage

### `add_file_sync` and encoded reads

`add_file_sync(path, encoding, key, restore_cache)` is `addFileSync`: it reads
a file, decodes it with a Node `Buffer` encoding, and feeds the resulting text
through the same path as `DocumentInput::Text` — lowercased, tokenized,
stop-word filtered. The encoding is not mere validation; it decides what text
gets tokenized. Reading a UTF-8 file as `base64` does not fail — it tokenizes
the **base64 text of the bytes**:

```rust
use verbora_tfidf::{DocKey, TfIdf};

fn main() {
    let dir = std::env::temp_dir().join(format!(
        "verbora-tfidf-docs-addfilesync-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.txt");
    std::fs::write(&path, "this document is about node.").unwrap();

    let mut b64 = TfIdf::new();
    b64.add_file_sync(&path, Some("base64"), DocKey::Undefined, false)
        .unwrap();
    let terms: Vec<String> = b64.list_terms(0).unwrap().into_iter().map(|t| t.term).collect();
    // The base64 TEXT of the file's bytes, lowercased and stop-worded like any
    // other string document -- not the file's own words.
    assert_eq!(terms, ["dghpcybkb2n1bwvudcbpcybhym91dcbub2rllg"]);

    let mut plain = TfIdf::new();
    plain.add_file_sync(&path, None, DocKey::Undefined, false).unwrap();
    // utf8 (the default): the file's own words, tokenized normally.
    assert_eq!(plain.tf_at("node", 0).unwrap().to_number(), 1.0);

    std::fs::remove_dir_all(&dir).ok();
}
```

`encoding: None` (and `Some("")`) becomes `utf8` — `if (!encoding) encoding =
'utf8'`, reproduced with the same falsy test rather than a `match`. See
[Encoding](#encoding) for the full accepted set and how it compares to Node's
`Buffer.isEncoding`.

### Concurrency

`TfIdf`, `Document`, `DocKey` and `DynValue` are all `Send + Sync` — the crate's
own tests assert it. That buys you nothing extra here, though: every mutating
method takes `&mut self`, so `TfIdf` itself is not a type you share behind an
`Arc` and query concurrently the way [`Trie`](./core) is.
[`par_add_documents_batch`](#adding-many-documents-at-once-par-add-documents-batch)
does not change that: it is still one `&mut self` call from one thread, whose
*internal* tokenizing phase happens to run on more than one thread, behind
the `parallel` Cargo feature. What *is* genuinely
process-wide is [the tokenizer and stop-word globals](#the-process-global-tokenizer-and-stop-word-list)
— a thread calling `set_tokenizer` changes what every other thread's
`add_document` does next, with no error to tell you it happened. See
[Parallelism](../performance/parallelism).

## Faithful, not flattering

The crate's own module documentation lists five reasons this port is harder
than a plain reimplementation would be. Each is reproduced deliberately, and
each is covered by the test fixture.

### `Object.create(null)` vs `{}`: the idf cache's own prototype

The reference installs two visibly different objects as `this._idfCache`
depending on how the instance got there: the **constructor** and
**`addFileSync`** install a plain `{}`, which inherits from
`Object.prototype`; **`addDocument`** and **`removeDocument`** install
`Object.create(null)`, which inherits from nothing. The cache probe,
`this._idfCache[term]`, is a **truthiness** test, not a `hasOwnProperty`
check — so on the prototype-backed kind, a term named after a real
`Object.prototype` method (`toString`, `constructor`, `hasOwnProperty`, …)
finds that inherited method and returns it, *as if it had been cached*.

This is modeled with a public type built exactly for this: `DynValue`, whose
`Function` and `Prototype` variants exist so an inherited member can be a
real return value instead of being coerced to `NaN` on the way out, and
`TfIdf::idf_cache_is_prototype_backed`, which reports which kind of cache is
currently installed. `idf` (the `f64`
convenience) coerces every variant with `to_number()`, same as every
arithmetic use in the reference; `idf_value` is the one that shows you what
actually happened:

```rust
use verbora_tfidf::{DocKey, DocumentInput, DynValue, TfIdf};

fn main() {
    // A fresh TfIdf::new() starts prototype-backed, exactly like `new TfIdf()`.
    let mut t = TfIdf::new();
    assert!(t.idf_cache_is_prototype_backed());
    assert!(matches!(
        t.idf_value("toString", false).unwrap(),
        DynValue::Function("toString")
    ));

    // The FIRST addDocument call (without restore_cache) swaps the cache's
    // identity to Object.create(null) -- from here on, "toString" is just a
    // term like any other.
    t.add_document(DocumentInput::Text("x"), DocKey::Undefined, false)
        .unwrap();
    assert!(!t.idf_cache_is_prototype_backed());
    assert!(matches!(t.idf_value("toString", false).unwrap(), DynValue::Num(_)));
}
```

`addFileSync` puts the flag back: unlike `addDocument`, its non-`restore_cache`
branch reinstalls a plain `{}`, not `Object.create(null)` — reproduced with the
matching `add_file_sync` example in [Advanced usage](#add-file-sync-and-encoded-reads).
A `HashMap`-backed cache would answer `idf("toString")` the same way
regardless of which constructor built the instance, silently erasing a
distinction the reference's own spec suite depends on.

### `__proto__` and `__key`: term collisions with the accumulator's own shape

Because a built document's underlying accumulator is (conceptually) `{
__key: key }`, two token spellings are not ordinary terms:

- **`__proto__`.** `document.__proto__ = <number>` invokes the prototype
  accessor, which silently ignores anything that is not an object or `null`.
  A token spelled this way is never stored.
- **`__key`.** A token spelled this way is folded into the document's own
  key with the reference `+`: a string key gets `1` string-concatenated onto it
  each time; an absent key starts at `DocKey::Num(1.0)` and increments
  numerically from there.

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut t = TfIdf::new();
    t.add_document(
        DocumentInput::Text("__proto__ __proto__ alpha"),
        DocKey::Undefined,
        false,
    )
    .unwrap();
    // Both __proto__ tokens vanished; only "alpha" was actually stored.
    assert_eq!(t.tf_at("alpha", 0).unwrap().to_number(), 1.0);

    let mut t2 = TfIdf::new();
    t2.add_document(
        DocumentInput::Text("__key __key alpha"),
        DocKey::string("mykey"),
        false,
    )
    .unwrap();
    // "mykey" + 1 + 1, as STRING concatenation, not arithmetic.
    let key = t2.documents().unwrap()[0]
        .key_value(t2.interner())
        .unwrap();
    assert_eq!(key.to_text(), "mykey11");
}
```

Issue #119 adds a third: a term that spells an **inherited**
`Object.prototype` method name (`toString`, `hasOwnProperty`, …) shadows that
method on first use, so the reference resets the slot to a literal `0` before
counting — even when the term is about to be stop-word filtered, which is why
a stop-worded `toString` still leaves a zero-valued own property behind rather
than no property at all. `BuiltDocument::observe` reproduces the reset
unconditionally, ahead of the stop-word test, exactly in that order.

### `for…in` order in `list_terms`

A document's terms are visited in **`for…in` order**: every key that is the
canonical decimal spelling of an integer in `0..=2^32-2` — an *array
index*, stricter than "parses as a number": `"01"`, `"1.0"`, `"-1"` and
`"1e3"` all fail this test — is hoisted to the front, sorted ascending
numerically, ahead of every other key, which keeps insertion order. The
default `WordTokenizer` keeps digit runs as terms, so a real corpus hits this
constantly, not as an edge case:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut t = TfIdf::new();
    t.add_document(
        DocumentInput::Text("zeta 2020 alpha 10 beta"),
        DocKey::Undefined,
        false,
    )
    .unwrap();
    let names: Vec<String> = t.list_terms(0).unwrap().into_iter().map(|s| s.term).collect();
    // "10" and "2020" hoist ahead of the words, in ASCENDING NUMERIC order --
    // not the order they appeared in the text (2020 was typed before 10).
    assert_eq!(names, ["10", "2020", "zeta", "alpha", "beta"]);
}
```

A `BTreeMap` (lexicographic order) or a plain hash map (arbitrary order)
would both diverge from this silently. `list_terms`' own descending sort by
score is stable, so when scores tie, this `for…in` order is exactly what
survives — see the next two sections for why the sort has to be a specific
algorithm, not merely "a stable one".

### Float accumulation order, and why `f64::ln` is not `Math.log`

The entire numeric content of `idf` is one line:

```text
const idf = 1 + Math.log(this.documents.length / (1 + docsWithTerm))
```

and the reference specs assert its result with `toBe` — bit equality. Two hazards
compound in that single line, and this crate reproduces both:

1. **The division happens inside the logarithm**, written exactly that way.
   The algebraically equal `ln(n) - ln(1 + d)` differs in the last bit for
   some inputs, so `TfIdf::idf_value` computes
   `1.0 + math_log(total / (1.0 + docs_with_term))`, not the rearranged form.
2. **`Math.log` is not the platform's `libm`.** The reference engine runs its own port of Sun's
   fdlibm `__ieee754_log`, and glibc's `log` disagrees with it by one ULP on
   a measured 5.6% of the 3,418 inputs recorded in the `mathLog` fixture
   suite — including `log(3)`, which is exactly what a three-document corpus
   with one match produces:

   ```text
   Math.log(3)   the reference engine      1.0986122886681096
                 glibc   1.0986122886681098
   ```

   `math_log` is that fdlibm port, transcribed with the reference C's own
   bit-pattern constants (`f64::from_bits`, not decimal literals, because a
   truncated decimal is a silently different double) and its exact
   parenthesisation, down to comments noting which re-associations are
   "algebraically free and numerically not". `1 + ln(3)` is not an exotic
   corner case this crate happens to get right; it is the first number a
   real corpus produces.

```rust
fn main() {
    let a = verbora_tfidf::math_log(3.0);
    let b = 3.0_f64.ln();
    assert_ne!(a.to_bits(), b.to_bits());
    assert_eq!(a.to_bits(), 1.098_612_288_668_109_6_f64.to_bits());
}
```

`tfidf`'s own accumulation is equally exact: it sums `tf * idf` **strictly
left to right** over the query's term list, because the reference specs compare that
sum with `toBe` too. Only `+Infinity` is clamped to `0` (an idf of `+Infinity`
cannot arise from a real corpus but is defensively handled); `-Infinity` — a
real value, produced by `idf` over an **empty** corpus — and `NaN` pass
straight through unclamped.

### `list_terms`' sort is the reference engine's `TimSort`, not "a stable sort"

`listTerms` finishes with `terms.sort((x, y) => y.tfidf - x.tfidf)`. For
finite scores that is an ordinary descending sort and any stable algorithm
reproduces it — Rust's own `sort_by` included. It stops being that simple the
moment one score is `NaN`: a deserialized corpus with a non-numeric tf, or a
`toString`-shadowing term read through a prototype-backed cache, both produce
one. `NaN - x` is `NaN`, which the comparator's caller treats as "not less
than", i.e. as a **tie** — so the induced relation over the whole array stops
being transitive, and "the answer" becomes whatever the reference engine's specific algorithm
does with an inconsistent comparator, not a property of stability in general.

the private comparator-sort module (re-exposed only through `list_terms`'s output order)
is a full port of the reference engine's `ArrayTimSort`: natural-run detection with in-place
reversal of descending runs, binary insertion sort up to `minRunLength`, a
pending-run stack collapsed under the standard invariant, and galloping
merges with an adaptive threshold carried across merges — plus one boundary,
`MIN_TIM_SORT = 8`, below which the reference engine skips run detection entirely and goes
straight to binary insertion sort. That boundary only matters for an
inconsistent comparator: with one, a natural run of length 4 built from
scores `[NaN, 1, NaN, 3]` is reported as "already sorted, nothing to do" by
run detection, while a plain insertion sort still moves an element in front of
another, because that particular pair genuinely compares. The module's own
provenance note records how the boundary of exactly 8 was established: run
detection first against the reference engine's *comparator call sequence* (not just its final
output) on 45,150 randomised inputs and all 87,381 inputs of length ≤ 8 over
`{NaN, 0, 1, 2}`; moving the boundary to 7 or 9 breaks every trial at length 7
or 8 respectively.

Two implementation details exist **only** because a comparator can be
inconsistent, and a port that "tidied" either one away would loop forever on
exactly the corpus this section is about: a merge returning early when a
gallop consumes an entire run, and a zero-length check inside the merge
gallop loops. This module is a direct copy of `verbora-spellcheck`'s
that module, itself validated against real reference-engine output over 1,015 recorded
permutations; it is duplicated rather than shared because the two crates have
no dependency relationship.

Concretely, on a document whose four properties score `[1, 0, -3.39, NaN]` in
`for…in` order, this crate's sort reproduces exactly what the reference engine does — the
`NaN`-scored term neither panics nor corrupts the order of the others:

```rust
use verbora_tfidf::{DocKey, DocumentInput, JsonValue, TfIdf};

fn main() {
    let mut t = TfIdf::new();
    t.add_document(
        DocumentInput::Raw(JsonValue::Obj(vec![
            ("x".into(), JsonValue::Num(0.0)),
            ("y".into(), JsonValue::Num(-2.0)),
            ("z".into(), JsonValue::Str("three".into())), // a non-numeric tf -> NaN score
            ("w".into(), JsonValue::Num(1.0)),
        ])),
        DocKey::Undefined,
        false,
    )
    .unwrap();
    // A second document so idf isn't degenerately zero for every term.
    t.add_document(
        DocumentInput::Raw(JsonValue::Obj(vec![("x".into(), JsonValue::Num(1.0))])),
        DocKey::Undefined,
        false,
    )
    .unwrap();

    let names: Vec<String> = t.list_terms(0).unwrap().into_iter().map(|s| s.term).collect();
    assert_eq!(names, ["w", "x", "y", "z"]);
}
```

## Performance characteristics

`crates/verbora-tfidf/benches/tfidf.rs` is a Criterion suite with five
groups, run against a 167 kB real document (the English Wikipedia article on
the French Revolution that ships with the reference tree, falling back to a
synthetic repeat when that checkout is absent). The comments on each group
record what one run on one machine found, and two of the four numbers below
changed a design decision rather than merely describing one:

| Group | What it answers |
|---|---|
| `build` | ingestion cost, and whether the term interner is worth it over a plain `HashMap<String, f64>` per document |
| `idf_cold` | the cost of an `idf` cache miss, built vs. deserialized, at 1/8/64/256 documents |
| `query` | `tfidf`, `tfidfs`, `list_terms`, `to_json`, `from_json` on a warm 64-document corpus |
| `math_log` | the fdlibm port against the platform `f64::ln` it replaced |
| `documents` | `add_document` on a raw (unbuilt) document, and `remove_document` |

The interner's payoff is small for a **single** document (2.78 ms either way
— nothing to share yet) and measurable across a corpus: eight documents built
through one interning `TfIdf` cost ~18.1 ms against ~21.8 ms for eight
independent `HashMap`s, roughly 17% less, because a repeated term is
allocated once for the whole corpus instead of once per document. The larger
payoff is not in this group at all — it is what makes the `u32` `TermId` keys
of the incremental document-frequency table in `idf_cold` possible; see
[`TfIdf::new` vs `TfIdf::from_json`](#tfidf-new-vs-tfidf-from-json) for those
numbers. `math_log` prices exactness itself: the fdlibm port measured ~7.6 µs
against ~5.1 µs for `f64::ln` over 2,048 realistic ratios — roughly 1.5× for a
bit-exact logarithm, which is the entire cost of the fix in
[Float accumulation order](#float-accumulation-order-and-why-f64-ln-is-not-math-log).

<div class="callout callout-note">
<strong>Not yet benchmarked against the reference.</strong> These are Rust-only,
comparative-within-this-crate numbers (interner vs. baseline, built vs.
deserialized, fdlibm vs. platform libm) — there is no recorded reference
timing baseline or joined comparison table for TF-IDF. The only published
cross-language numbers on this site are the 26 <code>verbora-distance</code>
benchmarks. See <a href="../benchmarks/index">Benchmarks</a>, and reproduce
the numbers above yourself with <code>cargo bench -p verbora-tfidf</code>.
</div>

## Allocation behaviour

**Interning.** `Interner::intern` allocates one `Arc<str>` per **distinct**
term in the corpus, shared by every document that contains it — a term
appearing in fifty documents costs one allocation, not fifty. Query terms
passed to `tfidf`/`tfidfs`/`idf` are deliberately **not** interned:
`Interner::lookup` answers without inserting, so probing a corpus with
arbitrary or adversarial query terms cannot grow the table.

**A built document (`BuiltDocument`, the `TfIdf::new` path).** One
`Vec<(TermId, f64)>` holding the document's entries in insertion order, plus
one `FxHashMap<TermId, u32>` index over it. `observe` never reallocates the
`Vec` beyond normal growth — there is no separate "count" allocation per
occurrence, only per **distinct** term.

**A raw document (`RawDocument`, the `TfIdf::from_json` path).** One clone of
the parsed `JsonValue` (itself one `Vec<(String, JsonValue)>` per JSON
object, recursively, from `serde_json`'s deserializer through this crate's own
order-preserving `JsonValue` deserialization), plus one `FxHashMap<Box<str>,
u32>` own-property index built once at construction so later lookups are
O(1) rather than O(document's own keys).

**The idf cache.** One `Vec<(Arc<str>, f64)>` plus one `FxHashMap<Arc<str>,
u32>` index, mirroring the document-entry representation above. `idf_value`
only allocates when a **new** term is cached (one `Arc::from`); recomputing an
already-cached term overwrites its slot.

**`list_terms`.** One `Vec<TermScore>` sized to the document's own term
count, one `String` clone per term name (`TermScore::term` is owned so the
result can outlive the corpus's own interner), and — only when the query
touches strings — the same `Cow`-avoiding lowercasing every string document
goes through (see below). The sort itself allocates one `temp: Vec<T>` scratch
buffer, reused across merges within one call, matching the reference engine's own `tempArray`.

**Lowercasing.** `DocumentInput::Text` and `Terms::Text` both lowercase before
tokenizing, but not unconditionally: a private `lowercase_units` scans the input
first, and only allocates (`str::to_lowercase`, a full rewrite) when it finds
a byte that is non-ASCII or an ASCII uppercase letter. Prose that is already
lowercase ASCII — the common case for a real corpus — borrows the input
`&str` and allocates nothing for this step. The scan is deliberately
conservative: any non-ASCII byte hands the whole string to `to_lowercase`, so
the genuine Unicode special cases (`'İ'` expanding to two code points, final
sigma) are never handled by a byte-local approximation.

**The default tokenizer path.** `globals::tokenize_global` on the untouched
default tokenizer borrows tokens directly out of the input `&str`
(`GlobalTokens::Default`, wrapping `WordTokens<'_>`) — no per-token
allocation. Only once `set_tokenizer` has installed a custom `dyn
TfIdfTokenizer` does tokenizing allocate one owned `String` per token,
because a `dyn` boundary cannot hand back a borrow tied to the caller's input.

There is no `_into` variant and no caller-supplied output buffer anywhere in
this crate. See [Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Encoding

`Encoding` exists for exactly one call: `add_file_sync`'s `encoding`
argument, which the reference hands to `fs.readFileSync(path, encoding)` — a
call that returns a **string**, so the encoding is not validation, it decides
what text gets tokenized.

```rust ignore
pub enum Encoding { Utf8, Ascii, Latin1, Base64, Base64Url, Hex, Utf16Le }
impl Encoding {
    pub fn parse(name: &str) -> Option<Self>;
    pub fn decode(self, bytes: &[u8]) -> String;
}
```

`Encoding::parse` accepts exactly what Node's `Buffer.isEncoding` accepts,
case-insensitively: `utf8`/`utf-8`, `ascii`, `latin1`/`binary`, `base64`,
`base64url`, `hex`, and `ucs2`/`ucs-2`/`utf16le`/`utf-16le`. Nothing else,
which matters because `tfidf` also carries a **dead** hand-written
`isEncoding` switch, left over from "< node 0.10" support, whose branches are
unreachable on any modern Node — including a branch that accepts `'raw'`.
`Buffer.isEncoding('raw')` is `false`, so `add_file_sync(path, Some("raw"),
…)` returns `Error::InvalidEncoding`, reproducing what actually runs today,
not what the dead switch would have accepted:

```rust
use verbora_tfidf::Encoding;

fn main() {
    assert!(Encoding::parse("BASE64").is_some()); // case-insensitive
    assert!(Encoding::parse("raw").is_none());     // the dead switch's own branch
    assert!(Encoding::parse("buffer").is_none());

    // Node: Buffer.from('this document is about node.').toString('base64')
    assert_eq!(
        Encoding::Base64.decode(b"this document is about node."),
        "dGhpcyBkb2N1bWVudCBpcyBhYm91dCBub2RlLg=="
    );
    assert_eq!(Encoding::Hex.decode(b"node"), "6e6f6465");
}
```

`Utf8` decoding is lossy the same way Node's is: malformed sequences become
U+FFFD (`String::from_utf8_lossy`), not an error. `Utf16Le` drops a trailing
odd byte rather than erroring, matching Node. There is no `mmap`, no
byte-order-mark handling beyond what each encoding does natively, and no
detection — the caller names the encoding, exactly as `addFileSync`'s
signature requires.

## Unicode and language notes

**The default tokenizer path has no UTF-16-sensitive indexing of its own.**
`DocumentInput::Text` and `Terms::Text` tokenize with whichever tokenizer is
installed — the default `WordTokenizer`, splitting on `[^A-Za-zА-Яа-я0-9_]+`
— which operates on ordinary UTF-8 `&str` and yields borrowed `&str` slices,
not `Utf16Token`s. Every divergence you will see through this path is
downstream of *that* tokenizer's own word-character class, already documented
on [Tokenizers](./tokenizers), not something this crate adds:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    for (text, expected) in [
        ("naïve café crème brûlée", &["na", "ve", "caf", "cr", "br"][..]),
        ("İstanbul", &["stanbul"]),           // 'İ'.to_lowercase() splits into i + combining dot
        ("日本語 test 中文测试", &["test"]),    // CJK is outside the word class entirely
        ("😀abc😀", &["abc"]),                 // astral characters separate words, are dropped
    ] {
        let mut t = TfIdf::new();
        t.add_document(DocumentInput::Text(text), DocKey::Undefined, false).unwrap();
        let names: Vec<String> = t.list_terms(0).unwrap().into_iter().map(|s| s.term).collect();
        assert_eq!(names, expected, "{text:?}");
    }
}
```

**`RawDocument` string-shaped documents genuinely are
<span class="badge badge-utf16">UTF-16</span>-sensitive.**
This is specific to TF-IDF's own document model, not inherited from a
tokenizer: `buildDocument` returns a bare the reference string unchanged when it
is handed one (reachable through `DocumentInput::Raw(JsonValue::Str(..))`, or
through any corpus restored via `from_json` whose `documents` array holds a
plain string). Reading a term from such a slot is then a reference property
read on a **string**, and the reference indexes strings by UTF-16 code unit —
`s[i]` and `for (i in s)` walk code-unit positions, not Rust `char`s or UTF-8
byte offsets:

```rust
use verbora_tfidf::{Document, Interner, JsonValue, RawDocument};

fn main() {
    let interner = Interner::default();

    let cafe = Document::Raw(RawDocument::new(JsonValue::Str("café".into())));
    assert_eq!(cafe.for_in_keys(&interner), ["0", "1", "2", "3"]); // 4 UTF-16 units
    assert_eq!(cafe.get("3", &interner).unwrap().to_text(), "é"); // one unit: BMP

    // An astral character is TWO UTF-16 units -- a surrogate pair.
    let emoji = Document::Raw(RawDocument::new(JsonValue::Str("a\u{1F600}b".into())));
    assert_eq!(emoji.for_in_keys(&interner), ["0", "1", "2", "3"]);
    // Indexing lands ON one half of the pair: an unpaired surrogate, rendered
    // as U+FFFD by String::from_utf16_lossy, same as the reference would hand
    // back an unpaired surrogate code point.
    assert_eq!(emoji.get("1", &interner).unwrap().to_text(), "\u{FFFD}");
    assert_eq!(emoji.get("3", &interner).unwrap().to_text(), "b");
}
```

**Lookup normalisation is full Unicode lowercasing**, not
`str::to_ascii_lowercase` — `lowercase_units`'s ASCII fast path (see
[Allocation behaviour](#allocation-behaviour)) only ever *skips* the
allocating `str::to_lowercase` call; it never substitutes a cheaper, wrong
answer. `'İSTANBUL'.to_lowercase()` genuinely produces more code points than
it started with, and that expansion is what then gets tokenized.

## Common mistakes

**Assuming `DocumentInput::Tokens` behaves like `DocumentInput::Text` minus
tokenizing.** It also skips lowercasing and stop-word filtering — both, not
just one — because the reference's `buildDocument` takes an entirely
different branch for an array argument. `["The", "the", "THE"]` is three
distinct terms, not one:

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut t = TfIdf::new();
    t.add_document(
        DocumentInput::Tokens(&["The", "the", "THE"]),
        DocKey::Undefined,
        false,
    )
    .unwrap();
    let names: Vec<String> = t.list_terms(0).unwrap().into_iter().map(|s| s.term).collect();
    assert_eq!(names, ["The", "the", "THE"]); // NOT merged into one "the": 1
}
```

**Assuming a cached `idf` stays valid — or assuming `restore_cache` is free.**
Neither holds unconditionally. Without `restore_cache`, every previously
cached idf becomes stale the moment you call `add_document` or
`remove_document` again — reads after that are correct (the cache is empty,
so nothing stale is ever returned) but cold. With `restore_cache: true`, every
**currently cached** term is recomputed right then, which is worth doing when
you need every cached value correct immediately, and mostly wasted effort
when nothing is about to read them again soon. See
[The idf cache and `restore_cache`](#the-idf-cache-and-restore-cache).

**Treating `list_terms`'s order as arbitrary.** It is not: ties are broken by
the document's own `for…in` order (array-index-like keys first, ascending,
then insertion order), applied through the reference engine's specific TimSort rather than an
arbitrary stable sort. Two runs over the same corpus produce the same order
every time, including when a score is `NaN` — see
[`for…in` order in `list_terms`](#for-in-order-in-list-terms) and
[`list_terms`' sort is the reference engine's `TimSort`](#list-terms-sort-is-the-reference-engine-s-timsort-not-a-stable-sort).

## Related

- [Choosing an API](../choosing/index) — the cross-crate version of the
  decision trees on this page.
- [Ngrams](./ngrams) — the precedent this page's process-global
  tokenizer section builds on: the exact same `RwLock` + `AtomicBool` shape,
  explained once there.
- [Parallelism](../performance/parallelism) — the thirteen built-in `par_*`
  APIs across the workspace, this crate's own two-phase design among them,
  and the correctness (not memory-safety) hazard of the tokenizer and
  stop-word globals under concurrent use.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy) — the vocabulary behind
  [Allocation behaviour](#allocation-behaviour) above.
  disagrees with Rust `char`-based indexing.
- [Benchmarks](../benchmarks/index) — what has and has not been measured.
- [Core traits](./core) — `Tokenizer`, the process-global pattern's other
  workspace instance (`verbora_core::stopwords`), and the shared vocabulary
  the rest of the site uses.
- [Recipes](../recipes/index) — end-to-end pipelines.

## API reference

Everything the crate exports:

```rust ignore
// verbora_tfidf (crate root re-exports)
pub use document::{BuiltDocument, DocKey, Document, Interner, RawDocument, TermId};
pub use encoding::Encoding;
pub use globals::{StopwordElement, StopwordList, TfIdfTokenizer};
pub use value::{DynValue, JsonValue, Proto};
pub use mathlog::math_log;
pub use tfidf::{DocumentInput, TermScore, Terms, TfIdf, TfIdfError};

// tfidf — TfIdf itself
pub enum DocumentInput<'a> { Text(&'a str), Tokens(&'a [&'a str]), Raw(JsonValue) }
pub enum Terms<'a> { Text(&'a str), Tokens(&'a [&'a str]) }
pub struct TermScore { pub term: String, pub tf: DynValue, pub idf: DynValue, pub tfidf: f64 }
impl TermScore {
    pub fn tf_as_f64(&self) -> f64;
    pub fn idf_as_f64(&self) -> f64;
}

pub struct TfIdf { /* private */ }
impl Default for TfIdf { fn default() -> Self; }
impl TfIdf {
    pub fn new() -> Self;
    pub fn from_value(deserialized: &JsonValue) -> Self;
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error>;

    pub fn set_tokenizer(tokenizer: std::sync::Arc<dyn TfIdfTokenizer>);
    pub fn set_stopwords(list: &StopwordList) -> bool;

    pub fn documents(&self) -> Option<&[Document]>;
    pub fn interner(&self) -> &Interner;
    pub fn idf_cache(&self) -> Vec<(String, f64)>;
    pub fn idf_cache_is_prototype_backed(&self) -> bool;

    pub fn tf(term: &str, document: &Document, interner: &Interner) -> Result<DynValue, TfIdfError>;
    pub fn tf_at(&self, term: &str, d: usize) -> Result<DynValue, TfIdfError>;

    pub fn idf(&mut self, term: &str) -> Result<f64, TfIdfError>;
    pub fn idf_value(&mut self, term: &str, force: bool) -> Result<DynValue, TfIdfError>;

    pub fn add_document(&mut self, document: DocumentInput<'_>, key: DocKey, restore_cache: bool) -> Result<(), TfIdfError>;
    pub fn add_file_sync(&mut self, path: impl AsRef<std::path::Path>, encoding: Option<&str>, key: DocKey, restore_cache: bool) -> Result<(), TfIdfError>;
    // requires the `parallel` Cargo feature; Text only, always restore_cache: false
    pub fn par_add_documents_batch(&mut self, documents: &[(&str, DocKey)]) -> Result<(), TfIdfError>;
    pub fn remove_document(&mut self, key: &DocKey) -> Result<bool, TfIdfError>;

    pub fn tfidf(&mut self, terms: Terms<'_>, d: usize) -> Result<f64, TfIdfError>;
    pub fn tfidfs(&mut self, terms: Terms<'_>) -> Result<Vec<f64>, TfIdfError>;
    pub fn tfidfs_with(&mut self, terms: Terms<'_>, callback: impl FnMut(usize, f64, DynValue)) -> Result<Vec<f64>, TfIdfError>;
    pub fn list_terms(&mut self, d: usize) -> Result<Vec<TermScore>, TfIdfError>;

    pub fn to_json(&self) -> String;
}

pub enum TfIdfError { UndefinedRead(String), NullRead(String), InvalidEncoding(String), Io(std::io::Error) }
impl std::fmt::Display for TfIdfError { /* byte-identical to the reference TypeError text */ }
impl std::error::Error for TfIdfError { fn source(&self) -> Option<&(dyn std::error::Error + 'static)>; }
impl From<std::io::Error> for TfIdfError { fn from(e: std::io::Error) -> Self; }

// document — documents, keys, the term interner
pub type TermId = u32;
pub const KEY_PROPERTY: &str = "__key";
pub const PROTO_PROPERTY: &str = "__proto__";

pub struct Interner { /* private */ }
impl Interner {
    pub fn intern(&mut self, term: &str) -> TermId;
    pub fn lookup(&self, term: &str) -> Option<TermId>;
    pub fn name(&self, id: TermId) -> &str;
}

pub enum DocKey { Undefined, Null, Bool(bool), Num(f64), Str(std::sync::Arc<str>), Object(std::sync::Arc<JsonValue>) }
impl DocKey {
    pub fn string(s: impl AsRef<str>) -> Self;
    pub fn object(value: JsonValue) -> Self;
    pub fn strict_eq(&self, other: &Self) -> bool;   // reference ===
    pub fn is_truthy(&self) -> bool;
    pub fn plus_one(&self) -> Self;                   // the reference `key + 1`
    pub fn as_value(&self) -> DynValue;
    pub fn write_json(&self, out: &mut String) -> bool;
}

pub struct BuiltDocument { /* private */ }
impl BuiltDocument {
    pub fn new(key: DocKey) -> Self;
    pub fn key(&self) -> &DocKey;
    pub fn count(&self, id: TermId) -> Option<f64>;
    pub fn entries(&self) -> &[(TermId, f64)];
    pub fn observe(&mut self, id: TermId, term: &str, filtered: bool, interner: &Interner);
    pub fn ordered_slots(&self, interner: &Interner) -> Vec<Slot>;
}
pub enum Slot { Key, Term(u32) }

pub enum Document { Built(BuiltDocument), Raw(RawDocument) }
impl Document {
    pub fn get(&self, term: &str, interner: &Interner) -> Result<DynValue, ReadTarget>;
    pub fn key_value(&self, interner: &Interner) -> Result<DynValue, ReadTarget>;
    pub fn remove_key(&self) -> Result<DocKey, ReadTarget>;
    pub fn for_in_keys(&self, interner: &Interner) -> Vec<String>;
}
pub enum ReadTarget { Undefined, Null }

pub struct RawDocument { /* private */ }
impl RawDocument {
    pub fn new(value: JsonValue) -> Self;
    pub fn from_arc(value: std::sync::Arc<JsonValue>) -> Self;
    pub fn value(&self) -> &JsonValue;
    pub fn arc(&self) -> &std::sync::Arc<JsonValue>;
}

// value — the slice of reference value semantics this crate depends on
pub enum JsonValue { Null, Bool(bool), Num(f64), Str(String), Arr(Vec<JsonValue>), Obj(Vec<(String, JsonValue)>) }
impl JsonValue {
    pub fn own(&self, key: &str) -> Option<&JsonValue>;
    pub fn is_truthy(&self) -> bool;
    pub fn to_number(&self) -> f64;
    pub fn to_text(&self) -> String;
    pub fn write_json(&self, out: &mut String);
}
// impl<'de> Deserialize<'de> for JsonValue — preserves object key order

pub enum Proto { Object, Array, String, Number, Boolean }
impl Proto {
    pub const fn constructor_name(self) -> &'static str;
    pub const fn own_methods(self) -> &'static [&'static str];
}
pub const OBJECT_PROTOTYPE_METHODS: &[&str];
pub fn prototype_member(proto: Proto, key: &str) -> Option<DynValue>;

pub enum DynValue {
    Undefined, Null, Bool(bool), Num(f64), Str(std::sync::Arc<str>),
    Function(&'static str), Prototype(Proto), Json(std::sync::Arc<JsonValue>),
}
impl DynValue {
    pub fn is_truthy(&self) -> bool;
    pub fn to_number(&self) -> f64;
    pub fn to_text(&self) -> String;
    pub fn counts_as_present(&self) -> bool;   // `value && value > 0`
}

pub fn number_to_string(x: f64) -> String;   // the reference language Number::toString
pub fn string_to_number(s: &str) -> f64;     // the reference language ToNumber
pub fn write_json_string(s: &str, out: &mut String);
pub fn array_index(key: &str) -> Option<u32>;

// mathlog
pub fn math_log(x: f64) -> f64;   // the reference engine's Math.log, bit-identical

// encoding
pub enum Encoding { Utf8, Ascii, Latin1, Base64, Base64Url, Hex, Utf16Le }
impl Encoding {
    pub fn parse(name: &str) -> Option<Self>;
    pub fn decode(self, bytes: &[u8]) -> String;
}

// globals — the two process-global slots; see "The process-global
// tokenizer and stop-word list" above
pub trait TfIdfTokenizer: Send + Sync + std::fmt::Debug {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);
    fn tokenize(&self, text: &str) -> Vec<String> { /* default: calls tokenize_into */ }
}
pub fn set_tokenizer(tokenizer: std::sync::Arc<dyn TfIdfTokenizer>);
pub fn reset_tokenizer();
pub fn tokenizer_is_default() -> bool;
pub fn tokenize_global(text: &str) -> GlobalTokens<'_>;
pub enum GlobalTokens<'a> { Default(/* WordTokens<'a> */), Custom(std::vec::IntoIter<String>) }
impl GlobalTokens<'_> { pub fn for_each(self, f: impl FnMut(&str)); }

pub enum StopwordList { NotAnArray, Array(Vec<StopwordElement>) }
pub enum StopwordElement { Str(String), NotAString }
impl StopwordList { pub fn of<I: IntoIterator<Item = S>, S: Into<String>>(words: I) -> Self; }
pub fn set_stopwords(list: &StopwordList) -> bool;
pub fn reset_stopwords();
pub fn stopwords() -> Option<Vec<String>>;
pub fn is_stopword(term: &str) -> bool;
```

No `unsafe` anywhere in this crate. `TfIdf`, `Document`, `DocKey` and `DynValue`
are `Send + Sync`, but the crate's only *shared*, concurrency-relevant state
is the [two process-globals](#the-process-global-tokenizer-and-stop-word-list)
— see [Parallelism](../performance/parallelism) before sharing a tokenizer or
stop-word installation across threads.
`TfIdf::par_add_documents_batch` is the crate's one parallel entry point,
gated behind the `parallel` Cargo feature and off by default; see
[Adding many documents at once](#adding-many-documents-at-once-par-add-documents-batch)
above for why it is a two-phase design rather than a plain fan-out, and for
its modest, honestly-reported crossover numbers.
