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

<div class="callout callout-warn">
<strong>Upgrading from 0.1?</strong> <code>0.2.0</code> is a breaking release: a
caret requirement of <code>"0.1"</code> will not resolve to it, every module
path is gone in favour of the crate root, and several results changed without
changing type. <a href="upgrading">Upgrading from 0.1 to 0.2</a> is the
old-signature-to-new-signature guide, including the changes that do not produce
a compile error.
</div>

<div class="callout callout-note">
<strong>Pre-1.0, and versioned per crate.</strong> The examples on this site
are written against <code>0.2</code>, which is where eighteen of the nineteen
crates are. <code>verbora-wordnet</code> is at <code>0.3</code>: a crate moves
only when <em>its own</em> API does, so the numbers are deliberately not in
lockstep. If the version you want is not on crates.io yet, use the git or path
form below — the code is identical either way.
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
and pulls in nothing extra. Features add explicit parallel batch APIs and two
language-detection implementations — see [Cargo features](cargo-features.md).

<div class="callout callout-warn">
<strong>Removed in 0.2.0.</strong> <code>verbora-core</code> declared a
<code>serde</code> feature that nothing in the crate ever used — no item derived
or implemented a <code>serde</code> trait. It is gone, and no crate in 0.2.0 has
one. A dependency written as
<code>verbora-core = { version = "0.1", features = ["serde"] }</code> fails to
resolve once the pin moves to <code>"0.2"</code>; drop the
<code>features</code> key. Serialization lives in the crates that actually do it
— <code>verbora-tfidf</code> and <code>verbora-classifiers</code> — where
<code>serde</code> is a plain dependency, not a feature.
</div>

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
- [Upgrading from 0.1 to 0.2](upgrading.md) — what broke, what silently changed,
  and what to write instead.
