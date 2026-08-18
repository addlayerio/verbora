# Normalizers

`verbora-normalizers` provides six independent text normalizers: one expands
English contractions, three fold diacritics (a general Latin table plus narrower
Norwegian and Swedish ones), and one normalizes Japanese widths, kana and
compatibility symbols — with its seventeen individual conversions exposed
separately.

This is also the crate where `Cow` earns its keep. Normalizers are usually called
on text that needs no change at all: a Latin sentence handed to the katakana
converter, an ASCII token handed to the diacritic folder. Every function here that
returns text returns `Cow::Borrowed` when it changed nothing, and allocates only at
the first replacement — so the common case costs a scan and no heap traffic.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>6</strong> normalizer APIs are
documented and test-pinned on byte-exact output, with the deliberate behaviour
choices listed below. <code>cargo test -p verbora-normalizers</code> runs
<strong>63</strong> unit tests and <strong>18</strong> doctests.
</div>

## When to use it

- You want to fold Latin diacritics for a search key, a dictionary lookup or a
  fuzzy-match preprocessing step, and you are matching against text folded the
  same way.
- You are indexing Japanese and need halfwidth katakana, fullwidth alphanumerics
  and compatibility symbols collapsed onto one representation before tokenizing.
- You want contractions expanded before a downstream stage that does not
  understand apostrophes.
- You want the documented quirks and all — folding `ß` to `s`, `ſ` to `l`, and the
  rest catalogued below — not a textbook-correct normalization.

## When not to use it

- **You want correct Unicode normalization.** `remove_diacritics` is an 872-entry
  base-letter lookup table, not NFD followed by combining-mark stripping. It folds
  `ſ` U+017F to **`l`**, folds `ß` to `s` rather than `ss`, and leaves `e` + U+0301
  completely alone because it does not decompose. A Unicode normalization crate
  will give you different answers.
- **You want case folding, whitespace trimming or stopword removal.** None of
  these functions do any of that. `normalize` only expands contractions.
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

## The surface

Everything is a free function, everything is `#[must_use]`, nothing is fallible
and nothing holds state. There is no builder, no options struct and no trait to
implement.

| API | Job | Returns | Borrows on no-op |
|---|---|---|:--:|
| `normalize(&[S])` | expand English contractions across a token slice | `Vec<String>` | ❌ |
| `normalize_token(&str)` | the same, for the bare-string call | `Vec<String>` | ❌ |
| `remove_diacritics` | fold Latin diacritics to base letters | `Cow<'_, str>` | ✅ |
| `normalize_no` | fold Norwegian diacritics, keeping `ä ö ü å ø æ` | `Cow<'_, str>` | ✅ |
| `normalize_sv` | fold Swedish diacritics, keeping those plus `â ç ê î ñ ó ô û š` | `Cow<'_, str>` | ✅ |
| `normalize_ja` | normalize Japanese widths, kana and symbols (4 stages) | `Cow<'_, str>` | ✅ * |
| `ja::converters::*` | the seventeen individual Japanese conversions | `Cow<'_, str>` | ✅ |
| `par_remove_diacritics_batch` | many documents at once, feature `parallel` | `Vec<Cow<'_, str>>` | ✅ per document |

\* One exception: if the input contains U+3005 `々` but neither iteration-mark pass
matches (`"々"` on its own, or `"時\n々"`), `normalize_ja` still allocates the
`Vec<u16>` that stage requires, then returns the original `&str` borrowed.

"Borrows on no-op" is the column that matters at scale: the two `Vec<String>` APIs
always allocate, and the `Cow` APIs never do when they have nothing to change.

### `normalize` vs `normalize_token`

```rust  ignore
pub fn normalize<S: AsRef<str>>(tokens: &[S]) -> Vec<String>
pub fn normalize_token(token: &str) -> Vec<String>
```

