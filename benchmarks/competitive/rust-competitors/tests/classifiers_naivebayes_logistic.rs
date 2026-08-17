//! CORRECTNESS BEFORE PERFORMANCE for `benches/classifiers.rs`'s two new
//! competitor families: `naivebayes` (ruivieira) for Naive Bayes, and
//! smartcore/linfa-logistic/rustlearn for Logistic Regression.
//!
//! Real, lexically-meaningful text, not the bench file's shape-only `corpus`
//! generator — the point here is agreement on a classification *decision* on
//! an unambiguous case, not a timing shape (`tests/classifiers_accuracy.rs`
//! already covers real-signal accuracy for the pre-existing Bayes rows over
//! `benches/data/classification-corpus.json`).
//!
//! A `tests/*.rs` binary cannot import private items from a `[[bench]]`
//! target (`harness = false` leaves no libtest runner in that binary to
//! collect from, and its items are not `pub` besides), so the small `Vocab`/
//! label helpers below are a deliberate, minimal duplicate of
//! `benches/classifiers.rs`'s own — same reason `tests/classifiers_accuracy.rs`
//! already duplicates them, see that file's own doc comment.

use std::collections::HashMap;

use naivebayes::NaiveBayes;
use ndarray::Array2;
use smartcore::linalg::basic::matrix::DenseMatrix;
use smartcore::linear::logistic_regression::LogisticRegression;
use verbora_classifiers::LogisticRegressionClassifier;

/// Whitespace-split, lowercased tokens — identical to `benches/
/// classifiers.rs`'s own `tokenize`, duplicated for the same reason `Vocab`
/// below is.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// Byte-for-byte identical to `benches/classifiers.rs`'s own `Vocab`.
struct Vocab {
    index: HashMap<String, usize>,
}

impl Vocab {
    fn build(docs: &[(&str, &str)]) -> Self {
        let mut index = HashMap::new();
        for (text, _) in docs {
            for tok in text.split_whitespace() {
                let next_id = index.len();
                index.entry(tok.to_lowercase()).or_insert(next_id);
            }
        }
        Self { index }
    }

    fn len(&self) -> usize {
        self.index.len()
    }

    fn row(&self, text: &str) -> Vec<u32> {
        let mut row = vec![0u32; self.index.len()];
        for tok in text.split_whitespace() {
            if let Some(&id) = self.index.get(&tok.to_lowercase()) {
                row[id] += 1;
            }
        }
        row
    }

    fn matrix(&self, docs: &[(&str, &str)]) -> Vec<Vec<u32>> {
        docs.iter().map(|(t, _)| self.row(t)).collect()
    }
}

/// A small, two-class corpus with deliberately non-overlapping vocabularies,
/// so every algorithm below — regardless of smoothing/optimizer mechanics —
/// agrees on the obvious answer. Mirrors the shape of
/// `crates/verbora-classifiers/src/basic/logistic.rs`'s own doctest example
/// (`"i am long qqqq"` -> `"buy"`), scaled up to a few documents per class so
/// gradient descent has enough signal to separate the classes.
const TRAIN: &[(&str, &str)] = &[
    ("rust cargo compiler borrow checker memory safety", "tech"),
    ("cargo crate compiler rust ownership lifetimes", "tech"),
    ("compiler rust memory safety borrow checker cargo", "tech"),
    ("stew tomato onion garlic simmer basil oregano", "cooking"),
    ("garlic onion basil oregano simmer tomato sauce", "cooking"),
    ("simmer basil tomato sauce garlic pepper onion", "cooking"),
];

/// Held out entirely from [`TRAIN`], built only from `tech`-class
/// vocabulary — an unambiguous case every algorithm here should agree on.
const PROBE_TECH: &str = "rust compiler memory borrow safety cargo";

fn label_ids(docs: &[(&str, &str)]) -> Vec<usize> {
    docs.iter()
        .map(|(_, l)| if *l == "tech" { 0usize } else { 1usize })
        .collect()
}

fn label_ids_u32(docs: &[(&str, &str)]) -> Vec<u32> {
    label_ids(docs).into_iter().map(|v| v as u32).collect()
}

