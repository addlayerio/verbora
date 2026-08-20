//! The maximum-entropy contract, asserted through the public API alone.
//!
//! Every expected value here comes from one of two places, and never from
//! recording what the implementation printed:
//!
//! * **arithmetic shown in the test**, derived from the update rule of Darroch
//!   & Ratcliff (1972) as Berger, Della Pietra & Della Pietra (1996) §6.1 give
//!   it for the conditional model; or
//! * **a property the published theory states** — that the fit satisfies every
//!   feature constraint, that the maximum-entropy distribution under a set of
//!   constraints is unique, that generalised iterative scaling never lowers the
//!   conditional log-likelihood, and that the answers are probabilities.
//!
//! The unit tests inside `src/maxent` cover the same ground against the
//! module's internals; these run against nothing but the crate root.

use verbora_classifiers::{Event, Gis, MaxEntClassifier, Sample, StopReason};

/// A deterministic pseudo-random source, so every run sees the same corpus.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) % n as u64) as usize
    }
}

/// Six events over two outcomes and two predicates, arranged so that neither
/// predicate determines its outcome: the constraints are satisfiable strictly
/// inside `(0, 1)` and the fit converges to finite weights.
fn balanced() -> Sample {
    let mut sample = Sample::new();
    for _ in 0..2 {
        sample.add("x", ["a"]);
    }
    sample.add("y", ["a"]);
    sample.add("x", ["b"]);
    for _ in 0..2 {
        sample.add("y", ["b"]);
    }
    sample
}

/// The empirical expectation of the feature `(predicate, outcome)`:
/// `(1/N) · |{ i : predicate ∈ xᵢ and yᵢ = outcome }|`.
fn empirical(sample: &Sample, predicate: &str, outcome: &str) -> f64 {
    sample
        .events()
        .iter()
        .filter(|e| e.outcome() == outcome && e.predicates().iter().any(|p| p == predicate))
        .count() as f64
        / sample.len() as f64
}

/// The model expectation of that feature under `classifier`:
/// `(1/N) · Σ_{ i : predicate ∈ xᵢ } p(outcome | xᵢ)`.
fn expectation(classifier: &MaxEntClassifier, predicate: &str, outcome: &str) -> f64 {
    let model = classifier.model().expect("trained");
    let slot = model
        .outcomes()
        .iter()
        .position(|o| o == outcome)
        .expect("a declared outcome");
    let sample = classifier.sample();
    sample
        .events()
        .iter()
        .filter(|e| e.predicates().iter().any(|p| p == predicate))
        .map(|e| model.distribution(e.predicates())[slot])
        .sum::<f64>()
        / sample.len() as f64
}

#[test]
fn the_first_iteration_is_the_published_gis_update() {
    // Four events, two outcomes, two predicates:
    //   x/{a}, x/{a}, y/{a}, y/{b}      N = 4, C = 1
    // Every weight starts at 0, so every context is uniform at p = 1/2, and
    //   Ẽ[(a,x)] = 2/4,  E[(a,x)] = (1/4)(1/2 + 1/2 + 1/2) = 3/8
    //   Ẽ[(a,y)] = 1/4,  E[(a,y)] = 3/8
    //   Ẽ[(b,y)] = 1/4,  E[(b,y)] = (1/4)(1/2)             = 1/8
    // One step of λ ← λ + log(Ẽ / E) / C therefore gives ln(4/3), ln(2/3), ln 2.
    let sample: Sample = [
        Event::new("x", ["a"]),
        Event::new("x", ["a"]),
        Event::new("y", ["a"]),
        Event::new("y", ["b"]),
    ]
    .into_iter()
    .collect();

    let mut classifier = MaxEntClassifier::from_sample(sample);
    let report = *classifier
        .train_with(Gis::new(1, 0.0).expect("a valid tolerance"))
        .expect("four events");
    assert_eq!(report.iterations, 1);
    assert_eq!(report.scaling_constant, 1.0);

    let model = classifier.model().expect("trained");
    for (predicate, outcome, want) in [
        ("a", "x", (4.0f64 / 3.0).ln()),
        ("a", "y", (2.0f64 / 3.0).ln()),
        ("b", "y", 2.0f64.ln()),
    ] {
        let got = model.weight(predicate, outcome).expect("a fitted feature");
        assert!(
            (got - want).abs() < 1e-12,
            "λ({predicate},{outcome}) = {got}, want {want}"
        );
    }
    assert_eq!(
        model.weight("b", "x"),
        None,
        "b never occurred with x, so it has no weight rather than a zero one"
    );

    // Those weights are p(x | {a}) = (4/3)/(4/3 + 2/3) = 2/3 and
    // p(x | {b}) = 1/(1 + 2) = 1/3.
    let a = model.distribution(["a"]);
    let b = model.distribution(["b"]);
    assert!((a[0] - 2.0 / 3.0).abs() < 1e-12, "{a:?}");
    assert!((b[0] - 1.0 / 3.0).abs() < 1e-12, "{b:?}");
}

