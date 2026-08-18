# Documentation map

Verbora has one public documentation surface and three supporting sources.
This map makes the boundary explicit.

| Location | Audience | Owns |
|---|---|---|
| [`site/`](site/) | Library users | Installation, concepts, feature guides, recipes, API selection and published benchmark summaries. This is the source for the public site. |
| Rustdoc beside `crates/*/src/` | Rust callers | Exact API contracts: signatures, defaults, errors, Unicode semantics and examples. The site links to it; it does not duplicate generated API reference. |
| [`README.md`](README.md) | Evaluators and contributors | A short project introduction and links to the public site. It is not a second manual. |
| [`docs/`](docs/) | Maintainers | Research, design records, benchmark provenance and performance investigations. It is deliberately not user documentation. |
| [`AGENTS.md`](AGENTS.md) | Contributors and automated agents | Contribution rules, quality bar and documentation process. It is never product documentation. |

## Publishing rule

Public behaviour changes are documented in Rustdoc and, when a user needs
guidance to choose or use the behaviour, in `site/`. Internal investigation,
historical decisions and raw benchmark evidence stay in `docs/` and
`benchmarks/`.

The public site describes Verbora as the Rust-native library it is today. It
does not narrate development history, migrations or implementation ancestry.
Competitor names and comparisons belong only to the benchmark section, where
they provide necessary context for measured claims.

## Keeping it coherent

`site/` is checked with `npm run check`: published Rust snippets compile and
all internal links resolve. Rustdoc is checked by the crate test and doc
workflows. Benchmark summaries must be regenerated from their recorded
harness results rather than copied from investigation prose.
