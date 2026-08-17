//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/stemmers.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora and its Rust
//! competitors agree on the exact word lists the benchmark stems, before any
//! timing number from that file is trusted.

use std::borrow::Cow;
use std::num::NonZeroUsize;

use lindera::dictionary::{
    Dictionary as LinderaDictionary, WordId as LinderaWordId, load_dictionary,
};
use lindera::token::Token as LinderaToken;
use lindera_analysis::token_filter::TokenFilter as LinderaTokenFilter;
use lindera_analysis::token_filter::japanese_katakana_stem::JapaneseKatakanaStemTokenFilter;
use lindera_dictionary::viterbi::LexType;
use sastrawi::{Dictionary as SastrawiDictionary, Stemmer as SastrawiStemmer};
use verbora_stemmers::{
    CarryStemmerFr, PorterStemmer, PorterStemmerDe, PorterStemmerEs, PorterStemmerFr,
    PorterStemmerIt, PorterStemmerNl, PorterStemmerNo, PorterStemmerPt, PorterStemmerRu,
    PorterStemmerSv, StemmerId, StemmerJa,
};

fn load_words() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/stemmer-words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    serde_json::from_str(&body).expect("valid bench data")
}

fn words_for(data: &serde_json::Value, lang: &str) -> Vec<String> {
    data["languages"][lang]
        .as_array()
        .unwrap_or_else(|| panic!("no {lang:?} word list in stemmer-words.json"))
        .iter()
        .map(|w| w.as_str().expect("string").to_owned())
        .collect()
}

/// The nine shared canonical-Snowball languages: Verbora and `rust-stemmers`
/// must agree exactly on every word in the benchmarked list.
#[test]
fn snowball_agreement_on_benchmarked_words() {
    let data = load_words();

    macro_rules! check {
        ($lang:literal, $verbora:ty, $algo:expr) => {{
            let words = words_for(&data, $lang);
            let rs = rust_stemmers::Stemmer::create($algo);
            let v = <$verbora>::new();
            for w in &words {
                let a = v.stem(w);
                let b = rs.stem(w);
                assert_eq!(
                    a, b,
                    "{} stem({:?}): verbora={:?} rust-stemmers={:?}",
                    $lang, w, a, b
                );
            }
        }};
    }

    check!("de", PorterStemmerDe, rust_stemmers::Algorithm::German);
    check!("es", PorterStemmerEs, rust_stemmers::Algorithm::Spanish);
    check!("fr", PorterStemmerFr, rust_stemmers::Algorithm::French);
    check!("it", PorterStemmerIt, rust_stemmers::Algorithm::Italian);
    check!("no", PorterStemmerNo, rust_stemmers::Algorithm::Norwegian);
    check!("pt", PorterStemmerPt, rust_stemmers::Algorithm::Portuguese);
    check!("sv", PorterStemmerSv, rust_stemmers::Algorithm::Swedish);

    // Dutch: a fresh instance per word, deliberately not reused across the
    // list — see `dutch_sticky_flag_requires_a_fresh_instance_per_word`
    // immediately below for why a shared instance would fail this same
    // assertion.
    let rs_nl = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Dutch);
    for w in words_for(&data, "nl") {
        let a = PorterStemmerNl::new().stem(&w);
        let b = rs_nl.stem(&w);
        assert_eq!(a, b, "nl stem({w:?}): verbora={a:?} rust-stemmers={b:?}");
    }

    // Russian: the shared word list keeps 'ёлка' (see
    // tools/bench-data/generate.py's own comment on the `ru` entry — it is
    // also read, unfiltered, by the Verbora-vs-the reference comparison), so it
    // is filtered out here explicitly rather than assumed absent. The next
    // test asserts exactly why.
    let rs = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Russian);
    let v = PorterStemmerRu::new();
    for w in words_for(&data, "ru").into_iter().filter(|w| w != "ёлка") {
        let a = v.stem(&w);
        let b = rs.stem(&w);
        assert_eq!(a, b, "ru stem({w:?}): verbora={a:?} rust-stemmers={b:?}");
    }
}

