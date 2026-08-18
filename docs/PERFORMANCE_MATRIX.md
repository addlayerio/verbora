# Performance matrix

Per-crate status against the Fase 2 performance audit's checklist. `N/A` is
valid when justified — not every algorithm benefits from every technique, and
this project's own rule is that a claim needs evidence, not that every cell
must be checked.

**Do not hand-wave this file.** Every ✅ below traces to a real command run
during the Fase 2 audit (two orchestrated workflows: a per-module
audit-and-fix pass, then a Rayon rollout pass), not to intuition. See
`docs/PERFORMANCE.md` for the cross-language benchmark methodology and
`AGENTS.md`'s `# Rayon Policy` / `# Data Structures` / `# Build → Freeze →
Query` / `# Archived Data and Memory Mapping` sections for the permanent
rules this audit established.

Legend: ✅ reviewed and either already correct or fixed and verified · ⚠️
reviewed, a real opportunity exists but was deliberately deferred (see
notes) · ➖ not applicable, with reason.

| Crate | Lazy API | Zero-copy | Reusable memory | Batch | Parallel | Alloc reviewed | Data structures reviewed | mmap/rkyv reviewed | Benchmarked | Parity |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| `verbora-tokenizers` | ✅ | ✅ | ✅ | ✅ | ✅ `par_tokenize_batch` | ✅ 2 fixes applied | ✅ | ➖ compiled-in static tables only, no file-backed dataset | ✅ | ✅ |
| `verbora-ngrams` | ✅ | ✅ | ➖ output is inherently owned tuples/stats | ✅ `ngrams()`/`ngrams_owned()` | ⚠️ not separately evaluated for Rayon this pass | ✅ 1 change evaluated and rejected (regressed the realistic benchmark) | ✅ | ➖ | ✅ | ✅ |
| `verbora-normalizers` | ✅ | ✅ (`Cow` throughout) | ➖ | ✅ | ✅ `par_remove_diacritics_batch` | ✅ no changes needed — already maximally allocation-conscious | ✅ | ➖ compiled-in tables | ✅ | ✅ |
| `verbora-stemmers` | — (transform, not a sequence) | ⚠️ per-language, most already `Cow`-based; a `Vec<u16>` snapshot-clone opportunity in es/it/fr/pt.rs deferred (touches parity-critical control flow in 5 languages, cheap in absolute terms) | ✅ | ✅ `TokenizeAndStem` | ✅ `par_tokenize_and_stem_batch`, **per-document** not per-word (per-word cost measured at 26-628 ns, near Rayon's own dispatch overhead) | ✅ removed an unused `regex` dependency | ✅ | ➖ | ✅ | ✅ |
| `verbora-phonetics` | — | ⚠️ | ➖ | ✅ | ✅ `par_encode_batch`/`par_encode_double_batch`, **chunked** (`par_chunks`, not naive per-word, same dispatch-overhead reasoning as stemmers) | ✅ removed an unused `regex` dependency | ✅ | ➖ | ✅ | ✅ |
| `verbora-distance` | — | — (numeric result) | ✅ two-row DP, stack buffers (pre-existing, this crate is the project's own calibration example); plain Levenshtein additionally dispatches to a safe-Rust Myers bit-vector fast path for unit-cost, 8–64-unit inputs — see `docs/PERFORMANCE_GAPS.md` entry 26's update and `levenshtein.rs`'s own `plain_levenshtein` doc comment | ✅ | ✅ `par_*_batch` per metric | ✅ | ✅ | ➖ | ✅ 26 cross-language benchmarks, median 8.0× (competitive-benchmark numbers, incl. the `triple_accel`/`fast_radix_trie`/`fst`/`fast_symspell`/`snowball_stemmers_rs` round, in `docs/COMPETITIVE_BENCHMARKS.md` and `docs/PERFORMANCE_GAPS.md`) | ✅ |
| `verbora-trie` | ✅ `iter_keys_with_prefix` etc. | ✅ | ➖ | ✅ | ➖ evaluated and rejected — query cost ~67 ns, at or below typical Rayon dispatch overhead; construction is inherently sequential against one shared arena | ✅ | ✅ flat arena vs. `HashMap<u16,Box<Node>>`, own bench file documents the decision | ⚠️ flagged only — a caller shipping a very large fixed dictionary could benefit from `rkyv`+`mmap`, but this crate ships no bundled data itself | ✅ | ✅ |
| `verbora-inflectors` | — | ✅ | ✅ `pluralize_into` | ✅ | ➖ evaluated and rejected — ~360 ns/word, comparable to dispatch overhead; only relevant as one stage inside a larger parallel pipeline | ✅ | ✅ | ➖ | ✅ | ✅ |
| `verbora-wordnet` | ✅ `lookup_iter` | ✅ `DataRecordRef` borrows, `Cow`-based helpers | ✅ | ✅ | ✅ `par_lookup_batch` (already `Send+Sync`, zero synchronization added) | ✅ 2 fixes applied (`Indexed` line-start table -50–65%, `IndexFile` probe loop -20–31%) | ✅ no `HashMap` anywhere — sorted `Vec`+`partition_point`, `match` for the pointer-symbol table | ✅ **evaluated and rejected** — see `AGENTS.md`'s `# Archived Data and Memory Mapping`; `Pread`/`Resident`/`Indexed`/`PrebuiltIndex` already deliver mmap's benefits, verified with real numbers | ✅ | ✅ |
| `verbora-sentiment` | — (fold over `contributions`) | ✅ `Cow`-based lowercasing | ➖ | ✅ | ✅ `par_get_sentiment_batch` | ✅ | ✅ | ✅ evaluated — 1.2 MB embedded blobs decoded once behind `OnceLock`, already the right shape for this dataset size (two orders of magnitude smaller than WordNet's) | ✅ | ✅ |
| `verbora-spellcheck` | — | ✅ | ➖ | ✅ | ✅ `par_get_corrections_batch` — **strongest candidate found**, ms-scale per call | ✅ | ✅ | ➖ dictionary is caller-supplied at runtime, no bundled dataset | ✅ | ✅ |
| `verbora-tagger` | — | ✅ zero-parse `include_bytes!` lexicon | ➖ | ✅ | ✅ `par_tag_batch` | ⚠️ one `TransformationRule::apply` clone deferred (would need a lifetime-parameter refactor through the PEG parser; cheap in absolute terms) | ✅ | ✅ already the **reference example** for Build→Freeze→Query — `build.rs` measured and rejected a runtime-parse alternative (+4.6 MB/+55 ms vs. ~0 ms chosen) | ✅ | ✅ |
| `verbora-tfidf` | ✅ | ✅ term interning, borrowed `RawDocument` index | ✅ incremental idf cache | ✅ | ✅ `par_add_documents_batch` — narrower scope than its siblings (`DocumentInput::Text` + `restore_cache: false` only), see its own doc comment for why a naive per-document or map-reduce design was rejected | ✅ | ✅ `Interner`, `FxHashMap` | ⚠️ flagged — `to_json`/`from_json` restore cost (33 ms for a 3.7 MB/64-doc corpus) is real, but a second on-disk format wasn't attempted this pass | ✅ | ✅ |
| `verbora-classifiers` | — | ✅ | ➖ | ✅ | ✅ `par_classify_batch` on `Classifier<E>`, after fixing the `Rc<dyn Stemmer>`→`Arc<dyn Stemmer + Send + Sync>` prerequisite; `MaxEntClassifier` correctly excluded (`Rc<RefCell<_>>` state is load-bearing) | ✅ `FxHashMap`/`FxHashSet` swapped into `JsMap`/`features_for`; a `js_order()` caching opportunity deferred (more invasive, needs invalidation-on-mutate design) | ✅ | ➖ | ✅ | ✅ |
| `verbora-analyzers` | — | ✅ | ➖ | ✅ | ✅ `par_analyze_batch` (composes existing per-sentence calls, no single wrappable primitive existed) | ✅ | ✅ | ➖ | ✅ | ✅ |
| `verbora-transliterators` | ✅ | ✅ | ✅ `transliterate_into` | ✅ | ✅ `par_transliterate_batch` | ✅ | ✅ | ➖ | ✅ | ✅ |
| `verbora-util` | — | ✅ `Rc<str>` sharing in `JsSparse` | ➖ | ➖ single shared graph per call, no independent-item batch shape exists | ➖ evaluated and rejected for the reason above | ✅ `FxHashMap` swapped into `JsSparse::named_pos` | ✅ dense `Vec` + `BTreeMap` spill, documented in `sparse.rs` | ⚠️ flagged only, hypothetical (large fixed dictionaries), not this crate's own use case | ✅ | ✅ |

## Verbora-native extensions (not a parity crate)

The table above tracks the 17 reference-verified crates.
`verbora-phonetics`'s `PhoneticIndex` is not one of them — it is a
Verbora-native index with no reference counterpart (candidate generation over
a phonetic-encoded dictionary), reviewed on the same discipline as the table
above minus the parity-specific columns. See `AGENTS.md`'s
`# Verbora-Native Extensions` for the permanent policy this establishes, and
`site/features/phonetic-index.md` for the full write-up.

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ✅ | `neighbors()` returns a lazy iterator; `.take(n)` never materialises the rest |
| Zero-copy | ✅ query path / ⚠️ encode | bucket lookup and iteration allocate nothing; `encode()` still allocates one `String` per query — evaluated, deliberately deferred (see the crate's own module doc comment) |
| Reusable memory | ➖ | build-once/freeze/query shape; not an applicable dimension |
| Batch | ➖ | `PhoneticIndexBuilder::extend` already amortises the common "insert a whole dictionary" case |
| Parallel | ➖ evaluated only informally | build cost is dominated by `encode()` and an `O(n log n)` sort; not separately benchmarked for Rayon this pass |
| Alloc reviewed | ✅ | one `String` per `neighbors()` call (query encoding only), zero elsewhere on the query path |
| Data structures reviewed | ✅ | compressed-sparse-row (`InlineCode` codes + offset table) benchmarked head-to-head against a `String`-keyed `HashMap`, a frozen `HashMap<Code, Box<[EntryId]>>`, and a dense perfect-hash array |
| mmap/rkyv reviewed | ➖ | no persistence implemented yet at all (no `serde` dependency); see the feature's own "Persistence" section for what was checked without building it |
| Benchmarked | ✅ | `benches/phonetic_index.rs` — build, query (hit/miss/wide-bucket), encode-only, and the four-design comparison above, at 1K/10K/100K entries |
| Parity | N/A | not a parity crate; no reference behaviour exists to verify this against |

Real numbers, one development machine, `cargo bench -p verbora-phonetics
--bench phonetic_index` (see that file's own module doc comment for full
methodology; treat exact figures as machine-dependent, orders of magnitude as
the reproducible part):

**Query latency**, `neighbors()` fully drained via `.count()`:

| Entries | Scenario | SoundEx | DoubleMetaphone |
|---:|---|---:|---:|
| 1,000 | hit (1 match) | 106 ns | 162 ns |
| 100,000 | hit (1 match) | 138 ns | 218 ns |
| 100,000 | miss (0 matches) | 116 ns | 189 ns |
| 100,000 | wide bucket (1,000 matches) | 1.63 µs | 2.37 µs |

Encoding the query alone costs 40–58 ns (SoundEx) / 49–81 ns (DoubleMetaphone),
independent of dictionary size — the dominant cost for a hit or a miss at
every size tested.

**Memory at 100,000 SoundEx entries**, four bucket-storage designs:

| Design | Bytes/entry | Relative to shipped |
|---|---:|---:|
| `InlineCode` + CSR (shipped) | 29.00 | 1.00× |
| Frozen `HashMap<Code, Box<[EntryId]>>` | 31.05 | 1.07× |
| Dense perfect-hash array | 32.27 | 1.11× |
| `String`-keyed `HashMap` | 39.79 | 1.37× |

The shipped design is the most memory-compact of the four, and is *not* the
fastest on raw query latency at the same scale — roughly 2× slower than the
hash-based alternatives for a hit or a miss (binary search vs. a hash probe),
and further behind on a wide bucket, though that specific gap partly compares
draining a general two-bucket merge-and-dedup iterator (what `DoubleMetaphone`
needs) against an `O(1)` `.len()` call on a raw slice (what the throwaway
single-code-only alternatives return) — not a like-for-like cost. Reported in
full, unfavourable numbers included, on the feature's own site page.

### verbora-language (language/script detection, phonetic-strategy recommendation)

Also not one of the 17 parity crates — `verbora-language` (Fase 5) builds on
`verbora-phonetics`'s encoders (a prior Verbora-native extension, above) but
answers a question that module cannot: *given a word or document, which
encoder should even be used?* It has no reference counterpart. Reviewed on
the same discipline as `PhoneticIndex` above, against the aspects Fase 5's
own spec calls out by name rather than the parity table's column set (which
does not fit a crate with two very different cost shapes: a statistical
detector, and a closed lookup table). See `AGENTS.md`'s
`# Verbora-Native Extensions` for the permanent policy and
`crates/verbora-language/src/lib.rs`'s own module doc comment for the
three-layer design (script detection → language detection → phonetic
strategy) this section follows.

**Language detection** (`WhatlangDetector`, behind the `language-detection` feature):

| Aspect | Status | Notes |
|---|---|---|
| Model size | ✅ | `whatlang` 0.18's frequency tables are five compile-time `pub static … : LangProfileList` arrays (`src/trigrams/profiles.rs`, 15,583 lines) — baked into the binary's rodata like any other Rust `static`, not a runtime-loaded or deserialized model; no file I/O anywhere in the path |
| Lazy init | ➖ evaluated, not needed | `WhatlangDetector::new()` measured **0 heap allocations, 0 bytes**, deterministic across repeated runs (external counting-allocator probe); the type is zero-sized and stateless, so there is no loading step to guard. `whatlang`'s one internal runtime structure (`ALPHABET_LANG_MAP`) is already behind a `LazyLock` inside `whatlang` itself, one layer down — a second `OnceLock`/`LazyLock` here would add an atomic check to every `detect()` call to guard a cache holding nothing |
| Allocations | ✅ reviewed | `whatlang::Detector::detect()` costs a constant **25 heap allocations/reallocations**, identical from word (6 B) through long_document (10,460 B) — only byte totals scale with input (8,883 → 103,017 bytes), not allocation count. `WhatlangDetector::detect` adds at most one more (a single-element `Vec` for `candidates`): 26 total for a match, 25 for `LanguageDetection::none()` |
| Batch | ✅ `par_detect_batch` | generic over any `D: LanguageDetector + Sync`; body is `texts.par_iter().map(\|t\| detector.detect(t))` — cannot drift from sequential detection behaviour by construction |
| Parallel evaluated | ✅ | benchmarked at batch sizes 16/64/256/1,024/4,096 (`SHORT_TEXT`-sized items, 32 cores): **~5.3×** at 16 up to **~13–14×** at 4,096, parallel ahead at *every* size tested — no sequential-favoring crossover in this range, because one `detect()` call already costs tens of µs (see Allocations row), leaving Rayon's fork-join overhead comparatively negligible |
| Thread-safe | ✅ | `WhatlangDetector` is `Copy`, zero-sized, automatically `Send + Sync` (no `unsafe impl` needed, and `unsafe_code = "deny"` at the workspace level would reject one); `par_detect_batch` shares one detector instance, read-only, across every thread |
| Benchmarked | ✅ | `crates/verbora-language/benches/language.rs` — `script_detection`, `language_detection`, `strategy_lookup`, `manual_path`, `auto_end_to_end`, `par_batch` Criterion groups |

**Phonetic strategy** (`recommend()`, no feature required):

| Aspect | Status | Notes |
|---|---|---|
| Lookup complexity | ✅ O(1) / branch table | `recommend()` is a closed `match` over all 22 `Language::ALL` variants, nothing statistical — measured 5.67–7.48 ns (no-features run) / 6.33–6.42 ns (all-features run) for `recommend(Language::German)` alone. The explicit manual-vs-auto comparison in the same report: the manual path (~6.4 ns) costs **~4,260×–22,400× less** than the full auto (`detect` + `recommend`) path across short-text through long-document lengths — "the explicit-language path is cheap" is a measured fact here, not an assumption |
| Allocation behavior | ✅ reviewed | `PhoneticStrategy::alternatives` is a `Vec<PhoneticRecommendation>`. The three languages with no confident primary (`Persian`, `Hindi`, `Chinese`) return an empty `Vec` — `Vec::new()` does not allocate — and measured fastest, ~1.4–1.6 ns per `recommend()` call. Every other language's match arm builds a short (1–2 item) `alternatives` `Vec` and measured ~5.6–8.3 ns; the gap is attributable to that one small heap allocation. All 22 languages back-to-back: 207.16–207.52 ns (~9.4 ns/language average) |
| Static/dynamic dispatch reviewed | ✅ | `recommend(language: Language)` takes `Language` by value (`Copy`) and matches on it directly — no trait objects, no vtable, no dynamic dispatch anywhere in the call. `PhoneticRecommendation` is a plain enum by design, not `Box<dyn PhoneticEncoder>`: per `strategy.rs`'s own doc comment, this lets a caller match on *which* encoder is recommended without being forced to construct one, and keeps this module from needing to be generic over `verbora_phonetics::PhoneticEncoder` just to name a choice. The same static-dispatch discipline holds crate-wide: `AutoPhoneticStrategy<D>` and `par_detect_batch<D>` are both generic over `D: LanguageDetector`, never `Box<dyn LanguageDetector>` |
| Benchmarked | ✅ | same `benches/language.rs` — `strategy_lookup` (every `Language::ALL` variant individually, plus one `all_languages` pass) and `manual_path` (`recommend(Language::German)` alone, the manual-path baseline) groups |

Real numbers, one development machine, `cargo bench -p verbora-language
--all-features` (see the bench file's own module doc comment for full
methodology, including the external counting-allocator probe used for the
allocation figures above — written as a separate, non-workspace project
because this workspace's `unsafe_code = "deny"` forbids writing a
`GlobalAlloc` probe inside the crate itself, the same constraint
`verbora-phonetics/benches/phonetic_index.rs` already documents hitting;
treat exact figures as machine-dependent, orders of magnitude as the
reproducible part):

| Group | Scenario | Time |
|---|---|---:|
| A. `script_detection` | word (6 B) | 8.61–8.71 ns |
| A. `script_detection` | short_text (68 B) | 64.74–66.89 ns |
| A. `script_detection` | paragraph (523 B) | 527.09–530.50 ns |
| A. `script_detection` | long_document (10,460 B) | 8.80–8.88 µs |
| B. `language_detection` | short_text | 27.972–28.003 µs |
| B. `language_detection` | paragraph | 67.900–68.026 µs |
| B. `language_detection` | long_document | 147.77–148.03 µs |
| E. `manual_path` | `recommend(German)` | 5.67–7.48 ns |
| D. `auto_end_to_end` | auto short_text | 28.09–28.12 µs |
| D. `auto_end_to_end` | auto paragraph | 67.73–67.86 µs |
| D. `auto_end_to_end` | auto long_document | 147.76–147.98 µs |

**`par_batch`** (`--features language-detection,parallel`, `SHORT_TEXT`-sized items, 32 cores):

| Batch size | Sequential | Parallel | Speedup |
|---:|---:|---:|---:|
| 16 | 461.4–482.7 µs | 84.98–91.44 µs | ~5.3× |
| 64 | 1.862–2.057 ms | 223.2–334.5 µs | ~6–7× |
| 256 | 7.270–7.294 ms | 735.7–960.9 µs | ~8–9× |
| 1,024 | 29.15–29.37 ms | 2.643–3.510 ms | ~9–10× |
| 4,096 | 120.7–123.3 ms | 8.255–8.893 ms | ~13–14× |

Parallel wins at every tested batch size here, including the smallest (16)
— unlike some smaller-per-item primitives elsewhere in this workspace, a
single `detect()` call (tens of µs, see the `language_detection` row above)
is already expensive enough that Rayon's fork-join overhead never
dominates in this range.

### verbora-phonetics: Beider-Morse Phonetic Matching

The third Verbora-native extension, and `verbora-phonetics`'s second (after
`PhoneticIndex` above) — no reference counterpart exists (the reference has no
Beider-Morse implementation). Solves a problem none of the crate's other four
encoders do: the *same* historical family name has different
phonetically-equivalent spellings depending on which country's orthographic
conventions transcribed it. See `crates/verbora-phonetics/src/beider_morse/mod.rs`'s
own doc comment for the full design (provenance/licensing of the embedded
rule corpus, why the output type is not `PhoneticCodes`) and `AGENTS.md`'s
`# Verbora-Native Extensions` for the policy this section follows.

Correctness here has no reference oracle to differentially test against
(unlike the 105 ported APIs elsewhere in this workspace), so it was instead
verified during development against a live, disposable build of the
`rphonetic` crate (a mature, independently-verified Rust port of the same
Apache Commons Codec algorithm) reading the identical rule-file corpus — not
a dependency of this crate, only a development-time cross-check. That swept
106 Generic surnames (96.2% exact match — the four misses cluster around one
still-open word-final-consonant edge case, see the module's own doc comment),
10 Ashkenazi and 10 Sephardic surnames (100%), `RuleType::Exact` on 12 names
(100%), 16 explicit single-language calls spanning most of Generic's language
list (100%), and 5 prefix/multi-word names (`d'Angelo`, `van Gogh`, `de la
Cruz`, `Jean Paul`, `von Neumann` — 4/5, the one miss being the same open
edge case). Two real bugs were found and fixed by this process before it
reached those numbers: a compound language tag (`gv[portuguese+spanish]`)
being resolved as one unmatchable name and silently dropped (affected ~94
rules across the corpus), and the Rules pass wrongly passing an unmatched
character through literally instead of skipping it (only observable for
characters no rule covers at all, such as the space `concat` mode fuses
between words).

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ➖ | per-word `encode()`/`encode_language()`, no streaming/lazy shape applicable — output is a bounded (≤ `max_phonemes`, default 20) candidate list, not a large sequence worth deferring |
| Zero-copy | ➖ | rule application necessarily builds new candidate strings (cross-product branching at every rule match); no realistic zero-copy shape for this algorithm |
| Reusable memory | ➖ not attempted this pass | each `encode()` call allocates its own candidate `String`s; a caller encoding millions of names in a hot loop could benefit from a reusable-buffer API, not built speculatively ahead of a real need |
| Batch | ➖ not built this pass | no `par_encode_batch`-style helper yet; composes fine with a plain `.iter().map(bm.encode)` today |
| Parallel | ➖ not evaluated this pass | rule-table compilation is cached process-wide behind `RwLock`-guarded `HashMap`s (`NameTypeData::table`) and would need a read-heavy-workload check before adding a Rayon helper; deferred rather than added speculatively |
| Alloc reviewed | ⚠️ real gap found and fixed, not profiled with a counting allocator | an independent audit found `PhonemeBuilder::apply`'s per-candidate string rebuild compounding through `combine_prefix_split`'s recursion into an unbounded, algorithmic-complexity-DoS-shaped cost — a repeated Generic name prefix cost 14+ seconds at ~3,000 characters, reachable from the fully public API with no length guard. Fixed with a 512-char overall cap and a 128-char prefix-splitting cap in `encode_top` (`mod.rs`), both verified to collapse that case to single-digit milliseconds with zero effect on any real name; see `AGENTS.md`'s Beider-Morse section for the full finding |
| Data structures reviewed | ✅ | rules bucketed by pattern's first character (`HashMap<char, Vec<Rule>>`) for O(1) dispatch to the small in-bucket linear scan the "first match in file order wins" semantics require; language sets are a `u32` bitset (`LanguageSet`), not a `HashSet<String>` |
| mmap/rkyv reviewed | ➖ | the 127 rule files (a few hundred KB total) are `include_str!`-embedded at compile time, the same pattern this workspace's other compiled-in-table crates use; far below the scale `# Archived Data and Memory Mapping` targets |
| Benchmarked | ✅ | `crates/verbora-phonetics/benches/beider_morse.rs` — `name_types`, `rule_types`, `guess_vs_explicit`, `concat` Criterion groups |
| Parity | N/A | not a parity crate; verified against a disposable oracle build instead, see above |

Real numbers, one development machine, `cargo bench -p verbora-phonetics
--bench beider_morse` (16-surname batches; treat exact figures as
machine-dependent, orders of magnitude as the reproducible part):

| Group | Scenario | Time | Throughput |
|---|---|---:|---:|
| `name_types` | Generic | 102.39 µs | 156.27 Kelem/s |
| `name_types` | Ashkenazi | 202.69 µs | 78.94 Kelem/s |
| `name_types` | Sephardic | 47.04 µs | 340.17 Kelem/s |
| `rule_types` | Approx | 104.49 µs | 153.13 Kelem/s |
| `rule_types` | Exact | 70.94 µs | 225.56 Kelem/s |
| `guess_vs_explicit` | guess (`encode`) | 103.61 µs | 154.42 Kelem/s |
| `guess_vs_explicit` | explicit (`encode_language`) | 47.70 µs | 335.46 Kelem/s |
| `concat` | fused (default) | 104.14 µs | 38.41 Kelem/s |
| `concat` | split (hyphen-joined per word) | 86.05 µs | 46.49 Kelem/s |

The `name_types` row is the one genuinely surprising number here, reported in
full rather than smoothed over: Ashkenazi measured **slower** than Generic
despite having 8 fewer languages to consider, and Sephardic (5 languages)
measured fastest. The 16-surname list used is Romance/Slavic/Greek-biased
(picked for Generic); under Ashkenazi's narrower 10-language pool most of
those names guess ambiguously rather than to one confident language, falling
back to the wider `"any"` rule file and a larger candidate set than Generic's
own mostly-singleton guesses produce. The real cost driver is confirmed to be
guess confidence and resulting candidate-set size, not raw language count —
see `benches/beider_morse.rs`'s own doc comment for the full reasoning.
`guess_vs_explicit` isolates the auto-detection layer's own cost: guessing
first (`encode`) costs roughly 2.2× `encode_language`'s already-known-language
path on this list.

### verbora-spellcheck: FuzzyIndex (edit-distance candidate index)

The fourth Verbora-native extension — no reference counterpart (nothing in
this crate's own Norvig-style port needed a pre-built index; see
`crates/verbora-spellcheck/src/fuzzy_index.rs`'s own doc comment for the
full design and why it's a BK-tree rather than a SymSpell-style deletion
index). Reviewed on the same discipline as the extensions above.

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ✅ | `neighbors()` returns a lazy iterator (matches `PhoneticIndex::neighbors`'s own laziness); `.take(n)` never visits more of the tree than it has to |
| Zero-copy | ⚠️ one allocation per query | the query string is copied into the iterator (`query.to_owned()`) so `Neighbors<'a>` only needs one lifetime, tied to the index rather than the caller's own borrow of the query — matches `PhoneticIndex::neighbors`'s own accepted one-`String`-per-query cost |
| Reusable memory | ➖ | build-once/freeze/query shape; not an applicable dimension |
| Batch | ➖ not built this pass | composes fine with `.iter().map(index.neighbors)` today; no dedicated batch helper yet |
| Parallel | ➖ not evaluated this pass | queries are read-only against a shared, immutable `FuzzyIndex` and would parallelize trivially (`par_iter().map(...)`) if a real batch workload needs it; not added speculatively |
| Alloc reviewed | ✅ by inspection | one `Box<str>` per indexed word (build time), one `String` per query (see Zero-copy row), one `u32` stack entry per subtree actually visited during a query — no other allocation on the query path |
| Data structures reviewed | ✅ | BK-tree in a flat `Vec<Node>` arena, children referenced by index rather than `Box<Node>` — the same "flat arena over recursive `Box`-links" choice `verbora-trie` made, for cache-friendliness and to avoid a recursive `Drop` on a deep tree |
| mmap/rkyv reviewed | ➖ | no bundled dataset; the index is built at runtime over a caller-supplied word list, not compiled-in data |
| Benchmarked | ✅ | `crates/verbora-spellcheck/benches/fuzzy_index.rs` — construction and query-vs-brute-force, at 100/1,000/10,000/20,000 words |
| Parity | N/A | not a parity crate; correctness verified directly against a brute-force Levenshtein scan (below), not against a reference implementation |

**Correctness**, `crates/verbora-spellcheck/tests/fuzzy_index.rs`: a
BK-tree's pruning is a performance optimization, not a filter, so its
defining correctness property is returning *exactly* the same match set a
brute-force scan would. Verified directly — not just spot-checked — over
3,000 distinct real words from the shared corpus, ~62 queries (in-dictionary
words, one-character perturbations of them, and words definitely absent)
crossed with `max_distance` 0–3 (248 full result-set comparisons total),
plus dedicated tests for the empty index and duplicate-insert collapsing.

Real numbers, one development machine, `cargo bench -p verbora-spellcheck
--bench fuzzy_index` (200 queries/batch, `max_distance` 2; treat exact
figures as machine-dependent, the *trend* as the reproducible part):

| Words | Construction | Query: tree | Query: brute force | Speedup |
|---:|---:|---:|---:|---:|
| 100 | 32.17 µs | 646.08 µs | 1.5275 ms | ~2.4× |
| 1,000 | 852.26 µs | 11.436 ms | 32.155 ms | ~2.8× |
| 10,000 | 12.442 ms | 97.476 ms | 321.86 ms | ~3.3× |
| 20,000 | 27.254 ms | 191.47 ms | 685.96 ms | ~3.6× |

The tree's advantage *grows* with dictionary size, not just its raw speed —
the query column's ratio to brute force widens at every step tested, which
is the shape that justifies pre-indexing over a per-query scan in the first
place (a design that started faster but converged toward parity at scale
would undercut the whole rationale for building this). Construction cost is
paid once per index and is the dominant one-time cost; a caller rebuilding
the index per query would erase the query-side win entirely, so `FuzzyIndex`
is a build-once/query-many tool, not a per-call convenience — consistent
with its own Build → Freeze → Query design.

### verbora-trie: FrozenTrie (path-compressed query representation)

The fifth Verbora-native extension — no reference counterpart, and unlike
the four above it targets a *measured competitive loss* rather than a new
capability: `docs/COMPETITIVE_BENCHMARKS.md` §1.18 found `fast_radix_trie`
beating `Trie::keys_with_prefix`/`predictive_search` by 1.64×–2.19× while
`Trie` kept winning `build`/`contains` against the same competitor. See
`crates/verbora-trie/src/frozen.rs`'s own doc comment for the compression
algorithm and exactly why it is exact, not approximate.

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ✅ | `iter_keys_with_prefix` returns a lazy `FrozenKeysWithPrefix`, the same depth-first-with-restore-stack shape `verbora_trie::KeysWithPrefix` already uses, adapted to push a whole compressed edge label per step instead of one code unit |
| Zero-copy | N/A | queries return owned `String`s by construction (a stored word exists nowhere contiguously, spelled out across edge labels) — same as `Trie` itself, not a regression specific to freezing |
| Reusable memory | ➖ | build-once/freeze/query shape; not an applicable dimension |
| Batch | ➖ not built this pass | composes with a plain `.map()` over a caller's own prefix list today |
| Parallel | ➖ not evaluated this pass | queries are read-only against a shared, immutable `FrozenTrie` and would parallelize trivially if a real batch workload needs it |
| Alloc reviewed | ✅ by inspection | `freeze()` allocates the frozen node `Vec`, the shared `units: Vec<u16>` buffer, and one `SmallVec` per frozen node (inline for ≤2 children, matching `Trie`'s own `Child` inline-capacity reasoning); queries allocate the same one-`String`-per-emitted-word cost `Trie::keys_with_prefix` already pays, no more |
| Data structures reviewed | ✅ | flat `Vec<FrozenNode>` arena (same "flat arena over recursive links" choice `Trie`/`FuzzyIndex` both made), compressed edges as `(label_start, label_end, node)` ranges into one shared `Vec<u16>` rather than per-edge heap buffers |
| mmap/rkyv reviewed | ➖ | no bundled dataset; built at runtime from a caller-supplied `Trie` |
| Benchmarked | ✅ | `crates/verbora-trie/benches/trie.rs`'s `enumeration`/`contains_hit`/`contains_miss`/`freeze` groups (in-crate, vs. the mutable arena and two `HashMap`-per-node baselines) and `benchmarks/competitive/rust-competitors/benches/trie.rs`'s `predictive_search`/`contains_hit`/`contains_miss` groups (head-to-head vs. `fast_radix_trie`) |
| Parity | N/A | not a parity crate; correctness verified directly against `Trie` (below), not against a reference implementation |

**Correctness**, `crates/verbora-trie/src/frozen.rs`'s own test module: an
80-round randomized fuzzer (no external `rand` dependency — the same
dependency-free-PRNG discipline `verbora-distance`'s own randomized
Levenshtein tests use) comparing `Trie` and `FrozenTrie` on `contains` and
`keys_with_prefix` across every prefix of every generated word plus random
misses, mixing digit-hoisted keys, case folding, and astral (surrogate-pair)
characters. Independently re-verified by a second, adversarial audit with no
visibility into the implementation's own reasoning, which wrote its own
fresh tests (including a surrogate pair deliberately split across two
different compressed edges) and confirmed those tests had real teeth by
injecting and catching two deliberate bugs before reverting them. Neither
pass found a disagreement.

**Real numbers, one development machine.** In-crate
(`cargo bench -p verbora-trie --bench trie`, 20,000-word corpus):

| Group | Arena (`Trie`) | `FrozenTrie` | Ratio |
|---|---:|---:|---:|
| `enumeration/keys_stream` | 998.4 µs | 605.9 µs | **1.65× faster** |
| `enumeration/keys_collect` | 1.315 ms | 920.1 µs | **1.43× faster** |
| `enumeration/keys_with_prefix_1char` | 1.179 ms | 598.5 µs | **1.97× faster** |
| `contains_hit` | 1.241 ms | 1.895 ms | 1.53× **slower** |
| `contains_miss` | 1.272 ms | 2.064 ms | 1.62× **slower** |
| `freeze` (one-time cost) | — | 1.023 ms | comparable to `build` itself |

Head-to-head against `fast_radix_trie`
(`cargo bench -p competitive-rust --bench trie`): `FrozenTrie` **overtakes**
`fast_radix_trie` on `predictive_search/1char` (1.06× faster — the realistic
autocomplete shape) but still trails on `predictive_search/all`, full-corpus
enumeration (1.45× slower); `contains_hit`/`contains_miss` lose to
`fast_radix_trie` too, as a direct consequence of the same-direction
regression against the plain arena above. See `docs/PERFORMANCE_GAPS.md`
entry 32's "Update" section for the full table and the architectural
explanation (fewer-but-costlier hops helps only when a query crosses many
compressible nodes, which whole-tree enumeration does far more of than any
single-path point lookup) and `docs/COMPETITIVE_BENCHMARKS.md` §1.18's
Architectural decision note for the shipped recommendation: `Trie` for
point-lookup-heavy call sites, `FrozenTrie` (frozen once after bulk-loading)
for enumeration/autocomplete-heavy ones — a genuine, disclosed trade-off,
not a strict improvement kept only because it wins on average.

### verbora-spellcheck: DeletionIndex (SymSpell-style deletion index)

The sixth Verbora-native extension — no reference counterpart, and like
`FrozenTrie` it targets a *measured competitive loss* rather than a new
capability: `docs/PERFORMANCE_GAPS.md` entry 35 found `fast_symspell` (a
real, pinned third-party deletion-index crate) beating `FuzzyIndex`'s own
query speed by a margin that widens with corpus size (2.15×–66.7×).
`DeletionIndex` is Verbora's own answer, built in-house with
`verbora_distance` primitives rather than wrapping `fast_symspell` itself
(a young, low-adoption crate with its own real bug — see entry 36) — the
exact recommendation `docs/COMPETITIVE_BENCHMARKS.md` §1.17's Architectural
decision note made. See `crates/verbora-spellcheck/src/deletion_index.rs`'s
own doc comment for the full algorithm and — importantly — why deletion
generation operates on UTF-16 code units, not `char`s (a real correctness
risk for astral/non-BMP input, found and fixed during implementation, not a
theoretical concern).

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ⚠️ partial | candidate *discovery* (generating and looking up every deletion sequence) cannot be streamed — it must complete before any result can be verified — but the real-edit-distance *verification* of each discovered candidate is lazy, matching `FuzzyIndex::neighbors`'s own laziness for that half of the work |
| Zero-copy | N/A | queries return `&str` borrowed from the index's own stored words, no allocation on the result side beyond the owned `query: String` `DeletionNeighbors` holds (same shape `FuzzyIndex::Neighbors` already uses) |
| Reusable memory | ➖ | build-once/freeze/query shape; not an applicable dimension |
| Batch | ➖ not built this pass | composes with a plain `.map()` over a caller's own query list today |
| Parallel | ➖ not evaluated this pass | queries are read-only against a shared, immutable `DeletionIndex` and would parallelize trivially if a real batch workload needs it |
| Alloc reviewed | ✅ by inspection | construction allocates one `Vec<u16>` deletion sequence per (word, depth) combination during build (the real, disclosed combinatorial cost — see the Benchmarked row); queries allocate the same shape, once, then a `Vec<u32>` of deduplicated candidate indices |
| Data structures reviewed | ✅ | `FxHashMap<Box<[u16]>, Vec<u32>>` (deletion sequence → word indices) plus a flat `Vec<Box<str>>` word list — `rustc-hash`'s `FxHashMap`, already this crate's own choice for short-key hashing (see `Cargo.toml`'s comment on `Spellcheck`'s own edit generator) |
| mmap/rkyv reviewed | ➖ | no bundled dataset; built at runtime from a caller-supplied word list |
| Benchmarked | ✅ | `crates/verbora-spellcheck/benches/deletion_index.rs` — construction and query, `DeletionIndex` vs. `FuzzyIndex` vs. brute-force, at 100/1,000/10,000/20,000 words, `max_distance = 2` |
| Parity | N/A | not a parity crate; correctness verified directly against a brute-force Levenshtein scan and against `FuzzyIndex` itself (below), not against a reference implementation |

**Correctness**, `crates/verbora-spellcheck/tests/deletion_index.rs`: the
same "verify against ground truth" discipline `FuzzyIndex`'s own test
already established — a 3,000-word ASCII sample vs. brute force at
`max_distance` 0–3 — plus two checks specific to this structure's own real
risk areas: a dedicated astral-character-heavy dictionary (emoji,
mathematical alphanumerics, mixed BMP/astral words) vs. the same brute-force
baseline, exercising the UTF-16-code-unit-vs-`char` correctness fix
directly; and a 1,000-word sample where `DeletionIndex` and `FuzzyIndex` are
required to agree with *each other* (not just each independently with
brute force) at every `max_distance` within `DeletionIndex`'s own
build-time cap.

**Real numbers, one development machine**, `cargo bench -p verbora-spellcheck
--bench deletion_index` (`max_distance = 2` throughout — `DeletionIndex`'s
own build-time cap and every query, matching `FuzzyIndex`'s own benchmarked
query distance):

| Words | Construction: `DeletionIndex` | Construction: `FuzzyIndex` | Query: `DeletionIndex` | Query: `FuzzyIndex` | Query: brute force |
|---:|---:|---:|---:|---:|---:|
| 100 | 977.6 µs | 38.7 µs | 1.018 ms | 589.9 µs | 1.602 ms |
| 1,000 | 11.83 ms | 779.9 µs | 2.233 ms | 10.93 ms | 32.92 ms |
| 10,000 | 162.6 ms | 12.39 ms | 2.641 ms | 93.28 ms | 331.7 ms |
| 20,000 | 407.0 ms | 26.97 ms | 3.206 ms | 174.1 ms | 610.4 ms |

**A genuine, honest trade-off — not a clean win, reported in full.**
Construction: `DeletionIndex` is **13×–25× slower to build** than
`FuzzyIndex` at every size — the real, disclosed cost of precomputing every
deletion sequence up to `max_distance`, the same shape of cost the
competitive audit already found `fast_symspell` paying against Verbora's
own `Spellcheck` (entry 35). Query: a genuine **crossover, not a one-sided
result** — `FuzzyIndex` is actually **faster at the smallest size**
(100 words, 1.73×) where the BK-tree stays shallow and a deletion index's
fixed per-query overhead has not yet paid for itself, but `DeletionIndex`
**wins from 1,000 words up, by a rapidly widening margin** (4.9× → 35.3× →
54.3× at 20,000) — near-flat growth with corpus size (3.3× slower query
time over a 200× larger corpus) against `FuzzyIndex`'s roughly 300× growth
over the same range, the textbook SymSpell shape and the same widening
pattern the competitive audit measured for `fast_symspell` itself.
`DeletionIndex` also beats brute force by a wide and widening margin
throughout (1.6× → 190×), the same "speedup grows with scale" justification
bar `FuzzyIndex`'s own doc comment already established for itself.

**Recommendation.** Neither structure replaces the other — `FuzzyIndex`
stays the right default (query-time `max_distance`, cheap construction,
wins outright at small corpora); reach for `DeletionIndex` when the
dictionary is large (≥1,000 words in this measurement), `max_distance` is
known and fixed ahead of time, and query volume is high enough that the
steep one-time construction cost is worth paying — exactly the situation
`docs/COMPETITIVE_BENCHMARKS.md` §1.17's Architectural decision note
identified before this was built.

### verbora-phonetics: spec-pinned encoder extensions (Cologne, NYSIIS, Caverphone 1/2, Phonex, Refined Soundex, Match Rating, branching Daitch-Mokotoff)

The seventh Verbora-native extension, shipped as one coherent batch of seven
encoder types — no reference counterpart (the JS reference exports exactly
four phonetic classes), so each module pins its behavior **byte-for-byte to
rphonetic 3.0.6** (Apache commons-codec lineage), the same crate it is
benchmarked against. See `AGENTS.md`'s `# Spec-Pinned Phonetic Encoders`
section for the policy entry and each module's own `//!` doc comment in
`crates/verbora-phonetics/src/` for the authoritative behavioral record.

| Aspect | Status | Notes |
|---|---|---|
| Lazy API | ➖ | one short code (or small code set) per token; nothing to stream |
| Zero-copy | ➖ | codes are inherently owned output; one returned `String` per call |
| Reusable memory | ✅ | each encoder runs a single-pass scan over one reused internal buffer; no `_into` variant, same as the crate's four core encoders |
| Batch | ✅ | all seven implement `verbora_core::Phonetic`, so `par_encode_batch` and `phoneticize_tokens*` accept them unchanged |
| Parallel | ✅ | via the crate's existing chunked `par_encode_batch` (feature `parallel`); no new parallel surface added |
| Alloc reviewed | ✅ | one heap allocation per call (the returned code); `DaitchMokotoff` adds a small branch `Vec` (two allocations/call, per its module doc). rphonetic allocates intermediate `String`s per rewrite step — the main measured mechanism below |
| Data structures reviewed | ✅ | rule sets embedded as pre-sorted `static` tables (the crate's `dm_table` style; ordering invariant asserted by a unit test) vs. rphonetic's `nom`-parsed-at-builder-time rules and per-lookup `BTreeMap` walk |
| mmap/rkyv reviewed | ➖ | compiled-in static tables only, no file-backed dataset |
| Benchmarked | ✅ | `benchmarks/competitive/rust-competitors/benches/phonetics.rs`, 8 Criterion groups × {1, 10,000, 100,000} names, Verbora vs. rphonetic 3.0.6 — full-default Criterion settings |
| Parity | N/A (not a JS-parity surface) | **byte-exact vs. rphonetic instead** — a stronger equivalence than the crate's four Partial rphonetic rows: `tests/phonetics_correctness.rs` regime 2 asserts identical output over the 653-name corpus + per-algorithm extras, the MRA match decision over every ordered corpus pair (~426K), and a three-way `process`/`codes`/`encode` Daitch-Mokotoff check — all first-run green; an independent adversarial audit differentially fuzzed 104,114 inputs per encoder with zero mismatches, proved every documented divergence exactly as narrow as claimed, and mutation-tested the suites; crate suite 322 unit + 49 doctests |

**Real numbers, one development machine** (Criterion medians, Verbora vs.
rphonetic; ratios recomputed from the raw `estimates.json` medians —
Verbora wins all 24 cells):

| Encoder | 1 name | 10,000 names | 100,000 names |
|---|--:|--:|--:|
| `cologne` | 17.4 ns vs. 72.2 ns (4.16×) | 314.99 µs vs. 738.89 µs (2.35×) | 3.250 ms vs. 7.321 ms (2.25×) |
| `nysiis` | 29.7 ns vs. 224.5 ns (7.56×) | 266.53 µs vs. 1.889 ms (7.09×) | 2.688 ms vs. 20.517 ms (7.63×) |
| `caverphone1` | 176.4 ns vs. 914.3 ns (5.18×) | 1.907 ms vs. 10.317 ms (5.41×) | 19.013 ms vs. 99.617 ms (5.24×) |
| `caverphone2` | 153.9 ns vs. 811.8 ns (5.27×) | 1.770 ms vs. 9.262 ms (5.23×) | 17.748 ms vs. 88.009 ms (4.96×) |
| `phonex` | 41.3 ns vs. 145.7 ns (3.53×) | 483.95 µs vs. 1.358 ms (2.81×) | 4.608 ms vs. 12.933 ms (2.81×) |
| `refined_soundex` | 14.8 ns vs. 114.2 ns (7.73×) | 129.36 µs vs. 972.06 µs (7.51×) | 1.298 ms vs. 9.630 ms (7.42×) |
| `match_rating` | 31.9 ns vs. 489.3 ns (15.32×) | 323.12 µs vs. 5.344 ms (16.54×) | 3.059 ms vs. 53.249 ms (17.41×) |
| `daitch_mokotoff` | 154.9 ns vs. 363.4 ns (2.35×) | 1.788 ms vs. 3.720 ms (2.08×) | 16.834 ms vs. 37.170 ms (2.21×) |

A clean sweep, so unlike `FrozenTrie`/`DeletionIndex` there is no
unfavourable cell to publish — the honest disclosures here are behavioral
instead: the four rphonetic Daitch-Mokotoff quirks reproduced deliberately
for byte-parity, and the rphonetic panic-domain findings
(`docs/PERFORMANCE_GAPS.md` entry 36, item 4) whose input shapes are
excluded from the benchmark domain because only Verbora survives them.

## What this table does not yet cover

- `verbora-core` and `verbora-examples` are infrastructure
  crates (shared traits, the fixture-replay harness, the doc-snippet
  harness) rather than NLP subsystems, and are out of this matrix's scope —
  see `AGENTS.md` for their role.
- The site documentation pass (updating `site/performance/parallelism.md`,
  `site/recipes/parallel-corpus.md`, and the per-feature pages to describe
  the new `par_*` APIs) is tracked separately and was not yet complete when
  this file was written — `site/check-facts.py`'s structural check (every
  `par_*` function must be gated behind `parallel = ["dep:rayon"]`) passes,
  but the prose on several pages still predates this audit.
- `verbora-ngrams`'s own Rayon candidacy was not separately quantified in
  the cross-cutting Rayon-candidates pass; revisit before assuming ➖ is the
  final answer.
- `verbora-language`'s own `site/features/language.md` page (Fase 5's
  checklist item) now exists alongside this file's numbers — see that page
  for the narrative write-up (explicit-vs-automatic guidance, confidence and
  ambiguity, transliteration integration) this table only summarizes.

## How to reproduce a row

```bash
cargo bench -p <crate>                    # sequential baseline
cargo bench -p <crate> --features parallel -- parallel   # if a parallel group exists
cargo test -p <crate>                     # default features
cargo test -p <crate> --features parallel # parallel feature, if the crate has one
```
