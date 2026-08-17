# Phonetics

`verbora-phonetics` maps a word to a short **key** so that words which sound
alike collide. Four encoders are implemented — `SoundEx`, `Metaphone`,
`DoubleMetaphone` and `SoundExDM` (Daitch–Mokotoff) — each one pinned by its
own suite of recorded test cases. They are the
building block for fuzzy name lookup, deduplication and search *blocking*: encode
every record once, group by key, and only then run an expensive
[string metric](../features/distance.md) inside each group.

<div class="callout callout-spec">
<strong>Specification status.</strong> All four encoders, all 30 exposed
pipeline stages, <code>compare</code>, <code>find_rules</code>,
<code>normalize_length</code> and <code>is_vowel</code> are documented and
test-pinned, across the full range of <code>max_length</code> values including
the rejected ones — which return <code>Err</code>, never a silent fallback.
<code>cargo test -p verbora-phonetics</code> runs <strong>112</strong> unit
tests and <strong>9</strong> doctests.
</div>

## When to use it

- **Blocking a fuzzy-match pipeline.** Comparing every pair of a million records
  with Levenshtein is quadratic; bucketing by phonetic key first is linear, and
  only same-bucket pairs need the metric.
- **Name search that tolerates spelling.** `Robert`/`Rupert` share `R163`;
  `phonetics`/`fonetix` share `FNTKS`.
- **You need every historical quirk pinned, not silently fixed.** These
  functions' outputs — bugs included — are fixed by the crate's own
  regression suite, so an index you built stays valid release over release.

## When not to use it

- **Non-English text.** Every rule table is English (or, for `SoundExDM`,
  Latin-script Ashkenazi surnames). `Москва` encodes as `М000` under `SoundEx`
  and as `МОСКВА` under `Metaphone` — the algorithms pass unknown letters through
  rather than failing, which is quiet rather than useful. See
  [Unicode and language notes](#unicode-and-language-notes).
- **Ranking.** A phonetic key is a yes/no bucket, not a score. For "how similar
  are these two strings" use [distance metrics](../features/distance.md).
- **Whole sentences.** All four take a *token*. Feed a sentence and you get one
  key for the whole thing (`Metaphone::process("ch ch")` is `"SH KSH"`, spaces
  and all). Tokenize first — see [Tokenizers](../features/tokenizers.md).
- **As a hash or an identifier.** Keys are lossy by design, short, and (for
  `SoundEx` and `Metaphone`) can contain arbitrary input characters.

## Quick example

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

fn main() {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();
    let dm = SoundExDM::new();

    // One key each; two for Double Metaphone.
    assert_eq!(soundex.process("Robert"), "R163");
    assert_eq!(soundex.process("Rupert"), "R163");

    assert_eq!(metaphone.process("phonetics"), "FNTKS");
    assert_eq!(metaphone.process("fonetix"), "FNTKS");

    assert_eq!(
        double.process("astromech"),
        ("ATRMX".to_owned(), "ATRMK".to_owned())
    );

    assert_eq!(dm.process("MOSKOWITZ"), dm.process("MOSKOVITZ"));
    assert_eq!(dm.process("ALPERT"), "087930");
}
```

All four types are zero-sized (`#[derive(Debug, Clone, Copy, Default, PartialEq,
Eq)]`, `const fn new()`), hold no state and are trivially `Send + Sync`. Create
them wherever it reads best; there is nothing to cache.

## The four encoders

| Encoder | Keys | Default length | Output alphabet | Good for |
|---|:--:|---|---|---|
| `SoundEx` | 1 | 4 characters — the initial, then 3 digits | the token's first character (uppercased) + `0`–`6` | Coarse blocking of English surnames. Cheapest, and the most collisions. |
| `Metaphone` | 1 | 32 characters | uppercase letters, plus `0` for the `th` sound; unknown input characters pass through | General English words, where you want more precision than four characters buys. |
| `DoubleMetaphone` | **2** | 32 characters *per key* | uppercase letters, `0`, and a literal space in one edge case | English text containing names of mixed origin. Index both keys; a match on **either** counts. |
| `SoundExDM` | 1 | 6 digits | digits — plus, in one bug, the literal text `undefined` | Slavic, Germanic and Ashkenazi-Jewish surnames. Handles `SCH`, `CZ`, `TSCH`, `RZ` clusters that `SoundEx` flattens. |

<div class="callout callout-warn">
<strong>Careful.</strong> "Default length" is what you get from
<code>process()</code>. It is not an upper bound on the result: <code>SoundEx</code>
and <code>Metaphone</code> uppercase <em>after</em> truncating, and case mapping
can grow a string, so <code>Metaphone::process_with(&amp;"ß".repeat(40),
Some(3.0))</code> is the six-character <code>"SSSSSS"</code>. See
<a href="#common-mistakes">Common mistakes</a>.
</div>

The same surname through all four:

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

fn main() {
    let name = "Schwarzenegger";
    assert_eq!(SoundEx::new().process(name), "S625");
    assert_eq!(Metaphone::new().process(name), "SKHWRSNJR");
    assert_eq!(
        DoubleMetaphone::new().process(name),
        ("XRSNKR".to_owned(), "XFRTSNKR".to_owned())
    );
    assert_eq!(SoundExDM::new().process(name), "479465");
}
```

## Choosing the right API

There are three independent choices here: **which encoder**, **which entry
point** on that encoder, and **how to run it over a token stream**. They are
covered in that order.

### Decision tree — which encoder

```text
I need a phonetic key
│
├── English surnames, and I want the cheapest possible blocking key
│      └── SoundEx           4 characters, very coarse
│
├── General English words, one key, better precision
│      └── Metaphone         up to 32 characters
│
├── English text with names of many origins, and I can index two keys
│      └── DoubleMetaphone   two keys; match on either
│
└── Slavic / Germanic / Ashkenazi-Jewish surnames
       └── SoundExDM         6 digits, multi-letter clusters
```

<div class="callout callout-note">
<strong>Note.</strong> "Daitch–Mokotoff" normally means an algorithm that returns
<em>several</em> codes for an ambiguous spelling. <code>SoundExDM</code> is the
single-code variant: the genuine dual codes for <code>CK</code>, <code>RS</code>
and <code>RZ</code> are present in the transcribed table and never read, because
<code>SoundExDM</code> always takes the first legal state
(<code>legalState[0]</code>). If you need real D-M
multi-coding, this is not it — and substituting a correct implementation would
change established output.
</div>

### Comparison table — encoders

| Encoder | Keys | Scan | `Phonetic` impl | `DoubleKeyPhonetic` impl | Fallible entry point | UTF-16 entry point |
|---|:--:|---|:--:|:--:|:--:|:--:|
| `SoundEx` | 1 | one pass | ✅ | ❌ | `try_process` | `try_process_utf16` |
| `Metaphone` | 1 | 30 rewrite passes over 2 buffers | ✅ | ❌ | ❌ (cannot fail) | `process_utf16` |
| `DoubleMetaphone` | 2 | one pass | ❌ | ✅ | ❌ (cannot fail) | ❌ (output is ASCII) |
| `SoundExDM` | 1 | one pass + bounded trie walk | ✅ | ❌ | `try_process` | ❌ (output is ASCII) |

