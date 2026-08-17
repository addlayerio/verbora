# Normalizers

`verbora-normalizers` is tested against the reference normalizers: five
independent text normalizers that share nothing but a habit of doing surprising
things to Unicode. One expands English contractions, three fold diacritics
(a general Latin table, plus narrower Norwegian and Swedish ones), and one
normalizes Japanese widths, kana and compatibility symbols — with its seventeen
individual conversions exposed separately.

This is also the crate where `Cow` earns its keep. Normalizers are usually called
on text that needs no change at all: a Latin sentence handed to the katakana
converter, an ASCII token handed to the diacritic folder. Every function here
that returns text returns `Cow::Borrowed` when it changed nothing, and allocates
only at the first replacement — so the common case costs a scan and no heap
traffic.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>6</strong> normalizer APIs
are documented and test-pinned on byte-exact output, with the deliberate
behaviour choices listed below.
<code>cargo test -p verbora-normalizers</code> runs <strong>63</strong> unit
tests and <strong>18</strong> doctests.
</div>

## When to use it

- You are reproducing the reference behaviour and need the *same* answers, quirks
  included, not a "better" normalization.
- You want to fold Latin diacritics for a search key, a dictionary lookup or a
  fuzzy-match preprocessing step, and you are matching against text folded the
  same way.
- You are indexing Japanese and need halfwidth katakana, fullwidth alphanumerics
  and compatibility symbols collapsed onto one representation before tokenizing.
- You want contractions expanded before a downstream stage that does not
  understand apostrophes.

## When not to use it

- **You want correct Unicode normalization.** `remove_diacritics` is an
  872-entry lookup table transcribed from the reference, not NFD followed by
  combining-mark stripping. It folds `ſ` U+017F to **`l`** (the source lists it in
  the `l` character class), folds `ß` to `s` rather than `ss`, and leaves
  `e` + U+0301 completely alone because it does not decompose. If you want
  Unicode correctness, use a Unicode normalization crate; you will get different
  answers and you will change the output.
- **You want case folding, whitespace trimming or stopword removal.** None of
  these functions do any of that. `normalize` only expands contractions.
- **You want a full text-preprocessing pipeline.** Combine these with
  [tokenizers](../features/tokenizers.md) and [inflectors](../features/inflectors.md);
  the normalizers are single-purpose passes.
- **You want `normalize` on a sentence.** It works on *tokens*. Tokenize first —
  see [Common mistakes](#common-mistakes).

## Quick example

```rust
use verbora_normalizers::{
    normalize, normalize_ja, normalize_no, normalize_sv, normalize_token, remove_diacritics,
};

fn main() {
    // English contractions: token slice in, token list out.
    assert_eq!(
        normalize(&["I'm", "sure", "it's", "fine"]),
        ["I", "am", "sure", "it", "is", "fine"]
    );
    assert_eq!(normalize_token("couldn't've"), ["could", "not", "have"]);

    // Diacritics: general, Norwegian, Swedish.
    assert_eq!(remove_diacritics("crème brûlée"), "creme brulee");
    assert_eq!(normalize_no("blåbærsyltetøy à la façon"), "blåbærsyltetøy a la facon");
    assert_eq!(normalize_sv("Malmö à la carte"), "Malmö a la carte");

    // Japanese: widths, kana, iteration marks, composite symbols.
    assert_eq!(normalize_ja("ｶﾀｶﾅ　ＡＢＣ１２３ 時々刻々"), "カタカナ ABC123 時時刻刻");
}
```

## The six top-level normalizers

| Rust | Reference original | Job | Returns |
|---|---|---|---|
| `normalize` | `normalize(tokens)` (`normalizeTokens`) | expand English contractions across a token slice | `Vec<String>` |
| `normalize_token` | `normalize(string)` | the same, for the bare-string call | `Vec<String>` |
| `remove_diacritics` | `removeDiacritics` | fold Latin diacritics to base letters | `Cow<'_, str>` |
| `normalize_no` | `normalizeNo` | fold Norwegian diacritics, keeping `ä ö ü å ø æ` | `Cow<'_, str>` |
| `normalize_sv` | `normalizeSv.removeDiacritics` | fold Swedish diacritics, keeping `ä ö å` and more | `Cow<'_, str>` |
| `normalize_ja` | `normalizeJa` | normalize Japanese widths, kana and symbols | `Cow<'_, str>` |
| `ja::converters::*` | `Converters` | the seventeen individual Japanese conversions | `Cow<'_, str>` |

The last row is a module, not a seventh top-level normalizer; it is listed here
because it is the other half of the Japanese surface. Everything is a free
function, everything is `#[must_use]`, nothing is fallible and nothing holds
state. There is no builder, no options struct and no trait to implement.

## Choosing the right API

### Comparison table

| API | Best for | Output | Borrows on no-op | Allocates on no-op | Buffer reuse | Parallel |
|---|---|---|:--:|:--:|:--:|:--:|
| `normalize(&[S])` | a whole document's tokens | `Vec<String>` | ❌ | ✅ | ❌ | ❌ |
| `normalize_token(&str)` | one token you already have | `Vec<String>` | ❌ | ✅ | ❌ | ❌ |
| `remove_diacritics` | any Latin-script text | `Cow<'_, str>` | ✅ | ❌ | n/a | ❌ |
| `par_remove_diacritics_batch` | many independent documents, feature `parallel` | `Vec<Cow<'_, str>>` | ✅ (per document) | ❌ | n/a | ✅ per document |
| `normalize_no` / `normalize_sv` | Norwegian / Swedish text | `Cow<'_, str>` | ✅ | ❌ | n/a | ❌ |
| `normalize_ja` | Japanese, full 4-stage pipeline | `Cow<'_, str>` | ✅ | ❌ * | n/a | ❌ |
| `ja::converters::*` | one conversion stage only | `Cow<'_, str>` | ✅ | ❌ | n/a | ❌ |

"Allocates on no-op" is the column that matters at scale. The two `Vec<String>`
APIs always allocate — see [Allocation behaviour](#allocation-behaviour) — and
the `Cow` APIs never do when they have nothing to change.

\* One exception: if the input contains U+3005 `々` but neither iteration-mark
pass matches (`"々"` on its own, or `"時\n々"`), `normalize_ja` still allocates
the `Vec<u16>` that stage requires, then returns the original `&str` borrowed.

### Decision tree

```text
I need to normalize text
│
├── English contractions
│      ├── I have a token slice (the usual case)
│      │      └── normalize(&tokens)
│      └── I have exactly one token
│             └── normalize_token(&token)
│
├── Latin diacritics
│      ├── Any language, fold everything the table knows
│      │      └── remove_diacritics()
│      ├── Norwegian — keep ä ö ü å ø æ
│      │      └── normalize_no()
│      └── Swedish — keep those, plus â ç ê î ñ ó ô û š
│             └── normalize_sv()
│
└── Japanese
       ├── I want the whole normalization
       │      └── normalize_ja()
       ├── I want one width/kana conversion
       │      └── ja::converters::{alphabet_fh, katakana_hf, …}
       └── I want hiragana and katakana on one syllabary
              └── ja::converters::{hiragana_to_katakana, katakana_to_hiragana}
```

### `normalize` vs `normalize_token`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;String&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, one <code>String</code> per output token; plus one intermediate <code>String</code> per token that a fallback rule rewrites</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — there is no <code>_into</code> variant</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No <code>_batch</code> entry point; <code>normalize</code> itself takes a slice</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Expanding contractions across an already-tokenized document</span></div>
</div>

```rust  ignore
pub fn normalize<S: AsRef<str>>(tokens: &[S]) -> Vec<String>
pub fn normalize_token(token: &str) -> Vec<String>
```

They are the same algorithm; the difference is only how the reference is called.
`normalize(&[S])` mirrors the reference's `normalize(['a', 'b'])` and
`normalize_token(&str)` mirrors the reference's `normalize('a')`, which the reference
implements by wrapping the string in a one-element array.

**Both return `Vec<String>`, not `String`, because one input token can expand
into several output tokens.** `"couldn't've"` is a single conversion-table entry
whose value is `["could", "not", "have"]`, and a rule hit produces an expanded
string that is then split on `/\W+/`, so `"it's!"` becomes three fields. There is
no shape of the API that can return one string without losing information.

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("couldn't've"), ["could", "not", "have"]);
    assert_eq!(normalize_token("it's!"), ["it", "is", ""]);   // trailing empty field is real
    assert_eq!(normalize_token("o'clock"), ["o'clock"]);      // no rule matched: one token, unsplit
}
```

**Pick `normalize` unless you genuinely have one token.** Calling
`normalize_token` in a loop allocates one `Vec` per token; calling `normalize`
once over the slice allocates one `Vec` for the whole document.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>normalize_token</code> does <em>not</em> split on
whitespace. <code>normalize_token("I'm here")</code> is
<code>["I'm here"]</code> — the whole string is looked up as one token, the
conversion table misses, no rule matches, and it comes back verbatim. Tokenize
first with <a href="../features/tokenizers">a tokenizer</a>.
</div>

### `normalize_ja` vs a single `ja::converters` stage

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>
<span class="badge badge-utf16">UTF-16</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, four stages</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'_, str&gt;</code> — borrowed when all four stages were no-ops</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None on unchanged input without U+3005; otherwise at most one <code>String</code> per stage that actually changes something, plus stage one's <code>Vec&lt;u16&gt;</code> when U+3005 is present (the borrow is carried across stages)</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Internal only — the owned buffer is handed forward through no-op stages</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Producing one canonical form of Japanese text before tokenizing or indexing</span></div>
</div>

`normalize_ja` runs four stages, in this order:

1. **Expand iteration marks.** `時々刻々` becomes `時時刻刻`. This stage has **no
   standalone public function** — it is only reachable through `normalize_ja`.
2. **`converters::normalize`.** Fullwidth alphanumerics, the ideographic space and
   fullwidth symbols go halfwidth; halfwidth punctuation and halfwidth katakana go
   fullwidth. It deliberately contains neither the fullwidth punctuation table nor
   the halfwidth-to-fullwidth alphabet, so `、。・「」` stay fullwidth and ASCII
   stays ASCII.
3. **`converters::fix_fullwidth_kana`.** Composes a base kana with a following
   standalone voiced mark, and rewrites small tsu before the n-row.
4. **`converters::fix_composite_symbols`.** `㍼` becomes `昭和`, `㌫` becomes
   `パーセント`.

**Call `normalize_ja` when you want a canonical form.** Reach for an individual
converter when you want exactly one of those transformations and specifically do
*not* want the others:

```rust
use verbora_normalizers::ja::converters;
use verbora_normalizers::normalize_ja;

