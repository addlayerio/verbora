//! Generalised iterative scaling: settings, the fitting loop, and its report.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::maxent::MaxEntError;
use crate::maxent::model::{MaxEntModel, log_probability, normalise};
use crate::maxent::sample::Sample;
use crate::transcendental::log;

/// How generalised iterative scaling should be run.
///
/// ```
/// use verbora_classifiers::Gis;
///
/// // The default: at most 100 iterations, stopping once one of them raises the
/// // conditional log-likelihood by 1e-6 or less.
/// assert_eq!(Gis::default().max_iterations, 100);
/// assert_eq!(Gis::default().tolerance, 1e-6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gis {
    /// The greatest number of iterations to perform.
    ///
    /// An exact count, not a lower bound: `0` performs none and leaves the
    /// uniform model, and `n` performs at most `n`. Reaching it is reported as
    /// [`StopReason::MaxIterations`], which means the fit was cut short rather
    /// than finished.
    pub max_iterations: u32,
    /// The least increase in mean conditional log-likelihood worth another
    /// iteration.
    ///
    /// Generalised iterative scaling never decreases that quantity, so this is a
    /// one-sided test: fitting stops as soon as an iteration gains this much or
    /// less. Must be finite and non-negative; anything else is
    /// [`MaxEntError::InvalidTolerance`]. A tolerance of exactly `0` stops only
    /// on an iteration that gains nothing at all.
    pub tolerance: f64,
}

impl Default for Gis {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1e-6,
        }
    }
}

impl Gis {
    /// Settings, rejecting a tolerance that could never be met.
    ///
    /// The same check runs again at fitting time, because the fields are public
    /// and can be written after construction; this constructor exists so the
    /// mistake is reported where it was made.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::InvalidTolerance`] when `tolerance` is not finite and
    /// non-negative.
    pub fn new(max_iterations: u32, tolerance: f64) -> Result<Self, MaxEntError> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(MaxEntError::InvalidTolerance(tolerance));
        }
        Ok(Self {
            max_iterations,
            tolerance,
        })
    }
}

/// Why fitting stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// An iteration raised the log-likelihood by at most [`Gis::tolerance`].
    /// The fit is as good as the settings asked for.
    Converged,
    /// [`Gis::max_iterations`] iterations ran without the tolerance being met.
    /// The parameters are the best found so far, not a converged fit.
    ///
    /// One shape of sample reports this forever, and it is a property of the
    /// data rather than of the fit: a predicate observed with **exactly one**
    /// outcome sets a constraint of `p = 1`, whose maximum-likelihood weight is
    /// unbounded. GIS approaches it only logarithmically — the weight grows
    /// like `log(iterations)` — so no finite [`Gis::max_iterations`] reaches a
    /// tolerance for it, even though every iteration up to that point is a
    /// perfectly usable fit. A caller who needs convergence reported adds
    /// events so the predicate is no longer deterministic.
    MaxIterations,
    /// An iteration would have produced a parameter that is not a finite
    /// number, so it was discarded and the previous parameters kept.
    ///
    /// Unreachable for a sample of ordinary size — the update is a logarithm of
    /// a ratio of two strictly positive expectations, and both stay positive at
    /// finite parameters. It exists so that "no `NaN` escapes" is enforced by
    /// the code rather than only argued for.
    NumericalLimit,
}

/// What one fit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrainingReport {
    /// How many iterations ran.
    pub iterations: u32,
    /// The mean conditional log-likelihood `(1/N) Σᵢ log p(yᵢ | xᵢ)` of the
    /// training sample under the fitted parameters. Always finite and at most
    /// `0`; `-ln |Y|` before any iteration has run.
    pub log_likelihood: f64,
    /// How much the last iteration raised [`Self::log_likelihood`]. `0` when no
    /// iteration ran.
    pub improvement: f64,
    /// Why fitting stopped.
    pub stop: StopReason,
    /// `C`, the scaling constant: the greatest number of features active on any
    /// `(training context, outcome)` pair. `0` when the sample has no
    /// predicates at all, in which case no iteration is needed because the
    /// uniform model already is the maximum-entropy one.
    pub scaling_constant: f64,
}

/// The sample's features, laid out as a compressed sparse row over predicates.
struct Index {
    outcomes: Vec<String>,
    predicates: Vec<Arc<str>>,
    row_start: Vec<u32>,
    feature_outcome: Vec<u32>,
    /// `Ẽ[fⱼ]`, the empirical expectation of each feature.
    empirical: Vec<f64>,
    /// Each event's predicate rows, flattened: event `i` owns
    /// `event_start[i] .. event_start[i + 1]`.
    event_row: Vec<u32>,
    event_start: Vec<u32>,
    /// Each event's outcome index.
    event_outcome: Vec<u32>,
}

