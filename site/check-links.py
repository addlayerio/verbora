#!/usr/bin/env python3
"""Validate every internal link and anchor in the built Verbora site (VitePress).

VitePress does not check links itself. This reads the **rendered** output under
`.vitepress/dist/` rather than reimplementing VitePress's heading-id algorithm,
so what it validates is what will actually be deployed. It checks:

  * relative links (both Markdown-rendered and inside raw-HTML components —
    the cards, badges, callouts) pointing at a page that does not exist;
  * links to a `#fragment` no element on the target page defines;
  * pages under `src/` (this directory) that no sidebar entry in
    `.vitepress/config.ts` reaches.

External links (http/https/mailto) are not fetched: CI should not depend on the
network or on other people's uptime.

Usage:  npx vitepress build && python3 check-links.py
Exit code 0 when clean, 1 otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

HERE = Path(__file__).parent
DIST = HERE / ".vitepress" / "dist"
CONFIG = HERE / ".vitepress" / "config.ts"

# `base` in config.ts — a URL prefix applied at request time, not a directory
# that exists under dist/. Absolute hrefs carry it and must have it stripped
# before resolving against dist/, which IS the site root.
_base_match = re.search(r"const BASE = '([^']+)'", CONFIG.read_text(encoding="utf-8"))
BASE = _base_match.group(1) if _base_match else "/"

HREF = re.compile(r'href="([^"]+)"')
ID = re.compile(r'id="([^"]+)"')
LINK_TARGET = re.compile(r"link:\s*'([^']+)'")

# rustdoc, if assembled alongside the book, validates its own output and is
# full of runtime-generated anchors this script cannot see statically.
EXTERNAL_TREES = ("/api/",)


def ids_in(html: str) -> set[str]:
    return set(ID.findall(html))


def resolve(page: Path, target: str) -> Path | None:
    """Map an href to the dist file it should serve, VitePress's own way."""
    if target.startswith("/"):
        # Absolute hrefs carry `base` (e.g. /verbora/features/) — a URL prefix,
        # not a directory under dist/, which already *is* the site root.
        rest = target[len(BASE):] if target.startswith(BASE) else target.lstrip("/")
        base = DIST
        target = rest
    else:
        base = page.parent

    if target == "":
        # Only reachable if a caller passes an empty path_part directly; the
        # main loop already short-circuits real fragment-only hrefs before
        # calling resolve() at all.
        return page

    if target == "./":
        # VitePress's rendering of a link to a directory's own index page —
        # NOT necessarily the current page. `[x](index.md#foo)` written from a
        # sibling file renders as `href="./#foo"`, and must resolve to this
        # directory's index.html, not back to the linking page.
        candidate = page.parent / "index.html"
        return candidate if candidate.exists() else None

    candidate = (base / target).resolve()

    if candidate.is_dir():
        idx = candidate / "index.html"
        return idx if idx.exists() else None
    if candidate.exists():
        # A real file already — a page, or a static asset (css/js/woff2/svg).
        return candidate
    if candidate.suffix:
        # Had an extension but doesn't exist — not an extensionless page link.
        return None
    html = candidate.with_suffix(".html")
    if html.exists():
        return html
    idx = candidate / "index.html"
    return idx if idx.exists() else None


def main() -> int:
    if not DIST.is_dir():
        print(f"error: {DIST} does not exist — run `npx vitepress build` first", file=sys.stderr)
        return 1

    pages = sorted(p for p in DIST.rglob("*.html") if p.name != "404.html")
    if not pages:
        print(f"error: no HTML found under {DIST}", file=sys.stderr)
        return 1

    anchors: dict[Path, set[str]] = {p.resolve(): ids_in(p.read_text(encoding="utf-8")) for p in pages}

    problems: list[str] = []

    for page in pages:
        html = page.read_text(encoding="utf-8")
        for raw in HREF.findall(html):
            target = unquote(raw)

            if target.startswith(("http://", "https://", "mailto:", "javascript:", "data:")):
                continue
            if any(seg in target for seg in EXTERNAL_TREES):
                continue

            path_part, _, fragment = target.partition("#")
            rel = page.relative_to(DIST)

            if not path_part:
                if fragment and fragment not in anchors[page.resolve()]:
                    problems.append(f"{rel}: no element defines #{fragment}")
                continue

            dest = resolve(page, path_part)
            if dest is None:
                problems.append(f"{rel}: broken link -> {target}")
                continue

            if fragment and fragment not in anchors.get(dest.resolve(), set()):
                problems.append(
                    f"{rel}: {target} — target page defines no #{fragment}"
                )

    # Source-side check: every Markdown page reachable from the sidebar config.
    config_text = CONFIG.read_text(encoding="utf-8")
    reachable = {"index"}  # the home page, reached via the logo/site title
    for link in LINK_TARGET.findall(config_text):
        slug = link.strip("/")
        reachable.add(slug if slug else "index")

    SKIP_DIRS = {".vitepress", "node_modules"}
    for md in sorted(HERE.rglob("*.md")):
        if SKIP_DIRS & set(md.relative_to(HERE).parts):
            continue
        rel = md.relative_to(HERE).with_suffix("")
        parts = rel.parts
        if parts and parts[-1] == "index":
            slug = "/".join(parts[:-1]) or "index"
        else:
            slug = "/".join(parts)
        if slug not in reachable:
            problems.append(f"{md.relative_to(HERE)}: not reachable from any sidebar entry")

    if problems:
        print(f"{len(problems)} documentation link problem(s):\n", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    total_anchors = sum(len(a) for a in anchors.values())
    print(
        f"OK — {len(pages)} rendered pages, {total_anchors} anchors, "
        "all internal links resolve."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
