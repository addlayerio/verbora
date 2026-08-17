# Rust API reference

The guides on this site explain *when* and *why*. The rustdoc explains *what*,
with every signature, every field and every trait implementation.

Both are generated from the same repository, and the API reference is published
alongside this site.

## Browse

<div class="cards">

<a class="card" href="../api/verbora_tokenizers/">
<span class="card-title">verbora_tokenizers →</span>
<span class="card-desc">25 tokenizers, the <code>Tokenize</code> trait, <code>Utf16Token</code>, <code>Pattern</code>.</span>
</a>

<a class="card" href="../api/verbora_distance/">
<span class="card-title">verbora_distance →</span>
<span class="card-desc">Levenshtein, Damerau, Jaro–Winkler, Dice, Hamming, and the <code>units</code> module.</span>
</a>

<a class="card" href="../api/verbora_phonetics/">
<span class="card-title">verbora_phonetics →</span>
<span class="card-desc">SoundEx, Metaphone, Double Metaphone, Daitch–Mokotoff, <code>phoneticize_tokens</code>.</span>
</a>

<a class="card" href="../api/verbora_ngrams/">
<span class="card-title">verbora_ngrams →</span>
<span class="card-desc">The window engine, frequency stats, text entry points, Chinese n-grams.</span>
</a>

<a class="card" href="../api/verbora_normalizers/">
<span class="card-title">verbora_normalizers →</span>
<span class="card-desc">Diacritics, English contractions, Nordic, and the 17 Japanese converters.</span>
</a>

<a class="card" href="../api/verbora_inflectors/">
<span class="card-title">verbora_inflectors →</span>
<span class="card-desc">Noun, verb and ordinal inflection, runtime <code>Rule</code>s, the <code>pattern</code> translator.</span>
</a>

<a class="card" href="../api/verbora_trie/">
<span class="card-title">verbora_trie →</span>
<span class="card-desc">The prefix tree and its two lazy iterators.</span>
</a>

<a class="card" href="../api/verbora_transliterators/">
<span class="card-title">verbora_transliterators →</span>
<span class="card-desc">Japanese kana → romaji: <code>transliterate_ja</code>, <code>transliterate_into</code>, the <code>Phase</code> pipeline.</span>
</a>

<a class="card" href="../api/verbora_core/">
<span class="card-title">verbora_core →</span>
<span class="card-desc">The six shared traits, <code>Token</code>, <code>StopWords</code>, reference string semantics.</span>
</a>

</div>

<div class="callout callout-note">
<strong>If those links 404</strong>, the rustdoc has not been deployed to this
site yet, or you are reading a local <code>vitepress dev</code> build. Generate it
locally with the command below, or read it on
<a href="https://docs.rs">docs.rs</a> once the crates are published.
</div>

## Generating it locally

```bash
cargo doc --workspace --no-deps --open
```

To place it where this site expects, as CI does — resolving the target
directory rather than assuming `./target`, since `CARGO_TARGET_DIR` may be set:

```bash
cargo doc --workspace --no-deps
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
cd site && npx vitepress build
cp -r "$TARGET_DIR/doc" .vitepress/dist/api
```

## How the two fit together

```text
This site                          rustdoc
──────────                         ───────
Why does this API exist?           What is its exact signature?
When should I use it?              What traits does it implement?
What does it cost?                 What are the field types?
Which variant do I pick?           What does it return on error?
How do I combine it with X?        What is it generic over?
```

A rough rule: if the question has a single correct answer, rustdoc has it. If the
answer is "it depends on your workload", this site has it.

## Conventions in the rustdoc

**Module-level docs carry the behaviour notes.** Every crate's `lib.rs` opens with
the reference behaviours a naive port gets wrong, and each is cross-referenced
to the code that reproduces it. Those are worth reading before using a subsystem
seriously.

**Doc examples are tested.** `cargo test -p <crate>` runs them; the nine
documented crates carry 81 passing doctests between them at the time of
writing.

**`missing_docs` is a warning workspace-wide**, so every public item has at least
a summary line.

**Divergences are documented at the item.** Where Verbora deliberately differs
from established behaviour, the reason is on the type or function that differs.

## Where else to look

| | |
|---|---|
| Measured performance | [`docs/PERFORMANCE.md`](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md) |
| Behavioural analysis of the reference modules | [`docs/specs/`](https://github.com/addlayerio/verbora/tree/main/docs/specs) |
| Runnable examples | [`crates/verbora-examples/examples/`](https://github.com/addlayerio/verbora/tree/main/crates/verbora-examples/examples) |
