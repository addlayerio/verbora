#!/usr/bin/env python3
"""Fail the build if an *alpha-blended* code token falls below WCAG AA.

Shiki emits every token's colour inline as `--shiki-dark` / `--shiki-light`, and
its dark theme ships three colours with an alpha channel. The transparency is
what breaks them: composited over the code background they land at 2.36, 2.64
and 3.89 against AA's 4.5 for body text. Opaque, the same colours pass.

`theme/oxide.css` overrides those three. This script exists because that
override matches on the literal hex: a Shiki upgrade changes the values and the
rules would stop applying with nothing to say so. Measuring the contrast itself
survives a theme change, a new token colour, and a background change.

Lighthouse found one of the three — the one it happened to sample. The worst of
them is on 3,704 spans.

**Scope, deliberately narrow.** This checks only colours carrying an alpha
channel, because those are the ones where the contrast failure is an accident of
compositing rather than a choice. Several of the theme's *opaque* colours also
sit below 4.5 against these grounds by this script's arithmetic — but this
script composites against a background it is told about, while a browser
composites against what it actually rendered, and the two disagree (Lighthouse
measures the dark ground as #101314 where the stylesheet declares #121516).
Failing a build on arithmetic that cannot see the rendered page is how a gate
earns its way into someone's skip list. Changing the theme's own palette is a
design decision; run Lighthouse for that.
"""
import re
import sys
from pathlib import Path

# theme/oxide.css §7: the dark code-block ground, and the light one.
GROUND = {"dark": "#121516", "light": "#f3f0eb"}  # oxide.css --vp-c-bg-alt
AA = 4.5

def _channel(c: float) -> float:
    c /= 255
    return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

def luminance(hexcolor: str) -> float:
    h = hexcolor.lstrip("#")
    r, g, b = (int(h[i : i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * _channel(r) + 0.7152 * _channel(g) + 0.0722 * _channel(b)

def composite(fg: str, alpha: float, bg: str) -> str:
    f, b = fg.lstrip("#"), bg.lstrip("#")
    return "#" + "".join(
        f"{round(int(f[i:i+2],16)*alpha + int(b[i:i+2],16)*(1-alpha)):02x}"
        for i in (0, 2, 4)
    )

def contrast(a: str, b: str) -> float:
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)

def main() -> int:
    dist = Path(".vitepress/dist")
    if not dist.is_dir():
        print("FAIL — no build to check. Run `npm run build` first.")
        return 1

    # `--shiki-dark:#RRGGBB` or `#RRGGBBAA`, and the CSS override that may follow
    # it on the same span.
    token = re.compile(r"--shiki-(dark|light):(#[0-9A-Fa-f]{6,8})")

    css = "".join(p.read_text(encoding="utf8") for p in dist.rglob("*.css"))
    overridden = {m.group(1).lower() for m in re.finditer(
        r'\[style\*="--shiki-\w+:(#[0-9A-Fa-f]{8})"\]', css, re.I)}

    seen: dict[tuple[str, str], int] = {}
    for page in dist.rglob("*.html"):
        for mode, value in token.findall(page.read_text(encoding="utf8")):
            key = (mode, value.lower())
            seen[key] = seen.get(key, 0) + 1

    bad = []
    for (mode, value), count in sorted(seen.items(), key=lambda kv: -kv[1]):
        ground = GROUND[mode]
        if len(value) != 9:  # opaque: the theme's own choice, not ours to fail on
            continue
        if value in overridden:
            continue  # theme/oxide.css replaces it with the opaque form
        alpha = int(value[7:], 16) / 255
        effective = composite(value[:7], alpha, ground)
        ratio = contrast(effective, ground)
        if ratio < AA:
            bad.append((mode, value, effective, ratio, count))

    if bad:
        print(f"FAIL — {len(bad)} token colour(s) below WCAG AA ({AA}:1):\n")
        for mode, value, eff, ratio, count in bad:
            note = " (alpha composited)" if len(value) == 9 else ""
            print(f"  {mode:5s} {value} -> {eff}{note}  ratio {ratio:.2f}  on {count} spans")
        print("\nDrop the alpha in theme/oxide.css, or replace the colour. See this")
        print("script's docstring for why the alpha is usually the whole problem.")
        return 1

    alpha_seen = sum(1 for (_, v) in seen if len(v) == 9)
    print(f"OK — {alpha_seen} alpha-blended token colours, none below {AA}:1 "
          f"(of {len(seen)} distinct colours in the build).")
    return 0

if __name__ == "__main__":
    sys.exit(main())