`DoubleMetaphone` deliberately does **not** implement `verbora_core::Phonetic` —
it has no single key to return. It implements `verbora_core::DoubleKeyPhonetic`
(`process_double`) instead. `SoundExDM` implements only `Phonetic`, because
despite the name it produces one code.

### `process()`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>String</code> (a <code>(String, String)</code> pair for <code>DoubleMetaphone</code>)</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Always at least one <code>String</code>; see <a href="#allocation-behaviour">Allocation behaviour</a> for exact counts</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — there is no <code>_into</code> variant</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_encode_batch</code> / <code>par_encode_double_batch</code> (feature <code>parallel</code>)</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes, chunked — <code>par_encode_batch</code> / <code>par_encode_double_batch</code>, feature <code>parallel</code>; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Everything, unless you need a non-default length or the fallible <code>Err</code>-returning behaviour instead of the lenient default</span></div>
</div>

The default. All four encoders have it, with the signature
`fn process(&self, token: &str) -> String` (`-> (String, String)` for
`DoubleMetaphone`).

Two of the four are lenient by design, on the grounds that a text-processing
library should not fail on punctuation:

- **`SoundEx::process` never fails.** A token starting with `(`, `)`, `*`,
  `+`, `?`, `[` or `\` — regular-expression metacharacters — would otherwise
  make constructing an anchored pattern from the token's first character
  fail. `process` skips the initial-sound strip for those characters and
  returns a code anyway, instead of surfacing that as an error.
- **`SoundExDM::process` never fails**, because the default code length of 6
  is always a valid array length. Only a custom length can reach the
  fallible path (see `try_process` below).

### `process_with(token, max_length)`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

Available on `SoundEx`, `Metaphone` and `DoubleMetaphone`. **Not** on
`SoundExDM`, whose length-taking entry point is `try_process` (see below).

The argument is `Option<f64>`, not `Option<usize>`, because its odd values —
falsy zero, `NaN`, negative — are individually significant and observable in
the output. Read the coercions from the source, not from intuition:

```rust
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();

    // `max_length`'s odd values (falsy zero, NaN, negative) are individually significant.
    assert_eq!(soundex.process_with("phonetics", Some(2.0)), "P5");
    assert_eq!(soundex.process_with("phonetics", Some(1.0)), "P532"); // 1 && 0 is falsy
    assert_eq!(soundex.process_with("phonetics", Some(0.0)), "P532"); // falsy
    assert_eq!(soundex.process_with("phonetics", Some(-1.0)), "P");

    assert_eq!(metaphone.process_with("phonetics", Some(4.0)), "FNTK");
    assert_eq!(metaphone.process_with("phonetics", Some(-1.0)), "");
    // Truncation happens BEFORE uppercasing, and case mapping can grow a string.
    assert_eq!(metaphone.process_with(&"ß".repeat(40), Some(3.0)), "SSSSSS");
}
```

`SoundEx` computes `(maxLength && maxLength - 1) || 3` and feeds that to
`substr`, which is why `Some(1.0)` behaves like the default rather than
producing a one-character code. `Metaphone` computes `maxLength || 32` and feeds
`substring`. `None`, `Some(0.0)` and `Some(f64::NAN)` all select the default in
every case.

### `try_process(…)`

<span class="badge badge-fallible">FALLIBLE</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Result&lt;String, PhoneticError&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Identical to <code>process</code> on the success path; none on the error path</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Getting an explicit error instead of the lenient fallback, on inputs that would otherwise need special-casing</span></div>
</div>

Only `SoundEx` and `SoundExDM` have one, and they guard different things:

| Method | Signature | Guards against |
|---|---|---|
| `SoundEx::try_process` | `(&str, Option<f64>) -> Result<String, PhoneticError>` | `PhoneticError::InvalidInitialPattern(char)` — the token's first character is a regular-expression metacharacter |
| `SoundExDM::try_process` | `(&str, Option<f64>) -> Result<String, PhoneticError>` | `PhoneticError::InvalidArrayLength(f64)` — padding needs `new Array(n)` and `n` is not a non-negative integer below 2³² |

**When you genuinely need `SoundEx::try_process`:** whenever your tokens can
start with punctuation and you want that surfaced as an error rather than
silently handled. That is not hypothetical — a punctuation-preserving
tokenizer emits `(` as a token of its own:

```rust
use verbora_phonetics::{PhoneticError, SoundEx};
use verbora_tokenizers::WordPunctTokenizer;

fn main() {
    let soundex = SoundEx::new();

    // Lenient: no initial-sound strip, and a code comes back.
    assert_eq!(soundex.process("(abc"), "(120");

    // Strict: '(' is a regex metacharacter, so this returns Err instead.
    assert_eq!(
        soundex.try_process("(abc", None),
        Err(PhoneticError::InvalidInitialPattern('('))
    );

    // Not a theoretical case — a punctuation-preserving tokenizer emits `(`.
    let tokens = WordPunctTokenizer::new()
        .tokenize("(see figure 2)")
        .expect("this tokenizer never returns null");
    assert!(tokens.iter().any(|t| t.as_str() == Some("(")));
}
```

Only those seven characters throw. `.` is a wildcard and eats one transformed
character (`try_process(".bcd", None)` is `".230"`); `^`, `$` and `|` match the
empty string and change nothing.

**When you need `SoundExDM::try_process`:** any time you pass a non-default code
length, because it is the only way to pass one at all.

```rust
use verbora_phonetics::{PhoneticError, SoundExDM};

fn main() {
    let dm = SoundExDM::new();

    // Padding goes through `new Array(n).join('0')`, which throws for 2.5.
    assert_eq!(
        dm.try_process("ALPERT", Some(6.5)),
        Err(PhoneticError::InvalidArrayLength(2.5))
    );
    // No padding needed, so `new Array` is never reached.
    assert_eq!(dm.try_process("LONGWORDXYZ", Some(3.0)).unwrap(), "865");
    // `slice`, not `substring`: a negative length counts back from the end.
    assert_eq!(dm.try_process("ALPERT", Some(-1.0)).unwrap(), "0879");
}
```

Note the asymmetry: the throw only fires when *padding* is required. `6.5` on a
six-digit code needs 0.5 zeros and throws; `6.5` on a seven-digit code truncates
and succeeds.

### `try_process_utf16()` / `process_utf16()`

<span class="badge badge-utf16">UTF-16</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;u16&gt;</code> (wrapped in <code>Result</code> for <code>SoundEx</code>)</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec&lt;u16&gt;</code>, plus the same working buffers <code>process</code> uses</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Getting the exact UTF-16 code units when the <code>String</code>-returning methods' <code>U+FFFD</code> substitution would lose information</span></div>
</div>

Two encoders can produce a key containing an **unpaired surrogate**, which a Rust
`String` cannot hold:

- `SoundEx` keeps `token.charAt(0)` — the first UTF-16 *code unit* — so an astral
  first character leaves its high surrogate orphaned at the front of the code.
- `Metaphone` truncates to `maxLength` code units, and the cut can fall between
  the two halves of a pair.

The `String`-returning methods substitute `U+FFFD`; these return the exact code
units.

```rust
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();

    // SoundEx keeps `charAt(0)` — the first CODE UNIT — so an astral initial
    // orphans a high surrogate.
    assert_eq!(
        soundex.try_process_utf16("😀", None).unwrap(),
        vec![0xD83D, 0x30, 0x30, 0x30]
    );
    assert_eq!(soundex.process("😀"), "\u{FFFD}000");

    // Metaphone truncates at a code-unit boundary, which can split a pair.
    assert_eq!(metaphone.process_utf16("😀", Some(1.0)), vec![0xD83D]);
    assert_eq!(metaphone.process_with("😀", Some(1.0)), "\u{FFFD}");
}
```

`SoundEx`'s version is fallible (`try_process_utf16`) and takes the same
`Option<f64>` length; `Metaphone`'s is infallible (`process_utf16`) and also
takes the length, so there is no `process_utf16(token)` shorthand — pass `None`.
There is **no** `SoundEx::process_utf16`: if you want code units, you accept the
`Result`.

`DoubleMetaphone` and `SoundExDM` need no such method. Both build their output
from ASCII literals only, so a `String` always represents it exactly.

### `compare()` versus comparing two `process()` results

**`compare` does not short-circuit.** Read the bodies:

- `SoundEx::compare`, `Metaphone::compare` and `SoundExDM::compare` are literally
  `self.process(a) == self.process(b)` — two full encodings, two `String`
  allocations, every call.
- `DoubleMetaphone::compare` is `pa == pb || sa == sb` after encoding *both*
  sides — four keys, four `String` allocations. It overrides
  `verbora_core::Phonetic`'s default `compare`, which only applies to a
  single-key `process`; a two-key encoder needs its own "match on either"
  rule instead.

The `verbora_core::Phonetic::compare` default body has the same shape, despite a
doc comment saying it "avoids allocating the second key when the implementation
can compare incrementally" — no implementation in this workspace does that.

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone};

fn main() {
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();

    assert!(metaphone.compare("phonetics", "fonetix"));
    // Exactly equivalent — `compare` does not short-circuit.
    assert_eq!(
        metaphone.compare("phonetics", "fonetix"),
        metaphone.process("phonetics") == metaphone.process("fonetix")
    );

    // Double Metaphone matches on EITHER key, so this is not the same test.
    assert!(double.compare("love", "luv"));
    let (pa, sa) = double.process("love");
    let (pb, sb) = double.process("luv");
    assert!(pa == pb || sa == sb);
}
```