fn main() {
    // The pipeline widens halfwidth katakana …
    assert_eq!(normalize_ja("ｶﾀｶﾅ"), "カタカナ");
    // … and so does this one stage, on its own.
    assert_eq!(converters::katakana_hf("ｶﾀｶﾅ"), "カタカナ");

    // But one stage does not do the others' work.
    assert_eq!(converters::katakana_hf("時々刻々"), "時々刻々");
    assert_eq!(normalize_ja("時々刻々"), "時時刻刻");

    // Fold both syllabaries onto one, for a search key.
    assert_eq!(converters::hiragana_to_katakana("こんにちは"), "コンニチハ");
    assert_eq!(converters::katakana_to_hiragana("コンニチハ"), "こんにちは");

    // Widen instead of narrow — the pipeline never does this.
    assert_eq!(converters::alphabet_hf("ABC "), "ＡＢＣ　");
}
```

Concretely, use an individual converter when:

- **You need a width conversion the pipeline does not perform.** The pipeline only
  narrows Latin; `alphabet_hf`, `numbers_hf`, `symbol_hf` and `punctuation_hf`
  widen it. Nothing in `normalize_ja` widens ASCII.
- **You want hiragana and katakana collapsed onto one syllabary.** `normalize_ja`
  never converts between the syllabaries. `hiragana_to_katakana` and
  `katakana_to_hiragana` are the only functions that do, and they are the right
  tool for a case-insensitive-style search key over kana.
- **You want the fullwidth CJK punctuation `、。・「」` narrowed.**
  `pure_punctuation_fh` and `punctuation_fh` do it; the pipeline deliberately
  does not.
- **You are reproducing a specific `Converters.*` call from a reference
  codebase.** Each one maps to exactly one function; see
  [the converter surface](#the-ja-converters-surface).
- **The iteration-mark stage is unwanted.** It is the only stage that pays for a
  UTF-16 round trip, and it is the source of the one astral-input divergence.

### `Cow<'_, str>` or `Vec<String>` at the call site

Four of the six top-level functions — and all seventeen converters — return
`Cow<'_, str>`. The other two return `Vec<String>`. This is not stylistic: it is
what the reference's return types force.

| Returns | Functions | What that means for you |
|---|---|---|
| `Cow<'_, str>` | `remove_diacritics`, `normalize_no`, `normalize_sv`, `normalize_ja`, all 17 `ja::converters` | The result may be your own input, unchanged and un-copied. It borrows the input, so it cannot outlive it. Reading through `Deref` costs nothing; `.into_owned()` costs a copy when the value is borrowed. |
| `Vec<String>` | `normalize`, `normalize_token` | You always get an owned, independent allocation. It can outlive the input. There is no borrowing variant and no output-buffer variant. |

The practical rule: **hold on to the `Cow` as long as you can, and convert at the
boundary where a `String` is genuinely required.** A `Cow<str>` derefs to `&str`,
so almost everything you want to do with the result needs no decision at all:

