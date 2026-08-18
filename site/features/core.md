# Core vocabulary (`verbora-core`)

`verbora-core` is the crate every other Verbora crate is written against. It
contains no algorithm — six traits, one list-trimming helper, a mutable
work-buffer for stemming, the stop-word list and its process-global mirror, and
two functions implementing Verbora's own definition of `\s`. It depends on no
other `verbora-*` crate, which keeps the crate graph acyclic and lets a leaf
crate like `verbora-distance` be used without pulling in data assets it does not
need.

<div class="callout callout-spec">
<strong>Specification status.</strong> Behaviour is pinned by <strong>21</strong>
in-crate unit tests, plus indirect coverage from every crate that depends on it.
</div>

## When to use it

- You are writing code generic over "any tokenizer" or "any string metric" and
  want it to work with every implementation in the workspace.
- You are implementing your own tokenizer, stemmer or phonetic encoder and want
  the downstream crates to accept it.
- You need `trim_edge_empties`, `is_whitespace` or `collapse_whitespace` —
  Verbora's specific definition of whitespace, not Rust's.
- You need the stop-word list, or Verbora's process-wide mutable stop-word state.

## When not to use it

- **You just want to tokenize a string.** Call the concrete tokenizer's own
  method — see [Tokenizers](./tokenizers.md). Every tokenizer in
  `verbora-tokenizers` is built around a lazy `Tokenize::tokens` iterator that is
  strictly more capable than `verbora_core::Tokenizer`: it can yield borrowed
  slices, `Cow`, or UTF-16 tokens holding unpaired surrogates, none of which fit
  in the `Vec<String>` the core trait is fixed to.
- **You want a concrete stemmer.** Use `verbora-stemmers`; this crate defines
  only the shared `Stemmer` contract.
