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
