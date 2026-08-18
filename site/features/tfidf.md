# TF-IDF

`verbora-tfidf` scores terms against documents. It keeps a growable in-memory
corpus, an inverse-document-frequency cache, and five queries built on top of
them — `tf`, `idf`, `tfidf`, `tfidfs`, `list_terms`.

A document is a property-keyed accumulator mapping term to count, with the
document's own key stored under the reserved property `__key`. Enumeration order,
reserved term names, and the dynamic values a deserialized corpus can hold are
all part of the contract, and all documented below.

<div class="callout callout-spec">
<strong>Specification status.</strong> The full stateful surface — corpus
construction, serialization, the idf cache, stop-word and tokenizer
configuration, term ordering and the empty-corpus case — is documented and
test-pinned, with no external data required.
<code>cargo test -p verbora-tfidf</code> runs <strong>52</strong> unit tests and
<strong>2</strong> doctests.
</div>

## When to use it

- **Ranking documents against a query, or ranking a document's own terms**, for a
  corpus that fits comfortably in memory and is built once and queried many times.
- **A corpus that grows incrementally.** `add_document` and `remove_document`
  maintain an incremental document-frequency table, so `idf` on a built corpus
  stays O(1) as documents come and go.
- **Ingesting a large batch of text documents in one call**, via
  [`par_add_documents_batch`](#adding-many-documents-at-once).
- **Exactly reproducible scores.** Term ordering, tie-breaking and the arithmetic
  of `idf` are pinned to bit equality, so the same corpus ranks the same way on
  every platform and every run.

## When not to use it

- **You want a `terms × documents` matrix, sparse vectors, or cosine similarity
  between documents.** Nothing here builds one: a document is a
  `Vec<(TermId, f64)>`, and the only cross-document operation is `idf`'s
  document-frequency count. Vector search has to be written on top.
- **You need plain hash-map semantics with no reserved keys.** `__proto__` and
  `__key` are special, and `list_terms`'s tie-break order is the enumeration order
  documented below rather than a lexicographic one.
- **You are counting term frequency for something other than ranking.** For a
  plain bag-of-words histogram, a `HashMap<&str, u32>` over your own tokenizer
  output is simpler.

## Quick example

```rust
use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};

fn main() {
    let mut tfidf = TfIdf::new();
    for text in [
        "this document is about rust.",
        "this document is about python.",
        "this document is about python and rust.",
        "this document is about rust. it has rust examples",
    ] {
        tfidf
            .add_document(DocumentInput::Text(text), DocKey::Undefined, false)
            .unwrap();
    }

    // "rust" is in three of the four documents: 1 + ln(4 / (1 + 3)) = 1.
    assert_eq!(tfidf.idf("rust").unwrap(), 1.0 + (4.0f64 / 4.0).ln());
    assert_eq!(tfidf.tfidfs(Terms::Text("rust")).unwrap(), [1.0, 0.0, 1.0, 2.0]);

    let ranked = tfidf.list_terms(3).unwrap();
    assert_eq!(ranked[0].term, "rust");
}
```

## Choosing the right API

| Decision | Options | What actually differs |
|---|---|---|
| Building the corpus | [`TfIdf::new`](#tfidf-new-vs-tfidf-from-json) / `TfIdf::from_json` | which internal representation every document gets — count table or scanned value |
| Feeding one document | [`DocumentInput::Text` / `::Tokens` / `::Raw`](#the-three-shapes-of-add-document) | lowercasing, stop-word filtering, and whether the slot can ever match a query |
| After mutating the corpus | [`restore_cache: bool`](#the-idf-cache-and-restore-cache) | discard every cached idf vs. recompute every cached idf in place, now |
| Adding many text documents | `add_document` in a loop / [`par_add_documents_batch`](#adding-many-documents-at-once) | parallel tokenizing ahead of a sequential ingest — `Text` only, modest measured win |
| Reading a score | [`tfidf` / `tfidfs` / `list_terms`](#tfidf-vs-tfidfs-vs-list-terms) | one term × one document, one term × every document, or every term × one document, ranked |
| Tokenizing a string document | the [process-global tokenizer](#the-process-global-tokenizer-and-stop-word-list) | affects every `TfIdf` in the process, including ones built earlier |

### `TfIdf::new` vs `TfIdf::from_json`

Nothing here materialises a `terms × documents` matrix. A document built through
`add_document` is a `Vec<(TermId, f64)>` plus a hash index — `BuiltDocument` —
and terms are interned to a `u32` `TermId` **once per corpus**, so a term repeated
across fifty documents costs one allocation, not fifty.

`TfIdf::from_json` cannot use that path: a deserialized document can hold values a
count table has no room for — a string, a literal `0`, a negative number — so a
restored corpus keeps each document as a `RawDocument`, the original `JsonValue`
indexed by an `FxHashMap<Box<str>, u32>` so property lookup stays O(1).

The representation is decided once, at deserialization, not per document. The two
paths disagree about exactly one thing — the cost of an `idf` cache miss — and
never about the answer:

| Corpus | Cache-miss cost | Measured (`idf_cold`, `benches/tfidf.rs`) |
|---|---|---|
| Built via `TfIdf::new` + `add_document` | O(1) — an incremental document-frequency table | flat at ~18 ns from 1 document to 256 |
| Restored via `TfIdf::from_json` | O(documents) — every document is scanned | 24 ns at 1 document, 2.6 µs at 256 |

`built.idf(t)` and `TfIdf::from_json(&built.to_json()).idf(t)` agree for every
term: `from_json` changes how the answer is found, not what it is.

### The three shapes of `add_document`

| Variant | Lowercased | Stop-word filtered | Tokenizer used | Can it match a query? |
|---|:--:|:--:|---|:--:|
| `DocumentInput::Text(&str)` | ✅ | ✅ | the process-global tokenizer | ✅ |
| `DocumentInput::Tokens(&[&str])` | ❌ | ❌ | none — used verbatim | ✅ (exact strings only) |
| `DocumentInput::Raw(JsonValue)` | — | — | none | ❌ — never matches, but still counts toward every `idf`'s denominator |

`Raw` covers a document slot holding something other than a string or an array of
tokens — an object, a number, even `null`. The value is stored **unchanged**, and
a raw document gets no `__key`, so a key assigned at `add_document` time never
matches it on removal; only `DocKey::Undefined` does.

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let mut from_text = TfIdf::new();
    from_text
        .add_document(DocumentInput::Text("The The THE rust"), DocKey::Undefined, false)
        .unwrap();

    let mut from_tokens = TfIdf::new();
    from_tokens
        .add_document(
            DocumentInput::Tokens(&["The", "The", "THE", "rust"]),
            DocKey::Undefined,
            false,
        )
        .unwrap();

    // Text: lowercased, and "the" is a default stop word, so it never lands in
    // the document at all.
    assert_eq!(from_text.tf_at("the", 0).unwrap().to_number(), 0.0);
    assert_eq!(from_text.tf_at("rust", 0).unwrap().to_number(), 1.0);

    // Tokens: used exactly as given. Three different terms, none filtered.
    assert_eq!(from_tokens.tf_at("The", 0).unwrap().to_number(), 2.0);
    assert_eq!(from_tokens.tf_at("THE", 0).unwrap().to_number(), 1.0);
    assert_eq!(from_tokens.tf_at("the", 0).unwrap().to_number(), 0.0);
}
```

### The idf cache and `restore_cache`

Every `idf` result is cached by term. `add_document` and `remove_document` take a
`restore_cache: bool` deciding what happens to that cache after the corpus
changes:

- **`false`** (the common case) — the cache is discarded and replaced with an
  empty one. Every term becomes cold; the next read of each recomputes it lazily.
- **`true`** — nothing is discarded. Every term **currently in the cache** is
  recomputed immediately, in place, in the cache's own key order.

Choose `true` when every cached value must be correct immediately, or when the
corpus holds `RawDocument`s (from `from_json`), where a miss is O(documents) and
refreshing eagerly front-loads real work. On the built-document fast path a miss
is already O(1), so eagerly refreshing N cached terms costs roughly what N lazy
misses would have anyway.

`restore_cache: false` also **replaces the cache's identity** — see
[The idf cache's two forms](#the-idf-cache-s-two-forms).

### Adding many documents at once

Behind the `parallel` Cargo feature,
`TfIdf::par_add_documents_batch(&mut self, documents: &[(&str, DocKey)]) ->
Result<(), TfIdfError>` adds many **text** documents in one call. Only the
tokenizing is parallel: lowercasing and tokenizing a text document is a pure
function of that document's own text, so it fans out across threads, and then
interning, stop-word filtering and counting replay in the original order through
the same sequential primitives `add_document` uses. That replay makes the method
exactly equivalent to calling
`add_document(DocumentInput::Text(text), key, false)` once per pair, verified by
the sequential-vs-parallel suite in `tests/parallel.rs`.

**Scope.** `DocumentInput::Text` only — `Tokens` has no tokenizing to parallelize
and `Raw` does no processing at all; add those with sequential `add_document`.
There is no `restore_cache` parameter: the method always behaves as `false`,
discarding the cache once after the whole batch.

**Measured**, `benches/tfidf.rs`'s `parallel_batch` group (`--features parallel`),
on one 32-core machine:

| Shape | N | Sequential | `par_add_documents_batch` | Δ |
|---|---:|---:|---:|---:|
| `few_large` (≈167 kB docs) | 8 | 18.4 ms | 17.5 ms | ~5% faster |
| `few_large` | 64 | 141.1 ms | 130.9 ms | ~7% faster |
| `few_large` | 256 | 560.5 ms | 514.4 ms | ~8% faster |
| `many_small` (≈1.2 kB docs) | 128 | 3.46 ms | 3.69 ms | ~7% **slower** |
| `many_small` | 1,024 | 25.6 ms | 23.5 ms | ~8% faster |
| `many_small` | 8,192 | 211.6 ms | 183.2 ms | ~13% faster |

The interner and index lookups left sequential are a real share of ingestion
time, which caps the win at 5–13%. Below roughly a thousand small documents (or a
few dozen large ones) the fork-join cost is not amortized and the sequential loop
can win outright. Reach for this when ingesting a genuinely large batch in one
call; otherwise `add_document` in a loop is simpler and at least as fast.

```rust  ignore
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};

fn main() {
    let docs = [
        ("this document is about rust.", DocKey::Num(0.0)),
        ("this document is about python.", DocKey::Num(1.0)),
    ];

    let mut parallel = TfIdf::new();
    parallel.par_add_documents_batch(&docs).unwrap();

    let mut sequential = TfIdf::new();
    for (text, key) in docs {
        sequential
            .add_document(DocumentInput::Text(text), key, false)
            .unwrap();
    }

    // Same corpus either way.
    assert_eq!(parallel.to_json(), sequential.to_json());
}
```

### `tfidf` vs `tfidfs` vs `list_terms`

| API | Answers | Shape | Terms come from |
|---|---|---|---|
| `tfidf(terms, d)` | one score | `f64` | your query, against document `d` |
| `tfidfs(terms)` | one score per document | `Vec<f64>`, index = document order | your query, against every document |
| `tfidfs_with(terms, cb)` | same, plus a callback | `cb(index, score, key)` per document, synchronously | your query, against every document |
| `list_terms(d)` | every term of one document, ranked | `Vec<TermScore>`, descending by score | document `d`'s own keys |

`tfidf` and `tfidfs` take a query you supply — `Terms::Text` or `Terms::Tokens`,
the same string-vs-array distinction as `add_document`, **except that
`Terms::Text` is never stop-word filtered**: querying `"the"` computes and caches
an idf for a term a string-built document could never contain. `list_terms` takes
no query; its terms are the document's own, in
[enumeration order](#enumeration-order-in-list-terms), before being sorted by
score. On the four-document corpus from the [quick example](#quick-example),
`tfidf(Terms::Text("document"), 3)` is `0.776_856_448_685_790_3`, `tfidfs` of the
same term is that value four times over (it appears in all four documents with
the same tf), and `list_terms(3)` ranks `["rust", "examples", "document"]`.

## The process-global tokenizer and stop-word list

<a class="badge badge-global" href="../performance/parallelism">GLOBAL STATE</a>

`verbora-tfidf` keeps its tokenizer and stop-word list in **process-global** state,
not per-instance state. `TfIdf::set_tokenizer` and `TfIdf::set_stopwords` read as
instance methods but write to that shared state, so calling either on *one*
`TfIdf` changes how *every* `TfIdf` in the process tokenizes its next string
document — including instances created before the call. This is the same
`RwLock` + `AtomicBool` shape [Ngrams](./ngrams) explains in detail, with two
specifics:

- **Its own statics.** `verbora_tfidf::globals` owns a private
  `RwLock<Option<Arc<dyn TfIdfTokenizer>>>` and a private
  `RwLock<Option<Arc<StopwordSet>>>`, independent of any other crate's slot.
- **No explicit-argument variant.** There is no `..._with` sibling for
  `add_document`, `tfidf` or `tfidfs`. The escape hatch is
  `DocumentInput::Tokens` / `Terms::Tokens`: tokenize outside the crate however
  you like, hand over the `&[&str]`, and no global is read.

The stop-word list has one extra layer. Until `TfIdf::set_stopwords` is called,
`is_stopword` **aliases** `verbora_core::stopwords`'s process-global list — the
same one every stemmer's `add_global_stopword` mutates and
[Phonetics](./phonetics) reads. Only after `set_stopwords` does this crate switch
to its own independent list.

```rust
use verbora_tfidf::{DocKey, DocumentInput, TfIdf};
use std::sync::Arc;

fn main() {
    let mut a = TfIdf::new();
    a.add_document(DocumentInput::Text("this isn't rust"), DocKey::string("a"), false)
        .unwrap();
    assert_eq!(a.list_terms(0).unwrap()[0].term, "isn");

    // Installed through a DIFFERENT call, after `a` already exists.
    TfIdf::set_tokenizer(Arc::new(verbora_tokenizers::TreebankWordTokenizer::new()));

    // The OLD instance `a` is affected by a global installed after it was built.
    a.add_document(DocumentInput::Text("this isn't rust"), DocKey::string("b"), false)
        .unwrap();
    assert_eq!(a.list_terms(1).unwrap()[0].term, "n't");

    verbora_tfidf::globals::reset_tokenizer();
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> If you call <code>TfIdf::set_tokenizer</code> or
<code>TfIdf::set_stopwords</code> from your own tests, reset them afterwards
(<code>verbora_tfidf::globals::reset_tokenizer</code> /
<code>reset_stopwords</code>) or isolate those tests behind a mutex, the way the
crate's own tests do. Rust test binaries run on multiple threads in one process
by default, so an unguarded test can observe another one's tokenizer mid-flight.
</div>

## Concurrency

`TfIdf`, `Document`, `DocKey` and `DynValue` are all `Send + Sync`, but that buys
little: every mutating method takes `&mut self`, so a `TfIdf` is not a type you
share behind an `Arc` and query concurrently.
[`par_add_documents_batch`](#adding-many-documents-at-once) does not change that —
it is one `&mut self` call from one thread whose *internal* tokenizing phase runs
on more than one thread.

What *is* genuinely process-wide is
[the tokenizer and stop-word globals](#the-process-global-tokenizer-and-stop-word-list):
a thread calling `set_tokenizer` changes what every other thread's `add_document`
does next, with no error to tell you it happened. See
[Parallelism](../performance/parallelism).

## `add_file_sync` and encoded reads

`add_file_sync(path, encoding, key, restore_cache)` reads a file, decodes it using
the named byte encoding, and feeds the resulting text through the same path as
`DocumentInput::Text`. The encoding is not validation; it decides what text gets
tokenized. Reading a UTF-8 file as `base64` does not fail — it tokenizes the
**base64 text of the bytes**. A file containing
`"this document is about rust."` read as `base64` yields the single term
`"dghpcybkb2n1bwvudcbpcybhym91dcbydxn0lg"` — the decoded base64 string,
lowercased and stop-worded like any other string document — while the same file
read as `utf8` yields its own words.

```rust
use verbora_tfidf::Encoding;

fn main() {
    assert!(Encoding::parse("BASE64").is_some()); // case-insensitive
    assert!(Encoding::parse("raw").is_none());    // not an accepted encoding

    assert_eq!(
        Encoding::Base64.decode(b"this document is about rust."),
        "dGhpcyBkb2N1bWVudCBpcyBhYm91dCBydXN0Lg=="
    );
    assert_eq!(Encoding::Hex.decode(b"rust"), "72757374");
}
```

`Encoding::parse` accepts exactly this set, case-insensitively: `utf8`/`utf-8`,
`ascii`, `latin1`/`binary`, `base64`, `base64url`, `hex`, and
`ucs2`/`ucs-2`/`utf16le`/`utf-16le`. `encoding: None` (and `Some("")`) means
`utf8`. Anything else — including `raw` and `buffer` — returns `None`, and
`add_file_sync` with such a name returns `TfIdfError::InvalidEncoding`. `Utf8`
decoding is lossy (malformed sequences become U+FFFD); `Utf16Le` drops a trailing
odd byte rather than erroring. There is no encoding detection — the caller always
names it.

## Specified edge cases

### The idf cache's two forms

`TfIdf` installs one of two kinds of idf cache, depending on how the instance got
there:

| Cache form | Installed by | A term named `toString`, `constructor`, `hasOwnProperty`, … |
|---|---|---|
| Prototype-backed | `TfIdf::new()`, `add_file_sync` without `restore_cache` | resolves to the inherited member, returned as `DynValue::Function`/`Prototype` |
| Bare | `add_document`, `remove_document` | is an ordinary term, scored numerically |

The cache probe is a **truthiness** test rather than an existence check, which is
why the prototype-backed form answers with an inherited member as if it had been
cached. `idf_cache_is_prototype_backed` reports which form is installed; `idf`
coerces every variant through `to_number()`, while `idf_value` shows what actually
happened.

```rust
use verbora_tfidf::{DocKey, DocumentInput, DynValue, TfIdf};

fn main() {
    // A fresh TfIdf::new() starts prototype-backed.
    let mut t = TfIdf::new();
    assert!(t.idf_cache_is_prototype_backed());
    assert!(matches!(
        t.idf_value("toString", false).unwrap(),
        DynValue::Function("toString")
    ));

    // The FIRST add_document call (without restore_cache) swaps the cache's
    // identity to the bare form — from here on, "toString" is just a term.
    t.add_document(DocumentInput::Text("x"), DocKey::Undefined, false)
        .unwrap();
    assert!(!t.idf_cache_is_prototype_backed());
    assert!(matches!(t.idf_value("toString", false).unwrap(), DynValue::Num(_)));
}
```

### Reserved terms: `__proto__` and `__key`

Because a built document's accumulator is conceptually `{ __key: key }`, two token
spellings are not ordinary terms:

- **`__proto__`.** Assigning a number to this property is ignored, so a token
  spelled this way is never stored.
- **`__key`.** A token spelled this way is folded into the document's own key: a
  string key gets `1` concatenated onto it as text, each time it occurs; an absent
  key starts at `DocKey::Num(1.0)` and increments numerically.

A third case follows from the cache forms above: a term spelling one of the
inherited member names shadows that member on first use, so `BuiltDocument::observe`
resets the slot to a literal `0` before counting. That reset runs unconditionally,
ahead of the stop-word test — which is why a stop-worded `toString` leaves a
zero-valued entry rather than no entry at all.

Concretely: `"__proto__ __proto__ alpha"` stores only `alpha`, and
`"__key __key alpha"` added under `DocKey::string("mykey")` leaves the document's
key as `"mykey11"` — string concatenation, not arithmetic.

### Enumeration order in `list_terms`

A document's terms are visited in a fixed enumeration order: every key that is the
canonical decimal spelling of an integer in `0..=2^32-2` — an *array index*,
stricter than "parses as a number", so `"01"`, `"1.0"`, `"-1"` and `"1e3"` all
fail the test — is hoisted to the front, sorted ascending numerically, ahead of
every other key, which keeps insertion order. The default `WordTokenizer` keeps
digit runs as terms, so a real corpus hits this constantly.

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
    // "10" and "2020" hoist ahead of the words, in ASCENDING NUMERIC order —
    // not the order they appeared in the text.
    assert_eq!(names, ["10", "2020", "zeta", "alpha", "beta"]);
}
```

`list_terms`' descending sort by score is stable, so when scores tie this
enumeration order is exactly what survives.

### Bit-exact `idf` arithmetic

The numeric content of `idf` is one line:

```text
idf = 1 + log(total_documents / (1 + docs_with_term))
```

Two hazards compound in it, and `TfIdf::idf_value` accounts for both:

1. **The division happens inside the logarithm.** The algebraically equal
   `ln(n) - ln(1 + d)` differs in the last bit for some inputs, so the computed
   form is `1.0 + math_log(total / (1.0 + docs_with_term))`.
2. **The platform's `log` is not reproducible across platforms.** `math_log` is
   this crate's own bit-exact natural logarithm, built from bit-pattern constants
   and a fixed parenthesisation. It disagrees with `f64::ln` by one ULP on a
   measured 5.6% of a 3,418-input fixture — including `log(3)`, exactly what a
   three-document corpus with one match produces.

`tfidf`'s accumulation is equally exact: it sums `tf * idf` **strictly left to
right** over the query's term list. Only `+Infinity` is clamped to `0`;
`-Infinity` — a real value, produced by `idf` over an **empty** corpus — and `NaN`
pass through unclamped.

```rust
fn main() {
    let a = verbora_tfidf::math_log(3.0);
    let b = 3.0_f64.ln();
    assert_ne!(a.to_bits(), b.to_bits());
    assert_eq!(a.to_bits(), 1.098_612_288_668_109_6_f64.to_bits());
}
```

### Ranking stays deterministic even with `NaN` scores

`list_terms` finishes by sorting descending. That stops being an ordinary sort the
moment one score is `NaN` — a deserialized corpus with a non-numeric tf, or a
`toString`-shadowing term read through a prototype-backed cache, both produce one.
Comparing against `NaN` always reports "not less than", i.e. a **tie**, so the
induced relation is no longer transitive and the result depends on the exact
sorting algorithm rather than on stability in general.

`list_terms` therefore uses one fully specified algorithm: a TimSort with natural-
run detection, binary insertion sort below a run length of 8, galloping merges,
and the extra guards an inconsistent comparator requires. Two runs over the same
corpus produce the same order every time, `NaN` scores included, and no input can
make the sort loop or panic.

## Performance characteristics

`crates/verbora-tfidf/benches/tfidf.rs` is a Criterion suite with five groups —
`build`, `idf_cold`, `query`, `math_log`, `documents` — run against a 167 kB real
document, falling back to a synthetic repeat when that file is absent.

- **Interning** costs nothing for a single document (2.78 ms either way) and pays
  off across a corpus: eight documents through one interning `TfIdf` cost ~18.1 ms
  against ~21.8 ms for eight independent `HashMap`s, roughly 17% less. Its larger
  payoff is indirect — `u32` `TermId` keys are what make the O(1) incremental
  document-frequency table possible.
- **Exactness** is priced too: `math_log` measured ~7.6 µs against ~5.1 µs for
  `f64::ln` over 2,048 realistic ratios — roughly 1.5× for a reproducible
  logarithm.

Reproduce with `cargo bench -p verbora-tfidf`; see
[Benchmarks](../benchmarks/index) for workspace-wide results.

## Allocation behaviour

- **Interning.** `Interner::intern` allocates one `Arc<str>` per **distinct** term
  in the corpus, shared by every document containing it. Query terms passed to
  `tfidf`/`tfidfs`/`idf` are deliberately **not** interned: `Interner::lookup`
  answers without inserting, so probing with adversarial query terms cannot grow
  the table.
- **A built document.** One `Vec<(TermId, f64)>` in insertion order plus one
  `FxHashMap<TermId, u32>` index. `observe` allocates per **distinct** term, never
  per occurrence.
- **A raw document.** One clone of the parsed `JsonValue`, plus one
  `FxHashMap<Box<str>, u32>` own-property index built once at construction.
- **The idf cache.** One `Vec<(Arc<str>, f64)>` plus an index; `idf_value` only
  allocates when a **new** term is cached.
- **`list_terms`.** One `Vec<TermScore>` sized to the document's term count, one
  `String` clone per term name (so the result can outlive the interner), and one
  scratch `Vec` reused across the sort's merges.
- **Lowercasing.** `DocumentInput::Text` and `Terms::Text` scan first and only
  allocate on finding a non-ASCII byte or an ASCII uppercase letter — prose that is
  already lowercase ASCII borrows the input and allocates nothing. Any non-ASCII
  byte hands the whole string to `str::to_lowercase`, so genuine Unicode special
  cases are never approximated.
- **The default tokenizer path.** `globals::tokenize_global` on the untouched
  default tokenizer borrows tokens directly out of the input `&str`. Only once
  `set_tokenizer` has installed a custom `dyn TfIdfTokenizer` does tokenizing
  allocate one owned `String` per token, because a `dyn` boundary cannot hand back
  a borrow tied to the caller's input.

There is no `_into` variant and no caller-supplied output buffer in this crate.
See [Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Unicode and language notes

**The default tokenizer path has no UTF-16-sensitive indexing of its own.**
`DocumentInput::Text` and `Terms::Text` tokenize with whichever tokenizer is
installed — by default `WordTokenizer`, splitting on `[^A-Za-zА-Яа-я0-9_]+` —
which operates on ordinary UTF-8 `&str` and yields borrowed slices. Every
divergence through this path comes from that tokenizer's word-character class,
already documented on [Tokenizers](./tokenizers):

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

**String-shaped raw documents are
<span class="badge badge-utf16">UTF-16</span>-sensitive.**
`DocumentInput::Raw(JsonValue::Str(..))` stores the string unchanged (reachable
directly, or through any corpus restored via `from_json` whose `documents` array
holds a plain string). Reading a term from such a slot indexes the string **by
UTF-16 code unit**, not by Rust `char` or UTF-8 byte offset — so `"café"` has four
indexable positions, and an astral character occupies two, with a lone index
landing on an unpaired surrogate rendered as U+FFFD.

**Lookup normalisation is full Unicode lowercasing**, not
`str::to_ascii_lowercase`. The ASCII fast path only ever *skips* the allocating
call; it never substitutes a cheaper, wrong answer.

## Common mistakes

- **Assuming `DocumentInput::Tokens` behaves like `Text` minus tokenizing.** It
  also skips lowercasing and stop-word filtering — both, not just one.
  `["The", "the", "THE"]` is three distinct terms.
- **Assuming a cached `idf` stays valid, or that `restore_cache` is free.**
  Without it, every cached idf is dropped the moment you call `add_document` or
  `remove_document`; with it, every currently cached term is recomputed right
  then — worth it when you need those values immediately, wasted otherwise.
- **Treating `list_terms`'s order as arbitrary.** Ties are broken by the
  document's own enumeration order through a fully specified sort; two runs over
  the same corpus produce the same order every time.
- **Expecting a `Raw` document's inner properties to be terms.** A raw object's
  own `"text"` property is not a term named `"text"`; the slot never matches, but
  it still counts toward every `idf`'s denominator.

## Related

- [Ngrams](./ngrams) — the process-global tokenizer pattern this page's globals
  section builds on, explained once there.
- [Core traits](./core) — `Tokenizer`, `verbora_core::stopwords`, and the shared
  vocabulary the rest of the site uses.
- [Parallelism](../performance/parallelism) — every built-in `par_*` API across
  the workspace, and the correctness hazard of the globals under concurrent use.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy).
- [Choosing an API](../choosing/index), [Benchmarks](../benchmarks/index),
  [Recipes](../recipes/index).

## API reference

```rust ignore
// verbora_tfidf (crate root re-exports)
pub use document::{BuiltDocument, DocKey, Document, Interner, RawDocument, TermId};
pub use encoding::Encoding;
pub use globals::{StopwordElement, StopwordList, TfIdfTokenizer};
pub use value::{DynValue, JsonValue, Proto};
pub use mathlog::math_log;
pub use tfidf::{DocumentInput, TermScore, Terms, TfIdf, TfIdfError};

pub enum DocumentInput<'a> { Text(&'a str), Tokens(&'a [&'a str]), Raw(JsonValue) }
pub enum Terms<'a> { Text(&'a str), Tokens(&'a [&'a str]) }
pub struct TermScore { pub term: String, pub tf: DynValue, pub idf: DynValue, pub tfidf: f64 }
impl TermScore { pub fn tf_as_f64(&self) -> f64; pub fn idf_as_f64(&self) -> f64; }

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

// document — documents, keys, the term interner
pub type TermId = u32;
pub const KEY_PROPERTY: &str = "__key";
pub const PROTO_PROPERTY: &str = "__proto__";

pub enum DocKey { Undefined, Null, Bool(bool), Num(f64), Str(std::sync::Arc<str>), Object(std::sync::Arc<JsonValue>) }
impl DocKey {
    pub fn string(s: impl AsRef<str>) -> Self;
    pub fn object(value: JsonValue) -> Self;
    pub fn strict_eq(&self, other: &Self) -> bool;   // strict equality, no type coercion
    pub fn is_truthy(&self) -> bool;
    pub fn plus_one(&self) -> Self;                  // __key accumulation
    pub fn as_value(&self) -> DynValue;
}

pub enum Document { Built(BuiltDocument), Raw(RawDocument) }
impl Document {
    pub fn get(&self, term: &str, interner: &Interner) -> Result<DynValue, ReadTarget>;
    pub fn key_value(&self, interner: &Interner) -> Result<DynValue, ReadTarget>;
    pub fn remove_key(&self) -> Result<DocKey, ReadTarget>;
    pub fn for_in_keys(&self, interner: &Interner) -> Vec<String>;
}
pub enum ReadTarget { Undefined, Null }

// Interner, BuiltDocument, RawDocument and Slot are public too; see the rustdoc
// for their constructors and accessors.

// value — the dynamic-value semantics the document model depends on
pub enum JsonValue { Null, Bool(bool), Num(f64), Str(String), Arr(Vec<JsonValue>), Obj(Vec<(String, JsonValue)>) }
pub enum Proto { Object, Array, String, Number, Boolean }
pub enum DynValue {
    Undefined, Null, Bool(bool), Num(f64), Str(std::sync::Arc<str>),
    Function(&'static str), Prototype(Proto), Json(std::sync::Arc<JsonValue>),
}
// Both carry own/is_truthy/to_number/to_text/write_json; DynValue adds
// counts_as_present (`value && value > 0`). Deserialize for JsonValue preserves
// object key order. Free helpers: number_to_string, string_to_number,
// write_json_string, array_index, prototype_member, OBJECT_PROTOTYPE_METHODS.

// mathlog
pub fn math_log(x: f64) -> f64;   // bit-exact, platform-independent natural log

// encoding
pub enum Encoding { Utf8, Ascii, Latin1, Base64, Base64Url, Hex, Utf16Le }
impl Encoding {
    pub fn parse(name: &str) -> Option<Self>;
    pub fn decode(self, bytes: &[u8]) -> String;
}

// globals — the two process-global slots
pub trait TfIdfTokenizer: Send + Sync + std::fmt::Debug {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);
    fn tokenize(&self, text: &str) -> Vec<String>;   // default: calls tokenize_into
}
pub fn set_tokenizer(tokenizer: std::sync::Arc<dyn TfIdfTokenizer>);
pub fn reset_tokenizer();
pub fn tokenizer_is_default() -> bool;
pub fn tokenize_global(text: &str) -> GlobalTokens<'_>;

pub enum StopwordList { NotAnArray, Array(Vec<StopwordElement>) }
pub enum StopwordElement { Str(String), NotAString }
impl StopwordList { pub fn of<I: IntoIterator<Item = S>, S: Into<String>>(words: I) -> Self; }
pub fn set_stopwords(list: &StopwordList) -> bool;
pub fn reset_stopwords();
pub fn stopwords() -> Option<Vec<String>>;
pub fn is_stopword(term: &str) -> bool;
```

No `unsafe` anywhere in this crate. `TfIdf`, `Document`, `DocKey` and `DynValue`
are `Send + Sync`; the only *shared*, concurrency-relevant state is the
[two process-globals](#the-process-global-tokenizer-and-stop-word-list).
`par_add_documents_batch` is the crate's one parallel entry point, behind the
`parallel` Cargo feature and off by default.
