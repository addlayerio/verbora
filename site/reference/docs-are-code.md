# Documentation is part of the code

This site is not a companion to Verbora. It is part of its public interface, and
the repository enforces that.

## The rule

> A feature does not exist until its code, its tests **and** its documentation
> are updated together. Documentation drift is a bug.

Written into [`AGENTS.md`](https://github.com/addlayerio/verbora/blob/main/AGENTS.md),
which every contributor — human or agent — works from.

## Definition of done

A feature is not finished at *implementation + tests*. It is finished at:

```text
implementation
+ unit tests
+ test suite against the recorded golden output
+ performance review
+ benchmarks where applicable
+ rustdoc on every public item
+ GitHub Pages documentation
+ usage examples that compile
+ "Choosing the Right API" guidance where more than one variant exists
```

## What must trigger a documentation change

Any change to observable behaviour. Specifically:

| Change | Documentation that must change with it |
|---|---|
| New feature, API, trait, tokenizer, stemmer, algorithm, language | Feature page, [features overview](../features/index.md), [roadmap](../features/roadmap.md) |
| New Cargo feature | [Cargo features](../getting-started/cargo-features.md) |
| Changed default | Every page that states the old default |
| Changed error type or condition | Feature page, known divergences |
| Changed behaviour of any kind | Feature page, behaviour notes, affected examples |
| Deprecated API | Feature page, known divergences |
| Performance optimisation | [Benchmarks](../benchmarks/index.md), and the recommendation it changes |
| New iterator / `_into` / batch API | [Choosing the right API](../choosing/index.md), the relevant decision tree, the comparison table |
| Changed allocation behaviour | [Allocation reference](../performance/allocation.md), the API's `perf` card |
| Changed recommendation | Decision trees, comparison tables, recipes, [performance guide](../performance/index.md) |

That last row is the one people forget. If a new implementation makes `tokens()`
clearly better than `tokenize_into()` for a workload the site currently
recommends `tokenize_into()` for, the optimisation is not finished until the
decision tree, the comparison tables, the performance guide, the recipes and the
benchmark page say so.

## The "Choosing the Right API" rule

> Whenever Verbora exposes more than one API for the same conceptual problem, the
> documentation **must** explain why each variant exists and when users should
> choose it.

A group like

```text
tokenize()
tokens()
tokenize_into()
tokenize_batch()
```

may never appear without, for each member:

```text
use case
trade-off
performance characteristics
allocation behaviour
recommendation
```

And the corollary, which is a design rule rather than a documentation one: **if a
real difference between two APIs cannot be explained, the second API should not
exist.**

## The performance-API rule

> Every performance-oriented API must document the performance problem it solves.

`tokenize_into()` has to explain why it exists, when to use it, when *not* to,
how the memory reuse works, how it differs from `tokenize()`, how it differs from
lazy iteration, and what evidence supports the recommendation.

## How this is enforced

Six CI checks, all in `.github/workflows/docs.yml`:

| Check | What it catches |
|---|---|
| `check-snippets.py` | Any published example that stops compiling — or whose assertions stop holding |
| `cargo test --workspace` | Broken rustdoc doctests, and a behaviour regression |
| `cargo doc` with `-D warnings` | An undocumented public item, or a broken intra-doc link |
| `npx vitepress build` | A sidebar entry with no page behind it, and any Markdown that fails to render |
| `check-links.py` | Internal links or anchors that do not resolve, including inside raw HTML, and any page unreachable from the sidebar |
| `check-facts.py` | A recorded-case count, suite count or reference version that no longer matches `fixtures/` |

The last one exists because the other five can all pass while the site quietly
lies: the fixtures are regenerated as the regression suite grows, and a
number left behind on a page still renders, still links and still compiles.

## Why VitePress

<div class="callout callout-note">
<strong>This site started on mdBook</strong> and was migrated to VitePress
partway through writing it. The reasoning below is the decision as it stands
today, including the correction — because a site about measuring before
optimising should not hide its own reversed decisions.
</div>

| Tool | Verdict |
|---|---|
| **VitePress** ✅ | Vue components for the repeated structures on this site (performance cards, badges, decision trees) instead of raw HTML pasted into every page; built-in local search with no service to run; per-page SEO/OpenGraph control; a single, self-contained site toolchain |
| mdBook | The original choice. Its stated advantage — no separate toolchain to pin — is real but thin: the site build is the only place in this repository that needs one. Everything else about it (Rust-native, static output, clean Markdown diffs) is a comfort rather than a requirement, and mdBook's raw-HTML-only extensibility meant every repeated component was hand-written HTML rather than a reusable one |
| Zola | A general-purpose static site generator; the book structure, prev/next navigation and search would all be rebuilt by hand |
| Docusaurus | A heavier Node/React toolchain than this site needs, and its versioned-docs strength does not matter for a pre-1.0 library |
| MkDocs / Material | Good documentation UX, but its component story is weaker than Vue's for the repeated structures on this site |

The deciding factor was **integration with the code** either way: `cargo doc`
output drops into the published site at `/api/`, the source lives beside the
crates it documents, and the whole build is one toolchain — which is what makes
the six CI gates cheap enough to run on every pull request.

The one thing neither tool tests well is the snippets. VitePress has no snippet
test runner at all, and mdBook's `mdbook test` passes only `-L` to rustdoc,
which does not populate the extern prelude for edition-2018-or-later code, so
every `use verbora_*` fails to resolve regardless of which site generator is in
front of it. Rather than weaken the examples to suit either tool,
`site/check-snippets.py` extracts them and hands them to Cargo directly —
which also means the assertions actually *run*.

## Why examples live in a crate

Every non-trivial snippet on this site is compiled against the real crates, not
merely pasted. `crates/verbora-examples` depends on all ten implemented
`verbora-*` crates and exists purely so CI can prove the code works:

```bash
cargo build -p verbora-examples --examples
```

A snippet that stops compiling fails the build. That is the whole point: a
documentation example that no longer works is worse than no example, because it
costs a reader time before they discover it is wrong.

## Honesty rules

Three editorial constraints this site holds itself to, because they are what make
the rest of it trustworthy:

**No invented APIs.** Every function, type and method named here exists in the
source. Where something does not exist — parallel APIs, scratch buffers,
stemmers, TF-IDF — the site says so plainly rather than describing a plausible
design.

**No invented numbers.** The only measured cross-language results in this
repository are the 26 `verbora-distance` benchmarks. Everywhere else the site
describes asymptotics and allocation behaviour read from the source, and labels
itself "not yet benchmarked".

**Reproduced bugs are documented as bugs.** Verbora preserves several
behaviours that are plainly wrong, because callers depend on them. Each is named,
explained and linked rather than quietly kept.

## If you are contributing

1. Read the feature page for the subsystem you are touching. If it is wrong,
   that is part of your change.
2. If you add a variant to an existing operation, add it to the comparison table
   *and* the decision tree.
3. If you change allocation behaviour, update the `perf` card and the
   [allocation reference](../performance/allocation.md).
4. If you optimise something, publish the measurement — including if it went the
   wrong way. Two of this site's most useful anecdotes are optimisations that
   made things slower.
5. Run the checks:

```bash
cargo test --workspace                  # unit + integration + rustdoc doctests
python3 site/check-snippets.py     # every published snippet compiles and passes
(cd site && npx vitepress build)   # the site builds; no missing pages
python3 site/check-links.py        # no dangling internal link or anchor
python3 site/check-facts.py        # the published numbers still match fixtures/
```
