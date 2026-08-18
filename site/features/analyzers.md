# Sentence analyzers

`verbora-analyzers` operates on POS-tagged words. It marks prepositional
phrases, separates subject from predicate, and classifies statements,
questions, exclamations, and commands.

## Quick example

```rust
use verbora_analyzers::{SenType, SentenceAnalyzer, TaggedWord as Word};

fn main() {
    let mut analyzer = SentenceAnalyzer::new(vec![
        Word::new("Vote", "VB"),
        Word::new("for", "IN"),
        Word::new("me", "PRP"),
    ]);
    analyzer.part();

    assert!(analyzer.implicit_you());
    assert_eq!(analyzer.type_of(), Ok(Some(SenType::Command)));
}
```

## Input contract

The analyzer does not tokenize or tag raw text. Supply a `Vec<TaggedWord>` from
your tagger or another POS source. Tokens are borrowed when possible.

## Choosing an API

| Need | API |
|---|---|
| Mark phrases and split subject/predicate | `SentenceAnalyzer::part` |
| Classify sentence type | `SentenceAnalyzer::type_of` |
| Inspect without allocating strings | `tokens`, `subject_tokens`, `predicate_tokens` |
| Process many independent sentences | `par_analyze_batch`, with `parallel` enabled |

The analyzer is intentionally mutable. `part` annotates tags and can append an
implicit `You`; `type_of` may consume terminal punctuation. Repeating either
operation can therefore change the result.

## Related

- [Part-of-speech tagging](tagger.md)
- [Tokenizers](tokenizers.md)
- [Cargo features](../getting-started/cargo-features.md)
