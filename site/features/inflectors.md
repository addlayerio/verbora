# Inflectors

`verbora-inflectors` turns a word into another form of itself: `octopus` into
`octopi`, `parenthesis` into `parentheses`, `cheval` into `chevaux`, `go` into
`goes`, and `23` into `23rd`. Every rule and every edge case — down to
`pluralize("A")` returning `"As"` — is pinned by the regression suite, so output is
exact and stable rather than approximate.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>6</strong> inflector APIs are
documented and test-pinned, tables included.
<code>cargo test -p verbora-inflectors</code> runs <strong>68</strong> tests
and <strong>12</strong> doctests.
</div>

## The six public types

| Type | Language | Job |
|---|---|---|
| `NounInflector` | English | Noun singular ⇄ plural |
| `NounInflectorFr` | French | Noun singular ⇄ plural, 595 invariant nouns |
| `NounInflectorJa` | Japanese | Appends/strips `たち`, `達`, `等`, `共`, `方`; reduplicates a short irregular list |
| `PresentVerbInflector` | English | Present tense: base form ⇄ third-person singular |
| `OrdinalInflector` | English | Ordinal numerals (`1st`, `2nd`, `3rd`, `11th`) |
| `OrdinalInflectorFr` | French | Ordinal numerals (`1er`, `1re`, everything else `e`) |

