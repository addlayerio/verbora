#!/usr/bin/env python3
"""Fail the build if a theme rule turns a nav ancestor into a containing block.

`backdrop-filter`, `filter`, `transform`, `perspective` and `contain` make an
element the containing block for its `position: fixed` descendants. VitePress
renders several fixed overlays *inside* the nav rather than teleporting them to
`<body>`, so styling an ancestor with any of those silently re-anchors the
overlay to that ancestor's box.

This has shipped twice:

* glass on `.VPNav` collapsed `.VPNavScreen` (the mobile menu) to zero height —
  the hamburger flipped to its close state and nothing appeared;
* glass on `.VPNavBar` clipped `.VPLocalSearchBox` (`position: fixed; inset: 0`)
  to the 64px bar — the modal opened but could not be typed into.

Neither is visible in a build log, a link check or a snippet check. Both look
correct in a screenshot of the page behind them.

The fix in both cases is the same: put the effect on a pseudo-element, which is
not an ancestor of anything a component renders. This script enforces that.
"""
import re
import sys
from pathlib import Path

# Ancestors of a VitePress element that is `position: fixed`. Derived from the
# rendered DOM, not guessed: `.VPNavScreen` and `.VPBackdrop` are children of
# `.VPNav`/`.Layout`, and `.VPLocalSearchBox` is a child of `.VPNavBarSearch`.
# `.VPSidebar` is deliberately absent -- it is fixed itself but has no fixed
# descendants, so a filter on it anchors nothing.
ANCESTORS = [
    "VPApp", "Layout", "VPNav", "VPNavBar",
    "VPNavBarSearch", "VPNavBarSearchButton",
]
PROPS = re.compile(
    r"(?<![\w-])(backdrop-filter|filter|transform|perspective|contain)\s*:", re.I
)

def strip_comments(css: str) -> str:
    """Blank out comments, preserving offsets so line numbers stay right.

    Load-bearing: a rule's selector is captured as everything since the
    previous `}`, which includes any comment above it. The comment explaining
    this very fix contains the string `::`, so leaving comments in made the
    pseudo-element check skip the rule it was written to guard.
    """
    return re.sub(r"/\*.*?\*/", lambda m: " " * len(m.group(0)), css, flags=re.S)


def offenders(css: str):
    for block in re.finditer(r"([^{}]*)\{([^{}]*)\}", css):
        selector = block.group(1).strip()
        if not selector or selector.startswith("@"):
            continue
        # A pseudo-element is a separate box: styling it never re-anchors a
        # sibling component. That is the whole point of the fix. Checked
        # against the last comma-separated part, so `.A, .B::before` does not
        # excuse `.A`.
        parts = [p.strip() for p in selector.split(",")]
        body_at = block.start(2)
        for prop in PROPS.finditer(block.group(2)):
            name = prop.group(1).lower()
            for part in parts:
                if "::" in part:
                    continue
                for cls in ANCESTORS:
                    if re.search(rf"\.{cls}(?![\w-])", part):
                        yield part, cls, name, body_at + prop.start()


def main() -> int:
    bad = []
    for path in sorted(Path(".vitepress/theme").rglob("*.css")):
        raw = path.read_text(encoding="utf8")
        css = strip_comments(raw)
        for selector, cls, name, pos in offenders(css):
            line = raw.count("\n", 0, pos) + 1
            bad.append(f"  {path}:{line}\n    `{selector}` sets `{name}`, "
                       f"making `.{cls}` a containing block for the fixed "
                       f"overlay it wraps.\n    Move the effect to "
                       f"`{selector}::before`.")
    if bad:
        print("FAIL — a nav ancestor was made a containing block:\n")
        print("\n\n".join(bad))
        print("\nSee this script's docstring for the two bugs this catches.")
        return 1
    print(f"OK — no containing-block property on any of the "
          f"{len(ANCESTORS)} nav ancestors that wrap a fixed overlay.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
