# Classifiers

`verbora-classifiers` provides three document classifiers built on two unrelated
designs. `BayesClassifier` and `LogisticRegressionClassifier` are thin wrappers
around a naive-Bayes and a one-vs-rest logistic-regression engine, sharing one
generic base, `Classifier<E: Engine>`. `MaxEntClassifier` is a separate subsystem
implementing conditional maximum-entropy classification, fitted by generalised
iterative scaling (GIS) — Darroch & Ratcliff (1972), *Generalized iterative
scaling for log-linear models*, in the conditional form Berger, Della Pietra &
Della Pietra (1996) §6.1 give.

<div class="callout callout-spec">
<strong>Specification status.</strong> Three classifiers over two designs, one
persistence pair per design, and a fixed floating-point evaluation order pinned
by <code>tests/train_parity.rs</code> and <code>tests/predict_parity.rs</code> —
see <a href="#api-reference">API reference</a> below for the full public surface.
<code>cargo test -p verbora-classifiers --all-features</code> runs
<strong>166</strong> tests across the crate's unit tests and its
<code>tests/</code> suite (edge cases, parity, and the maximum-entropy
contract) plus <strong>12</strong> doctests — <strong>178 tests, 0
failures</strong>.
</div>

## When to use it

- **Fast incremental document classification with cheap retraining.**
  `BayesClassifier::train()` only processes documents added since the last call.
- **Feature-based scoring with hand-engineered context predicates.**
  `MaxEntClassifier` learns a weight per `(predicate, outcome)` feature over
  contexts built from predicate strings you supply — useful when a bag-of-words
  model is not expressive enough and you have features like "the previous tag
  was DT".
