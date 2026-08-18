//! Training-side model math for `HashedLinearDetector`.
//!
//! Everything numeric here must match the inference code in
//! `crates/verbora-language/src/hashed_linear.rs` exactly. Two mechanisms
//! enforce that:
//!
//! 1. **Feature extraction is not reimplemented.** [`featurize`] calls
//!    `verbora_language::train_support::hashed_features` — the same
//!    function inference runs — so the train-time and inference-time
//!    feature transforms are one piece of code, equal by construction.
//!    `tests/tokenizer_differential.rs` additionally pins that function
//!    against an independent reference implementation.
//! 2. **Scoring is duplicated but pinned.** Training needs to score with
//!    weights that do not exist as compiled statics yet, so [`Model::score`]
//!    reimplements the inference formula (`sum(w[bucket]) / sqrt(n) +
//!    intercept`); `tests/scoring_parity.rs` asserts the duplicate agrees
//!    with the real `HashedLinearDetector` end to end on real sentences
//!    once the weights are compiled in.
//!
//! Determinism: every routine is single-threaded, iterates in fixed
//! order, and draws randomness only from [`XorShift64`] with a seed
//! recorded in the training manifest — re-running the pipeline on the
//! same corpus reproduces byte-identical generated weights.

use verbora_language::train_support::{DIMENSION, hashed_features, hashed_features_cyrillic};

/// One prepared sample: the raw hashed-bucket stream (duplicates kept —
/// inference accumulates per occurrence, so training must too).
pub struct Sample {
    /// Feature buckets in emission order, each `< DIMENSION`.
    pub buckets: Vec<u16>,
}

/// Extracts a Latin-model training sample from `text` using the
/// *inference* crate's own feature extractor (see the module docs for why
/// that matters).
#[must_use]
pub fn featurize(text: &str) -> Sample {
    let mut buckets = Vec::new();
    hashed_features(text, |b| {
        buckets.push(u16::try_from(b).expect("bucket < DIMENSION < u16::MAX"));
    });
    Sample { buckets }
}

/// Extracts a Cyrillic-model training sample — same contract as
/// [`featurize`], via the inference crate's Cyrillic feature set (see
/// `hashed_features_cyrillic`'s doc comment in `verbora-language` for why
/// that model has its own).
#[must_use]
pub fn featurize_cyrillic(text: &str) -> Sample {
    let mut buckets = Vec::new();
    hashed_features_cyrillic(text, |b| {
        buckets.push(u16::try_from(b).expect("bucket < DIMENSION < u16::MAX"));
    });
    Sample { buckets }
}

/// A trained (or in-training) linear model over `n_classes` languages.
pub struct Model {
    /// Interleaved `[DIMENSION * n_classes]`: `weights[bucket * n + class]`,
    /// the exact memory layout the generated `hashed_linear_weights.rs`
    /// ships.
    pub weights: Vec<f32>,
    /// One intercept per class.
    pub intercepts: Vec<f32>,
    /// Class count (16 for Latin, 2 for Cyrillic).
    pub n_classes: usize,
}

impl Model {
    /// A zero-initialized model.
    #[must_use]
    pub fn new(n_classes: usize) -> Self {
        Self {
            weights: vec![0.0; DIMENSION * n_classes],
            intercepts: vec![0.0; n_classes],
            n_classes,
        }
    }

    /// Scores every class for `sample` — the same formula inference uses:
    /// accumulate interleaved weights per bucket *occurrence*, scale by
    /// `1/sqrt(feature_count)`, add intercepts.
    #[must_use]
    pub fn score(&self, sample: &Sample) -> Vec<f32> {
        let n = self.n_classes;
        let mut scores = vec![0.0f32; n];
        for &bucket in &sample.buckets {
            let row = &self.weights[bucket as usize * n..bucket as usize * n + n];
            for (s, &w) in scores.iter_mut().zip(row) {
                *s += w;
            }
        }
        if !sample.buckets.is_empty() {
            let inv = 1.0f32 / (sample.buckets.len() as f32).sqrt();
            for (s, &b) in scores.iter_mut().zip(&self.intercepts) {
                *s = *s * inv + b;
            }
        }
        scores
    }

    /// `(argmax class, margin over runner-up)` with the inference tie
    /// rule (first index wins ties). `None` for a featureless sample.
    #[must_use]
    pub fn predict(&self, sample: &Sample) -> Option<(usize, f32)> {
        if sample.buckets.is_empty() {
            return None;
        }
        let scores = self.score(sample);
        let mut best = 0usize;
        let mut second = 1usize;
        for i in 1..self.n_classes {
            if scores[i] > scores[best] {
                second = best;
                best = i;
            } else if scores[i] > scores[second] || second == best {
                second = i;
            }
        }
        Some((best, (scores[best] - scores[second]).max(0.0)))
    }
}

