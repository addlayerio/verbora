# Upgrading from 0.1 to 0.2

Verbora 0.2.0 is **not source-compatible with 0.1.0**. Bumping a dependency from
`"0.1"` to `"0.2"` will produce compile errors in most programs, and in a few
places it will produce different answers without producing a compile error at
all. This page is for fixing both.

<div class="callout callout-warn">
<strong>Read the second half even if the first half fixes your build.</strong>
The changes that <em>do not</em> break compilation are the dangerous ones — a
different ranking, a different tag, a different sentiment score. They are
listed under <a href="#changes-that-do-not-break-the-build">Changes that do not
break the build</a>.
</div>

## What happened

0.1.0 was a port. Its behaviour was defined by agreement with the implementation
it had been ported from: the fixtures recorded that implementation's output, the
documentation explained where a rule came from rather than what it does, and
several tables existed only because something else shipped them.

0.2.0 finishes the migration to a Rust-native specification. Every behaviour is
now defined by a published standard or by an explicit Verbora contract, and the
tests assert the contract rather than the ancestry. Six rules were applied across
all nineteen crates, and between them they account for most of the churn:

1. **No sentinels.** Absence is `Option::None`, never a magic value carved out of
   a numeric range.
2. **No `NaN` escapes.** It poisons comparison and sorting silently.
3. **No panics outside preconditions the type system cannot express.** Invalid
   states are made unrepresentable by `Result`-returning constructors instead.
4. **No function silently rewrites its input.** Case folding, trimming and
   normalisation are the caller's explicit choice.
5. **The crate root is the entire public surface.** Modules are private and
   everything is re-exported, so each item has exactly one path.
6. **The text unit is stated and justified per crate**, never inherited.

Rule 5 alone breaks every `use verbora_distance::levenshtein::…`-style import in
existing code. Rules 1–3 are why so many return types moved.

## Fix the build first

### Every module path is gone

This is the single most common error, and the fix is mechanical.

```rust ignore
// 0.1
use verbora_distance::levenshtein::{Options, levenshtein};
use verbora_spellcheck::edits::edits;
use verbora_ngrams::text::ngrams_str;
use verbora_core::stopwords::StopWords;
```

Import from the crate root instead. Every public item has exactly one path now,
and it is `verbora_<crate>::Item`. If the root does not export it, it is no
longer public — see the per-crate tables below for what replaced it.

### The rename table for everything that survived

These items still exist, under a different name or a different signature. This is
the list to scan first, because each row is a small edit rather than a redesign.