```rust
use verbora_normalizers::remove_diacritics;
fn read_without_deciding(s: &str) -> (usize, bool, char) {
    let folded = remove_diacritics(s);
    (
        folded.len(),                              // Deref
        folded == "cafe",                          // PartialEq<&str>
        folded.chars().next().unwrap_or('?'),      // Deref
    )
}

fn main() {
    assert_eq!(read_without_deciding("café"), (4, true, 'c'));
}
```

### Composing normalizers without allocating per stage

This is the technique `normalize_ja` uses internally, and it is worth copying.

The naive chain does not compile, because each stage's `Cow` borrows the previous
stage's local:

```rust  ignore
// error[E0515]: cannot return value referencing local variable `first`
fn two_stages(s: &str) -> Cow<'_, str> {
    let first = normalize_no(s);
    remove_diacritics(&first)
}
```

The obvious fix — `.into_owned()` after each stage — compiles, and throws away
the whole point of the `Cow`. It allocates once per stage even when every stage
was a no-op:

```rust
use verbora_normalizers::{normalize_no, remove_diacritics};
fn two_stages_wasteful(s: &str) -> String {
    let first = normalize_no(s).into_owned();  // allocates even when nothing changed
    remove_diacritics(&first).into_owned()     // allocates again
}
fn main() { assert_eq!(two_stages_wasteful("àà ö"), "aa o"); }
```

The fix is a small adapter that applies the next stage to a `Cow` and gives the
borrow back when neither step changed anything. `verbora-normalizers` has this as
a private `map_cow` in `src/table.rs`; it is **not exported**, so write your own —
this is the whole of it:

```rust
use std::borrow::Cow;

use verbora_normalizers::{normalize_no, remove_diacritics};

/// Applies `f` to a `Cow` without giving up the borrow when neither step
/// changed anything.
fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            // Re-borrowing `owned` inside the match keeps `f`'s temporary alive
            // only for the statement, so `owned` can be handed back untouched
            // when `f` made no change.
            let next = match f(&owned) {
                Cow::Borrowed(_) => None,
                Cow::Owned(v) => Some(v),
            };
            Cow::Owned(next.unwrap_or(owned))
        }
    }
}

/// Norwegian folding, then the general table, in one borrow.
fn fold_no_then_latin(s: &str) -> Cow<'_, str> {
    let out = normalize_no(s);
    map_cow(out, remove_diacritics)
}

fn main() {
    // Nothing to fold: no allocation at all, across both stages.
    assert!(matches!(fold_no_then_latin("blabaer"), Cow::Borrowed(_)));
    // Something to fold: exactly one `String`, reused by the second stage.
    assert_eq!(fold_no_then_latin("àà ö"), "aa o");
}
```

The two invariants that make this exact:

- On a `Borrowed` input, the next stage runs directly on the original `&str`, so
  the lifetime `'a` flows straight through and nothing is copied.
- On an `Owned` input, the next stage's result is inspected *before* the temporary
  borrow ends. If it is `Borrowed`, the stage changed nothing and the existing
  buffer is returned unmoved. Only a genuine change allocates.

It composes to any depth, including over `normalize_ja`, which is already four
stages internally. Text that needs none of them costs one scan per stage and zero
allocations:

```rust
use std::borrow::Cow;
use verbora_normalizers::ja::converters;
use verbora_normalizers::normalize_ja;
fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            let next = match f(&owned) { Cow::Borrowed(_) => None, Cow::Owned(v) => Some(v) };
            Cow::Owned(next.unwrap_or(owned))
        }
    }
}
/// Canonical Japanese form, then both syllabaries folded onto katakana.
fn search_key(s: &str) -> Cow<'_, str> {
    let out = normalize_ja(s);
    map_cow(out, converters::hiragana_to_katakana)
}

fn main() {
    assert_eq!(search_key("ｶﾀｶﾅのﾃｽﾄ"), "カタカナノテスト");
    assert!(matches!(search_key("plain ascii"), Cow::Borrowed(_)));
}
```

## `Cow` in depth

<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>
<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

### What `Cow::Borrowed` means for the caller

`Cow::Borrowed(s)` is a **guarantee about what happened**, not just a
representation choice:

- No `String` was allocated. No bytes were copied.
- The returned `&str` *is* your input — same pointer, same length.
- Nothing in the normalizer's table matched. For the diacritic folders that is
  equivalent to "the text is unchanged", because no table key maps to itself:
  every non-ASCII key folds to a different, ASCII base string.