They run the same algorithm — `normalize_token(s)` is `normalize(&[s])` — so a
bare-string call and a slice call always agree. **Both return `Vec<String>`, not
`String`, because one input token can expand into several output tokens.**
`"couldn't've"` is a single conversion-table entry whose value is
`["could", "not", "have"]`, and a rule hit produces an expanded string that is then
split on `/\W+/`, so `"it's!"` becomes three fields.

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("couldn't've"), ["could", "not", "have"]);
    assert_eq!(normalize_token("it's!"), ["it", "is", ""]);   // trailing empty field is real
    assert_eq!(normalize_token("o'clock"), ["o'clock"]);      // no rule matched: one token, unsplit
}
```

**Pick `normalize` unless you genuinely have one token.** Calling `normalize_token`
in a loop allocates one `Vec` per token; `normalize` over the slice allocates one
`Vec` for the whole document. `normalize` is generic over `S: AsRef<str>`, so
`&[&str]`, `&[String]` and `&Vec<String>` all work without converting first.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>normalize_token</code> does <em>not</em> split on
whitespace. <code>normalize_token("I'm here")</code> is
<code>["I'm here"]</code> — the whole string is looked up as one token, the
conversion table misses, no rule matches, and it comes back verbatim. Tokenize
first with <a href="../features/tokenizers">a tokenizer</a>.
</div>

### `normalize_ja` vs a single `ja::converters` stage

<span class="badge badge-utf16">UTF-16</span>

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

Call `normalize_ja` when you want a canonical form. Reach for an individual
converter when you want a width conversion the pipeline does not perform (nothing
in `normalize_ja` widens ASCII), when you want hiragana and katakana collapsed onto
one syllabary (the pipeline never converts between them), when you want the
fullwidth CJK punctuation `、。・「」` narrowed, or when the iteration-mark stage —
the only one that pays for a UTF-16 round trip — is unwanted.

```rust
use verbora_normalizers::ja::converters;
use verbora_normalizers::normalize_ja;

fn main() {
    // The pipeline widens halfwidth katakana, and so does this one stage.
    assert_eq!(normalize_ja("ｶﾀｶﾅ"), "カタカナ");
    assert_eq!(converters::katakana_hf("ｶﾀｶﾅ"), "カタカナ");
    // But one stage does not do the others' work.
    assert_eq!(converters::katakana_hf("時々刻々"), "時々刻々");
    assert_eq!(normalize_ja("時々刻々"), "時時刻刻");

    // Fold both syllabaries onto one, for a search key.
    assert_eq!(converters::hiragana_to_katakana("こんにちは"), "コンニチハ");
    // Widen instead of narrow — the pipeline never does this.
    assert_eq!(converters::alphabet_hf("ABC "), "ＡＢＣ　");
}
```

## Working with `Cow`

`Cow::Borrowed(s)` is a **guarantee about what happened**, not just a
representation choice: no `String` was allocated, no bytes were copied, and the
returned `&str` *is* your input — same pointer, same length. For the diacritic
folders that is equivalent to "the text is unchanged", because no table key maps to
itself. `Cow::Owned(buf)` means at least one replacement fired and exactly one
`String` was allocated to hold the result.

ASCII text through a diacritic folder allocates nothing at all, and that
short-circuit is *exact*, not an approximation: every ASCII character is either
absent from the table or maps to itself, so an all-ASCII string cannot change.
Non-ASCII text the table does not touch is rejected almost as cheaply by a
two-level bitmap, so Cyrillic, Greek, Hebrew, CJK, kana and emoji never reach the
key search. The Japanese converters work the same way — eleven of the fifteen
tables have no ASCII key at all, and the four that do are precisely the ones meant
to rewrite ASCII.

```rust
use std::borrow::Cow;

use verbora_normalizers::remove_diacritics;

