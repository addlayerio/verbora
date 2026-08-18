# Part-of-speech tagging

`verbora-tagger` implements Brill part-of-speech tagging: a lexicon assigns an
initial tag and context-sensitive transformation rules correct it. English and
Dutch lexicons and rules are embedded at build time.

## Quick example

```rust
use verbora_tagger::{BrillPosTagger, Language, Lexicon, RuleSet};

fn main() {
    let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
    let rules = RuleSet::for_language(Language::English);
    let tagger = BrillPosTagger::new(&lexicon, &rules);
    let sentence = tagger.tag(["I", "would", "book", "a", "flight"]).unwrap();
    assert_eq!(sentence.len(), 5);
}
```

## Choosing an API

| Need | API |
|---|---|
| Tag one owned or borrowed token sequence | `BrillPosTagger::tag` |
| Consume results lazily | `BrillPosTagger::tag_iter` |
| Tag many documents | `par_tag_batch`, with `parallel` enabled |
| Evaluate a tagger | `BrillPosTester` |
| Learn transformation rules | `BrillPosTrainer` |

Use `Lexicon::detached` when mutations must stay local: language-default
lexicons share one mutable dictionary per language, so `add_word` on one
instance is visible to every other lexicon of that language.

## Bundled data and behavior

The bundled English lexicon contains 92,662 entries and 18 rules; Dutch
contains 11,699 entries and 285 rules. Exact tags and rule-template semantics
are part of the crate's tested behavior. Positions used by rules follow UTF-16
code-unit semantics.

## Related

- [Sentence analyzers](analyzers.md)
- [Tokenizers](tokenizers.md)
- [Cargo features](../getting-started/cargo-features.md)
