# TF-IDF

`verbora-tfidf` scores terms against documents. It keeps a growable in-memory
corpus, an incremental document-frequency table over it, and the statistics and
queries built on top of them — `idf`, `tfidf`, `tfidfs`, `rank`, `list_terms`.

TF-IDF is a family of weightings rather than one formula, so this crate states
which member it computes and pins it by test. With `n` documents,
`count(t, d)` occurrences of term `t` in document `d`, and `df(t)` documents
containing `t`:

```text
idf(t)      = 1 + ln(n / (1 + df(t)))
tfidf(q, d) = Σ  count(t, d) · idf(t)          summed left to right
             t∈q
```

Term frequency is the **raw count**, not a normalised frequency: nothing is
divided by document length, because a caller who wants that division can perform
it with `Document::total_terms` and a caller who does not cannot undo it. The
two `1 +` terms are the smoothing — the inner one keeps a term that appears in no
document finite, the outer one keeps a term that appears in *every* document
weighted rather than annihilated.

<div class="callout callout-spec">
<strong>Specification status.</strong> The full stateful surface — corpus
construction, the analyzer, ingestion, removal, every statistic and query, both
published orderings, serialization and the compatibility stamp, and the
empty-corpus case — is documented and test-pinned, with no external data
required. <code>cargo test -p verbora-tfidf --all-features</code> runs
<strong>95</strong> tests (75 unit, 15 contract, 5 parallel-equivalence) and
<strong>10</strong> doctests.
</div>

## When to use it

- **Ranking documents against a query, or ranking a document's own terms**, for a
  corpus that fits comfortably in memory and is built once and queried many
  times.
- **A corpus that grows incrementally.** `add_document` and `remove_document`
  maintain the document-frequency table as they go, so `idf` stays an array load
  as documents come and go. There is no cache, and so no question about when one
  is stale.
- **Ingesting a large batch of text in one call**, via
  [`par_add_documents`](#ingesting-a-batch-in-parallel).
- **Exactly reproducible scores.** The logarithm, the accumulation order and both
  sort orders are specified, so the same corpus ranks the same way on every
  platform and every run.

## When not to use it

- **You want a `terms × documents` matrix, sparse vectors, or cosine similarity
  between documents.** Nothing here builds one: a document is a
  `Vec<(TermId, u32)>`, and the only cross-document quantity is the document
  frequency. Vector search has to be written on top.
- **You want a normalised term frequency.** The `tf` here is the raw count.
  Divide by `Document::total_terms` yourself if you want the ratio.
- **You are counting term frequency for something other than ranking.** For a
  plain bag-of-words histogram, a `HashMap<&str, u32>` over your own tokenizer
  output is simpler.
- **Your corpus outgrows memory.** Everything lives in one `TfIdf` value; there
  is no on-disk index and no mmap path.

## Quick example

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let mut corpus = TfIdf::new();
    for text in [
        "this document is about rust.",
        "this document is about python.",
        "this document is about python and rust.",
        "this document is about rust. it has rust examples",
    ] {
        corpus.add_document(text);
    }

    // "rust" is in three of the four documents: 1 + ln(4 / (1 + 3)) = 1.
    assert_eq!(corpus.document_frequency("rust"), 3);
    assert_eq!(corpus.idf("rust"), Some(1.0));

    // …so its idf is exactly 1, and every score is the raw count.
    assert_eq!(corpus.tfidfs("rust"), [1.0, 0.0, 1.0, 2.0]);

    // Ranked, best first.
    let ranked = corpus.rank("rust");
    assert_eq!((ranked[0].document, ranked[0].score), (3, 2.0));

    // The terms that make one document distinctive.
    let terms = corpus.list_terms(3).unwrap();
    assert_eq!((terms[0].term.as_str(), terms[0].count), ("rust", 2));
}
```

## What a term is

Everything above is defined over *terms*, and a term is whatever the corpus's
`Analyzer` produces. The pipeline is three steps, in order:

1. **Tokenize.** The text is cut into tokens. The default is
   [`WordTokenizer`](./tokenizers.md): the [UAX #29] word segments containing at
   least one scalar that is `Alphabetic` or has `General_Category` in
   `{Nd, Nl, No}`. Tokens are contiguous substrings of the input, in order,
   non-overlapping, never empty.
2. **Fold.** Each token is case-folded according to `CaseFold` — `Lowercase` by
   default, `None` to keep the tokenizer's own spelling.
3. **Filter.** If the analyzer carries a stop-word list, a token whose *folded*
   form is on it is dropped.

What survives step 3 is a term. This crate does not implement word boundaries:
it calls `verbora-tokenizers`, which owns the UAX #29 contract, so there is one
boundary rule in the workspace and no second copy to drift from it. That matters
concretely — `"don't"`, `"3.14"`, `"1,000"` and `"a:b"` are each one token under
the standard, and any hand-rolled "letters and digits" scan splits all four.

**Documents and queries are analyzed identically.** A query for `"The"` against a
lowercasing corpus finds `"the"`; a query for a stop word against a filtered
corpus finds nothing, because the *query* term was dropped too.

**Stop-word filtering is off by default**, because it deletes information: a
corpus built with a filter cannot answer a question about a filtered term and
cannot be persuaded to later. Turning it on is one call, and it is recorded in
every serialized artifact.

```rust
use verbora_core::{StopWordLanguage, StopWords};
use verbora_tfidf::{Analyzer, CaseFold, TfIdf};

