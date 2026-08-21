# verbora-tagger

A Brill transformation-based part-of-speech tagger **engine**. Every token is
first given its most frequent tag from a lexicon, then an ordered list of
contextual rules rewrites the tags that the surrounding words contradict. A
`Trainer` learns a rule set from your own annotated corpus.

**No dictionary ships with this crate.** You bring the lexicon.

## Bring your own lexicon

Two ways in. If you have an annotated corpus, let it count the tag frequencies
for you:

```rust
use verbora_tagger::{BrillTagger, Corpus, RuleSet, Tag};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// Brown `token_TAG` form — three lines here, a real corpus in practice.
let corpus = Corpus::parse_brown(
    "the_DT dog_NN barks_VBZ\n\
     the_DT book_NN is_VBZ good_JJ\n\
     I_PRP would_MD book_VB a_DT flight_NN",
)?;

// Tag frequencies become the initial-state annotator, most frequent first.
// `NN` is what an unknown token falls back to.
let lexicon = corpus.build_lexicon(Tag::new("NN")?)?;
assert_eq!(lexicon.tag_of("book").as_str(), "NN"); // noun twice, verb once

// One rule, written in the same tag set the corpus uses.
let rules: RuleSet = "NN VB PREV-TAG MD".parse()?;
let tagger = BrillTagger::new(&lexicon, &rules);

// The caller supplies the tokens; this crate does not produce them.
let tagged = tagger.tag("I would book a flight".split(' '));
let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();

// `book` is a noun in the lexicon; the rule is what makes it a verb here.
assert_eq!(tags, ["PRP", "MD", "VB", "DT", "NN"]);
# Ok(())
# }
```

If you have the entries themselves, write them down:

```rust
use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut lexicon = Lexicon::new(Tag::new("NN")?)
    .with_capitalized_default_tag(Tag::new("NNP")?);
lexicon.insert("the", vec![Tag::new("DT")?])?;
lexicon.insert("runs", vec![Tag::new("VBZ")?])?;
lexicon.insert("book", vec![Tag::new("NN")?, Tag::new("VB")?])?; // most frequent first

// Every unknown token starts as `NN`; this rule moves the `-ly` ones.
let rules: RuleSet = "NN RB CURRENT-WORD-ENDS-WITH ly".parse()?;
let tagger = BrillTagger::new(&lexicon, &rules);

let tagged = tagger.tag("the dog runs quickly".split(' '));
let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
assert_eq!(tags, ["DT", "NN", "VBZ", "RB"]);
# Ok(())
# }
```

Where to get a lexicon is your call, and that is deliberate — whoever wants one
downloads or builds it under terms they are happy with. Anything in Brown
`token_TAG` format goes straight into `Corpus::parse_brown`; anything else is a
loop over `Lexicon::insert`. `Trainer` then learns the rules from the same
corpus, so a tagger fitted to your tag set never needs a bundled anything.

### Why nothing is bundled

Versions before 0.3 shipped English and Dutch dictionaries. The English pair was
LGPL-3.0 and could not be redistributed under this crate's MIT licence; no terms
could be located for the Dutch pair at all. Both were removed rather than
carried as risk. `data/NOTICE.md` records exactly which files, where they came
from and what was wrong with each.

The one data file that remains is `RuleSet::brill_1992` — the ten
transformations of Brill (1992), Table 1, which are a published, citable table
rather than a redistributed corpus.

## The one thing to know about `RuleSet::brill_1992`

Those ten rules are written in **Brown corpus tags** — `AT`, `PPS`, `PPO`,
`HVD`, `NP` — not Penn Treebank ones. Against a Penn-tagged lexicon (`DT`,
`PRP`, `VBD`, `NNP`) they match almost nothing, and that failure is silent: the
tagger runs, costs a pass per rule, and hands back the initial-state annotation
unchanged.

They are useful as a worked example of the rule format, as a published set to
check an implementation against, and as a starting point for Brown-annotated
text. They are not a general-purpose English tagger.

```rust
use verbora_tagger::RuleSet;

let rules = RuleSet::brill_1992();
assert_eq!(rules.len(), 10);
assert_eq!(rules.rules()[0].to_string(), "TO IN NEXT-TAG AT");
```

## What it expects, and what it promises

**This crate never tokenizes.** It takes tokens you already have, and a token is
simply a non-empty string containing no Unicode `White_Space` scalar.

That matters more than it sounds, because it decides which lexicon keys you can
ever reach. A lexicon keyed by whitespace-delimited corpus tokens holds
`well-known` and `A.A.U.` as single keys, and a UAX #29 word tokenizer splits
inside both — so those entries can never be looked up, whatever the surrounding
text. Key the lexicon with the same producer that will tokenize the text, which
`Corpus::build_lexicon` does for you, and the problem does not arise.

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
  rule templates, and the ten rules of `RuleSet::brill_1992`.
- *Some Advances in Transformation-Based Part of Speech Tagging*, AAAI-94 — the
  lexicalised templates.
- *Transformation-Based Error-Driven Learning and Natural Language Processing: A
  Case Study in Part-of-Speech Tagging*, Computational Linguistics 21(4), 1995,
  543–565, §2 — initial-state annotation and the error-driven training
  procedure.

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