| You want | Use |
|---|---|
| A single ad-hoc "do these two sound alike?" test | `compare(a, b)` — it reads better and costs the same |
| The `DoubleMetaphone` "either key matches" rule | `compare(a, b)` — reproducing it by hand is easy to get wrong |
| To test one word against many, or many against many | `process` **once per word**, then compare the keys |
| To store the key | `process` — `compare` throws its keys away |

The last two rows matter. `compare` in a nested loop is *O(n²)* encodings;
precomputing is *n* encodings and *O(n²)* `&str` comparisons.

```rust
use verbora_phonetics::SoundEx;

fn main() {
    let names = ["Robert", "Rupert", "Ashcraft"];
    let soundex = SoundEx::new();

    // n encodings, then O(n^2) &str comparisons — not O(n^2) encodings.
    let keys: Vec<String> = names.iter().map(|n| soundex.process(n)).collect();
    let mut matches = 0;
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            if keys[i] == keys[j] {
                matches += 1;
            }
        }
    }
    assert_eq!(matches, 1);
}
```

### `phoneticize_tokens_with` versus `phoneticize_tokens`

Both encode an already-tokenized stream and drop stop words — the reusable
half of "tokenize, then phoneticize." Tokenizing lives in a different crate
(`verbora-tokenizers`), so there is no string-input wrapper here — these
take the tokens directly, and you choose the tokenizer.

```rust  ignore
pub fn phoneticize_tokens<'a, T, O>(
    tokens: T,
    keep_stops: bool,
    process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where T: IntoIterator<Item = &'a str>;

pub fn phoneticize_tokens_with<'a, T, O>(
    tokens: T,
    stop_words: &verbora_core::StopWords,
    keep_stops: bool,
    process: impl FnMut(&'a str) -> O,
) -> Vec<O>
where T: IntoIterator<Item = &'a str>;
```

| | `phoneticize_tokens` | `phoneticize_tokens_with` |
|---|---|---|
| Stop-word source | a **process-global mutable** list | A `&StopWords` you own |
| Reproducible | ❌ — any call to `verbora_core::stopwords::add_global_stopword` anywhere in the process changes the result | ✅ |
| Thread-safe in the "no surprises" sense | ⚠️ Safe, but the global sits behind an `RwLock` another thread can write | ✅ No shared state |
| Badge | <span class="badge badge-global">GLOBAL STATE</span> | — |

**Default to `phoneticize_tokens_with`.** `phoneticize_tokens` reads
`verbora_core::stopwords::is_default_stopword`, which consults a
`LazyLock<RwLock<StopWords>>` that `add_global_stopword` /
`remove_global_stopword` can mutate from anywhere in the process. That
global-mutable-state design is exactly the property that makes a test suite
order-dependent and a multi-threaded worker non-deterministic. Passing a
`&StopWords` costs one line and removes the whole category of problem. (There *is* a fast path: while the global has never been
mutated, lookups are a lock-free binary search over a static sorted slice. Once
anything mutates it, every lookup takes the read lock.)

The `T: IntoIterator<Item = &'a str>` bound is the useful part of the signature:
a tokenizer's lazy `tokens()` iterator satisfies it directly, so there is no
intermediate `Vec` of tokens.

```rust
use verbora_core::StopWords;
use verbora_phonetics::{DoubleMetaphone, Metaphone, phoneticize_tokens_with};
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let tokenizer = AggressiveTokenizer::new();
    let metaphone = Metaphone::new();
    let stops = StopWords::english();

    // `tokens()` is lazy and yields `&str`, which is exactly the
    // `IntoIterator<Item = &str>` this takes — no intermediate `Vec`.
    let keys = phoneticize_tokens_with(
        tokenizer.tokens("the quick brown fox"),
        &stops,
        false,
        |t| metaphone.process(t),
    );
    assert_eq!(keys, ["KK", "BRN", "FKS"]);

    // Two keys per token works too: the closure's return type is free.
    let double = DoubleMetaphone::new();
    let pairs = phoneticize_tokens_with(["phonetic", "modules"], &stops, false, |t| {
        double.process(t)
    });
    assert_eq!(
        pairs,
        [
            ("FNTK".to_owned(), "FNTK".to_owned()),
            ("MTLS".to_owned(), "MTLS".to_owned())
        ]
    );
}
```

