# Inflectors

`verbora-inflectors` turns a word into another form of itself: `octopus` into
`octopi`, `parenthesis` into `parentheses`, `cheval` into `chevaux`, `go` into
`goes`, and `23` into `23rd`. It is tested against the recorded reference
corpus, and it is *bug-compatible*: the four inflectors that share the
`TenseInflector` engine reproduce the reference's the reference quirks on purpose,
down to `pluralize("A")` returning `"AS"`.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>6</strong> inflector APIs are
documented and test-pinned, tables included.
<code>cargo test -p verbora-inflectors</code> runs <strong>50</strong> unit
tests and <strong>10</strong> doctests.
</div>

## The six public types

| Type | Language | Job | Reference file |
|---|---|---|---|
| `NounInflector` | English | Noun singular ⇄ plural | `noun_inflector` |
| `NounInflectorFr` | French | Noun singular ⇄ plural, 744 invariant nouns | `fr/noun_inflector` |
| `NounInflectorJa` | Japanese | Appends/strips `たち`, `達`, `等`, `共`, `方`; reduplicates a short irregular list | `ja/noun_inflector` |
| `PresentVerbInflector` | English | Present tense: base form ⇄ third-person singular | `present_verb_inflector` |
| `CountInflector` | English | Ordinal suffixes (`1st`, `2nd`, `3rd`, `11th`) | `count_inflector` |
| `CountInflectorFr` | French | Ordinal suffixes (`1er`, everything else `e`) | `fr/count_inflector` |