/// The one *documented* Russian exception, checked explicitly so the
/// divergence is asserted rather than merely described in a comment: Verbora
/// folds `ё`→`е` before stemming (`porter_stemmer_ru`'s own behavior);
/// `rust-stemmers`' Russian port does not, because that fold is not part of
/// the canonical Snowball algorithm specification.
#[test]
fn russian_yo_fold_is_a_real_and_isolated_divergence() {
    let v = PorterStemmerRu::new().stem("ёлка");
    let rs = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Russian).stem("ёлка");
    assert_eq!(v, "елк");
    assert_eq!(rs, "ёлка");
    assert_ne!(v, rs, "the ё-fold divergence is expected to still exist");
}

/// `CarryStemmerFr` must genuinely diverge from `rust-stemmers`' standard
/// Snowball French — confirming the matrix's `No` verdict for that pairing
/// (and why `benches/stemmers.rs` never benchmarks it).
#[test]
fn carry_french_is_not_rust_stemmers_french() {
    let rs = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::French);
    let carry = CarryStemmerFr::new();
    let mut any_mismatch = false;
    for w in ["subitement", "instruments", "publicité", "tempérament"] {
        if carry.stem(w) != rs.stem(w) {
            any_mismatch = true;
        }
    }
    assert!(
        any_mismatch,
        "expected CarryStemmerFr to diverge from rust-stemmers' standard Snowball French"
    );
}

/// English: Verbora's original-1980 Porter vs. `nltk-porter`'s
/// `Mode::Original`, on the realistic word list `benches/stemmers.rs`
/// benchmarks, plus five inputs chosen specifically to exercise Verbora's own
/// documented Porter quirks (`crates/verbora-stemmers/src/en.rs`'s doc
/// comment: `measure`-as-float, no-early-exit rule application,
/// empty-string-falsy).
#[test]
fn english_nltk_porter_original_agrees_on_benchmarked_words() {
    let data = load_words();
    let words = words_for(&data, "en");
    let nltk = nltk_porter::PorterStemmer::new(nltk_porter::Mode::Original);
    let v = PorterStemmer::new();

    for w in &words {
        let a = v.stem(w);
        let b = nltk.stem(w);
        assert_eq!(a, b, "stem({w:?}): verbora={a:?} nltk-porter={b:?}");
    }

    for w in ["formalizeful", "ed", "sya", "syaing", "fifugyed"] {
        let a = v.stem(w);
        let b = nltk.stem(w);
        assert_eq!(a, b, "quirk stem({w:?}): verbora={a:?} nltk-porter={b:?}");
    }
}

/// Demonstrates directly why `benches/stemmers.rs`'s `bench_nl` uses a fresh
/// [`PorterStemmerNl`] per word rather than one shared instance: reusing an
/// instance across several words makes a later word's result depend on
/// earlier ones, purely because of `nl.rs`'s documented sticky
/// `suffix_e_removed` flag — not anything `rust-stemmers` has an equivalent
/// of. `rust-stemmers::Stemmer::stem` is unaffected either way (it is pure),
/// so a shared instance would make this specific comparison unfair without
/// this being an "algorithm" difference at all — just a benchmark-methodology
/// trap this test exists to catch by name.
#[test]
fn dutch_sticky_flag_requires_a_fresh_instance_per_word() {
    let shared = PorterStemmerNl::new();
    // Any word ending in a stripped-e suffix trips the flag; "lichte" (also
    // in the benchmarked `nl` word list) does.
    let _ = shared.stem("lichte");
    let after_priming = shared.stem("jongensgebaren").into_owned();

    let fresh = PorterStemmerNl::new().stem("jongensgebaren").into_owned();

    assert_ne!(
        after_priming, fresh,
        "expected the sticky flag to change this word's result after priming"
    );
    // The fresh, per-word answer is the one that agrees with rust-stemmers —
    // already asserted in `snowball_agreement_on_benchmarked_words` above,
    // restated here for a single word so the two tests read as one story.
    let rs = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Dutch)
        .stem("jongensgebaren")
        .into_owned();
    assert_eq!(fresh, rs);
    assert_ne!(after_priming, rs);
}

// ---------------------------------------------------------------------------
// A second, independent Snowball-family competitor: `snowball_stemmers_rs`
// 1.0.1, per-language, alongside `rust-stemmers` above.
// ---------------------------------------------------------------------------

