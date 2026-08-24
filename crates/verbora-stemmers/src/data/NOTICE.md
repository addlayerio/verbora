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
| `lancaster_rules.rs` | Paice/Husk rule table | Paice, *Another stemmer*, SIGIR Forum 24(3), 1990 — verified rule by rule and in order against Paice's own distributed rule file | **unknown** |
| `indonesian_dict.rs` | 29,932 Indonesian root words | `natural`'s runtime `kata-dasar.json`, entry for entry, which is PHP Sastrawi's `kata-dasar.txt` (MIT) with one corrupt entry substituted | MIT upstream; the substitution is unattributed |

## Why this matters more than paperwork

Two of these are load-bearing in ways a licence audit would not surface:

- **`lancaster_rules.rs` array order is semantic** — the first rule whose
  pattern matches and whose result is acceptable wins — and it is now grounded:
  115 rules, 21 sections, compared entry by entry and in sequence against
  Paice's own distributed rule file, which is fetchable and machine-parseable.
  The order matches. A test pins it, so the next reorder is not silent.
- **The walk found wrong answers in two of the three tables.** `carry_tables.rs`
  turns `joyeux` into `jooeil`, and 22 of `indonesian_dict.rs`'s own hyphenated
  roots do not stem to themselves. Both are faithful to the implementations
  these tables came from, which is exactly why they survived: under a parity
  standard they were correct by definition. See each file's notes.
- **The precedent that made this worth doing.** The same walk on the
  Lancaster rules, the Carry suffixes and the Indonesian dictionary, and until
  it happens nobody knows whether they carry the same class of defect.

## Under the current rule

`AGENTS.md`'s `# Licensing` treats an unlocatable licence as a refusal. Every
row above predates that rule. They are recorded here so the decision to keep or
replace them is deliberate rather than inherited — which is what `verbora-tagger`
did not have when it shipped LGPL-3.0 dictionaries for two published versions.
