# Phonetics

`verbora-phonetics` maps a word to a short **key** so that words which sound
alike collide. Eleven encoders are implemented: four core encoders —
`SoundEx`, `Metaphone`, `DoubleMetaphone` and `SoundExDM` (Daitch–Mokotoff) —
plus [seven more](#seven-more-encoders) covering algorithms the first four do
not: `Cologne`, `Nysiis`, `Caverphone1`/`Caverphone2`, `Phonex`,
`RefinedSoundex`, `MatchRatingApproach` and the branching `DaitchMokotoff`.

They are the building block for fuzzy name lookup, deduplication and search
*blocking*: encode every record once, group by key, and only then run an
expensive [string metric](../features/distance.md) inside each group.

<div class="callout callout-spec">
<strong>Specification status.</strong> All eleven encoder types, all 30 exposed
pipeline stages, <code>compare</code>, <code>find_rules</code>,
<code>normalize_length</code> and <code>is_vowel</code> are documented and
test-pinned, across the full range of <code>max_length</code> values including
the rejected ones — which return <code>Err</code>, never a silent fallback.
<code>cargo test -p verbora-phonetics</code> runs <strong>322</strong> unit
tests and <strong>49</strong> doctests.
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
  are English (or, for `SoundExDM`, Latin-script Ashkenazi surnames), and the
  extension encoders widen that only where a published algorithm exists.
  `Москва` encodes as `М000` under `SoundEx` and as `МОСКВА` under `Metaphone`
  — the algorithms pass unknown letters through rather than failing, which is
  quiet rather than useful. See
  [Unicode and language notes](#unicode-and-language-notes).
- **Ranking.** A phonetic key is a yes/no bucket, not a score. For "how similar
  are these two strings" use [distance metrics](../features/distance.md).
- **Whole sentences.** Every encoder takes a *token*. Feed a sentence and you
  get one key for the whole thing (`Metaphone::process("ch ch")` is `"SH KSH"`,
  spaces and all). Tokenize first — see
  [Tokenizers](../features/tokenizers.md).
- **As a hash or an identifier.** Keys are lossy by design, short, and (for
  `SoundEx` and `Metaphone`) can contain arbitrary input characters.

## Quick example

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

fn main() {
    // One key each; two for Double Metaphone.
    assert_eq!(SoundEx::new().process("Robert"), "R163");
    assert_eq!(SoundEx::new().process("Rupert"), "R163");
    assert_eq!(Metaphone::new().process("phonetics"), "FNTKS");
    assert_eq!(Metaphone::new().process("fonetix"), "FNTKS");
    assert_eq!(
        DoubleMetaphone::new().process("astromech"),
        ("ATRMX".to_owned(), "ATRMK".to_owned())
    );
    assert_eq!(SoundExDM::new().process("ALPERT"), "087930");

    // The same surname through all four.
    let name = "Schwarzenegger";
    assert_eq!(SoundEx::new().process(name), "S625");
    assert_eq!(Metaphone::new().process(name), "SKHWRSNJR");
    assert_eq!(SoundExDM::new().process(name), "479465");
}
```

Every encoder type is zero-sized (`Debug + Clone + Copy + Default + PartialEq +
Eq`, `const fn new()`), holds no state and is trivially `Send + Sync`. Create
them wherever it reads best; there is nothing to cache.

## The four encoders

| Encoder | Keys | Default length | Output alphabet | Good for |
|---|:--:|---|---|---|
| `SoundEx` | 1 | 4 characters — the initial, then 3 digits | the token's first character (uppercased) + `0`–`6` | Coarse blocking of English surnames. Cheapest, and the most collisions. |
| `Metaphone` | 1 | 32 characters | uppercase letters, plus `0` for the `th` sound; unknown input characters pass through | General English words, where you want more precision than four characters buys. |
| `DoubleMetaphone` | **2** | 32 characters *per key* | uppercase letters, `0`, and a literal space in one edge case | English text containing names of mixed origin. Index both keys; a match on **either** counts. |
| `SoundExDM` | 1 | 6 digits | digits — plus the literal text `undefined` when the input contains a digit | Slavic, Germanic and Ashkenazi-Jewish surnames. Handles `SCH`, `CZ`, `TSCH`, `RZ` clusters that `SoundEx` flattens. |

<div class="callout callout-warn">
<strong>Careful.</strong> "Default length" is what you get from
<code>process()</code>. It is not an upper bound on the result:
<code>SoundEx</code> and <code>Metaphone</code> uppercase <em>after</em>
truncating, and case mapping can grow a string, so
<code>Metaphone::process_with(&amp;"ß".repeat(40), Some(3.0))</code> is the
six-character <code>"SSSSSS"</code>. See
<a href="#common-mistakes">Common mistakes</a>.
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
| `Caverphone1` | exactly 6 bytes, `1`-padded | New Zealand English (Hood, 2002) | Historical/electoral-roll name matching, coarser |
| `Caverphone2` | exactly 10 bytes, `1`-padded | New Zealand English (Hood, 2004) | Same, with more precision and revised vowel handling |
| `Phonex` | letter + digits, configurable length (default 4) | British surnames (Lait & Randell, 1996) | Soundex-style blocking tuned to reduce false negatives on British names |
| `RefinedSoundex` | letter + digits, unbounded | Spell-checking (Soundex refinement) | Finer buckets than `SoundEx` (ten consonant groups, vowels kept as `0`), plus the standard `difference()` similarity |
| `DaitchMokotoff` | one or more 6-digit codes | Slavic/Yiddish surname spellings (Daitch & Mokotoff, 1985) | Genealogical matching where an ambiguous cluster should yield **every** reading |
| `MatchRatingApproach` | short consonant skeleton | Homophonous personal names (Western Airlines, 1977) | Not just an encoder — `compare` is the published name-match *decision* |

All seven expose `process(&str) -> String` and `compare(&str, &str) -> bool`
plus the type-specific extras below, and all seven implement
`verbora_core::Phonetic`, so they slot into the same `&dyn Phonetic` lists,
`phoneticize_tokens*` closures and `par_encode_batch` calls as the core
encoders. `MatchRatingApproach` and `DaitchMokotoff` override the trait's
default key-equality `compare` with their own match decisions, and keep those
semantics behind the trait object too.

<div class="callout callout-note">
<strong>Note.</strong> The <a href="phonetic-index"><code>PhoneticIndex</code></a>
<em>dictionary index</em> is the one place these seven do not plug in: its
<code>PhoneticEncoder</code> trait, with its compact typed codes, is
implemented for the four core encoders only.
</div>

```rust
use verbora_phonetics::{
    Caverphone2, Cologne, DaitchMokotoff, MatchRatingApproach, Nysiis, Phonex, RefinedSoundex,
};

fn main() {
    assert_eq!(Cologne::new().process("Müller"), "657");
    assert_eq!(Nysiis::new().process("KNUTH"), "NAT");
    assert_eq!(Caverphone2::new().process("Thompson"), "TMPSN11111");
    assert_eq!(Phonex::new().process("Wright"), "R623");
    assert_eq!(RefinedSoundex::new().process("testing"), "T6036084");
    assert_eq!(MatchRatingApproach::new().process("Catherine"), "CTHRN");
    assert_eq!(DaitchMokotoff::new().process("AUERBACH"), "097400|097500");
}
```

All seven share one performance shape: a single pass over one reused buffer,
static compiled-in rule tables, and one heap allocation per call for the
returned code (`DaitchMokotoff` adds a small branch list).

### Cologne

Kölner Phonetik, the German-language analogue of `SoundEx`: umlauts fold
(`Müller` → `657`), the code is never truncated, vowels survive only at the
very front, and `C`, `D`, `T`, `P`, `X` encode differently depending on their
neighbors. Unknown characters are skipped without panicking.
`Cologne::compare("Meyer", "Mayr")` is `true`.

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
    assert!(Nysiis::new().compare("Trueman", "Truman"));
}
```

### Caverphone

Two revisions, two types, both fixed-width and `1`-padded: `Caverphone1` (2002,
6-byte codes) and `Caverphone2` (2004, 10-byte codes, revised handling of `y`,
word-final `w`/`r`/`l` and trailing vowels — `ready` is `RT1111` under 1.0 but
`RTA1111111` under 2.0). Prefer `Caverphone2` unless you need to match an index
built with 1.0 codes.

### Phonex

A Soundex refinement for British surnames: a preprocessing stage (trailing-`S`
removal, leading-pair rewrites like `KN`→`NN`, first-letter substitutions)
followed by context-sensitive digit rules. Code length is configurable at
construction, defaults to 4, and is measured in **bytes**, so a non-ASCII first
letter shortens the visible code (`é` → `É00`).

```rust
use verbora_phonetics::Phonex;

fn main() {
    assert_eq!(Phonex::new().process("Sinatra"), "S536");
    assert_eq!(Phonex::with_max_code_length(6).process("Sinatra"), "S53600");
    assert!(Phonex::new().compare("Knuth", "Nuth"));
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
    assert!(refined.compare("Smith", "Smythe"));
    assert_eq!(refined.difference("Smithers", "Smythers"), 8); // high similarity
    assert_eq!(refined.difference("Margaret", "Andrew"), 1); // low
}
```

### Match Rating Approach

MRA is two things at once, and the second is the point. **`process`** produces
the encoding: uppercase, fold 60 listed accented letters, drop non-initial
vowels, collapse doubled consonants pairwise, and past six bytes keep the first
three and last three. **`compare` is the published match decision, not code
equality** — it short-circuits on raw string equality, rejects encoded lengths
differing by 3 or more bytes, blanks out agreeing characters in a left-to-right
and a right-to-left pass, and accepts when the resulting rating clears a
minimum that depends on the combined encoded length.

```rust
use verbora_phonetics::MatchRatingApproach;

fn main() {
    let mra = MatchRatingApproach::new();
    assert_eq!(mra.process("Smith"), "SMTH");
    assert!(mra.compare("smith", "smyth"));
    assert!(mra.compare("Franciszek", "Frances"));
    assert!(!mra.compare("Karl", "Alessandro"));
}
```

Two names with *equal codes* can still fail `compare`, and two names with
different codes can pass — treat `process` as the index key and `compare` as
the decision, not two views of one operation.

### Daitch-Mokotoff, branching

The genuine multi-code Daitch–Mokotoff: where a cluster is phonetically
ambiguous (`CH` as in *chair* or as in *Bach*, Polish `RS`/`RZ`, initial `J`),
the encoder follows **every** reading. `process` joins the codes with `|`;
`codes` returns them as a `Vec<String>`, with `codes(x)[0]` always the code a
non-branching walk would produce; `compare` matches when the two names share
**any** code.

```rust
use verbora_phonetics::DaitchMokotoff;

fn main() {
    let dm = DaitchMokotoff::new();
    assert_eq!(dm.codes("AUERBACH"), vec!["097400", "097500"]);
    assert_eq!(dm.codes("GOLDEN"), vec!["583600"]);
    assert!(dm.compare("Moskowitz", "Moskovitz"));
    assert!(!dm.compare("Peters", "Peterson"));
}
```

**`DaitchMokotoff` vs. `SoundExDM`.** Different algorithms, not two names for
one thing. `SoundExDM` is the single-code variant: one 6-digit `String`, always
the first legal reading — pick it when an index was built with its codes, or
when you need exactly one key per name. `DaitchMokotoff` is the branching
algorithm — pick it for genealogical matching, where missing the second reading
of `AUERBACH` means missing real matches. Their codes are not interchangeable
in an index; choose one per index and stay with it.

## Choosing the right API

Three independent choices: **which encoder**, **which entry point** on that
encoder, and **how to run it over a token stream**. The rich entry-point
surface (`process_with`, `try_process`, the UTF-16 variants) belongs to the
four core encoders; the [seven more](#seven-more-encoders) expose
`process`/`compare` plus their per-type extras. Every free function below
accepts all eleven, via `Phonetic`. Full signatures are in the
[API reference](#api-reference).

### Decision tree — which encoder

| Your case | Encoder |
|---|---|
| English surnames, cheapest possible blocking key | `SoundEx` |
| General English words, one key, better precision | `Metaphone` |
| English text with names of many origins, two keys indexable | `DoubleMetaphone` |
| Slavic / Germanic / Ashkenazi-Jewish surnames, one key | `SoundExDM` |
| Genealogy — every reading of an ambiguous cluster | `DaitchMokotoff` |
| German words and names | `Cologne` |
| British surnames | `Phonex` |
| New Zealand English | `Caverphone2` (`Caverphone1` only for a legacy index) |
| NYSIIS-compatible surname codes | `Nysiis` |
| Spell-check-style finer buckets, plus a similarity score | `RefinedSoundex` |
| A name-match *decision* rather than a key | `MatchRatingApproach` |

For "which encoder for this *language*", see
[Choosing a phonetic algorithm](#choosing-a-phonetic-algorithm) below.

### Comparison table — encoders

| Encoder | Keys | Scan | `Phonetic` impl | `DoubleKeyPhonetic` impl | Fallible entry point | UTF-16 entry point |
|---|:--:|---|:--:|:--:|:--:|:--:|
| `SoundEx` | 1 | one pass | ✅ | ❌ | `try_process` | `try_process_utf16` |
| `Metaphone` | 1 | 30 rewrite passes over 2 buffers | ✅ | ❌ | ❌ (cannot fail) | `process_utf16` |
| `DoubleMetaphone` | 2 | one pass | ❌ | ✅ | ❌ (cannot fail) | ❌ (output is ASCII) |
| `SoundExDM` | 1 | one pass + bounded trie walk | ✅ | ❌ | `try_process` | ❌ (output is ASCII) |

`DoubleMetaphone` does **not** implement `verbora_core::Phonetic` — it has no
single key to return, and implements `DoubleKeyPhonetic` (`process_double`)
instead. `SoundExDM` implements only `Phonetic`, because despite the name it
produces one code.

### `process()`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

The default, on every encoder. Eager, always allocates (see
[Allocation behaviour](#allocation-behaviour) for counts), no `_into` variant.
Use it unless you need a non-default length or an `Err` instead of the lenient
fallback. Two of the core four are lenient by design, on the grounds that a
text-processing library should not fail on punctuation:

- **`SoundEx::process` never fails.** For a token starting with `(`, `)`, `*`,
  `+`, `?`, `[` or `\`, it skips the initial-sound strip and returns a code
  anyway rather than surfacing that as an error.
- **`SoundExDM::process` never fails**, because the default code length of 6
  always pads cleanly. Only a custom length reaches the fallible path.

### `process_with(token, max_length)`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

On `SoundEx`, `Metaphone` and `DoubleMetaphone`. **Not** on `SoundExDM`, whose
length-taking entry point is `try_process`. The argument is `Option<f64>` and
uses f64 number semantics: `0.0`, `NaN` and `None` all select the default
length, and a negative length is meaningful rather than rejected.

| `max_length` | `SoundEx` | `Metaphone` |
|---|---|---|
| `None`, `Some(0.0)`, `Some(f64::NAN)` | default (initial + 3 digits) | default (32) |
| `Some(1.0)` | default — the digit count is `max_length - 1`, and 0 digits falls back to 3 | 1 character |
| `Some(n)`, `n > 1` | initial + `n - 1` digits | `n` characters |
| negative | the initial letter only | the empty string |

```rust
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    let soundex = SoundEx::new();
    assert_eq!(soundex.process_with("phonetics", Some(2.0)), "P5");
    assert_eq!(soundex.process_with("phonetics", Some(1.0)), "P532"); // 0 digits -> default
    assert_eq!(soundex.process_with("phonetics", Some(-1.0)), "P");

    let metaphone = Metaphone::new();
    assert_eq!(metaphone.process_with("phonetics", Some(4.0)), "FNTK");
    assert_eq!(metaphone.process_with("phonetics", Some(-1.0)), "");
    // Truncation happens BEFORE uppercasing, and case mapping can grow a string.
    assert_eq!(metaphone.process_with(&"ß".repeat(40), Some(3.0)), "SSSSSS");
}
```

### `try_process(…)`

<span class="badge badge-fallible">FALLIBLE</span>

Only `SoundEx` and `SoundExDM` have one, and they guard different things.
Allocation on the success path is identical to `process`; the error path
allocates nothing.

| Method | Guards against |
|---|---|
| `SoundEx::try_process` | `PhoneticError::InvalidInitialPattern(char)` — the token starts with one of the seven characters the initial-sound strip cannot anchor on: `(`, `)`, `*`, `+`, `?`, `[`, `\` |
| `SoundExDM::try_process` | `PhoneticError::InvalidArrayLength(f64)` — the requested length would need a fractional number of padding zeros |

Reach for `SoundEx::try_process` when your tokens can start with punctuation
and you want that surfaced as an error — a punctuation-preserving tokenizer
emits `(` as a token of its own. Reach for `SoundExDM::try_process` any time
you pass a non-default code length, because it is the only way to pass one.

```rust
use verbora_phonetics::{PhoneticError, SoundEx, SoundExDM};

fn main() {
    let soundex = SoundEx::new();
    assert_eq!(soundex.process("(abc"), "(120"); // lenient: no strip, code anyway
    assert_eq!(
        soundex.try_process("(abc", None),
        Err(PhoneticError::InvalidInitialPattern('('))
    );
    assert_eq!(soundex.try_process(".bcd", None).unwrap(), ".230"); // other punctuation is fine

    let dm = SoundExDM::new();
    // Padding a six-digit code to 6.5 needs half a zero: Err.
    assert_eq!(
        dm.try_process("ALPERT", Some(6.5)),
        Err(PhoneticError::InvalidArrayLength(2.5))
    );
    assert_eq!(dm.try_process("LONGWORDXYZ", Some(3.0)).unwrap(), "865"); // truncates
    assert_eq!(dm.try_process("ALPERT", Some(-1.0)).unwrap(), "0879"); // counts back
}
```

### `try_process_utf16()` / `process_utf16()`

<span class="badge badge-utf16">UTF-16</span>

Two encoders can produce a key containing an **unpaired surrogate**, which a
Rust `String` cannot hold: `SoundEx` keeps the first UTF-16 *code unit*, so an
astral first character orphans a high surrogate; `Metaphone` truncates to
`max_length` code units, and the cut can fall between the halves of a pair. The
`String`-returning methods substitute `U+FFFD`; these return the exact code
units as a `Vec<u16>`. `SoundEx`'s version is fallible, `Metaphone`'s is not,
and both take the length argument — there is no one-argument shorthand.
`DoubleMetaphone` and `SoundExDM` build output from ASCII literals only, so
they need no such method.

```rust
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    assert_eq!(SoundEx::new().process("😀"), "\u{FFFD}000");
    assert_eq!(
        SoundEx::new().try_process_utf16("😀", None).unwrap(),
        vec![0xD83D, 0x30, 0x30, 0x30]
    );
    assert_eq!(Metaphone::new().process_with("😀", Some(1.0)), "\u{FFFD}");
    assert_eq!(Metaphone::new().process_utf16("😀", Some(1.0)), vec![0xD83D]);
}
```

### `compare()` versus comparing two `process()` results

**`compare` does not short-circuit**, on any encoder. `SoundEx`, `Metaphone`
and `SoundExDM` define it as `self.process(a) == self.process(b)` — two full
encodings, two `String` allocations, every call. `DoubleMetaphone::compare`
encodes both sides and matches on either key (`pa == pb || sa == sb`) — four
keys, four allocations.

| You want | Use |
|---|---|
| A single ad-hoc "do these two sound alike?" test | `compare(a, b)` — it reads better and costs the same |
| The `DoubleMetaphone` "either key matches" rule | `compare(a, b)` — reproducing it by hand is easy to get wrong |
| To test one word against many, or many against many | `process` **once per word**, then compare the keys |
| To store the key | `process` — `compare` throws its keys away |

The last two rows matter: `compare` in a nested loop is *O(n²)* encodings,
while encoding each word once and comparing the keys is *n* encodings and
*O(n²)* `&str` comparisons. See
[Reducing it at the call site](#reducing-it-at-the-call-site).

### `phoneticize_tokens_with` versus `phoneticize_tokens`

Both encode an already-tokenized stream and drop stop words. Tokenizing lives
in a different crate (`verbora-tokenizers`), so these take the tokens directly
and you choose the tokenizer.

| | `phoneticize_tokens` | `phoneticize_tokens_with` |
|---|---|---|
| Stop-word source | a **process-global mutable** list | a `&StopWords` you own |
| Reproducible | ❌ — any call to `verbora_core::stopwords::add_global_stopword` anywhere in the process changes the result | ✅ |
| Shared state | ⚠️ Safe, but the global sits behind an `RwLock` another thread can write | ✅ None |
| Badge | <span class="badge badge-global">GLOBAL STATE</span> | — |

<div class="callout callout-warn">
<strong>Default to <code>phoneticize_tokens_with</code>.</strong> Passing a
<code>&amp;StopWords</code> costs one line and removes a whole category of
order-dependent test and non-deterministic worker. The global does have a fast
path — while it has never been mutated, lookups are a lock-free binary search
over a static sorted slice — but once anything mutates it, every lookup takes
the read lock.
</div>

The `T: IntoIterator<Item = &'a str>` bound is the useful part of the
signature: a tokenizer's lazy `tokens()` iterator satisfies it directly, so
there is no intermediate `Vec`. The closure's return type is free, so a
two-key encoder works the same way.

```rust
use verbora_core::StopWords;
use verbora_phonetics::{Metaphone, phoneticize_tokens_with};
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let metaphone = Metaphone::new();
    let keys = phoneticize_tokens_with(
        AggressiveTokenizer::new().tokens("the quick brown fox"),
        &StopWords::english(),
        false,
        |t| metaphone.process(t),
    );
    assert_eq!(keys, ["KK", "BRN", "FKS"]);
}
```

Only tokenizers yielding `&'a str` compose directly — the `AggressiveTokenizer`
family, and `WordTokenizer` once you have unwrapped its `Option`.
`RegexpTokenizer` yields `Option<&'a str>` items, so it needs `.flatten()`
first. Tokenizers yielding `Cow<'a, str>` or `Utf16Token<'a>` do not compose:
collect their tokens first and pass a slice of the collected `&str`s.

Two behaviours worth knowing: **filtering is case-sensitive and tests the raw
token** (`the` is dropped, `The` is not), and **tokens are encoded verbatim,
punctuation included** (`it's`, `co-op` and `/path` all reach the encoder
as-is).

### `par_encode_batch` / `par_encode_double_batch` — parallel batch (feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Behind the `parallel` Cargo feature (never on by default), `par_encode_batch`
fans `Phonetic::process` out across a `rayon` thread pool, and
`par_encode_double_batch` does the same for `DoubleKeyPhonetic::process_double`
(`DoubleMetaphone`). Both call the same encoder the sequential API uses, and
allocate one output `Vec` plus exactly what `process` allocates per token — no
extra per-chunk buffering.

```rust  ignore
use verbora_phonetics::{SoundEx, par_encode_batch};

// `chunk_size` is a required argument, not a hidden default.
let keys = par_encode_batch(&SoundEx::new(), &["Robert", "Rupert", "phonetics"], 2);
assert_eq!(keys, ["R163", "R163", "P532"]);
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

**When to reach for it.** A handful of words, or words arriving one at a time:
call `process` directly. Building an index over tens of thousands of words:
this is the intended use. In between, measure — the crossover moves with core
count and machine load. See [Parallelism](../performance/parallelism.md).

## Choosing a phonetic algorithm

The [decision table above](#decision-tree-—-which-encoder) answers "which
encoder for this use case". This section answers "which encoder for this
*language*".

<div class="callout callout-warn">
<strong>There is no "best" phonetic algorithm.</strong> Every encoder in the
table below is one of the four core encoders — all English-oriented, and none
written with Cyrillic, Devanagari, Han or Arabic phonotactics in mind.
"Recommended" means <em>the closest fit among those four</em>, never an
unqualified claim that an encoder is correct for a language in the way it is
correct for English surnames. The
<a href="#seven-more-encoders">seven extension encoders</a> widen the language
story where a published algorithm exists, but sit outside this table's source,
<code>recommend()</code>.
</div>

### Per-language table

Sourced directly from [`verbora-language`](../features/language.md)'s
[`recommend()`](../features/language.md#phonetic-strategy-recommend) — a closed
`match` over all 22 languages that crate has a strategy for, not a separate
opinion maintained on this page.

| Language | Recommended | Alternative(s) | Category |
|---|---|---|---|
| English | `DoubleMetaphone` | `Metaphone`, `SoundEx` | Recommended |
| German, Dutch, Swedish, Norwegian, Finnish | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Spanish, Portuguese, Italian, French, Galician, Catalan, Basque | `DoubleMetaphone` | `SoundEx` | Recommended |
| Indonesian, Vietnamese | `DoubleMetaphone` | `SoundEx` | Recommended |
| Polish, Ukrainian, Russian | `SoundExDM`* | `DoubleMetaphone` | Recommended, with a caveat* |
| Japanese | `DoubleMetaphone`† | `SoundEx` | Recommended, after transliteration† |
| Persian, Hindi, Chinese | — | — | **Not designed for this language** |

**\*** Polish, Ukrainian and Russian also get
`TransliterationAdvice::Unsupported` — there is no Cyrillic transliterator, so
the recommendation only produces a meaningful key once *you* have romanized the
input yourself.

**†** Japanese gets `TransliterationAdvice::Recommended`: the recommendation
assumes `verbora_transliterators::transliterate_ja` runs first, since applying
an encoder directly to native kana/kanji is not meaningful. See
[Language § Transliteration Integration](language.md#transliteration-integration).

**Persian, Hindi and Chinese get no primary recommendation** — `recommend()`
returns `primary: None` for exactly these three. There is no transliterator for
Arabic, Devanagari or Han script, *and* none of the four encoders were designed
for those languages' phonotactics. The encoders would still return a key, but
it would carry no phonetic meaning.

For German *words* specifically, reach for [`Cologne`](#cologne) directly:
`recommend()` ranges over the four core encoders only, so its German answer is
tuned to Germanic *surnames*. If you do not know the language yet, that
determination belongs to [`verbora-language`](language.md), which calls this
exact `recommend()` once it has an answer it trusts.

### How to read "Alternative"

An alternative is **also legitimate**, not a downgrade ranked by quality — the
ordering within `alternatives` carries no ranking beyond "reasonable". Reach
for one when its trade-off fits better: `SoundEx` over `DoubleMetaphone` when
you want the cheapest four-character key and can tolerate coarser collisions;
`Metaphone` over `DoubleMetaphone` for English when a single key is enough. See
[Comparison table — encoders](#comparison-table-—-encoders) for what each
trade-off costs.

## Advanced usage

### The exposed pipeline stages

`Metaphone` exposes 21 stage methods and `SoundEx` exposes 9 — public,
individually callable pipeline stages, each pinned by the regression suite.
**A normal user should ignore all of them.** `process` runs the stages in the
one order that produces a correct key; calling them individually is slower
(each builds its own buffers) and easy to get wrong (the order is load-bearing,
and two stages actively undo each other). Reach for them only when debugging a
surprising result.

| Owner | Methods | Returns |
|---|---|---|
| `SoundEx` | `transform_lipps`, `transform_throats`, `transform_toungue`, `transform_l`, `transform_hum`, `transform_r`, `transform`, `condense`, `pad_right0` | `Cow<'a, str>` <a class="badge badge-cow" href="../performance/zero-copy">COW</a> — borrows the input when the rule changes nothing |
| `Metaphone` | `dedup`, `drop_initial_letters`, `drop_b_after_m_at_end`, `c_transform`, `d_transform`, `drop_g`, `transform_g`, `drop_h`, `transform_ck`, `transform_ph`, `transform_q`, `transform_s`, `transform_t`, `drop_t`, `transform_v`, `transform_wh`, `drop_w`, `drop_y`, `transform_x`, `transform_z`, `drop_vowels` | `String` <a class="badge badge-owned" href="../performance/allocation">OWNED</a> — always allocates |

Every `SoundEx` stage matches lower-case letters only, so
`soundex.transform("RENDER")` returns `"RENDER"` unchanged while
`soundex.transform("render")` is `"6e53e6"`. `process` lowercases first, so
this only affects callers of the stages directly.

### `SoundExDM::find_rules` and `normalize_length`

Also public surface. `find_rules(&str) -> Rules` reports the longest legal
prefix and its codes; `normalize_length(&str, Option<f64>) -> Result<String,
PhoneticError>` pads or cuts a code. `Rules` and `RuleMapping::{Triple,
Number}` are public so the rule table is inspectable.

### Trait objects

`SoundEx`, `Metaphone`, `SoundExDM` and all
[seven extension encoders](#seven-more-encoders) implement
`verbora_core::Phonetic`, so they can be held behind a `&dyn Phonetic`.
`MatchRatingApproach` and `DaitchMokotoff` keep their overridden `compare`
semantics behind the trait object too.

```rust
use verbora_core::Phonetic;
use verbora_phonetics::{Metaphone, SoundEx, SoundExDM};

fn main() {
    let encoders: [&dyn Phonetic; 3] = [&SoundEx::new(), &Metaphone::new(), &SoundExDM::new()];
    let keys: Vec<String> = encoders.iter().map(|e| e.process("Robert")).collect();
    assert_eq!(keys, ["R163", "RBRT", "979300"]);
}
```

`DoubleMetaphone` is not in that list — it implements `DoubleKeyPhonetic`
(`process_double(&self, &str) -> (String, String)`) instead.

## Performance characteristics

All encoders are *O(n)* in the token's length, and tokens are short, so the
per-call constants — allocation and case folding — dominate.

| Encoder | Work per token | Notes |
|---|---|---|
| `SoundEx` | one pass over the code units | Six character-class rules are provably a per-code-unit map, so they are fused into a single table lookup. |
| `Metaphone` | only the rewrite passes that fire, over 2 pooled scratch buffers | The 21 stages are fused into one skip-gated driver: a letter mask decides which rules can fire on this word, and a skipped stage touches nothing. The scratch pair is pooled per thread and reused across calls. |
| `DoubleMetaphone` | one left-to-right scan | Builds both keys simultaneously. |
| `SoundExDM` | one scan, with a trie walk per position | The trie is at most 7 deep (`SCHTSCH`), and `pos` advances by at least 1 per walk, so the total is linear. |
| The other seven | one single-pass scan over one reused buffer | Static compiled-in rule tables; one heap allocation per call for the returned code. |

**Non-ASCII input costs more.** Each entry point tests `str::is_ascii` (which
is vectorised, so the test is close to free) and picks the narrowest
representation provably identical to UTF-16: `&[u8]` for ASCII, `Vec<u16>`
otherwise. Leaving the fast path adds a `Vec<u16>` promotion and, for
`Metaphone`, a UTF-16 uppercase pass and a lossy re-encode.

Measured numbers are on
[Competitive benchmarks § Phonetics](../benchmarks/competitive.md#phonetics);
`crates/verbora-phonetics/benches/phonetics.rs` is the Criterion harness.

## Allocation behaviour

<div class="callout callout-warn">
<strong>Careful.</strong> This crate has <strong>no <code>_into</code>
API</strong>. <code>process()</code> returns a freshly allocated, owned
<code>String</code> every time — <code>DoubleMetaphone::process()</code>
returns two. There is no way to encode into a caller-supplied buffer, and the
parallel batch entry points change how the work is <em>scheduled</em>, not how
much is allocated. Encoding millions of tokens means millions of small
allocations; the levers you have are below.
</div>

Per call:

| Encoder | ASCII input | Non-ASCII input |
|---|---|---|
| `SoundEx::process` | The digit `Vec<u8>`, a `String` for the uppercased initial, and usually one growth of that `String` — up to **three** small allocations | The same, plus the lowercased copy |
| `Metaphone::process` | The output `String` — **one** in steady state; the pipeline's two scratch buffers are pooled per thread, and ASCII input folds lowercase directly into that scratch | **Six**: the lowercased `String`, the `Vec<u16>` promotion, the second scratch buffer, the pipeline-result copy, the uppercase `Vec<u16>`, and the output `String` |
| `DoubleMetaphone::process` | Two `String::with_capacity(16)` accumulators, both returned — **two** | **Four**: the uppercased `String`, the `Vec<u16>` promotion, and the two accumulators |
| `SoundExDM::process` | One `String::with_capacity(8)`, padded in place — **one** | **Three**: the uppercased `String`, the `Vec<u16>` promotion, and the output |

Add one allocation to the `SoundEx`, `DoubleMetaphone` and `SoundExDM` ASCII
rows when the input needs case folding. `Metaphone`'s ASCII path is exempt: it
folds byte-wise straight into the pooled scratch, cased input or not.

### Reducing it at the call site

1. **Encode once, and move the `String` where it belongs.** A `HashMap` key
   takes the `String` by value, so
   `buckets.entry(soundex.process(name)).or_default().push(i)` allocates one
   key per name — no clone, no re-derivation.
2. **Never call `compare` in a loop.** Encoding each word once and comparing
   the keys turns *O(n²)* encodings into *n* — see
   [`compare()` versus comparing two `process()` results](#compare-versus-comparing-two-process-results).
3. **Fold case upstream, once.** `SoundEx` and `Metaphone` lowercase first;
   `DoubleMetaphone` and `SoundExDM` uppercase first. Feeding an already-folded
   token lets the case-fold `Cow` borrow instead of allocating. This is a
   different fold for each pair — you cannot satisfy all four at once.
4. **Avoid the individual stage methods in hot code.** Calling the 21 public
   `Metaphone` stages by hand allocates two buffers and one `String` *each*.
5. **Prefer the built-in parallel batch over a hand-rolled `par_iter().map()`**,
   which reintroduces exactly the per-word dispatch cost chunking exists to
   avoid. Use `phoneticize_tokens_with` in any parallel context, since
   `phoneticize_tokens` reads process-global state.

Further reading: [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md),
[Performance](../performance/index.md).

## Unicode and language notes

This crate indexes strings by UTF-16 code unit, and these algorithms index,
slice and truncate constantly. A character outside the Basic Multilingual Plane
counts as **two**, and a rule can match one half of a surrogate pair. That is
observable:

```text
Metaphone.dedup("😀😀")
  UTF-16 code units : "😀😀"   the four code units are D83D DE00 D83D DE00 —
                              no two ADJACENT ones are equal, so nothing collapses
  Rust chars        : "😀"     two identical `char`s collapse to one
```

The algorithms are written once, generically over a `Unit` trait implemented
for `u8` and `u16`, and each entry point picks the narrowest representation
*provably identical* to UTF-16 for the given input: `&[u8]` for ASCII (no
allocation), `Vec<u16>` otherwise (one). For ASCII one byte *is* one UTF-16
code unit, so the fast path is the same computation on a narrower type, not an
approximation.

The public `units` module exposes the primitives for reasoning about those
coercions — notably `uppercase_utf16` (preserves unpaired surrogates, which
`String::from_utf16_lossy` would destroy) and `trim_units` (a whitespace set
that includes `U+FEFF` and excludes `U+0085`). Full list in the
[API reference](#module-units).

**Unpaired surrogates.** `SoundEx` (it keeps the first code unit) and
`Metaphone` (it truncates mid-pair) can produce one. Their `String`-returning
methods render it as `U+FFFD`; `SoundEx::try_process_utf16` and
`Metaphone::process_utf16` return the exact code units. `DoubleMetaphone` and
`SoundExDM` emit only ASCII.

**Language coverage.** The four core encoders are English-language algorithms,
and none rejects input it does not understand:

- `SoundEx` uppercases the first character and passes unknown letters through
  the digit filter, so `café` is `C100`, `ÉCOLE` is `É240`, `Москва` is `М000`
  and `日本語` is `日000`. Case mapping can lengthen the code: `ß` is `SS000`.
- `Metaphone` passes non-ASCII through untouched: `café` is `KFÉ`, `Москва` is
  `МОСКВА`, `😀` is `😀`.
- `DoubleMetaphone` names exactly five non-ASCII characters in its switch — `Ê`,
  `É`, `À` (word-initial vowels), `Ç` → `S` and `Ñ` → `N`. Every other accented
  letter contributes nothing, so `Москва`, `日本語` and `😀` all encode as
  `("", "")`. Its `is_vowel` counts `Y` as a vowel and rejects `É`, even though
  the main switch treats `É` as a word-initial vowel.
- `SoundExDM` uppercases first (so `ß` becomes `SS` and encodes as `400000`)
  and treats any non-ASCII code unit as an unknown key that stops the trie
  walk.

## Common mistakes

**Assuming the result is no longer than `max_length`.** `SoundEx` and
`Metaphone` uppercase *after* truncating, and case mapping can grow a string
(`ß` → `SS`, `ﬁ` → `FI`). `Metaphone::process_with(&"ß".repeat(40), Some(3.0))`
is `"SSSSSS"`.

**Expecting `Some(1.0)` to give a one-character `SoundEx` code.** The digit
count is `max_length - 1`, and 0 digits falls back to the default 3. Use
`Some(-1.0)` to get just the initial letter.

**Calling `compare` in a nested loop.** It re-encodes both sides every time and
allocates two (or four) `String`s per call. Precompute the keys.

**Assuming a code matches a textbook worked example.** These implementations
are specified and test-pinned in their own right: `Ashcraft` is `A226` under
`SoundEx` (adjacent equal digits merge *before* `h`, `w` and the vowels are
stripped), and `chemical` is `SHMKL` under `Metaphone` (word-initial `ch`
becomes `x`, then `x` becomes `s`). Pin your expectations to the encoder you
are calling.

**Assuming `SoundEx` output starts with a letter.** Letters map to digits and
existing digits are left alone, so `process("12345")` is `"1234"`. An empty
token has no initial letter at all: `process("")` is `"000"`, three characters
where every other input yields four.

**Assuming `SoundExDM` output is always digits.** An input digit collides with
the rule table's stop marker and halts the walk, so `process("B0")` is
`"undefi"` and `process("MOSKOWITZ0")` is `"6457un"`. If you index D-M codes,
strip digits from your input or accept that an `undefi`-prefixed bucket exists.

**Reaching for `verbora_core::Phonetic` with `DoubleMetaphone`.** It does not
implement that trait — there is no single key. Use `DoubleKeyPhonetic`, or the
inherent `process`.

**Slicing a `Metaphone` key by bytes.** The output can contain non-ASCII
(`process("café")` is `"KFÉ"`), so `&key[..3]` can panic. Use `process_with` to
bound the length, or a char-aware slice.

**Assuming `phoneticize_tokens` filters case-insensitively or strips
punctuation.** It does neither: `The` survives the stop-word filter and `it's`
is encoded with the apostrophe.

**Treating `try_process` as "the safe one".** It is the *exact* one — it
surfaces the fallible cases as `Err` instead of silently handling them.
`process` is the lenient default.

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
- [Core traits](../features/core.md) — `Phonetic`, `DoubleKeyPhonetic`,
  `StopWords` and the process-global stop-word list.
- [Allocation](../performance/allocation.md) ·
  [Zero-copy](../performance/zero-copy.md) ·
  [Parallelism](../performance/parallelism.md)
- [Recipes](../recipes/index.md) — end-to-end fuzzy-matching pipelines.
- [Choosing an API](../choosing/index.md) — the cross-crate decision tables.

## API reference

### Types

| Item | Description |
|---|---|
| `SoundEx` | Russell/NARA SoundEx. Zero-sized, `const fn new()`. |
| `Metaphone` | Original (Philips, 1990) Metaphone. Zero-sized, `const fn new()`. |
| `DoubleMetaphone` | Double Metaphone. Zero-sized, `const fn new()`. |
| `SoundExDM` | Daitch–Mokotoff SoundEx, single-code variant. Zero-sized, `const fn new()`. |
| `Cologne` | Kölner Phonetik (Postel, 1969), German-tuned, unbounded digit code. Zero-sized, `const fn new()`. |
| `Nysiis` | NYSIIS (Taft, 1970). `new()` is strict (6-byte cap, the default); `with_strict(false)` is unbounded. |
| `Caverphone1` / `Caverphone2` | Caverphone 1.0 / 2.0 (Hood, 2002 / 2004), fixed 6- / 10-byte `1`-padded codes. Zero-sized, `const fn new()`. |
| `Phonex` | Phonex (Lait & Randell, 1996). `new()` defaults to max code length 4; `with_max_code_length(n)` configures it. |
| `RefinedSoundex` | Refined Soundex, ten consonant groups, unbounded code, plus `difference()`. Zero-sized, `const fn new()`. |
| `MatchRatingApproach` | Match Rating Approach (1977). `compare` is the published match decision, not code equality. |
| `DaitchMokotoff` | Branching Daitch–Mokotoff — one or more codes per name. Zero-sized, `const fn new()`. |
| `PhoneticError` | `InvalidInitialPattern(char)`, `InvalidArrayLength(f64)`. Implements `std::error::Error`. |
| `Rules` | `{ length: usize, mapping: RuleMapping }` — the result of `SoundExDM::find_rules`. |
| `RuleMapping` | `Triple([i32; 3])` or `Number(i32)`; `get(index) -> Option<i32>`. |

### Methods

| Method | Signature |
|---|---|
| `SoundEx::process` | `(&self, &str) -> String` |
| `SoundEx::process_with` | `(&self, &str, Option<f64>) -> String` |
| `SoundEx::try_process` | `(&self, &str, Option<f64>) -> Result<String, PhoneticError>` |
| `SoundEx::try_process_utf16` | `(&self, &str, Option<f64>) -> Result<Vec<u16>, PhoneticError>` |
| `SoundEx::compare` | `(&self, &str, &str) -> bool` |
| `SoundEx` stages | `transform`, `transform_lipps`, `transform_throats`, `transform_toungue`, `transform_l`, `transform_hum`, `transform_r`, `condense`, `pad_right0` — each `(&self, &'a str) -> Cow<'a, str>` |
| `Metaphone::process` | `(&self, &str) -> String` |
| `Metaphone::process_with` | `(&self, &str, Option<f64>) -> String` |
| `Metaphone::process_utf16` | `(&self, &str, Option<f64>) -> Vec<u16>` |
| `Metaphone::compare` | `(&self, &str, &str) -> bool` |
| `Metaphone` stages | 21 methods, each `(&self, &str) -> String` (listed [above](#the-exposed-pipeline-stages)) |
| `DoubleMetaphone::process` | `(&self, &str) -> (String, String)` |
| `DoubleMetaphone::process_with` | `(&self, &str, Option<f64>) -> (String, String)` |
| `DoubleMetaphone::compare` | `(&self, &str, &str) -> bool` — matches on **either** key |
| `DoubleMetaphone::is_vowel` | `(&self, &str) -> bool` — `Y` counts, accents do not |
| `SoundExDM::process` | `(&self, &str) -> String` |
| `SoundExDM::try_process` | `(&self, &str, Option<f64>) -> Result<String, PhoneticError>` |
| `SoundExDM::compare` | `(&self, &str, &str) -> bool` |
| `SoundExDM::find_rules` | `(&self, &str) -> Rules` |
| `SoundExDM::normalize_length` | `(&self, &str, Option<f64>) -> Result<String, PhoneticError>` |
| `process` on the other seven | `(&self, &str) -> String` — infallible on any input; `DaitchMokotoff::process` returns the branch codes `\|`-joined |
| `compare` on the other seven | `(&self, &str, &str) -> bool` — key equality, except `MatchRatingApproach::compare` (the MRA match decision) and `DaitchMokotoff::compare` (true when any code is shared) |
| `Nysiis::with_strict` / `Nysiis::is_strict` | `(bool) -> Nysiis` / `(&self) -> bool` |
| `Phonex::with_max_code_length` / `Phonex::max_code_length` | `(usize) -> Phonex` / `(&self) -> usize` |
| `RefinedSoundex::difference` | `(&self, &str, &str) -> usize` — positions at which the two codes agree |
| `DaitchMokotoff::codes` | `(&self, &str) -> Vec<String>` — every branch code; `codes(x)[0]` is the non-branching walk's code |

### Free functions

| Function | Signature |
|---|---|
| `phoneticize_tokens` | `<'a, T: IntoIterator<Item = &'a str>, O>(tokens: T, keep_stops: bool, process: impl FnMut(&'a str) -> O) -> Vec<O>` |
| `phoneticize_tokens_with` | `<'a, T: IntoIterator<Item = &'a str>, O>(tokens: T, stop_words: &StopWords, keep_stops: bool, process: impl FnMut(&'a str) -> O) -> Vec<O>` |
| `par_encode_batch` (feature `parallel`) | `<P: Phonetic + Sync>(phonetic: &P, tokens: &[&str], chunk_size: usize) -> Vec<String>` |
| `par_encode_double_batch` (feature `parallel`) | `<P: DoubleKeyPhonetic + Sync>(phonetic: &P, tokens: &[&str], chunk_size: usize) -> Vec<(String, String)>` |
| `DEFAULT_CHUNK_SIZE` (feature `parallel`) | `pub const DEFAULT_CHUNK_SIZE: usize = 64;` — a tuning starting point, not a claim of optimality for your data |

### Module `units`

`Unit` trait (implemented for `u8` and `u16`), `eq_ascii_slice`,
`any_ascii_slice`, `push_ascii`, `to_utf16`, `utf16_len`, `utf16_to_lowercase`,
`utf16_to_uppercase`, `uppercase_utf16`, `is_trim_units_unit`, `trim_units`,
`clamp_take`, `clamp_slice_end`, `coerce_or_default`.
