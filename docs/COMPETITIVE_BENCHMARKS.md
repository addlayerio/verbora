# Competitive Benchmarks — Initial Matrix (Fase 6)

This is the **initial competitive matrix** produced by the research phase of
Fase 6's competitive performance audit. It consolidates the findings of 7
parallel Competitor Research Agent dossiers, one per module group:

| Agent | Modules covered |
|---|---|
| A | Tokenizers + N-Grams |
| B | Stemmers + Normalizers + Inflectors |
| C | Phonetics + Phonetic Index / Phonetic Neighbors |
| D | Distances |
| E | Language Detection + Script Detection + Transliteration |
| F | TF-IDF + Classifiers + Sentiment |
| G | WordNet + POS Tagging + Spellcheck + Trie + Analyzers |

Per the spec's own **PRINCIPIO FUNDAMENTAL**: Verbora is an all-in-one NLP
library, but the Rust ecosystem is segmented into specialized crates. This
audit therefore does **not** search for "one Rust library equivalent to
Verbora" — it searches, module by module and algorithm by algorithm, for the
best-known/relevant real competitor(s), and honestly records
`NO FAIR COMPETITOR FOUND` wherever none exists rather than inventing a
comparison. Every module in the spec's `MODULE-BY-MODULE AUDIT` list is
covered below, confirmed against the 19 NLP crates in this workspace
(`Cargo.toml`'s `[workspace] members`): `verbora-tokenizers`, `verbora-ngrams`,
`verbora-stemmers`, `verbora-normalizers`, `verbora-inflectors`,
`verbora-phonetics` (includes `PhoneticIndex`), `verbora-distance`,
`verbora-language` (language detection + script detection),
`verbora-transliterators`, `verbora-tfidf`, `verbora-classifiers`,
`verbora-sentiment`, `verbora-wordnet`, `verbora-tagger` (POS tagging),
`verbora-spellcheck`, `verbora-trie`, `verbora-analyzers`. No additional
implemented module beyond this list was surfaced by any research agent
(`verbora-core`, `verbora-util`, and `verbora-examples` are
internal infrastructure/test-support crates, not independently benchmarkable
NLP capabilities, and none of the 7 reports flagged them as such).

**This is the initial, pre-benchmark matrix.** Per the spec's `VERSION
PINNING` section, exact locked-and-pinned versions with a reproducible
lockfile are a later step; every version quoted below is a real version the
research agents actually found live (crates.io / GitHub, fetched during
this research pass), not a placeholder — but it is not yet the final pinned
set used for execution.

## How to read this matrix

- **Equivalent?** — `Yes` (same algorithm/scope), `Partial` (same general
  task, documented divergence in algorithm, scope, or output), `No`
  (investigated and rejected — different algorithm/scope, kept for the
  record per the spec's "document every real candidate found" requirement),
  or blank/`—` where the row exists only to record a `NO FAIR COMPETITOR
  FOUND` outcome.
- **Benchmarkable?** — `Yes`, `Selected cases` (only a restricted
  input domain or configuration is fair — documented in the Notes column and
  in the dossier below), or `No`.
- Rows with `NO FAIR COMPETITOR FOUND` are kept in the matrix (competitor
  column literally reads `—`) precisely so each required module can be
  confirmed present even when research turned up nothing fair to compare
  against, per the task's explicit requirement not to silently drop a
  module.
- The reference is pinned at **8.1.1** everywhere it appears. All 7 reports
  independently verified this against the vendored copy in use at research
  time — no version drift to reconcile.

---

# 1. Competitive Matrix

## 1.1 Tokenizers

**Update, text-shaping migration (2026-08) — every Verbora figure in this
section is retired, and eight of the capabilities the rows describe no longer
exist.** `verbora-tokenizers` was rewritten to the Rust-native contract in
`docs/design/text-shaping-contract.md` and now exposes three tokenizers over
one token shape — `WordTokenizer`, `SegmentTokenizer`, `SentenceTokenizer`,
every token a borrowed `&str` substring of the input. Row group by row group:

- **Whitespace tokenization — capability deleted, benchmark group deleted.**
  The `RegexpTokenizer`/`Pattern` engine this row's Verbora side was a
  configuration of is removed by contract §3.4, along with
  `verbora-tokenizers`' `regex` dependency: Verbora performs no regex or
  whitespace tokenization at *any* API, and a caller who wants it is told to
  use `regex` directly. `benches/tokenizers.rs`' `whitespace_tokenization`
  group went with it, so `tantivy::WhitespaceTokenizer` and Hugging Face
  `WhitespaceSplit` have no Verbora counterpart left to time. Every figure in
  the tantivy row — the original flat 3.6×–4.6× loss *and* the 1.11× / 1.70× /
  2.36× / 1.97× reversal that closed it, together with the 18.8×–33.1×
  Hugging Face win — measured the SWAR whitespace scanner inside that engine.
  ⚠ **Retired. No current figure replaces them, and none can until Verbora
  reacquires the capability** (`docs/PERFORMANCE_GAPS.md` entry 3).
- **`AggressiveTokenizer` (English) vs. `unicode_words()` — benchmark group
  deleted.** `AggressiveTokenizer` and its fifteen language variants are
  removed by contract §3.4; §4.1 records why (nineteen hand-derived character
  classes with documented bugs, not linguistics). The
  `aggressive_tokenization_en` group is gone, and its result — "roughly at
  parity at every size (16-8192 words), no consistent directional winner
  across two runs, differences under ~1.3× either way" — is ⚠ **retired**.
  The comparison it stood for is not lost, but it is no longer a *rival*
  comparison; see the next item.
- **`WordTokenizer` vs. `unicode-segmentation` — renamed, and reclassified out
  of the competitive set.** `WordTokenizer::tokens` is literally
  `str::unicode_words()` (`crates/verbora-tokenizers/src/word.rs`), so this row
  measures Verbora against its own dependency, which `AGENTS.md`
  § Cross-Implementation Benchmark Fairness forbids reporting as a competitive
  result. The `word_tokenization_unicode_segmentation` group was therefore
  renamed `word_tokenization_wrapper_overhead`, following
  `benches/language.rs`' existing `whatlang_wrapper_overhead` precedent: its
  numbers state what Verbora's wrapper costs over the primitive and are never
  to be reported as Verbora beating or losing to `unicode-segmentation`. Both
  recorded results — the "roughly at parity, ratios under ~1.3× in either
  direction" against `unicode_words()` and the 5.5×–8.3× margin over
  `split_word_bounds()` — are ⚠ **retired on two independent counts**: they
  measured the deleted `WordRuns` class scanner, and they are the wrong *kind*
  of claim for the group they now live in.
- **`WordTokenizer` vs. `tantivy::SimpleTokenizer` / Hugging Face
  `Whitespace`, and `SentenceTokenizer` vs. `segtok` — capabilities survive,
  figures do not.** `word_tokenization`, `sentence_tokenization` and
  `sentence_tokenization_boundary_density` remain genuine rival comparisons,
  and their narrowed-domain boundary agreement is re-proved against the new
  implementations in `tests/tokenizers_correctness.rs` before any timing is
  trusted. But both Verbora tokenizers were reimplemented on UAX #29 —
  `WordTokenizer` now does categorically *more* than a character-class test
  (full WB1–WB999 segmentation), and `SentenceTokenizer` is built on
  `split_sentence_bound_indices()` with no placeholder mask, no unmask pass and
  no trimming (contract §3.1 removed trimming outright, because a tokenizer
  that trims does not return substrings). The `segtok` row's 15.6×–24.0× /
  5.9× margins, and the `SentenceTokenizer` row's whole crossover story — the
  original `O(sentences²)` loss up to 3.40× and the flat 1.14×–1.22× reversal
  that followed — all measured the mask-and-restore algorithm.
  ⚠ **Pending re-measurement** (`docs/PERFORMANCE_GAPS.md` entries 4 and 23).
- **`WordPunctTokenizer`, `TreebankWordTokenizer`, `CaseTokenizer`,
  `OrthographyTokenizer`, the generic `RegexpTokenizer` engine and
  `TokenizerJa` — capabilities deleted** by contract §3.4 (`TokenizerJa` with
  its whole `ja/` subtree; `TreebankWordTokenizer` deferred rather than
  dropped, §4.5). These rows carry no figures, so nothing is retired, but
  their `NO FAIR COMPETITOR FOUND` verdicts are moot in a new way: the answer
  is now "no Verbora side", not "no competitor". The `CaseTokenizer`
  `İstanbul`→`İstanbulundefined` bug and the `SentenceTokenizer` non-fixpoint
  unmask bug the rows cite as parity targets are gone with the code.

⚠ **No Verbora tokenizer figure in this section is currently backed.**
Competitor figures are unaffected; no competitor version moved. `results/raw/
tokenizers-*` and the `tokenizers` rows of `results/results.json` are stale
rather than approximately right, and must not be republished from anything but
a fresh full-precision run. `docs/design/text-shaping-contract.md` §7 item 1
names the specific question that run must answer: UAX #29 word segmentation
against the deleted class scan, where a regression is expected and lands on
the hottest path in the workspace.

### Whitespace / simple tokenization

Neither the reference nor Verbora ship a *named* whitespace-only tokenizer
class — the capability exists in both as a configuration of the generic
regex-splitting engine, not a preset.

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Whitespace tokenization (`RegexpTokenizer` configured with `\s+`) | the reference `RegexpTokenizer` (same config) | reference | 8.1.1 | Yes | Yes | Same generic engine, same pattern, same `gaps` semantics on both sides. |
| Whitespace tokenization | `tantivy::tokenizer::WhitespaceTokenizer` | Rust | 0.26.1 | Partial | Yes | Splits on `char::is_ascii_whitespace()` (confirmed by reading `tantivy-0.26.1/src/tokenizer/whitespace_tokenizer.rs` directly — narrower than an earlier draft of this note assumed) — ASCII whitespace only, vs. Verbora/the reference `\s`'s wider Unicode set (e.g. NBSP, U+2028/U+2029). Diverges from Verbora/the reference `\s` on any non-ASCII whitespace input; the executed benchmark (`benchmarks/competitive/rust-competitors/benches/tokenizers.rs`) narrows its input domain to ASCII-only text specifically to stay inside the region where this divergence cannot manifest. **Benchmarked** — originally a flat ~3.6×–4.6× loss at every size, filed as `docs/PERFORMANCE_GAPS.md` entry 3 (the general `captures_iter`-driven regex engine vs. tantivy's hand-written scanner). **That gap is now closed and reversed** (entry 3's own later update): capture-free patterns route through `find_iter`, and the exact `\s+` pattern through a dedicated ASCII-first SWAR whitespace scanner — proven identical to the regex engine's `\s` by exhaustive test over every Unicode scalar value (`\s` == `char::is_whitespace` agreement), so the full-Unicode semantics tantivy lacks are preserved, not narrowed away for speed. Verbora now **wins at every size**: 1.11× at 123 B, 1.70× at 1187 B, 2.36× at 9709 B, 1.97× at 77684 B. |
| Whitespace tokenization | `tokenizers::pre_tokenizers::whitespace::WhitespaceSplit` (Hugging Face) | Rust | 0.23.1 | Partial | Selected cases | Same `char::is_whitespace` split; must be benchmarked as an isolated `PreTokenizer::pre_tokenize_str` call, never through the full BPE/WordPiece pipeline. |

### Word tokenization — `WordTokenizer`

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `WordTokenizer` (splits on `[^A-Za-zА-Яа-я0-9_]+`) | the reference `WordTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target; same regex-derived character class, same `gaps`/`matching` modes. |
| `WordTokenizer` | `unicode-segmentation::unicode_words()` / `split_word_bounds()` | Rust | 1.13.3 | Partial | Selected cases | UAX#29 word-boundary algorithm vs. a fixed ASCII+Cyrillic regex class — diverges on non-ASCII/non-Cyrillic scripts and contractions. **Benchmarked** (audit round: wired into `rust-competitors/Cargo.toml`, `word_tokenization_unicode_segmentation` group in `benches/tokenizers.rs`, exact-boundary agreement on the narrowed punctuation-free domain proven by `tests/tokenizers_correctness.rs` before any timing was trusted). Real result: Verbora and `unicode_words()` are **roughly at parity** at every size (16-8192 words) — ratios stay under ~1.3× in either direction across two independent runs, consistent with measurement noise on a shared machine rather than a real directional winner. `split_word_bounds()` (filtered to non-whitespace spans, the real work a caller needs to reproduce a word list from this lower-level "every run" API) is consistently and clearly **slower — 5.5×-8.3× — than Verbora at every size**, not a loss for Verbora. |
| `WordTokenizer` | `tantivy::tokenizer::SimpleTokenizer` | Rust | 0.26.1 | Partial | Yes | Splits on `char::is_alphanumeric()`; Unicode-wide alnum class vs. Verbora's ASCII+Cyrillic-only class. |
| `WordTokenizer` | `tokenizers::pre_tokenizers::whitespace::Whitespace` (Hugging Face) | Rust | 0.23.1 | Partial | Selected cases | Pattern `\w+\|[^\w\s]+`, Unicode-aware `\w`; isolated pre-tokenizer call only. |

### Word tokenization — `AggressiveTokenizer` family (16 language variants)

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `AggressiveTokenizer` (English) + 15 variants (`Nl,De,Fr,Es,It,Pt,No,Sv,Vi,Id,Hi,Uk,Ru,Pl,Fa`) | the reference `AggressiveTokenizer` + same 15 variants | reference | 8.1.1 | Yes | Yes | Exact port target for all 16 variants, confirmed file-for-file. |
| `AggressiveTokenizer` (English variant only) | `unicode-segmentation::unicode_words()` | Rust | 1.13.3 | Partial | Selected cases | Only the English variant is plausibly comparable; UAX#29 still disagrees on contraction/hyphen edge cases. **Benchmarked** (audit round: `aggressive_tokenization_en` group in `benches/tokenizers.rs`, exact-boundary agreement on the narrowed punctuation-free domain proven by `tests/tokenizers_correctness.rs`). Real result: **roughly at parity** at every size (16-8192 words), same shape as the `WordTokenizer` row above — no consistent directional winner across two independent runs, differences under ~1.3× either way. |
| 15 language-specific variants (De, Fr, Es, Ru, Pl, Pt, No, Sv, Vi, Id, Hi, Uk, Nl, Fa) | — | — | — | No | No | **NO FAIR COMPETITOR FOUND.** Several variants intentionally reproduce the reference bugs (e.g. German drops uppercase umlauts); a Unicode-standard tokenizer cannot replicate a bug, and no Rust crate attempts these per-language classes. |

### Word + punctuation-aware tokenization — `WordPunctTokenizer`

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `WordPunctTokenizer` | the reference `WordPunctTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target; pattern `([A-Za-zÀ-ÿ-]+\|[0-9._]+\|.)`. |
| `WordPunctTokenizer` | — | Rust | — | No | No | **NO FAIR COMPETITOR FOUND.** `tantivy::SimpleTokenizer` discards punctuation (less work); HF `Whitespace` groups punctuation runs into single tokens (`"!!"`→1 token vs. Verbora's 2) — a real output difference on common input, not just an API difference. |

### Treebank-like tokenization — `TreebankWordTokenizer`

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `TreebankWordTokenizer` | the reference `TreebankWordTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target — all 17 rewrite passes, including the "Whadddya" bug and the position-dependent final-period rule. |
| `TreebankWordTokenizer` | — | Rust | — | No | No | **NO FAIR COMPETITOR FOUND.** `rust_tokenizers` and Hugging Face `tokenizers` both implement WordPiece/BPE/Unigram — vocabulary-driven subword segmentation, a fundamentally different algorithm class solving a different problem than fixed-rule word/punctuation boundary detection. |

### Sentence tokenization — `SentenceTokenizer`

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `SentenceTokenizer` | the reference `SentenceTokenizer`/`SentenceTokenizerNew` | reference | 8.1.1 | Yes | Yes | Exact port target — placeholder-substitution algorithm, abbreviation-list matching, non-fixpoint unmask bug reproduced. |
| `SentenceTokenizer` | `unicode-segmentation::UnicodeSentences`/`USentenceBounds` | Rust | 1.13.3 | Partial | Yes | UAX#29 sentence-boundary rules — locale-agnostic, no caller-configurable abbreviation list; diverges on abbreviation-heavy/URL text. **Benchmarked** (audit round: `sentence_tokenization` group in `benches/tokenizers.rs`, on a narrowed plain-declarative-sentence domain — no abbreviations/URIs/digits/quotes/brackets — where `tests/tokenizers_correctness.rs` proves all three implementations agree exactly, after trimming `unicode-segmentation`'s trailing-whitespace-attached spans, a documented formatting convention, not a boundary disagreement). Originally a genuine crossover — Verbora winning at small sizes, losing by a widening margin (up to 3.40×) at the two largest, from an `O(sentences²)` `unmask` cost the UAX#29 single-pass scanner does not pay. **That algorithmic gap is now fixed** (`docs/PERFORMANCE_GAPS.md` entry 23's own later update): `unmask` visits only a document's *relevant* placeholders per sentence instead of the whole document's placeholder map, and the crossover is **gone** — Verbora now wins at all four sizes, by a flat **1.14×–1.22×**, no longer widening with document size. |
| `SentenceTokenizer` | `segtok` | Rust | 0.1.5 | Partial | Selected cases | Rule-based, orthographic-feature sentence+word segmenter (Rust port of Python `segtok`); adoption signal is ambiguous (452K/90d downloads but only 2 GitHub stars) — likely transitive usage, flag prominently. **Benchmarked** (audit round: same `sentence_tokenization` group and narrowed domain as the row above; `segtok` also `trim()`s each returned sentence internally, confirmed by reading `segtok-0.1.5/src/segmenter/mod.rs` directly, so no whitespace normalization is needed to compare it against Verbora). Real result: Verbora is **faster at every size tested, by a wide and non-monotonic margin — 15.6×-24.0× at the three smaller sizes, 5.9× at the largest** — a clean win for Verbora, not a gap; the ambiguous adoption signal flagged in this row's own Notes is reproduced/reaffirmed here, not resolved. |
| `SentenceTokenizer` | `punkt` (ferristseng/rust-punkt) | Rust | 1.0.5 | — | No | Investigated, not selected: statistical/unsupervised Punkt algorithm (different family) and stale since 2020-01-27 (38 stars). Recorded for completeness. |

*Scouted, real, deliberately not yet pinned* (found by a later competitor-discovery pass, per this file's own "Verbora's performance target is not a fixed list of libraries" policy — see `AGENTS.md`'s Competitive Benchmark Policy — but not yet version-pinned/benchmarked, so not given a matrix row): **`icu_segmenter`** (`unicode-org/icu4x`, 12.6M downloads, current release 2.3.0) — ICU4X's own line-breaking/text-segmentation crate, a heavier and more standards-complete UAX#29+UAX#14 implementation than `unicode-segmentation`'s already-benchmarked UAX#29-only rules (entry above), plausible for the same `SentenceTokenizer`/word-boundary rows this file already benchmarks `unicode-segmentation` against, not evaluated here. **`sentencex`** (`wikimedia/sentencex`, 949.9K downloads, 0.1.30) — a Wikimedia-maintained, "wide language support" sentence segmenter, a real second candidate alongside `segtok` for the `sentence_tokenization` row above. Both confirmed to exist and be real, current, non-trivial-adoption crates as of this check (crates.io API, re-verified directly rather than assumed stale); neither has a pinned version, a `Cargo.toml` entry, or a benchmark row yet — a real, disclosed gap for a future pass, not silently dropped.

### Other tokenizer types (`CaseTokenizer`, `OrthographyTokenizer`, generic `RegexpTokenizer` engine, `TokenizerJa`)

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `CaseTokenizer` | the reference `CaseTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target, including the `İstanbul`→`İstanbulundefined` bug. |
| `CaseTokenizer` | — | Rust | — | No | No | **NO FAIR COMPETITOR FOUND** — exists specifically to reproduce a reference-runtime bug; no equivalent should exist. |
| `OrthographyTokenizer` (Finnish + `WordTokenizer` fallback) | the reference `OrthographyTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target. |
| `OrthographyTokenizer` | — | Rust | — | No | No | **NO FAIR COMPETITOR FOUND** — no Rust crate implements this per-language matcher-table API shape. |
| Generic `RegexpTokenizer` engine (`gaps`/`matching`) | the reference `RegexpTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target; the engine sections above are built from. |
| Generic `RegexpTokenizer` engine | — | Rust | — | No | No | **NO FAIR COMPETITOR FOUND** as a standalone module — comparing to Rust's own `regex` crate would be Verbora vs. its own dependency, not a competing NLP library. |
| `TokenizerJa` (script-class based) | the reference `TokenizerJa` | reference | 8.1.1 | Yes | Yes | Exact port target. |
| `TokenizerJa` | Lindera / Vibrato (dictionary-based Japanese morphological analyzers) | Rust | not pinned (not evaluated in depth) | No | No | Real Japanese morphological analyzers do dictionary/lattice-based segmentation — categorically different (and more capable) than Verbora's/the reference's script-class heuristic. |

## 1.2 N-Grams

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `ngrams`/`bigrams`/`trigrams`/`multrigrams` (array input, reference-parity padding) | the reference `NGrams.ngrams`/`.bigrams`/`.trigrams`/`.multrigrams` | reference | 8.1.1 | Yes | Yes | Exact port target, including the negative-slice re-anchoring padding quirk. |
| `ngrams_str`/`bigrams_str`/`trigrams_str` (string input, pluggable/global tokenizer) | the reference `NGrams.ngrams(string, …)` + `setTokenizer` | reference | 8.1.1 | Yes | Yes | Exact port target, including the process-global mutable tokenizer binding. |
| `ngrams_with_stats`/`ngram_key` (frequency map, `Nr`, `numberOfNgrams`) | the reference `NGrams.ngrams(seq, n, s, e, true)` stats mode | reference | 8.1.1 | Yes | Yes | Exact port target, including the `")"` empty-key `String#substr`-clamping bug. |
| `zh::ngrams_zh`/`bigrams_zh`/`trigrams_zh` (UTF-16-code-unit splitting) | the reference `NGramsZH.ngrams`/`.bigrams`/`.trigrams` | reference | 8.1.1 | Yes | Yes | Exact port target; narrower surface matched exactly. |
| Generic `ngrams()` engine, array input with `T = char` + a caller-side frequency-count fold (the same generic primitive `ngrams`/`ngrams_str`/`zh::ngrams_zh` are all built on, called directly here rather than through a string-specific wrapper) | `ngrammatic::Ngram`/`NgramBuilder` (the character n-gram + frequency-count generator `Corpus`'s own fuzzy-search feature is built on) | Rust | 0.7.0 | Partial | Yes | **Benchmarked** (`benches/ngrams.rs`). Both sides pad with `arity - 1` copies of the same character (space, by default) and slide an identical window; `tests/ngrams_correctness.rs` confirms byte-identical `(gram, count)` sets across all 20,000 words in the shared word list, at arity 2 and arity 3 (the one disclosed divergence, on inputs shorter than `arity - 1`, is not exercised by that word list — its shortest entry is 3 characters). `ngrammatic`'s headline `Corpus`/`search` fuzzy-matching feature has no Verbora equivalent and stays unbenchmarked — see the row below. **Result** (full-default Criterion, median metric, 3 independent runs, consistent direction every time): bigrams — Verbora wins all 3 runs, ~1.07×–1.16× faster; trigrams — Verbora loses all 3 runs, ngrammatic ~1.03×–1.08× faster — see `docs/PERFORMANCE_GAPS.md` entry 38. |
| `ngrams_str`/`bigrams_str`/`trigrams_str` (word-tokenizing string input), `ngrams_with_stats`/`ngram_key`, and the `zh::*` UTF-16-code-unit-splitting family | (Rust, dedicated n-gram crate) | Rust | — | No | No | **NO FAIR COMPETITOR FOUND**, for these specific capabilities. `ngrammatic`'s `Ngram`/`NgramBuilder` (row above) is now a genuine Rust competitor for plain character-level n-gram + frequency-count generation, but nothing found tokenizes into words first the way `ngrams_str` does, replicates the UTF-16 `zh::*` splitting behavior, or matches these functions' output shape. Every other dedicated Rust n-gram crate remains either abandoned (`ngrams` pwoolcoc, ~10y stale; `ngram` nytopop, 6y stale, 4 stars; `ngram-search`, abandoned) or solves a different problem (`creature_feature` = ML featurization). Most Rust code reaches for `slice::windows(n)` inline instead of a crate. |

**Update, text-shaping migration (2026-08) — three of the four Verbora rows
describe deleted functions, and the fourth's figures are retired.**
`verbora-ngrams` was rewritten to `docs/design/text-shaping-contract.md` §3.3
and its public surface is now `ngrams`, `Padded`, and
`char_ngrams`/`CharNGrams`.

- **Deleted by contract §3.4:** `bigrams`, `trigrams`, `multrigrams`,
  `ngrams_owned`, `ngrams_iter`/`NGramIter`; the whole `text` module
  (`ngrams_str`/`bigrams_str`/`trigrams_str`); the whole `stats` module
  (`ngrams_with_stats`/`ngram_key`); the `tokenizer` module and its
  process-global mutable tokenizer binding; and the whole `zh::*`
  UTF-16-code-unit-splitting family. The three rows built on them describe
  capabilities Verbora no longer has, and the quirks they were pinned to —
  the negative-slice re-anchoring padding, the `")"` empty-key
  `String#substr` clamping bug, the global tokenizer binding, `zh::split_lossy`'s
  fabricated `U+FFFD` — are gone with the code, not preserved.
- **The `ngrammatic` row's benchmark survives under the same group names**
  (`bigrams`, `trigrams` in `benches/ngrams.rs`) **but its Verbora side was
  rewritten.** `ngrams(&chars, arity, Some(' '), Some(' '))` became
  `Padded::new(&chars, arity, Some(&' '), Some(&' ')).ngrams()`: the arity is
  now a `NonZeroUsize`, and the padded sequence is materialised once instead of
  a lazy `Cow` window being cloned per gram. The recorded result — bigrams
  Verbora ~1.07×–1.16× faster in all 3 runs, trigrams `ngrammatic` ~1.03×–1.08×
  faster in all 3 runs — measured the deleted engine call, and the `Cow`-clone
  cost its "Likely reason" rests on is exactly what the rewrite removed.
  ⚠ **Retired, pending re-measurement** (`docs/PERFORMANCE_GAPS.md` entry 38
  carries the same retirement; `docs/design/text-shaping-contract.md` §7
  item 7 states the open question — `Padded` against the lazy `Cow` windows it
  replaces, direction expected to favour the new shape and unverified).

What did survive without a benchmark is correctness, and it strengthened:
`tests/ngrams_correctness.rs` now checks identical `(gram, count)` maps over
all 20,000 words at both benchmarked arities **and** over inputs shorter than
`arity` — the one case the previous revision recorded as a disclosed,
unexercised divergence. Competitor figures are unaffected; `ngrammatic` is
still pinned at 0.7.0.

## 1.3 Stemmers

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `PorterStemmer` (English, original 1980 Porter) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline; Verbora ports this exact file, including its `measure`-as-float and empty-string-falsy quirks. |
| `PorterStemmer` (English, original 1980 Porter) | rust-stemmers `Algorithm::English` | Rust | 1.2.0 | **No** | No | **Different algorithm** — rust-stemmers' "English" is Snowball Porter2, not the original 1980 Porter. Diverges on >5% of a sample vocabulary. |
| `PorterStemmer` (English, original 1980 Porter) | `porter-stemmer` (samgiles) | Rust | 0.1.2 | Partial | Yes | **Benchmarked and verified.** Real original-Porter implementation, operates on grapheme clusters — the architecture question turns out not to matter on this plain-ASCII corpus: 63/64 exact agreement with Verbora (plus all 5 documented Porter-quirk inputs). The one mismatch is a real, isolated bug unrelated to graphemes (`"sky"` → `"ski"`; both Verbora and `nltk-porter` agree it should stay `"sky"`), excluded from the benchmarked sample. Verbora **loses** on time at every size (1.62×–1.90×, re-measured after the `ends_with` fast-path landed — see `docs/PERFORMANCE_GAPS.md` entry 24's "Update" section, which also explains why this narrowed from an earlier, noisier 1.6×–5.0×-with-one-win reading) and on allocation count (1571 vs. 493 over the 63-word list) — see `docs/PERFORMANCE_GAPS.md`. |
| `PorterStemmer` (English, original 1980 Porter) | `nltk-porter` (VoiceLessQ) | Rust | 0.1.0 | Partial | Yes | Faithful port of NLTK's `PorterStemmer(ORIGINAL_ALGORITHM)`; brand-new (created 2026-06-26), essentially untested in the wild. |
| `LancasterStemmer` (English, Paice/Husk) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `LancasterStemmer` (English, Paice/Husk) | `nltk-lancaster` (VoiceLessQ) | Rust | 0.1.0 | Partial | Yes | Only real Lancaster port found in the Rust ecosystem; near-zero adoption (25 downloads), but self-reports zero mismatches against NLTK over 68K+ words. |
| `PorterStemmerDe` (German, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerDe` (German, Snowball) | rust-stemmers `Algorithm::German` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora wins**, 1.09×–1.39× faster (n=4…1024) — see `docs/PERFORMANCE_GAPS.md` entry 34. |
| `PorterStemmerDe` (German, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | Second, independent Snowball-compiler-generated port; brand-new (created 2026-03-09, 2 releases at pinning), published by the original SymSpell author. 100% byte-exact agreement, no exclusions. **Benchmarked: Verbora wins**, 1.25×–1.39× faster — the one language Verbora wins outright against both Snowball competitors; see entry 34. |
| `PorterStemmerEs` (Spanish, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerEs` (Spanish, Snowball) | rust-stemmers `Algorithm::Spanish` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 4.14×–5.29× slower — entry 34. |
| `PorterStemmerEs` (Spanish, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 6.78×–7.70× slower, the wider of the two Spanish losses — entry 34. |
| `PorterStemmerFr` (French, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerFr` (French, Snowball) | rust-stemmers `Algorithm::French` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 2.12×–2.48× slower — entry 34. |
| `PorterStemmerFr` (French, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 2.82×–3.56× slower — entry 34. |
| `PorterStemmerIt` (Italian, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerIt` (Italian, Snowball) | rust-stemmers `Algorithm::Italian` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 2.31×–2.52× slower — entry 34. |
| `PorterStemmerIt` (Italian, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 2.70×–3.03× slower — entry 34. |
| `PorterStemmerNl` (Dutch, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline; Verbora's own doc flags Dutch's sticky `suffix_e_removed` cross-call state. |
| `PorterStemmerNl` (Dutch, Snowball) | rust-stemmers `Algorithm::Dutch` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora wins**, 1.69×–2.93× faster — entry 34. |
| `PorterStemmerNl` (Dutch, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | **Must use `Algorithm::DutchPorter`, not the plainly-named `Algorithm::Dutch`** — the crate ships two Dutch algorithms and `Algorithm::Dutch` is actually Kraaij–Pohlmann, a different, non-canonical stemmer that disagrees with both Verbora and rust-stemmers on most words (confirmed by reading the crate's own algorithm list). `DutchPorter` agrees byte-exact. **Benchmarked: Verbora loses**, 1.28×–1.44× slower — the one language where the two Snowball competitors split (Verbora beats rust-stemmers but loses to this one) — entry 34. |
| `PorterStemmerNo` (Norwegian, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerNo` (Norwegian, Snowball) | rust-stemmers `Algorithm::Norwegian` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 5.17×–5.97× slower — entry 34. |
| `PorterStemmerNo` (Norwegian, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 5.99×–7.75× slower — entry 34. |
| `PorterStemmerPt` (Portuguese, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerPt` (Portuguese, Snowball) | rust-stemmers `Algorithm::Portuguese` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 3.37×–4.50× slower — entry 34. |
| `PorterStemmerPt` (Portuguese, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 6.47×–6.96× slower — entry 34. |
| `PorterStemmerRu` (Russian, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerRu` (Russian, Snowball) | rust-stemmers `Algorithm::Russian` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm, outside words containing `ё` (see the existing note above). **Benchmarked: Verbora loses**, 6.44×–9.61× slower — entry 34. |
| `PorterStemmerRu` (Russian, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | **100% byte-exact agreement including `ёлка`** — stronger than the rust-stemmers row above: this crate's `russian.sbl` carries the same ё→е fold Verbora's port does, `rust-stemmers` does not. **Benchmarked: Verbora loses**, 6.20×–8.85× slower — entry 34. |
| `PorterStemmerSv` (Swedish, Snowball) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerSv` (Swedish, Snowball) | rust-stemmers `Algorithm::Swedish` | Rust | 1.2.0 | Yes | Yes | Same canonical Snowball algorithm. **Benchmarked: Verbora loses**, 5.17×–5.63× slower — entry 34. |
| `PorterStemmerSv` (Swedish, Snowball) | `snowball_stemmers_rs` (SeekStorm) | Rust | 1.0.1 | Yes | Yes | 100% byte-exact agreement. **Benchmarked: Verbora loses**, 6.07×–7.07× slower — entry 34. |
| `CarryStemmerFr` (French, Carry variant) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline; distinct 3-pass suffix-table algorithm, not standard Snowball French. |
| `CarryStemmerFr` (French, Carry variant) | rust-stemmers `Algorithm::French` | Rust | 1.2.0 | **No** | No | Different algorithm (standard Snowball French, not Carry). |
| `CarryStemmerFr` (French, Carry variant) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — no Rust crate implements the Carry variant. |
| `PorterStemmerFa` (Persian) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline (the reference itself is a documented no-op stub — see below). |
| `PorterStemmerFa` (Persian) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — the reference's own Farsi "stemmer" is a documented no-op identity function; there is no real algorithm here to compare against. |
| `PorterStemmerUk` (Ukrainian) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PorterStemmerUk` (Ukrainian) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — Ukrainian is not an official Snowball algorithm and absent from rust-stemmers' 18-language list; the one unverified crate found (`porter_stemmers_rs`, ~40 downloads) was too weak to select. |
| `StemmerJa` (Japanese katakana, trailing U+30FC drop, min length 4) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `StemmerJa` (Japanese katakana, trailing U+30FC drop, min length 4) | `lindera-analysis`'s `japanese_katakana_stem` filter | Rust | 5.2.0 | Partial | Selected cases | **Package name corrected during implementation**: `lindera-filter` (the sub-crate named at research time) never published a 5.2.0 release — its own last version is 0.32.3 (2025-03-18), from before Lindera's 5.x rewrite folded token/character filters into `lindera-analysis` instead (confirmed by reading both crates' published source). `lindera-analysis` 5.2.0 is the real, correctly-versioned current carrier of this filter. **Benchmarked and verified**: `min = 3` (the filter's own default) reproduces `StemmerJa`'s `>= 4`-unit threshold exactly on the shared word list — Verbora wins decisively on both time (~13×–15× faster at every batch size) and allocations (0 vs. 6 over the 7-word list; Verbora's algorithm allocates nothing when it borrows, Lindera's `Vec<Token>`-batch API always allocates at least the Vec). |
| `StemmerId` (Indonesian, Sastrawi/Nazief–Adriani) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `StemmerId` (Indonesian, Sastrawi/Nazief–Adriani) | `sastrawi` (iDevoid/rust-sastrawi) | Rust | 0.1.1 | Partial | Yes | Genuine shared lineage — both Verbora and this crate independently port the same PHP Sastrawi reference (confirmed directly: both dictionaries hold exactly 29,932 root words); very low adoption (3K downloads), untouched since 2020. **Benchmarked and verified**: this is the real correctness pass the matrix itself never performed, and it found two genuine algorithmic gaps versus the shared reference — no hyphenated-reduplication/compound-plural handling at all, and only a single (not iterated-up-to-3×) prefix-stripping pass; 13 of 16 benchmarked words agree byte-for-byte, the three exercising those gaps excluded from the benchmarked sample. Verbora **loses** decisively on time (`sastrawi` ~3.6×–6.8× faster across all batch sizes) despite a mixed memory picture (fewer allocations, 464 vs. 525, but more bytes, 39,676 vs. 12,168, over the 13-word list) — see `docs/PERFORMANCE_GAPS.md`. `sastrawi`'s own one-time dictionary+regex-compilation construction cost (~47K allocations, ~21 MB) is real but paid once, unlike Verbora's `StemmerId::new()`, a zero-sized unit struct backed entirely by compiled-in static data. |
| `StemmerId` (Indonesian, Sastrawi/Nazief–Adriani) | `sastrawi-rs` (ibahasa) | Rust | not on crates.io at research time | Partial | Selected cases | GitHub-only, "zero-regex/zero-copy/FST-powered" rewrite; no pinned release existed at research time so it could not be version-pinned per spec's "no implicit latest" rule. **Re-checked during this round's implementation: this crate now has real crates.io releases** (newest `0.5.3`) — this round's own assigned scope kept it unpinned regardless (see §5 data-quality note 2); a real candidate for a **future** pinning pass, not benchmarked here. |

⚠ **No Verbora stemmer ratio in this section is currently backed.** Competitor
figures are unaffected; no competitor version moved. The equivalence verdicts
are unaffected too — the rewrite below is pinned byte-exact against the
implementation it replaced, and `tests/stemmers_correctness.rs` still asserts
agreement with both competitors. Only the Verbora timings are retired, and they
are retired *pending re-measurement*: the comparison is live and
`benches/stemmers.rs` is waiting to answer it.

**What changed underneath them.** Entry 34 diagnosed the gap as a *linear*
suffix scan — `for s in suffixes { ends_with(w, s) }` in
`crates/verbora-stemmers/src/units.rs`' `longest_suffix`/`first_suffix` — and
recorded that the competitors' real advantage, the official Snowball compiler's
`find_among`/`find_among_b` binary search over a sorted table with common-prefix
tracking, "was not reimplemented here". **It has been since.** `longest_suffix`
and `first_suffix` no longer exist; `crates/verbora-stemmers/src/among.rs`
implements that same binary search (sorted by reversed code-unit sequence,
`common_i`/`common_j` prefix tracking, `substring_i`-style links), and ten of
the twelve benchmarked groups now route through it — `porter_de`, `porter_en`,
`porter_es`, `porter_fr`, `porter_it`, `porter_nl`, `porter_no`, `porter_pt`,
`porter_ru`, `porter_sv`, i.e. every language whose module imports
`crate::among`. That is a different algorithm on the timed path, not a
constant-factor tweak, so **the argument this paragraph used to make — that the
only post-measurement change was the `ends_with` fast path, "a single-digit-
percentage narrowing, not a reclassification" — no longer covers the table.**
Whether any row's verdict flips is unknown, must not be inferred from the
direction of the change, and is exactly what the re-run has to answer.
`results/raw/stemmers-*` and the `stemmers` rows of `results/results.json` are
stale rather than approximately right.

**Two groups are *not* retired on this ground.** `stemmer_id` and `stemmer_ja`
do not reach `among.rs` or `ends_with` at all (`id.rs` uses
`eq_str`/`starts_with`, `ja.rs` only `slen`), so the `sastrawi` and `lindera`
ratios are untouched by the suffix-matching rewrite. They are in scope of the
text-shaping migration's own open question for this crate — see §7's
"Downstream reach", where `verbora-stemmers` is listed as *named, not
resolved*.

**Two further changes that do *not* reach these numbers, recorded so the
distinction is not lost.** `verbora-stemmers` now tokenizes with
`verbora_tokenizers::WordTokenizer` (UAX #29) instead of fourteen deleted
per-language character classes, and `PorterStemmerNo`/`PorterStemmerSv`'s
`prepare` no longer folds diacritics (`å ä ö` and `æ ø å` are letters of those
alphabets, and the fold had made 124 stop words unreachable). Both change
*stems*, and both sit in `tokenize_and_stem`; `benches/stemmers.rs` times
`stem(word)` per word over a fixed word list and never calls it, so neither is
a reason to retire a figure here. Entry 10 carries the `tokenize_and_stem`
question separately.

*Historical: entry 34's own "Update" section records the two provably-correct
`longest_suffix` rewrites that were tried before this one, measured, found to
regress Spanish and French specifically, and reverted — and the `ends_with`
fast path that did ship.*

## 1.4 Normalizers

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | `unaccent` | Rust | 0.1.1 | Partial | Selected cases | NFD-decomposition-based (different mechanism from Verbora's non-decomposing table lookup); license field is "non-standard" on crates.io — verify actual terms. |
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | `diacritics` (YesSeri) | Rust | 0.2.2 | Partial | Selected cases | Case-preserving, closer semantic shape; GPL-3.0; only 5 GitHub stars, thin adoption. |
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | `secular` | Rust | 1.0.1 | **No** | No | Forces lowercasing as part of its only public API — does different/additional work than Verbora's case-preserving function. |
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | `deunicode` / `any_ascii` / `unidecode` | Rust | 1.6.2 / 0.3.3 / 0.3.0 | **No** | No | Full Unicode-to-ASCII transliterators (romanize CJK, Cyrillic, Greek) — categorically more/different work than a Latin-only fold. |
| `remove_diacritics` (Latin diacritic fold, case-preserving, non-decomposing) | `unicode-normalization` | Rust | 0.1.25 | No | No | NFD/NFC decomposition primitive only, not itself a diacritic-folder; composing it with a mark-stripper is still a different algorithm (decompose-then-strip) than Verbora's direct table lookup. |
| `normalize_no` (Norwegian selective diacritic fold) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `normalize_no` (Norwegian selective diacritic fold) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — no Rust crate replicates this exact selective per-alphabet fold. |
| `normalize_sv` (Swedish selective diacritic fold) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `normalize_sv` (Swedish selective diacritic fold) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — same reasoning as `normalize_no`, an even narrower 4-letter fold. |
| `normalize`/`normalize_token` (English contraction expansion, 5-entry table + 6 rules) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `normalize`/`normalize_token` (English contraction expansion, 5-entry table + 6 rules) | `contractions` (TomLouisKeller) | Rust | 0.5.4 | **No** | No | Comprehensive contraction dictionary — much broader scope than Verbora's deliberately tiny, the reference-quirk-preserving rule set. |
| `normalize_ja` (Japanese width/kana/symbol normalization, 17 conversions) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `normalize_ja` (Japanese width/kana/symbol normalization, 17 conversions) | `unicode-jp` (gemmarx) | Rust | 0.4.0 | Partial | Selected cases | **Benchmarked** (`benches/normalizers.rs`'s `ja_hiragana_to_katakana`/`ja_katakana_to_hiragana` groups). Covers only 2 of 17 conversions — `hira2kata`/`kata2hira`, a bare codepoint shift, vs. Verbora's `hiragana_to_katakana`/`katakana_to_hiragana`, which additionally fold halfwidth katakana and fix small-tsu-before-n-row/standalone voiced marks. Verified byte-exact in `tests/normalizers_correctness.rs` on pure hiragana/katakana input (the Iroha pangram) with neither extra case; two real divergences (small-tsu fix, halfwidth folding) confirmed explicitly, not just described, and excluded from the benchmarked domain. Real result: **Verbora loses, 3.7×–4.8× slower both directions** at every tested size — filed as `docs/PERFORMANCE_GAPS.md` entry 30 (three full-string passes on this input vs. `unicode-jp`'s one). |
| `normalize_ja` (Japanese width/kana/symbol normalization, 17 conversions) | `kana-converter` | Rust | 0.1.2 | Partial | Selected cases | **Benchmarked** (`benches/normalizers.rs`'s `ja_katakana_halfwidth_to_fullwidth` group). Even narrower single-purpose kana-width converter — its `to_double_byte(_, KanaOnly)` composes voiced/semi-voiced halfwidth katakana via a raw codepoint-offset heuristic (not a table like Verbora's `katakana_hf`), which is verified to agree on the standard gojuon pairs but genuinely diverges on `ｦ`/`ﾜ` + dakuten, an orphan mark, and halfwidth punctuation/space (which it folds and `katakana_hf` does not) — three real divergences, all confirmed with assertions in `tests/normalizers_correctness.rs` and excluded from the benchmarked domain. Real result: **Verbora loses, 1.19×–1.47× slower** — a narrower, single-pass-vs-single-pass gap (filed as `docs/PERFORMANCE_GAPS.md` entry 31) despite `kana-converter` allocating 2.4× the bytes Verbora does on the same input. |
| `case::restore_case` (internal case-pattern restoration for inflectors) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline (reference behavior). |
| `case::restore_case` (internal case-pattern restoration for inflectors) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — a UTF-16-indexed, reference-quirk case-pattern restorer with no Rust ecosystem target. |

**Update, text-shaping migration (2026-08) — `verbora-normalizers` was reduced
to six functions, and every Verbora figure in this section is retired.** Per
`docs/design/text-shaping-contract.md` §3.2/§3.4 the crate's public surface is
now `nfd`, `nfc`, `nfkd`, `nfkc`, `remove_diacritics`,
`par_remove_diacritics_batch` and a `unicode_version()` accessor. (The
`case::restore_case` row above is unaffected — that function lives in
`verbora-inflectors`, not here, and was not touched by this migration.)

- **`normalize` / `normalize_token` (English contraction expansion),
  `normalize_no`, `normalize_sv`, and `normalize_ja` with `ja::converters`'
  seventeen functions — deleted.** Their rows describe capabilities Verbora no
  longer has; the `contractions`-crate rejection and the two selective-fold
  `NO FAIR COMPETITOR FOUND` verdicts are moot in the same way §1.1's are —
  the answer is now "no Verbora side", not "no competitor".
- **`ja_hiragana_to_katakana` / `ja_katakana_to_hiragana` — benchmark groups
  deleted, and the `unicode-jp` dependency with them.** Hiragana ↔ katakana
  conversion is a *transliteration*, not a Unicode normalization (contract
  §3.2, "Cut: the Japanese normalizers"), and it belongs to
  `verbora-transliterators`, which today ships only kana → romaji
  (`transliterate_ja`). Timing `unicode-jp`'s `hira2kata`/`kata2hira` alone
  would measure nothing about Verbora. The recorded result — Verbora 3.7×–4.8×
  slower in both directions at every tested size, and the later 4.1%-at-1024
  pre-check speedup that narrowed it — measured `ja.rs`'s three-stage
  `Table::translate` pipeline. ⚠ **Retired. No current figure replaces it, and
  none can until the capability reappears somewhere**
  (`docs/PERFORMANCE_GAPS.md` entry 30).
- **`ja_katakana_halfwidth_to_fullwidth` — re-pointed and renamed
  `nfkc_halfwidth_katakana`.** This capability genuinely survives: NFKC's
  compatibility decomposition maps halfwidth katakana to its fullwidth form and
  decomposes the halfwidth voiced sound mark `U+FF9E` to combining `U+3099`,
  which canonical composition then recombines, so `nfkc("ｶﾞ") == "ガ"` — the
  same user-visible operation `katakana_hf` performed. The competitor
  (`kana-converter` 0.1.2, `to_double_byte(_, KanaOnly)`) and the narrowed
  domain are unchanged, and per-character agreement is re-proved over the whole
  of `U+FF66..=U+FF9D` in `tests/normalizers_correctness.rs`. But the recorded
  1.19×–1.47× loss measured `katakana_hf`'s single purpose-built
  `Table::translate` pass, and the replacement is general UAX #15 NFKC over
  arbitrary Unicode — the two sides now do measurably different amounts of
  work, which `benches/normalizers.rs`' `bench_nfkc_halfwidth_katakana` states
  in full. ⚠ **Retired, pending re-measurement against `nfkc`; the new figure
  is a new comparison, not a continuation of the old one**
  (`docs/PERFORMANCE_GAPS.md` entry 31).
- **`remove_diacritics` — same name, opposite mechanism, and two verdicts in
  the table above invert with it.** The function is now `s` under NFD with
  every scalar whose `Canonical_Combining_Class` is non-zero removed, under
  NFC (contract §3.2), replacing an 820-entry precomposed-scalar table. The
  `remove_diacritics_ascii`/`remove_diacritics_accented` groups and the pinned
  `diacritics` 0.2.2 competitor both survive, and
  `tests/normalizers_correctness.rs` proves the two still agree byte-for-byte
  on everything those groups feed them — but the divergence *outside* that
  domain grew from 7 codepoints in `U+00C0..=U+024F` to 105, because Verbora
  now leaves every letter whose accent is part of its identity (`ø`, `æ`, `ß`,
  `đ`, `ł`, `ſ`, `ı` — no canonical decomposition, so nothing to remove)
  untouched where `diacritics`' table folds each to a bare ASCII letter. Two
  rows above are therefore stale as *selection reasoning*, not just as
  numbers, and are recorded rather than silently rewritten: **`unaccent`
  0.1.1** was rejected partly *because* it decomposes and Verbora did not —
  that objection is void and it is now the mechanism-matched competitor,
  still unpinned only because the "non-standard" crates.io licence field needs
  the review `AGENTS.md` § Licensing requires; and **`unicode-normalization`**
  is no longer classifiable as a competitor at all, being the crate
  `verbora-normalizers` is now implemented on — benchmarking against it would
  be a wrapper-overhead measurement, and `benches/normalizers.rs` does not
  pretend otherwise by including it. `secular` and
  `deunicode`/`any_ascii`/`unidecode` keep their `No` verdicts unchanged.

⚠ **No Verbora normalizer figure in this section is currently backed.**
Competitor figures are unaffected; no competitor version moved.
`results/raw/normalizers-ja_*` and the `normalizers` rows of
`results/results.json` are stale rather than approximately right.
`docs/design/text-shaping-contract.md` §7 items 2 and 3 name what a
re-measurement must answer: `remove_diacritics`' three passes (NFD, filter,
NFC) against the deleted one-lookup table — direction near-certain, magnitude
not — and whether the four normalization forms' quick-check path makes the
`Cow::Borrowed` guarantee cheap enough to justify the wrapper at all.

## 1.5 Inflectors

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `NounInflector` (English pluralize/singularize) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `NounInflector` (English pluralize/singularize) | `pluralizer` (KennethGomez) | Rust | 0.5.0 | Partial | Yes | **Benchmarked** (`benches/inflectors.rs`'s `noun_inflector_pluralize`/`noun_inflector_singularize` groups). Same algorithmic strategy (regex rule chain + irregulars + count-aware call), but an independently-maintained rule table (`pluralize`-inspired, not the reference's own table) — outputs diverge on contested irregulars (e.g. `pluralize("octopus", 2, false)` is `"octopuses"` here vs. Verbora's `"octopi"`). Narrowed to a 73-word (singular, plural) domain, probed from ~120 candidates spanning every rule class in `crates/verbora-inflectors/src/data.rs` and kept only where `pluralize` and `singularize` both agree with Verbora — verified in `tests/inflectors_correctness.rs`'s `benchmarked_pairs_agree_across_all_three_implementations`. Real result: **Verbora wins**, 2.6×–3.3× faster pluralizing, 2.5×–2.9× faster singularizing at every batch size, and two to three orders of magnitude fewer allocations. |
| `NounInflector` (English pluralize/singularize) | `Inflector` (whatisinternet) | Rust | 0.11.4 | Partial | Yes | **Benchmarked**, same groups and 73-word verified-agreeing domain as `pluralizer` above (`Inflector`'s own published `[lib] name` is `inflector`, lowercase). Massive adoption (106.7M downloads) but stale since Jan 2019. On the headline "octopus" divergence specifically, `Inflector`'s own forward rule table (unlike `pluralizer`'s) happens to list the `octop` prefix, so it agrees with Verbora there (`"octopi"`) — a genuine three-way split, confirmed with assertions in `tests/inflectors_correctness.rs`'s `octopus_is_a_documented_three_way_divergence` rather than only described in prose. Real result: **Verbora wins**, 2.0×–2.2× faster pluralizing, 2.4×–5.6× faster singularizing (widest at n=256, a batch-to-batch variance rather than a trend). |
| `NounInflector` (English pluralize/singularize) | `inflector-plus` (victorteo) | Rust | 0.11.7 | Partial | Selected cases | Fork/continuation of `Inflector`; thin adoption (14.9K downloads), stale since Sept 2022. Deliberately left unpinned: once `Inflector` itself is benchmarked (this pass), `inflector-plus` would add a second, strictly weaker implementation of the identical comparison for no additional evidence — see `benches/inflectors.rs`'s own doc comment. |
| `PresentVerbInflector` (English verb number agreement: fly↔flies) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `PresentVerbInflector` (English verb number agreement: fly↔flies) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** at matched scope — the one candidate found (`english` crate) does full multi-tense/person/form conjugation, a much bigger job. |
| `NounInflectorFr` (French nouns) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `NounInflectorFr` (French nouns) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — no dedicated Rust French-noun-pluralization crate located. |
| `NounInflectorJa` (Japanese nouns) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `NounInflectorJa` (Japanese nouns) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — nothing targets Japanese noun "pluralization," a marginal grammatical category to begin with. |
| `CountInflector` (English ordinal suffix: 1st/2nd/3rd/…) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `CountInflector` (English ordinal suffix: 1st/2nd/3rd/…) | `ordinal` (heaths/ordinal-rs) | Rust | 0.4.0 | Yes | Yes | Same narrow job, actively maintained (April 2025) — best single match found in the whole Inflectors group. |
| `CountInflector` (English ordinal suffix: 1st/2nd/3rd/…) | `Inflector::ordinalize` (whatisinternet) | Rust | 0.11.4 | Partial | Yes | **Benchmarked** (`benches/inflectors.rs`'s `count_inflector_nth_str` group, paired against `CountInflector::nth_str` — the fair match for `ordinalize`'s own `&str -> String` shape, not `nth`'s `i64 -> String`). Found while implementing this pass, not in the original dossier: `ordinalize` has none of `ordinal`'s `% 20`/`% 100` teens-exception bug (it never computes a remainder at all) and fully agrees with `CountInflector::nth_str` over every non-negative `i64` in `0..2_000_000`, exhaustively checked in `tests/inflectors_correctness.rs` — a cleaner result than the `ordinal` crate's own row above. Shares `ordinal`'s negative-integer divergence (the reference's signed `%` vs. taking the absolute value first). Real result: **Verbora wins**, a flat ~2.1×–2.2× at every batch size. |
| `CountInflectorFr` (French ordinal suffix: 1er/2e) | the reference | reference | 8.1.1 | Yes | Yes | Required baseline. |
| `CountInflectorFr` (French ordinal suffix: 1er/2e) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND** — the one French-numerals crate found (`french-numbers`) spells out full cardinal words, a fundamentally larger job than appending "er"/"e". |

## 1.6 Phonetics

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| SoundEx | the reference | reference | 8.1.1 | Yes | Yes | Byte-exact port; mandatory baseline. |
| SoundEx | rphonetic | Rust | 3.0.6 | Partial | Yes | Textbook Russell/NARA Soundex vs. The reference's digit-passthrough/condense-before-drop variant — same shape of work, diverges on digit/H/W-placement inputs. |
| Metaphone | the reference | reference | 8.1.1 | Yes | Yes | Byte-exact port; mandatory baseline. |
| Metaphone | rphonetic | Rust | 3.0.6 | Partial | Yes | Apache commons-codec Metaphone; default max code length is 4 vs. Verbora's default 32 — must reconfigure `Metaphone::new(None)`/`Some(32)` or it does strictly less work. **Benchmarked** — originally a consistent ~2.2×–2.6× loss at all three scales, filed as `docs/PERFORMANCE_GAPS.md` entry 6 (21 whole-string rewrite stages vs. rphonetic's single indexed scan). After entry 6's own later update landed — the 21-stage pipeline fused into a single skip-gated driver (letter-mask gates, window edits, fused rules), verified byte-identical against the retained 21-stage original over a ~900K-comparison differential corpus, plus one entry allocation removed (the owned lowercase buffer becomes the first scratch) — the result is honestly mixed, not a clean reversal: Verbora now **wins the single-name case** (61.4 ns vs. 75.9 ns, 1.24× faster) but still **narrowly loses the batches**, 1.11× at 10,000 names and 1.09× at 100,000. A subsequent scratch-pooling pass (entry 6's second update: the pipeline's two scratch buffers moved to a per-thread pool, ASCII tokens fold lowercase directly into pooled scratch, per-call allocator traffic cut to the one returned `String`; byte-identical, re-verified by the same differential suite — 151 tests, ~900K comparisons) flipped the batches too — **Verbora now wins all three sizes**: 51.5 vs. 73.5 ns single (1.43×), 711.91 vs. 735.77 µs at 10,000 (1.03×), 6.893 vs. 7.565 ms at 100,000 (1.10×). |
| Double Metaphone | the reference | reference | 8.1.1 | Yes | Yes | Byte-exact port; mandatory baseline. |
| Double Metaphone | rphonetic | Rust | 3.0.6 | Partial | Yes | Same max-code-length reconfiguration requirement; does not reproduce the reference's ~9 truthiness bugs. |
| Double Metaphone | pixelglow/double_metaphone | C++11 | 79dd226 (2014-08-26, the repository's only commit) | Partial | Yes | Vendored header-only C++11 transcription of Lawrence Philips' algorithm (BSD-2-Clause, stated in the header/README; no separate `LICENSE` file — same disclosed license-metadata gap as `segtok`), compiled by `build.rs` and called through FFI — the only non-Cargo, non-Rust competitor in this workspace (`vendor/pixelglow-double_metaphone/`). 584/653 (89.4%) of real English surnames agree exactly with Verbora's `DoubleMetaphone::process` (`benches/data/names.json`). Confirmed root cause of the majority of mismatches: Verbora's `handle_s` silences any trailing `S` preceded by `A`/`I` unconditionally, while the vendored library only silences it in the narrower `ISL`/`YSL` pattern (island, isle, carlisle) — two independently-arrived-at readings of a rule Double Metaphone's own published write-up never fully disambiguates, not a bug on either side (see `tests/double_metaphone_cpp_correctness.rs`). **Benchmarked (3 runs, full Criterion defaults, medians from `estimates.json`): Verbora wins, ~1.8× faster** — ~47 µs vs. ~85 µs on the 653-name throughput benchmark. |
| SoundExDM (Daitch-Mokotoff) | the reference | reference | 8.1.1 | Yes | Yes | Byte-exact port; mandatory baseline. |
| SoundExDM (Daitch-Mokotoff) | rphonetic `DaitchMokotoffSoundex` | Rust | 3.0.6 | Partial | Selected cases | rphonetic implements the genuine multi-code D-M algorithm; only its single-branch `encode()` (not `soundex()`, up to 8 codes) matches Verbora's single-`String` output shape. |
| `Cologne` (Verbora-native extension — Kölner Phonetik, Postel 1969) | rphonetic `Cologne` | Rust | 3.0.6 | **Yes** | Yes | The strongest equivalence class in this module — stronger than the Partial rows above: byte-identical output on rphonetic's full accepted input domain, no divergence found or introduced anywhere (rphonetic's Cologne never panics). Pinned by `tests/phonetics_correctness.rs` regime 2 (653-name corpus + umlaut/`ß`/lookahead extras) and an independent adversarial audit (104,114 differential-fuzz inputs, zero mismatches, suites mutation-tested). **Benchmarked: Verbora wins all three sizes** — 4.16× (1 name), 2.35× (10K), 2.25× (100K). |
| `Nysiis` (Verbora-native extension — NYSIIS, Taft 1970; strict flag, commons-codec-default strict) | rphonetic `Nysiis` | Rust | 3.0.6 | **Yes** | Yes | Byte-identical on rphonetic's non-panicking domain, in both strict and non-strict modes (both checked, so agreement is not a truncation coincidence). One documented divergence: rphonetic's strict `result[..min(len, 6)]` byte-slice **panics** when byte 6 splits a multi-byte char (4,233 of 104,114 fuzz inputs) — Verbora backs the cut off to the char boundary instead; see `docs/PERFORMANCE_GAPS.md` entry 36. **Benchmarked: Verbora wins all three sizes** — 7.56× / 7.09× / 7.63×. |
| `Caverphone1` (Verbora-native extension — Caverphone 1.0, Hood 2002) | rphonetic `Caverphone1` | Rust | 3.0.6 | **Yes** | Yes | Byte-identical on rphonetic's non-panicking domain; the one divergence is the non-ASCII inputs whose 6-byte code cut splits a multi-byte char, where rphonetic panics and Verbora pads to length instead (entry 36). **Benchmarked: Verbora wins all three sizes** — 5.18× / 5.41× / 5.24×. |
| `Caverphone2` (Verbora-native extension — Caverphone 2.0, Hood 2004) | rphonetic `Caverphone2` | Rust | 3.0.6 | **Yes** | Yes | Same regime as Caverphone1, 10-byte codes, same panic-domain caveat (entry 36). **Benchmarked: Verbora wins all three sizes** — 5.27× / 5.23× / 4.96×. |
| `Phonex` (Verbora-native extension — Lait & Randell 1996; configurable max length, default 4) | rphonetic `Phonex` | Rust | 3.0.6 | **Yes** | Yes | Byte-identical over rphonetic's entire accepted domain (any `&str` — rphonetic's Phonex never panics), quirks included (byte-length padding, `==` early exit, duplicate-suppression reset, `ß`-vanishing); where rphonetic deviates from the 1996 paper, Verbora sides with rphonetic. Default max code length 4 on both sides. **Benchmarked: Verbora wins all three sizes** — 3.53× / 2.81× / 2.81×. |
| `RefinedSoundex` (Verbora-native extension, plus commons-codec's `difference()`) | rphonetic `RefinedSoundex` | Rust | 3.0.6 | **Yes** | Yes | Byte-identical on rphonetic's non-panicking domain (US-English mapping, the shipped commons-codec instance). Documented divergence: rphonetic's `mapping[ch as usize - 65]` panics on any alphabetic char uppercasing outside A–Z — Verbora drops such chars as if absent (entry 36). **Benchmarked: Verbora wins all three sizes** — 7.73× / 7.51× / 7.42×. |
| `MatchRatingApproach` (Verbora-native extension — MRA 1977; `process` = encoding, `compare` = the real MRA match decision) | rphonetic `MatchRatingApproach` | Rust | 3.0.6 | **Yes** | Yes | Encoding byte-identical *and* the match decision (`compare` ↔ `is_encoded_equals`) decision-identical over every ordered corpus pair (~426K pairs) on rphonetic's non-panicking domain. Two documented panic-domain divergences (mid-char truncation in `encode`; empty-encoding underflow in `is_encoded_equals` — release rphonetic returns `false` for `("..", "ab")` but panics on `("ab", "..")`; Verbora is defined and symmetric) — entry 36. **Benchmarked: Verbora wins all three sizes** — 15.32× / 16.54× / 17.41×, the widest margins in the module. |
| `DaitchMokotoff` (Verbora-native extension — the genuine branching D-M; distinct from `SoundExDM` above, which stays the single-branch reference variant — the two coexist deliberately, both module docs explain when to use which) | rphonetic `DaitchMokotoffSoundex` | Rust | 3.0.6 | **Yes** | Yes | Unlike the SoundExDM row, this pairing is output-format identical: `process` ↔ `soundex()` (pipe-joined branch codes), `codes()` ↔ `inner_soundex(_, true)`, `codes()[0]` ↔ non-branching `encode()` — all three checked at once in `tests/phonetics_correctness.rs`. Four rphonetic behavioral quirks reproduced deliberately and documented in the module doc (non-ASCII rule keys `ą`/`ę`/`ţ`/`ț` consuming a following char; before-vowel probe off-by-one; duplicate final codes; `ü`/`œ` missing from the ASCII-folding list). Rules embedded as static tables vs. rphonetic's `nom`-parsed-at-builder-time rules. **Benchmarked: Verbora wins all three sizes** — 2.35× / 2.08× / 2.21×. |
| `BeiderMorse` (Verbora-native extension, no reference counterpart — 19-language auto-detecting Beider-Morse phonetic matching) | rphonetic `BeiderMorseBuilder` | Rust | 3.0.6 | Partial | Yes | `BeiderMorse`'s own doc comment names rphonetic's implementation as its design-time cross-checking oracle, but nothing benchmarked it until this round (`rphonetic = { ..., features = ["embedded_bm"] }`, `benches/phonetics.rs`'s `bench_beider_morse`). **Real, disclosed coverage asymmetry, not a fully equivalent comparison:** rphonetic's `embedded_bm` feature ships only the `"any"`/`"common"` rule files per `NameType`, not the full per-language corpus, so `ConfigFiles::default()` can never resolve a specific guessed language and always falls back to `"any"` — `verbora_phonetics::BeiderMorse::encode()`'s full 18/10/5-language (Generic/Ashkenazi/Sephardic) `LangGuesser` auto-detection has no equivalent on the rphonetic side to compare against. Benchmarked via each side's own default `.encode()` call at batch sizes 1/100/1,000; throughput only, output equivalence never asserted (both sides are textbook-derived but independently implemented). |

*Confirmed: the reference exports exactly these four phonetic classes — so every other row above is a Verbora-native extension with no reference counterpart (see `AGENTS.md`'s "Verbora-Native Extensions" policy). The seven spec-pinned encoder extensions (Cologne through the branching `DaitchMokotoff`, added 2026-08) invert this module's usual equivalence direction: because the JS reference has none of them, each one's behavior is pinned to its own published algorithm instead — Postel (1969) for Cologne, Taft (1970) for Nysiis, Hood (2002/2004) for Caverphone1/2, Moore et al. (1977) for MatchRatingApproach, and Mokotoff & Daitch (1985)'s coding chart for the branching `DaitchMokotoff` — each verified by that module's own transcription-equivalence tests (an independently re-transcribed table checked against the one the encoder consults) and adversarial fuzz audits, not by rphonetic's output. `RefinedSoundex` and MatchRatingApproach's accent-fold table are the two cases with no independent paper: each states its own table in its own doc comment and treats *that statement* as the specification, citing Apache Commons Codec only as the table's provenance — never consulted as a behavioral oracle (see `refined_soundex.rs`'s module documentation). Their **Yes** classification against rphonetic (byte-exact, full output equivalence on rphonetic's accepted domain) is therefore a separately-verified competitive-benchmark result, not a definitional one — unlike the four reference-ported rows, which can never be more than Partial. Their only divergences from rphonetic are the panic-domain substitutions and the deliberately-reproduced quirks itemized per row above, each documented in the owning module's own doc comment and re-verified by the adversarial fuzz audit. `BeiderMorse` remains Partial: it is benchmarked against rphonetic but not pinned to it.*

*One further exception to the "every other row is a Verbora-native extension" generalization above: `pixelglow/double_metaphone` is neither a reference-ported baseline (the reference has no C++ side to port) nor a Verbora-native extension (Double Metaphone already has a reference counterpart, benchmarked against rphonetic in the row above it). It is this workspace's first and only vendored, FFI-called, non-Cargo competitor — a second, independent Double Metaphone implementation benchmarked against the same `DoubleMetaphone::process` capability. See `vendor/pixelglow-double_metaphone/README.md` for provenance and `tests/double_metaphone_cpp_correctness.rs`/`benches/double_metaphone_cpp.rs` for the full correctness and fairness writeup (both under `benchmarks/competitive/rust-competitors/`).*

**Update, five encoder groups unmeasured in the 2026-08-22 campaign.** The
benchmark process for this module aborted before reaching `Phonex`,
`RefinedSoundex`, `MatchRatingApproach`, the branching `DaitchMokotoff` and
`BeiderMorse`: `rphonetic`'s own `Phonex` implementation panicked with a raw
allocation failure (`memory allocation of 1 bytes failed`) while timing
input of length 10, killing the rest of the `cargo bench` process for this
target. The partial rows that completed before the crash were removed
rather than published as a one-point sweep. The specific per-size figures
recorded in the rows above for those five encoders (7.73×/7.51×/7.42× for
RefinedSoundex, 15.32×/16.54×/17.41× for MatchRatingApproach, and so on)
predate this campaign and were not re-verified against the current commit;
treat them as historical rather than current. The seven groups that did
complete — SoundEx, Metaphone, DoubleMetaphone, Cologne, Nysiis, Caverphone1
and Caverphone2 — have current 2026-08-22 figures in `results/results.json`.
The correctness record for all twelve encoders (byte-exact agreement against
`rphonetic`, differential fuzzing) is unaffected by this gap, since it comes
from `tests/phonetics_correctness.rs` independently of any bench run.

**Update, gap closed (2026-08-22, same day).** The abort above was
environmental, not a defect in `rphonetic`: `phonex` re-run alone on a quiet
machine completed all ten scales with no crash, and the full `--bench
phonetics` target then finished clean. `results.json` now carries 119
phonetics rows across all twelve groups (381 measurements taken; every
non-`beider_morse` group sweeps ten scales, `beider_morse` nine), at commit
`0313eae` — one commit after the `80c302b` this campaign's other modules are
stamped at, and a commit that touches `scripts/` and `site/`, not
`crates/phonetics` source, so this is the same code re-measured rather than
a different implementation. `Phonex`, `RefinedSoundex`, `MatchRatingApproach`,
the branching `DaitchMokotoff` and `BeiderMorse` all have current figures now;
the historical per-size numbers quoted in the rows above (7.73×/7.51×/7.42×
for RefinedSoundex, 15.32×/16.54×/17.41× for MatchRatingApproach, and so on)
predate both this gap and its closure and should not be read as current —
`site/benchmarks/competitive.md`'s Phonetics section carries the present
figures for all twelve groups.

## 1.7 Phonetic Index / Phonetic Neighbors

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `PhoneticIndex` / phonetic-neighbors (build/freeze/query candidate generation over a dictionary) | — | — | — | — | No | **NO FAIR COMPETITOR FOUND.** Verbora-native extension with no the reference equivalent (`lib/natural/phonetics/phonetic` offers only a per-token encode-and-filter helper, no index type). Every Rust candidate found (rphonetic, soundex-rs) is a single-word encoder, not a build-once/query-many index. Tantivy/Lucene/generic search explicitly excluded per the spec's own PHONETIC INDEX / NEIGHBORS section — categorically larger scope (ranking, query language, full-text indexing). Recommended path: internal-only benchmark (build/lookup/memory/sequential/parallel), which `benches/phonetic_index.rs` already implements. **MEMORY, actually benchmarked (real numbers, not estimated):** `benches/phonetic_index.rs`'s own `bench_alt_designs_query` group already printed an *analytical* (`size_of`-based) memory estimate for the shipped `InlineCode`+CSR design — **2,899,788 B (29.00 B/entry)** at 100K SoundEx entries. `rust-competitors/examples/memory_report.rs` independently re-measured the same design with the new real allocator-counting infra (`memory::measure`, a live global-allocator trace, not `size_of` arithmetic) over its own cycled-corpus 100K-entry dictionary: **2,937,590 B net retained (29.38 B/entry)** for SoundEx, **5,925,049 B (59.25 B/entry)** for DoubleMetaphone (128-byte `MetaphoneCode` vs. 17-byte `SoundexCode` — see `src/index.rs`'s own doc comment on why the margin is that generous). The two independent methods agree to within 1.3% for SoundEx — real confirmation, not a repeated assumption. See `benchmarks/competitive/results/memory-report.json`'s `phonetic_index` section for the full 10K/100K, both-encoder breakdown. |

## 1.8 Distances

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Levenshtein | the reference | reference | 8.1.1 | Yes | Yes | Direct port source; required baseline. |
| Levenshtein | strsim | Rust | 0.11.1 | Yes | Yes | Char-indexed, and so is Verbora — one Unicode scalar is one unit on both sides, so the unit agreement is now exact rather than confined to the Basic Multilingual Plane. Canonical Rust crate, ~990M downloads. |
| Levenshtein | rapidfuzz | Rust | 0.5.0 | Yes | Yes | Char-indexed; most complete single-crate algorithm parity found. |
| Levenshtein | stringmetrics | Rust | 2.2.2 | Yes | Yes | Char-indexed; also has weighted costs; high recent adoption. |
| Levenshtein | triple_accel | Rust | 0.4.0 | Partial | Selected cases | Byte-level (UTF-8 bytes), SIMD; fair only on ASCII corpora. |
| Levenshtein | editdistancek | Rust | 1.0.2 | Partial | Selected cases | Byte-level, fixed unit costs only, no Damerau; ASCII-only fair. |

**Competitive shape coverage.** In addition to the shared random and
near-identical size sweep, `rust-competitors/benches/distance.rs` exposes
`levenshtein_edge_shapes`: `near/1024` (one central substitution),
`disjoint/1024` (no shared symbols), and `late-overlap/65x10000` (a single
overlap at the end of the long target). They are ASCII/unit-cost cases, so all
six timed implementations have identical semantics. The group is deliberately
reported separately from random-input medians; it measures shortcut behavior,
not a representative mixed workload. Its exact equivalence is pinned by
`levenshtein_competitors_agree_on_the_timed_edge_shapes` before timings are
accepted.
| Damerau-Levenshtein (unrestricted/true) | the reference | reference | 8.1.1 | Yes | Yes | The reference's `restricted:false` default; Verbora's `damerau_levenshtein()`, a separate named fn since the `Options.restricted` flag was retired. |
| Damerau-Levenshtein (unrestricted/true) | strsim | Rust | 0.11.1 | ~~Yes~~ ~~**Partial**~~ **Yes** | Yes | `damerau_levenshtein()` is explicitly the unrestricted variant, and so is Verbora's. **Verdict restored this round** (it had been narrowed to `Partial` while Verbora shipped a non-canonical, asymmetric recurrence inherited from the reference; that recurrence is gone — see `docs/PERFORMANCE_GAPS.md` entry 29's final update). Verbora's `damerau_levenshtein` is now canonical unrestricted Damerau-Levenshtein — the same Lowrance-Wagner function strsim computes, over the same Zhao–Sahni linear-space formulation — so this is full algorithm equivalence, not corpus equivalence. Byte-exact agreement verified over **202,000 randomized pairs** (four alphabet widths 2/3/4/26, length classes 1..=25 with unequal lengths and empty operands, plus 2,000 binary-alphabet mutation chains of up to eight edits — the shape that actually separates unrestricted Damerau from OSA), zero divergences, reproducible via `unrestricted_damerau_agrees_with_both_competitors_over_a_wide_randomized_sweep` in `rust-competitors/tests/distance_correctness.rs`. The former counterexample `"bb"`→`"abbb"` now answers 2 on both sides. |
| Damerau-Levenshtein (unrestricted/true) | rapidfuzz | Rust | 0.5.0 | ~~Yes~~ ~~**Partial**~~ **Yes** | Yes | `distance::damerau_levenshtein` module, explicitly unrestricted — same **verdict restored this round** as the strsim row above, on the same 202,000-pair evidence (both crates are asserted against Verbora in the same sweep, so the agreement is three-way). Entry 29's earlier finding that strsim and rapidfuzz share the Zhao–Sahni algorithm line-for-line now cuts the other way: Verbora adopted that algorithm rather than being structurally forbidden from it. |
| Damerau-Levenshtein (restricted/OSA) | the reference | reference | 8.1.1 | Yes | Yes | The reference's `restricted:true`; Verbora's `osa()`, likewise now a first-class named fn rather than a flag on `Options`. |
| Damerau-Levenshtein (restricted/OSA) | strsim | Rust | 0.11.1 | Yes | Yes | `osa_distance()` — separate named fn. |
| Damerau-Levenshtein (restricted/OSA) | rapidfuzz | Rust | 0.5.0 | Yes | Yes | `distance::osa` module. |
| Damerau-Levenshtein (restricted/OSA) | triple_accel | Rust | 0.4.0 | Partial | Selected cases | `rdamerau()` restricted-only; byte-level, ASCII-only fair. |
| Jaro | strsim | Rust | 0.11.1 | ~~Yes~~ **Partial** | Yes | `jaro()` public fn — the same general task, computing a **different function**. **The former `Yes` is disproved by a passing test, not by a re-reading.** `docs/design/distance-contract.md` §3.4 defines the transposition term `t` as `raw as f64 / 2.0` — exact, so an odd raw count contributes `x.5`. `strsim` 0.11.1's `generic_jaro` accumulates the same raw count into a `usize` and then does `transpositions /= 2`, an **integer** division, dropping the half whenever the raw count is odd (exactly when the matched characters form an odd-length permutation cycle) and scoring higher than the contract. Minimal fixture: `jaro("abccba", "abbaca")` is `0.788…` for Verbora and `eddie`, `0.822…` here. Pinned by `strsim_and_rapidfuzz_jaro_diverge_from_the_contract_by_truncating_transpositions` in `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`, which sweeps 8,200 deterministic pairs and fails if the divergence stops appearing on more than one pair in five; `benchmarks/competitive/README.md`'s wider measurement of the same convention puts it at **23,428 of 82,000 random pairs (28.6%)**, smallest diverging operand length 6 — inside the benchmarked corpus, and reproducible on the timed `"<n>-near"` shape at n=64. Every swept pair additionally asserts the *direction* truncation forces (`strsim >= verbora`), so a divergence of any other kind fails loudly. `Benchmarkable?` stays `Yes` because the work is comparable, but **the `jaro` timing rows are not like-for-like and must not be published as an equivalence**. `eddie` is the only implementation in this harness that computes §3.4's function — see its row below, and note it carries no timing row. |
| Jaro | rapidfuzz | Rust | 0.5.0 | ~~Yes~~ **Partial** | Yes | `distance::jaro` module — **same correction, same evidence, same fixture** as the `strsim` row above: 0.5.0 truncates the half-transposition count with the identical integer division, answers `0.822…` on `jaro("abccba", "abbaca")`, and is asserted in the same test (both crates are checked against Verbora on every pair of every sweep, so the finding is three-way, not a single-crate quirk). `Benchmarkable?` stays `Yes` on comparable work; the `jaro` timing row is **not** a like-for-like comparison. |
| Jaro | eddie | Rust | 0.4.2 | Yes | ~~Yes~~ **No** | Correct algorithm — and, since the two rows above dropped to `Partial`, the only one here that is §3.4's function — ~~but crate abandoned since 2020 — verify against fresh vectors before trusting~~ **but the implementation is unsound, and that flag has now been discharged against it rather than left open.** `eddie-0.4.2`'s `utils/buffer.rs` `Buffer::store` calls `buf.clear()`, then writes through `buf.get_unchecked_mut(i)` for `i` beyond `buf.len()`, and only calls `set_len(i)` afterwards — a write into reserved-but-uninitialised capacity through a slice whose length is still `0`. Read directly from the crate's own published source, not reported from elsewhere. Every `eddie::Jaro::similarity` and `eddie::JaroWinkler::similarity` call routes through it, so `eddie::Jaro::new().similarity("a", "a")` alone is undefined behaviour: `tests/distance_correctness.rs` aborts with SIGABRT under a debug build (Rust's `unsafe` precondition checks catch it) and passes 13/13 under `--release` only because those checks are compiled out, not because the UB is absent. It was invisible until the `verbora-distance` migration's import repair made that target compile again. **Every `eddie` comparison in this section is affected, including the "no-loss result on Jaro/Jaro-Winkler" recorded below, which is retired.** **Disposition — decided, not open.** `eddie` is retained as a **correctness oracle only, reached exclusively through its sound slice API, and carries no timing row; none may be added.** `eddie::slice::Jaro`/`JaroWinkler::similarity` never touch `Buffer` and reach zero `unsafe` on their whole call graph, and `eddie`'s `str` Jaro is literally `buffer.store(s.chars())` followed by the slice call, so wrapping the slice API computes the same function soundly. It is kept rather than dropped because it is the **only** implementation in this harness that computes the function `docs/design/distance-contract.md` §3.4 specifies — the `strsim` and `rapidfuzz` rows above are now `Partial`, not `Yes`, so dropping `eddie` would leave the Jaro rows with no same-function cross-implementation check at all. A timing row is refused on two independent grounds: a timing row must call the competitor's published API as published, and `eddie`'s published `str` API is undefined behaviour on every call; and timing the slice wrapper instead would hand `eddie` pre-decoded `Vec<char>` operands while Verbora's `jaro(&str, &str)` decodes scalars inside the timed region — the "excluding real costs from only one implementation" that `AGENTS.md` § *Cross-Implementation Benchmark Fairness* forbids. Access is confined to `tests/distance_correctness.rs`'s `eddie_slice` module, `eddie` is a **dev**-dependency so the crate's own `src/` cannot reach it, and `every_reference_to_eddie_goes_through_the_sound_slice_wrapper` walks every `.rs` file in the crate and fails the suite if any other `eddie` path appears in code. See `benchmarks/competitive/README.md` § "Resolved: `eddie` 0.4.2 is unsound, and is now contained" → "The decision: isolate for correctness, drop from timing", and `manifests/competitors.json`'s own `eddie` entry. |
| Jaro | the reference | reference | 8.1.1 | — | No | **NO FAIR COMPETITOR** — plain Jaro is an unexported private helper in the reference; nothing to call from outside. |
| Jaro-Winkler | the reference | reference | 8.1.1 | Yes | Yes | Direct port source; required baseline. |
| Jaro-Winkler | strsim | Rust | 0.11.1 | ~~Yes~~ **Partial** | Yes | `jaro_winkler()` — **two** independent divergences from `docs/design/distance-contract.md` §3.4, not one. (a) It inherits the plain-Jaro half-transposition truncation from the row above, since Winkler is an affine function of the Jaro score. (b) It gates the Winkler boost behind `sim > 0.7`, where §3.4 fixes `jaro_winkler` at `sim_j + l * p * (1 - sim_j)`, `l = min(4, common_prefix_len)` in scalars and `p = 0.1`, applied **unconditionally** — so on any pair scoring at or below 0.7 with a shared prefix, `strsim` returns the unboosted Jaro value and Verbora does not. Verbora's side of (b) is pinned bit-exactly (not within a tolerance) by `assert_jaro_family_agrees` in `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`, which recomputes the affine relation from Verbora's own `jaro` on every swept pair and therefore pins both the fixed `p` and the absence of the `0.7` threshold. `Benchmarkable?` stays `Yes` on comparable work; the `jaro_winkler` timing rows are **not** a like-for-like comparison. |
| Jaro-Winkler | rapidfuzz | Rust | 0.5.0 | ~~Yes~~ **Partial** | Yes | `distance::jaro_winkler` — **both** divergences of the `strsim` row above, for the same two reasons, asserted over the same sweeps. `Benchmarkable?` stays `Yes` on comparable work; the `jaro_winkler` timing row is **not** a like-for-like comparison. |
| Jaro-Winkler | eddie | Rust | 0.4.2 | Yes | ~~Yes~~ **No** | ~~Same maintenance caveat as plain Jaro.~~ **Same unsoundness as plain Jaro, for the same reason**: `JaroWinkler::similarity` routes through the identical `Buffer::store`. `Equivalent?` stays `Yes` — the slice-level function is the one §3.4 specifies, and it agrees with Verbora on every swept pair — but `Benchmarkable?` is now **`No`**: correctness oracle only, no timing row exists and none may be added. See the Jaro row above for the defect and the decision. |
| Sørensen-Dice coefficient | the reference | reference | 8.1.1 | Yes | Yes | Direct port source; required baseline. |
| Sørensen-Dice coefficient | strsim | Rust | 0.11.1 | Partial | Selected cases | Still a different Dice variant, but the divergence narrowed when Verbora stopped preprocessing its operands: **case sensitivity now agrees** (both treat `"ABC"`/`"abc"` as disjoint), and so do the degenerate pairs (both give `1.0` for two identical operands, including two empty ones, and `0.0` otherwise). Three real divergences remain, all confirmed by reading `strsim-0.11.1/src/lib.rs`'s `sorensen_dice`: it counts bigram **multiplicity** (a `HashMap<(char,char),usize>` multiset intersection) where Verbora uses the bigram **set**; it **strips every whitespace character** from both operands up front where Verbora treats `' '` as an ordinary unit forming ordinary bigrams; and its `< 2` short-circuit and its `a.len() + b.len() - 2` denominator are **byte** lengths, not character counts, so any non-ASCII operand gives it a denominator that does not match its own bigram count. Fair only for ASCII pairs of ≥2 characters with no whitespace and no repeated bigrams. |
| Sørensen-Dice coefficient | fuzzt | Rust | 0.3.1 | Partial | Selected cases | Forked from strsim, inherits the same three divergences — functionally redundant with strsim for every other metric. |
| Hamming | the reference | reference | 8.1.1 | Yes | Yes | Direct port source; required baseline. |
| Hamming | strsim | Rust | 0.11.1 | Yes | Yes | `hamming()` — a lockstep `chars()` walk returning `Result<usize, StrSimError>`, `Err(DifferentLengthArgs)` when the operands run out at different points. Verbora's `hamming` walks the same scalars and reports the same counts; the only divergence left is how each spells "no answer" — `Option::None` against an error value that carries nothing the caller did not already know. Both are total and neither panics. |
| Hamming | rapidfuzz | Rust | 0.5.0 | Yes | Yes | `distance::hamming`, `Result` + optional padding (`Args::pad`, off by default; padded mode charges the surplus positions as differences, which Verbora has no equivalent for and the benchmark does not enable). |
| Hamming | stringmetrics | Rust | 2.2.2 | Yes | Yes | `hamming()`, `Result<u32, LengthMismatchError>`. |
| Hamming | triple_accel | Rust | 0.4.0 | Partial | Selected cases | Byte-level; ASCII-only fair. |
| Fuzzy substring search (`levenshtein_search`/`damerau_levenshtein_search`) | the reference | reference | 8.1.1 | Yes | Yes | Direct port source; required baseline. |
| Fuzzy substring search (`levenshtein_search`/`damerau_levenshtein_search`) | triple_accel | Rust | 0.4.0 | Partial | Selected cases | Bounded-`k`, multi-match search — a different problem shape (an iterator of matches vs. a single best match, always found, reported as the borrowed matched text plus its byte range in the target), not just an implementation detail. |
| Fuzzy substring search (`levenshtein_search`/`damerau_levenshtein_search`) | (all other surveyed crates) | Rust | — | — | No | **NO FAIR COMPETITOR** — strsim/rapidfuzz/eddie/editdistancek/stringmetrics only compute a scalar two-string distance; none locate a best-matching substring and its position within a longer target. |

**Now actually benchmarked (real numbers, TIME and MEMORY) — `stringmetrics`, `eddie`, `triple_accel`, and `editdistancek` were selected by this matrix but never wired into `rust-competitors/Cargo.toml`/`manifests/competitors.json` before this round; that gap is now closed.** All four are pinned at the exact versions above, correctness-checked once against Verbora in `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs` before any timing number was trusted (two real, narrow, documented divergences found for `eddie` there — both-empty-string and equal-single-character Jaro similarity — neither reachable by the benchmarked corpus), and real numbers for every row above now exist in `site/benchmarks/competitive.md`'s Distance section: **Update, third pass — the whole distance group re-measured after this round's kernel work landed (see `docs/PERFORMANCE_GAPS.md` entries 1, 26, and 27's own later updates).** Plain Levenshtein first: the second-pass multi-block bit-vector fix had already flipped the large sizes against `stringmetrics`/`triple_accel`/`editdistancek`; this round's BitPeq rewrite (flat/packed bit-tables replacing the `HashMap`-based `Peq` inside the same Myers kernels, and the single-word gate widened from 8..=64 to 1..=64 chars) finishes the job — Verbora now **beats all five Levenshtein competitors at every size 4–1024**, including `rapidfuzz` (2.16× faster at n=4, narrowing to 1.09× at n=1024 — entry 1's original headline 90.8× loss, fully reversed) and the former small-size holdouts `stringmetrics` (1.76× at n=4) and `triple_accel` (4.48× at n=4); margins over the other four crates reach 17×–37× at n=1024. Restricted Damerau-Levenshtein (OSA) flipped the same way (entry 27's own later update): brand-new bit-parallel OSA kernels (Hyyrö's 2003 transposition extension of Myers, single-word + multi-word block, gated to unit costs) turn what was a one-sided loss into **beating every competitor at every size** — 1.90× (n=4) to 1.39× (n=1024) faster than `rapidfuzz`, 4.8×–22.7× faster than `triple_accel`'s SIMD `rdamerau`, up to ~75× faster than `strsim`. Jaro/Jaro-Winkler likewise: new bit-parallel match-flagging kernels in Verbora's own greedy orientation (fractional-transposition semantics preserved exactly; the scalar loop kept for `max_len<=16` and as the differential oracle) put Verbora **ahead of both `rapidfuzz` and `strsim` at every size** — 3.4× (n=4), ~2.1× (n=64), 1.26× (n=1024) over `rapidfuzz`, up to ~32× over `strsim`. Unrestricted Damerau-Levenshtein is the honestly-mixed one, dramatically narrowed but **not** reversed everywhere — ⚠ **every unrestricted-Damerau timing figure from here to the end of the fourth-pass update below is retired: it was measured against the pinned-recurrence kernels, which no longer exist. Pending re-measurement; do not cite.** Retained as-is for provenance, since the reasoning attached to those numbers is what the fifth-pass update overturns. — distance mode no longer builds the full `f64` cost+parent matrices (a two-rows-plus-per-symbol-snapshot-arena kernel now evaluates the same pinned recurrence exactly, `u16` cells where the combined lengths fit, `u32` beyond), and Verbora now **wins outright at n=16** against both crates (1.06×), edges `rapidfuzz` at n=64 (1.06×) and n=256 (0.2%, within noise), is level with `strsim` at n=64 (0.5%), but trails `strsim` ~1.08×–1.11× at n=256/1024, trails `rapidfuzz` ~4% at n=1024 (also within run noise), and still loses to both at n=4 (1.13× vs. `rapidfuzz`, 1.46× vs. `strsim`). That residual is structural, not unfinished tuning: Verbora's pinned unrestricted recurrence is deliberately *not* the textbook Zhao–Sahni algorithm those two crates share (see the amended `~~Yes~~ **Partial**` verdicts on their rows above — `"bb"`→`"abbb"` is 1 vs. their 2, and the recurrence is not even symmetric), which structurally forbids Verbora from adopting their linear-space formulation or affix trimming. **Update, fourth pass (2026-08) — unrestricted Damerau-Levenshtein re-tiered, and the mixed verdict becomes a near-sweep** (see `docs/PERFORMANCE_GAPS.md` entry 29's second update): the byte path now dispatches across three measured tiers — a table-free stack-matrix kernel for operands ≤ 8 bytes, a register-carried peeled kernel ≤ 128, a memory-carried variant beyond, all evaluating the pinned recurrence exactly (differentially pinned against the `full_matrix` oracle plus cross-tier agreement tests, 91 tests green; UTF-16 path unchanged) — and Verbora now **beats `rapidfuzz` at all five sizes** (2.39× at n=4, 1.34× at n=16, 1.61× at n=64, 1.15× at n=256, 1.11× at n=1024) and **beats `strsim` at four of five** (1.88× / 1.11× / 1.49× / 1.03× at n=4/16/64/256), with one remaining ~2% loss at n=1024 (1.906 vs. 1.866 ms) — a statistical tie at the measured structural floor (the bare min-chain of the pinned recurrence alone costs 1.86–1.88 ms at this size, and the recurrence's measured divergence from textbook DL, 38.6% of random small-alphabet pairs, is what forbids strsim's Zhao–Sahni candidate pruning), recorded as a loss, not rounded away. **Update, fifth pass (2026-08) — the premise under every unrestricted-Damerau sentence above is gone, so all of their numbers are retired rather than amended** (see `docs/PERFORMANCE_GAPS.md` entry 29's final update): `damerau_levenshtein`/`damerau_levenshtein_search` now compute **canonical** unrestricted Damerau-Levenshtein via the Zhao–Sahni linear-space algorithm — symmetric, with common-affix trimming applied. The three claims those figures rested on are all void: the recurrence is no longer "deliberately not the textbook algorithm", the 38.6%-divergence measurement described a function Verbora no longer computes, and nothing structurally forbids the linear-space formulation or affix trimming any more — Verbora uses both. Consequently the n=4/16/64/256/1024 ratios against `strsim` and `rapidfuzz`, the "structural floor" argument, and the ≤8/≤128/beyond tier split they describe are all **pending re-measurement against the current kernels**; no number here should be quoted until that re-benchmark lands. What did get re-verified without a benchmark is correctness, and it strengthened: the two competitor rows in the table above returned from `Partial` to `Yes` on 202,000 randomized pairs with zero divergences. Unchanged through all of the above: `stringmetrics` still wins on Levenshtein MEMORY (~4×, entry 28 — a genuinely separate axis, its single-`u32`-row design uses less memory regardless of how fast either side runs); `triple_accel` still wins Hamming (its widest margin, up to 20.6×) and fuzzy substring search (neither path routes through any of the new kernels), and by a much larger margin on MEMORY for fuzzy substring search specifically (up to 283×, entry 29 — whose companion 683× unrestricted-Damerau memory figure was measured against the old full-`f64`-matrix distance path, was never re-measured against the pinned-recurrence kernels that replaced it, and is now two kernel generations stale — ⚠ retired, pending re-measurement against the canonical Zhao–Sahni path); `eddie`'s no-loss result on Jaro/Jaro-Winkler (recorded at the time as a real win for it, not filed as a gap, despite the crate's own maintenance caveat) ⚠ **is retired outright, and is not pending re-measurement — there is nothing to re-measure.** It predates this round's bit-parallel Jaro kernels and was never re-measured against them, but that is the smaller objection: it was produced by a release build that executed undefined behaviour, so it does not "stand for the code it measured" either. No fresher `eddie` number exists, none may be produced, and the ten `eddie` timing rows it came from have been removed from `benchmarks/competitive/results/results.json` and `results/distance-memory.json` (see the `eddie` row in the matrix above). `stringmetrics`' own `damerau_levenshtein` — deliberately never called, verified by reading `stringmetrics-2.2.2`'s own source: the whole `damerau` module is commented out of both `mod` and `pub use` in `algorithms.rs`, so it is not merely an unused stub but genuinely unreachable through the published 2.2.2 API. See `manifests/competitors.json`'s four new entries for the full per-crate detail.

**Update, sixth pass — the retirement widens from unrestricted Damerau to the
whole group.** `verbora-distance` was rewritten to the Rust-native contract
specified in `docs/design/distance-contract.md`, and the rewrite reaches every
Verbora row in this section:

- **The unit changed.** One Unicode scalar value (`char`) is one unit,
  replacing the UTF-16 code unit, and the non-ASCII path materialises
  `Vec<char>` rather than `Vec<u16>`. Every timed row here uses an ASCII
  corpus, where one byte is one scalar is one code unit by definition, so no
  *result* on the benchmarked domain moves — but the dispatch that selects the
  path is not the code that was measured.
- **Unit costs became the absence of an argument.** `levenshtein`,
  `damerau_levenshtein` and `osa` take no cost set and return `usize`; the
  weighted forms are separate functions over validated `LevenshteinCosts` /
  `OsaCosts` / `DamerauCosts`. The per-call cost comparison that used to
  choose between the bit-parallel and scalar tiers is gone. The kernels on
  either side of it are the same code; the removal itself is untimed.
- **Hamming's signature and its general path both changed.** `hamming` returns
  `Option<usize>`, and the fall-through path is one fused `chars()` walk that
  decides comparability and counts differences together, allocation-free on
  every input. The ASCII tiered kernel is untouched, but the wrapper around it
  is not free — `Option<usize>` returns in two registers rather than one, which
  the contract flags as costing the SWAR tier a tail call, unmeasured — so even
  the untouched kernel's published figure is no longer backed.
- **Jaro–Winkler lost its equality short-circuit.** The `if s1 == s2 { 1.0 }`
  exit at the top of the function is gone: the identity now falls out of the
  formula itself, because Jaro's match window is clamped at zero rather than
  going negative for one-unit operands. Any equal pair the corpus contains
  therefore runs the kernels instead of returning on one comparison. The
  `ignore_case` and `dj` options are deleted with it.
- **Dice's algorithm changed.** The unconditional lowercase/whitespace-collapse/
  trim preprocessing and the one-scalar space padding are both deleted, so the
  function does strictly less work per call *and* returns a different score for
  any operand containing an upper-case letter or whitespace.
- **Search returns borrowed text.** `levenshtein_search` and its siblings report
  a `&str` borrowed from the target plus its byte range, with no lossy-decode
  allocation, and unit-cost plain Levenshtein search runs a bit-parallel
  per-column kernel rather than the full cost-plus-parent matrix.

⚠ **Every Verbora timing and memory figure in this section therefore predates
the code it describes and is retired pending re-measurement** — including the
Levenshtein, OSA, Jaro/Jaro-Winkler, Hamming and fuzzy-substring-search rows
the fifth-pass update above left standing. Competitor figures are unaffected;
no competitor version moved. The equivalence verdicts in the table above are
also unaffected on the benchmarked ASCII domain, and where the underlying
comparison genuinely shifted — Sørensen-Dice against `strsim`/`fuzzt`, Hamming
against `strsim` — the rows themselves have been re-derived rather than
reworded. `docs/PERFORMANCE_GAPS.md` entries 26–29 carry the same retirement.

**Separately — `eddie` 0.4.2 is unsound, so its rows are not merely stale.**
The two paragraphs above retire Verbora figures because Verbora's code moved
underneath them. This is a different failure, on the competitor's side, and it
is not fixed by any re-measurement: every `eddie::Jaro`/`eddie::JaroWinkler`
call is undefined behaviour (`Buffer::store` writes past a cleared `Vec`
through `get_unchecked_mut` before `set_len`; the two eddie rows in the table
above carry the full reading of the crate's source, and
`docs/PERFORMANCE_GAPS.md` entry 36 item 3 carries it as an upstream-defect
finding, with the earlier, milder framing of that item explicitly withdrawn).
Two consequences for this
section's record, stated because they contradict text that stands above:

- The sentence "correctness-checked once against Verbora in
  `…/tests/distance_correctness.rs` before any timing number was trusted (two
  real, narrow, documented divergences found for `eddie` there — both-empty-
  string and equal-single-character Jaro similarity — neither reachable by the
  benchmarked corpus)" is accurate about what that pass found, and wrong about
  what it implies. Those two divergences were **Verbora** defects, since fixed
  by `docs/design/distance-contract.md` §3.4 and now asserted as *agreements*;
  the correctness pass did not, and could not, clear `eddie` itself, because
  the target it ran in did not compile at the time. It compiles now, and it
  aborts.
- `eddie`'s "no-loss result on Jaro/Jaro-Winkler" was already recorded as
  standing only for the pre-bit-parallel code it measured. That framing is too
  generous and is withdrawn: the numbers came out of a release build with the
  UB checks compiled out, so they do not stand for any code at all. ⚠ **The
  result is retired outright, not pending re-measurement, and may not be cited
  as a competitor result on either ground.** The ten `eddie` timing rows it was
  drawn from have been removed from
  `benchmarks/competitive/results/results.json`, together with their ten
  `results/raw/distance-*-eddie-*.json` copies and the ten `eddie` rows in
  `results/distance-memory.json`.

**The decision is now recorded, in three places that agree.** `eddie` stays
pinned as a **dev**-dependency and is reached only through the sound
slice-level `Jaro`/`JaroWinkler`, as a correctness oracle; it carries **no
timing row, and none may be added**. Replacing it with the `strsim`/`rapidfuzz`
Jaro rows — the alternative this paragraph used to leave open — is not
available: those rows are `Partial`, not `Yes` (they truncate the
half-transposition count and gate the Winkler boost), so `eddie` is the only
same-function cross-implementation check the Jaro rows have. See
`benchmarks/competitive/README.md` § "Resolved: `eddie` 0.4.2 is unsound, and
is now contained", `manifests/competitors.json`'s `eddie` entry, and the
machine-enforced containment test
`every_reference_to_eddie_goes_through_the_sound_slice_wrapper`.

## 1.9 Language Detection

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Language Detection | whatlang (raw crate) | Rust | 0.18.0 | Yes | Yes | The literal engine `WhatlangDetector` already wraps — isolates wrapper overhead, not a rival algorithm; must not be reported as "Verbora beats/loses to whatlang." |
| Language Detection | lingua | Rust | 1.8.0 | Partial | Selected cases | Different algorithm (1–5-gram + alphabet pre-filter vs. trigram model); default covers 75 languages vs. Verbora's 22 — must build with `from_languages()` restricted to the shared 21-language overlap (all but Galician). |
| Language Detection | whichlang | Rust | 0.1.1 | Partial | Selected cases | Only 13 of Verbora's 22 languages overlap (verified from `src/weights.rs`); no confidence/abstention signal at all — always returns a `Lang`, biasing forced-choice accuracy comparisons. |
| Language Detection | the reference | reference | 8.1.1 | No | No | **Verified: the reference has no general statistical language-detection module.** No equivalent functionality to compare — correctly excluded, not a research gap. |

**Memory (real, measured — `cargo run --release -p competitive-rust --example memory_report`, raw counts in `results/memory-report.json`'s `language_detection.measurements` array).** Detector *construction* and per-call *detection* are measured and reported as two separate numbers — a startup question and a steady-state question with different real answers, never conflated into one. **Construction:** `WhatlangDetector::new`/raw `whatlang::Detector::new` are genuinely free — 0 allocations, 0 bytes, a `const fn` unit struct with no runtime model to build; `lingua::LanguageDetectorBuilder::from_languages(...).build()` (21-language-restricted, per this section's own note above) allocates 399 times/68,081 bytes; `whichlang` has no detector type at all to construct (`detect_language` is a bare fn — reported `n/a` in the raw data, not a fabricated zero). **Per-call detection**, each measured after one unmeasured warm-up call (`whatlang`'s `ALPHABET_LANG_MAP` is a process-wide `LazyLock` that only fires on the first-ever alphabet-path call — see the example's own doc comment): raw `whatlang::Detector::detect()` measured **25 allocations/11,468 bytes** on the dataset's English `sentence`-tier text, independently confirming — via the new shared `memory` module, not by re-citing — `crates/verbora-language/benches/language.rs`'s prior, separately-probed figure of exactly 25 allocations for this input; `WhatlangDetector::detect` (Verbora's own wrapper) measured 26/11,476 — the documented +1 for its own `candidates` `Vec` on a `Some`-result input. `lingua`'s per-call cost is 244 allocations/33,841 bytes — real API overhead (`detect_language_of` takes its argument by value, so one `String` allocation per call is intrinsic to its published contract, not a benchmarking artifact) — and its process RSS jumps by roughly 42 MB during its *first* (unmeasured, warm-up) detection call specifically, not during `build()`: `lingua` lazily loads its real n-gram frequency model on first use, a genuine cold-start-vs-steady-state distinction its construction number alone does not capture. **`whichlang` measured 0 allocations, 0 bytes per detection call** — a real, meaningful memory win over Verbora's 26 (`WhatlangDetector`) on this specific dimension, purchased at the already-documented cost of only a 13/22-language overlap and no abstention signal (see `docs/PERFORMANCE_GAPS.md` entry 18, and entry 7 for the same pairing's TIME-dimension result).

**Update, two new Verbora detector strategies benchmarked (2026-08-22
campaign).** `crates/verbora-language` added `HashedLinearDetector` (a
zero-allocation, stack-only hashed linear model, opt-in behind
`fast-language-detection`) and `FallbackDetector<HashedLinearDetector,
WhatlangDetector>` (the fast model as primary, deferring to
`WhatlangDetector` only where it declines to judge). Both are now measured
in `benches/language.rs` alongside the existing `WhatlangDetector` default,
against the same `lingua`/`whichlang` competitors above. `HashedLinearDetector`
beats `whichlang` at every tier including the hardest (single-word: 44.2 ns
vs. 62.6 ns) but trades accuracy for it (45/52 vs. `whichlang`'s 48/52 on
the published 13-language × 4-tier evaluation set, all four losses
concentrated in the word/phrase tiers) — which is why it stays opt-in
rather than becoming `DefaultDetector`. `FallbackDetector` matches
`WhatlangDetector`'s accuracy exactly (49/52) while beating `whichlang` on
both accuracy and speed at every tier except single words, where it defers
to the slow path. `crates/verbora-language/src/lib.rs`'s own crate-level doc
comment carries the canonical version of this comparison table.

## 1.10 Script Detection

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Script Detection | `whatlang::detect_script` | Rust | 0.18.0 | Partial | Yes | Same crate as language detection, corrects the task's own "likely no direct competitor" hint — a real, public, standalone function: per-character Unicode-range classification + majority vote, algorithmically the same idea as Verbora's. Scope difference: `whatlang::Script` covers 25 scripts vs. Verbora's 10 (Thai/Armenian/Georgian etc. fall into Verbora's `Other` bucket). |
| Script Detection | lingua (internal alphabet filter) | Rust | 1.8.0 | No | No | Private implementation detail of its language-detection engine; nothing publicly callable to benchmark. |
| Script Detection | the reference | reference | 8.1.1 | No | No | Verified: no script/writing-system detection module anywhere in the reference. |

## 1.11 Transliteration

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Transliteration (Japanese kana→romaji) | the reference (`TransliterateJa`) | reference | 8.1.1 | Yes | Yes | Byte-exact parity port, verified against 143,060 real calls into the reference module — as fair as a comparison gets. |
| Transliteration (Japanese kana→romaji) | wana_kana | Rust | 5.0.0 | Partial | Selected cases | Same task shape (kana→romaji, kanji/Latin pass-through) but a verified-different romanization convention (doubled vowels, e.g. `"スーパー"→"suupaa"`) vs. Verbora/the reference's modified-Hepburn macrons (`"tōkyō"`). Fair for throughput on identical kana input, not for output correctness. |
| Transliteration (Japanese kana→romaji) | kakasi | Rust | 0.1.0 | No | No | Different task: dictionary-based kanji reading resolution, not kana-only table transliteration; also stale (single 2022 release), GPL-3.0. |
| Transliteration (Japanese kana→romaji) | romkan | Rust | 0.2.2 | No | No | Doubled-vowel convention (own README example); effectively abandoned (3 stars, no activity since 2021). |
| Transliteration (Japanese kana→romaji) | romaji (uzimith) | Rust | 0.1.1 | No | No | Kunrei-shiki romanization (a third scheme); abandoned since 2017 (5 stars). |
| Transliteration (Japanese kana→romaji) | kana-jp | Rust | 0.1.0 | No | No | Too immature/unadopted (single 2024 release, 1,298 lifetime downloads, GPL-3.0-only) to be a credible "known/relevant" competitor. |

*Every Rust crate located that performs kana→romaji conversion uses a romanization convention other than modified Hepburn with macrons — a genuine, honestly-reported ecosystem gap, not a search failure.*

## 1.12 TF-IDF

### Corpus build / ingestion

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| TF-IDF — corpus build (`add_document`, `add_file_sync`, `par_add_documents_batch`) | the reference (`TfIdf.addDocument`) | reference | 8.1.1 | Yes | Yes | Reference implementation; byte-identical semantics by construction. |
| TF-IDF — corpus build | `tfidf` (afshinm) | Rust | 0.3.0 | Partial | Selected cases | Stateful `add()`/`add_vec()`, but a different weighting variant (see Query row below) — build-phase timing is fair, output values are not. |
| TF-IDF — corpus build | `rust-tfidf` | Rust | 1.1.1 | No | No | No ingestion step exists at all — it is stateless; nothing to benchmark as "build." |
| TF-IDF — corpus build | Tantivy | Rust | — | No | No | Excluded per TANTIVY POLICY — full segmented indexing pipeline does categorically more work than a standalone tf-idf corpus. |

### Query / scoring

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| TF-IDF — query/scoring (`idf`, `tf`, `tfidf`, `tfidfs`, `list_terms`) | the reference (`TfIdf.tfidf`/`.idf`) | reference | 8.1.1 | Yes | Yes | Same source; Verbora's own doc-comment benchmark (18 ns cached vs. 2.6 µs cache-miss) is against this exact implementation. |
| TF-IDF — query/scoring | `rust-tfidf` | Rust | 1.1.1 | Partial | Selected cases | Same log-based family, but different default weighting (augmented/normalized TF × plain-log IDF, no `+1`) and stateless (recomputes idf over the full corpus every call, no cache). |
| TF-IDF — query/scoring | `tfidf` (afshinm) | Rust | 0.3.0 | Partial | Selected cases | `idf = log₁₀(N/df)` (no smoothing), `tf = log₁₀(count)+1` — a genuinely different variant; closest in *architecture* (stateful struct, `.idf()`/`.tfidf()` methods) of any Rust candidate found. |
| TF-IDF — query/scoring | Tantivy | Rust | — | No | No | Excluded per TANTIVY POLICY — BM25 scoring runs inside full query execution, not a standalone queryable tf-idf table. |

**Memory (real, measured — `cargo run --release -p competitive-rust --example memory_report`, raw counts in `results/memory-report.json`'s `tfidf.measurements` array).** At n=256 rotated documents of the same ~163 kB article `benches/tfidf.rs` already uses, **build** (Verbora `add_document` × 256 vs. `tfidf` afshinm's `add()` × 256 — `rust-tfidf` has no ingestion step, matrix-confirmed above, not measured) allocates 11,513 times/112,193,216 bytes for Verbora and 3,591 times/134,223,872 bytes for afshinm: Verbora does more allocation *churn* (more, smaller allocations) but frees more of it back during construction (3,094 deallocations vs. afshinm's 0), ending with a smaller net live footprint despite the higher call count. **Query** (one cold `.tfidf("the", 0)` call, deliberately measured as a cache-miss on every side — Verbora's own idf-cache benefit is per-corpus-instance and already documented separately in `crates/verbora-tfidf/src/tfidf.rs`, not reproduced here) is where the real finding is: Verbora — **5 allocations, 335 bytes** — and `rust-tfidf` — 1 allocation, 3 bytes, operating over input vectorized outside the measured region — are both trivially cheap, but `tfidf` (afshinm) measured **20,183,718 allocations, 100,360,252 bytes** for the identical single query on the identical 256-document corpus — a real, substantial **Verbora win** on the memory dimension (consistent with entry 13's own TIME-dimension finding that afshinm rescans every document on every query with no cache or index), not a loss — no `docs/PERFORMANCE_GAPS.md` entry follows from this row.

## 1.13 Classifiers

### Naive Bayes

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `BayesClassifier` | the reference (`BayesClassifier`) | reference | 8.1.1 | Yes | Yes | Direct source of the port. |
| `BayesClassifier` | `classifier` (jackm321/Rust_Classifier) | Rust | 0.0.3 | Yes | Yes | Text-in/text-out API essentially method-for-method equivalent (`add_document`, `train`, `classify`, `set_smoothing`) — same task shape, no adapter needed; effectively unmaintained since 2015. **Not actually benchmarked**: verified during implementation that `classifier = "=0.0.3"` fails to compile on this workspace's pinned toolchain (rustc 1.97.1, edition 2024) — its `Classifier` struct derives `rustc-serialize`'s pre-1.15 `RustcDecodable`/`RustcEncodable` compiler-plugin macros, removed from stable Rust years ago (`rustc-serialize` itself has had no release since 2016). Recorded with the failing probe in `manifests/competitors.json`'s `classifier (jackm321/Rust_Classifier)` entry rather than silently dropped; `smartcore` and `linfa-bayes` below are the two Naive Bayes competitors actually executed. |
| `BayesClassifier` | `naivebayes` (ruivieira) | Rust | 0.1.2 | Partial | Selected cases | Pre-tokenized input, but smoothing is a fixed probability floor (1e-9) rather than count-based `1+smoothing` — different smoothing mechanics, not just API, confirmed concretely (not just from reading the source) by `rust-competitors/tests/classifiers_naivebayes_logistic.rs`'s `naivebayes_smoothing_floor_is_fixed_not_count_based`, which shows the floor stays exactly `1e-9` regardless of a 1000x difference in a label's own document count — a speed-only comparison, per that test file's own doc comment. **Now actually benchmarked**: pinned in `rust-competitors/Cargo.toml` and `manifests/competitors.json`, `naivebayes` group added to `benches/classifiers.rs`'s `bayes_train`/`bayes_predict`. A clean, one-sided TIME win for `naivebayes` at every corpus size — **4.4×-9.5× faster training** (growing with corpus size), **2.9× faster prediction** — filed as `docs/PERFORMANCE_GAPS.md` entry 20 (memory dimension: entry 19, a sibling agent's earlier pass). |
| `BayesClassifier` | smartcore `naive_bayes::multinomial::MultinomialNB` | Rust | 0.6.5 | Partial | Selected cases | Same algorithm family, but operates on a pre-built dense count matrix — no text pipeline; most widely adopted/actively maintained (475K downloads, updated 2026-08-10) general Rust ML crate found. |
| `BayesClassifier` | linfa `linfa-bayes::MultinomialNb` | Rust | 0.8.1 | Partial | Selected cases | Same matrix-level situation as smartcore; second major Rust ML framework, included per spec's "don't limit to one rival." |

**Update, `linfa-bayes` timing retired (2026-08-22 campaign).** `linfa-bayes`
0.8.1's `MultinomialNb::fit_with` calls an unconditional `dbg!` once per
class on every `.fit()` call, not gated behind `#[cfg(test)]` or a feature —
`bayes_train`'s `b.iter` closure calls `fit` millions of times, so a single
campaign run produced a 1.5 GB stderr log that could not be committed.
`smartcore` and `naivebayes` (ruivieira) 0.1.2 remain the two Naive Bayes
timing competitors; `linfa-bayes` stays a dev-dependency and continues to
appear in `tests/classifiers_accuracy.rs` and `examples/memory_report.rs`,
both of which call `fit` a handful of times rather than in a hot loop. Full
account: `benchmarks/competitive/README.md`'s "Retired: `linfa-bayes` 0.8.1
cannot be timed as published" section. This also corrects the `naivebayes`
row's prediction claim above: the 2026-08-22 campaign measured Verbora
**winning** prediction against `naivebayes` (1.78 µs vs. 3.08 µs, 1.73×
faster) with `naivebayes` still winning training (1.00×–3.17× faster,
narrowing with corpus size) — a mixed result, not the one-sided time loss
recorded above from the pass that added the `naivebayes` row.

### Logistic Regression

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `LogisticRegressionClassifier` | the reference (`LogisticRegressionClassifier`) | reference | 8.1.1 | Yes | Yes | Direct source of the port. |
| `LogisticRegressionClassifier` | smartcore `linear::logistic_regression` | Rust | 0.6.5 | Partial | Selected cases | Multiclass softmax via L-BFGS, dense-matrix input, no text vectorizer — different multiclass strategy/optimizer than Verbora's one-vs-rest/gradient descent. |
| `LogisticRegressionClassifier` | linfa `linfa-logistic` | Rust | 0.8.1 | Partial | Selected cases | Same caveat as smartcore: softmax via `argmin`-based optimizer, dense `ndarray` input, no text pipeline. |
| `LogisticRegressionClassifier` | rustlearn | Rust | 0.5.0 | Partial | Selected cases | SGD-based, no built-in multiclass one-vs-rest text classifier; unmaintained since 2018 (last push 2018-07-29) — included for historical prominence (646 stars), flagged as stale. |

**Now actually benchmarked** (all three rows above; matrix's earlier framing as a "future pass" — see `benches/classifiers.rs`'s own module doc comment, now corrected). `logistic_train`/`logistic_predict` groups added, reusing the `bayes_train`/`bayes_predict` groups' own `Vocab` tokenize+vectorize adapter, at `crates/verbora-classifiers/benches/classifiers.rs`'s own small-corpus `logistic_training` sizes (4/8/16 docs — Verbora's gradient descent runs to convergence per class, so larger sizes are impractically slow on every side). Genuinely mixed, not one-sided: Verbora **wins at every size tested against smartcore** (1.6×-5.4× faster, margin narrowing with corpus size) and **at the smallest size against linfa-logistic** (1.5× faster at 4 docs), but **loses to linfa-logistic from 8 docs on** (1.5×-2.8× slower, a real crossover) and **loses decisively and consistently to rustlearn at every size** (11.5×-17.7× slower training; all three competitors also beat Verbora 8.0×-11.9× on single-document prediction). Filed in full, wins and losses alike, as `docs/PERFORMANCE_GAPS.md` entry 21 (memory dimension: entry 22, same competitor set, same conclusion — rustlearn's single-epoch SGD does asymptotically less work than every iterate-to-convergence competitor, Verbora included).

**Update, logistic regression re-measured (2026-08-22 campaign).** Verbora's
margins moved materially in its favor since the paragraph above was
written. Against `smartcore`: Verbora wins at every size by a wider margin
now (4.6×–11.4×, up from 1.6×–5.4×). Against `linfa-logistic`: Verbora now
wins at 4, 8 *and* 12 docs (up to 2.9× faster, up from only 4 docs at
1.5×) and loses only at 16 docs, and more narrowly (1.11× slower, down from
the earlier 1.5×–2.8× range) — the crossover point moved from 8 docs to 16.
Against `rustlearn`: Verbora still loses training at every size, but by
5.4×–7.1× rather than 11.5×–17.7×. Single-document prediction is similarly
narrower: all three competitors still beat Verbora, but by 1.8×–2.25× now,
not 8.0×–11.9×. `docs/PERFORMANCE_GAPS.md` entries 21 and 22 need
re-reading against these figures before either is cited as current.

### Maximum Entropy (GIS)

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `MaxEntClassifier` | the reference (`maxent`) | reference | 8.1.1 | Yes | Yes | Direct source of the port (used internally by the reference's own POS tagger). |
| `MaxEntClassifier` | — | — | — | — | No | **NO FAIR COMPETITOR FOUND** — no Rust crate implementing generalised iterative scaling / log-linear MaxEnt classification was found on crates.io; a genuinely niche, OpenNLP-style algorithm with no Rust ecosystem presence. |

**Memory — Naive Bayes (real, measured — `cargo run --release -p competitive-rust --example memory_report`, raw counts in `results/memory-report.json`'s `classifiers.measurements` array).** MaxEnt is not measured (no Rust competitor exists at all, per the table above); Logistic Regression's own memory numbers are a separate paragraph below, from a separate real measurement (`naivebayes` (ruivieira) was pinned in `Cargo.toml` by a sibling agent partway through this pass and is included below, checked and added rather than skipped on a stale snapshot). **Train** (`add_document`/`train()` × 256, same 256-document corpus shape as the existing `bayes_train` timing sweep): Verbora — 59,489 allocations, 2,890,139 bytes — vs. smartcore — 7,197 allocations, 747,494 bytes — vs. linfa-bayes — 6,472 allocations, 1,119,050 bytes — vs. `naivebayes` — 10,634 allocations, 283,074 bytes. Verbora allocates roughly **8x more times** and **2.6x-3.9x more bytes** than smartcore/linfa-bayes for the same corpus — a real, meaningful memory loss, though not an apples-to-apples one: Verbora's own tokenizer stems and drops stop words inside the timed/measured region (parity-required, per `crates/verbora-classifiers/tests/parity.rs`), while the smartcore/linfa/naivebayes adapters are a bare whitespace-split with no stemming or stop-word filtering — see `docs/PERFORMANCE_GAPS.md` entry 19 for the full caveat and entry 15 for the same pairing's TIME-dimension result. `naivebayes` is its own, distinct shape: **fewer bytes** than either matrix-based competitor (283,074 vs. 747,494/1,119,050) but **more allocations** than both (10,634 vs. 7,197/6,472) — many small allocations rather than a few large dense-matrix ones, consistent with its pre-tokenized-`Vec<String>`, per-document (not pre-built-matrix) training shape. **Classify** (one call on an already-trained/-fit model, the existing `bench_predict` group's fixed corpus): Verbora — 229 allocations, 18,005 bytes — vs. smartcore — 19 allocations, 4,276 bytes — vs. linfa-bayes — 26 allocations, 5,072 bytes (model `fit` performed outside the measured closure, matching `bench_predict`'s own boundary — an earlier draft of this measurement mistakenly left the model re-fit inside the timed region, inflating this row roughly 4x before it was corrected and re-run) — vs. `naivebayes` — 61 allocations, 2,468 bytes (fewest bytes of any competitor, but more allocations than smartcore/linfa-bayes, the same many-small-allocations shape as its own training row). `classifier` (jackm321) is not measured for memory, for the same reason it is not measured for time: it does not compile on this toolchain (see the row above).

**Memory — Logistic Regression (real, measured — `cargo test -p competitive-rust --release --test classifiers_memory -- --nocapture`, `rust-competitors/tests/classifiers_memory.rs`).** **Train** (`add_document`/`train()`/`.fit()` × 16, the same 16-document corpus the `logistic_train`/`logistic_predict` timing groups use at their largest size): Verbora — 22,905 allocations, 1,633,112 bytes — vs. smartcore — 11,276 allocations, 797,144 bytes — vs. linfa-logistic — 1,594 allocations, 1,484,076 bytes — vs. rustlearn — 320 allocations, 17,750 bytes. A one-sided loss on allocation *count* against all three (2.0×/14.4×/71.6× more than smartcore/linfa-logistic/rustlearn respectively); on gross bytes, only 1.10× more than linfa-logistic (whose LBFGS machinery allocates a comparable number of total bytes, in far fewer, larger allocations) but 92.0× more than rustlearn — full detail, including the likely mechanism (rustlearn's single-epoch SGD allocates its coefficient arrays once and nothing more per row, vs. every iterate-to-convergence competitor allocating fresh intermediate vectors every optimizer iteration), in `docs/PERFORMANCE_GAPS.md` entry 22 (TIME dimension: entry 21, same competitor set).

## 1.14 Sentiment

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| Sentiment — AFINN lexicon (English/Spanish/Portuguese) | the reference (`SentimentAnalyzer`, `"afinn"`) | reference | 8.1.1 | Yes | Yes | Direct source of the port; same AFINN-165 data, same negation/summation semantics. |
| Sentiment — AFINN lexicon (English) | `sentiment` (mount-research) | Rust | 0.1.1 | Partial | No | Embeds an older/smaller AFINN-111 (vs. Verbora/the reference's AFINN-165) and has **no negation handling** — a real semantic mismatch, not a rounding difference; own internal tokenizer, not swappable. Three simultaneous, non-narrowable divergences (lexicon version, negation handling, tokenizer) with no shared input domain where all three become moot, unlike e.g. rphonetic's Metaphone (§1.6), which closes its one divergence with a single reconfiguration flag — so this is marked `No` rather than `Selected cases` per the spec's own `SENTIMENT BENCHMARKS`/`NO FAIR DIRECT BENCHMARK` instruction. Not benchmarked in the executed suite; no `sentiment` group exists in `rust-competitors/benches/`. |
| Sentiment — AFINN lexicon (English) | `vader-sentimental` | Rust | 0.1.3 | No | No | Implements VADER — a different lexicon *and* a different rule-based scoring algorithm (intensifiers, punctuation/caps emphasis); actively maintained (2026-05-08) but not the same algorithm. |
| Sentiment — ML-SentiCon lexicon (es/en/gl/ca/eu) | the reference (`SentimentAnalyzer`, `"senticon"`) | reference | 8.1.1 | Yes | Yes | Direct source of the port. |
| Sentiment — ML-SentiCon lexicon (es/en/gl/ca/eu) | — | — | — | — | No | **NO FAIR COMPETITOR FOUND** — no Rust crate implements or embeds this multilingual polarity lexicon. |
| Sentiment — CLiPS Pattern lexicon (nl/it/en/fr/de) | the reference (`SentimentAnalyzer`, `"pattern"`) | reference | 8.1.1 | Yes | Yes | Direct source of the port. |
| Sentiment — CLiPS Pattern lexicon (nl/it/en/fr/de) | — | — | — | — | No | **NO FAIR COMPETITOR FOUND** — no Rust crate implements or embeds the CLiPS Pattern polarity lexicon. |

**Update, `sentiment` row amended to `Selected cases` (2026-08-22
campaign).** The `No` verdict above is correct for arbitrary text, but
`benches/sentiment.rs` found a domain where all three divergences become
simultaneously moot: the 2,438-word intersection of AFINN-111 and AFINN-165
where the two tables agree exactly, excluding Verbora's four negation words
(absent from `sentiment`'s vocabulary anyway) and restricted to
single-space-joined lowercase ASCII tokens, where `sentiment`'s internal
tokenizer and Verbora's `WordTokenizer` produce identical output. Every
exclusion is proved, not assumed, in `rust-competitors/tests/sentiment_correctness.rs`.
On that narrowed domain, `sentiment_score_document` is benchmarked and
Verbora wins at every size (4 to 1024 words), by 10.7×–241×, narrowing as
input grows because `sentiment`'s fixed per-call cost (two internal
tokenizations, four compiled `Regex`es) amortizes. This is a genuine
`Selected cases` row, not a `Yes` — see `benches/sentiment.rs`'s own module
doc comment for the full narrowing argument.

## 1.15 WordNet

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| WordNet (lookup/synset/relation traversal, both reading the `wordnet-db` 3.1.14 data) | the reference | reference | 8.1.1 | Yes | Yes | Literal parity target — same probe positions, same false-negative search bug (944 real lemmas), same reversed traversal order. Required baseline. |
| WordNet | `wordnet` (njaard/wordnet-rs) | Rust | 0.1.2 | No | No | Investigated and rejected — abandoned 9 years (last commit 2017-10-22), 11 GitHub stars, README unreachable. |
| WordNet | — | — | — | — | — | **NO FAIR COMPETITOR FOUND (Rust)** — no actively-maintained Rust crate provides Princeton-WordNet index/data access at comparable scope (lookup + synset + pointer/relation traversal + closure). `wordnet-ls` (archived) and `wordnet-lmf` (different input format entirely — LMF XML, not the flat index/data files) also investigated and rejected. **MEMORY, actually benchmarked (real numbers, not estimated):** `AGENTS.md`'s "Archived Data and Memory Mapping" section previously reported file-size-*estimated* figures (`Resident` ~27 MB, `Indexed` ~27 MB + ~600 KB) from a Fase 2 memmap2 feasibility review — not an allocator trace. `rust-competitors/examples/memory_report.rs` now cross-checks those with real `memory::measure` counts, at `open()` alone and `open()`+first `lookup("entity")` (cold), for all four `Storage` strategies: `Pread`/`LazyResident` **2,013 B** net at `open` (both defer all reading — confirms "none" resident), `LazyResident` **21,632,868 B (~21.6 MB)** at `cold` (less than the full dictionary because `lookup("entity")` — a Noun-only lemma — never touches `data.verb`/`data.adj`/`data.adv`, only their index files, once each POS index reports no match), `Resident` **28,098,948 B (~28.1 MB)** at both `open` and `cold` (matches the ~27 MB estimate directly), `Indexed` **29,192,908 B (~29.2 MB)** at both (its line-start-table overhead measures ~1.09 MB here, real but almost 2× the earlier ~600 KB estimate — a real, allocator-confirmed discrepancy from the old estimate, reported rather than silently reconciled). Full per-strategy breakdown, including gross allocation counts and RSS deltas, in `benchmarks/competitive/results/memory-report.json`'s `wordnet` section. |

**Update, `wordnet-db` amends the "NO FAIR COMPETITOR FOUND" verdict
(2026-08-22 campaign).** `wordnet-db` 0.1.3 (johanneswd) was published after
the "NO FAIR COMPETITOR FOUND (Rust)" row above was written and reads the
same Princeton `index.*`/`data.*` files, from the same directory, answering
the same questions — a real, timeable Rust competitor now exists.
`benches/wordnet.rs` measures `open`, `cold` (open + first lookup) and
`lookup`/`index_entry` for the headline `verbora_resident` ↔
`wordnet_db_owned` pair (both read all eight files into owned buffers with
no `unsafe`), plus `verbora_lazy` ↔ `wordnet_db_mmap` (each side's "defer
what you can" answer, and `Mmap` is `wordnet-db`'s default). Verbora opens
roughly 23,000× faster (`Pread`, 9.83 µs vs. `wordnet-db`'s `Mmap`, 228.17
ms) because `wordnet-db` parses every index line and data record eagerly at
open regardless of `LoadMode`; `wordnet-db` then wins per-query lookups by
roughly 7×–8.5× once both sides are resident, the payoff of its
fully-parsed `HashMap` representation. This measurement also depended on
the `PointerSymbol::from_symbol` fix recorded in
`benchmarks/competitive/README.md`'s "Resolved: `verbora-wordnet` rejected
8.8% of a real dictionary's index entries" section — every figure above
covers the whole WordNet 3.1 dictionary, not the 91.2% that was readable
before that fix. This row should be split into a real matrix entry (`Rust`
/ `Partial` / `Selected cases`) rather than left as `NO FAIR COMPETITOR
FOUND`; recorded here as the finding, not yet reformatted into the table.

## 1.16 POS Tagging

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `BrillPosTagger` (transformation-based, English + Dutch) | the reference | reference | 8.1.1 | Yes | Yes | Literal parity target (`lib/natural/brill_pos_tagger/`); required baseline. |
| `BrillPosTagger` | `postagger` (shubham0204/postagger.rs) | Rust | 0.0.3 | Partial | Yes | Pretrained averaged-perceptron model (from NLTK), same Penn-Treebank tagset family, ready out of the box — but a different algorithm class (perceptron vs. rule-transformation); English only, ~2 years stale. |
| `BrillPosTagger` | rust-bert (`POSModel` pipeline) | Rust | 0.23.0 | Partial | Selected cases | BERT-family transformer POS pipeline, by far the most widely adopted general Rust NLP crate found (254K downloads, 3,077 stars) — only fair with model-load cost isolated from steady-state latency. |
| `BrillPosTagger` | `viterbi_pos_tagger` | Rust | 0.1.0 | No | No | Investigated and rejected — ships no pretrained model (caller must train per invocation), near-zero adoption (1 star), GPL-3.0. |

**Update, tagger data removal (2026-08) — the Verbora side of this comparison
is withdrawn.** `verbora-tagger` 0.3.0 removed the English and Dutch lexicons
and rule sets it shipped (LGPL-3.0 for the English pair, no locatable terms for
the Dutch), so `BrillPosTagger` no longer names any language: the lexicon is a
caller-supplied input. Every Verbora POS figure recorded in this campaign
measured the bundled English configuration and is retired, not pending
re-measurement. The competitor selections and their `Partial`/`No` ratings above
are unaffected — those are judgements about `postagger`, rust-bert and
`viterbi_pos_tagger`, which did not move. Reinstating the comparison is a
measurement-design decision first: `benchmarks/competitive/rust-competitors`
must choose a lexicon and hand the same one to every side, and until that is
settled it does not compile against 0.3.0 (six call sites of
`Lexicon::bundled`/`RuleSet::bundled` across `benches/pos_tagging.rs` and
`tests/pos_tagging_smoke.rs`).

## 1.17 Spellcheck

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `Spellcheck` (Norvig's edit-distance/frequency algorithm, built on Verbora's own `Trie`) | the reference | reference | 8.1.1 | Yes | Yes | Literal parity target — the reference's spellcheck is confirmed Norvig-based (not DoubleMetaphone-based, correcting the task's own hint), built on its own `Trie`. Required baseline. |
| `Spellcheck` | `symspell` (reneklacan/symspell) | Rust | 0.5.2 | Partial | Yes | Same task shape (word in, distance-bounded ranked corrections out, loaded frequency dictionary), different algorithm (delete-precomputation, O(1)-average lookup vs. combinatorial edit generation); actively maintained (release 2026-03-22). **MEMORY, actually benchmarked:** construction (`SymSpell::load_dictionary_line`) over the identical corpus retains **4.5×-4.6× more net memory than Verbora at every size measured** (100: 194,923 B vs. 43,140 B · 1,000: 1,725,668 vs. 376,112 · 10,000: 15,070,872 vs. 3,338,824 · 20,000: 30,252,369 vs. 6,684,700 B) — a decisive Verbora win, and the mirror image of `docs/PERFORMANCE_GAPS.md` entry 8's TIME result (symspell wins query speed, loses construction time *and* construction memory). Real `memory::measure` counts in `benchmarks/competitive/results/memory-report.json`'s `spellcheck` section. |
| `Spellcheck` | `harper-core` (Automattic/harper) | Rust | 2.8.0 | Partial | Yes | By far the most widely adopted/actively maintained standalone spellchecking crate found (14,470 GitHub stars parent repo, released 2026-08-13); FST + Levenshtein-automaton bounded correction, but a curated/fixed dictionary rather than an arbitrary caller-supplied corpus — must load a comparable dictionary or label the comparison explicitly. **MEMORY, actually benchmarked — a real Verbora loss, filed as `docs/PERFORMANCE_GAPS.md` entry 17:** construction (`FstDictionary::new`) over the identical corpus retains **10%-33% LESS net memory than Verbora at every size measured** (100: 28,872 B vs. 43,140 · 1,000: 337,992 vs. 376,112 · 10,000: 2,703,432 vs. 3,338,824 · 20,000: 5,406,792 vs. 6,684,700 B) — `harper-core`'s minimized FST (`fst::MapBuilder`, confirmed by reading `harper-core-2.8.0/src/spell/fst_dictionary.rs`) ends up more memory-compact than Verbora's unminimized `Trie`, at the cost of 3.3×-23.9× more allocator churn to build it (net-retained and gross-churn move in opposite directions — both reported, neither hidden). |
| `Spellcheck` | `spellbook` (helix-editor/spellbook) | Rust | 0.4.2 | Partial | Selected cases | Hunspell affix-rule + morphological correction — same top-level task, meaningfully different (morphology-aware) mechanism; dictionaries can't be shared between the two, so only a matched-workload timing comparison is fair. **MEMORY, actually benchmarked (matched-workload, not same-input — never a ratio):** loading a real `en_US` Hunspell `.aff`/`.dic` pair retains **2,181,949 B** net; Verbora's own full 20,000-word corpus retains **6,684,700 B** — printed side by side in `benchmarks/competitive/results/memory-report.json`'s `spellcheck` section for scale only, per this row's own "matched-workload" note. |
| `Spellcheck` | `zspell` | Rust | 0.5.5 | No | No | Its own README lists `Suggestions: WIP` — only boolean `check` is implemented, not `get_corrections`-equivalent output. |
| `Spellcheck` | `symspell_complete_rs` | Rust | 0.0.4 | No | No | Different capability (typo-tolerant multi-term autocomplete, not single-word correction); own README states "not for third-party use." |
| `Spellcheck` | `fast_symspell` | Rust | 0.1.10 | ~~No~~ **Partial** | ~~No~~ **Yes** | **Verdict overturned this round** (was `No`/`No`: "no repository URL — unverifiable provenance; download curve consistent with abandonment"). Both claims re-checked directly rather than re-asserted (`benches/spellcheck.rs`'s own "re-investigated, not taken on faith" doc comment has the full evidence): crates.io always hosts the raw source tarball regardless of repository metadata, and it is a confirmed, readable, near-verbatim fork of the already-vetted `symspell` 0.5.2 (`ahash` hasher, `triple_accel` SIMD verification pass, plus an `rkyv` zero-copy archived-load path); the "download curve" is genuinely low (22,760 total, 129/90-day) but a real, steady 1–9/day spread, not a cliff to zero — "stale development" (no release since 2025-01-17), not "abandonment" in the download sense the prior verdict claimed. Same task shape and algorithm family as `symspell` above (delete-precomputation), so `Partial`/`Yes`, same classification `symspell` itself carries. **Benchmarked and verified** (`tests/spellcheck_fast_symspell_correctness.rs`, 5 tests before any timing trusted): construction **Verbora wins decisively**, 25×–34× faster (widening with corpus size); `get_corrections` at distance 1 **fast_symspell wins**, 26×–31× faster (both flat with size); at distance **2**, **fast_symspell wins by a dramatically wider margin**, 1,686×–2,769× — see `docs/PERFORMANCE_GAPS.md` entry 35. Also carries a real, independently-confirmed upstream bug (`triple_accel`'s `rdamerau_exp` over-counts a doubled-letter-insertion distance — entry 36) that can cause it to silently miss or misrank a correction on an ordinary typo shape. |
| `FuzzyIndex` (Verbora-native extension, no reference counterpart — BK-tree candidate generation over edit distance, see `AGENTS.md`'s "Verbora-Native Extensions" policy) | `fast_symspell` `SymSpell::lookup` | Rust | 0.1.10 | Partial | Yes | Different data structure entirely (deletion-index vs. BK-tree), same "candidates within edit distance k" question. **Benchmarked and verified**: `FuzzyIndex` **wins construction** (1.72×–9.91× faster, gap narrowing as corpus size grows since deletion-index build cost scales worse); `fast_symspell` **wins query**, and the margin **widens sharply with corpus size**, 2.15×–66.7× — see `docs/PERFORMANCE_GAPS.md` entry 35, and the Architectural decision note directly below. |
| `DeletionIndex` (Verbora-native extension, no reference counterpart — in-house SymSpell-style deletion index, built alongside `FuzzyIndex` rather than replacing it, see `AGENTS.md`'s "Verbora-Native Extensions" policy) | `fast_symspell` `SymSpell::lookup` | Rust | 0.1.10 | Partial | Yes | Verbora's own answer to the query-speed loss above, implemented this round — see the Architectural decision note directly below for the full result. `DeletionIndex` and `FuzzyIndex` were also benchmarked head-to-head against each other (`crates/verbora-spellcheck/benches/deletion_index.rs`). ⚠ **Every ratio from that head-to-head is retired, pending re-measurement; do not cite it here or anywhere else.** The index's map has since been re-keyed from the full deletion sequence onto a 64-bit hash of it, which changes both timed paths and takes the cost of indexing one `n`-scalar word from cubic in `n` to quadratic — so the numbers describe a structure that no longer exists, and the direction of the change must not be inferred in speed either. Retained for provenance, in the shape they were recorded: a genuine crossover, `FuzzyIndex` winning at the smallest size (100 words, 1.73× faster query) and `DeletionIndex` winning from 1,000 words up by a rapidly widening margin (4.9×–54.3×) — the same shape `fast_symspell` itself showed against `FuzzyIndex` — against an honest construction cost, `DeletionIndex` measuring 13×–25× slower to build than `FuzzyIndex` at every size. What survives without a timing is the shape alone, because it follows from the designs: dearer construction bought against a query cost that grows far more slowly with corpus size. See `docs/PERFORMANCE_MATRIX.md`'s own `DeletionIndex` entry for the full table and the same caveat. |
| `FuzzyIndex` (Verbora-native extension) | `fst::Set` + `fst::automaton::Levenshtein` | Rust | 0.4.7 | NARROWED_EXACT | Yes | Both answer "which stored words are within edit distance k of this query" identically on ASCII/BMP input (verified `BTreeSet`-equal across a spread of real-corpus queries in `tests/fst_fuzzy_correctness.rs` before any timing trusted) — the strongest-equivalence row `fst` carries anywhere in this document. Narrowed because of one disclosed, real divergence outside this domain: `fst` 0.4.7's Levenshtein automaton silently returns incomplete results for same-byte-length multi-byte UTF-8 substitutions (a confirmed, still-open upstream bug — `docs/PERFORMANCE_GAPS.md` entry 36, item 2). `fst` also has a real, hard failure mode `FuzzyIndex` does not share: `Levenshtein::new` errors past a default 10,000-DFA-state budget (a sufficiently long query at a wide-enough `max_distance` can simply refuse to build). **Benchmarked** (`cargo bench --bench fst_fuzzy`, medians): construction is a genuine **crossover** — `FuzzyIndex` wins small (1.88× at n=100) but `fst` wins from n=1,000 up (1.64×–3.66×, widening); query is the **opposite crossover** — `FuzzyIndex` wins small and mid-size, dramatically at n=100 (54.3×) narrowing through n=1,000 (6.55×) to near-parity at n=10,000 (1.06×), then `fst` overtakes at n=20,000 (1.46×). Consistent with `fst`'s per-query DFA-construction cost (real, and separately why `Levenshtein::new` can fail outright — see above) dominating at low query volume/small corpora, amortizing better as corpus size grows. See `docs/PERFORMANCE_GAPS.md` entry 37 for the full breakdown. |

**Architectural decision (per `docs/research/fase6-benchmark-brief.md`'s directive to weigh this, not just measure it) — implemented, not just recommended.** `fast_symspell`'s real, widening query-speed win over `FuzzyIndex` (entry 35) was genuine evidence that a second, deletion-index-backed spellcheck structure was worth having; `verbora_spellcheck::DeletionIndex`/`DeletionIndexBuilder` now exist, built in-house with `verbora_distance` primitives exactly as recommended (not by wrapping `fast_symspell` itself, given its real upstream bug — entry 36 — and young/low-adoption profile). `FuzzyIndex` stays the default: simpler correctness argument (triangle-inequality pruning only vs. `DeletionIndex`'s real over-generate-then-verify step), cheaper and more predictable construction (⚠ the 13×–25× figure that quantified this is retired with the rest of the head-to-head above; the *direction* follows from the designs and stands, the magnitude does not), and no `max_distance`-fixed-at-build-time constraint — `DeletionIndex`'s own `neighbors` rejects a query asking for more distance than the index was built for, returning `Err(DistanceBeyondIndex)` with the requested value and the ceiling. **Corrected:** this note previously said `neighbors` "silently caps" such a query, which described an earlier revision; it never caps and never silently narrows the question, because returning the distance-2 answer to a distance-3 query would hide a caller mistake rather than surface it. The build-time ceiling is still a real, disclosed, structural limitation — it is simply a loud one. `DeletionIndex` earns its place for the specific case its own construction cost is worth paying for: a large dictionary (the retired measurement put the crossover at ≥1,000 words, below which `FuzzyIndex` won the query benchmark too; no replacement threshold is stated until the re-run), a known and fixed `max_distance`, and query volume high enough to amortize the one-time build cost. See `docs/PERFORMANCE_MATRIX.md`'s own `DeletionIndex` entry for the full numbers and correctness discipline (including a real UTF-16-code-unit-vs-`char` bug found and fixed during implementation, before any benchmark number was trusted).

## 1.18 Trie

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `Trie` (UTF-16-code-unit keying, the reference `for…in` child order, `keys_with_prefix` case-folding bug preserved) | the reference | reference | 8.1.1 | Yes | Yes | Literal parity target (`lib/natural/trie/trie`); required baseline. |
| `Trie` (exact semantics) | — | — | — | — | — | **NO FAIR COMPETITOR FOUND (Rust)** — no Rust crate replicates UTF-16-code-unit keying, the reference `for…in` enumeration order, or the case-folding bug. |
| `Trie` (generic prefix-search throughput only) | `trie-rs` (laysakura) | Rust | 0.4.2 | Partial | Selected cases | `predictive_search`/`common_prefix_search` map onto `keys_with_prefix`/`find_matches_on_path`; byte-keyed/byte-ordered, not UTF-16/reference-ordered; highest download count of any candidate in this whole audit (5.9M total). |
| `Trie` (generic prefix-search throughput only) | `qp-trie` (sdleffler) | Rust | 0.8.2 | Partial | Selected cases | General ordered radix map with prefix iteration; weaker fit than trie-rs — not purpose-built for text completion. |
| `Trie` (`build`/`contains`/`predictive_search` throughput) | `fast_radix_trie` (bluecatengineering) | Rust | 1.2.0 | Partial | Selected cases | Path-compressed radix map, created 2025-10-30, actively updated; uses `unsafe` internally (dynamically-sized nodes via raw pointers, `miri`-tested per its own docs) — this benchmarking workspace is not bound by the main workspace's `unsafe_code = "deny"` policy, but Verbora's own trie has zero `unsafe`, a real architectural difference behind any timing gap. **Benchmarked and verified** (`tests/trie_correctness.rs`, order-blind set equality): a genuine split, not a one-sided result. Verbora **wins** `build` (1.35×–1.54×) and `contains` (1.07×–1.18×); Verbora **loses** `predictive_search` — 1.64× ("1char" prefix) to 2.19× ("all"/empty prefix), the operation path compression specifically targets. See `docs/PERFORMANCE_GAPS.md` entry 32. |
| `Trie` (`build`/`contains`/`predictive_search` throughput) | `fst` (BurntSushi) | Rust | 0.4.7 | Partial | Selected cases | Finite-state transducer, not a trie — built once from sorted input via a streaming builder, queried via `Streamer`, never mutated again; added per `docs/research/fase6-benchmark-brief.md`'s "specialized frozen competitor" directive. `build` includes the required sort+dedup step inside the timed closure (a real, disclosed asymmetry — `Trie::add_strings` accepts any order for free). **Benchmarked and verified**: Verbora **wins every operation measured** — `build` 2.09×–4.49× faster, `contains` 1.50×–1.63× faster, `predictive_search` 1.21×–1.61× faster. See `docs/PERFORMANCE_GAPS.md` entry 33. Also used for a *second*, separate comparison — Levenshtein-automaton fuzzy lookup against `verbora-spellcheck::FuzzyIndex`, see §1.17's `FuzzyIndex` rows below. |
| `FrozenTrie` (Verbora-native extension, no reference counterpart — a safe-Rust, path-compressed, read-only trie built by `Trie::freeze`, see `AGENTS.md`'s "Verbora-Native Extensions" policy) | `fast_radix_trie` (bluecatengineering) | Rust | 1.2.0 | Partial | Selected cases | Verbora's own answer to the `predictive_search` loss above, implemented this round — see the Architectural decision note directly below for the full result and why it is a genuine trade-off, not a clean win: `FrozenTrie` closes most of the gap and **overtakes** `fast_radix_trie` on the realistic single-letter-prefix shape (1.06× faster), but still trails on full-corpus enumeration (1.45× slower), and its `contains` genuinely regresses against both the plain `Trie` arena and `fast_radix_trie`. Uses **zero `unsafe`**, unlike `fast_radix_trie`'s raw-pointer dynamically-sized nodes. |

*`radix_trie` (michaelsproul) was investigated (79.7M total downloads) but not included as a primary row — that volume is almost certainly transitive-dependency traffic from unrelated routing-table use, not genuine text-completion adoption; flagged rather than headlined. (Distinct crate from `fast_radix_trie` (bluecatengineering) above — confirmed different publishers/codebases, not a naming collision.)*

**Architectural decision (per `docs/research/fase6-benchmark-brief.md`'s directive to weigh this, not just measure it) — implemented, not just recommended.** `fast_radix_trie`'s real `predictive_search` win was genuine evidence that a compressed, radix-style frozen query representation for `verbora-trie` was architecturally plausible; `verbora_trie::Trie::freeze(&self) -> FrozenTrie` now exists, built via exactly the `Build → Freeze → Query` step this note originally proposed (analogous to `verbora-tagger`'s existing `build.rs`-packed-lexicon pattern, §1.16), keeping the current mutable arena as the build-time representation and adding compression only at query time. It uses no `unsafe` at all (compressed edges are ranges into a shared `Vec<u16>` buffer, not raw pointers) — the `unsafe`-acceptance question this note originally flagged turned out not to be necessary.

**The real result is a genuine trade-off, not a clean win — reported in full, not cherry-picked.** Re-measured against `fast_radix_trie` head-to-head (`cargo bench -p competitive-rust --bench trie`): `FrozenTrie` is 1.06× **faster** than `fast_radix_trie` on `predictive_search/1char` (single-letter prefixes — the shape a real autocomplete issues) but still 1.45× **slower** on `predictive_search/all` (empty-prefix, full-corpus enumeration). It is also, honestly, a **regression** relative to the plain `Trie` arena on `contains` (1.65×–1.71× slower), which in turn means `FrozenTrie` now *loses* `contains_hit`/`contains_miss` to `fast_radix_trie` too, where the unfrozen arena used to win. See `docs/PERFORMANCE_GAPS.md` entry 32's "Update" section for the full numbers, the architectural reason the trade-off runs this direction (fewer-but-costlier hops for a single-path point lookup vs. genuinely fewer total node-visits across a whole-subtree enumeration), and the shipped recommendation: keep both representations, `Trie` for point-lookup-heavy call sites, `FrozenTrie` (frozen once after bulk-loading) for enumeration/autocomplete-heavy ones — never a blanket replacement of one by the other.

Correctness was checked two independent ways before any of the numbers above were trusted: an 80-round randomized fuzzer inside `crates/verbora-trie/src/frozen.rs`'s own test module, and a fully separate adversarial audit (a second agent with no visibility into the implementation's own design reasoning, which wrote and ran its own fresh adversarial tests — including a surrogate pair deliberately split across two different compressed edges — and validated its own tests had real teeth by injecting and then catching two deliberately-introduced bugs before reverting them). Neither pass found a disagreement between `Trie` and `FrozenTrie`.

## 1.19 Analyzers

| Verbora capability | Competitor | Language | Version | Equivalent? | Benchmarkable? | Notes |
|---|---|---|---|---|---|---|
| `SentenceAnalyzer` (PP-marking + subject/predicate split + 4-way sentence-type classification, over caller-supplied POS-tagged input) | the reference | reference | 8.1.1 | Yes | Yes | Literal parity target (`lib/natural/analyzers/`) — same substring-match tag bugs (`pos.match('IN')` matching `VBIN`), same no-default-arm `switch`. Required baseline. |
| `SentenceAnalyzer` | — | — | — | — | — | **NO FAIR COMPETITOR FOUND (Rust)** — no Rust crate performs this specific composed task. `nlprule` (bminixhofer, 670 stars, stale since 2023-05-23) does grammar/chunking but no statement/question/exclamation/command classifier; rust-bert could theoretically be fine-tuned for sentence-type classification but ships no such model — a different technique entirely (trained neural classification vs. fixed tag-substring rules). |

---

# 2. Competitor Research Dossiers

Full research detail for every candidate marked `Yes` or `Partial` above, organized by module in the same order as the matrix. The reference's baseline profile is documented once, since every module-specific entry shares the same library/version/adoption/maintenance/license facts — module-specific caveats (e.g. "no Jaro export," "no language detection at all") are called out inline in each module's own section.

## the reference — baseline profile (applies to every "the reference" row above)

- **Library / language / version**: `natural`, the reference, **8.1.1** — confirmed by all 7 research agents as the exact version of the vendored copy in use at research time (published 2026-02-27). No version drift between the vendored copy and current upstream.
- **Popularity/adoption**: widely adopted; figures independently reproduced by 5 of the 7 reports — see the "Data-quality notes" section below for one reporting discrepancy on open-issue count.
- **Maintenance**: last GitHub push 2026-02-22, repo `updated_at` 2026-08-15; not archived; actively maintained by a multi-person maintainer team.
- **License**: MIT.
- **Why selected everywhere it appears**: mandated by the spec's own `NATURAL IS A REQUIRED BASELINE` section — wherever the reference implements semantically equivalent functionality it must appear, and in every module above it is not merely "a" competitor but the literal reference implementation each Verbora module is byte-for-byte ported from (confirmed by every research agent reading the actual vendored source, not assuming from the module name).
- **Why fair**: by construction — line-for-line algorithmic identity for every ported module, verified against real parity-test fixtures in Verbora's own test suite (`fixtures/*.json`).

## 2.1 Tokenizers

- **tantivy** (`WhitespaceTokenizer`, `SimpleTokenizer`) — Rust, **0.26.1**. Repo: `github.com/quickwit-oss/tantivy`. 16,689,434 all-time downloads, 3,684,884/90d; 15,699 GitHub stars, 441 open issues; pushed 2026-08-15 (day of research); MIT. Selected because it is the most widely-adopted, actively-maintained Rust text-analysis library with dedicated whitespace/alnum-run tokenizers, and the TANTIVY POLICY explicitly permits tokenization-layer overlap. Fair for throughput; `WhitespaceTokenizer` splits on ASCII whitespace only (`is_ascii_whitespace()`, confirmed by reading `tantivy-0.26.1/src/tokenizer/whitespace_tokenizer.rs` directly), diverging from the reference `\s`'s wider Unicode whitespace set; `SimpleTokenizer` uses a Unicode-wide alnum class vs. Verbora's ASCII+Cyrillic-only class. The executed benchmark narrows its input to punctuation-free, single-ASCII-space-joined lowercase words — the domain where every one of these divergences is unreachable, proven in `benchmarks/competitive/rust-competitors/tests/tokenizers_correctness.rs`.
- **Hugging Face `tokenizers`** (`WhitespaceSplit`, `Whitespace` pre-tokenizers) — Rust, **0.23.1**. Repo: `github.com/huggingface/tokenizers`. 26,971,643 all-time / 10,726,267/90d downloads; 10,970 stars, 254 open issues; pushed 2026-08-13; Apache-2.0. Selected as the ecosystem-dominant Rust tokenization library; only the narrow pre-tokenizer *components* are in scope (never the full BPE/WordPiece `Tokenizer::encode` pipeline, which does categorically more work).
- **unicode-segmentation** — Rust, **1.13.3**. Repo: `github.com/unicode-rs/unicode-segmentation`. 501,129,492 all-time / 118,012,255/90d downloads (one of the highest-download crates in the Rust ecosystem, mostly transitive); 672 GitHub stars; pushed 2026-07-31; MIT OR Apache-2.0. Selected as the canonical, UAX#29-conformant Rust word/sentence segmentation library. Fair as "two ways to split text into words/sentences," not as "the same algorithm" — the word-boundary *definition* differs from Verbora's fixed regex class.
- **segtok** — Rust, **0.1.5**. Repo: `github.com/xamgore/segtok`. 1,038,536 all-time / 452,824/90d downloads but only 2 GitHub stars — a real discrepancy suggesting mostly transitive use, flagged rather than hidden. License listed MIT on crates.io; GitHub's own detector reports no LICENSE file present (unresolved inconsistency). Selected as the closest available rule-based (not statistical) sentence/word segmenter to the reference/Verbora's own philosophy; secondary/labeled comparator, not the flagship Rust competitor.
- **punkt (rust-punkt, ferristseng)** — Rust, **1.0.5**. Investigated, not selected as a primary competitor: implements the statistical/unsupervised Punkt algorithm (different family) and is stale since 2020-01-27 (38 stars, 522 downloads/90d). Recorded for completeness per the spec's "document every real candidate found."

## 2.2 N-Grams

- **ngrammatic** (compenguy) — Rust, **0.7.0**. Repo: `github.com/compenguy/ngrammatic`. Its headline feature, fuzzy corpus search (`Corpus`/`CorpusBuilder`/`search`), stays **NO FAIR COMPETITOR FOUND** — a genuinely different problem from plain n-gram generation, with no Verbora equivalent to compare it against. Re-examined at implementation time, though: the crate's own `Ngram`/`NgramBuilder` — the character n-gram + frequency-count generator `Corpus` is itself built on — turns out to be a fair, comparable primitive against Verbora's generic `ngrams()` engine called with `T = char`. `tests/ngrams_correctness.rs` confirms byte-identical `(gram, count)` output across all 20,000 words in the shared word list, arity 2 and arity 3, before any timing number was trusted. **Benchmarked** (`benches/ngrams.rs`, full-default Criterion, median metric, 3 independent runs, consistent direction every time): bigrams — Verbora wins all 3 runs, ~1.07×–1.16× faster; trigrams — Verbora loses all 3 runs, ngrammatic ~1.03×–1.08× faster — see `docs/PERFORMANCE_GAPS.md` entry 38.
- Every other dedicated Rust n-gram crate found remains rejected: `ngrams` pwoolcoc 1.0.1 and `ngram` nytopop 0.1.13 are both abandoned (~10y and ~6y stale respectively), `ngram-search` 0.1.1 is abandoned, and `creature_feature` 0.2.0 solves a different problem (ML featurization). None of these cover the word-tokenizing string-input helpers (`ngrams_str`/`bigrams_str`/`trigrams_str`), the `ngrams_with_stats`/`ngram_key` stats mode, or the `zh::*` UTF-16-splitting family — those remain without a Rust competitor; see §3.

The reference clears the bar for every capability in this module; see its baseline profile above.

## 2.3 Stemmers

- **rust-stemmers** (CurrySoftware) — Rust, **1.2.0**. Repo: `github.com/CurrySoftware/rust-stemmers`. 28,286,893 total / 8,387,970/90d downloads; 135 GitHub stars; last commit 2021-05-13 (version published 2019-11-17) — de facto ecosystem standard despite being stale for 5+ years. MIT/BSD-3-Clause dual. Confirmed 18-language `Algorithm` enum via docs.rs. Selected as the well-known Rust Snowball wrapper (also what Tantivy's own stemmer token filter is built on). **Fair for the 9 non-English Snowball languages** (de es fr it nl no pt ru sv) — same canonical algorithm. **Not fair for English** — `Algorithm::English` is Snowball Porter2, a documented different algorithm from the original 1980 Porter Verbora's `PorterStemmer` implements (diverges on >5% of a sample vocabulary per snowballstem.org).
- **porter-stemmer** (samgiles) — Rust, **0.1.2**. Repo: `github.com/samgiles/porter-stemmer`. 16,304 total / 1,742/90d downloads; 8 stars; last updated 2019-07-17 (~7 years stale). MPL-2.0. The best-adopted crate implementing the *correct* (original, non-Porter2) algorithm, filling the gap rust-stemmers leaves for English. Operates on grapheme clusters rather than UTF-16 units — an architectural difference that shouldn't matter on ordinary English words. **Implementation-time finding**: confirmed — the grapheme-cluster question never manifests on the plain-ASCII benchmarked corpus (63/64 exact agreement, one unrelated real bug found and excluded: `"sky"` → `"ski"`). This alternative beats Verbora on both time and allocation count at every batch size ≥16 — see §1.3's matrix row and `docs/PERFORMANCE_GAPS.md`.
- **nltk-porter** (VoiceLessQ) — Rust, **0.1.0**. Repo: `github.com/VoiceLessQ/nltk-porter`. 194 total downloads; created 2026-06-26 (~7 weeks old at research time), essentially unproven. Apache-2.0. Exposes NLTK's `ORIGINAL_ALGORITHM` mode, the closest documented-intent match to "the 1980 Porter algorithm" among all candidates.
- **nltk-lancaster** (VoiceLessQ) — Rust, **0.1.0**. Repo: `github.com/VoiceLessQ/nltk-lancaster`. 25 total downloads, 0 stars; created 2026-06-26. Apache-2.0. **The only Rust crate found anywhere implementing the Paice/Husk Lancaster algorithm.** Self-reports "differential-tested against Python nltk's LancasterStemmer: 68,000+ words, zero mismatches" — a strong claim, independent verification still required.
- **lindera `japanese_katakana_stem` filter** — Rust, **5.2.0** (`lindera-filter`). Repo: `github.com/lindera/lindera`. 1,853,255 total / 702,991/90d downloads; last published 2026-08-14 (day before this research); MIT. Leading actively-maintained Japanese morphological-analysis crate; its katakana-stem filter is the same well-known Kuromoji-style technique as Verbora's `StemmerJa`, likely configurable to match exactly via its `min` parameter — but only runs inside a full tokenizer+dictionary pipeline by default, so must be isolated to the filter step alone. **Implementation-time correction**: the `1,853,255`/`702,991` download figures and `2026-08-14` publish date above belong to the *parent* `lindera` crate, which this research pass conflated with `lindera-filter` itself — checked directly on crates.io during implementation, `lindera-filter`'s own last release is **0.32.3**, published 2025-03-18, over a year stale relative to the parent crate and never reaching version 5.2.0 at all: Lindera's 5.x rewrite dropped the separate `-core`/`-tokenizer`/`-filter` crates and folded token/character filters into a new **`lindera-analysis`** crate instead, which *does* carry a real, matching `5.2.0` release with the identical `JapaneseKatakanaStemTokenFilter` (same `min` parameter, confirmed by reading both crates' published source) — pinned and benchmarked as `lindera-analysis` 5.2.0, not `lindera-filter`. `min = 3` (the filter's own default) verified to reproduce `StemmerJa`'s threshold exactly; Verbora wins decisively on both time and memory — see §1.3's matrix row.
- **sastrawi** (iDevoid/rust-sastrawi) — Rust, **0.1.1**. Repo: `github.com/idevoid/rust-sastrawi`. 3,033 total / 49/90d downloads; MIT; last updated 2020-07-18, single day of activity then dormant ~6 years. Genuinely rare true algorithmic-lineage match: both Verbora and this crate independently port the same PHP Sastrawi reference project. `sastrawi-rs` (ibahasa) looks more modern ("zero-regex/zero-copy/FST-powered") but has no discoverable crates.io release, so cannot be version-pinned per the spec's rule against implicit `latest`. **Implementation-time finding**: the shared lineage is real — both dictionaries hold exactly 29,932 root words — but this port is *not* a complete implementation of the reference algorithm: it has no hyphenated-reduplication/compound-plural handling at all and only a single (not iterated) prefix-stripping pass, real algorithmic gaps found and documented in `tests/stemmers_correctness.rs`, not merely "dormant since 2020." On the 13-of-16 words where it does agree with Verbora, it is dramatically faster (~3.6×–6.8× across all batch sizes) — see §1.3's matrix row and `docs/PERFORMANCE_GAPS.md`.

## 2.4 Normalizers

- **unaccent** — Rust, **0.1.1**. Repo: `github.com/crowdtech-io/unaccent`. 62,172 total / 21,046/90d downloads (strong recent uptake for a young crate); created 2025-01-11, updated 2025-02-14. License listed "non-standard" by crates.io — exact terms need pulling from the repo's own LICENSE file before use. NFD-decompose-then-strip mechanism vs. Verbora's deliberately non-decomposing table lookup — agrees on plain precomposed Latin-1 text, diverges on decomposed input/ligatures/ß/ſ.
- **diacritics** (YesSeri) — Rust, **0.2.2**. Repo: `github.com/YesSeri/diacritics`. 63,745 total / 3,547/90d downloads; 5 GitHub stars; last updated 2024-05-13 (~2 years stale). GPL-3.0 — copyleft, flagged as a materially different license class from the mostly-MIT/Apache field. Semantically the closest shape to Verbora's function (case-preserving, Latin-diacritic-focused).
- **unicode-jp** (gemmarx) — Rust, **0.4.0**. Repo: `github.com/gemmarx/unicode-jp-rs`. 68,402 total / 3,234/90d downloads; last updated 2020-04-11 (~6 years stale); MIT. Covers only 2 of Verbora's 17 `normalize_ja` conversions.
- **kana-converter** — Rust, **0.1.2**. `github.com/kitsuneninetails/kana-converter`. 7,119 total / 95/90d downloads; single day of activity, August 2018; MIT. Narrower still than unicode-jp.

## 2.5 Inflectors

- **pluralizer** (KennethGomez) — Rust, **0.5.0**. Repo: `github.com/KennethGomez/pluralizer`. 1,493,517 total / 755,282/90d downloads (strongly active); updated 2025-01-17 on crates.io with a further release as recent as 2026-08-01 per docs.rs; MIT/Apache-2.0. Same general algorithmic strategy (ordered regex-suffix rule chain + irregulars + uncountables) as the `TenseInflector` engine underlying Verbora's `NounInflector`, best-maintained English pluralizer found — but an independently-maintained rule table (`pluralize`-derived), not the reference's own.
- **Inflector** (whatisinternet) — Rust, **0.11.4**. Repo: `github.com/whatisinternet/inflector`. **106,743,134 total / 13,927,214/90d downloads** — the single largest crate found in this entire research pass by raw download count (likely inflated by transitive/build-time dependents, still a real, verifiable figure); created 2015, last updated 2019-01-20 (~7 years stale); BSD-2-Clause. Covers both `NounInflector` (`to_plural`/`to_singular`) and `CountInflector` (`ordinalize`) capability areas.
- **inflector-plus** (victorteo) — Rust, **0.11.7**. Fork/continuation of `Inflector`; 14.9K downloads; stale since Sept 2022.
- **ordinal** (heaths/ordinal-rs) — Rust, **0.4.0**. Repo: `github.com/heaths/ordinal-rs`. 372,820 total / 35,308/90d downloads; last updated 2025-04-21 — actively maintained, unlike almost every other candidate in this dossier; MPL-2.0. The single best-fit match found across the whole Inflectors group: narrow, single-purpose, actively maintained, exactly matching `CountInflector::nth`'s job. Needs verification of the reference's signed-remainder quirk on negative integers (`nth(-1)`="-1th") before a clean benchmark.

## 2.6 Phonetics

- **rphonetic** (Dalvany) — Rust, **3.0.6**. Repo: `github.com/Dalvany/rphonetic`. #62 in crates.io's "Text processing" category; ~21,796 downloads/month; 17 GitHub stars; last push 2026-07-11; Apache-2.0 (confirmed via `Cargo.toml` — GitHub's own license detector mis-reports "Other/NOASSERTION," `Cargo.toml` is authoritative). Rust port of Apache commons-codec's phonetic package, covering `Soundex`, `Metaphone`, `DoubleMetaphone`, `DaitchMokotoffSoundex` in one actively-maintained crate — the tightest single-crate match found for all four of Verbora's algorithm families. Selected as the only actively-maintained, reasonably-adopted Rust crate covering all four families from the canonical Apache reference. Fair for throughput on every algorithm; not fair for byte-exact output comparison anywhere (textbook Soundex vs. The reference's condense-before-drop variant; default max-code-length-4 Metaphone/DoubleMetaphone must be reconfigured to match Verbora's 32; D-M's genuine multi-branch `soundex()` must not be conflated with the single-branch `encode()`).

## 2.7 Phonetic Index / Phonetic Neighbors

No competitor selected — see §3. `PhoneticIndex` already ships its own internal Criterion benchmarks (`crates/verbora-phonetics/benches/phonetic_index.rs`) comparing its `InlineCode`+CSR design against a plain `HashMap<String,Vec<EntryId>>`, a frozen `HashMap<Code,Box<[EntryId]>>`, and (Soundex only) a perfect-hash dense array — this internal "Verbora API Strategies" comparison is the recommended substitute for a cross-library row.

**Memory, now independently re-confirmed with the real allocator-counting infra** (`rust-competitors/src/memory.rs`'s `measure`, installed as `competitive-rust`'s `#[global_allocator]`): `phonetic_index.rs`'s own `bench_alt_designs_query` group prints an *analytical* (`size_of`-based) estimate for the shipped design — 2,899,788 B / 29.00 B-per-entry at 100K SoundEx entries. `rust-competitors/examples/memory_report.rs` measured the identical design with a live allocator trace over its own (differently-constructed, cycled-corpus) 100K-entry dictionary and got 2,937,590 B / 29.38 B-per-entry — a 1.3% difference, i.e. real, independent confirmation rather than a repeated assumption. See §1.7's own Notes cell for the full number set (both encoders, 10K and 100K) and `benchmarks/competitive/results/memory-report.json`'s `phonetic_index` section for the raw data.

## 2.8 Distances

- **strsim** — Rust, **0.11.1**. Repo: `github.com/rapidfuzz/strsim-rs`. **989,876,846 total / 204,458,609/90d downloads** — the de-facto standard string-similarity crate in the Rust ecosystem by every metric checked; 494 GitHub stars; pushed 2025-11-27 (under the `rapidfuzz` org, absorbed from the original `dguo/strsim-rs`); MIT. Char-indexed (`.chars()`), the same unit Verbora counts — one Unicode scalar on both sides, so the two agree on astral-plane input as well as on the Basic Multilingual Plane, with no restriction needed on the shared corpus. Sørensen-Dice remains a **different algorithm variant**, though a narrower one than previously recorded: the multiset-not-set bigram counting stands, and so does its up-front removal of every whitespace character and its use of **byte** lengths in both the `< 2` short-circuit and the `a.len() + b.len() - 2` denominator; what no longer diverges is case handling (both are case-sensitive) and the two-empty-strings case (both `1.0`). Only fair in a restricted input domain — see §1.8's row for the exact conditions.
- **rapidfuzz** (rapidfuzz-rs) — Rust, **0.5.0**. Repo: `github.com/rapidfuzz/rapidfuzz-rs`. 1,642,373 total / 980,955/90d downloads; published crate is stale (2023-12-01) though the GitHub repo itself was pushed 2024-06-29 (unreleased development exists past 0.5.0, not yet shipped); MIT per `Cargo.toml` (README claims dual MIT/Apache-2.0 — a real metadata mismatch). The only single crate found that explicitly separates unrestricted (`damerau_levenshtein`) from restricted/OSA (`osa`) Damerau-Levenshtein while also covering Levenshtein, Hamming, Jaro, Jaro-Winkler — the tightest algorithmic match to Verbora's own split, which mirrors rapidfuzz's shape exactly: two separately-named functions, `damerau_levenshtein` and `osa`, chosen by name rather than by a mode flag. Does not implement Dice at all.
- **triple_accel** — Rust, **0.4.0**. Repo: `github.com/Daniel-Liu-c0deb0t/triple_accel`. 1,874,447 total / 219,611/90d downloads; 110 stars; last commit 2023-03-13 (~3 years dormant, not archived). MIT. SIMD-accelerated (AVX2/SSE4.1) exact Hamming/Levenshtein/restricted-only Damerau plus bounded fuzzy substring search — a genuinely relevant *performance* competitor. Operates on raw UTF-8 bytes (not chars/UTF-16 units), so it diverges on ordinary accented/non-Latin text, not just rare astral-plane input — fair only on ASCII-only corpora.
- **editdistancek** — Rust, **1.0.2**. Repo: `github.com/nkkarpov/editdistancek`. 368,648 total / 55,227/90d downloads; 11 stars; last commit 2024-03-25. MIT. Specialist exact-Levenshtein-only crate (Ukkonen/Landau–Myers–Schmidt-inspired, bounded-`k` variant available), byte-level API — same ASCII-only caveat as triple_accel, plus fixed unit costs only (no parameterized costs).
- **stringmetrics** — Rust, **2.2.2**. Repo: `github.com/pluots/stringmetrics`. 8,543,533 total / 3,173,998/90d downloads (real, currently-growing); last commit 2024-09-03; Apache-2.0 (crates.io flags SPDX metadata as "non-standard," but the actual LICENSE file text is standard Apache-2.0). Char-indexed, genuinely supports independent per-operation costs (`LevWeights{insertion,deletion,substitution}`) — the closest ecosystem analogue to Verbora's `LevenshteinCosts` and its `levenshtein_weighted` entry point. The costs are `u32` there and `f64` here, so `stringmetrics` cannot price a fractional operation and has nothing to validate, while Verbora's constructor returns `Result` and rejects negative and non-finite costs before they can reach a metric. **Important trap found by reading source, not README**: its exported `damerau_levenshtein` is an unimplemented stub that always returns `0` — do not benchmark stringmetrics's Damerau-Levenshtein under any circumstances.
- **eddie** — Rust, **0.4.2**. Repo: `github.com/thaumant/eddie`. 45,522 total / only 1,112/90d downloads (declining); 20 stars; last publish/commit both 2020-01-18/19 (~6 years abandoned). MIT. Struct-based API covering exactly Verbora's Jaro/Jaro-Winkler surface, with a documented `str` (Unicode-safe) vs. `slice` (fast, "incorrect for UTF-8") split. Historically built to be benchmarked against strsim/`distance`/`natural` (visible in its own dev-dependencies) — a secondary/low-priority reference, re-verify against fresh vectors before trusting.
- **fuzzt** — Rust, **0.3.1**. Repo: `github.com/luizvbo/fuzzt`. 1,561,692 total / 558,944/90d downloads (striking spike for a 4-star crate — likely transitive, weak direct-trust signal); created 2024-02-09; MIT. Explicitly "heavily based on strsim-rs" — included only to show the Dice-variant divergence documented above is a shared lineage issue across two published crates, not a strsim-specific quirk; redundant with strsim for every other metric.

## 2.9 Language Detection

- **whatlang** (raw crate) — Rust, **0.18.0**. Repo: `github.com/greyblake/whatlang-rs`. 2,715,822 total / 806,674/90d downloads; 1,088 GitHub stars, 119 forks; last release 2025-10-16, last commit 2025-12-24; MIT. This is the literal dependency `WhatlangDetector` already wraps — benchmarking it standalone isolates wrapper/mapping overhead, not a rival algorithm; must be labeled as such.
- **lingua** — Rust, **1.8.0**. Repo: `github.com/pemistahl/lingua-rs`. 1,894,116 total / 411,649/90d downloads; 1,118 stars, 63 forks; last release 2026-03-09, last push 2026-03-26; Apache-2.0. Uses 1–5-gram n-grams plus a rule-based alphabet pre-filter, explicitly marketed as stronger on short/single-word input than trigram detectors — a directly relevant, substantively different algorithm (not redundant with whatlang). Default 75-language config must be restricted via `LanguageDetectorBuilder::from_languages()` to the 21-language overlap with Verbora (missing only Galician) or the comparison violates the spec's "5 idiomas vs. 75" rule.
- **whichlang** — Rust, **0.1.1**. Repo: `github.com/quickwit-oss/whichlang`. 355,258 total / 107,401/90d downloads; 457 stars, 26 forks (quickwit-oss org); only two crates.io releases ever (0.1.0 2023, 0.1.1 Jan 2025) — recent GitHub commits are dependabot-only, effectively maintenance-mode; MIT. Hashed-feature linear classifier, 0 dependencies, explicitly pitched as "blazingly fast." Enum has exactly 16 languages; overlap with Verbora's 22 is exactly 13 (missing Ukrainian, Polish, Persian, Indonesian, Norwegian, Finnish, Galician, Catalan, Basque). Cannot abstain (`detect_language` always returns a `Lang`, defaulting to `Eng` on zero-signal input) — must be accounted for separately in any accuracy metric, not folded in silently.

## 2.10 Script Detection

- **whatlang::detect_script** — same crate/version/adoption/maintenance/license as §2.9's whatlang entry (zero extra dependency cost if whatlang is already present). `pub fn detect_script(text: &str) -> Option<Script>` — for every non-stop char, tests up to 25 `is_<script>()` Unicode-range predicates (confirmed literal range-table matches, not a model), returns the plurality winner; `raw_detect_script` exposes full per-script counts. Same conceptual algorithm as Verbora's `detect_script`. Found by reading `whatlang`'s source tree directly rather than assuming from its "language detection" reputation. `whatlang::Script` enumerates 25 named scripts (strict superset of Verbora's 10 — `Mandarin`≡`Han`); text Verbora buckets as `Other` (Thai, Armenian, Georgian, etc.) `whatlang` correctly names. Implementation also differs structurally: Verbora does one `match` per codepoint (O(1)); whatlang linear-scans up to 25 boolean predicates per char — a legitimate thing to *measure*, not a disqualifier.

## 2.11 Transliteration

- **wana_kana** — Rust, **5.0.0**. Repo: `github.com/PSeitz/wana_kana_rust`. 556,612 total / 132,349/90d downloads; 90 stars, 16 forks; last push 2026-05-08; MIT. Ported from the reference `WanaKana` v4.0.2, since diverged with its own improvements. `ConvertJapanese` trait — `to_kana()`/`to_hiragana()`/`to_katakana()`/`to_romaji()`, plus character-class predicates, tokenization, okurigana trimming. The only Rust kana↔romaji crate with real current adoption/maintenance found — every alternative investigated (kakasi, romkan, romaji/uzimith, kana-jp) is either scope-mismatched or effectively abandoned. Verified from its own test suite (`src/to_romaji.rs`) to use doubled-vowel ("wāpuro") output (`"スーパー"→"suupaa"`) rather than modified-Hepburn macrons — structurally matches Verbora otherwise (kanji/Latin/punctuation pass-through, `&str` in/out), so fair for latency/throughput on identical kana input, never for output correctness.

## 2.12 TF-IDF

- **rust-tfidf** (ferristseng) — Rust, **1.1.1**. Repo: `github.com/ferristseng/rust-tfidf`. 29,531 total / 1,354/90d downloads; 19 stars, 4 forks; last release 2021-05-18, last push 2023-06-15; MIT OR Apache-2.0. Generic, strategy-pattern TF-IDF calculator (`DoubleHalfNormalizationTf` × `InverseFrequencyIdf` by default) over `NaiveDocument`/`ProcessedDocument` trait objects — no corpus object, no incremental add/removal, every call recomputes over the full collection passed as a parameter. Most-downloaded/starred standalone Rust TF-IDF crate found. Different default weighting scheme than Verbora/the reference (augmented/normalized TF vs. raw-count TF; plain-log IDF vs. `1+ln` IDF) — a genuine algorithm difference, and its stateless/no-cache architecture does asymptotically more work per query, worth showing explicitly rather than hiding.
- **tfidf** (afshinm/tf-idf) — Rust, **0.3.0** (only version ever published). Repo: `github.com/afshinm/tf-idf`. 6,176 total / 40/90d downloads; 9 stars; created Feb 2017, last update March 2017 (~9 years dormant). MIT. Stateful corpus (`TfIdf::new()`, `add()`/`add_vec()`, `tf`/`idf`/`tfidf`/`similarities`), case-insensitive. `idf = log₁₀(N/df)` unsmoothed, `tf = log₁₀(count)+1` — the only other general-purpose Rust TF-IDF crate found with a stateful, Verbora-shaped API (persistent object, not one-shot function), but a different weighting formula entirely.

## 2.13 Classifiers

- **classifier** (jackm321/Rust_Classifier) — Rust, **0.0.3** (only/latest version). Repo: `github.com/jackm321/Rust_Classifier`. 10,600 total / 141/90d downloads; 31 stars, 2 forks; last crates.io publish 2015-12-11 (a later GitHub push, 2021-09-29, appears to be non-release maintenance) — effectively unmaintained ~10 years. Apache-2.0. The only Rust naive-Bayes crate found whose public API takes raw or pre-tokenized documents directly (`add_document`, `train`, `classify`, `set_smoothing`) with multi-class support — same level of abstraction as Verbora's `BayesClassifier`, no adapter needed.
- **naivebayes** (ruivieira) — Rust, **0.1.2**. Repo: `gitlab.com/ruivieira/naive-bayes`. 5,900 total / 62/90d downloads; created 2018-11-28, last updated 2019-03-25 (~7 years dormant). Apache-2.0. Second independent lightweight bag-of-words NB implementation; smoothing is a fixed minimum-probability floor rather than count-based additive smoothing.
- **smartcore** — Rust, **0.6.5**. Repo: `github.com/smartcorelib/smartcore`. **475,970 total / 145,779/90d downloads** — by far the highest of any classifier candidate found; 947 GitHub stars, 103 forks, 52 open issues; crate/GitHub both updated 2026-08-10 (4 days before this research) — actively maintained. Apache-2.0. `MultinomialNB` and multiclass softmax `logistic_regression` (via L-BFGS), plus Gaussian/Bernoulli/Categorical NB variants. Canonical choice per the spec's selection policy (widely adopted + actively maintained + mature) among general Rust ML frameworks; no text tokenization/vectorization — requires an explicit, documented, identical adapter for a fair benchmark.
- **linfa** (`linfa-bayes`, `linfa-logistic`) — Rust, **0.8.1** for both sub-crates. Repo: `github.com/rust-ml/linfa`. `linfa-bayes`: 44,632 total / 13,607/90d downloads; `linfa-logistic`: 136,048 total / 30,716/90d downloads; workspace has 4,729 GitHub stars, 332 forks, 76 open issues; both sub-crates published 2025-12-23, workspace pushed 2026-05-30. MIT OR Apache-2.0. The other major scikit-learn-styled Rust ML framework, included alongside smartcore per the spec's own "don't limit to one framework" example; same no-text-pipeline caveat, plus a different multiclass strategy for logistic regression (joint softmax/L-BFGS vs. Verbora's one-vs-rest/plain gradient descent).
- **rustlearn** — Rust, **0.5.0**. Repo: `github.com/maciejkula/rustlearn`. 47,179 total / 1,560/90d downloads; 646 GitHub stars, 56 forks; crate and GitHub both last touched 2018-07-29 — unmaintained ~8 years. Apache-2.0. For years *the* well-known general Rust ML crate before smartcore/linfa existed; SGD-based logistic regression, no built-in naive Bayes, no text pipeline. Included for historical/completeness reasons, flagged as the weakest/lowest-priority of the three ML-framework candidates.
- **sentiment** (mount-research) — Rust, **0.1.1** (only version). Repo: `github.com/mount-research/sentiment`. 17,384 total / 685/90d downloads; 12 stars, 3 forks; published once 2017-04-20, last GitHub push 2018-05-03 (~8 years dormant). License MIT per crates.io, but GitHub's own license detector reports `NOASSERTION` — a discrepancy to flag before any commercial redistribution. Only Rust crate found using an AFINN-family lexicon by name.
- **vader-sentimental** — Rust, **0.1.3**. Repo: `github.com/bosun-ai/vader-sentimental`. 12,943 total / 9,663/90d downloads; 7 stars, 2 forks; last updated 2026-05-08 — the most actively-maintained sentiment crate found. MIT. Investigated specifically so the exclusion reasoning is visible rather than silently dropped — see matrix row, excluded as a different algorithm+lexicon (VADER, not AFINN).

## 2.14 WordNet

No Rust competitor selected — the sole named candidate (`wordnet` njaard/wordnet-rs, 0.1.2, BSD-2-Clause, 5,474 total/39·90d downloads, 11 stars, dormant since 2017-10-22) was investigated and excluded on maintenance/adoption grounds. `wordnet-ls` (jeffa5, formally archived on GitHub 2025-05-19) and `wordnet-lmf` (utkarshkukreti, parses a different XML interchange format entirely) were also checked and rejected. See §3.

**Memory, now real allocator counts, not file-size estimates:** `rust-competitors/examples/memory_report.rs` measured all four `Storage` strategies with `memory::measure` (a live global-allocator trace) at `open()` and `open()`+first `lookup("entity")` (cold) — cross-checking, not merely repeating, the file-size-based estimates in `AGENTS.md`'s "Archived Data and Memory Mapping" section (Fase 2's memmap2 feasibility review). `Resident` measured at 28,098,948 B (~28.1 MB, matching the prior ~27 MB estimate closely); `Indexed`'s line-start-table overhead measured at ~1.09 MB, real but nearly double the prior ~600 KB estimate — a genuine, now-quantified discrepancy between the old estimate and a real trace, not silently reconciled. See §1.15's own Notes cell for the full four-strategy breakdown.

## 2.15 POS Tagging

- **postagger** (shubham0204/postagger.rs) — Rust, **0.0.3**. Repo: `github.com/shubham0204/postagger.rs`. 4,212 total / 94/90d downloads; 8 stars; single release 2024-01-04, last push 2024-04-12 (~2 years stale). Apache-2.0. NLTK-inspired averaged-perceptron tagger, ships pretrained weights extracted directly from NLTK's `averaged_perceptron_tagger.zip` — ready to use immediately, matching Verbora's "load once, tag immediately" shape, but a fundamentally different algorithm (trained linear classifier vs. deterministic rule table). English only.
- **rust-bert** — Rust, **0.23.0**. Repo: `github.com/guillaume-be/rust-bert`. 254,557 total / 21,457/90d downloads; 3,077 GitHub stars, 76 open issues — by far the most widely adopted general Rust NLP crate found in this entire audit; last push 2026-01-13. Apache-2.0. LibTorch-backed BERT-family pipelines including a bundled `POSModel`. Genuinely canonical/widely-adopted per the spec's selection criteria, but a transformer forward pass over a downloaded checkpoint is a categorically different technique from Verbora's microsecond-scale rule table — model-load cost must be isolated from steady-state per-sentence latency, presented as its own "traditional vs. neural" comparison.

## 2.16 Spellcheck

- **symspell** (reneklacan/symspell) — Rust, **0.5.2**. Repo: `github.com/reneklacan/symspell`. 171,281 total / 19,726/90d downloads; 144 stars, 0 open issues; latest release 2026-03-22 — actively maintained. MIT. Rust port of Wolf Garbe's SymSpell (precomputed deletion dictionary, O(1)-average lookup) — most established, most actively maintained SymSpell-family Rust crate found. Same `lookup(word, verbosity, max_edit_distance)` shape as Verbora's `get_corrections`; must load both engines with the same frequency dictionary and `max_distance`.
- **harper-core** (Automattic/harper) — Rust, **2.8.0**. Repo: `github.com/Automattic/harper` (monorepo, crate at `harper-core/`). `harper-core` specifically: 104,167 total / 9,733/90d downloads; parent repo has **14,470 GitHub stars**, 754 open issues; latest release **2026-08-13**, days before this research — extremely actively maintained. Apache-2.0. Engine behind the Harper grammar/spelling checker (used via `harper-ls` in Zed, Neovim, etc.); `spell` module independently usable — `suggest_correct_spelling`/`suggest_correct_spelling_str` via FST + Levenshtein automaton. By a wide margin the most widely adopted, most actively maintained standalone spellchecking-capable crate found in this whole pass. Dictionary is curated/bundled, not an arbitrary corpus — either load a comparable dictionary into both sides or label the comparison explicitly.
- **spellbook** (helix-editor/spellbook) — Rust, **0.4.2**. Repo: `github.com/helix-editor/spellbook`. 114,919 total / **34,427/90d** downloads (highest recent figure besides the reference itself); 140 stars, 6 open issues; last push 2026-07-11 — very active; production use in `cargo-spellcheck` and a Zed/LSP extension. MPL-2.0. `no_std` Hunspell-`.aff`/`.dic`-compatible spellchecker (`check`/`suggest`), driven by affix rules over a curated stem+flag dictionary — a linguistically different, more sophisticated mechanism than Verbora's flat frequency-corpus + edit-distance approach; dictionaries can't be shared, only a matched-workload timing comparison is fair. Self-described "alpha" maturity.

**Memory, now actually benchmarked with the real allocator-counting infra** (`rust-competitors/src/memory.rs`'s `measure`): construction-time allocator counts for all three competitors, loaded exactly as described above (symspell/harper-core on Verbora's own corpus, spellbook on a real `en_US` Hunspell dictionary), via `rust-competitors/examples/memory_report.rs`. Result: Verbora decisively **wins** memory against symspell (which needed 4.5×-4.6× more net memory to build, on top of the 29×-32× more TIME `docs/PERFORMANCE_GAPS.md` entry 8 already recorded) but **loses** memory against harper-core (which needed 10%-33% *less* net memory to build, at every corpus size — filed as `docs/PERFORMANCE_GAPS.md` entry 17, a genuinely different shape from entry 8's mixed TIME crossover). See §1.17's own Notes cells for the full number set.

## 2.17 Trie

- **trie-rs** (laysakura) — Rust, **0.4.2**. Repo: `github.com/laysakura/trie-rs`. **5,931,573 total / 756,843/90d downloads** — highest of any candidate found in this whole research pass; 135 stars, 15 open issues; last release 2024-05-12, last push 2025-03-14 (over a year stale but functionally stable with heavy ongoing download volume). MIT OR Apache-2.0. LOUDS-based memory-efficient trie/trie-map; `predictive_search`/`common_prefix_search` are literally the "text-completion/prefix-search capability" the research hint asked to look for. Fair for insert/contains/prefix-enumeration throughput on ASCII English word lists; UTF-8-byte keying (vs. Verbora's UTF-16-code-unit keying), byte-lexicographic result order (vs. The reference `for…in` order), and no equivalent case-folding bug all invalidate any output/ordering-equivalence claim.
- **qp-trie** (sdleffler) — Rust, **0.8.2**. Repo: `github.com/sdleffler/qp-trie-rs`. 363,253 total / 67,770/90d downloads; 103 stars, 16 open issues; last push 2024-04-21 (over 2 years stale). MPL-2.0. QP-trie (nybble-branching radix trie), general ordered map with `remove_prefix` and prefix-scoped iteration — positioned as a general keyed map rather than a text-completion structure, the weaker of the two Selected-cases options.

## 2.18 Analyzers

No competitor selected. `nlprule` (bminixhofer, Rust, 0.6.4, Apache-2.0/MIT, 156,169 total/15,121·90d downloads, 670 stars, 27 open issues, stale since 2023-05-23) and rust-bert (already profiled in §2.15) were both investigated and rejected — see §3.

---

# 2.19 Candidate competitors not yet evaluated

Found after the matrix was written. Each closes a module that currently has no
competitive number at all. None is measured yet; adding one means a new bench
target, a correctness target, and a re-run of that target alone — not another
campaign.

| Module | Candidate | Version / last release | Recent downloads | Comparability note |
|---|---|---|---|---|
| Sentiment | `sentiment` | 0.1.1, Nov 2017 | 628 | Unmaintained for nine years, but a real AFINN-style scorer. Age must be stated beside any figure; a stale competitor is a fair comparison only if the reader knows it is stale. |
| WordNet | `wordnet` | 0.1.2, Nov 2017 | 39 | Same vintage, and barely used. Weakest of the three. |
| WordNet | `wordnet-db` | 0.1.3, Jan 2026 | 1,346 | **The interesting one.** A memory-mapped reader for prebuilt WordNet files, actively used. `verbora-wordnet` cannot mmap — `unsafe_code = "deny"` forbids it, and `LazyResident` is the declared stand-in. Measuring against this puts a number on the cost of that policy, which is today an assertion with no figure. Comparability limit: it is data-only, with no query functions, so the honest comparison is load-and-access, not synset lookup. Timing Verbora's richer operation against its narrower one would flatter whichever side we chose to under-describe. |
| WordNet / thesaurus | `thesaurus` | 0.5.2, Aug 2022 | 10,469 | By adoption the strongest WordNet-adjacent competitor by two orders of magnitude over `wordnet`. Offers WordNet by default and Moby behind a feature. Synonym lookup is the overlapping capability: WordNet synsets *are* synonym sets, so the comparison is fair on that operation and on nothing else — `thesaurus` has no hypernym/hyponym traversal to compare against. |

`analyzers`, `util` and `tagger` also carry no competitor; `tagger` is measured
under `pos_tagging`.

# 3. Modules / sub-capabilities with no fair competitor identified

Every `NO FAIR COMPETITOR FOUND` outcome from the matrix, in one place, with its one-line reason — so a reader can see at a glance what will and won't have a competitive number in the next phase.

| Module | Sub-capability | Reason |
|---|---|---|
| Tokenizers | 15 of 16 `AggressiveTokenizer` language variants (De, Fr, Es, Ru, Pl, Pt, No, Sv, Vi, Id, Hi, Uk, Nl, Fa) | Several reproduce specific, deliberate the reference bugs (e.g. German drops uppercase umlauts); no Rust crate attempts these per-language, bug-preserving classes. |
| Tokenizers | `WordPunctTokenizer` (Rust side) | Every candidate found either drops punctuation entirely (tantivy, less work) or groups punctuation runs into single tokens (HF `Whitespace`) — a real output mismatch on common input, not an API difference. |
| Tokenizers | `TreebankWordTokenizer` (Rust side) | No Rust port of the NLTK/Penn-Treebank fixed 17-pass rewrite exists; HF/`rust_tokenizers` do vocabulary-driven subword encoding, a different algorithm class solving a different problem. |
| Tokenizers | `CaseTokenizer` (Rust side) | Exists specifically to reproduce a reference-runtime bug (`"undefined"` suffix leak); no equivalent should exist in another language. |
| Tokenizers | `OrthographyTokenizer` (Rust side) | No Rust crate implements this per-language single-matcher-table API shape. |
| Tokenizers | Generic `RegexpTokenizer` engine (Rust side, standalone) | Comparing to Rust's own `regex` crate would be Verbora vs. its own dependency, not a competing NLP library. |
| N-Grams | `ngrams_str`/`bigrams_str`/`trigrams_str` (word-tokenizing string input), `ngrams_with_stats`/`ngram_key`, and the `zh::*` UTF-16-code-unit-splitting family (Rust side) | No actively-maintained, meaningfully-adopted dedicated Rust n-gram crate tokenizes into words first or replicates the UTF-16 `zh::*` splitting behavior; the closest matches (`ngrams` pwoolcoc, `ngram` nytopop) are ~10 and ~6 years stale respectively. Plain character-level n-gram + frequency-count generation now has a real Rust competitor — see §1.2/§2.2 (`ngrammatic`'s `Ngram`/`NgramBuilder`). |
| Stemmers | `CarryStemmerFr` (French Carry variant, Rust side) | No Rust crate implements this non-canonical 3-pass suffix-table algorithm; standard Snowball French (rust-stemmers) is a different algorithm. |
| Stemmers | `PorterStemmerFa` (Persian) | the reference's own Farsi "stemmer" is a documented no-op identity stub — there is no real algorithm on either side to benchmark. |
| Stemmers | `PorterStemmerUk` (Ukrainian, Rust side) | Not an official Snowball language; absent from rust-stemmers' 18-language list; the one unverified crate found was too weak to select. |
| Normalizers | `normalize_no` / `normalize_sv` (Rust side) | No Rust crate replicates these exact selective, per-alphabet diacritic-fold subsets. |
| Normalizers | `case::restore_case` (Rust side) | A UTF-16-indexed, reference-runtime-quirk case-pattern restorer; nothing in the Rust ecosystem targets this specific capability. |
| Inflectors | `PresentVerbInflector` (Rust side, at matched scope) | The one candidate found (`english` crate) does full multi-tense/person/form conjugation — a materially bigger, differently-shaped job. |
| Inflectors | `NounInflectorFr` (Rust side) | No dedicated Rust French-noun-pluralization crate was located. |
| Inflectors | `NounInflectorJa` (Rust side) | Nothing targets Japanese noun "pluralization," a marginal grammatical category in Japanese to begin with. |
| Inflectors | `CountInflectorFr` (Rust side) | The one French-numerals crate found spells out full cardinal number words — a fundamentally larger job than appending "er"/"e". |
| Phonetic Index / Phonetic Neighbors | Entire module | Verbora-native extension with no the reference equivalent and no library (Rust or otherwise) offering a build-once/query-many bucket-by-phonetic-code structure; Tantivy/Lucene/generic search explicitly excluded (categorically larger scope). Recommend internal-only benchmark (already implemented in `benches/phonetic_index.rs`). |
| Distances | Plain Jaro vs. The reference | the reference's Jaro computation is an unexported module-private helper — no way to call it from outside. |
| Distances | Sørensen-Dice, general case (Rust side) | Every Rust Dice implementation found (strsim, fuzzt) uses a materially different variant (multiset vs. set, case-sensitive vs. folded, no whitespace-collapse-and-pad) — only a narrow restricted-input intersection is fair. |
| Distances | Fuzzy substring search, general case (Rust side, beyond triple_accel) | strsim/rapidfuzz/eddie/editdistancek/stringmetrics all compute only a scalar two-string distance — none locate a best-matching substring + offset within a longer target. |
| Language Detection | vs. The reference | Verified: the reference has no general statistical language-detection module at all (no dependency, no exported functionality). |
| Script Detection | vs. The reference | Verified: no script/writing-system detection module anywhere in the reference. |
| Script Detection | vs. lingua | Its alphabet-determination logic is a private implementation detail, not a publicly callable function. |
| Transliteration | Byte-exact modified-Hepburn kana romanization (Rust side) | Every Rust kana→romaji crate found (wana_kana, kakasi, romkan, romaji, kana-jp) uses a different romanization convention (doubled-vowel wāpuro or Kunrei-shiki), or is scope-mismatched (kanji dictionary resolution), or effectively abandoned. |
| TF-IDF | Corpus build/ingestion (Rust side, `rust-tfidf`) | `rust-tfidf` has no stateful ingestion step at all — nothing to benchmark as "build." (`tfidf` afshinm and Tantivy remain Partial/excluded respectively — see matrix.) |
| Classifiers | Maximum Entropy / GIS (Rust side) | No Rust crate implements generalised iterative scaling / log-linear MaxEnt classification; the algorithm has essentially no Rust ecosystem presence outside ports of Java/Python NLP frameworks that don't exist in Rust. |
| Sentiment | ML-SentiCon lexicon family (es/en/gl/ca/eu, Rust side) | No Rust crate embeds or wraps this multilingual polarity lexicon. |
| Sentiment | CLiPS Pattern lexicon family (nl/it/en/fr/de, Rust side) | No Rust crate embeds or wraps this multilingual polarity lexicon. |
| WordNet | Rust side, entire module | The only named candidate (`wordnet` njaard/wordnet-rs) is abandoned ~9 years; no actively-maintained alternative exists at comparable scope (lookup + synset + relation traversal + closure). |
| Trie | Exact semantics (UTF-16 keying, the reference `for…in` order, `keys_with_prefix` case-folding bug) | No Rust crate replicates any of these three specific behaviors; `trie-rs`/`qp-trie` are fair only for generic prefix-search throughput, not output/ordering equivalence. |
| Analyzers | Rust side, entire module | No Rust crate performs the composed task (PP-marking + subject/predicate split + 4-way sentence-type classification over pre-tagged input) in one rule-based pass; the closest candidates (`nlprule`, rust-bert) solve different, differently-shaped problems. |

**Update, text-shaping migration (2026-08) — eight of the rows above have
stopped being "no competitor" rows and become "no Verbora side" rows.** The
migration deleted the capability itself in each case (see §1.1, §1.2 and
§1.4's own update blocks, and `docs/design/text-shaping-contract.md` §3.4):
Tokenizers' fifteen `AggressiveTokenizer` language variants,
`WordPunctTokenizer`, `TreebankWordTokenizer`, `CaseTokenizer`,
`OrthographyTokenizer` and the generic `RegexpTokenizer` engine; N-Grams'
`ngrams_str`/`bigrams_str`/`trigrams_str`, `ngrams_with_stats`/`ngram_key` and
the `zh::*` family; and Normalizers' `normalize_no`/`normalize_sv`. Their
stated reasons remain accurate about the Rust ecosystem and are kept for that
record, but none of them is something the next benchmark campaign has anything
to measure. Whitespace tokenization is a ninth case in the opposite direction:
it was never a `NO FAIR COMPETITOR FOUND` row — two real competitors were
found and benchmarked — and it belongs on this list now only because the
Verbora side of the comparison is gone (§1.1). The `case::restore_case` row is
**not** affected: that function lives in `verbora-inflectors`.

**Update, WordNet row superseded (2026-08-22 campaign).** The "Rust side,
entire module" row above no longer describes the current state: `wordnet-db`
0.1.3 (johanneswd), published after this row was written, is a real,
actively-maintained Rust competitor at comparable scope, and is now
benchmarked — see §1.15's own update block. The row stays here as the record
of what was true when it was written; it should not be read as still
current.

**19 of 19 required modules are represented in the matrix above**, each with at least one row — 18 have at least one genuine `Yes`/`Partial` competitor (usually the reference at minimum); only **Phonetic Index / Phonetic Neighbors** has zero competitors of any kind, including the reference, because it is a Verbora-native extension with no upstream equivalent to port from.

---

# 4. The reference baseline coverage confirmation

Per the spec's `NATURAL IS A REQUIRED BASELINE` section, the reference must appear wherever it implements semantically equivalent functionality. Cross-checking all 7 reports against their assigned modules:

- **Tokenizers, N-Grams, Stemmers, Normalizers, Inflectors, Phonetics, Distances, Transliteration, TF-IDF, Classifiers, Sentiment, WordNet, POS Tagging, Spellcheck, Trie, Analyzers**: the reference appears as `Yes`/required baseline everywhere real functional equivalence exists, confirmed by each report reading the actual vendored source rather than assuming from the module name (e.g. the WordNet/POS/Spellcheck/Trie/Analyzers report explicitly corrected the task's own hint that the reference's spellcheck was DoubleMetaphone-based — it verified from source that it is Norvig's algorithm).
- **Distances — plain Jaro**: correctly recorded as `No fair competitor` rather than silently matched to the reference, because the reference's Jaro computation is a private, unexported helper with no public entry point. This is a documented absence, not a missed check.
- **Language Detection**: correctly recorded `No`/`No` for the reference — the research agent verified directly (no `franc`/`langdetect`/`cld3`-style dependency, no top-level language-detection directory, nothing exported from the package root) that the reference has no general statistical language-detection module at all. This is not a gap in research; it is a confirmed, honestly-reported absence of functionality on the reference side.
- **Script Detection**: same situation — verified no script/writing-system detection module exists anywhere in the reference.
- **Phonetic Index / Phonetic Neighbors**: verified the reference's own `lib/natural/phonetics/phonetic` offers only a per-token encode-and-filter helper, no index/collection type — the reference correctly does not appear here either.

**No report skipped checking the reference for its assigned modules.** All 7 reports either (a) confirmed genuine equivalence and included the reference as the required baseline, or (b) read the actual the reference source and confirmed, with a specific citation, that no equivalent functionality exists — which is the correct outcome per the spec, not an omission.

---

# 5. Data-quality notes and open questions for a human before implementation

Small inconsistencies and unresolved items surfaced while consolidating the 7 reports. None of these block the initial matrix, but each should be resolved (or at least acknowledged) before the matrix is used to pin exact benchmark versions:

1. **the reference open-issue count disagreement.** 5 of 7 reports (Tokenizers/N-Grams, Phonetics, Distances section context, Language/Transliteration, WordNet/POS/Spellcheck/Trie/Analyzers) independently report **86 open issues** on the reference's repository. The Stemmers/Normalizers/Inflectors report reports **80 open issues** for the same repository. Every other reference figure is consistent across all reports — this is an isolated discrepancy, likely a timing artifact between when each agent's GitHub API call ran (issue counts change continuously), but worth a single confirming call before publishing a specific number on the site.
2. **`sastrawi-rs` (ibahasa) has no crates.io release.** Cannot be version-pinned per the spec's "no implicit `latest`" rule until it is published; re-check at implementation time in case that has changed. **Re-checked at implementation time (this round): it has changed** — `sastrawi-rs` now has real crates.io releases (`crates.io/api/v1/crates/sastrawi-rs`: created 2026-03-25, 7 versions, newest `0.5.3` as of 2026-07-28, 148 total downloads). This round's own scope explicitly kept it unpinned regardless ("stays unpinned per the 'no implicit latest' rule — do not attempt to pin it from GitHub directly" — a scope decision made before this specific re-check, not because no release exists); flagged here as a real, verified candidate for a **future** pinning pass to evaluate against `sastrawi` (iDevoid) properly (exact version, license, adoption signal, correctness), not acted on in this one.
3. **License-metadata mismatches worth resolving before external publication**, each independently confirmed by the reporting agent reading the actual `Cargo.toml`/`LICENSE` file rather than trusting crates.io's summary field: `unaccent` (crates.io shows "non-standard"), `stringmetrics` and `zspell` (crates.io shows "non-standard" but the repo's actual LICENSE file is standard Apache-2.0 in both cases), `segtok` (Cargo.toml declares MIT but GitHub's license detector finds no LICENSE file present), `rapidfuzz`-the-crate (README claims dual MIT/Apache-2.0 but the published `Cargo.toml license` field says MIT only — the field tooling actually reads), `sentiment` (mount-research) (crates.io says MIT, GitHub's detector reports `NOASSERTION`).
4. **`rphonetic`'s GitHub license field also mis-reports** ("Other/NOASSERTION") despite `Cargo.toml` correctly declaring Apache-2.0 — same class of issue as above, flagged once here rather than repeated per-row.
5. **`rapidfuzz` (rapidfuzz-rs) crate is stale relative to its own repository** — the published `0.5.0` crate is from 2023-12-01, but the GitHub repo has unreleased development as recent as 2024-06-29. Decide explicitly whether to pin the published `0.5.0` (reproducible, what `cargo add` actually installs) or build from a specific newer commit (more current, less reproducible) before implementation.
6. **Two brand-new, single-author, same-day-created crates** (`nltk-porter` and `nltk-lancaster`, both by VoiceLessQ, both created 2026-06-26) are currently the only Rust crates matching the correct stemmer algorithm for English-original-Porter and Lancaster respectively — real and relevant, but with essentially zero independent track record. Recommend an independent correctness pass against their self-reported test claims before trusting any of their timing numbers.
7. **Adoption-vs-engagement mismatches flagged by multiple agents independently** (worth a shared methodology note on the site rather than resolving one by one): `segtok` (452K/90d downloads, 2 GitHub stars), `fuzzt` (558K/90d downloads, 4 stars), `radix_trie` (16.7M/90d downloads, almost certainly transitive routing-table usage rather than text-completion adoption), `rapidfuzz`-the-crate (980K/90d downloads on a 2023-vintage unreleased-since crate). High download counts driven by transitive dependency graphs should not be read as direct-adoption evidence without the accompanying star/engagement context each dossier entry provides above.
8. **`harper-core`'s exact suggestion-ranking formula was not fully disclosed** in the public docs surface the WordNet/POS/Spellcheck report reviewed — recommend reading `harper-core::spell` source directly before finalizing the benchmark harness/adapter for it.

---

# 6. Summary

- **19 of 19 required modules** from the spec's `MODULE-BY-MODULE AUDIT` list are present in the matrix (tokenizers, stemmers, phonetics, distances, normalizers, ngrams, transliterators, inflectors, trie, spellcheck, sentiment, classifiers, TF-IDF, WordNet, POS tagging, language detection, script detection, phonetic neighbors/PhoneticIndex, analyzers). No additional implemented module beyond this list exists in the workspace's 19 NLP-capability crates.
- **18 of 19 modules** have at least one genuine `Yes`/`Partial` competitor (the reference alone, in the case of WordNet and Analyzers on the Rust side; the reference plus real Rust competitors everywhere else — including N-Grams as of a later pass, once `ngrammatic`'s `Ngram`/`NgramBuilder` was found to be a fair competitor for character-level n-gram generation specifically; see §1.2/§2.2). **1 of 19 modules** (Phonetic Index / Phonetic Neighbors) has zero competitors of any kind — a Verbora-native extension with no upstream equivalent anywhere, correctly resolved to an internal-only benchmark recommendation.
- The reference is present as the required cross-language baseline in every module where genuine functional equivalence was confirmed, and is correctly and explicitly absent (with a cited reason, not a silent gap) from Language Detection, Script Detection, Phonetic Index, and plain Jaro.
- **Status update (post-implementation).** The line above described this file's state at the end of the research phase, before a single benchmark had been run — it no longer describes the project's current state and is kept only as a historical marker. Every competitor selected above now has an exact `=x.y.z` version locked in `benchmarks/competitive/rust-competitors/Cargo.toml` (mirrored in `manifests/competitors.json`), and real, executed benchmark numbers exist for all 19 modules: `benchmarks/competitive/results/results.json` (205 benchmark rows) plus `results/raw/` (498 raw Criterion files) hold every Rust-vs-Rust competitive number (Distances, Tokenizers, Stemmers, Normalizers, Inflectors, Trie, Phonetics, Language/Script Detection/Transliteration, POS Tagging, Spellcheck, TF-IDF, Classifiers); this file's own §1.1–§1.19 sections carry a results write-up for every one of the 19 modules, including the three with no Rust competitor by design (N-Grams, WordNet, Analyzers) and Sentiment — no separate `docs/PERFORMANCE.md` file was ever created; and every real loss found along the way is recorded, not hidden, in `docs/PERFORMANCE_GAPS.md` (16 entries as of this round). Phonetic Index / Phonetic Neighbors remains the one module with zero competitors of any kind, exactly as this matrix already documents above — its internal-only Criterion suite (`crates/verbora-phonetics/benches/phonetic_index.rs`) predates Fase 6 (it shipped in Fase 4) and stays out of scope for a competitive comparison by design, not because it was skipped. The competitor **selection** reasoning throughout the rest of this file (candidates considered, why each was accepted/rejected, Yes/Partial/No judgments) was re-verified during implementation and stands unchanged — only this closing framing needed updating, per this round's own consolidation pass.
- **Follow-up audit correction.** The "every competitor selected above now has an exact version locked" claim two sentences up was itself found to be inaccurate by a later, dedicated fairness audit: several matrix rows marked `Yes`/`Selected cases` (§1.8 Distances: `stringmetrics`, `eddie`, `triple_accel`, `editdistancek`; other modules' own sections carry their own siblings' equivalent corrections) had never actually been pinned in `Cargo.toml` or benchmarked, with no documented reason distinguishing them from genuine, deliberate exclusions like `classifier` (jackm321, §1.13, excluded for a real compile failure) or `unaccent` (§1.4, excluded for a real license/algorithm mismatch). This round closes the Distances gaps specifically — see §1.8's own "Now actually benchmarked" note above, `docs/PERFORMANCE_GAPS.md` entries 26–29, and `site/benchmarks/competitive.md`'s Distance section — plus, separately, adds the memory/RSS dimension (`benchmarks/competitive/rust-competitors/src/memory.rs`) that was completely absent from every module's competitive suite before this round, timing having been the only dimension measured until now.
- **N-Grams competitor added (later pass).** The "three with no Rust competitor by design" named two paragraphs up (N-Grams, WordNet, Analyzers) is now two: re-examining `ngrammatic` 0.7.0 found its `Ngram`/`NgramBuilder` — the character n-gram + frequency-count generator its `Corpus` fuzzy-search feature is itself built on — to be a fair, comparable primitive against Verbora's generic `ngrams()` engine at char granularity. Its headline `Corpus`/`search` fuzzy-matching feature has no Verbora equivalent and remains unbenchmarked. See §1.2/§2.2 for the matrix rows and `docs/PERFORMANCE_GAPS.md` entry 38 for the measured numbers (bigrams: Verbora wins all 3 runs, ~1.07×–1.16× faster; trigrams: Verbora loses all 3 runs, ngrammatic ~1.03×–1.08× faster). WordNet and Analyzers remain the only two modules with no Rust competitor by design.
- **Text-shaping migration (2026-08) — the tokenizer, n-gram and normalizer
  numbers claimed two paragraphs up no longer describe shipped code.**
  `verbora-tokenizers`, `verbora-normalizers` and `verbora-ngrams` were
  rewritten to `docs/design/text-shaping-contract.md`: some of the
  capabilities those `results.json` rows measured were deleted outright, and
  the rest were reimplemented. §7 below lists, capability by capability, which
  figures are retired with nothing left to re-measure and which are pending a
  re-measurement the next campaign must schedule.

---

# 7. Text-shaping migration — what the next benchmark campaign must answer

**No figure in the lists below was re-measured, and none may be estimated.**
The distinction between the two lists is load-bearing: a *retired* entry has
no question left to ask, because the code it measured no longer exists and
nothing replaced it; a *pending* entry has a live question and a benchmark
group waiting to answer it.

`verbora-tokenizers`, `verbora-normalizers` and `verbora-ngrams` were
rewritten to `docs/design/text-shaping-contract.md`;
§1.1, §1.2 and §1.4 carry the per-row reasoning; this section is the
consolidated ask. "Entry" numbers throughout are `docs/PERFORMANCE_GAPS.md`
entries, which carry the same retirements from the other direction.

**Retired — nothing to re-measure, the Verbora capability is gone:**

| What it measured | Where it lived | Recorded result now withdrawn |
|---|---|---|
| Whitespace tokenization vs. `tantivy::WhitespaceTokenizer` and Hugging Face `WhitespaceSplit` | `benches/tokenizers.rs`' deleted `whitespace_tokenization` group; entry 3 | 3.6×–4.6× loss, then the 1.11×/1.70×/2.36×/1.97× reversal; 18.8×–33.1× HF win |
| `AggressiveTokenizer` (en) vs. `unicode_words()` | deleted `aggressive_tokenization_en` group | parity at every size, under ~1.3× either way |
| `hiragana_to_katakana`/`katakana_to_hiragana` vs. `unicode-jp` | deleted `ja_hiragana_to_katakana`/`ja_katakana_to_hiragana` groups; entry 30 | 3.7×–4.8× loss both directions; the later 4.1%-at-1024 pre-check speedup |
| `ngrams_str` vs. the reference | `crates/verbora-ngrams`' own `string_input` group; entry 5 | ~2.15×–2.31× loss; the 2.2×–2.5× pre-tokenized win beside it |
| `normalize_ja` vs. the reference | `crates/verbora-normalizers`' own `normalize_ja/mixed` group; entry 11 | 53.1× / 4.0× wins declining to 0.9× at 24576 B |

**Retired *and* reclassified — the group survives, but not as a competitive
comparison:** the `unicode-segmentation` rows moved out of
`word_tokenization`/`sentence_tokenization` into
`word_tokenization_wrapper_overhead`/`sentence_tokenization_wrapper_overhead`,
because `WordTokenizer::tokens` *is* `str::unicode_words()` and
`SentenceTokenizer` is built directly on `split_sentence_bound_indices()`.
Their future numbers state wrapper cost over a dependency and must never be
reported as Verbora beating or losing to `unicode-segmentation` —
`docs/PERFORMANCE_GAPS.md` entry 23 is the entry this reclassification
voids.

**Pending re-measurement — the comparison is still genuine, only Verbora's
side changed:**

| Comparison | Group | Entry |
|---|---|---|
| `WordTokenizer` vs. `tantivy::SimpleTokenizer` / HF `Whitespace` | `word_tokenization` | 4 |
| `SentenceTokenizer` vs. `segtok` | `sentence_tokenization`, `sentence_tokenization_boundary_density` | — (a Verbora win, never filed as a gap; entry 23 is the *`unicode-segmentation`* pairing and is retired, not pending) |
| padded character n-grams vs. `ngrammatic` | `bigrams`, `trigrams` | 38 |
| `remove_diacritics` vs. `diacritics` 0.2.2 | `remove_diacritics_ascii`, `remove_diacritics_accented` | — |
| `nfkc` vs. `kana-converter` `KanaOnly` | `nfkc_halfwidth_katakana` (was `ja_katakana_halfwidth_to_fullwidth`) | 31 |
| the whole `verbora-distance` group | `benches/distance.rs` | 1, 26–29 |

**Not text-shaping — one further pending item, from the same discipline.**
§1.3's per-language stemmer ratios were measured against a linear suffix scan
that no longer exists. `crates/verbora-stemmers/src/among.rs` replaced it with
the Snowball runtime's own `find_among`/`find_among_b` binary search — the
algorithm `docs/PERFORMANCE_GAPS.md` entry 34 recorded as *not* reimplemented —
and ten of the twelve benchmarked groups route through it:

| Comparison | Group | Entry |
|---|---|---|
| nine Snowball languages vs. `rust-stemmers` and `snowball_stemmers_rs` | `porter_de`, `porter_es`, `porter_fr`, `porter_it`, `porter_nl`, `porter_no`, `porter_pt`, `porter_ru`, `porter_sv` | 34 |
| English original-Porter vs. `porter-stemmer` and `nltk-porter` | `porter_en` | 24 |

`stemmer_id` (`sastrawi`) and `stemmer_ja` (`lindera`) do **not** reach
`among.rs` and are not retired on this ground; they remain covered by the
"named, not resolved" line for `verbora-stemmers` below. §1.3 carries the
per-row reasoning.

**Downstream reach.** Five crates tokenize *inside* a measured region and
therefore now run a different tokenizer there — `verbora-tfidf`,
`verbora-classifiers`, `verbora-stemmers`, `verbora-phonetics` and
`verbora-sentiment` — because each depends on
`verbora_tokenizers::WordTokenizer` directly. Only one of the five was traced
row by row in this pass:

- **`verbora-tfidf` — traced.** `docs/PERFORMANCE_GAPS.md` entries 13 and 14
  now carry row-level retirements: the whole `build/verbora/<n>` sweep and
  `query/tfidf_cold_cache`/`query/tfidfs_64_documents` run the tokenizer and
  are pending re-measurement; `idf_cold/deserialized` and
  `documents/add_document_raw` do not reach it and are unaffected *by this
  change*.
- **`verbora-classifiers` (entries 15, 19–22), `verbora-stemmers`
  (`tokenize_and_stem`, entry 10), `verbora-phonetics`, `verbora-sentiment` —
  named, not resolved.** Whether each affected figure is retired or merely
  pending depends on changes inside those crates that this pass did not
  verify, and asserting either way without verifying would be a guess. They
  are listed so the campaign can settle them, not marked.

`docs/design/text-shaping-contract.md` §7 items 4 and 5 carry the two
structural questions underneath all five: whether `verbora-tfidf`'s SWAR fast
path survives a UAX #29-correct ASCII rule at all (its bitmap encoded
`[a-z0-9_]`, and the rule needs `MidLetter`/`MidNum`/`MidNumLet` lookahead a
single bitmask cannot express), and the workspace-wide `Cow` and borrow
footprint now that the `Utf16Token`/`Cow`/`String` token shapes are gone.

**One item on this list is not a measurement question at all:** `eddie`
0.4.2's unsoundness (§1.8). No campaign can produce a trustworthy number
from it, because the only build that completes is the one with the UB checks
compiled out. It is therefore **not on either list**: its rows are retired with
nothing to re-measure, `eddie` is kept as a correctness oracle through its
sound slice API only, and no timing row exists or may be added. The campaign's
one obligation here is negative — do not collect `eddie` timings, and do not
restore the ten rows removed from `results/results.json`,
`results/raw/distance-*-eddie-*.json` and `results/distance-memory.json`.

**A second item the campaign should not have to rediscover:** §1.8's `jaro` and
`jaro_winkler` timing rows against `strsim` and `rapidfuzz` are legitimate
*timings* but not an *equivalence* — both crates truncate the
half-transposition count and gate the Winkler boost behind `sim > 0.7`, so the
verdicts are `Partial`. Re-measure them; do not re-mark them `Yes`.

