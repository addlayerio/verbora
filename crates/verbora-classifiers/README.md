# verbora-classifiers

Three text classifiers that train inside your process: multinomial naive Bayes,
one-vs-rest logistic regression, and a conditional maximum-entropy model. Bayes
and logistic regression learn from *documents* — hand them text and a class
label and they lowercase, segment, stop-word filter and stem it for you. The
maximum-entropy model learns from *contextual predicates you derive yourself*,
which is what makes it usable for tagging, disambiguation and anything else
whose features are not words. No model server, no BLAS, no build script.

## What it guarantees

For Bayes and logistic regression the unit of text is a **token**: a feature key
is the stem of one UAX #29 word token of the lowercased document, never a
character and never a byte. A maximum-entropy predicate is the opposite — an
opaque string that is never trimmed, lowercased, tokenized or stemmed. Scores
are `f64` with a *specified* summation order, computed through in-tree FDLIBM
`log`/`exp`/`pow` rather than the platform `libm`, so a model fitted on one
target scores identically on another. Bayes and logistic regression **can**
return a `NaN` score and will rank it as a tie rather than panic
(`BayesEngine::with_smoothing` deliberately admits a negative constant); the
maximum-entropy model cannot — everything it returns is a probability in
`[0, 1]`, and the scores over one context sum to `1`. Saved models carry a
four-fact compatibility stamp (schema, Unicode version, case mapping, stemmer)
and loading refuses an artifact whose stamp is absent, damaged or foreign,
because a model keyed by one set of word boundaries mispredicts *silently* under
another.

Each model is a published one, so the behaviour is checkable rather than
asserted:

- **Naive Bayes** — multinomial with additive smoothing; Manning, Raghavan &
  Schütze, *Introduction to Information Retrieval*, 2008, §13.2.
- **Logistic regression** — Cox (1958), *The regression analysis of binary
  sequences*; the one-vs-rest reduction is Rifkin & Klautau (2004).
- **Maximum entropy** — the conditional exponential model of Berger, Della
  Pietra & Della Pietra, *A Maximum Entropy Approach to Natural Language
  Processing*, Computational Linguistics 22(1), 1996, fitted by generalised
  iterative scaling — Darroch & Ratcliff, *Generalized iterative scaling for
  log-linear models*, Annals of Mathematical Statistics 43(5), 1972 — in the
  conditional form of Berger et al. §6.1.

## Example

```rust
use verbora_classifiers::{BayesClassifier, Gis, MaxEntClassifier};

// Documents in, label out. Tokenizing, folding and stemming are done for you.
let mut bayes = BayesClassifier::new();
bayes.add_document("my unit-tests failed.", "software");
bayes.add_document("tried the program, but it was buggy.", "software");
bayes.add_document("tomorrow we will do standard tests", "other");
bayes.add_document("the drive has a 2TB capacity", "other");
bayes.train().unwrap();

assert_eq!(bayes.classify("did the program crash?").unwrap(), "software");

// Maximum entropy takes predicates you derive — here, word-shape features for
// part-of-speech disambiguation. "saw" is ambiguous; its left context resolves it.
let mut tagger = MaxEntClassifier::new();
tagger.add("DT", ["w=the", "w-1=<s>"]);
tagger.add("NN", ["w=dog", "w-1=the"]);
tagger.add("VB", ["w=saw", "w-1=dog"]);
tagger.add("NN", ["w=saw", "w-1=the"]);
tagger.train_with(Gis::new(500, 1e-9).unwrap()).unwrap();

assert_eq!(tagger.classify(["w=saw", "w-1=dog"]).unwrap(), "VB");
// The whole distribution is available, so a near-tie is visible.
let scores = tagger.get_classifications(["w=saw", "w-1=the"]).unwrap();
assert_eq!(scores[0].label, "NN");
```

## See also

- Full documentation: <https://verbora.dev/features/classifiers>
- [`verbora-sentiment`](https://crates.io/crates/verbora-sentiment) — polarity
  from a published lexicon, when you have no labelled data to train on.
- [`verbora-tfidf`](https://crates.io/crates/verbora-tfidf) — term weighting and
  ranking, if what you wanted was to score documents against a query.
- [`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) and
  [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers) — the pipeline
  Bayes and logistic regression apply to a document, if you would rather run it
  yourself and feed the result in as tokens.