fn main() {
    // The default: UAX #29 words, lowercased, nothing filtered.
    let default = Analyzer::new();
    assert_eq!(default.terms("The Quick, brown fox"), ["the", "quick", "brown", "fox"]);

    // Case-sensitive.
    let exact = Analyzer::new().with_case_fold(CaseFold::None);
    assert_eq!(exact.terms("The The THE rust"), ["The", "The", "THE", "rust"]);

    // With a stop-word list.
    let filtered = Analyzer::new()
        .with_stop_words(StopWords::for_language(StopWordLanguage::En));
    assert_eq!(filtered.terms("this document is about rust"), ["document", "rust"]);

    // The analyzer belongs to the corpus and is fixed for its lifetime.
    let corpus = TfIdf::with_analyzer(filtered);
    assert_eq!(corpus.analyzer().case_fold(), CaseFold::Lowercase);
    assert!(corpus.analyzer().stop_words().is_some());
}
```

The analyzer is fixed at construction because changing how a term is derived
halfway through would leave the documents added before the change keyed
differently from the ones added after it, and no query could be right for both.

**Nothing is global.** There is no process-global tokenizer and no process-global
stop-word list, so two corpora in one program cannot change each other's answers
and nothing has to be locked to read one.

[UAX #29]: https://www.unicode.org/reports/tr29/

## Choosing the right API

Three axes, one rule each.

### Text or terms

Every query comes in two forms. The plain one takes text and runs the analyzer
on it; the `_terms` one takes strings that have already been analyzed and looks
them up verbatim.

| Call | Use when |
|---|---|
| `tfidf`, `tfidfs`, `rank` | you have text — a search box, a sentence, a document |
| `tfidf_terms`, `tfidfs_terms` | your terms came from elsewhere: a stemmer, an n-gram builder, or `add_terms` |

Use the text form unless you built the corpus with `add_terms`. The term form
skips the analyzer entirely, so a query it is given must already be spelled the
way the corpus stores it — that is its purpose, and its hazard.

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let mut corpus = TfIdf::new();
    // `add_terms` stores what it is given, with no folding and no filtering.
    corpus.add_terms(["Rust", "Rust", "Python"]);
    corpus.add_terms(["Python"]);

    // The text form folds the query to "rust", which this corpus never stored.
    assert_eq!(corpus.tfidf("Rust", 0), Some(0.0));

    // The term form looks it up exactly as given.
    assert_eq!(corpus.tfidf_terms(["Rust"], 0), Some(2.0));

    assert_eq!(
        corpus.document_terms(0).unwrap().collect::<Vec<_>>(),
        [("Rust", 2), ("Python", 1)]
    );
}
```

### One document or many

Resolving a query costs one analyzer pass plus one term-table lookup per term;
scoring one document costs one hash probe per term.

| Call | Resolves the query | Use when |
|---|---|---|
| `tfidf(query, index)` | once per call | you want one document's score |
| `tfidfs(query)` | once for the whole corpus | you want every score, in corpus order |
| `rank(query)` | once for the whole corpus | you want every score, best first |

`tfidfs` in a loop is not the same as `tfidf` in a loop: it resolves once. Prefer
it whenever you are about to score more than one document. `rank` is `tfidfs`
plus a sort into a total order — it costs the sort and saves you writing one.

### Statistics take terms, never text

`idf`, `document_frequency` and `term_count` match their argument **literally**
against the corpus's stored terms. If what you have is text, run
`analyzer().terms(text)` first. They are the primitives the query APIs are built
from, exposed because reproducing a score by hand is a reasonable thing to want.

```rust
use verbora_tfidf::{TfIdf, natural_log};

fn main() {
    let mut corpus = TfIdf::new();
    corpus.add_document_with_key("rust and python", "doc-1");
    corpus.add_document_with_key("rust and rust", "doc-2");

    // Present in both documents: 1 + ln(2 / (1 + 2)).
    assert_eq!(corpus.idf("rust"), Some(1.0 + natural_log(2.0 / 3.0)));
    assert_eq!(corpus.document_frequency("rust"), 2);

    // `Some(0)` means "the document does not contain it"; `None` means "there
    // is no such document". The two are never collapsed into one number.
    assert_eq!(corpus.term_count(1, "rust"), Some(2));
    assert_eq!(corpus.term_count(1, "perl"), Some(0));
    assert_eq!(corpus.term_count(9, "rust"), None);

    // …and the score is the product, reproduced by hand.
    let idf = 1.0 + natural_log(2.0 / 3.0);
    assert_eq!(corpus.tfidf("rust", 1), Some(2.0 * idf));
}
```

### Ingestion

| Call | Use when | Allocates |
|---|---|---|
| `add_document(text)` | the common case | nothing per term with the default analyzer |
| `add_document_with_key(text, key)` | you need to map a position back to a source | one `String` for the key |
| `add_documents(&[text])` | you have a slice and want the assigned positions | as above; **no** speed advantage over a loop |
| `add_terms(terms)` | your terms came from another pipeline | nothing beyond the corpus growth |
| `par_add_documents(&[text])` | a large batch, `parallel` feature | one `String` per term, plus a `Vec` per document |
| `add_document_from_path(path)` | the document is a UTF-8 file | the file's contents |