/// The nine shared canonical-Snowball languages, `snowball_stemmers_rs` side:
/// Verbora and `snowball_stemmers_rs` must agree exactly on every word in the
/// benchmarked list — and, unlike the `rust-stemmers` row above, they do,
/// with **no exclusions needed at all**, `ёлка` included (see
/// `russian_yo_fold_is_also_in_snowball_stemmers_rs` immediately below for
/// why). Dutch uses [`Algorithm::DutchPorter`], not the plain [`Algorithm::Dutch`]
/// — see `dutch_algorithm_variant_must_be_dutchporter_not_dutch` for why that
/// choice is a real, verified requirement, not an arbitrary pick.
#[test]
fn snowball_stemmers_rs_agreement_on_benchmarked_words() {
    let data = load_words();

    macro_rules! check {
        ($lang:literal, $verbora:ty, $algo:expr) => {{
            let words = words_for(&data, $lang);
            let sn = snowball_stemmers_rs::Stemmer::create($algo);
            let v = <$verbora>::new();
            for w in &words {
                let a = v.stem(w);
                let b = sn.stem(w);
                assert_eq!(
                    a, b,
                    "{} stem({:?}): verbora={:?} snowball_stemmers_rs={:?}",
                    $lang, w, a, b
                );
            }
        }};
    }

    check!(
        "de",
        PorterStemmerDe,
        snowball_stemmers_rs::Algorithm::German
    );
    check!(
        "es",
        PorterStemmerEs,
        snowball_stemmers_rs::Algorithm::Spanish
    );
    check!(
        "fr",
        PorterStemmerFr,
        snowball_stemmers_rs::Algorithm::French
    );
    check!(
        "it",
        PorterStemmerIt,
        snowball_stemmers_rs::Algorithm::Italian
    );
    check!(
        "no",
        PorterStemmerNo,
        snowball_stemmers_rs::Algorithm::Norwegian
    );
    check!(
        "pt",
        PorterStemmerPt,
        snowball_stemmers_rs::Algorithm::Portuguese
    );
    check!(
        "sv",
        PorterStemmerSv,
        snowball_stemmers_rs::Algorithm::Swedish
    );

    // Dutch: fresh Verbora instance per word, same reasoning as
    // `snowball_agreement_on_benchmarked_words`'s own `nl` block above —
    // `snowball_stemmers_rs::Stemmer` has no cross-call state (`SnowballEnv`
    // is built fresh inside `stem()` every call, confirmed by reading
    // `snowball_stemmers_rs-1.0.1/src/lib.rs`), so only the Verbora side
    // needs the fresh-instance discipline.
    let sn_nl = snowball_stemmers_rs::Stemmer::create(snowball_stemmers_rs::Algorithm::DutchPorter);
    for w in words_for(&data, "nl") {
        let a = PorterStemmerNl::new().stem(&w);
        let b = sn_nl.stem(&w);
        assert_eq!(
            a, b,
            "nl stem({w:?}): verbora={a:?} snowball_stemmers_rs={b:?}"
        );
    }

    // Russian: unlike the `rust-stemmers` row, NOT filtered — see
    // `russian_yo_fold_is_also_in_snowball_stemmers_rs` below for why 'ёлка'
    // genuinely agrees here.
    let sn_ru = snowball_stemmers_rs::Stemmer::create(snowball_stemmers_rs::Algorithm::Russian);
    let v_ru = PorterStemmerRu::new();
    for w in words_for(&data, "ru") {
        let a = v_ru.stem(&w);
        let b = sn_ru.stem(&w);
        assert_eq!(
            a, b,
            "ru stem({w:?}): verbora={a:?} snowball_stemmers_rs={b:?}"
        );
    }
}

