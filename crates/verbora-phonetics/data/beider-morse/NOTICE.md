# Third-party data: Beider-Morse Phonetic Matching rule tables

The 127 `.txt` files in this directory are the Beider-Morse Phonetic Matching
(BMPM) rule corpus — every one of them carries its own unmodified
[Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
header, preserved exactly as copied. This directory itself does not change
their license.

**Provenance.** These files are copied from
[Apache Commons Codec](https://commons.apache.org/proper/commons-codec/)
(`src/main/resources/org/apache/commons/codec/language/bm/`), which is itself
a Java port of the original Beider-Morse Phonetic Matching algorithm and rule
tables published by Alexander Beider and Stephen P. Morse
(the original reference implementation, in PHP, is
[GPL-3.0-licensed](https://www.gnu.org/licenses/gpl-3.0.html) — these
Apache-2.0 `.txt` files are Apache Commons Codec's own independently
re-licensed re-implementation of the same linguistic rule data, not a copy of
the GPL-licensed PHP source, and are the files Verbora's own Beider-Morse
engine reads and was verified against).

The immediate copy used here was pulled from the [`rphonetic`](https://crates.io/crates/rphonetic)
crate's own `test_assets/cc-rules/` directory (Dalvany, Apache-2.0), which is
itself a verbatim copy of the same Commons Codec files — confirmed identical
license headers and content structure. `rphonetic` is not a runtime
dependency of `verbora-phonetics`; it was used only as a design reference and
a live cross-checking oracle during development (see
`crates/verbora-phonetics/src/beider_morse/mod.rs`'s own doc comment for
details) and is a `[dev-dependencies]`-only tool at most, never shipped.

**Why Apache-2.0 data in an MIT-licensed crate is fine.** Apache-2.0 is
permissive and compatible with MIT redistribution; the requirement it places
on Verbora is exactly what this directory already does — keep each file's own
copyright/license notice intact. Verbora's own code that reads and interprets
these files (the parser, the rule-matching engine, the `BeiderMorse` type)
is Verbora's own MIT-licensed Rust, not a copy of Commons Codec's Java.

**Deliberate deviations from the imported bytes.** Five files differ from the
copy described above, in thirteen characters — nine in rule patterns, four in
comments. Each is a repair of import damage: a character that arrived as
U+FFFD REPLACEMENT CHARACTER, i.e. a character the import could not
represent, not a character the rule set specifies. No rule was added, removed
or re-ordered.

| File | Rule patterns | Restored to |
|---|---|---|
| `gen_rules_italian.txt` | four accented-vowel rules | `é`, `è` (→ `e`), `ó`, `ò` (→ `o`) |
| `sep_rules_italian.txt` | four accented-vowel rules | `é`, `è` (→ `e`), `ó`, `ò` (→ `o`) |
| `gen_rules_english.txt` | one apostrophe rule | `’` U+2019, paired with the ASCII `'` rule on the next line |

The remaining four sit inside `//` comments and never reach the parser: the
`// O’Neill` note on both apostrophe rules of `gen_rules_english.txt`, and the
`ç` named in `sep_approx_common.txt`'s and `sep_exact_common.txt`'s notes on
their commented-out `"C"` rules.

The accented vowels are fixed by the rule set itself, not chosen: the only
Italian vowels that carry the phoneme `e` are `é` and `è`, and the only ones
that carry `o` are `ó` and `ò`. `gen_rules_any.txt` — which the import left
intact — states the same mappings independently (`"é" "" "" "e"`,
`"è" "" "" "e"`, `"ò" "" "" "o" // Sp & It`), as do
`gen_rules_spanish.txt` and `sep_rules_spanish.txt` for Catalan. Acute is
listed before grave because every other Romance file in this corpus orders
them that way; the order is not observable in behaviour, since the four
patterns are distinct and carry the same phoneme in pairs.

**Known coverage gaps, inherited from this exact chain.** Documented here
rather than silently carried forward as unexplained gaps.

- The true original PHP reference lists Latvian as a nineteenth
  Generic-name-type language; Apache Commons Codec's own port omits it (18
  specific languages plus the `any` fallback in `gen_languages.txt`), and
  every file in this directory therefore does too.
- `gen_rules_italian.txt` and `sep_rules_italian.txt` carry no rule for `à`,
  `ì` or `ù`. Under an explicit `encode_language(_, "italian")` those vowels
  match no rule and the Rules pass drops them (`città` encodes as if spelled
  `citt`). Language-guessed `encode` is unaffected when the guess is not a
  confident singleton, since `gen_rules_any.txt` does cover them.
- `gen_rules_arabic.txt`'s last two rules — the two for `ي` (yeh) — carry a
  trailing U+200E LEFT-TO-RIGHT MARK inside the pattern field, so they match
  only input in which the letter is followed by that invisible bidi control.
  No other letter in the file does this and the file has no catch-all, so
  `ي` is dropped from every real Arabic input: `بيت` and `بت` encode
  identically. Whether the mark is present in Commons Codec's own file or was
  introduced somewhere in the copy chain is **not established**; it is
  therefore left exactly as imported rather than repaired on a guess.
