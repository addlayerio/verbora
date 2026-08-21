# Third-party data: the Brill (1992) transformation rules

This directory holds one data file, and it is the only data
`verbora-tagger` ships:

| File | Contents |
|---|---|
| `English/tr_from_brill_paper.json` | The ten transformation rules of Brill (1992), Table 1 |

**Provenance.** Eric Brill, *A Simple Rule-Based Part of Speech Tagger*,
Proceedings of the Third Conference on Applied Natural Language Processing
(ANLC '92), Association for Computational Linguistics, 1992, pages 152–155.
Table 1 of that paper lists the first ten transformations the learner
acquired from the Brown corpus. This file is those ten rules, transcribed
into `verbora-tagger`'s own rule-string syntax; the syntax is Verbora's, the
transformations are the paper's.

**Why they are here.** They are the one piece of tagger data in this crate
with clean, citable provenance: a published table in a peer-reviewed
conference paper, attributed to a named author, reproducible by anyone with
a copy of the proceedings. Ten short rules are a *de minimis* quotation of a
scientific result, cited as such — not a redistributed corpus.

**Their tag set is Brown, not Penn.** The rules name `AT`, `PPS`, `PPO`,
`HVD` and `NP`, which are Brown corpus tags. They therefore only do
something when paired with a lexicon that is itself Brown-tagged; against a
Penn Treebank lexicon (`DT`, `PRP`, `VBD`, `NNP`) most of them match nothing
and the tagger is a no-op. `RuleSet::brill_1992` says so in its own
documentation, and `ruleset::tests::the_brill_1992_tag_set_is_brown` pins
the exact set of thirteen tags the rules mention.

## Lexicons that used to be here, and are not

Four data files were removed from this directory before the crate's 0.3.0
release. Recorded here because someone will look for them.

| File | Contents | Origin | Terms |
|---|---|---|---|
| `English/lexicon_from_reference.json` | English lexicon | [`dariusk/pos-js`](https://github.com/dariusk/pos-js) | LGPL-3.0 |
| `English/tr_from_reference.json` | English contextual rules | same | LGPL-3.0 |
| `Dutch/brill_Lexicon.json` | Dutch lexicon | Brill-NL (Jeroen Geertzen) | unknown |
| `Dutch/brill_CONTEXTRULES.json` | Dutch contextual rules | same | unknown |

**The English pair could not be redistributed under this crate's licence.**
Both files were byte-identical to the copies shipped in the `natural` npm
package, whose own `data/English/README.txt` names `pos-js` as their source;
`dariusk/pos-js` declares LGPL-3.0. `verbora-tagger` is MIT and is published
on crates.io. LGPL-3.0 data cannot be redistributed under MIT terms, and
attribution does not cure that — the obligation is a licence obligation, not
a credit one.

**The Dutch pair could not be licensed at all.** They are not known to be
incompatible; no terms could be found for them. The author's site is
offline, the files carry no header, and no statement of terms is locatable
anywhere. Shipping data whose licence cannot be established is the same
exposure as shipping data whose licence forbids it, minus the ability to
comply, so they were removed too.

**What replaces them: nothing, deliberately.** `verbora-tagger` is now a
Brill tagger *engine* with no bundled dictionary. A lexicon is built by the
caller, from whatever source they have the right to use, with
`Lexicon::new` + `Lexicon::insert` or with `Corpus::parse_brown` +
`Corpus::build_lexicon`. The crate's `README.md` walks that path with
working code.