`add_document` is the right call for the large majority of programs. Each of them
returns the position the document was given, and `add_documents` returns the
`Range<usize>` of positions.

`add_documents` is exactly equivalent to `add_document` in a loop and carries no
performance advantage over it; it exists so that the sequential and parallel
batch calls read the same, and so that a caller who wants the assigned positions
does not have to collect them by hand.

`add_document_from_path` is a convenience over `std::fs::read_to_string` plus
`add_document`, and no more. **There is no encoding parameter**: this crate
indexes text, and decoding bytes into text is a separate concern with a separate
contract. A file that is not valid UTF-8 is an `std::io::Error` with
`ErrorKind::InvalidData`, not a stream of replacement characters silently indexed
as terms. Decode with a crate that owns that job, then hand the resulting `&str`
to `add_document`.

## Reading the corpus back

| Call | Answers |
|---|---|
| `len()`, `is_empty()` | how many documents |
| `documents()`, `document(index)` | the `Document` values themselves |
| `document_terms(index)` | `(term, count)` pairs in first-occurrence order |
| `find_document(key)` | the position of the first document with that key |
| `remove_document(index)` | removes it and hands it back |
| `distinct_terms()` | how many terms occur in at least one document |
| `list_terms(index)` | every term of one document, scored and ranked |

Keys are opaque: nothing is parsed out of them, nothing requires them to be
unique, and `find_document` returns the first match.

`remove_document` shifts every later document **down one position**, so an index
held across a removal no longer names the document it did. Document frequencies
are updated, so every idf and every score changes to match the smaller corpus.

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let mut corpus = TfIdf::new();
    corpus.add_document_with_key("rust and python", "doc-1");
    corpus.add_document_with_key("rust and rust", "doc-2");

    assert_eq!(corpus.find_document("doc-2"), Some(1));
    assert_eq!(
        corpus.document_terms(0).unwrap().collect::<Vec<_>>(),
        [("rust", 1), ("and", 1), ("python", 1)]
    );

    let removed = corpus.remove_document(0).unwrap();
    assert_eq!(removed.key(), Some("doc-1"));
    assert_eq!(corpus.len(), 1);
    assert_eq!(corpus.document_frequency("rust"), 1);

    // "doc-2" is now at position 0.
    assert_eq!(corpus.find_document("doc-2"), Some(0));
}
```

### `tfidf` vs `tfidfs` vs `rank` vs `list_terms`

| API | Answers | Shape | Terms come from |
|---|---|---|---|
| `tfidf(q, d)` | one score | `Option<f64>` | your query, against document `d` |
| `tfidfs(q)` | one score per document | `Vec<f64>`, index = corpus order | your query, against every document |
| `rank(q)` | every document, best first | `Vec<DocumentScore>` | your query, against every document |
| `list_terms(d)` | every term of one document, ranked | `Option<Vec<TermScore>>` | document `d`'s own terms |

Both orderings are **total**, so they are the same on every run. `rank` sorts by
score descending, breaking ties by corpus position ascending; `list_terms` sorts
by `tfidf` descending, breaking ties by term ascending in Unicode scalar order.
Documents scoring `0.0` are included in a ranking — what a threshold means is the
caller's decision, not this crate's.

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let mut corpus = TfIdf::new();
    corpus.add_document("rust rust python");
    corpus.add_document("python");

    let terms = corpus.list_terms(0).unwrap();
    assert_eq!(terms[0].term, "rust");
    assert_eq!(terms[0].count, 2);
    assert!(terms[0].tfidf > terms[1].tfidf);
    assert_eq!(terms[0].tfidf, f64::from(terms[0].count) * terms[0].idf);

    // A ranking is a total order: score descending, then position ascending.
    let ranked = corpus.rank("python");
    assert_eq!(ranked[0].score, ranked[1].score);
    assert_eq!((ranked[0].document, ranked[1].document), (0, 1));
}
```

## Ingesting a batch in parallel

Behind the `parallel` Cargo feature,
`TfIdf::par_add_documents(&mut self, texts: &[S]) -> Range<usize>` adds many text
documents in one call.

`add_document` cannot simply be fanned out: it takes `&mut self` and every call
mutates shared corpus state — the term table, the document-frequency table, the
document list. Wrapping the corpus in a mutex would compile and would serialize
exactly the work a parallel version exists to speed up. What *is* independent is
the analyzer, and it is the dominant cost of ingesting a document. So ingestion
splits in two:

1. **Parallel** — every text is run through `Analyzer::terms`, independently, on
   however many cores Rayon offers.
2. **Sequential** — the resulting term lists are counted into the corpus in the
   original order, through `add_terms`, the same counting loop `add_document`
   ends with.

Because step 2 replays step 1's output in order through the unmodified sequential
primitive, the result is *identical* to `add_documents` on the same input — same
positions, same counts, same term-id assignment order, same serialized bytes.
`tests/parallel.rs` pins that.

**What it costs.** One `Vec<Vec<String>>` sized to the batch, holding one
`String` per term — the price of getting terms out of a parallel closure and into
the sequential counter. `add_document` allocates neither, because its terms are
borrowed from the text it was given. That is the trade: one allocation per term,
in exchange for running the analyzer on more than one core.

