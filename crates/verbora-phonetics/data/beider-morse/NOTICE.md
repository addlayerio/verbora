# Third-party data: Beider-Morse Phonetic Matching rule tables

The 127 `.txt` files in this directory are the Beider-Morse Phonetic Matching
(BMPM) rule corpus — every one of them carries its own unmodified
[Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
header, preserved exactly as copied. This directory itself does not change
their license.

**Provenance.** These files are copied verbatim from
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

**One known coverage gap, inherited from this exact chain.** The true
original PHP reference lists Latvian as a nineteenth Generic-name-type
language; Apache Commons Codec's own port omits it (18 specific languages
plus the `any` fallback in `gen_languages.txt`), and every file in this
directory therefore does too. This is documented here rather than silently
carried forward as an unexplained gap.
