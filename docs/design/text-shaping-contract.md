# `verbora-tokenizers`, `verbora-normalizers`, `verbora-ngrams` — the Rust-native contract

**Status:** normative, not yet implemented. This document defines the public
behaviour of the three text-shaping crates after the Rust-native migration
(`docs/design/rust-native-migration.md`, per-crate item 2). It follows
`docs/design/distance-contract.md` in structure, voice and precedent: no
sentinels, no `NaN`, no panics outside preconditions the type system can
express, no function that silently rewrites its input, and the crate root as
the entire public surface.

Behaviour here is derived from published standards — UAX #29 *Unicode Text
Segmentation*, UAX #15 *Unicode Normalization Forms*, UAX #44 *Unicode
Character Database*, and the n-gram padding convention of Jurafsky & Martin,
*Speech and Language Processing* (3rd ed.) §3.1 — or, where a standard is
silent, from an explicit choice stated and justified below. No clause exists
because another implementation behaves that way.

These three crates are where that rule bites hardest. Together they currently
carry roughly 15,000 lines whose stated justification is another
implementation, including 2,300 lines of machine-dumped tables with no
generator in the tree, nineteen hand-derived character classes with documented
bugs, a process-global mutable tokenizer, and a frequency key that is not
injective. This contract deletes most of it. **The dominant design act here is
subtraction**, and §3.4 lists what goes.

Incompatible changes are authorised. There are no deprecation shims, no
`#[deprecated]` aliases, and no transition period.

---

## 1. The contract in brief

These rules hold across all three crates. Per-crate rules are in §3.

**Tokens are substrings.** Every token, segment and sentence
`verbora-tokenizers` produces is a contiguous slice of the input it was given,
returned as `&'a str` borrowed from that input. There is no `Cow`, no
`String`, no owned token type, and no tokenizer that folds case, strips
punctuation, folds diacritics, trims, or substitutes placeholders. This is the
distance contract's "no input rewriting" clause applied to a crate whose whole
job is cutting text: a tokenizer that rewrote its input could not be composed
with one that did not, and five currently test-pinned rewrites are removed
rather than documented.

`verbora-normalizers` is the crate whose job *is* rewriting. Every function it
exposes rewrites, the rewrite is named in the function, and nothing else in
these three crates rewrites at all.

**No UTF-16, anywhere.** No public type, no internal buffer, no index. As a
direct consequence:

- **No fabricated replacement characters.** No function in these three crates
  can emit `U+FFFD` unless `U+FFFD` was in the input. `Utf16Token`,
  `to_string_lossy`, `String::from_utf16_lossy` and `zh::split_lossy` are
  deleted rather than fixed. This is the `verbora-distance` search defect
  (`docs/design/rust-native-migration.md` §"Verified defects") removed by the
  same mechanism: delete the representation that made it expressible.
- **No string sentinels.** `CaseTokenizer`'s nine-character `"undefined"`
  spliced into token text is gone with the tokenizer. `ngram_key`'s `")"` is
  gone with the frequency key.
- **No numeric sentinels.** Absence is `Option::None`; zero is a count, never
  "not found".

**No `NaN`.** None of these crates contains floating point, and none acquires
any. Stated positively so it is not re-derived.

**No panics from public entry points**, on any input, under any feature
combination. Two preconditions are enforced, both by making the invalid state
unrepresentable rather than by checking at the point of use: an n-gram order
is `NonZeroUsize`, and a sentence-tokenizer abbreviation set is built by a
`Result`-returning constructor. Everything else is total. In particular
`Source::slice`, the one panicking public function in the current tree, is
deleted with the module that held it.

**The crate root is the entire public surface.** All three crates have private
modules and re-export everything from the root. Twenty `pub mod` declarations
and roughly a hundred items reachable only through them disappear. Because
making a module private un-publishes its `//!` prose, the user-facing half of
that prose moves onto public items in the same change; the rest — which is
almost entirely a description of another implementation — is deleted.

**Determinism, and its limit.** The same input produces the same output on
every platform and every build, for a fixed dependency version set. There is
no global mutable state, no hash-order dependence, and no interior mutability.

But **these crates are not frozen across Unicode versions, and cannot be.**
This is the sharpest deliberate contrast with `verbora-distance`, which
eliminated its last UCD dependency in order to promise frozen results for all
time. Segmentation and normalisation *are* UCD operations: UAX #29 defines
word boundaries in terms of the `Word_Break` property, UAX #15 in terms of
`Decomposition_Mapping` and `Canonical_Combining_Class`. A segmenter that
consulted no character database could only be an ASCII segmenter or a frozen
hand table, and the frozen hand table is precisely the artifact this migration
exists to remove. So:

- The Unicode version is whatever `unicode-segmentation` and
  `unicode-normalization` ship, pinned in `Cargo.lock` and recorded in each
  crate's rustdoc.
- **Any structure that persists tokenizer- or normalizer-derived keys must
  stamp the Unicode version and refuse to load across a change.** Today
  `verbora-tfidf` persists an interned term table (`to_json` / `from_json`)
  and `verbora-classifiers` persists stem-keyed models (`save` / `restore`),
  neither with a stamp. Adding the stamp is those crates' own migration item;
  §5 Step 4 records the obligation and the release note states the hazard.
- A UCD upgrade is a **semver-visible behaviour change** for these crates and
  is released as one.

**No table without a generator.** Any lookup table these crates ship must be
produced by a generator checked into `tools/`, from a versioned input (the
UCD, or a cited publication), with a test that re-derives the table and fails
if it drifts. The three tables in the current tree — `classes.rs`,
`ja/model.rs`, `ja/norm_tables.rs`, `diacritics/table.rs`, `ja/tables.rs` —
are headed "machine-derived … DO NOT EDIT BY HAND" and no generator exists
anywhere in the repository, which makes their provenance unverifiable. They
are deleted, not regenerated.

**What each crate is, in one sentence each.**

- **`verbora-tokenizers`** cuts text at UAX #29 boundaries and returns the
  pieces, borrowed and in order, such that concatenating them reproduces the
  input exactly.
- **`verbora-normalizers`** applies the four Unicode normalization forms and
  one Verbora-defined diacritic fold, each returning `Cow::Borrowed` when and
  only when it changed nothing.
- **`verbora-ngrams`** is padding and character windows: everything else it
  did is `slice::windows` and a three-line `HashMap` fold.

---

## 2. The text unit, per crate

The three crates choose three different units, and none of them is
`verbora-distance`'s. That is the intended outcome: the migration rule
forbids an *implicit or inherited* unit, not disagreement between crates that
answer different questions.

### 2.1 `verbora-tokenizers` — boundaries in scalars, positions in bytes

**One Unicode scalar value is one unit for the purpose of deciding where a
boundary goes. A token is reported as a byte-addressed `&str`.**

UAX #29 §4 assigns a `Word_Break` property value to every *code point* and
states its rules over sequences of code points; §5 does the same for
`Sentence_Break`. The unit is therefore not a choice this crate makes — it is
the unit the algorithm is written in, and adopting anything else would mean
implementing something other than the standard.

Positions are bytes for the same reason `verbora-distance`
`SearchResult::range` is: a `&str` accepts exactly one index type, every
scalar boundary is a byte boundary, and a token that is a borrowed slice
cannot disagree with its own position. The crate ships no offset accessor
(§4.6), so this only shows up as "tokens are `&str`".

*Where the choice is observable.* Today three units coexist inside the crate
(`crates/verbora-tokenizers/src/lib.rs:35-46`), and the UTF-16 half produces
text the caller never supplied:

| Call | Now | Under this contract |
|---|---|---|
| `TreebankWordTokenizer` on `"a😀b"` (via `verbora_core::Tokenizer`) | `["a", "\u{FFFD}", "\u{FFFD}", "b"]` | tokenizer deleted; `WordTokenizer` gives `["a", "b"]` |
| `CaseTokenizer::new().tokenize("İstanbul")` | `["İstanbulundefined"]` | tokenizer deleted; `WordTokenizer` gives `["İstanbul"]` |
| `AggressiveTokenizerHi::new().tokenize("a.b")` | `["ab"]` — a string absent from the input | `["a", "b"]`, both substrings |
| `classes::is_word_en('😀')` | `false` because a surrogate half is not in the class | predicate deleted; the scalar is its own segment and is not a word |

### 2.2 `verbora-normalizers` — the Unicode scalar value

**One `char` is one unit.** UAX #15 defines all four normalization forms as
maps over sequences of code points, and `Canonical_Combining_Class` is a
per-code-point property. There is no smaller unit the operation is defined at,
and a grapheme unit would be strictly larger than the thing being reordered.

*Where the choice is observable.* `normalize_ja` currently matches UTF-16 code
units in its first stage and Unicode scalars in stages two through four, and
that disagreement is exactly what makes `normalize_ja("😀々")` return
`"😀\u{FFFD}"` for well-formed input
(`crates/verbora-normalizers/src/ja.rs:92`, `:99`). The function is deleted
(§3.2); `nfkc("😀々")` is `"😀々"`.

`remove_diacritics` moves from a 820-entry precomposed-scalar table to a
decomposition-based definition, so its unit becomes observable in the other
direction: `remove_diacritics("e\u{0301}")` is unchanged today and is `"e"`
under this contract, because the operation is now defined over the decomposed
sequence rather than over whatever scalar the caller happened to type.

### 2.3 `verbora-ngrams` — the caller's element, and (for text) the scalar

**`ngrams` and `Padded` are generic over `T` and the unit is the element of
the caller's slice. That is stated, not implicit: the crate deliberately does
not choose.** An n-gram is a windowing operation over a sequence; what the
sequence holds is the caller's decision, and a crate that forced a unit here
would only be forcing a tokenizer.