fn main() {
    // ASCII: rejected by a single vectorised `is_ascii` scan.
    assert!(matches!(remove_diacritics("plain ascii text"), Cow::Borrowed(_)));

    // Non-ASCII the table has nothing to say about: rejected by the bitmap gate.
    assert!(matches!(remove_diacritics("Москва"), Cow::Borrowed(_)));

    // A real fold: one allocation.
    let folded = remove_diacritics("café");
    assert!(matches!(folded, Cow::Owned(_)));
    assert_eq!(folded, "cafe");
}
```

`matches!(result, Cow::Borrowed(_))` is the whole idiom, and it is how the crate's
own unit tests pin the behaviour, so the guarantee cannot rot silently.

### The cost of `.into_owned()`

| Value | `.into_owned()` cost |
|---|---|
| `Cow::Owned(buf)` | Free. Moves the existing `String` out; no allocation, no copy. |
| `Cow::Borrowed(s)` | One allocation plus a `memcpy` of the entire string. |

That asymmetry is the trap. `remove_diacritics(s).into_owned()` on ASCII input does
exactly the work the `Cow` existed to avoid. In a pipeline running a folder over
every document, the borrowed case is the overwhelming majority, so an unconditional
`.into_owned()` converts an allocation-free pass into one allocation per document.
Use it only where you need an owned `String` — storing the result in a struct,
sending it across a thread boundary, or returning it from a function that cannot
name the input's lifetime — and do it once, at the boundary.

A `Cow<str>` derefs to `&str` and implements `PartialEq<&str>`, so reading the
result — `.len()`, `.chars()`, comparing against a literal — needs no decision at
all. The decision only appears when you must own it.

### Composing normalizers without allocating per stage

Chaining stages naively does not compile, because each stage's `Cow` borrows the
previous stage's local. The obvious fix — `.into_owned()` after each stage —
compiles and throws away the whole point of the `Cow`, allocating once per stage
even when every stage was a no-op.

The fix is a small adapter that applies the next stage to a `Cow` and gives the
borrow back when neither step changed anything. `verbora-normalizers` uses one
internally; it is **not exported**, so write your own — this is the whole of it:

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
            // Inspect the next stage's result *before* the temporary borrow
            // ends: if it borrowed, nothing changed and the existing buffer is
            // handed back unmoved. Only a genuine change allocates.
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

It composes to any depth, including over `normalize_ja`, which is already four
stages internally. Text that needs none of them costs one scan per stage and zero
allocations.

## The `ja::converters` surface

Seventeen free functions: six convert fullwidth to halfwidth, six convert back, two
convert between the kana syllabaries, and three combine or repair the others. All
return `Cow<'_, str>`, none hold state, and all are pinned by their own regression
suite (5,742 recorded cases each).

| Function | Converts | Direction |
|---|---|---|
| `alphabet_fh` | Fullwidth Latin letters, and U+3000 ideographic space → ASCII letters and space (53 keys) | full → half |
| `numbers_fh` | Fullwidth digits `０-９` → ASCII digits (10 keys) | full → half |
| `symbol_fh` | Fullwidth symbols → ASCII. Both U+FF0D and U+2500 map to `-` (33 keys) | full → half |
| `pure_punctuation_fh` | `、。・「」` → `､｡･｢｣` (5 keys) | full → half |
| `punctuation_fh` | `symbol_fh` and `pure_punctuation_fh` merged into one pass (38 keys) | full → half |
| `katakana_fh` | Fullwidth katakana → halfwidth, expanding voiced kana into two characters (`ガ` → `ｶﾞ`) | full → half |
| `alphabet_hf` | ASCII letters, and the ASCII space, → fullwidth. The space really does widen to U+3000 | half → full |
| `numbers_hf` | ASCII digits → fullwidth digits (10 keys) | half → full |
| `symbol_hf` | ASCII symbols → fullwidth. `-` maps to U+2500, **not** U+FF0D (32 keys) | half → full |
| `pure_punctuation_hf` | `､｡･｢｣` → `、。・「」` (5 keys) | half → full |
| `punctuation_hf` | `symbol_hf` and `pure_punctuation_hf` merged into one pass (37 keys) | half → full |
| `katakana_hf` | Halfwidth katakana → fullwidth, composing base + voiced mark (`ｶﾞ` → `ガ`) (84 keys) | half → full |
| `normalize` | Fullwidth alnum, U+3000 and fullwidth symbols → half; halfwidth punctuation and katakana → full (185 keys) | mixed, both ways |
| `fix_fullwidth_kana` | Base kana + standalone spacing voiced mark → composed kana; small tsu before the n-row → `ん`/`ン` (64 two-char keys) | in-place, same width |
| `fix_composite_symbols` | Single-codepoint CJK compatibility symbols → spelled-out forms: `㍼`→`昭和`, `㌫`→`パーセント` (161 keys) | one char → many |
| `hiragana_to_katakana` | Hiragana → fullwidth katakana | hiragana → katakana |
| `katakana_to_hiragana` | Katakana → hiragana | katakana → hiragana |

