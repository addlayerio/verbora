//! The `apparatus` logistic-regression engine, and the `sylvester` linear
//! algebra it is built on.
//!
//! This is the most numerically delicate code in the crate. `descendGradient`
//! runs one-vs-rest gradient descent per class, iterating until successive costs
//! differ by less than `1e-4`, so a one-ULP perturbation anywhere inside can
//! change the *iteration count* and therefore the whole model. Five things must
//! be reproduced literally:
//!
//! 1. **Accumulation direction.** Every `sylvester` contraction sums descending
//!    over the contracted index (`while (k--)`), while `Vector.sum` — used only
//!    by the cost function — sums ascending. An idiomatic `iter().sum()` is
//!    wrong in one of the two places whichever way you write it.
//! 2. **The update expression** is `theta[k] - (g[k] * (1/m)) * learningRate`,
//!    in that parenthesisation. Folding `learningRate / m` into one constant
//!    rounds differently.
//! 3. **`if (last)` is a truthiness test**, not a null check, so a cost of
//!    exactly `0` disables the convergence check for that iteration and the loop
//!    runs to `maxIt = 500 * m` and throws.
//! 4. **The intercept is trained and then discarded.** A ones column is
//!    prepended, a zero appended to theta, and the optimised vector is returned
//!    through `chomp(1)` — which drops the *first* element, the bias. Prediction
//!    therefore applies no intercept at all.
//! 5. **`Vector.dot` returns `null` on a length mismatch**, and
//!    `sigmoid(null)` is exactly `0.5`. A model left stale by a post-training
//!    `addDocument` silently reports 0.5 for every class instead of failing.

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
#[derive(Debug, Clone, Default)]
pub struct LogisticEngine {
    /// Label -> observations. Enumerated with the reference object semantics when
    /// the training matrix is assembled, which is *not* the order the labels
    /// were first seen in.
    examples: OrderedMap<Vec<Vec<u8>>>,
    /// Labels in first-appearance order — the order `getClassifications` reads
    /// them back in. When it disagrees with the enumeration order above, the
    /// engine mislabels every class; see the module docs of [`crate::ordmap`].
    classifications: Vec<String>,
    example_count: usize,
    /// `undefined` until `train()`; asking for classifications before then is a
    /// `TypeError` in the reference.
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

/// `Observations.x(theta).map(sigmoid)`.
///
/// The inner product runs `while (k--)`, so each row is accumulated from the
/// last feature back to the first.
fn hypothesis(theta: &[f64], examples: &[Vec<f64>]) -> Vec<f64> {
    examples
        .iter()
        .map(|row| {
            let mut sum = 0.0;
            let mut k = row.len();
            while k > 0 {
                k -= 1;
                sum += row[k] * theta[k];
            }
            transcendental::sigmoid(sum)
        })
        .collect()
}

/// The apparatus cost function.
///
/// `(1 / m) * Σ_asc [ (0 - y[k]) * log(h[k]) - (1 - y[k]) * log(1 - h[k]) ]`,
/// with the sum running **ascending** — `Vector.sum` maps over the elements in
/// order, unlike every dot product in the same file.
fn cost(theta: &[f64], examples: &[Vec<f64>], y: &[f64]) -> f64 {
    let h = hypothesis(theta, examples);
    let m = examples.len();
    let mut sum = 0.0;
    for k in 0..m {
        let cost_1 = (0.0 - y[k]) * transcendental::log(h[k]);
        let cost_0 = (1.0 - y[k]) * transcendental::log(1.0 - h[k]);
        sum += cost_1 - cost_0;
    }
    (1.0 / m as f64) * sum
}

/// `descendGradient(theta, Examples, classifications)`.
///
/// Returns the optimised parameters with the intercept already chopped off.
fn descend_gradient(
    theta_init: &[f64],
    examples: &[Vec<f64>],
    y: &[f64],
) -> Result<Vec<f64>, ClassifierError> {
    let m = examples.len();
    let max_it = 500 * m;

    // `Matrix.One(m, 1).augment(Examples)`: a ones column in front.
    let x: Vec<Vec<f64>> = examples
        .iter()
        .map(|row| {
            let mut r = Vec::with_capacity(row.len() + 1);
            r.push(1.0);
            r.extend_from_slice(row);
            r
        })
        .collect();
    // `theta.augment([0])`: a zero at the end. Both are all-zero at this point,
    // so the asymmetry with the prepended ones column is harmless — but it is
    // why `chomp(1)` drops the bias rather than the last weight.
    let mut theta: Vec<f64> = theta_init.to_vec();
    theta.push(0.0);
    let n1 = theta.len();

    let mut learning_rate = 3.0f64;
    let mut learning_rate_found = false;
    let mut diff = vec![0.0; m];
    let mut gradient = vec![0.0; n1];

    while !learning_rate_found && learning_rate != 0.0 {
        let mut i = 0usize;
        // `last` is `null` on entry and tested with `if (last)`. Both `null` and
        // a cost of exactly `0` are falsy, so one variable models both.
        let mut last = 0.0f64;
        loop {
            let h = hypothesis(&theta, &x);
            for k in 0..m {
                diff[k] = h[k] - y[k];
            }
            // Examplesᵀ · (h - y), contracted descending over the row index.
            for (col, g) in gradient.iter_mut().enumerate() {
                let mut sum = 0.0;
                let mut r = m;
                while r > 0 {
                    r -= 1;
                    sum += x[r][col] * diff[r];
                }
                *g = sum;
            }
            for k in 0..n1 {
                theta[k] -= (gradient[k] * (1.0 / m as f64)) * learning_rate;
            }
            let current = cost(&theta, &x, y);
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
                return Err(ClassifierError::UnableToFindMinimum);
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
        let mut matrix: Vec<Vec<f64>> = Vec::with_capacity(self.example_count);

        let mut d = 0usize;
        // `for (var classification in this.examples)` — enumeration order, which
        // is where the label/theta misalignment comes from.
        for (c, label) in self.examples.enumeration_order().into_iter().enumerate() {
            let rows = self.examples.get(label).expect("key came from this map");
            for row in rows {
                matrix.push(row.iter().map(|&b| f64::from(b)).collect());
                targets[d][c] = 1.0;
                d += 1;
            }
        }

        if matrix.is_empty() {
            // `$M([])` dereferences `elements[0][0]`.
            return Err(ClassifierError::NoExamples);
        }

        let width = matrix[0].len();
        let zeros = vec![0.0f64; width];
        let mut theta = Vec::with_capacity(self.classifications.len());
        for i in 0..self.classifications.len() {
            let column: Vec<f64> = targets.iter().map(|row| row[i]).collect();
            theta.push(descend_gradient(&zeros, &matrix, &column)?);
        }
        self.theta = Some(theta);
        Ok(())
    }

    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError> {
        let Some(theta) = &self.theta else {
            return Err(ClassifierError::LogisticRegressionNotTrained);
        };
        let mut out: Vec<Classification> = theta
            .iter()
            .enumerate()
            .map(|(i, t)| {
                // `Vector.dot` returns null when the dimensions disagree, and
                // `sigmoid(null)` is `1 / (1 + Math.exp(0))` — exactly 0.5.
                let value = if observation.len() == t.len() {
                    let mut sum = 0.0;
                    let mut k = t.len();
                    while k > 0 {
                        k -= 1;
                        sum += f64::from(observation[k]) * t[k];
                    }
                    transcendental::sigmoid(sum)
                } else {
                    0.5
                };
                Classification {
                    label: self.classifications.get(i).cloned().unwrap_or_default(),
                    value,
                }
            })
            .collect();
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
                            // sylvester Vectors serialise as {"elements": [...]}.
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

    #[test]
    fn a_stale_model_scores_every_class_at_exactly_one_half() {
        let mut c = LogisticRegressionClassifier::new();
        c.add_document(&vec!["alpha".to_owned()], "p");
        c.add_document(&vec!["beta".to_owned()], "q");
        c.train().unwrap();
        // A new token widens the feature vector without retraining, so
        // `Vector.dot` sees mismatched lengths and returns null.
        c.add_document(&vec!["gamma".to_owned()], "r");
        for score in c.get_classifications(&vec!["alpha".to_owned()]).unwrap() {
            assert_eq!(score.value, 0.5);
        }
    }

    #[test]
    fn untrained_reports_the_typeerror() {
        let c = LogisticRegressionClassifier::new();
        assert_eq!(
            c.get_classifications("anything"),
            Err(ClassifierError::LogisticRegressionNotTrained)
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
