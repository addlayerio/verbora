# Inflectors

`verbora-inflectors` turns a word into another form of itself: `octopus` into
`octopi`, `parenthesis` into `parentheses`, `cheval` into `chevaux`, `go` into
`goes`, and `23` into `23rd`. Every rule and every edge case — down to
`pluralize("A")` returning `"AS"` — is pinned by the regression suite, so output is
exact and stable rather than approximate.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>6</strong> inflector APIs are
documented and test-pinned, tables included.
<code>cargo test -p verbora-inflectors</code> runs <strong>50</strong> unit tests
and <strong>10</strong> doctests.
</div>

## The six public types

| Type | Language | Job |
|---|---|---|
| `NounInflector` | English | Noun singular ⇄ plural |
| `NounInflectorFr` | French | Noun singular ⇄ plural, 744 invariant nouns |
| `NounInflectorJa` | Japanese | Appends/strips `たち`, `達`, `等`, `共`, `方`; reduplicates a short irregular list |
| `PresentVerbInflector` | English | Present tense: base form ⇄ third-person singular |
| `CountInflector` | English | Ordinal suffixes (`1st`, `2nd`, `3rd`, `11th`) |
| `CountInflectorFr` | French | Ordinal suffixes (`1er`, everything else `e`) |

The first four share one engine and one API shape — they all implement
[`SingularPluralInflector`](#the-singularpluralinflector-trait). The two `Count*`
types share nothing with them: they are stateless, so every method is an associated
function and there is no instance to construct.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>PresentVerbInflector</code>'s method names are
inverted relative to the noun inflectors:
<code>singularize("go")</code> is <code>"goes"</code> (the third-person
<em>singular</em> verb) and <code>pluralize("goes")</code> is <code>"go"</code>.
The names describe the <em>subject's</em> number, not the word's.
</div>

## When to use it

- Normalising a term index or a search query so `cats` and `cat` collide.
- Generating human-readable text: pluralising a label to match a count, or writing
  `3rd` instead of `3`.
- Needing exact, deterministic inflection output rather than an approximation. If
  you only need *approximately correct* English plurals, several of the behaviours
  documented below will look like bugs. They are specified, not accidental.

## When not to use it

- **Stemming or lemmatisation.** Inflection is generative (`child` → `children`); a
  stemmer is reductive and merges forms an inflector would not. See
  [Stemmers](../features/stemmers.md).
- **Phrases and sentences.** Every rule is anchored on the end of the whole input
  string, so `pluralize("mother in law")` is `"mother in laws"`. Tokenize first —
  see [Tokenizers](../features/tokenizers.md).
- **Languages other than English, French and Japanese.** You can add rules to an
  existing instance, but not register a new inflector type.
- **Case-insensitive matching.** Inflectors *restore* the input's case onto their
  output, so the case behaviour in
  [Case restoration](#four-behaviours-worth-knowing) applies. For a
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

`new()` is cheap. The rule tables — every compiled regex, both irregular maps and
the invariant list — are built once per process behind a `LazyLock` and shared by
every instance, so constructing an inflector copies a couple of pointers and
allocates nothing until you add a rule. `NounInflectorFr::new()` costs the same as
`NounInflector::new()` despite French's 744-entry invariant list.

## Choosing the right API

### Nouns and verbs

| API | Returns | Result allocation | Buffer reuse | Best for |
|---|---|:--:|:--:|---|
| `pluralize` / `singularize` | `Result<String, EmptyToken>` | one `String` | ❌ | one-off calls, readable code |
| `pluralize_into` / `singularize_into` | `Result<(), EmptyToken>` | none — appends to yours | ✅ | loops over a corpus |
| `SingularPluralInflector` (trait) | the same four methods | as above | as above | generic or dynamically dispatched code |

There is no batch API and no parallel API — see [Concurrency](#concurrency).

### Ordinals

| API | Argument | Returns | Allocations | Behaviour |
|---|---|---|---|---|
| `nth_form(i64)` | `i64` | `&'static str` | none | Ordinal suffix only, from an integer |
| `nth(i64)` | `i64` | `String` | one, `with_capacity(24)` | Number plus suffix; exact across the full `i64` range |
| `nth_form_f64(f64)` | `f64` | `&'static str` | none | Ordinal suffix only, from a float; `NaN`/`±Infinity` included |
| `nth_f64(f64)` | `f64` | `String` | result plus a short-lived formatting buffer | Number plus suffix, with `nth_f64`'s own float layout |
| `nth_form_str(&str)` | `&str` | `&'static str` | none | Ordinal suffix only, from a string coerced to a number |
| `nth_str(&str)` | `&str` | `String` | one, `with_capacity(len + 2)` | Input echoed verbatim with a suffix appended |

`CountInflectorFr` exposes the same six names with the same shapes; only the rule
differs (see [`CountInflectorFr`](#countinflectorfr)).

### `pluralize_into` and the buffer

<div class="callout callout-warn">
<strong>Careful.</strong> <code>pluralize_into</code> <em>appends</em>. It does not
clear <code>out</code>. That is deliberate — it is what lets you build one joined
output without an intermediate <code>Vec&lt;String&gt;</code> — but it means a
scratch-buffer loop must call <code>out.clear()</code> itself.
</div>

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let inflector = NounInflector::new();

    // Scratch-buffer pattern: capacity is reused, contents are not.
    let mut scratch = String::with_capacity(32);
    for word in ["hacker", "party", "child", "deer"] {
        scratch.clear(); // `_into` appends: clearing is YOUR job
        inflector.pluralize_into(word, &mut scratch).unwrap();
    }
    assert_eq!(scratch, "deer");

    // Accumulator pattern: appending is the point.
    let mut line = String::with_capacity(64);
    for word in ["box", "party", "deer"] {
        inflector.pluralize_into(word, &mut line).unwrap();
    }
    assert_eq!(line, "boxespartiesdeer");

    // On `EmptyToken` the buffer is left exactly as it was: nothing partial is
    // ever appended, so a failed call cannot corrupt an accumulator.
    assert!(inflector.pluralize_into("", &mut line).is_err());
    assert_eq!(line, "boxespartiesdeer");
}
```

`pluralize()` is literally `pluralize_into()` with a fresh
`String::with_capacity(token.len() + 4)` in front of it, so the two can never
disagree about a result. All four methods take `&self` — an inflector is only
mutable through `add_plural`, `add_singular` and `add_irregular` — so one instance
can be shared freely.

### The `SingularPluralInflector` trait

Implemented by `NounInflector`, `NounInflectorFr`, `NounInflectorJa` and
`PresentVerbInflector`. Each type also carries all seven methods inherently, so you
only need the trait when your code must not name one concrete inflector — either
for static generics, or for dynamic dispatch, since the trait is object safe.

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

The trait carries `pluralize`, `singularize`, `pluralize_into`, `singularize_into`,
`add_plural`, `add_singular` and `add_irregular`. It does **not** carry `new()`, so
construct the concrete type and then erase it. The `Count*` types are not part of
it.

### `CountInflector`

Three argument kinds, because coercing a string to a number and formatting an
integer are different operations, and Rust has no single signature covering
integers, floats and strings at once.

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

Three behaviours worth knowing before picking an entry point:

- **Negative ordinals are always `th`.** The suffix comes from `i % 10` using
  Rust's `%` — a remainder, not a modulo — so `-21 % 10` is `-1`, which matches
  none of the `st`/`nd`/`rd` cases.
- **`nth_f64` uses its own float-formatting rules, not Rust's `Display`.** It
  switches to exponential form outside `1e-7 … 1e21` and spells the specials `NaN`,
  `Infinity`, `-Infinity`, where Rust's `{}` would print
  `1000000000000000000000` and `inf`.
- **`nth_str` echoes its argument.** Only the *suffix* is derived from the string's
  coerced numeric value: `nth_str("abc")` is `"abcth"` because `"abc"` coerces to
  `NaN`.

`nth_form*` returns a `&'static str` that is one of exactly four values and
allocates nothing; `nth*` allocates a `String` with the number and suffix already
joined. If you are building output with `write!` anyway, the `String` from `nth` is
pure waste — reach for `nth*` when the ordinal is the whole value you want, and
`nth_form*` when it is one field in a larger string.

`nth` takes an `i64` and is exact across its entire range. `nth_f64` takes an
`f64`, whose 53-bit mantissa silently rounds any integer past 2⁵³−1, and exists for
callers who deliberately want that rounding:

### `CountInflectorFr`

French has one rule — `1er`, everything else `e` — implemented with **strict
equality** rather than numeric coercion. That makes the entry points disagree in a
way the English ones do not: `CountInflector::nth_str("1")` is `"1st"` while
`CountInflectorFr::nth_str("1")` is `"1e"`, because English derives its suffix by
numeric coercion and French by exact string equality.

```rust
use std::fmt::Write;

use verbora_inflectors::{CountInflector, CountInflectorFr};

fn main() {
    // One buffer, no per-item String.
    let mut line = String::new();
    for i in 1..=3i64 {
        write!(line, "{i}{} ", CountInflector::nth_form(i)).unwrap();
    }
    assert_eq!(line, "1st 2nd 3rd ");

    let n = 9_007_199_254_740_993i64; // 2^53 + 1
    assert_eq!(CountInflector::nth(n), "9007199254740993rd");
    assert_eq!(CountInflector::nth_f64(n as f64), "9007199254740992nd");

    assert_eq!(CountInflectorFr::nth(1), "1er");
    // The Roman numeral is accepted, by exact string comparison — and only
    // that one string: "1" is not "I", and "i" is not "I".
    assert_eq!(CountInflectorFr::nth_str("I"), "Ier");
    assert_eq!(CountInflectorFr::nth_str("1"), "1e");
}
```

## Extending the rules at run time

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
    // Every built-in rule still applies to everything else.
    assert_eq!(inflector.pluralize("bus").unwrap(), "buses");

    // Additions are strictly per-instance.
    assert_eq!(NounInflector::new().pluralize("code").unwrap(), "codes");
}
```

Four properties of the priority order, all load-bearing:

1. **Caller rules run first**, ahead of the invariant list *and* the irregular
   table. A rule for `deer` beats `deer`'s invariance.
2. **Insertion order decides between caller rules** — the earliest match wins, and
   later rules never run.
3. **Additions are per-instance.** Two `NounInflector`s never see each other's
   rules; only the immutable built-in tables are shared.
4. **`add_irregular` lowercases both arguments** and writes both directions with a
   plain overwrite, so re-registering an existing plural replaces its singular.
   (This is why `PresentVerbInflector` — which registers `('am','are')` and then
   `('is','are')` — singularises `are` to `is`, not to `am`.)

Caller rules are unanchored unless you anchor them, rewrite only the first match,
and are then re-cased from the *original* token — all three at once produce results
worth staring at. With `add_plural(Rule::from_pattern("o", true, "FIRST"))`,
`pluralize("dog")` is `"dfirstg"`.

### `Rule::from_pattern` versus `Rule::new`

`Rule::from_pattern(source, ignore_case, replacement)` is the primary constructor:
`source` is a regex pattern string, translated to `regex`-crate syntax under the
hood with Verbora's own semantics (see
[the `case` and `pattern` modules](#the-case-and-pattern-modules)).
It returns `Result<Rule, PatternError>`.

`Rule::new(regex::Regex, replacement)` is the escape hatch: it takes an
already-compiled Rust regex and therefore uses the `regex` crate's own semantics.
It is infallible, but it requires a `regex` dependency in *your* crate at a
compatible version, and it opts you out of this crate's semantics. Prefer
`from_pattern`.

The replacement template in both cases uses this crate's own syntax: `$1`, `$&`,
`` $` ``, `$'`, `$$`, `$<name>`. It is **not** the `regex` crate's syntax — a rule
reads `"$1s"` as group 1 followed by the letter `s`, whereas `Captures::expand`
would read it as a group *named* `1s`. `Rule` also exposes
`apply(&self, token: &str) -> Option<String>` for testing a rule in isolation,
where `Some("")` means "matched, and rewrote the token to nothing" — a distinct
answer from `None`. `Rule` is `Debug` but not `Clone`; to give two inflectors the
same rule, build it twice.

### The `case` and `pattern` modules

Both are public because they are useful on their own.

| Item | Signature | Notes |
|---|---|---|
| `restore_case` | `fn(&str) -> Option<CaseMode>` | `None` for `""` |
| `CaseMode` | `enum { Lower, Capitalize, Upper }` | `Copy`, `Eq`, `Debug` |
| `CaseMode::apply` | `fn(self, &str) -> String` | allocates `with_capacity(s.len() + 2)` |
| `CaseMode::apply_into` | `fn(self, &str, &mut String)` | **appends**, like the inflectors' `_into` |
| `pattern::compile` | `fn(&str, bool) -> Result<Regex, PatternError>` | Verbora's semantics baked in |
| `pattern::translate` | `fn(&str, bool) -> Result<String, PatternError>` | the rewritten pattern source |

`translate` rewrites a pattern into `regex`-crate syntax with Verbora's semantics
baked in — `.` becomes an explicit negated class and every case-insensitive literal
becomes an explicit character class from a fixed case-folding table, so the output
never sets `(?i)` and the `regex` crate's own folding tables never participate.
Supported: literals, `.`, `^`, `$`, `|`, quantifiers, groups (`(`, `(?:`,
`(?<name>`), character classes with ranges and negation, and the escapes
`\d \D \w \W \s \S \b \B \0 \n \r \t \v \f \xHH \uHHHH \u{…}` plus escaped
punctuation. **Deliberately rejected**, because a silent mistranslation would be
worse than an error: lookahead, lookbehind, backreferences and `\p{…}`.

## Four behaviours worth knowing

Everything interesting about this crate follows from one fallback chain: caller
rules, then the invariant ("ambiguous") list, then the irregular table, then the
built-in regular rules, then the token unchanged — each stage tried in order, first
usable result wins. Case restoration is computed from the *original* token and
applied to whichever stage won.

### 1. An empty rewrite counts as no match

A stage that genuinely matches but rewrites the token to the empty string is
discarded, and the chain falls through — ultimately to the *unchanged* token.

| Call | Rule that fires | Result |
|---|---|---|
| `PresentVerbInflector::pluralize("Es")` | `/e?s$/i → ''` | `"Es"` |
| `PresentVerbInflector::pluralize("s")` | `/e?s$/i → ''` | `"s"` |
| `NounInflectorFr::singularize("S")` | `/(.*)s$/i → '$1'` | `"S"` |
| `pluralize("cat")` after `add_plural(^cat$ → "")` | the caller's rule | `"cats"` |

Your own rules are subject to the same fallthrough, which is why `Rule::apply`
keeps "matched" and "produced something usable" as separate signals: every stage is
filtered with `!s.is_empty()` afterwards.

### 2. Case restoration indexes UTF-16 code units

<span class="badge badge-utf16">UTF-16</span>

`restore_case` decides how to re-case the result by inspecting the original token's
**first two UTF-16 code units**, not its first two characters, with a round-trip
case comparison rather than a character-class query:

```text
if first_unit == first_unit.uppercased():
  if second_unit exists and second_unit == second_unit.lowercased(): Capitalize
  else: Upper
else: Lower
```

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // "👍" is TWO code units, both case-invariant → Capitalize → "👍s".
    // Iterating `chars()` instead would find no second unit and produce "👍S".
    assert_eq!(nouns.pluralize("👍").unwrap(), "👍s");
    // "A" is one unit with no second code unit → Upper.
    assert_eq!(nouns.pluralize("A").unwrap(), "AS");
    // A digit's uppercased form is itself, so the round-trip test passes.
    assert_eq!(nouns.pluralize("1").unwrap(), "1S");
    assert_eq!(nouns.pluralize("12").unwrap(), "12s");
    // "ß".to_uppercase() is "SS", which is not "ß" → Lower.
    assert_eq!(nouns.pluralize("ß").unwrap(), "ßs");
    // The invariant list returns the LOWERCASED token, then re-cases it.
    assert_eq!(nouns.pluralize("DEER").unwrap(), "DEER");
    assert_eq!(nouns.pluralize("dEer").unwrap(), "deer");
}
```

Reaching for `char::is_uppercase` gets `"1"`, `"👍"` and `"ß"` wrong; iterating
`chars()` gets `"👍"` wrong.

### 3. Patterns use Verbora's own regex semantics

| Construct | Verbora's rule semantics | `regex` crate default |
|---|---|---|
| `.` | excludes `\n` `\r` `U+2028` `U+2029` | excludes `\n` only |
| case-insensitive `s` | does **not** match `ſ` (U+017F) | matches it |
| case-insensitive `k` | does **not** match `K` (U+212A) | matches it |

Neither row is hypothetical:

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // `(.*)` stops at the carriage return, so only "ab" is pluralised.
    assert_eq!(nouns.pluralize("ab\rcd").unwrap(), "abs\rcd");
    // Case-insensitive matching refuses to fold U+017F into `s`, so
    // `(x|ch|ss|sh|s|z)$` declines and the catch-all takes over.
    assert_eq!(nouns.pluralize("ma\u{17f}").unwrap(), "ma\u{17f}s");
    assert_eq!(nouns.pluralize("mas").unwrap(), "mases");
}
```

### 4. One rule needs a negative lookahead

The English plural table contains `/^(?!talis|.*hu)(.*)man$/i → '$1men'`. The
`regex` crate cannot express a lookahead, so this single rule is hand-written and
matched directly against the token rather than through the shared translation path.
`hu` anywhere in the token, or a leading `talis`, declines the rule and the `(.*)`
catch-all appends `s` instead; `.*` is greedy and `$` anchors, so the *last* `man`
is the one consumed.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("workman").unwrap(), "workmen");
    assert_eq!(nouns.pluralize("human").unwrap(), "humans");
    assert_eq!(nouns.pluralize("talisman").unwrap(), "talismans");
    assert_eq!(nouns.pluralize("manman").unwrap(), "manmen");
    assert_eq!(nouns.pluralize("xtalisman").unwrap(), "xtalismen");
}
```

## Error handling

Two error types, both `Debug + Display + std::error::Error`.

| Error | Raised by | Meaning |
|---|---|---|
| `EmptyToken` | `pluralize`, `singularize`, and their `_into` forms | the token was `""`. Unit struct, `Copy + Eq + Default`; `Display` is `cannot inflect the empty token` |
| `PatternError` | `Rule::from_pattern`, `pattern::compile`, `pattern::translate` | the pattern used an unsupported construct, or the translated pattern was rejected by the `regex` crate. `.message()` returns the reason |

`EmptyToken` is the only failure an inflection call can produce — every non-empty
token yields a result, because the last stage of the chain returns the token
unchanged. In a pipeline, `filter_map(|w| inflector.pluralize(w).ok())` is usually
what you want; use `?` when an empty token indicates a bug upstream. A
`PatternError` from `Rule::from_pattern` is the *good* outcome for an unsupported
construct: the alternative would be a pattern that compiles and quietly matches
something else.

```rust
use verbora_inflectors::Rule;

