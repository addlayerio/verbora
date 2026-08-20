// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the sixteen stemmers.
//!
//! Four things are measured, because they answer four different questions.
//!
//! * **Cross-stemmer cost** — every algorithm over a word list in its own
//!   language, reported per word. This is the number that shows what the design
//!   choices cost: the Snowball stemmers fill a stack buffer of characters per
//!   word, Lancaster and Carry stay on `&str`, and Indonesian does a binary
//!   search into a 29,932-word dictionary on every rule application.
//! * **Scaling** — `tokenize_and_stem` over documents spanning three orders of
//!   magnitude, as throughput, so a per-byte regression cannot hide inside a
//!   per-call number.
//! * **API shape** — `stems` (lazy) against `tokenize_and_stem` (collect) on the
//!   same document. The crate's central claim is that the iterator is the
//!   primitive and the `Vec` form is `collect()`, so the lazy form must be the
//!   cheaper of the two.
//! * **Worst cases** — the inputs whose *cost* is unusual rather than whose
//!   result is: an Indonesian reduplication (three singular-stemming passes and
//!   a prefix-restore loop), a long word that no rule matches, and the
//!   `keep_stops` flag, which decides whether the stop-word list is consulted at
//!   all.
//!
//! Word lists are embedded rather than read from an external tree, so the
//! numbers are reproducible in any checkout. English reuses the shared
//! `benches/data/words.json` that every other harness reads, so that one
//! language is measured on byte-identical data.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_stemmers::{
    CarryStemmerFr, LancasterStemmer, PorterStemmer, PorterStemmerDe, PorterStemmerEs,
    PorterStemmerFa, PorterStemmerFr, PorterStemmerIt, PorterStemmerNl, PorterStemmerNo,
    PorterStemmerPt, PorterStemmerRu, PorterStemmerSv, PorterStemmerUk, StemmerId, StemmerJa,
    TokenizeAndStem,
};

/// Document sizes, in words.
const SIZES: [usize; 4] = [16, 128, 1024, 8192];

const WORDS_DE: &[&str] = &[
    "bedürfnissen",
    "äckern",
    "ackers",
    "armes",
    "derbsten",
    "straße",
    "häuser",
    "fröhlich",
    "quelle",
    "feuer",
    "lichte",
    "bedürfnis",
    "heitkeit",
    "lichen",
    "verstehen",
    "wirtschaft",
];
const WORDS_ES: &[&str] = &[
    "árbol",
    "campa",
    "efecto",
    "uyendo",
    "cantándoselo",
    "digámoselo",
    "muéstrame",
    "aseis",
    "hablando",
    "comieron",
    "vivíamos",
    "trabajadores",
    "nacionalidad",
    "rápidamente",
];
const WORDS_FR: &[&str] = &[
    "volera",
    "volerait",
    "subitement",
    "tempérament",
    "voudriez",
    "vengeait",
    "saisissement",
    "transatlantique",
    "premièrement",
    "instruments",
    "trouverions",
    "publicité",
    "pitoyable",
];
const WORDS_IT: &[&str] = &[
    "abbandonandoglieli",
    "abbandonarsi",
    "acqua",
    "perché",
    "città",
    "quello",
    "casa",
    "trattamento",
    "nazionale",
    "continuamente",
    "istituzione",
    "abilità",
];
const WORDS_NL: &[&str] = &[
    "aachen",
    "kleurbaar",
    "onaantastbar",
    "lichte",
    "aanbelde",
    "lijkheden",
    "gemeen",
    "maan",
    "brood",
    "jongensgebaren",
    "molenwiekgebaren",
    "verzekeringen",
];
const WORDS_NO: &[&str] = &[
    "forebygger",
    "forenkla",
    "havnevirksomhetene",
    "hinder",
    "alltids",
    "akkumulerte",
    "hvorvidt",
    "innovativt",
    "lovleg",
    "arveavgiftslov",
    "vurderingene",
];
const WORDS_SV: &[&str] = &[
    "björks",
    "jaktbössa",
    "klockorna",
    "flickornas",
    "stiftelsen",
    "kloster",
    "frihetens",
    "svenskt",
    "härligt",
    "vackraste",
    "ordentligt",
    "körsbärsträdgårdarna",
];
const WORDS_PT: &[&str] = &[
    "coração",
    "ações",
    "você",
    "abilidades",
    "trabalhadores",
    "nacionalidade",
    "rapidamente",
    "continuamente",
    "instituição",
    "eiras",
    "logias",
    "ativamente",
];
const WORDS_RU: &[&str] = &[
    "важнейшими",
    "важностию",
    "валандался",
    "вагоном",
    "паденье",
    "падчерицей",
    "пакостей",
    "радость",
    "человеческий",
    "государственный",
    "ёлка",
    "мама",
];
const WORDS_UK: &[&str] = &[
    "важливий",
    "милосердість",
    "самостійність",
    "дивовижність",
    "колосальність",
    "миється",
    "читався",
    "радість",
    "гарність",
    "мама",
    "державний",
];
const WORDS_FA: &[&str] = &["کتاب", "کتاب‌ها", "می‌رود", "است", "را", "در", "خانه", "بزرگ"];
const WORDS_JA: &[&str] = &[
    "コーヒー",
    "タクシー",
    "パーティー",
    "コピー",
    "ヘルプ・センター",
    "アイウー",
    "パーティ",
];
const WORDS_ID: &[&str] = &[
    "hancurlah",
    "bukumukah",
    "berikanku",
    "dibuang",
    "belajar",
    "pelajar",
    "mempengaruhi",
    "mengkritik",
    "kesepersepuluhnya",
    "meniru-nirukan",
    "buku-buku",
    "perekonomian",
    "memberdayakan",
    "persemakmuran",
    "keberuntunganmu",
    "penstabilan",
];

