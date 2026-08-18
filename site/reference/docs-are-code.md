# Documentation is part of the code

This site is part of Verbora's public interface, and the repository treats it
that way. This page is what that means for you as a reader.

## What is verified before a page ships

| Check | What it guarantees |
|---|---|
| `check-snippets.py` | Every Rust snippet here is extracted into a real crate, compiled against the actual `verbora-*` crates and **run**. A wrong `assert_eq!` on this site fails the build exactly like a wrong test. |
| `vitepress build` | Every page renders and every sidebar entry has a page behind it. |
| `check-links.py` | Every internal link and `#anchor` resolves — including inside raw HTML — and no page is stranded outside the navigation. |

All three run together:

```bash
cd site && npm run check
```

Rustdoc is held to the same standard from the Rust side: public examples run as
doctests under `cargo test --workspace`.

## What you can rely on when reading

**Every API named here exists.** Where something does not exist — a parallel
API, a scratch buffer, a stemmer for a given language — the page says so plainly
instead of describing a plausible design.

**Every published number names its benchmark**, the machine it ran on and the
command that repeats it. Guidance derived from the shape of the code rather than
from a measurement is labelled as such, and never presented as a measurement.

**Wherever there is more than one API for the same problem**, the page says what
each variant is for, what it costs, and which to reach for by default. See
[Choosing the right API](../choosing/index.md) and
[Allocation behaviour](../performance/allocation.md).

## Found something wrong?

A documentation error is a bug, and reporting one is as useful as reporting a
crash — an example that no longer works costs a reader more time than no example
at all. Issues and corrections go to
[the repository](https://github.com/addlayerio/verbora).
