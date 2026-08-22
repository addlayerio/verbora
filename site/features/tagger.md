# Part-of-speech tagging

`verbora-tagger` is a Brill transformation-based part-of-speech tagging
**engine**. A lexicon gives every token its most frequent tag, then an ordered
list of contextual rules rewrites the tags the surrounding words contradict.

**No dictionary ships with this crate.** The lexicon is yours to supply, from any
source you have the right to use — see [Why nothing is
bundled](#why-nothing-is-bundled) for the reason, which is a licensing one.

## Bring your own lexicon

There are two ways in, and which one you want depends on what you already have.

If you have an annotated corpus, let it count the tag frequencies for you.
`Corpus::parse_brown` reads the `token_TAG` form, and `Corpus::build_lexicon`
turns the counts into an initial-state annotator:

```rust
use verbora_tagger::{BrillTagger, Corpus, RuleSet, Tag};

fn main() {
    // Three lines here; a real corpus in practice.
    let corpus = Corpus::parse_brown(
        "the_DT dog_NN barks_VBZ\n\
         the_DT book_NN is_VBZ good_JJ\n\
         I_PRP would_MD book_VB a_DT flight_NN",
    )
    .unwrap();

    // Tag frequencies, most frequent first. `NN` is what an unknown token
    // falls back to.
    let lexicon = corpus.build_lexicon(Tag::new("NN").unwrap()).unwrap();
    assert_eq!(lexicon.tag_of("book").as_str(), "NN"); // noun twice, verb once

    // One rule, written in the tag set the corpus itself uses.
    let rules: RuleSet = "NN VB PREV-TAG MD".parse().unwrap();
    let tagger = BrillTagger::new(&lexicon, &rules);

    // The caller supplies the tokens; this crate never produces them.
    let tagged = tagger.tag("I would book a flight".split(' '));
    let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();

    // `book` is a noun in the lexicon; the rule is what makes it a verb here.
    assert_eq!(tags, ["PRP", "MD", "VB", "DT", "NN"]);
}
```

If you have the entries themselves rather than a corpus to count, write them
down. `Lexicon::insert` takes the tags in frequency order, most frequent first:

```rust
use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag};

fn main() {
    let mut lexicon = Lexicon::new(Tag::new("NN").unwrap())
        .with_capitalized_default_tag(Tag::new("NNP").unwrap());
    lexicon.insert("the", vec![Tag::new("DT").unwrap()]).unwrap();
    lexicon.insert("runs", vec![Tag::new("VBZ").unwrap()]).unwrap();
    lexicon
        .insert(
            "book",
            vec![Tag::new("NN").unwrap(), Tag::new("VB").unwrap()],
        )
        .unwrap();

    // Every unknown token starts as `NN`; this rule moves the `-ly` ones.
    let rules: RuleSet = "NN RB CURRENT-WORD-ENDS-WITH ly".parse().unwrap();
    let tagger = BrillTagger::new(&lexicon, &rules);

    let tagged = tagger.tag("the dog runs quickly".split(' '));
    let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
    assert_eq!(tags, ["DT", "NN", "VBZ", "RB"]);
}
```

Anything in Brown `token_TAG` format goes straight into `Corpus::parse_brown`;
anything else is a loop over `Lexicon::insert`. `Trainer` then learns the rules
from the same corpus, so a tagger fitted to your own tag set never needs a
bundled anything.

### Why nothing is bundled

Versions before 0.3 shipped English and Dutch dictionaries. The English pair was
LGPL-3.0, which cannot be redistributed under this project's MIT licence, and no
terms could be located for the Dutch pair at all. Both were removed rather than
carried as risk. The crate's `data/NOTICE.md` records which files, where they
came from and what was wrong with each.

The one data file that remains is the rule table behind `RuleSet::brill_1992` —
a published, citable table rather than a redistributed corpus.

## The tag set is whatever your data says it is

A tag is a string, and Verbora attaches no meaning to it beyond string identity.
A lexicon and a rule set therefore agree only if they were written against the
same tag set, and a mismatch is **not** an error: a rule whose condition names a
tag nothing produces simply never fires. The tagger runs to completion, costs a
pass per rule, and hands back the initial-state annotation unchanged.

That is the one way this crate can quietly do nothing, so it is worth checking
deliberately rather than assuming. `BrillTagger::evaluate` against a held-out
slice of the same corpus is the direct test: if `correct_after_rules` equals
`correct_before_rules`, no rule fired.

## `RuleSet::brill_1992`

The ten transformations of Brill (1992), Table 1 — the first ten his learner
acquired from the Brown corpus:

```rust
use verbora_tagger::RuleSet;

fn main() {
    let rules = RuleSet::brill_1992();
    assert_eq!(rules.len(), 10);
    // Brown `AT` (article), not Penn `DT`.
    assert_eq!(rules.rules()[0].to_string(), "TO IN NEXT-TAG AT");
    assert_eq!(rules.context_span(), (9, 3));
}
```

<div class="callout callout-warn">
<strong>These rules are written in Brown corpus tags, not Penn Treebank
ones.</strong> They name <code>AT</code>, <code>PPS</code>, <code>PPO</code>,
<code>HVD</code> and <code>NP</code>. Against a Penn-tagged lexicon
(<code>DT</code>, <code>PRP</code>, <code>VBD</code>, <code>NNP</code>) they
match almost nothing, and by the rule above that failure is silent.
</div>

They are useful as a worked example of the rule-string format, as a published
set to check an implementation against, and as a starting point for tagging
Brown-annotated text. What they are not is a general-purpose English tagger.

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
`White_Space` scalar, and what produced it is the caller's decision.

That decides which keys your program can reach. A lexicon keyed by
whitespace-delimited corpus tokens holds `well-known` and `A.A.U.` as single
keys, and a UAX #29 word tokenizer splits inside both — `well-known` becomes
`well`, `-`, `known`, because U+002D HYPHEN-MINUS is `Word_Break=Other` — so
those entries can never be looked up, whatever the surrounding text says. Nothing
reports it, because nothing is wrong: the lookups miss and the tokens take a
default.

The fix is to key the lexicon with the same producer that will tokenize the
text. `Corpus::build_lexicon` does exactly that, which is why the corpus path
above is the one to prefer: every key it creates is reachable by construction.
Failing that, `str::split_whitespace` yields exactly conforming tokens and
reaches every key of a whitespace-delimited dictionary.

Nothing here rewrites a token: case folding, trimming and normalisation are the
caller's explicit choice, and tokens come out byte-identical to the ones that
went in.

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
rule set, not of the input — each rule is applied to the whole sentence before
the next runs, so the rules' individual reaches add. It is the tool when memory,
not throughput, is the constraint; if the document already fits in memory, `tag`
does strictly less work.

`Evaluation` reports counts, not percentages: `tokens`, `correct_before_rules`
and `correct_after_rules`. `accuracy` and `accuracy_before_rules` divide them and
return `None` for an empty corpus rather than inventing a value or yielding
`NaN`.

## Lexicons own their entries

`Lexicon::new(default_tag)` starts empty, and the lexicon owns what it holds: two
lexicons never share state, and `insert` on one is invisible to every other.
`with_default_tag`, `with_capitalized_default_tag` and `with_lowercase_retry`
adjust how unknown tokens are handled; "capitalised" is the Unicode `Uppercase`
property on the token's first scalar, so `Ålesund` and `Москва` count and `5` and
`日本` do not.

`tag_of` is total, and its lookup chain is fixed: the key exactly as spelled;
then — unless `with_lowercase_retry(false)` turned it off — the key lowercased by
the Unicode default full lowercase mapping; then the capitalised default; then
the default. The retry changes only what is *looked up*, never what is returned
to you.

## The text unit

Tagging is a whole-token operation, and only two things look inside a token:
the capitalisation test reads the first scalar, and `Condition::CurrentWordEndsWith`
is `str::ends_with`. Both are defined on **Unicode scalar values**. Nothing counts
UTF-16 code units and nothing indexes a token numerically, so an astral scalar is
one thing everywhere.

## Performance

Timing figures are **not published** for this crate: the benchmark groups exist,
but no run has been taken against the implementation as it now stands. The
[competitive benchmarks](../benchmarks/competitive.md) page carries no Verbora
figure for POS tagging either, and says why.

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