/// Deterministic xorshift64* PRNG — no external RNG dependency, and the
/// sequence is part of the reproducibility contract (seed goes in the
/// manifest).
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// Seeds the generator; `seed` must be non-zero.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        assert_ne!(seed, 0, "xorshift64 cannot be seeded with 0");
        Self { state: seed }
    }

    /// Next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform value in `0..bound`.
    pub fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Hyperparameters for [`train`], recorded verbatim in the manifest.
pub struct Hyperparams {
    /// Full passes of balanced sampling.
    pub epochs: usize,
    /// SGD draws per class per epoch (balanced sampling with replacement:
    /// thin classes are oversampled to the same draw count as rich ones,
    /// which is what equalizes class priors despite corpus imbalance).
    pub samples_per_class: usize,
    /// Initial learning rate.
    pub lr0: f32,
    /// Per-epoch multiplicative learning-rate decay.
    pub lr_decay: f32,
    /// PRNG seed.
    pub seed: u64,
}

/// Trains multinomial logistic regression by SGD with balanced
/// per-class sampling. `class_samples[c]` holds class `c`'s training
/// sentences; the class index directly becomes the weight-table column,
/// so callers must pass classes in the inference class-array order.
#[must_use]
pub fn train(class_samples: &[Vec<Sample>], hp: &Hyperparams) -> Model {
    let n = class_samples.len();
    let mut model = Model::new(n);
    let mut rng = XorShift64::new(hp.seed);
    let mut lr = hp.lr0;
    let mut probs = vec![0.0f32; n];
    for _epoch in 0..hp.epochs {
        for step in 0..hp.samples_per_class * n {
            let class = step % n;
            let samples = &class_samples[class];
            let sample = &samples[rng.next_below(samples.len())];
            if sample.buckets.is_empty() {
                continue;
            }
            // Forward: scores -> softmax (max-subtracted for stability).
            let scores = model.score(sample);
            let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mut sum = 0.0f32;
            for (p, &s) in probs.iter_mut().zip(&scores) {
                *p = (s - max).exp();
                sum += *p;
            }
            for p in &mut probs {
                *p /= sum;
            }
            // Backward: d(loss)/d(score_c) = p_c - y_c. Each bucket
            // occurrence contributed `inv` to the input, so its weight
            // gradient is `inv * (p_c - y_c)`.
            let inv = 1.0f32 / (sample.buckets.len() as f32).sqrt();
            for (c, p) in probs.iter().enumerate() {
                let grad = p - f32::from(c == class);
                model.intercepts[c] -= lr * grad;
            }
            for &bucket in &sample.buckets {
                let row = &mut model.weights[bucket as usize * n..bucket as usize * n + n];
                for (w, (c, p)) in row.iter_mut().zip(probs.iter().enumerate()) {
                    let grad = p - f32::from(c == class);
                    *w -= lr * inv * grad;
                }
            }
        }
        lr *= hp.lr_decay;
    }
    model
}

/// Held-out evaluation of one model: per-class accuracy plus the margin
/// distributions the abstention calibration needs.
pub struct HeldOutReport {
    /// `correct[c] / total[c]` per class.
    pub per_class_accuracy: Vec<(usize, usize)>,
    /// Margins of correctly classified samples, sorted ascending.
    pub correct_margins: Vec<f32>,
    /// Margins of misclassified samples, sorted ascending.
    pub incorrect_margins: Vec<f32>,
}

/// Runs `model` over held-out samples per class.
#[must_use]
pub fn evaluate_heldout(model: &Model, class_samples: &[Vec<Sample>]) -> HeldOutReport {
    let mut per_class_accuracy = Vec::with_capacity(class_samples.len());
    let mut correct_margins = Vec::new();
    let mut incorrect_margins = Vec::new();
    for (class, samples) in class_samples.iter().enumerate() {
        let mut correct = 0usize;
        for sample in samples {
            let Some((predicted, margin)) = model.predict(sample) else {
                continue;
            };
            if predicted == class {
                correct += 1;
                correct_margins.push(margin);
            } else {
                incorrect_margins.push(margin);
            }
        }
        per_class_accuracy.push((correct, samples.len()));
    }
    correct_margins.sort_by(f32::total_cmp);
    incorrect_margins.sort_by(f32::total_cmp);
    HeldOutReport {
        per_class_accuracy,
        correct_margins,
        incorrect_margins,
    }
}

/// Calibrates the abstention margin from held-out margins: the largest
/// threshold that abstains on at most 5% of *correct* held-out
/// predictions, capped by the median margin of *incorrect* ones (so
/// abstention removes roughly the worse half of errors, never a
/// meaningful share of right answers). Falls back to 0.0 (never abstain
/// on margin) when there are no errors to calibrate against.
#[must_use]
pub fn calibrate_abstain_margin(report: &HeldOutReport) -> f32 {
    if report.incorrect_margins.is_empty() || report.correct_margins.is_empty() {
        return 0.0;
    }
    let correct_p05 = report.correct_margins[report.correct_margins.len() / 20];
    let incorrect_median = report.incorrect_margins[report.incorrect_margins.len() / 2];
    incorrect_median.min(correct_p05)
}