**When to reach for it.** The crossover point is **unmeasured** for the current
ingestion path, and no figure is estimated in place of one. Fork-join has a fixed
cost, so a handful of short documents will be slower this way; a large batch of
substantial documents is what it is for. Measure your own corpus shape before
adopting it.

A panic inside a custom tokenizer propagates out of the offending Rayon worker
under Rayon's own rules — and because that happens in the parallel phase, before
anything is pushed, the corpus is left with none of the batch added.
`add_documents` differs only in how much of the batch survives a panic: it
pushes each document as it finishes, so a panic on the *k*th text leaves the
first *k − 1* already in the corpus. Both leave a corpus that is correct for
continued use — the document being ingested when a panic happens is never
added, every document added before it keeps exactly the counts it had, and
every document added afterwards counts exactly as it would have without the
panic.

```rust ignore
use verbora_tfidf::TfIdf;

fn main() {
    let texts = ["this document is about rust", "this document is about python"];

    let mut parallel = TfIdf::new();
    assert_eq!(parallel.par_add_documents(&texts), 0..2);

    let mut sequential = TfIdf::new();
    sequential.add_documents(&texts);

    // Same corpus either way, down to the bytes.
    assert_eq!(parallel.to_json().unwrap(), sequential.to_json().unwrap());
}
```

## Concurrency

`TfIdf`, `Analyzer` and `Document` are all `Send + Sync`, but that buys little:
every mutating method takes `&mut self`, so a `TfIdf` is not a type you share
behind an `Arc` and ingest into concurrently. Queries take `&self` and hold no
interior mutability, so a finished corpus *can* be shared read-only across
threads with no lock at all — there is no cache to invalidate and no global to
race on.

[`par_add_documents`](#ingesting-a-batch-in-parallel) does not change that: it is
one `&mut self` call from one thread whose *internal* analyzer phase runs on more
than one thread. See [Parallelism](../performance/parallelism.md).

## Persistence is version-locked

Every key a serialized corpus holds is a term, and terms come out of Unicode
tables that move between releases — so a corpus written by one build is not
readable by a build whose word boundaries or case mappings differ.

`to_json` therefore writes a compatibility stamp, and `from_json` refuses any
artifact whose stamp is absent, damaged or foreign. The shape is:

```json
{
  "_verbora": {"schema": 3, "unicode": "17.0.0", "lowercase": "0123456789abcdef"},
  "analyzer": {"case_fold": "lowercase", "stop_words": ["a", "the"]},
  "documents": [
    {"key": "doc-1", "terms": [["rust", 2], ["python", 1]]},
    {"terms": []}
  ]
}
```

Three deliberate choices:

- **Terms are an array of pairs, not an object.** An object would make term order
  a property of the JSON parser and would leave duplicate keys with no defined
  meaning. An array preserves first-occurrence order exactly and makes a
  duplicate detectable, which `RestoreError::DuplicateTerm` reports rather than
  silently resolving.
- **The analyzer travels with the corpus.** The stop-word list decides which
  terms exist at all and the case fold decides how they are spelled, so an
  artifact that did not carry them could not be queried consistently after a
  restore. `from_json` installs the *recorded* analyzer, not the default one.
- **`key` and `stop_words` are omitted when absent** rather than written as
  `null`. Absent and `null` both read back as absent.

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let mut corpus = TfIdf::new();
    corpus.add_document_with_key("rust and python", "doc-1");
    corpus.add_document_with_key("rust and rust", "doc-2");

    let json = corpus.to_json().unwrap();
    let restored = TfIdf::from_json(&json).unwrap();

    assert_eq!(restored.find_document("doc-1"), Some(0));
    assert_eq!(restored.tfidfs("rust"), corpus.tfidfs("rust"));
    assert_eq!(restored.analyzer().case_fold(), corpus.analyzer().case_fold());
}
```

### What the stamp covers

Three build facts, compared for exact equality — none with an ordering, so an
artifact from a *newer* build is refused just as one from an older build is,
because "newer" says nothing about whether the term partition agrees:

| Fact | Why it is in the stamp |
|---|---|
| `SCHEMA` | A Verbora-owned counter, bumped by hand when the serialized shape or the term-derivation pipeline changes in a way that makes an older artifact wrong. It covers a change no external version number would show. |
| The Unicode version | Read from `verbora_tokenizers::unicode_version`. Word boundaries (UAX #29 `Word_Break`) are computed from that crate's own tables, which this version number pins directly. The "contains a letter or digit" filter is pinned to the same version by a different route: `unicode-segmentation` compares its own Unicode version against the toolchain's at compile time and, when they agree, delegates to `std`'s `char::is_alphabetic`/`char::is_numeric` rather than consulting its tables. Either way the property is fixed as of the recorded version, so one number still covers the whole of tokenization. |
| `lowercase_fingerprint()` | A fingerprint of what `str::to_lowercase` actually does in this build. That mapping is `std`'s, so it moves with the Rust toolchain and `Cargo.lock` records nothing about it. |

The lowercase fingerprint needed a fact of its own because the alternative is a
silent mismatch. `str::to_lowercase` reads case-mapping tables that `std`
regenerates whenever a Rust release adopts a newer UCD, and nothing in
`Cargo.lock` names the toolchain that compiled the crate. Upgrading the toolchain
can therefore re-key an entire corpus while leaving a schema-and-dependency stamp
identical. The fingerprint is taken over *behaviour* rather than a version number
for two reasons: `std` publishes no stable version number for those tables, and a
behavioural fingerprint changes exactly when a mapping changes and never merely
because a version string was bumped.

### Telling the failures apart

A corrupt file and an incompatible file need opposite responses — repair or
re-fetch the first, re-index the second — so they are different errors, and
`RestoreError` never collapses them into one.

| Error | Means |
|---|---|
| `RestoreError::Parse` | The bytes are not the JSON this crate writes. Repair or re-fetch the file. |
| `RestoreError::Stamp(StampError::Missing)` | No `_verbora` member. The build behind its terms is unrecorded and unrecoverable, so it cannot be validated. Rebuild from the source documents. |
| `RestoreError::Stamp(StampError::Malformed)` | A `_verbora` member that is not a stamp. This is damage, not a version difference. |
| `RestoreError::Stamp(StampError::Incompatible)` | A well-formed stamp naming a different build. Both stamps are carried so the message can name them. |
| `RestoreError::UnknownCaseFold` | A recorded `case_fold` this build does not know. |
| `RestoreError::ZeroCount` | A term recorded with a count of zero. A term with no occurrences is not a term of the document, and accepting it would make `document_frequency` count a document that does not contain it. |
| `RestoreError::DuplicateTerm` | One document recorded the same term twice. Which count wins would be a property of the reader rather than of the artifact. |

An unstamped artifact is the dangerous one, because it is detectable only as an
*absence*: a well-formed JSON object with no `_verbora` member is
indistinguishable from a hand-written object, from one produced by a different
tool, and from one produced by a build that dropped the stamp. Accepting it would
mean guessing a version, and a wrong guess reproduces exactly the silent mismatch
the stamp exists to prevent — so it is refused, with its own variant so the
message can say "rebuild this artifact" rather than "your file is damaged".

### A custom tokenizer cannot be serialized

`Analyzer::with_tokenizer` installs any `Tokenize` implementation. A corpus built
that way is fully usable and **cannot be written**: the artifact would carry
terms whose derivation it could not describe, because a custom tokenizer is code,
not data, and no stamp can record it. Writing the file anyway would produce
something that *looks* validated and is not, which is the exact failure the stamp
exists to prevent — so `to_json` refuses at write time rather than producing a
lie.

```rust
use std::sync::Arc;
use verbora_tfidf::{Analyzer, ExportError, TfIdf, Tokenize};

