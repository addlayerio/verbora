use crate::basic::classifier::{
    Classification, Classifier, ClassifierError, Engine, sort_descending,
};
use crate::dynval::DynValue;
use crate::ordmap::OrderedMap;
use crate::transcendental;

/// A document classifier over one-vs-rest logistic regression.
///
/// ```
/// use verbora_classifiers::LogisticRegressionClassifier;
///
/// let mut c = LogisticRegressionClassifier::new();
/// c.add_document("i am long qqqq", "buy");
/// c.add_document("i am short qqqq", "sell");
/// c.train().unwrap();
/// assert_eq!(c.classify("i am short qqqq").unwrap(), "sell");
/// ```
pub type LogisticRegressionClassifier = Classifier<LogisticEngine>;

/// The logistic-regression engine: examples per class, and a theta per class.
/// The logistic-regression engine: one-vs-rest batch gradient descent.
///
/// This is the most numerically delicate code in the crate. `descendGradient`
/// runs one-vs-rest gradient descent per class, iterating until successive costs
/// differ by less than `1e-4`, so a one-ULP perturbation anywhere inside can
/// change the *iteration count* and therefore the whole model. Five things must
/// be reproduced literally:
///
/// 1. **Accumulation direction.** Every contraction sums descending over the
///    contracted index; the cost function sums ascending. An idiomatic
///    `iter().sum()` is wrong in one of the two places whichever way you write
///    it, and the difference reaches the model through the iteration count.
/// 2. **The update expression** is `theta[k] - (g[k] * (1/m)) * learningRate`,
///    in that parenthesisation. Folding `learningRate / m` into one constant
///    rounds differently.
/// 3. **`if (last)` is a truthiness test**, not a null check, so a cost of
///    exactly `0` disables the convergence check for that iteration and the loop
///    runs to `maxIt = 500 * m` and throws.
/// 4. **The intercept is trained and then discarded.** A ones column is
///    prepended, a zero appended to theta, and the optimised vector is returned
///    through `chomp(1)` — which drops the *first* element, the bias. Prediction
///    therefore applies no intercept at all.
/// 5. **A weight vector narrower than the observation is a stale model, and is
///    reported as one.** Adding a document after `train()` widens the feature
///    vocabulary without refitting, so the learned `theta` no longer describes
///    it. This used to answer `0.5` for every class — a sentinel that reads as
///    a real, perfectly balanced score and sorts among the others — and now
///    fails with [`ClassifierError::StaleModel`], which names both widths. Call
///    `train()` again.
///
/// Within those constraints the fit path is restructured for speed in three
/// ways, each bit-exact (differentially verified against the dense reference
/// form over thousands of randomized corpora — see `tests/train_parity.rs`):
///
/// * **The hypothesis is computed once per iteration, not twice.** The
///   reference evaluates `hypothesis(theta_new)` inside `cost()` and then again
///   at the top of the next iteration — the same pure function of the same
///   unchanged inputs. Keeping the cost pass's result halves the `exp` count.
///   The memo also survives a learning-rate restart, because `theta` does:
///   the restarted loop's opening hypothesis is for the very `theta` the last
///   cost pass evaluated.
/// * **The 0/1 design matrix is stored as sparse index lists** — for each
///   example the columns it has a 1 in, and for each column the examples that
///   have a 1 in it, both descending — rather than as a dense `Vec<Vec<f64>>`.
///   They are walked in the same descending order as the dense `while (k--)` /
///   `while (r--)` contractions. A skipped term is `0.0 * v = ±0.0`, which can
///   only affect a sum through the sign of zero; that never happens here
///   because `sigmoid` never returns `-0.0` (so no `diff` entry, and no
///   partial sum with nonzero terms remaining, sits at `-0.0`) and `theta`
///   stays finite (a diverging learning rate fails `current < last` within two
///   iterations, long before overflow could make a skipped term `0.0 * ±inf`).
/// * **The design and the scratch buffers are built once per `fit()`** and
///   shared across the one-vs-rest passes; only the target column changes
///   between classes.
///
#[derive(Debug, Clone, Default)]
pub struct LogisticEngine {
    /// Label -> observations. Enumerated in
    /// [`OrderedMap`](crate::OrderedMap) order when the training matrix is
    /// assembled, which is *not* the order the labels were first seen in.
    examples: OrderedMap<Vec<Vec<u8>>>,
    /// Labels in first-appearance order — the order `getClassifications` reads
    /// them back in. When it disagrees with the enumeration order above, the
    /// engine mislabels every class; see the module docs of [`OrderedMap`](crate::OrderedMap).
    classifications: Vec<String>,
    example_count: usize,
    /// `None` until `train()`; asking for classifications before then is
    /// [`ClassifierError::NotFitted`].
    theta: Option<Vec<Vec<f64>>>,
}