- **You want `dyn` dispatch.** Four of the six traits are not dyn-compatible —
  see [`dyn` compatibility](#dyn-compatibility).

## Quick example

```rust
use verbora_core::Tokenizer;

struct SpaceTokenizer;

// tokenize_into is the only method you must write.
impl Tokenizer for SpaceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split(' ').map(str::to_owned));
    }
}

fn main() {
    let t = SpaceTokenizer;

    // Provided: allocates a fresh Vec, then delegates to tokenize_into.
    assert_eq!(t.tokenize("the quick fox"), ["the", "quick", "fox"]);

    // Required: APPENDS into a buffer you own; you call clear().
    let mut buf = vec!["stale".to_owned()];
    t.tokenize_into("a b", &mut buf);
    assert_eq!(buf, ["stale", "a", "b"]);

    // Provided: one fresh Vec per document, no reuse.
    assert_eq!(t.tokenize_batch(&["a b", "c"]), vec![vec!["a", "b"], vec!["c"]]);
}
```

## The six traits

| Trait | Required | Provided | Output | `dyn` | Implementors |
|---|---|---|---|:--:|:--:|
| `Tokenizer` | `tokenize_into` | `tokenize`, `tokenize_batch` | `Vec<String>` | ❌ | 22 |
| `BorrowingTokenizer` | `tokenize_borrowed_into` | `tokenize_borrowed` | `Vec<&'a str>` | ❌ | 14 |
| `Stemmer` | `stem` | `stem_into`, `stem_batch` | `Cow<'a, str>` | ❌ | **0** |
| `Phonetic` | `process` | `compare` | `String` | ✅ | 3 |
| `DoubleKeyPhonetic` | `process_double` | — | `(String, String)` | ✅ | 1 |
| `StringMetric` | `IS_SIMILARITY`, `measure` | — | `f64` | ❌ | 5 |

```rust  ignore
pub trait Tokenizer {
    fn tokenize(&self, text: &str) -> Vec<String>;                                  // provided
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);                     // required
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>>;       // provided
}

pub trait BorrowingTokenizer: Tokenizer {
    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str>;                 // provided
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>);    // required
}

pub trait Stemmer {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str>;                             // required
    fn stem_into(&self, token: &str, out: &mut String);                             // provided
    fn stem_batch<S: AsRef<str>>(&self, tokens: &[S]) -> Vec<String>;               // provided
}

pub trait Phonetic {
    fn process(&self, token: &str) -> String;                                       // required
    fn compare(&self, a: &str, b: &str) -> bool;                                    // provided
}

pub trait DoubleKeyPhonetic {
    fn process_double(&self, token: &str) -> (String, String);                      // required
}

pub trait StringMetric {
    const IS_SIMILARITY: bool;                                                      // required
    fn measure(&self, a: &str, b: &str) -> f64;                                     // required
}
```

Which one to pick: tokens that are always substrings of the input →
`BorrowingTokenizer` (zero-copy); tokens that may be rewritten →
`Tokenizer`; one sound-alike key → `Phonetic`, primary + alternate →
`DoubleKeyPhonetic`; a closeness score → `StringMetric`.

<div class="callout callout-warn">
<strong>Careful — the two <code>_into</code> methods disagree.</strong>
<code>Stemmer::stem_into</code> <strong>clears</strong> <code>out</code> before writing.
<code>Tokenizer::tokenize_into</code> and <code>BorrowingTokenizer::tokenize_borrowed_into</code>
<strong>append</strong> and never clear. Assume the wrong one and the failure is silent:
believing <code>tokenize_into</code> clears leaves every previous document's tokens
in the buffer; believing <code>stem_into</code> appends gives you only the last stem.
Reuse one buffer per operation and call <code>clear()</code> yourself on the
tokenizer's — see <a href="../performance/buffer-reuse">Buffer reuse</a>.
</div>

<div class="callout callout-warn">
<strong>Careful — <code>tokenize_batch</code> and <code>stem_batch</code> reuse nothing.</strong>
<code>tokenize_batch</code> maps <code>tokenize</code> over the slice, allocating a fresh
<code>Vec</code> per document; <code>stem_batch</code> calls <code>.into_owned()</code>, costing
one <code>String</code> per token even for words that are their own stem. Both are
sequential and no workspace type overrides them. Batch for convenience, not for
speed.
</div>

**Not every tokenizer can borrow.** `BorrowingTokenizer` returns slices of the
*input*, so a tokenizer that rewrites text before splitting has nothing to hand
back:

| Tokenizer | Why it cannot borrow |
|---|---|
| `AggressiveTokenizerNo`, `AggressiveTokenizerSv` | strip diacritics before splitting |
| `AggressiveTokenizerHi` | deletes punctuation, producing non-contiguous tokens |
| `CaseTokenizer`, `TreebankWordTokenizer`, `TokenizerJa` | cut at UTF-16 code-unit boundaries, so a token can be an unpaired surrogate |
| `SentenceTokenizer` | substitutes placeholders, rewriting the text wholesale |

A case-folding tokenizer fails the same way. If you need that, implement
`Tokenizer` only, or return `Cow<'a, str>` from your own iterator the way
`verbora-tokenizers` does with its `Tokenize` trait.

**Metric conventions are not normalised.** Some metrics count distance, others
similarity; `IS_SIMILARITY` records which. It is an associated const, so a
generic ranking branches on it at monomorphisation, not at runtime.

| Type | `IS_SIMILARITY` | Range | Sentinel values |
|---|:--:|---|---|
| `Levenshtein` | `false` | `0..` | — |
| `DamerauLevenshtein` | `false` | `0..` | — |
| `JaroWinkler` | `true` | `0..=1` | — |
| `Dice` | `true` | `0..=1` | `NaN` |
| `Hamming` | `false` | `-1`, `0..` | `-1.0` means incomparable lengths |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>IS_SIMILARITY</code> tells you the direction but not the
sentinels. <code>Hamming::measure</code> returns <code>-1.0</code> when the two strings differ in
UTF-16 length — smaller than every real distance, so a "lowest wins" ranking
picks the <em>incomparable</em> pair as the best match. <code>Dice</code> can return
<code>NaN</code>, which loses every comparison and is silently dropped rather than
ranked. Filter both before ranking; see <a href="./distance">Distance metrics</a>.
</div>

```rust
use std::borrow::Cow;
use verbora_core::{Stemmer, StringMetric};
use verbora_distance::{JaroWinkler, Levenshtein};

struct StripS;
impl Stemmer for StripS {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(token.strip_suffix('s').unwrap_or(token))
    }
}

/// Closest candidate, in whichever direction `metric` counts.
fn best<M: StringMetric>(metric: &M, query: &str, candidates: &[&str]) -> usize {
    let mut best = 0;
    for (i, c) in candidates.iter().enumerate() {
        let score = metric.measure(query, c);
        let incumbent = metric.measure(query, candidates[best]);
        if score.is_nan() {
            continue;
        }
        let better = if M::IS_SIMILARITY { score > incumbent } else { score < incumbent };
        if better {
            best = i;
        }
    }
    best
}

fn main() {
    let mut out = String::from("stale");
    StripS.stem_into("cats", &mut out);
    assert_eq!(out, "cat"); // CLEARED first — unlike tokenize_into

    let candidates = ["kitten", "sitting", "mitten"];
    // Same answer, opposite comparison.
    assert_eq!(best(&Levenshtein::default(), "mitten", &candidates), 2);
    assert_eq!(best(&JaroWinkler::default(), "mitten", &candidates), 2);
}
```

## Who implements what

| Trait | Implementing types | Crate |
|---|---|---|
| `Tokenizer` | the 16 `AggressiveTokenizer*` types, `CaseTokenizer`, `TreebankWordTokenizer`, `TokenizerJa`, `SentenceTokenizer` | `verbora-tokenizers` |
| `Tokenizer` | `WordTokenizer`, `FnTokenizer<F>` | `verbora-ngrams` |
| `BorrowingTokenizer` | the 13 character-class aggressive tokenizers — every `AggressiveTokenizer*` **except** `…No`, `…Sv`, `…Hi` | `verbora-tokenizers` |
| `BorrowingTokenizer` | `WordTokenizer` | `verbora-ngrams` |
| `Stemmer` | *none* | — |
| `Phonetic` | `Metaphone`, `SoundEx`, `SoundExDM` | `verbora-phonetics` |
| `DoubleKeyPhonetic` | `DoubleMetaphone` | `verbora-phonetics` |
| `StringMetric` | `Levenshtein`, `DamerauLevenshtein`, `JaroWinkler`, `Dice`, `Hamming` | `verbora-distance` |

`DoubleMetaphone` implements `DoubleKeyPhonetic` and **not** `Phonetic`: its
`process` returns `(String, String)`, which does not fit the single-key
signature. `RegexpTokenizer`, `verbora_tokenizers::WordTokenizer`,
`WordPunctTokenizer` and `OrthographyTokenizer` implement **neither** tokenizer
trait: their pattern-matching mode can produce "no match at all", which neither
signature expresses, so they wrap the same shape in `Option`.

<div class="callout callout-warn">
<strong>Careful — two different <code>WordTokenizer</code>s.</strong>
<code>verbora_ngrams::WordTokenizer</code> implements <code>Tokenizer</code> and
<code>BorrowingTokenizer</code>; <code>verbora_tokenizers::WordTokenizer</code> implements
neither. Unrelated types, shared name — import the one you mean by its full path.
</div>

## `dyn` compatibility

| Trait | `dyn`-compatible | Why not |
|---|:--:|---|
| `Tokenizer` | ❌ | `tokenize_batch<S: AsRef<str>>` is a generic method |
| `BorrowingTokenizer` | ❌ | inherits the problem from its `Tokenizer` supertrait |
| `Stemmer` | ❌ | `stem_batch<S: AsRef<str>>` is a generic method |
| `Phonetic` | ✅ | — |
| `DoubleKeyPhonetic` | ✅ | — |
| `StringMetric` | ❌ | `IS_SIMILARITY` is an associated const |

To store heterogeneous tokenizers at runtime, define your own object-safe
projection and blanket-implement it — `verbora_ngrams::NGramTokenizer` already
does, and accepts any `verbora_core::Tokenizer`:

```rust
use verbora_core::Phonetic;
use verbora_ngrams::{NGramTokenizer, WordTokenizer};
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    // Phonetic is dyn-compatible.
    let encoders: Vec<Box<dyn Phonetic>> =
        vec![Box::new(Metaphone::new()), Box::new(SoundEx::new())];
    assert!(encoders[0].compare("Smith", "Smyth"));

    // Tokenizer is not, so go through an object-safe projection.
    let boxed: Box<dyn NGramTokenizer> = Box::new(WordTokenizer);
    assert_eq!(boxed.tokenize_text("a b"), ["a", "b"]);
}
```

## `trim_edge_empties`

```rust  ignore
pub fn trim_edge_empties<T: AsRef<str>>(tokens: &mut Vec<T>)
```

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

Removes empty strings from the **two ends** of a token list and leaves interior
empties exactly where they are — in place, no allocation. The asymmetry is
load-bearing: `SentenceTokenizer` relies on `"   "` tokenizing to `[""]` rather
than `[]`, which a generalised "remove all empties" would delete.
`verbora-tokenizers` re-exports this function rather than defining its own.

```rust
use verbora_core::trim_edge_empties;

fn main() {
    let mut v = vec!["", "", "a", "", "b", "", ""];
    trim_edge_empties(&mut v);
    assert_eq!(v, ["a", "", "b"]); // interior empty survives

    let mut all_empty = vec!["", ""];
    trim_edge_empties(&mut all_empty);
    assert!(all_empty.is_empty());
}
```

## `Token` — the stemming work-buffer

<span class="badge badge-utf16">UTF-16</span>

The public mutable word buffer shared with the stemming surface: the word being
stemmed, the alphabet's vowel set, and the named regions (`R1`, `R2`, `RV`)
Snowball rules use. It stores `Vec<char>` for O(1) character-position indexing,
plus the original input untouched by later rules. Derives `Debug`, `Clone`,
`PartialEq`, `Eq` — equality compares all fields, so two tokens with the same
working string are unequal if built from different inputs — and `Display`,
which writes the working string.

| Method | Behaviour |
|---|---|
| `new(&str) -> Self` | Collects into `chars`, clones into `original`. Two allocations. |
| `using_vowels(self, &str) -> Self` | Sets the vowel set, returns `self`. `#[must_use]`. |
| `as_string(&self) -> String` | **Allocates a fresh `String` on every call.** |
| `chars(&self) -> &[char]` | Free; the working string as a slice. |
| `original(&self) -> &str` | The input as first supplied. |
| `len()` / `is_empty()` | Length of `chars`, in characters. |
| `set_string(&mut self, &str)` | Refills `chars`. Does **not** touch `original`. |
| `mark_region(&mut self, region, index)` | Updates in place if the name exists, else pushes. Chainable. |
| `mark_region_with(&mut self, region, f)` | Computes the index from `&self`, then marks. |
| `region(&self, region) -> usize` | Linear scan; **0** for an unmarked region. |
| `has_vowel_at_index(&self, i) -> bool` | Out-of-range is `false`. |
| `next_vowel_index(&self, start)` / `next_consonant_index` | First match at or after `start`, or `len()`. |
| `has_suffix(&self, suffix) -> bool` | Character-based, case-sensitive. An **empty suffix matches only an empty token**. |
| `has_suffix_in_region(&self, suffix, region)` | `has_suffix` **and** the suffix starts at or after the region. |
| `replace_suffix_in_region(&mut self, suffixes, replacement, region)` | **First** matching suffix wins; the rest are not tried. |
| `replace_all(&mut self, find, replace)` | Replaces every occurrence. An empty needle is a no-op. |

Because `replace_suffix_in_region` short-circuits, the order of the suffix list
is part of the rule: `&["ing", "ning"]` on `"running"` gives `"runn"`, while
`&["ning", "ing"]` gives `"run"`.

```rust
use verbora_core::Token;

fn main() {
    let mut t = Token::new("nationals").using_vowels("aeiouy");

    // R1 = the position after the first vowel-then-consonant pair.
    t.mark_region_with("R1", |t| t.next_consonant_index(t.next_vowel_index(0)) + 1);
    assert_eq!(t.region("R1"), 3);

    // "als" starts at index 6, inside R1, so the rule fires.
    t.replace_suffix_in_region(&["als"], "", "R1");
    assert_eq!(t.as_string(), "nation");
    assert_eq!(t.original(), "nationals");
    assert_eq!(t.region("R2"), 0); // an unmarked region starts at 0

    assert!(!Token::new("word").has_suffix(""));
    assert!(Token::new("").has_suffix(""));
}
```

<div class="callout callout-note">
<strong>Note.</strong> <code>Token</code> is re-exported by <code>verbora-stemmers</code>.
Individual Snowball implementations may use their own UTF-16-precise internal
representation; consult their rustdoc before depending on positional behaviour
for non-BMP input.
</div>

## Stop words

`StopWords` is an **ordered** list with O(1) membership testing: insertion order
preserved in a `Vec`, lookups through a `HashSet`, so every word is stored twice.
Derives `Debug`, `Clone`, `Default`, and implements `FromIterator<String>`.

| Method | Behaviour | Cost |
|---|---|---|
| `new()` | Empty list. | No allocation |
| `english()` | The default English list — `DEFAULT_EN`, 170 entries. | 170 `String`s + a `HashSet` |
| `from_iter_of(words)` | From any `IntoIterator<Item: Into<String>>`, order preserved. | One `String` per input |
| `contains(word)` | Hash lookup. **Case-sensitive.** | O(1) |
| `words()` | `&[String]` in insertion order. | Free |
| `len()` / `is_empty()` | Read from the ordered view, so duplicates count. | O(1) |
| `add(word)` / `add_all(words)` | Pushes **unconditionally** — a duplicate appears twice. | O(1) amortised |
| `remove(word)` / `remove_all(words)` | Removes the **first** occurrence only. | O(n) |

`DEFAULT_EN` is a `&'static [&'static str]` of 170 entries in a fixed, observable
order: 132 words, then `a`–`z`, then `$`, the digits `1`–`9` and `0`, and `_`.

### The process-global list

<span class="badge badge-global">GLOBAL STATE</span>

```rust  ignore
pub fn is_default_stopword(word: &str) -> bool;
pub fn add_global_stopword(word: impl Into<String>);
pub fn add_global_stopwords<I, S>(words: I);
pub fn remove_global_stopword(word: &str);
pub fn remove_global_stopwords<'a, I>(words: I);
pub fn global_stopwords() -> Vec<String>;
pub fn reset_global_stopwords();
```

These mutate one process-wide list, and every call site that reads it observes
the change. It is thread-safe: a `LazyLock<RwLock<StopWords>>` guarded by an
`AtomicBool` recording whether it has ever been mutated. Until the first
mutation, `is_default_stopword` binary-searches a sorted copy of `DEFAULT_EN`
with no lock taken; after any mutation, both readers take the `RwLock`.
`reset_global_stopwords()` exists so tests can isolate themselves.

```rust
use verbora_core::StopWords;
use verbora_core::stopwords::{
    add_global_stopword, is_default_stopword, remove_global_stopword, reset_global_stopwords,
};

fn main() {
    let mut stops = StopWords::english();
    assert_eq!(stops.len(), 170);
    assert!(stops.contains("the"));
    assert!(!stops.contains("The")); // case-sensitive; the list is lowercase

    // `add` pushes unconditionally; `remove` splices the first match only.
    stops.add("verbora");
    stops.add("verbora");
    stops.remove("verbora");
    assert!(stops.contains("verbora"));

    // The global is a separate, process-wide list.
    add_global_stopword("verbora");
    assert!(is_default_stopword("verbora"));
    remove_global_stopword("the");
    assert!(!is_default_stopword("the"));
    reset_global_stopwords();
    assert!(is_default_stopword("the"));
}
```

<div class="callout callout-warn">
<strong>Prefer an explicit <code>&amp;StopWords</code>.</strong> The global is
process-wide: a library that calls <code>add_global_stopword</code> changes what
every other caller in the binary observes, and tests that touch it must serialise
against each other. Every consumer in this workspace offers a sibling that takes
a list explicitly — <code>verbora_phonetics::phoneticize_tokens</code> reads the
global, <code>phoneticize_tokens_with</code> takes a <code>&amp;StopWords</code>.
Note also that <code>DEFAULT_EN</code> is entirely lowercase and lookups compare raw
strings, so <code>"The"</code> is <strong>not</strong> a stop word; lower-case your
tokens yourself if you want case-insensitive filtering.
</div>

## `whitespace` — Verbora's `\s` semantics

```rust  ignore
pub const fn is_whitespace(c: char) -> bool;
pub fn collapse_whitespace(s: &str) -> Cow<'_, str>;
```

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>
<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

`is_whitespace` is not `char::is_whitespace`. Two code points differ, in opposite
directions, and **both** are reachable from ordinary text:

| Character | `is_whitespace` | Rust `char::is_whitespace` | Where it shows up |
|---|:--:|:--:|---|
| `U+0085` NEXT LINE | ❌ | ✅ | text transcoded from EBCDIC and Latin-1-adjacent encodings |
| `U+FEFF` ZERO WIDTH NO-BREAK SPACE (BOM) | ✅ | ❌ | the start of files saved by much Windows tooling |

The full matched set: `U+0009`–`U+000D`, `U+0020`, `U+00A0`, `U+1680`,
`U+2000`–`U+200A`, `U+2028`, `U+2029`, `U+202F`, `U+205F`, `U+3000`, `U+FEFF`.
`U+200B` ZERO WIDTH SPACE is **not** in it. `is_whitespace` is a `const fn`,
`#[inline]`, a single `matches!` over character ranges. Any routine that splits
or trims on `\s` should use it rather than `char::is_whitespace`, or it will
tokenize text containing either character differently.

`collapse_whitespace` collapses every run of matching characters to a single
ASCII space and trims both ends, with no regex engine involved. It returns
`Cow::Borrowed` when the input contains **no** whitespace at all; otherwise it
allocates one `String` of `s.len()` capacity. The borrow test is "contains no
whitespace", not "needs no change" — an already-normalised `"a b"` still takes
the owned path.

```rust
use std::borrow::Cow;
use verbora_core::{collapse_whitespace, is_whitespace};

fn main() {
    // U+FEFF is whitespace here, not to Rust's char::is_whitespace.
    assert!(is_whitespace('\u{FEFF}'));
    assert!(!'\u{FEFF}'.is_whitespace());

    // U+0085 is the reverse.
    assert!(!is_whitespace('\u{0085}'));
    assert!('\u{0085}'.is_whitespace());

    assert_eq!(collapse_whitespace("  a \t\n b  "), "a b");
    assert!(matches!(collapse_whitespace("oneword"), Cow::Borrowed(_)));
    assert!(matches!(collapse_whitespace("a b"), Cow::Owned(_)));
}
```

## Cost and allocation

| Item | Complexity | Allocations |
|---|---|---|
| `Tokenizer::tokenize` | O(n) | One `Vec` from zero capacity + the impl's per-token `String`s |
| `Tokenizer::tokenize_into` | O(n) | Only what the impl pushes; amortises to zero across a reused buffer |
| `Tokenizer::tokenize_batch` | O(total) | One exactly-sized outer `Vec` + one inner `Vec` per document; no reuse |
| `BorrowingTokenizer::tokenize_borrowed[_into]` | O(n) | One `Vec` of `&str`; nothing once `out` has capacity |
| `Stemmer::stem` / `stem_into` / `stem_batch` | impl | Nothing on the `Cow::Borrowed` path / a copy into `out` / one `String` per token always |
| `Phonetic::process` / `compare` | impl | One `String` / two `String`s |
| `trim_edge_empties` | O(trailing empties) + one O(n) drain | None — in place |
| `is_whitespace` | O(1), `const fn`, inlined | None |
| `collapse_whitespace` | Two O(n) scans (one on the borrowed path) | None when the input has no whitespace; otherwise one `String` |
| `Token::new` | O(n) | Two: `Vec<char>` and `String` |
| `Token::as_string` | O(n) | One `String`, **every call** — use `chars()` in a rule chain |
| `Token::replace_all` | O(n) | One `String` per call, **even when nothing matches** |
| `Token::replace_suffix_in_region` | O(n) | None unless the replacement grows past capacity |
| `StopWords::contains` / `remove` | O(1) hash / O(n) | None |
| `is_default_stopword` | O(log 170) binary search, no lock — or an `RwLock` read + hash after any mutation | None |
| `global_stopwords()` | O(170) | 170 `String`s + one `Vec`, every call |

See [Allocation](../performance/allocation.md),
[Buffer reuse](../performance/buffer-reuse.md), and
[Benchmarks](../benchmarks/index.md) for measured timings.

## Parallelism

`verbora-core` ships no `par_*` function — it is shared traits, not an algorithm;
thirteen concrete crates do, behind an opt-in `parallel` feature (see
[Parallelism](../performance/parallelism.md)). Every implementation in the
workspace takes `&self`, holds no per-call mutable state, and is `Send + Sync`,
so a `rayon` `par_iter` over your documents is sound — but it gives up buffer
reuse unless each worker gets a thread-local buffer and calls `tokenize_into`.
The process-global stop-word list is shared across threads: reads are lock-free
until the first mutation, after which one thread's write changes what every other
thread observes.

## Cargo features

`verbora-core` declares one Cargo feature, `serde` (off by default), which pulls
in the optional `serde` dependency.

<div class="callout callout-warn">
<strong>Careful — the feature gates no code today.</strong> Neither <code>Token</code> nor
<code>StopWords</code> derives <code>Serialize</code> or <code>Deserialize</code>, under any
configuration. Enabling it compiles <code>serde</code> into your graph and changes no
Verbora API; the name is reserved for when those derives land.
</div>

Other crates expose opt-in parallel and language-detection features — see
[Cargo features](../getting-started/cargo-features.md).

## Unicode and language notes

- **`Token` indexes by Unicode scalar value**, exact across the Basic
  Multilingual Plane; it diverges only for astral-plane characters, which no
  Snowball stemming rule matches. `has_suffix` is character-based and
  case-sensitive: `"époque"` ends with `"que"` but not `"Que"`, and the accented
  character is one position.
- **`StopWords` comparisons are byte-exact.** No case folding, no Unicode
  normalisation: `"the"` matches; `"The"` and `"the\u{0301}"` do not.
  `DEFAULT_EN` is English only.

## Common mistakes

- **Assuming `tokenize_into` clears.** It appends; `stem_into` clears. Writing
  the two loops symmetrically produces wrong output in one of them.
- **Batching for speed.** `tokenize_batch` and `stem_batch` reuse nothing; write
  the `tokenize_into` + `clear()` loop instead.
- **Comparing `StringMetric` scores without checking `IS_SIMILARITY`.** A ranking
  hard-coding "higher is better" silently returns the *worst* Levenshtein match —
  and even with the direction right, `Hamming`'s `-1.0` and `Dice`'s `NaN` need
  filtering first.
- **Reaching for `dyn Tokenizer`.** It does not compile. Use a generic parameter,
  or an object-safe projection like `verbora_ngrams::NGramTokenizer`.
- **Expecting `"The"` to be filtered as a stop word.** The list is lowercase and
  lookups are exact.
- **Calling `add_global_stopword` from library code.** It changes behaviour for
  the whole process. Take a `&StopWords` parameter instead.
- **Using `char::is_whitespace` instead of `is_whitespace`.** `U+FEFF` and
  `U+0085` disagree, and it shows up as a tokenization difference on real input.
- **Confusing the two `WordTokenizer`s.** `verbora_ngrams::WordTokenizer`
  implements both tokenizer traits; `verbora_tokenizers::WordTokenizer` neither.
- **Calling `Token::as_string()` inside a rule chain.** It allocates every time;
  use `chars()` and convert once at the end.

## Related

- [Tokenizers](./tokenizers.md) — the concrete implementations, and the richer
  lazy `Tokenize` trait they are actually built on.
- [Distance metrics](./distance.md), [Phonetics](./phonetics.md),
  [Stemmers](./stemmers.md) — the concrete implementations of the traits above.
- [Choosing an API](../choosing/index.md) and
  [API shapes](../choosing/api-shapes.md) — the `x` / `x_into` / `x_batch`
  convention this crate establishes.
- [Allocation](../performance/allocation.md),
  [Buffer reuse](../performance/buffer-reuse.md),
  [Zero-copy](../performance/zero-copy.md), [Recipes](../recipes/index.md).

## API reference

Free functions: `trim_edge_empties`, `is_whitespace`, `collapse_whitespace` at
the crate root; `is_default_stopword`, `add_global_stopword[s]`,
`remove_global_stopword[s]`, `global_stopwords`, `reset_global_stopwords` under
`verbora_core::stopwords::`.

Types: `Token` (`token::Token`) and `StopWords` (`stopwords::StopWords`), both
re-exported at the crate root; the static `stopwords::DEFAULT_EN`. Trait
signatures are listed under [The six traits](#the-six-traits) above.

```bash
cargo doc -p verbora-core --no-deps --open
```