#[test]
fn the_fit_satisfies_every_feature_constraint() {
    let mut classifier = MaxEntClassifier::from_sample(balanced());
    let report = *classifier
        .train_with(Gis::new(20_000, 0.0).expect("a valid tolerance"))
        .expect("six events");
    assert_eq!(report.stop, StopReason::Converged);

    for predicate in ["a", "b"] {
        for outcome in ["x", "y"] {
            let want = empirical(classifier.sample(), predicate, outcome);
            let got = expectation(&classifier, predicate, outcome);
            assert!(
                (want - got).abs() < 1e-8,
                "({predicate}, {outcome}): Ẽ = {want}, E = {got}"
            );
        }
    }

    // One predicate per context leaves the constraints no slack, so the fitted
    // conditional is the empirical one: two of the three `a` events were `x`.
    let a = classifier.model().expect("trained").distribution(["a"]);
    assert!((a[0] - 2.0 / 3.0).abs() < 1e-8, "{a:?}");
}

#[test]
fn a_perfectly_correlated_predicate_changes_no_probability() {
    // The maximum-entropy distribution is determined by the *constraints*, and
    // a predicate that fires exactly when another one does adds none. Copying
    // every predicate must therefore leave every probability alone — the point
    // of a maximum-entropy model over a naive-Bayes one, which would treat the
    // copy as independent evidence and double-count it.
    let plain = balanced();
    let doubled: Sample = plain
        .events()
        .iter()
        .map(|event| {
            let mut predicates: Vec<String> = event.predicates().to_vec();
            predicates.extend(event.predicates().iter().map(|p| format!("{p}-copy")));
            Event::new(event.outcome(), predicates)
        })
        .collect();

    let settings = Gis::new(20_000, 0.0).expect("a valid tolerance");
    let mut one = MaxEntClassifier::from_sample(plain);
    let mut two = MaxEntClassifier::from_sample(doubled);
    one.train_with(settings).expect("six events");
    two.train_with(settings).expect("six events");

    for predicate in ["a", "b"] {
        let from_one = one.model().expect("trained").distribution([predicate]);
        let from_two = two
            .model()
            .expect("trained")
            .distribution([predicate, &format!("{predicate}-copy")]);
        for (x, y) in from_one.iter().zip(&from_two) {
            assert!(
                (x - y).abs() < 1e-6,
                "{predicate}: {from_one:?} vs {from_two:?}"
            );
        }
    }
}

#[test]
fn more_iterations_never_lower_the_log_likelihood() {
    let sample = balanced();
    let mut previous = f64::NEG_INFINITY;
    for iterations in [0u32, 1, 2, 4, 8, 16, 64, 256] {
        let mut classifier = MaxEntClassifier::from_sample(sample.clone());
        classifier
            .train_with(Gis::new(iterations, 0.0).expect("a valid tolerance"))
            .expect("six events");
        let now = classifier
            .model()
            .expect("trained")
            .log_likelihood(&sample)
            .expect("every outcome is declared and every score is finite");
        assert!(now >= previous, "at {iterations}: {now} < {previous}");
        assert!(now <= 0.0, "a mean log probability cannot be positive");
        previous = now;
    }
}

