# Cargo features

Verbora's feature surface is deliberately tiny. There is exactly **one**
optional feature in the whole workspace, and it is off by default.

## The complete list

| Crate | Feature | Default | What it enables |
|---|---|:--:|---|
| `verbora-core` | `serde` | off | Adds the optional `serde` dependency (`serde = ["dep:serde"]`) |
| every other crate | — | — | No optional features |

That is the entire list. No crate has a `default` feature set with anything in
it, so `default-features = false` changes nothing anywhere.

<div class="callout callout-warn">
<strong>Honest status of <code>verbora-core/serde</code>.</strong> The feature is
declared in <code>crates/verbora-core/Cargo.toml</code> and enabling it pulls in
the <code>serde</code> crate, but no type in <code>verbora-core</code> currently
carries a <code>#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]</code>
attribute — a workspace-wide search for <code>cfg(feature</code> finds nothing.
Enabling it today therefore adds a dependency and no derives. It is reserved for
the serializable model types the classifier and TF-IDF work will need. If you
need <code>Serialize</code> on a Verbora type now, implement it locally with the
<a href="https://serde.rs/remote-derive.html">remote derive</a> pattern.
</div>

## Why so few

Every dependency in this workspace has to justify itself with a benchmark or an
architectural need, and the same rule applies to feature flags: each one doubles
the number of build configurations the test suite would have to prove correct.

The alternative to feature flags here is crate granularity. Rather than
`verbora = { features = ["distance", "phonetics"] }`, you depend on
`verbora-distance` and `verbora-phonetics` directly, and pull in nothing else:

- `verbora-normalizers` has **no dependencies at all** — not even `verbora-core`
  and not `regex`, both deliberately, with the reasoning recorded in its
  `Cargo.toml`.
- `verbora-distance` avoids `regex` entirely and hand-writes its scanners.
- `verbora-trie` depends only on `smallvec`, to keep a bulk load from making one
  heap allocation per node.

So the dependency reduction you would normally get from a feature flag, you get
from choosing crates. See [Installation](installation.md#which-crate-do-i-need).

## Profiles

The workspace defines four Cargo profiles. They are not features, but they
change measured performance enough to be worth knowing about.

| Profile | Settings | Use it for |
|---|---|---|
| `release` | `opt-level = 3`, `lto = "thin"`, `codegen-units = 16` | Normal production builds |
| `release-max` | inherits `release`, `lto = "fat"`, `codegen-units = 1` | Maximum runtime speed, at a significant compile-time cost |
| `bench` | inherits `release`, `debug = true` | Profiling with `perf` / `samply`; symbols resolve |
| `test` / `dev` | `opt-level = 2` (and `2` for all dev dependencies) | Test suites replay large corpora and are unusable unoptimised |

```bash
cargo build --profile release-max
```

The trade-off between `release` and `release-max` is measured, not assumed — see
[`docs/PERFORMANCE.md`](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md).

## Lints you inherit

These are workspace lints, so they apply to the crates themselves rather than to
your code — but they tell you what kind of library you are depending on:

```toml
[workspace.lints.rust]
missing_docs = "warn"
unsafe_code = "deny"        # there is no unsafe in Verbora
rust_2018_idioms = "warn"
```

`unsafe_code = "deny"` is the load-bearing one: every fast path on this
site — the ASCII `&[u8]` promotion in `verbora-distance`, the flat `u32` arena in
`verbora-trie`, the stack-allocated match flags in Jaro–Winkler — is written in
safe Rust.