`Cow::Owned(buf)` means the opposite: at least one replacement fired, and exactly
one `String` was allocated to hold the result (see
[Allocation behaviour](#allocation-behaviour) for when it can grow).

### How to check

```rust
use std::borrow::Cow;

use verbora_normalizers::remove_diacritics;

fn main() {
    // ASCII: rejected by a single vectorised `is_ascii` scan.
    let untouched = remove_diacritics("plain ascii text");
    assert!(matches!(untouched, Cow::Borrowed(_)));

    // Non-ASCII the table has nothing to say about: rejected by the bitmap gate.
    let untouched = remove_diacritics("Москва");
    assert!(matches!(untouched, Cow::Borrowed(_)));

    // A real fold: one allocation.
    let folded = remove_diacritics("café");
    assert!(matches!(folded, Cow::Owned(_)));
    assert_eq!(folded, "cafe");
}
```

`matches!(result, Cow::Borrowed(_))` is the whole idiom. It is also how the
crate's own unit tests pin the behaviour, so the guarantee cannot rot silently.

### Why "ASCII text through a diacritic folder allocates nothing"

`remove_diacritics` opens with:

```rust  ignore
if s.is_ascii() {
    return Cow::Borrowed(s);
}
```

That short-circuit is *exact*, not an approximation. The reference's table has
872 keys, 52 of which are ASCII letters mapping to themselves; the generated
table omits those 52 identities, leaving 820 non-ASCII keys. So every ASCII
character is either absent from the table or maps to itself, and an all-ASCII
string cannot change. `str::is_ascii` is vectorised and stops at the first
non-ASCII byte, so the check costs a linear scan at memory bandwidth and the rest
of the function never runs.

Non-ASCII text that the table still does not touch is rejected almost as cheaply.
The 820 keys start in only ten 256-codepoint blocks (`0x00`–`0x02`, `0x1D`,
`0x1E`, `0x21`, `0x24`, `0x2C`, `0xA7`, `0xFF`), so a two-level bitmap — four
`u64` over `codepoint >> 8`, then a per-block bitmap over the low byte — rejects
Cyrillic, Greek, Hebrew, CJK, kana and emoji in a handful of operations, without
ever reaching the binary search. Cyrillic's block `0x04` is not in that list, so
`remove_diacritics("Москва")` never touches the key table at all. The second
level exists because the coarse bitmap alone is not enough: block `0xFF` holds
both the fullwidth Latin letters that *do* fold and the halfwidth kana that do
not. Astral characters (`> U+FFFF`) are rejected before either test, because the
table has no non-BMP keys.

The Japanese converters use the same shape. Eleven of the fifteen conversion
tables have no ASCII key whatsoever — every fullwidth-to-halfwidth table, the
composite normalizer, both kana fixers, and the halfwidth punctuation and
katakana tables — so `Table::translate` returns pure-ASCII input untouched after
one `is_ascii` scan. Only `alphabet_hf`, `numbers_hf`, `symbol_hf` and
`punctuation_hf` have ASCII keys, and those are precisely the four that are
*supposed* to rewrite ASCII.

### The cost of `.into_owned()`

| Value | `.into_owned()` cost |
|---|---|
| `Cow::Owned(buf)` | Free. Moves the existing `String` out; no allocation, no copy. |
| `Cow::Borrowed(s)` | One allocation plus a `memcpy` of the entire string. |

That asymmetry is the trap. `remove_diacritics(s).into_owned()` on ASCII input
does exactly the work the `Cow` existed to avoid: it allocates and copies a
string that was already correct. In a pipeline that runs a folder over every
document, the borrowed case is the overwhelming majority, so an unconditional
`.into_owned()` converts an allocation-free pass into one allocation per
document.

Use it only where you actually need an owned `String` — storing the result in a
struct, sending it across a thread boundary, or returning it from a function that
cannot name the input's lifetime. Note that the crate's own tests call
`.into_owned()` deliberately, because comparing against a recorded JSON string is
exactly such a boundary.

<div class="callout callout-note">
<strong>Note.</strong> <code>Cow&lt;'_, str&gt;</code> borrows the input, so it cannot
outlive it. If you need the result to outlive the buffer it was read from, that
is a legitimate reason to call <code>.into_owned()</code> — just do it once, at
the boundary, rather than at every stage.
</div>

## The `ja::converters` surface

The reference exposes a `Converters` class used purely as a namespace: no
constructor body, no fields, no state. Six prototype methods convert fullwidth to
halfwidth, six convert back, two convert between the kana syllabaries, and three
are `static`. In Rust they are all free functions in `verbora_normalizers::ja::converters`;
the prototype/static split has no observable meaning here. All seventeen return
`Cow<'_, str>` and all seventeen are covered by the test suite (5,742 recorded
cases each — 97,614 in total).

| Function | Reference | Converts | Direction |
|---|---|---|---|
| `alphabet_fh` | `alphabetFH` | Fullwidth Latin letters, and U+3000 ideographic space → ASCII letters and space (53 keys) | full → half |
| `numbers_fh` | `numbersFH` | Fullwidth digits `０-９` → ASCII digits (10 keys) | full → half |
| `symbol_fh` | `symbolFH` | Fullwidth symbols → ASCII. Both U+FF0D and U+2500 map to `-` (33 keys) | full → half |
| `pure_punctuation_fh` | `purePunctuationFH` | `、。・「」` → `､｡･｢｣` (5 keys) | full → half |
| `punctuation_fh` | `punctuationFH` | `symbol_fh` and `pure_punctuation_fh` merged into one pass (38 keys) | full → half |
| `katakana_fh` | `katakanaFH` | Fullwidth katakana → halfwidth, expanding voiced kana into two characters (`ガ` → `ｶﾞ`) | full → half |
| `alphabet_hf` | `alphabetHF` | ASCII letters, and the ASCII space, → fullwidth. The space really does widen to U+3000 | half → full |
| `numbers_hf` | `numbersHF` | ASCII digits → fullwidth digits (10 keys) | half → full |
| `symbol_hf` | `symbolHF` | ASCII symbols → fullwidth. `-` maps to U+2500, **not** U+FF0D (32 keys) | half → full |
| `pure_punctuation_hf` | `purePunctuationHF` | `､｡･｢｣` → `、。・「」` (5 keys) | half → full |
| `punctuation_hf` | `punctuationHF` | `symbol_hf` and `pure_punctuation_hf` merged into one pass (37 keys) | half → full |
| `katakana_hf` | `katakanaHF` | Halfwidth katakana → fullwidth, composing base + voiced mark (`ｶﾞ` → `ガ`) (84 keys) | half → full |
| `normalize` | `Converters.normalize` (static) | Fullwidth alnum, U+3000 and fullwidth symbols → half; halfwidth punctuation and katakana → full (185 keys) | mixed, both ways |
| `fix_fullwidth_kana` | `Converters.fixFullwidthKana` (static) | Base kana + standalone spacing voiced mark → composed kana; small tsu before the n-row → `ん`/`ン` (64 two-char keys) | in-place, same width |
| `fix_composite_symbols` | `Converters.fixCompositeSymbols` (static) | Single-codepoint CJK compatibility symbols → spelled-out forms: `㍼`→`昭和`, `㌫`→`パーセント` (161 keys) | one char → many |
| `hiragana_to_katakana` | `hiraganaToKatakana` | Hiragana → fullwidth katakana | hiragana → katakana |
| `katakana_to_hiragana` | `katakanaToHiragana` | Katakana → hiragana | katakana → hiragana |

```rust
use verbora_normalizers::ja::converters;

fn main() {
    assert_eq!(converters::alphabet_fh("ＡＢＣ　ＡＢＣ"), "ABC ABC");
    assert_eq!(converters::symbol_fh("－─"), "--");
    assert_eq!(converters::katakana_fh("ヴガパー゛゜"), "ｳﾞｶﾞﾊﾟｰﾞﾟ");

    assert_eq!(converters::alphabet_hf("ABC "), "ＡＢＣ　");
    assert_eq!(converters::symbol_hf("-"), "─");
    assert_eq!(converters::katakana_hf("ｳﾞｶﾞﾊﾟｰﾞﾟ"), "ヴガパー゛゜");

    assert_eq!(converters::normalize("ＡＢｶﾞ"), "ABガ");
    assert_eq!(converters::fix_fullwidth_kana("か゛"), "が");
    assert_eq!(converters::fix_fullwidth_kana("っな"), "んな");
    assert_eq!(converters::fix_composite_symbols("㍼㍿㋀"), "昭和株式会社1月");

    assert_eq!(
        converters::hiragana_to_katakana("ぁゖゝゞっなあいうえお"),
        "ァヶヽヾンナアイウエオ"
    );
}
```

Three behaviours in that table are worth reading twice, because they look like
bugs and are in fact faithful:

- **`symbol_hf("-")` is U+2500 BOX DRAWINGS LIGHT HORIZONTAL, not U+FF0D.** The
  halfwidth-to-fullwidth tables do not exist in the reference source; they are
  built at module load by `flip()`, which is last-writer-wins. Both U+FF0D and
  U+2500 map to `-` in the forward table, U+2500 is listed second, and so it wins
  the inversion. That also shrinks the table from 33 entries to 32.
- **`alphabet_hf` widens the ASCII space to U+3000.** The table is the flip of
  `alphabet_fh`'s, which contains U+3000 → U+0020.
- **`fix_fullwidth_kana` is not only a diacritic fixer.** Five entries per
  syllabary rewrite a small tsu before the n-row: `っな` → `んな`. That is a
  phonetic change, it is why `normalize_ja("まっなか")` is `"まんなか"`, and it
  fires inside `hiragana_to_katakana` too.

## Deliberate divergences from the reference

Three, each recorded in `crates/verbora-normalizers/src/lib.rs` and pinned by a
test so it cannot drift. ### 1. `normalize_sv` is callable here

`normalizers/index` exports the whole module *object*, so
The reference's `normalizeSv` is `{ removeDiacritics: [Function] }` and
calling `normalizeSv('x')` throws `TypeError: normalizeSv is not a function` —
even though the bundled `index.d.ts` declares it as `(str: string) => string`.
The real callable is the reference's `normalizeSv.removeDiacritics`, which is what the
Swedish tokenizer uses internally and what `normalize_sv` mirrors. Rust has no
analogue of accidentally exporting a module namespace, so the broken top-level
export is not reproduced.

The fixture records the reference `TypeError` in a dedicated
`normalizeSv.notAFunction` suite, and the crate's tests assert the recorded
error message contains `"not a function"` — so the claim is checked, not asserted
in prose.

### 2. `normalize(["constructor"])` does not panic

The reference's conversion table is a plain object literal, so
`conversionTable[token.toLowerCase()]` for `"constructor"` and `"__proto__"`
finds `Object.prototype` members. Both are truthy and neither is a string, so the
following `.split` throws `TypeError: ....split is not a function`. A Rust lookup
has no prototype chain, so both come back as ordinary unmatched tokens.

```rust
use verbora_normalizers::normalize;
fn main() {
    assert_eq!(normalize(&["constructor"]), ["constructor"]);
    assert_eq!(normalize(&["__proto__"]), ["__proto__"]);
}
```

The test does not skip these cases: it asserts that the original threw *only*
for these two tokens, and that the Rust path passed the token through unchanged.
`"toString"` and `"hasOwnProperty"` never throw in the reference either — their
lowercased names miss the prototype — and are ordinary misses on both sides.

### 3. `normalize_ja` cannot emit a lone surrogate

<span class="badge badge-utf16">UTF-16</span>

Stage one matches UTF-16 code units, so on a surrogate pair it can capture half
of one. `normalizeJa("😀々")` really does return `"😀"` followed by a **lone low
surrogate** in the reference. A Rust `String` cannot hold that, so the matching is
reproduced exactly over UTF-16 and only the final conversion back is lossy: the
unpaired surrogate becomes U+FFFD. Positions, lengths and every well-formed
result are unaffected. This has the same shape as divergence D2 in
the crate's own module documentation.

```rust
use verbora_normalizers::normalize_ja;
fn main() {
    // Two-unit pass captures the whole pair: well-formed, exact.
    assert_eq!(normalize_ja("a😀々々"), "a😀😀");
    // One-unit pass captures the low half: the reference emits a lone surrogate.
    assert_eq!(normalize_ja("😀々"), "😀\u{FFFD}");
}
```

JSON cannot carry a lone surrogate either, so the fixture generator boxes those
results as `{ "$utf16": [...] }` and the test compares them after the same
lossy conversion — the divergence is measured, not assumed.

## Generated tables, and why you can trust them

`src/ja/tables.rs` (1,331 lines) and `src/diacritics/table.rs` (952 lines) are
not transcribed by hand. They were machine-derived,
which **`require`s the reference module and dumps the tables it actually built at
runtime**. That matters for two reasons.

**Several of the tables do not exist in the reference source at all.** The
`halfwidthToFullwidth.*`, `.punctuation` and `.normalize` tables are constructed
at module load by `flip()` and `merge()`, whose collision rules are observable:
`flip()` is last-writer-wins, `merge()` preserves first-insertion position. A
hand-written inversion gets those wrong silently. The most visible casualty is
`-`, which flips back to U+2500 rather than the U+FF0D it also came from,
shrinking that table from 33 entries to 32.

**The diacritics table is a proof obligation, not a transcription.** The
reference stores 86 `{ base, letters: /[...]/g }` rules and runs 86 sequential
global-regex passes over the whole string. Collapsing that to one per-character
lookup is only valid if no pass can cascade into another.

The generator re-proves both load-bearing properties **on every run**, and
refuses to emit a table if either fails:

| Invariant | How it is proved | What breaks without it |
|---|---|---|
| No key is a proper prefix of a *later* key, in every table | Checked over every key pair in every emitted table | the reference's leftmost-**first** alternation would stop being identical to the leftmost-**longest** matching `src/table.rs` implements, and `ｳﾞ`→`ヴ` could lose to `ｳ`→`ウ` |
| The 86-pass diacritics algorithm equals one per-character lookup | Run both, exhaustively over all 63,488 non-surrogate BMP codepoints, plus 20,000 random strings | A replacement could be re-matched by a later pass, making pass order observable and the single-pass port wrong |

Both invariants are then re-asserted from the Rust side. The unit tests in
`src/diacritics.rs` and `src/ja.rs` check that each generated bitmap gate is
*exact* — that it admits every key's first character and rejects every other BMP
codepoint — by enumerating every non-surrogate BMP codepoint and counting the
admitted set. A generated gate that was merely *nearly* right would otherwise
show up as a silent behaviour change rather than a compile error.

The same tests pin the entry counts the shipped spec records, including the two
the `flip()` collision shrinks: 33 → 32 for the symbol table and 38 → 37 for the
merged punctuation table.

## Advanced usage

### Folding a search key

The idiomatic use is one function that produces a canonical form and returns a
`Cow`, so callers who pass already-canonical text pay nothing:

```rust
use std::borrow::Cow;

use verbora_normalizers::remove_diacritics;

/// A folded key for lookups. Borrows when the input is already folded.
fn key(s: &str) -> Cow<'_, str> {
    remove_diacritics(s)
}

fn main() {
    assert_eq!(key("résumé"), "resume");
    assert!(matches!(key("resume"), Cow::Borrowed(_)));
}
```

Fold both sides of a comparison with the same function. Folding only the query is
a common bug, and so is assuming the folder repairs mixed forms: it does not
decompose, so a stored `e` + U+0301 folds to itself while a query `é` U+00E9
folds to `e`, and the two never meet.

### Skipping the per-token `String` when a token cannot change

`normalize` and `normalize_token` always allocate. But both the five-entry
conversion table and all six fallback rules require an **ASCII apostrophe**
U+0027: every table key contains one, and each rule scans for one. A token with
no `'` therefore always comes back as exactly one output token equal to the
input, which you can exploit at the call site:

```rust
use std::borrow::Cow;

use verbora_normalizers::normalize_token;

/// Expands a token, borrowing when the expander provably cannot change it.
fn expand(token: &str) -> Vec<Cow<'_, str>> {
    if !token.contains('\'') {
        return vec![Cow::Borrowed(token)];
    }
    normalize_token(token).into_iter().map(Cow::Owned).collect()
}

fn main() {
    assert!(matches!(expand("plain")[0], Cow::Borrowed(_)));
    assert_eq!(expand("hasn't"), ["has", "not"]);
}
```

`str::contains('\'')` on a `char` needle is a `memchr`-class scan. This does not
remove the outer `Vec`, but it removes the per-token `String` for the vast
majority of real tokens — and it also skips the six per-rule apostrophe scans the
expander would otherwise run before concluding nothing matched.

### Accumulating across documents

```rust
use verbora_normalizers::normalize;
fn accumulate(docs: &[Vec<String>]) -> Vec<String> {
    let mut all = Vec::new();
    for tokens in docs {
        // One `Vec` per document; the `String`s are moved into `all`, not copied.
        all.extend(normalize(tokens));
    }
    all
}
fn main() {
    assert_eq!(
        accumulate(&[vec!["it's".to_owned()], vec!["fine".to_owned()]]),
        ["it", "is", "fine"]
    );
}
```

`normalize` is generic over `S: AsRef<str>`, so `&[&str]`, `&[String]` and
`&Vec<String>` all work without converting first.

### Parallelism

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>
<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, fanned out across a <code>rayon</code> thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Cow&lt;'_, str&gt;&gt;</code> — one <code>Cow</code> per input document</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One output <code>Vec</code>, plus exactly what <code>remove_diacritics</code> allocates per document — nothing for a document with nothing to fold</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Yes — this is the batch entry point</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — feature <code>parallel</code>; per document</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Folding hundreds of documents or more in one call</span></div>
</div>

`verbora-normalizers` ships one built-in parallel API: `par_remove_diacritics_batch`,
behind this crate's `parallel` Cargo feature (`parallel = ["dep:rayon"]`, never
on by default). It is exactly `inputs.par_iter().map(remove_diacritics).collect()`
— a thin fan-out over the sequential function above, not a second
implementation of the diacritic table, the ASCII fast path or the single-pass
`Cow` construction.

```rust  ignore
use verbora_normalizers::par_remove_diacritics_batch;

let inputs = ["café", "naïve", "ASCII only"];
let got = par_remove_diacritics_batch(&inputs);
assert_eq!(got, ["cafe", "naive", "ASCII only"]);
```

<div class="callout callout-note">
<strong>Note.</strong> This block needs the <code>parallel</code> feature
enabled on <code>verbora-normalizers</code>, which this site's own snippet
checker builds without, so it is marked <code>ignore</code> rather than
compiled — every other block on this page compiles and runs in CI.
</div>

Unlike a hand-rolled `rayon` fan-out that collects into an owned `Vec<String>`,
`par_remove_diacritics_batch` returns `Vec<Cow<'a, str>>` — it does **not**
force `.into_owned()` on every result the way collecting across threads with
your own closure typically would, so the zero-copy property survives
parallelising: a batch of already-ASCII documents still allocates nothing.

**When to reach for it.** `remove_diacritics` is already cheap per call — this
crate's own benchmark measures roughly 270 ns for a 128-byte accented document
up to roughly 35 µs for a 19 KB one, while a `rayon` task costs on the order of
a microsecond to schedule. Measured directly (`par_remove_diacritics_batch`'s
own Criterion group, at a fixed ~1.2 KB document per call): a 16-document
batch is roughly **1.5× slower** in parallel than the sequential loop, while
256- and 4096-document batches are **5–8× faster**. A plain
`inputs.iter().map(remove_diacritics).collect()` loop wins for a handful of
documents; reach for the parallel batch once it runs to hundreds of documents
or more, and measure your own workload rather than assuming the win.

For anything not covered by `par_remove_diacritics_batch` — `normalize_no`,
`normalize_sv`, `normalize_ja`, the `ja::converters`, or `normalize` /
`normalize_token` — every function in this crate is still a free function with
no shared state, no interior mutability and no globals, so all of them are
trivially `Send`-safe and you can parallelise over them yourself with your own
`rayon` dependency and no Cargo feature required:

```rust  ignore
// Your own rayon, over Verbora's stateless APIs.
use rayon::prelude::*;

let folded: Vec<String> = docs
    .par_iter()
    .map(|d| remove_diacritics(d).into_owned())
    .collect();
```

Note that collecting across threads this way forces `.into_owned()`, which
reintroduces the allocation the `Cow` avoids — that is exactly what
`par_remove_diacritics_batch` avoids for `remove_diacritics` specifically, by
returning `Cow` all the way through. For every other normalizer here, a
hand-rolled fan-out like the one above is still the only parallel option, and
it is only worth it once the surrounding work dominates the `.into_owned()`
cost. See [Parallelism](../performance/parallelism.md).

## Performance characteristics

All six top-level functions are **O(n) in the input length** with a small
constant. There is no backtracking, no regex engine and no automaton: the crate
has an empty `[dependencies]` section, which is itself a documented design
decision:

```toml
[dependencies]
# Deliberately empty.
#
# No `regex`: every pattern in this cluster is either a one-or-two-character
# table lookup or a scan needing the reference's ASCII-only `\w`, which `regex`
# makes Unicode-aware by default. Hand-written scanners are both faster and the
# only way to keep the documented behaviour. See src/table.rs and src/english.rs.
#
# No `verbora-core`: nothing here splits or trims on `\s`, so `is_whitespace`
# has no call site. The one place the reference's character semantics do bite —
# `.` in normalizeJa's iteration-mark passes — needs the *complement* of the four
# LineTerminators over UTF-16 code units, which is local to src/ja.rs.
```

| Function | Passes over the input | Per-character cost when nothing matches |
|---|---|---|
| `remove_diacritics` | 1 (0 for ASCII — one vectorised `is_ascii` scan and return) | Two bitmap tests; the 820-entry binary search is only reached by a genuine key |
| `normalize_no` / `normalize_sv` | 1, stopping early once all 26 (or 8) pairs have fired | One range test against U+00C0..=U+0161 |
| `normalize_ja` | 4 stages; stage 1 is skipped entirely unless the input contains U+3005 | Per stage: `is_ascii` short-circuit, else two bitmap tests |
| `ja::converters::*` | 1 (3 for `hiragana_to_katakana` / `katakana_to_hiragana`) | `is_ascii` short-circuit for the 11 tables with no ASCII key, else two bitmap tests |
| `normalize` / `normalize_token` | 1 per token, plus one `/\W+/` split per token a rule rewrote | Up to 5 key comparisons against a stack-buffered ASCII lowercase fold, then one byte scan for `'` per rule (six) before concluding no rule matched |

Two algorithmic differences from the reference are structural rather than
micro-optimisations:

- **`remove_diacritics` is one pass, not 86.** Every replacement the reference
  emits is an ASCII letter, and every ASCII letter is itself a key of the table
  mapping to itself, so no pass can cascade into another and the pass order is
  irrelevant. The reference's cost scales with 86 × length; this port's scales
  with length.
- **`normalize_no` / `normalize_sv` are one pass, not 26.** The reference calls
  `String#replace` once per pair, allocating a fresh string every time. Because
  the needles are distinct and no replacement is itself a needle, "first
  occurrence of each" is order-independent, so one left-to-right scan carrying a
  32-bit spent-pair mask is exactly equivalent and allocates at most once.

Criterion benchmarks live in `crates/verbora-normalizers/benches/normalizers.rs`
and are deliberately split into *rejection cost* (Latin prose handed to the
katakana converter, ASCII handed to the diacritic folder) and *work cost* (text
that is entirely replacements). A reference baseline exists for the same
inputs.

> Not yet benchmarked against the reference — the only published cross-language
> numbers today are the 26 `verbora-distance` benchmarks.
> See [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

| Function | Unchanged input | Changed input |
|---|---|---|
| `remove_diacritics` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `s.len()`. Never reallocates: no entry in the 820-key table has a replacement longer in bytes than its source character |
| `normalize_no` / `normalize_sv` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `text.len()`. Never reallocates: every replacement is a 1-byte ASCII character replacing a 2-byte source |
| `normalize_ja` | **Zero allocations** when the input contains no U+3005 and no stage changes anything. If U+3005 *is* present, stage 1 pays one `Vec<u16>` of the input even when neither pass matches | Stage 1: that `Vec<u16>`, plus up to one more per matching pass, plus one `String` from `from_utf16_lossy`. Stages 2–4: at most one `String` each, and only for a stage that actually changes something |
| `ja::converters::*` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `s.len()`. Six tables have replacements longer than their keys and so can outgrow it: `alphabet_hf`, `numbers_hf`, `symbol_hf`, `punctuation_hf`, `katakana_fh` (voiced-kana expansion) and `fix_composite_symbols`. The other nine — including `converters::normalize`, `fix_fullwidth_kana`, `katakana_hf` and every fullwidth-to-halfwidth table — never reallocate |
| `normalize` | One `Vec<String>` with capacity `tokens.len()`, plus one `String` per output token | Same, plus one intermediate `String` per token a fallback rule rewrote, and a `Vec` regrow if expansions push past the initial capacity |
| `normalize_token` | One `Vec<String>` with capacity 2, plus one `String` | Same, plus one intermediate `String` on a rule hit |

Three honest constraints, stated plainly:

1. **There is no `_into` API.** No function in this crate writes into a
   caller-supplied buffer, so there is no way to reuse an output allocation
   across calls. The `Cow` functions do not need one — they allocate nothing on
   the common path — but `normalize` and `normalize_token` genuinely cannot avoid
   it today.
2. **There is no batch API for most functions.** `normalize` accepting a slice
   is not a batch entry point in the `tokenize_batch` sense; it is simply the
   shape of the reference's `normalizeTokens(tokens)`. Every other function
   except `remove_diacritics` takes one string with no batch counterpart.
3. **There is one parallel API.** `remove_diacritics` has a batch, parallel
   sibling — `par_remove_diacritics_batch`, behind the `parallel` Cargo
   feature (see [Parallelism](#parallelism) above). Nothing else in this
   crate has one; parallelise the rest from your own code over the stateless
   functions.

The mitigations for (1) at the call site are the two shown in
[Advanced usage](#advanced-usage): call `normalize` once over the whole token
slice rather than `normalize_token` per token, and pre-filter on the ASCII
apostrophe so tokens that provably cannot change never reach the allocator.

See [Allocation](../performance/allocation.md) and
[Zero-copy](../performance/zero-copy.md) for the general principles.

## Unicode and language notes

### `remove_diacritics` is a table, not Unicode normalization

- **It does not decompose.** Precomposed `é` U+00E9 folds to `e`; `e` + U+0301
  passes through untouched.
- **The shipped quirks are part of the contract.** `ſ` U+017F LATIN SMALL LETTER
  LONG S folds to **`l`**, not `s`, because the original source lists it inside
  the `l` character class. `ß` folds to `s` and `ẞ` to `S` — not `ss`/`SS`. `İ`
  folds to `I` and `ı` to `i`.
- **It misses things a correct implementation handles.** Ligatures `ﬁ`/`ﬂ`,
  `Ĳ`/`ĳ`, `Ŋ`/`ŋ`, `ĸ`, `ȸ`/`ȹ` and every non-BMP character are left alone.
- **Some non-Latin-looking things do fold**, because the source table lists them:
  circled letters `ⒶⓏⓐⓩ` and fullwidth `ＡＺａｚ` both fold to plain ASCII.

Adding an NFD step or swapping in a transliteration crate would change the output on
real text, in both directions.

### The Nordic folders fold only their own accents

Each preserves the letters its own alphabet actually uses. Norwegian keeps
`ä ö ü å ø æ`; Swedish keeps those *and* `â ç ê î ñ ó ô û š`, folding only the
four `a`/`e` accents.

**And each fires once per pair.** The reference calls
`text.replace('à', 'a')` with a *string* first argument, and the reference's
`String#replace` replaces only the first occurrence unless given a global regex.
So each of the 26 (or 8) passes rewrites exactly one character. Upper- and
lowercase are separate pairs, so both fold once.

```rust
use verbora_normalizers::{normalize_no, normalize_sv};
fn main() {
    assert_eq!(normalize_no("ààà"), "aàà");           // first occurrence only
    assert_eq!(normalize_no("àÀàÀ"), "aAàÀ");         // each pair has its own first
    assert_eq!(normalize_sv("ààà ééé À É è È"), "aàà eéé A E e E");
}
```

### `normalize` uses the reference's ASCII-only `\w`

Expanded tokens are split on `/\W+/` with the reference's word class `[A-Za-z0-9_]`
— **not** Rust's `regex` crate default, which is Unicode-aware and would make `é`
and `漢` word characters:

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("héllo's"), ["h", "llo", "is"]);
    assert_eq!(normalize_token("漢字's"), ["", "is"]);
    assert_eq!(normalize_token("a_b1's"), ["a_b1", "is"]);
}
```

Only the ASCII apostrophe U+0027 counts. `I’d` with U+2019 RIGHT SINGLE
QUOTATION MARK is left alone entirely. The `n` of `n't` is matched **literally
and lowercase only**, while the letters after the apostrophe are matched
case-insensitively — so `"N'T"` does not match but `"hasn'T"` does.

Conversion-table lookups do use full Unicode lowercasing, not an ASCII fold: U+212A
KELVIN SIGN lowercases to ASCII `k`, so "a non-ASCII token cannot fold into an
ASCII key" is not a safe assumption to hard-code.

### `normalize_ja` stage one matches UTF-16 code units

<span class="badge badge-utf16">UTF-16</span>

The reference's `.` matches one UTF-16 code unit, not one scalar value, and excludes
exactly four line terminators: `\n`, `\r`, U+2028 and U+2029. It *does* match
`\t`, `\v`, `\f`, U+0085, U+00A0, U+3000 and U+FEFF — all of which a
`char::is_whitespace`-based test would get wrong in one direction or the other.

```rust
use verbora_normalizers::normalize_ja;
fn main() {
    assert_eq!(normalize_ja("\n々"), "\n々");                    // excluded
    assert_eq!(normalize_ja("\u{FEFF}々"), "\u{FEFF}\u{FEFF}");   // matched
}
```

The pass order is load-bearing and observable. `/(..)々々/g` fires first, and its
output is then eligible for `/(.)々/g`:

```rust
use verbora_normalizers::normalize_ja;
fn main() {
    assert_eq!(normalize_ja("あ々々"), "ああ々");
    assert_eq!(normalize_ja("あ々々々"), "ああああ");
    assert_eq!(normalize_ja("あい々々々々"), "あいあいい々");
    assert_eq!(normalize_ja("々々"), "々々");   // the mark doubles itself
}
```

Working over code units also reproduces the surrogate behaviour: for `a😀々々`
the two-unit pass captures the whole pair and yields `a😀😀`, whereas a
`char`-based port would capture `a` and `😀` and yield `a😀a😀`.

### Tildes are never converted

`～` U+FF5E and `〜` U+301C pass through `symbol_fh` and `normalize_ja`
unchanged. This is deliberate in the reference, not an omission here.

## Common mistakes

### Reaching for `str::replace` on Nordic text

```rust
use verbora_normalizers::normalize_no;
fn main() {
    assert_eq!(normalize_no("ààà"), "aàà");        // what the reference does
    assert_eq!("ààà".replace('à', "a"), "aaa");    // what Rust's replace does
}
```

Rust's `str::replace` replaces *all* occurrences, so a direct translation
silently diverges on any word with a repeated accent.

### Filtering the empty strings out of `normalize`

They are deliberate output, produced by `/\W+/` matching at the start or end of
the expanded token. Dropping them changes token counts and offsets:

```rust
use verbora_normalizers::normalize;
fn main() {
    assert_eq!(normalize(&["it's!"]), ["it", "is", ""]);
    assert_eq!(normalize(&["!it's"]), ["", "it", "is"]);
}
```

### Passing a sentence to `normalize_token`

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("I'm here"), ["I'm here"]);   // one token, table misses
    assert_eq!(normalize_token("I'm"), ["I", "am"]);         // this is the intended shape
}
```

Tokenize first. Note the trap: if a *rule* happens to match, the string **is**
split — `normalize_token("hasn't hasn't")` gives `["has", "not", "has", "not"]` —
so the failure is intermittent rather than obvious.

### Expecting consistent case handling

A conversion-table hit discards the token's casing; a fallback-rule hit preserves
it, because the rules run against the original token:

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("CaN'T"), ["can", "not"]);    // table: case dropped
    assert_eq!(normalize_token("Hasn't"), ["Has", "not"]);   // rule: case kept
}
```

### Assuming the diacritic folders are interchangeable

```rust
use verbora_normalizers::{normalize_no, normalize_sv, remove_diacritics};
fn main() {
    assert_eq!(remove_diacritics("blåbærsyltetøy"), "blabaersyltetoy");
    assert_eq!(normalize_no("blåbærsyltetøy"), "blåbærsyltetøy");
    assert_eq!(normalize_sv("âçêîñóôûš"), "âçêîñóôûš");
    assert_eq!(normalize_no("âçêîñóôûš"), "aceinoous");
}
```

Using `remove_diacritics` on Norwegian destroys `å ø æ`, which are distinct
letters of that alphabet, not accented variants.

### Calling `.into_owned()` on every result

The single most expensive mistake available here. On the common path
(`Cow::Borrowed`) it allocates and copies a string that was already correct. See
[The cost of `.into_owned()`](#the-cost-of-into-owned).

### Expecting `normalize_ja` to convert kana between syllabaries

It does not. `normalize_ja` normalizes *widths*, composes voiced marks and
expands compatibility symbols; hiragana stays hiragana. Use
`converters::hiragana_to_katakana` or `converters::katakana_to_hiragana`.

## Related

- [Zero-copy](../performance/zero-copy.md) — the `Cow` contract across the workspace
- [Allocation](../performance/allocation.md) — where allocations come from and how to avoid them
- [Parallelism](../performance/parallelism.md) — the one built-in `par_*` API this crate has, why the other five functions have none, and how to add your own
- [Performance](../performance/index.md) — the measurement methodology
- [Benchmarks](../benchmarks/index.md) — what has actually been measured
- [Tokenizers](../features/tokenizers.md) — what to run before `normalize`
- [Inflectors](../features/inflectors.md) — the other token-rewriting cluster
- [Core](../features/core.md) — the shared traits and the reference character semantics
- [Recipes](../recipes/index.md) — end-to-end pipelines
- [Choosing an API](../choosing/index.md) — the cross-crate decision guide

## API reference

```rust  ignore
// verbora_normalizers

pub fn normalize<S: AsRef<str>>(tokens: &[S]) -> Vec<String>;
pub fn normalize_token(token: &str) -> Vec<String>;
pub fn remove_diacritics(s: &str) -> Cow<'_, str>;
pub fn normalize_no(text: &str) -> Cow<'_, str>;
pub fn normalize_sv(text: &str) -> Cow<'_, str>;
pub fn normalize_ja(s: &str) -> Cow<'_, str>;

// feature = "parallel" — a thin rayon::par_iter().map(remove_diacritics).collect()
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>;

// verbora_normalizers::ja::converters — all seventeen, all `fn(&str) -> Cow<'_, str>`

pub fn alphabet_fh(s: &str) -> Cow<'_, str>;
pub fn numbers_fh(s: &str) -> Cow<'_, str>;
pub fn symbol_fh(s: &str) -> Cow<'_, str>;
pub fn pure_punctuation_fh(s: &str) -> Cow<'_, str>;
pub fn punctuation_fh(s: &str) -> Cow<'_, str>;
pub fn katakana_fh(s: &str) -> Cow<'_, str>;

pub fn alphabet_hf(s: &str) -> Cow<'_, str>;
pub fn numbers_hf(s: &str) -> Cow<'_, str>;
pub fn symbol_hf(s: &str) -> Cow<'_, str>;
pub fn pure_punctuation_hf(s: &str) -> Cow<'_, str>;
pub fn punctuation_hf(s: &str) -> Cow<'_, str>;
pub fn katakana_hf(s: &str) -> Cow<'_, str>;

pub fn normalize(s: &str) -> Cow<'_, str>;
pub fn fix_fullwidth_kana(s: &str) -> Cow<'_, str>;
pub fn fix_composite_symbols(s: &str) -> Cow<'_, str>;

pub fn hiragana_to_katakana(s: &str) -> Cow<'_, str>;
pub fn katakana_to_hiragana(s: &str) -> Cow<'_, str>;
```

Every function is `#[must_use]`. Modules `diacritics`, `english`, `ja` and
`nordic` are public, so the functions are also reachable at their defining paths
(`verbora_normalizers::english::normalize`, and so on); the re-exports at the
crate root are the intended spelling.

There are no types, no traits, no errors and no configuration in this crate's
public API.