fn label_name(id: usize) -> &'static str {
    if id == 0 { "tech" } else { "cooking" }
}

/// `naivebayes` (ruivieira)'s own smoothing is NOT count-based additive
/// smoothing (matrix note, `benches/classifiers.rs`'s own doc comment): an
/// attribute unseen under a given label, but seen under some *other* label,
/// gets a FIXED `minimum_probability = 1e-9` (`naivebayes-0.1.2/src/lib.rs`'s
/// `calculate_attr_prob`'s `(None, true) => Some(self.minimum_probability)`
/// arm) — never shrinking as that label's own document count grows, unlike
/// Verbora's `(count + smoothing) / (total + smoothing * |V|)`.
///
/// Demonstrated concretely, not just asserted from reading the source: two
/// models differing only in how many "sell"-labelled documents exist (1 vs.
/// 1000, all containing "python", never "rust") are classified on a
/// "rust"-only input. Dividing out each model's own `prior("sell")` isolates
/// the per-attribute probability factor for `"rust"` under `"sell"` — it is
/// exactly `1e-9` in both models, regardless of the 1000x difference in
/// corpus size for that label.
#[test]
fn naivebayes_smoothing_floor_is_fixed_not_count_based() {
    let rust = vec!["rust".to_string()];
    let python = vec!["python".to_string()];
    let buy = "buy".to_string();
    let sell = "sell".to_string();

    let mut small = NaiveBayes::new();
    small.train(&rust, &buy);
    small.train(&python, &sell);

    let mut large = NaiveBayes::new();
    large.train(&rust, &buy);
    for _ in 0..1000 {
        large.train(&python, &sell);
    }

    let small_scores = small.classify(&rust);
    let large_scores = large.classify(&rust);

    let small_sell = *small_scores
        .get("sell")
        .expect("\"sell\" was trained, must have a score");
    let large_sell = *large_scores
        .get("sell")
        .expect("\"sell\" was trained, must have a score");

    // prior(sell) = doc_count(sell) / total_docs: 1/2 for `small`, 1000/1001
    // for `large` — genuinely different between the two models.
    let small_prior = 1.0 / 2.0_f64;
    let large_prior = 1000.0 / 1001.0_f64;

    let small_floor_factor = small_sell / small_prior;
    let large_floor_factor = large_sell / large_prior;

    assert!(
        (small_floor_factor - 1e-9).abs() < 1e-15,
        "small model's isolated attribute-probability factor for an \
         unseen-under-label attribute should be exactly the fixed floor \
         1e-9, got {small_floor_factor}"
    );
    assert!(
        (large_floor_factor - 1e-9).abs() < 1e-15,
        "large model's isolated attribute-probability factor should STILL \
         be exactly 1e-9 despite 1000x more \"sell\" documents -- a count- \
         based additive-smoothing implementation would have shrunk this; \
         naivebayes' fixed floor does not, got {large_floor_factor}"
    );
}

/// On an unambiguous, non-overlapping-vocabulary case, `naivebayes`'
/// `classify` argmax agrees with Verbora's `BayesClassifier::classify` —
/// establishing the narrow domain (clear-cut cases) where the two algorithms'
/// different smoothing mechanics do not change the *decision*, even though
/// (per the test above) they can change the *magnitude*.
#[test]
fn naivebayes_agrees_with_verbora_on_a_clear_cut_case() {
    let mut verbora = verbora_classifiers::BayesClassifier::new();
    for (text, label) in TRAIN {
        verbora.add_document(*text, label);
    }
    verbora.train().expect("Bayes training cannot fail");
    let verbora_pred = verbora.classify(PROBE_TECH).expect("trained classifier");
    assert_eq!(
        verbora_pred, "tech",
        "sanity: Verbora itself must get the obvious case right"
    );

    let mut nb = NaiveBayes::new();
    for (text, label) in TRAIN {
        nb.train(&tokenize(text), &(*label).to_string());
    }
    let scores = nb.classify(&tokenize(PROBE_TECH));
    let nb_pred = scores
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).expect("scores are finite"))
        .map(|(label, _)| label.clone())
        .expect("both classes were trained");

    assert_eq!(
        nb_pred, "tech",
        "naivebayes should agree with Verbora on an unambiguous, \
         non-overlapping-vocabulary case, scores = {scores:?}"
    );
}