#[test]
fn every_answer_is_a_probability_distribution() {
    let mut rng = Lcg(0x243F_6A88_85A3_08D3);
    for outcomes in [2usize, 3, 5] {
        let mut sample = Sample::new();
        for _ in 0..60 {
            let outcome = format!("o{}", rng.below(outcomes));
            let width = 1 + rng.below(4);
            let predicates: Vec<String> =
                (0..width).map(|_| format!("p{}", rng.below(12))).collect();
            sample.add(outcome, predicates);
        }
        let mut classifier = MaxEntClassifier::from_sample(sample.clone());
        classifier
            .train_with(Gis::new(60, 1e-9).expect("a valid tolerance"))
            .expect("sixty events");

        let contexts: Vec<Vec<String>> = sample
            .events()
            .iter()
            .map(|e| e.predicates().to_vec())
            .chain([vec!["never-seen".to_owned()], Vec::new()])
            .collect();
        for context in contexts {
            let p = classifier
                .model()
                .expect("trained")
                .distribution(context.iter());
            assert_eq!(p.len(), outcomes);
            for value in &p {
                assert!(
                    value.is_finite() && (0.0..=1.0).contains(value),
                    "{context:?} scored {p:?}"
                );
            }
            assert!(
                (p.iter().sum::<f64>() - 1.0).abs() < 1e-12,
                "{context:?} scored {p:?}"
            );
            // And the ranked view agrees with the raw one.
            let ranked = classifier
                .get_classifications(context.iter())
                .expect("trained");
            assert_eq!(ranked.len(), outcomes);
            for pair in ranked.windows(2) {
                assert!(pair[0].value >= pair[1].value, "{ranked:?}");
            }
            assert_eq!(
                classifier.classify(context.iter()).expect("trained"),
                ranked[0].label
            );
        }
    }
}

#[test]
fn a_fit_depends_on_the_sample_and_not_on_what_came_before_it() {
    let settings = Gis::new(200, 1e-9).expect("a valid tolerance");
    let mut fresh = MaxEntClassifier::from_sample(balanced());
    fresh.train_with(settings).expect("six events");

    let mut reused = MaxEntClassifier::from_sample(balanced());
    reused
        .train_with(Gis::new(3, 0.0).expect("a valid tolerance"))
        .expect("six events");
    reused.train_with(settings).expect("six events");

    for predicate in ["a", "b"] {
        for outcome in ["x", "y"] {
            assert_eq!(
                fresh
                    .model()
                    .expect("trained")
                    .weight(predicate, outcome)
                    .map(f64::to_bits),
                reused
                    .model()
                    .expect("trained")
                    .weight(predicate, outcome)
                    .map(f64::to_bits),
                "({predicate}, {outcome})"
            );
        }
    }
}

#[test]
fn a_saved_model_scores_identically_after_a_round_trip() {
    let mut classifier = MaxEntClassifier::from_sample(balanced());
    classifier
        .train_with(Gis::new(200, 1e-9).expect("a valid tolerance"))
        .expect("six events");
    let json = classifier.to_json();
    let revived = MaxEntClassifier::restore(&json).expect("what was just written parses");

    for context in [
        vec!["a"],
        vec!["b"],
        vec!["a", "b"],
        vec!["unknown"],
        vec![],
    ] {
        assert_eq!(
            revived
                .model()
                .expect("restored trained")
                .distribution(context.iter())
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            classifier
                .model()
                .expect("trained")
                .distribution(context.iter())
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "{context:?}"
        );
    }
    assert_eq!(revived.to_json(), json);
}

#[test]
fn a_model_with_no_constraints_is_uniform() {
    // No predicate ever occurs, so nothing constrains the distribution and the
    // maximum-entropy answer is the uniform one — reached in zero iterations,
    // because it is already the starting point.
    let mut classifier = MaxEntClassifier::new();
    for outcome in ["a", "b", "c", "d"] {
        classifier.add(outcome, Vec::<&str>::new());
    }
    let report = *classifier.train().expect("four events");
    assert_eq!(report.iterations, 0);
    assert_eq!(report.stop, StopReason::Converged);
    assert_eq!(
        classifier
            .model()
            .expect("trained")
            .distribution(Vec::<&str>::new()),
        vec![0.25; 4]
    );
    assert!((report.log_likelihood + 4.0f64.ln()).abs() < 1e-15);
}
