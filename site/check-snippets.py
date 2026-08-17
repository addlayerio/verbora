#!/usr/bin/env python3
"""Compile and run every Rust snippet on the Verbora documentation site.

`mdbook test` cannot do this job here: it passes only `-L` to rustdoc, which is
not enough to populate the extern prelude for an edition-2018-or-later snippet,
so every `use verbora_*` fails to resolve. Rather than weaken the snippets, this
script extracts them and hands them to Cargo, which already knows how to link
the workspace.

Each ```rust block (except those marked `ignore`) becomes a module in a
generated example under `crates/verbora-examples/examples/`. Snippets are then
**executed**, not merely compiled, so a wrong `assert_eq!` on the site fails CI
exactly like a wrong test does.

Usage:  python3 site/check-snippets.py [--keep]
Exit code 0 when every snippet compiles and passes.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent
SRC = HERE  # site/ — VitePress source pages live at the site root
ROOT = HERE.parent  # site/ is a top-level directory, one level below the repo root
GENERATED = ROOT / "crates" / "verbora-examples" / "examples" / "doc_snippets.rs"
SKIP_DIRS = {".vitepress", "node_modules", "public"}

FENCE = re.compile(
    r"^```(rust[^\n]*)\n(.*?)^```\s*$",
    re.MULTILINE | re.DOTALL,
)
MAIN = re.compile(r"^(\s*)fn main\s*\(", re.MULTILINE)


def snippet_body(raw: str) -> str:
    """Strip rustdoc's hidden-line markers, as mdBook and rustdoc do."""
    out = []
    for line in raw.splitlines():
        stripped = line.lstrip()
        if stripped == "#":
            continue
        if stripped.startswith("# "):
            indent = line[: len(line) - len(stripped)]
            out.append(indent + stripped[2:])
            continue
        if stripped.startswith("##"):  # an escaped literal '#'
            indent = line[: len(line) - len(stripped)]
            out.append(indent + stripped[1:])
            continue
        out.append(line)
    return "\n".join(out)


def collect() -> list[tuple[str, int, str]]:
    """Return (page, line, code) for every non-ignored Rust snippet."""
    found: list[tuple[str, int, str]] = []

    for page in sorted(SRC.rglob("*.md")):
        if SKIP_DIRS & set(page.relative_to(SRC).parts):
            continue
        text = page.read_text(encoding="utf-8")
        for match in FENCE.finditer(text):
            info = match.group(1)
            attrs = {a.strip() for a in re.split(r"[,\s]+", info[len("rust"):]) if a.strip()}
            if "ignore" in attrs or "compile_fail" in attrs or "no_run" in attrs:
                continue
            line = text[: match.start()].count("\n") + 1
            found.append(
                (str(page.relative_to(SRC)), line, snippet_body(match.group(2)))
            )

    return found


def generate(snippets: list[tuple[str, int, str]]) -> str:
    parts = [
        "//! GENERATED — do not edit. Produced by `site/check-snippets.py`.",
        "//!",
        "//! Every Rust snippet published on the documentation site, compiled and",
        "//! run so that a stale example fails CI. Regenerate with:",
        "//!",
        "//!     python3 site/check-snippets.py",
        "",
        "#![allow(unused_variables, unused_mut, dead_code, unused_imports)]",
        "#![allow(missing_docs, clippy::all)]",
        "",
    ]
    calls = []

    for i, (page, line, code) in enumerate(snippets):
        has_main = MAIN.search(code) is not None
        body = MAIN.sub(r"\1pub fn main(", code, count=1) if has_main else code

        parts.append(f"/// `{page}` line {line}")
        parts.append(f"pub mod s{i} {{")
        if has_main:
            parts.append(body)
        else:
            parts.append("    pub fn run() {")
            parts.extend("    " + l if l.strip() else l for l in body.splitlines())
            parts.append("    }")
        parts.append("}")
        parts.append("")
        calls.append((i, page, line, "main" if has_main else "run"))

    parts.append("fn main() {")
    for i, page, line, fname in calls:
        parts.append(f'    print!("{page}:{line} ... ");')
        parts.append("    use std::io::Write as _;")
        parts.append("    std::io::stdout().flush().ok();")
        parts.append(f"    s{i}::{fname}();")
        parts.append('    println!("ok");')
    parts.append(f'    println!("\\n{len(calls)} snippets compiled and passed.");')
    parts.append("}")
    parts.append("")

    return "\n".join(parts)


def main() -> int:
    keep = "--keep" in sys.argv

    snippets = collect()
    if not snippets:
        print("error: no Rust snippets found", file=sys.stderr)
        return 1

    GENERATED.write_text(generate(snippets), encoding="utf-8")
    print(f"Extracted {len(snippets)} snippets -> {GENERATED.relative_to(ROOT)}")

    result = subprocess.run(
        ["cargo", "run", "--quiet", "-p", "verbora-examples", "--example", "doc_snippets"],
        cwd=ROOT,
    )

    if not keep:
        GENERATED.unlink(missing_ok=True)

    if result.returncode != 0:
        print(
            "\nA documentation snippet failed to compile or its assertions did "
            "not hold.\nRe-run with --keep to inspect the generated file.",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
