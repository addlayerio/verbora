# verbora-inflectors

Number inflection for nouns and verbs, and ordinal numerals: `cactus` →
`cacti`, `parentheses` → `parenthesis`, `fly` → `flies`, `23` → `23rd`. Nouns
in English, French and Japanese; present-tense English verbs; English and
French ordinals. Each inflector can be extended with your own rules and
irregular pairs, which are consulted before every built-in table.

## The contract

**The text unit is one Unicode scalar value.** Case classification iterates
`char`s; nothing counts UTF-16 code units or bytes, so `"👍"` is one character.

**Every inflector method is total.** There is no input — empty, whitespace,
astral, malformed-looking — for which one returns an error or panics. A word
no rule matches comes back byte-identical, and the empty token is returned
unchanged. The only fallible operation in the crate is building a `Rule`,
which reports a `RuleError` at construction so that applying it later cannot
fail.

**Nothing silently rewrites its input.** A token whose case the crate cannot
classify as capitalised or upper-case keeps its own case exactly, so `iPhone`
pluralises to `iPhones`, not `iphones`. **No sentinels and no `NaN`** —
absence is `Option::None`, and the crate contains no floating point at all:
`OrdinalInflector::nth` takes an `i64`, because `1.5th` is not an ordinal.
The sign is orthographic and does not affect the suffix, so `nth(-1)` is
`"-1st"`, read *minus first*; the rule is exact across the whole of `i64`,
including `i64::MIN`. Output is deterministic on every platform and build:
no global mutable state, no hash-order dependence.

Every rule table states a productive pattern of its language and cites the
grammar that describes it — Quirk et al., *A Comprehensive Grammar of the
English Language*, and Huddleston & Pullum, *The Cambridge Grammar of the
English Language*, for English; Grevisse & Goosse, *Le Bon Usage*, and the
*Dictionnaire de l'Académie française*, for French; Martin, *A Reference
Grammar of Japanese*, and Shibatani, *The Languages of Japan*, for Japanese;
*The Chicago Manual of Style* and *New Hart's Rules* for the ordinals. Every
rule also carries a *witness*, a token that must reach that rule and no
earlier one, and the test suite fails if any witness is claimed by an earlier
rule.

## Example

```rust
use verbora_inflectors::{NounInflector, OrdinalInflector, PresentVerbInflector, Rule};

let nouns = NounInflector::new();
assert_eq!(nouns.pluralize("cactus"), "cacti");
assert_eq!(nouns.singularize("parentheses"), "parenthesis");

// Case is restored from the original token, never imposed on it.
assert_eq!(nouns.pluralize("iPhone"), "iPhones");

let verbs = PresentVerbInflector::new();
assert_eq!(verbs.singularize("fly"), "flies");

// Ordinals take an i64; the sign is orthographic, not arithmetic.
assert_eq!(OrdinalInflector::nth(23), "23rd");
assert_eq!(OrdinalInflector::nth(112), "112th");
assert_eq!(OrdinalInflector::nth(-1), "-1st");
assert_eq!(OrdinalInflector::suffix(22), "nd"); // allocation-free

// Your rules are consulted before every built-in table, on this instance only.
let mut custom = NounInflector::new();
custom.add_plural(Rule::new("(?i)(code|ware)$", "${1}z").unwrap());
assert_eq!(custom.pluralize("code"), "codez");
assert_eq!(custom.pluralize("bus"), "buses"); // built-ins still apply
```

## See also

Full documentation, including the four-stage pipeline every inflector runs and
when the allocation-free `*_into` forms pay for themselves:
<https://verbora.dev/features/inflectors>.

Inflection is not stemming: it produces a well-formed word of the language,
where a stemmer produces a conflation key that need not be a word. If that is
what you wanted, see `verbora-stemmers`. For case folding, accent stripping
and other normalisation, see `verbora-normalizers`; for splitting text into
the tokens you would feed an inflector, `verbora-tokenizers`.