Three rows look like bugs and are deliberate:

- **`symbol_hf("-")` is U+2500 BOX DRAWINGS LIGHT HORIZONTAL, not U+FF0D.** The
  half-to-full tables are built by inverting their counterparts, last-writer-wins;
  both U+FF0D and U+2500 map to `-`, and U+2500 is listed second. That also shrinks
  the table from 33 entries to 32, and the merged punctuation table from 38 to 37.
- **`alphabet_hf` widens the ASCII space to U+3000**, because the table is the flip
  of `alphabet_fh`'s, which contains U+3000 → U+0020.
- **`fix_fullwidth_kana` is not only a diacritic fixer.** Five entries per syllabary
  rewrite a small tsu before the n-row: `っな` → `んな`. That is a phonetic change,
  it is why `normalize_ja("まっなか")` is `"まんなか"`, and it fires inside
  `hiragana_to_katakana` too.

```rust
use verbora_normalizers::ja::converters;

fn main() {
    assert_eq!(converters::alphabet_fh("ＡＢＣ　ＡＢＣ"), "ABC ABC");
    assert_eq!(converters::symbol_fh("－─"), "--");
    assert_eq!(converters::katakana_fh("ヴガパー゛゜"), "ｳﾞｶﾞﾊﾟｰﾞﾟ");
    assert_eq!(converters::katakana_hf("ｳﾞｶﾞﾊﾟｰﾞﾟ"), "ヴガパー゛゜");
    assert_eq!(converters::symbol_hf("-"), "─");           // U+2500, not U+FF0D
    assert_eq!(converters::normalize("ＡＢｶﾞ"), "ABガ");
    assert_eq!(converters::fix_fullwidth_kana("か゛"), "が");
    assert_eq!(converters::fix_fullwidth_kana("っな"), "んな");
    assert_eq!(converters::fix_composite_symbols("㍼㍿㋀"), "昭和株式会社1月");
}
```

## Advanced usage

### Folding a search key

The idiomatic use is one function that produces a canonical form and returns a
`Cow`, so callers who pass already-canonical text pay nothing. Fold **both sides**
of a comparison with the same function: folding only the query is a common bug, and
so is assuming the folder repairs mixed forms — it does not decompose, so a stored
`e` + U+0301 folds to itself while a query `é` U+00E9 folds to `e`, and the two
never meet.

### Skipping the per-token `String` when a token cannot change

`normalize` and `normalize_token` always allocate. But both the five-entry
conversion table and all six fallback rules require an **ASCII apostrophe** U+0027,
so a token with no `'` always comes back as exactly one output token equal to the
input — which you can exploit at the call site:

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

`str::contains('\'')` is a `memchr`-class scan. This does not remove the outer
`Vec`, but it removes the per-token `String` for the vast majority of real tokens,
and skips the six per-rule apostrophe scans the expander would otherwise run.

### Parallelism

`par_remove_diacritics_batch` is the one built-in parallel API, behind this crate's
`parallel` Cargo feature (`parallel = ["dep:rayon"]`, never on by default). It is
exactly `inputs.par_iter().map(remove_diacritics).collect()` — a thin fan-out over
the sequential function, not a second implementation. Crucially it returns
`Vec<Cow<'a, str>>` and does **not** force `.into_owned()` on every result the way
a hand-rolled cross-thread collect typically would, so the zero-copy property
survives: a batch of already-ASCII documents still allocates nothing.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled.
use verbora_normalizers::par_remove_diacritics_batch;

