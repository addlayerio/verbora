# Classifiers

`verbora-classifiers` provides three document classifiers built on two
unrelated designs. `BayesClassifier` and `LogisticRegressionClassifier` are
thin wrappers around a naive-Bayes and a one-vs-rest logistic-regression
engine, sharing one generic base,
[`Classifier<E: Engine>`](#the-shared-classifier-e-engine-design).
`MaxEntClassifier` is a completely different subsystem — roughly 1,184 lines
across a dozen files — implementing maximum-entropy classification by
generalised iterative scaling (GIS), including its own part-of-speech
feature-generation machinery. All three learn from labelled documents (or,
for MaxEnt, feature functions over class/context pairs) and score new
observations against what they learned. Behaviour across all three is pinned
by this crate's own regression suite — part of the 526,341 recorded cases
across the workspace; see [Getting started: the workspace](../getting-started/workspace.md).

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>13</strong> classifier APIs
(<code>BayesClassifier</code>, <code>LogisticRegressionClassifier</code>,
<code>MaxEntClassifier</code>, <code>Context</code>, <code>Feature</code>,
<code>FeatureSet</code>, <code>Sample</code>, <code>Element</code>,
<code>SEElement</code>, <code>GISScaler</code>, <code>POSElement</code>,
<code>MESentence</code>, <code>MECorpus</code>) are documented and test-pinned.
<code>cargo test -p verbora-classifiers</code> runs <strong>83</strong> in-crate
unit tests, <strong>11</strong> boundary-input tests in
<code>tests/edge_cases.rs</code> and <strong>7</strong> doctests —
<strong>101 tests, 0 failures</strong>.
</div>

## When to use it

- **Fast incremental document classification with cheap retraining.**
  `BayesClassifier::train()` only processes documents added since the last
  call; see [Which classifier?](#which-classifier).
- **Feature-based scoring, possibly with hand-engineered context features.**
  `MaxEntClassifier` scores `(class, context)` pairs against feature
  functions you (or `POSElement`) supply — useful when a bag-of-words model
  is not expressive enough and you have features like "the previous two
  tags were DT, JJ" to offer it.
- **Part-of-speech-style sequence tagging built on maximum entropy.**
  `MECorpus`/`MESentence`/`POSElement` provide the training-data pipeline for
  that use case: a corpus of tagged sentences becomes context/class training
  samples through a fixed window-based feature generator.
- **Labelling a large corpus offline against one already-trained
  `BayesClassifier` or `LogisticRegressionClassifier`.**
  `Classifier<E>::par_classify_batch`, behind the `parallel` Cargo feature,
  fans `classify` out across threads — see
  [`classify()` vs `par_classify_batch()`](#classify-vs-par-classify-batch).
  `MaxEntClassifier` has no equivalent, deliberately.

## When not to use it

- **You want probabilities out of `MaxEntClassifier`.** Its scores are
  deliberately unnormalised weights — see
  [MaxEnt's unnormalised weights](#_4-maxent-s-unnormalised-weights).
- **You want state-of-the-art classification.** These are three specific
  circa-2010s algorithms — hand-written naive-Bayes and one-vs-rest
  logistic-regression engines and a hand-written GIS trainer, quirks and all
  — not a modern gradient-boosted tree or a neural model. If you want
  current-generation accuracy, this is not the crate for it.
- **You want cheap retraining from `LogisticRegressionClassifier`.** Every
  `train()` call reruns gradient descent from scratch over every stored
  document — see the callout in
  [Which classifier?](#which-classifier).
- **You need document classification independent of the process-global
  stop-word list.** String documents go through the default Porter stemmer,
  which reads Verbora's process-wide mutable stop-word list — see
  [Process-global stop words](#process-global-stop-words). Pass tokens
  instead of a string to bypass it entirely.

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

This is the crate's own module-doc example — every string is tokenised and
stemmed by the default English Porter stemmer, filtered against the stop-word
list, then folded into a growing feature vocabulary. `train()` hands each new
document's 0/1 feature vector to the naive-Bayes engine.

## Choosing the right API

Two decisions sit on top of each other, and the second is really three
decisions in a trench coat: **which of the three classifiers to reach for**,
and then, having picked one, **which of its own API pairs to call** — `train`
vs `retrain`, `classify` vs `get_classifications`, and two shapes of
persistence. MaxEnt gets its own short answer for each, because its design
does not track the other two.

### Which classifier?

| | `BayesClassifier` | `LogisticRegressionClassifier` | `MaxEntClassifier` |
|---|---|---|---|
| Learns | per-class feature counts (Laplace-smoothed) | one-vs-rest weight vectors, intercept discarded | feature weights `alpha` |
| Trained by | one incremental pass | gradient descent, **from scratch on every call** | generalised iterative scaling (GIS) |
| Retraining cost | cheap — `train()` only processes documents past `lastAdded` | expensive — every call discards the engine and re-adds every document | you decide when to call `train()` again; a correction feature from a **prior** run can persist — see below |
| Score meaning | probability-like, not calibrated | sigmoid output in `(0, 1)` | **unnormalised weight**, not a probability |
| Shared base | `Classifier<BayesEngine>` | `Classifier<LogisticEngine>` | none — a free-standing type |
| Parallel classification | `par_classify_batch` (`parallel` feature) | `par_classify_batch` (`parallel` feature) | **none** — `Rc<RefCell<_>>` state is load-bearing, not incidental; see [below](#classify-vs-par-classify-batch) |

<div class="callout callout-warn">
<strong>Careful.</strong> The crate's own module doc states this in one line:
<code>LogisticRegressionClassifier</code> is "trained by gradient descent,
from scratch each call." Retraining is <strong>not</strong> incremental the
way <code>BayesClassifier</code>'s is — every <code>train()</code> call throws
away the engine and re-adds every stored document, re-running convergence
from zero. Calling it in a loop as new documents trickle in has the cost
profile of training a fresh classifier every time. See
<a href="#train-vs-retrain"><code>train()</code> vs <code>retrain()</code></a>.
</div>

```text
I need to build a document classifier
│
├── Fast incremental updates as documents arrive
│      └── BayesClassifier — train() only processes new documents
│
├── A linear decision boundary, and full retraining cost is acceptable
│      └── LogisticRegressionClassifier
│
└── Feature-based scoring, possibly with hand-engineered context
    features (e.g. part-of-speech windows)
       └── MaxEntClassifier — generalised iterative scaling
```

See [Decision trees](../choosing/decision-trees) for how this site expects
you to read one of these.

### The shared `Classifier<E: Engine>` design

`BayesClassifier` and `LogisticRegressionClassifier` are both
`pub type` aliases over one generic struct:

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
```

`Classifier<E>` owns everything `BayesClassifier` and `LogisticRegressionClassifier`
share — the document list, the feature vocabulary, the stemmer, the
`last_added` cursor — and defers only the numerically interesting part to the
`Engine` trait:

<div class="callout callout-note">
<strong>Note.</strong> <code>stemmer</code> was <code>Rc&lt;dyn Stemmer&gt;</code>
until this crate grew a <code>parallel</code> Cargo feature. <code>Rc</code> is
not <code>Send</code>, so a <code>Classifier&lt;E&gt;</code> built around it could
never be shared across threads at all — not even read-only. Moving to
<code>Arc&lt;dyn Stemmer + Send + Sync&gt;</code> was the prerequisite
<a href="#classify-vs-par-classify-batch"><code>par_classify_batch</code></a>
needed, verified as a pure representation change with zero behaviour change:
nothing about how a document is stemmed differs, only what kind of pointer
holds the stemmer.
</div>

```rust ignore
pub trait Engine: Default + Clone {
    const RESETS_ON_TRAIN: bool;
    fn add_example(&mut self, observation: &[u8], label: &str);
    fn fit(&mut self) -> Result<(), ClassifierError>;
    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError>;
    fn to_value(&self) -> DynValue;
    fn from_value(value: &DynValue) -> Self;
}
```

`BayesEngine` and `LogisticEngine` are the two implementors. `RESETS_ON_TRAIN`
is the entire override that distinguishes their `train()` behaviour — `false`
for Bayes (accumulates), `true` for logistic regression (discards and
restarts) — which is why the two classifiers can share one `train()` method
body at all.

```rust
use verbora_classifiers::{BayesClassifier, LogisticRegressionClassifier};

fn main() {
    let _bayes = BayesClassifier::new();
    let _logreg = LogisticRegressionClassifier::new();
}
```

**`MaxEntClassifier` does not implement `Engine`, and is not
`Classifier<SomethingElse>`.** Verified directly from the source: there is no
`impl Engine for` anything in `src/maxent/`, and `MaxEntClassifier`'s own
fields and methods have a completely different shape —

```rust ignore
pub struct MaxEntClassifier {
    features: Rc<RefCell<FeatureSet>>,
    sample: Rc<RefCell<Sample>>,
    scaler: Option<GISScaler>,
    p: Option<Rc<Distribution>>,
}
```

— shared feature set and sample by `Rc<RefCell<_>>` rather than owned, a
`train(max_iterations: i64, min_improvement: f64)` with no `Engine::fit()`
counterpart, and `get_classifications`/`classify` that take `&Rc<Context>`
rather than a `&[u8]` feature vector. `MaxEntClassifier` shares no code, no
base type, and no method signature with `Classifier<E>`, and this crate does
not invent a shared abstraction to paper over that: a fourth `Engine` impl
for MaxEnt would need to flatten `Context`/`Element`/`FeatureSet`/`Sample`
down into a `&[u8]` vector, discarding exactly the feature-function
flexibility that is the point of using MaxEnt in the first place.

### `train()` vs `retrain()`

Both methods exist only on `Classifier<E>` (Bayes and logistic regression).
`train()` respects `E::RESETS_ON_TRAIN`; `retrain()` always rebuilds the
engine from scratch first, then calls `train()`.

```rust
use verbora_classifiers::{BayesClassifier, LogisticRegressionClassifier, TrainingEvent};

fn main() {
    let docs = [("alpha", "p"), ("beta", "q")];

    let mut bayes = BayesClassifier::new();
    let mut logreg = LogisticRegressionClassifier::new();
    for (text, label) in docs {
        bayes.add_document(&vec![text.to_owned()], label);
        logreg.add_document(&vec![text.to_owned()], label);
    }

    let mut bayes_events = Vec::new();
    let mut logreg_events = Vec::new();
    bayes.train_with(|e| bayes_events.push(e)).unwrap();
    logreg.train_with(|e| logreg_events.push(e)).unwrap();
    // Calling train() again, as if nothing new had happened:
    bayes.train_with(|e| bayes_events.push(e)).unwrap();
    logreg.train_with(|e| logreg_events.push(e)).unwrap();

    let trained = |events: &[TrainingEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, TrainingEvent::TrainedWithDocument { .. }))
            .count()
    };
    // Bayes: the second train() found nothing new past `lastAdded`.
    assert_eq!(trained(&bayes_events), 2);
    // LogisticRegression: the second train() redid gradient descent over
    // BOTH documents again, from scratch.
    assert_eq!(trained(&logreg_events), 4);
}
```

`retrain()` rebuilds the engine with **no arguments** regardless of which
classifier it is — `E::default()`, not "the engine you configured." For
`BayesClassifier::with_smoothing`, that silently reverts a custom smoothing
constant to `1.0`:

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::with_smoothing(0.1);
    c.add_document("my unit-tests failed.", "software");
    c.train().unwrap();
    assert_eq!(c.engine().smoothing(), 0.1);

    c.retrain().unwrap();
    assert_eq!(
        c.engine().smoothing(),
        1.0,
        "retrain() rebuilds the engine with no arguments"
    );
}
```

`MaxEntClassifier` has neither method. Its only entry point is
`train(max_iterations, min_improvement)`, and every call to it builds a
**brand-new** `GISScaler` — see
[MaxEnt's own internal choice points](#maxent-s-own-internal-choice-points)
for what "calling `train()` again" actually does there.

### `classify()` vs `get_classifications()`

Both `Classifier<E>` and `MaxEntClassifier` expose this pair, but they do not
behave identically. `classify()` always returns the single best label;
`get_classifications()` returns every class's score. The difference worth
knowing is **sort order**:

- `Classifier<E>::get_classifications` sorts **descending by score**, with a
  stable, `NaN`-safe comparator (`sort_descending`) — the array you get back
  is already ranked.
- `MaxEntClassifier::get_classifications` does **not sort**. It returns one
  score per class in **class-insertion order** — the order classes first
  appeared in the training sample — and only `classify()` sorts internally
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

    // get_classifications comes back in class-insertion order ('x' then
    // 'y'), NOT sorted by score — 'y' scores higher here but stays second.
    let scores = classifier.get_classifications(&one).unwrap();
    assert_eq!(scores[0].label, "x");
    assert_eq!(scores[0].value, 0.777_505_117_061_545);
    assert_eq!(scores[1].label, "y");
    assert_eq!(scores[1].value, 1.885_178_571_428_571_1);
    assert!(scores[1].value > scores[0].value);

    // classify() sorts internally and returns the actual winner.
    assert_eq!(classifier.classify(&one).unwrap(), "y");
}
```

One more divergence in the error/abstain shape: `Classifier<E>::classify`
returns `Err(ClassifierError::NotTrained)` when there are no classes to
score. `MaxEntClassifier::classify` instead returns `Ok(String::new())` — the
empty string — whenever the highest and lowest scores are **exactly equal**,
including the everyday case of an unseen context where every feature scores
zero and every class ties at weight `1`. A classifier that cannot
discriminate declines to answer rather than guessing; see the worked example
in the persistence section's doctest for a context that triggers this.

### `classify()` vs `par_classify_batch()`

Only `Classifier<E>` — `BayesClassifier` and `LogisticRegressionClassifier` —
has this pair. `MaxEntClassifier` does not, and that absence is deliberate,
not an oversight; see the callout below.

Behind this crate's `parallel` Cargo feature,
`Classifier<E>::par_classify_batch(&self, texts: &[&str]) -> Vec<Result<String,
ClassifierError>>` (requiring `E: Sync`) classifies many independent texts
against one already-trained classifier. A trained `Classifier<E>` is
read-only from `classify`'s point of view — `text_to_features`,
`get_classifications` and `classify` all take `&self` — and every field
involved (`docs`, `features`, the engine, and the `stemmer`, now
`Arc<dyn Stemmer + Send + Sync>` for exactly this reason — see
[The shared `Classifier<E: Engine>` design](#the-shared-classifier-e-engine-design)
above) is `Send + Sync`. So this method is exactly
`texts.par_iter().map(|t| self.classify(*t)).collect()` — a thin fan-out over
the existing sequential `classify`, not a second implementation of it.
`get_classifications` and `text_to_features` are untouched; for those shapes
in parallel, apply the same `par_iter().map(...)` pattern at your own call
site (see [Parallelism](../performance/parallelism)).

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>texts.par_iter().map(|t| self.classify(*t)).collect()</code> over Rayon's global thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;Result&lt;String, ClassifierError&gt;&gt;</code>, input order preserved</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code> sized to <code>texts.len()</code>, plus whatever <code>classify</code> itself allocates per text (a stemmed token <code>Vec&lt;String&gt;</code>, a 0/1 feature vector, the engine's own per-call scoring allocations)</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — behind the <code>parallel</code> Cargo feature; requires <code>E: Sync</code> (true for both <code>BayesEngine</code> and <code>LogisticEngine</code>). Not available on <code>MaxEntClassifier</code></span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Labelling a large corpus offline against one already-trained classifier</span></div>
</div>

**When to reach for it vs. the sequential loop.** Reach for this only when
the *batch*, not the single classification, is the unit of work. A single
`classify` call costs on the order of 13 µs for a Bayes classifier trained on
a few dozen documents (this crate's own `bayes/predict/classify` benchmark),
and a `rayon` task costs on the order of a microsecond to schedule, so a
handful of texts is close to the break-even point — measure before reaching
for this on small batches, and prefer a plain
`texts.iter().map(Classifier::classify)` loop there. Batches in the thousands
amortise the scheduling cost easily; reproduce with
`cargo bench -p verbora-classifiers --features parallel -- bayes/predict_batch`.

Output order matches input order — `results[i]` is `self.classify(texts[i])`
— via `rayon`'s order-preserving `map` + `collect`. Each element carries its
own `Result`, exactly as a sequential
`texts.iter().map(|t| self.classify(*t)).collect::<Vec<_>>()` would: one
text's `ClassifierError` does not abort the others.

```rust  ignore
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut classifier = BayesClassifier::new();
    classifier.add_document("my unit-tests failed.", "software");
    classifier.add_document("tried the program, but it was buggy.", "software");
    classifier.add_document("tomorrow we will do standard tests", "other");
    classifier.add_document("the drive has a 2TB capacity", "other");
    classifier.train().unwrap();

    let texts = ["did the tests pass?", "did the tests pass?"];
    let results = classifier.par_classify_batch(&texts);
    assert_eq!(results[0].as_deref(), Ok("other"));
    assert_eq!(results[1].as_deref(), Ok("other"));
}
```

<div class="callout callout-warn">
<strong><code>MaxEntClassifier</code> has no <code>par_classify_batch</code>,
and that is deliberate.</strong> <code>Classifier&lt;E&gt;</code>'s fields are
all <code>Send + Sync</code> after the <code>Arc</code> migration above;
<code>MaxEntClassifier</code>'s are not, on purpose —
<code>features: Rc&lt;RefCell&lt;FeatureSet&gt;&gt;</code> and
<code>sample: Rc&lt;RefCell&lt;Sample&gt;&gt;</code> (see
<a href="#the-shared-classifier-e-engine-design">The shared
<code>Classifier&lt;E: Engine&gt;</code> design</a>) are shared, interior-mutable
state a caller can read and write through at any time — load-bearing, not
incidental. This workspace's Rayon policy does not paper over that with
<code>unsafe impl Send</code>/<code>Sync</code>: a type is made shareable only
when it genuinely is, never to satisfy a <code>par_*</code> API (the same
restraint applied to <code>PorterStemmerNl</code>'s sticky
<code>Cell&lt;bool&gt;</code> flag elsewhere in this workspace — see
<a href="../performance/parallelism">Parallelism</a>). Classifying many
contexts against one <code>MaxEntClassifier</code> in parallel would require
redesigning its state to be genuinely safe to share first, which is out of
scope for a thin <code>par_*</code> wrapper by this crate's own rule.
</div>

### Persistence: `to_json`/`restore` vs `save`/`load`

Every classifier in this crate has the same pair-of-pairs: `to_json`/`restore`
round-trip an in-memory string with no I/O, and `save`/`load` are thin
synchronous wrappers writing and reading a file.

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

`save` and `load` are synchronous, the same choice [WordNet](../features/wordnet)
makes for its own `lookup`/`get` API: within one operation, file I/O is
strictly sequential, so there is nothing for an asynchronous, callback-based
shape to buy a caller here — a plain `Result` return value carries the same
information.

Two things differ between the `Classifier<E>` shape and `MaxEntClassifier`'s:

- **Compact vs pretty JSON.** `Classifier<E>::to_json` produces compact
  JSON (`{"a":1,"b":2}`); `MaxEntClassifier::to_json` pretty-prints with a
  2-space indent. This is a deliberate difference between the two
  persistence paths, not an inconsistency to "fix."
- **`restore`/`load` need an extra argument.** `MaxEntClassifier::restore`
  and `::load` take a `revive: impl FnMut(&str, Rc<Context>) -> Rc<Element>`
  closure, because rebuilding a sample's elements requires knowing which
  `Element` subclass to construct (`SEElement`, `POSElement`, or your own).
  `Classifier<E>::restore` needs nothing extra: Bayes and logistic
  regression have no equivalent per-element polymorphism.

**`MaxEntClassifier::restore` also throws away the trained model — see
[Persistence detail: `restore` returns an untrained classifier](#persistence-detail-restore-returns-an-untrained-classifier)
below**, which is different enough from what "restore" means for the other
two classifiers that it gets its own flagged section rather than a footnote
here.

### MaxEnt's own internal choice points

`GISScaler`, `Sample` and `FeatureSet` expose **no lazy or incremental
variant of anything**. Every mutation is eager and immediate:

- `Sample::add_element` pushes onto `Vec<Rc<Element>>` and updates the
  frequency/class indices synchronously — there is no buffered or batched
  form.
- `FeatureSet::add_feature` performs its dedup check and push in one call —
  nothing defers feature registration.
- `GISScaler::run` is single-shot: one call always runs to convergence (or
  `max_iterations`), computing the correction feature, building a
  `Distribution`, and iterating GIS updates start to finish. There is no
  "advance one iteration and give me a checkpoint" entry point.
- `Sample::with_elements` exists as a constructor, but always returns
  `Err(MaxEntError::SampleAnalyseIsBroken)` on a non-empty slice — see
  [`Sample::with_elements` always errors](#behaviour-worth-knowing). It is not a usable
  "batch-construct with data" alternative to `Sample::new()` plus a loop of
  `add_element` calls; that loop is the only path this crate's own
  `MECorpus::generate_sample` takes, building `Sample::new()` and adding
  elements one at a time.

The closest thing to a "choice" is what calling `train()` again actually
does, and it is worth seeing directly:

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
    let features = Rc::new(RefCell::new(features));
    let sample = Rc::new(RefCell::new(sample));
    let mut classifier = MaxEntClassifier::new(Rc::clone(&features), Rc::clone(&sample));
    classifier.train(20, 0.01).unwrap();
    let first_alpha = classifier.distribution().unwrap().alpha();

    // Calling train() again on an UNCHANGED sample is idempotent: there is
    // no incremental variant to reach for instead, so retraining just
    // repeats the same computation and lands on the same alpha.
    classifier.train(20, 0.01).unwrap();
    assert_eq!(classifier.distribution().unwrap().alpha(), first_alpha);

    // Still 3 features (2 shipped + 1 correction), not 4: the correction
    // feature from the FIRST training run was never replaced, because
    // FeatureSet::add_feature rejects a second feature under the same name.
    assert_eq!(features.borrow().size(), 3);
}
```

That last assertion is a real trap once the sample *does* change between
runs — see
[The correction feature outlives its scaler](#the-correction-feature-outlives-its-scaler)
under Advanced usage.

## Advanced usage

The crate's own module doc identifies five places where the obvious Rust
translation is not enough on its own to get the numerically or structurally
correct answer. Each one is verified — by a bit-exact test against a recorded
fixture, by the crate's own regression suite, or both.

### 1. FDLIBM float precision inside a convergence loop

Rust's `f64::ln` and `f64::exp` call the platform's own libm.
`verbora_classifiers::transcendental` implements the FDLIBM `log` and `exp`
algorithms directly instead — the same polynomial coefficients, the same bit
manipulation via `f64::to_bits`/`from_bits` (this workspace forbids `unsafe`,
so no pointer tricks). The two disagree by exactly one ULP on a meaningful
fraction of inputs: over 20,000 pseudo-random arguments, the platform's `ln`
differs from the FDLIBM value in 981 cases (4.9%) and `exp` in 1,933 (9.7%).

That would not matter if the results were only reported. They are not:
`BayesEngine::probability_of_class` sums `log(count / total)` and
exponentiates the result, so a one-ULP difference lands directly in the
score, and the scores are then sorted — a near-tie can flip which class wins.
Logistic regression's `sigmoid` and cost function run inside a
gradient-descent loop that iterates until successive costs differ by less
than `1e-4`; a one-ULP perturbation compounds over hundreds of iterations and
can change the *number* of iterations the loop runs, and therefore the whole
model.

`verbora_classifiers::transcendental::log` and `::exp` are pinned bit-exact
against this crate's own recorded fixture of 45,039 arguments for `log` and
50,000 for `exp`: zero differences. One argument is a known, deliberate
exception: `exp(1.0)` is one ULP away from its recorded fixture value,
because the fixture records the correctly-rounded constant `e` there as a
special case rather than FDLIBM's own output; the crate does not patch
around it, because a hand-inserted special case FDLIBM itself lacks would be
a second, unverifiable divergence.

```rust
use verbora_classifiers::transcendental;

fn main() {
    // A value where the platform libm's `ln` disagrees with this crate's
    // FDLIBM-based `log` by exactly one ULP.
    let x = 11.262_564_292_775_972_f64;
    assert_eq!(transcendental::log(x).to_bits(), 0x4003_5f33_2d5c_29fc);
    assert_ne!(x.ln().to_bits(), transcendental::log(x).to_bits());
}
```

### 2. Feature-vector key order is not insertion order

`Classifier::text_to_features` builds a document's 0/1 feature vector using a
two-tier key order, not insertion order: keys that are the canonical decimal
spelling of an integer in `0..=2^32-2` ("array-index" keys) come first, in
ascending numeric order; every other key follows in insertion order.
`OrderedMap<V>` (`src/ordmap.rs`) implements exactly this two-tier order and
— critically — **recomputes it on every call** rather than caching stable
indices, because indices shift whenever an integer-like token is learned
later.

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    c.add_document(
        &vec!["zebra".to_owned(), "42".to_owned(), "appl".to_owned()],
        "A",
    );

    // "42" was the SECOND token added, but integer-like keys enumerate
    // first: it lands at slot 0, ahead of "zebra".
    assert_eq!(c.feature_order(), vec!["42", "zebra", "appl"]);
    assert_eq!(c.text_to_features(&vec!["42".to_owned()]), vec![1, 0, 0]);
}
```

**What "silently invalidating a trained model" actually means.** Because
`BayesClassifier::train()` is incremental, each document's feature vector is
built against whatever the *current* feature order is at the moment that
document is trained — not a schema fixed once at the start. Add a new
integer-like token later, and every *future* `text_to_features` call
recomputes a shifted layout, while the counts the engine already learned
stay keyed under the *old* slot numbers:

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

    // "99" is hoisted to slot 0, silently shifting "alpha" and "beta" one
    // slot to the right in every FUTURE text_to_features call — but the
    // counts learned for A and B are still stored under their OLD slots.
    assert_eq!(c.feature_order(), vec!["99", "alpha", "beta"]);
    assert_eq!(c.classify(&vec!["alpha".to_owned()]).unwrap(), "B");

    let scores = c.get_classifications(&vec!["alpha".to_owned()]).unwrap();
    assert_eq!(scores[0].label, "B");
    assert_eq!(scores[0].value, 0.5);
    assert_eq!(scores[1].label, "A");
    assert_eq!(scores[1].value, 0.25);
    assert_eq!(scores[2].label, "C");
    assert_eq!(scores[2].value, 0.25);
}
```

No error, no warning: `classify(["alpha"])` silently flips from `"A"` to
`"B"`, and `"A"`'s confidence drops to a tied guess with the brand-new class
`"C"` that has nothing to do with `"alpha"` at all. This exact scenario, down
to these exact scores, is pinned in this crate's own recorded fixture.

### 3. Floating-point accumulation order per algorithm

IEEE-754 addition is not associative, so the *order* a sum is accumulated in
is part of the observable output, not an implementation detail. Every
bit-exact assertion already on this page is indirect proof of this: change
any one of the iteration directions below and the least-significant bits of
every score on this page would stop matching the recorded fixture.

**Bayes** sums its log-probabilities from the **highest set feature index
down to zero** (excerpted; see `src/basic/bayes.rs` for the full function):

```rust ignore
let mut prob = 0.0;
let mut i = observation.len();
while i > 0 {
    i -= 1;
    if observation[i] != 0 {
        let count = /* classFeatures[label][i], falling back to smoothing */;
        prob += transcendental::log(count / total);
    }
}
```

**Logistic regression** is the one place two *different* directions coexist
in the same algorithm. Every matrix/vector contraction — the hypothesis's dot
product, the gradient's contraction over the row index — sums **descending**
over the contracted index (`while k > 0 { k -= 1; … }`, `src/basic/logistic.rs`).
The **cost function's** sum, in contrast, runs **ascending** (`for k in 0..m`),
mapping over its elements in order rather than contracting. An idiomatic
`iter().sum()` written once and reused for both would be wrong in whichever
of the two places it was not designed for.

**MaxEnt** walks `sample.elements()` — a plain `Vec<Rc<Element>>` — in
insertion order, **including duplicates**, in every summation:
`Feature::observed_expectation`, `Feature::expectation_approx`,
`Distribution::prepare_weights`, `Distribution::kullback_liebler_distance`,
and more. Three identical `("x", "0")` observations are summed as three
separate terms, not folded into one term multiplied by three — the ten-element
`SimpleExample` sample the doctests use is quite literally ten loop
iterations, not however many *distinct* elements it contains.

### 4. MaxEnt's unnormalised weights

`Distribution::calculate_a_priori` returns `∏ⱼ αⱼ^fⱼ(x)` with no normalising
division. The values routinely exceed `1` and do not sum to `1` across a
context's classes:

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

    let scores = classifier.get_classifications(&zero).unwrap();
    assert_eq!(scores[0].value, 0.844_285_714_285_714_2);
    assert_eq!(scores[1].value, 0.777_505_117_061_545);

    let sum: f64 = scores.iter().map(|c| c.value).sum();
    // Neither score is a probability, and together they overshoot 1 — this
    // is correct, not a rounding error.
    assert_eq!(sum, 1.621_790_831_347_259_3);
    assert_ne!(sum, 1.0);
}
```

This is correct, not a bug to route around. `entropy()` is similarly `+Σ p
log p` over these unnormalised weights rather than a true (negative) entropy,
and `KullbackLieblerDistance()` is typically *negative* because it both
divides by an unnormalised weight and iterates duplicate elements. "Fixing"
any of this by adding a normalising division changes every score, changes the
Kullback-Leibler trajectory `GISScaler::run` uses for its convergence check,
and therefore changes the **iteration count** training stops at — silently
producing a different model, not merely differently-scaled output from the
same one.

### 5. Context keys sort by UTF-16 code unit

`Context::to_key` produces a JSON-like serialisation whose object keys are
sorted by **UTF-16 code unit**, not by Rust's default UTF-8 scalar-value
`str: Ord`, and not by array-index-first insertion order either — see
`own_key_order` below. This crate genuinely needs two *different* orderings
side by side, and conflating them is easy to get wrong:

- `utf16_cmp` (`src/dynval.rs`) — sorts **every** key by UTF-16 code unit.
  Used only for `Context::to_key`/`stable_stringify`, the hash key every
  frequency table, weight memo, and normalisation constant is stored under.
- `own_key_order` (`src/dynval.rs`) — array-index keys ascending numerically
  first, then everything else in **insertion order**. Used only for
  `to_json`/`save`.

A `POSElement`'s context window is a concrete case where these two orders
visibly disagree. `MESentence::generate_sample_elements` inserts window keys
in the order `0`, `-2`, `-1`, `1`, `2` — but the *context key* sorts them by
UTF-16 code unit, so `"-1"` (whose second character `'1'` is code unit
`0x0031`) sorts **before** `"-2"` (`0x0032`), which sorts before the
array-index keys `"0"`, `"1"`, `"2"`:

```rust
use verbora_classifiers::{MESentence, Sample, TaggedWord};

fn main() {
    let sentence = MESentence::with_tagged_words(vec![
        TaggedWord::new("the", "DT"),
        TaggedWord::new("big", "JJ"),
        TaggedWord::new("dog", "NN"),
        TaggedWord::new("runs", "VB"),
    ]);
    let mut sample = Sample::new();
    sentence.generate_sample_elements(&mut sample);

    let keys: Vec<String> = sample.elements().iter().map(|e| e.to_key()).collect();
    // "-1" before "-2" before "0" — UTF-16 code-unit order, not insertion
    // order and not array-index-first order.
    assert_eq!(
        keys[2],
        r#"NN{"tagWindow":{"-1":"JJ","-2":"DT","0":"NN","1":"VB"},"wordWindow":{"-1":"big","-2":"the","0":"dog","1":"runs"}}"#
    );
}
```

The same sorting rule holds for arbitrary payloads, ASCII or not:

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

**What this means for a Rust caller.** It is tempting to reach for a
`HashMap<String, DynValue>` to build a context's payload, expecting
"unordered" semantics to be harmless here of all places. In one sense that
instinct is *safe*: `Context::to_key` always sorts before rendering, so two
contexts built with the same fields in different orders hash to the
identical key — there is no accidental cache miss from field order alone.
What it does **not** protect you from is `to_json`/`save`: those use
`own_key_order`, which *is* insertion-order-sensitive for non-array-index
keys, so a `HashMap`'s effectively-random iteration order would make two
semantically identical classifiers serialise to visibly different bytes.
Build `Context` payloads as an ordered `DynValue::Obj(Vec<(String, DynValue)>)`
— which is what every constructor in this crate already does — not from a
`HashMap`.

## Behaviour worth knowing

<div class="callout callout-note">
Behavioural notes drawn from the crate's own module documentation.
</div>

- **Labels and class names are always `String`.** `Document::label: String`
  is set from the moment `add_document` runs, and both classifiers read that
  same field. Integer-like string labels enumerate out of insertion order
  the same way integer-like tokens do — see
  [Feature-vector key order is not insertion order](#_2-feature-vector-key-order-is-not-insertion-order)
  — which can misassign logistic regression's theta columns to the wrong
  label. That is reachable from any input where a class label happens to
  look like an integer, e.g. `add_document(text, "42")`.
- **`add_document` requires an explicit label.** There is no way to add a
  document while skipping the classification argument — `add_document`'s
  `classification: &str` parameter is not optional. A caller wanting to
  conditionally skip a document must check before calling `add_document`.
- **Bayes smoothing is `f64`-only, with a truthy-and-finite guard.**
  `BayesClassifier::with_smoothing` falls back to the default smoothing
  constant of `1.0` whenever the value passed is `0`, `-0.0`, `NaN`, `+Inf`,
  or `-Inf`; any other finite value, including a negative one such as
  `-1.0`, is accepted as-is:
  ```rust
  use verbora_classifiers::BayesClassifier;

  fn main() {
      let mut c = BayesClassifier::with_smoothing(0.0);
      c.add_document("my unit-tests failed.", "software");
      c.train().unwrap();
      // 0, -0, NaN, +Inf and -Inf are all falsy-or-non-finite and fall back
      // to the default; -1 would be accepted, since it is truthy and finite.
      assert_eq!(c.engine().smoothing(), 1.0);
  }
  ```

## Persistence detail: `restore` returns an untrained classifier

<div class="callout callout-warn">
<strong>Careful.</strong> <code>MaxEntClassifier::restore</code> (and
<code>::load</code>) return a classifier with <strong>no trained
model</strong> — <code>alpha</code>, the scaler, and the distribution in the
saved file are read but discarded. You must call <code>train()</code> again
before classifying anything.
</div>

This is the single most surprising behaviour anywhere in this crate,
precisely because a caller would reasonably not expect it:
`MaxEntClassifier::restore` reads *only* the saved sample's elements,
revives each one through your `ElementClass` constructor, and calls
`Sample::generate_features` to regenerate the feature set from scratch — the
file's own `features`, `scaler`, and `p` (including the trained `alpha`) are
parsed and then simply never used.

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
    assert!(classifier.distribution().is_some());

    let json = classifier.to_json();
    let revived =
        MaxEntClassifier::restore(&json, |a, b| Rc::new(SEElement::new(a, b))).unwrap();

    // The sample round-trips and features regenerate from it...
    assert_eq!(revived.sample().borrow().size(), 10);
    // ...but alpha, the scaler and the distribution are gone: restore()
    // hands back an UNTRAINED classifier. You must call train() again.
    assert!(revived.distribution().is_none());
}
```

This is *not* how `Classifier<E>::restore` behaves for Bayes or logistic
regression — those genuinely restore the full trained engine, and
`revived.get_classifications(...)` works immediately with no retraining
step. A caller working across all three classifiers must not assume one
persistence contract covers all of them: verify which of the two shapes you
are dealing with, per classifier, rather than assuming.

## Common mistakes

**Calling `LogisticRegressionClassifier::train()` repeatedly, expecting
`BayesClassifier`-style incremental behaviour.** It resets and reruns
gradient descent over every stored document on every call — see the worked
comparison in
[`train()` vs `retrain()`](#train-vs-retrain), and the callout under
[Which classifier?](#which-classifier). This is exactly the "two
similarly-shaped APIs, very different cost" trap this site exists to warn
about.

**Adding a vocabulary token that collides with an integer-like string, and
being surprised a previously-trained model's feature indices shifted.** See
[Feature-vector key order is not insertion order](#_2-feature-vector-key-order-is-not-insertion-order)
for the full worked example — `classify()` can silently change its answer
for input that was never touched, because the *vocabulary*, not the
observation, is what moved.

**Expecting `MaxEntClassifier` scores to be probabilities.** They are
unnormalised weights that can exceed `1` and do not sum to `1` across a
context's classes — see
[MaxEnt's unnormalised weights](#_4-maxent-s-unnormalised-weights). Sort or
threshold on them for *relative* ranking only; never feed them into anything
expecting a probability distribution.

**Expecting `MaxEntClassifier::get_classifications` to come back sorted, the
way `Classifier<E>::get_classifications` does.** It returns scores in
class-insertion order; only `classify()` sorts. See
[`classify()` vs `get_classifications()`](#classify-vs-get-classifications).

**Passing a token slice and expecting the stemmer or stop-word list to still
apply.** `Observation::Tokens` is used **verbatim** — no lowercasing, no
stemming, no stop-word filtering, and `keep_stops` has no effect on it at
all. Only `Observation::Text` (a `&str`) goes through the stemmer:

```rust
use verbora_classifiers::BayesClassifier;

fn main() {
    let mut c = BayesClassifier::new();
    // "the" and "a" are stop words in string form, so this document
    // tokenises to nothing and is dropped in silence.
    c.add_document("the a", "dropped");
    assert_eq!(c.docs().len(), 0);

    // The exact same words, as a token slice, bypass the stemmer — and
    // therefore the stop-word list — entirely.
    c.add_document(&vec!["the".to_owned(), "a".to_owned()], "kept");
    assert_eq!(c.docs().len(), 1);
}
```

See [Ergonomics vs throughput](../performance/ergonomics-vs-throughput) for
the general shape of this trade-off: `Observation::Text` is the ergonomic
default that "just works" on raw strings; `Observation::Tokens` trades that
convenience for exact control over what a document's feature list contains.

**Calling `MaxEntClassifier::train()` again after the sample changed and
expecting a fresh correction feature.**

### The correction feature outlives its scaler

`GISScaler::add_correction_feature` builds a closure over its own scaler's
`C`/`featureSums` state and appends it to the **shared** `FeatureSet` under
the fixed name `"Correction feature"`. `FeatureSet::add_feature` rejects any
second feature with the same dedup key, so a **second** `train()` call — even
one that builds a brand-new `GISScaler` internally, as every call does —
cannot replace it. If the sample is unchanged between the two calls this is
invisible (verified above, under
[MaxEnt's own internal choice points](#maxent-s-own-internal-choice-points)):
the new scaler's `C`/`featureSums` happen to match the old ones exactly. If
you call `add_element` on the sample and then `train()` again, the
correction feature keeps evaluating against the **first** run's stale
`C`/`featureSums`, not the second run's — a genuine staleness quirk, pinned
by this crate's own tests, not an oversight.

## Performance characteristics

<div class="callout callout-note">
<strong>Not yet benchmarked.</strong> Unlike
<code>verbora-distance</code>, there is no recorded baseline for
this cluster in <code>benches/results/</code>. See
<a href="../benchmarks/index">Benchmarks</a> for what has and has not been
measured across the workspace, and reproduce the in-tree numbers yourself
with <code>cargo bench -p verbora-classifiers</code>.
</div>

One real, checkable correctness measurement *is* available without a
benchmark: `verbora_classifiers::transcendental` is pinned bit-exact across
95,039 recorded arguments (45,039 for `log`, 50,000 for `exp`), part of this
crate's own regression suite — see
[FDLIBM float precision](#_1-fdlibm-float-precision-inside-a-convergence-loop).
That is a correctness claim, not a timing one; do not read it as a
performance number.

Asymptotics, read from the source:

| Classifier | Per-document cost | Notes |
|---|---|---|
| `BayesClassifier::add_document` | amortised O(1) per token | one `OrderedMap` insert per distinct token |
| `BayesClassifier::train` | O(new docs × features per doc) | incremental — only documents past `lastAdded` |
| `Classifier::text_to_features` | O(\|features\| + \|observation\|) | a `HashSet` probe, rather than an O(\|features\| × \|observation\|) linear scan per feature |
| `LogisticRegressionClassifier::train` | O(iterations × classes × m × n) | gradient descent per class, `m` examples, `n` features; bounded by `max_it = 500 × m` per class, typically far fewer at convergence |
| `MaxEntClassifier::train` | O(iterations × features × distinct contexts × classes) | generalised iterative scaling |
| `Distribution::weight` | O(\|alpha\|) | one power-and-multiply operation per feature, per element scored |

`crates/verbora-classifiers/benches/classifiers.rs` is a Criterion suite
answering four separate questions, quoted from its own doc comment: where
Bayes and logistic-regression training time actually goes; what a single
classification costs, isolating `text_to_features` from `classify`; how
maximum-entropy training scales with corpus size; and what the
transcendental math primitives (`transcendental::log`/`exp`, `stable_stringify`)
cost, since both sit in every model's innermost loop. Its groups:
`bayes/train`, `logistic/train`, `bayes/predict` (`text_to_features` /
`get_classifications` / `classify`), `bayes/persist` (`to_json` /
`restore`), `maxent/train` (the `SimpleExample` sample, and part-of-speech
corpora at 1/2/4 sentences), `maxent/predict` (`classify` on a memoised vs.
an unseen context), and `reference_primitives` (`log`, `exp`, `sigmoid`,
`stable_stringify` on a POS context and on a plain string).

Every API in this crate is **eager** — there is no `Tokenize`-style lazy
iterator anywhere in `verbora-classifiers`, and no `_into`-shaped buffer-reuse
API either. See [Iterator vs `_into`](../performance/iterator-vs-into) for
what that trade-off looks like elsewhere in the workspace; nothing here has a
lazy or buffer-reusing counterpart to reach for. There is likewise no
batch/streaming choice for *training*: `train()` always processes its entire
backlog of untrained documents in one call — see
[Batch vs streaming](../performance/batch-vs-streaming) for the pattern this
crate does not offer a training variant of. *Classification* is the one
exception: `Classifier<E>::par_classify_batch`, behind the `parallel` Cargo
feature, is a genuine batch/parallel API over `classify` — see
[`classify()` vs `par_classify_batch()`](#classify-vs-par-classify-batch)
above, and [Parallelism](../performance/parallelism) for how it compares to
the workspace's other twelve `par_*` APIs. `MaxEntClassifier` has no
equivalent, deliberately — see the callout in that section.

## Allocation behaviour

**`Classifier<E>`.** `docs: Vec<Document>` owns one `Vec<String>` per stored
document (the tokens, already stemmed for string input); `features: OrderedMap<f64>`
holds one entry per distinct token, as an insertion-ordered `Vec` plus a
`HashMap` index.

**`BayesEngine`.** `class_features: OrderedMap<BTreeMap<u32, f64>>` — one
`BTreeMap` entry per **set** feature per class, sparse rather than dense.

**`LogisticEngine`.** `examples: OrderedMap<Vec<Vec<u8>>>` retains every training
document's 0/1 observation vector **after** training completes — it has to,
because `train()` rebuilds the whole matrix from scratch on every call (see
[Which classifier?](#which-classifier)). `theta: Option<Vec<Vec<f64>>>` is
one `Vec<f64>` per class, populated only once training succeeds.

**MaxEnt.** `FeatureSet` holds `Vec<Rc<Feature>>`; `Sample` holds
`Vec<Rc<Element>>` — one entry per element, **duplicates included**: a corpus
with 1,000 repeated observations allocates 1,000 `Rc<Element>` handles, not
one `Rc` with a count, because every summation in the algorithm depends on
walking that exact sequence (see
[Floating-point accumulation order](#_3-floating-point-accumulation-order-per-algorithm)).
`Context` and `Element` keys are lazily computed `String`s cached in a
`RefCell<Option<String>>` — computed at most once per object, but never
invalidated if the underlying data changes afterwards.

There is no `_into` variant and no caller-supplied output buffer anywhere in
this crate. See [Allocation](../performance/allocation).

## Unicode and language notes

- **Astral-plane tokens are ordinary feature keys.** They are compared for
  string equality and never indexed by code unit, so `"😀"` behaves exactly
  like any other token. `tests/edge_cases.rs` trains and classifies across
  ten Unicode categories — accented Latin, Cyrillic, Greek, CJK, astral,
  punctuation, digits, and combining characters — and every one round-trips
  through both `BayesClassifier` and `LogisticRegressionClassifier`, astral
  emoji tokens included.
- **MaxEnt context keys sort by UTF-16 code unit and emit non-ASCII raw** —
  see [Context keys sort by UTF-16 code unit](#_5-context-keys-sort-by-utf-16-code-unit).
  An astral character's context key sorts *before* `U+FFFD`, because its
  lead surrogate (`0xD83D`) is numerically below `0xFFFD`, which is the
  opposite of what comparing by Unicode scalar value would give.
- **String documents inherit the tokenizer/stemmer's own Unicode quirks.**
  `Observation::Text` goes through `verbora-stemmers`' `TokenizeAndStem`,
  so any language-specific tokenization behaviour documented on
  [Tokenizers](../features/tokenizers) applies transitively to every
  string-input document.

### Process-global stop words

<span class="badge badge-global">GLOBAL STATE</span>

The default stemmer (English Porter, used whenever you construct a
classifier with `new()` rather than `with_stemmer`) tests stop words with
`verbora_core::stopwords::is_default_stopword`, which is backed by a
process-wide `LazyLock<RwLock<StopWords>>` — the same shared state
[Core vocabulary](../features/core) describes for the rest of the workspace.

<div class="callout callout-warn">
<strong>Careful.</strong> Any <code>add_stop_word</code>/<code>remove_stop_word</code>
call anywhere in the process — including from an unrelated
<code>verbora-stemmers</code> or <code>verbora-phonetics</code> caller —
changes how every classifier in that process tokenises string documents from
that point on, retroactively affecting classifiers already constructed. A
classifier built with a token-slice API (<code>Observation::Tokens</code>)
never touches this state at all.
</div>

## Related

- [WordNet](../features/wordnet) — the site's other worked example of a
  synchronous, callback-free `save`/`load`-style persistence API.
- [Tokenizers](../features/tokenizers) — the tokenization/stemming pipeline
  every string-input document goes through before it becomes a feature
  vector.
- [Core vocabulary](../features/core) — the process-global stop-word list
  and the `is_whitespace` helper this crate's label trimming depends on.
- [Roadmap](../features/roadmap) and [Features overview](../features/index).
- [Choosing an API](../choosing/index) and
  [Decision trees](../choosing/decision-trees) — the general pattern behind
  [Which classifier?](#which-classifier) above.
- [Performance](../performance/index), and in detail:
  [Allocation](../performance/allocation),
  [Batch vs streaming](../performance/batch-vs-streaming),
  [Ergonomics vs throughput](../performance/ergonomics-vs-throughput),
  [Parallelism](../performance/parallelism) — the thirteen built-in `par_*`
  APIs across the workspace, including `par_classify_batch` and why
  `MaxEntClassifier` has no equivalent.
- [Benchmarks](../benchmarks/index).
- [Recipes](../recipes/index).

## API reference

Generate the rustdoc locally:

```bash
cargo doc -p verbora-classifiers --no-deps --open
```

Once published, the same content is at
<https://docs.rs/verbora-classifiers/latest/verbora_classifiers/>. The items
you will use most often:

| Item | Path |
|---|---|
| `BayesClassifier`, `BayesEngine` | `verbora_classifiers::{BayesClassifier, BayesEngine}` |
| `LogisticRegressionClassifier`, `LogisticEngine` | `verbora_classifiers::{LogisticRegressionClassifier, LogisticEngine}` |
| `Classifier<E>`, `Engine`, `Observation`, `Document`, `TrainingEvent`, `Classification`, `ClassifierError`, `LoadError` | `verbora_classifiers::{basic::*}` (also re-exported at the crate root) |
| `Classifier::par_classify_batch` (requires `parallel`) | same path as `Classifier<E>` above |
| `MaxEntClassifier`, `RestoreError` | `verbora_classifiers::{MaxEntClassifier, RestoreError}` |
| `Context`, `Element`, `GenerateFeatures` | `verbora_classifiers::{Context, Element, GenerateFeatures}` |
| `Feature`, `FeatureFn`, `FeatureSet` | `verbora_classifiers::{Feature, FeatureFn, FeatureSet}` |
| `Sample`, `Distribution`, `GISScaler`, `ScalerState` | `verbora_classifiers::{Sample, Distribution, GISScaler, ScalerState}` |
| `SEElement` | `verbora_classifiers::SEElement` |
| `POSElement`, `TaggedWord`, `MESentence`, `MECorpus` | `verbora_classifiers::{POSElement, TaggedWord, MESentence, MECorpus}` |
| `MaxEntError` | `verbora_classifiers::MaxEntError` |
| FDLIBM-based `log`/`exp`/`pow`/`sigmoid` | `verbora_classifiers::transcendental` |
| array-index-first, insertion-order map | `verbora_classifiers::OrderedMap` (also `verbora_classifiers::ordmap`) |
| JSON-like value, UTF-16-ordered stringify/parse | `verbora_classifiers::DynValue` (also `verbora_classifiers::dynval`) |
| Tokenize-and-stem adapter | `verbora_classifiers::{Stemmer, StemmerOf, default_stemmer}` |

Source: `crates/verbora-classifiers/src/`. Boundary-input suite:
`crates/verbora-classifiers/tests/edge_cases.rs`. Benchmarks:
`crates/verbora-classifiers/benches/classifiers.rs`.