**`char_ngrams` works in Unicode scalars,** and says so in its name. This
replaces `zh`'s UTF-16 code-unit splitting, whose observable failure is total
data loss: `ngrams_zh("👍", 1, None, None)` is
`[["\u{FFFD}"], ["\u{FFFD}"]]` today — two tokens, neither of which occurs in
the input, and neither distinguishable from a `U+FFFD` the caller supplied.
`char_ngrams("👍你好", 2)` yields `["👍你", "你好"]`, each a borrowed
two-scalar slice of the input.

The unit is *not* the grapheme cluster, for `char_ngrams` or anywhere else.
Grapheme clusters are UAX #29 §3 and would be defensible, but they change with
the Unicode version in a way that scalars do not, they make the "every window
is `n` units and a substring" invariant depend on a tailoring, and no consumer
wants them. A caller who wants grapheme windows segments first and uses
`Padded` over the resulting slice.

---

## 3. Per-crate specification

### 3.1 `verbora-tokenizers`

#### The whole public surface

```rust
// crate root; every module is private

/// Yields the UAX #29 word segments of `text` that contain at least one
/// `Alphabetic` scalar or one scalar with `General_Category` in {Nd, Nl, No}.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WordTokenizer;

/// Yields every UAX #29 word segment of `text`, including whitespace and
/// punctuation runs. Concatenating them reproduces `text` exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SegmentTokenizer;

/// Yields the UAX #29 sentences of `text`, optionally suppressing breaks that
/// follow a caller-supplied abbreviation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SentenceTokenizer { /* private: abbreviations */ }

impl SentenceTokenizer {
    pub fn new() -> Self;

    /// # Errors
    /// [`AbbreviationError::Empty`] if any abbreviation is the empty string.
    pub fn with_abbreviations<I, S>(abbreviations: I) -> Result<Self, AbbreviationError>
    where I: IntoIterator<Item = S>, S: Into<String>;

    pub fn abbreviations(&self) -> &[String];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbbreviationError { Empty { index: usize } }
// Display + std::error::Error.

/// Tokenizes many independent documents in parallel.
#[cfg(feature = "parallel")]
pub fn par_tokenize_batch<'a, T>(tokenizer: &T, texts: &[&'a str]) -> Vec<Vec<&'a str>>
where T: verbora_core::BorrowingTokenizer + Sync;

pub use verbora_core::{BorrowingTokenizer, Tokenizer};
```

All three tokenizers implement `verbora_core::BorrowingTokenizer`, which gains
one required method (§3.1, "The trait"). There is no `Tokenize` trait, no
`Utf16Token`, no `Pattern`, no `Option` return, no `bool` return.

#### The trait

`verbora_core::BorrowingTokenizer` gains the lazy primitive and loses nothing:

```rust
pub trait BorrowingTokenizer: Tokenizer {
    /// The primitive. Every token is a contiguous slice of `text`.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str>;

    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str> {
        self.tokens(text).collect()
    }

    /// Appends; does **not** clear `out`.
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        out.extend(self.tokens(text));
    }
}
```

`verbora_tokenizers::Tokenize` is deleted. It existed because the old
tokenizers needed a GAT (`type Token<'a>`) to express four different token
shapes; with one shape there is nothing to abstract, and two traits with a
`tokenize` method each made an unqualified call ambiguous whenever both were
imported — a defect the current crate documents rather than fixes
(`lib.rs:71-74`).

`par_tokenize_batch` is a free function rather than a defaulted trait method
so that `verbora-core` acquires neither a `parallel` feature nor a `rayon`
dependency for one method body. Per `AGENTS.md` § Rayon Policy its body is
`texts.par_iter().map(|t| tokenizer.tokenize_borrowed(t)).collect()` and
nothing else; output order matches input order.

#### `WordTokenizer` and `SegmentTokenizer`

`SegmentTokenizer` yields the segments produced by the UAX #29 §4 word
boundary rules WB1–WB999, in order. `WordTokenizer` yields the subsequence of
those segments containing at least one scalar with the `Alphabetic` property
or with `General_Category` in `{Nd, Nl, No}`.

**Both exist because the filter is irreversible.** `SegmentTokenizer` carries
the guarantee that concatenation reproduces the input, which a highlighter, a
re-assembler or an offset consumer needs and which no filtered form can offer;
`WordTokenizer` is what every consumer in this workspace actually wants, and
requiring each of them to write the filter is how four independent copies of a
boundary rule came to exist in the first place. The difference is one
sentence, which is the test `AGENTS.md` § Choosing the Right API applies.

**Guarantees.**

1. `SegmentTokenizer.tokens(t).collect::<String>() == t`, for every `t`.
2. Every token of either tokenizer is a contiguous slice of the input, at
   strictly increasing, non-overlapping byte ranges.
3. Neither yields the empty string, for any input. `tokens("")` is empty.
4. `WordTokenizer.tokens(t)` is a subsequence of `SegmentTokenizer.tokens(t)`,
   with equal pointer identity for corresponding tokens.

**What moves, and it is not a small list.** The sixteen language-specific
tokenizers are deleted (§3.4), so every consumer that used one is now on UAX
#29. The differences below are the ones that reach real corpora; none is a
rounding difference and none fails to compile at the call site once the type
name is changed.

| Input | `AggressiveTokenizer` (deleted) | `WordTokenizer` | Rule |
|---|---|---|---|
| `"well-known"` | `["well-known"]` (`-` was in the class) | `["well", "known"]` | `U+002D` is `Word_Break=Other`; WB999 breaks |
| `"and/or"` | `["and/or"]` | `["and", "or"]` | `U+002F` is `Word_Break=Other` |
| `"don't"` | `["don't"]` | `["don't"]` | WB6/WB7 over `MidNumLetQ` |
| `"3.14"` | `["3", "14"]` | `["3.14"]` | WB11/WB12, `Numeric × MidNumLet × Numeric` |
| `"1,000"` | `["1", "000"]` | `["1,000"]` | WB11/WB12, `MidNum` |
| `"node_js"` | `["node", "js"]` | `["node_js"]` | WB13a/WB13b, `ExtendNumLet` |
| `"Äpfel"` (de) | `["pfel"]` — `Ä` was a separator | `["Äpfel"]` | `Ä` is `ALetter` |
| `"A B"` (id) | `[]` — every capital was a separator | `["A", "B"]` | ASCII capitals are `ALetter` |
| `"a×b÷c"` (es) | `["a×b÷c"]` — Latin-1 range swallowed `×`/`÷` | `["a", "b", "c"]` | `U+00D7`/`U+00F7` are `Other` |
| `"привет, мир"` (it) | `[]` — ASCII-only `\W` | `["привет", "мир"]` | Cyrillic is `ALetter` |
| `"café naïve"` (en) | `["caf", "na", "ve"]` | `["café", "naïve"]` | `é`, `ï` are `ALetter` |
| `"日本語"` | `["日本語"]` or `[]` by tokenizer | `["日", "本", "語"]` | Han is `Other`; WB999 |
| `"すもももももも"` | `TokenizerJa` gave a nine-token segmentation | one segment per scalar | see below |

The last row is the honest limitation and it is stated in the rustdoc, not
hidden: **UAX #29 §4 explicitly says its default rules do not segment
languages that do not use spaces** — Thai, Lao, Khmer, Myanmar, Chinese,
Japanese — and that a dictionary or statistical approach is required.
Verbora ships no such segmenter, because the one currently in the tree is
1,480 lines of weights extracted from a re-implementation of TinySegmenter
with no version, checksum or upstream URL recorded, and no generator anywhere
in the repository. An unattributable model is worse than an absent one: it
cannot be audited, updated or defended. §4.4 records what shipping one later
would have to look like.

#### `SentenceTokenizer`

Sentences are the segments produced by the UAX #29 §5 sentence boundary rules
SB1–SB998, in order, **with no trimming**. A sentence includes its own
trailing whitespace, so concatenation reproduces the input exactly, and
`tokens("   ")` is `["   "]` — one segment — rather than today's `[""]`, a
token that occurs nowhere in the input.

Callers who want trimmed sentences write `.map(str::trim)`. Trimming is
removed rather than defaulted-off because a tokenizer that trims is a
tokenizer whose tokens are not substrings, and §1 has no exceptions.

**Abbreviations are a tailoring, and the standard says one is needed.** UAX
#29 §5 notes that the default rules break after any `STerm` and that
abbreviation handling requires tailoring. Verbora's tailoring is stated
exactly:

> Let `B` be the set of boundary positions the default rules produce over
> `text`. A position `b` with `0 < b < text.len()` is **suppressed** if some
> abbreviation `a` in the set satisfies
> `text[..b].trim_end_matches(char::is_whitespace).ends_with(a)`.
> Suppressed boundaries are not emitted; the segments on either side are
> joined. The final boundary at `text.len()` is never suppressed.

Four consequences, each deliberate and each pinned:

- **Matching is case-sensitive and is an exact scalar-sequence comparison.**
  Case-insensitive matching would require a case-folding decision (UAX #21)
  that belongs to the caller, and the current implementation's
  `ci_match_at` is ECMAScript `Canonicalize`, which refuses to fold non-ASCII
  onto ASCII — a rule with no bearing on text. A caller wanting both casings
  supplies both strings.
- **`char::is_whitespace` is Unicode `White_Space`.** This is the one place in
  the crate that consults a whitespace set, and it is Unicode's, not
  ECMAScript's. The current `verbora_core::whitespace::is_whitespace` includes
  `U+FEFF` and excludes `U+0085`; under this contract `U+0085` NEXT LINE is
  whitespace and `U+FEFF` is not. That function stays in `verbora-core` for
  its other consumers and is no longer used here.
- **Suppression is suffix matching, so it can over-suppress.** With `"No."` in
  the set, `"Visit the casino. Then leave."` is one sentence, because
  `"casino."` ends with `"No."` is false — but with `"no."` in the set it is
  true. This is stated, with that example, in the rustdoc. A word-boundary
  qualification was considered and rejected (§4.3).
- **An empty abbreviation is unrepresentable.** `with_abbreviations([""])`
  returns `Err(AbbreviationError::Empty { index })`. Today it is accepted and
  matches at every position, minting one placeholder per character: measured
  at 14.2 ms for 400 bytes, 34.8 ms for 800 and 146.0 ms for 1600 — quadratic,
  from a one-character constructor argument. A `Result` constructor removes
  the state rather than documenting the hazard, exactly as `DamerauCosts::new`
  does in the distance contract.

The whole placeholder-substitution machinery goes with it: the URI mask, the
number mask, the `{{ABBREV_n}}` / `{{DELIM_n}}` namespace that mangles user
text spelling it, the single-pass ordered unmask that leaks nested
placeholders into output, and the `$`-expansion that can splice unrelated
document text into a sentence (`"Visit www.a.b/$'x. Next."` currently returns
`["Visit www.a.b/ Next.x. Next."]`). None of it has an authority and all of it
is deleted.

#### Choosing the right API

Required by `AGENTS.md` § Choosing the Right API and written as part of the
implementation, not deferred:

| Call | Use when | Allocates | Notes |
|---|---|---|---|
| `tokens(text)` | streaming, composing with `map`/`filter`, early exit | nothing | the primitive; everything else is defined on it |
| `tokenize_borrowed(text)` | you want a `Vec` and the input outlives it | one `Vec` of `&str` | the correct default for most programs |
| `tokenize_borrowed_into(text, &mut buf)` | one buffer reused across a corpus | nothing once warm | **`buf` is not cleared** — forgetting `buf.clear()` is a silent correctness bug |
| `Tokenizer::tokenize(text)` | tokens must outlive the input | one `String` per token | the owned path; lossless, because tokens are already valid `&str` |
| `par_tokenize_batch(&t, texts)` | many independent documents | one `Vec` per document | crossover `UNMEASURED` (§7); prefer a sequential loop for a handful of short strings |

### 3.2 `verbora-normalizers`

#### The whole public surface

```rust
// crate root; every module is private

/// Canonical Decomposition. UAX #15 §1.2.
pub fn nfd(s: &str) -> Cow<'_, str>;
/// Canonical Decomposition followed by Canonical Composition. UAX #15 §1.2.
pub fn nfc(s: &str) -> Cow<'_, str>;
/// Compatibility Decomposition. UAX #15 §1.2.
pub fn nfkd(s: &str) -> Cow<'_, str>;
/// Compatibility Decomposition followed by Canonical Composition. UAX #15 §1.2.
pub fn nfkc(s: &str) -> Cow<'_, str>;

/// Removes combining diacritical marks. See "The definition" below.
pub fn remove_diacritics(s: &str) -> Cow<'_, str>;

#[cfg(feature = "parallel")]
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>;
```

Six functions. The crate's dependency list becomes `unicode-normalization`
plus optional `rayon`.

#### The `Cow` contract

Every function returns `Cow::Borrowed(s)` **if and only if** the result is
byte-identical to `s`. This is a guarantee, not a fast-path description: a
caller branching on `matches!(r, Cow::Borrowed(_))` to skip downstream work is
writing correct code. The quick-check properties (`NFC_QD`, `NFKC_QD`, …,
UAX #15 §9) permit deciding this without materialising the result in the
common case; where a quick check answers `Maybe`, the implementation compares
and returns `Borrowed` if it can.

#### The definition of `remove_diacritics`

> `remove_diacritics(s)` is `s` under Canonical Decomposition (NFD, UAX #15
> §1.2), with every scalar whose `Canonical_Combining_Class` is non-zero
> removed, under Canonical Composition (NFC).

`Canonical_Combining_Class` is a UCD property (The Unicode Standard §4.3,
*Combining Classes*). Three parts of the definition are load-bearing and each
is justified:

- **NFD first**, so the function is independent of the caller's normalization
  form. Today `remove_diacritics("é")` folds and `remove_diacritics("e\u{301}")`
  does not, which makes the function's answer depend on how the text was typed.
- **`ccc != 0` rather than `General_Category ∈ {Mn, Mc, Me}`.** The
  non-zero-combining-class marks are exactly the ones canonical ordering
  reorders — the technical sense of "accent" — and the two sets differ where
  it matters. Thai vowel signs, Devanagari matras and Hangul jamo are `ccc = 0`
  and survive, so Thai and Indic text is not destroyed; Hebrew niqqud
  (`ccc` 10–26), Arabic harakat (`ccc` 27–34) and the Devanagari nukta
  (`ccc = 7`) are non-zero and are removed, which is the operation those
  scripts call diacritic removal. Stripping all marks would silently mangle
  three major scripts for the benefit of none.
- **NFC last**, because NFD decomposes Hangul syllables into `ccc = 0` jamo
  and only composition puts them back. Without it `remove_diacritics("한국")`
  would return decomposed jamo — a different string that renders the same,
  which is the class of surprise this contract exists to remove.

**Guarantees.** Idempotent: `remove_diacritics(remove_diacritics(s)) ==
remove_diacritics(s)`, for every `s`. Position-independent: the result is the
concatenation of the results on any decomposition of `s` at `ccc = 0`
boundaries, so the same word folds the same way wherever it appears. Both are
new; both are currently false (below).

**What does not fold, and why.** The definition is honest about its own edges
and the rustdoc leads with them rather than burying them:

| Input | Result | Reason |
|---|---|---|
| `"ø"`, `"Æ"`, `"đ"`, `"ł"`, `"ħ"`, `"ŋ"`, `"ı"` | unchanged | no canonical decomposition; the mark is part of the letter's identity |
| `"ß"` | `"ß"` | not a diacritic. `ß → ss` is *case folding*, UAX #21 |
| `"Ａ"` (fullwidth), `"Ⓐ"` (circled), `"ǅ"` | unchanged | compatibility decompositions, not canonical. Compose `nfkc` first if that is wanted |
| `"ſ"` | `"ſ"` | a letter. Today it returns `"l"`, because the source listed long-s in the `l` class |
| `"Å"` (`U+212B`) | `"A"` | `U+212B` has a canonical singleton decomposition to `U+00C5`, which decomposes further |
| `"İ"` | `"I"` | `U+0130` decomposes to `I` + `U+0307` (`ccc = 230`) |

**What this replaces.** The shipped table is 820 precomposed BMP scalars
dumped from another implementation's runtime, and its bugs are documented as
"part of the contract". Beyond `ſ → l`, it folds `Ⓐ → A` and `Ａ → A` under a
function named for diacritics, maps `ǅ → Dz` (a case mapping), and never
touches decomposed input. `normalize_no` and `normalize_sv` fold only the
*first* occurrence of each needle — `normalize_no("ààà") == "aàà"`, and
feeding that back gives `"aaà"` — so they are neither idempotent nor
position-independent, and `verbora-stemmers` applies them to whole documents,
which means the same word stems differently depending on what preceded it in
the corpus. Both are deleted; §5 Step 2 covers the migration.

#### Cut: the Japanese normalizers

`normalize_ja` and the seventeen `ja::converters` functions are deleted, along
with 2,612 lines of tables. **NFKC subsumes the defensible part and covers
more of it**: compatibility decomposition maps fullwidth forms to halfwidth
and back, decomposes the halfwidth voiced sound mark `U+FF9E` to the combining
`U+3099` so that canonical composition recombines `ｶ` + `ﾞ` into `ガ`, and
decomposes the whole of `U+3300..U+33FF`. The shipped `fix_composite_symbols`
covers 149 of those 256 code points, omitting every SI-unit square that NFKD
does decompose.

What NFKC does not do, and what Verbora therefore no longer ships:

- **Iteration-mark expansion** (`々`, `ゝ`, `ゞ`, `ヽ`, `ヾ`). Not a Unicode
  operation; an orthographic rewrite. The shipped one is not idempotent —
  `normalize_ja("あ々々")` is `"ああ々"`, then `"あああ"` — so it cannot be
  used to canonicalise text for comparison, which is what a normalizer is for.
- **The small-tsu rewrite** (`っな → んな`). A *phonetic* rewrite shipped under
  the name `fix_fullwidth_kana`, which also breaks the hiragana/katakana pair:
  `hiragana_to_katakana("っな")` is `"ンナ"`, whose katakana-to-hiragana is
  `"んな"`, so the two functions are not an involution.
- **Hiragana ↔ katakana conversion.** This one is citable and worth having,
  but it is a *transliteration*, not a normalisation, and it belongs to
  `verbora-transliterators` (migration item 3). Recorded so it is not
  re-derived: The Unicode Standard §12.4 encodes the two syllabaries in
  parallel, so `U+3041..=U+3096` maps to `U+30A1..=U+30F6` and
  `U+309D..=U+309E` to `U+30FD..=U+30FE`, both at an offset of `0x60`;
  `U+30F7..=U+30FA` have no hiragana counterpart.

The one behaviour NFKC gives that the shipped code does not is that `U+3099`
and `U+309A` — the *combining* voiced marks — now compose. Today only the
spacing `U+309B`/`U+309C` are handled, so NFD-normalised Japanese passes
through untouched while legacy text is rewritten.

#### Cut: English contraction expansion

`normalize` and `normalize_token` are deleted. They have no library consumer
in the workspace, no authority of any kind, and three independent defects:
they **delete every non-ASCII character** from any token in which a rule fired
(`normalize(["café's"]) == ["caf", "is"]`, `normalize(["日本's"]) == ["", "is"]`)
while leaving bare `"naïve"` intact; they emit empty strings as tokens; and
whether a multi-word string is split at all depends on whether a rule fired
(`normalize_token("I'm here")` is one token, `normalize_token("can't can't")`
is four). The rules themselves encode two regex accidents as contract — a
four-character set `[azAZ]` read as a range, and a `n't` whose `n` is matched
lowercase-only while its tail is case-insensitive, so `"N'T"` does not expand
and `"n'T"` does.

An English contraction lexicon is a reasonable thing to want. It is a
*lexicon*, not a normalisation form, it is English-only, and if it returns it
returns as a table with a cited source in a crate that says so in its name.

#### Parallelism

`par_remove_diacritics_batch` survives, unchanged in shape, per `AGENTS.md`
§ Rayon Policy: `inputs.par_iter().map(remove_diacritics).collect()`, order
preserving. No `par_*` variant is added for the four normalization forms —
they are adapters over `unicode-normalization` with no Verbora-side per-item
work to fan out, so `inputs.par_iter().map(nfc).collect()` at the call site is
the same code with one fewer name. The benchmark tables currently baked into
`diacritics.rs:180` and `ja.rs:322`/`:534` are removed rather than adjusted;
per `CLAUDE.md` no unmeasured number is published, and the algorithm changes
under this contract (§7).

### 3.3 `verbora-ngrams`

#### The whole public surface

```rust
// crate root; every module is private

/// The `n`-grams of `seq`: every window of `n` consecutive elements, in order.
pub fn ngrams<T>(seq: &[T], n: NonZeroUsize) -> std::slice::Windows<'_, T>;

/// `seq` with `n - 1` copies of each supplied symbol prepended and appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Padded<T> { /* private: buf, n */ }

impl<T: Clone> Padded<T> {
    pub fn new(seq: &[T], n: NonZeroUsize, start: Option<&T>, end: Option<&T>) -> Self;
    pub fn ngrams(&self) -> std::slice::Windows<'_, T>;
    pub fn as_slice(&self) -> &[T];
}

/// The `n`-grams of `text`'s Unicode scalars, as borrowed slices of `text`.
pub fn char_ngrams(text: &str, n: NonZeroUsize) -> CharNGrams<'_>;

#[derive(Debug, Clone)]
pub struct CharNGrams<'a> { /* private */ }
// Iterator<Item = &'a str> + ExactSizeIterator + FusedIterator + DoubleEndedIterator
```

Four items. The crate's dependency list becomes empty.

#### `ngrams` — why it exists when `slice::windows` does

It is `seq.windows(n.get())`, and the only difference is that
`slice::windows` **panics** when the size is zero while `ngrams` cannot be
called with zero. That difference is exactly the distance contract's
precondition rule — make the invalid state unrepresentable — and it is the
whole justification for the function. Nothing else is added: no `bigrams`, no
`trigrams`, no `multrigrams`, no owned variant, no statistics.

`n > seq.len()` yields an empty iterator, which is `slice::windows`' own
behaviour and the only sensible answer.

#### `Padded` — the padding definition

Padding is the crate's one piece of real content, and it is redefined from the
ground up.

> `Padded::new(seq, n, start, end)` is the sequence formed by prepending
> `k` copies of `start` (if supplied) and appending `k` copies of `end` (if
> supplied), where `k = n - 1`. `Padded::ngrams()` is `ngrams` over that
> sequence.

Cite: Jurafsky & Martin, *Speech and Language Processing* (3rd ed.) §3.1,
which augments each sequence with `n - 1` start symbols so that every real
element appears as the final element of exactly one window. The **symmetry** —
`n - 1` end symbols rather than the single `</s>` a language model uses for
probability normalisation — is a Verbora decision, and the reason is that this
crate's two symbols are independent options: a rule reading "`n - 1` of the
start symbol but exactly one of the end symbol" is asymmetric for a reason
that does not apply when only the end symbol is supplied. Symmetric padding
makes the first and last real elements each appear in exactly `n` windows,
which is the property feature extraction wants.

**Guarantees.** Every window has exactly `n` elements. Windows are emitted in
left-to-right position order. The window count is `len + k_start + k_end - n + 1`
when that is positive and `0` otherwise. `n == 1` gives `k == 0`, so no
padding is added — not because an argument is discarded, but because zero
copies is what the formula says; a caller wanting boundary symbols in a
unigram model prepends them to the sequence.

**Overflow is total, not a panic.** The padded length and window count are
computed with checked arithmetic; if either overflows `usize`, `Padded` holds
an empty buffer and `ngrams()` is empty with `len() == 0`. This closes three
confirmed panics in the current implementation — a `Vec::with_capacity`
capacity overflow reachable from every public entry point that accepts `n` and
a pad symbol, a `size_hint` add overflow reachable from `.len()` and from
`collect()`, and a `2 * len` multiply overflow whose guarding comment ("a
slice length never exceeds `isize::MAX`") is false for zero-sized element
types and which *silently wraps to a wrong slice start* in release.

**What this replaces.** `clamp_slice_start` reproduces JavaScript
`Array#slice`'s re-anchoring of a negative start index to `length + start`, and
both padding halves clamp independently. The consequences, all confirmed:
padded tuples are frequently shorter than `n`; the output order is broken —
`ngrams(["a","b","c"], 5, Some("<s>"), Some("</s>"))` emits `["c","</s>"]`
*before* the tuple that should precede it; an **empty** sequence with `n = 4`
and both symbols yields six tuples of lengths 3,2,1,1,2,3. Under this contract
that call yields three windows of four elements each, and an empty sequence
with `n = 4` and both symbols yields `2n - 2 - n + 1 = 3` windows drawn from
`[S,S,S,E,E,E]`.

#### `char_ngrams`

Yields `&str` slices of exactly `n` consecutive Unicode scalars, borrowed from
`text`, in order. `char_ngrams("abc", 2)` is `["ab", "bc"]`;
`char_ngrams("👍你好", 2)` is `["👍你", "你好"]`; `char_ngrams("ab", 3)` is
empty. `ExactSizeIterator::len` is `text.chars().count().saturating_sub(n - 1)`.

Every yielded slice is a substring of `text`, so the lossy-splitting class of
defect cannot occur. No padding variant is offered: a caller who wants padded
character n-grams builds `Padded` over `text.chars().collect::<Vec<_>>()`,
which is one line and makes the allocation visible.

#### Cut: statistics, string input, and the global tokenizer

The `stats`, `text` and `tokenizer` modules are deleted in full — twenty-three
public items.

**`ngram_key` and `NGramStats`.** The key is not injective:
`ngram_key(&["a, b"]) == ngram_key(&["a", "b"]) == "(a, b)"`, and elements are
not escaped for parentheses. Because `nr` is consumed as a Good–Turing
count-of-counts, a collision corrupts a statistical estimate rather than a
lookup. `frequency` returns `0` for an absent key — a numeric sentinel §1
forbids. `number_of_ngrams` is always exactly `ngrams.len()` and all fields
are `pub`, so it can be made inconsistent. `nr` counts padding tuples, so an
empty corpus with padding reports `nr == {1: 4}` and a Good–Turing estimator
fed it infers four hapax n-grams and an unseen mass of `1.0`.

The replacement is three lines at the call site, keyed on the n-gram itself
rather than on a lossy rendering of it, and it is shown in the crate rustdoc:

```rust
let mut counts: HashMap<&[&str], u64> = HashMap::new();
for w in ngrams(&tokens, n) { *counts.entry(w).or_default() += 1; }
```

**The `_str` family** (eight functions) is deleted. `ngrams(&tokenizer
.tokenize_borrowed(text), n)` is the composition, and writing it out is the
point: it makes the tokenizer an argument rather than a hidden policy, which
is §1's rule applied to a crate that was silently deleting every character its
tokenizer did not recognise (`ngrams_str("café", 1, None, None) == [["caf"]]`,
with no signal to the caller).

**The process-global tokenizer** — `set_tokenizer`, `reset_tokenizer`,
`current_tokenizer`, `tokenize`, the `NGramTokenizer` trait, `FnTokenizer` and
a fourth private copy of a word-boundary rule — is deleted. It is a
`static RwLock<Option<Arc<dyn …>>>` that makes eight public functions
non-deterministic and non-reentrant across the whole process; the crate's own
tests need a private mutex to survive it, and the site page already tells
users to avoid it. Its stated justification is that another implementation's
spec suite depends on one test having run before another. Three
`.expect("tokenizer lock poisoned")` sites go with it, one of which is on the
path of every string-input call.

**The `zh` module** is deleted (§2.3). `char_ngrams` is its lossless
replacement; `ngrams_zh_utf16`, the only lossless path it offered, was not
re-exported at the crate root while the lossy one was.

### 3.4 Removed from the public surface

| Crate | Removed | Replacement |
|---|---|---|
| tokenizers | `AggressiveTokenizer` and its 15 language variants | `WordTokenizer` |
| tokenizers | `CaseTokenizer` | `WordTokenizer` |
| tokenizers | `TokenizerJa`, `ja/model.rs`, `ja/norm_tables.rs`, `ja/normalize.rs` | nothing; see §3.1 and §4.4 |
| tokenizers | `TreebankWordTokenizer` | nothing; deferred, §4.5 |
| tokenizers | `RegexpTokenizer`, `Pattern`, `WordPunctTokenizer`, `OrthographyTokenizer` | `SegmentTokenizer`, or `regex` directly |
| tokenizers | `SentenceTokenizerNew` (type alias) | `SentenceTokenizer` |
| tokenizers | `Utf16Token` | `&'a str` |
| tokenizers | `Tokenize` trait | `verbora_core::BorrowingTokenizer` |
| tokenizers | `pub mod` × 10; `classes` (19 predicates), `scan` (`CharClass`, `WordRuns`, `Source`, `SourceRuns`), `whitespace` (10 items), 15 `*Class` markers, 5 iterator types | private or deleted |
| tokenizers | `trim_edge_empties` re-export | stays in `verbora-core`; no tokenizer emits edge empties |
| tokenizers | `regex` dependency | — |
| normalizers | `normalize`, `normalize_token` | nothing; §3.2 |
| normalizers | `normalize_no`, `normalize_sv` | `remove_diacritics` (different, and better; §5 Step 2) |
| normalizers | `normalize_ja`, `ja::converters` (17 fns), `ja/tables.rs`, `diacritics/table.rs`, `table.rs` | `nfkc`, `remove_diacritics` |
| normalizers | `pub mod` × 4 | private |
| ngrams | `bigrams`, `trigrams`, `multrigrams`, `ngrams_owned`, `ngrams_iter`, `NGramIter` | `ngrams` |
| ngrams | `stats` module (7 items) | a three-line `HashMap` fold |
| ngrams | `text` module (10 items) | `ngrams(&t.tokenize_borrowed(s), n)` |
| ngrams | `tokenizer` module (8 items, incl. global state) | `verbora-tokenizers` |
| ngrams | `zh` module (8 items) | `char_ngrams` |
| ngrams | `verbora-core`, `rustc-hash` dependencies | — |

Considered and **not added**: byte-offset accessors (§4.6); a `Padding<T>`
enum (§4.7); a `Tokenize` GAT retained for future non-borrowing tokenizers
(§4.8); grapheme-cluster segmentation (§4.9).

---

## 4. Rejected alternatives

### 4.1 Keeping the per-language character classes

Rejected. Nineteen `is_word_*` predicates whose entire stated justification is
"derived from *(regex)* by enumerating the Basic Multilingual Plane". None is
a text-segmentation rule; every one is an artifact of a regex literal, and
several are outright bugs the crate reproduces on purpose: German omits the
uppercase umlauts so `Ä Ö Ü` split words; Indonesian has no `i` flag so every
uppercase ASCII letter is a separator and `"A B"` tokenizes to `[]`; Spanish's
raw Latin-1 range swallows `×` and `÷`; Italian's `\W` is ASCII-only so
`"привет, мир"` tokenizes to `[]`.

The strongest counter-argument is that per-language tokenization is a real
thing and UAX #29 is language-neutral. It is answered by looking at what the
per-language content actually *is*: sixteen tokenizers, and the only
differences between them are which accented Latin letters and which Cyrillic
ranges are in the class. That is not linguistics; it is the character
repertoire, and UAX #29's `Word_Break` covers the whole repertoire at once.
The genuinely language-dependent cases — Thai, Khmer, Chinese, Japanese —
are exactly the ones none of the sixteen handled.

### 4.2 A hand-frozen boundary table instead of `unicode-segmentation`

Rejected, and this is the closest call in the document, because
`verbora-distance` chose exactly this and gained a frozen-forever guarantee
from it.

It goes for two reasons. First, the guarantee is not available here: freezing
a boundary rule means freezing the `Word_Break` property assignment, and a
frozen assignment is wrong for every character encoded after the freeze — a
tokenizer that treats a 2027 script as unassigned punctuation is not stable,
it is broken. `verbora-distance` could freeze because comparing two scalars
for equality needs no property at all. Second, a frozen table with no
generator in the tree is the exact artifact being removed; a frozen table
*with* a generator is a Unicode version pin with extra steps, and
`Cargo.lock` already is one.

The cost is stated rather than minimised: §1's persistence clause and §5 Step
4's version-stamp obligation exist because of this decision, and a UCD upgrade
is a semver-visible change.

### 4.3 Qualifying abbreviation suppression by a word boundary

Rejected. The suffix rule over-suppresses: with `"no."` in the abbreviation
set, `"casino."` suppresses a break. Requiring the match to start at a word
boundary would fix that case — and would break `"e.g."`, `"i.e."`, `"Ph.D."`
and every other abbreviation that contains an interior period, because the
period is itself a boundary and the match would have to span it. A rule that
is right for `"Dr."` and wrong for `"e.g."` is worse than one that is right
for both and occasionally over-suppresses on a suffix collision, which the
caller controls by choosing the abbreviation set. The over-suppression is
documented with a worked example rather than patched.

### 4.4 Shipping a Japanese word segmenter

Cut, deferred, not rejected on principle. Dictionary or statistical
segmentation for Japanese is genuinely wanted and UAX #29 §4 says so itself.
It is cut from *this* contract because the only implementation available is
1,480 lines of weights extracted from a re-implementation of TinySegmenter,
with no version, no checksum, no upstream URL, and no generator in the
repository — so it cannot be audited or updated, and "reproduce it exactly"
is not a specification anyone can check.

Recorded so it is not re-derived: a future Japanese segmenter arrives as a
separate crate with its own model file, a checked-in generator, a cited source
(Kudo's TinySegmenter with a pinned version, or an ipadic/unidic-based
approach), and a licence review — not as a table pasted into a tokenizer.
Adding it later is not a breaking change.

### 4.5 Re-deriving the Penn Treebank tokenizer now

Cut, deferred. PTB tokenization has a genuine published authority —
MacIntyre's 1995 `tokenizer.sed`, distributed with the Treebank — and it is
the convention parsers and taggers expect, so this is not an abstraction with
no consumer.

It is cut because the shipped implementation is not working behaviour to
preserve: `CONTRACTIONS_3` contains `\b(Whad)(dd)(ya)\b`, with three `d`s, so
it can never match real text and a test pins the wrong output; `FINAL_PERIOD`
uses `\. *(\n|$)` with no `m` flag, so tokenization is position-dependent
(`"home."` mid-text stays one token, at end-of-input it splits); and every
`\w`/`\W`/`\b` is forced ASCII-only, so `"café"` tokenizes to `["caf", "é"]`.
Re-deriving it faithfully is real work with its own token type — PTB
directionalizes quotes, so its tokens are *not* all substrings and it needs
`Cow<'a, str>`, which would reintroduce the GAT §3.1 deletes. Shipping a
half-derived version now is the aspirational surface this contract is supposed
to avoid.

Recorded: it returns as its own module with `type Token<'a> = Cow<'a, str>`,
derived rule-by-rule from the sed script with each rule's line cited, or not
at all.

### 4.6 Byte-offset accessors on tokenizers

Cut. `tokens_with_ranges` or a `Token { text, range }` struct would serve
highlighting, and `unicode-segmentation` exposes the index-carrying iterators
that would back it at no extra cost. It goes because it has no consumer in the
workspace, because `SegmentTokenizer`'s concatenation guarantee makes offsets
recoverable by running length for the caller who needs them, and because
adding it later is a pure addition. Doubling the method count of every
tokenizer for a speculative consumer is what the distance contract's §4.3
declined to do for sequence metrics.

### 4.7 A `Padding<T>` enum instead of two `Option<&T>` parameters

Rejected. The audit-of-shape argument for an enum is that the two `Option`s
are really one policy, and that was true when `n == 0` was legal and `n == 1`
silently discarded both symbols. Both are gone: `n` is `NonZeroUsize` and
`k = n - 1 == 0` is the arithmetic rather than a skipped loop. What remains is
two genuinely independent options — pad the start, pad the end — and an enum
with four variants and a lifetime parameter expresses that with more syntax
and no more safety.

### 4.8 Keeping the `Tokenize` GAT for future non-borrowing tokenizers

Rejected. A GAT that abstracts over one type is not an abstraction. If a
tokenizer that must rewrite its input is ever added — a Treebank tokenizer
(§4.5) is the concrete candidate — it will need `Cow<'a, str>`, and the
right move then is to add the associated type back in the same change that
adds the tokenizer needing it, when its shape is known. Carrying it now costs
every consumer a `T::Token<'a>` bound and every reader a question with no
answer, and it perpetuates the two-traits-one-method-name ambiguity the
current crate documents.

### 4.9 Grapheme clusters as the tokenizer's or `char_ngrams`' unit

Rejected as a default, for the reasons `docs/design/distance-contract.md` §4.2
gives and one more: UAX #29 word segmentation is *already* grapheme-aware
where it matters, because WB4 attaches `Extend`, `Format` and `ZWJ` to the
preceding character, so a word token never splits a combining sequence. Making
the unit grapheme clusters would change only `char_ngrams`, where it would
make the "every window is a substring of exactly `n` units" invariant depend
on a tailoring and would tie character n-gram keys — which language
identification persists — to the Unicode version at a second point.

### 4.10 Folding `verbora-ngrams` into another crate

Considered. After §3.3 the crate is four public items and no dependencies,
which is small enough to ask whether it should exist. It stays because the
question is a workspace-layout question with its own consumers (a published
crate name, a benchmark harness, a site section) and because merging it would
be a second breaking change layered on this one for no behavioural gain. If
the workspace is ever consolidated, this crate is a candidate; that decision
is not this contract's.

### 4.11 Keeping ECMAScript `\s` as the whitespace set

Rejected. `verbora_core::whitespace::is_whitespace` includes `U+FEFF` and
excludes `U+0085`, which is the exact complement of Unicode `White_Space` at
those two points, and the consequence is reachable: today
`AggressiveTokenizerFa::new().tokenize("\u{85}")` returns `["\u{85}"]` — NEL
is a token — while `tokenize("\u{feff}")` returns `[]` — BOM is whitespace.
ECMA-262 is a published standard, so this is documented rather than folklore,
but the reason the tokenizers adopted it is compatibility with one
implementation, not a property of text. Under this contract the tokenizers
consult a whitespace set in exactly one place (§3.1, abbreviation
suppression) and it is Unicode's. The `verbora-core` function stays for
`verbora-tagger` and `verbora-classifiers`, which have their own migration
items.

---

## 5. Implementation plan

Six steps. Each compiles, passes `cargo test --workspace --all-features` in
debug and release, and passes
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo fmt --check` on its own. **`cargo check` must be run with
`--all-targets`**: plain `--workspace` skips tests and benches and has already
hidden two real breakages in this migration.

**No benchmarks are run at any point during implementation.** See §7.

Site snippets under `site/**/*.md` are compiled and executed by
`site/check-snippets.py` through `crates/verbora-examples`, so a stale
executable snippet is CI breakage; **prose tables asserting deleted behaviour
are caught by nothing and are the larger risk.** Per `CLAUDE.md` the site half
of each step is delegated to `doc-sync` with a self-contained brief but lands
in the same commit as its code.

### Step 1 — Dependencies and the shared trait

Pure addition; nothing is deleted and no behaviour changes.

- Workspace `Cargo.toml`: add `unicode-segmentation` and
  `unicode-normalization`. **Record the resolved crate versions and the
  Unicode version each implements**, and run the licence review `AGENTS.md`
  § Licensing requires before either lands.
- `crates/verbora-core/src/lib.rs:88` — add
  `fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str>;` to
  `BorrowingTokenizer` as the required method; default
  `tokenize_borrowed` and `tokenize_borrowed_into` on it.
- Implement `tokens` for the existing `BorrowingTokenizer` impls so the
  workspace still builds: `aggressive.rs:111`, `aggressive.rs:378`,
  `verbora-ngrams/src/tokenizer.rs:126`. Each is one line delegating to the
  iterator it already has.

**Blast radius: three impl sites, all loud. No silent breakage.**

### Step 2 — `verbora-normalizers`

- Delete `english.rs`, `nordic.rs`, `ja.rs`, `ja/tables.rs`,
  `diacritics/table.rs`, `table.rs`. Rewrite `diacritics.rs` to the §3.2
  definition. Add `nfc`/`nfd`/`nfkc`/`nfkd`. Make all modules private and
  re-export from the root, relocating the user-facing half of the `//!` prose
  at `diacritics.rs:1`, `english.rs:1`, `nordic.rs:1`, `ja.rs:1` and
  `ja.rs:152` onto public items first — the rest describes another
  implementation and is deleted.
- Remove the baked-in benchmark numbers from `diacritics.rs:180`,
  `ja.rs:322`, `ja.rs:534`, and the citations of `site/performance/*` and
  `docs/PERFORMANCE_GAPS.md` from public rustdoc.
- Delete the `lib.rs:42` claim about `fixtures/normalizers.json` and
  `tests/parity.rs`, and every citation of `docs/PARITY.md`. **Neither file
  exists** — there is no `crates/verbora-normalizers/tests/` directory, no
  `fixtures/` anywhere, and `docs/PARITY.md` is absent though six crates cite
  it. The crate's entire stated correctness argument currently rests on
  evidence that is not in the repository.

**Consumers, all compile errors:**

- `crates/verbora-stemmers/src/no.rs:41` and `sv.rs:42` —
  `use verbora_normalizers::normalize_{no,sv}` no longer resolves.
- `crates/verbora-transliterators/src/ja.rs:491` — `normalize_ja` no longer
  resolves.

**Silent breakage — the two changes hidden behind those compile errors:**

1. **`verbora-stemmers`' `prepare` changes meaning even after the mechanical
   fix.** Replacing `normalize_no`/`normalize_sv` with `remove_diacritics`
   folds *every* occurrence rather than the first, is position-independent,
   and does **not** fold `ø` or `æ` — which are precisely the letters those
   two languages need. Norwegian and Swedish stems change. The
   position-dependence that goes away is a genuine defect (the same word
   stemmed differently depending on where in the document it appeared), but
   the ø/æ gap is a real regression and the Snowball-grounded answer belongs
   to the stemmers' own contract (migration item 3). **Until that lands, the
   call sites must be marked with the open question rather than silently
   swapped.**
2. **`verbora-transliterators`' Japanese path loses iteration-mark expansion
   and the small-tsu rewrite** once `normalize_ja` becomes `nfkc`, and gains
   composition of the combining voiced marks `U+3099`/`U+309A`. Its own tests
   must be checked for coverage of `々`; if none exists, the change is
   invisible in CI.

### Step 3 — `verbora-tokenizers`, and every consumer, in one commit

**The dangerous step, and it cannot be split.** Deleting the old tokenizers
breaks six consumers, and the standing "no old-and-new side by side" rule
forbids landing replacements alongside. The workspace compiles only when both
halves land together.

*Tokenizers.* Delete `aggressive.rs`, `case.rs`, `classes.rs`, `ja/`,
`regexp.rs`, `scan.rs`, `treebank.rs`, `utf16.rs`, `whitespace.rs` — 7,900 of
8,240 lines. Rewrite `sentence.rs` to §3.1. Add a private `word.rs`. Delete
the `Tokenize` trait, the `trim_edge_empties` re-export and the `regex`
dependency. Every module private; everything re-exported from the root.

*Consumers, loud (compile errors):*

- `crates/verbora-stemmers` × 14 files — `use verbora_tokenizers::classes;`
  at `en.rs:32`, `de.rs:33`, `nl.rs:39`, `fr.rs:52`, `es.rs:41`, `it.rs:37`,
  `pt.rs:57`, `ru.rs:58`, `uk.rs:36`, `sv.rs:43`, `no.rs:42`, `id.rs:64`,
  `carry.rs:37`, `lancaster.rs:33`, plus `base.rs:359`. The inline `next_run`
  scan at `base.rs:52` is replaced by `WordTokenizer::tokens`.
- `crates/verbora-stemmers/src/ja.rs:85`, `:101` — `StemmerJa::stems` and its
  `Utf16Token::{Text, Raw}` match. `TokenizerJa` is gone, so `stems` and
  `tokenize_and_stem` are **deleted**; `stem` and `is_katakana` stay and the
  caller supplies segmentation. This is the only runtime tokenizer call in
  `verbora-stemmers` and it has zero test coverage today
  (`tests/parallel.rs:30` excludes it).
- `crates/verbora-tfidf/src/globals.rs:24`, `:123` —
  `use verbora_tokenizers::regexp::WordTokens;` and
  `GlobalTokens::Default(WordTokens<'a>)`, a public enum leaking a
  private-module type.
- `crates/verbora-phonetics/src/lib.rs:271`, `:289` — `AggressiveTokenizer`
  plus `Tokenize::tokenize`.
- `crates/verbora-sentiment/src/lib.rs:26-32` — the crate doctest's
  `.expect("splitting mode")` on `WordTokenizer::tokens`, which no longer
  returns `Option`.
- `crates/verbora-tagger/Cargo.toml:23` — an **unused** dev-dependency.
  `grep -rn tokeniz` over the whole crate returns that line and nothing else.
  Delete it; `verbora-tagger` is not a consumer and must be removed from every
  list that says it is.
- `crates/verbora-examples/examples/hello.rs:5` and every executable site
  snippet using a deleted tokenizer.

*Silent breakages — the reason this step needs specific attention:*

1. **`verbora-tfidf` carries two independent copies of the boundary rule that
   bypass the tokenizer entirely, and neither fails to compile.**
   `tfidf.rs:166`'s `is_one_whole_token` hand-rolls `[0-9a-z_]` to answer
   single-word queries without tokenizing; `fast_build.rs:112`'s SWAR bitmap
   hardcodes the same class for document ingestion. Under UAX #29 both are
   **wrong on ASCII input**: `"don't"` is one word (WB6/WB7) where the SWAR
   path splits it, `"3.14"` and `"1,000"` are one word each (WB11/WB12) where
   the SWAR path splits them, and `"a:b"` is one word (WB6, `MidLetter`).
   Every ingested document is silently re-partitioned. **Both must be
   re-derived from the UAX #29 ASCII subset or deleted in this commit**, and
   the equivalence tests that guard them must be widened per §6.4 — as
   written they are structurally incapable of detecting this
   (`tfidf.rs:1785` iterates `0u8..=0x7F`; `fast_build.rs`'s two tests are
   ASCII-lowercase-only by construction).
2. **`verbora-tfidf` persists the interned term table** through `to_json`
   (`tfidf.rs:1092`) / `from_json` (`tfidf.rs:414`), with no version stamp and
   no validation. A corpus serialized before this step and restored after it
   silently mismatches every query. **Add a Unicode-version stamp and refuse
   to load across a change** — this is `verbora-tfidf`'s own migration item,
   but the obligation is created here and the release note must lead with it.
3. **`verbora-classifiers` is an unlisted transitive consumer.** Its Bayes and
   MaxEnt feature keys are stems produced by `verbora-stemmers`' scan over
   `classes::is_word_en`, and trained models are serialized
   (`basic/classifier.rs:707`, `:729`; `maxent/classifier.rs:202`, `:242`). A
   boundary change re-partitions features, so a model trained before and
   loaded after silently mispredicts. Same obligation as (2).
4. **`verbora-phonetics` changes results wherever hyphens, slashes or
   apostrophes appear**, once the compile error is fixed. Its stop-word filter
   is deliberately case-sensitive on raw tokens, and its only coverage of the
   whole path is the `lib.rs:265` doctest — `"The quick brown fox"`, which
   happens not to move. The encoders themselves are safe: `metaphone.rs:88-135`
   and `soundex.rs:396-414` handle case and non-ASCII themselves.
5. **`verbora-sentiment` rescales every score.** `analyzer.rs:322` divides the
   summed polarity by the *token count*, and unknown tokens contribute `0.0`
   while still counting toward the denominator. Any boundary change moves
   every score. It is immune to a casing change (it lowercases each token
   itself at `analyzer.rs:413`) and to nothing else. Its only coverage is one
   lowercase-ASCII doctest with no hyphens.
6. **`verbora-stemmers`' documented raw-versus-lowered asymmetry survives, but
   only because the tokenizer does not lowercase.** `base.rs:8-15` records a
   three-axis table across thirteen languages: which text is tokenized, which
   string the stop-word list is consulted with, which string is stemmed. Six
   languages (de, es, it, nl, ru, uk) filter stop words on the **raw** token,
   so a tokenizer that folded case would start dropping capitalised stop words
   — German `"Das"` is the worked example — with no compile error. This
   contract's §1 forbids the fold, so the asymmetry holds; it is listed
   because it is the tripwire that would fire if §1 were ever relaxed.
7. **`verbora-ngrams` keeps a fourth private copy of the boundary rule**
   (`tokenizer.rs:113`) until Step 5. It does not depend on
   `verbora-tokenizers` at all, so nothing cross-checks the two and nothing
   breaks when they diverge in this step.

*Loud tripwires to preserve and run:* `verbora-stemmers/src/base.rs:468`
(`run_scanning_agrees_with_the_verified_tokenizers`) is the only cross-check
between the stemmers' inline scan and a real tokenizer, and it carries
Cyrillic and astral input — but it covers only 5 of the 14 classes in use.
Widen it to all fourteen call sites *before* deleting `classes`, so the
deletion is measured against a complete cross-check rather than a partial one.

### Step 4 — `verbora-ngrams`

Independent of Step 3 except for the site snippets both touch.

- Delete `stats.rs`, `text.rs`, `tokenizer.rs`, `zh.rs`. Rewrite `engine.rs`
  to §3.3. Drop the `verbora-core` and `rustc-hash` dependencies. All modules
  private.
- Relocate the user-facing half of the `//!` prose on `engine.rs:1-18`,
  `stats.rs:1-12`, `text.rs:1-19`, `tokenizer.rs:1-34` and `zh.rs:1-40`; most
  of it describes another implementation and is deleted rather than moved.
- Delete the `lib.rs:37` and `tokenizer.rs:72` citations of
  `fixtures/ngrams.json`, which does not exist in the repository.

**Blast radius: benchmarks and site snippets only.** `verbora-ngrams` has no
library consumer in the workspace —
`benchmarks/competitive/rust-competitors/{benches,tests}/ngrams*` and
`crates/verbora-examples` are the whole list. **No silent breakage.**

### Step 5 — Consumer contracts and the version stamp

The obligations Step 3 creates, discharged where they belong.

- `verbora-tfidf` and `verbora-classifiers`: Unicode-version stamp on every
  persisted artifact, with a load-time refusal across a change (§1). These are
  those crates' migration items; this step opens them and states the
  requirement so it is not lost.
- `verbora-stemmers`: the Snowball-grounded decision for Norwegian and Swedish
  `prepare` (Step 2, silent breakage 1) and for the fourteen deleted character
  classes (Step 3).
- `docs/design/rust-native-migration.md`: record per-crate item 2 and the
  standing findings.

### Step 6 — Documentation, and the measurement ask

- `Choosing the Right API` blocks for all three crates, which `AGENTS.md`
  requires and which only `verbora-tokenizers` has today.
- The site pages listed below are invalidated by this contract. Executable
  snippets fail the build and will be found; **the prose tables will not**:
  - `site/features/tokenizers.md` — the whole page (151 tokenizer references).
    :16-17 (equality pinned on UTF-16 code units), :67-96 (per-language word-class
    catalogue), :98-124 (regex family, `Option` contract, U+FFFD rendering),
    :126-165 (Choosing the Right API + token-type table), :232-264
    (`verbora_core` traits), :265-333 (four optional tokenizers), :334-386
    (UTF-16 tokens and unpaired surrogates), :387-403 (`trim_edge_empties`),
    :434-465 (allocation), :466-484 ("Five semantics this crate defines for
    itself" — prose table, caught by no test), :485-575 (quirks; executable,
    self-detecting), :521, :527-531.
  - `site/features/normalizers.md`, `site/features/ngrams.md` (:25-28, :37-38,
    :75-83, :113, :173-232, :248-320, :322-372, :391),
    `site/choosing/ngrams.md` (:22-44, :143-147, :162, :219-256, :274-295,
    :306, :328-335), `site/choosing/tokenization.md` (:21-27, :60-61, :105,
    :181, :227-242, :351).
  - `site/features/core.md` :32-33, :75-76, :88-89, :109, :121, :138-150 (the
    "Not every tokenizer can borrow" table — invalidated wholesale), :272,
    :384, :409, :520, :538.
  - `site/features/phonetics.md` :387, :464-466, :485, :492-506;
    `site/features/sentiment.md` :22, :51, :56-61, :392, :455-456;
    `site/features/stemmers.md` :20-21, :34, :45-48, :52, :71, :129, :137,
    :142; `site/features/tfidf.md`; `site/features/transliterators.md`.
  - `site/choosing/api-shapes.md` :50, :62, :87-90, :159, :179-203;
    `site/choosing/decision-trees.md`; `site/getting-started/first-program.md`
    :12-20, :32-39, :49-51.
  - `site/performance/{zero-copy,buffer-reuse,iterator-vs-into,batch-vs-streaming,ergonomics-vs-throughput,parallelism,allocation,cache-locality}.md`
    — every borrowing and allocation claim.
  - `site/recipes/{batch,parallel-corpus,streaming,interactive}.md`.
  - `docs/COMPETITIVE_BENCHMARKS.md` (37 references, and where the narrowed
    ASCII benchmark domain is justified), `docs/PERFORMANCE_GAPS.md`,
    `docs/PERFORMANCE_MATRIX.md`, `docs/design/verbora-transliteration.md`,
    `AGENTS.md:1489`, `:1544`, `site/reference/api.md`, `README.md`.
- **Then** ask whether to run a measurement campaign, naming the targets and
  the expected duration. Benchmarks are never launched on the implementer's
  own initiative.

---

## 6. Test obligations

Every expected value below comes from a published standard or from arithmetic
shown inline. **No test may assert "matches current behaviour",** and no
expected value may be produced by running the new code. Each obligation names
the input class a naive test would fail to reach — because that is where the
defect will be, and because it is exactly how the `verbora-distance`
length-lemma bug survived four years.

### 6.1 UAX #29 conformance

**The word and sentence conformance files.** Run `WordBreakTest.txt` and
`SentenceBreakTest.txt` from the UCD of the pinned Unicode version, in tree,
as data. This is what pins the Unicode version Verbora claims: a version bump
that changes any boundary fails here, loudly, which is the mechanism §1's
persistence clause depends on. Check the Unicode data licence before the file
lands.

*Class a naive test misses:* the whole `Extend`/`Format`/`ZWJ` machinery of
WB4, regional-indicator pairs (WB15/WB16), and ZWJ emoji sequences (WB3c). No
hand-written fixture set reaches them, and they are where a boundary
implementation drops or duplicates text.

### 6.2 The tokenizer invariants

**Concatenation.** `SegmentTokenizer.tokens(t).collect::<String>() == t` and
the same for `SentenceTokenizer::new()`, over a corpus spanning ASCII,
Latin-1, Greek, Cyrillic, Arabic, Hebrew, Devanagari, Thai, Hangul, CJK,
astral scalars, lone `U+FEFF`, lone `U+0085`, `CR LF`, and the existing
`lib.rs:213` battery.

*Class a naive test misses:* `"a\r\nb"` (WB3 keeps CRLF together, and a naive
line-splitting oracle does not) and text ending mid-combining-sequence.

**Substring and order.** For every token of every tokenizer, the token's
pointer lies within the input's allocation at a byte range strictly greater
than the previous token's, and the ranges do not overlap.

*Class a naive test misses:* repeated tokens. `assert!(text.contains(tok))`
passes vacuously for `"a a a"` and cannot detect an off-by-one range.

**No empty token, ever.** Over the same corpus, plus `""`, `" "`, `"\u{feff}"`.

**Subsequence.** `WordTokenizer.tokens(t)` is a pointer-identical subsequence
of `SegmentTokenizer.tokens(t)`.

**The word filter.** For every segment of the corpus, membership in
`WordTokenizer`'s output equals an independently written predicate: contains a
scalar with `Alphabetic`, or with `General_Category` in `{Nd, Nl, No}`.

*Class a naive test misses:* segments that are numeric but not alphabetic
(`"3.14"`, Arabic-Indic `"١٢٣"`, `U+2160` ROMAN NUMERAL ONE, which is `Nl`),
and segments consisting entirely of `Extend` marks.

**The moved-behaviour table of §3.1**, asserted directly, every row. These are
the cases a consumer will notice, and each expected value is derived from a
named WB rule in the table itself.

**No panic.** The `lib.rs:213` battery, extended with lone surrogante-adjacent
astral input, 64 KiB of a single combining mark, text beginning with a
combining mark, and text that is one unpaired regional indicator, swept across
all three tokenizers and all three call shapes.

### 6.3 Sentence abbreviations

**Suppression fixtures**, derived from the rule in §3.1:

- `"Dr. Smith arrived. He left."` with `["Dr."]` → two sentences; with `[]` →
  three. The suppressed boundary is the one after `"Dr. "`.
- `"He works at Acme Inc. She does not."` with `["Inc."]` → one sentence.
- `"Ends with an abbreviation Inc."` with `["Inc."]` → **one** sentence. The
  final boundary at `text.len()` is never suppressed.
- `"Visit the casino. Then leave."` with `["no."]` → one sentence. The
  documented over-suppression (§4.3), pinned so it cannot regress silently
  into a "fix".
- `SentenceTokenizer::with_abbreviations([""])` is
  `Err(AbbreviationError::Empty { index: 0 })`.
- Concatenation still reproduces the input when suppression fires.

*Class a naive test misses:* the abbreviation at end-of-input (where a
naive implementation drops the last sentence entirely) and the abbreviation
that is a suffix of an ordinary word. A fixture set of mid-sentence `"Dr."`
cases reaches neither.

### 6.4 The `verbora-tfidf` fast paths

**This is the obligation most likely to be discharged wrongly**, because the
current tests are structurally incapable of failing.

- `is_one_whole_token(q) == true` implies
  `WordTokenizer::tokenize_borrowed(q) == [q]`, swept over a generated corpus
  that includes **non-ASCII** and **ASCII punctuation**. The existing test
  (`tfidf.rs:1785`) iterates `0u8..=0x7F` one byte at a time, so it cannot
  reach `"don't"`, `"3.14"`, `"1,000"`, `"a:b"` or `"café"` at all.
- The SWAR ingestion path agrees with `WordTokenizer` over a corpus that
  includes apostrophes, decimal points, thousands separators, colons between
  letters, and underscores. The existing generators emit "punctuation-free,
  single-ASCII-space-joined lowercase-letter text"
  (`benchmarks/.../tests/tokenizers_correctness.rs:1-18`, which says so) —
  a domain that **excludes by construction** every input class this step
  moves. A green competitive run is not validation of Step 3.

*Class a naive test misses:* ASCII punctuation inside a word. ASCII is
invariant under every candidate *unit* choice, so an ASCII-only test can still
be a good test — but only if it carries the ASCII characters whose
`Word_Break` value is not `Other`.

### 6.5 Normalization

**The UCD conformance file.** Run `NormalizationTest.txt` for the pinned
version against all four form functions.

*Class a naive test misses:* Part 0's algorithmically-composed Hangul, and the
"not listed in any part" invariant — that every character absent from the file
is its own NFC, NFD, NFKC and NFKD. A fixture set drawn from the file's rows
alone never tests the second, which is where a table-driven implementation
with a missing entry fails.

**`remove_diacritics` idempotence**, over an exhaustive sweep of the BMP
**and** a sampled sweep of the astral planes.

*Class a naive test misses:* astral combining marks. The current suite's
exhaustive sweep is BMP-only, and `U+1D167..=U+1D169` MUSICAL SYMBOL COMBINING
have non-zero `Canonical_Combining_Class`.

**Normalization-form independence**: `remove_diacritics(nfd(s)) ==
remove_diacritics(s) == remove_diacritics(nfc(s))`, over the same corpus.

*Class a naive test misses:* decomposed input. The current implementation
fails this on `"e\u{301}"` and the current fixtures are all precomposed.

**Position independence**: for every `s` and every split of `s` at a `ccc = 0`
boundary, `remove_diacritics(s)` equals the concatenation of the parts'
results. This is the property `normalize_no`/`normalize_sv` fail and that
propagates into stemming.

*Class a naive test misses:* repeated accented characters. `normalize_no("ààà")`
is `"aàà"` today and a single-occurrence fixture cannot see it.

**Non-folding fixtures**, each with its derivation stated: `ø`, `Æ`, `đ`, `ł`,
`ħ`, `ŋ`, `ı`, `ß` (empty `Decomposition_Mapping`); `Ａ`, `Ⓐ`, `ǅ`, `ſ`
(compatibility, not canonical); `Å` `U+212B` → `"A"` (canonical singleton then
canonical decomposition); `İ` → `"I"` (`I` + `U+0307`, `ccc = 230`).

**Script preservation**: `remove_diacritics` is the identity on a Thai corpus
(`ccc = 0` vowel signs), on a Devanagari corpus without nuktas, and on Hangul;
and removes Hebrew niqqud, Arabic harakat and the Devanagari nukta.

*Class a naive test misses:* everything but Latin. A Latin-only fixture set
cannot distinguish the `ccc != 0` rule from the strip-all-marks rule, which is
the design decision the definition rests on.

**The `Cow` iff-guarantee**: over every corpus above,
`matches!(r, Cow::Borrowed(_)) == (r.as_ref() == s)`, for all five functions.

*Class a naive test misses:* a string already in NFC whose NFD contains marks
— `"café"` precomposed. An ASCII-only test never distinguishes the two arms.

### 6.6 N-grams

**Padding shape**, derived from §3.3 rather than recorded:

- Every window from `Padded::ngrams()` has exactly `n` elements, over a sweep
  of `len` in `0..=8` × `n` in `1..=8` × all four padding combinations.
- The window count equals `len + k_start + k_end - n + 1` when positive and
  `0` otherwise, checked against an independently written count.
- `Padded::new(&["a","b","c"], 5, Some(&"<s>"), Some(&"</s>"))` yields
  `len + n - 1 = 7` windows of five elements each, the first being
  `["<s>","<s>","<s>","<s>","a"]` and the last
  `["c","</s>","</s>","</s>","</s>"]`, in that order.
- `Padded::new(&[] as &[&str], 4, Some(&"S"), Some(&"E"))` yields three
  windows of four, drawn from `[S,S,S,E,E,E]`. Today this call returns six
  tuples of lengths 3,2,1,1,2,3.
- `n == 1` with both symbols supplied adds no padding.

*Class a naive test misses:* `n > len`, which is where the current
implementation produces short and out-of-order tuples, and `len == 0`.

**Overflow totality.** `Padded::new(&arr, NonZeroUsize::MAX, Some(&()), None)`
over `arr: &[(); usize::MAX / 2 + 1]` is empty, `len()` is `0`, and nothing
panics — in debug and in release. The same for `n = usize::MAX` on a
three-element `&[&str]` with each padding combination.

*Class a naive test misses:* **zero-sized element types entirely.** The
current code's guarding comment — "a slice length never exceeds
`isize::MAX`" — is false for ZSTs, and `[(); N]` is itself zero-sized, so the
input costs nothing to construct and no non-ZST test can reach it.

**`char_ngrams` losslessness.** For every window `w` of every input in the
mixed-script corpus: `w.chars().count() == n`, `text.contains(w)`, and `w`
contains `U+FFFD` only if `text` does. `ExactSizeIterator::len` equals the
number of items actually yielded.

*Class a naive test misses:* astral input and combining marks. `char_ngrams`
exists to replace a UTF-16 splitter whose only failure mode is astral.

**`ngrams` is `windows`.** `ngrams(seq, n).eq(seq.windows(n.get()))` over a
sweep, and `ngrams(seq, n)` is empty for `n > seq.len()`.

---

## 7. `UNMEASURED` — what the next benchmark campaign must answer

No benchmark was run in producing this contract, and none is run during
implementation (`CLAUDE.md`; `docs/design/rust-native-migration.md`
§ "Performance baseline"). Every item below is a structural argument, not a
measurement, and must not be published as a number until a full-precision run
exists.

1. **`UNMEASURED` — UAX #29 word segmentation versus the deleted class scan.**
   The current `WordRuns` is a `matches!` over a handful of `char` ranges;
   UAX #29 is a property lookup plus a rule automaton with lookahead. A
   regression is expected and it lands on the hottest path in the workspace —
   `verbora-tfidf` ingestion, `verbora-stemmers`, `verbora-sentiment`.
   **The designated fallback, recorded so it is not rediscovered: an ASCII
   fast path.** Over ASCII the `Word_Break` classes reduce to `ALetter`
   `[A-Za-z]`, `Numeric` `[0-9]`, `ExtendNumLet` `_`, `MidLetter` `:`,
   `MidNum` `,` `;`, `MidNumLet` `.`, `Single_Quote` `'`, `WSegSpace` ` `,
   `Newline` `LF VT FF CR`, `Other` everywhere else — a 128-entry LUT and a
   small automaton. It is **not** pre-built, and it may only ship with an
   exhaustive equivalence proof against the general path over every ASCII
   string up to a stated length, plus the conformance file. An unproved fast
   path here is how the workspace acquired four divergent copies of a boundary
   rule.
2. **`UNMEASURED` — `remove_diacritics`.** Three passes (NFD, filter, NFC)
   replace one per-scalar table lookup. Direction is near-certain; magnitude is
   not. The designated mitigation is an early `Cow::Borrowed` return for
   `s.is_ascii()`, which is exact — ASCII is invariant under all four forms and
   has no combining marks — and a quick-check gate for the rest. Both are
   correctness-preserving by construction, unlike item 1's.
3. **`UNMEASURED` — the four normalization forms.** Whether the quick-check
   path makes the `Cow::Borrowed` guarantee cheap enough that the wrapper is
   worth its existence over `unicode-normalization`'s iterators. If it is not,
   the honest answer is to say so in the rustdoc, not to drop the guarantee.
4. **`UNMEASURED` — `verbora-tfidf`'s fast paths after re-derivation.** The
   SWAR bitmap currently encodes `[a-z0-9_]`; a UAX #29-correct ASCII rule
   needs the `MidLetter`/`MidNum`/`MidNumLet` lookahead, which a single
   bitmask cannot express. Whether the path survives at all, and what
   `is_one_whole_token` costs once it must handle `'`, `.` and `,`, decides
   whether the documented ~40 ns of an ~82 ns `tfidf` call is still there.
5. **`UNMEASURED` — the `Cow` and borrow footprint across the workspace.**
   Every published zero-copy and allocation figure changes: `verbora-tfidf`'s
   `GlobalTokens`/`Resolved` split, `verbora-phonetics`' `&'a str` pipeline,
   and the `Cow`/`Utf16Token`/`String` token shapes that no longer exist. The
   whole of `site/performance/` is stale on landing.
6. **`UNMEASURED` — `par_tokenize_batch`'s crossover.** The current rustdoc
   quotes ~118–120 µs for an ~8192-word document from a benchmark of a deleted
   tokenizer. Removed, not adjusted.
7. **`UNMEASURED` — `Padded` versus the lazy `Cow` windows it replaces.**
   Materialising the padded sequence once trades `k` element clones for zero
   per-window allocation, and the unpadded path becomes `slice::windows` with
   no wrapper at all. Direction is expected to favour the new shape and is
   unverified.
8. **`UNMEASURED` — compile time and binary size.** Two new dependencies
   against roughly 15,000 deleted lines, including 2,300 lines of table data.
   `AGENTS.md` § Dependencies requires both to be evaluated.

Two things that are **not** performance questions and must not be deferred to
a benchmark: the conformance obligations of §6.1 and §6.5, and the
`verbora-tfidf` fast-path equivalence of §6.4. All three are settled by tests,
and all three are what a green competitive run will fail to tell you — its own
module documentation restricts its inputs to "punctuation-free,
single-ASCII-space-joined lowercase-letter text", which excludes by
construction every input class this contract moves.