impl LogisticEngine {
    /// The learned parameter vectors, one per class, or `None` before training.
    pub fn theta(&self) -> Option<&[Vec<f64>]> {
        self.theta.as_deref()
    }

    /// Class labels in first-appearance order.
    pub fn classifications(&self) -> &[String] {
        &self.classifications
    }

    /// Total number of recorded examples.
    pub fn example_count(&self) -> usize {
        self.example_count
    }

    /// Recorded examples per class.
    pub fn examples(&self) -> &OrderedMap<Vec<Vec<u8>>> {
        &self.examples
    }
}

/// `Matrix.One(m, 1).augment(Examples)`, stored as sparse index lists.
///
/// Every entry of the augmented matrix is 0 or 1, so a dot product against it
/// is a sum of the other operand's entries at the 1-positions. Both list
/// families are kept **descending** so that walking one accumulates in exactly
/// the order the dense `while (k--)` / `while (r--)` loops did — the module
/// docs explain why dropping the zero terms is bit-exact.
struct SparseDesign {
    /// Example count `m`.
    m: usize,
    /// `theta` length: one weight per feature, plus the intercept column.
    n1: usize,
    /// Per example: positions of its 1-entries in the augmented row,
    /// descending, ending at 0 — the always-on intercept column, which sits in
    /// front of the features and is therefore the *last* index a descending
    /// walk visits.
    rows_desc: Vec<Vec<u32>>,
    /// Per column: the examples with a 1 in that column, descending, matching
    /// the `while (r--)` contraction of `Examplesᵀ · (h − y)`. Column 0 is the
    /// intercept and lists every example.
    cols_desc: Vec<Vec<u32>>,
}

impl SparseDesign {
    /// Builds both index families in one pass over the recorded observations.
    ///
    /// `rows` must be non-empty and rectangular — `fit()` guarantees both,
    /// because a single `train()` call snapshots the vocabulary once and
    /// rebuilds the engine from scratch.
    fn build(rows: &[&Vec<u8>]) -> Self {
        let m = rows.len();
        let width = rows[0].len();
        let n1 = width + 1;
        let mut rows_desc = Vec::with_capacity(m);
        let mut cols_desc: Vec<Vec<u32>> = vec![Vec::new(); n1];
        for (r, row) in rows.iter().enumerate() {
            let mut nz: Vec<u32> = Vec::new();
            for (k, &cell) in row.iter().enumerate().rev() {
                if cell != 0 {
                    nz.push(k as u32 + 1);
                }
            }
            nz.push(0);
            rows_desc.push(nz);
            for (k, &cell) in row.iter().enumerate() {
                if cell != 0 {
                    cols_desc[k + 1].push(r as u32);
                }
            }
            cols_desc[0].push(r as u32);
        }
        // The rows were visited ascending to fill the columns; the contraction
        // wants them descending.
        for col in &mut cols_desc {
            col.reverse();
        }
        Self {
            m,
            n1,
            rows_desc,
            cols_desc,
        }
    }

    /// `Observations.x(theta).map(sigmoid)`, written into `h`.
    ///
    /// The dense inner product runs `while (k--)`; walking the descending
    /// index list adds the same nonzero terms in the same order.
    fn hypothesis_into(&self, theta: &[f64], h: &mut [f64]) {
        for (r, nz) in self.rows_desc.iter().enumerate() {
            let mut sum = 0.0;
            for &k in nz {
                sum += theta[k as usize];
            }
            h[r] = transcendental::sigmoid(sum);
        }
    }
}