| Crate | 0.1 | 0.2 |
|---|---|---|
| `verbora-distance` | `damerau_levenshtein(a, b, &Options)` → `f64` | `damerau_levenshtein(a, b)` → `usize` |
| | `levenshtein(a, b, &Options)` → `f64` | `levenshtein(a, b)` → `usize` |
| | `Options { restricted: true, .. }` | `osa(a, b)` → `usize` — a separate function |
| | `Options { insertion_cost, .. }` | `LevenshteinCosts::new(…)` / `OsaCosts::new(…)` / `DamerauCosts::new(…)`, each returning `Result<_, CostError>`, passed to `*_weighted` |
| | `jaro_winkler(a, b, &Options)` | `jaro_winkler(a, b)` |
| | `hamming(a, b, ignore_case)` → `i64`, `-1` for incomparable | `hamming(a, b)` → `Option<usize>` |
| | `hamming_checked`, `INCOMPARABLE` | gone — `hamming` is the checked form |
| | `SearchResult { substring: String, distance: f64, offset: isize }` | `SearchResult<'t, D>` with `substring() -> &'t str`, `distance() -> D`, `range() -> Range<usize>` (bytes) |
| | `StringMetric`, `Levenshtein`, `DamerauLevenshtein`, `JaroWinkler`, `Dice`, `Hamming` | gone — call the free functions |
| `verbora-core` | `StopWords::english()` | `StopWords::for_language(StopWordLanguage::En)` |
| | `StopWords::remove(&mut self, w)` → `()` | → `bool` |
| | `StopWords::remove_all(…)` → `()` | → `usize` |
| | `Token`, `collapse_whitespace`, `is_whitespace`, `trim_edge_empties` | gone |
| | `DoubleKeyPhonetic::process_double` → `(String, String)` | → `(String, Option<String>)` |
| `verbora-trie` | `add_string`, `add_strings` | `insert`, `insert_all` |
| | `get_size()` | `node_count()` — and `len()` is new, and counts **words** |
| | `is_case_sensitive()` → `bool` | `case_handling()` → `CaseHandling` |
| | `with_case_sensitivity(bool)` | `with_case_handling(CaseHandling)` |
| | `find_matches_on_path`, `iter_matches_on_path`, `MatchesOnPath` | `prefix_matches`, `iter_prefix_matches`, `PrefixMatches` |
| | `find_prefix` → `(Option<Cow>, Cow)` | `longest_prefix` → `PrefixSplit { word, rest }` |
| | `find_prefix_lengths` → `(Option<usize>, usize)` | `longest_prefix_lengths` → `PrefixSplitLengths { word, rest }` |
| `verbora-transliterators` | `transliterate`, `transliterate_into` | `transliterate_ja`, `transliterate_ja_into`, `transliterate_ja_normalized` |
| | `Phase` | gone — the pipeline is internal |
| `verbora-normalizers` | `normalize`, `normalize_token` (English contraction expansion: `"I'D"` → `["I", "would"]`) | gone — this crate is Unicode normalization and diacritic folding only |
| | `normalize_ja`, `normalize_no`, `normalize_sv` | gone — `nfkc` covers the width and kana folding `normalize_ja` did; `normalize_no` and `normalize_sv` existed for the Norwegian and Swedish stemmers, which is exactly where the umlaut-folding defect lived, and they are no longer public |
| `verbora-inflectors` | `pluralize` → `Result<String, EmptyToken>` | → `String` (total; the empty token has no inflected form and comes back unchanged) |
| | `CountInflector`, `CountInflectorFr` | `OrdinalInflector`, `OrdinalInflectorFr` (the French one takes a `Gender`) |
| | `CaseMode::{Lower, Capitalize, Upper}` | `CaseMode::{Preserve, Title, Upper}` |
| | `restore_case(token)` | `CaseMode::of(token)` |
| | `PatternError` | `RuleError` (`#[non_exhaustive]`) |
| | `Rule::apply` → `Option<String>` | → `Option<Cow<'t, str>>` |
| `verbora-spellcheck` | `Spellcheck::get_corrections` → `Vec<String>` | `corrections` → `Vec<Correction<'_>>`, or `correction_words` → `Vec<String>` |
| | `par_get_corrections_batch` | `par_corrections_batch` → `Vec<Vec<Correction<'_>>>` |
| | `frequency` → `Option<f64>` | → `Option<u32>` |
| | `frequencies` → `(&str, f64)` | → `(&str, u32)` |
| | `edits`, `edits_utf16`, `edits_with_max_distance*`, `Edits`, `EditUnit`, `ALPHABET`, `sort_by_frequency` | gone — candidate generation is internal |
| | `Spellcheck::trie()` | gone |
| | `DeletionIndex::neighbors` → `DeletionNeighbors` | → `Result<DeletionNeighbors, DistanceBeyondIndex>` |
| `verbora-phonetics` | `DoubleMetaphone::process` → `(String, String)` | → `DoubleMetaphoneCode`, with `primary()`, `alternate() -> Option<&str>`, `into_parts()` |
| | `SoundEx::process_with`, `Metaphone::process_with`, `*_utf16`, `try_process` | gone — one key, one call, no `max_length`, no `PhoneticError` |
| | `SoundExDM` | gone. 0.1 shipped two Daitch–Mokotoff implementations; `DaitchMokotoff` is the one that remains. They were separate code, so re-check your keys rather than assuming a rename |
| | `DaitchMokotoffCode` | `DaitchMokotoff::codes` → `Vec<String>` |
| | `phoneticize_tokens_with`, `tokenize_and_phoneticize_with` | folded into `phoneticize_tokens` / `tokenize_and_phoneticize` |
| `verbora-sentiment` | `SentimentAnalyzer::new(lang: &str, stemmer, kind: &str)` | `SentimentAnalyzer::with_stemmer(Language, VocabularyKind, stemmer)` |
| | `without_stemmer(&str, &str)` | `without_stemmer(Language, VocabularyKind)` |
| | `get_sentiment` → `f64` | → `Option<f64>` (`None` when no word scored) |
| | `Score::value` → `f64` (`sum / count`, so `NaN` when nothing scored) | `Score::mean` → `Option<f64>` — `None` is the empty case; `Score::over` is `Option<f64>` for the same reason |
| | `Polarity` (an enum of `Number`/`Text`) | a struct: `value() -> f64`, `as_written() -> Option<&'static str>` |
| | `Error` | `UnsupportedPair` / `UnknownName` |
| | `Vocabulary::shared_for` | `Vocabulary::shared(kind, Language)` |
| | `Language::from_pattern`, `as_str` | `Language::from_code`, `code` (plus `FromStr`) |
| `verbora-language` | `f32` confidences | `Confidence`, built with `Confidence::new(f32) -> Option<Self>` |
| | `PhoneticRecommendation::SoundExDaitchMokotoff` | `PhoneticRecommendation::DaitchMokotoff` (plus new `Cologne` and `BeiderMorse` variants) |
| | `LanguageDetection { candidates }` (public field) | `candidates()`, `len()`, `is_empty()`, `single(…)` |
| `verbora-util` | `CyclicDependency` | `Cycle` |
| | `VertexKey`, `Vertex` | `VertexId` |
| | `Bag`, `StorageBackend`, `FileBackend`, `StoragePlugin`, `StorageType` | gone |
| | `Language` (stop words) | still `verbora_util::Language`, now a re-export of `verbora_core::StopWordLanguage` |
| `verbora-wordnet` | `Pos` | `PartOfSpeech` |
| | `Find`, `Source`, `Probe`, `Probes`, `FilePair`, `IndexHit`, `IndexRecord`, `DataRecord`, `PointerRef` | gone or replaced by `Synset`, `SynsetRef`, `IndexEntry`, `WordRef`, `Gloss` |
| `verbora-stemmers` | `verbora_stemmers::Token` re-export | gone with `verbora_core::Token` |
| | `stopwords::{add_all, contains, remove, …}(Language, …)` | inherent methods on `Language`: `Language::De.contains(w)`, `.add_all(…)`, `.reset()` |
| | `TokenizeAndStem::is_word_char` | gone (`HYPHEN_JOINS_LETTERS` is the remaining knob) |

