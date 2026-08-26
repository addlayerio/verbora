# Claude Code — operational instructions for this repo

This file is for how Claude Code should *work* in this repository — which
subagent to use for which kind of task. It is separate from `AGENTS.md`,
which documents how Verbora itself should be engineered (architecture,
performance philosophy, API design) — read that for the codebase, not for
workflow.

## Documentation and site updates go to `doc-sync`, not inline edits

Whenever a task requires updating `docs/*.md`, `site/**/*.md`, `README.md`,
or `AGENTS.md` itself to match a code change (new function, changed
algorithm, fixed bug, new benchmark number, new competitor evaluated) — do
**not** edit those files directly in the main session. Instead:

1. Finish and verify the code change first (source, tests, `cargo fmt`/
   `clippy` all clean).
2. Delegate the documentation update via the `Agent` tool, targeting the
   `doc-sync` subagent (`.claude/agents/doc-sync.md`), and launch it with
   `run_in_background: true` so the main session can keep working on the
   next piece of code instead of context-switching into docs.
3. Give it a self-contained brief: what changed, which file(s) and
   function(s), what the old behavior/numbers were, what the new ones are
   (or how to measure them), and which doc/site pages are known to
   reference the old behavior if you already know.

Do not wait on it unless the very next step genuinely depends on the doc
update being done — usually it doesn't, so keep coding.

This is a **strong preference, not a hard guarantee** — automatic
delegation is heuristic. If you notice yourself about to `Edit` a file
under `docs/` or `site/` directly, stop and delegate to `doc-sync` instead
unless the edit is a trivial one-line fix already in the same diff as a
larger docs task already delegated.

## A crate's public API and its README change together

Every crate under `crates/` has a `README.md`, and it is that crate's landing
page on crates.io — the first thing anyone evaluating it reads. Unlike the
site, it is compiled by nothing, linked from nothing and checked by no gate,
so a renamed type or a changed return value can survive there indefinitely.

Treat it as part of the API change, not as follow-up: when a crate's public
surface moves — a signature, a removed item, a changed guarantee, a new
default — its README moves in the same piece of work. Delegate the writing to
`doc-sync` along with the rest of the documentation brief; the point is that
it gets listed, not that you write it yourself.

The root `README.md` is a different document with a different audience. A
crate README answers "what does *this* crate do"; the root answers "what is
Verbora". Never point one at the other.

## Never run benchmarks on your own initiative — ask, then batch

**Do not run `cargo bench`, `benchmarks/competitive/**` scripts, or any
timing sweep unless the user has asked for it in this turn.** This is a hard
default, not a preference.

The reason is arithmetic. A full competitive campaign in this repo takes
**hours**; a single crate's suite takes tens of minutes. A change takes
minutes. Benchmarking after every change spends more wall-clock on
measurement than on the work being measured — and every result is
invalidated the moment the next change lands, so the early runs were waste
even when they were correct.

What to do instead, after finishing and verifying a change:

1. Say the change is done and that its performance is currently unmeasured.
2. **Ask** whether to measure now or keep going and batch it — name what
   *would* be measured (which target, roughly how long) so the answer is an
   informed one.
3. Default to continuing. Accumulate changes; measure once, at the end of a
   batch, on settled code.

Corollaries:

- **Never publish an unmeasured number.** If code changed since the last
  run, its published figures are stale — say so rather than inferring, and
  do not update `site/` benchmark tables from anything but a fresh
  full-precision run.
- **Every subagent brief must carry this prohibition explicitly.** Agents
  do not inherit this file's rules reliably, and a stray `cargo bench` in a
  subagent both burns hours and corrupts any measurement running elsewhere.
- **Builds and tests count as contention.** While a benchmark runs, no
  other agent may run `cargo build/test/check/clippy` — CPU competition
  silently skews the numbers rather than failing loudly.
- Run benchmarks **one target at a time**, never the whole workspace at
  once: a crash in one target cannot then abort the campaign.

`AGENTS.md`'s optimization cycle (correctness → benchmark → profile →
optimize → benchmark again) still governs *what* to measure once the user
says yes. This rule governs *when* — and the answer is not "automatically".

## A scratch copy shares this machine's build cache — set `CARGO_TARGET_DIR`

`CARGO_TARGET_DIR` is exported globally here
(`/home/mpanichella/.cargo-targets/fedora`), so **every checkout of this repo
compiles into one shared tree**, including worktrees, `/tmp` copies and any
scratch clone made for mutation testing.

The consequence is not slowness, it is a false result. An agent that copies the
tree, reverts a fix to prove a test goes red, and runs `cargo test` there,
poisons the cache the *real* tree then reads. That has already happened: a
verification run left the working tree failing five `verbora-stemmers` tests
with the source provably untouched — same md5, same mtime, same `git status`.
A stale artifact and a real regression look identical from the test output.

So, whenever work happens in a copy of this repo:

```bash
CARGO_TARGET_DIR=<a path unique to that copy> cargo test ...
```

and if you suspect you have already crossed the streams, `cargo clean -p <crate>`
in the real tree before believing any red you see there.

This is the same hazard, one layer up, as the Criterion group-name collision
that put four fabricated rows into `results.json`: two workspaces writing into
one namespace with nothing recording which of them wrote what.
