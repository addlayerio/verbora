# Installation

Verbora is a Cargo workspace of focused crates. Depend on the ones you need, not
on a monolith: `verbora-distance` pulls in no data assets, and
`verbora-normalizers` has no dependencies at all.

## Requirements

| | |
|---|---|
| Rust | 1.85 or newer (`rust-version = "1.85"`) |
| Edition | 2024 |
| Platform | Any Rust target the crate supports; no C runtime dependency |
| `unsafe` | Denied workspace-wide (`unsafe_code = "deny"`) |

## Add a crate

```toml
[dependencies]
verbora-tokenizers = "0.1"
```

<div class="callout callout-note">
<strong>Pre-1.0.</strong> The crates are at <code>0.1.0</code>. If the version
you want is not on crates.io yet, use the git or path form below — the code is
identical either way.
</div>

From git:

```toml
[dependencies]
verbora-tokenizers = { git = "https://github.com/addlayerio/verbora" }
```

From a local checkout:

```toml
[dependencies]
verbora-tokenizers = { path = "../verbora/crates/verbora-tokenizers" }
```

## Which crate do I need?

Each crate stands alone. `verbora-core` is pulled in transitively when a crate
needs the shared traits; you only name it yourself if you are writing generic
code or implementing the traits.

| Crate | Depend on it for | Pulls in |
|---|---|---|
| `verbora-tokenizers` | 25 tokenizers, the `Tokenize` trait | `verbora-core`, `regex` |
| `verbora-distance` | Levenshtein, Damerau, Jaro–Winkler, Dice, Hamming | `verbora-core`, `rustc-hash` |
| `verbora-phonetics` | SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff | `verbora-core`, `regex` |
| `verbora-ngrams` | n-gram windows, frequency stats, Chinese n-grams | `verbora-core`, `rustc-hash` |
| `verbora-normalizers` | diacritic folding, English contractions, Japanese width/kana | *nothing* |
| `verbora-inflectors` | pluralise/singularise, ordinals (en/fr/ja) | `verbora-core`, `regex` |
| `verbora-trie` | prefix tree, prefix search, path matching | `smallvec` |
| `verbora-core` | the shared traits, `Token`, `StopWords`, whitespace helpers | *nothing* (optional `serde`) |
| `verbora-stemmers` | stemming for sixteen languages | core, tokenizers, normalizers |
| `verbora-spellcheck` | correction and fuzzy/deletion indexes | core, distance, trie, `rustc-hash` |
| `verbora-tagger` | Brill POS tagging, training and testing | core, `rustc-hash`, embedded lexicons |
| `verbora-analyzers` | sentence structure analysis over POS-tagged input | core |
| `verbora-language` | script/language detection and phonetic recommendations | core, phonetics, transliterators |
| `verbora-transliterators` | Japanese kana-to-romaji transliteration | normalizers |
| `verbora-wordnet` | WordNet lookup and relation traversal | core, `memchr` |
| `verbora-tfidf` | sparse TF-IDF indexing and querying | core, tokenizers, `rustc-hash`, `serde` |
| `verbora-sentiment` | multilingual lexicon sentiment | stemmers, `rustc-hash` |
| `verbora-classifiers` | Bayes, logistic regression and MaxEnt | core, stemmers, `rustc-hash` |
| `verbora-util` | stop words, abbreviations, graphs and storage | core, `rustc-hash`, `serde` |

A typical text pipeline:

```toml
[dependencies]
verbora-tokenizers = "0.1"
verbora-normalizers = "0.1"
verbora-ngrams = "0.1"
```

A fuzzy-matching service:

```toml
[dependencies]
verbora-distance = "0.1"
verbora-phonetics = "0.1"
verbora-trie = "0.1"
```

## Cargo features

Every optional feature is off by default, so a plain dependency stays sequential
and pulls in nothing extra. Features add explicit parallel batch APIs, two
language-detection implementations, and a `serde` hook on the core types — see
[Cargo features](cargo-features.md).

## Building from source

```bash
git clone https://github.com/addlayerio/verbora
cd verbora

cargo build --workspace          # everything
cargo test  --workspace          # unit + integration + doctests
cargo bench -p verbora-distance  # Criterion benchmarks
```

The test suites replay large recorded corpora, so the workspace sets
`opt-level = 2` for the `test` profile and for every dependency in `dev` — an
unoptimised debug build makes them unusably slow.

## Next

- [Your first program](first-program.md) — the same task written four ways, one
  per API shape.
- [The workspace map](workspace.md) — what lives where, and why the crates are
  split the way they are.
- [Cargo features](cargo-features.md) — parallel batch APIs and the other
  opt-ins.
