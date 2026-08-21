# `docs/` — engineering and research archive

This folder is **not** Verbora's user documentation. It is the internal
record of the research and performance work that produced the library:
competitor research, measured performance investigations, and
pre-implementation design proposals.

**If you're looking for how to install or use Verbora, go to the published
site: https://addlayerio.github.io/verbora/** (source in [`site/`](../site/)).

## What's here

| File | What it is |
|---|---|
| [`COMPETITIVE_BENCHMARKS.md`](COMPETITIVE_BENCHMARKS.md) | The competitor research matrix: every library considered as a benchmark comparison, per module, and why it was selected or rejected. |
| [`PERFORMANCE_GAPS.md`](PERFORMANCE_GAPS.md) | Every real performance loss found against a competitor, its investigated cause, and — where closed — the fix and re-measured numbers. A running investigation log, not a to-do list. |
| [`PERFORMANCE_MATRIX.md`](PERFORMANCE_MATRIX.md) | Per-crate checklist against the project's performance audit (laziness, zero-copy, batching, parallelism, allocation, data structures). |
| [`design/`](design/) | Design and standards-research documents, each opening with a **Status** line that says which it is. Two have since been built and are now normative records of shipped behaviour (`distance-contract.md`, `rust-native-migration.md`) — where one of those and the code disagree, one of them is a defect and both are edited in the same change; the rest describe work that does not exist yet. Read the Status line before treating any of them as either. |
| [`research/`](research/) | Original task briefs that drove the work recorded in the files above. |

## Why these read differently from `site/`

The pages under `site/` describe Verbora as it is today, in one voice, with
no trace of how it got there. The files in this folder are the opposite on
purpose: they are a dated, revised-in-place investigation log kept for
provenance — later entries say "closed and reversed" or "narrowed this
round" because that history is the point of keeping them.

That distinction is a hard boundary, not a style preference: this folder's
narrative style must never leak into `site/`, and `site/`'s numbers must
never be sourced by copying prose from here — only by re-verifying the
current facts. See [`.claude/agents/doc-sync.md`](../.claude/agents/doc-sync.md)
for the rules that enforce this.

The repository-wide ownership map is in
[`DOCUMENTATION.md`](../DOCUMENTATION.md).