let inputs = ["café", "naïve", "ASCII only"];
let got = par_remove_diacritics_batch(&inputs);
assert_eq!(got, ["cafe", "naive", "ASCII only"]);
```

**When it pays.** `remove_diacritics` is already cheap per call — a few hundred
nanoseconds to tens of microseconds per document — while a `rayon` task costs on
the order of a microsecond to schedule. Measured against a fixed ~1.2 KB document:
a 16-document batch is roughly **1.5× slower** in parallel than the sequential
loop, while 256- and 4096-document batches are **5–8× faster**. Reach for it once
the batch runs to hundreds of documents.

Every other function here is a free function with no shared state, no interior
mutability and no globals, so you can parallelise over them yourself with your own
`rayon` dependency and no Cargo feature — but collecting across threads that way
forces `.into_owned()`, reintroducing the allocation the `Cow` avoids. See
[Parallelism](../performance/parallelism.md).

## Performance and allocation

All six top-level functions are **O(n) in the input length** with a small constant.
There is no backtracking, no regex engine and no automaton — every pattern here is
a one-or-two-character table lookup or a hand-written scanner.
`verbora-normalizers` has **no dependencies at all** on its default features, not
even on the rest of the workspace; the single optional one is `rayon`.

Passes over the input: one for `remove_diacritics` (zero for ASCII — one
vectorised `is_ascii` scan and return), one for `normalize_no` / `normalize_sv`,
one per converter (three for the two kana converters), four stages for
`normalize_ja` (stage 1 skipped entirely without U+3005), and one per token for
`normalize` plus one `/\W+/` split per token a rule rewrote. `remove_diacritics`
is **one pass, not 86**: every replacement is itself an ASCII letter that maps to
itself, so no replacement can be re-matched by a later lookup and cost scales with
input length rather than with the number of underlying rules. `normalize_no` /
`normalize_sv` are likewise one pass carrying a spent-pair mask, not one scan per
accent pair.

| Function | Unchanged input | Changed input |
|---|---|---|
| `remove_diacritics` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `s.len()`. Never reallocates: no replacement is longer in bytes than its source character |
| `normalize_no` / `normalize_sv` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `text.len()`. Never reallocates: every replacement is a 1-byte ASCII character replacing a 2-byte source |
| `normalize_ja` | **Zero allocations** with no U+3005; with U+3005, stage 1 pays one `Vec<u16>` even when neither pass matches | Stage 1: that `Vec<u16>` plus one per matching pass and one `String`. Stages 2–4: at most one `String` each, only for a stage that changes something |
| `ja::converters::*` | **Zero allocations**, `Cow::Borrowed` | One `String`, capacity `s.len()`. Six tables can outgrow it: `alphabet_hf`, `numbers_hf`, `symbol_hf`, `punctuation_hf`, `katakana_fh` and `fix_composite_symbols`. The other nine never reallocate |
| `normalize` | One `Vec<String>` with capacity `tokens.len()`, plus one `String` per output token | Same, plus one intermediate `String` per token a fallback rule rewrote |
| `normalize_token` | One `Vec<String>` with capacity 2, plus one `String` | Same, plus one intermediate `String` on a rule hit |

Two constraints to plan around: **there is no `_into` API**, so there is no way to
reuse an output allocation across calls — the `Cow` functions do not need one, but
`normalize` and `normalize_token` genuinely cannot avoid it today; and **there is
no batch API except `par_remove_diacritics_batch`** (`normalize` taking a slice is
the natural unit for contraction expansion, not a batch entry point).

Measured today: `remove_diacritics` runs from roughly **270 ns** for a 128-byte
accented document to roughly **35 µs** for a 19 KB one. The Criterion suite is
split into *rejection cost* (Latin prose handed to the katakana converter, ASCII
handed to the diacritic folder) and *work cost* (text that is entirely
replacements). See [Benchmarks](../benchmarks/index.md).

## Unicode and language notes

**`remove_diacritics` is a table, not Unicode normalization.** It does not
decompose: precomposed `é` U+00E9 folds to `e`, while `e` + U+0301 passes through
untouched. The quirks are part of the contract — `ſ` U+017F folds to **`l`**, `ß`
folds to `s` and `ẞ` to `S` (not `ss`/`SS`), `İ` folds to `I` and `ı` to `i`.
Ligatures `ﬁ`/`ﬂ`, `Ĳ`/`ĳ`, `Ŋ`/`ŋ`, `ĸ`, `ȸ`/`ȹ` and every non-BMP character are
left alone, while circled letters `ⒶⓏⓐⓩ` and fullwidth `ＡＺａｚ` do fold, because
the table lists them.

**The Nordic folders fold only their own accents, and each pair fires once.**
`normalize_no` and `normalize_sv` replace only the first occurrence of each accented
character; upper- and lowercase are separate pairs, so each folds once.

```rust
use verbora_normalizers::{normalize_no, normalize_sv};
fn main() {
    assert_eq!(normalize_no("ààà"), "aàà");           // first occurrence only
    assert_eq!(normalize_no("àÀàÀ"), "aAàÀ");         // each pair has its own first
    assert_eq!(normalize_sv("ààà ééé À É è È"), "aàà eéé A E e E");
}
```

**`normalize` uses an ASCII-only word class**, `[A-Za-z0-9_]` — not Rust's `regex`
crate default, which is Unicode-aware and would make `é` and `漢` word characters.
Only the ASCII apostrophe U+0027 counts, so `I’d` with U+2019 is left alone
entirely. The `n` of `n't` is matched literally and lowercase only, while the
letters after the apostrophe are matched case-insensitively — `"N'T"` does not
match, `"hasn'T"` does. Conversion-table lookups do use full Unicode lowercasing:
U+212A KELVIN SIGN lowercases to ASCII `k`, so "a non-ASCII token cannot fold into
an ASCII key" is not a safe assumption.

