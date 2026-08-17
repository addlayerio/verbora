# Transliterators

`verbora-transliterators` is tested against the reference transliterators,
which has exactly one export: `TransliterateJa`, Japanese kana to modified-Hepburn
romaji. `とうきょう` becomes `tōkyō`, `ザッシ` becomes `zasshi`, `ほんや` becomes
`hon'ya`. That is the whole subsystem — one conversion, in one direction, for one
language.

The reference is 584 lines of table plus twelve lines of pipeline, and the twelve
lines are the hard part: thirty of its rules use regex lookahead, its closing rule
keys on the reference's ASCII-only `\B`, and its five passes are order-dependent in
ways that change the output of ordinary words. This port reproduces all of it
without a regex engine at all, as five ordered phases over generated tables. Text
with no kana in it is returned borrowed after a single vectorised byte scan.

<div class="callout callout-spec">
<strong>Specification status.</strong> <code>TransliterateJa</code> and each of
its five pipeline phases are documented and test-pinned, including the ordered
kana pairs and every input the API rejects. One behaviour is a direct
consequence of the type system rather than a choice:
<a href="#divergence-this-cannot-throw">this cannot fail</a>.
<code>cargo test -p verbora-transliterators</code> runs <strong>24</strong>
unit tests and <strong>6</strong> doctests.
</div>

## When to use it

- **You are porting the reference that called the reference transliterator.** The
  results are byte-identical, quirks included.
- **You need a Latin search key for Japanese text** and you are producing both
  sides of the comparison with this same function.
- **You want kana romanised inside otherwise mixed text.** Kanji, Latin,
  punctuation and halfwidth katakana pass through untouched, so running this over
  a mixed-language corpus is safe and — for documents with no kana at all —
  nearly free.
