# verbora-tagger

A Brill transformation-based part-of-speech tagger. Every token is first given
its most frequent tag from a lexicon, then an ordered list of contextual rules
rewrites the tags that the surrounding words contradict. English and Dutch
lexicons and rule sets ship with the crate, packed at build time and read in
place from the executable — constructing a `Lexicon` parses nothing and
allocates nothing. A `Trainer` learns a new rule set from your own annotated
corpus.

## What it expects, and what it promises

**This crate never tokenizes.** It takes tokens you already have, and a token is
simply a non-empty string containing no Unicode `White_Space` scalar. That
matters more than it sounds: the bundled dictionaries are keyed by
*whitespace-delimited corpus tokens*, so `well-known` and `A.A.U.` are single
keys, and a tokenizer that splits inside them cannot reach those entries at all
— measured at 15,543 of 92,538 English keys (16.8%) for a UAX #29 word
tokenizer. Split on whitespace to reach the bundled lexicon as it is keyed; use
a UAX #29 tokenizer only with a lexicon keyed to match.

Nothing rewrites a token: no case folding, no trimming, no normalisation, and
tokens come out byte-identical to the ones that went in. **Tagging cannot
fail** — `BrillTagger::tag` returns a value, not a `Result`; every condition is
total, every out-of-range position simply does not match, and every token gets a
tag.

What is fallible is building the inputs, and there are five such operations:
parsing a `Rule` or a `RuleSet` from text, `Corpus::parse_brown`,
`Corpus::build_lexicon`, `Lexicon::insert`, and constructing a `Tag` or a `Word`
with `new`, `FromStr` or `TryFrom<&'static str>`. Only the first two are
parsing; the rest are validation. Each reports precisely what it rejected rather
than repairing it. Note in particular that `Tag::new("*")` is an error — the
rule language spells its wildcard pattern that way, so a tag named `*` could be
written into a rule but never read back out of one. `Word::new("*")` is fine,
because there `*` is an ordinary token.

The algorithm is Eric Brill's, and the implementation follows the papers:

- *A Simple Rule-Based Part of Speech Tagger*, ANLP-92, 152–155 — the contextual
  rule templates.
- *Some Advances in Transformation-Based Part of Speech Tagging*, AAAI-94 — the
  lexicalised templates.
- *Transformation-Based Error-Driven Learning and Natural Language Processing: A
  Case Study in Part-of-Speech Tagging*, Computational Linguistics 21(4), 1995,
  543–565, §2 — initial-state annotation and the error-driven training
  procedure.

## Example

```rust
use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet};

let lexicon = Lexicon::bundled(Language::English);
let rules = RuleSet::bundled(Language::English);
let tagger = BrillTagger::new(&lexicon, &rules);

// The caller supplies the tokens; this crate does not produce them.
let tagged = tagger.tag("would book a flight".split(' '));
let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();

// `book` is a noun in the lexicon; the rule `NN VB PREV-WORD-IS would`
// is what makes it a verb here.
assert_eq!(tags, ["MD", "VB", "DT", "NN"]);
```

## See also

- Full documentation: <https://verbora.dev/features/tagger>
- [`verbora-analyzers`](https://crates.io/crates/verbora-analyzers) — what to do
  with the tags: prepositional phrases, subject/predicate split and clause type
  over a Penn Treebank tagged sentence.
- [`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) — UAX #29
  word, segment and sentence tokenizers, subject to the key-reachability caveat
  above.
- [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers) — if what you
  actually wanted was to reduce a word to a stem, not to label its part of
  speech.
