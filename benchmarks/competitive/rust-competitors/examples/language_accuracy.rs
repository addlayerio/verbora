//! Accuracy report for `docs/COMPETITIVE_BENCHMARKS.md` §1.9's three real
//! language detectors — Verbora (via `WhatlangDetector`), `lingua`
//! (21-language restricted via `from_languages()`), and `whichlang`
//! (13-language, forced-choice, cannot abstain) — over
//! `datasets/language-accuracy/dataset.json` (see that directory's own
//! `README.md` for sourcing/license/extraction methodology).
//!
//! Per `Fase 6 Benchmark.md`'s own `LANGUAGE DETECTION BENCHMARKS` section,
//! "aquí performance sola NO es suficiente. Medir también accuracy" — this
//! is the accuracy half of §1.9, sitting alongside `benches/language.rs`'s
//! speed half. Neither file's numbers are published without the other, per
//! the spec's `ACCURACY + PERFORMANCE` section ("Una librería 10x más
//! rápida pero sustancialmente menos correcta no debe presentarse
//! simplemente como '10x faster' sin contexto").
//!
//! Run with: `cargo run --release --example language_accuracy`
//!
//! Writes `../results/language-accuracy.json` (every number in this file's
//! stdout table and in `docs/PERFORMANCE.md`'s own accuracy table traces
//! back to that JSON) and prints a human-readable report to stdout.
//!
//! # Why `whichlang`'s numbers need an asterisk, not an exclusion
//!
//! `whichlang::detect_language` never returns an `Option` — it always
//! guesses (defaulting to `Eng` when it extracts zero features at all; see
//! `whichlang-0.1.1/src/lib.rs`). `whatlang` and `lingua` can both abstain
//! (`None`). This report prints **coverage** (the fraction of items where
//! a detector actually committed to an answer) alongside accuracy for
//! exactly this reason — `whichlang`'s coverage is always 100% by
//! construction, which is not the same claim as "100% confident," and the
//! report says so rather than letting a bare accuracy number imply it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use competitive_rust::language_support::{
    TIERS, lingua_iso, lingua_restricted_languages, load_dataset, whichlang_iso,
};
use serde::Serialize;
use verbora_language::{LanguageDetector, WhatlangDetector};

/// One detector's answer for one dataset item.
#[derive(Debug, Clone, Serialize)]
struct Prediction {
    iso639_1: String,
    tier: &'static str,
    expected: String,
    predicted: Option<String>,
    correct: bool,
}

/// Accuracy/precision/recall/F1 for one detector over one set of
/// predictions (either one tier, or "overall" — all four tiers pooled).
#[derive(Debug, Clone, Serialize)]
struct Metrics {
    total: usize,
    correct: usize,
    abstained: usize,
    accuracy: f64,
    coverage: f64,
    macro_precision: f64,
    macro_recall: f64,
    macro_f1: f64,
}

fn compute_metrics(preds: &[&Prediction]) -> Metrics {
    let total = preds.len();
    let correct = preds.iter().filter(|p| p.correct).count();
    let abstained = preds.iter().filter(|p| p.predicted.is_none()).count();

    let mut classes: Vec<&str> = preds.iter().map(|p| p.expected.as_str()).collect();
    classes.sort_unstable();
    classes.dedup();

    let mut precisions = Vec::with_capacity(classes.len());
    let mut recalls = Vec::with_capacity(classes.len());
    for &class in &classes {
        let tp = preds
            .iter()
            .filter(|p| p.expected == class && p.predicted.as_deref() == Some(class))
            .count();
        let fp = preds
            .iter()
            .filter(|p| p.expected != class && p.predicted.as_deref() == Some(class))
            .count();
        let fnn = preds
            .iter()
            .filter(|p| p.expected == class && p.predicted.as_deref() != Some(class))
            .count();
        // Standard convention (matches scikit-learn's default): a class the
        // detector never predicts at all gets precision 0.0, not undefined
        // -- silently excluding it from the macro-average would flatter a
        // detector that simply never guesses a hard class.
        precisions.push(if tp + fp > 0 {
            tp as f64 / (tp + fp) as f64
        } else {
            0.0
        });
        recalls.push(if tp + fnn > 0 {
            tp as f64 / (tp + fnn) as f64
        } else {
            0.0
        });
    }
    let macro_precision = precisions.iter().sum::<f64>() / precisions.len() as f64;
    let macro_recall = recalls.iter().sum::<f64>() / recalls.len() as f64;
    let macro_f1 = if macro_precision + macro_recall > 0.0 {
        2.0 * macro_precision * macro_recall / (macro_precision + macro_recall)
    } else {
        0.0
    };

    Metrics {
        total,
        correct,
        abstained,
        accuracy: correct as f64 / total as f64,
        coverage: (total - abstained) as f64 / total as f64,
        macro_precision,
        macro_recall,
        macro_f1,
    }
}

