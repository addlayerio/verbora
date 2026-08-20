# verbora-analyzers

Rule-based clause analysis over part-of-speech-tagged English sentences. Give it
a tagged sentence and it reports the prepositional phrases, splits subject from
predicate, and classifies the clause as declarative, interrogative, imperative
or exclamative — with the understood *you* of an imperative reported as a value
rather than spliced in as a synthetic word.

## What it expects, and what it promises

This crate has no upstream stage of its own: **it consumes tags, it does not
produce them**, so everything it can do rests on assumptions about whatever did.
Those assumptions are:

1. **The tag set is Penn Treebank** — Santorini, *Part-of-Speech Tagging
   Guidelines for the Penn Treebank Project*, 3rd revision, 1990; Marcus,
   Santorini & Marcinkiewicz, *Building a Large Annotated Corpus of English: The
   Penn Treebank*, Computational Linguistics 19(2), 1993. A tag outside that set
   takes part in no rule.
2. **One tag per word.** Ambiguity classes such as `NN|IN`, which a Brill lexicon
   carries by the hundred, are not tags and match nothing; resolve them first.
3. **The words are one clause, in order.** Nothing here finds clause boundaries.
4. **Tags are spelled exactly.** Comparison is byte-for-byte and case-sensitive:
   `"nn"` and `" NN"` are not `"NN"`.
5. **The language is English.** Every rule is a rule of English syntax. (The
   imperative test — the first word that is neither an adverb nor an
   interjection being tagged `VB` — follows Quirk, Greenbaum, Leech & Svartvik,
   *A Comprehensive Grammar of the English Language*, 1985, §11.24.)

The crate validates none of this, and that is deliberate: bad tags raise no
error, they simply match no rule, and the analysis degrades to whatever the
remaining evidence supports. When the evidence runs out entirely, the clause
type is `None` — no guess, and no `Unknown` sentinel. **Nothing rewrites your
sentence**: `analyze` borrows a `&[TaggedWord]` and returns an analysis that
borrows it too, so no token is copied, split, folded, trimmed or annotated in
place, and every index it reports is a word index into the slice you passed. The
one thing that looks inside a token is terminator recognition, which works in
Unicode scalar values: a token is terminal punctuation when it is *exactly one*
scalar drawn from a specified table.

## Example

```rust
use verbora_analyzers::{SentenceType, TaggedWord as W, analyze};

let sentence = [
    W::new("The", "DT"),
    W::new("angry", "JJ"),
    W::new("bear", "NN"),
    W::new("chased", "VBD"),
    W::new("the", "DT"),
    W::new("squirrel", "NN"),
    W::new(".", "."),
];
let analysis = analyze(&sentence);

assert_eq!(analysis.subject_to_string(), "The angry bear");
assert_eq!(analysis.predicate_to_string(), "chased the squirrel");
assert_eq!(analysis.sentence_type(), Some(SentenceType::Declarative));
// No implied subject: this clause has an overt one.
assert_eq!(analysis.implied_subject(), None);
```

## See also

- Full documentation: <https://verbora.dev/features/analyzers>
- [`verbora-tagger`](https://crates.io/crates/verbora-tagger) — the Brill tagger
  that produces the Penn Treebank tags this crate consumes.
- [`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) — sentence
  and word segmentation, for getting one clause's worth of tokens in the first
  place.
