# Verbora

A high-performance, Rust-native natural language processing toolkit:
tokenization, stemming, phonetic matching, string distance, n-grams,
normalization, inflection, a trie, transliteration, WordNet, TF-IDF,
sentiment analysis, and Bayes/logistic-regression/MaxEnt classifiers — each
designed for Rust from the start.

> **Status: pre-1.0; every production crate is implemented, documented, and test-pinned.**

## Documentation

**[Read the documentation →](https://addlayerio.github.io/verbora/)**

The site is not a summary of the API — it is where the library explains *when*
to use each of its variants and what each one costs. Its centrepiece is
**Choosing the Right API**: whenever more than one function solves the same
conceptual problem, the docs must say why each exists, what it allocates,
whether it is lazy, whether it reuses memory, and which one you should call.

The complete documentation layout — public site, Rust API contracts and
internal engineering records — is defined in [DOCUMENTATION.md](DOCUMENTATION.md).
Contribution expectations are in [CONTRIBUTING.md](CONTRIBUTING.md).

| | |
|---|---|
| [Getting started](https://addlayerio.github.io/verbora/getting-started/installation) | Install, first program, workspace map |
| [Choosing the Right API](https://addlayerio.github.io/verbora/choosing/) | Comparison tables, decision trees, trade-offs |
| [Features](https://addlayerio.github.io/verbora/features/) | Every implemented subsystem |
| [Performance](https://addlayerio.github.io/verbora/performance/) | Borrowing, laziness, `Cow`, buffer reuse, batching, parallelism — and the measured results against competing libraries |
| [Recipes](https://addlayerio.github.io/verbora/recipes/) | Organised by workload, not by function name |

Source in [`site/`](site/), built with [VitePress](https://vitepress.dev)
and published from `main`. Building it locally:

```bash
cd site
npm install
npm run dev                            # http://localhost:5173

python3 check-snippets.py              # every published example compiles and passes
npm run build && python3 check-links.py  # no dangling internal link or anchor
```

Documentation is treated as part of the code: a change to public behaviour that
does not update the site is an incomplete change. The rules are in
[`AGENTS.md`](AGENTS.md#documentation-is-part-of-the-code).

## How correctness is verified

Every public behaviour is written down before it is implemented, and every
documented behaviour is pinned by a test that fails when it changes. Nothing
is left to "obvious from the code": defaults, error cases, Unicode handling,
ordering and floating-point behaviour are all specified explicitly.

Where a subsystem's output is large or structured — tagger output, WordNet
lookups, classifier scores — it is pinned with committed golden files
reviewed by hand once and diffed on every run thereafter. A diff in a golden
file is a behaviour change and has to be justified.

## Layout

```
crates/
  verbora-core/            traits, Token, stop-word state, string semantics
  verbora-tokenizers/      25 tokenizers, the Tokenize trait
  verbora-distance/        Levenshtein, Damerau, Jaro–Winkler, Dice, Hamming
  verbora-phonetics/       SoundEx, Metaphone, Double Metaphone, D-M Soundex
  verbora-ngrams/          n-gram windows, frequency stats, Chinese n-grams
  verbora-normalizers/     diacritics, contractions, Japanese width/kana
  verbora-inflectors/      pluralise/singularise, ordinals (en/fr/ja)
  verbora-trie/            prefix tree
  verbora-transliterators/ Japanese kana → romaji
  verbora-wordnet/         lexical database, synsets, relation traversal
  verbora-tfidf/           term interning, incremental idf cache
  verbora-sentiment/       fourteen lexicons across ten languages
  verbora-classifiers/     Bayes, logistic regression, MaxEnt + GIS
  verbora-analyzers/       sentence analysis
  verbora-spellcheck/      Norvig-style correction
  verbora-stemmers/        Porter × 13, Lancaster, Japanese, Indonesian
  verbora-tagger/          Brill POS tagger, trainer, tester
  verbora-language/        script and language detection, phonetic strategy
  verbora-util/            stop words, abbreviations, graph utilities
  verbora-examples/        the code the documentation site publishes (dev-only)
site/                 the documentation site (VitePress) — see "Documentation" above
docs/                 internal engineering/research archive, not user docs — see docs/README.md
tools/
  bench-data/           shared benchmark inputs
benches/data/         inputs shared across benchmarks
```

## Getting started

```bash
cargo test                             # unit + integration + doctests
cargo bench -p verbora-distance        # benchmarks
```

## Design commitments

**Correctness before speed, and speed proven by measurement.** The priority order
is correctness → specified behaviour → performance-aware architecture → memory
→ API quality → maintainability → hot-path tuning. No optimisation lands
without the test suite re-run, and none is claimed without a benchmark. A
measured regression is documented in
[the benchmarks page](https://addlayerio.github.io/verbora/benchmarks/distance#a-measured-regression-and-its-fix)
precisely because "it's Rust, so it's fast" would have shipped it.

**Several API levels, sharing one primitive.** A high-level, ergonomic API; a
lazy iterator; a low-level one that writes into caller-supplied buffers so
hot loops can amortise allocation; and, for thirteen crates where a real
benchmark justified it, an opt-in parallel batch API — each built on top of
the one before it, never a second implementation:

```rust
tokenizer.tokens(text)                // lazy iterator — the primitive
tokenizer.tokenize(text)              // owned, ergonomic — collects the iterator
tokenizer.tokenize_into(text, &mut v) // reuses the caller's buffer
tokenizer.tokenize_borrowed(text)     // &str slices, zero copies
```

**Fast paths that are exact, not approximate.** String distances count Unicode
scalar values — one `char` is one unit — so `levenshtein("a😀b", "ab")` is 1.
`verbora-distance` runs ASCII operands over `&[u8]` and promotes only
genuinely non-ASCII input to `Vec<char>`. For ASCII, one byte *is* one scalar,
so the fast path computes the same answer as the general one rather than a
cheaper approximation of it.

**Explicit, not flattering.** No metric in `verbora-distance` rewrites its
input to make a score look better: nothing folds case, trims, collapses
whitespace or normalises, so `dice_coefficient("ABC", "abc")` is `0.0` and
`hamming("ABC", "abc")` is `Some(3)`. Caseless matching belongs to the caller,
applied once at ingestion rather than re-applied against every candidate.
Absence is spelled as absence rather than as a magic number — `hamming`
returns `None` when the two operands have different scalar counts, no
similarity in the crate ever returns `NaN`, and no function in it panics on
any input. Each of these is a deliberate, documented choice with a test behind
it, not an accident smoothed over in prose.

## Measured performance

`verbora-distance` across its own API levels and input sizes — full table and
method in [the benchmarks page](https://addlayerio.github.io/verbora/benchmarks/distance).

| Benchmark | Verbora |
|---|--:|
| `levenshtein/ascii/1024` | 27.5 µs |
| `levenshtein/ascii/16` | 41.1 ns |
| `levenshtein_shapes/near/1024` | 0.35 µs |
| `levenshtein_shapes/disjoint/1024` | 1.18 µs |
| `hamming/256` | 72.8 ns |
| `dice/1024` | 10.84 µs |
| `jaro_winkler/16` | 81.9 ns |

Thirteen crates additionally ship an optional `parallel` Cargo feature for
batch workloads where a real crossover benchmark justified it — see
[Parallelism](https://addlayerio.github.io/verbora/performance/parallelism)
for which crates, and the measured numbers.

### Competitive benchmarks

Verbora is measured against the wider **Rust** ecosystem: 290 real,
version-pinned benchmarks across 13 modules — strsim, rapidfuzz, tantivy,
rust-stemmers, rphonetic, symspell, harper-core, smartcore and more — plus a
language-detection accuracy report. Every loss is published alongside every
win, with its investigated cause. See
[Competitive benchmarks](https://addlayerio.github.io/verbora/benchmarks/competitive)
for the full results and [`docs/PERFORMANCE_GAPS.md`](docs/PERFORMANCE_GAPS.md)
for every gap this audit found.

## Contributing a module

`crates/verbora-distance` is the worked reference for every step: specify the
behaviour in rustdoc first, implement it, pin it with tests, benchmark it, and
publish the documentation page in the same change. The rules are in
[`AGENTS.md`](AGENTS.md).

## Licence

MIT. Linguistic data assets (WordNet, sentiment lexicons) carry their original
licences and attribution — see each feature's own documentation page before
redistributing.