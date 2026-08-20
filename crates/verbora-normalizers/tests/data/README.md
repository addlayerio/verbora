# UAX #15 conformance data

One file, taken verbatim from the Unicode Character Database:

| File | Source |
|---|---|
| `NormalizationTest.txt` | <https://www.unicode.org/Public/17.0.0/ucd/NormalizationTest.txt> |

**Version: Unicode 17.0.0**, which is the version `unicode-normalization`
0.1.25 implements (`unicode_normalization::UNICODE_VERSION`) and therefore the
version `verbora-normalizers` claims. It is also the version
`unicode-segmentation` 1.13.3 implements, which is what lets
`verbora-tokenizers` and this crate agree about the same text.
`tests/conformance.rs` asserts the version, so a dependency bump that moves the
Unicode version fails loudly here until this file is refreshed from the
matching `https://www.unicode.org/Public/<version>/ucd/` directory.

**Licence.** The file carries its own copyright header — "© 2025 Unicode®,
Inc." — and points at <https://www.unicode.org/terms_of_use.html>. The Unicode
licence (v3) permits redistribution of unmodified data files provided the
copyright notice and permission notice are kept with the data, which is
satisfied by shipping the file byte-for-byte as downloaded, header included.
Do not edit it; refresh it from upstream instead.

**Do not "fix" a failure here by editing the data.** A conformance failure
means Verbora's normalization disagrees with the standard, or that the data and
the dependency are at different Unicode versions. Both are real defects.
