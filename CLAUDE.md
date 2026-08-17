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