/// smartcore, linfa-logistic and rustlearn all agree with Verbora's
/// `LogisticRegressionClassifier` on the same unambiguous case, despite each
/// using a different multiclass strategy/optimizer (matrix: Partial, selected
/// cases in every row — see `benches/classifiers.rs`'s own doc comment).
/// Exact probabilities are never compared, only the predicted class.
#[test]
fn logistic_regression_competitors_agree_on_a_linearly_separable_case() {
    // Verbora.
    let mut verbora = LogisticRegressionClassifier::new();
    for (text, label) in TRAIN {
        verbora.add_document(*text, label);
    }
    verbora.train().expect("the corpus has examples");
    let verbora_pred = verbora.classify(PROBE_TECH).expect("trained classifier");
    assert_eq!(
        verbora_pred, "tech",
        "sanity: Verbora itself must get the obvious case right"
    );

    let vocab = Vocab::build(TRAIN);
    let rows = vocab.matrix(TRAIN);

    // smartcore: dense f64 matrix, u32 labels.
    let rows_f64: Vec<Vec<f64>> = rows
        .iter()
        .map(|r| r.iter().map(|&v| f64::from(v)).collect())
        .collect();
    let refs: Vec<&[f64]> = rows_f64.iter().map(Vec::as_slice).collect();
    let x = DenseMatrix::<f64>::from_2d_array(&refs).expect("rectangular matrix");
    let y = label_ids_u32(TRAIN);
    let sm_model = LogisticRegression::fit(&x, &y, Default::default()).expect("fits");
    let probe_row = vocab.row(PROBE_TECH);
    let probe_row_f64: Vec<f64> = probe_row.iter().map(|&v| f64::from(v)).collect();
    let xt = DenseMatrix::<f64>::from_2d_array(&[probe_row_f64.as_slice()]).expect("one row");
    let sm_pred = sm_model.predict(&xt).expect("predicts")[0];
    assert_eq!(
        label_name(sm_pred as usize),
        "tech",
        "smartcore should agree with Verbora on an unambiguous case"
    );

    // linfa-logistic: dense f64 ndarray, usize labels.
    {
        use linfa::prelude::*;
        use linfa_logistic::MultiLogisticRegression;
        let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
        let lx =
            Array2::from_shape_vec((rows.len(), vocab.len()), flat).expect("rectangular matrix");
        let ly: Vec<usize> = label_ids(TRAIN);
        let ds = DatasetView::new(lx.view(), ndarray::ArrayView1::from(&ly));
        let model = MultiLogisticRegression::default().fit(&ds).expect("fits");
        let xt = Array2::from_shape_vec(
            (1, vocab.len()),
            probe_row.iter().map(|&v| f64::from(v)).collect(),
        )
        .expect("one row");
        let pred = model.predict(&xt)[0];
        assert_eq!(
            label_name(pred),
            "tech",
            "linfa-logistic should agree with Verbora on an unambiguous case"
        );
    }

    // rustlearn: f32 dense Array, one-vs-rest SGD.
    {
        use rustlearn::linear_models::sgdclassifier::Hyperparameters;
        use rustlearn::prelude::*;
        let rows_f32: Vec<Vec<f32>> = rows
            .iter()
            .map(|r| r.iter().map(|&v| v as f32).collect())
            .collect();
        let x_train = Array::from(&rows_f32);
        let y: Vec<f32> = label_ids(TRAIN).into_iter().map(|v| v as f32).collect();
        let y_train = Array::from(y);
        let mut model = Hyperparameters::new(vocab.len()).one_vs_rest();
        model.fit(&x_train, &y_train).expect("fits");
        let probe_row_f32: Vec<f32> = probe_row.iter().map(|&v| v as f32).collect();
        let xt = Array::from(&vec![probe_row_f32]);
        let pred = model.predict(&xt).expect("predicts");
        let pred_id = pred.get(0, 0) as usize;
        assert_eq!(
            label_name(pred_id),
            "tech",
            "rustlearn should agree with Verbora on an unambiguous case"
        );
    }
}