#[derive(Debug)]
struct SplitOnWhitespace;

impl Tokenize for SplitOnWhitespace {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split_whitespace().map(str::to_owned));
    }
}

fn main() {
    let analyzer = Analyzer::new().with_tokenizer(Arc::new(SplitOnWhitespace));
    assert_eq!(analyzer.terms("Hello, world!"), ["hello,", "world!"]);

    let mut corpus = TfIdf::with_analyzer(analyzer);
    corpus.add_document("Hello, world!");
    assert_eq!(corpus.tfidf("world!", 0), Some(1.0 + verbora_tfidf::natural_log(1.0 / 2.0)));

    // Usable, but not writable.
    assert!(matches!(corpus.to_json(), Err(ExportError::CustomTokenizer)));
}
```

Serialize such a corpus's source documents instead, or rebuild it under the
default tokenizer.

## Specified edge cases

### No sentinels, no `NaN`, no infinity

Both of the crate's numeric hazards are closed rather than merely documented.

| Situation | Answer |
|---|---|
| empty corpus | `idf` is `None`; every per-document query is `None`; `rank` and `tfidfs` are empty |
| document index out of range | `None` — never a sentinel score |
| term in no document | `df` is `0`, `idf` is `1 + ln n`, `count` is `0`, so it contributes `0.0` |
| document with no terms | scores `0.0` for every query, and still counts towards `n` |
| empty query | `0.0`, exactly |

An out-of-range document is `None`, never a score. An empty corpus has no idf and
says so with `None` rather than returning the `-∞` that `ln(0)` would produce.
And **no score this crate can produce is `NaN` or infinite**: `n >= 1` and
`0 <= df <= n` make the logarithm's argument positive and finite, so `idf` lies
in `[1 + ln(n/(n+1)), 1 + ln n]` — strictly positive, always finite. Each term
contributes at most `u32::MAX · (1 + ln n)`, under `1.6e11` for any corpus that
fits in memory, so reaching `f64::MAX` would take on the order of `1e297` query
terms.

```rust
use verbora_tfidf::TfIdf;