The first four share one engine and one API shape — they all implement
[`SingularPluralInflector`](#the-singularpluralinflector-trait). The two `Ordinal*`
types share nothing with them: they are stateless, so every method is an associated
function and there is no instance to construct.

Every inflection call is **total**. There is no input — empty, astral, or
malformed-looking — for which an inflector returns an error or panics; a token no
rule claims comes back byte-identical, and the empty token has no inflected form and
is returned unchanged. The only fallible operation in the crate is
[building a rule](#building-a-rule).

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
- **Case folding.** Inflectors carry the input's own case onto their output rather
  than normalising it, so the case behaviour in
  [Case classification](#four-behaviours-worth-knowing) applies. For a
  case-folded index, normalise first — see
  [Normalizers](../features/normalizers.md).

## Quick example

```rust
use verbora_inflectors::{NounInflector, OrdinalInflector, PresentVerbInflector};

fn main() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("octopus"), "octopi");
    assert_eq!(nouns.singularize("parentheses"), "parenthesis");

    let verbs = PresentVerbInflector::new();
    assert_eq!(verbs.singularize("go"), "goes");
    assert_eq!(verbs.pluralize("catches"), "catch");

    assert_eq!(OrdinalInflector::nth(23), "23rd");
}
```

`new()` is cheap. The rule tables — every compiled regex, both irregular maps and
the invariant list — are built once per process behind a `LazyLock` and shared by
every instance, so constructing an inflector copies a couple of pointers and
allocates nothing until you add a rule. `NounInflectorFr::new()` costs the same as
`NounInflector::new()` despite French's 595-entry invariant list.

## Choosing the right API

### Nouns and verbs

| API | Returns | Result allocation | Buffer reuse | Best for |
|---|---|:--:|:--:|---|
| `pluralize` / `singularize` | `String` | one `String` | ❌ | one-off calls, readable code |
| `pluralize_into` / `singularize_into` | `()` | none — appends to yours | ✅ | loops over a corpus |
| `SingularPluralInflector` (trait) | the same four methods | as above | as above | generic or dynamically dispatched code |

There is no batch API and no parallel API — see [Concurrency](#concurrency).

### Ordinals

| API | Argument | Returns | Allocations | Behaviour |
|---|---|---|---|---|
| `suffix(i64)` | `i64` | `&'static str` | none, ever | The two-letter suffix alone, one of exactly four values |
| `nth(i64)` | `i64` | `String` | one, `with_capacity(24)` | Numeral plus suffix; exact across the full `i64` range |
| `nth_into(i64, &mut String)` | `i64` | `()` | none — appends to yours | Numeral plus suffix, into a buffer you own |

Ordinals are defined for integers, so the argument is an `i64` and nothing else.
There is no floating-point entry point — `1.5th` is not an ordinal — and no string
entry point, which is what keeps `nth("abc")` from being a question the API can be
asked.

[`OrdinalInflectorFr`](#ordinalinflectorfr) exposes the same three names, each
taking an extra `Gender` argument; only the rule differs.

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
        inflector.pluralize_into(word, &mut scratch);
    }
    assert_eq!(scratch, "deer");

    // Accumulator pattern: appending is the point.
    let mut line = String::with_capacity(64);
    for word in ["box", "party", "deer"] {
        inflector.pluralize_into(word, &mut line);
    }
    assert_eq!(line, "boxespartiesdeer");

    // The empty token has no inflected form, so nothing is appended for it and
    // an accumulator cannot be corrupted by one.
    inflector.pluralize_into("", &mut line);
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
    words.iter().map(|w| inflector.pluralize(w)).collect()
}

fn main() {
    assert_eq!(
        plural_column(&NounInflector::new(), &["party", "box"]),
        ["parties", "boxes"]
    );

    let by_lang: Vec<Box<dyn SingularPluralInflector>> = vec![
        Box::new(NounInflector::new()),
        Box::new(NounInflectorFr::new()),
    ];
    assert_eq!(by_lang[0].pluralize("child"), "children");
    assert_eq!(by_lang[1].pluralize("cheval"), "chevaux");
}
```

The trait carries `pluralize`, `singularize`, `pluralize_into`, `singularize_into`,
`add_plural`, `add_singular` and `add_irregular`. It does **not** carry `new()`, so
construct the concrete type and then erase it. The `Ordinal*` types are not part of
it.

### `OrdinalInflector`

The suffix is chosen from the **last two decimal digits of the magnitude**:

| Last two digits | Suffix |
|---|---|
| `11`, `12`, `13` | `th` |
| otherwise, last digit `1` | `st` |
| otherwise, last digit `2` | `nd` |
| otherwise, last digit `3` | `rd` |
| otherwise | `th` |

This is the form given by *The Chicago Manual of Style* and by *New Hart's Rules*:
`21st` and `102nd`, but `111th`, `112th` and `113th`, because the teens are
pronounced *eleventh*, *twelfth*, *thirteenth*.

```rust
use verbora_inflectors::OrdinalInflector;

fn main() {
    assert_eq!(OrdinalInflector::nth(1), "1st");
    assert_eq!(OrdinalInflector::nth(21), "21st");
    assert_eq!(OrdinalInflector::nth(112), "112th"); // the teens exception
    assert_eq!(OrdinalInflector::nth(1013), "1013th");
    assert_eq!(OrdinalInflector::suffix(21), "st");

    // The sign is orthographic: the magnitude decides the suffix.
    assert_eq!(OrdinalInflector::nth(-1), "-1st");
    assert_eq!(OrdinalInflector::nth(-11), "-11th");

    // Exact across the whole of i64, i64::MIN included.
    assert_eq!(OrdinalInflector::nth(i64::MIN), "-9223372036854775808th");
    assert_eq!(
        OrdinalInflector::nth(9_007_199_254_740_993),
        "9007199254740993rd"
    );
}
```

Two behaviours worth knowing:

- **The sign does not change the suffix.** `nth(-1)` is `"-1st"`, read *minus
  first*, not `"-1th"`. The suffix is taken from the magnitude, so a negative
  ordinal reads the way it is said.
- **The whole `i64` range is exact.** The magnitude is computed with
  `unsigned_abs`, which is why `i64::MIN` — the one value with no positive
  counterpart — is total rather than an overflow.

`suffix` returns a `&'static str` that is one of exactly four values and allocates
nothing; `nth` allocates one `String` with the numeral and suffix already joined,
sized once at 24 bytes so it is never resized. If you are building output with
`write!` anyway, that `String` is pure waste — reach for `nth` when the ordinal is
the whole value you want, and `suffix` when it is one field in a larger string, or
when the numeral is formatted elsewhere with thousands separators.

### `OrdinalInflectorFr`

French marks only *premier*/*première*. Every other ordinal is formed with *-ième*
and abbreviated `e`, whatever the gender and whatever the number — `2e`, `3e`,
`21e`, `100e` — the abbreviation prescribed by *Le Bon Usage* and by the
*Dictionnaire de l'Académie française*. Note that `21` is `21e`, not `21er`: *vingt
et unième* is an `-ième` form, so the suffix depends on the whole number being
exactly one in magnitude, not on its last digit.

`Gender` is a `Copy` enum of `Masculine` and `Feminine`, and defaults to
`Masculine`. It exists for French alone, since English ordinals do not agree with
their noun.

```rust
use std::fmt::Write;

use verbora_inflectors::{Gender, OrdinalInflector, OrdinalInflectorFr};

fn main() {
    // One buffer, no per-item String.
    let mut line = String::new();
    for i in 1..=3i64 {
        write!(line, "{i}{} ", OrdinalInflector::suffix(i)).unwrap();
    }
    assert_eq!(line, "1st 2nd 3rd ");

    assert_eq!(OrdinalInflectorFr::nth(1, Gender::Masculine), "1er");
    assert_eq!(OrdinalInflectorFr::nth(1, Gender::Feminine), "1re");
    assert_eq!(OrdinalInflectorFr::nth(2, Gender::Feminine), "2e");
    assert_eq!(OrdinalInflectorFr::nth(21, Gender::Masculine), "21e");
    assert_eq!(OrdinalInflectorFr::nth(-1, Gender::Masculine), "-1er");
    assert_eq!(Gender::default(), Gender::Masculine);
}
```

Both `nth_into` forms **append** to a buffer you own and never clear it, exactly
like the inflectors' `_into` methods:

```rust
use verbora_inflectors::{Gender, OrdinalInflector, OrdinalInflectorFr};

fn main() {
    let mut buf = String::from("the ");
    OrdinalInflector::nth_into(3, &mut buf);
    buf.push_str(" of ");
    OrdinalInflectorFr::nth_into(1, Gender::Feminine, &mut buf);
    assert_eq!(buf, "the 3rd of 1re");
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
    inflector.add_plural(Rule::new("(?i)(code|ware)$", "${1}z").unwrap());
    inflector.add_singular(Rule::new("(?i)(code|ware)z$", "${1}").unwrap());
    inflector.add_irregular("gizmo", "gizmoi");

    assert_eq!(inflector.pluralize("code"), "codez");
    assert_eq!(inflector.singularize("warez"), "ware");
    assert_eq!(inflector.pluralize("gizmo"), "gizmoi");
    // Every built-in rule still applies to everything else.
    assert_eq!(inflector.pluralize("bus"), "buses");

    // Additions are strictly per-instance.
    assert_eq!(NounInflector::new().pluralize("code"), "codes");
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
worth staring at. With `add_plural(Rule::new("(?i)o", "FIRST"))`, `pluralize("dog")`
is `"dFIRSTg"`.

### Building a rule

Two constructors, both returning `Result<Rule, RuleError>`:

| Constructor | Signature | Does |
|---|---|---|
| `Rule::new` | `fn(&str, impl Into<String>) -> Result<Rule, RuleError>` | rewrites the first match with a replacement template |
| `Rule::keep` | `fn(&str) -> Result<Rule, RuleError>` | matches and leaves the token alone |

A `keep` rule is how an ordered list says "this shape is already the form we were
asked for, stop scanning" — `dresses` must not become `dresseses`, so a guard for
the already-plural shape precedes the sibilant rule.

**Patterns are ordinary [`regex`](https://docs.rs/regex) crate patterns**, with that
crate's semantics in full: leftmost-first alternation, `$` matching only at the end
of the token, and inline flags such as `(?i)`. Every built-in rule that must ignore
case spells `(?i)` in its own pattern rather than inheriting a flag. Lookaround and
backreferences do not exist in the `regex` crate and so cannot be used; where a rule
needs "except these words", the exception is a separate rule placed *before* it (see
[behaviour 4](#four-behaviours-worth-knowing)).

**Replacements use brace-delimited group references**: `${0}` for the whole match,
`${1}`, `${2}`, … for numbered groups, `${name}` for a named group, and `$$` for a
literal `$`. A bare `$1` is **rejected at construction**, not accepted — in the
`regex` crate `"$1s"` names the group `1s`, so a template written the way it would
be in `sed` would silently expand to nothing and delete the suffix. Requiring the
braces turns that silent wrong answer into a `RuleError`. Group names the pattern
does not declare are refused for the same reason.

```rust
use verbora_inflectors::{Rule, RuleError};

fn main() {
    let rule = Rule::new("(?i)(x|ch|ss|sh|s|z)$", "${1}es").unwrap();
    assert_eq!(rule.apply("church").as_deref(), Some("churches"));
    assert_eq!(rule.apply("cat"), None);

    let guard = Rule::keep("(?i)(ses|xes|zes|ches|shes)$").unwrap();
    assert_eq!(guard.apply("dresses").as_deref(), Some("dresses"));
    assert_eq!(guard.apply("dress"), None);

    // A bare group reference is refused rather than silently expanded to "".
    assert!(matches!(
        Rule::new("(a)", "$1s"),
        Err(RuleError::BareGroupReference { offset: 0 })
    ));
    assert!(Rule::new("(a)", "${2}").is_err());   // no such group
    assert!(Rule::new("(unclosed", "x").is_err()); // no such pattern
}
```

`apply(&self, token: &str) -> Option<Cow<'_, str>>` tests a rule in isolation. It
returns `None` both when the pattern does not match and when it matches but the
rewrite would empty a non-empty token (see
[behaviour 1](#four-behaviours-worth-knowing)); a `keep` rule that matches borrows
the token rather than copying it. `Rule::pattern()` gives the pattern back as
written. `Rule` is `Debug` but not `Clone`; to give two inflectors the same rule,
build it twice.

### `CaseMode`

Case classification is public because it is useful on its own — it is how you
reproduce the crate's case handling outside it.

| Item | Signature | Notes |
|---|---|---|
| `CaseMode` | `enum { Preserve, Title, Upper }` | `Copy`, `Eq`, `Hash`, `Debug`, `Display`; defaults to `Preserve` |
| `CaseMode::of` | `fn(&str) -> CaseMode` | total — every string, `""` included, has a mode |
| `CaseMode::apply` | `fn(self, &str) -> String` | allocates `with_capacity(s.len() + 2)` |
| `CaseMode::apply_into` | `fn(self, &str, &mut String)` | **appends**, like the inflectors' `_into` |

```rust
use verbora_inflectors::CaseMode;

fn main() {
    assert_eq!(CaseMode::of("word"), CaseMode::Preserve);
    assert_eq!(CaseMode::of("Word"), CaseMode::Title);
    assert_eq!(CaseMode::of("WORD"), CaseMode::Upper);
    assert_eq!(CaseMode::of("A"), CaseMode::Title);
    assert_eq!(CaseMode::of("iPhone"), CaseMode::Preserve);
    assert_eq!(CaseMode::of("👍"), CaseMode::Preserve);
    assert_eq!(CaseMode::of(""), CaseMode::Preserve);

    assert_eq!(CaseMode::Title.apply("children"), "Children");
    // The uppercase mapping of U+00DF is two characters.
    assert_eq!(CaseMode::Upper.apply("ßs"), "SSS");

    let mut buf = String::from("<");
    CaseMode::Title.apply_into("child", &mut buf);
    assert_eq!(buf, "<Child");
}
```

## Four behaviours worth knowing

Everything interesting about this crate follows from one fallback chain: caller
rules, then the invariant list, then the irregular table, then the built-in regular
rules, then the token unchanged — each stage tried in order, first usable result
wins. Case is classified from the *original* token and applied to whichever stage
won, and a stage that returns the token unchanged skips restoration entirely, so it
cannot rewrite anything.

### 1. An empty rewrite counts as no match

A stage that genuinely matches but rewrites the token to the empty string is
discarded, and the chain falls through — ultimately to the *unchanged* token. An
inflected form of a word is a word, and a rule that erases its input has not
produced one.

| Call | Rule that fires | Result |
|---|---|---|
| `PresentVerbInflector::pluralize("Es")` | `(?i)e?s$ → ""` | `"Es"` |
| `PresentVerbInflector::pluralize("s")` | `(?i)e?s$ → ""` | `"s"` |
| `NounInflectorFr::singularize("S")` | `(?i)s$ → ""` | `"S"` |
| `pluralize("cat")` after `add_plural(Rule::new("^cat$", ""))` | the caller's rule | `"cats"` |

Your own rules are subject to the same fallthrough, which is why `Rule::apply`
keeps "matched" and "produced something usable" as separate signals: a rewrite that
would empty a non-empty token returns `None`, and the next stage is consulted.

### 2. Case classification counts cased characters, and never rewrites the rest

One Unicode scalar value is one character: classification iterates `char`s, never
UTF-16 code units and never bytes, so `"👍"` is exactly one character. A character
counts as **cased** when `char::is_uppercase` or `char::is_lowercase` holds — the
Unicode `Uppercase` and `Lowercase` derived properties. Digits, punctuation,
symbols, emoji and every CJK ideograph are uncased, and uncased characters are
ignored entirely when choosing a mode.

Applied to the token in order, first match winning:

| Condition on the token's cased characters | Mode |
|---|---|
| none at all | `Preserve` |
| two or more, every one uppercase | `Upper` |
| the first uppercase, every later one lowercase | `Title` |
| anything else | `Preserve` |

The "two or more" clause is why `"A"` is `Title` rather than `Upper`: one uppercase
letter is indistinguishable from a capitalised word, and treating it as a shout
would pluralise `"A"` to `"AS"`. The final row is why the crate never silently
re-cases its input — `"iPhone"`, `"McDonald"` and `"aBC"` are all `Preserve`, so
their interior capitals survive.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // One uppercase letter is Title, not Upper.
    assert_eq!(nouns.pluralize("A"), "As");
    // Uncased characters select nothing, so the form is emitted as produced.
    assert_eq!(nouns.pluralize("👍"), "👍s");
    assert_eq!(nouns.pluralize("1"), "1s");
    // U+00DF is Lowercase, so it is cased and starts a lowercase word.
    assert_eq!(nouns.pluralize("ß"), "ßs");
    // Upper is carried onto the whole produced form.
    assert_eq!(nouns.pluralize("CHURCH"), "CHURCHES");
    // Interior capitals are never rewritten.
    assert_eq!(nouns.pluralize("iPhone"), "iPhones");
    // An invariant word is returned verbatim, whatever its case.
    assert_eq!(nouns.pluralize("DEER"), "DEER");
    assert_eq!(nouns.pluralize("dEer"), "dEer");
}
```

### 3. Case-insensitive matching is Unicode simple case folding

`(?i)` in a rule means what it means in the `regex` crate: **Unicode simple case
folding** (UTS #18 §1.5, from the UCD `CaseFolding.txt` `C` and `S` mappings). So
`(?i)s$` matches `U+017F ſ`, and a word ending in the long s takes the sibilant
plural exactly as one ending in a plain `s` does.

The `$` anchor is equally literal: it matches only at the end of the token, so an
embedded line terminator is carried through untouched rather than cutting the match
short.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    // `(?i)(s|x|z|ch|sh)$` folds U+017F into `s`, so both take `-es`.
    assert_eq!(nouns.pluralize("ma\u{17f}"), "ma\u{17f}es");
    assert_eq!(nouns.pluralize("mas"), "mases");
    // `$` anchors at the end of the token, newline or no newline.
    assert_eq!(nouns.pluralize("ab\rcd"), "ab\rcds");
}
```

### 4. Exceptions are earlier rules, not lookahead

Lookaround does not exist in the `regex` crate, so a rule that needs "except these
words" is expressed as a separate rule placed *before* it. The English plural table
spells the `-man` exception as a closed list —
`(?i)(caiman|cayman|desman|…|shaman|talisman)$ → ${1}s` — sitting immediately
before `(?i)man$ → men`. Those nouns end in the same three letters for unrelated
reasons and are regular, so they claim the token before the mutation rule can.

The exception list is anchored at the end but not at the start, so it also claims
any word that *ends* in one of its members.

```rust
use verbora_inflectors::NounInflector;

fn main() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("workman"), "workmen");
    assert_eq!(nouns.pluralize("human"), "humans");
    assert_eq!(nouns.pluralize("talisman"), "talismans");
    // `$` anchors, so the LAST `man` is the one consumed.
    assert_eq!(nouns.pluralize("manman"), "manmen");
    // The exception list is a suffix match, so this is regular too.
    assert_eq!(nouns.pluralize("xtalisman"), "xtalismans");
}
```

## Error handling

**Inflection cannot fail.** No `pluralize`, `singularize` or `_into` call returns a
`Result`: every input yields a form, because the last stage of the chain returns the
token unchanged, and the empty token is returned as it is. There is nothing to
handle in a pipeline — `words.iter().map(|w| inflector.pluralize(w))` is the whole
of it.

The one error type is `RuleError`, raised only at rule construction, so that
applying a rule cannot fail later. It is `Debug + Clone + Eq + Display +
std::error::Error`, and `#[non_exhaustive]`.

| Variant | Meaning |
|---|---|
| `Pattern { pattern, message }` | the pattern is not valid `regex`-crate syntax; `message` is the compiler's own diagnostic |
| `BareGroupReference { offset }` | the replacement has a `$` group reference without braces, such as `$1`. Write `${1}` |
| `UnterminatedGroupReference { offset }` | the replacement has a `${` with no closing `}` |
| `UnknownGroup { name }` | the replacement refers to a group the pattern does not declare |

Refusing at construction is the *good* outcome in each case: the alternative is a
template that compiles and quietly expands to something else, deleting a suffix
without any diagnostic.

```rust
use verbora_inflectors::Rule;

fn main() {
    // Lookaround does not exist in the `regex` crate.
    assert!(Rule::new("(?=a)b", "x").is_err());
    // A bare group reference would name the group `1ed`.
    assert!(Rule::new("(.*)ing$", "$1ed").is_err());
    // Braced, it is accepted.
    assert!(Rule::new("(.*)ing$", "${1}ed").is_ok());

    let err = Rule::new("(unclosed", "x").unwrap_err();
    assert!(err.to_string().contains("did not compile"));
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
| Regular rules | an ordered list of compiled regexes, first match wins — eighteen for English `pluralize`, twenty-one for `singularize` | `party`, `church`, `workman` |
| Fallthrough | all of the above, then the token unchanged | English `singularize("hacker")` |

English `pluralize` ends with a `$ → s` catch-all that every token reaches, so it
never falls through; English `singularize` ends at `s$`, so it often does.

Per call, nothing allocates until a rule actually rewrites the token: `CaseMode::of`
returns a `Copy` enum, lowercasing borrows when the token is already lowercase
ASCII, invariant-list and irregular-table hits borrow, and a regular rule that does
not match never allocates capture slots — each rule tests `is_match` before asking
for captures, so the misses ahead of the winner stay allocation-free. A rule that
*does* match allocates one `String`, and `pluralize` adds one more for the result —
so `_into` removes the **result** allocation and lets you keep capacity across a
corpus, but does not make the call allocation-free. English `pluralize` allocates
two `String`s per word and `pluralize_into` one. A freshly constructed inflector
holds no heap allocation of its own.

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

- **One Unicode scalar value is one character**, as described in
  [behaviour 2](#four-behaviours-worth-knowing). Nothing in the crate counts UTF-16
  code units or bytes.
- **Case mapping is the full Unicode one, applied per character**, so it can
  lengthen the string: uppercasing `ß` gives `SS`. Both Greek sigmas map to `Σ`.
- **Pattern case folding is Unicode simple case folding**, the `regex` crate's own,
  so `(?i)s` matches `ſ` and `(?i)k` matches `K` (U+212A).
- **French** carries 595 invariant nouns, almost all in `-s` (`abus`, `chassis`)
  plus a smaller set in `-aux`/`-eaux`/`-eux` (`faux`, `vieux`) that the singular
  rules would otherwise mis-rewrite; plain `-x` and `-z` endings (`afflux`,
  `quartz`) are invariant by rule rather than by lexical entry. Irregulars such
  as `œil` → `yeux` and `bijou` → `bijoux` are a separate table.
- **Japanese** does not normally mark number. `pluralize` appends `たち` to anything
  via a single `$ → たち` rule; twelve nouns instead reduplicate with the iteration
  mark (`人` → `人々`), and `友達` and its compounds are on the invariant list.
  `singularize` strips `たち`, `達` and `等` *unless* the whole word is on the
  invariant list of words that only look plural (`かたち` "shape", `配達`
  "delivery", `平等` "equality"), and strips `共`/`ども` and `方`/`がた` only for an
  explicit allowlist of stems.

```rust
use verbora_inflectors::NounInflectorJa;

fn main() {
    let ja = NounInflectorJa::new();
    assert_eq!(ja.pluralize("私"), "私たち");
    assert_eq!(ja.singularize("私たち"), "私");
    assert_eq!(ja.pluralize("人"), "人々");   // irregular reduplication
    assert_eq!(ja.pluralize("友達"), "友達"); // invariant
    assert_eq!(ja.singularize("かたち"), "かたち"); // only looks plural
    assert_eq!(ja.singularize("野郎共"), "野郎"); // allowlisted stem
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

**Reaching for a `Result`.** Inflection is total. `pluralize` returns a `String`,
not a `Result<String, _>`, and the empty token comes back as the empty string.

**Writing a bare group reference.** `"$1s"` is refused at construction, because the
`regex` crate would read it as a group *named* `1s` and expand it to nothing. Write
`"${1}s"`. `${0}` is the whole match, and `$$` is a literal `$`.

**Expecting a lookahead to work.** The `regex` crate has none. Express the
exception as its own rule, added before the general one — insertion order decides.

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
| `pluralize` / `singularize` | `fn(&self, token: &str) -> String` |
| `pluralize_into` / `singularize_into` | `fn(&self, token: &str, out: &mut String)` (appends) |
| `add_plural` / `add_singular` | `fn(&mut self, rule: Rule)` |
| `add_irregular` | `fn(&mut self, singular: &str, plural: &str)` |

`SingularPluralInflector` carries the same seven methods minus `new`.
`OrdinalInflector` and `OrdinalInflectorFr` are unit structs whose three methods
(`suffix`, `nth`, `nth_into`) are all associated functions; the French three take an
extra `Gender`. `Rule` exposes `new`, `keep`, `pattern` and `apply`, and reports
`RuleError`. `CaseMode` exposes `of`, `apply` and `apply_into`.

Every module is private and the crate root is the entire public surface:
`CaseMode`, `Gender`, `NounInflector`, `NounInflectorFr`, `NounInflectorJa`,
`OrdinalInflector`, `OrdinalInflectorFr`, `PresentVerbInflector`, `Rule`,
`RuleError` and `SingularPluralInflector`.

Source: `crates/verbora-inflectors/src/`. Benchmarks:
`crates/verbora-inflectors/benches/inflectors.rs`.