```rust
use verbora_normalizers::normalize_token;
fn main() {
    assert_eq!(normalize_token("héllo's"), ["h", "llo", "is"]);
    assert_eq!(normalize_token("漢字's"), ["", "is"]);
    assert_eq!(normalize_token("a_b1's"), ["a_b1", "is"]);
}
```

**`normalize_ja` stage one matches UTF-16 code units**, not scalar values, and
excludes exactly four line terminators: `\n`, `\r`, U+2028 and U+2029. It *does*
match `\t`, `\v`, `\f`, U+0085, U+00A0, U+3000 and U+FEFF. Because it works over
code units, a surrogate pair is matched as a single unit — but a one-unit pass can
capture only half of one, and since a Rust `String` cannot hold an unpaired
surrogate, that half becomes U+FFFD. Positions, lengths and every well-formed
result are unaffected. The pass order is load-bearing and observable: `/(..)々々/g`
fires first, and its output is then eligible for `/(.)々/g`.

```rust
use verbora_normalizers::normalize_ja;
fn main() {
    assert_eq!(normalize_ja("\n々"), "\n々");                    // excluded
    assert_eq!(normalize_ja("\u{FEFF}々"), "\u{FEFF}\u{FEFF}");   // matched
    assert_eq!(normalize_ja("あ々々"), "ああ々");
    assert_eq!(normalize_ja("々々"), "々々");   // the mark doubles itself
    // Surrogate pairs: whole from the two-unit pass, halved (and so U+FFFD)
    // from the one-unit pass.
    assert_eq!(normalize_ja("a😀々々"), "a😀😀");
    assert_eq!(normalize_ja("😀々"), "😀\u{FFFD}");
}
```

**Tildes are never converted.** `～` U+FF5E and `〜` U+301C pass through `symbol_fh`
and `normalize_ja` unchanged: neither is a key in the symbol table.

## Common mistakes

**Reaching for `str::replace` on Nordic text.** Rust's `str::replace` replaces
*all* occurrences; `normalize_no` replaces only the first occurrence of each accent
pair, so substituting it silently changes the output on any word with a repeated
accent.