### The crates that were rewritten rather than renamed

For these, there is no row-by-row mapping worth printing, because almost nothing
survived under a recognisable shape. Read the feature page and the rustdoc, and
plan on rewriting the call site.

| Crate | What is there now |
|---|---|
| [`verbora-tokenizers`](../features/tokenizers.md) | Three tokenizers grounded in UAX #29 — `WordTokenizer`, `SegmentTokenizer`, `SentenceTokenizer`. The sixteen `AggressiveTokenizer*` language variants, `TreebankWordTokenizer`, `RegexpTokenizer`, `WordPunctTokenizer`, `CaseTokenizer`, `OrthographyTokenizer`, `TokenizerJa` and `Utf16Token` are all gone: their rules could not be traced to any standard. |
| [`verbora-ngrams`](../features/ngrams.md) | `ngrams(seq, n: NonZeroUsize)` over a slice, `Padded` for boundary symbols, `char_ngrams(text, n)` for scalar windows. The `*_str`, `*_with_stats`, `*_zh`, `bigrams`/`trigrams`/`multrigrams` families, `NGramStats`, `ngram_key` and the process-global `set_tokenizer` are all gone. Word n-grams are now the composition you write: tokenize, then `ngrams` over the token slice. |
| [`verbora-tagger`](../features/tagger.md) | `BrillTagger`, `Lexicon`, `RuleSet`, `Corpus`, `Trainer`, `Evaluation`, `Tag`, `TaggedToken`. `BrillPosTagger`, `BrillPosTrainer`, `BrillPosTester`, `TransformationRule`, `RuleTemplate`, `Predicate`, `TaggerError` and the rest of the 0.1 surface are gone. |
| [`verbora-tfidf`](../features/tfidf.md) | `TfIdf` with `add_document(&str) -> usize`, `add_terms`, `tfidf(query, index) -> Option<f64>`, `rank(query) -> Vec<DocumentScore>`, and an owned `Analyzer` holding the tokenizer, case folding and stop-word list. The dynamic `DocKey`/`DynValue`/`JsonValue` layer, `Interner`, `TermId`, `Terms`, `Encoding`, `TfIdfError` and the process-global tokenizer and stop-word setters are gone. `to_json` now returns `Result<String, ExportError>`; `from_json` returns `Result<Self, RestoreError>`. |
| [`verbora-classifiers`](../features/classifiers.md) | `BayesClassifier`, `LogisticRegressionClassifier`, `MaxEntClassifier` over a reworked training and persistence surface (`TrainingReport`, `TrainingStep`, `StopReason`, `ModelDefect`, stamped model files). `Context`, `Feature`, `FeatureSet`, `GenerateFeatures`, `GISScaler`, `MECorpus`, `MESentence` and the rest are gone. Maximum entropy is now an implementation of Generalised Iterative Scaling rather than a reproduction; in 0.1 `Sample::new` rejected every non-empty argument, so the feature did not work at all. |
| [`verbora-analyzers`](../features/analyzers.md) | `analyze(&[TaggedWord]) -> SentenceAnalysis`, with `Role`, `TagClass`, `Terminator`, `SentenceType` and `ImpliedSubject`. The mutable `SentenceAnalyzer` with its `part()`/`type_of()` staging, `TaggedSentence`, `Punct`, `Field`, `SenType` and `TypeError` are gone. |
| [`verbora-wordnet`](../features/wordnet.md) | `WordNet` over `Synset`, `SynsetRef`, `Sense`, `Pointer`, `PointerSymbol`, `Gloss`, `IndexEntry`, `PrebuiltIndex` and four `Storage` modes. |

