# Classifiers

`verbora-classifiers` provides three document classifiers built on two unrelated
designs. `BayesClassifier` and `LogisticRegressionClassifier` are thin wrappers
around a naive-Bayes and a one-vs-rest logistic-regression engine, sharing one
generic base, `Classifier<E: Engine>`. `MaxEntClassifier` is a separate subsystem
implementing maximum-entropy classification by generalised iterative scaling
(GIS), including its own part-of-speech feature-generation machinery.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>13</strong> classifier APIs are
documented and test-pinned. <code>cargo test -p verbora-classifiers</code> runs
<strong>83</strong> in-crate unit tests, <strong>11</strong> boundary-input tests
in <code>tests/edge_cases.rs</code> and <strong>7</strong> doctests —
<strong>101 tests, 0 failures</strong>.
</div>

## When to use it

- **Fast incremental document classification with cheap retraining.**
  `BayesClassifier::train()` only processes documents added since the last call.
- **Feature-based scoring with hand-engineered context features.**
  `MaxEntClassifier` scores `(class, context)` pairs against feature functions you
  (or `POSElement`) supply — useful when a bag-of-words model is not expressive
  enough and you have features like "the previous two tags were DT, JJ".
- **Part-of-speech-style sequence tagging built on maximum entropy.**
  `MECorpus`/`MESentence`/`POSElement` turn a corpus of tagged sentences into
  context/class training samples through a window-based feature generator.