/// `snowball_stemmers_rs`'s `Algorithm` enum exposes **two** Dutch entries:
/// `Dutch` and `DutchPorter`. Reading `algorithms/dutch.sbl`'s own header
/// comment ("Dutch stemming algorithm developed by Wessel Kraaij and Renée
/// Pohlmann") shows `Dutch` is a genuinely different, non-canonical Dutch
/// stemming algorithm — not the one `rust-stemmers::Algorithm::Dutch` and
/// Verbora's [`PorterStemmerNl`] implement. `dutch_porter.sbl`'s header
/// ("Dutch stemming algorithm developed by Martin Porter") is the one that
/// matches: confirmed here directly, not assumed from the name, by checking
/// both variants against the same word twice — `DutchPorter` agrees with
/// both Verbora and `rust-stemmers`; plain `Dutch` disagrees with both, and
/// disagrees with `DutchPorter` itself, proving this is a real algorithm
/// distinction the crate ships, not a naming quirk with no behavioral
/// consequence.
#[test]
fn dutch_algorithm_variant_must_be_dutchporter_not_dutch() {
    let word = "verzekeringen"; // in the benchmarked `nl` word list
    let verbora = PorterStemmerNl::new().stem(word).into_owned();
    let rust_stemmers_dutch = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Dutch)
        .stem(word)
        .into_owned();
    let snowball_dutch_porter =
        snowball_stemmers_rs::Stemmer::create(snowball_stemmers_rs::Algorithm::DutchPorter)
            .stem(word)
            .into_owned();
    let snowball_dutch_plain =
        snowball_stemmers_rs::Stemmer::create(snowball_stemmers_rs::Algorithm::Dutch)
            .stem(word)
            .into_owned();

    assert_eq!(verbora, rust_stemmers_dutch);
    assert_eq!(
        verbora, snowball_dutch_porter,
        "Algorithm::DutchPorter is the canonical Snowball Dutch algorithm shared with Verbora"
    );
    assert_ne!(
        verbora, snowball_dutch_plain,
        "Algorithm::Dutch (Kraaij/Pohlmann) is expected to genuinely diverge"
    );
    assert_ne!(
        snowball_dutch_porter, snowball_dutch_plain,
        "the two Dutch variants must be genuinely different algorithms, not a naming-only distinction"
    );
}

/// The one *documented* Russian exception the `rust-stemmers` row carries
/// (`russian_yo_fold_is_a_real_and_isolated_divergence` above) does **not**
/// apply to `snowball_stemmers_rs`: its `russian.sbl` source (confirmed by
/// reading `snowball_stemmers_rs-1.0.1/algorithms/russian.sbl` directly)
/// contains the same "normalise {ё} to {е}" pre-stemming step Verbora's own
/// `porter_stemmer_ru`-derived port applies — `rust-stemmers` 1.2.0 simply
/// does not carry this step in its own vendored copy of the algorithm. A
/// real, positive finding: `snowball_stemmers_rs` needs no Russian exclusion
/// at all, stronger than the existing `rust-stemmers` row.
#[test]
fn russian_yo_fold_is_also_in_snowball_stemmers_rs() {
    let v = PorterStemmerRu::new().stem("ёлка");
    let sn = snowball_stemmers_rs::Stemmer::create(snowball_stemmers_rs::Algorithm::Russian)
        .stem("ёлка");
    let rs = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Russian).stem("ёлка");

    assert_eq!(v, "елк");
    assert_eq!(
        sn, v,
        "snowball_stemmers_rs is expected to fold ё->е exactly like Verbora"
    );
    assert_ne!(
        sn, rs,
        "rust-stemmers, unlike snowball_stemmers_rs, does not perform this fold"
    );
}

// ---------------------------------------------------------------------------
// English, second alternative: `porter-stemmer` (samgiles) 0.1.2.
// ---------------------------------------------------------------------------

/// `porter-stemmer` operates on grapheme clusters rather than UTF-16 units
/// (the matrix's own flagged architectural difference) and agrees with
/// Verbora on every word in the benchmarked English list except one: `"sky"`,
/// where `porter-stemmer` alone produces `"ski"`. This is a real, isolated
/// bug in that crate — unrelated to the grapheme-cluster question, since
/// `nltk-porter` (which processes `Vec<char>`, not graphemes) agrees with
/// Verbora that `"sky"` is unchanged — confirmed here explicitly, then
/// excluded from `benches/stemmers.rs`'s own `porter_en` group sample so the
/// benchmark's shared input is one every implementation already agrees on.
#[test]
fn porter_stemmer_samgiles_agrees_on_benchmarked_words_except_sky() {
    let data = load_words();
    let v = PorterStemmer::new();
    let mut mismatches = Vec::new();

    for w in words_for(&data, "en") {
        let a = v.stem(&w).into_owned();
        let b = porter_stemmer::stem(&w);
        if a != b {
            mismatches.push((w, a, b));
        }
    }

    assert_eq!(
        mismatches,
        vec![("sky".to_owned(), "sky".to_owned(), "ski".to_owned())],
        "expected exactly one, already-known mismatch (\"sky\"); found: {mismatches:?}"
    );

    // The five quirk inputs `english_nltk_porter_original_agrees_on_benchmarked_words`
    // already exercises above: `porter-stemmer` must agree on all of them too,
    // confirming the grapheme-cluster architecture does not touch Verbora's
    // documented Porter quirks.
    for w in ["formalizeful", "ed", "sya", "syaing", "fifugyed"] {
        let a = v.stem(w);
        let b = porter_stemmer::stem(w);
        assert_eq!(
            a, b,
            "quirk stem({w:?}): verbora={a:?} porter-stemmer={b:?}"
        );
    }
}