/// `descendGradient(theta, Examples, classifications)`.
///
/// Returns the optimised parameters with the intercept already chopped off.
/// `h`, `diff` and `gradient` are caller-owned scratch buffers so the
/// one-vs-rest passes share their allocations; their contents on entry are
/// irrelevant.
fn descend_gradient(
    x: &SparseDesign,
    y: &[f64],
    h: &mut Vec<f64>,
    diff: &mut Vec<f64>,
    gradient: &mut Vec<f64>,
) -> Result<Vec<f64>, ClassifierError> {
    let m = x.m;
    let n1 = x.n1;
    let max_it = 500 * m;

    // A zero is appended to the all-zero init while the ones column goes in
    // *front*, which is why dropping index 0 at the end removes the bias rather
    // than the last weight.
    let mut theta = vec![0.0f64; n1];
    h.clear();
    h.resize(m, 0.0);
    diff.clear();
    diff.resize(m, 0.0);
    gradient.clear();
    gradient.resize(n1, 0.0);

    // Hoisting `1/m` is exact: the update is `(g[k] * (1/m)) * rate` with `1/m`
    // as its own division, so one shared quotient is bit-for-bit the same value.
    let inv_m = 1.0 / m as f64;
    let mut learning_rate = 3.0f64;
    let mut learning_rate_found = false;
    // Whether `h` already holds the hypothesis for the current `theta`. The
    // cost pass below evaluates `hypothesis(theta_new)`, and the next
    // iteration's opening hypothesis is the same pure function of the same
    // unchanged `theta` — including across a learning-rate restart.
    let mut have_h = false;

    while !learning_rate_found && learning_rate != 0.0 {
        let mut i = 0usize;
        // `last` is `null` on entry and tested with `if (last)`. Both `null` and
        // a cost of exactly `0` are falsy, so one variable models both.
        let mut last = 0.0f64;
        loop {
            if !have_h {
                x.hypothesis_into(&theta, h);
            }
            for k in 0..m {
                diff[k] = h[k] - y[k];
            }
            // Examplesᵀ · (h - y), contracted descending over the row index.
            for (col, g) in gradient.iter_mut().enumerate() {
                let mut sum = 0.0;
                for &r in &x.cols_desc[col] {
                    sum += diff[r as usize];
                }
                *g = sum;
            }
            for k in 0..n1 {
                theta[k] -= (gradient[k] * inv_m) * learning_rate;
            }
            // The cost function:
            // `(1/m) * Σ_asc [ (0 - y[k]) * log(h[k]) - (1 - y[k]) * log(1 - h[k]) ]`,
            // summed **ascending**, unlike every dot product here, which
            // contracts descending. Its hypothesis is for the just-updated
            // `theta`, so it is kept for reuse.
            x.hypothesis_into(&theta, h);
            have_h = true;
            let mut sum = 0.0;
            for k in 0..m {
                let cost_1 = (0.0 - y[k]) * transcendental::log(h[k]);
                let cost_0 = (1.0 - y[k]) * transcendental::log(1.0 - h[k]);
                sum += cost_1 - cost_0;
            }
            let current = inv_m * sum;
            i += 1;

            if last != 0.0 && !last.is_nan() {
                if current < last {
                    learning_rate_found = true;
                } else {
                    break;
                }
                if last - current < 0.0001 {
                    break;
                }
            }
            if i >= max_it {
                return Err(ClassifierError::NoMinimum);
            }
            last = current;
        }
        learning_rate /= 3.0;
    }

    // `theta.chomp(1)` — drops the FIRST element, which is the intercept.
    theta.remove(0);
    Ok(theta)
}

impl Engine for LogisticEngine {
    /// Logistic regression rebuilds from scratch on every `train()`: the
    /// matrices have to have consistent widths, so the engine is replaced and
    /// every document re-added — and every `trainedWithDocument` re-emitted.
    const RESETS_ON_TRAIN: bool = true;

    fn add_example(&mut self, observation: &[u8], label: &str) {
        if !self.examples.contains_key(label) {
            self.examples.insert(label, Vec::new());
            self.classifications.push(label.to_owned());
        }
        if let Some(rows) = self.examples.get_mut(label) {
            rows.push(observation.to_vec());
        }
        self.example_count += 1;
    }