fn main() {
    let empty = TfIdf::new();
    assert_eq!(empty.idf("anything"), None);
    assert_eq!(empty.tfidf("anything", 0), None);
    assert!(empty.tfidfs("anything").is_empty());
    assert!(empty.rank("anything").is_empty());

    // A document whose text yields no terms still occupies a position and
    // still counts towards n.
    let mut corpus = TfIdf::new();
    corpus.add_document("   ...   ");
    assert_eq!(corpus.len(), 1);
    assert!(corpus.documents()[0].is_empty());
    assert_eq!(corpus.documents()[0].total_terms(), 0);
    assert_eq!(corpus.tfidf("anything", 0), Some(0.0));
    assert_eq!(corpus.idf("anything"), Some(1.0));
}
```

### Floating point is part of the contract

- **The logarithm is `natural_log`, not `f64::ln`.** `f64::ln` delegates to the
  platform `libm`, which Rust does not specify, differs between platforms, and
  differs between versions of the same platform's C library. Two builds of the
  same program can therefore disagree in the last bit of every score, for reasons
  the caller cannot see.
- **The sum is accumulated strictly left to right** over the query's terms,
  starting from `0.0`, and counts a term once per occurrence in the query.
  Floating-point addition is not associative, so the order is specified rather
  than left to the implementation.

`natural_log` is Sun Microsystems' `__ieee754_log` from fdlibm, implemented in
safe Rust with no platform inputs at all — only IEEE 754 double arithmetic, which
Rust *does* specify — so the same input yields the same 64 bits on every target.
Its accuracy is fdlibm's documented bound, **strictly under 1 ulp**, which is not
a claim of correct rounding, and the difference is observable at the smallest
input this crate routinely reaches:

```text
ln 3 = 1.09861228866810969139524523692252570464749055782…
  nearest double        1.0986122886681098   (0x3FF193EA7AAD030B)
  natural_log(3.0)      1.0986122886681096   (0x3FF193EA7AAD030A)   1 ulp low
```

A three-document corpus with one match produces exactly `1 + ln 3`, so this is
not a corner case. It is within the documented bound, it is identical on every
platform, and it is pinned by a test that computes the true value independently
from its decimal expansion rather than from anything this crate emits.

`natural_log` is public, so its special values are specified too: `1.0` maps to
`0.0` exactly, `±0.0` to `-∞`, any negative input and `NaN` to `NaN`, and `+∞`
to `+∞`. No score this crate computes can reach any of them — every logarithm it
evaluates has a positive, finite argument — but the function is callable, so the
rows exist.

```rust
use verbora_tfidf::natural_log;

fn main() {
    // Specified to the bit, identically on every platform.
    assert_eq!(natural_log(3.0), 1.0986122886681096);
    assert_eq!(natural_log(3.0).to_bits(), 0x3FF1_93EA_7AAD_030A);

    assert_eq!(natural_log(1.0), 0.0);
    assert_eq!(natural_log(0.0), f64::NEG_INFINITY);
    assert!(natural_log(-1.0).is_nan());
    assert_eq!(natural_log(f64::INFINITY), f64::INFINITY);
}
```

### Limits

Three, all consequences of the integer widths the representation is built on.
None is reachable by a corpus that fits in memory, and each is checked rather
than wrapped, because a wrapped id would alias two different terms and a wrapped
count would be a wrong answer that nothing could detect.

| Limit | What happens at it |
|---|---|
| `u32::MAX - 1` distinct terms in one corpus | ingestion panics (over 34 GiB of term text) |
| `u32::MAX` distinct terms in one document | ingestion panics |
| `MAX_TERM_COUNT` occurrences of one term in one document | the count saturates |

## Performance characteristics

Nothing here builds a `terms × documents` matrix.

- **Terms are interned once per corpus**, so a word appearing in fifty documents
  is stored once and compared as a `u32` id rather than as text.
- **A document is a `Vec<(TermId, u32)>` in first-occurrence order plus a hash
  index** from id to position.
- **The document-frequency table is a `Vec<u32>` maintained incrementally**,
  which makes `idf` an array load behind a term-table probe rather than a scan of
  the corpus — and removes the need for a cache, and with it every question about
  when a cache is stale.
- **Counting during ingestion goes through a dense slot table indexed by term
  id**, not through the document's hash index, so a repeated term costs one array
  load instead of one hash probe. The table is owned by the corpus and restored
  to all-zeroes after each document by walking only the ids that document
  touched, so the reset is O(distinct terms in the document) and never
  O(corpus vocabulary).

**Timings are unmeasured.** No benchmark has been run against the current
implementation of this crate, and no figure is estimated in place of one. The
Criterion suite in `crates/verbora-tfidf/benches/tfidf.rs` has five groups —
`build`, `idf`, `query`, `persistence`, `natural_log` — plus a sixth comparing
`par_add_documents` against the sequential loop when the `parallel` feature is
on. What each group is *for* is stated in that file. The representation facts
above are properties of the implementation and are stated as such; no timing
claim is made, and none should be inferred. See
[Benchmarks](../benchmarks/index.md).

## Allocation behaviour

- **Interning.** One `Box<str>` per **distinct** term in the corpus; every
  document that contains it stores a `u32` id instead of a second copy. Query
  terms passed to `tfidf`/`tfidfs`/`idf` are deliberately **not** interned: the
  lookup answers without inserting, so probing with adversarial query terms
  cannot grow the table.
- **A document.** One `Vec<(TermId, u32)>` in first-occurrence order plus one
  `FxHashMap<TermId, u32>` index. Ingestion allocates per **distinct** term,
  never per occurrence.
- **Ingest scratch is reused.** The dense counting table and the term buffer live
  on the corpus and are reused document to document, not reallocated per call.
- **Case folding.** The analyzer scans a token first and only allocates on
  finding a non-ASCII byte or an ASCII uppercase letter — prose that is already
  lowercase ASCII borrows and allocates nothing. Any non-ASCII byte hands the
  whole token to `str::to_lowercase`, so genuine Unicode special cases are never
  approximated.
- **The default tokenizer path borrows.** `WordTokenizer` yields tokens borrowed
  directly out of the input `&str`. Only once `Analyzer::with_tokenizer` has
  installed a `dyn Tokenize` does tokenizing allocate one owned `String` per
  token, because a `dyn` boundary cannot hand back a borrow tied to the caller's
  input.
- **`Analyzer::terms`** is the convenience form and allocates one `String` per
  term; the ingest path does not, which is why it is a convenience rather than
  the primitive.
- **`list_terms`.** One `Vec<TermScore>` sized to the document's term count, plus
  one `String` per term name so the result can outlive the corpus's term table.

There is no `_into` variant and no caller-supplied output buffer in this crate.
See [Allocation](../performance/allocation.md) and
[Zero-copy](../performance/zero-copy.md).

## Unicode and language notes

Segmentation runs on the text **as given**; folding runs afterwards, per token.
Folding therefore cannot move a boundary, and every term is the folded image of a
substring of the input. Every divergence you will see through the default path is
`WordTokenizer`'s segmentation, documented on [Tokenizers](./tokenizers.md):

```rust
use verbora_tfidf::Analyzer;

