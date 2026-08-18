# Contributing to Verbora

Thank you for helping build Verbora: a Rust-native NLP toolkit focused on
correctness, clear APIs and measured performance.

## What a contribution must include

A public behaviour change is complete only when its implementation, tests and
documentation agree. Documentation drift is a bug.

| Change | Expected evidence |
|---|---|
| Public API or behaviour | Rustdoc, user-facing site guidance when needed, and a test that pins the behaviour |
| Bug fix | A regression test that fails before the fix |
| Performance change | A representative benchmark before and after; no unsupported speed claim |
| New feature | Defined inputs, outputs, defaults, errors, Unicode and empty-input behaviour |
| Parallel API | Sequential equivalence test and measured crossover evidence |

## Where documentation belongs

Follow the [documentation map](DOCUMENTATION.md).

- `site/` is the public documentation source: installation, concepts,
  features, recipes and published benchmark summaries.
- Rustdoc beside each crate owns exact API contracts.
- `docs/` records internal research, design and benchmark provenance; it is
  not a second user manual.
- `AGENTS.md` is the detailed engineering policy for automated agents and
  maintainers.

The public site describes the library as it is today. Development history and
competitor context belong in internal records and benchmark material, not in
feature guides.

## Before opening a pull request

Run the narrowest relevant checks first, then the broader suite when the scope
requires it:

```bash
cargo test -p <affected-crate>
cargo clippy -p <affected-crate> --all-targets --all-features -- -D warnings

cd site
npm run check
```

`npm run check` compiles published snippets, builds the site and verifies
internal links. Do not edit generated benchmark output by hand; regenerate it
from the committed harness and record the environment.

## Pull request description

Explain the user-visible outcome, the affected crates, the tests run and any
benchmark result. Call out intentional semantic changes and trade-offs plainly.

Keep a pull request focused. Unrelated formatting or generated-file changes
make correctness and performance review harder.

## Code and design

Prefer idiomatic, safe Rust and preserve the crate's documented semantics.
Avoid allocations, copies and clones in hot paths unless measurement justifies
them. A faster implementation that changes specified behaviour is not an
optimization.

For the full project engineering policy, read [AGENTS.md](AGENTS.md).