<div class="callout callout-note">
<strong><code>SenType</code> did not simply get renamed.</strong>
0.1's <code>SenType</code> had five variants including <code>Unknown</code> and
<code>Command</code>. 0.2's <code>SentenceType</code> has four —
<code>Declarative</code>, <code>Interrogative</code>, <code>Imperative</code>,
<code>Exclamative</code> — and absence is <code>Option::None</code> rather than
an <code>Unknown</code> variant. A <code>match</code> ported across mechanically
will compile and mean something different.
</div>

## `#[non_exhaustive]`, once and properly

**35 public enums are `#[non_exhaustive]` in 0.2.0, up from 6 in 0.1.0.** A
downstream `match` over any of them now needs a wildcard arm, or it fails to
compile with `E0004: non-exhaustive patterns`.

```rust ignore
// 0.1: compiled, because the enum was closed.
match err {
    CostError::NotFinite { .. } => …,
    CostError::Negative { .. } => …,
    CostError::TranspositionBelowThreshold { .. } => …,
}
```

```rust ignore
// 0.2: add the arm.
match err {
    CostError::NotFinite { .. } => …,
    CostError::Negative { .. } => …,
    CostError::TranspositionBelowThreshold { .. } => …,
    _ => …,
}
```

The 21 error enums are the ones you are most likely to be matching on:

`verbora_classifiers::{ClassifierError, LoadError, MaxEntError, ModelDefect,
RestoreError, StampError}` · `verbora_distance::CostError` ·
`verbora_inflectors::RuleError` · `verbora_tagger::{CorpusParseError,
LexiconError, LiteralError, RuleParseError}` ·
`verbora_tfidf::{ExportError, RestoreError, StampError}` ·
`verbora_tokenizers::AbbreviationError` · `verbora_util::{GraphError, PathError}` ·
`verbora_wordnet::{Error, ParseSenseError, RecordError}`

The other 14 are the data enums those errors carry, or that a caller matches
alongside them: `verbora_core::StopWordLanguage` ·
`verbora_classifiers::TrainingEvent` · `verbora_distance::Operation` ·
`verbora_language::{Language, PhoneticRecommendation, Script, StrategyBasis,
TransliterationAdvice}` · `verbora_sentiment::{Language, VocabularyKind}` ·
`verbora_tagger::{Condition, Language, Template}` ·
`verbora_util::AbbreviationLanguage`.

### Why now, and why the payloads too

