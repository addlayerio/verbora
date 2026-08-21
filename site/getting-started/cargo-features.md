# Cargo features

Every optional capability is off by default. A plain dependency therefore stays
sequential, pulls in no Rayon and no statistical language detector, and costs
you nothing you did not ask for.

## Available features

| Crate(s) | Feature | What it enables |
|---|---|---|
| 14 crates, listed below | `parallel` | Explicit `par_*` batch APIs backed by Rayon. |
| `verbora-language` | `language-detection` | `WhatlangDetector`, backed by the optional `whatlang` dependency. |
| `verbora-language` | `fast-language-detection` | `HashedLinearDetector`, using compiled-in model weights and no extra dependency. |

Those three are the whole list. Nothing here gates a serialization hook: where
serialization exists it is unconditional — `verbora-tfidf` persists a corpus
through `TfIdf::to_json` / `TfIdf::from_json`, and `verbora-classifiers` through
its own model format — so there is no feature to turn on and no feature that
compiles to nothing.

## `parallel`

Available in:

```text
verbora-analyzers       verbora-classifiers     verbora-distance
verbora-language        verbora-normalizers     verbora-phonetics
verbora-sentiment       verbora-spellcheck      verbora-stemmers
verbora-tagger          verbora-tfidf           verbora-tokenizers
verbora-transliterators verbora-wordnet
```

It never turns a sequential API into an implicitly parallel one. You opt in
twice: once to the Cargo feature, once to an explicitly named `par_*` call.

```toml
[dependencies]
verbora-tokenizers = { version = "0.2", features = ["parallel"] }
verbora-language = {
  version = "0.2",
  features = ["fast-language-detection", "parallel"]
}
```

Use it for batches large enough to amortize scheduling; for a single item or a
small batch, the ordinary sequential API wins. See
[Parallelism](../performance/parallelism.md) for the available operations and
the measured crossover points.

## Crates without optional features

`verbora-core`, `verbora-inflectors`, `verbora-ngrams`, `verbora-trie` and
`verbora-util` have no optional features. `default-features = false` is harmless
anywhere in the workspace, because every default feature set is empty.

Both extremes of the feature matrix are supported:

```bash
cargo test --workspace --no-default-features
cargo test --workspace --all-features
```

## Build profiles

| Profile | Settings | Use it for |
|---|---|---|
| `release` | `opt-level = 3`, thin LTO, 16 codegen units | Normal production builds |
| `release-max` | fat LTO, one codegen unit | Maximum runtime speed when longer builds are acceptable |

```bash
cargo build --profile release-max
```