Only tokenizers that yield `&'a str` compose this directly — the
`AggressiveTokenizer` family, and `WordTokenizer` once you have unwrapped its
`Option`. (`RegexpTokenizer` yields `Option<&'a str>` items, so it needs a
`.flatten()` first.) Tokenizers that yield `Cow<'a, str>` or
`Utf16Token<'a>` do **not**: neither can hand back a `&'a str` borrowed from the
original input (`Utf16Token::as_str` returns `Option<&str>` tied to the token,
not to the text). For those, collect the tokens first and pass a slice of the
collected `&str`s, or skip the helper and call `process` in your own loop.

Two behaviours worth knowing:

- **Filtering is case-sensitive and tests the raw token.** `the` is dropped;
  `The` is not.
- **Tokens are encoded verbatim, punctuation included.** `it's`, `co-op` and
  `/path` all reach the encoder as-is.

```rust
use verbora_phonetics::{Metaphone, phoneticize_tokens};

fn main() {
    let metaphone = Metaphone::new();
    // Reads the process-global list that `add_global_stopword` can mutate.
    let keys = phoneticize_tokens(["the", "quick"], false, |t| metaphone.process(t));
    assert_eq!(keys, ["KK"]);
    // Filtering is case-sensitive and tests the RAW token.
    let keys = phoneticize_tokens(["The", "quick"], false, |t| metaphone.process(t));
    assert_eq!(keys, ["0", "KK"]);
}
```

### `par_encode_batch` / `par_encode_double_batch` — parallel batch (feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, chunked across a <code>rayon</code> thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;String&gt;</code> (<code>Vec&lt;(String, String)&gt;</code> for the double variant)</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One output <code>Vec</code>, plus exactly what <code>process</code>/<code>process_double</code> allocates per token — no extra buffering per chunk</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Yes — this is the batch entry point</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — feature <code>parallel</code>; chunked (<code>par_chunks</code>), **not** one task per word</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Building an index over tens of thousands of words or more</span></div>
</div>

Behind this crate's `parallel` Cargo feature (`parallel = ["dep:rayon"]`,
never on by default), `par_encode_batch` fans `Phonetic::process` out across a
`rayon` thread pool for `SoundEx`, `Metaphone` and `SoundExDM`, and
`par_encode_double_batch` does the same for `DoubleKeyPhonetic::process_double`
(`DoubleMetaphone`). Both call the exact same encoder the sequential API
uses — there is no second implementation of any of the four algorithms to keep
in sync.

```rust  ignore
use verbora_phonetics::{SoundEx, par_encode_batch};

let soundex = SoundEx::new();
let words = ["Robert", "Rupert", "phonetics"];
let keys = par_encode_batch(&soundex, &words, 2);
assert_eq!(keys, ["R163", "R163", "P532"]);
```

```rust  ignore
use verbora_phonetics::{DoubleMetaphone, par_encode_double_batch};

let dm = DoubleMetaphone::new();
let words = ["astromech", "Matrix"];
let keys = par_encode_double_batch(&dm, &words, 2);
assert_eq!(
    keys,
    [
        ("ATRMX".to_owned(), "ATRMK".to_owned()),
        ("MTRKS".to_owned(), "MTRKS".to_owned()),
    ]
);
```

<div class="callout callout-note">
<strong>Note.</strong> Both blocks above need the <code>parallel</code>
feature enabled on <code>verbora-phonetics</code>, which this site's own
snippet checker builds without, so they are marked <code>ignore</code> rather
than compiled — every other block on this page compiles and runs in CI.
</div>

**Why chunked, and not one `rayon` task per word.** A single call to
`process` costs on the order of tens to low hundreds of nanoseconds — this
crate's own module documentation puts it at roughly 40–200 ns depending on
the encoder and the word, and the cross-crate write-up in
[Parallelism](../performance/parallelism.md) narrows that to ~42–183 ns/word —
either way, the same order of magnitude as a `rayon` task's own scheduling
cost. Dispatching one task per word
(`words.par_iter().map(soundex)`) was measured as *noisy* rather than
uniformly bad: across repeated runs it ranged from more than 2× slower than
the plain sequential loop to several times faster, depending on host load and
`rayon`'s work-stealing state at the moment — predictable behaviour, not raw
speed, is the point of chunking. `par_encode_batch` and
`par_encode_double_batch` instead hand each `rayon` task an explicit **chunk**
of words, via `chunk_size` — a required argument, not a hidden default, in
keeping with this project's rule that Rayon-backed APIs are opted into
explicitly. `DEFAULT_CHUNK_SIZE` (`64`) is a measured starting point, tuned on
a 32-core machine against `SoundEx`; tune from there for your own word
lengths and core count. Passing `1` reproduces the discouraged one-task-per-word
form — legal, and useful only for the tests and benchmark that measure
the difference.

**When to reach for it vs. the sequential loop.** A handful of words, or
words arriving one at a time (a per-HTTP-request lookup): call `process`
directly, or loop with `.iter().map(...)`. Building an index over tens of
thousands of words or more, where the encoding only has to happen once: this
is the intended use. Anywhere in between, measure — the crossover point moves
with core count and how busy the machine already is. See
[Parallelism](../performance/parallelism.md) and
`crates/verbora-phonetics/src/parallel.rs`'s module documentation for the
full reasoning and the benchmark command
(`cargo bench -p verbora-phonetics --features parallel -- parallel_batch`).

## Choosing a Phonetic Algorithm

