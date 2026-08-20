# Core vocabulary (`verbora-core`)

`verbora-core` is the crate every other Verbora crate is written against. It
contains no algorithm — two things live here and nothing else does: the **five
traits** more than one crate needs to agree on, and the **stop-word lists**,
which are shared data rather than shared behaviour. It depends on no other
`verbora-*` crate, which keeps the crate graph acyclic and lets a crate that
needs only the shared vocabulary depend on it without pulling in data assets it
does not need.

The crate root is the entire public surface: every module is private and
everything public is re-exported there, so there is exactly one path to each
item.

<div class="callout callout-spec">
<strong>Specification status.</strong> Behaviour is pinned by <strong>16</strong>
in-crate unit tests, plus indirect coverage from every crate that depends on it.
</div>

## When to use it

- You are writing code generic over "any tokenizer", "any stemmer" or "any
  phonetic encoder" and want it to work with every implementation in the
  workspace.
- You are implementing your own tokenizer, stemmer or phonetic encoder and want
  the downstream crates to accept it.
- You need a stop-word list for one of sixteen languages, or Verbora's
  process-wide mutable stop-word state.

## When not to use it

- **You just want to tokenize a string.** Call the concrete tokenizer's own
  method — see [Tokenizers](./tokenizers.md). Every tokenizer in
  `verbora-tokenizers` implements `BorrowingTokenizer`, whose lazy
  `tokens` iterator yields slices of your input and allocates nothing, rather
  than the `Vec<String>` `Tokenizer` is fixed to.
- **You want a concrete stemmer.** Use `verbora-stemmers`; this crate defines
  only the shared `Stemmer` contract.
- **You want `dyn` dispatch.** Three of the five traits are not dyn-compatible —
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

## The five traits

| Trait | Required | Provided | Output | `dyn` | Implementors |
|---|---|---|---|:--:|:--:|
| `Tokenizer` | `tokenize_into` | `tokenize`, `tokenize_batch` | `Vec<String>` | ❌ | 3 |
| `BorrowingTokenizer` | `tokens` | `tokenize_borrowed`, `tokenize_borrowed_into` | `impl Iterator<Item = &'a str>` | ❌ | 3 |
| `Stemmer` | `stem` | `stem_into`, `stem_batch` | `Cow<'a, str>` | ❌ | 16 |
| `Phonetic` | `process` | `compare` | `String` | ✅ | 10 |
| `DoubleKeyPhonetic` | `process_double` | — | `(String, Option<String>)` | ✅ | 1 |

```rust  ignore
pub trait Tokenizer {
    fn tokenize(&self, text: &str) -> Vec<String>;                                  // provided
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);                     // required
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>>;       // provided
}

pub trait BorrowingTokenizer: Tokenizer {
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str>;           // required
    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str>;                 // provided
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>);    // provided
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
    fn process_double(&self, token: &str) -> (String, Option<String>);              // required
}
```

Which one to pick: tokens that are always substrings of the input →
`BorrowingTokenizer` (zero-copy); tokens that may be rewritten →
`Tokenizer`; a word reduced to its stem → `Stemmer`; one sound-alike key →
`Phonetic`, primary + alternate → `DoubleKeyPhonetic`.

