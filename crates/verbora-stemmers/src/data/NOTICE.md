# Data tables compiled into `verbora-stemmers`

This module is Rust source, but its contents are **data**: suffix tables,
character sets, rule tables and a dictionary, embedded as `static` arrays.
Nothing here was written by hand for Verbora, and the provenance of most of it
is not established.

| Table | What it is | Source | Terms |
|---|---|---|---|
| `charsets.rs` | case-insensitive character sets | no generator in the tree | **unknown** |
| `gates.rs` | per-language "is this worth stemming" predicates | derived from the character sets | **unknown** |
| `carry_tables.rs` | Carry (French) suffix maps | Paternostre et al.'s Carry algorithm | **unknown** |
| `lancaster_rules.rs` | Paice/Husk rule table | the published Lancaster rule set | **unknown, and the array order is not grounded in it** |
| `indonesian_dict.rs` | 29,932 Indonesian root words | **not established** | **unknown** |

## Why this matters more than paperwork

Two of these are load-bearing in ways a licence audit would not surface:

- **`lancaster_rules.rs` array order is semantic.** The first rule whose
  pattern matches and whose result is acceptable wins, so the order decides the
  output — and it has never been checked against the published Paice/Husk rule
  set. The stemmer may be producing stems the algorithm does not specify, and
  there is currently nothing to check it against.
- **These tables have not been walked.** The equivalent audit *was* run on the
  gates, and it found `gate_de` was a verbatim copy of `gate_es` — German
  words were being stemmed with `á é í ñ ó ú` and without `ä ö ß`. Wrong
  answers, silently, in a shipped release. The same walk is owed on the
  Lancaster rules, the Carry suffixes and the Indonesian dictionary, and until
  it happens nobody knows whether they carry the same class of defect.

## Under the current rule

`AGENTS.md`'s `# Licensing` treats an unlocatable licence as a refusal. Every
row above predates that rule. They are recorded here so the decision to keep or
replace them is deliberate rather than inherited — which is what `verbora-tagger`
did not have when it shipped LGPL-3.0 dictionaries for two published versions.