- **Labelling a large corpus offline** against one already-trained
  `BayesClassifier` or `LogisticRegressionClassifier`, via
  [`par_classify_batch`](#par-classify-batch).

## When not to use it

- **You want state-of-the-art accuracy.** These are three classical algorithms,
  not a gradient-boosted tree or a neural model.
- **You want cheap retraining from `LogisticRegressionClassifier`.** Every
  `train()` call reruns gradient descent from scratch over every stored document.
- **You need document classification independent of the process-global stop-word
  list.** String documents go through the default Porter stemmer, which reads
  that list — see [Process-global stop words](#process-global-stop-words). Pass
  tokens instead of a string to bypass it entirely.

## Quick example

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut classifier = BayesClassifier::new();
    classifier.add_document("my unit-tests failed.", "software");
    classifier.add_document("tried the program, but it was buggy.", "software");
    classifier.add_document("tomorrow we will do standard tests", "other");
    classifier.add_document("the drive has a 2TB capacity", "other");
    classifier.train().unwrap();

    assert_eq!(classifier.classify("the program crashed").unwrap(), "software");
    assert_eq!(classifier.classify("the drive is full").unwrap(), "other");
}
```

Every string is tokenised and stemmed by the default English Porter stemmer,
filtered against the stop-word list, then folded into a growing feature
vocabulary. `train()` hands each new document's 0/1 feature vector to the engine.

Tokenization follows UAX #29 word boundaries, so a feature is whatever
[`WordTokenizer`](./tokenizers.md) yields after stemming: `"my unit-tests
failed."` becomes `["unit", "test", "fail"]` — three features, because
`U+002D HYPHEN-MINUS` is a word boundary and `"my"` is a stop word. When a
query's evidence is split evenly between two classes their scores tie, and
`classify` still returns one label — use
[`get_classifications`](#classify-vs-get-classifications) when you need to see that it was a
tie rather than a decision.

## Which classifier?

| | `BayesClassifier` | `LogisticRegressionClassifier` | `MaxEntClassifier` |
|---|---|---|---|
| Learns | per-class feature counts (Laplace-smoothed) | one-vs-rest weight vectors, intercept discarded | a weight per `(predicate, outcome)` feature |
| Trained by | one incremental pass | gradient descent, **from scratch on every call** | generalised iterative scaling, **from the uniform model on every call** |
| Retraining cost | cheap — only documents past `last_added` | expensive — discards the engine and re-adds every document | you choose when to call `train()`/`train_with()` again; either always refits the whole sample |
| Score meaning | probability-like, not calibrated | sigmoid output in `(0, 1)` | a genuine conditional probability `p(y \| x)` in `[0, 1]`, summing to `1` over a context |
| Shared base | `Classifier<BayesEngine>` | `Classifier<LogisticEngine>` | none — a free-standing type |
| Abstains by | `Err(ClassifierError::NotTrained)` when there are no classes | same | `Err(MaxEntError::NotTrained)` before training; never abstains after — a tie is broken by outcome order |
| Parallel classification | [`par_classify_batch`](#par-classify-batch) | [`par_classify_batch`](#par-classify-batch) | **none yet** — every `par_*` API here needs benchmark evidence, and there is none yet for this model |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>LogisticRegressionClassifier</code> trains by
gradient descent, from scratch on each call. Retraining is <strong>not</strong>
incremental the way <code>BayesClassifier</code>'s is: train twice over two
documents and Bayes trains each document once, while logistic regression trains
all four document-passes. Calling it in a loop as new documents trickle in has
the cost profile of training a fresh classifier every time.
</div>

## The shared `Classifier<E: Engine>` design

`BayesClassifier` and `LogisticRegressionClassifier` are `pub type` aliases over
one generic struct that owns everything they share — the document list, the
feature vocabulary, the stemmer, the `last_added` cursor — and defers only the
numerically interesting part to `Engine`:

```rust ignore
pub struct Classifier<E: Engine> {
    engine: E,
    docs: Vec<Document>,
    features: OrderedMap<f64>,
    stemmer: Arc<dyn Stemmer + Send + Sync>,
    last_added: usize,
    keep_stops: Option<bool>,
}

pub type BayesClassifier = Classifier<BayesEngine>;
pub type LogisticRegressionClassifier = Classifier<LogisticEngine>;

pub trait Engine: Default + Clone {
    const RESETS_ON_TRAIN: bool;
    fn add_example(&mut self, observation: &[u8], label: &str);
    fn fit(&mut self) -> Result<(), ClassifierError>;
    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError>;
    fn to_value(&self) -> DynValue;
    fn from_value(value: &DynValue) -> Self;
}
```

`RESETS_ON_TRAIN` is the entire override distinguishing the two classifiers'
`train()` behaviour — `false` for Bayes (accumulates), `true` for logistic
regression (discards and restarts). The stemmer is held as
`Arc<dyn Stemmer + Send + Sync>`, which is what lets a trained `Classifier<E>` be
shared read-only across threads.

`MaxEntClassifier` does not implement `Engine`. It owns its `Sample` and its
fitted `Option<MaxEntModel>` directly — no shared or interior-mutable state at
all, which is exactly why it is `Send + Sync` — and its `classify`/
`get_classifications` take caller-supplied predicate strings rather than the
`&[u8]` feature vector `Classifier<E>` builds through its own tokenizer. Forcing
it behind `Engine` would mean routing hand-built predicates through that byte
layout, discarding the feature-function flexibility that is the reason to use
MaxEnt at all.

## `train()` vs `retrain()`

Both exist only on `Classifier<E>`. `train()` respects `E::RESETS_ON_TRAIN`;
`retrain()` always rebuilds the engine first, then calls `train()`.

<div class="callout callout-warn">
<strong>Careful — <code>retrain()</code> rebuilds with no arguments.</strong> It
uses <code>E::default()</code>, not "the engine you configured". For
<code>BayesClassifier::with_smoothing</code>, that silently reverts a custom
smoothing constant to <code>1.0</code>.
</div>

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::with_smoothing(0.1);
    c.add_document("my unit-tests failed.", "software");
    c.train().unwrap();
    assert_eq!(c.engine().smoothing(), 0.1);

    c.retrain().unwrap();
    assert_eq!(c.engine().smoothing(), 1.0, "retrain() rebuilds with no arguments");
}
```

`MaxEntClassifier` has neither method. Its two training entry points are
`train()`, which fits with `Gis::default()` settings, and `train_with(settings:
Gis)` for anything else, and both always restart from the uniform model rather
than from parameters already held — training twice over an unchanged sample
gives bit-identical parameters, and training after the sample grew gives a fit
to the sample as it now is, with nothing carried over from the previous run.

## `classify()` vs `get_classifications()`

Both types expose this pair, and they behave identically: `classify()` returns
the single best label; `get_classifications()` returns every outcome's score,
**sorted descending**, with a stable comparator — outcomes tied at exactly the
same score come back in the order the sample first saw them, for both
classifier families.

For `MaxEntClassifier` the scores are genuine probabilities: non-negative, and
summing to `1` over a context.

```rust
use verbora_classifiers::{Gis, MaxEntClassifier};

fn main() {
    let mut classifier = MaxEntClassifier::new();
    for _ in 0..2 {
        classifier.add("x", ["a"]);
    }
    classifier.add("y", ["a"]);
    classifier.add("x", ["b"]);
    for _ in 0..2 {
        classifier.add("y", ["b"]);
    }
    classifier
        .train_with(Gis::new(5_000, 0.0).unwrap())
        .unwrap();

    // Sorted descending: "a" was seen twice with "x" and once with "y".
    let scores = classifier.get_classifications(["a"]).unwrap();
    assert_eq!(scores[0].label, "x");
    assert!((scores[0].value - 2.0 / 3.0).abs() < 1e-8);
    assert!((scores[1].value - 1.0 / 3.0).abs() < 1e-8);
    assert!((scores.iter().map(|c| c.value).sum::<f64>() - 1.0).abs() < 1e-12);

    assert_eq!(classifier.classify(["a"]).unwrap(), "x");
    assert_eq!(classifier.classify(["b"]).unwrap(), "y");
}
```

## `par_classify_batch`

Behind the `parallel` Cargo feature, `Classifier<E>::par_classify_batch(&self,
texts: &[&str]) -> Vec<Result<String, ClassifierError>>` (requiring `E: Sync`)
classifies many independent texts against one already-trained classifier. It is
exactly `texts.par_iter().map(|t| self.classify(*t)).collect()` — a thin fan-out
over the existing sequential `classify`, order-preserving, with each element
carrying its own `Result` so one text's error does not abort the others.

**When to reach for it.** Only when the *batch*, not the single classification,
is the unit of work. A single `classify` costs on the order of 13 µs for a Bayes
classifier trained on a few dozen documents, and a `rayon` task costs about a
microsecond to schedule, so a handful of texts is close to break-even — prefer a
plain `texts.iter().map(...)` loop there. Batches in the thousands amortise the
scheduling cost easily. Reproduce with
`cargo bench -p verbora-classifiers --features parallel -- bayes/predict_batch`.

`get_classifications` and `text_to_features` have no parallel sibling; apply the
same `par_iter().map(...)` pattern at your own call site.

```rust  ignore
let texts = ["the program crashed", "the drive is full"];
let results = classifier.par_classify_batch(&texts);
assert_eq!(results[0].as_deref(), Ok("software"));
assert_eq!(results[1].as_deref(), Ok("other"));
```

<div class="callout callout-warn">
<strong><code>MaxEntClassifier</code> has no <code>par_classify_batch</code>
yet.</strong> It is already <code>Send + Sync</code> — its state is a plain
owned <code>Sample</code> and <code>Option&lt;MaxEntModel&gt;</code>, nothing
shared or interior-mutable — but this workspace ships a <code>par_*</code> API
only backed by sequential-vs-parallel benchmark evidence, and there is none yet
for this model. See <a href="../performance/parallelism">Parallelism</a>.
</div>

## Persistence

Every classifier has the same pair of pairs: `to_json`/`restore` round-trip an
in-memory string with no I/O, and `save`/`load` are thin synchronous wrappers
writing and reading a file.

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    c.add_document("my unit-tests failed.", "software");
    c.add_document("tomorrow we will do standard tests", "other");
    c.train().unwrap();

    // In-memory round trip.
    let json = c.to_json();
    let revived = BayesClassifier::restore(&json).unwrap();
    assert_eq!(revived.to_json(), json);

    // Synchronous file round trip, wrapping the same to_json/restore.
    let path = std::env::temp_dir().join(format!(
        "verbora-classifiers-docs-persist-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    c.save(&path).unwrap();
    let from_disk = BayesClassifier::load(&path).unwrap();
    assert_eq!(from_disk.to_json(), json);
    std::fs::remove_file(&path).ok();
}
```

One difference between the `Classifier<E>` shape and `MaxEntClassifier`'s:

- **JSON shape.** `Classifier<E>::to_json` is compact (`{"a":1,"b":2}`);
  `MaxEntClassifier::to_json` pretty-prints with a 2-space indent, opening with
  the same compatibility stamp every classifier in this crate writes.

<div class="callout callout-note">
<strong><code>MaxEntClassifier::restore</code> restores the fit, not just the
sample.</strong> A file carrying a trained model comes back trained and
classifies identically to the classifier that saved it; a file written before
training comes back untrained, exactly as it was. The training report itself is
not persisted — it describes a run, not a model — so <code>report()</code> is
<code>None</code> until the next <code>train()</code>/<code>train_with()</code>.
</div>

## Behaviour that changes your numbers

### Reproducible transcendental math

Rust's `f64::ln` and `f64::exp` call the platform's libm, which is not specified
to be correctly rounded and disagrees between targets and between versions of one
target. Over 20,000 pseudo-random arguments, two such implementations differed on
981 logarithms (4.9%) and 1,933 exponentials (9.7%) — always by exactly one ULP.

Verbora therefore computes these itself. `log`, `exp`, `pow` and `sigmoid` are
public at the crate root (fixed polynomial coefficients and
`f64::to_bits`/`from_bits`, no `unsafe`), so a model trains and scores
identically everywhere. That matters because a one-ULP difference lands directly
in a Bayes score that is then *sorted* — a near-tie can flip which class wins —
and because logistic regression's descent loop stops when successive costs differ
by less than `1e-4`, so a perturbation can change the *number* of iterations, and
therefore the whole model. `pow` needs no such treatment and delegates to
`f64::powf`: 20,000 random `(base, exponent)` pairs agreed bit-for-bit.

A model is a persisted artifact. If its scores depended on the libm of the
machine that fitted it, it would not be reproducible, and no compatibility stamp
could describe that.

Each algorithm's summation direction is fixed and observable for the same reason
(IEEE-754 addition is not associative): Bayes sums from the highest set feature
index down, logistic regression contracts descending but sums its cost function
ascending, and MaxEnt walks `sample.events()` in insertion order **including
duplicates**.

```rust
use verbora_classifiers::log;

fn main() {
    // A value where the platform libm's `ln` disagrees with this crate's own
    // `log` by exactly one ULP.
    let x = 11.262_564_292_775_972_f64;
    assert_eq!(log(x).to_bits(), 0x4003_5f33_2d5c_29fc);
    assert_ne!(x.ln().to_bits(), log(x).to_bits());
}
```

### Feature slots are stable across vocabulary growth

`Classifier::text_to_features` builds a document's 0/1 vector directly from the
feature vocabulary's insertion order (`OrderedMap`): the first token the
vocabulary ever saw takes slot 0, and a token — integer-like or not — keeps its
slot for as long as it stays in the vocabulary. Adding a new token later,
including one that looks like an integer, appends it after every slot already
handed out.

That is what lets `BayesClassifier::train()` stay incremental. A document
trained early reads back the same slots it was fitted on even after the
vocabulary has grown around it — the vector merely grows a tail of new,
as-yet-untrained slots, which is what `LogisticRegressionClassifier` reports as
`ClassifierError::StaleModel` once a fitted `theta` is narrower than the current
observation:

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    c.add_document(&vec!["alpha".to_owned()], "A");
    c.add_document(&vec!["beta".to_owned()], "B");
    c.train().unwrap();
    assert_eq!(c.classify(&vec!["alpha".to_owned()]).unwrap(), "A");

    // A new document whose only token looks like an integer.
    c.add_document(&vec!["99".to_owned()], "C");
    c.train().unwrap();

    // "99" takes the next free slot; "alpha" and "beta" keep theirs, and the
    // prediction already fitted for "alpha" is unchanged.
    assert_eq!(c.feature_order(), vec!["alpha", "beta", "99"]);
    assert_eq!(c.classify(&vec!["alpha".to_owned()]).unwrap(), "A");
}
```

`Classifier::remove_document` is the one operation that moves slots: it deletes
the matched document's tokens from the vocabulary outright rather than
decrementing them, and closing the resulting gap shifts every later slot down
by one. A model fitted before such a call reads the wrong features afterwards,
and `ClassifierError::StaleModel` is not a safety net here — it only fires for
`LogisticRegressionClassifier`, and only when the resulting width actually
differs from what `theta` was fitted on. `retrain()` is what recovers a
classifier that has had documents removed.

The same insertion-order rule governs class labels: `LogisticEngine::fit`
assembles its per-class target columns from `classifications`, the identical
first-appearance order `get_classifications` reports weights back in, so a
label like `"42"` trains and predicts under the correct class.

### MaxEnt scores are genuine probabilities

`MaxEntModel::distribution` returns `p(y | x) = exp(Σⱼ λⱼ fⱼ(x, y)) / Z(x)` for
every outcome: non-negative, and summing to `1` over a context, up to rounding
(see the worked sum in the [`classify` vs
`get_classifications`](#classify-vs-get-classifications) example above).
Normalisation subtracts the greatest raw score before exponentiating and sums
the exponentials in outcome order, so the computation stays numerically stable
without changing the answer. No maximum-entropy API returns a `NaN`, an
infinity, or an out-of-band value standing in for one: every feature is created
from an observed event, so no ratio the fit computes ever divides by zero, and a
context built entirely from predicates the model never saw is scored by the
uniform distribution rather than by an empty sum.

Sort or threshold on these scores exactly as you would any probability — there
is no unnormalised intermediate value anywhere in the public surface.

## Behaviour worth knowing

- **MaxEnt's mutations are all eager.** `Sample::push`/`Sample::add` append an
  event and update the outcome list synchronously; there is no batch
  constructor. `MaxEntClassifier::train`/`train_with` is single-shot, running to
  convergence or `Gis::max_iterations` with no "advance one iteration" entry
  point, and it rebuilds its feature index from the whole sample every time it
  runs — nothing from a previous fit is reused.
- **Labels are always `String`,** set from the moment `add_document` runs, and
  enumerate in insertion order like every other key.
- **`add_document` requires an explicit label.** Check before calling if you want
  to skip a document conditionally.
- **Bayes smoothing is `f64`-only with a truthy-and-finite guard.**
  `with_smoothing` falls back to `1.0` when passed `0`, `-0.0`, `NaN`, `+Inf` or
  `-Inf`; any other finite value — including `-1.0` — is accepted as-is.

## Common mistakes

- **Calling `LogisticRegressionClassifier::train()` repeatedly, expecting
  `BayesClassifier`-style incremental behaviour.** It resets and reruns gradient
  descent over every stored document on every call.
- **Calling `remove_document` and expecting `train()` alone to fix things up.**
  Removal deletes the matched document's tokens from the vocabulary outright and
  shifts every later feature slot down by one — a model fitted before the call
  can silently read the wrong features afterwards, and a coincidental length
  match will not raise `ClassifierError::StaleModel` to catch it. See
  [Feature slots are stable across vocabulary growth](#feature-slots-are-stable-across-vocabulary-growth).
- **Adding events after `train()`/`train_with()` and forgetting to refit.** The
  classifier keeps answering with the previous fit — nothing is invalidated
  automatically — until you call `train()`/`train_with()` again over the grown
  sample.
- **Passing a token slice and expecting the stemmer or stop-word list to still
  apply.** `Observation::Tokens` is used **verbatim** — no lowercasing, no
  stemming, no stop-word filtering, and `keep_stops` has no effect on it:

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    // "the" and "a" are stop words in string form, so this document tokenises
    // to nothing and is dropped in silence.
    c.add_document("the a", "dropped");
    assert_eq!(c.docs().len(), 0);

    // The exact same words, as a token slice, bypass the stemmer — and
    // therefore the stop-word list — entirely.
    c.add_document(&vec!["the".to_owned(), "a".to_owned()], "kept");
    assert_eq!(c.docs().len(), 1);
}
```

`Observation::Text` is the ergonomic default that works on raw strings;
`Observation::Tokens` trades that convenience for exact control over what a
document's feature list contains. See
[Ergonomics vs throughput](../performance/ergonomics-vs-throughput).

## Performance characteristics

| Operation | Cost | Notes |
|---|---|---|
| `BayesClassifier::add_document` | amortised O(1) per token | one `OrderedMap` insert per distinct token |
| `BayesClassifier::train` | O(new docs × features per doc) | incremental — only documents past `last_added` |
| `Classifier::text_to_features` | O(\|features\| + \|observation\|) | one `OrderedMap::slot_of` hash lookup per token, not a linear scan per feature |
| `LogisticRegressionClassifier::train` | O(iterations × classes × m × n) | bounded by `max_it = 500 × m` per class, typically far fewer at convergence |
| `MaxEntClassifier::train`/`train_with` | O(iterations × Σ over events of that event's active features) | generalised iterative scaling, refit from the uniform model every call |
| `MaxEntModel::distribution` | O(\|context predicates\| × outcomes each is known with) | a linear scan for dedup, since contexts are small — tens of predicates at most |

Every API in this crate is **eager**: no lazy iterator, no `_into`-shaped
buffer-reuse API, and no batch/streaming choice for *training* — `train()` always
processes its entire backlog in one call. Classification is the one exception, via
[`par_classify_batch`](#par-classify-batch).

`benches/classifiers.rs` groups: `bayes/train`, `logistic/train`,
`bayes/predict` (`text_to_features` / `get_classifications` / `classify`),
`bayes/persist`, `maxent/train` (a worked four-event example plus a generated
sample swept at 16/64/256 events), `maxent/predict` (`classify` and
`distribution` over a known vs. an unknown context, and an allocating vs. a
buffer-reusing `distribution` call), and `referenceprimitives` (`log`, `exp`,
`sigmoid`, `stable_stringify`). Run with `cargo bench -p verbora-classifiers`;
see [Benchmarks](../benchmarks/index).

## Allocation behaviour

- **`Classifier<E>`.** `docs: Vec<Document>` owns one `Vec<String>` per stored
  document (tokens, already stemmed for string input); `features: OrderedMap<f64>`
  holds one entry per distinct token as an insertion-ordered `Vec` plus a
  `HashMap` index.
- **`BayesEngine`.** `class_features: OrderedMap<BTreeMap<u32, f64>>` — one entry
  per **set** feature per class, sparse rather than dense.
- **`LogisticEngine`.** `examples: OrderedMap<Vec<Vec<u8>>>` retains every
  training document's observation vector **after** training, because `train()`
  rebuilds the whole matrix from scratch on every call. `theta` is one
  `Vec<f64>` per class, populated only once training succeeds.
- **MaxEnt.** `Sample` holds `Vec<Event>`, each owning its own deduplicated
  `Vec<String>` of predicates — **duplicate events are still kept**: 1,000
  repeated observations allocate 1,000 `Event`s, not one with a count, because
  `N` and every expectation are counted over occurrences.
  `train`/`train_with` builds its own sparse-row `Index` (`Vec<Arc<str>>`
  predicate names plus several parallel `u32`/`f64` `Vec`s) fresh from the
  sample on every call and drops it when the call returns; nothing from one fit
  is reused by the next.

There is no `_into` variant and no caller-supplied output buffer anywhere in this
crate. See [Allocation](../performance/allocation).

## Unicode and language notes

- **Astral-plane tokens are ordinary feature keys.** They are compared for string
  equality and never indexed by code unit, so `"😀"` behaves like any other token.
  `tests/edge_cases.rs` trains and classifies across ten Unicode categories —
  accented Latin, Cyrillic, Greek, CJK, astral, punctuation, digits, combining
  characters — through both `BayesClassifier` and `LogisticRegressionClassifier`.
- **String documents inherit the tokenizer/stemmer's Unicode quirks.**
  `Observation::Text` goes through `verbora-stemmers`' `TokenizeAndStem`, so any
  language-specific behaviour documented on
  [Tokenizers](../features/tokenizers) applies transitively.

### Process-global stop words

<span class="badge badge-global">GLOBAL STATE</span>

The default stemmer (English Porter, used whenever you construct a classifier
with `new()` rather than `with_stemmer`) tests stop words with
`verbora_core::stopwords::is_default_stopword`, backed by a process-wide
`LazyLock<RwLock<StopWords>>` — the same shared state
[Core vocabulary](../features/core) describes.

<div class="callout callout-warn">
<strong>Careful.</strong> Any <code>add_stop_word</code>/<code>remove_stop_word</code>
call anywhere in the process — including from an unrelated
<code>verbora-stemmers</code> or <code>verbora-phonetics</code> caller — changes
how every classifier in that process tokenises string documents from that point
on, retroactively affecting classifiers already constructed. A classifier fed
token slices (<code>Observation::Tokens</code>) never touches this state.
</div>

## Related

- [Tokenizers](../features/tokenizers) — the tokenization/stemming pipeline every
  string-input document goes through before it becomes a feature vector.
- [Core vocabulary](../features/core) — the process-global stop-word list.
- [WordNet](../features/wordnet) — the site's other synchronous, callback-free
  `save`/`load` persistence API.
- [Choosing an API](../choosing/index),
  [Decision trees](../choosing/decision-trees).
- [Allocation](../performance/allocation),
  [Batch vs streaming](../performance/batch-vs-streaming),
  [Ergonomics vs throughput](../performance/ergonomics-vs-throughput),
  [Parallelism](../performance/parallelism).
- [Benchmarks](../benchmarks/index), [Recipes](../recipes/index),
  [Roadmap](../features/roadmap).

## API reference

```bash
cargo doc -p verbora-classifiers --no-deps --open
```

| Item | Path |
|---|---|
| `BayesClassifier`, `BayesEngine` | `verbora_classifiers::{BayesClassifier, BayesEngine}` |
| `LogisticRegressionClassifier`, `LogisticEngine` | `verbora_classifiers::{LogisticRegressionClassifier, LogisticEngine}` |
| `Classifier<E>`, `Engine`, `Observation`, `Document`, `TrainingEvent`, `Classification`, `ClassifierError`, `LoadError` | `verbora_classifiers::{Classifier, Engine, Observation, Document, TrainingEvent, Classification, ClassifierError, LoadError}` |
| `Classifier::par_classify_batch` (requires `parallel`) | same path as `Classifier<E>` |
| `MaxEntClassifier`, `RestoreError`, `MaxEntError` | `verbora_classifiers::{MaxEntClassifier, RestoreError, MaxEntError}` |
| `Event`, `Sample` | `verbora_classifiers::{Event, Sample}` |
| `Gis`, `StopReason`, `TrainingReport` | `verbora_classifiers::{Gis, StopReason, TrainingReport}` |
| `MaxEntModel`, `ModelDefect` | `verbora_classifiers::{MaxEntModel, ModelDefect}` |
| Bit-exact `log`/`exp`/`pow`/`sigmoid` | `verbora_classifiers::{log, exp, pow, sigmoid}` |
| insertion-order map | `verbora_classifiers::OrderedMap` |
| JSON-like value, UTF-16-ordered stringify/parse | `verbora_classifiers::{DynValue, ParseError, json_stringify_pretty, number_to_string, utf16_cmp}` |
| Tokenize-and-stem adapter | `verbora_classifiers::{StemCache, Stemmer, StemmerOf, default_stemmer}` |

Source: `crates/verbora-classifiers/src/`. Boundary-input suite:
`crates/verbora-classifiers/tests/edge_cases.rs`. Benchmarks:
`crates/verbora-classifiers/benches/classifiers.rs`.