impl Index {
    /// Builds the feature index in first-appearance order.
    fn build(sample: &Sample) -> Self {
        let outcomes = sample.outcomes().to_vec();
        let outcome_of: FxHashMap<&str, u32> = outcomes
            .iter()
            .enumerate()
            .map(|(i, o)| (o.as_str(), i as u32))
            .collect();

        let mut predicates: Vec<Arc<str>> = Vec::new();
        let mut row_of: FxHashMap<&str, usize> = FxHashMap::default();
        let mut rows: Vec<Vec<(u32, f64)>> = Vec::new();
        let mut event_row: Vec<u32> = Vec::new();
        let mut event_start: Vec<u32> = Vec::with_capacity(sample.len() + 1);
        let mut event_outcome: Vec<u32> = Vec::with_capacity(sample.len());
        event_start.push(0);

        for event in sample.events() {
            let outcome = outcome_of[event.outcome()];
            event_outcome.push(outcome);
            for predicate in event.predicates() {
                let row = *row_of.entry(predicate.as_str()).or_insert_with(|| {
                    predicates.push(Arc::from(predicate.as_str()));
                    rows.push(Vec::new());
                    predicates.len() - 1
                });
                event_row.push(row as u32);
                match rows[row].iter_mut().find(|(o, _)| *o == outcome) {
                    Some((_, count)) => *count += 1.0,
                    None => rows[row].push((outcome, 1.0)),
                }
            }
            event_start.push(event_row.len() as u32);
        }

        let features: usize = rows.iter().map(Vec::len).sum();
        let mut row_start = Vec::with_capacity(rows.len() + 1);
        let mut feature_outcome = Vec::with_capacity(features);
        let mut empirical = Vec::with_capacity(features);
        let n = sample.len() as f64;
        row_start.push(0);
        for row in &rows {
            for &(outcome, count) in row {
                feature_outcome.push(outcome);
                empirical.push(count / n);
            }
            row_start.push(feature_outcome.len() as u32);
        }

        Self {
            outcomes,
            predicates,
            row_start,
            feature_outcome,
            empirical,
            event_row,
            event_start,
            event_outcome,
        }
    }

    fn features(&self) -> usize {
        self.empirical.len()
    }

    fn events(&self) -> usize {
        self.event_outcome.len()
    }

    fn range(&self, row: u32) -> std::ops::Range<usize> {
        self.row_start[row as usize] as usize..self.row_start[row as usize + 1] as usize
    }

    fn rows_of(&self, event: usize) -> &[u32] {
        &self.event_row[self.event_start[event] as usize..self.event_start[event + 1] as usize]
    }

    /// `C`: the greatest feature activation over the `(context, outcome)` grid
    /// the objective evaluates.
    fn scaling_constant(&self) -> u32 {
        let mut active = vec![0u32; self.outcomes.len()];
        let mut c = 0;
        for event in 0..self.events() {
            active.fill(0);
            for &row in self.rows_of(event) {
                for k in self.range(row) {
                    active[self.feature_outcome[k] as usize] += 1;
                }
            }
            c = c.max(active.iter().copied().max().unwrap_or(0));
        }
        c
    }

    /// One sweep at `lambda`: fills `expect` with `E[fⱼ]` and returns the mean
    /// conditional log-likelihood.
    ///
    /// Both accumulate over events in insertion order, and within an event over
    /// each predicate's features in row order. `None` when a context's scores
    /// are not all finite, which leaves the log-likelihood undefined.
    fn sweep(&self, lambda: &[f64], expect: &mut [f64], scratch: &mut Sweep) -> Option<f64> {
        expect.fill(0.0);
        let mut total = 0.0;
        for event in 0..self.events() {
            scratch.scores.clear();
            scratch.scores.resize(self.outcomes.len(), 0.0);
            for &row in self.rows_of(event) {
                for k in self.range(row) {
                    scratch.scores[self.feature_outcome[k] as usize] += lambda[k];
                }
            }
            total += log_probability(&scratch.scores, self.event_outcome[event] as usize)?;
            scratch.posterior.clear();
            scratch.posterior.extend_from_slice(&scratch.scores);
            normalise(&mut scratch.posterior);
            for &row in self.rows_of(event) {
                for k in self.range(row) {
                    expect[k] += scratch.posterior[self.feature_outcome[k] as usize];
                }
            }
        }
        let n = self.events() as f64;
        for e in expect.iter_mut() {
            *e /= n;
        }
        Some(total / n)
    }
}

