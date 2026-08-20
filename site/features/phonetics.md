# Phonetics

`verbora-phonetics` maps a word to a short **key** so that words which sound
alike collide. Twelve algorithms ship in the crate. Eleven are keyed encoders
documented here — four core encoders (`SoundEx`, `Metaphone`,
`DoubleMetaphone` and `DaitchMokotoff`) plus [seven more](#seven-more-encoders)
covering algorithms the first four do not: `Cologne`, `Nysiis`,
`Caverphone1`/`Caverphone2`, `Phonex`, `RefinedSoundex` and
`MatchRatingApproach`. The twelfth, [`BeiderMorse`](beider-morse.md), returns a
candidate list rather than a key and has its own page.

They are the building block for fuzzy name lookup, deduplication and search
*blocking*: encode every record once, group by key, and only then run an
expensive [string metric](../features/distance.md) inside each group.

<div class="callout callout-spec">
<strong>Specification status.</strong> Every encoder is <strong>total</strong> —
no <code>Result</code>, no panic, on any <code>&amp;str</code> — and every
encoder's <em>declared output shape</em> (Soundex's four characters, Double
Metaphone's four-per-key, Daitch–Mokotoff's six digits, Caverphone's fixed
6 / 10) is <strong>enumerated</strong> over a non-ASCII name corpus and a
pathological-Unicode set rather than sampled. The Daitch–Mokotoff coding
chart's 124 rules are each proved reachable through their own witness, so the
embedded rule table is normative rather than decorative.
</div>

## When to use it

- **Blocking a fuzzy-match pipeline.** Comparing every pair of a million records
  with Levenshtein is quadratic; bucketing by phonetic key first is linear, and
  only same-bucket pairs need the metric.
- **Name search that tolerates spelling.** `Robert`/`Rupert` share `R163`;
  `phonetics`/`fonetix` share `FNTKS`.
- **You need output that never shifts under you.** Every encoder's output is
  pinned by the crate's regression suite, so an index you built stays valid
  release over release.

## When not to use it