fn print_row(detector: &str, tier: &str, m: &Metrics) {
    println!(
        "{detector:<10} {tier:<12} n={:<3} correct={:<3} abstained={:<3} accuracy={:>6.1}% coverage={:>6.1}% macroP={:.3} macroR={:.3} macroF1={:.3}",
        m.total,
        m.correct,
        m.abstained,
        m.accuracy * 100.0,
        m.coverage * 100.0,
        m.macro_precision,
        m.macro_recall,
        m.macro_f1,
    );
}

fn main() {
    let dataset = load_dataset();
    println!(
        "Language Detection accuracy report -- {} languages x {} tiers = {} items per detector\n",
        dataset.len(),
        TIERS.len(),
        dataset.len() * TIERS.len()
    );

    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut predictions: BTreeMap<&'static str, Vec<Prediction>> = BTreeMap::new();

    for tier in TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let expected = entry.iso639_1.clone();

            let verbora_predicted = verbora
                .detect(text)
                .best()
                .map(|c| c.language.iso639_1().to_owned());
            let lingua_predicted = lingua_detector.detect_language_of(text).map(lingua_iso);
            let whichlang_predicted =
                Some(whichlang_iso(whichlang::detect_language(text)).to_owned());

            for (name, predicted) in [
                ("verbora", verbora_predicted),
                ("lingua", lingua_predicted),
                ("whichlang", whichlang_predicted),
            ] {
                let correct = predicted.as_deref() == Some(expected.as_str());
                predictions.entry(name).or_default().push(Prediction {
                    iso639_1: entry.iso639_1.clone(),
                    tier,
                    expected: expected.clone(),
                    predicted,
                    correct,
                });
            }
        }
    }

    #[derive(Serialize)]
    struct DetectorReport {
        by_tier: BTreeMap<&'static str, Metrics>,
        overall: Metrics,
        predictions: Vec<Prediction>,
    }

    let mut report: BTreeMap<&'static str, DetectorReport> = BTreeMap::new();

    for (&detector, preds) in &predictions {
        println!("=== {detector} ===");
        let mut by_tier = BTreeMap::new();
        for tier in TIERS {
            let tier_preds: Vec<&Prediction> = preds.iter().filter(|p| p.tier == tier).collect();
            let m = compute_metrics(&tier_preds);
            print_row(detector, tier, &m);
            by_tier.insert(tier, m);
        }
        let all_preds: Vec<&Prediction> = preds.iter().collect();
        let overall = compute_metrics(&all_preds);
        print_row(detector, "OVERALL", &overall);
        println!();

        report.insert(
            detector,
            DetectorReport {
                by_tier,
                overall,
                predictions: preds.clone(),
            },
        );
    }

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .expect("benchmarks/competitive/ is one level up from rust-competitors/")
        .join("results/language-accuracy.json");
    let json = serde_json::to_string_pretty(&report).expect("serializable report");
    fs::write(&out_path, &json)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_path.display()));
    println!("wrote {}", out_path.display());
}