- **Labelling a large corpus offline** against one already-trained
  `BayesClassifier` or `LogisticRegressionClassifier`, via
  [`par_classify_batch`](#par-classify-batch).

## When not to use it

- **You want probabilities out of `MaxEntClassifier`.** Its scores are
  unnormalised weights — see [MaxEnt scores are not probabilities](#maxent-scores-are-not-probabilities).
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

    assert_eq!(classifier.classify("did the tests pass?").unwrap(), "other");
}
```

Every string is tokenised and stemmed by the default English Porter stemmer,
filtered against the stop-word list, then folded into a growing feature
vocabulary. `train()` hands each new document's 0/1 feature vector to the engine.

## Which classifier?

| | `BayesClassifier` | `LogisticRegressionClassifier` | `MaxEntClassifier` |
|---|---|---|---|
| Learns | per-class feature counts (Laplace-smoothed) | one-vs-rest weight vectors, intercept discarded | feature weights `alpha` |
| Trained by | one incremental pass | gradient descent, **from scratch on every call** | generalised iterative scaling |
| Retraining cost | cheap — only documents past `last_added` | expensive — discards the engine and re-adds every document | you choose when to call `train()` again |
| Score meaning | probability-like, not calibrated | sigmoid output in `(0, 1)` | **unnormalised weight**, not a probability |
| Shared base | `Classifier<BayesEngine>` | `Classifier<LogisticEngine>` | none — a free-standing type |
| Abstains by | `Err(ClassifierError::NotTrained)` when there are no classes | same | `Ok(String::new())` when top and bottom scores tie exactly |
| Parallel classification | [`par_classify_batch`](#par-classify-batch) | [`par_classify_batch`](#par-classify-batch) | **none**, deliberately |

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

`MaxEntClassifier` does not implement `Engine`. Its state is
`features: Rc<RefCell<FeatureSet>>` and `sample: Rc<RefCell<Sample>>` shared with
the caller, its `train` takes `(max_iterations, min_improvement)`, and its
`classify` takes an `&Rc<Context>` rather than a `&[u8]` feature vector. Forcing
it behind `Engine` would mean flattening `Context`/`Element`/`FeatureSet`/`Sample`
into a byte vector, discarding the feature-function flexibility that is the
reason to use MaxEnt at all.

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

`MaxEntClassifier` has neither method; its only entry point is
`train(max_iterations, min_improvement)`, and every call builds a brand-new
`GISScaler` — see [The correction feature outlives its
scaler](#the-correction-feature-outlives-its-scaler).

## `classify()` vs `get_classifications()`

Both types expose this pair, but they do not behave identically. `classify()`
always returns the single best label; `get_classifications()` returns every
class's score. The difference worth knowing is **sort order**:

- `Classifier<E>::get_classifications` sorts **descending by score**, with a
  stable, `NaN`-safe comparator — the array you get back is already ranked.
- `MaxEntClassifier::get_classifications` does **not sort**. It returns one score
  per class in **class-insertion order**; only `classify()` sorts internally
  before picking the winner.

```rust
use std::cell::RefCell;
use std::rc::Rc;

use verbora_classifiers::{Context, FeatureSet, MaxEntClassifier, SEElement, Sample};

fn main() {
    let mut sample = Sample::new();
    let zero = Rc::new(Context::of_str("0"));
    let one = Rc::new(Context::of_str("1"));
    for _ in 0..3 {
        sample.add_element(SEElement::new("x", Rc::clone(&zero)));
    }
    for _ in 0..3 {
        sample.add_element(SEElement::new("y", Rc::clone(&zero)));
    }
    sample.add_element(SEElement::new("x", Rc::clone(&one)));
    for _ in 0..3 {
        sample.add_element(SEElement::new("y", Rc::clone(&one)));
    }

    let mut features = FeatureSet::new();
    sample.generate_features(&mut features).unwrap();
    let mut classifier = MaxEntClassifier::new(
        Rc::new(RefCell::new(features)),
        Rc::new(RefCell::new(sample)),
    );
    classifier.train(20, 0.01).unwrap();

    // Class-insertion order ('x' then 'y'), NOT sorted by score — 'y' scores
    // higher here but stays second.
    let scores = classifier.get_classifications(&one).unwrap();
    assert_eq!(scores[0].label, "x");
    assert_eq!(scores[1].label, "y");
    assert!(scores[1].value > scores[0].value);

    // classify() sorts internally and returns the actual winner.
    assert_eq!(classifier.classify(&one).unwrap(), "y");

    // The scores are unnormalised weights: they overshoot 1 together.
    let sum: f64 = classifier
        .get_classifications(&zero)
        .unwrap()
        .iter()
        .map(|c| c.value)
        .sum();
    assert_eq!(sum, 1.621_790_831_347_259_3);
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
let texts = ["did the tests pass?", "the drive is full"];
let results = classifier.par_classify_batch(&texts);
assert_eq!(results[0].as_deref(), Ok("other"));
```

<div class="callout callout-warn">
<strong><code>MaxEntClassifier</code> has no <code>par_classify_batch</code>.</strong>
Its <code>Rc&lt;RefCell&lt;FeatureSet&gt;&gt;</code> and
<code>Rc&lt;RefCell&lt;Sample&gt;&gt;</code> are shared, interior-mutable state a
caller can read and write through at any time — load-bearing, not incidental.
This workspace makes a type shareable only when it genuinely is, never with an
<code>unsafe impl Send</code>/<code>Sync</code> to satisfy a <code>par_*</code>
API. See <a href="../performance/parallelism">Parallelism</a>.
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

Two differences between the `Classifier<E>` shape and `MaxEntClassifier`'s:

- **JSON shape.** `Classifier<E>::to_json` is compact (`{"a":1,"b":2}`);
  `MaxEntClassifier::to_json` pretty-prints with a 2-space indent.
- **`restore`/`load` need an extra argument.** `MaxEntClassifier::restore` and
  `::load` take a `revive: impl FnMut(&str, Rc<Context>) -> Rc<Element>` closure,
  because rebuilding a sample's elements requires knowing which `Element`
  subclass to construct (`SEElement`, `POSElement`, or your own).

<div class="callout callout-warn">
<strong>Careful — <code>MaxEntClassifier::restore</code> returns an untrained
classifier.</strong> It reads only the saved sample's elements, revives each one
through your constructor, and regenerates the feature set from scratch; the file's
<code>features</code>, <code>scaler</code> and <code>p</code> (including the trained
<code>alpha</code>) are parsed and never used. <code>distribution()</code> comes
back <code>None</code> — you must call <code>train()</code> again before
classifying. <code>Classifier&lt;E&gt;::restore</code> does <em>not</em> behave
this way: Bayes and logistic regression restore the full trained engine and work
immediately.
</div>

## Behaviour that changes your numbers

### Reproducible transcendental math

Rust's `f64::ln` and `f64::exp` call the platform's libm, whose results differ
between platforms — by one ULP on 4.9% and 9.7% of inputs respectively.
`verbora_classifiers::transcendental` provides its own `log` and `exp` instead
(fixed polynomial coefficients and `f64::to_bits`/`from_bits`, no `unsafe`), so a
model trains and scores identically everywhere. That matters because a one-ULP
difference lands directly in a Bayes score that is then *sorted* — a near-tie can
flip which class wins — and because logistic regression's descent loop stops when
successive costs differ by less than `1e-4`, so a perturbation can change the
*number* of iterations, and therefore the whole model.

Each algorithm's summation direction is fixed and observable for the same reason
(IEEE-754 addition is not associative): Bayes sums from the highest set feature
index down, logistic regression contracts descending but sums its cost function
ascending, and MaxEnt walks `sample.elements()` in insertion order **including
duplicates**.

```rust
use verbora_classifiers::transcendental;

fn main() {
    // A value where the platform libm's `ln` disagrees with this crate's own
    // `log` by exactly one ULP.
    let x = 11.262_564_292_775_972_f64;
    assert_eq!(transcendental::log(x).to_bits(), 0x4003_5f33_2d5c_29fc);
    assert_ne!(x.ln().to_bits(), transcendental::log(x).to_bits());
}
```

### Feature-vector key order is not insertion order

`Classifier::text_to_features` builds a document's 0/1 vector using a two-tier key
order: keys that are the canonical decimal spelling of an integer in
`0..=2^32-2` ("array-index" keys) come first in ascending numeric order, and every
other key follows in insertion order. `OrderedMap<V>` **recomputes** that order on
every call rather than caching stable indices, because indices shift whenever an
integer-like token is learned later.

Because `BayesClassifier::train()` is incremental, each document's feature vector
is built against whatever the current order was when *that* document was trained.
Add a new integer-like token later and every future `text_to_features` call
recomputes a shifted layout, while the counts already learned stay keyed under the
old slot numbers — silently invalidating the model with no error and no warning:

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    c.add_document(&vec!["alpha".to_owned()], "A");
    c.add_document(&vec!["beta".to_owned()], "B");
    c.train().unwrap();
    assert_eq!(c.classify(&vec!["alpha".to_owned()]).unwrap(), "A");

    // A brand-new document whose only token LOOKS LIKE AN INTEGER.
    c.add_document(&vec!["99".to_owned()], "C");
    c.train().unwrap();

    // "99" is hoisted to slot 0, shifting "alpha" and "beta" one slot right in
    // every FUTURE text_to_features call — but the counts learned for A and B
    // are still stored under their OLD slots.
    assert_eq!(c.feature_order(), vec!["99", "alpha", "beta"]);
    assert_eq!(c.classify(&vec!["alpha".to_owned()]).unwrap(), "B");
}
```

The same hazard applies to **class labels**: `add_document(text, "42")` gives an
integer-like label, which can misassign logistic regression's theta columns.

### MaxEnt scores are not probabilities

`Distribution::calculate_a_priori` returns `∏ⱼ αⱼ^fⱼ(x)` with no normalising
division. Values routinely exceed `1` and do not sum to `1` across a context's
classes (see the worked sum in the
[`classify` vs `get_classifications`](#classify-vs-get-classifications) example
above). This is correct, not a bug to route around: `entropy()` is likewise
`+Σ p log p` over these unnormalised weights, and `KullbackLieblerDistance()` is
typically negative. Adding a normalising division would change every score, change
the Kullback-Leibler trajectory `GISScaler::run` uses for its convergence check,
and therefore change the **iteration count** training stops at — silently
producing a different model, not differently-scaled output from the same one.

Sort or threshold on these scores for *relative* ranking only.

### Context keys sort by UTF-16 code unit

This crate needs two different key orderings side by side:

| Order | Used for | Rule |
|---|---|---|
| `utf16_cmp` | `Context::to_key` / `stable_stringify` — the hash key every frequency table, weight memo and normalisation constant is stored under | **every** key sorted by UTF-16 code unit |
| `own_key_order` | `to_json` / `save` | array-index keys ascending numerically first, then everything else in insertion order |

So `"-1"` (second character `'1'`, code unit `0x0031`) sorts **before** `"-2"`
(`0x0032`), which sorts before the array-index keys `"0"`, `"1"`, `"2"` — even
though `MESentence::generate_sample_elements` inserts a POS window in the order
`0`, `-2`, `-1`, `1`, `2`.

```rust
use verbora_classifiers::{Context, DynValue};

fn main() {
    let data = DynValue::Obj(vec![
        ("b".to_owned(), DynValue::Num(1.0)),
        ("a".to_owned(), DynValue::Num(2.0)),
        ("-1".to_owned(), DynValue::Str("z".into())),
        ("0".to_owned(), DynValue::Str("q".into())),
        ("2".to_owned(), DynValue::Str("w".into())),
    ]);
    assert_eq!(
        Context::new(data).to_key().unwrap(),
        r#"{"-1":"z","0":"q","2":"w","a":2,"b":1}"#
    );
}
```

**What this means for a caller.** `Context::to_key` always sorts before
rendering, so two contexts built with the same fields in different orders hash to
the identical key — field order alone cannot cause a cache miss. It does **not**
protect `to_json`/`save`, which use `own_key_order` and *are*
insertion-order-sensitive for non-array-index keys. Build `Context` payloads as an
ordered `DynValue::Obj(Vec<(String, DynValue)>)` — which every constructor in this
crate already does — not from a `HashMap`, whose iteration order would make two
semantically identical classifiers serialise to different bytes.

### The correction feature outlives its scaler

`GISScaler::add_correction_feature` builds a closure over its own scaler's
`C`/`feature_sums` state and appends it to the **shared** `FeatureSet` under the
fixed name `"Correction feature"`. `FeatureSet::add_feature` rejects a second
feature with the same dedup key, so a second `train()` call — which always builds
a brand-new `GISScaler` — cannot replace it.

If the sample is unchanged between calls this is invisible: the new scaler's
`C`/`feature_sums` match the old ones exactly, `train()` is idempotent, and the
feature set stays at 3 entries (2 shipped + 1 correction) rather than growing to
4. If you call `add_element` and *then* `train()` again, the correction feature
keeps evaluating against the **first** run's state. Build a fresh `FeatureSet`
(and a fresh `MaxEntClassifier` over it) when the sample changes.

## Behaviour worth knowing

- **MaxEnt's mutations are all eager.** `Sample::add_element` and
  `FeatureSet::add_feature` do their index update and dedup check synchronously;
  `GISScaler::run` is single-shot, running to convergence or `max_iterations` with
  no "advance one iteration" entry point. `Sample::with_elements` always returns
  `Err(MaxEntError::SampleAnalyseIsBroken)` on a non-empty slice — use
  `Sample::new()` plus a loop of `add_element`, which is the path
  `MECorpus::generate_sample` itself takes.
- **Labels are always `String`,** set from the moment `add_document` runs.
  Integer-like labels enumerate out of insertion order, as above.
- **`add_document` requires an explicit label.** Check before calling if you want
  to skip a document conditionally.
- **Bayes smoothing is `f64`-only with a truthy-and-finite guard.**
  `with_smoothing` falls back to `1.0` when passed `0`, `-0.0`, `NaN`, `+Inf` or
  `-Inf`; any other finite value — including `-1.0` — is accepted as-is.

## Common mistakes

- **Calling `LogisticRegressionClassifier::train()` repeatedly, expecting
  `BayesClassifier`-style incremental behaviour.** It resets and reruns gradient
  descent over every stored document on every call.
- **Adding a vocabulary token that looks like an integer,** then being surprised
  a trained model's feature indices shifted — `classify()` can silently change
  its answer for input it never saw. See
  [Feature-vector key order](#feature-vector-key-order-is-not-insertion-order).
- **Expecting `MaxEntClassifier` scores to be probabilities.** They are
  unnormalised weights that can exceed `1`.
- **Expecting `MaxEntClassifier::get_classifications` to come back sorted.** It
  returns class-insertion order; only `classify()` sorts.
- **Calling `MaxEntClassifier::train()` after the sample changed and expecting a
  fresh correction feature.** It keeps the first run's.
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
| `Classifier::text_to_features` | O(\|features\| + \|observation\|) | a `HashSet` probe, not a linear scan per feature |
| `LogisticRegressionClassifier::train` | O(iterations × classes × m × n) | bounded by `max_it = 500 × m` per class, typically far fewer at convergence |
| `MaxEntClassifier::train` | O(iterations × features × distinct contexts × classes) | generalised iterative scaling |
| `Distribution::weight` | O(\|alpha\|) | one power-and-multiply per feature, per element scored |

Every API in this crate is **eager**: no lazy iterator, no `_into`-shaped
buffer-reuse API, and no batch/streaming choice for *training* — `train()` always
processes its entire backlog in one call. Classification is the one exception, via
[`par_classify_batch`](#par-classify-batch).

`benches/classifiers.rs` groups: `bayes/train`, `logistic/train`,
`bayes/predict` (`text_to_features` / `get_classifications` / `classify`),
`bayes/persist`, `maxent/train` (the `SimpleExample` sample and POS corpora at
1/2/4 sentences), `maxent/predict` (memoised vs. unseen context), and
`reference_primitives` (`log`, `exp`, `sigmoid`, `stable_stringify`). Run with
`cargo bench -p verbora-classifiers`; see [Benchmarks](../benchmarks/index).

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
- **MaxEnt.** `Sample` holds `Vec<Rc<Element>>` — one entry per element,
  **duplicates included**: 1,000 repeated observations allocate 1,000 handles, not
  one with a count, because every summation depends on walking that exact
  sequence. `Context` and `Element` keys are lazily computed `String`s cached in a
  `RefCell<Option<String>>` — computed at most once, never invalidated if the
  underlying data changes afterwards.

There is no `_into` variant and no caller-supplied output buffer anywhere in this
crate. See [Allocation](../performance/allocation).

## Unicode and language notes

- **Astral-plane tokens are ordinary feature keys.** They are compared for string
  equality and never indexed by code unit, so `"😀"` behaves like any other token.
  `tests/edge_cases.rs` trains and classifies across ten Unicode categories —
  accented Latin, Cyrillic, Greek, CJK, astral, punctuation, digits, combining
  characters — through both `BayesClassifier` and `LogisticRegressionClassifier`.
- **MaxEnt context keys sort by UTF-16 code unit and emit non-ASCII raw.** An
  astral character's key sorts *before* `U+FFFD`, because its lead surrogate
  (`0xD83D`) is numerically below `0xFFFD` — the opposite of comparing by Unicode
  scalar value.
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
| `Classifier<E>`, `Engine`, `Observation`, `Document`, `TrainingEvent`, `Classification`, `ClassifierError`, `LoadError` | `verbora_classifiers::basic::*` (also re-exported at the crate root) |
| `Classifier::par_classify_batch` (requires `parallel`) | same path as `Classifier<E>` |
| `MaxEntClassifier`, `RestoreError`, `MaxEntError` | `verbora_classifiers::{MaxEntClassifier, RestoreError, MaxEntError}` |
| `Context`, `Element`, `GenerateFeatures` | `verbora_classifiers::{Context, Element, GenerateFeatures}` |
| `Feature`, `FeatureFn`, `FeatureSet` | `verbora_classifiers::{Feature, FeatureFn, FeatureSet}` |
| `Sample`, `Distribution`, `GISScaler`, `ScalerState` | `verbora_classifiers::{Sample, Distribution, GISScaler, ScalerState}` |
| `SEElement`, `POSElement`, `TaggedWord`, `MESentence`, `MECorpus` | `verbora_classifiers::{SEElement, POSElement, TaggedWord, MESentence, MECorpus}` |
| Bit-exact `log`/`exp`/`pow`/`sigmoid` | `verbora_classifiers::transcendental` |
| array-index-first, insertion-order map | `verbora_classifiers::OrderedMap` (also `::ordmap`) |
| JSON-like value, UTF-16-ordered stringify/parse | `verbora_classifiers::DynValue` (also `::dynval`) |
| Tokenize-and-stem adapter | `verbora_classifiers::{Stemmer, StemmerOf, default_stemmer}` |

Source: `crates/verbora-classifiers/src/`. Boundary-input suite:
`crates/verbora-classifiers/tests/edge_cases.rs`. Benchmarks:
`crates/verbora-classifiers/benches/classifiers.rs`.
