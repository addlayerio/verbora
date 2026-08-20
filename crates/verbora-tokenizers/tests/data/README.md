# UAX #29 conformance data

Two files, taken verbatim from the Unicode Character Database:

| File | Source |
|---|---|
| `WordBreakTest.txt` | <https://www.unicode.org/Public/17.0.0/ucd/auxiliary/WordBreakTest.txt> |
| `SentenceBreakTest.txt` | <https://www.unicode.org/Public/17.0.0/ucd/auxiliary/SentenceBreakTest.txt> |

**Version: Unicode 17.0.0**, which is the version `unicode-segmentation`
1.13.3 implements (`unicode_segmentation::UNICODE_VERSION`) and therefore the
version `verbora-tokenizers` claims. `tests/conformance.rs` asserts the two
agree, so a dependency bump that moves the Unicode version fails loudly here
until these files are refreshed from the matching
`https://www.unicode.org/Public/<version>/ucd/auxiliary/` directory.

**Licence.** Each file carries its own copyright header — "© 2025 Unicode®,
Inc." — and points at <https://www.unicode.org/terms_of_use.html>. The Unicode
licence (v3) permits redistribution of unmodified data files provided the
copyright notice and permission notice are kept with the data, which is
satisfied by shipping the files byte-for-byte as downloaded, headers included.
Do not edit them; refresh them from upstream instead.

**Do not "fix" a failure here by editing the data.** A conformance failure
means Verbora's boundaries disagree with the standard, or that the data and the
dependency are at different Unicode versions. Both are real defects.
