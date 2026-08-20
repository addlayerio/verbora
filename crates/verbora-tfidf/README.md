# verbora-tfidf

TF-IDF term weighting over a growable in-memory corpus. Add documents as text or
as pre-analyzed terms, then score one document against a query, score every
document at once, rank them best-first, or ask which terms make a single
document distinctive. Nothing here builds a `terms × documents` matrix: terms
are interned once per corpus, a document is a sparse `Vec<(TermId, u32)>`, and
the document-frequency table is maintained incrementally so `idf` is an array
load rather than a scan.

## What it guarantees

TF-IDF is a family of weightings, not one formula, so this crate states which
member it computes — `idf(t) = 1 + ln(n / (1 + df(t)))`, with term frequency the
**raw count**, summed over the query left to right — and pins it by test. A term
is whatever the corpus's own `Analyzer` produces: UAX #29 word segments
containing a letter or a digit, case-folded, minus the stop-word list. The
analyzer belongs to the corpus, not to the process, so two corpora in one
program cannot change each other's answers.

Floating-point behaviour is part of the contract rather than an accident of it.
**Every score this crate can produce is finite** — no `NaN`, no infinity — and
absence is never a number: an out-of-range document index is `None`, and an
**empty corpus has no inverse document frequency at all**, so `idf` answers
`None` rather than the `-∞` that `ln(0)` would give. A term that appears in *no*
document is a different case and stays finite at `1 + ln(n / 1)`, because the
inner `1 +` is exactly what keeps it so. Logarithms go through `natural_log`,
an in-tree implementation, not `f64::ln`, which is the platform's unspecified
`libm` — so the same corpus scores identically on every target.

Persistence is version-locked. Every key a serialized corpus holds is a term,
and terms come out of Unicode tables that move between releases, so
`TfIdf::to_json` writes a compatibility stamp recording the schema version, the
Unicode version whose word boundaries produced the terms, and a fingerprint of
this build's case fold. `TfIdf::from_json` refuses any artifact whose stamp is
absent, damaged or foreign, and a corpus whose derivation the stamp *cannot*
describe — one built with a custom tokenizer — is refused at write time instead.
The hazard it closes is silent: every number in a mismatched corpus stays
arithmetically valid while the term table no longer describes the documents.

## Example

```rust
use verbora_tfidf::{TfIdf, natural_log};

let mut corpus = TfIdf::new();
corpus.add_document("this document is about node");
corpus.add_document("this document is about ruby");
corpus.add_document("this document is about ruby and node");
corpus.add_document("this document is about node, it has node examples");

// Every score, in corpus order…
assert_eq!(corpus.tfidfs("node"), [1.0, 0.0, 1.0, 2.0]);
// …or ranked, best first.
assert_eq!(corpus.rank("node")[0].document, 3);
// The terms that make one document distinctive.
assert_eq!(corpus.list_terms(3).unwrap()[0].term, "node");

// Absence is `None`, not a sentinel score.
assert_eq!(TfIdf::new().idf("anything"), None);
// A term in no document is finite, not `-inf`.
assert_eq!(corpus.idf("perl"), Some(1.0 + natural_log(4.0)));
```

## See also

- Full documentation: <https://verbora.dev/features/tfidf>
- [`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) — the word
  segmentation this crate's analyzer is built on; use it directly if you want to
  derive terms yourself and ingest them with `add_terms`.
- [`verbora-classifiers`](https://crates.io/crates/verbora-classifiers) — if you
  wanted to *label* documents rather than weight and rank them.
- [`verbora-ngrams`](https://crates.io/crates/verbora-ngrams) — to build the
  multi-word terms this crate will happily weight but does not form for you.
