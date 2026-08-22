# Third-party data: sentiment lexicons

Fourteen packed lexicons across three families. They have **separate
provenance and separate terms**, and three of the four rows below have terms
this project has not been able to establish.

| Vocabulary | Upstream | Terms |
|---|---|---|
| AFINN English | [`afinn-165`](https://www.npmjs.com/package/afinn-165), Titus Wormer | MIT — the package's own `license` file |
| AFINN Spanish, Portuguese | shipped JSON, no attribution in the source | **none found** |
| ML-SentiCon (es, en, gl, ca, eu) | [ML-SentiCon](http://timm.ujaen.es/recursos/ml-senticon/) | **none found** |
| CLiPS Pattern (nl, it, en, fr, de) | [CLiPS Pattern](https://github.com/clips/pattern) | **none found** |

## What "none found" means here

It is not an assurance. Under `AGENTS.md`'s `# Licensing`, an unlocatable
licence is a refusal, not permission — so by the standard this project now
holds itself to, three of these four rows should not be shipping.

They predate that standard. They are recorded here rather than quietly carried
so that the decision to keep or remove them is a decision someone makes on
purpose, with the facts in front of them.

**Anyone shipping this crate commercially should establish the ML-SentiCon and
Pattern terms independently before relying on it.**

## Precedent

`verbora-tagger` shipped dictionaries under exactly this description until
2026-08, when they turned out to be LGPL-3.0 and unlicensable respectively, and
had to be deleted from a published crate. See that crate's `data/NOTICE.md`.