- **Text in a language none of the encoders was designed for.** The core four
  are English (or, for `DaitchMokotoff`, Latin-script Slavic and Yiddish
  surname spellings), and the extension encoders widen that only where a
  published algorithm exists. An encoder reads the alphabet its own publication
  names and skips every other scalar, so text in a script none of them mentions
  yields an empty key — the absence of a key, rather than a meaningless one.
  See [Unicode and language notes](#unicode-and-language-notes).
- **Ranking.** A phonetic key is a yes/no bucket, not a score. For "how similar
  are these two strings" use [distance metrics](../features/distance.md).
- **Whole sentences.** Every encoder takes a *token*. Because the encoders skip
  characters they do not recognise — spaces included — feeding a sentence runs
  its words together into a single key rather than producing one key per word.
  Tokenize first, or use
  [`tokenize_and_phoneticize`](#phoneticize-tokens-and-tokenize-and-phoneticize).
- **As a hash or an identifier.** Keys are lossy by design and short: distinct
  names are *supposed* to collide.

## Quick example

```rust
use verbora_phonetics::{DaitchMokotoff, DoubleMetaphone, Metaphone, SoundEx};

fn main() {
    // One key each; a primary and an optional alternate for Double Metaphone.
    assert_eq!(SoundEx::new().process("Robert"), "R163");
    assert_eq!(SoundEx::new().process("Rupert"), "R163");
    assert_eq!(Metaphone::new().process("phonetics"), "FNTKS");
    assert_eq!(Metaphone::new().process("fonetix"), "FNTKS");

    let smith = DoubleMetaphone::new().process("Smith");
    assert_eq!(smith.primary(), "SM0");
    assert_eq!(smith.alternate(), Some("XMT"));

    // Daitch-Mokotoff follows every reading of an ambiguous cluster.
    assert_eq!(DaitchMokotoff::new().process("AUERBACH"), "097400|097500");
}
```

Every encoder type is zero-sized (`Debug + Clone + Copy + Default + PartialEq +
Eq`, `const fn new()`), holds no state and is trivially `Send + Sync`. Create
them wherever it reads best; there is nothing to cache. (`Phonex` and `Nysiis`
carry their one configuration value and are otherwise the same.)

## The four encoders

| Encoder | Keys | Code shape | Output alphabet | Good for |
|---|:--:|---|---|---|
| `SoundEx` | 1 | 4 characters — the initial, then 3 digits — or none | an `A`–`Z` initial + `0`–`6` | Coarse blocking of English surnames. Cheapest, and the most collisions. |
| `Metaphone` | 1 | letters, unbounded | uppercase `A`–`Z`, plus `0` for the `th` sound | General English words, where you want more precision than four characters buys. |
| `DoubleMetaphone` | **1 or 2** | at most 4 characters *per key* | uppercase letters and `0` | English text containing names of mixed origin. Index both keys; a match on **either** counts. |
| `DaitchMokotoff` | **1 or more** | exactly 6 digits per code | digits only | Slavic, Germanic and Ashkenazi-Jewish surnames. Handles the `SCH`, `CZ`, `TSCH`, `RZ` clusters `SoundEx` flattens, and branches where a cluster is ambiguous. |

<div class="callout callout-note">
<strong>Note.</strong> A code shape is a guarantee, not a default you can
change. There is no length argument on any entry point: each encoder emits the
shape its own publication specifies. Where you need a shorter blocking key,
truncate the code yourself — and see
<a href="phonetic-index"><code>PhoneticIndex</code></a>, which stores codes
inline at their published width.
</div>

Measured throughput is on
[Competitive benchmarks § Phonetics](../benchmarks/competitive.md#phonetics).

## Seven more encoders

Seven further encoders each implement a published algorithm the four above do
not cover. Every one is **total**: no `Err` path and no panic, on any `&str`,
non-ASCII included.

| Encoder | Code shape | Designed for | Good for |
|---|---|---|---|
| `Cologne` | digits, unbounded length | German (Kölner Phonetik, Postel 1969) | German names and words; the German-language analogue of `SoundEx` |
| `Nysiis` | letters, 6 bytes when strict (the default) | US surnames (Taft, 1970) | Higher-precision English surname matching than `SoundEx` |
| `Caverphone1` | exactly 6 characters, `1`-padded | New Zealand English (Hood, 2002) | Historical/electoral-roll name matching, coarser |
| `Caverphone2` | exactly 10 characters, `1`-padded | New Zealand English (Hood, 2004) | Same, with more precision and revised vowel handling |
| `Phonex` | letter + digits, configurable length (default 4) | British surnames (Lait & Randell, 1996) | Soundex-style blocking tuned to reduce false negatives on British names |
| `RefinedSoundex` | letter + digits, unbounded | Spell-checking (Soundex refinement) | Finer buckets than `SoundEx` (ten consonant groups, vowels kept as `0`), plus the standard `difference()` similarity |
| `MatchRatingApproach` | short consonant skeleton | Homophonous personal names (Moore et al., 1977) | Not just an encoder — `compare` is the published name-match *decision* |

All seven expose `process(&str) -> String` and `compare(&str, &str) -> bool`
plus the type-specific extras below, and all seven implement
`verbora_core::Phonetic`, so they slot into the same `&dyn Phonetic` lists,
`phoneticize_tokens` closures and `par_encode_batch` calls as the core
encoders. `MatchRatingApproach` overrides the trait's default key-equality
`compare` with its own match decision, and keeps that semantics behind the
trait object too.

<div class="callout callout-note">
<strong>Note.</strong> The <a href="phonetic-index"><code>PhoneticIndex</code></a>
<em>dictionary index</em> is the one place these seven do not plug in: its
<code>PhoneticEncoder</code> trait, with its compact typed codes, is
implemented for <code>SoundEx</code>, <code>Metaphone</code> and
<code>DoubleMetaphone</code> only.
</div>

```rust
use verbora_phonetics::{
    Caverphone2, Cologne, MatchRatingApproach, Nysiis, Phonex, RefinedSoundex,
};

fn main() {
    assert_eq!(Cologne::new().process("Müller"), "657");
    assert_eq!(Nysiis::new().process("KNUTH"), "NAT");
    assert_eq!(Caverphone2::new().process("Thompson"), "TMPSN11111");
    assert_eq!(Phonex::new().process("Wright"), "R623");
    assert_eq!(RefinedSoundex::new().process("testing"), "T6036084");
    assert_eq!(MatchRatingApproach::new().process("Catherine"), "CTHRN");
}
```

All seven share one performance shape: a single pass over one reused buffer,
static compiled-in rule tables, and one heap allocation per call for the
returned code.

### Cologne

Kölner Phonetik, the German-language analogue of `SoundEx`: umlauts fold
(`Müller` → `657`), the code is never truncated, vowels survive only at the
very front, and `C`, `D`, `T`, `P`, `X` encode differently depending on their
neighbors. A scalar with no rule is skipped without panicking, so text in a
script the rules do not mention yields an empty code.

```rust
use verbora_phonetics::Cologne;

fn main() {
    let cologne = Cologne::new();
    assert_eq!(cologne.process("schmidt"), "862");
    assert_eq!(cologne.process("Breschnew"), "17863");
    assert_eq!(cologne.process("Wikipedia"), "3412");
    assert!(cologne.compare("Meyer", "Mayr"));
    assert!(!cologne.compare("Meyer", "Müller"));
    assert_eq!(cologne.process("東京 123 🙂"), "");
}
```

### Nysiis

The New York State Identification and Intelligence System code, a
higher-precision replacement for `SoundEx` in surname matching. One flag, fixed
at construction: `Nysiis::new()` is **strict** (the default, 6-byte cap),
`with_strict(false)` is unbounded, `is_strict()` reports which you have.

```rust
use verbora_phonetics::Nysiis;

fn main() {
    assert_eq!(Nysiis::new().process("Westerlund"), "WASTAR");
    assert_eq!(Nysiis::with_strict(false).process("Westerlund"), "WASTARLAD");
    assert_eq!(Nysiis::new().process("MACINTOSH"), "MCANT");
    assert_eq!(Nysiis::new().process("o'daniel"), "ODANAL");
    assert!(Nysiis::new().compare("Trueman", "Truman"));
    assert_eq!(Nysiis::new().process("12345"), "");
}
```

### Caverphone

Two revisions, two types, both fixed-width and `1`-padded: `Caverphone1` (2002,
6-character codes) and `Caverphone2` (2004, 10-character codes, revised
handling of `y`, word-final `w`/`r`/`l` and trailing vowels). Prefer
`Caverphone2` unless you need to match an index built with 1.0 codes. The
padding is unconditional, so even an empty token has a full-width code.

```rust
use verbora_phonetics::{Caverphone1, Caverphone2};

fn main() {
    assert_eq!(Caverphone1::new().process("Thompson"), "TMPSN1");
    assert_eq!(Caverphone2::new().process("Thompson"), "TMPSN11111");
    assert_eq!(Caverphone2::new().process("ready"), "RTA1111111");
    assert!(Caverphone2::new().compare("Peter", "Peady"));

    // Fixed width always, so an empty token is all padding.
    assert_eq!(Caverphone1::new().process(""), "111111");
    assert_eq!(Caverphone2::new().process(""), "1111111111");
}
```

### Phonex

A Soundex refinement for British surnames: a preprocessing stage (trailing-`S`
removal, leading-pair rewrites like `KN`→`NN`, first-letter substitutions)
followed by context-sensitive digit rules. Code length is configurable at
construction and defaults to 4.

```rust
use verbora_phonetics::Phonex;

fn main() {
    assert_eq!(Phonex::new().process("Sinatra"), "S536");
    assert_eq!(Phonex::with_max_code_length(6).process("Sinatra"), "S53600");
    assert_eq!(Phonex::new().max_code_length(), 4);
    assert_eq!(Phonex::new().process("KNUTH"), "N300");
    assert!(Phonex::new().compare("Knuth", "Nuth"));

    // A token with nothing to code still fills the configured width.
    assert_eq!(Phonex::new().process("12345"), "0000");
}
```

### Refined Soundex

Splits classic Soundex's six consonant groups into ten, keeps vowels as `0` (so
they *separate* consonant runs instead of vanishing), and never truncates or
pads. It also ships `difference(a, b) -> usize`, the standard companion
similarity measure: the number of positions at which the two codes agree.

```rust
use verbora_phonetics::RefinedSoundex;

fn main() {
    let refined = RefinedSoundex::new();
    assert_eq!(refined.process("jumped"), "J408106");
    assert!(refined.compare("Smith", "Smythe"));
    assert_eq!(refined.difference("Smithers", "Smythers"), 8); // high similarity
    assert_eq!(refined.difference("Margaret", "Andrew"), 1); // low
}
```

### Match Rating Approach

MRA is two things at once, and the second is the point. **`process`** produces
the encoding: uppercase, drop non-initial vowels, collapse doubled consonants
pairwise, and past six bytes keep the first three and last three.
**`compare` is the published match decision, not code equality** — it
short-circuits on raw string equality, rejects encoded lengths differing by 3
or more, blanks out agreeing characters in a left-to-right and a right-to-left
pass, and accepts when the resulting rating clears a minimum that depends on
the combined encoded length.

```rust
use verbora_phonetics::MatchRatingApproach;

fn main() {
    let mra = MatchRatingApproach::new();
    assert_eq!(mra.process("Smith"), "SMTH");
    assert_eq!(mra.process("Franciszek"), "FRNSZK");
    assert!(mra.compare("smith", "smyth"));
    assert!(mra.compare("Franciszek", "Frances"));
    assert!(mra.compare("Burns", "Bourne"));
    assert!(!mra.compare("Karl", "Alessandro"));
}
```

Two names with *equal codes* can still fail `compare`, and two names with
different codes can pass — treat `process` as the index key and `compare` as
the decision, not two views of one operation.

## Daitch-Mokotoff, in detail

Daitch–Mokotoff Soundex (Randy Daitch and Gary Mokotoff, published through the
Jewish genealogical societies in *Avotaynu* from 1985) exists because plain
Soundex garbles the Slavic and Yiddish spellings of Ashkenazi surnames. Its
defining feature is **branching**: where a cluster is phonetically ambiguous —
`CH` as in *chair* or as in *Bach*, Polish `RS`/`RZ`, initial `J` — the encoder
follows every reading and returns every resulting six-digit code. `AUERBACH` is
both `097400` and `097500`, and a genealogical index needs both.

One type carries the whole algorithm, with three entry points:

| Method | Returns | Use it when |
|---|---|---|
| `process(token)` | `String` — every code, joined with `\|` | you want one printable value per name, or a single column in a report |
| `codes(token)` | `Vec<String>` — the codes as separate values | you are indexing, and each code needs its own bucket |
| `compare(a, b)` | `bool` — true when the two names share **any** code | you are matching two names directly |

```rust
use verbora_phonetics::DaitchMokotoff;

fn main() {
    let dm = DaitchMokotoff::new();

    // One code when nothing branches, several when something does.
    assert_eq!(dm.process("GOLDEN"), "583600");
    assert_eq!(dm.process("AUERBACH"), "097400|097500");
    assert_eq!(dm.codes("AUERBACH"), vec!["097400", "097500"]);
    assert_eq!(dm.codes("GOLDEN"), vec!["583600"]);

    // codes(x)[0] is always the code a non-branching walk would produce.
    assert_eq!(dm.codes("Rosochowaciec")[0], "944744");

    // compare is code-set intersection, the published match criterion:
    // "Ceniow" is 467000|567000 and "Tsenyuv" is 467000, so they intersect.
    assert!(dm.compare("Moskowitz", "Moskovitz"));
    assert!(dm.compare("Ceniow", "Tsenyuv"));
    assert!(!dm.compare("Peters", "Peterson"));
}
```

**Every code is exactly six digits**, zero-padded or truncated — including for
a token with nothing to code, which yields the single code `000000` rather than
an empty string. That makes Daitch–Mokotoff the one encoder here whose empty
key is not `""`:

```rust
use verbora_phonetics::DaitchMokotoff;

fn main() {
    let dm = DaitchMokotoff::new();
    assert_eq!(dm.process(""), "000000");
    assert_eq!(dm.process("Mintz"), "664000");

    // Whitespace is removed everywhere, not merely trimmed: the chart is
    // stated over a surname written as one word.
    assert_eq!(dm.process("Ben Aron"), dm.process("BENARON"));
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> <code>codes()</code> can return the <em>same</em>
code twice. Branches are deduplicated during the walk, on the pair
(partial code, last replacement), rather than on the finished codes — so two
different readings that converge on one code both survive, and the list tells
you how many readings a spelling has. If you need a set, deduplicate it
yourself.
</div>

## Choosing the right API

Three independent choices: **which encoder**, **which entry point** on that
encoder, and **how to run it over a token stream**. Every encoder answers
"what is this word's key?" with `process` and "do these two words sound alike?"
with `compare`; the rest of the surface exists for workloads that pair cannot
serve. Full signatures are in the [API reference](#api-reference).

| Shape | Use case | Allocation | Recommendation |
|---|---|---|---|
| `encoder.process(word)` | one word, one key | one `String` | **the default.** Reach for anything else only when this one is measurably in the way. |
| `encoder.compare(a, b)` | "do these sound alike?" | two `String`s | the readable way to ask; it is key equality except where the algorithm publishes its own rule (see below) |
| `encoder.process_into(word, &mut buf)` | encoding a dictionary into a buffer you own | none, once `buf` has grown | you manage `buf`, including clearing it; offered on `SoundEx` and `Metaphone`, whose keys are most often accumulated in bulk |
| [`PhoneticIndex`](phonetic-index) | "which of these ten thousand names sound like this one?" | one build-time structure; queries allocate only the query's key | the only shape that answers a *set* question; a loop of `compare` over a dictionary is O(n) per query |
| `par_encode_batch` (feature `parallel`) | encoding tens of thousands of words at once | one output `Vec` | measure first: a single call costs tens of nanoseconds, so the thread pool only pays for itself in bulk |

### Decision tree — which encoder

| Your case | Encoder |
|---|---|
| English surnames, cheapest possible blocking key | `SoundEx` |
| General English words, one key, better precision | `Metaphone` |
| English text with names of many origins, two keys indexable | `DoubleMetaphone` |
| Slavic / Germanic / Ashkenazi-Jewish surnames, or genealogy needing every reading of an ambiguous cluster | `DaitchMokotoff` |
| German words and names | `Cologne` |
| British surnames | `Phonex` |
| New Zealand English | `Caverphone2` (`Caverphone1` only for a legacy index) |
| NYSIIS-compatible surname codes | `Nysiis` |
| Spell-check-style finer buckets, plus a similarity score | `RefinedSoundex` |
| A name-match *decision* rather than a key | `MatchRatingApproach` |
| The same family name spelled by different countries' conventions | [`BeiderMorse`](beider-morse.md) |

For "which encoder for this *language*", see
[Choosing a phonetic algorithm](#choosing-a-phonetic-algorithm) below.

### Comparison table — encoders

| Encoder | Keys per token | `Phonetic` impl | `DoubleKeyPhonetic` impl | `PhoneticEncoder` impl | `process_into` |
|---|:--:|:--:|:--:|:--:|:--:|
| `SoundEx` | 1 | ✅ | ❌ | ✅ | ✅ |
| `Metaphone` | 1 | ✅ | ❌ | ✅ | ✅ |
| `DoubleMetaphone` | 1 or 2 | ❌ | ✅ | ✅ | ❌ |
| `DaitchMokotoff` | 1 or more | ✅ | ❌ | ❌ | ❌ |
| The other seven | 1 | ✅ | ❌ | ❌ | ❌ |

`DoubleMetaphone` does **not** implement `verbora_core::Phonetic` — it has no
single key to return, and implements `DoubleKeyPhonetic` (`process_double`)
instead. `DaitchMokotoff` *does* implement `Phonetic`, with `process` returning
its `|`-joined codes and `compare` overridden to the published code-set
intersection.

### `process()`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

The default, on every encoder. Eager, always allocates (see
[Allocation behaviour](#allocation-behaviour)), and **never fails**: there is no
`Result` and no panic on any `&str`. A token with no letter the algorithm
recognises yields an empty key — the absence of a key, rather than a value
standing in for one. No input *with* a recognised letter can produce it.

```rust
use verbora_phonetics::{Nysiis, RefinedSoundex, SoundEx};

fn main() {
    // The empty key means "nothing here I can index".
    assert_eq!(SoundEx::new().process("日本語"), "");
    assert_eq!(Nysiis::new().process("12345"), "");
    assert_eq!(RefinedSoundex::new().process("1-2-3"), "");
}
```

`DaitchMokotoff` is the exception worth remembering: its empty key is the
six-digit `000000`, because every code it emits is padded to six digits.

### `process_into(token, &mut buf)`

`SoundEx` and `Metaphone` also append into a buffer you own, which is the
allocation-free way to encode a dictionary in bulk.

`buf` is **never cleared**, so a caller accumulating many codes into one buffer
keeps what is already there — and a caller who wants one code at a time must
clear it between calls. Appending nothing is how these report the empty key,
exactly as `process` returns `""` for the same input.

```rust
use verbora_phonetics::SoundEx;

fn main() {
    let soundex = SoundEx::new();
    let mut buf = String::new();

    // Accumulating: the buffer keeps growing, no per-word allocation.
    soundex.process_into("Robert", &mut buf);
    soundex.process_into("Rupert", &mut buf);
    assert_eq!(buf, "R163R163");

    // One at a time: clear it yourself.
    buf.clear();
    soundex.process_into("Ashcraft", &mut buf);
    assert_eq!(buf, "A261");
}
```

### `compare()` versus comparing two `process()` results

**`compare` does not short-circuit**, on any encoder: it encodes both sides
every call and throws the keys away. For most encoders it is defined as
`self.process(a) == self.process(b)`. Three publish a different rule, and for
those `compare` is the algorithm's own answer rather than a convenience:

| Encoder | What `compare` means |
|---|---|
| `DoubleMetaphone` | the two names share **either** key — which is the entire reason the algorithm produces two |
| `DaitchMokotoff` | the two code **sets intersect** — sharing *a* code is the published match criterion for a branching encoder |
| `MatchRatingApproach` | the published match-rating decision, which is not code equality in either direction |
| everything else | key equality |

| You want | Use |
|---|---|
| A single ad-hoc "do these two sound alike?" test | `compare(a, b)` — it reads better and costs the same |
| The `DoubleMetaphone` "either key matches" rule, or the `DaitchMokotoff` shared-code rule | `compare(a, b)` — reproducing either by hand is easy to get wrong |
| To test one word against many, or many against many | `process` **once per word**, then compare the keys |
| To store the key | `process` — `compare` throws its keys away |

The last two rows matter: `compare` in a nested loop is *O(n²)* encodings,
while encoding each word once and comparing the keys is *n* encodings and
*O(n²)* `&str` comparisons. Past a few dozen words, reach for
[`PhoneticIndex`](phonetic-index) instead of either. See
[Reducing it at the call site](#reducing-it-at-the-call-site).

### `phoneticize_tokens` and `tokenize_and_phoneticize`

Both encode a token stream and drop stop words; they differ only in where the
tokens come from. `phoneticize_tokens` takes them from you, keeping the choice
of tokenizer visible at the call site. `tokenize_and_phoneticize` is the
wrapper that pairs it with `verbora_tokenizers::WordTokenizer`.

Both take a `&StopWords` you own. There is deliberately **no variant that reads
a process-global stop-word list**: a key that depends on whether some other part
of the program has mutated a global is not reproducible, and no publication here
calls for one. Pass `StopWords::new()` — the empty list — to encode every token.

```rust
use verbora_core::{StopWordLanguage, StopWords};
use verbora_phonetics::{Metaphone, phoneticize_tokens, tokenize_and_phoneticize};

fn main() {
    let metaphone = Metaphone::new();

    // You supply the tokens.
    let stops = StopWords::for_language(StopWordLanguage::En);
    let keys = phoneticize_tokens(["the", "quick", "brown", "fox"], &stops, |t| {
        metaphone.process(t)
    });
    assert_eq!(keys, ["KK", "BRN", "FKS"]);

    // Or it tokenizes for you. The empty list keeps every token.
    let keys = tokenize_and_phoneticize("The quick brown fox", &StopWords::new(), |t| {
        metaphone.process(t)
    });
    assert_eq!(keys, ["0", "KK", "BRN", "FKS"]);
}
```

The `T: IntoIterator<Item = &'a str>` bound is the useful part of
`phoneticize_tokens`'s signature: a tokenizer's lazy iterator satisfies it
directly, so there is no intermediate `Vec`. The closure's return type is free,
so a two-key encoder works the same way — pass `|t| dm.process(t)` and get a
`Vec<DoubleMetaphoneCode>`.

Two behaviours worth knowing. **Filtering tests the raw token**, so it is
exactly as case-sensitive as the list you pass: `the` is dropped by a lowercase
list and `The` is not. And **`tokenize_and_phoneticize` cuts at UAX #29 word
boundaries**, so a hyphen or a slash splits a token (`well-known` becomes two
keys) while an apostrophe, a decimal point, a thousands separator and an
underscore do not.

### `par_encode_batch` / `par_encode_double_batch` — parallel batch (feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Behind the `parallel` Cargo feature (never on by default), `par_encode_batch`
fans `Phonetic::process` out across a `rayon` thread pool, and
`par_encode_double_batch` does the same for `DoubleKeyPhonetic::process_double`
(`DoubleMetaphone`). Both call the same encoder the sequential API uses, in the
same order, and allocate one output `Vec` plus exactly what `process` allocates
per token — no extra per-chunk buffering.

```rust  ignore
use verbora_phonetics::{DoubleMetaphone, SoundEx, par_encode_batch, par_encode_double_batch};

// `chunk_size` is a required argument, not a hidden default.
let keys = par_encode_batch(&SoundEx::new(), &["Robert", "Rupert", "phonetics"], 2);
assert_eq!(keys, ["R163", "R163", "P532"]);

// The two-key form keeps the "no alternate" distinction intact.
let keys = par_encode_double_batch(&DoubleMetaphone::new(), &["Smith", "Thompson"], 2);
assert_eq!(
    keys,
    [
        ("SM0".to_owned(), Some("XMT".to_owned())),
        ("TMPS".to_owned(), None),
    ]
);
```

<div class="callout callout-note">
<strong>Note.</strong> The block above needs the <code>parallel</code> feature,
which this site's snippet checker builds without, so it is marked
<code>ignore</code> rather than compiled — every other block on this page
compiles and runs in CI.
</div>

**Why chunked, not one task per word.** One `process` call costs ~42–183
ns/word — the same order of magnitude as a `rayon` task's own scheduling cost,
so one task per word measures as *unpredictable*, ranging from 2× slower than a
sequential loop to several times faster depending on host load.
`DEFAULT_CHUNK_SIZE` (`64`) is a measured starting point tuned on a 32-core
machine against `SoundEx`; passing `1` reproduces the one-task-per-word form.
`chunk_size` is clamped to at least 1, so `0` is legal rather than a panic.

**When to reach for it.** A handful of words, or words arriving one at a time:
call `process` directly. Building an index over tens of thousands of words:
this is the intended use. In between, measure — the crossover moves with core
count and machine load. See [Parallelism](../performance/parallelism.md).

## Choosing a phonetic algorithm

The [decision table above](#decision-tree-—-which-encoder) answers "which
encoder for this use case". This section answers "which encoder for this
*language*".

<div class="callout callout-warn">
<strong>There is no "best" phonetic algorithm.</strong> Only three encoders were
designed for a language other than English — <code>Cologne</code> for German,
<code>DaitchMokotoff</code> for Slavic and Yiddish surname spellings, and
<code>BeiderMorse</code> for a per-language rule corpus covering eighteen.
Everything else is an English-oriented algorithm that reads <code>A</code>–<code>Z</code>
and skips the rest. So the table below reports two things a caller cannot get
from the encoders themselves: <em>which encoder, if any, was actually designed
for this language</em>, and <em>whether a transliteration step has to run
first</em> — never an unqualified claim that an encoder is correct for a
language in the way it is correct for English surnames.
</div>

### Per-language table

Sourced directly from [`verbora-language`](../features/language.md)'s
[`recommend()`](../features/language.md#phonetic-strategy-recommend) — a closed
`match` over all 22 languages that crate has a strategy for, not a separate
opinion maintained on this page. The **basis** column is the load-bearing one:
`Named` means an encoder was written for this language, `Script` means one
merely runs on the script it is written in, and `NoFit` means neither.

| Language | Primary | Alternative(s) | Basis |
|---|---|---|---|
| German | `Cologne` | `DaitchMokotoff`, `BeiderMorse` (`german`), `DoubleMetaphone` | Named |
| English | `DoubleMetaphone` | `Metaphone`, `SoundEx`, `BeiderMorse` (`english`) | Named |
| Polish | `DaitchMokotoff` | `BeiderMorse` (`polish`), `DoubleMetaphone` | Named |
| Dutch, French, Italian, Spanish, Portuguese | `BeiderMorse` (own language tag) | `DoubleMetaphone`, `SoundEx` | Named |
| Russian, Ukrainian | `BeiderMorse` (`cyrillic`) | — | Script |
| Persian | `BeiderMorse` (`arabic`) | — | Script |
| Norwegian, Swedish, Finnish, Galician, Catalan, Basque, Indonesian, Vietnamese | `DoubleMetaphone` | `Metaphone`, `SoundEx` | Script |
| Japanese | `DoubleMetaphone`† | `Metaphone`, `SoundEx` | Script, after transliteration† |
| Hindi, Chinese | — | — | **NoFit** |

**†** Japanese is the one language carrying
`TransliterationAdvice::Recommended`: the recommendation assumes
`verbora_transliterators::transliterate_ja` runs first, since applying an
encoder directly to native kana/kanji is not meaningful. See
[Language § Transliteration Integration](language.md#transliteration-integration).

**Russian and Ukrainian get `Script`, not `Named`, on purpose.** Beider-Morse
*does* have a `russian` rule table, but it is written over Latin
transliterations of Russian names and returns nothing for Cyrillic input. The
table that reads the script Russian is actually written in is the script-level
`cyrillic` one — so that is the primary, and the basis is labelled honestly.
Persian is the same shape: it shares the Arabic script the `arabic` rules read,
and shares none of the language those rules were written for.

**Hindi and Chinese get no primary recommendation at all.** No encoder reads
Devanagari or Han, and no Verbora transliterator romanizes them. Every encoder
is total, so naming one would still produce a key — and that key would be
worthless, because no rule in any of them mentions a Devanagari or Han
character. A recommendation that cannot be honoured is exactly the false
confidence this table exists to avoid.

If you do not know the language yet, that determination belongs to
[`verbora-language`](language.md), which calls this exact `recommend()` once it
has an answer it trusts.

### How to read "Alternative"

An alternative is **also legitimate**, not a downgrade ranked by quality — the
ordering within `alternatives` carries no ranking beyond "reasonable". Reach
for one when its trade-off fits better: `SoundEx` over `DoubleMetaphone` when
you want the cheapest four-character key and can tolerate coarser collisions;
`Metaphone` over `DoubleMetaphone` for English when a single key is enough. See
[Comparison table — encoders](#comparison-table-—-encoders) for what each
trade-off costs.

## Advanced usage

### Trait objects

`SoundEx`, `Metaphone`, `DaitchMokotoff` and all
[seven extension encoders](#seven-more-encoders) implement
`verbora_core::Phonetic`, so they can be held behind a `&dyn Phonetic`.
`DaitchMokotoff` and `MatchRatingApproach` keep their overridden `compare`
semantics behind the trait object too.

```rust
use verbora_core::Phonetic;
use verbora_phonetics::{DaitchMokotoff, Metaphone, SoundEx};

fn main() {
    let encoders: [&dyn Phonetic; 3] =
        [&SoundEx::new(), &Metaphone::new(), &DaitchMokotoff::new()];
    let keys: Vec<String> = encoders.iter().map(|e| e.process("phonetics")).collect();
    assert_eq!(keys[0], "P532");
    assert_eq!(keys[1], "FNTKS");

    // The overridden compare survives the trait object: these two names have
    // different process() strings but a shared code.
    let dm: &dyn Phonetic = &DaitchMokotoff::new();
    assert!(dm.compare("Ceniow", "Tsenyuv"));
}
```

`DoubleMetaphone` is not in that list — it implements `DoubleKeyPhonetic`
(`process_double(&self, &str) -> (String, Option<String>)`) instead. The
`Option` is the point: most names have no second pronunciation, and duplicating
the primary into the alternate slot to mean "absent" would make "this name has
two spellings that sound the same" indistinguishable from "this name has one".

### Working with a `DoubleMetaphoneCode`

`DoubleMetaphone::process` returns a `DoubleMetaphoneCode` rather than a bare
tuple, so the "one key or two" distinction cannot be lost by accident.

```rust
use verbora_phonetics::DoubleMetaphone;

fn main() {
    let dm = DoubleMetaphone::new();
    let smith = dm.process("Smith");

    assert_eq!(smith.primary(), "SM0");
    assert_eq!(smith.alternate(), Some("XMT"));
    assert!(smith.contains("XMT"));

    // Thompson has one pronunciation, so there is no alternate key at all.
    assert_eq!(dm.process("Thompson").alternate(), None);

    // Schmidt's primary is Smith's alternate, which is the whole point.
    assert!(smith.shares_key_with(&dm.process("Schmidt")));
    assert!(dm.compare("Smith", "Schmidt"));

    // Flatten it when you need owned values.
    let (primary, alternate) = dm.process("Smith").into_parts();
    assert_eq!(primary, "SM0");
    assert_eq!(alternate, Some("XMT".to_owned()));
}
```

## Performance characteristics

All encoders are *O(n)* in the token's length, and tokens are short, so the
per-call constants — allocation and case folding — dominate.

| Encoder | Work per token | Notes |
|---|---|---|
| `SoundEx` | one pass over the letters | The character-class rules are provably a per-letter map, so they fuse into a single table lookup. |
| `Metaphone` | one letter-collection pass, then one encoding pass | Doubled letters are reduced and the word-initial exceptions applied before the rule table runs. |
| `DoubleMetaphone` | one left-to-right scan | Builds both keys simultaneously. |
| `DaitchMokotoff` | one scan, with a bounded branch list carried along | Rules are static arrays indexed by first character, so a lookup is an array index; a rule fans out to at most two alternatives and the walk deduplicates as it goes, so the branch list stays small. |
| The other seven | one single-pass scan over one reused buffer | Static compiled-in rule tables; one heap allocation per call for the returned code. |

Daitch–Mokotoff's branch count is bounded in practice as well as in principle:
codes saturate at six digits and the dedup works on that bounded state, so
concatenating a name to itself fifty times collapses to the same code list the
single name produces.

Measured numbers are on
[Competitive benchmarks § Phonetics](../benchmarks/competitive.md#phonetics);
`crates/verbora-phonetics/benches/phonetics.rs` is the Criterion harness.

## Allocation behaviour

<div class="callout callout-warn">
<strong>Careful.</strong> <code>process()</code> returns a freshly allocated,
owned <code>String</code> every time. Encoding millions of tokens means
millions of small allocations, and the parallel batch entry points change how
the work is <em>scheduled</em>, not how much is allocated. The two levers that
do reduce it are <code>process_into</code> (on <code>SoundEx</code> and
<code>Metaphone</code>) and <a href="phonetic-index"><code>PhoneticIndex</code></a>,
which stores codes inline on the stack.
</div>

Per call:

| Entry point | Allocates |
|---|---|
| `SoundEx::process` | one `String`, pre-sized to the four characters the code always fits in |
| `SoundEx::process_into` | nothing, once your buffer has grown |
| `Metaphone::process` | the internal letter buffer, plus the output `String` |
| `Metaphone::process_into` | the internal letter buffer only |
| `DoubleMetaphone::process` | the internal letter buffer, plus one `String` per key produced |
| `DaitchMokotoff::process` | the folded copy of the token, the branch lists, and the joined output `String` |
| `DaitchMokotoff::codes` | the same, but one `String` per branch instead of one joined `String` |
| The other seven | one reused internal buffer, plus the output `String` |

### Reducing it at the call site

1. **Encode once, and move the `String` where it belongs.** A `HashMap` key
   takes the `String` by value, so
   `buckets.entry(soundex.process(name)).or_default().push(i)` allocates one
   key per name — no clone, no re-derivation.
2. **Never call `compare` in a loop.** Encoding each word once and comparing
   the keys turns *O(n²)* encodings into *n* — see
   [`compare()` versus comparing two `process()` results](#compare-versus-comparing-two-process-results).
   Past a few dozen words, [`PhoneticIndex`](phonetic-index) removes the
   *O(n²)* entirely.
3. **Use `process_into` for bulk encoding.** `SoundEx` and `Metaphone` append
   into a buffer you own, which is the only entry point here that allocates
   nothing in steady state.
4. **Prefer the built-in parallel batch over a hand-rolled `par_iter().map()`**,
   which reintroduces exactly the per-word dispatch cost chunking exists to
   avoid.

Further reading: [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md),
[Performance](../performance/index.md).

## Unicode and language notes

**The text unit is one Unicode scalar.** Every encoder here reads text one
scalar at a time. No encoder indexes text by byte or by UTF-16 code unit, so
**no input can be split in the middle of a character** and **no output can
contain a character the input did not imply** — in particular, no encoder can
emit `U+FFFD`, which is the signature of a string cut through the middle of a
character.

What an encoder does with a scalar depends on its own alphabet:

- **The three Latin-alphabet encoders** (`SoundEx`, `Metaphone`,
  `DoubleMetaphone`) read only `A`–`Z` after simple ASCII case folding, and
  **skip** everything else. Skipped is stronger than ignored: a skipped scalar
  does not act as a separator either, so inserting an accented letter, a digit,
  an emoji or a combining mark anywhere in a word leaves the key unmoved. A
  token with no `A`–`Z` letter yields the empty key.
- **The encoders specified for a particular language** fold the accented
  letters their own publications name — `Cologne` for German,
  `DaitchMokotoff` for Slavic and Yiddish spellings,
  [`BeiderMorse`](beider-morse.md) for eighteen. Those fold lists are
  **closed**: `DaitchMokotoff` folds `ß`→`s`, `à`–`å`→`a`, `ł`→`l`, `ś`→`s`,
  `ż`/`ź`→`z` and the rest of its chart's list, and does *not* fold `ü`, `ě` or
  `œ`, which are therefore skipped rather than transliterated.

Where you need `Müller` to code as `Mueller` under an encoder that does not
fold it, transliterate before encoding.

`DoubleMetaphone` is specified over whole personal names, so it treats a space
as a word boundary rather than skipping it: `"O'Brien"` and `"OBrien"` encode
identically, while `"Van Der Berg"` and `"VanDerBerg"` do not.

## Common mistakes

**Expecting a length knob.** There is none. Each encoder emits the code shape
its publication specifies, and the only configurable widths are
`Phonex::with_max_code_length` and `Nysiis::with_strict`. Truncate the code
yourself if you need a shorter blocking key.

**Reading the empty key as a failure.** It is not an error and not a sentinel:
it means the token contained no letter the algorithm recognises. No input *with*
a recognised letter can produce it, so the two cases stay distinguishable.

**Expecting `DaitchMokotoff`'s empty key to be `""`.** It is `000000` — every
code it emits is padded to exactly six digits, empty tokens included.

**Assuming `DaitchMokotoff::codes` returns a set.** It can contain the same
code twice, because branches are deduplicated on (partial code, last
replacement) during the walk rather than on the finished codes. Deduplicate it
yourself if you need a set.

**Calling `compare` in a nested loop.** It re-encodes both sides every time and
allocates two (or more) `String`s per call. Precompute the keys, or use
[`PhoneticIndex`](phonetic-index).

**Assuming a code matches a textbook worked example.** These implementations
are specified and test-pinned in their own right: `Ashcraft` is `A261` under
`SoundEx`, and `chemical` is `XMKL` under `Metaphone` (`ch` is `X`, the "sh"
sound, exactly as the 1990 table specifies). Pin your expectations to the
encoder you are calling.

**Assuming `SoundEx` output always starts with a letter.** It starts with an
`A`–`Z` letter or the code is empty — those are the only two outcomes, so a
token of digits or punctuation gives `""` rather than a digit-led code.

**Reaching for `verbora_core::Phonetic` with `DoubleMetaphone`.** It does not
implement that trait — there is no single key. Use `DoubleKeyPhonetic`, or the
inherent `process` and its `DoubleMetaphoneCode`.

**Assuming `phoneticize_tokens` filters case-insensitively or strips
punctuation.** It does neither: filtering tests the raw token against the list
you passed, so `The` survives a lowercase list, and `it's` reaches the encoder
with its apostrophe intact.

**Forgetting to clear a `process_into` buffer.** It appends and never clears,
which is what makes bulk accumulation free — and what makes one-at-a-time use
wrong unless you clear between calls.

## Related

- [Phonetic neighbors](phonetic-index) — indexes a whole dictionary so
  `neighbors()` can answer "which stored words sound like this one?" without
  re-encoding on every call. Start here past a few dozen words.
- [Beider-Morse](beider-morse.md) — for a problem none of the encoders here
  solve: the same historical family name has different "correct" spellings
  depending on which country's conventions transcribed it.
- [Language](language.md) — script and language detection, plus the
  `recommend()` function
  [Choosing a phonetic algorithm](#choosing-a-phonetic-algorithm) quotes.
- [Distance metrics](../features/distance.md) — the scoring step that runs
  inside each phonetic bucket.
- [Tokenizers](../features/tokenizers.md) — where the tokens come from.
- [Core traits](../features/core.md) — `Phonetic`, `DoubleKeyPhonetic` and
  `StopWords`.
- [Allocation](../performance/allocation.md) ·
  [Zero-copy](../performance/zero-copy.md) ·
  [Parallelism](../performance/parallelism.md)
- [Recipes](../recipes/index.md) — end-to-end fuzzy-matching pipelines.
- [Choosing an API](../choosing/index.md) — the cross-crate decision tables.

## API reference

### Types

| Item | Description |
|---|---|
| `SoundEx` | Russell 1918; NARA, *The Soundex Indexing System*. Zero-sized, `const fn new()`. |
| `Metaphone` | Philips 1990, *Computer Language* 7(12). Zero-sized, `const fn new()`. |
| `DoubleMetaphone` | Philips 2000, *C/C++ Users Journal* 18(6). Zero-sized, `const fn new()`. |
| `DoubleMetaphoneCode` | The primary key and an optional alternate. `primary`, `alternate`, `contains`, `shares_key_with`, `into_parts`. |
| `DaitchMokotoff` | Daitch–Mokotoff Soundex (Gary Mokotoff and Randy Daitch, 1985), branching. Zero-sized, `const fn new()`. |
| `Cologne` | Kölner Phonetik (Postel 1969, *IBM-Nachrichten* 19), German-tuned, unbounded digit code. Zero-sized, `const fn new()`. |
| `Nysiis` | Taft 1970, *Name Search Techniques*. `new()` is strict (6-byte cap, the default); `with_strict(false)` is unbounded. |
| `Caverphone1` / `Caverphone2` | Hood 2002 / 2004, Caversham Project. Fixed 6- / 10-character `1`-padded codes. Zero-sized, `const fn new()`. |
| `Phonex` | Lait and Randell 1996. `new()` defaults to max code length 4; `with_max_code_length(n)` configures it. |
| `RefinedSoundex` | Refined Soundex, ten consonant groups, unbounded code, plus `difference()`. Zero-sized, `const fn new()`. |
| `MatchRatingApproach` | Moore et al. 1977, *Western Union*. `compare` is the published match decision, not code equality. |
| `BeiderMorse` | Beider and Morse phonetic matching — a candidate list, not a key. See [Beider-Morse](beider-morse.md). |

### Methods

| Method | Signature |
|---|---|
| `SoundEx::process` | `(&self, &str) -> String` |
| `SoundEx::process_into` | `(&self, &str, &mut String)` — appends; never clears |
| `SoundEx::compare` | `(&self, &str, &str) -> bool` |
| `Metaphone::process` | `(&self, &str) -> String` |
| `Metaphone::process_into` | `(&self, &str, &mut String)` — appends; never clears |
| `Metaphone::compare` | `(&self, &str, &str) -> bool` |
| `DoubleMetaphone::process` | `(&self, &str) -> DoubleMetaphoneCode` |
| `DoubleMetaphone::compare` | `(&self, &str, &str) -> bool` — matches on **either** key |
| `DoubleMetaphoneCode::primary` / `alternate` | `(&self) -> &str` / `(&self) -> Option<&str>` |
| `DoubleMetaphoneCode::contains` / `shares_key_with` | `(&self, &str) -> bool` / `(&self, &Self) -> bool` |
| `DoubleMetaphoneCode::into_parts` | `(self) -> (String, Option<String>)` |
| `DaitchMokotoff::process` | `(&self, &str) -> String` — every code, `\|`-joined |
| `DaitchMokotoff::codes` | `(&self, &str) -> Vec<String>` — every branch code; `codes(x)[0]` is the non-branching walk's code |
| `DaitchMokotoff::compare` | `(&self, &str, &str) -> bool` — true when the code sets intersect |
| `process` on the other seven | `(&self, &str) -> String` — infallible on any input |
| `compare` on the other seven | `(&self, &str, &str) -> bool` — key equality, except `MatchRatingApproach::compare` (the MRA match decision) |
| `Nysiis::with_strict` / `Nysiis::is_strict` | `(bool) -> Nysiis` / `(&self) -> bool` |
| `Phonex::with_max_code_length` / `Phonex::max_code_length` | `(usize) -> Phonex` / `(&self) -> usize` |
| `RefinedSoundex::difference` | `(&self, &str, &str) -> usize` — positions at which the two codes agree |

### Free functions

| Function | Signature |
|---|---|
| `phoneticize_tokens` | `<'a, T: IntoIterator<Item = &'a str>, O>(tokens: T, stop_words: &StopWords, process: impl FnMut(&'a str) -> O) -> Vec<O>` |
| `tokenize_and_phoneticize` | `<O>(text: &str, stop_words: &StopWords, process: impl FnMut(&str) -> O) -> Vec<O>` |
| `par_encode_batch` (feature `parallel`) | `<P: Phonetic + Sync>(phonetic: &P, tokens: &[&str], chunk_size: usize) -> Vec<String>` |
| `par_encode_double_batch` (feature `parallel`) | `<P: DoubleKeyPhonetic + Sync>(phonetic: &P, tokens: &[&str], chunk_size: usize) -> Vec<(String, Option<String>)>` |
| `DEFAULT_CHUNK_SIZE` (feature `parallel`) | `pub const DEFAULT_CHUNK_SIZE: usize = 64;` — a tuning starting point, not a claim of optimality for your data |