fn main() {
    let analyzer = Analyzer::new();

    // Accented letters are word characters; nothing is stripped.
    assert_eq!(analyzer.terms("naïve café crème brûlée"), ["naïve", "café", "crème", "brûlée"]);

    // Lowercasing 'İ' expands it to 'i' plus a combining dot above.
    assert_eq!(analyzer.terms("İstanbul"), ["i\u{307}stanbul"]);

    // UAX #29 does not segment scripts written without spaces: one token per
    // Han scalar.
    assert_eq!(analyzer.terms("日本語 test"), ["日", "本", "語", "test"]);

    // An astral scalar is not a word, and is not replaced by anything.
    assert_eq!(analyzer.terms("😀abc😀"), ["abc"]);

    // Interior punctuation follows the standard, not a character class.
    assert_eq!(
        analyzer.terms("well-known and/or 3.14 snake_case"),
        ["well", "known", "and", "or", "3.14", "snake_case"]
    );
}
```

**Folding is full Unicode lowercasing**, not `str::to_ascii_lowercase`. The ASCII
fast path only ever *skips* the allocating call; it never substitutes a cheaper,
wrong answer. That includes the mappings that change a token's length — `İ`
becoming two scalars above — and the context-sensitive Greek final sigma rule,
which is why the compatibility stamp fingerprints the mapping's behaviour rather
than trusting a version number.

## Common mistakes

- **Assuming `tf` is a frequency.** It is the raw count. Divide by
  `Document::total_terms` if you want the ratio — and note that `total_terms`
  counts tokens that survived the analyzer, not tokens in the source text.
- **Querying an `add_terms` corpus with the text APIs.** `add_terms` stores
  exactly what it is given, so a corpus holding `"Rust"` is unreachable from
  `tfidf("Rust", …)` on a lowercasing corpus. Use `tfidf_terms`.
- **Passing raw text to `idf`, `document_frequency` or `term_count`.** They match
  literally against stored terms. Run `analyzer().terms(text)` first.
- **Confusing `Some(0)` with `None`.** `term_count` returns `Some(0)` for "the
  document does not contain it" and `None` for "there is no such document";
  `tfidf` returns `None` only for an out-of-range index, never as a score.
- **Calling `tfidf` in a loop over the corpus.** It resolves the query once per
  call. `tfidfs` resolves once for the whole corpus, and `rank` sorts the result
  for you.
- **Holding a document index across `remove_document`.** Every later document
  shifts down one position.
- **Expecting a stop-word-filtered corpus to answer about a filtered term.** The
  filter deleted the information at ingest time; the query term is dropped too,
  so the answer is `0.0` rather than an error.
- **Expecting `to_json` to work with a custom tokenizer.** It refuses, on
  purpose. Serialize the source documents instead.

## Related

- [Tokenizers](./tokenizers.md) — the default analyzer's tokenizer, and what it
  cuts at.
- [Core traits](./core.md) — `Tokenizer`, `StopWords`, and the shared vocabulary
  the rest of the site uses.
- [Parallelism](../performance/parallelism.md) — every built-in `par_*` API
  across the workspace.
- [Allocation](../performance/allocation.md) and
  [Zero-copy](../performance/zero-copy.md).
- [Choosing an API](../choosing/index.md), [Benchmarks](../benchmarks/index.md),
  [Recipes](../recipes/index.md).

## API reference

```rust ignore
// verbora_tfidf (crate root re-exports)
pub use analyzer::{Analyzer, CaseFold, Tokenize};
pub use corpus::{DocumentScore, TermScore, TfIdf};
pub use document::{Document, MAX_TERM_COUNT};
pub use log::natural_log;
pub use persist::{ExportError, RestoreError};
pub use stamp::{
    ArtifactStamp, CONTEXT_PROBES, SCHEMA, STAMP_PROPERTY, StampError, lowercase_fingerprint,
};

pub trait Tokenize: std::fmt::Debug + Send + Sync {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);   // appends
}
// Blanket: every `verbora_core::Tokenizer` that is Debug + Send + Sync is one.

pub enum CaseFold { Lowercase, None }

