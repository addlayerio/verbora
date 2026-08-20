//! CLI for the `HashedLinearDetector` training pipeline. See README.md in
//! this directory for the end-to-end workflow; the short version:
//!
//! ```text
//! prepare --corpus-dir <tatoeba dumps> --data-dir <workdir>
//! train   --data-dir <workdir> [--weights-out <path>] [--manifest-out <path>]
//! <rebuild: the generated weights are Rust source compiled into the crate>
//! eval    [--data-dir <workdir>] [--dataset <dataset.json>]
//! golden  < inputs.json
//! ```

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use langdetect_train::{
    Hyperparams, Model, Sample, calibrate_abstain_margin, evaluate_heldout, featurize,
    featurize_cyrillic, train,
};
use verbora_language::train_support::{CYRILLIC_CLASSES, DIMENSION, LATIN_CLASSES};
use verbora_language::{
    FallbackDetector, HashedLinearDetector, Language, LanguageDetector, Script, WhatlangDetector,
    detect_script,
};

/// One corpus language: Tatoeba ISO 639-3 file stem, this crate's ISO
/// 639-1 code, and the script `prepare` filters sentences by.
struct CorpusLang {
    iso3: &'static str,
    lang: Language,
    script: Script,
}

/// Latin corpus languages, in `LATIN_CLASSES` order (the class index is
/// the weight-table column, so this order is load-bearing — the
/// `class_orders_match` test pins it).
const LATIN_CORPUS: [CorpusLang; 16] = [
    CorpusLang {
        iso3: "eng",
        lang: Language::English,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "spa",
        lang: Language::Spanish,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "por",
        lang: Language::Portuguese,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "ita",
        lang: Language::Italian,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "fra",
        lang: Language::French,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "deu",
        lang: Language::German,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "nld",
        lang: Language::Dutch,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "pol",
        lang: Language::Polish,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "ind",
        lang: Language::Indonesian,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "vie",
        lang: Language::Vietnamese,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "nob",
        lang: Language::Norwegian,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "swe",
        lang: Language::Swedish,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "fin",
        lang: Language::Finnish,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "glg",
        lang: Language::Galician,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "cat",
        lang: Language::Catalan,
        script: Script::Latin,
    },
    CorpusLang {
        iso3: "eus",
        lang: Language::Basque,
        script: Script::Latin,
    },
];

/// Cyrillic corpus languages, in `CYRILLIC_CLASSES` order.
const CYRILLIC_CORPUS: [CorpusLang; 2] = [
    CorpusLang {
        iso3: "rus",
        lang: Language::Russian,
        script: Script::Cyrillic,
    },
    CorpusLang {
        iso3: "ukr",
        lang: Language::Ukrainian,
        script: Script::Cyrillic,
    },
];

