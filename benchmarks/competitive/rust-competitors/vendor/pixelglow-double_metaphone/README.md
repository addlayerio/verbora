# Vendored: pixelglow/double_metaphone

`double_metaphone.h` is vendored verbatim (not a Cargo dependency — this is
a header-only C++11 library with no build system of its own) from:

- Source: https://github.com/pixelglow/double_metaphone
- Commit: `79dd226e1793fda59445c8d792838423f2110347` (the repository's only
  commit on `master`; last pushed 2014-08-26, dormant since — but a single,
  unambiguous, frozen version, so there is no "which version" question the
  way there would be for an actively-developed dependency)
- SHA-256 of the vendored file: `b220ad3d7a82887a635695806bf6f07f4c892af806628cdb4ee5b5eb748e3d26`

## License

BSD (2-clause), stated in the header's own comment block and in the
upstream `README.markdown`'s "License" section. GitHub's automated license
detector reports none, because there is no separate `LICENSE` file in the
repository — the same kind of license-metadata gap already disclosed
elsewhere in this workspace for other competitors (e.g. `segtok`, see
`docs/COMPETITIVE_BENCHMARKS.md`). The license text itself is unambiguous
and is preserved verbatim at the top of the vendored file.

Copyright (c) 2014, Pixelglow Software. Original algorithm copyright (c)
1998, 1999, Lawrence Philips; modified by Kevin Atkinson.

## What this is, for benchmarking purposes

A from-scratch C++11 implementation of Lawrence Philips' Double Metaphone
algorithm — the same algorithm `verbora_phonetics::DoubleMetaphone`
implements. `dm::double_metaphone(std::string) -> std::pair<std::string,
std::string>` returns (primary key, secondary key), directly comparable to
`DoubleMetaphone::process(&self, token: &str) -> (String, String)`. See
`../../benches/double_metaphone_cpp.rs` for the fairness reasoning and
`../../tests/double_metaphone_cpp_correctness.rs` for the verification that
the two agree.

`shim.cpp` is this workspace's own code (not vendored) — a thin `extern
"C"` wrapper so Criterion can call into `double_metaphone.h` through FFI;
see `build.rs` at the crate root for how it's compiled and linked.