/// `"sky"` really does stay `"sky"` on both Verbora and `nltk-porter` — the
/// half of the story that makes `porter-stemmer`'s `"ski"` a real bug rather
/// than an ambiguous case with no ground truth.
#[test]
fn sky_is_the_correct_answer_per_verbora_and_nltk_porter() {
    let nltk = nltk_porter::PorterStemmer::new(nltk_porter::Mode::Original);
    assert_eq!(PorterStemmer::new().stem("sky"), "sky");
    assert_eq!(nltk.stem("sky"), "sky");
    assert_eq!(porter_stemmer::stem("sky"), "ski");
}

// ---------------------------------------------------------------------------
// Japanese katakana: `lindera-analysis` 5.2.0's `JapaneseKatakanaStemTokenFilter`.
// ---------------------------------------------------------------------------

/// Builds a bare, pre-tokenized [`LinderaToken`] — see
/// `../benches/stemmers.rs`'s own `lindera_token` doc comment for why
/// `word_id`/`dictionary` can be dummy/shared values here without affecting
/// the filter's real behavior.
fn lindera_token<'a>(surface: &'a str, dictionary: &'a LinderaDictionary) -> LinderaToken<'a> {
    LinderaToken {
        surface: Cow::Borrowed(surface),
        byte_start: 0,
        byte_end: surface.len(),
        position: 0,
        position_length: 1,
        word_id: LinderaWordId::new(LexType::Unknown, 0),
        dictionary,
        user_dictionary: None,
        details: None,
    }
}

fn lindera_stem_one(
    filter: &JapaneseKatakanaStemTokenFilter,
    dictionary: &LinderaDictionary,
    word: &str,
) -> String {
    let mut tokens = vec![lindera_token(word, dictionary)];
    filter
        .apply(&mut tokens)
        .expect("filter never errors on plain-text tokens");
    tokens[0].surface.clone().into_owned()
}

/// Proves `min` is a genuine, load-bearing lever — not a coincidence that
/// happens to compile — before trusting that `min = 3` is the deliberately
/// correct value below. Mirrors `phonetics_correctness.rs`'s own
/// reconfigured-vs-unreconfigured pattern (`Some(32)` vs. `default()`).
#[test]
fn lindera_min_parameter_genuinely_changes_behavior() {
    let dictionary = load_dictionary("embedded://ipadic").expect("embedded IPADIC dictionary");
    // "コーヒー" is 4 UTF-16/char units; min=3 requires length > 3 (i.e. >= 4)
    // to stem, min=10 does not.
    let permissive = JapaneseKatakanaStemTokenFilter::new(NonZeroUsize::new(3).unwrap());
    let strict = JapaneseKatakanaStemTokenFilter::new(NonZeroUsize::new(10).unwrap());

    let stemmed = lindera_stem_one(&permissive, &dictionary, "コーヒー");
    let unstemmed = lindera_stem_one(&strict, &dictionary, "コーヒー");

    assert_eq!(stemmed, "コーヒ", "min=3 must stem a 4-unit katakana word");
    assert_eq!(
        unstemmed, "コーヒー",
        "min=10 must NOT stem the same 4-unit katakana word"
    );
    assert_ne!(
        stemmed, unstemmed,
        "the min parameter must be a genuine lever"
    );
}