The [decision tree above](#decision-tree-—-which-encoder) answers "which
encoder for this use case" — coarse blocking vs. precision vs. two-key
recall vs. Slavic/Germanic surnames. This section answers a different
question: **which encoder for this *language*, specifically**, and it comes
with a harder constraint than the use-case tree does. Read
[The four encoders](#the-four-encoders) above for what each one actually
does mechanically; what follows is about fit, not mechanics.

<div class="callout callout-warn">
<strong>There is no "best" phonetic algorithm.</strong> Every encoder below
is an English-oriented algorithm — none has a language-specific variant, and
none of the four was written with Cyrillic, Devanagari, Han or Arabic
phonotactics in mind (see <a href="#unicode-and-language-notes">Unicode and
language notes</a>). "Recommended" below means <em>the closest fit among
Verbora's four encoders, given that constraint</em> — never an unqualified
claim that an encoder is correct for a language in the way it is correct for
English surnames.
</div>

A fifth category sometimes appears in phonetic-matching literature —
**Kölner Phonetik ("Cologne phonetic")**, tuned for German. It is **not
implemented anywhere in this workspace** (`grep -ri cologne crates/` finds
nothing but an unrelated JSON fixture key). It is not on this page as a
fifth row in the table below, and there is no page for it on this site —
naming an algorithm here that does not exist would be exactly the invented
API this site's own [editorial rules](../reference/docs-are-code.md#honesty-rules)
forbid. German gets `SoundExDM` (Daitch–Mokotoff) instead — see the table.

### Per-language table

Sourced directly from [`verbora-language`](../features/language.md)'s
[`recommend()`](../features/language.md#phonetic-strategy-recommend)
function — a closed `match` over all 22 languages that crate has a strategy
for, not a separate opinion maintained on this page. If `recommend()`
changes, this table is stale; it is not an independent source of truth.

| Language | Recommended | Alternative(s) | Category |
|---|---|---|---|
| English | `DoubleMetaphone` | `Metaphone`, `SoundEx` | Recommended |
| German | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Dutch | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Swedish | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Norwegian | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Finnish | `SoundExDM` | `DoubleMetaphone`, `SoundEx` | Recommended |
| Spanish | `DoubleMetaphone` | `SoundEx` | Recommended |
| Portuguese | `DoubleMetaphone` | `SoundEx` | Recommended |
| Italian | `DoubleMetaphone` | `SoundEx` | Recommended |
| French | `DoubleMetaphone` | `SoundEx` | Recommended |
| Galician | `DoubleMetaphone` | `SoundEx` | Recommended |
| Catalan | `DoubleMetaphone` | `SoundEx` | Recommended |
| Basque | `DoubleMetaphone` | `SoundEx` | Recommended |
| Indonesian | `DoubleMetaphone` | `SoundEx` | Recommended |
| Vietnamese | `DoubleMetaphone` | `SoundEx` | Recommended |
| Polish | `SoundExDM`* | `DoubleMetaphone` | Recommended, with a caveat* |
| Ukrainian | `SoundExDM`* | `DoubleMetaphone` | Recommended, with a caveat* |
| Russian | `SoundExDM`* | `DoubleMetaphone` | Recommended, with a caveat* |
| Japanese | `DoubleMetaphone`† | `SoundEx` | Recommended, after transliteration† |
| Persian | — | — | **Not designed for this language** |
| Hindi | — | — | **Not designed for this language** |
| Chinese | — | — | **Not designed for this language** |

**\*** Polish, Ukrainian and Russian get a primary recommendation, but
`recommend()` also reports `TransliterationAdvice::Unsupported` for all
three — Verbora has no Cyrillic transliterator, so this only produces a
meaningful key once *you* have romanized the input yourself. `SoundExDM`
(built for exactly this family of Slavic/Germanic surname variation) is the
honest choice once that has happened, not before.

**†** Japanese gets `TransliterationAdvice::Recommended`: applying any of
the four encoders directly to native kana/kanji is not meaningful (see
[Unicode and language notes](#unicode-and-language-notes) — non-Latin
characters mostly pass through unchanged), so the recommendation assumes
`verbora_transliterators::transliterate_ja` runs first. See
[Language § Transliteration Integration](language.md#transliteration-integration)
for the composed example.

**Persian, Hindi and Chinese get no primary recommendation at all** —
`recommend()` returns `primary: None` for exactly these three, on purpose.
Two things are true at once for each: Verbora has no transliterator for
Arabic, Devanagari or Han script, *and* none of the four encoders were
designed for those languages' phonotactics (Chinese is also tonal, which
none of the four model). Recommending `SoundEx` or `Metaphone` anyway would
produce a key — the algorithms don't refuse non-Latin input, they pass it
through quietly (see [Language coverage](#unicode-and-language-notes) below)
— but that key would carry no real phonetic meaning, which is precisely the
false confidence this feature exists to avoid. "Not designed for this
language" is the honest category for that case, not a lower-confidence
"Alternative."

### How to read "Alternative"

An alternative is **also legitimate**, not a downgrade ranked by quality —
`recommend()`'s own doc comment is explicit that the ordering within
`alternatives` carries no ranking beyond "reasonable." Reach for one instead
of the primary when its trade-off fits your situation better: `SoundEx`
over `DoubleMetaphone` when you want the cheapest possible four-character
key and can tolerate coarser collisions; `Metaphone` over `DoubleMetaphone`
for English when a single key is enough and you don't need the second
code's extra recall. See [Comparison table — encoders](#comparison-table-—-encoders)
above for what each trade-off actually costs.

### Getting this automatically

Everything above assumes you already know the language. If you don't —
you only have a piece of text and need to find out — that determination
belongs to a different crate: [`verbora-language`](language.md) detects
script and language, and calls this exact `recommend()` function for you
once it has an answer it trusts enough to act on. Start there for the "how
do I get a recommendation without knowing the language up front" story,
including why single words and short names are the one case where
automatic detection should not be trusted blindly.

## Deliberate deviations from the published algorithms

Every one of these deviations from a textbook algorithm is observable,
deliberate, and pinned by a unit test and by `fixtures/phonetics.json`. Do
not substitute a textbook implementation or a third-party crate: you would
change the encoding of real words and break compatibility with any index
already built against this crate's output.

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

fn main() {
    // SoundEx condenses BEFORE dropping non-digits.
    assert_eq!(SoundEx::new().process("Ashcraft"), "A226"); // textbook: A261
    // Digits in the input survive `transform` and become part of the code.
    assert_eq!(SoundEx::new().process("12345"), "1234");

    // Metaphone runs transformX after cTransform, so initial `ch` -> `xh` -> `sh`.
    assert_eq!(Metaphone::new().process("chemical"), "SHMKL"); // textbook: KMKL
    // Double Metaphone does not have that bug.
    assert_eq!(
        DoubleMetaphone::new().process("chemical"),
        ("KMKL".to_owned(), "KMKL".to_owned())
    );

    // Double Metaphone: `['C','Q','G'].indexOf(x)` used as a boolean. `C` is at
    // index 0 — falsy — so only a non-C skips two characters.
    let double = DoubleMetaphone::new();
    assert_eq!(double.process("MAC CAT").0, "MKKT");
    assert_eq!(double.process("MAC GAT").0, "MKT");
    // `add('J', 'H')` discards its second argument: both keys get `J`.
    assert_eq!(double.process("jose"), ("JS".to_owned(), "JS".to_owned()));
    // A stray `pos++` swallows the S after `OWSKI`.
    assert_eq!(
        double.process("lebowski"),
        ("LPK".to_owned(), "LPFK".to_owned())
    );

    // SoundExDM: the digit `0` collides with the table's legality marker, and
    // `output += undefined` appends the literal string.
    let dm = SoundExDM::new();
    assert_eq!(dm.process("B0"), "undefi");
    assert_eq!(dm.process("MOSKOWITZ0"), "6457un");
}
```

### `SoundEx` — condensation happens before filtering

This crate condenses before stripping non-digit characters. Textbook SoundEx
removes `h`, `w` and vowels first and only then merges
adjacent equal digits; here the letters are still present when merging happens,
so they **separate** two equal codes instead of letting them collapse.
`Ashcraft` is `A226`, not `A261`.

A second consequence: `transform` maps letters to digits but leaves existing
digits alone, so digits already in the input survive the `\D` filter and become
part of the code. `SoundEx::process("12345")` is `"1234"`.

A third: `padRight0` pads to exactly three characters (`Array(4 - n).join('0')`
yields `3 - n` zeros), so an empty token has no initial letter at all —
`process("")` is `"000"`, three characters, while every other input yields four.

### `Metaphone` — `transformX` runs after `cTransform`

`cTransform` turns a word-initial `ch` into `xh`. `transformX` then runs and
rewrites a leading `x` to `s`. The result is that `chemical` encodes as `SHMKL`
rather than `KMKL`.

Two more orderings in the same pipeline that look like mistakes and are:

- `dropG` runs before `transformG`, and `dropG`'s first pattern contains a dead
  `^$` alternative, so a word-final `gh` survives to become `f` later — which is
  why `tough` is `TF`.
- `dedup` uses `/([^c])\1/g`, which collapses non-overlapping **pairs**, not
  runs: `dedup("aaa")` is `"aa"`, `dedup("aaaa")` is `"aa"`, `dedup("aaaaa")` is
  `"aaa"`. The `c` exemption is case-sensitive (no `/i` flag), so `dedup("CC")`
  is `"C"` while `dedup("cc")` is `"cc"`.
- `transformT`'s `/th/` and `transformZ`'s `/z/` are **not global**: only the
  first occurrence is replaced. `transform_t("thth")` is `"0th"`.

### `DoubleMetaphone` — a family of truthiness accidents

Nine distinct sites, each catalogued in the module documentation. The ones with
the widest reach:

| Site | Source | Effect |
|---|---|---|
| `handleC` | `token.substring(pos-2, pos+1) !== 'WICZ'` | A ≤3-character slice compared with a 4-character literal — **always true**, so `CZ` always takes the `S`/`X` branch. `markiewicz` depends on it. |
| `handleC` | `['C','Q','G'].indexOf(token[pos+2])` used as a condition | `indexOf` returns `0` for `C`, which is falsy — the test is **inverted**. `MAC CAT` encodes its second C; `MAC GAT` skips two characters. |
| `handleJ` | `add('J', 'H')` | `add` takes one parameter, so the `H` is discarded and **both** keys get `J`. |
| `handleR` | `!subMatch(-4, -3, ['ME','MA'])` | A 1-character slice against 2-character literals — **always true**. `papier` depends on it. |
| `handleT` | `token.substring(1, 2) === 'TH'` | A 1-character slice against a 2-character literal — **always false**. |
| `handleH`, `handleW`, `handleZ` | a `pos++` in a branch that consumed nothing | The next character is **silently skipped**: in `LEBOWSKI` the `S` after `OWSKI` is never encoded. |

There are also two structurally unreachable branches (`pos === 1` inside a branch
guarded by `pos !== 1`; a doubled-`L` disjunct that compares a boolean with `-1`)
and a `token[pos+1] !== 'Y'` test inside a branch that has already established
`token[pos+1] === 'N'`.

### `SoundExDM` — the digit `0` collides with the legality marker

This crate's transformation table is a nested object that doubles as a
state machine: a node's `'0'` key holds its code triple and marks the node
*legal*; every other key is a transition. `find_rules` indexes those nodes
with characters taken straight from the input. So the input digit `0`
selects the legality marker instead of a transition, indexing *that* yields
a number, indexing the number resolves to nothing — and this crate
deliberately emits the literal text `undefined` at that point, which
truncation to the default length then cuts down to `"undefi"`.

`process("B0")` is `"undefi"`. `process("MOSKOWITZ0")` is `"6457un"`.
`process("A0")` is `"000000"` — safe only because `A`'s start-of-word code is the
falsy number `0`, which stops the walk. The digits `1` and `2` collide with array
indices in the same way, which is how `find_rules("CK1")` walks into the D-M dual
code the encoder itself never reads.

## Advanced usage

### The exposed pipeline stages

`Metaphone` exposes 21 stage methods and `SoundEx` exposes 9 — public,
individually callable pipeline stages. They exist **for compatibility**, not
for composition: the test suite replays 1,634 recorded cases against each
one.

**A normal user should ignore all of them.** `process` runs the stages in the one
order that produces a correct key, and calling them individually is both slower
(each builds its own buffers) and easy to get wrong (the order is load-bearing,
and two stages actively undo each other). Reach for them only when you are
debugging a surprising result and need to see exactly which stage produced
it.

They differ in return type, and the difference is not decorative:

| Owner | Methods | Returns |
|---|---|---|
| `SoundEx` | `transform_lipps`, `transform_throats`, `transform_toungue`, `transform_l`, `transform_hum`, `transform_r`, `transform`, `condense`, `pad_right0` | `Cow<'a, str>` <a class="badge badge-cow" href="../performance/zero-copy">COW</a> — borrows the input when the rule changes nothing |
| `Metaphone` | `dedup`, `drop_initial_letters`, `drop_b_after_m_at_end`, `c_transform`, `d_transform`, `drop_g`, `transform_g`, `drop_h`, `transform_ck`, `transform_ph`, `transform_q`, `transform_s`, `transform_t`, `drop_t`, `transform_v`, `transform_wh`, `drop_w`, `drop_y`, `transform_x`, `transform_z`, `drop_vowels` | `String` <a class="badge badge-owned" href="../performance/allocation">OWNED</a> — always allocates |

`Metaphone`'s stages cannot borrow: each runs through the internal two-buffer
`Pipe` (so it costs two `Vec` allocations plus the output `String`), and several
stages rewrite in place at every position anyway.

```rust
use verbora_phonetics::{Metaphone, SoundEx};

fn main() {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();

    // SoundEx stages return `Cow<'_, str>` and borrow when nothing changes.
    assert!(matches!(
        soundex.transform("RENDER"),
        std::borrow::Cow::Borrowed(_)
    ));
    assert_eq!(soundex.transform("render"), "6e53e6");
    assert_eq!(soundex.condense("11222556"), "1256");
    assert_eq!(soundex.pad_right0("12"), "120");

    // Metaphone stages always return an owned `String`.
    let s: String = metaphone.c_transform("chch");
    assert_eq!(s, "xhkh");
    assert_eq!(metaphone.dedup("aaa"), "aa");
    assert_eq!(metaphone.drop_h("ohhoh"), "oho");
}
```

Note `soundex.transform("RENDER")` returning `"RENDER"` unchanged: none of the
six class patterns carries the `/i` flag, so every `SoundEx` stage is
lowercase-only. `process` lowercases first, so this only bites callers of the
stages.

### `SoundExDM::find_rules` and `normalize_length`

Also public surface. `find_rules(str) -> Rules` reports the longest legal prefix
and its codes, and `normalize_length(token, Option<f64>) -> Result<String,
PhoneticError>` pads or cuts a code. `Rules { length: usize, mapping: RuleMapping
}` and `RuleMapping::{Triple, Number}` are public so that the digit-collision bug
above is inspectable rather than merely observable.

### Trait objects

`SoundEx`, `Metaphone` and `SoundExDM` implement `verbora_core::Phonetic`, so
they can be held behind a `&dyn Phonetic`:

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

All four encoders are *O(n)* in the token's length, and tokens are short, so the
per-call constants — allocation and case folding — dominate.

| Encoder | Work per token | Notes |
|---|---|---|
| `SoundEx` | one pass over the code units | Six character-class rules are provably a per-code-unit map, so they are fused into a single table lookup. |
| `Metaphone` | only the rewrite passes that fire, over 2 pooled scratch buffers | The 21 stages are fused into one skip-gated driver: a letter mask decides which rules can possibly fire on this word, and a skipped stage touches nothing. The scratch pair here is pooled per thread and reused across calls, rather than allocating a fresh string per rewrite. |
| `DoubleMetaphone` | one left-to-right scan | Builds both keys simultaneously. |
| `SoundExDM` | one scan, with a trie walk per position | The trie is at most 7 deep (`SCHTSCH`), and `pos` advances by at least 1 per walk, so the total is linear. The 26 first-letter entries are a direct index, not a scan. |

None of the rewrites goes through a regular-expression engine. Three of them
*could not*: `dedup` uses a backreference, `transform_t` and `transform_z` are
non-global, and six stages match an alternation that **consumes** its context
character (so adjacent matches are skipped) — none of which the `regex` crate can
express.

**Non-ASCII input costs more.** Each entry point tests `str::is_ascii` (which is
vectorised, so the test is close to free) and picks the narrowest representation
provably identical to UTF-16: `&[u8]` for ASCII, `Vec<u16>` otherwise. Leaving
the fast path adds a `Vec<u16>` promotion and, for `Metaphone`, a UTF-16
uppercase pass and a lossy re-encode.

**With the `parallel` feature**, `par_encode_batch` / `par_encode_double_batch`
(see "Choosing the right API" above) amortise this per-token cost by chunking
rather than dispatching one `rayon` task per word — the per-token cost above
(tens to low hundreds of nanoseconds) is close enough to `rayon`'s own
scheduling cost that naive per-word parallelism is unreliable, sometimes
faster and sometimes markedly slower than the sequential loop. Chunking with
`par_chunks` avoids that unpredictability; see
`crates/verbora-phonetics/src/parallel.rs`'s module documentation for the
measurements behind that design.

> **Not yet benchmarked.** `crates/verbora-phonetics/benches/phonetics.rs` is a
> criterion harness with five groups (`encoders`, `ascii_vs_utf16`, `surnames`,
> `compare`, `metaphone_stages`). No side-by-side comparison against another
> implementation has been published for this crate, so no ratio is quoted
> here. See [Benchmarks](../benchmarks/index.md); `docs/PERFORMANCE.md`
> currently covers `verbora-distance` only.

## Allocation behaviour

<div class="callout callout-warn">
<strong>Careful.</strong> This crate has <strong>no <code>_into</code> API</strong>.
<code>process()</code> returns a freshly allocated, owned <code>String</code>
every single time — <code>DoubleMetaphone::process()</code> returns two. There is
no way to encode into a caller-supplied buffer (<code>Metaphone</code> pools its
internal pipeline scratch per thread, but the returned key is always a fresh
allocation). With the <code>parallel</code> feature there is a chunked batch
entry point (<code>par_encode_batch</code> / <code>par_encode_double_batch</code>,
see "Choosing the right API" above), but it changes how the work is
<em>scheduled</em>, not how much is allocated — it still produces one
<code>String</code> (or pair) per token. If you are encoding millions of
tokens, that is millions of small allocations regardless of which entry point
or feature flags you use, and the levers you have are the ones below.
</div>

Per call, reading the source:

| Encoder | ASCII input | Non-ASCII input |
|---|---|---|
| `SoundEx::process` | The digit `Vec<u8>`, a `String` for the uppercased initial, and usually one growth of that `String` when the digits are appended — up to **three** small allocations | The same, plus the lowercased copy (non-ASCII always allocates one) |
| `Metaphone::process` | The output `String` — **one** in steady state. The pipeline's two scratch buffers are pooled per thread and reused across calls, and ASCII input folds lowercase directly into that scratch | Six: the lowercased `String`, the `Vec<u16>` promotion (which doubles as the pipeline's first scratch), the second scratch buffer, the pipeline-result copy, the uppercase `Vec<u16>`, and the output `String` |
| `DoubleMetaphone::process` | Two `String::with_capacity(16)` accumulators, both returned — **two** | Four: the uppercased `String`, the `Vec<u16>` promotion, and the two accumulators |
| `SoundExDM::process` | One `String::with_capacity(8)`, padded in place — **one** | Three: the uppercased `String`, the `Vec<u16>` promotion, and the output |

Add one allocation to the `SoundEx`, `DoubleMetaphone` and `SoundExDM` ASCII
rows when the input needs case folding (`SoundEx` lowercases first, the other
two uppercase first, and `utf16_to_lowercase` / `utf16_to_uppercase` return
`Cow::Borrowed` when there is nothing to fold). `Metaphone`'s ASCII path is
exempt: it folds byte-wise straight into the pooled scratch, cased input or
not.

### Reducing it at the call site

**1. Encode once, and move the `String` where it belongs.** Do not clone the key,
and do not re-derive it. A `HashMap` key takes the `String` by value:

```rust
use std::collections::HashMap;
use verbora_phonetics::SoundEx;

fn main() {
    let names = ["Robert", "Rupert", "Ashcraft", "Ashcroft"];
    let soundex = SoundEx::new();

    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, name) in names.iter().enumerate() {
        // One `String` per name, moved into the key: no clone, no re-encoding.
        buckets.entry(soundex.process(name)).or_default().push(i);
    }
    assert_eq!(buckets["R163"], vec![0, 1]);
}
```

**2. Never call `compare` in a loop.** See the
[precomputing example](#compare-versus-comparing-two-process-results) above:
it turns *O(n²)* encodings into *n*.

**3. Fold case upstream, once.** If your pipeline already lowercases tokens, feed
`SoundEx` and `Metaphone` the lowercased form and their case-fold `Cow` borrows
instead of allocating. The same holds for `DoubleMetaphone` and `SoundExDM` with
uppercase. Note that this is a different fold for each pair of encoders — you
cannot satisfy all four at once.

```rust
use verbora_phonetics::{SoundEx, SoundExDM};

fn main() {
    // SoundEx and Metaphone lowercase first; Double Metaphone and SoundExDM
    // uppercase first. Feeding an already-folded token skips that allocation.
    assert_eq!(
        SoundEx::new().process("robert"),
        SoundEx::new().process("Robert")
    );
    assert_eq!(
        SoundExDM::new().process("ALPERT"),
        SoundExDM::new().process("Alpert")
    );
}
```

**4. Avoid the individual stage methods in hot code.** `Metaphone::process` runs
30 rewrites through two buffers; calling the 21 public stages by hand allocates
two buffers and one `String` *each*.

**5. Reach for the built-in parallel batch, or roll your own.** All four
encoders are zero-sized, stateless and `Send + Sync`, and `process` takes
`&self`. With the `parallel` feature, `par_encode_batch` /
`par_encode_double_batch` (above) already do this in chunks tuned to avoid
per-word dispatch overhead — prefer them over a hand-rolled `rayon`
`par_iter().map(|w| soundex.process(w))`, which reintroduces exactly the
one-task-per-word cost the chunking exists to avoid. See
[Parallelism](../performance/parallelism.md). Note that `phoneticize_tokens`
reads process-global state, so use `phoneticize_tokens_with` in any parallel
context, including alongside these batch functions.

Further reading: [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md),
[Performance](../performance/index.md).

## Unicode and language notes

This crate indexes strings by UTF-16 code unit, and these algorithms index,
slice and truncate constantly. A character outside the Basic Multilingual
Plane counts as **two**, and a rule can match one half of a surrogate pair.
That is observable:

```text
Metaphone.dedup("😀😀")
  UTF-16 code units : "😀😀"   the four code units are D83D DE00 D83D DE00 —
                              no two ADJACENT ones are equal, so nothing collapses
  Rust chars        : "😀"     two identical `char`s collapse to one
```

The `units` module is how that exactness is paid for. The algorithms are written
once, generically over a `Unit` trait implemented for `u8` and `u16`, and each
entry point picks the narrowest representation *provably identical* to UTF-16 for
the given input:

| Input | Representation | Allocates |
|---|---|---|
| ASCII | `&[u8]` | no |
| anything else | `Vec<u16>` | yes |

For ASCII one byte *is* one UTF-16 code unit, so the fast path is the same
computation on a narrower type — not an approximation. `str::is_ascii` is
vectorised, so the dispatch itself is close to free.

`units` is public because these string primitives are the pieces you need to
reason about the coercions: `utf16_len` (UTF-16 code-unit length),
`utf16_to_lowercase` / `utf16_to_uppercase` (`Cow`-returning), `uppercase_utf16`
(preserves unpaired surrogates, which `String::from_utf16_lossy` would destroy),
`trim_units` (a whitespace set that includes `U+FEFF` and excludes `U+0085` —
Rust's `str::trim` is the other way round), `clamp_take` (`substring`/`substr`-style
clamping), `clamp_slice_end` (`slice`-style, where a negative counts from the
end) and `coerce_or_default` (falsy-or-default coercion).

**Unpaired surrogates.** Two encoders can produce one — `SoundEx` (it keeps
`charAt(0)`) and `Metaphone` (it truncates mid-pair). Their `String`-returning
methods render it as `U+FFFD`; `SoundEx::try_process_utf16` and
`Metaphone::process_utf16` return the exact code units. `DoubleMetaphone` and
`SoundExDM` emit only ASCII, so they need no such method.

**Language coverage.** All four are English-language algorithms, and none rejects
input it does not understand:

- `SoundEx` uppercases the first character and passes unknown letters through the
  digit filter, so `café` is `C100`, `ÉCOLE` is `É240`, `Москва` is `М000` and
  `日本語` is `日000`. Case mapping can lengthen the code: `ß` is `SS000`, five
  characters.
- `Metaphone` passes non-ASCII through untouched: `café` is `KFÉ`, `Москва` is
  `МОСКВА`, `😀` is `😀`.
- `DoubleMetaphone` names exactly five non-ASCII characters in its switch — `Ê`,
  `É`, `À` (word-initial vowels), `Ç` → `S` and `Ñ` → `N`. Every other accented
  letter contributes nothing, so `Москва`, `日本語` and `😀` all encode as
  `("", "")`. Note its `is_vowel` counts `Y` as a vowel and rejects `É` — even
  though the main switch treats `É` as a word-initial vowel.
- `SoundExDM` uppercases first (so `ß` becomes `SS` and encodes as `400000`) and
  treats any non-ASCII code unit as an unknown key that stops the trie walk.

## Common mistakes

**Assuming the result is no longer than `max_length`.** `SoundEx` and `Metaphone`
uppercase *after* truncating, and case mapping can grow a string (`ß` → `SS`,
`ﬁ` → `FI`). `Metaphone::process_with(&"ß".repeat(40), Some(3.0))` is `"SSSSSS"`.

**Expecting `Some(1.0)` to give a one-character `SoundEx` code.**
`process_with` computes `(max_length && max_length - 1) || 3`, and `1 - 1` is
falsy, so `Some(1.0)` selects the default. Use `Some(-1.0)` to get just the
initial letter.

**Calling `compare` in a nested loop.** It re-encodes both sides every time, and
allocates two (or four) `String`s per call. Precompute the keys.

**Expecting textbook results.** `Ashcraft` is `A226`, not `A261`. `chemical` is
`SHMKL` under `Metaphone`, not `KMKL`. See
[Deliberate deviations](#deliberate-deviations-from-the-published-algorithms).

**Reaching for `verbora_core::Phonetic` with `DoubleMetaphone`.** It does not
implement that trait — there is no single key. Use `DoubleKeyPhonetic`, or the
inherent `process`.

**Assuming `SoundExDM` output is always digits.** `process("B0")` is `"undefi"`.
If you index D-M codes, either sanitise your input of digits or accept that a
`"undefi"`-prefixed bucket exists.

**Slicing a `Metaphone` key by bytes.** The output can contain non-ASCII
(`process("café")` is `"KFÉ"`), so `&key[..3]` can panic. Use `process_with` to
bound the length, or a char-aware slice.

**Assuming `phoneticize_tokens` filters case-insensitively or strips
punctuation.** It does neither: `The` survives the stop-word filter and `it's` is
encoded with the apostrophe.

**Treating `try_process` as "the safe one".** It is the *exact* one — it
surfaces the fallible cases as `Err` instead of silently handling them.
`process` is the lenient default. Pick based on whether you want that
strictness, not on which name sounds safer.

## Related

- [Phonetic neighbors](phonetic-index) — a Verbora-native extension that
  indexes a whole dictionary so `neighbors()` can answer
  "which stored words sound like this one?" without re-encoding it on every
  call. Start here if you have more than a few dozen words to check against.
- [Beider-Morse](beider-morse.md) — a third Verbora-native extension, for a
  problem none of the four encoders on this page solve: the same historical
  family name has different "correct" spellings depending on which country's
  conventions transcribed it. Start here for genealogical/cross-language
  surname matching.
- [Language](language.md) — another Verbora-native extension: script and
  language detection, plus the `recommend()` function
  [Choosing a Phonetic Algorithm](#choosing-a-phonetic-algorithm) above
  quotes directly. Start here for "I don't know the language yet" — and for
  why a single short word or name should never make that determination for
  you automatically.
- [Distance metrics](../features/distance.md) — the scoring step that runs inside
  each phonetic bucket.
- [Tokenizers](../features/tokenizers.md) — where the tokens come from; only the
  `&str`-yielding ones compose with `phoneticize_tokens*` without a `.map()`.
- [Core traits](../features/core.md) — `Phonetic`, `DoubleKeyPhonetic`,
  `StopWords` and the process-global stop-word list.
  across all crates.
- [Allocation](../performance/allocation.md) ·
  [Zero-copy](../performance/zero-copy.md) ·
  [Parallelism](../performance/parallelism.md)
- [Recipes](../recipes/index.md) — end-to-end fuzzy-matching pipelines.
- [Choosing an API](../choosing/index.md) — the cross-crate version of the
  decision trees above.

## API reference

### Types

| Item | Description |
|---|---|
| `SoundEx` | Russell/NARA SoundEx. Zero-sized, `const fn new()`. |
| `Metaphone` | Original (Philips, 1990) Metaphone. Zero-sized, `const fn new()`. |
| `DoubleMetaphone` | Double Metaphone. Zero-sized, `const fn new()`. |
| `SoundExDM` | Daitch–Mokotoff SoundEx, single-code variant. Zero-sized, `const fn new()`. |
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
`utf16_to_uppercase`, `uppercase_utf16`, `is_trim_units_unit`, `trim_units`, `clamp_take`,
`clamp_slice_end`, `coerce_or_default`.
