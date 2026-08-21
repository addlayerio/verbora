# Transliterators

`verbora-transliterators` has exactly one entry point, `transliterate_ja`:
Japanese kana to modified-Hepburn romaji. `とうきょう` becomes `tōkyō`, `ザッシ`
becomes `zasshi`, `ほんや` becomes `hon'ya`. That is the whole subsystem — one
conversion, in one direction, for one language.

Modified Hepburn is deceptively intricate: the geminate consonant, the syllabic
nasal and the prolonged sound mark each depend on a neighbouring character
rather than on themselves, and a long vowel can be written three different ways.
This crate implements all of it as **one left-to-right pass** over a generated
index, with no regex engine and no backtracking. Text with no kana in it is
returned borrowed after a single vectorised byte scan.

## When to use it

- **You need a Latin search key for Japanese text**, and you produce both sides
  of the comparison with this same function.
- **You want kana romanised inside otherwise mixed text.** Kanji, Latin,
  punctuation and halfwidth katakana pass through untouched, so running this
  over a mixed-language corpus is safe and — for documents with no kana —
  nearly free.
- **You want the replacements without the string** — byte ranges for
  highlighting, alignment or counting morae. See
  [Inspecting the rewrites](#inspecting-the-rewrites).
- **You have many independent, document-scale inputs.** See
  [`par_transliterate_ja_batch`](#par-transliterate-ja-batch).

## When not to use it

- **You want romaji back into kana.** There is no reverse function, and the
  mapping is not injective: `ā` comes from `ああ` and `あー` alike.
- **You want linguistically correct romanisation.** This is grapheme-driven
  modified Hepburn with no dictionary and no notion of a word. The topic
  particle `は` is romanised `ha`, so `こんにちは` becomes `konnichiha`, not
  `konnichiwa`; and `おう` is always long, so `おもう` is `omō` where a
  morphological analyser would say `omou`.
- **You want kanji read aloud.** Kanji is not touched at all —
  `これは日本語のテストです。` becomes `koreha日本語notesutodesu。`.
- **Your input is halfwidth or decomposed.** The index is spelled in NFC, so
  `ｱｲｳｴｵ` comes back unchanged. Use
  [`transliterate_ja_normalized`](#transliterate-ja-normalized), or fold widths
  first with [`nfkc`](./normalizers.md).
- **You want iteration marks expanded.** `々` passes through unchanged, and
  nothing in Verbora expands it — iteration-mark expansion is an orthographic
  rewrite with no Unicode definition, and it is not idempotent.
- **You want tokens.** This is a character-level rewriter. Verbora ships no
  Japanese word segmenter; see [Tokenizers](./tokenizers.md) for what UAX #29
  does and does not do here.

## Quick example

```rust
use verbora_transliterators::transliterate_ja;

fn main() {
    assert_eq!(transliterate_ja("あいうえお かきくけこ"), "aiueo kakikukeko");
    assert_eq!(transliterate_ja("とうきょう"), "tōkyō");
    assert_eq!(transliterate_ja("コーヒー"), "kōhī");

    // The geminate consonant and the two faces of ン.
    assert_eq!(transliterate_ja("まっか ざっし たった はっぱ"), "makka zasshi tatta happa");
    assert_eq!(transliterate_ja("まんと ばんび ほんや"), "manto bambi hon'ya");

    // Everything that is not kana passes through.
    assert_eq!(transliterate_ja("abc ABC 漢字 (.)"), "abc ABC 漢字 (.)");
    assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
}
```

## The unit is the mora

Not the byte, not the scalar value, not the grapheme cluster. A **mora** is
spelled in kana as one or two Unicode scalar values, optionally followed by one
more that lengthens it:

| Spelling | Scalars | Romaji |
|---|---|---|
| `か` | 1 | `ka` |
| `きょ` | 2 (base + small `ょ`) | `kyo` |
| `かー` | 1 + prolonged sound mark | `kā` |
| `こう` | 1 + lengthening vowel kana | `kō` |

The scalar value is the wrong unit because `きょ` is one mora written with two of
them; the grapheme cluster is the wrong unit because `かー` is two clusters and
one mora.

Three marks carry a mora but have **no reading of their own**, because what they
romanize as depends on their neighbour. The scanner resolves them rather than a
table:

| Mark | Rule | Example |
|---|---|---|
| sokuon `っ` `ッ` | doubles the following consonant; `t` before `ch` | `ざっし` → `zasshi`, `まっちゃ` → `matcha` |
| syllabic nasal `ん` `ン` | `m` before `b`/`m`/`p`, `n'` before a vowel or `y`, else `n` | `ばんび` → `bambi`, `ほんや` → `hon'ya`, `まんと` → `manto` |
| prolonged sound mark `ー` | macron over the preceding vowel | `スーパー` → `sūpā` |

The sokuon rule is applied to the **romanization**, not to the kana, which is
what reproduces the columns that look like exceptions when written in kana:
`っし` is `sshi` because `し` is `shi`, and `っふ` is `ffu` because `ふ` is `fu`.

```rust
use verbora_transliterators::transliterate_ja;

fn main() {
    assert_eq!(transliterate_ja("ざっし"), "zasshi");
    assert_eq!(transliterate_ja("まっちゃ"), "matcha");
    assert_eq!(transliterate_ja("かんぱい"), "kampai");
    assert_eq!(transliterate_ja("しんぶん"), "shimbun");
    assert_eq!(transliterate_ja("スーパー"), "sūpā");
}
```

A mark with nothing to modify romanizes as **nothing at all**: a sokuon with no
consonant after it, and a prolonged sound mark with no vowel before it, are
modifiers with no target. No romanization standard assigns either a segment of
its own, and leaving the kana in place would put a character in romanized text
that the caller asked to have romanized.

```rust
use verbora_transliterators::transliterate_ja;

fn main() {
    assert_eq!(transliterate_ja("ッ"), "");
    assert_eq!(transliterate_ja("ー"), "");
    // A run of prolonged marks lengthens once, rather than lengthening and
    // then having nothing left to lengthen.
    assert_eq!(transliterate_ja("あーー"), "ā");
}
```

### Where the readings come from

**Modified Hepburn**, as codified in the *ALA-LC Romanization Tables: Japanese*
(American Library Association / Library of Congress), which follows
ANSI Z39.11-1972 and BS 4812:1972 — plus 内閣告示第二号「外来語の表記」 (Cabinet
of Japan, Notification No. 2 of 1991) for the extended syllables Japanese writes
foreign sounds with (`ファ`, `ティ`, `ヴァ`, `クォ`, …).

Every mora, its reading and the citation for it live in `src/syllabary.rs`,
which is the crate's single source of truth: `build.rs` derives the katakana
half, the long-vowel forms and the lookup index from that one file. Six
characters take their reading from the Unicode Character Database instead,
because their character names *are* their readings — the four
`KATAKANA LETTER V*` (U+30F7..U+30FA) and the digraphs `ゟ` U+309F and `ヿ`
U+30FF.

Two sequences are deliberately **not** treated as long, because ALA-LC does not
treat them as long — a long `i` is written doubled, and `えい` is two vowels:

```rust
use verbora_transliterators::transliterate_ja;

fn main() {
    assert_eq!(transliterate_ja("おいしい"), "oishii");  // not "oishī"
    assert_eq!(transliterate_ja("せんせい"), "sensei");  // not "sensē"
}
```

The prolonged sound mark is not bound by that rule, since it lengthens *any*
vowel: `シー` is `shī` while `しい` is `shii`.

## Choosing the right API

One conversion, five ways to ask for it. The genuine decision is between
`transliterate_ja` (hands you a `Cow`) and `transliterate_ja_into` (writes into
a buffer you own); the rest is whether your input needs normalizing first, and
whether you want the replacements without the string.

| API | Best for | Lazy | Output | Clears `out` | Allocations |
|---|---|:--:|---|:--:|---|
| `transliterate_ja(s)` | one string, simplest call | ❌ | `Cow<'_, str>` | n/a | none when no mora matched; else one `String` |
| [`par_transliterate_ja_batch(inputs)`](#par-transliterate-ja-batch) | many independent, document-scale strings | ❌ | `Vec<Cow<'_, str>>` | n/a | one output `Vec`, plus the above per input; feature `parallel` |
| [`transliterate_ja_into(s, &mut out)`](#transliterate-ja-into) | concatenating many results into one buffer | ❌ | `()`, appends | ❌ **appends** | none of its own, plus `out`'s growth |
| [`transliterate_ja_normalized(s)`](#transliterate-ja-normalized) | halfwidth katakana, decomposed kana | ❌ | `Cow<'_, str>` | n/a | up to three `String`s |
| [`Rewrites::new(s)`](#inspecting-the-rewrites) | inspecting *what* would change | ✅ | `Rewrites<'_>` → `Rewrite<'_>` | n/a | **none, ever** |

Two columns deserve a second look. **`transliterate_ja_into` appends** — Verbora
has two clearing conventions and this crate uses the appending one (see
[Buffer reuse](../performance/buffer-reuse.md)); `out.clear()` is yours to
call, and it is safe, since `clear()` never frees capacity. **Only `Rewrites` is
lazy** — but every other entry point is built on it, so none of them allocates
until a replacement is actually spliced.

None of the four is faster than `transliterate_ja` at what `transliterate_ja`
does: they all drive the same scan, and the only cost they can remove is the
output `String`.

### `transliterate_ja`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

```rust  ignore
pub fn transliterate_ja(text: &str) -> Cow<'_, str>   // #[must_use]
```

The default. It returns `Cow::Borrowed` for Latin text, kanji, halfwidth
katakana, emoji and every other script — the common case in mixed corpora, and
one `memchr`-class scan for the lead byte `0xE3`:

```rust
use std::borrow::Cow;

use verbora_transliterators::transliterate_ja;

fn main() {
    // No 0xE3 lead byte: one vectorised scan, zero allocations.
    assert!(matches!(transliterate_ja("the quick brown fox"), Cow::Borrowed(_)));
    assert!(matches!(transliterate_ja("ｱｲｳｴｵ"), Cow::Borrowed(_)));

    // Kana: owned.
    assert!(matches!(transliterate_ja("カタカナ"), Cow::Owned(_)));
}
```

Hold the `Cow` as long as you can — it derefs to `&str` — and call
`.into_owned()` only at a boundary that genuinely needs a `String`.

The function's contract is four properties, each pinned by the crate's own
tests:

- **Total.** Every `&str` is accepted and no input panics.
- **Idempotent.** `transliterate_ja(transliterate_ja(t))` equals
  `transliterate_ja(t)` for every `t`: the output contains no mora the scanner
  would rewrite again.
- **Borrowed exactly when unchanged.** `Cow::Borrowed` is returned if and only
  if no mora was found, which makes matching on the `Cow` a correct way to ask
  "did this contain kana?" rather than a fast path that might stop working.
- **Nothing is invented.** The output contains no `U+FFFD` unless the input did,
  and no character that was not either a reading from the syllabary or a byte
  copied from the input.

<div class="callout callout-note">
<strong>Note.</strong> The gate is a <em>superset</em> test: it admits any
string containing the UTF-8 lead byte <code>0xE3</code>, which covers all of
U+3000..U+3FFF. <code>々</code> U+3005 passes the gate and then matches
nothing, so <code>transliterate_ja("々")</code> still returns
<code>Cow::Borrowed</code> — just after a scan instead of after the gate.
</div>

### `par_transliterate_ja_batch`

```rust  ignore
#[cfg(feature = "parallel")]
pub fn par_transliterate_ja_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>
```

`transliterate_ja` is a pure function of one `&str` with no shared state, so many
independent documents are embarrassingly parallel. This function, behind the
`parallel` Cargo feature, is exactly
`inputs.par_iter().map(transliterate_ja).collect()` — a thin fan-out over the
sequential primitive, not a second implementation. Output order matches input
order, and the `Vec<Cow<'_, str>>` keeps every borrowed document borrowed.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled.
use verbora_transliterators::par_transliterate_ja_batch;

fn main() {
    let inputs = ["あいうえお", "ざっし", "plain ascii"];
    let got = par_transliterate_ja_batch(&inputs);
    assert_eq!(got, ["aiueo", "zasshi", "plain ascii"]);
}
```

A `rayon` task costs on the order of a microsecond to schedule, so a batch of
short strings can easily cost more to distribute than to romanize. Reach for
this when the inputs are document-scale and there are more than a handful of
them; a plain `inputs.iter().map(transliterate_ja).collect()` loop is the better
answer otherwise. See
[Performance characteristics](#performance-characteristics) for the shape of the
trade.

This is the crate's only built-in parallel API. For a different shape (a shared
output buffer built with `transliterate_ja_into`, say), apply the same
`par_iter().map(...)` at your own call site — every item here is a free function
over `&'static` tables, with no state, no interior mutability and no globals.
Note that collecting `String`s across threads forces `.into_owned()`,
reintroducing an allocation for documents that would otherwise have been
borrowed. See [Parallelism](../performance/parallelism.md).

### `transliterate_ja_into`

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

```rust  ignore
pub fn transliterate_ja_into(text: &str, out: &mut String)   // APPENDS
```

**It appends. It does not clear.** That is the contract, and it is what makes
the accumulate pattern need no special API:

```rust
use verbora_transliterators::transliterate_ja_into;

fn main() {
    let mut doc = String::new();
    for word in ["こんにちは", " ", "せかい"] {
        transliterate_ja_into(word, &mut doc);   // no clear, ever
    }
    assert_eq!(doc, "konnichiha sekai");

    // Non-kana fragments are pushed through verbatim, so separators and
    // punctuation can go through the same call.
    let mut mixed = String::from("[");
    transliterate_ja_into("カナ", &mut mixed);
    transliterate_ja_into("]", &mut mixed);
    assert_eq!(mixed, "[kana]");
}
```

For one result per iteration rather than one accumulated document, the usual
[buffer-reuse](../performance/buffer-reuse.md) ritual applies unchanged:
`String::with_capacity` once outside the loop, `buf.clear()` at the top of each
iteration, then `transliterate_ja_into(word, &mut buf)`.

Unlike `transliterate_ja` it never allocates an intermediate `String` — it
splices directly into `out` — and unlike `Rewrites` it does the splicing for
you. A loop over `n` inputs therefore performs the growth of one buffer instead
of `n` allocations and `n` copies.

### `transliterate_ja_normalized`

```rust  ignore
pub fn transliterate_ja_normalized(text: &str) -> Cow<'_, str>   // #[must_use]
```

The index is spelled in NFC — `が` is U+304C, not `か` U+304B followed by U+3099
— so decomposed kana, halfwidth katakana and fullwidth Latin do not match and
pass through. This function respells the spacing voiced sound marks, applies
[`nfkc`](./normalizers.md), and then runs the scan:

```rust
use verbora_transliterators::{transliterate_ja, transliterate_ja_normalized};

fn main() {
    // Halfwidth katakana is invisible to the index …
    assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
    // … until it is widened first.
    assert_eq!(transliterate_ja_normalized("ｱｲｳｴｵ"), "aiueo");

    // Composed voiced marks come along too: ｶ + ﾞ → ガ → ga.
    assert_eq!(transliterate_ja_normalized("ｶﾞｯｷ"), "gakki");
}
```

**Reach for it by default on input whose spelling you do not control**, and for
`transliterate_ja` when you know the text is already NFC.

The extra re-spelling it does before `nfkc` is not cosmetic. The **spacing**
voiced sound marks U+309B `゛` and U+309C `゜` — the legacy Shift-JIS spelling —
carry the compatibility mappings `<compat> 0020 3099` and `<compat> 0020 309A`,
and that `U+0020` is a starter, so NFKC on its own strands the mark on an
invented space instead of composing it onto the preceding kana. This function
re-spells those two scalars as the bare combining marks first, which is the
standard's own mapping without the space, and is what the halfwidth U+FF9E
already decomposes to. There is no `_into` variant.

## Inspecting the rewrites

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

`Rewrites` and `Rewrite` are the crate's whole type surface, and a normal caller
needs neither. `Rewrites` is the crate's single implementation of what
romanization *is* — `transliterate_ja`, `transliterate_ja_into` and
`transliterate_ja_normalized` are all built on it, so there is one description
of the behaviour and no second copy to drift. Reach for it when you need the
byte ranges rather than the text: highlighting, alignment, counting morae, or
stopping early.

```rust  ignore
pub struct Rewrite<'a> {
    pub start: usize,        // byte offset where the replaced text begins
    pub end: usize,          // byte offset one past the end
    pub from: &'a str,       // the slice being replaced — &text[start..end]
    pub to: &'static str,    // the text written in its place, possibly empty
}
```

Byte offsets, not character indices, because that is what splicing needs. `to`
is `&'static str` because every reading comes from a static table — which is why
yielding rewrites allocates nothing at all.

```rust
use verbora_transliterators::Rewrites;

fn main() {
    // What the scan would do, without doing it.
    let hits: Vec<_> = Rewrites::new("かんぱい")
        .map(|r| (r.start, r.end, r.from, r.to))
        .collect();
    assert_eq!(
        hits,
        [(0, 3, "か", "ka"), (3, 6, "ん", "m"), (6, 9, "ぱ", "pa"), (9, 12, "い", "i")]
    );

    // One mora can span two scalars, and the rewrite reports its whole extent.
    let hits: Vec<_> = Rewrites::new("とうきょう").map(|r| (r.from, r.to)).collect();
    assert_eq!(hits, [("とう", "tō"), ("きょう", "kyō")]);

    // Counting allocates nothing — no output string is ever built, and the
    // whole-input gate makes the iterator empty without scanning.
    assert_eq!(Rewrites::new("カタカナ").count(), 4);
    assert_eq!(Rewrites::new("abc").size_hint(), (0, Some(0)));
}
```

A mora that romanizes to nothing is reported as a rewrite **to `""`** rather
than skipped, so the stream describes the whole transformation and not merely
its visible half:

```rust
use verbora_transliterators::Rewrites;

fn main() {
    let hits: Vec<_> = Rewrites::new("ざっし").map(|r| (r.from, r.to)).collect();
    assert_eq!(hits, [("ざ", "za"), ("っ", "s"), ("し", "shi")]);
}
```

`size_hint` is `(0, Some(remaining bytes))`: every rewrite consumes at least one
byte, and nothing is guaranteed to match. `Rewrites` is `Clone` and `Debug`, and
is **not** declared `FusedIterator`, though it does keep returning `None` once
the scan is finished. `Rewrite` is `Copy`, `PartialEq` and `Eq`.

## Performance characteristics

The scan is **O(n) in the input length** with a small constant. One left-to-right
pass, no backtracking, no regex engine, no automaton, no runtime construction
step. The crate's only dependency is `verbora-normalizers`, and only for
`transliterate_ja_normalized`.

| Stage | Cost per character when nothing matches |
|---|---|
| Whole input, once | One vectorised `slice::contains(&0xE3)`. A document with no such byte skips the scan entirely |
| The index | One subtraction and one bounds test against `codepoint - 0x3041`, which is a direct slot index rather than a search. All kana are BMP, so astral characters fail the bounds test |
| A two-scalar mora | A short scan of the slot's own range in the 142-entry two-scalar table, entered only for a character that genuinely begins one |
| The three marks | Resolved from the neighbouring mora's first letter, which is one more index lookup |

The index is 191 slots — one per code point in U+3041..=U+30FF — plus 142
two-scalar morae. Because a slot is reached by subtraction rather than by
search, the cost of the lookup does not vary with how much of the syllabary a
document uses.

<div class="callout callout-warn">
<strong>No current figure is published for this crate.</strong> The four cases
below were measured against a five-phase pipeline that no longer exists — this
crate now makes a single left-to-right pass over a generated mora index — so
they describe code that is not running and none may be quoted. They stay
visible only so the next run has something to diff against. Refreshing them is
one command: <code>cargo bench -p verbora-transliterators</code>.
</div>

| Case | Cost † |
|---|---|
| Rejection path, Latin prose | under 100 ns |
| ~20 KB all-kana document | ~81 µs |
| 4 × ~23.5 KB documents, `par_transliterate_ja_batch` | ~4× the sequential loop |
| 32–256 × ~23.5 KB documents | 6–10× |

† Pending re-measurement, and left as recorded rather than replaced with a guess.

Benchmarks live in `crates/verbora-transliterators/benches/transliterators.rs`,
split into rejection cost, work cost, the buffered-vs-fresh-allocation
comparison, and the parallel batch group. See
[Benchmarks](../benchmarks/index.md).

## Allocation behaviour

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

| Call | Input with no kana | Input the scan changes |
|---|---|---|
| `transliterate_ja` | **Zero allocations**, `Cow::Borrowed`, after one byte scan | Exactly one `String`, allocated at the first rewrite |
| `transliterate_ja_into` | One `push_str` into `out`; `out` grows if it must | **None of its own** — it splices straight into `out` |
| `transliterate_ja_normalized` | **Zero**, borrowed through both halves | the width fold's allocations, then the scan's |
| `Rewrites::new` | **Zero** | **Zero** — the iterator yields offsets and `&'static str` |

Unmatched runs between rewrites are copied in bulk rather than character by
character, and the output buffer is allocated at the first rewrite rather than
up front, so a document that turns out to contain no kana after passing the gate
still allocates nothing.

See [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md) and
[Buffer reuse](../performance/buffer-reuse.md).

## Unicode and language notes

<div class="callout callout-note">
<strong>No UTF-16 semantics here.</strong> Unlike much of Verbora, every
decision in this crate is made on whole characters, so a surrogate pair can
never be split — the tests assert that on every output rather than assuming it.
</div>

**The sokuon classes have deliberate holes.** `ジ` is excluded from the `ッ` →
`z` class and gets its own `j`; `フ` is excluded from `ッ` → `h` and gets `f`.
That falls out of applying the rule to the romanization rather than to the kana,
and writing out "the whole ざ row" instead would break real words:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ざっし"), "zasshi");   // し is shi
    assert_eq!(transliterate_ja("ジャッジ"), "jajji");   // ジ is ji, not zi
    assert_eq!(transliterate_ja("バッファ"), "baffa");   // フ is fu, not hu
}
```

**Twelve scalar values in the Hiragana and Katakana blocks are deliberately not
romanized**, and a test pins the list exactly: U+3040, U+3097 and U+3098
(unassigned); U+3099 and U+309A (the combining voiced and semi-voiced sound
marks) and U+309B and U+309C (their spacing forms), which are diacritics rather
than morae and which `transliterate_ja_normalized` composes; U+309D, U+309E,
U+30FD and U+30FE (iteration marks); and U+30A0 `゠` KATAKANA-HIRAGANA DOUBLE
HYPHEN, which is punctuation.

**`・` KATAKANA MIDDLE DOT becomes an ASCII space.** It is the syllabary's one
non-kana entry and the one decision the standards do not cover:
`transliterate_ja("ボージョレー・ヌーヴォー")` is `"bōjorē nūvō"`.

**What passes through untouched.** Halfwidth katakana, kanji, iteration marks
(`々`), Latin text, digits, punctuation, every non-Japanese script and every
astral character — including the ideographic full stop `。` (U+3002), which is
inside the gated block but is not a mora.

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("時々刻々"), "時々刻々");
    assert_eq!(transliterate_ja("これは日本語のテストです。"), "koreha日本語notesutodesu。");
}
```

## Common mistakes

**Assuming `transliterate_ja_into` clears its buffer.** It appends:

```rust
use verbora_transliterators::transliterate_ja_into;
fn main() {
    let mut buf = String::from("already here: ");
    transliterate_ja_into("カナ", &mut buf);
    assert_eq!(buf, "already here: kana");   // not "kana"
}
```

**Feeding it halfwidth or decomposed kana.** `ｱｲｳｴｵ` comes back unchanged and
nothing warns you. Use `transliterate_ja_normalized`, or run
[`nfkc`](./normalizers.md) yourself first.

**Expecting a lone `ッ` or `ー` to survive.** Both are modifiers, and a modifier
with nothing to modify romanizes as the empty string — `transliterate_ja("ッ")`
is `""`, not `"t"`.

**Expecting `えい` or `いい` to carry a macron.** They do not: `せんせい` is
`sensei` and `おいしい` is `oishii`. The prolonged sound mark is the general
lengthener, and it is the one that does apply to `i`.

**Calling `.into_owned()` on every result.** On any document without kana that
allocates and copies a string that was already correct. Hold the `Cow`; it
derefs to `&str`.

**Expecting the romaji to be reversible, or phonetically correct.** `ā` has more
than one source, so there is no inverse; and `こんにちは` is `konnichiha`,
because nothing here knows that `は` is a particle pronounced `wa`.

## Related

- [Normalizers](./normalizers.md) — `nfkc`, which you almost always
  want in front of this
- [Tokenizers](./tokenizers.md) — what UAX #29 does with Japanese text
- [Zero-copy](../performance/zero-copy.md),
  [Allocation](../performance/allocation.md),
  [Buffer reuse](../performance/buffer-reuse.md) and
  [Iterator vs reusable buffer](../performance/iterator-vs-into.md)
- [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md)
- [Choosing an API](../choosing/index.md)

## API reference

The crate root is the entire public surface; the modules behind it are private.

```rust  ignore
// verbora_transliterators
pub fn transliterate_ja(text: &str) -> Cow<'_, str>;             // #[must_use]
pub fn transliterate_ja_into(text: &str, out: &mut String);      // APPENDS to out
pub fn transliterate_ja_normalized(text: &str) -> Cow<'_, str>;  // #[must_use]

#[cfg(feature = "parallel")]
pub fn par_transliterate_ja_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>; // #[must_use]

pub struct Rewrite<'a> {
    pub start: usize,
    pub end: usize,
    pub from: &'a str,
    pub to: &'static str,
}

pub struct Rewrites<'a> { /* private */ }
impl<'a> Rewrites<'a> {
    pub fn new(text: &'a str) -> Self;                           // #[must_use]
}

// Trait implementations
impl Debug + Clone + Copy + PartialEq + Eq for Rewrite<'_>;
impl Debug + Clone for Rewrites<'_>;
impl<'a> Iterator for Rewrites<'a>;   // Item = Rewrite<'a>; not declared FusedIterator
```

No errors, no panics, no configuration, no builder, no trait to implement, and
no batch or parallel API outside `par_transliterate_ja_batch`.
