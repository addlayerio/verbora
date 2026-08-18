# Transliterators

`verbora-transliterators` has exactly one entry point, `transliterate_ja`:
Japanese kana to modified-Hepburn romaji. `とうきょう` becomes `tōkyō`, `ザッシ`
becomes `zasshi`, `ほんや` becomes `hon'ya`. That is the whole subsystem — one
conversion, in one direction, for one language.

Modified Hepburn is deceptively intricate: thirty of its rules need lookahead
over neighbouring characters, its closing rule keys on an ASCII-only word
boundary, and its five transformation passes are order-dependent in ways that
change the output of ordinary words. This crate implements all of it as five
ordered phases over generated tables, with no regex engine. Text with no kana
in it is returned borrowed after a single vectorised byte scan.

## When to use it

- **You need a Latin search key for Japanese text**, and you produce both sides
  of the comparison with this same function.
- **You want kana romanised inside otherwise mixed text.** Kanji, Latin,
  punctuation and halfwidth katakana pass through untouched, so running this
  over a mixed-language corpus is safe and — for documents with no kana —
  nearly free.
- **You want the phases individually**, to debug a surprising result or build
  on one table. See [Phases and introspection](#phases-and-introspection).
- **You have many independent, document-scale inputs.** See
  [`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch).

## When not to use it

- **You want romaji back into kana.** There is no reverse function, and the
  mapping is not injective: `ā` comes from `aぁ`, `aァ` and `aー` alike.
- **You want linguistically correct romanisation.** This is character-by-character
  modified Hepburn with no lexical knowledge. The topic particle `は` is
  romanised `ha`, so `こんにちは` becomes `konnichiha`, not `konnichiwa`.
- **You want kanji read aloud.** Kanji is not touched at all —
  `これは日本語のテストです。` becomes `koreha日本語notesutodesu。`.
- **Your input is halfwidth katakana.** No table key is a halfwidth character,
  so `ｱｲｳｴｵ` comes back unchanged. Use
  [`ja::transliterate_normalized`](#ja-transliterate-normalized), or normalize
  first with [`normalize_ja`](./normalizers.md).
- **You want iteration marks expanded.** `々` passes through unchanged;
  `normalize_ja` expands them.
- **You want tokens.** This is a character-level rewriter. Use
  [`TokenizerJa`](./tokenizers.md).

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

## The pipeline

Five phases, strictly ordered. Each runs over the *whole* string before the
next begins, and several of them depend on that.

| `Phase` | What it does | Table size |
|---|---|--:|
| `CompoundKana` | the u/vu digraphs: `ウァ` → `wa`, `ヴュ` → `vyu`, `ウー` → `ū` | 20 keys |
| `SokuonAndN` | the geminate consonant, and `ン` before a labial or a vowel | 4 sources, 148 pairs |
| `Kana` | the main kana table: 191 katakana entries and their 191 hiragana mirrors | 382 keys |
| `LongVowels` | long vowels and the small-vowel fallback, over the now-mixed Latin+kana text | 21 keys |
| `FinalSokuon` | whatever small tsu is left, at an ASCII-only word boundary | 2 characters |

The order is load-bearing, not stylistic:

- `ハイジャッンプ` is `haijanmpu` **only** because `ッ` before `ン` is rewritten
  before `ン` before a labial gets a look at the same `ン`. Swap those and it
  comes out `haijammpu`.
- `LongVowels` must run after `Kana`, because eleven of its twenty-one keys
  begin with a **Latin** vowel — the one `Kana` just produced:

```rust
use verbora_transliterators::ja::Phase;
use verbora_transliterators::transliterate_ja;

fn main() {
    // Phase 4 on raw kana finds nothing: its key is 'a' + 'ー', not 'カ' + 'ー'.
    assert_eq!(Phase::LongVowels.apply("カー"), "カー");
    // Phase 3 produces the Latin vowel phase 4 keys on.
    assert_eq!(Phase::Kana.apply("カー"), "kaー");
    // In order, the two compose.
    assert_eq!(transliterate_ja("カー"), "kā");
}
```

The tables are generated, not hand-transcribed, and the generator re-proves the
model's equivalence over 160,401 inputs before emitting anything.

## Choosing the right API

One conversion, five ways to ask for it. The genuine decision is between
`transliterate_ja` (hands you a `Cow`) and `transliterate_into` (writes into a
buffer you own); the rest is whether you want a single phase instead of the
pipeline, and whether you want the replacements without the string.

| API | Best for | Lazy | Output | Clears `out` | Allocations |
|---|---|:--:|---|:--:|---|
| `transliterate_ja(s)` | one string, simplest call | ❌ | `Cow<'_, str>` | n/a | none when nothing changed; else one `String` per phase that rewrites (0–5) |
| [`ja::par_transliterate_ja_batch(inputs)`](#ja-par-transliterate-ja-batch) | many independent, document-scale strings | ❌ | `Vec<Cow<'_, str>>` | n/a | the same 0–5 per input, plus one output `Vec`; feature `parallel` |
| [`ja::transliterate_into(s, &mut out)`](#ja-transliterate-into) | concatenating many results into one buffer | ❌ | `()`, appends | ❌ **appends** | the same 0–5, plus `out`'s growth |
| [`ja::transliterate_normalized(s)`](#ja-transliterate-normalized) | halfwidth katakana, fullwidth Latin, `々` | ❌ | `Cow<'_, str>` | n/a | `normalize_ja`'s, then the above |
| `Phase::apply(s)` | one stage of the pipeline | ❌ | `Cow<'_, str>` | n/a | none when that phase matched nothing; else one `String` |
| `Phase::apply_into(s, &mut out)` | one stage, straight into your buffer | ❌ | `()`, appends | ❌ **appends** | none of its own |
| `Phase::rewrites(s)` | inspecting *what* would change | ✅ | `Rewrites<'_>` → `Rewrite<'_>` | n/a | **none, ever** |

Two columns deserve a second look. **Both `_into` functions append** — Verbora
has two clearing conventions and this crate uses the appending one (see
[Buffer reuse](../performance/buffer-reuse.md)); `out.clear()` is yours to
call, and it is safe, since `clear()` never frees capacity. **Only
`Phase::rewrites` is lazy** — `transliterate_ja` is eager across the five
phases, though each phase is internally built on that same lazy iterator and
allocates nothing until a replacement is actually found.

### `transliterate_ja`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

```rust  ignore
pub fn transliterate(text: &str) -> Cow<'_, str>   // re-exported as transliterate_ja
```

The default. It returns `Cow::Borrowed` for Latin text, kanji, halfwidth
katakana, emoji and every other script — the common case in mixed corpora, and
one `memchr`-class scan for the lead byte `0xE3`:

```rust
use std::borrow::Cow;

use verbora_transliterators::transliterate_ja;

fn main() {
    // Nothing in U+3000..U+3FFF: one vectorised scan, zero allocations.
    assert!(matches!(transliterate_ja("the quick brown fox"), Cow::Borrowed(_)));
    assert!(matches!(transliterate_ja("ｱｲｳｴｵ"), Cow::Borrowed(_)));

    // Kana: owned, and exactly the phases that fired paid for it.
    assert!(matches!(transliterate_ja("カタカナ"), Cow::Owned(_)));
}
```

Hold the `Cow` as long as you can — it derefs to `&str` — and call
`.into_owned()` only at a boundary that genuinely needs a `String`.

<div class="callout callout-note">
<strong>Note.</strong> The gate is a <em>superset</em> test: it admits any
string containing the UTF-8 lead byte <code>0xE3</code>, which covers all of
U+3000..U+3FFF. <code>々</code> U+3005 passes the gate and then matches
nothing, so <code>transliterate_ja("々")</code> still returns
<code>Cow::Borrowed</code> — just after five scans instead of one.
</div>

### `ja::par_transliterate_ja_batch`

```rust  ignore
#[cfg(feature = "parallel")]
pub fn par_transliterate_ja_batch(inputs: &[&str]) -> Vec<Cow<'_, str>>
```

`transliterate` is a pure function of one `&str` with no shared state, so many
independent documents are embarrassingly parallel. This function, behind the
`parallel` Cargo feature, is exactly
`inputs.par_iter().map(transliterate).collect()` — a thin fan-out over the
sequential primitive, not a second implementation. Output order matches input
order, and the `Vec<Cow<'_, str>>` keeps every borrowed document borrowed.

```rust  ignore
use verbora_transliterators::ja::par_transliterate_ja_batch;

fn main() {
    let inputs = ["あいうえお", "ざっし", "plain ascii"];
    let got = par_transliterate_ja_batch(&inputs);
    assert_eq!(got, ["aiueo", "zasshi", "plain ascii"]);
}
```

| Workload | Use |
|---|---|
| A handful of short (non-document-scale) inputs | `inputs.iter().map(transliterate).collect()` |
| 4 document-scale (~23.5 KB) inputs | this function — already ~4× faster |
| 32–256 document-scale inputs | this function — 6–10× faster |

This is the crate's only built-in parallel API. For a different shape (a shared
output buffer built with `transliterate_into`, say), apply the same
`par_iter().map(...)` at your own call site — every item here is a free
function or a `Copy` enum over `&'static` tables, with no state, no interior
mutability and no globals. Note that collecting `String`s across threads forces
`.into_owned()`, reintroducing an allocation for documents that would otherwise
have been borrowed. See [Parallelism](../performance/parallelism.md).

### `ja::transliterate_into`

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

```rust  ignore
pub fn transliterate_into(text: &str, out: &mut String)   // APPENDS
```

**It appends. It does not clear.** That is the contract, and it is what makes
the accumulate pattern need no special API:

```rust
use verbora_transliterators::transliterate_into;

fn main() {
    let mut doc = String::new();
    for word in ["こんにちは", " ", "せかい"] {
        transliterate_into(word, &mut doc);   // no clear, ever
    }
    assert_eq!(doc, "konnichiha sekai");

    // Non-kana fragments are pushed through verbatim, so separators and
    // punctuation can go through the same call.
    let mut mixed = String::from("[");
    transliterate_into("カナ", &mut mixed);
    transliterate_into("]", &mut mixed);
    assert_eq!(mixed, "[kana]");
}
```

For one result per iteration rather than one accumulated document, the usual
[buffer-reuse](../performance/buffer-reuse.md) ritual applies unchanged:
`String::with_capacity` once outside the loop, `buf.clear()` at the top of each
iteration, then `transliterate_into(word, &mut buf)`.

<div class="callout callout-note">
<strong>The move optimisation.</strong> When <code>out</code> has
<em>never allocated</em> — a fresh <code>String::new()</code> —
<code>transliterate_into</code> moves the pipeline's own buffer in instead of
copying. Reserve capacity up front and that guard is false from the very first
call, so every call copies with <code>push_str</code> and your reservation
survives. Either way the buffer's capacity is stable across every later call.
</div>

`transliterate_into` does **not** save the pipeline's own allocations. It wraps
`transliterate`, so a string that five phases rewrite still allocates up to
five intermediates inside the call; only the last reaches `out`. The phases
cannot be fused, because each needs the previous phase's *entire* output. What
it removes is the caller's side: no `Vec<String>` of fragments, no second
concatenation pass. See
[Iterator vs reusable buffer](../performance/iterator-vs-into.md).

### `ja::transliterate_normalized`

```rust  ignore
pub fn transliterate_normalized(text: &str) -> Cow<'_, str>
```

`transliterate_ja` alone has no halfwidth-katakana keys, so real-world Japanese
input needs normalizing first. This composes `normalize_ja` and the pipeline
for you:

```rust
use verbora_transliterators::ja::transliterate_normalized;
use verbora_transliterators::transliterate_ja;

fn main() {
    // Halfwidth katakana is invisible to every table …
    assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
    // … until it is widened first.
    assert_eq!(transliterate_normalized("ｱｲｳｴｵ"), "aiueo");

    // Composed voiced marks come along too: ｶ + ﾞ → ガ → ga.
    assert_eq!(transliterate_normalized("ｶﾞｯｷ"), "gakki");
}
```

**Reach for it by default on input you did not produce yourself**, and for
`transliterate_ja` when you know the text is already fullwidth — or when you
need byte-exact agreement with a call site that did *not* normalise. For every
input on which `normalize_ja` is the identity, the composition is asserted to
agree exactly with the bare transliteration; it never changes an answer
`transliterate_ja` already gives. There is no `_into` variant.

### `Phase::apply` and `Phase::apply_into`

```rust
use verbora_transliterators::ja::Phase;

fn main() {
    // One phase, borrowing when it changed nothing.
    assert_eq!(Phase::Kana.apply("カタカナ"), "katakana");
    assert_eq!(Phase::Kana.apply("hello"), "hello");
    assert_eq!(Phase::CompoundKana.apply("ヴァイオリン"), "vaイオリン");

    // apply_into appends, and allocates nothing of its own.
    let mut out = String::new();
    Phase::Kana.apply_into("カナ", &mut out);
    Phase::Kana.apply_into("カナ", &mut out);
    assert_eq!(out, "kanakana");
}
```

`Phase::apply_into` is the only genuinely allocation-free writer here: it
splices rewrites directly into your buffer, and never moves, so a caller buffer
keeps its capacity across every call. `transliterate_into` cannot do that,
because the pipeline needs each phase's whole output before the next starts.

<div class="callout callout-warn">
<strong>Careful.</strong> Running the phases yourself is only equivalent to
<code>transliterate_ja</code> if you run <strong>all five, in
<code>Phase::ALL</code> order</strong>. Skipping one or reordering two changes
ordinary words: see <a href="#the-pipeline">The pipeline</a>. Note also that
<code>transliterate_ja</code> skips all five outright when the input has no
<code>0xE3</code> byte, which a hand-rolled loop does not.
</div>

## Phases and introspection

`Phase`, `Rewrite` and `Rewrites` are the crate's whole type surface, and a
normal caller needs none of them. `Phase::rewrites` is the single description of
every phase's behaviour — `Phase::apply`, `transliterate_ja` and
`transliterate_into` are all built on it, so there is no second copy to drift.
Per-phase test suites mean a failure says *which* of the five broke, and if a
result surprises you the rewrites tell you exactly which rule fired, over which
bytes.

```rust  ignore
pub struct Rewrite<'a> {
    pub start: usize,        // byte offset where the replaced text begins
    pub end: usize,          // byte offset one past the end
    pub from: &'a str,       // the slice being replaced — &text[start..end]
    pub to: &'static str,    // the text written in its place
}
```

Byte offsets, not character indices, because that is what splicing needs. `to`
is `&'static str` because every replacement comes from a static table — which
is why yielding rewrites allocates nothing at all.

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

```rust
use verbora_transliterators::ja::Phase;

fn main() {
    // What the kana table would do, without doing it.
    let hits: Vec<_> = Phase::Kana.rewrites("カナ").collect();
    assert_eq!((hits[0].start, hits[0].end, hits[0].from, hits[0].to), (0, 3, "カ", "ka"));

    // The lookahead phase consumes ONLY its source character: the character
    // that triggered the rule stays in the output and is re-examined next step.
    let hits: Vec<_> = Phase::SokuonAndN.rewrites("ッカ").collect();
    assert_eq!((hits[0].from, hits[0].to), ("ッ", "k"));

    // Counting matches allocates nothing — no output string is ever built,
    // and the whole-input gate makes the iterator empty without scanning.
    assert_eq!(Phase::Kana.rewrites("カタカナ").count(), 4);
    assert_eq!(Phase::Kana.rewrites("abc").size_hint(), (0, Some(0)));
}
```

`size_hint` is `(0, Some(remaining bytes))`: every match consumes at least one
byte, and nothing is guaranteed to match. `Rewrites` is `Clone` and `Debug`,
and is **not** declared `FusedIterator`, though it does keep returning `None`
once the scan is finished. `Phase` is a plain `Copy` enum with `Debug`,
`PartialEq`, `Eq`, `Hash`, `PartialOrd` and `Ord`, plus `Phase::ALL` in
pipeline order.

## Performance characteristics

Every phase is **O(n) in the input length** with a small constant. No
backtracking, no regex engine, no automaton, no runtime construction step. The
crate's only dependency is `verbora-normalizers`, and only for
`transliterate_normalized`.

| Stage | Cost per character when nothing matches |
|---|---|
| Whole input, once | One vectorised `slice::contains(&0xE3)`. A document with no such byte skips all five phases |
| Table phases (1, 3, 4) | One shift and one bitmap test against `codepoint >> 8`, then — only for a character that genuinely begins a key — a second bitmap test on the low byte. All keys are BMP, so astral characters are rejected by the first test |
| Lookahead phase (2) | One binary search over four source characters |
| Final sokuon (5) | One `contains` over two characters, then one byte test for the ASCII word class |

The two-level gate is why the table phases are cheap on realistic text: for the
compound table, four characters out of 191 reach the key tables at all. Where a
character *does* begin a key, "longest match here" is decided by the current
character and at most the next two, since no key is longer than three `char`s.

Measured with `cargo bench -p verbora-transliterators`:

| Case | Cost |
|---|---|
| Rejection path, Latin prose | under 100 ns |
| ~20 KB all-kana document | ~81 µs |
| 4 × ~23.5 KB documents, `par_transliterate_ja_batch` | ~4× the sequential loop |
| 32–256 × ~23.5 KB documents | 6–10× |

Benchmarks live in `crates/verbora-transliterators/benches/transliterators.rs`,
split into rejection cost, work cost, per-phase cost, the
buffered-vs-fresh-allocation comparison, and the parallel batch group. See
[Benchmarks](../benchmarks/index.md).

## Allocation behaviour

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

| Call | Input with no kana | Input the pipeline changes |
|---|---|---|
| `transliterate_ja` | **Zero allocations**, `Cow::Borrowed`, after one byte scan | One `String` per phase that rewrites something — at most five, typically one or two. Each is `String::with_capacity(that phase's input length)` |
| `ja::transliterate_into` | One `push_str` into `out`; `out` grows if it must | The above, then a move into `out` only on its very first call from a never-allocated `String::new()`; every other call copies with `push_str` |
| `ja::transliterate_normalized` | **Zero**, borrowed through both halves | `normalize_ja`'s allocations, then the pipeline's |
| `Phase::apply` | **Zero**, `Cow::Borrowed` | Exactly one `String` |
| `Phase::apply_into` | One `push_str` into `out` | **None of its own** |
| `Phase::rewrites` | **Zero** | **Zero** — the iterator yields offsets and `&'static str` |

**No phase buffer ever regrows.** Every replacement in every table is no longer
in bytes than the text it replaces — three-byte kana become one to three ASCII
bytes, `キョウ` (nine bytes) becomes `kyō` (four) — so
`String::with_capacity(text.len())` is always enough.

**The pipeline's intermediates are unavoidable.** Five phases, each needing the
previous phase's complete output, means a fully-rewritten string can pay for up
to five buffers. A phase that changes nothing hands the previous buffer forward
untouched rather than copying it, so the count is "phases that fired", not
"phases".

See [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md) and
[Buffer reuse](../performance/buffer-reuse.md).

## Unicode and language notes

<div class="callout callout-note">
<strong>No UTF-16 semantics here.</strong> Unlike much of Verbora, every
pattern in this crate matches whole characters and the only zero-width
assertion is a word boundary, so a surrogate pair can never be split — the
tests assert that on every output rather than assuming it.
</div>

**The tables have deliberate holes.** `ジ` is excluded from the `ッ` → `z`
class and gets its own `j` rule; `フ` is excluded from `ッ` → `h` and gets `f`.
Writing out "the whole ざ row" would break real words:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ざっし"), "zasshi");   // し is in the s class
    assert_eq!(transliterate_ja("ジャッジ"), "jajji");   // ジ is NOT in the z class
    assert_eq!(transliterate_ja("バッファ"), "baffa");   // フ is NOT in the h class
}
```

**`ン` has two faces.** Before a labial (the `ば`, `ぱ` and `ま` rows) it is
`m` (`かんぱい` → `kampai`, `しんぶん` → `shimbun`); before a vowel or a
`y`-row kana it is `n'` (`ほんや` → `hon'ya`); everywhere else the kana table's
plain `n` applies (`まんと` → `manto`).

**The final small-tsu pass uses an ASCII-only word boundary.** `\w` is
`[A-Za-z0-9_]` with no Unicode semantics, so the boundary holds exactly when
the next code unit is absent or is itself non-word:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ッ漢"), "t漢");   // boundary: 漢 is a non-word character
    assert_eq!(transliterate_ja("ッ"), "t");
    assert_eq!(transliterate_ja("ッ."), "t.");
    assert_eq!(transliterate_ja("ッA"), "ッA");   // no boundary: the tsu SURVIVES
    assert_eq!(transliterate_ja("ッ1"), "ッ1");
}
```

**The long-vowel table is lowercase-only.** Only lowercase `a i u e o`
participate, so `aー` becomes `ā` while `Aー` is left exactly as it is —
case-folding the input first would silently produce `Ā`.

**`・` KATAKANA MIDDLE DOT becomes an ASCII space.** It is the module's one
non-kana key, and only the katakana half of the table has it:
`transliterate_ja("ボージョレー・ヌーヴォー")` is `"bōjorē nūvō"`.

**What passes through untouched.** Halfwidth katakana, kanji, iteration marks
(`々`), Latin text, digits, punctuation, every non-Japanese script and every
astral character. A bare `ー` with no vowel in front of it is left alone too,
and so is the ideographic full stop `。` (U+3002) — inside the gated block, but
no table has a key for it.

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("時々刻々"), "時々刻々");
    assert_eq!(transliterate_ja("これは日本語のテストです。"), "koreha日本語notesutodesu。");
}
```

## Common mistakes

**Assuming `transliterate_into` clears its buffer.** It appends:

```rust
use verbora_transliterators::transliterate_into;
fn main() {
    let mut buf = String::from("already here: ");
    transliterate_into("カナ", &mut buf);
    assert_eq!(buf, "already here: kana");   // not "kana"
}
```

**Feeding it halfwidth katakana.** `ｱｲｳｴｵ` comes back unchanged and nothing
warns you. Use `ja::transliterate_normalized`, or run
[`normalize_ja`](./normalizers.md) yourself first.

**Expecting `ッ` before a Latin letter to become `t`.** `ッA` stays `ッA`
because the ASCII-only word-boundary rule finds no boundary before an ASCII
word character. It looks like a bug; it is intentional.

**Running the phases yourself in the wrong order.** `Phase::ALL` is the order,
and reordering it is observable on ordinary words — `カァ` is `kā` in order and
`kaa` with `LongVowels` moved before `Kana`; `ハイジャッンプ` is `haijanmpu` in
order and `haijaッnpu` with `Kana` moved before `SokuonAndN`.

**Calling `.into_owned()` on every result.** On any document without kana that
allocates and copies a string that was already correct. Hold the `Cow`; it
derefs to `&str`.

**Expecting the romaji to be reversible, or phonetically correct.** `ā` has
three possible sources, so there is no inverse; and `こんにちは` is
`konnichiha`, because nothing here knows that `は` is a particle pronounced
`wa`.

## Related

- [Normalizers](./normalizers.md) — `normalize_ja`, which you almost always
  want in front of this
- [Tokenizers](./tokenizers.md) — `TokenizerJa`, for splitting Japanese text
- [Zero-copy](../performance/zero-copy.md),
  [Allocation](../performance/allocation.md),
  [Buffer reuse](../performance/buffer-reuse.md) and
  [Iterator vs reusable buffer](../performance/iterator-vs-into.md)
- [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md)
- [Choosing an API](../choosing/index.md)

## API reference

```rust  ignore
// verbora_transliterators — the crate root re-exports
pub use ja::par_transliterate_ja_batch;                        // requires feature = "parallel"
pub use ja::{Phase, Rewrite, Rewrites, transliterate as transliterate_ja, transliterate_into};

// verbora_transliterators::ja
pub fn transliterate(text: &str) -> Cow<'_, str>;              // #[must_use]
#[cfg(feature = "parallel")]
pub fn par_transliterate_ja_batch(inputs: &[&str]) -> Vec<Cow<'_, str>>; // #[must_use]
pub fn transliterate_into(text: &str, out: &mut String);       // APPENDS to out
pub fn transliterate_normalized(text: &str) -> Cow<'_, str>;   // #[must_use]; not re-exported at the root

pub enum Phase {
    CompoundKana,
    SokuonAndN,
    Kana,
    LongVowels,
    FinalSokuon,
}

impl Phase {
    pub const ALL: [Self; 5];                                  // pipeline order
    pub fn rewrites(self, text: &str) -> Rewrites<'_>;         // #[must_use]; lazy
    pub fn apply(self, text: &str) -> Cow<'_, str>;            // #[must_use]
    pub fn apply_into(self, text: &str, out: &mut String);     // APPENDS to out
}

pub struct Rewrite<'a> {
    pub start: usize,
    pub end: usize,
    pub from: &'a str,
    pub to: &'static str,
}

pub struct Rewrites<'a> { /* private */ }

// Trait implementations
impl Debug + Clone + Copy + PartialEq + Eq + Hash + PartialOrd + Ord for Phase;
impl Debug + Clone + Copy + PartialEq + Eq for Rewrite<'_>;
impl Debug + Clone for Rewrites<'_>;
impl<'a> Iterator for Rewrites<'a>;   // Item = Rewrite<'a>; not declared FusedIterator
```

No errors, no panics, no configuration, no builder, no trait to implement, and
no batch or parallel API outside `ja::par_transliterate_ja_batch`.
`transliterate_normalized` lives at
`verbora_transliterators::ja::transliterate_normalized` only — it is the one
public item the crate root does not re-export.