/// Reads the shared English word list, failing loudly if it is missing.
fn words_en() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("word list")
        .iter()
        .map(|w| w.as_str().expect("string").to_owned())
        .collect()
}

/// `n` words drawn cyclically from `source`.
fn sample(source: &[&str], n: usize) -> Vec<String> {
    source
        .iter()
        .cycle()
        .take(n)
        .map(|s| (*s).to_owned())
        .collect()
}

/// A space-joined document of `n` words.
fn document(words: &[String], n: usize) -> String {
    words
        .iter()
        .cycle()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One `stem` call per word, for every stemmer, on 256 words of its language.
fn per_word(c: &mut Criterion) {
    const N: usize = 256;
    let en = words_en();
    let en: Vec<String> = en.iter().cycle().take(N).cloned().collect();

    let mut group = c.benchmark_group("stem-per-word");
    group.throughput(Throughput::Elements(N as u64));

    macro_rules! bench {
        ($name:literal, $stemmer:expr, $words:expr) => {{
            // Borrowed, not owned: every list is read-only here, and taking them
            // by reference is what lets the English list serve two stemmers
            // without a copy.
            let words: &Vec<String> = $words;
            let s = $stemmer;
            group.bench_function($name, |b| {
                b.iter(|| {
                    words
                        .iter()
                        .map(|w| s.stem(black_box(w)).len())
                        .sum::<usize>()
                });
            });
        }};
    }

    bench!("en-porter", PorterStemmer::new(), &en);
    bench!("en-lancaster", LancasterStemmer::new(), &en);
    bench!("de", PorterStemmerDe::new(), &sample(WORDS_DE, N));
    bench!("es", PorterStemmerEs::new(), &sample(WORDS_ES, N));
    bench!("fr-porter", PorterStemmerFr::new(), &sample(WORDS_FR, N));
    bench!("fr-carry", CarryStemmerFr::new(), &sample(WORDS_FR, N));
    bench!("it", PorterStemmerIt::new(), &sample(WORDS_IT, N));
    bench!("nl", PorterStemmerNl::new(), &sample(WORDS_NL, N));
    bench!("no", PorterStemmerNo::new(), &sample(WORDS_NO, N));
    bench!("sv", PorterStemmerSv::new(), &sample(WORDS_SV, N));
    bench!("pt", PorterStemmerPt::new(), &sample(WORDS_PT, N));
    bench!("ru", PorterStemmerRu::new(), &sample(WORDS_RU, N));
    bench!("uk", PorterStemmerUk::new(), &sample(WORDS_UK, N));
    bench!("fa-identity", PorterStemmerFa::new(), &sample(WORDS_FA, N));
    bench!("ja-katakana", StemmerJa::new(), &sample(WORDS_JA, N));
    bench!("id-dictionary", StemmerId::new(), &sample(WORDS_ID, N));
    group.finish();
}

/// `tokenize_and_stem` over growing documents, in three representative
/// languages: ASCII Latin, accented Latin, and Cyrillic.
fn scaling(c: &mut Criterion) {
    let en = words_en();

    let mut group = c.benchmark_group("tokenize-and-stem");
    for n in SIZES {
        let doc_en = document(&en, n);
        let doc_de = document(&sample(WORDS_DE, 32), n);
        let doc_ru = document(&sample(WORDS_RU, 32), n);

        group.throughput(Throughput::Bytes(doc_en.len() as u64));
        group.bench_with_input(BenchmarkId::new("en-porter", n), &doc_en, |b, d| {
            b.iter(|| {
                PorterStemmer::new()
                    .tokenize_and_stem(black_box(d), false)
                    .len()
            });
        });
        group.throughput(Throughput::Bytes(doc_de.len() as u64));
        group.bench_with_input(BenchmarkId::new("de", n), &doc_de, |b, d| {
            b.iter(|| {
                PorterStemmerDe::new()
                    .tokenize_and_stem(black_box(d), false)
                    .len()
            });
        });
        group.throughput(Throughput::Bytes(doc_ru.len() as u64));
        group.bench_with_input(BenchmarkId::new("ru", n), &doc_ru, |b, d| {
            b.iter(|| {
                PorterStemmerRu::new()
                    .tokenize_and_stem(black_box(d), false)
                    .len()
            });
        });
    }
    group.finish();
}

/// The lazy primitive against the collecting convenience API, and the cost of
/// consulting the stop-word list at all.
fn api_shape(c: &mut Criterion) {
    let doc = document(&words_en(), 1024);
    let s = PorterStemmer::new();

    let mut group = c.benchmark_group("api-shape");
    group.throughput(Throughput::Bytes(doc.len() as u64));

    // The primitive: no `Vec`, one `String` per emitted stem.
    group.bench_function("stems-lazy", |b| {
        b.iter(|| {
            s.stems(black_box(&doc), false)
                .map(|t| t.len())
                .sum::<usize>()
        });
    });
    // The convenience API: `stems(..).collect()`.
    group.bench_function("tokenize-and-stem", |b| {
        b.iter(|| s.tokenize_and_stem(black_box(&doc), false).len());
    });
    // `keep_stops` skips the membership test entirely and emits more tokens.
    group.bench_function("tokenize-and-stem-keep-stops", |b| {
        b.iter(|| s.tokenize_and_stem(black_box(&doc), true).len());
    });
    // Only the first few stems are consumed: what laziness is actually for.
    group.bench_function("stems-first-8", |b| {
        b.iter(|| {
            s.stems(black_box(&doc), false)
                .take(8)
                .map(|t| t.len())
                .sum::<usize>()
        });
    });
    group.finish();
}

/// Inputs chosen for their cost rather than their result.
fn worst_cases(c: &mut Criterion) {
    let long = "x".repeat(1000);
    let long_ing = "ing".repeat(300);

    let mut group = c.benchmark_group("worst-case");

    // Reduplication runs the singular stemmer two or three times and can reach the
    // prefix-restore loop, which re-runs the whole prefix table per removal.
    group.bench_function("id-reduplication", |b| {
        b.iter(|| StemmerId::new().stem(black_box("meniru-nirukan")).len());
    });
    // A word that is in the dictionary exits after the first lookup.
    group.bench_function("id-dictionary-hit", |b| {
        b.iter(|| StemmerId::new().stem(black_box("makan")).len());
    });
    // A word that is in no dictionary and matches no rule walks every rule.
    group.bench_function("id-miss", |b| {
        b.iter(|| StemmerId::new().stem(black_box("xxberkaerat")).len());
    });
    // 1000 characters through the UTF-16 buffer and the region scan.
    group.bench_function("en-long-word", |b| {
        b.iter(|| PorterStemmer::new().stem(black_box(&long)).len());
    });
    group.bench_function("lancaster-repeating-suffix", |b| {
        b.iter(|| LancasterStemmer::new().stem(black_box(&long_ing)).len());
    });
    // Carry rebuilds a candidate string for every suffix length.
    group.bench_function("carry-long-word", |b| {
        b.iter(|| {
            CarryStemmerFr::new()
                .stem(black_box("transatlantique"))
                .len()
        });
    });
    group.finish();
}

/// `n_docs` documents of `words_per_doc` words each, drawn from `words` at a
/// rotating offset per document so consecutive documents are not byte-identical
/// (which would let the allocator or the branch predictor cheat).
#[cfg(feature = "parallel")]
fn corpus(words: &[String], n_docs: usize, words_per_doc: usize) -> Vec<String> {
    (0..n_docs)
        .map(|i| {
            words
                .iter()
                .cycle()
                .skip(i % words.len().max(1))
                .take(words_per_doc)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Sequential `.iter().map(tokenize_and_stem).collect()` against
/// [`TokenizeAndStem::par_tokenize_and_stem_batch`], at corpus shapes spanning
/// small-and-short (where the audit expects parallel to lose or barely tie) up
/// to many-and-paragraph-sized (where per-document work is comfortably above
/// rayon's per-task scheduling floor).
///
/// This is the benchmark that justifies choosing *document* granularity over
/// *word* granularity: `stem-per-word` above measures Porter at ~628 ns/word,
/// which is close to what it costs rayon just to schedule a task, so
/// `words.par_iter().map(stem)` would mostly measure the scheduler. A document
/// is that per-word cost times however many words it holds, so it clears the
/// scheduling floor by a comfortable margin once it is paragraph-sized.
#[cfg(feature = "parallel")]
fn document_batch(c: &mut Criterion) {
    use verbora_stemmers::TokenizeAndStem;

    let en = words_en();
    // (documents in the batch, words per document).
    const SHAPES: [(usize, usize); 4] = [(4, 16), (32, 64), (256, 128), (2048, 256)];

    let mut group = c.benchmark_group("document-batch");
    for (n_docs, words_per_doc) in SHAPES {
        let docs = corpus(&en, n_docs, words_per_doc);
        let doc_refs: Vec<&str> = docs.iter().map(String::as_str).collect();
        let total_bytes: usize = docs.iter().map(String::len).sum();
        let shape = format!("{n_docs}x{words_per_doc}w");

        group.throughput(Throughput::Bytes(total_bytes as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential", &shape),
            &doc_refs,
            |b, docs| {
                b.iter(|| {
                    docs.iter()
                        .map(|d| PorterStemmer::new().tokenize_and_stem(black_box(d), false))
                        .collect::<Vec<_>>()
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("parallel", &shape),
            &doc_refs,
            |b, docs| {
                b.iter(|| PorterStemmer::new().par_tokenize_and_stem_batch(black_box(docs), false));
            },
        );
    }
    group.finish();
}

#[cfg(not(feature = "parallel"))]
criterion_group!(benches, per_word, scaling, api_shape, worst_cases);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);

#[cfg(feature = "parallel")]
criterion_group!(
    benches,
    per_word,
    scaling,
    api_shape,
    worst_cases,
    document_batch
);
#[cfg(feature = "parallel")]
criterion_main!(benches);