- **You want the phases individually**, for debugging a surprising result or for
  building something on top of one table. See
  [Phases, rewrites and introspection](#phases-rewrites-and-introspection).
- **You have many independent, document-scale inputs to transliterate**, and
  the batch is large enough to be worth a thread pool. See
  [`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch).

## When not to use it

- **You want romaji back into kana.** There is no reverse function, and the
  mapping is not injective: `ā` comes from `aぁ`, `aァ` and `aー` alike.
- **You want linguistically correct romanisation.** This is the reference's
  modified Hepburn, applied character by character with no lexical knowledge.
  The topic particle `は` is romanised `ha`, so `こんにちは` becomes
  `konnichiha`, not `konnichiwa`. Nothing here knows what a particle is.
- **You want kanji read aloud.** Kanji is not touched at all — `これは日本語のテストです。`
  becomes `koreha日本語notesutodesu。`. Reading kanji needs a morphological
  analyser, which Verbora does not have; see [Roadmap](./roadmap.md).
- **Your input is halfwidth katakana.** No table key is a halfwidth character, so
  `ｱｲｳｴｵ` comes back unchanged. Use
  [`ja::transliterate_normalized`](#ja-transliterate-normalized) or normalize
  first with [`normalize_ja`](./normalizers.md).
- **You want iteration marks expanded.** `々` passes through; the reference's own
  header lists it as a `@todo`. `normalize_ja` expands them, which is another
  reason to run it first.
- **You want tokens.** This is a character-level rewriter, not a tokenizer. Use
  [`TokenizerJa`](./tokenizers.md) for splitting.

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

Five phases, strictly ordered. Each runs over the *whole* string before the next
begins, and several of them depend on that.

| [`Phase`](#phases-rewrites-and-introspection) | Reference | What it does | Table size |
|---|---|---|--:|
| `Phase::CompoundKana` | `replace1(str)` | the u/vu digraphs: `ウァ` → `wa`, `ヴュ` → `vyu`, `ウー` → `ū` | 20 keys |
| `Phase::SokuonAndN` | 30 chained `.replace(/X(?=[…])/g, …)` | the geminate consonant, and `ン` before a labial or a vowel | 4 sources, 148 pairs |
| `Phase::Kana` | `replace2(str)` | the main kana table: 191 katakana entries and their 191 hiragana mirrors | 382 keys |
| `Phase::LongVowels` | `replace3(str)` | long vowels and the small-vowel fallback, over the now-mixed Latin+kana text | 21 keys |
| `Phase::FinalSokuon` | `.replace(/(ッ\|っ)\B/g, 't')` | whatever small tsu is left | 2 characters |

The order is load-bearing, not stylistic:

- `ハイジャッンプ` is `haijanmpu` **only** because `ッ` before `ン` is rewritten
  before `ン` before a labial gets a look at the same `ン`. Swap those and the word
  comes out `haijammpu`.
- `Phase::LongVowels` has to run after `Phase::Kana`, because eleven of its
  twenty-one keys begin with a **Latin** vowel — the one `Phase::Kana` just
  produced. `カー` is `ka` + `ー` before it is `kā`:

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

None of the tables are transcribed by hand. The derivation
loads the reference module, dumps what it actually built, parses the 30 lookahead
rules out of the source text, and re-proves three properties before emitting
anything: the prefix invariant that makes leftmost-longest matching equal to
the reference's leftmost-first alternation, the fusion of the 30 passes into one
scan, and the equivalence of the whole model to `TransliterateJa` over 160,401
inputs.

## Choosing the right API

There is one conversion here, and five ways to ask for it. The genuine decision is
between `transliterate_ja` (hands you a `Cow`) and `transliterate_into` (writes
into a buffer you own); the rest is whether you want a single phase instead of the
pipeline, and whether you want the replacements without the string.

### Comparison table

| API | Best for | Lazy | Output | Clears `out` | Allocations |
|---|---|:--:|---|:--:|---|
| `transliterate_ja(s)` | one string, simplest call | ❌ | `Cow<'_, str>` | n/a | none when nothing changed; else one `String` per phase that rewrites (0–5) |
| `ja::par_transliterate_ja_batch(inputs)` | many independent, document-scale strings at once | ❌ | `Vec<Cow<'_, str>>` | n/a | the same 0–5 per input, plus one output `Vec`; requires the `parallel` feature |
| `ja::transliterate_into(s, &mut out)` | concatenating many results into one buffer | ❌ | `()`, appends to `&mut String` | ❌ **appends** | the same 0–5, plus `out`'s growth; see [the move optimisation](#the-move-optimisation-and-why-it-does-not-bite-the-reuse-ritual) |
| `ja::transliterate_normalized(s)` | halfwidth katakana, fullwidth Latin, `々` | ❌ | `Cow<'_, str>` | n/a | `normalize_ja`'s, then the above |
| `Phase::apply(s)` | one stage of the pipeline | ❌ | `Cow<'_, str>` | n/a | none when that phase matched nothing; else one `String` |
| `Phase::apply_into(s, &mut out)` | one stage, straight into your buffer | ❌ | `()`, appends to `&mut String` | ❌ **appends** | none of its own — rewrites are written directly into `out` |
| `Phase::rewrites(s)` | inspecting *what* would change | ✅ | `Rewrites<'_>` → `Rewrite<'_>` | n/a | **none, ever** |

Two columns deserve a second look.

**"Clears `out`" is ❌ for both `_into` functions.** They append. Verbora has two
clearing conventions and this crate uses the appending one; see
[Buffer reuse](../performance/buffer-reuse.md). If you want one buffer's worth of
output per call, `out.clear()` is yours to write — and it is safe: `clear()`
never frees a `String`'s capacity, and `transliterate_into` preserves it too. See
[the move optimisation](#the-move-optimisation-and-why-it-does-not-bite-the-reuse-ritual).

**Only `Phase::rewrites` is lazy.** `transliterate_ja` is eager across the five
phases, though each phase is *internally* built on the lazy iterator and allocates
nothing until a replacement is actually found.

### Decision tree

```text
I have Japanese text and I want romaji
│
├── The input might be halfwidth katakana, fullwidth Latin, or contain 々
│      └── ja::transliterate_normalized()      (normalize_ja, then the pipeline)
│
├── One string, and I want the result back
│      └── transliterate_ja()                  → Cow<'_, str>
│
├── Many independent, document-scale strings, fanned out across cores
│      └── ja::par_transliterate_ja_batch()    → Vec<Cow<'_, str>> (parallel feature)
│
├── Many strings, all going into one document buffer
│      └── ja::transliterate_into()            (appends — you clear, if you want to)
│
├── I want exactly one stage of the pipeline
│      ├── … and the result back
│      │      └── Phase::apply()               → Cow<'_, str>
│      └── … written into my buffer
│             └── Phase::apply_into()          (appends; no allocation of its own)
│
└── I want to know WHAT would change, not the changed text
       └── Phase::rewrites()                   → lazy Rewrite { start, end, from, to }
```

### `transliterate_ja`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager across five phases; each phase is lazy inside</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'_, str&gt;</code> — borrowed when no phase changed anything</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None when the input holds no U+3000..U+3FFF character; otherwise one <code>String</code> per phase that actually rewrites something (0–5), each <code>with_capacity(len)</code> and never regrown</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Internal only — the owned buffer is carried through no-op phases</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">One string at a time, and any corpus where most documents have no kana</span></div>
</div>

```rust  ignore
pub fn transliterate(text: &str) -> Cow<'_, str>   // re-exported as transliterate_ja
```

This is the default. It returns `Cow::Borrowed` for Latin text, kanji, halfwidth
katakana, emoji and every other script, which is the common case in mixed
corpora and costs one `memchr`-class scan for the lead byte `0xE3`:

```rust
use std::borrow::Cow;

use verbora_transliterators::transliterate_ja;

fn main() {
    // Nothing in U+3000..U+3FFF: one vectorised scan, zero allocations.
    assert!(matches!(transliterate_ja("the quick brown fox"), Cow::Borrowed(_)));
    assert!(matches!(transliterate_ja("Москва"), Cow::Borrowed(_)));
    assert!(matches!(transliterate_ja("ｱｲｳｴｵ"), Cow::Borrowed(_)));

    // Kana: owned, and exactly the phases that fired paid for it.
    assert!(matches!(transliterate_ja("カタカナ"), Cow::Owned(_)));
}
```

Hold the `Cow` as long as you can — it derefs to `&str`, so reading it needs no
decision — and call `.into_owned()` only at a boundary that genuinely needs a
`String`. On the borrowed path `.into_owned()` allocates and copies a string that
was already correct.

<div class="callout callout-note">
<strong>Note.</strong> The gate is a <em>superset</em> test: it admits any string
containing the UTF-8 lead byte <code>0xE3</code>, which covers all of
U+3000..U+3FFF. <code>々</code> U+3005 passes the gate and then matches nothing,
so <code>transliterate_ja("々")</code> still returns <code>Cow::Borrowed</code> —
just after five scans instead of one.
</div>

### `ja::par_transliterate_ja_batch`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>inputs.par_iter().map(transliterate).collect()</code> over Rayon's global thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;Cow&lt;'_, str&gt;&gt;</code>, input order preserved</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec&lt;Cow&lt;str&gt;&gt;</code> sized to <code>inputs.len()</code>, plus whatever <code>transliterate</code> itself allocates per input — nothing for a document the gate rejects, or that no phase ends up changing</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Yes</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — behind the <code>parallel</code> Cargo feature</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Many document-scale (multi-kilobyte) inputs; a handful of short inputs is faster with a plain sequential loop</span></div>
</div>

```rust  ignore
pub fn par_transliterate_ja_batch(inputs: &[&str]) -> Vec<Cow<'_, str>>
```

[`transliterate`] is a pure function of one `&str` with no shared state — the
whole-input kana gate and the five-phase pipeline both read only their
argument — so transliterating many independent documents is embarrassingly
parallel with zero coordination cost between them. This function, behind the
crate's `parallel` Cargo feature, is exactly `inputs.par_iter().map(transliterate).collect()`
— a thin fan-out over the existing sequential primitive, not a second
implementation of it. The gate, the five ordered phases and the
`Cow`-borrowing behaviour inside `transliterate` are all untouched; if you
need a different shape in parallel (a shared output buffer built with
`transliterate_into`, for instance), apply the same `par_iter().map(...)`
pattern at your own call site — see
[Parallelism](../performance/parallelism.md).

`transliterate` is already fast per call — this crate's own `transliterate`
benchmark measures roughly 81 µs for a ~20 KB all-kana document, and under
100 ns for the rejection path on Latin text, while a `rayon` task costs on
the order of a microsecond to schedule. This crate's own
`par_transliterate_ja_batch` Criterion group measures the consequence
directly, at a fixed ~23.5 KB document per call: even a 4-document batch is
already **~4x faster** in parallel, and 32- and 256-document batches are
**6–10x faster**. A plain `inputs.iter().map(transliterate).collect()` loop
still wins for a handful of short (non-document-scale) inputs; reach for
this once inputs are document-scale (multi-kilobyte) and there is more than
a handful of them, and measure your own workload rather than assuming the
win.

Output order matches input order — `results[i]` is `transliterate(inputs[i])`
— via `rayon`'s order-preserving `map` + `collect`. `transliterate` never
errors, so there is no error shape to preserve.

```rust  ignore
use verbora_transliterators::ja::par_transliterate_ja_batch;

fn main() {
    let inputs = ["あいうえお", "ざっし", "plain ascii"];
    let got = par_transliterate_ja_batch(&inputs);
    assert_eq!(got, ["aiueo", "zasshi", "plain ascii"]);
}
```

### `ja::transliterate_into`

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — runs <code>transliterate</code>, then moves or copies the result into <code>out</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>()</code>; the romaji is <strong>appended</strong> to <code>out</code>, which is never cleared</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">The same 0–5 pipeline <code>String</code>s as <code>transliterate_ja</code>, plus <code>out</code>'s own growth. The very first call on a never-allocated <code>String::new()</code> moves the pipeline's buffer in instead of copying; every call after that — and every call on a buffer that already has capacity — copies with <code>push_str</code>, which preserves whatever <code>out</code> already had</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes — reserve once, <code>clear()</code> between calls, exactly like every other <code>_into</code> API in the workspace</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Building one document out of many transliterated fragments</span></div>
</div>

```rust  ignore
pub fn transliterate_into(text: &str, out: &mut String)
```

**It appends. It does not clear.** That is the crate's contract, it is what the
test suite asserts (`transliterate_into` is replayed into a buffer pre-seeded
with `"<"` and the whole result is compared), and it is what makes the accumulate
pattern need no special API:

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

The hot-loop shape, when you want one result per iteration rather than one
accumulated document:

```rust
use verbora_transliterators::transliterate_into;

fn romanise_each(words: &[&str]) -> Vec<usize> {
    let mut buf = String::new();
    let mut lengths = Vec::with_capacity(words.len());

    for w in words {
        buf.clear();                       // yours to call — it never clears for you
        transliterate_into(w, &mut buf);
        lengths.push(buf.len());
    }
    lengths
}

fn main() {
    assert_eq!(romanise_each(&["とうきょう", "ヨコハマ"]), [7, 8]);
}
```

#### The move optimisation, and why it does not bite the reuse ritual

`transliterate_into` moves the pipeline's own buffer into `out` instead of
copying into it — but only under a narrow guard that protects a caller's
reservation:

```rust ignore
Cow::Owned(s) if out.is_empty() && out.capacity() == 0 => *out = s,   // move
Cow::Owned(s) => out.push_str(&s),                                   // copy
```

The move only fires when `out` has **never allocated** — a fresh `String::new()`
that has not yet had anything pushed into it. Reserve capacity up front, and the
guard's `capacity() == 0` half is false from the very first call, so every call
copies with `push_str` and the reservation survives:

```rust
use verbora_transliterators::transliterate_into;

fn main() {
    // Reserved capacity survives every call, including the first.
    let mut reserved = String::with_capacity(1024);
    transliterate_into("カナ", &mut reserved);
    assert_eq!(reserved, "kana");
    assert_eq!(reserved.capacity(), 1024);
    reserved.clear();                          // clear() never frees capacity
    transliterate_into("テスト", &mut reserved);
    assert_eq!(reserved, "tesuto");
    assert_eq!(reserved.capacity(), 1024);

    // String::new() takes the move on its first call only. The capacity it
    // ends up with is then stable across every later call, moved-in or not.
    let mut fresh = String::new();
    transliterate_into("カナ", &mut fresh);
    let cap = fresh.capacity();
    fresh.clear();
    transliterate_into("テスト", &mut fresh);
    assert_eq!(fresh.capacity(), cap);          // unchanged: no second move
}
```

So the ordinary "`with_capacity` once, `clear()` each iteration" pattern from
[Buffer reuse](../performance/buffer-reuse.md) works here exactly as it does
everywhere else in the workspace. The move is a one-time saving for the common
case of building one document from `String::new()` — it means the first
fragment's allocation becomes `out`'s allocation instead of being copied into a
second one — and it never costs a caller who reserved ahead of time anything.

**What `transliterate_into` does *not* save you** is the pipeline's own
allocations. It is a wrapper over `transliterate`, so a string that five phases
rewrite still allocates up to five intermediate `String`s inside the call; only
the last one reaches `out`. The phases cannot be fused into one buffer, because
each phase needs the previous phase's *entire* output as its input. What
`transliterate_into` removes is the caller's side: no `Vec<String>` of fragments,
no second concatenation pass. See
[Iterator vs reusable buffer](../performance/iterator-vs-into.md).

### `ja::transliterate_normalized`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — <code>normalize_ja</code>, then the five phases</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Cow&lt;'_, str&gt;</code> — borrowed when neither half changed anything</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v"><code>normalize_ja</code>'s (see <a href="./normalizers">Normalizers</a>), plus the pipeline's 0–5; the borrow is carried across the join</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Internal only; there is no <code>_into</code> variant</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Real-world Japanese input of unknown width and normalisation</span></div>
</div>

```rust  ignore
pub fn transliterate_normalized(text: &str) -> Cow<'_, str>
```

This composition is **not** in the reference exports; it is spelled out here
because every caller of `TransliterateJa` inside the reference tree does it
anyway. `stemmer_ja` and `inflectors/ja/noun_inflector` both run
`normalizeJa` first, precisely because the transliterator has no
halfwidth-katakana keys:

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
`transliterate_ja` when you know the text is already fullwidth — or when you need
byte-exact agreement with a call site that did *not* normalise.

<div class="callout callout-spec">
<strong>Composition guarantee.</strong> This function is checked never to change
an answer <code>transliterate_ja</code> already gives: for every input on which
<code>normalize_ja</code> is the identity, the composition is asserted to agree
exactly with the bare transliteration.
</div>

### `Phase::apply` and `Phase::apply_into`

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>
<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager over one phase, driven by the lazy <code>Rewrites</code> iterator</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>apply</code> → <code>Cow&lt;'_, str&gt;</code>; <code>apply_into</code> → <code>()</code>, <strong>appending</strong> to <code>&amp;mut String</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v"><code>apply</code>: none if the phase matched nothing, else one <code>String</code>. <code>apply_into</code>: none of its own — matched and unmatched runs are pushed straight into <code>out</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v"><code>apply_into</code> appends and never moves, so a caller buffer keeps its capacity across every call</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Debugging a phase, or reusing exactly one table</span></div>
</div>

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

`Phase::apply_into` is the only genuinely allocation-free writer here: it splices
the rewrites directly into your buffer. `transliterate_into` cannot do that,
because the pipeline needs each phase's whole output before the next one starts.

<div class="callout callout-warn">
<strong>Careful.</strong> Running the phases yourself is only equivalent to
<code>transliterate_ja</code> if you run <strong>all five, in
<code>Phase::ALL</code> order</strong>. Skipping one or reordering two changes
ordinary words: see <a href="#the-pipeline">The pipeline</a>.
</div>

## Phases, rewrites and introspection

`Phase`, `Rewrite` and `Rewrites` are the crate's whole type surface, and a normal
caller needs none of them. They exist for three reasons, in descending order of
how likely you are to care:

1. **They are the implementation.** `Phase::rewrites` is the single description of
   every phase's behaviour; `Phase::apply`, `transliterate_ja` and
   `transliterate_into` are all built on it, so there is no second copy to drift.
2. **They are how behaviour is localised.** The recording captures the original's own
   `replace1` / `replace2` / `replace3` closures and both un-named regex chains,
   so a failing phase suite says *which* of the five broke instead of only that
   the output is wrong.
3. **They are introspection.** If a result surprises you, the rewrites tell you
   exactly which rule fired, over which bytes.

### `Rewrite`

```rust  ignore
pub struct Rewrite<'a> {
    pub start: usize,        // byte offset where the replaced text begins
    pub end: usize,          // byte offset one past the end
    pub from: &'a str,       // the slice being replaced — &text[start..end]
    pub to: &'static str,    // the text written in its place
}
```

Byte offsets, not character indices, because that is what splicing needs. `to` is
`&'static str` because every replacement comes from a static table — which is also
why yielding rewrites allocates nothing at all.

### `Rewrites`

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

```rust
use verbora_transliterators::ja::Phase;

fn main() {
    // What the kana table would do, without doing it.
    let hits: Vec<_> = Phase::Kana.rewrites("カナ").collect();
    assert_eq!(hits.len(), 2);
    assert_eq!((hits[0].start, hits[0].end, hits[0].from, hits[0].to), (0, 3, "カ", "ka"));
    assert_eq!((hits[1].start, hits[1].end, hits[1].from, hits[1].to), (3, 6, "ナ", "na"));

    // The lookahead phase consumes ONLY its source character: the character that
    // triggered the rule stays in the output and is re-examined next step.
    let hits: Vec<_> = Phase::SokuonAndN.rewrites("ッカ").collect();
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].from, hits[0].to), ("ッ", "k"));

    // Counting matches allocates nothing — no output string is ever built.
    assert_eq!(Phase::Kana.rewrites("カタカナ").count(), 4);

    // The whole-input gate makes the iterator empty without scanning.
    assert_eq!(Phase::Kana.rewrites("abc").size_hint(), (0, Some(0)));
    assert_eq!(Phase::Kana.rewrites("カタカナ").size_hint(), (0, Some(12)));
}
```

`size_hint` is `(0, Some(remaining bytes))`: every match consumes at least one
byte, and nothing is guaranteed to match. `Rewrites` is `Clone` and `Debug`; it is
**not** declared `FusedIterator`, though it does keep returning `None` once the
scan is finished.

`Phase` itself is a plain `Copy` enum with `Debug`, `PartialEq`, `Eq`, `Hash`,
`PartialOrd` and `Ord`, plus the `Phase::ALL` array in pipeline order.

## Advanced usage

### Reproducing the pipeline by hand

The five phases compose exactly as `transliterate_ja` composes them, and the
`map_cow` adapter that keeps the borrow alive through a no-op stage is the same
one described on the [Normalizers](./normalizers.md#composing-normalizers-without-allocating-per-stage)
page. The crate's version is private, so write your own:

```rust
use std::borrow::Cow;

use verbora_transliterators::ja::Phase;
use verbora_transliterators::transliterate_ja;

fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            let next = match f(&owned) {
                Cow::Borrowed(_) => None,
                Cow::Owned(v) => Some(v),
            };
            Cow::Owned(next.unwrap_or(owned))
        }
    }
}

/// Exactly what `transliterate_ja` does, spelled out.
fn by_hand(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for phase in Phase::ALL {
        out = map_cow(out, |s| phase.apply(s));
    }
    out
}

fn main() {
    for input in ["ハイジャッンプ", "ボージョレー・ヌーヴォー", "ああっ", ""] {
        assert_eq!(by_hand(input), transliterate_ja(input));
    }
}
```

This is worth doing only when you want to observe or instrument the intermediate
states. `transliterate_ja` additionally skips all five phases outright when the
input has no `0xE3` byte, which the hand-written version above does not.

### Parallelism

This crate ships one built-in parallel entry point,
[`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch), behind the
`parallel` Cargo feature — see that section above for what it costs and when
it wins. It exists because `transliterate` is a pure function of one `&str`
with no shared state, so fanning it out over many independent documents was
an unambiguous, measured win; it is the **only** operation in this crate that
ships a built-in parallel API; every other item here (`transliterate_into`,
`Phase::apply`, `Phase::apply_into`, `Phase::rewrites`) has none of its own.

For those, or for a different parallel shape than `par_transliterate_ja_batch`
provides (a shared output buffer built with `transliterate_into`, for
instance), parallelising is yours to do and trivially safe: every item here
is a free function or a `Copy` enum over `&'static` tables — no state, no
interior mutability, no globals.

```rust  ignore
// Your own rayon, over Verbora's stateless API — the same pattern
// par_transliterate_ja_batch uses internally, spelled out at your call site.
use rayon::prelude::*;

let romaji: Vec<String> = docs
    .par_iter()
    .map(|d| transliterate_ja(d).into_owned())
    .collect();
```

Collecting across threads forces `.into_owned()`, which reintroduces an
allocation for every document — including the ones that had no kana and would
otherwise have been borrowed. `par_transliterate_ja_batch` avoids exactly this:
its `Vec<Cow<'_, str>>` output keeps every borrowed document borrowed. See
[Parallelism](../performance/parallelism.md).

### There is one batch API, and it requires a Cargo feature

No plain, always-available `_batch` function, no slice-taking entry point
outside the `parallel` feature, no `Transliterator` trait. The reference has
one function taking one string, and so does this crate's default build:
[`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch) is the one
exception, gated behind `parallel` and off by default. Without that feature
enabled, to transliterate a collection, loop — and if the results are going
into one place, `transliterate_into` is the loop body that avoids the
intermediate `Vec<String>`.

## Performance characteristics

Every phase is **O(n) in the input length** with a small constant. There is no
backtracking, no regex engine, no automaton and no runtime construction step. The
crate's only dependency is `verbora-normalizers`, and only for
`transliterate_normalized`.

| Stage | Cost per character when nothing matches |
|---|---|
| Whole input, once | One vectorised `slice::contains(&0xE3)`. A document with no such byte skips all five phases |
| Table phases (1, 3, 4) | One shift and one bitmap test against `codepoint >> 8`, then — only for a character that genuinely begins a key — a second bitmap test on the low byte. All keys are BMP, so astral characters are rejected by the first test |
| Lookahead phase (2) | One binary search over four source characters |
| Final sokuon (5) | One `contains` over two characters, then one byte test for the ASCII word class |

The two-level gate is the reason the table phases are cheap on realistic text.
Every key of every table starts in block `0x30`, so a block-level test alone would
admit *all* hiragana and *all* katakana and then run three binary searches that
usually miss. Splitting the low byte out makes the gate exact — for the compound
table, four characters out of 191 reach the key tables at all.

Where a character *does* begin a key, "longest match here" is decided by the
current character and at most the next two, because no key is longer than three
`char`s. That is three binary searches over sorted static slices, which is
strictly less work than the 382-branch regex alternation the reference compiles.

Two structural differences from the reference, both of which change the shape of the
cost rather than its constant:

- **The reference runs 35 global regexes over every string it is handed**,
  whatever is in it. Latin prose, kanji and halfwidth katakana all pay full
  price. This port rejects them with one byte scan and returns the input
  borrowed.
- **The 30 lookahead passes are one scan.** Fusing ordered lookahead rewrites is
  not valid in general — a pass can consume a character a later pass was going to
  look ahead *at*. It is valid here for two specific reasons: every replacement is
  ASCII, and no ASCII character is a source or a member of any lookahead class; and
  the only source characters that also appear in a lookahead class (`ン` and `ん`)
  are read by rules that run before any rule that could rewrite them. The
  generator re-proves the equivalence against the 30 real passes on every run.

Criterion benchmarks live in
`crates/verbora-transliterators/benches/transliterators.rs` and are split
deliberately into *rejection cost* (ASCII prose, halfwidth katakana), *work cost*
(kana throughout), *per-phase cost*, and the buffered-vs-fresh-allocation
comparison. A reference baseline for the same inputs was recorded once. A fifth group,
`par_transliterate_ja_batch` (behind `--features parallel`), is the source of
the sequential-vs-parallel numbers quoted under
[`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch) above.

> Not yet benchmarked — no Rust-side numbers for this crate are published, and the
> only cross-language results today are the 26 `verbora-distance` benchmarks.
> See [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

| Call | Input with no kana | Input the pipeline changes |
|---|---|---|
| `transliterate_ja` | **Zero allocations**, `Cow::Borrowed`, after one byte scan | One `String` per phase that rewrites something — at most five, typically one or two. Each is `String::with_capacity(that phase's input length)` |
| `ja::transliterate_into` | One `push_str` into `out`; `out` grows if it must | The above, then a move into `out` only on its very first call from a never-allocated `String::new()` — every other call, and any call on a buffer with existing capacity, copies with `push_str` |
| `ja::transliterate_normalized` | **Zero**, borrowed through both halves | `normalize_ja`'s allocations, then the pipeline's |
| `Phase::apply` | **Zero**, `Cow::Borrowed` | Exactly one `String` |
| `Phase::apply_into` | One `push_str` into `out` | **None of its own** — every run is pushed straight into `out` |
| `Phase::rewrites` | **Zero** | **Zero** — the iterator yields offsets and `&'static str` |

**No phase buffer ever regrows.** Every replacement in all three tables, in the
lookahead table and in the final pass is *no longer in bytes* than the text it
replaces — three-byte kana become one to three ASCII bytes, `キョウ` (nine bytes)
becomes `kyō` (four) — so `String::with_capacity(text.len())` is always enough.

**The pipeline's intermediates are unavoidable today.** Five phases, each needing
the previous phase's complete output, means a fully-rewritten string can pay for
up to five buffers. The mitigation that exists is the one the crate already
applies: a phase that changes nothing hands the previous buffer forward untouched
rather than copying it, so the count is "phases that fired", not "phases".

See [Allocation](../performance/allocation.md),
[Zero-copy](../performance/zero-copy.md) and
[Buffer reuse](../performance/buffer-reuse.md).

## Unicode and language notes

### The tables have deliberate holes

`ジ` is excluded from the `ッ` → `z` class and gets its own `j` rule; `フ` is
excluded from `ッ` → `h` and gets `f`. Writing out "the whole ざ row" would break
real words:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ざっし"), "zasshi");   // し is in the s class
    assert_eq!(transliterate_ja("ジャッジ"), "jajji");   // ジ is NOT in the z class
    assert_eq!(transliterate_ja("バッファ"), "baffa");   // フ is NOT in the h class
}
```

### `ン` has two faces

Before a labial (the `ば`, `ぱ` and `ま` rows) it is `m`; before a vowel or a
`y`-row kana it is `n'`; everywhere else the kana table's plain `n` applies.

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("かんぱい"), "kampai");
    assert_eq!(transliterate_ja("しんぶん"), "shimbun");
    assert_eq!(transliterate_ja("ほんや"), "hon'ya");
    assert_eq!(transliterate_ja("まんと"), "manto");
}
```

### `・` KATAKANA MIDDLE DOT becomes an ASCII space

It is the module's one non-kana key, and it exists only in the katakana half of
the table — the hiragana half has no counterpart:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("・・"), "  ");
    assert_eq!(transliterate_ja("あ・い"), "a i");
    assert_eq!(transliterate_ja("ボージョレー・ヌーヴォー"), "bōjorē nūvō");
}
```

### The final `\B` is the reference's, which is ASCII-only

The reference's `\w` is `[A-Za-z0-9_]` with no Unicode semantics, so `ッ` is a
non-word character and `\B` after it holds exactly when the next code unit is
absent or is itself non-word. Rust's `regex` crate makes `\B` Unicode-aware and
gets `ッ漢` **exactly backwards**, which is why there is no regex here:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ッ漢"), "t漢");   // boundary: 漢 is non-word to the reference
    assert_eq!(transliterate_ja("ッ"), "t");
    assert_eq!(transliterate_ja("ッ."), "t.");
    assert_eq!(transliterate_ja("ッA"), "ッA");   // no boundary: the tsu SURVIVES
    assert_eq!(transliterate_ja("ッ1"), "ッ1");
    assert_eq!(transliterate_ja("ッ_"), "ッ_");
}
```

### The long-vowel table is lowercase-only

Only lowercase `a i u e o` participate, so `aー` becomes `ā` while `Aー` is left
exactly as it is. A port that case-folded first would silently produce `Ā`:

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("aー"), "ā");
    assert_eq!(transliterate_ja("Aー"), "Aー");
    assert_eq!(transliterate_ja("iー"), "ī");
    assert_eq!(transliterate_ja("Iー"), "Iー");
}
```

### What passes through untouched

Halfwidth katakana, kanji, iteration marks (`々`), Latin text, digits,
punctuation, every non-Japanese script and every astral character. A bare `ー`
with no vowel in front of it is also left alone.

```rust
use verbora_transliterators::transliterate_ja;
fn main() {
    assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
    assert_eq!(transliterate_ja("時々刻々"), "時々刻々");
    assert_eq!(transliterate_ja("ー"), "ー");
    assert_eq!(transliterate_ja("これは日本語のテストです。"), "koreha日本語notesutodesu。");
}
```

Note the ideographic full stop `。` in that last result: it is U+3002, inside the
gated block, and no table has a key for it.

<div class="callout callout-note">
<strong>Note.</strong> Unlike much of Verbora, this crate has <strong>no UTF-16
semantics to worry about</strong>. Every pattern matches whole characters and the
only zero-width assertion is <code>\B</code>, so a surrogate pair can never be
split — the tests assert that on every output rather than assuming it.
</div>

## Divergence: this cannot throw

One deliberate divergence, and it is a consequence of the type system. The
reference does no argument checking, so `TransliterateJa(null)` and
`TransliterateJa(42)` both raise a `TypeError` from inside `String#replace`. A
`&str` parameter makes those calls unrepresentable.

The thrown messages are still recorded — ten of them, in a dedicated
`TransliterateJa.throws` suite — and the crate's tests assert each one is the
expected `TypeError`. So "this is the *whole* difference" stays a checked claim
rather than an unexamined one. Everything else is byte-exact.

## Common mistakes

### Assuming `transliterate_into` clears its buffer

It appends. Verbora has two conventions and this crate uses the appending one:

```rust
use verbora_transliterators::transliterate_into;
fn main() {
    let mut buf = String::from("already here: ");
    transliterate_into("カナ", &mut buf);
    assert_eq!(buf, "already here: kana");   // not "kana"
}
```

### Feeding it halfwidth katakana

`ｱｲｳｴｵ` comes back unchanged and nothing warns you. Use
`ja::transliterate_normalized`, or run
[`normalize_ja`](./normalizers.md) yourself first.

### Expecting `ッ` before a Latin letter to become `t`

`ッA` stays `ッA` because the reference's `\B` finds no boundary before an ASCII word
character. It looks like a bug and it is faithful.

### Running the phases yourself, in the wrong order

`Phase::ALL` is the order, and reordering it is observable on ordinary words:

```rust
use verbora_transliterators::ja::Phase;
use verbora_transliterators::transliterate_ja;

/// Runs an arbitrary phase order, materialising between stages.
fn run(order: &[Phase], text: &str) -> String {
    let mut cur = text.to_owned();
    for phase in order {
        cur = phase.apply(&cur).into_owned();
    }
    cur
}

fn main() {
    use Phase::{CompoundKana, FinalSokuon, Kana, LongVowels, SokuonAndN};

    // Long vowels before the kana table: its keys never see the Latin vowel.
    assert_eq!(transliterate_ja("カァ"), "kā");
    assert_eq!(
        run(&[CompoundKana, SokuonAndN, LongVowels, Kana, FinalSokuon], "カァ"),
        "kaa"
    );

    // The kana table before the sokuon rules: the small tsu is stranded.
    assert_eq!(transliterate_ja("ハイジャッンプ"), "haijanmpu");
    assert_eq!(
        run(&[CompoundKana, Kana, SokuonAndN, LongVowels, FinalSokuon], "ハイジャッンプ"),
        "haijaッnpu"
    );
}
```

### Calling `.into_owned()` on every result

On the common path — any document without kana — that allocates and copies a
string that was already correct. Hold the `Cow`; it derefs to `&str`.

### Expecting the romaji to be reversible, or phonetically correct

`ā` has three possible sources, so there is no inverse. And the mapping is
character-by-character with no lexical knowledge: `こんにちは` is `konnichiha`,
because nothing here knows that `は` is a particle pronounced `wa`.

## Related

- [Normalizers](./normalizers.md) — `normalize_ja`, which you almost always want
  in front of this
- [Tokenizers](./tokenizers.md) — `TokenizerJa`, for splitting Japanese text
- [Zero-copy](../performance/zero-copy.md) — the `Cow` contract across the workspace
- [Allocation](../performance/allocation.md) — where allocations come from
- [Buffer reuse](../performance/buffer-reuse.md) — the two clearing conventions,
  and why this crate appends
- [Iterator vs reusable buffer](../performance/iterator-vs-into.md) — what each
  shape actually optimises
- [Parallelism](../performance/parallelism.md) — the thirteen built-in `par_*`
  APIs across the workspace, including this crate's own
  `par_transliterate_ja_batch`
- [Performance overview](../performance/index.md) and
  [Benchmarks](../benchmarks/index.md)
- [Core traits](./core.md) — the shared vocabulary the rest of the workspace uses
- [Features overview](./index.md) and [Roadmap](./roadmap.md)
- [Recipes](../recipes/index.md) — end-to-end pipelines
- [Choosing an API](../choosing/index.md) — the cross-crate decision guide

## API reference

Everything the crate exports:

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

No errors, no panics, no configuration, no builder, no trait to implement. No
batch or parallel API outside `ja::par_transliterate_ja_batch`, gated behind
the `parallel` Cargo feature and off by default — see
[`ja::par_transliterate_ja_batch`](#ja-par-transliterate-ja-batch) above.
`transliterate_normalized` lives at
`verbora_transliterators::ja::transliterate_normalized` only — it is the one
public item the crate root does not re-export.