    fn fit(&mut self) -> Result<(), ClassifierError> {
        let num_classes = self.examples.len();
        // createClassifications(): exampleCount rows of numClasses zeros.
        let mut targets = vec![vec![0.0f64; num_classes]; self.example_count];
        let mut rows: Vec<&Vec<u8>> = Vec::with_capacity(self.example_count);

        let mut d = 0usize;
        // `for (var classification in this.examples)` — enumeration order, which
        // is where the label/theta misalignment comes from.
        for (c, label) in self.examples.enumeration_order().into_iter().enumerate() {
            for row in self.examples.get(label).expect("key came from this map") {
                rows.push(row);
                targets[d][c] = 1.0;
                d += 1;
            }
        }

        if rows.is_empty() {
            // `$M([])` dereferences `elements[0][0]`.
            return Err(ClassifierError::NoExamples);
        }

        // One design and one set of scratch buffers for every one-vs-rest
        // pass; only the target column differs between classes.
        let design = SparseDesign::build(&rows);
        let mut h = Vec::new();
        let mut diff = Vec::new();
        let mut gradient = Vec::new();
        let mut theta = Vec::with_capacity(self.classifications.len());
        for i in 0..self.classifications.len() {
            let column: Vec<f64> = targets.iter().map(|row| row[i]).collect();
            theta.push(descend_gradient(
                &design,
                &column,
                &mut h,
                &mut diff,
                &mut gradient,
            )?);
        }
        self.theta = Some(theta);
        Ok(())
    }

    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError> {
        let Some(theta) = &self.theta else {
            return Err(ClassifierError::NotFitted);
        };
        let mut out: Vec<Classification> = theta
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if observation.len() != t.len() {
                    return Err(ClassifierError::StaleModel {
                        weights: t.len(),
                        observation: observation.len(),
                    });
                }
                let mut sum = 0.0;
                let mut k = t.len();
                while k > 0 {
                    k -= 1;
                    sum += f64::from(observation[k]) * t[k];
                }
                Ok(Classification {
                    label: self.classifications.get(i).cloned().unwrap_or_default(),
                    value: transcendental::sigmoid(sum),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        sort_descending(&mut out);
        Ok(out)
    }

    fn to_value(&self) -> DynValue {
        let mut fields = vec![
            (
                "examples".to_owned(),
                DynValue::Obj(
                    self.examples
                        .ordered_entries()
                        .into_iter()
                        .map(|(label, rows)| {
                            (
                                label.to_owned(),
                                DynValue::Arr(
                                    rows.iter()
                                        .map(|r| {
                                            DynValue::Arr(
                                                r.iter()
                                                    .map(|&b| DynValue::Num(f64::from(b)))
                                                    .collect(),
                                            )
                                        })
                                        .collect(),
                                ),
                            )
                        })
                        .collect(),
                ),
            ),
            // Three fields the engine writes once and never reads again. They
            // are dead code, but they are in the saved bytes.
            ("features".to_owned(), DynValue::Arr(vec![])),
            ("featurePositions".to_owned(), DynValue::Obj(vec![])),
            ("maxFeaturePosition".to_owned(), DynValue::Num(0.0)),
            (
                "classifications".to_owned(),
                DynValue::Arr(
                    self.classifications
                        .iter()
                        .cloned()
                        .map(DynValue::Str)
                        .collect(),
                ),
            ),
            (
                "exampleCount".to_owned(),
                DynValue::Num(self.example_count as f64),
            ),
        ];
        if let Some(theta) = &self.theta {
            fields.push((
                "theta".to_owned(),
                DynValue::Arr(
                    theta
                        .iter()
                        .map(|t| {
                            // A weight vector serialises as {"elements": [...]}.
                            DynValue::Obj(vec![(
                                "elements".to_owned(),
                                DynValue::Arr(t.iter().copied().map(DynValue::Num).collect()),
                            )])
                        })
                        .collect(),
                ),
            ));
        }
        DynValue::Obj(fields)
    }

    fn from_value(value: &DynValue) -> Self {
        let mut out = Self::default();
        if let Some(DynValue::Obj(classes)) = value.get("examples") {
            for (label, rows) in classes {
                let mut observations = Vec::new();
                if let DynValue::Arr(items) = rows {
                    for item in items {
                        if let DynValue::Arr(cells) = item {
                            observations.push(
                                cells
                                    .iter()
                                    .map(|c| u8::from(matches!(c, DynValue::Num(n) if *n != 0.0)))
                                    .collect(),
                            );
                        }
                    }
                }
                out.examples.insert(label.clone(), observations);
            }
        }
        if let Some(DynValue::Arr(labels)) = value.get("classifications") {
            out.classifications = labels.iter().map(DynValue::to_text).collect();
        }
        if let Some(DynValue::Num(n)) = value.get("exampleCount") {
            out.example_count = *n as usize;
        }
        if let Some(DynValue::Arr(vectors)) = value.get("theta") {
            out.theta = Some(
                vectors
                    .iter()
                    .map(|v| match v.get("elements") {
                        Some(DynValue::Arr(cells)) => cells
                            .iter()
                            .map(|c| if let DynValue::Num(n) = c { *n } else { 0.0 })
                            .collect(),
                        _ => Vec::new(),
                    })
                    .collect(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contradictory_examples_converge_to_a_zero_intercept_only_model() {
        // Two identical feature vectors with different labels. The only fit is
        // the bias, which is then discarded, so every score is exactly 0.5.
        let mut c = LogisticRegressionClassifier::new();
        let alpha = vec!["alpha".to_owned()];
        c.add_document(&alpha, "p");
        c.add_document(&alpha, "q");
        c.train().unwrap();
        assert_eq!(c.engine().theta().unwrap(), &[vec![0.0], vec![0.0]]);
        let scores = c.get_classifications(&alpha).unwrap();
        assert_eq!(scores[0].value, 0.5);
        assert_eq!(scores[1].value, 0.5);
        // Stable sort keeps first-appearance order among the tie.
        assert_eq!(c.classify(&alpha).unwrap(), "p");
    }

    /// A model whose fit predates the current vocabulary is a failure, not a
    /// score.
    ///
    /// It used to answer `0.5` for every class — a sentinel indistinguishable
    /// from a genuine, perfectly balanced score, which then sorted among the
    /// real ones and produced a confident label.
    #[test]
    fn a_stale_model_reports_both_widths_instead_of_scoring() {
        let mut c = LogisticRegressionClassifier::new();
        c.add_document(&vec!["alpha".to_owned()], "p");
        c.add_document(&vec!["beta".to_owned()], "q");
        c.train().unwrap();
        // A new token widens the feature vector without retraining, so the
        // fitted weights no longer describe an observation built from it.
        c.add_document(&vec!["gamma".to_owned()], "r");
        assert_eq!(
            c.get_classifications(&vec!["alpha".to_owned()]),
            Err(ClassifierError::StaleModel {
                weights: 2,
                observation: 3,
            })
        );
        assert_eq!(
            c.classify(&vec!["alpha".to_owned()]),
            Err(ClassifierError::StaleModel {
                weights: 2,
                observation: 3,
            })
        );
        // …and training again makes it answerable.
        c.train().unwrap();
        assert!(c.classify(&vec!["alpha".to_owned()]).is_ok());
    }

    #[test]
    fn asking_an_unfitted_model_is_a_distinct_failure() {
        let c = LogisticRegressionClassifier::new();
        assert_eq!(
            c.get_classifications("anything"),
            Err(ClassifierError::NotFitted)
        );
    }

    #[test]
    fn training_with_no_documents_fails_like_an_empty_matrix() {
        let mut c = LogisticRegressionClassifier::new();
        assert_eq!(c.train(), Err(ClassifierError::NoExamples));
    }

    #[test]
    fn training_twice_re_adds_every_document() {
        let mut c = LogisticRegressionClassifier::new();
        c.add_document(&vec!["alpha".to_owned()], "p");
        c.add_document(&vec!["beta".to_owned()], "q");
        let mut events = Vec::new();
        c.train_with(|e| events.push(e)).unwrap();
        c.train_with(|e| events.push(e)).unwrap();
        // Four TrainedWithDocument events for two documents.
        assert_eq!(events.len(), 6);
        assert_eq!(c.engine().example_count(), 2);
    }

    #[test]
    fn integer_like_labels_are_mislabelled_exactly_as_in_the_reference() {
        // `classifications` is ['2','1'] but the theta columns are built in
        // enumeration order ['1','2'], so the labels are swapped.
        let mut c = LogisticRegressionClassifier::new();
        c.add_document("i have a computer", "2");
        c.add_document("my laptop is fast", "1");
        c.train().unwrap();
        assert_eq!(c.engine().classifications(), ["2", "1"]);
        assert_eq!(c.engine().examples().enumeration_order(), vec!["1", "2"]);
    }
}