fn main() {
    let err = Rule::from_pattern("(?=a)b", true, "x").unwrap_err();
    assert!(err.message().contains("lookahead"));

    assert!(Rule::from_pattern(r"\p{L}", true, "x").is_err()); // not translatable
    assert!(Rule::from_pattern("(.*)ing$", true, "$1ed").is_ok());
}
```

## Performance and allocation

The interesting axis is not input size — tokens are words — but **which stage
resolves the call**:

| Stage | Cost | Example |
|---|---|---|
| Caller rules | one regex attempt per added rule, always, before anything else | any token, once you have added rules |
| Invariant list | one binary search over a sorted `&'static [&str]` | `deer`, `fish`, `rhinocéros` |
| Irregular table | a failed binary search, then another over the irregular pairs | `child`, `mouse`, `foot` |
| Regular rules | a dozen or so translated regexes, first match wins — twelve for English `pluralize`, fourteen for `singularize` | `party`, `church`, `workman` |
| Fallthrough | all of the above, then the token unchanged | English `singularize("hacker")` |

English `pluralize` has a `(.*)` catch-all as its last regular rule, so it never
reaches the true fallthrough; English `singularize` ends at `s$`, so it often does.

Per call, nothing allocates until a rule actually rewrites the token: `restore_case`
returns a `Copy` enum, lowercasing borrows when the token is already lowercase
ASCII, invariant-list and irregular-table hits borrow, and a regular rule that does
not match never allocates capture slots. A rule that *does* match allocates one
`String`, and `pluralize` adds one more for the result — so `_into` removes the
**result** allocation and lets you keep capacity across a corpus, but does not make
the call allocation-free. English `pluralize` allocates two `String`s per word and
`pluralize_into` one. A freshly constructed inflector holds no heap allocation of
its own.

