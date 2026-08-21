# Part-of-speech tagging

`verbora-tagger` implements Brill transformation-based part-of-speech tagging: a
lexicon assigns every token its most frequent tag, then context-sensitive
transformation rules correct it. English and Dutch lexicons and rule sets are
packed at build time and read in place from the executable.

## Quick example

```rust
use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet};

fn main() {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);

    let tagged = tagger.tag("would book a flight".split(' '));
    let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
    assert_eq!(tags, ["MD", "VB", "DT", "NN"]);
}
```

`book` is a noun in the lexicon; the rule `NN VB PREV-WORD-IS would` is what
makes it a verb here.

## Tagging cannot fail

`tag` returns a value, not a `Result`. Every condition is total, an out-of-range
position simply does not match, and every token gets a tag — the lexicon's entry,
or its default, or its capitalised default.

What is fallible is building the inputs, and each of those reports precisely what
it rejected rather than repairing it:

| Operation | Rejects |
|---|---|
| `Rule` and `RuleSet` from text, via `FromStr` or `RuleSet::parse_lines` | too few fields, an unknown condition name, the wrong argument count for the condition named, a field that is not a valid tag or word, and a boolean argument that is neither `YES` nor `NO` |
| `Corpus::parse_brown` | a `token_TAG` pair with no tag, an empty token, an empty tag, or the wildcard tag `*` |
| `Corpus::build_lexicon` | a corpus token that is not a conforming lexicon key — which `from_sentences` can produce and `parse_brown` cannot |
| `Lexicon::insert` | an empty key, a key containing whitespace, or an empty tag list |
| `Tag::new` and `Word::new` | the empty string, and any `White_Space` scalar |

`Tag::new` additionally rejects `"*"`: the rule language spells its wildcard that
way, so a tag named `*` could not be written in a rule. `Word::new("*")` is
accepted, because there `*` is an ordinary token.

## The token contract

**This crate never tokenizes.** A token is a non-empty string containing no
`White_Space` scalar, and what produced it is the caller's decision. That matters
more than it sounds: the bundled dictionaries are keyed by whitespace-delimited
corpus tokens, so `well-known` and `A.A.U.` are single keys, and a tokenizer that
splits inside them cannot reach them.

| Bundled lexicon | Keys | Never emitted whole by a UAX #29 word tokenizer |
|---|---:|---:|
| English | 92,538 | 15,543 (16.8%) |
| Dutch | 11,699 | 313 (2.7%) |

U+002D HYPHEN-MINUS is `Word_Break=Other`, which alone accounts for 14,417 of the
English figure. The guidance follows from the numbers: with the bundled
dictionaries, split on whitespace — `str::split_whitespace` yields exactly
conforming tokens and reaches every key. With a UAX #29 tokenizer, build the
lexicon from a corpus tokenized the same way, with `Corpus::build_lexicon`, and
every key is then reachable by construction.

Nothing here rewrites a token: case folding, trimming and normalisation are the
caller's explicit choice, and tokens come out byte-identical to the ones that went
in.

## Choosing an API

| Need | API |
|---|---|
| Tag one token sequence | `BrillTagger::tag` |
| Tag into a buffer you own | `BrillTagger::tag_into` |
| Stream a document in bounded memory | `BrillTagger::tag_stream` |
| Initial-state tags only, no rules | `BrillTagger::annotate` |
| Apply rules to tokens already tagged | `BrillTagger::transform` |
| Tag many independent documents | `BrillTagger::par_tag_batch`, with `parallel` enabled |
| Score a tagger against an annotated corpus | `BrillTagger::evaluate` → `Evaluation` |
| Learn transformation rules | `Trainer` → `Training` |

`tag_stream` yields the same tags a whole-document `tag` would, element for
element, holding `context_span.0 + 1024 + context_span.1` tokens at a time and
no more, whatever the document length: it finalises 1024 positions per refill
with exactly enough context on each side for those positions to come out
identical to a whole-document run. `RuleSet::context_span` is a property of the
rule set, not of the input — `(4, 0)` for the bundled English rules. It is the
tool when memory, not throughput, is the constraint; if the document already
fits in memory, `tag` does strictly less work.

`Evaluation` reports counts, not percentages: `tokens`, `correct_before_rules`
and `correct_after_rules`. `accuracy` and `accuracy_before_rules` divide them and
return `None` for an empty corpus rather than inventing a value or yielding
`NaN`.

## Lexicons own their entries

`Lexicon::bundled(language)` starts from the packed dictionary — no parse step,
no allocation, no lazily initialised table — and `Lexicon::new(default_tag)`
starts empty. Either way the lexicon owns what it holds: two lexicons never share
state, and `insert` on one is invisible to every other. `with_default_tag`,
`with_capitalized_default_tag` and `with_lowercase_retry` adjust how unknown
tokens are handled; "capitalised" is the Unicode `Uppercase` property on the
token's first scalar, so `Ålesund` and `Москва` count and `5` and `日本` do not.

## The text unit

Tagging is a whole-token operation, and only two things look inside a token:
the capitalisation test reads the first scalar, and `Condition::CurrentWordEndsWith`
is `str::ends_with`. Both are defined on **Unicode scalar values**. Nothing counts
UTF-16 code units and nothing indexes a token numerically, so an astral scalar is
one thing everywhere.

## Bundled data

| Language | Lexicon entries | Rules |
|---|---:|---:|
| English | 92,538 | 18 |
| Dutch | 11,699 | 273 |

Plus `brill_paper_rule_strings`, the ten rules of Brill (1992), Table 1.

Timing figures are **not published** for this crate: the benchmark groups exist,
but no run has been taken against the implementation as it now stands.

## References

- Eric Brill, *A Simple Rule-Based Part of Speech Tagger*, ANLP-92, 152–155.
- Eric Brill, *Some Advances in Transformation-Based Part of Speech Tagging*,
  AAAI-94.
- Eric Brill, *Transformation-Based Error-Driven Learning and Natural Language
  Processing: A Case Study in Part-of-Speech Tagging*, Computational Linguistics
  21(4), 1995, 543–565.

## Related

- [Sentence analyzers](analyzers.md)
- [Tokenizers](tokenizers.md)
- [Cargo features](../getting-started/cargo-features.md)