Pre-1.0 is the window where this costs a wildcard arm rather than a major
version. An error enum that is closed cannot gain a variant without a breaking
release, which in practice means either shipping a breaking release to describe
a newly-distinguished failure or folding it into an existing variant and losing
the distinction. Sealing them here buys the freedom to name failures precisely
later, at the price of one arm today.

The 14 data enums are marked for the same reason, one level down. Sealing an
error type while leaving the enum it *carries* closed gives the freedom straight
back: a newly-distinguished failure almost always needs a new payload value to
describe it — a new `Operation`, a new `Language`, a new `Template` — and adding
one to a closed enum is the breaking change the outer seal was meant to avoid.
They are marked together deliberately.

Three things this does *not* change:

- `if let` and `matches!` are unaffected unless they were exhaustive.
- You can still construct these enums' variants from your own crate. The
  attribute is on the enum, not on its variants, so it constrains matching only:
  `CostError::Negative { operation, value }` still compiles downstream.
- A `match` that already had a `_` arm, or that binds the whole value, compiles
  unchanged.

## Changes that do not break the build

These compile after the mechanical fixes above and then return something
different. Each one is a defect fix — 0.1's answer was wrong — but "wrong" is
not the same as "not what your snapshot test asserts".

### Ordering

<div class="callout callout-warn">
<strong>Correction ranking is a documented total order now, and re-sorting the
results cannot disagree with it.</strong> In 0.1,
<code>Spellcheck::get_corrections</code> returned <code>Vec&lt;String&gt;</code>
ranked by a comparator that reproduced another runtime's <code>sort</code>
semantics over <code>f64</code> frequencies. In 0.2 the ranking is
<strong>distance ascending, then frequency descending, then word
ascending</strong>, and it is written out as <code>Correction</code>'s own
<code>Ord</code> — hand-written rather than derived, because a derived
<code>Ord</code> compares fields in declaration order and would put
<code>word</code> first. <code>Neighbor</code>'s order is distance ascending,
then word ascending, on the same reasoning. Code that collected 0.1's results
and re-sorted them gets a different order.
</div>

```rust
use verbora_spellcheck::Spellcheck;

// Repeats are frequencies: "the" occurs three times, "he" twice, "she" once.
let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);

// Distance first, then frequency descending: the exact match leads, and
// "the" outranks "she" at the same distance because it is more common.
assert_eq!(sc.correction_words("he", 1), ["he", "the", "she"]);

let best = sc.best_correction("he", 1).expect("a correction exists");
assert_eq!((best.word, best.distance, best.frequency), ("he", 0, 2));
```

Frequencies are `u32` rather than `f64` for the same reason the ranking moved.
A count is an integer, and 0.1's `f64` frequencies were not merely imprecise:
for twelve specific words the stored value was `NaN`, which a comparator
silently reorders everything around. `Spellcheck::frequency` now returns
`Option<u32>`, and `frequencies()` yields `(&str, u32)`; there is no `NaN` left
in this crate to guard against.

`LogisticRegressionClassifier` had a related defect: `fit` built its target
columns in the feature map's enumeration order while `classifications()`
reported weights in insertion order. Those orders differ exactly when a label
looks like an integer, so a document trained as `"2"` classified as `"1"`, and
vice versa, confidently. **Retrain any model with integer-like labels.**

### The unit of measurement

`verbora-distance` and `verbora-stemmers` counted UTF-16 code units in 0.1 and
count **Unicode scalar values** in 0.2. `verbora-trie` counts scalars per node.
Below U+10000 the two readings coincide; above it they do not.

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert("a👍");

// 0.1's `get_size()` reported 4 here: root, 'a', and the two halves of the
// surrogate pair. One scalar is now one node, whatever plane it lives in.
assert_eq!(t.node_count(), 3);
assert_eq!(t.len(), 1); // and `len()` counts stored words, which is new
```

Stemmer output changes for input containing astral characters, for the same
reason.

`SearchResult` carried the same unit problem and a sentinel besides: 0.1's
`offset` was a **UTF-16 code unit** index into the target, signed, and genuinely
`-1` when the backtrace exited through column 0. 0.2 returns
`range() -> Range<usize>` in **bytes**, derived from the borrowed substring, so
`&target[found.range()] == found.substring()` for every input and there is no
negative case to handle:

```rust
use verbora_distance::levenshtein_search;