**Filtering the empty strings out of `normalize`.** They are deliberate output,
produced by `/\W+/` matching at the start or end of the expanded token — dropping
them changes token counts and offsets. `normalize(&["it's!"])` is `["it", "is", ""]`
and `normalize(&["!it's"])` is `["", "it", "is"]`.

**Passing a sentence to `normalize_token`.** Tokenize first. Note the trap: if a
*rule* happens to match, the string **is** split — `normalize_token("hasn't hasn't")`
gives `["has", "not", "has", "not"]` — so the failure is intermittent rather than
obvious.

**Expecting consistent case handling.** A conversion-table hit discards the token's
casing and a fallback-rule hit preserves it, because the rules run against the
original token: `normalize_token("CaN'T")` is `["can", "not"]` but
`normalize_token("Hasn't")` is `["Has", "not"]`.

**Assuming the diacritic folders are interchangeable.** Using `remove_diacritics`
on Norwegian destroys `å ø æ`, which are distinct letters of that alphabet, not
accented variants:

```rust
use verbora_normalizers::{normalize_no, normalize_sv, remove_diacritics};
fn main() {
    assert_eq!(remove_diacritics("blåbærsyltetøy"), "blabaersyltetoy");
    assert_eq!(normalize_no("blåbærsyltetøy"), "blåbærsyltetøy");
    assert_eq!(normalize_sv("âçêîñóôûš"), "âçêîñóôûš");
    assert_eq!(normalize_no("âçêîñóôûš"), "aceinoous");
}
```

**Calling `.into_owned()` on every result.** The single most expensive mistake
available here — see [The cost of `.into_owned()`](#the-cost-of-into-owned).

**Expecting `normalize_ja` to convert kana between syllabaries.** It does not. Use
`converters::hiragana_to_katakana` or `converters::katakana_to_hiragana`.

## Related

- [Zero-copy](../performance/zero-copy.md) — the `Cow` contract across the workspace
- [Allocation](../performance/allocation.md) · [Parallelism](../performance/parallelism.md) ·
  [Performance](../performance/index.md) · [Benchmarks](../benchmarks/index.md)
- [Tokenizers](../features/tokenizers.md) — what to run before `normalize`
- [Inflectors](../features/inflectors.md) — the other token-rewriting cluster
- [Core](../features/core.md) — the shared traits and whitespace/character semantics
- [Recipes](../recipes/index.md) · [Choosing an API](../choosing/index.md)

## API reference

```rust  ignore
// verbora_normalizers

pub fn normalize<S: AsRef<str>>(tokens: &[S]) -> Vec<String>;
pub fn normalize_token(token: &str) -> Vec<String>;
pub fn remove_diacritics(s: &str) -> Cow<'_, str>;
pub fn normalize_no(text: &str) -> Cow<'_, str>;
pub fn normalize_sv(text: &str) -> Cow<'_, str>;
pub fn normalize_ja(s: &str) -> Cow<'_, str>;

// feature = "parallel"
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>;

// verbora_normalizers::ja::converters — all seventeen, all `fn(&str) -> Cow<'_, str>`
pub fn alphabet_fh / numbers_fh / symbol_fh / pure_punctuation_fh / punctuation_fh / katakana_fh;
pub fn alphabet_hf / numbers_hf / symbol_hf / pure_punctuation_hf / punctuation_hf / katakana_hf;
pub fn normalize / fix_fullwidth_kana / fix_composite_symbols;
pub fn hiragana_to_katakana / katakana_to_hiragana;
```

Every function is `#[must_use]`. Modules `diacritics`, `english`, `ja` and `nordic`
are public, so the functions are also reachable at their defining paths; the
re-exports at the crate root are the intended spelling. There are no types, no
traits, no errors and no configuration in this crate's public API.

Source: `crates/verbora-normalizers/src/`. Benchmarks:
`crates/verbora-normalizers/benches/normalizers.rs`.