The first four share one engine and one API shape — they all implement
[`SingularPluralInflector`](#the-singularpluralinflector-trait). The two
`Count*` types share nothing with them: they are stateless, so every method is
an associated function, and there is no instance to construct.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>PresentVerbInflector</code>'s method names are
inverted relative to the noun inflectors, exactly as in the reference:
<code>singularize("go")</code> is <code>"goes"</code> (the third-person
<em>singular</em> verb) and <code>pluralize("goes")</code> is <code>"go"</code>.
</div>

## When to use it

- Normalising a term index or a search query so `cats` and `cat` collide.
- Generating human-readable text: pluralising a label to match a count, or
  writing `3rd` instead of `3`.
- Reproducing an existing the reference pipeline's output byte for byte. This is
  the case the crate is built for; if you only need *approximately correct*
  English plurals, several of the behaviours documented below will look like
  bugs, because they are — faithfully reproduced ones.

## When not to use it

- **Stemming or lemmatisation.** Inflection is generative (`child` → `children`);
  a stemmer is reductive and merges forms an inflector would not.
  `verbora_core::Stemmer` is a trait with no implementations in this workspace
  yet — see [Core](../features/core.md).
- **Phrases and sentences.** Every rule is anchored on the end of the whole
  input string, so `pluralize("dog house")` is `"dog houses"` but
  `pluralize("hot dog")` is `"hot dogs"` and `pluralize("mother in law")` is
  `"mother in laws"`. Tokenize first — see
  [Tokenizers](../features/tokenizers.md).
- **Languages other than English, French and Japanese.** There is no generic
  rule engine exposed for a new language; you can add rules to an existing
  instance, but not register a new inflector type.
- **Case-insensitive matching.** Inflectors *restore* the input's case onto
  their output, which means the case quirks in
  [Unicode and language notes](#unicode-and-language-notes) apply. If you want a
  case-folded index, normalise first — see
  [Normalizers](../features/normalizers.md).

## Quick example

```rust
use verbora_inflectors::{CountInflector, NounInflector, PresentVerbInflector};

fn main() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("octopus").unwrap(), "octopi");
    assert_eq!(nouns.singularize("parentheses").unwrap(), "parenthesis");

    let verbs = PresentVerbInflector::new();
    assert_eq!(verbs.singularize("go").unwrap(), "goes");
    assert_eq!(verbs.pluralize("catches").unwrap(), "catch");

    assert_eq!(CountInflector::nth(23), "23rd");
}
```

`new()` is cheap. The rule tables — every compiled regex, both irregular maps
and the invariant list — are built once per process behind a `LazyLock` and
shared by every instance, so constructing an inflector copies a couple of
pointers and allocates nothing until you add a rule. (The reference rebuilds all
of it in each constructor, including the 744-entry French invariant array.)

## Choosing the right API

Three independent decisions: owned result or caller buffer; concrete type or
trait; and for ordinals, whole string or bare suffix.

### Decision tree

```text
I need to inflect a word
│
├── Nouns or verbs
│   │
│   ├── One call, or a few
│   │      └── pluralize() / singularize()      → Result<String, EmptyToken>
│   │
│   ├── A corpus, in a loop, and I keep a buffer around
│   │      └── pluralize_into() / singularize_into()  → appends to your String
│   │
│   └── My function must work for several languages
│          └── take `&impl SingularPluralInflector`,
│             or store `Box<dyn SingularPluralInflector>`
│
└── Ordinals
    │
    ├── I want the whole thing ("23rd")
    │      └── nth(i64) / nth_f64(f64) / nth_str(&str)   → String
    │
    └── I want only the suffix ("rd"), and I am formatting anyway
           └── nth_form(i64) / nth_form_f64(f64) / nth_form_str(&str)
              → &'static str, allocates nothing
```

### Comparison table — nouns and verbs

| API | Returns | Result allocation | Buffer reuse | Batch | Parallel | Best for |
|---|---|:--:|:--:|:--:|:--:|---|
| `pluralize` / `singularize` | `Result<String, EmptyToken>` | one `String` | ❌ | ❌ | ❌ | one-off calls, readable code |
| `pluralize_into` / `singularize_into` | `Result<(), EmptyToken>` | none — appends to yours | ✅ | ❌ | ❌ | loops over a corpus |
| `SingularPluralInflector` (trait) | the same four methods | as above | as above | ❌ | ❌ | generic or dynamically dispatched code |

There is no batch API and no parallel API. See
[Allocation behaviour](#allocation-behaviour) for what `_into` does and does not
remove, and [Parallelism](../performance/parallelism.md) for running the
inflectors across threads yourself.

### Comparison table — ordinals

| API | Argument | Returns | Allocations | Reproduces |
|---|---|---|---|---|
| `nth_form(i64)` | `i64` | `&'static str` | none | `nthForm` over an integer |
| `nth(i64)` | `i64` | `String` | one, `with_capacity(24)` | `nth`, exact past 2⁵³−1 where reference rounds |
| `nth_form_f64(f64)` | `f64` | `&'static str` | none | `nthForm` over a reference number, `NaN`/`±Infinity` included |
| `nth_f64(f64)` | `f64` | `String` | result plus a short-lived formatting buffer | `nth` including `Number.prototype.toString` layout |
| `nth_form_str(&str)` | `&str` | `&'static str` | none | `nthForm` with the `%` operator's `ToNumber` coercion |
| `nth_str(&str)` | `&str` | `String` | one, `with_capacity(len + 2)` | `nth("11") === "11th"`, input echoed verbatim |

`CountInflectorFr` exposes the same six names with the same shapes; only the
rule differs (see [`CountInflectorFr`](#countinflectorfr)).

---

### `pluralize()` / `singularize()`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>
<span class="badge badge-fallible">FALLIBLE</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>String</code> in a <code>Result</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>String</code> for the result (<code>with_capacity(token.len() + 4)</code>), plus whatever the winning stage costs — see below</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No — use <code>_into</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Ordinary calls; anything not in a tight loop</span></div>
</div>

The default choice.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    assert_eq!(inflector.pluralize("radius").unwrap(), "radii");
    assert_eq!(inflector.singularize("radii").unwrap(), "radius");

    // The empty token is the one input that fails.
    assert!(inflector.pluralize("").is_err());
    assert_eq!(
        inflector.pluralize("").unwrap_err().to_string(),
        "cannot inflect the empty token"
    );
}
```

The methods take `&self`: an inflector is only mutable through `add_plural`,
`add_singular` and `add_irregular`, so you can share one instance freely.

### `pluralize_into()` / `singularize_into()`

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>
<span class="badge badge-fallible">FALLIBLE</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><strong>Appended</strong> to your <code>&amp;mut String</code>; returns <code>Result&lt;(), EmptyToken&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None for the result once the buffer has capacity; the winning stage may still allocate one intermediate <code>String</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes — the buffer is never cleared for you</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Millions of words through one buffer</span></div>
</div>

<div class="callout callout-warn">
<strong>Careful.</strong> <code>pluralize_into</code> <em>appends</em>. It does
not clear <code>out</code>. That is deliberate — it is what lets you build one
joined output without an intermediate <code>Vec&lt;String&gt;</code> — but it
means a scratch-buffer loop must call <code>out.clear()</code> itself.
</div>

The scratch-buffer pattern, where the buffer's capacity is reused and the
contents are not:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    let corpus = ["hacker", "party", "child", "deer"];

    let mut scratch = String::with_capacity(32);
    let mut lengths = Vec::new();
    for word in corpus {
        scratch.clear(); // `_into` appends: clearing is YOUR job
        inflector.pluralize_into(word, &mut scratch).unwrap();
        // `scratch` is now the plural; index it, hash it, write it to a sink…
        lengths.push(scratch.len());
    }
    assert_eq!(lengths, [7, 7, 8, 4]);
}
```

The accumulator pattern, where appending is the point:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    let mut line = String::with_capacity(64);
    for (i, word) in ["box", "party", "deer"].into_iter().enumerate() {
        if i > 0 {
            line.push(' ');
        }
        inflector.pluralize_into(word, &mut line).unwrap();
    }
    assert_eq!(line, "boxes parties deer");
}
```

On `EmptyToken` the buffer is left exactly as it was — nothing partial is ever
appended, so a failed call cannot corrupt an accumulator:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    let mut buf = String::from("<");
    inflector.pluralize_into("hacker", &mut buf).unwrap();
    assert_eq!(buf, "<hackers");

    assert!(inflector.pluralize_into("", &mut buf).is_err());
    assert_eq!(buf, "<hackers"); // untouched
}
```

`pluralize()` is literally `pluralize_into()` with a fresh
`String::with_capacity(token.len() + 4)` in front of it, so the two can never
disagree about a result. See
[Iterator vs `_into`](../performance/iterator-vs-into.md) for the general shape
of this trade-off across Verbora, and
[Buffer reuse](../performance/buffer-reuse.md) for the pattern itself.

### The `SingularPluralInflector` trait

Implemented by `NounInflector`, `NounInflectorFr`, `NounInflectorJa` and
`PresentVerbInflector`. Each type also carries all seven methods inherently, so
you only need the trait when your code must not name one concrete inflector.

Two reasons to reach for it:

- **Static generics** — one function that works for any language, monomorphised
  per instantiation.
- **Dynamic dispatch** — the trait is object safe, so a pipeline can choose its
  inflector from configuration at run time. (The test suite itself stores
  `Box<dyn SingularPluralInflector>` for exactly this reason.)

```rust
use verbora_inflectors::{NounInflector, NounInflectorFr, SingularPluralInflector};

fn plural_column<I: SingularPluralInflector>(inflector: &I, words: &[&str]) -> Vec<String> {
    words
        .iter()
        .filter_map(|w| inflector.pluralize(w).ok())
        .collect()
}

fn main() {
    assert_eq!(
        plural_column(&NounInflector::new(), &["party", "", "box"]),
        ["parties", "boxes"]
    );

    let by_lang: Vec<Box<dyn SingularPluralInflector>> = vec![
        Box::new(NounInflector::new()),
        Box::new(NounInflectorFr::new()),
    ];
    assert_eq!(by_lang[0].pluralize("child").unwrap(), "children");
    assert_eq!(by_lang[1].pluralize("cheval").unwrap(), "chevaux");
}
```

The trait carries `pluralize`, `singularize`, `pluralize_into`,
`singularize_into`, `add_plural`, `add_singular` and `add_irregular`. It does
**not** carry `new()`, so a generic constructor is not available; construct the
concrete type and then erase it. The `Count*` types are not part of this trait.

### `CountInflector`

<a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>
— the `nth_form*` half only; `nth*` allocates one `String`.

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager; stateless associated functions</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>&amp;'static str</code> from <code>nth_form*</code>, owned <code>String</code> from <code>nth*</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v"><code>nth_form*</code>: none. <code>nth</code>/<code>nth_str</code>: one <code>String</code>. <code>nth_f64</code>: the result plus one short-lived formatting buffer</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — use <code>nth_form*</code> with <code>write!</code> into your own buffer</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Ordinal labels in generated text</span></div>
</div>

Three argument kinds, because the reference's `%` coerces its operand and Rust has
no single signature that can express integers, floats and strings at once:

```rust
use verbora_inflectors::CountInflector;

fn main() {
    assert_eq!(CountInflector::nth(21), "21st");
    assert_eq!(CountInflector::nth(112), "112th");   // the `% 100` teens guard
    assert_eq!(CountInflector::nth(-1), "-1th");     // `%` keeps the sign
    assert_eq!(CountInflector::nth_form(21), "st");

    assert_eq!(CountInflector::nth_f64(1.5), "1.5th");
    assert_eq!(CountInflector::nth_f64(1e21), "1e+21th");
    assert_eq!(CountInflector::nth_f64(f64::NAN), "NaNth");

    assert_eq!(CountInflector::nth_str("11"), "11th");
    assert_eq!(CountInflector::nth_str("abc"), "abcth");
    assert_eq!(CountInflector::nth_str("0x1f"), "0x1fst");
}
```

Three behaviours worth knowing before you pick an entry point:

- **Negative ordinals are always `th`.** The reference's `%` is a remainder, not a
  modulo, so `-21 % 10` is `-1` and matches no case. Rust's `%` agrees;
  `rem_euclid` would not.
- **`nth_f64` formats like the reference, not like Rust.**
  `Number.prototype.toString` switches to exponential form outside
  `1e-7 … 1e21` and spells the specials `NaN`, `Infinity`, `-Infinity` —
  where Rust's `{}` would print `1000000000000000000000` and `inf`.
- **`nth_str` echoes its argument.** `i.toString()` on a string is the string,
  so only the *suffix* is derived from the coerced numeric value:
  `nth_str("abc")` is `"abcth"` because `"abc" % 100` is `NaN`.

#### `nth_form*` versus `nth*`: the real trade-off

`nth_form*` returns a `&'static str` that is one of exactly four values —
`"st"`, `"nd"`, `"rd"`, `"th"` — and allocates nothing at all. `nth*` allocates
a `String` and gives you the number and the suffix already joined. If you are
building output with `write!` anyway, the `String` from `nth` is pure waste:

```rust
use std::fmt::Write;

use verbora_inflectors::CountInflector;

fn main() {
    let mut line = String::new();
    for i in 1..=3i64 {
        // One buffer, no per-item String.
        write!(line, "{i}{} ", CountInflector::nth_form(i)).unwrap();
    }
    assert_eq!(line, "1st 2nd 3rd ");
}
```

Reach for `nth*` when the ordinal is the whole value you want; reach for
`nth_form*` when it is one field in a larger string. Which is faster in your
workload is
[not yet benchmarked](../benchmarks/index.md) beyond the criterion group in
`crates/verbora-inflectors/benches/inflectors.rs`, which measures `nth/i64`,
`nth_form/i64`, `nth_f64`, `nth_str` and `fr/nth/i64` separately — but the
allocation counts above are read straight from the source and are not in doubt.

#### `i64` exactness versus the reference's `f64`

`nth` takes an `i64` and is exact across its entire range. The reference stores all
numbers as `f64`, so the reference silently rounds beyond 2⁵³−1. `nth_f64` is
there when you want that rounding *on purpose*, for byte-identical output:

```rust
use verbora_inflectors::CountInflector;

fn main() {
    let n = 9_007_199_254_740_993i64; // 2^53 + 1
    assert_eq!(CountInflector::nth(n), "9007199254740993rd");
    // The reference cannot represent that value; the f64 entry point matches it.
    assert_eq!(CountInflector::nth_f64(n as f64), "9007199254740992nd");
}
```

### `CountInflectorFr`

French has one rule — `1er`, everything else `e` — but the reference implements
it with **strict equality** rather than arithmetic, so nothing is coerced. That
makes the three entry points disagree in a way the English ones do not:

```rust
use verbora_inflectors::CountInflectorFr;

fn main() {
    assert_eq!(CountInflectorFr::nth(1), "1er");
    assert_eq!(CountInflectorFr::nth(2), "2e");
    // The reference also accepts the Roman numeral, by exact string comparison.
    assert_eq!(CountInflectorFr::nth_str("I"), "Ier");
    // …and only that one: `"1" === 1` is false, and `"i"` is not `"I"`.
    assert_eq!(CountInflectorFr::nth_str("1"), "1e");
}
```

So `CountInflector::nth_str("1")` is `"1st"` while
`CountInflectorFr::nth_str("1")` is `"1e"`. This is not a porting mistake; it is
the difference between `%` and `===` in the two reference files.

## Advanced usage

### Extending the rules at run time

The reference lets callers add rules to a live inflector, and so does this port.
Three entry points, all `&mut self`:

| Method | Adds | Consulted |
|---|---|---|
| `add_plural(Rule)` | a pluralisation rewrite rule | before every built-in table |
| `add_singular(Rule)` | a singularisation rewrite rule | before every built-in table |
| `add_irregular(&str, &str)` | a singular/plural pair, in **both** directions | shadows the built-in irregular table |

```rust
use verbora_inflectors::{NounInflector, Rule};

fn main() {
    let mut inflector = NounInflector::new();
    inflector.add_plural(Rule::from_pattern("(code|ware)", true, "$1z").unwrap());
    inflector.add_singular(Rule::from_pattern("(code|ware)z", true, "$1").unwrap());
    inflector.add_irregular("gizmo", "gizmoi");

    assert_eq!(inflector.pluralize("code").unwrap(), "codez");
    assert_eq!(inflector.singularize("warez").unwrap(), "ware");
    assert_eq!(inflector.pluralize("gizmo").unwrap(), "gizmoi");
    assert_eq!(inflector.singularize("gizmoi").unwrap(), "gizmo");
    // Every built-in rule still applies to everything else.
    assert_eq!(inflector.pluralize("bus").unwrap(), "buses");

    // Additions are strictly per-instance.
    let fresh = NounInflector::new();
    assert_eq!(fresh.pluralize("code").unwrap(), "codes");
}
```

Four properties of the priority order, all load-bearing:

1. **Caller rules run first**, ahead of the invariant list *and* the irregular
   table. A rule for `deer` beats `deer`'s invariance.
2. **Insertion order decides between caller rules** — the earliest match wins,
   and later rules never run.
3. **Additions are per-instance.** Two `NounInflector`s never see each other's
   rules; only the immutable built-in tables are shared.
4. **`add_irregular` lowercases both arguments** and writes both directions with
   a plain overwrite, so re-registering an existing plural replaces its
   singular. (This is why the reference's own
   `PresentVerbInflector` — which registers `('am','are')` and then
   `('is','are')` — singularises `are` to `is`, not to `am`.)

```rust
use verbora_inflectors::{NounInflector, Rule};

fn main() {
    let mut inflector = NounInflector::new();
    // "deer" is on the invariant list, "child" is in the irregular table.
    inflector.add_plural(Rule::from_pattern("deer", true, "deerz").unwrap());
    inflector.add_singular(Rule::from_pattern("child", true, "childx").unwrap());
    assert_eq!(inflector.pluralize("deer").unwrap(), "deerz");
    assert_eq!(inflector.singularize("children").unwrap(), "childxren");

    // Earliest rule wins; the result is then re-cased from the ORIGINAL token,
    // which for a lowercase token means the whole result is lowercased.
    let mut ordered = NounInflector::new();
    ordered.add_plural(Rule::from_pattern("o", true, "FIRST").unwrap());
    ordered.add_plural(Rule::from_pattern("o", true, "SECOND").unwrap());
    assert_eq!(ordered.pluralize("dog").unwrap(), "dfirstg");
}
```

That last assertion is worth staring at: the rule is unanchored, so it matches
the `o` inside `dog`; only the first match is rewritten; and case restoration is
computed from `"dog"`, which selects `lowercaseify` and lowercases `FIRST`.

### `Rule::from_pattern` versus `Rule::new`

`Rule::from_pattern(source, ignore_case, replacement)` is the faithful constructor.
`source` is what `RegExp.prototype.source` returns and `ignore_case` is the `/i`
flag, so a rule copied out of the reference keeps the reference's matching semantics.
It returns `Result<Rule, PatternError>`.

`Rule::new(regex::Regex, replacement)` is the escape hatch: it takes an
already-compiled Rust regex and therefore uses the `regex` crate's own semantics
(Unicode simple case folding, `.` excluding only `\n`). It is infallible, but it
requires a `regex` dependency in *your* crate at a version compatible with the
one Verbora links, and it opts you out of the established behaviour. Prefer
`from_pattern`.

The replacement is a **the reference** template in both cases, because that is what
the engine speaks: `$1`, `$&`, `` $` ``, `$'`, `$$`, `$<name>`. It is not the
`regex` crate's syntax — the reference reads `"$1s"` as group 1 followed by the
letter `s`, whereas `Captures::expand` would read it as a group *named* `1s` and
substitute nothing.

`Rule` also exposes `apply(&self, token: &str) -> Option<String>` if you want to
test a rule in isolation. `Some("")` means "matched, and rewrote the token to
nothing" — a distinct answer from `None`, for the reason in
[detail 1 below](#the-four-reference-details-that-break-a-naive-port). `Rule`
is `Debug` but not `Clone`; to give two inflectors the same rule, build it
twice.

### The `case` module

`restore_case` and `CaseMode` are public because they are useful on their own:
they are the reference's `restoreCase` decision, exactly, and nothing else in the
Rust ecosystem reproduces its UTF-16 indexing.

| Item | Signature | Notes |
|---|---|---|
| `restore_case` | `fn(&str) -> Option<CaseMode>` | `None` for `""`, where the reference throws |
| `CaseMode` | `enum { Lower, Capitalize, Upper }` | `Copy`, `Eq`, `Debug` |
| `CaseMode::apply` | `fn(self, &str) -> String` | allocates `with_capacity(s.len() + 2)` |
| `CaseMode::apply_into` | `fn(self, &str, &mut String)` | **appends**, like the inflectors' `_into` |

```rust
use verbora_inflectors::{CaseMode, restore_case};

fn main() {
    assert_eq!(restore_case("word"), Some(CaseMode::Lower));
    assert_eq!(restore_case("Word"), Some(CaseMode::Capitalize));
    assert_eq!(restore_case("WORD"), Some(CaseMode::Upper));
    assert_eq!(restore_case("👍"), Some(CaseMode::Capitalize)); // two code units
    assert_eq!(restore_case("1"), Some(CaseMode::Upper));       // caseless, one unit
    assert_eq!(restore_case(""), None);

    assert_eq!(CaseMode::Capitalize.apply("abc"), "Abc");

    let mut out = String::from("[");
    CaseMode::Upper.apply_into("abc", &mut out);
    out.push(']');
    assert_eq!(out, "[ABC]");
}
```

### The `pattern` module

`pattern::compile(source, ignore_case) -> Result<Regex, PatternError>` and
`pattern::translate(source, ignore_case) -> Result<String, PatternError>` are
public too. `translate` rewrites a reference pattern into `regex`-crate syntax
with the reference's semantics baked in — `.` becomes an explicit negated class and
every case-insensitive literal becomes an explicit character class computed from
the reference's `Canonicalize`, so the output never sets `(?i)` and the `regex`
crate's folding tables cannot participate. Use it if you are porting other
the reference regexes and want the same guarantee.

Supported subset: literals, `.`, `^`, `$`, `|`, quantifiers (`*` `+` `?`
`{n,m}` and lazy forms), groups (`(`, `(?:`, `(?<name>`), character classes with
ranges and negation, and the escapes `\d \D \w \W \s \S \b \B \0 \n \r \t \v \f
\xHH \uHHHH \u{…}` plus escaped punctuation. **Deliberately rejected**, because
a silent mistranslation would be worse than an error: lookahead, lookbehind,
backreferences and `\p{…}`.

## The four reference details that break a naive port

Everything interesting about this crate follows from `TenseInflector.ize`, a
four-stage `||` chain: caller rules, then the invariant ("ambiguous") list, then
the irregular table, then the built-in rules, then the token unchanged. Case
restoration is computed from the *original* token and applied to whichever stage
won.

### 1. `||` treats `""` as no match

A stage that genuinely matched but rewrote the token to the empty string is
discarded by `||`, and the chain falls through — ultimately to the *unchanged*
token. A port that models each stage as `Option<String>` and happily returns
`Some("")` gets every one of these wrong:

| Call | Rule that fires | Result |
|---|---|---|
| `PresentVerbInflector::pluralize("Es")` | `/e?s$/i → ''` | `"Es"` |
| `PresentVerbInflector::pluralize("s")` | `/e?s$/i → ''` | `"s"` |
| `NounInflectorFr::singularize("S")` | `/(.*)s$/i → '$1'` | `"S"` |
| `pluralize("cat")` after `add_plural(^cat$ → "")` | the caller's rule | `"cats"` |

```rust
use verbora_inflectors::{NounInflector, PresentVerbInflector, Rule};

fn main() {
    let verbs = PresentVerbInflector::new();
    assert_eq!(verbs.pluralize("Es").unwrap(), "Es");
    assert_eq!(verbs.pluralize("s").unwrap(), "s");

    // Your own rules are subject to the same fallthrough.
    let mut nouns = NounInflector::new();
    nouns.add_plural(Rule::from_pattern("^cat$", true, "").unwrap());
    assert_eq!(nouns.pluralize("cat").unwrap(), "cats");
}
```

This is why `Rule::apply` keeps "matched" and "produced something usable" as
separate signals: every stage is filtered with `!s.is_empty()` afterwards.

### 2. `restoreCase` indexes UTF-16 code units

<span class="badge badge-utf16">UTF-16</span>

The reference is four lines, and each is a trap:

```text
if (token[0] === token[0].toUpperCase()) {
  if (token[1] && token[1] === token[1].toLowerCase()) return capitalize
  else return uppercaseify
} else return lowercaseify
```

`token[0]` and `token[1]` are **UTF-16 code units**, not characters, and the
test is a round-trip string comparison, not a character-class query. Three
consequences:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // "👍" is TWO code units, both case-invariant → capitalize → "👍s".
    // A `chars()`-based port sees one character, finds no token[1], and
    // produces "👍S".
    assert_eq!(nouns.pluralize("👍").unwrap(), "👍s");
    // "A" is one unit with no token[1] → uppercaseify.
    assert_eq!(nouns.pluralize("A").unwrap(), "AS");
    // Digits are neither upper nor lower, but `c === c.toUpperCase()` is true.
    assert_eq!(nouns.pluralize("1").unwrap(), "1S");
    assert_eq!(nouns.pluralize("12").unwrap(), "12s");
    // "ß".toUpperCase() is "SS", which is not "ß" → lowercaseify.
    assert_eq!(nouns.pluralize("ß").unwrap(), "ßs");
    // The invariant list returns the LOWERCASED token, then re-cases it.
    assert_eq!(nouns.pluralize("DEER").unwrap(), "DEER");
    assert_eq!(nouns.pluralize("dEer").unwrap(), "deer");
}
```

Reaching for `char::is_uppercase` gets `"1"`, `"👍"` and `"ß"` wrong; iterating
`chars()` gets `"👍"` wrong. ### 3. The patterns are the reference regexes

Handing the reference's pattern sources straight to `regex::Regex` compiles —
and silently changes what they match:

| Construct | the reference (no `/u`) | `regex` crate |
|---|---|---|
| `.` | excludes `\n` `\r` `U+2028` `U+2029` | excludes `\n` only |
| `/i` on `s` | does **not** match `ſ` (U+017F) | matches it |
| `/i` on `k` | does **not** match `K` (U+212A) | matches it |

Neither row is hypothetical:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // `(.*)` stops at the carriage return, so only "ab" is pluralised.
    assert_eq!(nouns.pluralize("ab\rcd").unwrap(), "abs\rcd");
    // `/i` refuses to fold U+017F into `s`, so `(x|ch|ss|sh|s|z)$` declines
    // and the catch-all takes over.
    assert_eq!(nouns.pluralize("ma\u{17f}").unwrap(), "ma\u{17f}s");
    assert_eq!(nouns.pluralize("mas").unwrap(), "mases");
}
```

The fix is not per-rule patching: `pattern::translate` rewrites every pattern so
`(?i)` is never enabled and the `regex` crate's folding tables never participate
at all.

### 4. One rule needs a negative lookahead

The English plural table contains `/^(?!talis|.*hu)(.*)man$/i → '$1men'`. The
`regex` crate cannot express a lookahead, so this single rule is hand-written,
term by term, and matched by source text so the data tables can stay a verbatim
transcription:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("man").unwrap(), "men");
    assert_eq!(nouns.pluralize("workman").unwrap(), "workmen");
    // `hu` anywhere in the token declines the rule; the `(.*)` catch-all
    // then appends `s`.
    assert_eq!(nouns.pluralize("human").unwrap(), "humans");
    // …and so does a leading "talis".
    assert_eq!(nouns.pluralize("talisman").unwrap(), "talismans");
    // `.*` is greedy and `$` anchors, so the LAST "man" is consumed.
    assert_eq!(nouns.pluralize("manman").unwrap(), "manmen");
    assert_eq!(nouns.pluralize("xtalisman").unwrap(), "xtalismen");
}
```

## Divergences from the reference

Documented, deliberate, and listed in full at the top of
`crates/verbora-inflectors/src/lib.rs`.

| Divergence | Reference behaviour | Here |
|---|---|---|
| Empty token | throws `TypeError: Cannot read properties of undefined (reading 'toUpperCase')` from `restoreCase` | `Err(EmptyToken)`. Returning `""` would be a silent divergence: The reference produces nothing at all, not an empty string |
| Argument coercion | `nth(true)` is `"truest"`, `nth([1])` is `"1st"` | not modelled; there is no Rust analogue. Integers, floats and strings get their own entry points |
| Large integers | `nth` rounds past 2⁵³−1 (`f64`) | `nth(i64)` is exact; `nth_f64` reproduces the rounding on demand |
| Global `/g` flag | a `RegExp` carrying `/g` would rewrite every match | caller rules always rewrite the **first** match only, matching the reference's non-global patterns. `Rule` has no `/g` equivalent |
| `addForm` and `FormSet` | public on `TenseInflector` | not exposed. `addForm` takes two raw table objects and `FormSet` is a bare pair of a mutable array and a null-prototype object; neither survives translation as an API. Everything they are used for is reachable through `add_irregular` and `Rule` — which is all the reference itself uses them for |
| Table construction | rebuilt in every constructor | compiled once per process behind a `LazyLock` and shared. Not observable: the tables are immutable and caller additions stay per-instance, which the recorded suite checks explicitly |

## Error handling

Two error types, both `Debug + Display + std::error::Error`.

| Error | Raised by | Meaning |
|---|---|---|
| `EmptyToken` | `pluralize`, `singularize`, `pluralize_into`, `singularize_into` | the token was `""`. Unit struct, `Copy + Eq + Default`; `Display` is `cannot inflect the empty token` |
| `PatternError` | `Rule::from_pattern`, `pattern::compile`, `pattern::translate` | the pattern used an unsupported construct, or the translated pattern was rejected by the `regex` crate. `.message()` returns the reason |

`EmptyToken` is the only failure an inflection call can produce — every
non-empty token yields a result, because the last stage of the chain returns the
token unchanged. In a pipeline, `filter_map(|w| inflector.pluralize(w).ok())` is
usually what you want; use `?` when an empty token indicates a bug upstream.

```rust
use verbora_inflectors::Rule;

fn main() {
    let err = Rule::from_pattern("(?=a)b", true, "x").unwrap_err();
    assert!(err.message().contains("lookahead"));

    assert!(Rule::from_pattern(r"\p{L}", true, "x").is_err()); // not translatable
    assert!(Rule::from_pattern("(.*)ing$", true, "$1ed").is_ok());
}
```

Note that `Rule::from_pattern` returning `Err` is the *good* outcome for an
unsupported construct: the alternative would be a pattern that compiles and
quietly matches something else.

## Performance characteristics

The interesting axis is not input size — tokens are words — but **which stage
resolves the call**:

| Stage | Cost | Example |
|---|---|---|
| Caller rules | one regex attempt per added rule, always, before anything else | any token, once you have added rules |
| Invariant list | one binary search over a sorted `&'static [&str]` | `deer`, `fish`, `rhinocéros` |
| Irregular table | a failed binary search, then another over the irregular pairs | `child`, `mouse`, `foot` |
| Regular rules | a dozen or so translated regexes, first match wins — twelve for English `pluralize`, fourteen for English `singularize` | `party`, `church`, `workman` |
| Fallthrough | all of the above, then the token unchanged | English `singularize("hacker")` |

English `pluralize` has a `(.*)` catch-all as its last regular rule, so it never
reaches the true fallthrough; English `singularize` ends at `s$`, so it often
does.

Construction is flat and language-independent: the shared `LazyLock` tables mean
`NounInflectorFr::new()` costs the same as `NounInflector::new()`, where the
reference would rebuild 744 invariant entries and ten regexes each time.

The criterion suite in `crates/verbora-inflectors/benches/inflectors.rs` is
organised along exactly these axes — `noun-en/by-path` (ambiguous / irregular /
regular / fallback / case-shapes), `by-language`, `noun-en/bulk` (which compares
`pluralize` against `pluralize_into` over the shared 4,000-word corpus),
`construct`, `custom-rules` and `count`.

> No measured inflector numbers are published yet — the only recorded
> the reference comparisons in this repository are the 26 `verbora-distance`
> benchmarks in `docs/PERFORMANCE.md`. See
> [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

Read from the source, per call:

| Step | Allocates |
|---|---|
| `restore_case(token)` | nothing — returns a `Copy` enum |
| lowercasing the token | nothing when the token is already lowercase ASCII (returns `Cow::Borrowed`); one `String` when it contains an ASCII uppercase letter or any non-ASCII character |
| invariant-list hit | nothing — borrows the lowercased token |
| irregular-table hit | nothing — borrows a `&'static str` |
| a regular rule that **does not** match | nothing; group-reading rules run `is_match` before `captures`, so a miss never allocates capture slots |
| a regular rule that **does** match | one `String` for the rewritten token, plus a capture-slot vector when the replacement template actually reads `$1`/`$<name>` |
| writing the result | `pluralize`: one `String::with_capacity(token.len() + 4)`. `pluralize_into`: nothing, unless your buffer must grow |

So `_into` removes the **result** allocation and lets you keep capacity across a
corpus; it does not make the call allocation-free, because the winning rule
still builds its rewritten token. That is a real saving on the common path —
English `pluralize` allocates two `String`s per word and `pluralize_into`
allocates one — but it is not zero.

`add_plural` / `add_singular` push onto a per-instance `Vec<Rule>`;
`add_irregular` pushes two owned `(String, String)` pairs. A freshly constructed
inflector holds no heap allocation of its own.

For ordinals, `nth_form*` allocates nothing at all, `nth` and `nth_str` allocate
exactly one `String`, and `nth_f64` allocates the result plus a short-lived
buffer from the shortest-round-trip float formatting (one more again for
negatives, which recurse). See [Allocation](../performance/allocation.md).

## Concurrency

`verbora-inflectors` ships **no `par_*` API** — a `par_*` candidate was
evaluated and rejected: per-word cost measured at ~360 ns, comparable to
`rayon`'s own dispatch overhead, so a naive `par_iter` over words would likely
lose to its own scheduling cost. (Thirteen other Verbora crates do ship a
`par_*_batch` API where the per-item cost cleared that bar — see
[Parallelism](../performance/parallelism.md).) What there is: inflectors are
`Send + Sync` (their state is `&'static` tables plus owned `Vec`s), and
`pluralize`/`singularize` take `&self` and are pure, so sharing one instance
across threads is sound and you can parallelise yourself. Each worker should own
its own `_into` buffer.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    let words = ["party", "child", "deer", "box"];

    let plurals: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = words
            .chunks(2)
            .map(|chunk| {
                let inflector = &inflector;
                scope.spawn(move || {
                    let mut out = Vec::new();
                    let mut buf = String::new();
                    for word in chunk {
                        buf.clear();
                        inflector.pluralize_into(word, &mut buf).unwrap();
                        out.push(buf.clone());
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    assert_eq!(plurals, ["parties", "children", "deer", "boxes"]);
}
```

The same shape works with `rayon::par_iter` in your own crate. Adding rules
needs `&mut self`, so do all `add_*` calls before sharing the instance. See
[Parallelism](../performance/parallelism.md).

## Unicode and language notes

- **Case restoration is UTF-16-shaped**, as described in
  [detail 2](#_2-restorecase-indexes-utf-16-code-units). This is the single most
  surprising behaviour in the crate for anyone who has not read the reference.
- **Case mapping is the full Unicode one**, including the context-sensitive
  Greek final-sigma rule, matching the reference's `toLowerCase`/`toUpperCase`.
- **Pattern case folding is the reference's legacy `Canonicalize`**, not Unicode
  simple case folding, so `ſ`, `K`, `ı` and `ß` behave as they do in a
  non-`/u` the reference regex.
- **French** carries 744 invariant nouns (mostly `-s`, `-x`, `-z` endings) plus
  irregulars such as `œil` → `yeux` and `bijou` → `bijoux`.
- **Japanese** does not normally mark number. `pluralize` appends `たち` to
  anything via a single `^(.+)$` rule; twelve nouns instead reduplicate
  (`人` → `人人`); `友達` and relatives are on the invariant list. `singularize`
  has five rules: `たち`, `達` and `等` are stripped *unless* the stem is on a
  per-suffix exception list of words that only look plural (`かたち` "shape",
  `配達` "delivery"), while `共`/`ども` and `方`/`がた` are stripped only for an
  explicit allowlist of stems (`野郎共` → `野郎`, `先生方` → `先生`).

```rust
use verbora_inflectors::NounInflectorJa;

fn main() {
    let ja = NounInflectorJa::new();
    assert_eq!(ja.pluralize("私").unwrap(), "私たち");
    assert_eq!(ja.singularize("私たち").unwrap(), "私");
    assert_eq!(ja.pluralize("人").unwrap(), "人人");   // irregular reduplication
    assert_eq!(ja.pluralize("友達").unwrap(), "友達"); // invariant
    assert_eq!(ja.singularize("かたち").unwrap(), "かたち"); // only looks plural
}
```

## Common mistakes

**Forgetting that `_into` appends.**

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();
    let mut buf = String::new();
    inflector.pluralize_into("cat", &mut buf).unwrap();
    inflector.pluralize_into("dog", &mut buf).unwrap();
    assert_eq!(buf, "catsdogs"); // not "dogs"
}
```

Call `buf.clear()` between items when you want a scratch buffer.

**Assuming verbs read like nouns.** `PresentVerbInflector::singularize("go")` is
`"goes"`, not `"go"`. The names describe the *subject's* number, not the word's.

**Passing a phrase.** Rules anchor on the end of the whole string, so
`pluralize("hot dog")` is `"hot dogs"` (right, by luck) and
`pluralize("mother in law")` is `"mother in laws"` (wrong). Tokenize first.

**Expecting the empty string back for the empty token.** It is an `Err`, because
The reference throws. Silently returning `""` would be a divergence with no
recorded justification.

**Writing `Regex::new` where you meant `Rule::from_pattern`.** `Rule::new` opts you
out of the reference semantics — `.` and `/i` will both behave differently, and the
difference only shows up on inputs containing line terminators or characters
like `ſ`.

**Using the `regex` crate's replacement syntax.** The template is the reference's.
`"$1s"` means "group 1, then the letter s"; `"${1}s"` is not special here, and
`"$0"` is a literal `$0`, not the whole match (`$&` is).

**Adding a rule and expecting it everywhere.** Additions are per-instance and
are consulted before every built-in table — including the invariant list, which
means one broad rule can shadow a lot of correct behaviour. Anchor your
patterns.

## Related

- [Choosing an API](../choosing/index.md) — the cross-subsystem version of the
  decision tree above.
- [Buffer reuse](../performance/buffer-reuse.md) and
  [Iterator vs `_into`](../performance/iterator-vs-into.md) — `pluralize_into`
  is one of the few genuine `_into` pairs in the workspace.
- [Allocation](../performance/allocation.md),
  [Performance](../performance/index.md),
  [Parallelism](../performance/parallelism.md).
- [Tokenizers](../features/tokenizers.md) — split text into the words an
  inflector expects.
- [Normalizers](../features/normalizers.md) — case folding and diacritic
  handling, which inflection does *not* do for you.
- [Core](../features/core.md) — the `Stemmer` and `Tokenizer` traits.
- [Recipes](../recipes/index.md), [Benchmarks](../benchmarks/index.md).

## API reference

### Types

```text
verbora_inflectors
├── NounInflector            ├── NounInflectorFr
├── NounInflectorJa          ├── PresentVerbInflector
├── CountInflector           ├── CountInflectorFr
├── SingularPluralInflector  (trait, object safe)
├── Rule                     ├── CaseMode
├── EmptyToken               ├── PatternError
├── restore_case             (fn)
├── case                     (module: CaseMode, restore_case)
└── pattern                  (module: compile, translate, PatternError)
```

### `NounInflector`, `NounInflectorFr`, `NounInflectorJa`, `PresentVerbInflector`

Identical surfaces; each also implements `Default` and `Debug`.

| Method | Signature |
|---|---|
| `new` | `fn() -> Self` |
| `pluralize` | `fn(&self, token: &str) -> Result<String, EmptyToken>` |
| `singularize` | `fn(&self, token: &str) -> Result<String, EmptyToken>` |
| `pluralize_into` | `fn(&self, token: &str, out: &mut String) -> Result<(), EmptyToken>` (appends) |
| `singularize_into` | `fn(&self, token: &str, out: &mut String) -> Result<(), EmptyToken>` (appends) |
| `add_plural` | `fn(&mut self, rule: Rule)` |
| `add_singular` | `fn(&mut self, rule: Rule)` |
| `add_irregular` | `fn(&mut self, singular: &str, plural: &str)` |

### `SingularPluralInflector`

The same seven methods minus `new`. Implemented by all four types above.

### `CountInflector` / `CountInflectorFr`

Unit structs; all methods are associated functions.

| Method | Signature |
|---|---|
| `nth` | `fn(i: i64) -> String` |
| `nth_form` | `fn(i: i64) -> &'static str` |
| `nth_f64` | `fn(x: f64) -> String` |
| `nth_form_f64` | `fn(x: f64) -> &'static str` |
| `nth_str` | `fn(s: &str) -> String` |
| `nth_form_str` | `fn(s: &str) -> &'static str` |

### `Rule`

| Method | Signature |
|---|---|
| `from_js` | `fn(source: &str, ignore_case: bool, replacement: impl Into<String>) -> Result<Rule, PatternError>` |
| `new` | `fn(pattern: regex::Regex, replacement: impl Into<String>) -> Rule` |
| `apply` | `fn(&self, token: &str) -> Option<String>` — `Some("")` means "matched, rewrote to nothing" |

### `case`

`restore_case(&str) -> Option<CaseMode>`; `CaseMode::{Lower, Capitalize, Upper}`
with `apply(self, &str) -> String` and `apply_into(self, &str, &mut String)`
(appends). `restore_case` and `CaseMode` are also re-exported at the crate root.

### `pattern`

`compile(&str, bool) -> Result<regex::Regex, PatternError>`;
`translate(&str, bool) -> Result<String, PatternError>`;
`PatternError::message(&self) -> &str`. `PatternError` is re-exported at the
crate root.
