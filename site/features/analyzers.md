# Sentence analyzers

`verbora-analyzers` performs rule-based clause analysis over part-of-speech-tagged
English sentences. It reports the prepositional phrases, splits subject from
predicate, and classifies the clause as declarative, interrogative, imperative or
exclamative.

## Quick example

```rust
use verbora_analyzers::{ImpliedSubject, SentenceType, TaggedWord as W, analyze};

fn main() {
    let sentence = [
        W::new("Vote", "VB"),
        W::new("for", "IN"),
        W::new("me", "PRP"),
        W::new("!", "."),
    ];
    let analysis = analyze(&sentence);

    assert_eq!(analysis.sentence_type(), Some(SentenceType::Imperative));
    assert_eq!(analysis.implied_subject(), Some(ImpliedSubject::SecondPerson));
    assert_eq!(analysis.implied_subject().unwrap().pronoun(), "you");
    assert_eq!(analysis.predicate_to_string(), "Vote");
}
```

## Input contract

The analyzer does not tokenize or tag raw text: it consumes tags, it does not
produce them. Supply a `&[TaggedWord]` from your tagger or another POS source.
Four assumptions come with that, and the crate validates none of them — bad tags
raise no error, they simply match no rule and the analysis degrades to what the
remaining evidence supports:

1. **The tag set is Penn Treebank.** A tag outside it is `TagClass::Other` and
   takes part in no rule.
2. **One tag per word.** Ambiguity classes such as `NN|IN` are not tags and match
   nothing; resolve them before calling.
3. **The words are one clause, in order.** Nothing here finds clause boundaries.
4. **Tags are spelled exactly.** Comparison is byte-for-byte and case-sensitive.

The language is English, and every rule is a rule of English syntax; none
generalises to another language by relabelling tags.

## Nothing rewrites your sentence

`analyze` borrows a `&[TaggedWord]` and returns a `SentenceAnalysis` that borrows
it too. No token is copied, no word is appended, no word is removed, and no tag is
annotated in place — so analysing the same slice twice gives equal results. The
understood subject of an imperative is reported as a value, `ImpliedSubject`,
rather than inserted as a synthetic word.

## Choosing an API

| Need | API |
|---|---|
| One sentence, punctuation kept as a token | `analyze` |
| One sentence, punctuation stripped by the tokenizer | `analyze_with_terminator` |
| Which words are subject, predicate, phrase or terminator | `SentenceAnalysis::roles` / `role` |
| The prepositional phrases as index ranges | `SentenceAnalysis::prepositional_phrases` |
| Classify the clause | `SentenceAnalysis::sentence_type` |
| Inspect without allocating strings | `subject_tokens`, `predicate_tokens`, `words_with_role` |
| One `String` per side | `subject_to_string`, `predicate_to_string` |
| Process many independent sentences | `par_analyze_batch`, with `parallel` enabled |

`analyze` is the right call for most programs: it is the only one that can report
which word supplied the terminator (`terminator_index`). `analyze_with_terminator`
exists for tokenizers that discard punctuation; it never consumes a word, so
passing `None` is also how to force a trailing full stop to be analysed as an
ordinary word.

## The pipeline

Four stages, in this order — each sees the output of the ones before it, so the
order is part of the contract.

| Stage | What it decides |
|---|---|
| Terminator | The last word is the terminator when its **token** is one of the scalars `Terminator` specifies; its tag is not consulted. The rest is the body. |
| Prepositional phrases | A word tagged `IN` opens a phrase unless one is open; the first following noun closes it, inclusive. Phrases never overlap and never contain the terminator. |
| Imperative test | The body is imperative when its first word that is neither an adverb nor an interjection is tagged `VB`. Only `VB`: `MD`, `VBD`, `VBP` and `VBZ` are finite and cannot head an imperative. |
| Subject and predicate | The predicate starts at the first body word that is a verb and is **not** inside a prepositional phrase; everything before it is the subject. An imperative has no overt subject. |

Terminal punctuation then decides the clause type when it is present. With no
terminator the clause itself is the evidence: an imperative is `Imperative`; a
body starting with a wh-word or a finite verb is `Interrogative`; a tag question
is `Interrogative`. Failing all of those the type is `None` — no evidence, rather
than a guess or an `Unknown` sentinel.

**Terminator recognition works in Unicode scalar values**: a token is terminal
punctuation when it is exactly one scalar and that scalar is in `Terminator`'s
table. A token of two scalars is not a terminator, and neither is a token that
merely contains one. Everything else is analysed in whole tagged words, and every
index this crate reports is a word index into the sentence you supplied.

## What it does not do

It does not tokenize, tag, find clause or sentence boundaries, resolve tag
ambiguity, parse a constituency or dependency tree, or handle any language but
English. A prepositional phrase here is the flat `IN … noun` span above, not a
nested constituent, and attachment is never resolved.

`analyze` is a constant number of linear passes over the sentence, none nested,
with three allocations for a sentence that contains a prepositional phrase and
two for one that does not: a `Vec<bool>` of `body.len()` marking which body
words fall inside a phrase, a growable `Vec<Range<usize>>` of the phrases
themselves — not allocated at all when there are none — and one `Vec<Role>` of
exactly `sentence.len()` elements. An empty sentence allocates nothing.
`benches/analyzers.rs` measures the pipeline, but no run exists against it, so
this crate publishes no timing figures.

## Related

- [Part-of-speech tagging](tagger.md)
- [Tokenizers](tokenizers.md)
- [Cargo features](../getting-started/cargo-features.md)