/// Per-sweep scratch, reused across iterations.
#[derive(Default)]
struct Sweep {
    scores: Vec<f64>,
    posterior: Vec<f64>,
}

/// One GIS step: `λⱼ ← λⱼ + log(Ẽ[fⱼ] / E[fⱼ]) / C`.
///
/// Written into `next` first and copied over only if every parameter is finite,
/// so a step that cannot be taken leaves `lambda` exactly as it was rather than
/// half-updated. Returns whether the step was taken.
fn step(lambda: &mut [f64], empirical: &[f64], expect: &[f64], c: f64, next: &mut [f64]) -> bool {
    for k in 0..lambda.len() {
        let ratio = empirical[k] / expect[k];
        if !ratio.is_finite() || ratio <= 0.0 {
            return false;
        }
        let updated = lambda[k] + log(ratio) / c;
        if !updated.is_finite() {
            return false;
        }
        next[k] = updated;
    }
    lambda.copy_from_slice(next);
    true
}

/// Fits a model to `sample` by generalised iterative scaling.
///
/// # Errors
///
/// [`MaxEntError::NoEvents`] for an empty sample and
/// [`MaxEntError::InvalidTolerance`] for a tolerance outside its domain.
pub(crate) fn fit(
    sample: &Sample,
    settings: Gis,
) -> Result<(MaxEntModel, TrainingReport), MaxEntError> {
    if sample.is_empty() {
        return Err(MaxEntError::NoEvents);
    }
    if !settings.tolerance.is_finite() || settings.tolerance < 0.0 {
        return Err(MaxEntError::InvalidTolerance(settings.tolerance));
    }

    let index = Index::build(sample);
    let uniform = -log(index.outcomes.len() as f64);
    if index.features() == 0 {
        // No predicate ever occurred, so there is no constraint to satisfy and
        // the uniform distribution already is the maximum-entropy one.
        let model =
            MaxEntModel::assemble(index.outcomes, Vec::new(), vec![0], Vec::new(), Vec::new());
        return Ok((
            model,
            TrainingReport {
                iterations: 0,
                log_likelihood: uniform,
                improvement: 0.0,
                stop: StopReason::Converged,
                scaling_constant: 0.0,
            },
        ));
    }

    let c = f64::from(index.scaling_constant());
    let mut lambda = vec![0.0; index.features()];
    let mut previous = lambda.clone();
    let mut next = lambda.clone();
    let mut expect = vec![0.0; index.features()];
    let mut scratch = Sweep::default();

    let mut iterations = 0u32;
    let mut improvement = 0.0;
    let mut stop = StopReason::MaxIterations;
    // At `λ = 0` every score is zero, so this is `-ln |Y|` and always finite.
    let mut log_likelihood = index
        .sweep(&lambda, &mut expect, &mut scratch)
        .unwrap_or(uniform);

    while iterations < settings.max_iterations {
        previous.copy_from_slice(&lambda);
        if !step(&mut lambda, &index.empirical, &expect, c, &mut next) {
            lambda.copy_from_slice(&previous);
            stop = StopReason::NumericalLimit;
            break;
        }
        let Some(next_log_likelihood) = index.sweep(&lambda, &mut expect, &mut scratch) else {
            lambda.copy_from_slice(&previous);
            stop = StopReason::NumericalLimit;
            break;
        };
        iterations += 1;
        improvement = next_log_likelihood - log_likelihood;
        log_likelihood = next_log_likelihood;
        if improvement <= settings.tolerance {
            stop = StopReason::Converged;
            break;
        }
    }

    let model = MaxEntModel::assemble(
        index.outcomes,
        index.predicates,
        index.row_start,
        index.feature_outcome,
        lambda,
    );
    Ok((
        model,
        TrainingReport {
            iterations,
            log_likelihood,
            improvement,
            stop,
            scaling_constant: c,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example the module documentation derives by hand:
    /// four events, two outcomes, two predicates.
    fn worked() -> Sample {
        let mut sample = Sample::new();
        sample.add("x", ["a"]);
        sample.add("x", ["a"]);
        sample.add("y", ["a"]);
        sample.add("y", ["b"]);
        sample
    }

    #[test]
    fn the_first_iteration_matches_the_published_update_computed_by_hand() {
        // N = 4, C = 1. At λ = 0 every context is uniform, p = 1/2.
        //   Ẽ[(a,x)] = 2/4,  E[(a,x)] = (1/4)(1/2 + 1/2 + 1/2) = 3/8
        //   Ẽ[(a,y)] = 1/4,  E[(a,y)] = 3/8
        //   Ẽ[(b,y)] = 1/4,  E[(b,y)] = (1/4)(1/2)             = 1/8
        // λ ← λ + log(Ẽ/E) / C, so the three weights are ln(4/3), ln(2/3), ln 2.
        let (model, report) = fit(&worked(), Gis::new(1, 0.0).unwrap()).unwrap();
        assert_eq!(report.iterations, 1);
        assert_eq!(report.scaling_constant, 1.0);
        for (predicate, outcome, want) in [
            ("a", "x", (4.0f64 / 3.0).ln()),
            ("a", "y", (2.0f64 / 3.0).ln()),
            ("b", "y", 2.0f64.ln()),
        ] {
            let got = model.weight(predicate, outcome).unwrap();
            assert!(
                (got - want).abs() < 1e-12,
                "λ({predicate},{outcome}) = {got}, want {want}"
            );
        }
        assert_eq!(model.weight("b", "x"), None, "b never occurred with x");

        // Those weights give p(x | {a}) = (4/3) / (4/3 + 2/3) = 2/3, and
        // p(x | {b}) = 1 / (1 + 2) = 1/3.
        let p = model.distribution(["a"]);
        assert!((p[0] - 2.0 / 3.0).abs() < 1e-12, "{p:?}");
        let p = model.distribution(["b"]);
        assert!((p[0] - 1.0 / 3.0).abs() < 1e-12, "{p:?}");
    }

    #[test]
    fn no_iterations_leaves_the_uniform_model() {
        let (model, report) = fit(&worked(), Gis::new(0, 0.0).unwrap()).unwrap();
        assert_eq!(report.iterations, 0);
        assert_eq!(report.stop, StopReason::MaxIterations);
        assert_eq!(report.improvement, 0.0);
        // -ln 2 for two outcomes.
        assert!((report.log_likelihood + 2.0f64.ln()).abs() < 1e-12);
        assert_eq!(model.distribution(["a"]), vec![0.5, 0.5]);
        assert!(model.predicates().len() == 2 && model.feature_count() == 3);
    }

    #[test]
    fn the_log_likelihood_never_decreases() {
        let sample = worked();
        let mut previous = f64::NEG_INFINITY;
        for iterations in 0..12 {
            let (_, report) = fit(&sample, Gis::new(iterations, 0.0).unwrap()).unwrap();
            assert!(
                report.log_likelihood >= previous,
                "iteration {iterations}: {} < {previous}",
                report.log_likelihood
            );
            previous = report.log_likelihood;
        }
    }

    /// The defining property of the maximum-entropy solution (Berger et al.
    /// §4): at the fitted parameters the model expectation of every feature
    /// equals its empirical expectation.
    ///
    /// Both expectations are recomputed here from their definitions against the
    /// model's public API, rather than read out of the fitting code, so the
    /// assertion is a statement about the published property and not a
    /// restatement of how [`fit`] happens to accumulate.
    #[test]
    fn constraints_are_satisfied_at_convergence() {
        let mut sample = Sample::new();
        for _ in 0..2 {
            sample.add("x", ["a", "shared"]);
        }
        sample.add("y", ["a"]);
        sample.add("x", ["b"]);
        for _ in 0..2 {
            sample.add("y", ["b", "shared"]);
        }

        let (model, report) = fit(&sample, Gis::new(20_000, 0.0).unwrap()).unwrap();
        assert_eq!(report.stop, StopReason::Converged);

        let n = sample.len() as f64;
        for (o, outcome) in model.outcomes().iter().enumerate() {
            for predicate in ["a", "b", "shared"] {
                //  Ẽ[f] = (1/N) · |{ i : predicate ∈ xᵢ and yᵢ = outcome }|
                let empirical = sample
                    .events()
                    .iter()
                    .filter(|e| {
                        e.outcome() == outcome && e.predicates().iter().any(|p| p == predicate)
                    })
                    .count() as f64
                    / n;
                //  E[f] = (1/N) · Σ_{ i : predicate ∈ xᵢ } p(outcome | xᵢ)
                let expectation = sample
                    .events()
                    .iter()
                    .filter(|e| e.predicates().iter().any(|p| p == predicate))
                    .map(|e| model.distribution(e.predicates())[o])
                    .sum::<f64>()
                    / n;
                assert!(
                    (empirical - expectation).abs() < 1e-8,
                    "({predicate}, {outcome}): Ẽ = {empirical}, E = {expectation}"
                );
            }
        }
    }

    /// With one predicate per context, the maximum-entropy fit reproduces the
    /// empirical conditional distribution exactly: the constraints leave no
    /// freedom.
    #[test]
    fn a_single_predicate_context_recovers_the_empirical_conditional() {
        let mut sample = Sample::new();
        for _ in 0..2 {
            sample.add("x", ["a"]);
        }
        sample.add("y", ["a"]);
        sample.add("x", ["b"]);
        for _ in 0..2 {
            sample.add("y", ["b"]);
        }
        let (model, report) = fit(&sample, Gis::new(5_000, 0.0).unwrap()).unwrap();
        assert_eq!(report.stop, StopReason::Converged);
        let a = model.distribution(["a"]);
        let b = model.distribution(["b"]);
        assert!((a[0] - 2.0 / 3.0).abs() < 1e-8, "{a:?}");
        assert!((b[0] - 1.0 / 3.0).abs() < 1e-8, "{b:?}");
    }

    #[test]
    fn a_sample_with_no_predicates_is_already_maximum_entropy() {
        let mut sample = Sample::new();
        sample.add("x", Vec::<&str>::new());
        sample.add("y", Vec::<&str>::new());
        sample.add("y", Vec::<&str>::new());
        let (model, report) = fit(&sample, Gis::default()).unwrap();
        assert_eq!(report.iterations, 0);
        assert_eq!(report.stop, StopReason::Converged);
        assert_eq!(report.scaling_constant, 0.0);
        assert_eq!(model.feature_count(), 0);
        assert_eq!(model.distribution(Vec::<&str>::new()), vec![0.5, 0.5]);
        assert!((report.log_likelihood + 2.0f64.ln()).abs() < 1e-15);
    }

    #[test]
    fn a_single_outcome_is_predicted_with_certainty() {
        let mut sample = Sample::new();
        sample.add("only", ["p"]);
        let (model, report) = fit(&sample, Gis::default()).unwrap();
        assert_eq!(model.distribution(["p"]), vec![1.0]);
        assert_eq!(report.log_likelihood, 0.0, "log 1 is 0");
    }

    #[test]
    fn an_empty_sample_and_an_impossible_tolerance_are_refused() {
        assert_eq!(
            fit(&Sample::new(), Gis::default()),
            Err(MaxEntError::NoEvents)
        );
        let settings = Gis {
            max_iterations: 1,
            tolerance: f64::NAN,
        };
        assert!(matches!(
            fit(&worked(), settings),
            Err(MaxEntError::InvalidTolerance(t)) if t.is_nan()
        ));
        assert_eq!(Gis::new(1, -1.0), Err(MaxEntError::InvalidTolerance(-1.0)));
    }

    #[test]
    fn a_fit_is_bit_identical_for_the_same_events_in_the_same_order() {
        let first = fit(&worked(), Gis::default()).unwrap().0;
        let second = fit(&worked(), Gis::default()).unwrap().0;
        for predicate in ["a", "b"] {
            for outcome in ["x", "y"] {
                assert_eq!(
                    first.weight(predicate, outcome).map(f64::to_bits),
                    second.weight(predicate, outcome).map(f64::to_bits)
                );
            }
        }
    }

    /// The guard that makes [`StopReason::NumericalLimit`] real rather than
    /// decorative. It is unreachable through the public API — the update is a
    /// logarithm of a ratio of two positive expectations — so the decision
    /// function is exercised directly.
    #[test]
    fn a_step_that_would_leave_the_finite_reals_is_refused_whole() {
        let mut lambda = vec![1.0, 2.0];
        let mut next = vec![0.0, 0.0];
        assert!(step(&mut lambda, &[1.0, 1.0], &[1.0, 1.0], 1.0, &mut next));
        assert_eq!(lambda, vec![1.0, 2.0], "log 1 is 0");

        // A model expectation that underflowed to zero: the second parameter
        // would be infinite, so neither is written.
        let mut lambda = vec![1.0, 2.0];
        assert!(!step(&mut lambda, &[1.0, 1.0], &[1.0, 0.0], 1.0, &mut next));
        assert_eq!(lambda, vec![1.0, 2.0], "no half-applied step");

        let mut lambda = vec![f64::MAX];
        assert!(!step(
            &mut lambda,
            &[1.0],
            &[f64::MIN_POSITIVE],
            1e-320,
            &mut next[..1]
        ));
        assert_eq!(lambda, vec![f64::MAX]);
    }
}