pub struct Analyzer { /* private */ }
impl Analyzer {
    pub fn new() -> Self;                                    // WordTokenizer, Lowercase, no stop words
    pub fn with_case_fold(self, fold: CaseFold) -> Self;
    pub fn with_stop_words(self, words: verbora_core::StopWords) -> Self;
    pub fn without_stop_words(self) -> Self;
    pub fn with_tokenizer(self, tokenizer: std::sync::Arc<dyn Tokenize>) -> Self;

    pub fn case_fold(&self) -> CaseFold;
    pub fn stop_words(&self) -> Option<&verbora_core::StopWords>;
    pub fn uses_default_tokenizer(&self) -> bool;            // serialization requires this
    pub fn terms(&self, text: &str) -> Vec<String>;
}

pub struct TermScore { pub term: String, pub count: u32, pub idf: f64, pub tfidf: f64 }
pub struct DocumentScore { pub document: usize, pub score: f64 }

impl TfIdf {
    // Construction
    pub fn new() -> Self;
    pub fn with_analyzer(analyzer: Analyzer) -> Self;
    pub fn analyzer(&self) -> &Analyzer;

    // Ingestion — each returns the position(s) assigned
    pub fn add_document(&mut self, text: &str) -> usize;
    pub fn add_document_with_key(&mut self, text: &str, key: impl Into<String>) -> usize;
    pub fn add_terms<I, S>(&mut self, terms: I) -> usize
    where I: IntoIterator<Item = S>, S: AsRef<str>;
    pub fn add_terms_with_key<I, S>(&mut self, terms: I, key: impl Into<String>) -> usize
    where I: IntoIterator<Item = S>, S: AsRef<str>;
    pub fn add_documents<S: AsRef<str>>(&mut self, texts: &[S]) -> std::ops::Range<usize>;
    pub fn add_document_from_path(&mut self, path: impl AsRef<std::path::Path>)
        -> std::io::Result<usize>;
    // requires the `parallel` Cargo feature
    pub fn par_add_documents<S: AsRef<str> + Sync>(&mut self, texts: &[S])
        -> std::ops::Range<usize>;

    // The corpus
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn documents(&self) -> &[Document];
    pub fn document(&self, index: usize) -> Option<&Document>;
    pub fn document_terms(&self, index: usize) -> Option<impl Iterator<Item = (&str, u32)>>;
    pub fn find_document(&self, key: &str) -> Option<usize>;
    pub fn remove_document(&mut self, index: usize) -> Option<Document>;

    // Statistics — arguments are matched literally against stored terms
    pub fn distinct_terms(&self) -> usize;
    pub fn document_frequency(&self, term: &str) -> u32;
    pub fn term_count(&self, index: usize, term: &str) -> Option<u32>;
    pub fn idf(&self, term: &str) -> Option<f64>;            // None on an empty corpus

    // Queries
    pub fn tfidf(&self, query: &str, index: usize) -> Option<f64>;
    pub fn tfidf_terms<I, S>(&self, terms: I, index: usize) -> Option<f64>
    where I: IntoIterator<Item = S>, S: AsRef<str>;
    pub fn tfidfs(&self, query: &str) -> Vec<f64>;
    pub fn tfidfs_terms<I, S>(&self, terms: I) -> Vec<f64>
    where I: IntoIterator<Item = S>, S: AsRef<str>;
    pub fn rank(&self, query: &str) -> Vec<DocumentScore>;
    pub fn list_terms(&self, index: usize) -> Option<Vec<TermScore>>;

    // Persistence
    pub fn to_json(&self) -> Result<String, ExportError>;
    pub fn from_json(json: &str) -> Result<Self, RestoreError>;
}

pub struct Document { /* private */ }
impl Document {
    pub fn key(&self) -> Option<&str>;
    pub fn distinct_terms(&self) -> usize;
    pub fn total_terms(&self) -> u64;                        // occurrences, after the analyzer
    pub fn is_empty(&self) -> bool;
}
pub const MAX_TERM_COUNT: u32 = u32::MAX;                    // counts saturate here

pub enum ExportError { CustomTokenizer, Json(serde_json::Error) }   // #[non_exhaustive]
pub enum RestoreError {
    Parse(serde_json::Error),
    Stamp(StampError),
    UnknownCaseFold(String),
    ZeroCount { document: usize, term: String },
    DuplicateTerm { document: usize, term: String },
    // #[non_exhaustive]
}

// The compatibility stamp
pub const SCHEMA: u32;
pub const STAMP_PROPERTY: &str;                              // "_verbora"
pub const CONTEXT_PROBES: [&str; 6];                         // Greek final-sigma probes
pub fn lowercase_fingerprint() -> u64;                       // FNV-1a over str::to_lowercase
pub struct ArtifactStamp { pub schema: u32, pub unicode: (u64, u64, u64), pub lowercase: Option<u64> }
impl ArtifactStamp { pub fn current() -> Self; }
pub enum StampError { Missing, Malformed, Incompatible { found: ArtifactStamp, expected: ArtifactStamp } }  // #[non_exhaustive]

// The specified logarithm
pub fn natural_log(x: f64) -> f64;   // fdlibm's __ieee754_log, platform-independent
```

No `unsafe` anywhere in this crate, and no process-global state of any kind.
`TfIdf`, `Analyzer` and `Document` are `Send + Sync`; every mutating method takes
`&mut self` and every query takes `&self`, so a finished corpus shares read-only
across threads without a lock. `par_add_documents` is the crate's one parallel
entry point, behind the `parallel` Cargo feature and off by default.
