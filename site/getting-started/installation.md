# Installation

Verbora is a Cargo workspace of eight independent crates. You depend on the ones
you need, not on a monolith — `verbora-distance` pulls in no data assets,
`verbora-normalizers` has no dependencies at all.

## Requirements

| | |
|---|---|
| Rust | 1.85 or newer (`rust-version = "1.85"`) |
| Edition | 2024 |
| Platform | Anything Rust targets; no C dependencies, no build scripts |
| `unsafe` | Denied workspace-wide (`unsafe_code = "deny"`) |

## Adding a crate

<div class="callout callout-note">
<strong>Publication status.</strong> The crates are at version <code>0.1.0</code>
in this workspace. If they are not yet on crates.io for your target version,
use the git or path form below — the code is identical either way.
</div>

From crates.io:

```toml
[dependencies]
verbora-tokenizers = "0.1"
```

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
| `verbora-ngrams` | n-gram windows, frequency stats, Chinese n-grams | `verbora-core` |
| `verbora-normalizers` | diacritic folding, English contractions, Japanese width/kana | *nothing* |
| `verbora-inflectors` | pluralise/singularise, ordinals (en/fr/ja) | `verbora-core`, `regex` |
| `verbora-trie` | prefix tree, prefix search, path matching | `smallvec` |
| `verbora-core` | the shared traits, `Token`, `StopWords`, reference string semantics | *nothing* (optional `serde`) |

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

There is exactly one optional feature in the entire workspace, and it is off by
default. See [Cargo features](cargo-features.md).

## Building the workspace from source

```bash
git clone https://github.com/addlayerio/verbora
cd verbora

cargo build --workspace          # everything
cargo test  --workspace          # unit + integration + doctests
cargo bench -p verbora-distance  # Criterion benchmarks
```

The test suites replay large recorded corpora, so the workspace sets
`opt-level = 2` for the `test` and `dev` profiles — an unoptimised debug build
makes them unusably slow.

## Building this documentation

The site is a VitePress project at `site/`:

```bash
cd site
npm install
npm run dev       # live reload at http://localhost:5173
npm run build     # static output to .vitepress/dist/
```

To check that every Rust snippet on this site still compiles *and passes its
assertions* against the real crates — this is what CI runs on every push, and a
snippet that stops compiling or holding breaks the build:

```bash
python3 site/check-snippets.py
```

And that the build has no broken internal links or anchors, and every page is
reachable from the sidebar:

```bash
cd site && npm run build && cd - && python3 site/check-links.py
```

## Next

- [Your first program](first-program.md) — three versions of the same task, one
  per API shape.
- [The workspace map](workspace.md) — what lives where, and why the crates are
  split the way they are.