/// Per-language training-sentence cap: balances compute against the
/// biggest corpora (eng ~2M raw) while balanced sampling in `train`
/// handles the thin ones (eus ~6k) by oversampling.
const TRAIN_CAP: usize = 120_000;
/// Held-out cap per language.
const HELDOUT_CAP: usize = 5_000;
/// Sentence length window (chars): drop empties and pathological outliers.
const MAX_CHARS: usize = 400;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    match mode {
        "prepare" => prepare(&args[1..]),
        "train" => run_train(&args[1..]),
        "eval" => run_eval(&args[1..]),
        "golden" => run_golden(),
        _ => {
            eprintln!("usage: langdetect-train <prepare|train|eval|golden> [--flags]");
            std::process::exit(2);
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn required_flag(args: &[String], name: &str) -> String {
    flag(args, name).unwrap_or_else(|| {
        eprintln!("missing required flag {name} <value>");
        std::process::exit(2);
    })
}

// ---------------------------------------------------------------------------
// prepare
// ---------------------------------------------------------------------------

/// Reads each Tatoeba per-language export (`{iso3}_sentences.tsv.bz2`,
/// columns `id \t iso3 \t text`), keeps sentences whose majority script
/// matches the language's expected script (drops mislabeled and
/// foreign-script quotes), deduplicates, and splits every 10th kept
/// sentence into the held-out file. Deterministic: file order and split
/// rule carry no randomness.
fn prepare(args: &[String]) {
    let corpus_dir = PathBuf::from(required_flag(args, "--corpus-dir"));
    let data_dir = PathBuf::from(required_flag(args, "--data-dir"));
    fs::create_dir_all(&data_dir).expect("create data dir");

    let mut stats = Vec::new();
    for cl in LATIN_CORPUS.iter().chain(&CYRILLIC_CORPUS) {
        let file = corpus_dir.join(format!("{}_sentences.tsv.bz2", cl.iso3));
        let sha256 = sha256_of(&file);
        let out = Command::new("bzcat")
            .arg(&file)
            .output()
            .expect("bzcat must be installed and the corpus file readable");
        assert!(out.status.success(), "bzcat failed for {}", file.display());
        let body = String::from_utf8(out.stdout).expect("corpus must be UTF-8");

        let mut seen: HashSet<&str> = HashSet::new();
        let mut train_lines: Vec<&str> = Vec::new();
        let mut heldout_lines: Vec<&str> = Vec::new();
        let mut raw = 0usize;
        let mut kept = 0usize;
        for line in body.lines() {
            raw += 1;
            let mut cols = line.splitn(3, '\t');
            let (Some(_id), Some(iso3), Some(text)) = (cols.next(), cols.next(), cols.next())
            else {
                continue;
            };
            if iso3 != cl.iso3 || text.is_empty() || text.chars().count() > MAX_CHARS {
                continue;
            }
            if detect_script(text) != Some(cl.script) {
                continue;
            }
            if !seen.insert(text) {
                continue;
            }
            if kept % 10 == 9 {
                if heldout_lines.len() < HELDOUT_CAP {
                    heldout_lines.push(text);
                } else if train_lines.len() < TRAIN_CAP {
                    train_lines.push(text);
                }
            } else if train_lines.len() < TRAIN_CAP {
                train_lines.push(text);
            }
            kept += 1;
            if train_lines.len() >= TRAIN_CAP && heldout_lines.len() >= HELDOUT_CAP {
                break;
            }
        }
        let iso1 = cl.lang.iso639_1();
        fs::write(
            data_dir.join(format!("{iso1}.train.txt")),
            train_lines.join("\n"),
        )
        .expect("write train split");
        fs::write(
            data_dir.join(format!("{iso1}.heldout.txt")),
            heldout_lines.join("\n"),
        )
        .expect("write heldout split");
        println!(
            "{iso1}: raw {raw}, kept {kept}, train {}, heldout {}",
            train_lines.len(),
            heldout_lines.len()
        );
        stats.push(serde_json::json!({
            "iso639_1": iso1,
            "source_file": file.file_name().unwrap().to_string_lossy(),
            "source_sha256": sha256,
            "raw_lines": raw,
            "kept_after_filters": kept,
            "train": train_lines.len(),
            "heldout": heldout_lines.len(),
        }));
    }
    let summary = serde_json::json!({
        "corpus": "Tatoeba per-language exports (https://tatoeba.org, CC-BY 2.0 FR)",
        "filters": {
            "max_chars": MAX_CHARS,
            "script_filter": "detect_script(text) must equal the language's script",
            "dedup": "exact string",
            "split": "every 10th kept sentence held out",
            "train_cap": TRAIN_CAP,
            "heldout_cap": HELDOUT_CAP,
        },
        "languages": stats,
    });
    fs::write(
        data_dir.join("prepare.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .expect("write prepare.json");
    println!("wrote {}", data_dir.join("prepare.json").display());
}

fn sha256_of(path: &Path) -> String {
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum must be installed");
    assert!(
        out.status.success(),
        "sha256sum failed for {}",
        path.display()
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

// ---------------------------------------------------------------------------
// train
// ---------------------------------------------------------------------------

fn load_split(
    data_dir: &Path,
    lang: Language,
    split: &str,
    featurizer: fn(&str) -> Sample,
) -> Vec<Sample> {
    let path = data_dir.join(format!("{}.{split}.txt", lang.iso639_1()));
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} (run `prepare` first): {e}", path.display()));
    body.lines().map(featurizer).collect()
}

/// Trains both models, calibrates abstention margins on held-out data,
/// writes the generated weights source and the reproducibility manifest.
fn run_train(args: &[String]) {
    let data_dir = PathBuf::from(required_flag(args, "--data-dir"));
    let weights_out = flag(args, "--weights-out")
        .unwrap_or_else(|| "../../crates/verbora-language/src/hashed_linear_weights.rs".to_owned());
    let manifest_out = flag(args, "--manifest-out").unwrap_or_else(|| "manifest.json".to_owned());

    let hp = Hyperparams {
        epochs: 8,
        samples_per_class: 25_000,
        lr0: 0.5,
        lr_decay: 0.65,
        seed: 0x5EED_C0DE_2026_0818,
    };

    println!("featurizing latin…");
    let latin_train: Vec<Vec<Sample>> = LATIN_CLASSES
        .iter()
        .map(|&l| load_split(&data_dir, l, "train", featurize))
        .collect();
    let latin_heldout: Vec<Vec<Sample>> = LATIN_CLASSES
        .iter()
        .map(|&l| load_split(&data_dir, l, "heldout", featurize))
        .collect();
    println!("training latin (16 classes)…");
    let latin = train(&latin_train, &hp);
    let latin_report = evaluate_heldout(&latin, &latin_heldout);
    let latin_abstain = calibrate_abstain_margin(&latin_report);

    println!("featurizing cyrillic…");
    let cyr_train: Vec<Vec<Sample>> = CYRILLIC_CLASSES
        .iter()
        .map(|&l| load_split(&data_dir, l, "train", featurize_cyrillic))
        .collect();
    let cyr_heldout: Vec<Vec<Sample>> = CYRILLIC_CLASSES
        .iter()
        .map(|&l| load_split(&data_dir, l, "heldout", featurize_cyrillic))
        .collect();
    println!("training cyrillic (2 classes)…");
    let cyr = train(&cyr_train, &hp);
    let cyr_report = evaluate_heldout(&cyr, &cyr_heldout);
    let cyr_abstain = calibrate_abstain_margin(&cyr_report);

    for (name, classes, report) in [
        ("latin", &LATIN_CLASSES[..], &latin_report),
        ("cyrillic", &CYRILLIC_CLASSES[..], &cyr_report),
    ] {
        println!("held-out accuracy ({name}):");
        for (lang, &(correct, total)) in classes.iter().zip(&report.per_class_accuracy) {
            println!(
                "  {}: {correct}/{total} ({:.2}%)",
                lang.iso639_1(),
                100.0 * correct as f64 / total.max(1) as f64
            );
        }
    }
    println!("abstain margins: latin {latin_abstain}, cyrillic {cyr_abstain}");

    let prepare_json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(data_dir.join("prepare.json")).expect("prepare.json must exist"),
    )
    .expect("prepare.json must parse");

    let code = codegen(
        &latin,
        latin_abstain,
        &cyr,
        cyr_abstain,
        &latin_report,
        &cyr_report,
    );
    fs::write(&weights_out, code).expect("write generated weights");
    println!("wrote {weights_out}");

    let accuracy_json = |classes: &[Language], report: &langdetect_train::HeldOutReport| {
        classes
            .iter()
            .zip(&report.per_class_accuracy)
            .map(|(lang, &(correct, total))| {
                serde_json::json!({
                    "iso639_1": lang.iso639_1(),
                    "correct": correct,
                    "total": total,
                })
            })
            .collect::<Vec<_>>()
    };
    let manifest = serde_json::json!({
        "tool": "langdetect-train 0.1.0",
        "dimension": DIMENSION,
        "hyperparams": {
            "epochs": hp.epochs,
            "samples_per_class_per_epoch": hp.samples_per_class,
            "lr0": hp.lr0,
            "lr_decay": hp.lr_decay,
            "seed": format!("{:#X}", hp.seed),
        },
        "abstain_margins": { "latin": latin_abstain, "cyrillic": cyr_abstain },
        "heldout_accuracy": {
            "latin": accuracy_json(&LATIN_CLASSES, &latin_report),
            "cyrillic": accuracy_json(&CYRILLIC_CLASSES, &cyr_report),
        },
        "prepare": prepare_json,
    });
    fs::write(
        &manifest_out,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .expect("write manifest");
    println!("wrote {manifest_out}");
}

/// Emits the generated Rust source. Floats are written with `{:?}`
/// (shortest round-trip formatting), so the compiled constants are
/// bit-identical to the trained values; non-finite weights are a training
/// bug and abort codegen.
fn codegen(
    latin: &Model,
    latin_abstain: f32,
    cyr: &Model,
    cyr_abstain: f32,
    latin_report: &langdetect_train::HeldOutReport,
    cyr_report: &langdetect_train::HeldOutReport,
) -> String {
    let mut out = String::new();
    out.push_str(
        "// GENERATED by tools/langdetect-train — do not edit by hand.\n\
         //\n\
         // Interleaved hashed-linear weight tables for HashedLinearDetector:\n\
         // `*_WEIGHTS[bucket * n_classes + class]`, plus per-class intercepts\n\
         // and the held-out-calibrated abstention margins. Class order is\n\
         // LATIN_CLASSES / CYRILLIC_CLASSES in src/hashed_linear.rs — the\n\
         // trainer imports those arrays, so the orders cannot drift.\n\
         //\n\
         // Training data: Tatoeba per-language sentence exports\n\
         // (https://tatoeba.org, licensed CC-BY 2.0 FR — attribution:\n\
         // Tatoeba contributors). Corpus file hashes, sentence counts,\n\
         // hyperparameters, seed, and held-out accuracy are recorded in\n\
         // tools/langdetect-train/manifest.json; regeneration with the same\n\
         // corpus and manifest settings reproduces this file byte for byte.\n\
         //\n\
         // The published evaluation set (UDHR-derived dataset.json under\n\
         // benchmarks/competitive/datasets/language-accuracy) is NOT part of\n\
         // the training data.\n",
    );
    write_report_comment(&mut out, "latin", &LATIN_CLASSES, latin_report);
    write_report_comment(&mut out, "cyrillic", &CYRILLIC_CLASSES, cyr_report);
    write_table(&mut out, "LATIN_WEIGHTS", &latin.weights);
    write_table(&mut out, "LATIN_INTERCEPTS", &latin.intercepts);
    writeln!(
        out,
        "pub const LATIN_ABSTAIN_MARGIN: f32 = {latin_abstain:?};"
    )
    .unwrap();
    write_table(&mut out, "CYRILLIC_WEIGHTS", &cyr.weights);
    write_table(&mut out, "CYRILLIC_INTERCEPTS", &cyr.intercepts);
    writeln!(
        out,
        "pub const CYRILLIC_ABSTAIN_MARGIN: f32 = {cyr_abstain:?};"
    )
    .unwrap();
    out
}

fn write_report_comment(
    out: &mut String,
    name: &str,
    classes: &[Language],
    report: &langdetect_train::HeldOutReport,
) {
    write!(out, "// held-out accuracy ({name}):").unwrap();
    for (lang, &(correct, total)) in classes.iter().zip(&report.per_class_accuracy) {
        write!(
            out,
            " {} {:.1}%",
            lang.iso639_1(),
            100.0 * correct as f64 / total.max(1) as f64
        )
        .unwrap();
    }
    out.push('\n');
}

fn write_table(out: &mut String, name: &str, values: &[f32]) {
    writeln!(out, "pub static {name}: [f32; {}] = [", values.len()).unwrap();
    for chunk in values.chunks(16) {
        for v in chunk {
            assert!(v.is_finite(), "non-finite weight in {name}");
            write!(out, "{v:?},").unwrap();
        }
        out.push('\n');
    }
    out.push_str("];\n");
}

// ---------------------------------------------------------------------------
// eval
// ---------------------------------------------------------------------------

/// End-to-end evaluation with the *compiled-in* weights: run `train`,
/// rebuild, then run this. Reports held-out accuracy through the real
/// `HashedLinearDetector` (abstentions counted separately, and as
/// incorrect in the accuracy column) plus the repository's UDHR tier
/// dataset, side by side with `WhatlangDetector`.
///
/// Both sections are optional and independent, because the two inputs
/// have very different availability: the held-out splits exist only in a
/// maintainer's `prepare` workdir (the Tatoeba corpus is not committed —
/// see README.md), while `dataset.json` *is* committed. Requiring
/// `--data-dir` for a dataset-only run would make the committed half of
/// the evaluation unreproducible for anyone who has not downloaded ~95 MB
/// of corpus, so `--data-dir` is honoured when given and skipped (with a
/// printed note, never silently) when it is not.
fn run_eval(args: &[String]) {
    let fast = HashedLinearDetector::new();
    let whatlang = WhatlangDetector::new();

    match flag(args, "--data-dir") {
        Some(dir) => eval_heldout_splits(&PathBuf::from(dir), &fast),
        None => println!(
            "== held-out, end-to-end: skipped (no --data-dir; run `prepare` first to produce one) =="
        ),
    }

    match flag(args, "--dataset") {
        Some(dataset) => eval_tier_dataset(&dataset, &fast, &whatlang),
        None => println!("== UDHR tier dataset: skipped (no --dataset) =="),
    }
}

/// Held-out accuracy per language through the shipped detector.
/// Abstentions are reported separately *and* counted as not-correct in
/// the accuracy column — an abstention is a non-answer, and letting it
/// quietly leave the denominator would inflate every number here.
fn eval_heldout_splits(data_dir: &Path, fast: &HashedLinearDetector) {
    println!("== held-out, end-to-end (HashedLinearDetector) ==");
    for cl in LATIN_CORPUS.iter().chain(&CYRILLIC_CORPUS) {
        let path = data_dir.join(format!("{}.heldout.txt", cl.lang.iso639_1()));
        let body = fs::read_to_string(&path).expect("heldout split must exist");
        let (mut correct, mut abstain, mut total) = (0usize, 0usize, 0usize);
        for line in body.lines() {
            total += 1;
            match fast.detect(line).best().map(|c| c.language) {
                Some(lang) if lang == cl.lang => correct += 1,
                Some(_) => {}
                None => abstain += 1,
            }
        }
        println!(
            "  {}: {correct}/{total} ({:.2}%), abstained {abstain} ({:.2}%)",
            cl.lang.iso639_1(),
            100.0 * correct as f64 / total.max(1) as f64,
            100.0 * abstain as f64 / total.max(1) as f64,
        );
    }
}

/// The committed UDHR tier dataset, every shipped detector side by side,
/// with a per-tier miss list. The misses are printed rather than
/// summarised because the decision this evaluation feeds — whether the
/// fast detector may become anyone's default — turns on *which* languages
/// it loses, not only on how many.
///
/// The third column is `FallbackDetector<HashedLinear, Whatlang>`, the
/// composition the crate documents: it is the only configuration that can
/// be both fast and (on this set) as accurate as the reference, so the
/// numbers backing that claim have to come from the same run as the
/// numbers backing the other two, not from a separate hand-check.
fn eval_tier_dataset(dataset: &str, fast: &HashedLinearDetector, whatlang: &WhatlangDetector) {
    let composed = FallbackDetector::new(*fast, *whatlang);
    let body = fs::read_to_string(dataset).expect("dataset.json must be readable");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    let languages = json["languages"].as_array().unwrap();
    let n = languages.len();
    println!("== UDHR tier dataset (eval only — never trained on) ==");
    let mut totals = [0usize; 3];
    for tier in ["short_word", "short_phrase", "sentence", "paragraph"] {
        let mut correct = [0usize; 3];
        let mut misses: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for entry in languages {
            let iso = entry["iso639_1"].as_str().unwrap();
            let text = entry["items"][tier].as_str().unwrap();
            let got = [
                fast.detect(text).best().map(|c| c.language.iso639_1()),
                whatlang.detect(text).best().map(|c| c.language.iso639_1()),
                composed.detect(text).best().map(|c| c.language.iso639_1()),
            ];
            for (i, answer) in got.iter().enumerate() {
                if *answer == Some(iso) {
                    correct[i] += 1;
                } else {
                    misses[i].push(format!("{iso}->{}", answer.unwrap_or("(abstain)")));
                }
            }
        }
        for (i, c) in correct.iter().enumerate() {
            totals[i] += c;
        }
        println!(
            "  {tier:<12} fast {}/{n} ({:.1}%)  whatlang {}/{n} ({:.1}%)  fallback {}/{n} ({:.1}%)",
            correct[0],
            100.0 * correct[0] as f64 / n as f64,
            correct[1],
            100.0 * correct[1] as f64 / n as f64,
            correct[2],
            100.0 * correct[2] as f64 / n as f64,
        );
        for (label, list) in ["fast", "whatlang", "fallback"].iter().zip(&misses) {
            println!("    {label:<9} misses: {}", list.join(" "));
        }
    }
    println!(
        "  {:<12} fast {}/{}  whatlang {}/{}  fallback {}/{}",
        "TOTAL",
        totals[0],
        4 * n,
        totals[1],
        4 * n,
        totals[2],
        4 * n
    );
}

// ---------------------------------------------------------------------------
// golden
// ---------------------------------------------------------------------------

/// Reads a JSON array of strings from stdin and prints the pinned-output
/// table rows for `src/hashed_linear.rs`'s `GOLDEN` test — the mechanical
/// re-pinning step after a deliberate retrain.
fn run_golden() {
    let mut body = String::new();
    std::io::stdin().read_to_string(&mut body).unwrap();
    let inputs: Vec<String> =
        serde_json::from_str(&body).expect("stdin must be a JSON string array");
    let fast = HashedLinearDetector::new();
    for input in &inputs {
        match fast.detect(input).best() {
            None => println!("({input:?}, None),"),
            Some(c) => println!(
                "({input:?}, Some((Language::{:?}, {:#010X}))),",
                c.language,
                c.confidence.get().to_bits()
            ),
        }
    }
}