There is deliberately no metric trait here. Distance and similarity metrics are
free functions over two strings, and the direction convention is recorded by the
name of the function you call rather than by an associated const — see
[Distance metrics](./distance.md#direction-and-range-differ-per-metric).

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

**Not every tokenizer can borrow.** `BorrowingTokenizer` yields slices of the
*input*, so a tokenizer that rewrites text before splitting has nothing to hand
back — a case-folding splitter, a transliterating splitter, or one that deletes
punctuation from inside a token. Implement `Tokenizer` only for those, and put
the rewrite in a name that says so; every tokenizer Verbora ships is a pure
boundary scanner, which is why all three implement both traits.

```rust
use std::borrow::Cow;
use verbora_core::{Stemmer, Tokenizer};

struct StripS;
impl Stemmer for StripS {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(token.strip_suffix('s').unwrap_or(token))
    }
}

struct SpaceTokenizer;
impl Tokenizer for SpaceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split(' ').map(str::to_owned));
    }
}

fn main() {
    let mut out = String::from("stale");
    StripS.stem_into("cats", &mut out);
    assert_eq!(out, "cat"); // CLEARED first

    let mut tokens = vec!["stale".to_owned()];
    SpaceTokenizer.tokenize_into("a b", &mut tokens);
    assert_eq!(tokens, ["stale", "a", "b"]); // APPENDED — the opposite rule

    // The provided methods are written in terms of the required ones.
    assert_eq!(StripS.stem_batch(&["cats", "dog"]), ["cat", "dog"]);
}
```

## Who implements what

| Trait | Implementing types | Crate |
|---|---|---|
| `Tokenizer` | `WordTokenizer`, `SegmentTokenizer`, `SentenceTokenizer` | `verbora-tokenizers` |
| `BorrowingTokenizer` | the same three | `verbora-tokenizers` |
| `Stemmer` | `PorterStemmer`, `LancasterStemmer`, `CarryStemmerFr`, `StemmerId`, `StemmerJa`, and the eleven other language Porter variants (`PorterStemmerDe`, `PorterStemmerEs`, `PorterStemmerFa`, `PorterStemmerFr`, `PorterStemmerIt`, `PorterStemmerNl`, `PorterStemmerNo`, `PorterStemmerPt`, `PorterStemmerRu`, `PorterStemmerSv`, `PorterStemmerUk`) | `verbora-stemmers` |
| `Phonetic` | `SoundEx`, `Metaphone`, `DaitchMokotoff`, `Cologne`, `Nysiis`, `Caverphone1`, `Caverphone2`, `Phonex`, `RefinedSoundex`, `MatchRatingApproach` | `verbora-phonetics` |
| `DoubleKeyPhonetic` | `DoubleMetaphone` | `verbora-phonetics` |

`DoubleMetaphone` implements `DoubleKeyPhonetic` and **not** `Phonetic`: its
`process` returns a `DoubleMetaphoneCode` carrying a primary key and an
optional alternate, which does not fit the single-key signature.
`process_double` flattens that into `(String, Option<String>)`.

Two `Phonetic` implementors override the trait's default
`process(a) == process(b)` comparison with the match rule their own
publication specifies: `DaitchMokotoff::compare` is true when the two names
share **any** code, and `MatchRatingApproach::compare` is the published
match-rating decision rather than code equality. Both keep those semantics
behind a `&dyn Phonetic`.

`verbora-tokenizers` re-exports both tokenizer traits from its own crate root, so
`use verbora_tokenizers::{BorrowingTokenizer, Tokenizer, WordTokenizer};` is one
import line rather than two.

## `dyn` compatibility

| Trait | `dyn`-compatible | Why not |
|---|:--:|---|
| `Tokenizer` | ❌ | `tokenize_batch<S: AsRef<str>>` is a generic method |
| `BorrowingTokenizer` | ❌ | inherits the problem from its `Tokenizer` supertrait |
| `Stemmer` | ❌ | `stem_batch<S: AsRef<str>>` is a generic method |
| `Phonetic` | ✅ | — |
| `DoubleKeyPhonetic` | ✅ | — |

To store heterogeneous tokenizers at runtime, define your own object-safe
projection and blanket-implement it. It is four lines:

```rust
use verbora_core::Phonetic;
use verbora_phonetics::{Metaphone, SoundEx};
use verbora_tokenizers::{SegmentTokenizer, Tokenizer, WordTokenizer};

// The dyn-compatible projection: no generic method, so it can go behind `dyn`.
trait AnyTokenizer {
    fn tokenize_text(&self, text: &str) -> Vec<String>;
}
impl<T: Tokenizer + ?Sized> AnyTokenizer for T {
    fn tokenize_text(&self, text: &str) -> Vec<String> {
        self.tokenize(text)
    }
}

fn main() {
    // Phonetic is dyn-compatible as it stands.
    let encoders: Vec<Box<dyn Phonetic>> =
        vec![Box::new(Metaphone::new()), Box::new(SoundEx::new())];
    assert!(encoders[0].compare("Smith", "Smyth"));

    // Tokenizer is not, so go through the projection.
    let boxed: Vec<Box<dyn AnyTokenizer>> =
        vec![Box::new(WordTokenizer), Box::new(SegmentTokenizer)];
    assert_eq!(boxed[0].tokenize_text("a b"), ["a", "b"]);
    assert_eq!(boxed[1].tokenize_text("a b"), ["a", " ", "b"]);
}
```

## Stop words

Two shapes serve two different needs. `StopWordLanguage` is the **shipped
data**: a `Copy` enum over sixteen languages, whose lists are `&'static` and
immutable, so `is_stopword` is a pure function of the data and two calls with
the same argument always agree. `StopWords` is an **owned, mutable list** you
build and pass by reference.

| Language | Code | Entries |
|---|---|---:|
| English | `en` | 168 |
| German | `de` | 620 |
| Spanish | `es` | 70 |
| Persian | `fa` | 26 |
| French | `fr` | 168 |
| Indonesian | `id` | 809 |
| Italian | `it` | 290 |
| Japanese | `ja` | 109 |
| Dutch | `nl` | 143 |
| Norwegian | `no` | 129 |
| Polish | `pl` | 291 |
| Portuguese | `pt` | 117 |
| Russian | `ru` | 137 |
| Swedish | `sv` | 428 |
| Ukrainian | `uk` | 124 |
| Chinese | `zh` | 78 |

`STOPWORD_LANGUAGES` is that list as a slice, English first and the rest in ISO
639-1 code order. `StopWordLanguage::from_code` looks one up **case-sensitively**
— `from_code("en")` is `Some`, `from_code("EN")` is `None` — because a caller
holding a code of unknown casing is the one that knows how to fold it.

| Method | Behaviour | Cost |
|---|---|---|
| `StopWordLanguage::ALL` / `STOPWORD_LANGUAGES` | Every language, in a fixed order. | Free |
| `StopWordLanguage::code()` | The ISO 639-1 code, lower-case. | Free, `const fn` |
| `StopWordLanguage::from_code(code)` | Case-sensitive lookup. | O(16) |
| `StopWordLanguage::stopwords()` | The shipped list, in source order. | Free |
| `StopWordLanguage::is_stopword(word)` | Binary search over a de-duplicated view built on first use. | O(log n) |

The shipped lists are in **source order**, which carries no meaning beyond being
the order the data was compiled in — it is not a frequency ranking. Several
lists repeat an entry, which cannot affect membership, since `is_stopword`
searches a de-duplicated view.

`StopWords` is an **ordered** list with O(1) membership testing: insertion order
preserved in a `Vec`, lookups through a `HashSet`, so every word is stored twice.
Derives `Debug`, `Clone`, `Default`, `PartialEq`, `Eq`, and implements
`FromIterator<S>` for any `S: Into<String>`.

| Method | Behaviour | Cost |
|---|---|---|
| `new()` | Empty list. | No allocation |
| `for_language(lang)` | An **independent** owned copy of a shipped list. | One `String` per entry |
| `from_iter_of(words)` | From any `IntoIterator<Item: Into<String>>`, order preserved. | One `String` per input |
| `contains(word)` | Hash lookup. **Case-sensitive.** | O(1) |
| `words()` | `&[String]` in insertion order, duplicates included. | Free |
| `len()` / `is_empty()` | Read from the ordered view, so duplicates count. | O(1) |
| `add(word)` / `add_all(words)` | Pushes **unconditionally** — a duplicate appears twice. | O(1) amortised |
| `remove(word)` → `bool` / `remove_all(words)` → `usize` | Removes the **first** occurrence of each; reports how many were found. | O(n) |

The copy `for_language` hands back is independent: adding to it changes neither
`StopWordLanguage::stopwords`, nor the process-global list, nor anyone else's
copy.

The English list holds 168 entries in a fixed, observable order: 132 words, then
`a`–`z`, then the digits `1`–`9` and `0`. Single letters and digits are on it
because those are real word tokens; no entry is punctuation, because every
consumer of a stop-word list in this workspace reaches it through a word
tokenizer, whose tokens hold a letter or a digit by definition.

### The process-global list

<span class="badge badge-global">GLOBAL STATE</span>

```rust  ignore
pub fn is_global_stopword(word: &str) -> bool;
pub fn add_global_stopword(word: impl Into<String>);
pub fn add_global_stopwords<I, S>(words: I);
pub fn remove_global_stopword(word: &str) -> bool;
pub fn remove_global_stopwords<'a, I>(words: I) -> usize;
pub fn global_stopwords() -> Vec<String>;
pub fn reset_global_stopwords();
```

The process-global list starts from the shipped English list and is the one
every English stemmer and the phonetics helpers consult. These functions mutate
it, and every call site that reads it observes the change. It is thread-safe: a
`LazyLock<RwLock<StopWords>>` guarded by an `AtomicBool` recording whether it
has ever been mutated. Until the first mutation, `is_global_stopword`
binary-searches the shipped English list's sorted view with no lock taken; after
any mutation, both readers take the `RwLock`. `reset_global_stopwords()` exists
so tests can isolate themselves — it *replaces* the list with the shipped one
rather than emptying it, so a reader that catches the call mid-flight sees
either the list as it was or the shipped list, never a list that is missing.

`is_global_stopword` and `StopWordLanguage::En.is_stopword` answer different
questions and are meant to. The first reports the list as the process has left
it, additions and removals included; the second is a pure function of the
shipped data that never consults the global. Reach for whichever you actually
mean, and never expect the two to agree after a mutation.

```rust
use verbora_core::{
    STOPWORD_LANGUAGES, StopWordLanguage, StopWords, add_global_stopword,
    is_global_stopword, remove_global_stopword, reset_global_stopwords,
};

fn main() {
    assert_eq!(STOPWORD_LANGUAGES.len(), 16);
    assert_eq!(StopWordLanguage::from_code("sv"), Some(StopWordLanguage::Sv));
    assert_eq!(StopWordLanguage::from_code("SV"), None); // case-sensitive

    // The shipped data: a pure function, never affected by the global list.
    assert!(StopWordLanguage::En.is_stopword("the"));
    assert!(!StopWordLanguage::En.is_stopword("The"));
    assert_eq!(StopWordLanguage::En.stopwords().len(), 168);

    // An owned, independent copy you can mutate.
    let mut stops = StopWords::for_language(StopWordLanguage::En);
    assert_eq!(stops.len(), 168);
    assert!(stops.contains("the"));
    assert!(!stops.contains("The")); // case-sensitive; the list is lowercase

    // `add` pushes unconditionally; `remove` splices the first match only.
    stops.add("verbora");
    stops.add("verbora");
    assert!(stops.remove("verbora")); // reports that one was found
    assert!(stops.contains("verbora")); // the second copy is still there
    assert!(!StopWordLanguage::En.is_stopword("verbora")); // shipped data untouched

    // The global is a separate, process-wide list.
    add_global_stopword("verbora");
    assert!(is_global_stopword("verbora"));
    remove_global_stopword("the");
    assert!(!is_global_stopword("the"));
    reset_global_stopwords();
    assert!(is_global_stopword("the"));
}
```

<div class="callout callout-warn">
<strong>Prefer an explicit <code>&amp;StopWords</code>.</strong> The global is
process-wide: a library that calls <code>add_global_stopword</code> changes what
every other caller in the binary observes, and tests that touch it must serialise
against each other. Consumers in this workspace take a list explicitly instead:
<code>verbora_phonetics::phoneticize_tokens</code> and
<code>tokenize_and_phoneticize</code> both require a <code>&amp;StopWords</code>,
and neither offers a variant that reads the global — a key that depends on
whether some other part of the program has mutated process-wide state is not
reproducible. Note also that the default English list is entirely lowercase and
lookups compare raw
strings, so <code>"The"</code> is <strong>not</strong> a stop word; lower-case your
tokens yourself if you want case-insensitive filtering.
</div>

## Cost and allocation

| Item | Complexity | Allocations |
|---|---|---|
| `Tokenizer::tokenize` | O(n) | One `Vec` from zero capacity + the impl's per-token `String`s |
| `Tokenizer::tokenize_into` | O(n) | Only what the impl pushes; amortises to zero across a reused buffer |
| `Tokenizer::tokenize_batch` | O(total) | One exactly-sized outer `Vec` + one inner `Vec` per document; no reuse |
| `BorrowingTokenizer::tokenize_borrowed[_into]` | O(n) | One `Vec` of `&str`; nothing once `out` has capacity |
| `Stemmer::stem` / `stem_into` / `stem_batch` | impl | Nothing on the `Cow::Borrowed` path / a copy into `out` / one `String` per token always |
| `Phonetic::process` / `compare` | impl | One `String` / two `String`s |
| `StopWordLanguage::stopwords` | O(1) | None — a `&'static` slice |
| `StopWordLanguage::is_stopword` | O(log n) binary search | None; the de-duplicated view is built once per language, on first use |
| `StopWords::for_language` | O(n) | One `String` per entry + a `HashSet` |
| `StopWords::contains` / `remove` | O(1) hash / O(n) | None |
| `is_global_stopword` | O(log 168) binary search, no lock — or an `RwLock` read + hash after any mutation | None |
| `global_stopwords()` | O(168) | 168 `String`s + one `Vec`, every call |

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

`verbora-core` declares **no Cargo features**. Its one dependency, `rustc-hash`,
is unconditional: it supplies the hash set behind `StopWords::contains`, which
runs on every token of every document a stemmer filters.

Other crates expose opt-in parallel and language-detection features — see
[Cargo features](../getting-started/cargo-features.md).

## Unicode and language notes

- **Stop-word comparisons are byte-exact.** No case folding, no Unicode
  normalisation, no trimming: `"the"` matches; `"The"` and `"the\u{0301}"` do
  not. This holds for both `StopWords::contains` and
  `StopWordLanguage::is_stopword`, and folding case is the caller's choice
  rather than the function's.
- **`""` is not a stop word in any language.**
- **`StopWordLanguage::from_code` is case-sensitive.** ISO 639-1 codes are
  lower-case, and accepting `"EN"` would be inventing a spelling the standard
  does not define.

## Common mistakes

- **Assuming `tokenize_into` clears.** It appends; `stem_into` clears. Writing
  the two loops symmetrically produces wrong output in one of them.
- **Batching for speed.** `tokenize_batch` and `stem_batch` reuse nothing; write
  the `tokenize_into` + `clear()` loop instead.
- **Looking here for a metric trait.** There is none. Distance and similarity
  metrics are free functions — see [Distance metrics](./distance.md).
- **Reaching for `dyn Tokenizer`.** It does not compile. Use a generic parameter,
  or an object-safe projection of your own, as shown above.
- **Expecting `"The"` to be filtered as a stop word.** The list is lowercase and
  lookups are exact.
- **Calling `add_global_stopword` from library code.** It changes behaviour for
  the whole process. Take a `&StopWords` parameter instead.
- **Expecting `StopWordLanguage::En.is_stopword` to see a global mutation.** It
  never consults the global list. Use `is_global_stopword` when you want the
  list as the process has left it.
- **Removing a word added twice, once.** `add` appends unconditionally, so the
  second copy survives and `contains` still answers `true`. The `bool` `remove`
  returns is what distinguishes a call that did nothing from one that did.

## Related

- [Tokenizers](./tokenizers.md) — the concrete implementations, and the richer
  lazy `tokens()` iterator they are actually built on.
- [Phonetics](./phonetics.md), [Stemmers](./stemmers.md) — the concrete
  implementations of the traits above.
- [Distance metrics](./distance.md) — free functions over two strings, with no
  trait in this crate behind them.
- [Choosing an API](../choosing/index.md) and
  [API shapes](../choosing/api-shapes.md) — the `x` / `x_into` / `x_batch`
  convention this crate establishes.
- [Allocation](../performance/allocation.md),
  [Buffer reuse](../performance/buffer-reuse.md),
  [Zero-copy](../performance/zero-copy.md), [Recipes](../recipes/index.md).

## API reference

The crate root is the entire public surface — every module is private and
everything public is re-exported there, so there is exactly one path to each
item.

Free functions, all of them operating on the process-global English list:
`is_global_stopword`, `add_global_stopword`, `add_global_stopwords`,
`remove_global_stopword`, `remove_global_stopwords`, `global_stopwords`,
`reset_global_stopwords`.

Types: `StopWords`, `StopWordLanguage`, and the `STOPWORD_LANGUAGES` slice.

Traits: `Tokenizer`, `BorrowingTokenizer`, `Stemmer`, `Phonetic` and
`DoubleKeyPhonetic` — signatures under
[The five traits](#the-five-traits) above.

```bash
cargo doc -p verbora-core --no-deps --open
```