let target = "Zürich, Berlin, Wien";
let found = levenshtein_search("Berlin", target);

// "Zürich, " is eight characters but nine bytes, because "ü" takes two.
assert_eq!(found.range(), 9..15);
assert_eq!(&target[found.range()], found.substring());
assert_eq!(found.distance(), 0);
```

### Behavioural fixes that change results

Each was found during the migration, each is now pinned by a test that fails
without the fix, and each will move an output your program may be asserting on.

| Crate | What changed | What it means for you |
|---|---|---|
| `verbora-sentiment` | The tokenizer's UAX #29 boundaries split hyphenated lexicon keys, making 2,313 entries unreachable, several of them sign-inverted — `"non-approved"` scored `+1` against a stored polarity of `-2`. Multi-token span matching now reaches them, and the phrase keys single-token lookup never could. | Scores move, sometimes by a sign. Re-baseline any threshold tuned on 0.1. |
| `verbora-stemmers` | Swedish and Norwegian folded a/o umlauts before consulting stop-word lists spelled with them, so 116 of 428 Swedish entries could never match. In those languages those are distinct letters. | Swedish and Norwegian stop-word filtering removes more than it used to. |
| `verbora-stemmers` | The German stemmer's character gate was a byte-for-byte copy of the Spanish one: it admitted a/e/i/n/o/u accents and omitted a/o/u umlauts and eszett. | German stems change. |
| `verbora-stemmers` | The Dutch stop-word list spelled an entry with a trailing space, so the pronoun *je* was never filtered. | One more Dutch stop word is filtered. |
| `verbora-tagger` | 292 keys of the bundled English lexicon carried corpus markup rather than tokens — 256 with a backslash, 36 with an embedded Penn tag — and 199 of them had no correctly-spelled counterpart, so those tokens fell through to the capitalised default. `tag_of("Asia/Pacific")` returned `NNP`; it now returns `JJ`. | Tags change on tokens the broken keys covered. The bundled English lexicon now holds 92,538 keys. |
| `verbora-tagger` | `Tag::new("*")` is refused (`Err(LiteralError::Wildcard)`), because `*` printed as the wildcard and reparsed as `TagPattern::Any` — a rule that rewrote one tag became one that rewrote every tag, across the documented persistence path. `Corpus::parse_brown` inherits this as `CorpusParseError::WildcardTag`. | A corpus or rule file containing `*` as a literal tag is now an error instead of silently corrupting the rule set. |
| `verbora-tagger` | Eleven bundled Dutch rules could never fire, ten of them naming a sentence-boundary marker that is not a tag in this tagger. They are gone, so the advertised count is the count that can fire: `RuleSet::bundled(Language::Dutch)` now reports 273. | The Dutch rule count changes; no rule that ever fired was removed. |
| `verbora-tagger` | `Template::instantiate` pushed one `Condition` per *position* inspected rather than one per distinct condition, so a window template double- (or triple-) counted a corrected token, defeating the trainer's `min_score` guard. | Trained rule sets differ. Retrain. |
| `verbora-transliterators` | Vowel lengthening consumed the next scalar without asking whether it began a longer key, so `ハロウィン` came out `harōin` and `スウェーデン` came out `sūēden`. Six keys collide this way after any of seventy morae. | Romanisations change for the affected syllables. |
| `verbora-language` | `fold_cyrillic` folded two blocks while the script router feeds it a third, so 100 uppercase letters went unfolded — including `Ґ`, one of four Ukrainian-versus-Russian discriminators. The same text in different case gave a different answer. | Cyrillic detection is now case-invariant, and uppercase Ukrainian text carries its signal. The Cyrillic model was retrained against the corrected extractor. |
| `verbora-phonetics` | Four Beider–Morse Italian rules carried U+FFFD where accented vowels belonged, so real Italian input had the vowel deleted from its encoding. | Beider–Morse encodings change for the affected Italian inputs. |
| `verbora-distance` | `jaro(x, x)` returned `0.0` for single-unit inputs while `jaro_winkler` returned `1.0` for the same pair; `dice_coefficient("", "")` returned `NaN`. Search could return text absent from the target, because a UTF-16 slice could split a surrogate pair and `from_utf16_lossy` substituted U+FFFD. | Those cases now return the documented answers, and `SearchResult::substring()` is a borrow of the target, so it cannot be text the target does not contain. |
| `verbora-tfidf` | The between-documents invariant was restored in `finish`, which unwinding skips, so a panicking caller iterator left the counter dirty and the *next* document reported a present term as `Some(0)` — documented to mean absent. | Corpora built through a panicking tokenizer were wrong; they are not now. |

### Resource behaviour

Not a correctness change, but it will change how your program behaves under
load:

- `verbora-spellcheck`'s deletion generation was cubic in word length. An
  800-scalar token cost 4.0 GB of peak RSS; it now costs 32.4 MB. `k = 0`,
  documented as a membership test, no longer builds an index at all.
  `DeletionIndex` was re-keyed onto a `u64` hash for the same reason — cubic to
  quadratic in word length. The index itself stays quadratic in the longest
  word: that is the symmetric-delete structure's own price, now documented.
- `verbora-trie`'s `insert_all` fed `size_hint().0` straight to `Vec::reserve`,
  so an iterator that lies — which `size_hint` explicitly permits — could abort
  with a capacity overflow or reserve tens of gigabytes. The hint is now clamped.
- `verbora-wordnet`'s `Resident` and `LazyResident` allocated by file metadata
  with no ceiling. The dictionary file is caller-supplied, so its size was always
  an input; the three preloading storage modes now share the ceiling `Indexed`
  already had. (`Storage::Pread` preloads nothing and never had the problem.)
- `MaxEntClassifier::restore`'s JSON parser descended one stack frame per nesting
  level with no bound, so a model file of 20,000 nested brackets overflowed the
  stack — which aborts rather than unwinding, so no caller could catch it. It is
  bounded at 128 levels.

## What this page does not tell you

<div class="callout callout-warn">
<strong>No performance figure on this site has been re-measured against
0.2.0.</strong> The benchmark campaign for this release has not been run. Every
timing published here and on the <a href="../benchmarks/">benchmark pages</a>
was produced against 0.1.0-era code and is marked pending; the kernels
underneath several of them have been replaced since. Treat every number as
historical, and measure your own workload rather than reasoning from a ratio on
this site. The resource figures in the section above are peak-RSS measurements
recorded with the fix, not throughput.
</div>

It also does not cover the places where only the *justification* moved. Several
claims in 0.1's documentation were wrong about code that is unchanged, and
several fixtures were re-derived from the publications they should always have
come from without any value moving:

- An empty phonetic key does **not** imply the input had no recognised letter
  (`Metaphone` on `"y"`, `Cologne` on `"h"`, `MatchRatingApproach` on any single
  vowel), and `DaitchMokotoff` *does* advance by byte length past a matched
  pattern, which consumes one character too many for four two-byte keys.
- A shortest-path tie-break follows relaxation order, not insertion order, and
  149 French inflector entries described as load-bearing are reached by no rule.
- Cologne's 170 expected values were re-derived from Postel (1969) and Match
  Rating's from NBS Special Publication 500-2 (1977). Where that paper reads its
  two comparison passes as consecutive, Verbora interleaves them — a difference
  of 31,044 ratings in 1,129,284 pairs, now stated as a Verbora decision rather
  than inherited. The implementation did not change; the claim about it did.

If you were relying on one of those claims rather than on the code, your program
was already doing something else.

## If you are stuck

1. **Check the crate's rustdoc first.** The crate root is the whole public
   surface now, so `cargo doc --open -p verbora-<crate>` shows you everything in
   one list.
2. **Check the crate's `README.md`.** Every crate has one as of 0.2.0, it is that
   crate's crates.io landing page, and its examples are compiled as doctests.
3. **Check the feature page** for the subsystem — linked from
   [Features](../features/index.md) — for what the replacement is *for*, not just
   what it is called.
4. **Report a documentation error as a bug.** If this page sent you somewhere
   that does not exist, that is worth an issue:
   [the repository](https://github.com/addlayerio/verbora).

## Next

- [Installation](installation.md) — the version pins and the crate table.
- [Your first program](first-program.md) — the current API, four ways.
- [Choosing the right API](../choosing/index.md) — what to reach for now.