No inflector results are published yet; see [Benchmarks](../benchmarks/index.md).

## Concurrency

`verbora-inflectors` ships **no `par_*` API**: per-word cost measures at ~360 ns,
comparable to `rayon`'s own dispatch overhead, so a naive `par_iter` over words
would mostly measure its own scheduling. (Thirteen other Verbora crates do ship a
`par_*_batch` where the per-item cost cleared that bar — see
[Parallelism](../performance/parallelism.md).)

Inflectors are `Send + Sync` — their state is `&'static` tables plus owned `Vec`s —
and `pluralize`/`singularize` take `&self` and are pure, so sharing one instance
across threads is sound and you can parallelise yourself with `rayon` or
`std::thread::scope` in your own crate. Give each worker its own `_into` buffer, and
do all `add_*` calls (which need `&mut self`) before sharing the instance.

## Unicode and language notes

- **Case restoration is UTF-16-shaped**, as described in
  [behaviour 2](#four-behaviours-worth-knowing). This is the single
  most surprising behaviour in the crate.
- **Case mapping is the full Unicode one**, including the context-sensitive Greek
  final-sigma rule.
- **Pattern case folding uses a fixed, deliberately-chosen table**, not Unicode
  simple case folding, so `ſ`, `K`, `ı` and `ß` do not fold the way the `regex`
  crate's default Unicode mode would.
- **French** carries 744 invariant nouns (mostly `-s`, `-x`, `-z` endings) plus
  irregulars such as `œil` → `yeux` and `bijou` → `bijoux`.
- **Japanese** does not normally mark number. `pluralize` appends `たち` to anything
  via a single `^(.+)$` rule; twelve nouns instead reduplicate (`人` → `人人`), and
  `友達` and relatives are on the invariant list. `singularize` strips `たち`, `達`
  and `等` *unless* the stem is on a per-suffix exception list of words that only
  look plural (`かたち` "shape", `配達` "delivery"), and strips `共`/`ども` and
  `方`/`がた` only for an explicit allowlist of stems.

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

**Forgetting that `_into` appends.** Two calls into the same buffer give
`"catsdogs"`, not `"dogs"`. Call `buf.clear()` between items when you want a scratch
buffer.

**Assuming verbs read like nouns.** `PresentVerbInflector::singularize("go")` is
`"goes"`.

**Passing a phrase.** Rules anchor on the end of the whole string, so
`pluralize("hot dog")` is `"hot dogs"` (right, by luck) and
`pluralize("mother in law")` is `"mother in laws"` (wrong). Tokenize first.

**Expecting the empty string back for the empty token.** It is an `Err`, by design.

**Writing `Regex::new` where you meant `Rule::from_pattern`.** `Rule::new` opts you
out of Verbora's pattern semantics — `.` and case-insensitive matching both behave
differently, and the difference only shows up on inputs containing line terminators
or characters like `ſ`.

**Using the `regex` crate's replacement syntax.** `"$1s"` here means "group 1, then
the letter s"; `"${1}s"` is not special, and `"$0"` is a literal `$0` (`$&` is the
whole match).

