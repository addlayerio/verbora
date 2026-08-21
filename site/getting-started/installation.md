# Installation

Verbora is a Cargo workspace of focused crates. Depend on the ones you need, not
on a monolith: `verbora-distance` pulls in no data assets and no Unicode
character database, and `verbora-ngrams` has no dependencies at all.

## Requirements

| | |
|---|---|
| Rust | 1.85 or newer (`rust-version = "1.85"`) |
| Edition | 2024 |
| Platform | Any Rust target the crate supports; no C runtime dependency |
| `unsafe` | Denied workspace-wide (`unsafe_code = "deny"`). One narrow exception, test-only: `verbora-spellcheck`'s `counting_alloc` module. No `unsafe` is compiled into any published library. |

## Add a crate

```toml
[dependencies]
verbora-tokenizers = "0.2"
```

<div class="callout callout-note">
<strong>Pre-1.0.</strong> The crates are at <code>0.2.0</code>, which is a
breaking release relative to <code>0.1.0</code>: a caret requirement of
<code>"0.1"</code> will not resolve to it, and the examples on this site are
written against the newer API. If the version you want is not on crates.io yet,
use the git or path form below — the code is identical either way.
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
needs the shared traits or the stop-word tables; you only name it yourself if
you are writing generic code or implementing the traits.

| Crate | Depend on it for | Pulls in |
|---|---|---|
| `verbora-tokenizers` | word, segment and sentence tokenizers | core, `unicode-segmentation` |
| `verbora-distance` | Levenshtein, Damerau, OSA, Jaro–Winkler, Dice, Hamming | `rustc-hash` |
| `verbora-phonetics` | twelve encoders plus `PhoneticIndex` | core, tokenizers, `regex` |
| `verbora-ngrams` | n-gram windows, padding, character windows | *nothing* |
| `verbora-normalizers` | the four Unicode normalization forms, diacritic folding | `unicode-normalization` |
| `verbora-inflectors` | pluralise/singularise, ordinals (en/fr/ja) | `regex` |
| `verbora-trie` | prefix tree, prefix search, path matching, `FrozenTrie` | `smallvec` |
| `verbora-core` | the shared traits and `StopWords` | `rustc-hash` |
| `verbora-stemmers` | sixteen stemmers across fourteen languages | core, tokenizers |
| `verbora-spellcheck` | correction and fuzzy/deletion indexes | distance, `rustc-hash` |
| `verbora-tagger` | Brill POS tagging, training and testing | `rustc-hash`, embedded lexicons |
| `verbora-analyzers` | sentence structure analysis over POS-tagged input | *nothing* |
| `verbora-language` | script/language detection and phonetic recommendations | phonetics, transliterators, optional `whatlang` |
| `verbora-transliterators` | Japanese kana-to-romaji transliteration | normalizers |
| `verbora-wordnet` | WordNet lookup and relation traversal | `memchr`, `rustc-hash` |
| `verbora-tfidf` | sparse TF-IDF indexing, querying and JSON persistence | core, tokenizers, `rustc-hash`, `serde` |
| `verbora-sentiment` | multilingual lexicon sentiment | stemmers, tokenizers, `rustc-hash` |
| `verbora-classifiers` | Bayes, logistic regression and MaxEnt | stemmers, tokenizers, `rustc-hash` |
| `verbora-util` | stop words, abbreviations, graphs, path trees | core, `rustc-hash` |

`rayon` is absent from every row above because it is optional everywhere it is
used — see [Cargo features](cargo-features.md).

A typical text pipeline:

```toml
[dependencies]
verbora-tokenizers = "0.2"
verbora-normalizers = "0.2"
verbora-ngrams = "0.2"
```

A fuzzy-matching service:

```toml
[dependencies]
verbora-distance = "0.2"
verbora-phonetics = "0.2"
verbora-trie = "0.2"
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