/// `min = 3` (the filter's own default) reproduces [`StemmerJa`]'s
/// `slen(token) >= 4`-UTF-16-unit threshold exactly on the shared `ja` word
/// list — real, byte-for-byte agreement checked before
/// `benches/stemmers.rs`'s `stemmer_ja` group timing numbers are trusted.
#[test]
fn lindera_katakana_stem_min3_matches_verbora_on_benchmarked_words() {
    let data = load_words();
    let dictionary = load_dictionary("embedded://ipadic").expect("embedded IPADIC dictionary");
    let filter = JapaneseKatakanaStemTokenFilter::new(NonZeroUsize::new(3).unwrap());
    let v = StemmerJa::new();

    for w in words_for(&data, "ja") {
        let a = v.stem(&w);
        let b = lindera_stem_one(&filter, &dictionary, &w);
        assert_eq!(a, b, "stem({w:?}): verbora={a:?} lindera={b:?}");
    }

    // A handful of extra boundary vectors from `crates/verbora-stemmers/
    // src/ja.rs`'s own documented test battery: too-short, halfwidth
    // (excluded from the Katakana Unicode block), and a run of the mark
    // itself (only one ever removed).
    for (w, want) in [
        ("コピー", "コピー"),
        ("ﾀｸｼｰ", "ﾀｸｼｰ"),
        ("ーーーー", "ーーー"),
        ("", ""),
    ] {
        let a = v.stem(w);
        let b = lindera_stem_one(&filter, &dictionary, w);
        assert_eq!(a, want, "verbora stem({w:?})");
        assert_eq!(b, want, "lindera stem({w:?})");
    }
}

// ---------------------------------------------------------------------------
// Indonesian: `sastrawi` (iDevoid) 0.1.1.
// ---------------------------------------------------------------------------

/// Both Verbora's port and this crate independently derive from the same PHP
/// Sastrawi reference dictionary — confirmed directly rather than assumed
/// from the matrix's dossier: both hold exactly 29,932 root words.
#[test]
fn sastrawi_shares_verboras_dictionary_size() {
    assert_eq!(SastrawiDictionary::new().length(), 29_932);
    assert_eq!(StemmerId::new().dictionary().len(), 29_932);
}

/// The real correctness pass the matrix's own dossier flagged as never
/// having been performed. Finds two genuine algorithmic gaps in this crate
/// versus the shared PHP-Sastrawi reference both ports independently target:
///
/// * No hyphenated-reduplication/compound-plural handling at all —
///   `"buku-buku"` and `"meniru-nirukan"` pass straight through unchanged,
///   where Verbora's port implements the reference's `stemPluralWord`.
/// * Only a single, non-iterated prefix-stripping pass — `"kesepersepuluhnya"`
///   needs the reference's up-to-3× `removePrefixes` loop (which Verbora's
///   port implements) to fully reduce; this crate's `stem_word` calls
///   `remove_prefixes` exactly once.
///
/// Every other word in the shared `id` list agrees byte-for-byte, confirming
/// the matrix's "Partial" (not "No") verdict, and those three words are the
/// ones `benches/stemmers.rs`'s `stemmer_id` group excludes from its own
/// benchmarked sample.
#[test]
fn sastrawi_agrees_with_verbora_except_three_documented_gaps() {
    let data = load_words();
    let dictionary = SastrawiDictionary::new();
    let stemmer = SastrawiStemmer::new(&dictionary);
    let v = StemmerId::new();

    let mut mismatches = Vec::new();
    for w in words_for(&data, "id") {
        let a = v.stem(&w).into_owned();
        let mut b = w.clone();
        stemmer.stem_word(&mut b);
        if a != b {
            mismatches.push((w, a, b));
        }
    }

    let mut got: Vec<&str> = mismatches.iter().map(|(w, _, _)| w.as_str()).collect();
    got.sort_unstable();
    let mut want = ["buku-buku", "meniru-nirukan", "kesepersepuluhnya"];
    want.sort_unstable();
    assert_eq!(
        got, want,
        "expected exactly these three documented divergences; found: {mismatches:?}"
    );

    // Pin down exactly what each gap looks like, so a future crate update
    // that fixes one of them is caught (this test would then fail on an
    // outdated `want` list above, which is the point).
    for (w, verbora_result, sastrawi_result) in &mismatches {
        match w.as_str() {
            "buku-buku" => {
                assert_eq!(verbora_result, "buku");
                assert_eq!(
                    sastrawi_result, "buku-buku",
                    "unchanged: no reduplication handling"
                );
            }
            "meniru-nirukan" => {
                assert_eq!(verbora_result, "tiru");
                assert_eq!(
                    sastrawi_result, "meniru-nirukan",
                    "unchanged: no compound-plural handling"
                );
            }
            "kesepersepuluhnya" => {
                assert_eq!(verbora_result, "sepuluh");
                assert_ne!(
                    sastrawi_result, "sepuluh",
                    "single-pass prefix stripping should under-reduce this word"
                );
            }
            other => panic!("unexpected mismatch word: {other}"),
        }
    }
}