**Adding a rule and expecting it everywhere.** Additions are per-instance and are
consulted before every built-in table — including the invariant list — so one broad
rule can shadow a lot of correct behaviour. Anchor your patterns.

## Related

- [Buffer reuse](../performance/buffer-reuse.md) and
  [Iterator vs `_into`](../performance/iterator-vs-into.md) — `pluralize_into` is
  one of the few genuine `_into` pairs in the workspace
- [Allocation](../performance/allocation.md) · [Performance](../performance/index.md) ·
  [Parallelism](../performance/parallelism.md)
- [Tokenizers](../features/tokenizers.md) — split text into the words an inflector
  expects · [Normalizers](../features/normalizers.md) — the case folding and
  diacritic handling inflection does *not* do for you
- [Stemmers](../features/stemmers.md) — the reductive direction ·
  [Core](../features/core.md) — the `Stemmer` and `Tokenizer` traits
- [Choosing an API](../choosing/index.md) · [Recipes](../recipes/index.md) ·
  [Benchmarks](../benchmarks/index.md)

## API reference

```bash
cargo doc -p verbora-inflectors --no-deps --open
```

`NounInflector`, `NounInflectorFr`, `NounInflectorJa` and `PresentVerbInflector`
have identical surfaces, and each also implements `Default` and `Debug`:

| Method | Signature |
|---|---|
| `new` | `fn() -> Self` |
| `pluralize` / `singularize` | `fn(&self, token: &str) -> Result<String, EmptyToken>` |
| `pluralize_into` / `singularize_into` | `fn(&self, token: &str, out: &mut String) -> Result<(), EmptyToken>` (appends) |
| `add_plural` / `add_singular` | `fn(&mut self, rule: Rule)` |
| `add_irregular` | `fn(&mut self, singular: &str, plural: &str)` |

`SingularPluralInflector` carries the same seven methods minus `new`.
`CountInflector` and `CountInflectorFr` are unit structs whose six methods
(`nth`, `nth_form`, `nth_f64`, `nth_form_f64`, `nth_str`, `nth_form_str`) are all
associated functions. `Rule` exposes `from_pattern`, `new` and `apply`; the `case`
module exposes `restore_case` and `CaseMode`; the `pattern` module exposes
`compile`, `translate` and `PatternError`. `restore_case`, `CaseMode` and
`PatternError` are also re-exported at the crate root.

Source: `crates/verbora-inflectors/src/`. Benchmarks:
`crates/verbora-inflectors/benches/inflectors.rs`.
