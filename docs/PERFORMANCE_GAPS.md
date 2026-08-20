# Performance gaps

Per `Fase 6 Benchmark.md`'s own `PERFORMANCE REGRESSION OPPORTUNITY` and `NO
CHERRY PICKING` sections: when a competitive benchmark shows Verbora losing,
the benchmark is **not** modified or removed. It is recorded here, with the
real numbers, a profiled/sourced likely reason, and — where one exists — an
optimization opportunity to evaluate in a future pass. This file is a record
of honest findings, not a to-do list that must be cleared before Fase 6 can
be considered done; the spec's own `FINAL PRINCIPLE` is explicit that the
goal is truth, not proving Verbora is fastest.

Every entry links to the raw benchmark that produced it — see
`benchmarks/competitive/results/raw/` and
`docs/COMPETITIVE_BENCHMARKS.md`.

**Reading a retired entry.** Where code has changed underneath an entry, its
numbers are marked `⚠ … retired` in place and the entry is kept, because the
reasoning attached to a number is usually what a later pass overturns and is
worth more than the number was. A retirement says one of two things, and the
difference decides whether the next benchmark campaign has any work to do:

- **retired *pending re-measurement*** — the comparison is still live and a
  benchmark group is waiting to answer it (entries 1, 4, 10 in part, 13, 14,
  24, 26–29, 31, 34, 38);
- **retired with nothing to re-measure** — the Verbora capability itself was
  deleted, or the competitor cannot produce a trustworthy number at all, so
  there is no question left to ask (entries 3, 5, 11, 23, 30, and `eddie`'s
  Jaro/Jaro-Winkler rows under entry 36 item 3).

Either way, no figure in a retired entry may be quoted, and none may be
estimated, adjusted or interpolated to fill the gap. Competitor figures are
called out separately wherever they are unaffected. The consolidated list of
what the next campaign must answer lives in `docs/COMPETITIVE_BENCHMARKS.md`
§7.

---

## 1. Levenshtein distance — Verbora vs. `rapidfuzz` (Rust)

| | |
|---|---|
| **Capability** | Levenshtein edit distance |
| **Competitor** | `rapidfuzz` (rapidfuzz-rs) 0.5.0, `rapidfuzz::distance::levenshtein::distance` |
| **Verbora result** | 1024-char ASCII pair: **3.090 ms** (median, `levenshtein/verbora/1024`) |
| **Competitor result** | Same pair: **34.02 µs** (median, `levenshtein/rapidfuzz/1024`) |
| **Gap** | **~90.8× slower** at 1024 characters (3.090 ms vs. 34.02 µs). The gap grows with input length, not constant: **~44.4×** at 64 characters (9.84 µs vs. 221.9 ns), **~56.7×** at 256 characters (186.28 µs vs. 3.29 µs), **~90.8×** at 1024 characters — see "Likely reason" for why this specific growth pattern is expected, not noise. |
| **Likely reason** | `rapidfuzz`'s `levenshtein` module is a direct implementation of Hyyrö's bit-parallel extension of Myers' 1999 algorithm (confirmed by reading `rapidfuzz-0.5.0/src/distance/levenshtein.rs`'s own module doc comment and its `u64`-packed `vp`/`vn` bit-vector state) — it packs 64 characters of DP-matrix state into one machine word and updates a whole diagonal-band of cells per word-level instruction, giving it roughly `O(nm/64)` bit-level work instead of Verbora's `O(nm)` cell-by-cell dynamic-programming sweep (`crates/verbora-distance/src/levenshtein.rs`, two-row DP as documented in `docs/PERFORMANCE.md`'s own "Where the Levenshtein win comes from" section — a design that already beats the reference's heap-allocated matrix, but is still the classical scalar algorithm, not bit-parallel). The growing gap with length is exactly what this explanation predicts: bit-parallelism's advantage over a scalar DP sweep scales with string length divided by word size. |
| **Profiling evidence** | Read `rapidfuzz-0.5.0/src/distance/levenshtein.rs` directly (not assumed from its README): module doc comment cites "Explaining and Extending the Bit-parallel Approximate String Matching Algorithm of Myers" (Heikki Hyyrö); `BitVectorInterface`/`BlockPatternMatchVector`/`ShiftedBitMatrix<u64>` types and `vp`/`vn: u64` fields confirm a genuine bit-parallel implementation, not merely a well-tuned scalar one. Real benchmark run: `cargo bench -p competitive-rust --bench distance -- levenshtein/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/`. |
| **Optimization opportunity** | ~~A bit-parallel Levenshtein implementation (Myers'/Hyyrö-style) is a legitimate, well-understood algorithmic upgrade path for `verbora-distance`'s scalar two-row DP — but it is a genuinely different algorithm, not a tuning pass, and changing it touches parity-critical code (`levenshtein`, `damerau_levenshtein`, and `levenshtein_search` all share this DP core, and `levenshtein_search`'s backtracking needs the full parent matrix, which a pure bit-parallel distance-only algorithm does not naturally provide). This is flagged as a real, evidence-backed opportunity for a **future, separate, dedicated phase** — implementing it inside Fase 6 itself would violate this project's own "measure first, then a focused follow-up phase" discipline (the same discipline Fase 2's audit established) and risks rushing a change to code four other public functions depend on for correctness. Not implemented in Fase 6.~~ **Implemented across the dedicated follow-up passes — see the update below and entry 26's two earlier updates.** |

**Update, this round (2026-08) — closed and reversed: Verbora now beats
`rapidfuzz` at every size measured.** The table above (and its 90.8×
headline) describes the original scalar two-row DP and stays as history;
entry 26's two earlier updates then landed exactly the Myers'/Hyyrö
bit-vector algorithm this entry's opportunity flagged, narrowing this gap
to ~4.3× at 1024. This round removed the last structural disadvantage
inside those same kernels: the `HashMap`-based `Peq` pattern-match table
was replaced with flat/packed `BitPeq` tables (a flat `[u64; 256]` table
plus a packed distinct-rows matrix when the operands are pure `u8`;
`FxHashMap` retained only for genuine `u16` input), and the single-word
gate widened from 8..=64 to **1..=64** units — the old lower bound of 8
existed precisely because `HashMap` setup swamped a 4-entry table, and
with the flat table that setup cost is gone. Zero behavior changes:
verified by the differential tests against the trusted scalar `plain_rows`
DP plus an independent adversarial audit with mutation testing.
Re-measured (same bench, full Criterion defaults, quiet machine), Verbora
vs. `rapidfuzz` medians: n=4 **14.8 vs. 32.0 ns** · n=16 **41.9 vs.
74.3 ns** · n=64 **164.7 vs. 247.8 ns** · n=256 **2.09 vs. 3.30 µs** ·
n=1024 **29.07 vs. 31.72 µs** — Verbora wins every size: **2.2×** at n=4,
**1.8×** at n=16, **1.5×** at n=64, **1.6×** at n=256, and **1.09×** at
n=1024 (the narrowest margin, at the one size where both sides are running
the same Hyyrö block formulation and the difference comes down to the Peq
table representation). The file's original headline gap is closed, and
reversed.

**Update, closing pass (2026-08) — the reversal stands as a finding; its
medians do not.** The Rust-native contract
(`docs/design/distance-contract.md`) removed the cost argument from
`levenshtein` and with it the per-call cost comparison that stood in front of
these kernels, so every Verbora median in this entry is retired pending
re-measurement — see entry 26's closing update for the full reasoning and for
what did and did not change inside the kernels. ⚠ Do not quote the ratios
above until the re-benchmark lands. `rapidfuzz`'s own figures are unaffected.

## 2. Trie lookup and prefix enumeration — Verbora vs. `qp-trie` (Rust)

`qp-trie` is the weaker-fit of the two §1.18 competitors (a general ordered
radix map, not purpose-built for text completion — see
`docs/COMPETITIVE_BENCHMARKS.md` §1.18), but "weaker fit" turned out to
describe its API surface, not its speed: on every read-path operation
benchmarked, it beats `verbora-trie`. This is recorded in full, not
minimized, per this file's own charter — see §1.18's other competitor,
`trie-rs`, for the opposite result (Verbora is 34×–113× faster there; see
`docs/PERFORMANCE.md`'s Trie section).

| | |
|---|---|
| **Capability** | Trie exact-match lookup (`contains`) and full prefix enumeration (`keys_with_prefix`) |
| **Competitor** | `qp-trie` 0.8.2, `qp_trie::Trie::contains_key_str` / `iter_prefix_str` |
| **Verbora result** | 20 000-word corpus: `contains_hit` **1.224 ms** median (`contains_hit/verbora/words`); full-corpus `predictive_search` (empty prefix) **1.774 ms** median (`predictive_search/verbora/all`) |
| **Competitor result** | Same corpus: `contains_hit` **0.869 ms** median (`contains_hit/qp_trie/words`); full-corpus prefix enumeration **0.122 ms** median (`predictive_search/qp_trie/all`) |
| **Gap** | **~1.4× slower** on exact-match lookup (1.224 ms vs. 0.869 ms hits; 1.225 ms vs. 0.797 ms misses — `contains_miss/verbora/words` vs. `contains_miss/qp_trie/words`), widening to **~14.6× slower** on full-corpus prefix enumeration (1.774 ms vs. 0.122 ms) and **~9.5× slower** on single-letter-prefix enumeration (1.171 ms vs. 0.124 ms, `predictive_search/verbora/1char` vs. `predictive_search/qp_trie/1char`). Verbora **wins** the corresponding write-path benchmark by a similar margin in the other direction — `build/verbora/random` **1.646 ms** vs. `build/qp_trie/random` **3.042 ms**, `build/verbora/prefix_heavy` **2.177 ms** vs. `build/qp_trie/prefix_heavy` **4.602 ms** — so this is a genuine read/write trade-off between the two structures, not a one-sided loss. |
| **Likely reason** | Two independent structural differences, confirmed by reading `qp-trie-0.8.2/src/node.rs`, `src/util.rs` and `src/sparse.rs` directly. **(1) Path compression.** `qp-trie` is a crit-bit-style radix trie: a `Branch` node exists only at a nybble position where two *already-stored* keys actually differ (`Branch::choice`, computed via `nybble_mismatch` in `util.rs`), so tree depth is bounded by the number of *distinguishing* nybbles between stored keys, not by key length. Verbora's `Trie` has no path compression — every lookup walks one arena hop per UTF-16 code unit of the query, so an 8-unit word is always 8 hops regardless of how few other stored words share its structure. **(2) Keys are stored whole at the leaf.** `qp-trie`'s `Leaf<K, V>` holds the caller's original `key: K` directly (`node.rs`), so `iter_prefix_str` yields a reference to an already-materialized string — no reconstruction. Verbora's `Trie` stores no string anywhere; `keys_with_prefix` rebuilds every returned word one code unit at a time into a reused buffer during the walk (`crates/verbora-trie/src/iter.rs`), which is `O(word length)` extra work *per word yielded*, on top of the walk itself — the dominant cost for a 20 000-word full enumeration. The same two properties invert for `build`: `qp-trie`'s insert must compute `nybble_mismatch` against the nearest existing key and then splice a `Sparse` bitmap-indexed array (`src/sparse.rs`, a popcount-positioned `Vec`) at the correct branch depth, while Verbora's insert is a fixed one-hop-per-code-unit descent through a flat arena with an at-most-2-element inline child array — cheaper per insert, at the cost of no compression to exploit on read. |
| **Profiling evidence** | Read `qp-trie-0.8.2/src/node.rs` (`Leaf`/`Branch` definitions, `Branch::choice`), `src/util.rs` (`nybble_index`, `nybble_mismatch`) and `src/sparse.rs` (`Sparse<T>`'s `u32` bitmap + `Vec` layout) directly — not assumed from the crate's README. Real benchmark run: `cargo bench -p competitive-rust --bench trie` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/trie-*-qp_trie-*.json` and `trie-*-verbora-*.json`. Correctness of the compared operations (same *set* of words returned/matched, order intentionally not compared — see `benchmarks/competitive/rust-competitors/benches/trie.rs`'s own doc comment) is checked by `benchmarks/competitive/rust-competitors/tests/trie_correctness.rs`, run before this benchmark's timings were trusted. |
| **Optimization opportunity** | Path compression (collapsing chains of single-child nodes) is the standard fix for exactly this shape of gap, and is a well-understood technique (PATRICIA/crit-bit tries, of which `qp-trie` is one instance) — but it is a fundamentally different node layout from the current flat two-child-inline arena, touches every one of `verbora-trie`'s public methods (`add_string`, `contains`, `keys_with_prefix`, `find_matches_on_path`, `find_prefix`), and — critically — compression changes *where* a lookup can stop mid-node, which interacts with the UTF-16-code-unit-at-a-time semantics `keys_with_prefix`'s the reference `for…in`-order parity and the `find_prefix` unpaired-surrogate handling both depend on byte/unit-for-unit today. Storing the full key at each word-terminal node (fixing the enumeration-side cost independently of compression) is a smaller, more isolated change with the same parity risk in miniature: `find_matches_on_path`'s `Cow`-borrowing-from-`search` optimization (documented in `crates/verbora-trie/src/trie.rs`) specifically depends on *not* storing the key, so it would need to be re-justified, not just added to. Both are flagged as real, evidence-backed opportunities for a **future, separate, dedicated phase** — not implemented in Fase 6, consistent with entry 1's discipline. |

## 3. Whitespace tokenization — Verbora vs. `tantivy::WhitespaceTokenizer` (Rust)

| | |
|---|---|
| **Capability** | Whitespace tokenization (`RegexpTokenizer` configured with `\s+`) |
| **Competitor** | `tantivy` 0.26.1, `tantivy::tokenizer::WhitespaceTokenizer` |
| **Verbora result** | 123 B: **631.0 ns** · 1187 B: **5.356 µs** · 9709 B: **52.22 µs** · 77684 B: **429.34 µs** (medians, `whitespace_tokenization/verbora/<size>`) |
| **Competitor result** | Same documents: **138.1 ns** · **1.157 µs** · **13.82 µs** · **113.49 µs** (`whitespace_tokenization/tantivy/<size>`) |
| **Gap** | **~3.6×–4.6× slower** at every size measured, worst at 1187 B (**4.63×**) and 123 B (**4.57×**), narrowing at the two larger sizes (**3.78×** at 9709 B, **3.78×** at 77684 B) — a genuinely flat, per-byte gap, not one that opens up or closes sharply with scale (contrast entry 1's growing gap). See "Likely reason" for why a flat ratio is exactly what this explanation predicts. |
| **Likely reason** | Two different classes of implementation, confirmed by reading both sides' real source. Verbora's `RegexpTokenizer` (`crates/verbora-tokenizers/src/regexp.rs`) is a **general reference-`String#split`-parity engine**: it exists to support *any* caller-supplied pattern, including ones with capture groups that must be interleaved into the output (the exact mechanism `WordPunctTokenizer` is built on), so even a capture-free pattern like `\s+` is still driven through `Regex::captures_iter` — the `regex` crate's general NFA/Pike-VM match-and-capture machinery — once per match. `tantivy::WhitespaceTokenizer` (`tantivy-0.26.1/src/tokenizer/whitespace_tokenizer.rs`) is not a regex at all: it is a hand-written `CharIndices` scan that tests one predicate (`c.is_ascii_whitespace()`) per character with no engine dispatch, no capture bookkeeping, and no dynamic pattern to interpret. This is structurally the same story as entry 1 (a general mechanism vs. a purpose-built scanner for one fixed shape of work) — and Verbora's own `WordTokenizer`, which is *not* built on the regex engine (it uses the hand-written `WordRuns` scanner in `crates/verbora-tokenizers/src/scan.rs`, the same file's doc comment explaining why a scanner beats a lookup-table or regex approach here), is competitive with tantivy's equally hand-written `SimpleTokenizer` in the `word_tokenization` group below — direct evidence that the gap traces to the regex engine specifically, not to Verbora's tokenization code in general. |
| **Profiling evidence** | Read `crates/verbora-tokenizers/src/regexp.rs`'s `js_split_into` (calls `re.captures_iter`) and `tantivy-0.26.1/src/tokenizer/whitespace_tokenizer.rs`'s `WhitespaceTokenStream::advance`/`search_token_end` (a raw `CharIndices` loop) directly — not assumed from either crate's docs. Real benchmark run: `cargo bench -p competitive-rust --bench tokenizers -- whitespace_tokenization/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/tokenizers-whitespace_tokenization-*.json`. Correctness of the compared token boundaries (not just counts) on the narrowed punctuation-free input domain this benchmark uses is checked by `benchmarks/competitive/rust-competitors/tests/tokenizers_correctness.rs`, run before this benchmark's timings were trusted. Hugging Face `tokenizers`' `WhitespaceSplit` is benchmarked on the same rows and is *slower* than Verbora at every size (2.9×–4.4×, `whitespace_tokenization/huggingface/<size>`) — see `docs/PERFORMANCE.md`'s Tokenizers section — so this entry is specifically a loss to `tantivy`, not to "Rust tokenizers" as a category. |
| **Optimization opportunity** | ~~A hand-written ASCII/Unicode-whitespace scanner analogous to `WordRuns` — bypassing the `regex` crate entirely for the fixed, non-configurable `\s+`-splitting case — is a legitimate, well-understood optimization (it is exactly what `WordTokenizer` already does relative to a hypothetical regex-based implementation of *its* character class). It is not implemented here because `RegexpTokenizer` is deliberately generic: it is the shared engine behind `WordPunctTokenizer`'s capture-group interleaving and any future caller-supplied pattern, and the reference's own `RegexpTokenizer` explicitly exposes the general `pattern`/`gaps`/`matching` option matrix as public API (`crates/verbora-tokenizers/src/regexp.rs`'s own module doc comment) — a special-cased fast path for the single literal pattern `\s+` would need to preserve that general contract's exact `_.without`/zero-width-match/capture-interleaving semantics for every *other* pattern while only accelerating this one, which is a real but nontrivial follow-up, not a one-line change. Flagged as a real, evidence-backed opportunity for a **future, separate, dedicated phase**, consistent with entries 1 and 2's discipline. Not implemented in Fase 6.~~ **Implemented in the follow-up optimization round — see the update below.** |

**Update, this round (2026-08) — closed and reversed: Verbora now beats
`tantivy` at every size measured, while handling strictly more whitespace
than tantivy does.** Both layers of the opportunity above shipped, with
the general contract preserved exactly as the opportunity demanded. First,
`RegexpTokenizer` now drives capture-free patterns through `find_iter`
instead of `captures_iter` — patterns *with* capture groups (the
`WordPunctTokenizer` interleaving mechanism) still take the original path,
so the general `pattern`/`gaps`/capture-interleaving contract is
untouched. Second, the exact pattern `\s+` gets a dedicated ASCII-first
SWAR whitespace scanner that bypasses the regex engine entirely — provably
identical to the engine, not assumed identical: the scanner's predicate
was exhaustively tested against Rust `regex`'s `\s` over all ~1.1M Unicode
scalar values (`\s` matched `char::is_whitespace` at every one), so the
fast path handles **full Unicode whitespace**, where
`tantivy::WhitespaceTokenizer`'s predicate is `c.is_ascii_whitespace()` —
ASCII only (see the original Likely reason above). Re-measured, Verbora
vs. `tantivy` medians: 123 B **101.3 vs. 112.3 ns** (**1.11× faster**) ·
1187 B **567.6 vs. 962.9 ns** (**1.70×**) · 9709 B **4.27 vs. 10.06 µs**
(**2.36×**) · 77684 B **49.82 vs. 98.31 µs** (**1.97×**). The Hugging Face
comparison this entry already noted as a Verbora win widened in the same
direction (now **18.8×–33.1×** faster, from the earlier 2.9×–4.4×). The
original table and its "general engine vs. purpose-built scanner" story
stay as an accurate history of the `captures_iter`-era implementation —
the purpose-built scanner is simply on Verbora's side now.

**Update, text-shaping migration (2026-08) — this entry has no live side
left.** `verbora-tokenizers` was rewritten to
`docs/design/text-shaping-contract.md`, and §3.4 removes `RegexpTokenizer`,
`Pattern` and the crate's `regex` dependency outright: Verbora performs no
regex or whitespace tokenization at *any* API, and a caller who wants it is
directed to `regex` directly. Both implementations this entry measured are
gone — the `captures_iter`-era engine of the original table *and* the
`find_iter`-plus-SWAR-scanner that reversed it. `benches/tokenizers.rs`'
`whitespace_tokenization` group was deleted with them, because timing
`tantivy::WhitespaceTokenizer` and Hugging Face `WhitespaceSplit` against
nothing would measure nothing about Verbora.

⚠ **Every figure in this entry is retired, and — unlike entries 26–29 — not
pending re-measurement: there is nothing to re-measure.** The original
3.6×–4.6× loss, the 1.11×/1.70×/2.36×/1.97× reversal, and the 18.8×–33.1×
Hugging Face win all describe a capability Verbora no longer has. No current
figure replaces them and none can until the capability returns.
`tantivy`'s and Hugging Face's own numbers are unaffected; neither crate
moved. The entry is kept, not deleted, because the finding it records — a
general pattern-driven engine losing to a purpose-built scanner, and then
beating it once the scanner was written on Verbora's side too — is the reason
the SWAR whitespace scanner existed at all, and that reasoning is what
`docs/COMPETITIVE_BENCHMARKS.md` §1.1's update block now points back to.

## 4. Word tokenization — Verbora vs. `tantivy::SimpleTokenizer` (Rust): a size-dependent crossover, not a one-sided loss

Included for completeness under this file's own "no cherry-picking" charter,
even though — unlike entries 1–3 — this is not a clean loss: Verbora is
slightly slower than `tantivy::SimpleTokenizer` on small documents and
faster on large ones, crossing over between the 1187 B and 9709 B sizes.

| | |
|---|---|
| **Capability** | `WordTokenizer` (splits on `[^A-Za-zА-Яа-я0-9_]+`) |
| **Competitor** | `tantivy` 0.26.1, `tantivy::tokenizer::SimpleTokenizer` |
| **Verbora result** | 123 B: **182.9 ns** · 1187 B: **1.285 µs** · 9709 B: **9.450 µs** · 77684 B: **115.12 µs** (medians, `word_tokenization/verbora/<size>`) |
| **Competitor result** | Same documents: **136.6 ns** · **1.155 µs** · **11.66 µs** · **124.91 µs** (`word_tokenization/tantivy/<size>`) |
| **Gap** | **Verbora is slower at the two smallest sizes** — **1.34×** at 123 B, **1.11×** at 1187 B — **and faster at the two largest** — **1.23× faster** at 9709 B, **1.09× faster** at 77684 B. Both implementations use a hand-written scanner (see entry 3's "Likely reason"), so the gap here is small either direction — under 1.4× at every size — consistent with two structurally similar implementations rather than a difference of algorithm class. |
| **Likely reason** | Both scanners do the same shape of work (a byte/char scan with one word-membership predicate per position), so the crossover most plausibly reflects constant-factor differences rather than a complexity difference: Verbora's `WordRuns` (`crates/verbora-tokenizers/src/scan.rs`) checks an ASCII-range `matches!` ladder plus a Cyrillic range per character and is generic over a monomorphised `CharClass`, while tantivy's `SimpleTokenStream::search_token_end` (`tantivy-0.26.1/src/tokenizer/simple_tokenizer.rs`) calls the standard library's `char::is_alphanumeric()`, a Unicode-table lookup covering a much wider class than Verbora's fixed ASCII+Cyrillic ranges. Which of "branchier predicate" (Verbora, small-input-friendly if it inlines well) vs. "table lookup" (tantivy, more uniform cost per character) wins is exactly the kind of small, size-dependent effect neither this file's methodology nor a single profiling pass can fully attribute without a dedicated microarchitectural investigation — recorded honestly as "not fully explained" rather than guessed at. |
| **Profiling evidence** | Read `crates/verbora-tokenizers/src/scan.rs` (`next_run`, the `CharClass` trait) and `tantivy-0.26.1/src/tokenizer/simple_tokenizer.rs` (`SimpleTokenStream::search_token_end`, `char::is_alphanumeric()`) directly. Real benchmark run: `cargo bench -p competitive-rust --bench tokenizers -- word_tokenization/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/tokenizers-word_tokenization-*.json`. Token-boundary correctness on the narrowed input domain: `benchmarks/competitive/rust-competitors/tests/tokenizers_correctness.rs`. |
| **Optimization opportunity** | None flagged: the two smallest-size losses are both under 1.35×, the crossover itself already shows Verbora ahead at realistic document sizes (9709 B and 77684 B), and the likely cause (predicate-ladder vs. Unicode-table lookup) is a constant-factor tuning question rather than an algorithmic gap — not the kind of finding this file's `Optimization opportunity` field exists to route into a dedicated future phase. |

**Update, text-shaping migration (2026-08) — the group survives, the Verbora
side does not.** `WordTokenizer` no longer splits on
`[^A-Za-zА-Яа-я0-9_]+` via the hand-written `WordRuns` class scanner: both the
character class and the scanner are removed by
`docs/design/text-shaping-contract.md` §3.4 (`classes`' nineteen predicates,
`scan`'s `CharClass`/`WordRuns`/`Source`/`SourceRuns`), and
`WordTokenizer::tokens` is now `str::unicode_words()` — full UAX #29
WB1–WB999 segmentation plus an `Alphabetic`/`Nd`/`Nl`/`No` filter. Verbora's
side of this comparison therefore does categorically **more** work than it did
when these numbers were taken, and more than either competitor does now.

The comparison itself stays genuine: `benches/tokenizers.rs`'
`word_tokenization` group still times Verbora against
`tantivy::SimpleTokenizer` and Hugging Face `Whitespace`, three independently
written implementations, and `tests/tokenizers_correctness.rs` re-proves they
draw identical boundaries on the narrowed punctuation-free ASCII domain before
any timing is trusted. Two things changed about its shape, both of which
affect what a re-measured number means: every Verbora row is now **two** rows
(`verbora` collecting a `Vec<&str>`, `verbora-lazy` counting the
allocation-free iterator), because the pre-migration file charged Verbora a
`Vec` allocation the competitors never paid; and the size grid widened from
four documents to eight.

⚠ **Every figure in this entry is retired pending re-measurement.** The
182.9 ns / 1.285 µs / 9.450 µs / 115.12 µs Verbora medians, the 1.34×/1.11×
small-size losses and the 1.23×/1.09× large-size wins, and the crossover
between 1187 B and 9709 B that this entry exists to document, all measured
`WordRuns`. Whether a crossover exists at all against a UAX #29 scanner is now
an open question rather than a recorded result.
`docs/design/text-shaping-contract.md` §7 item 1 names it as the migration's
single largest performance unknown — a regression is expected, on the hottest
path in the workspace, with an ASCII fast path recorded as the designated
fallback but deliberately not pre-built. `tantivy`'s and Hugging Face's own
numbers are unaffected; neither crate moved.

## 5. N-grams from string input (`ngrams_str`) — Verbora vs. The reference (the reference runtime)

No Rust competitor exists for n-grams at all (`docs/COMPETITIVE_BENCHMARKS.md`
§1.2: every dedicated Rust n-gram crate found is abandoned or solves a
different problem), so this entry's competitor is the reference itself — still
in scope for this file, since the reference is the spec's own required baseline
and a real, fair competitor, not merely a category this file happens to have
filled with Rust crates so far.

**A methodology note before the finding itself.** This benchmark's own
machine ran under sustained heavy concurrent load for the whole of this
module's work (several other Fase 6 module agents' `cargo bench`/`node`
processes running in parallel — `uptime` showed a load average above 21 on a
32-thread machine at points during this run; see `docs/PERFORMANCE.md`'s
Methodology section for the full disclosure). Two independent full runs of
`crates/verbora-ngrams`' n-gram benchmarks against the reference
were taken specifically to separate real signal from that noise:
`bigrams/collect_owned` (plain array-window materialization,
`ngrams_owned` on borrowed `&str` elements) **reversed sign** between the two
runs (0.9×/0.4×/0.5×/0.6× the first run, 1.3×/0.8×/0.8×/1.1× the second, at
16/256/4096/20000 elements respectively) and `string_input/tokenize` alone
did the same (0.7× the first run, 1.3× the second) — both are **not**
reported as gaps here; a ratio that flips sign between two runs on the same
machine is exactly the outlier this file's own charter says to document the
*methodology* for rather than dress up as a finding. `string_input/ngrams_str`
did not flip: it was a loss in both runs, and Verbora's own absolute number
barely moved between them (**444.87 µs** then **445.33 µs**, a 0.1%
difference) while the reference's moved more (192.68 µs then 207.60 µs) — the
one number in this whole investigation stable enough to build a "why" on.

| | |
|---|---|
| **Capability** | N-gram generation from a raw string via the string-input entry point (`ngrams_str`, mirroring `NGrams.ngrams(text, n)`'s string-input path — tokenize, then window every token sequence into an owned `Vec<Vec<String>>`) |
| **Competitor** | the reference 8.1.1, `NGrams.ngrams(text, 2)` (string input, default `WordTokenizer`) |
| **Verbora result** | 4096-word text: **444.87 µs** (run 1) / **445.33 µs** (run 2), median, `string_input/ngrams_str` |
| **Competitor result** | Same text: **192.68 µs** (run 1) / **207.60 µs** (run 2) |
| **Gap** | **~2.31× slower** (run 1) / **~2.15× slower** (run 2) — stable within a narrow band across two independent runs on a noisy machine, unlike the sibling benchmarks described above. |
| **Likely reason** | `ngrams_str` is `tokenize(text)` (owned `Vec<String>`, via `verbora_ngrams`'s own small `WordTokenizer` — `crates/verbora-ngrams/src/tokenizer.rs`, not `verbora_tokenizers::WordTokenizer` — whose `Tokenizer::tokenize_into` does `.map(str::to_owned)`, one allocation *and* one content copy per token) followed by `ngrams_owned(&tokens, …)`. The critical detail is `ngrams_owned`'s generic parameter: called on the corpus directly (as `bigrams/collect_owned` does) it is `T = &str` — cloning a `&str` is a cheap pointer-and-length copy, `Copy`-cheap in all but name. Called from `ngrams_str` on the *tokenized* sequence, `T = String` — and `Cow::into_owned()` on a borrowed slice clones every element via `Clone::clone()`, which for `String` allocates a fresh buffer and `memcpy`s the bytes. So each output n-gram in `ngrams_str` pays for `n` *full string content copies*, not `n` pointer copies — a real, structurally different (and heavier) cost than the plain array-of-references case, and the reason its signal survives system noise that swamps the lighter `&str`-only benchmarks. The reference pays an equivalent tokenize-then-copy cost on its own side (`sequence.slice(i, i+n)` over an array of reference strings the reference's own `WordTokenizer#tokenize` already produced), but on the reference engine's bump-pointer young-generation allocator rather than the workspace's plain system allocator — the same allocator-cost asymmetry as this file's other allocation-bound entries, compounded here by the extra full-string-copy step `T = String` adds on the Rust side specifically. |
| **Profiling evidence** | Read `crates/verbora-ngrams/src/text.rs` (`ngrams_str` calling `tokenize` then `ngrams_owned`), `crates/verbora-ngrams/src/tokenizer.rs` (`WordTokenizer`'s `Tokenizer` impl doing `.map(str::to_owned)`), and `crates/verbora-ngrams/src/engine.rs` (`ngrams_owned<T: Clone>`, `Cow::into_owned` — generic over `T`, so `T = String` at this call site clones full string content, vs. `T = &str` elsewhere in the same file) directly. Confirmed no custom global allocator via `grep -rn global_allocator crates/ Cargo.toml` (no matches). Real benchmark runs (×2, for reproducibility): `cargo bench -p verbora-ngrams -- string_input`. |
| **Optimization opportunity** | None that preserves the current contract cheaply: `ngrams_str`'s `Vec<Vec<String>>` return type is what lets it hand back fully independent, 'static owned data with no borrow tied to a temporary token buffer — reasonable for a convenience string-input API. A caller who tokenizes once and then calls the borrowing `ngrams`/`ngrams_iter` API directly (as `string_input/pretokenized`'s own benchmark does, and as `crates/verbora-ngrams/src/text.rs`'s module doc comment's "The cheap path" section recommends) avoids the double-copy entirely and is reproducibly **2.2×–2.5× faster tha reference's own equivalent** (21.0–21.6 µs vs. 47.5–53.4 µs across both runs) rather than ~2× slower — so the guidance for throughput-sensitive callers is already documented, not a missing feature. Not a candidate for a future optimization phase; recorded as a real, understood cost of the convenience API's shape. |

**Update, text-shaping migration (2026-08) — `ngrams_str` is gone, and so is
every function this entry contrasts it with.** `verbora-ngrams` was rewritten
to `docs/design/text-shaping-contract.md` §3.3, and §3.4 deletes the whole
`text` module (`ngrams_str`/`bigrams_str`/`trigrams_str`), the whole
`tokenizer` module including the small `WordTokenizer` whose
`.map(str::to_owned)` this entry identifies as the cost, `ngrams_owned` and
`ngrams_iter`/`NGramIter`. A caller now writes
`ngrams(&t.tokenize_borrowed(s), n)` against `verbora-tokenizers`, so the
`T = String` full-content-copy step that made this gap survive machine noise
does not occur on any path: `verbora-ngrams` has no string-input entry point
and no tokenizer of its own.

⚠ **Every figure in this entry is retired, and not pending re-measurement:
the function is deleted, not changed.** The 444.87/445.33 µs Verbora medians,
the ~2.15×–2.31× loss against the reference, and the 2.2×–2.5× win the
pre-tokenized path recorded beside it all describe an API that no longer
exists. The reference's own 192.68/207.60 µs figures are unaffected but have
nothing left to be compared against.

The *methodology* note at the top of this entry — two full runs taken
specifically to separate signal from a load average above 21, with
`bigrams/collect_owned` and `string_input/tokenize` discarded because they
reversed sign between them — is kept deliberately. It is the reason this entry
was ever trustworthy, and the discipline it demonstrates outlives the numbers
it was applied to.

## 6. Metaphone encoding — Verbora vs. `rphonetic` (Rust)

| | |
|---|---|
| **Capability** | Original (Lawrence Philips, 1990) Metaphone encoding, single call and 10,000/100,000-name batches, over a curated multilingual name list |
| **Competitor** | `rphonetic` 3.0.6, `Metaphone::new(Some(32))` (reconfigured to match Verbora's real default of 32 — the crate's own default is `Some(4)`), `Encoder::encode` |
| **Verbora result** | Single name (`"Kowalski"`): **175.6 ns** · 10,000 names: **1.975 ms** · 100,000 names: **19.596 ms** (medians, `metaphone/verbora/<size>`) |
| **Competitor result** | Same input: **78.2 ns** · **774.04 µs** · **7.792 ms** (`metaphone/rphonetic/<size>`) |
| **Gap** | **~2.25× slower** at a single call (175.6 ns vs. 78.2 ns), **~2.55× slower** at 10,000 names, **~2.51× slower** at 100,000 names. Consistent across all three scales — not a size-dependent crossover like entry 4, a genuine constant-factor loss. |
| **Likely reason** | Verbora's `Metaphone` is specified, and stays faithful to the reference, as **twenty-one ordered whole-string rewrite stages** (`crates/verbora-phonetics/src/metaphone.rs`'s `pipeline` function: `s_dedup`, `s_drop_initial_letters`, … `s_drop_vowels`, run in sequence through `Pipe`). `Pipe` (`crates/verbora-phonetics/src/pipe.rs`) holds exactly two buffers and swaps them after each rule — an encoding costs two allocations no matter how many rewrites run, which is what already makes this port ~4×–10× faster tha reference's own per-stage-allocating original (see this module's the reference comparison above) — but every one of the 21 stages still walks the whole buffer once, so the *character-touch* cost is `O(21n)` regardless of allocation count. rphonetic's `Metaphone::encode` (`rphonetic-3.0.6/src/metaphone.rs`) is the textbook single indexed forward scan: one pass over the (case-mapped, initial-letter-adjusted) string with a `skip` counter that consumes multi-character lookahead inline — `O(n)`. Twenty-one full passes over short (4–14 character) names is the direct, source-confirmed explanation for a consistent ~2.2×–2.6× gap: the two implementations are doing the same *algorithmic* job (per-word Metaphone encoding) with a structurally different amount of *scanning*, exactly the kind of difference this file exists to record rather than paper over. The `Some(32)` reconfiguration this bench applies is not the cause: `tests/phonetics_correctness.rs` confirms it genuinely changes rphonetic's observed output length (proving the crate is not silently still capped at 4), and real names in the shared dataset never approach a 32-character Metaphone code, so rphonetic's early-exit-at-`max_code_length` check essentially never fires — both sides are doing the same real work on this input, not a truncation artifact. |
| **Profiling evidence** | Read `crates/verbora-phonetics/src/metaphone.rs` (`pipeline`, the 21 `s_*` stage calls), `crates/verbora-phonetics/src/pipe.rs` (`Pipe::apply`'s two-buffer swap), and `rphonetic-3.0.6/src/metaphone.rs` (`Encoder::encode`'s single `for (index, symb) in local.chars().enumerate()` loop with a `skip` counter) directly, plus `rphonetic-3.0.6/src/lib.rs`'s `Metaphone::new`/`Default` (`max_code_length: Some(4)` vs. this bench's `Some(32)`). Real benchmark run: `cargo bench -p competitive-rust --bench phonetics -- metaphone/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/phonetics-metaphone-*.json`. Reconfiguration and output-shape correctness verified in `benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs` (`rphonetic_metaphone_max_code_length_is_genuinely_32_not_the_crate_default_of_4`). |
| **Optimization opportunity** | ~~A single-pass Metaphone (index-and-lookahead over one buffer, matching rphonetic's shape) is a legitimate throughput upgrade path for `verbora-phonetics`'s `Metaphone` — but collapsing 21 independently-testable, independently-documented rewrite stages (each pinned by its own unit test and traceable to a specific line of `lib/natural/phonetics/metaphone`, per this crate's own "these are transcriptions, not implementations" module doc comment) into one fused scan is a real parity-risk rewrite, not a tuning pass: the documented `ch`→`sh` stage-ordering bug and the reference's own commented-out test case are exactly the kind of subtle cross-stage interaction a fused rewrite could silently lose. Flagged as a real, evidence-backed opportunity for a **future, separate, dedicated phase** with its own parity re-verification against `fixtures/phonetics.json` — not implemented in Fase 6, per this project's "measure first, then a focused follow-up phase" discipline (the same discipline entry 1's bit-parallel-Levenshtein opportunity follows).~~ **Implemented in the follow-up optimization round, with the parity-risk warning honored — see the updates below (the second of which closes the entry).** |

**Update, this round (2026-08) — mostly closed: the fusion happened,
Verbora now wins the single-call case, and the batch loss narrowed to
~1.1× but did not flip.** The rewrite the opportunity above flagged was
implemented as a skip-gated single-driver pipeline: the 21 ordered
whole-string rewrite stages are fused into one driver with letter-mask
gates (a stage whose trigger letters never occur in the word is skipped
without scanning it), window edits, and fused rules, plus removal of the
per-call entry allocation (the owned lowercase buffer now doubles as the
first scratch buffer). The parity risk was handled exactly the way the
warning demanded rather than waved off: the 21-stage original is **kept
in-tree as the test oracle**, and the fused driver is byte-identical to it
over a ~900K-comparison differential corpus, additionally checked by
mutation testing in an independent adversarial audit. Re-measured, Verbora
vs. `rphonetic` medians: single name **61.4 vs. 75.9 ns** — a **1.24×
Verbora win**, from a 2.25× loss — but batches remain narrow, real losses:
10,000 names **862.80 vs. 776.24 µs** (**1.11× slower**) and 100,000 names
**8.58 vs. 7.87 ms** (**1.09× slower**). The old ~2.2×–2.6×
constant-factor loss is gone; what remains is a ~1.1× batch gap, recorded
here as a loss, not rounded away.

**Update, later pass (2026-08) — fully closed: the remaining batch losses
flip, and Verbora now wins all three sizes.** The fused driver's residual
per-call allocator traffic was pooled away: the pipeline's two scratch
buffers moved to a per-thread pool (`ASCII_SCRATCH` `thread_local!` in
`crates/verbora-phonetics/src/metaphone.rs`; `Driver` now borrows its two
scratch `Vec`s instead of owning them), and ASCII tokens fold lowercase
directly into pooled scratch with no intermediate `String` — cutting
per-call allocator traffic to the one returned output `String`. Parity
was handled the same way the fusion above handled it: byte-identical
output, verified by the crate's full differential suite (151 tests, the
~900K-comparison corpus against the retained 21-stage oracle, both
feature sets). Re-measured (full-default Criterion, quiet machine,
medians), Verbora vs. `rphonetic`: single name **51.5 vs. 73.5 ns**
(**1.43× faster**), 10,000 names **711.91 vs. 735.77 µs** (**1.03×
faster**), 100,000 names **6.893 vs. 7.565 ms** (**1.10× faster**). This
entry is closed: every benchmarked size is now a Verbora win.

## 7. Language detection — Verbora (via `whatlang`) vs. `whichlang` (Rust)

The largest gap recorded in this file, by a wide margin — reported in full,
not minimized, per this file's own charter. `whichlang`'s narrower
scope (13 of Verbora's 22 languages, and it cannot abstain — see
`docs/COMPETITIVE_BENCHMARKS.md` §1.9) is real context, not an excuse: it is
explained below and quantified separately in the accuracy report
(`benchmarks/competitive/rust-competitors/examples/language_accuracy.rs`),
never used to wave away the timing gap itself.

| | |
|---|---|
| **Capability** | Statistical language detection over free text (Verbora's `WhatlangDetector`, wrapping `whatlang` 0.18.0's combined alphabet+trigram model) |
| **Competitor** | `whichlang` 0.1.1, `whichlang::detect_language` (hashed n-gram linear model) |
| **Verbora result** | English text: short word **38.09 µs** · short phrase **36.97 µs** · sentence **32.12 µs** · paragraph **103.87 µs** (medians, `language_detection_by_length/verbora/<tier>`) |
| **Competitor result** | Same text: **92.6 ns** · **290.8 ns** · **757.1 ns** · **6.528 µs** (`language_detection_by_length/whichlang/<tier>`) |
| **Gap** | **~411× slower** at the short-word tier, narrowing (but still severe) as input grows: **~127×** at short phrase, **~42×** at sentence, **~16×** at paragraph — see "Likely reason" for why shrinking-with-length is exactly what the explanation below predicts, and see `docs/PERFORMANCE.md`'s own accuracy table for why this is *not* simply "whichlang is the better detector": lingua and Verbora both meaningfully out-accuracy whichlang at the short-word tier on this project's own 13-language test set (92.3%/76.9% vs. 69.2%). |
| **Likely reason** | Two structurally different algorithms, confirmed by reading both crates' source. `whichlang::detect_language` (`whichlang-0.1.1/src/lib.rs`) extracts ASCII n-gram/Unicode-class features into one fixed `[f32; NUM_LANGUAGES]` score array on the stack, hashes each feature with a single `murmurhash2` call, and does one dot-product-style accumulation pass — **zero heap allocations, one linear pass, 16 languages' worth of weights** baked into a `&'static` table (`weights.rs`). `whatlang`'s `Detector::detect` (the engine `WhatlangDetector` wraps) runs its documented `Method::Combined` — *both* an alphabet-filter pass (`generic_alphabet_calculate_scores`, four intermediate `Vec`s) *and* a trigram-frequency pass (two `HashMap`s plus their own intermediate `Vec`s) and merges the two — a **measured 25 heap allocations per call**, confirmed in `crates/verbora-language/benches/language.rs`'s own doc comment from a dedicated counting-allocator probe. 25 allocations plus two independent scoring passes against a **22-language** (vs. `whichlang`'s 13) reference set is a fundamentally heavier per-call shape, not a tuning gap — and the ratio shrinking from ~411× to ~16× as input grows is consistent with this: `whichlang`'s per-feature cost scales with input length too (more n-grams to hash), while `whatlang`'s fixed ~25-allocation overhead is largely length-*independent*, so it is amortized away at the paragraph tier but dominates completely at the short-word tier where the actual detection work is nearly free on both sides. |
| **Profiling evidence** | Read `whichlang-0.1.1/src/lib.rs` (`detect_language`'s `emit_tokens`/`murmurhash2`/single accumulation loop, no `Vec`/`HashMap` anywhere in the hot path) and `whichlang-0.1.1/src/weights.rs` (`NUM_LANGUAGES = 16`, `&'static` weight table) directly. `whatlang`'s 25-allocations-per-call figure is `crates/verbora-language/benches/language.rs`'s own documented finding from a dedicated, non-workspace counting-allocator probe (see that file's "Allocations per detection" section) — not re-measured here, cited as the existing, already-verified source. Real benchmark run: `cargo bench -p competitive-rust --bench language -- language_detection_by_length` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/language-language_detection_by_length-*.json`. Accuracy context: `cargo run --release -p competitive-rust --example language_accuracy`, output in `benchmarks/competitive/results/language-accuracy.json`. |
| **Optimization opportunity** | None flagged for `verbora-language` itself: `WhatlangDetector` is a thin, honest wrapper around `whatlang`'s own algorithm (see `docs/COMPETITIVE_BENCHMARKS.md` §1.9's explicit instruction that this pairing is "wrapper overhead, not a rival algorithm" from Verbora's side — the gap here belongs entirely to `whatlang` upstream's own combined-method design, not to anything `verbora-language` adds or could remove without dropping to a narrower, `whichlang`-like feature set and losing 9 of its 22 supported languages plus the `is_reliable()` abstention signal `crate::LanguageDetection` depends on for its honest "can be empty" contract. Swapping in a `whichlang`-style hashed-linear-model detector as an *additional*, opt-in `LanguageDetector` implementation (alongside `WhatlangDetector`, not replacing it) is a real, evidence-backed idea for a **future, separate, dedicated phase** — it would need its own from-scratch model/weights (whichlang's own table only covers 16 languages, 9 short of Verbora's 22) and its own accuracy validation before being trusted, which is out of scope for Fase 6's measure-not-optimize charter. Not implemented in Fase 6. |

## 8. Spellcheck corrections — Verbora vs. `symspell` and `harper-core` (Rust)

A textbook, expected result reported in full: `symspell` exists specifically
to be fast at the operation this entry measures, and it is — by a wide,
widening-with-distance margin. The matching construction-cost trade-off
(`symspell` pays far more to build its dictionary) is real context, not an
excuse for the query-time loss, and is quantified separately below rather
than used to offset it. `harper-core`, loaded with the same corpus, shows a
related but distinct pattern — a real loss at small corpora and distance 2,
but a crossover to a Verbora win at distance 1 on the two largest corpora —
covered in its own subsection below rather than folded silently into the
`symspell` numbers.

| | |
|---|---|
| **Capability** | Distance-bounded spelling correction against a loaded frequency dictionary (`get_corrections(word, max_distance)` / `SymSpell::lookup(word, Verbosity::All, max_distance)`), same 20,000-word `benches/data/words.json` corpus and per-word frequencies on both sides |
| **Competitor** | `symspell` 0.5.2, `SymSpell<AsciiStringStrategy>::lookup` |
| **Verbora result** | Distance 1, 8-char-word typo probe: **~22.0–24.3 µs** across all four corpus sizes (100: 22.00 µs · 1000: 22.38 µs · 10000: 24.31 µs · 20000: 22.95 µs, medians, `spellcheck_get_corrections_d1/verbora/<size>`). Distance 2, 6-char-word typo probe: **4.962 ms** at 1,000 words, **5.801 ms** at 20,000 words (`spellcheck_get_corrections_d2/verbora/<size>`) |
| **Competitor result** | Same probes, same corpora: distance 1 **~857–930 ns** across all four sizes (100: 921 ns · 1000: 930 ns · 10000: 869 ns · 20000: 857 ns). Distance 2: **2.306 µs** at 1,000 words, **3.501 µs** at 20,000 words |
| **Gap** | Distance 1: **~23.9×** slower at 100 words, **~24.1×** at 1,000, **~28.0×** at 10,000, **~26.8×** at 20,000 — roughly flat, corpus-size-independent. Distance 2: **~2,152×** slower at 1,000 words, **~1,657×** at 20,000 words — the gap *shrinks* as the corpus grows (symspell's own lookup cost grows with corpus size for a fixed `prefix_length`, while Verbora's candidate-generation cost is independent of corpus size and dominates throughout), but stays enormous at both points measured. |
| **Likely reason** | The two algorithms do fundamentally different amounts of work at query time, confirmed by reading both implementations directly. Verbora's `Spellcheck::get_corrections` (`crates/verbora-spellcheck/src/spellcheck.rs`'s `corrections_over`) generates every edit of the **query word** combinatorially, per distance level, at query time — `~54n + 26` raw candidates per level for an `n`-character word (`spellcheck_edits`'s own documented figure), each checked against a trie, with distance 2 re-running the generator on every distance-1 candidate (`~500×` more candidates per the crate's own doc comment). `symspell::SymSpell::lookup` (`symspell-0.5.2/src/symspell.rs`) instead generates deletion-only edits of the **dictionary words**, once, at `load_dictionary_line` time (paid in the `spellcheck_new` group, not here — see below), and stores them in a `HashMap<u64, Vec<Box<str>>>` keyed by a hash of the deletion. A query then generates only its *own* deletions (a strict subset of "every edit" — no insertions, substitutions or transpositions to generate, only deletions of the query itself) and does `O(1)`-average hash lookups against the precomputed table. This is precisely SymSpell's own published design goal (Wolf Garbe's original SymSpell writeup claims roughly three-orders-of-magnitude speedups over "naive" combinatorial edit-distance correction for exactly this reason), and the observed magnitudes — a low-tens-of-× gap at distance 1 widening to a low-thousands-of-× gap at distance 2 — is exactly the shape that prediction makes: distance 2 is where combinatorial edit generation's `O(candidates²)`-ish blowup (Verbora) is most punishing relative to a hash lookup whose cost barely changes with `max_distance` (symspell). |
| **Profiling evidence** | Read `crates/verbora-spellcheck/src/spellcheck.rs`'s `corrections_over` (the `for depth in 1..=distance` loop, `Edits::next_edit` generator) and `symspell-0.5.2/src/symspell.rs`'s `lookup` (`self.deletes.get(&self.get_string_hash(&candidate))`, the precomputed `deletes: HashMap<u64, Vec<Box<str>>>` populated in `create_dictionary_entry`/`edits_prefix`) directly — not assumed from either crate's docs. Real benchmark run: `cargo bench -p competitive-rust --bench spellcheck -- spellcheck_get_corrections_d1 spellcheck_get_corrections_d2` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/spellcheck-spellcheck_get_corrections_d1-*.json` and `spellcheck-spellcheck_get_corrections_d2-*.json`. Both engines are loaded from the identical corpus and per-word frequency counts (`frequencies()` in `benches/spellcheck.rs`), verified once, outside the timed code, in `tests/spellcheck_smoke.rs`. |
| **Construction-cost context (not an offset)** | `symspell`'s query speed is bought with real, and much larger, up-front cost: the `spellcheck_new` group (same benchmark run) shows `SymSpell::load_dictionary_line` over the same corpus taking **402.4 µs** (100 words) to **115.4 ms** (20,000 words) against Verbora's `Spellcheck::new` at **14.0 µs** to **3.58 ms** over the same range — symspell is **~29×–32× slower to build** across every size measured, and that gap *widens* with corpus size (delete-set precomputation is combinatorial per word, same shape as Verbora's own query-time cost, just paid once at load instead of on every query). For a process that builds one dictionary and answers many corrections, symspell's trade is a clear net win; for a process that rebuilds its dictionary often relative to how many corrections it answers, the trade inverts. Both numbers are real and neither cancels the other — recorded together so neither side of the trade-off is hidden. |
| **Optimization opportunity** | A delete-precomputed (SymSpell-style) index is the standard, well-understood fix for query-time correction latency at scale, but it is a different algorithm and a different data structure from Verbora's live combinatorial generator — not a tuning pass. It would also change `Spellcheck`'s cost model in the direction symspell's own numbers above show (much slower `new`, much faster `get_corrections`), which is a real API/behavior trade-off for callers to weigh, not a strict improvement — precisely the kind of change this project's charter reserves for a **future, separate, dedicated phase** with its own design discussion, not something to fold into Fase 6's measure-only pass. Not implemented in Fase 6. |

### 8b. The same query, against `harper-core` — a crossover, not a one-sided loss

`harper-core` is loaded with the identical `words.json` corpus and
frequencies as the `symspell` row above (same `benches/spellcheck.rs`
group). Unlike `symspell`, the result is not one-sided:

| Corpus | Distance | Verbora | `harper-core` | Ratio |
|---|--:|--:|--:|--:|
| 100 | 1 | 22.00 µs | 5.02 µs | **harper-core 4.4× faster** |
| 1,000 | 1 | 22.38 µs | 14.28 µs | **harper-core 1.6× faster** |
| 10,000 | 1 | 24.31 µs | 27.90 µs | Verbora 1.1× faster |
| 20,000 | 1 | 22.95 µs | 36.87 µs | Verbora 1.6× faster |
| 1,000 | 2 | 4.962 ms | 34.79 µs | **harper-core 142.6× faster** |
| 20,000 | 2 | 5.801 ms | 333.83 µs | **harper-core 17.4× faster** |

Verbora loses at small corpora and at distance 2 throughout, but *wins* at
distance 1 once the corpus reaches 10,000–20,000 words — the opposite
direction from the `symspell` comparison above, where Verbora loses at
every corpus size measured. `harper-core`'s `fuzzy_match_str` walks an FST
(finite-state transducer) with a Levenshtein-automaton bound
(`harper-core-2.8.0`'s own `spell` module, confirmed by reading
`Dictionary::fuzzy_match_str`'s signature and doc comment directly) — a
structure whose per-query cost grows with the *automaton's* work, not
linearly with corpus size the way Verbora's combinatorial edit-generation
does at a fixed word length; this is consistent with harper-core's
distance-2 numbers barely moving between the two corpus sizes shown (34.79
µs → 333.83 µs is corpus-size growth in the FST's own transition table, not
a repeat of Verbora's `O(candidates²)`-shaped blowup) while Verbora's
distance-1 numbers stay flat and its distance-2 numbers do not. Why Verbora
specifically overtakes harper-core at distance 1 only once the corpus is
large was not isolated further in this pass (no allocation-counting or
flamegraph probe was run for this sub-entry) — recorded as an open question
rather than guessed at, consistent with this file's own standard elsewhere
(entries 4, 10, 11, 14b) for effects not fully root-caused. |

**Optimization opportunity**: none flagged pending the profiling pass noted
above — the crossover means there is no single "harper-core wins, fix
Verbora" story to route into a future phase; the distance-2/small-corpus
loss shares the same underlying cause (and the same non-fix, per this
project's parity-preservation discipline) as the `symspell` entry above.

## 9. Seven of nine shared Snowball stemmers — Verbora vs. `rust-stemmers` (Rust)

The largest, most consistent loss recorded in this file, reported in full
per this file's own charter. `rust-stemmers` (the official Snowball-to-Rust
compiler's own output) beats Verbora's Snowball ports on seven of the nine
canonically-shared languages, often by several times, at every batch size
measured — only German and Dutch go Verbora's way.

| | |
|---|---|
| **Capability** | Per-word Snowball stemming (`stem`), batches of 4–1024 cycled words |
| **Competitor** | `rust-stemmers` 1.2.0, `Stemmer::create(Algorithm::{German,Spanish,French,Italian,Dutch,Norwegian,Portuguese,Russian,Swedish})` |
| **Verbora result** | 1024-word batch medians (`porter_<lang>/verbora/1024`): `de` **154.49 µs** · `es` **1.27 ms** · `fr` **942.92 µs** · `it` **669.96 µs** · `nl` **142.43 µs** · `no` **303.82 µs** · `pt` **709.07 µs** · `ru` **628.54 µs** · `sv` **310.00 µs** |
| **Competitor result** | Same batches (`porter_<lang>/rust-stemmers/1024`): `de` **156.57 µs** · `es` **257.65 µs** · `fr` **210.33 µs** · `it` **251.16 µs** · `nl` **249.56 µs** · `no` **49.38 µs** · `pt` **192.95 µs** · `ru` **81.21 µs** · `sv` **50.64 µs** |
| **Gap** | Verbora **wins** `de` (1.0×–1.7× across sizes) and `nl` (1.7×–2.8×) but **loses** the other seven, consistently across every batch size (4 to 1024): `es` 0.16×–0.32×, `fr` 0.22×–0.66×, `it` 0.34×–0.38×, `no` 0.15×–0.22×, `pt` 0.22×–0.27×, `ru` **0.12×–0.14×** (worst), `sv` 0.15×–0.17×. Full per-size table in `docs/PERFORMANCE.md`'s Stemmers section. |
| **Likely reason** | Two layers, both confirmed by reading source directly. **(1) A fixed, necessary cost every language pays, winners included.** Every Verbora Snowball port encodes to `Vec<u16>` and decodes back via `String::from_utf16_lossy` once per word — required for the reference's UTF-16-code-unit-indexed algorithm semantics (`units.rs`'s own "Cost" section) — while `rust-stemmers`' `SnowballEnv` (`rust-stemmers-1.2.0/src/snowball/snowball_env.rs`) works directly on the input's UTF-8 bytes via `Cow<str>` and never pays this round trip. This alone does not explain the split, since German and Dutch pay it too and still win. **(2) The actual discriminator: additional buffer allocations beyond that round trip.** Counting `.clone()`/`Vec::new()`/`Vec::with_capacity`/`.to_vec()` call sites in each language's `stem()`: `nl` 0, `de` 1, `pt` 3, `es`/`it` 12 each, `ru`/`sv` 14 each, `no` 18, `fr` 28 — closely tracking the win/loss split (zero-or-one sites: both winners; three or more: every loser, roughly deepening with count). `es.rs` read concretely: `PorterStemmerEs::stem` takes whole-buffer `.clone()` snapshots between Snowball steps (`after0`, `after1`, `after2a`) and repeatedly materializes region views as owned `Vec<u16>` copies (`rv_text`, `r1_text`, `r2_text` via `.to_vec()`) rather than borrowing slices — several full word-length allocations per word, on top of the round trip every language pays. `rust-stemmers`' equivalent step comparisons are `usize` cursor-position checks into one buffer, allocating only when a rule *actually replaces* text (`replace_s`). |
| **Profiling evidence** | Read `rust-stemmers-1.2.0/src/snowball/snowball_env.rs` (`SnowballEnv`'s `Cow<str>` field, `eq_s`/`eq_s_b` cursor comparisons, `replace_s`'s single allocation point) and `crates/verbora-stemmers/src/{de,es,fr,it,nl,no,pt,ru,sv}.rs` directly, including a call-site count (`grep -c '\.to_vec()\|Vec::with_capacity\|\.clone()\|Vec::new()'` per file) for the allocation-density comparison in the table above. Real benchmark run: `cargo bench -p competitive-rust --bench stemmers -- porter_` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/stemmers-porter_*-*.json`. Correctness of the compared languages (byte-exact `stem()` agreement on the benchmarked word lists, with the Russian `ё` and Dutch sticky-flag exclusions documented and verified rather than silently applied) is checked by `benchmarks/competitive/rust-competitors/tests/stemmers_correctness.rs`, run before this benchmark's timings were trusted. |
| **Optimization opportunity** | Slice-based, allocation-free region tracking (the shape `de.rs`/`nl.rs` and `rust-stemmers` itself already use) is a legitimate, well-understood fix for the seven losing ports' whole-buffer-`.clone()`/region-`.to_vec()` pattern — but it is a real rewrite of parity-critical code across seven independently-verified algorithms (each pinned by its own fixture-derived test suite), not a tuning pass: the snapshot pattern in `es.rs` exists specifically to make each Snowball step's before/after state easy to verify line-by-line against the reference reference, and collapsing it risks silently losing a subtle cross-step interaction the same way entry 6's Metaphone fusion risk was flagged. A real, evidence-backed opportunity for a **future, separate, dedicated phase** with its own re-verification against each language's fixture suite — not implemented in Fase 6, consistent with entries 1, 2 and 6's discipline. |

## 10. Four Snowball/dictionary stemmers at or below parity — Verbora vs. The reference (the reference runtime)

⚠ **Partly retired pending re-measurement, per language — enumerated rather
than blanketed, because the four languages here did not all change.**

- **`sv` and `ru` — retired.** Both modules' `stem()` now routes through
  `crate::among`'s `find_among` binary search (`sv.rs` builds `AmongTable`/
  `UnionTable` for `step1a`/`step3`; `ru.rs` builds nine of them), replacing
  the linear `ends_with` scan the recorded medians measured. See entry 34.
- **`ru`'s `tokenize_and_stem` rows — retired on a second, independent
  ground.** `verbora-stemmers` now tokenizes with
  `verbora_tokenizers::WordTokenizer` (UAX #29) instead of the fourteen deleted
  per-language character classes, and that runs inside the timed region for
  `tokenize-and-stem/ru/<n>` (it does not for the per-word `stem-per-word/*`
  rows). Boundaries move, so the token count per document moves too.
- **`fr-carry` — not retired on either ground.** `carry.rs` imports nothing
  from `among.rs` and its `stem()` is a table-driven 3-pass algorithm, not a
  suffix-alternation scan; the per-word row does not reach the tokenizer.
- **`uk` — named, not resolved.** Its `stem()` calls `ru::alt_suffix`, which is
  *still* the linear `for a in alts { ends_with(w, a) }` scan, so the obvious
  path is unchanged — but it is reached through `ru.rs`, whose sibling tables
  did convert. Whether `uk`'s timed path moved was not verified here, and
  asserting either way would be a guess. The campaign should settle it.

Competitor figures (the reference 8.1.1) are unaffected throughout.

Recorded in full, not minimized, per this file's own charter: fourteen of
`verbora-stemmers`' seventeen ported stemmers beat the reference by 1.5×–16.0×
(see `docs/PERFORMANCE.md`'s Stemmers section), but four sit at or below
1.0×, and the cause is not fully isolated — reported honestly as such,
consistent with entry 4's own precedent for a real, measured effect this
project's methodology could not fully attribute within one pass.

| | |
|---|---|
| **Capability** | Per-word stemming (`stem`), and for Russian additionally `tokenize_and_stem` at four document sizes |
| **Competitor** | the reference 8.1.1: `CarryStemmerFr.stem`, `PorterStemmerSv.stem`, `PorterStemmerRu.stem`/`.tokenizeAndStem`, `PorterStemmerUk.stem` |
| **Verbora result** | Per-word, 256 cycled words (medians, `stem-per-word/<lang>`): `fr-carry` **267.63 µs** · `sv` **82.55 µs** · `ru` **173.18 µs** · `uk` **165.72 µs`. Russian scaling (`tokenize-and-stem/ru/<n>`): 16 words **13.59 µs** · 128 words **113.77 µs** · 1024 words **921.07 µs** · 8192 words **8.54 ms** |
| **Competitor result** | Same inputs: `fr-carry` **243.09 µs** · `sv` **75.40 µs** · `ru` **141.93 µs** · `uk` **127.94 µs**. Russian scaling: **14.15 µs** · **115.42 µs** · **914.76 µs** · **7.58 ms** |
| **Gap** | **fr-carry 0.9×, sv 0.9×, ru 0.8×, uk 0.8×** at the per-word grain. Russian's `tokenize_and_stem` repeats the same ~0.9×–1.0× story at every one of four document sizes spanning a 512× range (16 to 8192 words) — four independent measurements agreeing, which is why this is reported as a real pattern rather than a single noisy sample, even though (see "Likely reason") no confirmed single cause was found. |
| **Likely reason** | **Not fully isolated.** Two plausible structural explanations were checked directly against source and both ruled out as the sole cause. (1) *Lowercasing.* The reference's own reference and Verbora both call an allocating `.toLowerCase()`/`to_lowercase()` on every `stem()` for `sv`, `ru`, `uk` — but also for `fr-porter`, `it`, `no`, `pt` (confirmed by reading `fr.rs`, `it.rs`, `no.rs`, `pt.rs` directly), which win 1.5×–3.7× despite paying the identical cost, so lowercasing alone cannot be the discriminator. (2) *Script.* `ru`/`uk` are Cyrillic, but `sv` is Latin-with-diacritics like `de`/`no` (which win comfortably) and `fr-carry` stays on `&str` (not `Vec<u16>`, per `crates/verbora-stemmers/src/lib.rs`'s own design note) and shares an alphabet with `fr-porter` (2.4×) and `it` (1.8×) — no script or buffer-representation property common to exactly these four languages, and only these four, was found. |
| **Profiling evidence** | Read `crates/verbora-stemmers/src/{de,es,fr,it,no,nl,pt,ru,sv,uk}.rs` directly to confirm which `stem()` implementations call `to_lowercase()` (fr-porter, it, no, pt: yes; de, es, nl: no) and which stay on `&str` vs. `Vec<u16>` (fr-carry: `&str`; the twelve Snowball ports: `Vec<u16>`). Real benchmark run: `cargo bench -p verbora-stemmers`; both read `benches/data/stemmer-words.json` (generated by `tools/bench-data/generate.py`) for identical per-language input. |
| **Optimization opportunity** | None flagged: without a confirmed cause, there is nothing concrete to route into a future optimization phase — flagging one anyway would be guessing, which this file's own standard (entry 4) explicitly declines to do. A dedicated per-language allocation-counting/profiling pass (the same technique `crates/verbora-language/benches/language.rs` already used to pin down entry 7's whatlang allocation count) is the natural next step, and is noted here as the concrete follow-up investigation this entry motivates, not as a guessed fix. |

## 11. Japanese normalization at the largest tested size — Verbora vs. The reference (the reference runtime)

| | |
|---|---|
| **Capability** | `normalize_ja` (Japanese width/kana/symbol normalization, the full four-stage pipeline) on mixed kana/kanji/fullwidth-ASCII text |
| **Competitor** | the reference 8.1.1, `normalizeJa` |
| **Verbora result** | 96 B: **1.03 µs** · 1536 B: **15.55 µs** · 24576 B: **250.01 µs** (medians, `normalize_ja/mixed/<bytes>`) |
| **Competitor result** | Same inputs: **54.80 µs** · **62.97 µs** · **213.51 µs** |
| **Gap** | Verbora is **53.1× faster** at 96 bytes and **4.0× faster** at 1536 bytes, then **0.9× — measurably slower** at 24576 bytes: a monotonic decline across a single 256×-scaling axis (same generator, `japaneseProse(repeats)` for `repeats` in 1/16/256, both sides), not three unrelated data points. |
| **Likely reason** | Not confirmed within this pass. `normalize_ja`'s only UTF-16 round trip is stage one (`expand_iteration_marks`, `s.encode_utf16().collect()` then `String::from_utf16_lossy`, confirmed by reading `crates/verbora-normalizers/src/ja.rs` directly); stages two through four (`converters::normalize`/`fix_fullwidth_kana`/`fix_composite_symbols`) call `table::translate`, a `&str`-based lookup with no obvious `O(n²)` shape. The pipeline is four sequential `O(n)` passes threaded through one `Cow` (per the module's own "Everything returns a `Cow`" design note), which should keep total cost linear — consistent with the 96 B and 1536 B results, not with the 24576 B one. No allocator-counting or flamegraph profiling was run in this pass to confirm or rule out a specific mechanism at the largest size. |
| **Profiling evidence** | Read `crates/verbora-normalizers/src/ja.rs` (`normalize_ja`'s four-stage `map_cow` chain, `expand_iteration_marks`'s UTF-16 round trip) directly — no allocation-counting probe was run for this entry (contrast entry 7, where one was). Real benchmark run: `cargo bench -p verbora-normalizers`; both read the identical `japaneseProse` generator. |
| **Optimization opportunity** | None flagged pending an actual profiling pass at the 24576 B size specifically (allocator counting or `perf`/flamegraph, the same techniques entries 6 and 7 already used successfully elsewhere in this file) — recording a fix without first confirming the mechanism would risk optimizing the wrong thing. Flagged as the concrete next step for a future pass, not guessed at here. |

**Update, text-shaping migration (2026-08) — `normalize_ja` is deleted, and
the investigation this entry was about to start cannot be run.**
`verbora-normalizers` was rewritten to
`docs/design/text-shaping-contract.md` §3.2, whose §3.4 removes
`normalize_ja`, `ja::converters`' seventeen functions, `ja/tables.rs` and the
shared `table.rs` the whole four-stage pipeline was built on. The crate's
public surface is now `nfd`, `nfc`, `nfkd`, `nfkc`, `remove_diacritics` and
`par_remove_diacritics_batch`. Nothing in it performs Japanese width, kana or
symbol normalization as a named operation; NFKC covers part of what stage four
did, as a general Unicode operation rather than a seventeen-conversion
pipeline (see entry 31), and kana ↔ kana conversion is reclassified as a
transliteration that `verbora-transliterators` does not yet ship (entry 30).

⚠ **Every figure in this entry is retired, and not pending re-measurement.**
The 1.03 µs / 15.55 µs / 250.01 µs Verbora medians, the 53.1× and 4.0× wins,
and the 0.9× loss at 24576 B that is the whole point of the entry, all measure
deleted code. The one open question this entry carried — what happens at
24576 B specifically, never resolved because no allocator-counting or
flamegraph pass was run — is now unanswerable rather than outstanding, and is
**withdrawn** rather than carried into the next campaign. The reference's own
54.80/62.97/213.51 µs figures are unaffected but have nothing left to compare
against.

## 12. Brill tagger lexicon lookup — Verbora vs. The reference (the reference runtime)

| | |
|---|---|
| **Capability** | Bare lexicon word→category lookup (`Lexicon::first_category` / `Lexicon#tagWord`), and the lexicon-only tagging pass (`tag_with_lexicon` / `tagWithLexicon`) that is built on it |
| **Competitor** | the reference 8.1.1, `Lexicon#tagWord` (`lib/natural/brill_pos_tagger/lib/Lexicon`) |
| **Verbora result** | Construction+first lookup: english **159.4 ns** · dutch **100.7 ns**. Lookup alone: hit-short **91.7 ns** · hit-long **89.0 ns** · miss **98.7 ns** · lowercase-retry **89.5 ns** · non-ascii **215.0 ns** · empty **69.4 ns**. `tag_with_lexicon`: 8 tok **893.8 ns** · 64 tok **7.96 µs** · 512 tok **64.34 µs** · 4096 tok **512.05 µs** (all medians) |
| **Competitor result** | Same probes: construction+first lookup english **15.8 ns** · dutch **69.6 ns**. Lookup: hit-short **12.9 ns** · hit-long **13.4 ns** · miss **136.0 ns** · lowercase-retry **15.9 ns** · non-ascii **58.5 ns** · empty **13.7 ns**. `tag_with_lexicon`: **168.8 ns** · **1.46 µs** · **6.19 µs** · **50.56 µs** |
| **Gap** | Verbora is slower on every row except `miss` (where Verbora is 1.4× *faster* — see "Likely reason"). Construction+lookup: **10.1× slower** (english), 1.4× slower (dutch). Bare lookup: **7.1× slower** (hit-short), **6.6× slower** (hit-long), **5.6× slower** (lowercase-retry), **3.7× slower** (non-ascii), **5.1× slower** (empty). `tag_with_lexicon`: **5.3× slower** (8 tokens) widening to **10.1× slower** (4096 tokens). All at nanosecond-to-low-microsecond absolute scale — genuinely small numbers on both sides, but a consistent, reproducible direction, not noise (see `docs/PERFORMANCE.md`'s POS Tagging section, which cites the same rows and confirms Verbora otherwise wins the vast majority of nanosecond-scale rows across this whole project). |
| **Likely reason** | Verbora's English/Dutch lexicons ship as a **packed binary index** rather than parsed JSON, specifically to make *first use* free — `crates/verbora-tagger/src/lexicon.rs`'s own module doc comment documents the trade explicitly: "First lookup: after full parse" (JSON, ~55 ms) vs. "~17 byte-compare probes" (packed index, 0 ms init). That "~17 byte-compare probes" figure is `StaticLexicon::find` (`crates/verbora-tagger/src/data.rs`): a textbook binary search over a sorted packed array — `O(log n)`, `log2(92_662) ≈ 16.5` rounds for the English lexicon, each round an unaligned `u32` offset read plus a byte-slice `.cmp()`. The reference's `Lexicon#tagWord` is `this.lexicon[word]` — one the reference engine property access on what is, after the first few calls, a monomorphic object with an inline cache, close to `O(1)`. A handful of nanoseconds' difference between "one hashed property read" and "seventeen rounds of binary search, each doing real work" is exactly what naming the two lookup strategies predicts. `miss` is the one row where Verbora *wins* (1.4×): a binary search on a genuine miss still terminates in the same ~17 rounds, but the reference's *reference* implementation retries the lookup a second time in lowercase on any falsy result (`tagWord`'s `if (!categories \|\| ...) categories = this.lexicon[word.toLowerCase()]`) — for the all-lowercase miss probe used here (`"zzzznotawordatall"`), that means the reference engine pays for the property miss *twice*, once directly and once after an unnecessary `toLowerCase()` allocation, while Verbora's `first_category` only retries when the input actually contains an uppercase byte (confirmed in `Lexicon::first_category`'s own `word.bytes().any(\|b\| b.is_ascii_uppercase())` guard) — a case this specific probe never triggers on either side otherwise. `tag_with_lexicon`'s widening-with-length gap (5.3×→10.1×) is the same per-token lookup cost simply repeated `n` times with nothing to amortize it against, since this benchmark deliberately excludes the rule pass. |
| **Profiling evidence** | Read `crates/verbora-tagger/src/data.rs`'s `StaticLexicon::find` (binary search, `u32_at` unaligned reads, byte-slice `Ord::cmp`) and `crates/verbora-tagger/src/lexicon.rs`'s `first_category` (the ASCII-uppercase guard deciding whether to retry lowercase) directly, plus `lib/natural/brill_pos_tagger/lib/Lexicon`'s `tagWord` (the unconditional lowercase retry on any falsy result, `typeof categories === 'function'` prototype-pollution guard included) directly. Real benchmark run: `cargo bench -p verbora-tagger`; both scripts read the identical hard-coded word lists (the reference harness's own doc comment on why they are copied rather than shared via JSON). |
| **Optimization opportunity** | A hash map (`FxHashMap<&str, Categories>`, matching this crate's own `Spellcheck`'s choice of `FxHashMap` for a similar keyed-lookup problem) would restore `O(1)` lookup, but it is a real trade-off, not a strict improvement: the packed binary index's entire purpose (per its own module doc comment's comparison table) is **zero-cost startup** — no allocation, no hashing, no parse step, the dictionary is read directly out of the compiled binary. A `HashMap` alternative would need to be built at first use (paying real time and allocating real memory, the exact cost the packed index exists to avoid — see this crate's own comparison of "JSON at first use" vs. "packed index" startup cost) or shipped as a second, larger, differently-encoded binary asset. Given the absolute scale involved (tens of nanoseconds per lookup, in a pipeline where the *rule* pass — not the lexicon pass — dominates `tag`'s total cost for every non-trivial rule set per `docs/PERFORMANCE.md`'s own `tag` numbers, where Verbora wins 1.6×–15.5×), this is flagged as a real but low-priority opportunity for a **future, separate, dedicated phase** that would need its own startup-cost/lookup-cost trade-off analysis before committing to a specific data structure — not implemented in Fase 6. |

## 13. TF-IDF ingestion — Verbora vs. `tfidf` (afshinm, Rust)

| | |
|---|---|
| **Capability** | TF-IDF corpus build/ingestion (`add_document`) |
| **Competitor** | `tfidf` (afshinm) 0.3.0, `TfIdf::add` |
| **Verbora result** | 4 docs: **8.73 ms** · 16 docs: **33.99 ms** · 64 docs: **134.79 ms** · 256 docs: **522.49 ms** (medians, `build/verbora/<n>`, each document a rotation of the same ~163 kB Wikipedia article) |
| **Competitor result** | Same documents: **1.10 ms** · **4.54 ms** · **18.76 ms** · **75.19 ms** (`build/afshinm/<n>`) |
| **Gap** | Verbora is **7.9× slower** at 4 documents, narrowing slightly to **6.9× slower** at 256 — a consistent ~7×-8× ratio across a 64× size range, not a fixed constant overhead. |
| **Likely reason** | Per `docs/COMPETITIVE_BENCHMARKS.md` §1.12 and this module's own competitive bench doc comment, this is a documented **build/query-time SPEED comparison only** — `tfidf` (afshinm) is architecturally the closest Rust match (a stateful struct with `.add()`/`.idf()`/`.tf()`/`.tfidf()`) but does categorically less work per document. Read `tfidf-0.3.0/src/tfidf.rs` directly: `TfIdf::add` is `document.split(' ').map(Term)` — a single pass splitting on the literal space byte, storing borrowed `&str` slices with **zero allocation, no lowercasing, no tokenizer, no stop-word filtering**. Verbora's `add_document(DocumentInput::Text(...), ...)` (`crates/verbora-tfidf/src/tfidf.rs`) reproduces the reference's `buildDocument` exactly: `text.toLowerCase()` (a full-string pass), the process-global `WordTokenizer` (a real word-boundary tokenizer, not a byte-split), a stop-word membership check per token, and an interner lookup-or-insert plus an incremental document-frequency table update per surviving token. Every one of those steps is required for Verbora's own parity contract (`docs/PARITY.md`) and for the O(1) query-time payoff measured below — afshinm's crate has neither the parity requirement nor the query-time optimization to pay for. |
| **Profiling evidence** | Read `tfidf-0.3.0/src/tfidf.rs`'s `add`/`add_vec` directly (no tokenizer, no lowercasing, no stop-word list, borrowed `&'a str` throughout — confirmed by the crate's own `pub struct TfIdf<'a> { pub documents: Vec<Vec<Term<'a>>> }` signature). Real benchmark run: `cargo bench -p competitive-rust --bench tfidf -- build/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/tfidf-build-*.json`. |
| **Optimization opportunity** | None flagged: closing this gap would mean dropping lowercasing, real tokenization, and stop-word filtering from `add_document`, which would break the reference parity (`crates/verbora-tfidf/tests/parity.rs`) — the two implementations are solving different problems at ingestion time, and the same benchmark file's query numbers show what Verbora's extra ingestion work buys back: `idf`/`tfidf` queries against a built corpus run in **~17-65 ns flat regardless of corpus size**, against afshinm's **milliseconds-to-hundreds-of-milliseconds per query, growing linearly with corpus size** (`tfidf_idf_256`: Verbora 16.3 ns vs. afshinm 225.4 ms — afshinm rescans every document on every single query, confirmed by reading `TfIdf::count`'s `self.documents.iter()` scan in the same source file). This is a genuine architecture trade — cheap-and-linear-per-query vs. expensive-and-O(1)-per-query — not a one-sided loss, and is reported as both directions per this project's `NO CHERRY PICKING` rule rather than only the row that favors Verbora. |

**Update, text-shaping migration (2026-08) — the tokenizer inside the measured
region was replaced.** This entry's whole explanation rests on what
`add_document(DocumentInput::Text(...), …)` does per token: lowercase, run a
real word-boundary tokenizer, check a stop-word list, intern. That is still
the shape, but the tokenizer is no longer the same one. `verbora-tfidf` calls
`verbora_tokenizers::WordTokenizer`, which the text-shaping migration
reimplemented on UAX #29 (`docs/design/text-shaping-contract.md`); the ASCII
SWAR bitmap that used to decide fast paths encoded `[a-z0-9_]`, and a
UAX #29-correct ASCII rule needs `MidLetter`/`MidNum`/`MidNumLet` lookahead
that a single bitmask cannot express — contract §7 item 4 flags as
`UNMEASURED` whether that path survives at all.

⚠ **The `build/verbora/<n>` medians (8.73/33.99/134.79/522.49 ms) and the
7.9×→6.9× ratio are retired pending re-measurement.** `tfidf` (afshinm)'s own
1.10/4.54/18.76/75.19 ms figures are unaffected; the crate is still pinned at
0.3.0 and does not tokenize at all, which is the asymmetry this entry
documents and which has not changed. The *finding* — that Verbora's ingestion
buys an O(1)-per-query index the competitor has no equivalent for — is
structural and survives; only its magnitude is now unbacked.

## 14. TF-IDF query-path costs — Verbora vs. The reference (the reference runtime)

| | |
|---|---|
| **Capability** | Several TF-IDF operations where the reference measured faster than Verbora on identical corpora (`crates/verbora-tfidf/benches/tfidf.rs` vs. the reference, both reading the same rotated-article corpus) |
| **Competitor** | the reference 8.1.1, `TfIdf` |
| **Verbora result** / **Competitor result** / **Gap** | Four separate rows: <br>• `idf_cold/deserialized/{1,8,64,256}` (a forced cache-miss `idf` recompute, scanning every document — the fallback path a corpus loaded via `TfIdf::from_json` always takes): Verbora 20.7 ns / 87.4 ns / 616.8 ns / 2.51 µs vs. The reference 17.9 ns / 68.1 ns / 455.0 ns / 1.82 µs — Verbora **0.7×-0.9× (consistently ~25-30% slower)** at every size. <br>• `query/tfidf_cold_cache` (one `tfidf()` call against a freshly-cleared idf cache, corpus built — not deserialized — so Verbora's `O(1)` document-frequency fast path applies): Verbora **38.27 µs** vs. The reference **2.78 µs** — Verbora **~13.8× slower**. <br>• `query/tfidfs_64_documents` (`tfidfs` over a 64-document corpus): Verbora **42.42 µs** vs. The reference **9.02 µs** — Verbora **~4.7× slower**. <br>• `documents/add_document_raw` (adding a 64-key raw/pre-tokenized document): Verbora **4.12 µs** vs. The reference **27.6 ns** — Verbora **~149× slower**. |
| **Likely reason** | Three different mechanisms, not one: <br>**`idf_cold/deserialized`** — a modest, consistent gap explained by reading `crates/verbora-tfidf/src/tfidf.rs`'s `docs_with_term`'s scanning fallback: each document check is `doc.get(term, &self.interner)`, a hash lookup plus `JsVal` pattern-matching, against the reference's `document[term] && document[term] > 0` — a single the reference engine inline-cached property read on a monomorphic object shape. A per-document hash probe genuinely costs more tha reference engine's optimized property access at this scale; this is the same class of effect `docs/PERFORMANCE.md`'s own trie section documents for hash-map-per-node structures, just smaller in magnitude here. <br>**`documents/add_document_raw`** — traced to the benchmark harness, not the library: `crates/verbora-tfidf/benches/tfidf.rs`'s own `bench_documents` times `t.add_document(DocumentInput::Raw(black_box(raw.clone())), ...)`, and that `raw.clone()` (a 64-`String`-keyed `JsonValue::Obj`) runs **inside** the timed closure, because the benchmark reuses one `raw` fixture across every Criterion sample and `add_document` takes ownership. The reference's equivalent never clones anything — `buildDocument`'s non-string, non-array branch returns the same object reference. A real Rust caller parsing one fresh raw document per call (the realistic case) would not pay this clone at all; the published number is real and reproducible as measured, but is documented here as measuring "clone a 64-entry object + ingest" rather than "ingest" in isolation. <br>**`query/tfidf_cold_cache`** and **`tfidfs_64_documents`** — **not confirmed within this pass**. A standalone probe (`std::time::Instant`, outside Criterion) measured the `tfidf()` call itself, excluding the untimed `iter_batched` corpus-clone setup, at ~1.2 µs — well under both Criterion's 38.27 µs figure and the reference's 2.78 µs — while a control benchmark (a large `Vec` clone as `iter_batched` setup ahead of a cheap 4-element sum as the timed routine) confirmed Criterion's `iter_batched` does correctly exclude setup time in a structurally similar case. The two results are in tension and were not reconciled with a sampling profiler in this pass; the Criterion number is real and reproducible (`cargo bench -p verbora-tfidf --bench tfidf -- query/tfidf_cold_cache`) and is published as measured, but the *mechanism* is left open rather than guessed at. |
| **Profiling evidence** | Read `crates/verbora-tfidf/src/tfidf.rs`'s `docs_with_term` (interner+`built_df` fast path vs. the `doc.get`-per-document scanning fallback) and `crates/verbora-tfidf/benches/tfidf.rs`'s `bench_documents`/`bench_query` directly. Ran a standalone `std::time::Instant` probe against `verbora-tfidf` via a scratch crate to isolate clone cost from query cost, and a control Criterion benchmark (`iter_batched` with an oversized setup and a trivial routine) to check for setup-time leakage — see "Likely reason" for both results. Real benchmark run: `cargo bench -p verbora-tfidf --bench tfidf`. |
| **Optimization opportunity** | `add_document_raw`: none needed in the library — the gap is a benchmark-fixture artifact (see "Likely reason"), not a real per-call cost; if this specific number is ever cited, it should be captioned as "clone + ingest", not "ingest alone". `idf_cold/deserialized`: a `FxHashMap`-backed lookup instead of the current `JsVal`-typed `get` path is a plausible, low-risk future micro-optimization, flagged but not attempted in this pass (deserialized/scanning corpora are already the documented slow path, used only when the O(1) incremental table cannot apply). `query/tfidf_cold_cache`/`tfidfs_64_documents`: flagged as needing a real sampling-profiler pass (e.g. `perf`/`samply`) before any optimization is attempted, precisely because the standalone probe above contradicts the Criterion number and guessing at a fix without resolving that contradiction first would risk optimizing the wrong thing — not implemented in Fase 6. |

**Update, text-shaping migration (2026-08) — two of the four rows run the
replaced tokenizer, two do not; recorded row by row rather than blanket-marked.**
Same cause as entry 13: `verbora_tokenizers::WordTokenizer` was reimplemented
on UAX #29.

- ⚠ **`query/tfidf_cold_cache` (38.27 µs) and `query/tfidfs_64_documents`
  (42.42 µs) are retired pending re-measurement.** `tfidf()` resolves its
  query through `Terms::Lowered(text)`, which tokenizes with
  `WordTokenizer` on every call (`crates/verbora-tfidf/src/tfidf.rs`), and
  `tfidfs` loops `tfidf`. The unresolved mechanism these two rows carry — a
  standalone `Instant` probe measuring ~1.2 µs against Criterion's 38.27 µs,
  never reconciled with a sampling profiler — is carried forward as still
  open, not closed by the migration; it must be re-probed against the new
  tokenizer rather than assumed to have moved.
- **`idf_cold/deserialized/{1,8,64,256}` is unaffected by this change.**
  `idf(term: &str)` takes an already-extracted term and never reaches a
  tokenizer; the row measures `docs_with_term`'s per-document scanning
  fallback. Its figures stand on the same footing they always did.
- **`documents/add_document_raw` is unaffected by this change**, for the same
  reason: a `DocumentInput::Raw` document is used verbatim, not tokenized. Its
  own caveat is unchanged and still applies — the number measures "clone a
  64-entry object + ingest", not "ingest".

`crates/verbora-tfidf/src/tfidf.rs` and `fast_build.rs` were themselves
substantially rewritten in the same change. This pass verified only which
rows' measured paths reach the tokenizer, not the full effect of that rewrite;
anything beyond the two rows marked above is unresolved rather than cleared.
The reference's own figures are unaffected throughout.

## 15. Naive Bayes training and prediction — Verbora vs. smartcore and linfa-bayes (Rust)

| | |
|---|---|
| **Capability** | Naive Bayes training (`add_document` × n + `train()`) and single-document prediction (`classify`) |
| **Competitor** | smartcore 0.6.5 `naive_bayes::multinomial::MultinomialNB`; linfa-bayes 0.8.1 `MultinomialNb` |
| **Verbora result** | Training, 4/16/64/256/1024 docs (`bayes_train/verbora/<n>`): **32.1 µs / 136.6 µs / 583.8 µs / 2.30 ms / 8.90 ms**. Prediction, one fixed 64-doc/6-class corpus (`bayes_predict/verbora`): **9.31 µs** |
| **Competitor result** | smartcore training: **5.3 µs / 24.6 µs / 89.9 µs / 313.9 µs / 1.52 ms** — consistently fastest. linfa-bayes training: **422.8 µs / 1.06 ms / 1.76 ms / 2.04 ms / 2.89 ms** — slower than Verbora at 4-64 docs, faster at 256 and 1024 (a crossover, not a one-sided result). Prediction: smartcore **3.71 µs**, linfa-bayes **1.02 µs**. |
| **Gap** | vs. smartcore: Verbora is **6.0×-6.9× slower at every training size** and **2.5× slower at prediction** — the one clean, one-sided loss in this entry. vs. linfa-bayes: Verbora is faster at small sizes (4 docs: Verbora 32.1 µs vs. 422.8 µs, **13× faster**; 16 docs: **7.8× faster**) but **slower from 256 docs on** (256: 0.9× ; 1024: **0.3×, i.e. 3.2× slower**) — a genuine crossover reported in both directions, and **9.1× slower** at prediction. |
| **Likely reason** | Verbora's `BayesClassifier::add_document`+`.train()` (`crates/verbora-classifiers/src/basic/classifier.rs`) does real, parity-required work per document: `Stemmer::tokenize_and_stem` (Porter stemming plus stop-word filtering via `verbora-stemmers`), a `JsMap`-ordered feature-vocabulary update reproducing the reference object key-enumeration order (`crate::jsmap`, needed because the reference's own feature indexing depends on it), and a `BTreeMap<u32, f64>`-keyed per-class count table. smartcore's and linfa-bayes' adapters (`benches/classifiers.rs`'s own `Vocab::build`) do none of that: a whitespace split, a lowercase, and a `HashMap<String, usize>` insertion-order vocabulary feeding a dense `DenseMatrix<u32>`/`Array2<f64>` that a highly-optimized, allocation-light multinomial-NB `fit`/`predict` routine consumes directly — no stemming, no stop-word filtering, no reference-order-preserving map. This is the same documented "pre-built dense count matrix, not raw text" gap `docs/COMPETITIVE_BENCHMARKS.md` §1.13 already flags for both competitors, now measured rather than only described; the crossover against linfa-bayes specifically (Verbora ahead below 256 docs, behind above) is consistent with linfa-bayes' own likely fixed per-`fit()`-call overhead (its published `dbg!` call alone writes one line to stderr per class on every `.fit()`, confirmed by reading `linfa-bayes-0.8.1/src/multinomial_nb.rs:78` directly) amortizing better as the corpus grows, while Verbora's per-document cost stays roughly proportional throughout. |
| **Profiling evidence** | Read `crates/verbora-classifiers/src/basic/classifier.rs`'s `add_document`/`text_to_features` (stemming, stop-word filtering, `JsMap` ordering) and `benches/classifiers.rs`'s own `Vocab::build`/`bench_train` (the whitespace-only adapter, built *inside* the timed closure to match Verbora's own text-in/model-out boundary — see that file's own doc comment) directly. Confirmed the `dbg!` call in `linfa-bayes-0.8.1/src/multinomial_nb.rs` by reading the published crate source, not assumed from behavior. Real benchmark run: `cargo bench -p competitive-rust --bench classifiers` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/classifiers-bayes_train-*.json` and `classifiers-bayes_predict-*.json`. |
| **Optimization opportunity** | None flagged as a like-for-like fix: Verbora's per-document cost is the price of features smartcore/linfa-bayes do not have at all (real tokenization, stemming, reference-order parity) — matching their preprocessing thoroughness would break `crates/verbora-classifiers/tests/parity.rs`. A `FxHashMap`-backed feature-count table in place of `BTreeMap<u32, f64>` is a plausible, parity-preserving micro-optimization worth a future look (this crate already reaches for `FxHashMap` elsewhere, e.g. `Classifier::text_to_features`'s presence set), but was not attempted or measured in this pass — flagged, not implemented, per this project's "measure first, then a focused follow-up phase" discipline. |

## 16. Classifier persistence (`to_json`/`restore`) — Verbora vs. The reference (the reference runtime)

| | |
|---|---|
| **Capability** | Serializing a trained `BayesClassifier` to JSON and restoring it |
| **Competitor** | the reference 8.1.1, `BayesClassifier`'s inherited `Classifier` (`JSON.stringify(this)` / `BayesClassifier.restore`) |
| **Verbora result** | `to_json`: **147.08 µs**. `restore`: **172.06 µs** (medians, `bayes_persist/{to_json,restore}`, same trained 64-document/6-class classifier both sides) |
| **Competitor result** | `to_json`: **48.12 µs**. `restore`: **61.45 µs** |
| **Gap** | Verbora is **3.1× slower** serializing and **2.8× slower** restoring. |
| **Likely reason** | Not root-caused with a profiler in this pass. Verbora's `to_json`/`restore` (`crates/verbora-classifiers/src/basic/classifier.rs`) reproduce the reference's exact `JSON.stringify(this)` byte shape — including own-property order, the empty `stemmer` object, and dead logistic-regression fields the reference's `save()`/`load()` still emit (documented in `verbora-classifiers`' own crate-level "Persistence" doc section) — which plausibly costs more tha reference engine's native, JIT-compiled `JSON.stringify`/`JSON.parse` built-ins, but this was not confirmed against an allocation-counting or sampling-profiler pass. |
| **Profiling evidence** | Read `crates/verbora-classifiers/src/basic/classifier.rs`'s `to_json`/`restore` and their value-conversion doc comments (the parity-shape requirements) directly; no allocator-counting or flamegraph probe was run for this entry. Real benchmark run: `cargo bench -p verbora-classifiers --bench classifiers`. |
| **Optimization opportunity** | Flagged for a future profiling pass (allocation counting or `perf`, the same techniques used successfully elsewhere in this file) before attempting a fix — the parity-shape requirement itself cannot be dropped, but the *mechanism* generating that shape (manual `JsValue` tree construction vs. a more direct serializer) has not been examined closely enough in this pass to recommend a specific change. |

## 17. Spellcheck construction memory — Verbora vs. `harper-core` (Rust)

The first MEMORY-dimension entry in this file (every entry above is a TIME
result). Real allocator counts via `benchmarks/competitive/rust-competitors/
src/memory.rs`'s `measure` — installed as `competitive-rust`'s
`#[global_allocator]` — not RSS or an estimate. Entry 8 already recorded
that `harper-core` construction is *faster in time* than Verbora's at every
corpus size; this entry adds the memory dimension entry 8 did not have, and
the shape of the two results does not match: `harper-core` beats Verbora on
memory at **every** size measured, not only the sizes where it also wins on
time.

| | |
|---|---|
| **Capability** | Building a spellchecking dictionary from a word list: `Spellcheck::new(words)` vs. `harper_core::spell::FstDictionary::new(entries)`, both loaded with the identical `benches/data/words.json` corpus (same words, same 100/1,000/10,000/20,000 sizes as entry 8 and `crates/verbora-spellcheck/benches/spellcheck.rs`'s own `CORPUS_SIZES`) |
| **Competitor** | `harper-core` 2.8.0, `spell::FstDictionary::new` |
| **Verbora result** | Net bytes retained after construction (`bytes_allocated - bytes_deallocated`, i.e. what stayed live once the closure returned — see `memory.rs`'s own doc comment on why this is reported separately from RSS): **43,140 B** (100 words) · **376,112 B** (1,000) · **3,338,824 B** (10,000) · **6,684,700 B** (20,000). Gross allocation churn (`bytes_allocated`): **47,856 / 439,644 / 3,945,124 / 7,897,224 B**. Allocation *count*: **263 / 2,225 / 22,347 / 43,984** |
| **Competitor result** | Net bytes retained: **28,872 B** (100) · **337,992 B** (1,000) · **2,703,432 B** (10,000) · **5,406,792 B** (20,000). Gross churn: **1,142,006 / 2,812,521 / 14,734,579 / 26,536,298 B**. Allocation count: **1,315 / 11,906 / 83,624 / 145,929** |
| **Gap** | `harper-core` retains **33.1% less** memory at 100 words (28,872 vs. 43,140 B — Verbora uses **1.49×** as much), **10.1% less** at 1,000 (**1.11×**), **19.0% less** at 10,000 (**1.24×**), **19.1% less** at 20,000 (**1.24×**) — a real, one-sided loss on the *net-retained* dimension at every size measured. The picture inverts on *gross churn*: `harper-core` allocates **23.9×** more total bytes at 100 words, **6.4×** more at 1,000, **3.7×** more at 10,000, **3.4×** more at 20,000 (and 3.3×-5.4× more allocation *calls* throughout) — Verbora does dramatically less allocator work to reach a construction that ends up holding more memory, the mirror image of `harper-core`'s result. Neither number cancels the other; both are reported per this file's own "no cherry-picking" charter. |
| **Likely reason** | Confirmed by reading `harper-core-2.8.0/src/spell/fst_dictionary.rs` directly, not assumed: `FstDictionary` is built on the `fst` crate's `MapBuilder` — a finite-state transducer that shares common prefixes and suffixes across the whole vocabulary and stores the result as a minimized automaton, the same data structure family search engines and spell checkers commonly reach for specifically because its resting memory footprint is close to the information-theoretic minimum for a sorted string set. Building that minimized form costs more allocator *activity* during construction (insertion into an FST builder is not a single append — it can revisit and rewrite recently-added states as new keys arrive in sorted order), which is exactly the 3×-24× gross-churn gap above. Verbora's `Spellcheck` is built on `verbora-trie`'s `Trie` (`crates/verbora-spellcheck/src/spellcheck.rs`), a straightforward (unminimized) trie: cheap, low-allocation-count construction, but no cross-branch state sharing, so common suffixes across the vocabulary are stored once per branch rather than once globally — the structural reason its resting footprint is larger. |
| **Profiling evidence** | Read `harper-core-2.8.0/src/spell/fst_dictionary.rs` (`fst::MapBuilder::memory()` at `FstDictionary::new`) and `crates/verbora-spellcheck/src/spellcheck.rs`'s `Spellcheck::new` (`Trie::new`-based) directly. Real run: `cargo run --release -p competitive-rust --example memory_report` in `benchmarks/competitive/`, both engines loaded from the identical `words()`/corpus-slice helper `benches/spellcheck.rs` already uses; raw counts written to `benchmarks/competitive/results/memory-report.json`'s `spellcheck.measurements` array (`verbora_new_<n>` / `harper_core_new_<n>` labels). Every measurement is a single real allocator trace (`memory::measure`'s own doc comment: no batching/statistics problem to solve for allocation counts, unlike Criterion's timing numbers), reproduced twice in this pass with byte-for-byte identical results both times. |
| **Optimization opportunity** | An FST-backed (or otherwise state-shared/minimized) storage layer is the standard fix for this shape of trie memory overhead, but it is a different data structure from `verbora-trie::Trie` — the same shared structure `verbora-spellcheck`, `verbora-trie`'s own public API, and (per `docs/COMPETITIVE_BENCHMARKS.md` §1.18) the Trie module's own competitive benchmarks all depend on for their documented UTF-16-code-unit-keyed, reference-`for…in`-ordered semantics. Minimizing it would touch parity-critical, publicly-depended-on code for a memory win only, not a fix within this pass's "measure, don't redesign" scope — flagged for a future, separate, dedicated phase, consistent with this file's own convention for structural (not tuning) fixes (see entry 1, entry 9). |

## 18. Language detection per-call memory — Verbora (via `whatlang`) vs. `whichlang` (Rust)

The memory-dimension counterpart to entry 7's TIME result, same pairing,
same root cause, seen from a different axis: real allocator counts via
`benchmarks/competitive/rust-competitors/src/memory.rs`'s `measure` (see
entry 17 for why this is a real allocator trace, not RSS or an estimate).

| | |
|---|---|
| **Capability** | Per-call allocator cost of statistical language detection (`WhatlangDetector::detect`), measured after one identical, unmeasured warm-up call on every implementation, on `datasets/language-accuracy/dataset.json`'s English `sentence`-tier text |
| **Competitor** | `whichlang` 0.1.1, `whichlang::detect_language` |
| **Verbora result** | 26 allocations, 11,476 bytes allocated, 13 deallocations, 11,468 bytes deallocated (`language_detection/detect/verbora`) |
| **Competitor result** | 0 allocations, 0 bytes allocated, 0 deallocations, 0 bytes deallocated (`language_detection/detect/whichlang`) |
| **Gap** | `whichlang` performs **zero heap allocations** for a call Verbora's `WhatlangDetector` performs 26 of — a categorical difference, not a ratio (dividing by zero): one side never touches the heap, the other does 26 times. Raw `whatlang::Detector::detect()` alone (the engine `WhatlangDetector` wraps, isolated from wrapper overhead per §1.9's own instruction) measured 25 of those 26 allocations — independently confirming, not merely citing, `crates/verbora-language/benches/language.rs`'s prior, separately-probed figure of exactly 25 for this shape of input. |
| **Likely reason** | The same structural difference entry 7 already names for the TIME gap, confirmed again here by reading source directly. `whichlang::detect_language` (`whichlang-0.1.1/src/lib.rs`) extracts features into one fixed-size `[f32; NUM_LANGUAGES]` array on the stack and hashes each with a single `murmurhash2` call — no `Vec`, no `HashMap`, nothing heap-allocated anywhere in its hot path. `whatlang::Detector::detect` (`Method::Combined`, the crate default) runs both an alphabet-filter pass (`generic_alphabet_calculate_scores`, four intermediate `Vec`s) and a trigram-frequency pass (two `HashMap`s plus their own intermediate `Vec`s) and merges the two; `WhatlangDetector::detect` itself adds one more `Vec` allocation for its own `candidates` field on this `Some`-result input, for 26 total. Entry 7's TIME gap and this entry's memory gap are the same underlying design difference — one hashed-feature stack pass vs. two multi-`Vec`/`HashMap` passes — read from two different instruments. |
| **Profiling evidence** | Read `whichlang-0.1.1/src/lib.rs` directly (already cited in entry 7). Real run: `cargo run --release -p competitive-rust --example memory_report` in `benchmarks/competitive/`, raw counts in `benchmarks/competitive/results/memory-report.json`'s `language_detection.measurements` array (`language_detection/detect/verbora`, `language_detection/detect/raw_whatlang`, `language_detection/detect/whichlang` labels). Every detection call is measured after one identical, unmeasured warm-up call on every implementation — `whatlang` lazily builds `ALPHABET_LANG_MAP` behind a process-wide `LazyLock` on its first-ever alphabet-path call (confirmed by reading `whatlang-0.18.0/src/alphabets/latin.rs`, already cited in `crates/verbora-language/benches/language.rs`) — so this is the steady-state cost, not a one-time initialization artifact conflated into it. |
| **Optimization opportunity** | None flagged, for the same reason entry 7's TIME gap has none: `WhatlangDetector` is a thin, honest wrapper around `whatlang`'s own algorithm (`docs/COMPETITIVE_BENCHMARKS.md` §1.9's explicit "wrapper overhead, not a rival algorithm" instruction), and the allocation count belongs to `whatlang` upstream's combined alphabet+trigram design, not to anything `verbora-language` adds or could remove without narrowing to `whichlang`'s own 13-language, no-abstention feature set — the same trade-off entry 7 already declines to make unilaterally to `WhatlangDetector` itself, flagging an additional opt-in detector as the real future option instead. Not implemented in Fase 6. |

## 19. Naive Bayes training and prediction memory — Verbora vs. smartcore, linfa-bayes and naivebayes (Rust)

The memory-dimension counterpart to entry 15's TIME result. Real allocator
counts via `memory.rs`'s `measure` (see entry 17). Measured at n=256 only
(entry 15's own TIME sweep covers 4–1024 docs; this pass measured one
"realistic size" point per this round's own scope, not the full sweep —
noted so the crossover entry 15 documents against linfa-bayes at larger
sizes is not overclaimed here). `naivebayes` (ruivieira) was pinned in
`Cargo.toml` by a sibling agent partway through this pass and is included
below — checked immediately before each measurement was written and added
once it landed, rather than skipped on a stale snapshot.

| | |
|---|---|
| **Capability** | Naive Bayes training (`add_document`/`train()`/`.fit()` × 256) and single-document prediction (`classify`/`predict`) on an already-trained/-fit model |
| **Competitor** | smartcore 0.6.5 `naive_bayes::multinomial::MultinomialNB`; linfa-bayes 0.8.1 `MultinomialNb`; naivebayes (ruivieira) 0.1.2 `NaiveBayes` |
| **Verbora result** | Training: 59,489 allocations, 2,890,139 bytes allocated (`classifiers/train/verbora_256`). Prediction, entry 15's own fixed 64-doc/6-class corpus: 229 allocations, 18,005 bytes allocated (`classifiers/classify/verbora`) |
| **Competitor result** | smartcore training: 7,197 allocations, 747,494 bytes (`classifiers/train/smartcore_256`); prediction: 19 allocations, 4,276 bytes (`classifiers/classify/smartcore`). linfa-bayes training: 6,472 allocations, 1,119,050 bytes (`classifiers/train/linfa_bayes_256`); prediction: 26 allocations, 5,072 bytes (`classifiers/classify/linfa_bayes`, model `fit` performed outside the measured region, matching entry 15's own `bench_predict` boundary). naivebayes training: 10,634 allocations, 283,074 bytes (`classifiers/train/naivebayes_256`); prediction: 61 allocations, 2,468 bytes (`classifiers/classify/naivebayes`, model trained outside the measured region, same boundary as the other two competitors) |
| **Gap** | Training, vs. smartcore/linfa-bayes: Verbora allocates **~8.3× more times** than smartcore and **~9.2× more** than linfa-bayes, **~3.9× more bytes** than smartcore and **~2.6× more** than linfa-bayes. Training, vs. naivebayes: **~5.6× more allocations**, **~10.2× more bytes**. Prediction, vs. smartcore/linfa-bayes: **~12.1× more times** than smartcore and **~8.8× more** than linfa-bayes, **~4.2× more bytes** than smartcore and **~3.5× more** than linfa-bayes. Prediction, vs. naivebayes: **~3.8× more allocations**, **~7.3× more bytes**. One-sided losses on both operations against all three competitors at this size — unlike entry 15's TIME result, which shows Verbora *ahead* of linfa-bayes below 256 docs; on memory, every competitor wins at 256 docs on both training and prediction. A secondary finding among the competitors themselves: naivebayes uses **fewer bytes** than either smartcore or linfa-bayes (283,074 vs. 747,494/1,119,050 training; 2,468 vs. 4,276/5,072 prediction) but **more allocations** than both (10,634 vs. 7,197/6,472 training; 61 vs. 19/26 prediction) — a many-small-allocations shape, not the dense-matrix-competitors' few-large-allocations one, consistent with naivebayes' pre-tokenized-`Vec<String>`/per-document training API rather than a pre-built matrix. |
| **Likely reason** | The same root cause entry 15 already names for the TIME gap, restated for allocator counts. Verbora's `BayesClassifier::add_document`+`.train()` (`crates/verbora-classifiers/src/basic/classifier.rs`) does real, parity-required work per document: `Stemmer::tokenize_and_stem` (Porter stemming, itself allocation-heavy), a `JsMap`-ordered feature-vocabulary update reproducing the reference object key-enumeration order, and a `BTreeMap<u32, f64>`-keyed per-class count table — a red-black tree that allocates roughly one node per insertion, a heavier allocation shape than a hash table's amortized bulk-resize pattern. smartcore's and linfa-bayes' adapters (`benches/classifiers.rs`'s own `Vocab::build`, built *inside* the same measured region as Verbora's own work, matching its raw-text-in boundary per that file's own doc comment) do none of that: a whitespace split, a lowercase, and a `HashMap<String, usize>` insertion-order vocabulary feeding a dense `DenseMatrix<u32>`/`Array2<f64>` that a purpose-built, allocation-light `fit`/`predict` routine consumes directly — no stemming, no `BTreeMap`, no reference-order-preserving map. `naivebayes` (`naivebayes-0.1.2/src/lib.rs`) sits in between structurally, not just in its numbers, confirmed by reading its source directly: `NaiveBayes::train` takes a pre-tokenized `&Vec<String>` (one `Vec` allocation per document from this file's own `classifiers_tokenize`, matching `benches/classifiers.rs`'s `tokenize`) and feeds each token into `Attributes::add`, which owns a nested `HashMap<String, HashMap<String, i64>>` — every token/label pair touches `attribute.to_string()` and `label.to_string()` (two fresh, small `String` clones per token, since the crate stores owned keys rather than borrowing from the caller's `Vec<String>`), plus an inner-`HashMap`-creation allocation the first time a given attribute is seen. No dense matrix is ever materialized (explaining its lower total bytes than smartcore/linfa-bayes' `DenseMatrix<u32>`/`Array2<f64>`), but two small `String` allocations per token rather than one dense-matrix cell write explains its higher allocation *count* than either. |
| **Profiling evidence** | Read `crates/verbora-classifiers/src/basic/classifier.rs`'s `add_document`/`text_to_features` (stemming, `JsMap` ordering, `BTreeMap`-keyed counts) directly — already cited in entry 15. Read `naivebayes-0.1.2/src/lib.rs`'s `NaiveBayes::train`/`classify` (`calculate_attr_prob`'s per-attribute `HashMap` lookups, no matrix type anywhere in the crate) directly. Real run: `cargo run --release -p competitive-rust --example memory_report` in `benchmarks/competitive/`, raw counts in `benchmarks/competitive/results/memory-report.json`'s `classifiers.measurements` array. An earlier draft of this measurement mistakenly left linfa-bayes' model `fit` call inside the measured `classify` closure (conflating training cost into a per-prediction number); corrected to fit the model once, outside the measured closure, before publishing the numbers above — matching `benches/classifiers.rs`'s own `bench_predict` boundary exactly; the same outside-the-measured-closure boundary was applied to naivebayes' `classify` row from the start. |
| **Optimization opportunity** | None flagged as a like-for-like fix, for the same reason entry 15 flags none: Verbora's per-document allocation cost is the price of features none of the three competitors have at all (real tokenization, stemming, reference-order parity) — matching their allocation profile would mean dropping stemming/stop-word filtering/`JsMap` ordering, breaking `crates/verbora-classifiers/tests/parity.rs`. Entry 15's own flagged idea — an `FxHashMap`-backed feature-count table in place of `BTreeMap<u32, f64>` — is also the most plausible parity-preserving lever on the memory axis (a hash table's bulk-resize pattern should reduce allocation *count* more than it reduces total *bytes*, relative to a per-node red-black tree), still not attempted or measured in this pass — flagged, not implemented, consistent with entry 15's own disposition. |

## 20. Naive Bayes training and prediction — Verbora vs. `naivebayes` (Rust)

| | |
|---|---|
| **Capability** | Naive Bayes training (`add_document` × n + `train()` vs. `NaiveBayes::train` × n) and single-document prediction (`classify`) |
| **Competitor** | naivebayes (ruivieira) 0.1.2, `NaiveBayes::train`/`classify` |
| **Verbora result** | Training, 4/16/64/256/1024 docs (`bayes_train/verbora/<n>`, Criterion's `median` estimate — see this entry's own "Profiling evidence" note on why `median`, not the console's printed slope-based range): **34.24 µs / 146.38 µs / 651.58 µs / 2.480 ms / 9.233 ms**. Prediction, entry 15's own fixed 64-doc/6-class corpus (`bayes_predict/verbora`): **10.757 µs** |
| **Competitor result** | Training: **7.707 µs / 29.757 µs / 98.081 µs / 300.10 µs / 975.47 µs**. Prediction: **3.698 µs** |
| **Gap** | A clean, one-sided loss at every size, growing with corpus size: **4.4×** slower at 4 docs, **4.9×** at 16, **6.6×** at 64, **8.3×** at 256, **9.5×** at 1024. Prediction: **2.9× slower**. |
| **Likely reason** | The same structural gap entry 19 (memory) already root-causes for this exact pairing, now showing up in wall-clock time too. Verbora's `add_document`+`.train()` (`crates/verbora-classifiers/src/basic/classifier.rs`) does real, parity-required work per document: Porter stemming and stop-word filtering (`Stemmer::tokenize_and_stem`), a `JsMap`-ordered feature-vocabulary update reproducing the reference object key-enumeration order, and a `BTreeMap<u32, f64>`-keyed per-class count table (a red-black tree, O(log n) per insert). `naivebayes` (`naivebayes-0.1.2/src/lib.rs`, read directly) does none of that: `Model::train` is a flat loop pushing into two `HashMap`s (`Labels::counts: HashMap<String, i64>`, `Attributes::attributes: HashMap<String, HashMap<String, i64>>`), O(1) amortized per insert, over tokens this file's own [`tokenize`] already split — no stemming, no stop-word filtering, no ordered-map overhead. The ratio growing with corpus size (4.4× at 4 docs to 9.5× at 1024) tracks entry 19's own memory-allocation-count ratio growing similarly, consistent with the same per-document cost difference driving both dimensions. |
| **Profiling evidence** | Read `naivebayes-0.1.2/src/lib.rs`'s `Model::train`/`Attributes::add`/`Labels::add` (flat `HashMap` inserts, no stemming/ordering) and `crates/verbora-classifiers/src/basic/classifier.rs`'s `add_document`/`text_to_features` (stemming, `JsMap` ordering, `BTreeMap`-keyed counts) directly. Real benchmark run: `cargo bench -p competitive-rust --bench classifiers -- bayes_` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/classifiers-bayes_train-naivebayes-*.json` and `classifiers-bayes_predict-naivebayes.json` — numbers above are each file's own `median.point_estimate`, matching `scripts/collect-results.py`'s own established convention (Criterion's console `time: […]` line prints the *slope* estimate's confidence interval instead, a different, less outlier-robust statistic — this benchmark's own run hit real outliers on a heavily CPU-contended shared machine, per the raw JSON's own outlier counts, which is exactly the condition `median` is more robust to). Correctness: `rust-competitors/tests/classifiers_naivebayes_logistic.rs`'s `naivebayes_agrees_with_verbora_on_a_clear_cut_case` and `naivebayes_smoothing_floor_is_fixed_not_count_based`, run before this timing number was trusted. |
| **Optimization opportunity** | None flagged as a like-for-like fix, same reasoning as entries 15/19: Verbora's per-document cost is the price of features `naivebayes` does not have at all (real tokenization, stemming, reference-order parity) — matching its preprocessing would break `crates/verbora-classifiers/tests/parity.rs`. Entries 15's/19's own flagged idea (an `FxHashMap`-backed feature-count table in place of `BTreeMap<u32, f64>`) remains the most plausible parity-preserving lever, still not attempted or measured in this pass. |

## 21. Logistic Regression training and prediction — Verbora vs. smartcore, linfa-logistic and rustlearn (Rust)

Three competitors, three different shapes — reported in full, not averaged
together, per this file's own no-cherry-picking charter.

| | |
|---|---|
| **Capability** | Logistic Regression training (`add_document` × n + `train()`) and single-document prediction (`classify`) |
| **Competitor** | smartcore 0.6.5 `linear::logistic_regression::LogisticRegression`; linfa-logistic 0.8.1 `MultiLogisticRegression`; rustlearn 0.5.0 `multiclass::OneVsRestWrapper<linear_models::sgdclassifier::SGDClassifier>` |
| **Verbora result** | Training, 4/8/16 docs (`logistic_train/verbora/<n>`, Criterion's `median` estimate — see entry 20's "Profiling evidence" for why `median` rather than the console's printed slope-based range): **53.03 µs / 153.33 µs / 419.77 µs**. Prediction, fixed 16-doc/3-class corpus (`logistic_predict/verbora`): **4.939 µs** |
| **Competitor result** | smartcore training: **288.77 µs / 398.14 µs / 651.43 µs**. linfa-logistic training: **81.29 µs / 101.57 µs / 151.46 µs**. rustlearn training: **4.621 µs / 10.14 µs / 23.69 µs**. Prediction: smartcore **617.7 ns**, linfa-logistic **446.4 ns**, rustlearn **413.8 ns**. |
| **Gap** | Training is genuinely mixed, not one-sided. vs. smartcore: Verbora **wins at every size tested** — **5.4×** faster at 4 docs, **2.6×** at 8, **1.6×** at 16 (the margin *narrowing* as the corpus grows). vs. linfa-logistic: a real crossover — Verbora is **1.5× faster** at 4 docs, then **1.5× slower** at 8 and **2.8× slower** at 16 (the *opposite* shape from the smartcore comparison: this one widens against Verbora as the corpus grows). vs. rustlearn: a clean, one-sided, growing loss — **11.5×** slower at 4 docs, **15.1×** at 8, **17.7×** at 16. Prediction is one-sided against all three: **8.0×** slower than smartcore, **11.1×** slower than linfa-logistic, **11.9×** slower than rustlearn. |
| **Likely reason** | Not one root cause — three different mechanisms. **rustlearn** (confirmed by reading `rustlearn-0.5.0/src/linear_models/sgdclassifier.rs`'s `SGDClassifier::fit` directly): a single pass over the rows per one-vs-rest binary sub-model, no convergence loop at all — the crate's own module doc comment states "repeated calls to the fit function are equivalent to running multiple epochs of training," and this benchmark calls `.fit()` exactly once, so rustlearn here does asymptotically *less total work* than Verbora's (`crates/verbora-classifiers/src/basic/logistic.rs`) full-batch gradient descent, which iterates per class until successive costs differ by less than `1e-4` — a genuinely different quantity of computation, not merely a faster implementation of the same one. **smartcore/linfa-logistic** (both `argmin`-family LBFGS, confirmed by reading `linfa-logistic-0.8.1/src/lib.rs`'s `argmin::solver::quasinewton::LBFGS`/`MoreThuenteLineSearch` imports and smartcore's own `optimization::first_order::lbfgs::LBFGS`): the crossover/narrowing-margin shape at these very small corpus sizes (4-16 documents) is consistent with a *fixed* per-`fit()`-call optimizer-setup cost (constructing solver state, line-search machinery) dominating more at the smallest sizes — plausible, not confirmed with a profiler in this pass, the same honest hedge entry 15 already applies to linfa-bayes' comparable crossover. **Prediction** follows the same shape entries 15/20 already document for Bayes: Verbora's `classify()` re-tokenizes and re-stems the probe text and does a hash-set feature lookup on every call, while all three competitors predict from an already-vectorized dense row (this file's own `Vocab::row`, a bare whitespace split) against a pre-fitted, allocation-light dot-product/softmax. |
| **Profiling evidence** | Read `rustlearn-0.5.0/src/linear_models/sgdclassifier.rs`'s `SGDClassifier::fit` and its own module doc comment directly — confirms this benchmark's single `.fit()` call is exactly one epoch, not iterate-to-convergence. Read `crates/verbora-classifiers/src/basic/logistic.rs`'s gradient-descent loop (iterates until successive costs differ by `<1e-4`) and `linfa-logistic-0.8.1/src/lib.rs`'s/smartcore's own LBFGS imports directly. Real benchmark run: `cargo bench -p competitive-rust --bench classifiers -- logistic_` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/classifiers-logistic_train-*.json` and `classifiers-logistic_predict-*.json`. Correctness: `rust-competitors/tests/classifiers_naivebayes_logistic.rs`'s `logistic_regression_competitors_agree_on_a_linearly_separable_case`, run before these timing numbers were trusted. |
| **Optimization opportunity** | The rustlearn gap is not a like-for-like fix opportunity: comparing a single-epoch SGD pass against iterate-to-convergence gradient descent is comparing two algorithms with different convergence guarantees, not two implementations of the same one — adopting single-epoch SGD would be an accuracy-affecting algorithm change, out of scope for a performance-only pass. The linfa-logistic crossover and the narrow-but-real smartcore win are consistent with fixed per-call LBFGS setup overhead (the same mechanism entry 15 already flags for linfa-bayes) but this was not confirmed with a profiler in this pass — flagged for a future look, not fixed here. |

## 22. Logistic Regression training memory — Verbora vs. smartcore, linfa-logistic and rustlearn (Rust)

The memory-dimension counterpart to entry 21's TIME result, at entry 21's own
largest training size (16 documents).

| | |
|---|---|
| **Capability** | Logistic Regression training (`add_document` × 16 + `train()`/`.fit()`), the same 16-document/8-word/40-vocab/3-class corpus entry 21's own `logistic_train` uses at its largest size |
| **Competitor** | smartcore 0.6.5 `LogisticRegression`; linfa-logistic 0.8.1 `MultiLogisticRegression`; rustlearn 0.5.0 `OneVsRestWrapper<SGDClassifier>` |
| **Verbora result** | 22,905 allocations, 1,633,112 bytes allocated |
| **Competitor result** | smartcore — 11,276 allocations, 797,144 bytes. linfa-logistic — 1,594 allocations, 1,484,076 bytes. rustlearn — 320 allocations, 17,750 bytes. |
| **Gap** | A one-sided loss against all three on allocation *count*: **2.0×** more than smartcore, **14.4×** more than linfa-logistic, **71.6×** more than rustlearn. On gross bytes allocated the picture is less extreme for two of the three — **~2.0×** more than smartcore, only **1.10×** more than linfa-logistic (linfa's optimizer machinery allocates a comparable number of total bytes, just in far fewer, larger allocations) — but **92.0×** more than rustlearn, the same rustlearn gap entry 21's TIME result already shows, now confirmed on the memory axis too. |
| **Likely reason** | Dominant factor, same as entry 21: rustlearn's `SGDClassifier::fit` (`rustlearn-0.5.0/src/linear_models/sgdclassifier.rs`, read directly) allocates its coefficient/gradient-accumulator arrays once, in `Hyperparameters::build`, and its per-row `fit` loop makes no further `Vec`/`String` allocation at all. Verbora's `LogisticEngine` and both LBFGS competitors instead allocate fresh intermediate vectors on every gradient-descent/optimizer iteration (Verbora: a `hypothesis`/`cost` vector per iteration per class, `crates/verbora-classifiers/src/basic/logistic.rs`; smartcore/linfa-logistic: their own LBFGS internal state, plus this file's `Vocab`-adapter matrix construction, measured inside the same closure on every side, matching the boundary entry 21/15 already establish). smartcore's higher allocation *count* despite a smaller total-*byte* figure than linfa-logistic suggests smartcore's LBFGS makes more, smaller per-iteration allocations where linfa's makes fewer, larger ones — a secondary finding among the competitors themselves, not root-caused further in this pass. |
| **Profiling evidence** | Read `rustlearn-0.5.0/src/linear_models/sgdclassifier.rs`'s `Hyperparameters::build`/`SGDClassifier::fit` directly (fixed-size arrays allocated once, no per-row allocation). Real measurement: `cargo test -p competitive-rust --release --test classifiers_memory -- --nocapture` in `benchmarks/competitive/`, using `competitive_rust::memory::measure` (`rust-competitors/src/memory.rs`) — see `rust-competitors/tests/classifiers_memory.rs`'s own `logistic_regression_training_memory`, which asserts every model actually classifies its own training probe correctly (not a degenerate/optimized-away measurement) before printing these numbers. |
| **Optimization opportunity** | Same disposition as entry 21: the rustlearn gap reflects a different algorithm (one SGD epoch vs. iterate-to-convergence gradient descent), not a tuning opportunity. No parity-preserving lever identified for the smartcore/linfa-logistic gap in this pass — flagged, not implemented. |

## 23. Sentence tokenization at large document sizes — Verbora vs. `unicode-segmentation` (Rust)

A genuine, reproduced crossover, not a one-sided loss: Verbora **wins** at the
two smallest sizes tested and **loses by a widening margin** at the two
largest — included in full per this file's own no-cherry-picking charter,
the same shape entry 4 already documents for `WordTokenizer` vs.
`tantivy::SimpleTokenizer`.

| | |
|---|---|
| **Capability** | Sentence tokenization (`SentenceTokenizer`), on the narrowed plain-declarative-sentence domain `benches/tokenizers.rs`'s own `sentence_prose` builds (no abbreviations/URIs/digits/quotes/brackets — the domain `tests/tokenizers_correctness.rs` proves all three implementations agree on) |
| **Competitor** | `unicode-segmentation` 1.13.3, `str::unicode_sentences()` and `str::split_sentence_bounds()` (matrix §1.1, "Yes") |
| **Verbora result** | 200 B (4 sentences): **1.926 µs** · 1836 B (32 sentences): **17.87 µs** · 14806 B (256 sentences): **214.7 µs** · 118588 B (2048 sentences): **4.898 ms** (Criterion `median.point_estimate`, `sentence_tokenization/verbora/<size>`, matching `results/results.json`) |
| **Competitor result** | `unicode-sentences`: **2.527 µs** · **22.59 µs** · **180.6 µs** · **1.458 ms**. `unicode-bounds`: **2.386 µs** · **23.02 µs** · **186.6 µs** · **1.442 ms** (same sizes, `sentence_tokenization/unicode-sentences\|unicode-bounds/<size>`) |
| **Gap** | **Verbora wins at the two smallest sizes** — **1.31×** faster than `unicode-sentences` / **1.24×** faster than `unicode-bounds` at 200 B (4 sentences); **1.26×** / **1.29×** faster at 1836 B (32 sentences) — **and loses at the two largest, by a widening margin**: **1.19×** / **1.15×** slower at 14806 B (256 sentences), **3.36×** / **3.40×** slower at 118588 B (2048 sentences). Reproduced across two independent full runs of the group (`cargo bench -p competitive-rust --bench tokenizers -- sentence_tokenization`); the crossover direction and its widening trend at the two largest sizes reproduced identically both times, only the exact ratios moved by a few percent — consistent with real machine noise on a shared box (see `docs/PERFORMANCE.md`'s own Tokenizers methodology note), not with the crossover itself being noise. |
| **Likely reason** | Confirmed by reading `crates/verbora-tokenizers/src/sentence.rs` directly, not assumed. `SentenceTokenizer::split` calls `unmask(&s, &delimiters)` once per output sentence, where `delimiters` is the **whole document's** placeholder map (one entry per sentence boundary in the entire text, built once up front by the `mask(...)` pass) — not a per-sentence-local map. `unmask`'s cheap-rejection short-circuit (`map.is_empty() \|\| !s.contains("{{")`) cannot fire for the delimiters map on this domain: `split_on_delimiters` deliberately keeps each sentence's own trailing `{{DELIM_n}}` placeholder attached to the text it splits off (so the delimiter can later be restored to its literal `. `), which means essentially every emitted sentence chunk (all but the last) genuinely contains a `{{` and *does* enter `unmask`'s main loop — `for (code, original) in map`, an iteration over **every** placeholder in the whole document, for **every** sentence. That is `O(sentences)` work repeated once per sentence: `O(sentences²)` total for this one pass alone, on top of the rest of `split`'s otherwise-linear work (masking, delimiter-splitting, `unmask`'s own internal early-outs via `present`/`present_codes` only prune *which* map entries do real substitution work per sentence, not how many entries the outer loop must visit to find them). `unicode-segmentation`'s two APIs are a single forward `CharIndices`-style scan with no whole-document side table consulted per emitted span — genuinely `O(n)`. This is exactly the shape that predicts a small, constant-factor Verbora win at low sentence counts (where the quadratic term is negligible) crossing over to a large, widening Verbora loss as sentence count grows — which is precisely what both runs show. |
| **Profiling evidence** | Read `crates/verbora-tokenizers/src/sentence.rs`'s `split` (the `.map(\|s\| { let unmasked = unmask(&s, &replacements); unmask(&unmasked, &delimiters) })` closure, closing over the whole-document `delimiters` `Vec`), `split_on_delimiters` (confirms each split keeps its own trailing `{{DELIM_n}}` attached — the file's own doc comment on that function explains why), and `unmask`/`present_codes` (the `for (code, original) in map` loop with no index by code — a linear `Vec`, not a `HashMap`) directly — not assumed from behavior alone. Independently confirmed on the **memory** axis, not just time: `cargo run -p competitive-rust --release --example memory_report` reports `sentence_tokenizer_verbora_2048s` at **26,460 allocations / 2,504,315 bytes** for the 2048-sentence document, against **0 allocations** for both `sentence_tokenizer_unicode_sentences_2048s` and `sentence_tokenizer_unicode_bounds_2048s` (their `.count()` calls never materialize an owned `String`) — see `benchmarks/competitive/results/memory-report.json`'s own `tokenizers` section. Real benchmark run: `cargo bench -p competitive-rust --bench tokenizers -- sentence_tokenization` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/tokenizers-sentence_tokenization-*.json`. Correctness of the compared sentence boundaries (not just counts) on this narrowed domain is checked by `tests/tokenizers_correctness.rs`'s `sentence_tokenization_agrees_on_narrowed_domain`, run before any of these numbers were trusted. |
| **Optimization opportunity** | ~~Indexing `delimiters`/`replacements` by placeholder code (a plain `HashMap<&str, &str>`)...~~ **Implemented — but not as a plain hash map; see the update below for why that original idea was itself wrong.** |

**Update, later pass — implemented, and a real bug found along the way.** This entry's own original "Optimization opportunity" reasoning turned out to be **incomplete**: it claimed map iteration order "only matters within one sentence's own placeholders," but a real counterexample (found during implementation, not anticipated up front) disproves that — see below. A plain `HashMap<&str, &str>`, with no ordering information at all, would have been the wrong fix.

What shipped instead (`crates/verbora-tokenizers/src/sentence.rs`): `code_positions` builds a `HashMap<&str, usize>` — code to its position in `map` — once per document (`split`, outside the per-sentence loop), not once per sentence. `unmask` uses it to avoid visiting irrelevant `map` entries, but the set of positions to visit cannot be fixed up front: it starts from the codes present in the sentence *before* any substitution, then **grows** — via a `BTreeSet<usize>` frontier, popped in ascending order — whenever a substitution reveals a code whose position is *still ahead* of the position that revealed it. A **first attempt** fixed the position set once, up front, reasoning that a later masking phase's stored text can only ever reference an *earlier* phase's placeholder (true — see `nested_placeholders_are_left_unresolved`) — but an independent adversarial audit found the real gap: a caller-supplied abbreviation's own *literal text* can coincidentally spell out `{{BASE_n}}` syntax for a placeholder not yet created (`with_abbreviations(["{{ABBREV_1}}", "Y."])`), which the fixed-position version silently left unresolved even though the reference's real single-pass sweep — and a full-scan Rust port of it — still resolves it whenever that position hasn't been "passed" yet. The growable-frontier version fixes this exactly, confirmed by a regression test (`unmask_resolves_a_forward_referencing_abbreviation_across_a_sentence_boundary`) the fixed-position version failed and the final version passes.

Correctness before any of this was trusted: this crate's full test suite (75 unit tests, 16 doctests, all passing), a 700-round randomized differential test (`unmask_matches_a_full_scan_on_randomized_documents`/`..._with_no_abbreviations_configured`) against a preserved full-scan reference implementation, and the independent audit above (fresh eyes, no visibility into the implementation's own reasoning, which found the bug the author's own differential-test corpus never happened to generate — its fixed `SENTENCE_PIECES` set never produces the forward-reference shape, a real, disclosed limit of that test's own coverage).

**Real result — the loss is gone, not just narrowed.** Re-measured
(`cargo bench -p competitive-rust --bench tokenizers -- sentence_tokenization`,
same four sizes, same machine): Verbora now **wins at all four sizes**,
including the two largest where it previously lost by up to 3.40×.

| Size | Verbora (before → after) | `unicode-sentences` | `unicode-bounds` | Verbora vs. sentences | Verbora vs. bounds |
|---|--:|--:|--:|--:|--:|
| 200 B (4 sentences) | 1.926 µs → **2.070 µs** | 2.409 µs | 2.363 µs | 1.16× faster | 1.14× faster |
| 1836 B (32 sentences) | 17.87 µs → **17.76 µs** | 21.67 µs | 20.96 µs | 1.22× faster | 1.18× faster |
| 14806 B (256 sentences) | 214.7 µs → **151.9 µs** | 177.8 µs | 173.6 µs | 1.17× faster (was 1.19× **slower**) | 1.14× faster (was 1.15× **slower**) |
| 118588 B (2048 sentences) | 4.898 ms → **1.186 ms** | 1.361 ms | 1.356 ms | 1.15× faster (was 3.36× **slower**) | 1.14× faster (was 3.40× **slower**) |

The crossover this entry originally documented is gone entirely — Verbora's
own time no longer grows superlinearly with document size (2048 sentences is
**~7.8×** the wall-clock of 256 sentences for an 8× larger document — roughly
linear, the scaling a single-pass algorithm should have — where the pre-fix
version was **22.8×** for the same 8× size increase, the signature of the
`O(sentences²)` term this entry identified; independently reproduced in a
later, standalone re-run at **~8.0×**, the numbers published in
`site/benchmarks/competitive.md`'s Sentence tokenization section). The
remaining
~1.14×–1.22× gap across every size is now a flat, size-independent constant
factor, not a widening one — consistent with `unicode-segmentation`'s own
single forward-scan design still being a genuinely leaner data path than
Verbora's placeholder-mask-and-restore one, not with any remaining quadratic
term. Closing *that* residual gap would need a different, non-placeholder-
based algorithm entirely — a much larger change than this pass, and not
attempted here.

**Update, text-shaping migration (2026-08) — the different algorithm this
entry's last paragraph asked for is what shipped, and it dissolves the
comparison rather than winning it.** `SentenceTokenizer` was rewritten to
`docs/design/text-shaping-contract.md` §3.1: it is now built directly on
`str::split_sentence_bound_indices()` — UAX #29 §5 — with an optional
abbreviation tailoring that re-joins segments across suppressed boundaries,
and nothing else. There is no `mask` pass, no `{{DELIM_n}}` placeholder, no
`unmask`, no `code_positions` map and no `BTreeSet` frontier. Contract §3.1
also removed sentence trimming outright, because a tokenizer that trims does
not return substrings, so a Verbora sentence now carries its own trailing
whitespace and concatenating the output reproduces the input exactly.

That change does two independent things to this entry, and they must not be
conflated:

- ⚠ **Every figure is retired.** The 1.926 µs / 17.87 µs / 214.7 µs / 4.898 ms
  Verbora medians, the 3.36×/3.40× loss at 2048 sentences, the post-fix
  1.16×–1.22× win table, and the ~22.8×-to-~7.8× scaling evidence for the
  `O(sentences²)` term all measure code that no longer exists. So does the
  companion memory finding (26,460 allocations / 2,504,315 bytes at 2048
  sentences against 0 for both `unicode-segmentation` APIs): the allocations
  counted were the placeholder machinery's.
- **The competitor is now the implementation.** `unicode-segmentation` is
  what `SentenceTokenizer` is built on, so a Verbora-versus-`unicode-
  segmentation` row is Verbora against its own dependency, which `AGENTS.md`
  § Cross-Implementation Benchmark Fairness forbids reporting as a competitive
  result. `benches/tokenizers.rs` split those rows out into
  `sentence_tokenization_wrapper_overhead`, whose numbers state what the
  suppression check and the `Vec` cost over the primitive and are never to be
  read as Verbora beating or losing to `unicode-segmentation`. **This entry is
  therefore not pending re-measurement — the gap it records cannot recur in
  the form it was recorded, because the two sides are no longer rivals.** The
  live sentence-tokenization rival comparison is against `segtok`
  (`sentence_tokenization`, `sentence_tokenization_boundary_density`), which
  *is* pending re-measurement — see `docs/COMPETITIVE_BENCHMARKS.md` §1.1.

The implementation history above is kept deliberately, and is worth more than
the numbers were: the `code_positions` fix, the first attempt that fixed the
position set up front, and the adversarial audit that found a caller-supplied
abbreviation spelling out `{{ABBREV_1}}` syntax for a not-yet-created
placeholder, together record why a placeholder-substitution tokenizer is hard
to get right — which is a substantial part of the argument for the rewrite
that replaced it.

## 24. English Porter stemming — Verbora vs. `porter-stemmer` (samgiles, Rust)

⚠ **Every Verbora figure in this entry is retired pending re-measurement**, for
the reason entry 34 now carries: `en.rs`' suffix matching was rewritten from
the linear `ends_with` scan this entry describes to the Snowball runtime's
`find_among` binary search (`crates/verbora-stemmers/src/among.rs`;
`units::longest_suffix`/`first_suffix` no longer exist). That reaches the
`step2`/`step3`/`step4` tables directly, which is where the `porter_en` timings
were spent. Competitor figures are unaffected — but note the update below
already found `porter-stemmer`'s own medians moving substantially between
measurement sessions, so the *ratios* must be re-derived from one fresh run,
not by pairing a new Verbora number with an old competitor one. The verdict
this entry reaches ("Verbora loses at every size") is likewise unbacked and may
not be inferred either way.

Found while wiring the matrix's `porter-stemmer` row into `benches/stemmers.rs`
for the first time (it was selected in the research matrix but never actually
benchmarked). `nltk-porter` (the matrix's *other* Rust alternative for
English, already benchmarked) loses to Verbora at every size, but
`porter-stemmer` — a second, independent Rust implementation of the same
original-1980-Porter algorithm — beats it at every size except the smallest.

| | |
|---|---|
| **Capability** | Per-word English Porter stemming (`stem`), batches of 4–1024 cycled words (the shared 64-word list, `"sky"` excluded — see Likely reason) |
| **Competitor** | `porter-stemmer` (samgiles) 0.1.2, `porter_stemmer::stem` |
| **Verbora result** | Medians (`porter_en/verbora/<n>`): 4 **1.63 µs** · 16 **19.27 µs** · 64 **35.52 µs** · 256 **141.29 µs** · 1024 **582.71 µs** |
| **Competitor result** | Same batches (`porter_en/porter-stemmer/<n>`): 4 **3.10 µs** · 16 **3.87 µs** · 64 **19.99 µs** · 256 **87.08 µs** · 1024 **343.99 µs** |
| **Gap** | Verbora **wins** only at the smallest batch (n=4, **1.9×**) and **loses** at every larger size: n=16 **5.0×** (widest), n=64 **1.8×**, n=256 **1.6×**, n=1024 **1.7×**. Real allocator counts over the full 63-word list agree with the direction, not just the timing: Verbora 1,571 allocations / 17,337 bytes vs. `porter-stemmer`'s 493 allocations / 15,264 bytes — `porter-stemmer` allocates roughly a third as often and slightly fewer total bytes, despite its own grapheme-cluster representation sounding heavier than Verbora's `Vec<u16>` one. |
| **Likely reason** | Not fully isolated in this pass. Correctness is not the explanation — `tests/stemmers_correctness.rs` confirms 63/64 exact agreement with Verbora (the one mismatch, `"sky"` → `"ski"`, is `porter-stemmer`'s own isolated bug, excluded from this benchmark's sample so the comparison is over words both sides answer identically). Verbora's `en.rs` has a similar allocation-call-site count (13) to `es.rs`/`it.rs` (12 each), which lose their own Snowball comparisons in entry 9 above for the same reason (whole-buffer `.clone()`/region `.to_vec()` snapshots between algorithm phases, kept for line-by-line reference-reference verifiability) — plausibly the same root cause recurring in the English port, though not confirmed directly against `porter-stemmer`'s own source in this pass the way entry 9 confirmed it against `rust-stemmers`. |
| **Profiling evidence** | Real timing run: `cargo bench -p competitive-rust --bench stemmers -- porter_en` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/stemmers-porter_en-*.json`. Real memory run: `cargo run --release -p competitive-rust --example memory_report`, counts in `benchmarks/competitive/results/memory-report.json`'s `stemmers` section (`stemmers/en/stem_all/{verbora,porter_stemmer}_63`). Correctness verified in `benchmarks/competitive/rust-competitors/tests/stemmers_correctness.rs`'s `porter_stemmer_samgiles_agrees_on_benchmarked_words_except_sky` and `sky_is_the_correct_answer_per_verbora_and_nltk_porter`, run before either number above was trusted. |
| **Optimization opportunity** | Same candidate entry 9 already flags for the losing Snowball ports: `en.rs`'s snapshot/region-copy pattern is a real, plausible target for slice-based, allocation-free region tracking, but touches parity-critical code and was not attempted or profiled directly against `porter-stemmer`'s own implementation in this pass — flagged for a future, dedicated profiling pass rather than guessed at. |

**Update, later pass — the `ends_with` fast-path (entry 34) reaches `en.rs`
too, and the one win it flipped.** `en.rs` calls `ends_with` directly (13
call sites, e.g. the `sses`/`ies`/`eed`/`ing`/`ed`/`ll` suffix checks in
`step1ab`/`step5`), so the same fast-path fix measured for the Snowball
languages applies here as well. Re-measured
(`cargo bench -p competitive-rust --bench stemmers -- porter_en`, full
default Criterion settings — 100 samples, not the reduced settings used for
a quick check earlier in this pass, confirmed reproducible by two
independent full-precision runs agreeing within ~2% at n=4): Verbora medians
(`porter_en/verbora/<n>`) 4 **1.53 µs** · 16 **7.15 µs** · 64 **32.54 µs** ·
256 **138.33 µs** · 1024 **522.48 µs** — each roughly 5–10% faster than the
pre-fix numbers above, consistent with entry 34's general 5–18% claim.
`porter-stemmer` itself is unchanged Verbora-independent third-party code,
but its own measured numbers this pass (4 **881.33 ns** · 16 **3.76 µs** ·
64 **20.11 µs** · 256 **77.32 µs** · 1024 **310.13 µs**) are substantially
faster than the ones recorded above — machine/toolchain variance between
measurement sessions, not a real change in `porter-stemmer`'s own code, but
real enough that it **flips this entry's one win**: Verbora now **loses at
every size**, including n=4 (1.73×, versus the 1.9× *win* recorded above),
narrowing to 1.62×–1.90× rather than the previous 1.6×–5.0× spread. A clean
sweep is a different, more consistent story than a 4-1 split, even though
Verbora's own code is measurably faster than before — see
`site/benchmarks/competitive.md`'s English stemmers section for the
currently-published numbers.

## 25. Indonesian Sastrawi stemming — Verbora vs. `sastrawi` (iDevoid, Rust)

Found while wiring the matrix's `sastrawi` row into `benches/stemmers.rs` for
the first time — this crate was selected (genuine shared PHP-Sastrawi
lineage) but never actually benchmarked, and the matrix's own "correctness
vs. Verbora unverified" caveat had never been resolved. The widest one-sided
margin of the three new stemmer competitors wired in alongside this entry.

| | |
|---|---|
| **Capability** | Per-word Indonesian Sastrawi/Nazief–Adriani stemming (`stem`), batches of 4–1024 cycled words (13 of the shared 16-word list — 3 words with documented `sastrawi` gaps excluded, see Likely reason) |
| **Competitor** | `sastrawi` (iDevoid/rust-sastrawi) 0.1.1, `Stemmer::stem_word` |
| **Verbora result** | Medians (`stemmer_id/verbora/<n>`): 4 **3.53 µs** · 16 **54.63 µs** · 64 **243.09 µs** · 256 **954.89 µs** · 1024 **3.97 ms** |
| **Competitor result** | Same batches (`stemmer_id/sastrawi/<n>`): 4 **969.23 ns** · 16 **8.59 µs** · 64 **35.87 µs** · 256 **152.38 µs** · 1024 **583.38 µs** |
| **Gap** | Verbora **loses at every size**, by **3.6×–6.8×** (n=4: 3.6×, n=16: 6.4×, n=64: 6.8×, n=256: 6.3×, n=1024: 6.8×) — the largest, most consistent margin among this round's three new competitors. Memory is more nuanced than the timing gap alone suggests: over the same 13-word list, `sastrawi` allocates **fewer times** than Verbora (464 vs. 525) but **more total bytes** (39,676 vs. 12,168) — a few-larger-allocations shape against Verbora's many-smaller-allocations one, so "fewer allocations" alone does not explain a 6× time win. |
| **Likely reason** | Not fully isolated in this pass. Two real, narrow algorithmic gaps were found and excluded from the benchmarked domain rather than conflated with a speed difference: `sastrawi` has no hyphenated-reduplication/compound-plural handling at all, and only a single (not iterated-up-to-3×) prefix-stripping pass (`tests/stemmers_correctness.rs`'s `sastrawi_agrees_with_verbora_except_three_documented_gaps`) — neither explains the *timing* gap on the 13 words both sides agree on, since those words never exercise either gap. Verbora's `StemmerId::stem` (`crates/verbora-stemmers/src/id.rs`) does real, `Vec<u16>`-based, reference-parity-preserving work per call — `units`/`text` UTF-16 round trips, a `State` struct with three `Vec<Removal>`/`Vec<u16>` fields rebuilt per call, and a dictionary lookup (`indonesian_dict::SORTED.binary_search`) after nearly every rule application — while `sastrawi`'s `stem_word` (`sastrawi-0.1.1/src/stemmer.rs`) operates directly on `&mut String`/`&str` slices with `regex::Regex` matching (`Affixation`'s ten precompiled regexes, built once at `Stemmer::new()` and reused, not per-call) rather than a hand-rolled rule-by-rule state machine — not confirmed via a call-site allocation count the way entry 9's Snowball languages were, so this is a plausible, source-read explanation rather than a fully quantified one. |
| **Profiling evidence** | Real timing run: `cargo bench -p competitive-rust --bench stemmers -- stemmer_id` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/stemmers-stemmer_id-*.json`. Real memory run: `cargo run --release -p competitive-rust --example memory_report`, counts in `benchmarks/competitive/results/memory-report.json`'s `stemmers` section (`stemmers/id/stem_all/{verbora,sastrawi}_13`, plus `sastrawi`'s own one-time `stemmers/id/construct/{sastrawi_dictionary,sastrawi_stemmer}` rows: 29,947 allocations/~4.5 MB and 17,464 allocations/~16.5 MB respectively — real but paid once, never inside the per-word comparison above). Correctness (including the two documented gaps and the dictionary-size lineage check) verified in `tests/stemmers_correctness.rs`'s `sastrawi_shares_verboras_dictionary_size` and `sastrawi_agrees_with_verbora_except_three_documented_gaps`, run before any number above was trusted. |
| **Optimization opportunity** | None flagged with confidence: without a call-site allocation-density comparison (entry 9's own technique) actually run against `id.rs`, attributing the gap to any single structural cause would be guessing, which this file's own standard (entry 4) explicitly declines to do. A dedicated profiling pass applying entry 9's exact method — counting `.clone()`/`Vec::new()`/`.to_vec()` sites in `id.rs`'s `State`/rule-application path and comparing against `sastrawi`'s precompiled-regex approach — is the natural next step, noted here as the concrete follow-up this entry motivates. |

## 26. Levenshtein distance — Verbora vs. `stringmetrics`, `triple_accel`, and `editdistancek` (Rust)

`docs/COMPETITIVE_BENCHMARKS.md` §1.8 selected all three (matrix `Yes`/
`Selected cases`) but none was ever wired into `Cargo.toml` or benchmarked
until this audit round — a real, undocumented gap alongside `eddie` (see
entry 27's own preamble) and the memory dimension (entries 28–29), all
closed together this round. Byte-identical to Verbora's own `Levenshtein`
correctness (`tests/distance_correctness.rs`, three `#[test]`s covering all
three crates against Verbora on the full ASCII corpus) before any timing
number below was trusted.

**Update, later pass — a real fix, not just measurement.** `levenshtein`'s
unit-cost dispatch (`crates/verbora-distance/src/levenshtein.rs`) now runs
Myers' (1999) bit-vector algorithm whenever every cost is the default 1.0 and
the shorter operand is 8–64 units — a *safe*-Rust algorithmic answer to
`triple_accel`'s SIMD, since matching literal SIMD intrinsics would require
`unsafe`, which this workspace's `unsafe_code = "deny"` policy rules out by
default (see `plain_levenshtein`'s own doc comment for the full reasoning,
including why 8 is the lower bound — `n=4` measured as a wash-to-slight-
regression, `HashMap` setup for a 4-entry `Peq` costing more than the DP
cells it replaces). Verified exhaustively against the pre-existing scalar DP
(`plain_rows`) across randomized ASCII and Cyrillic pairs spanning every
boundary the implementation has (lengths 0, 1, 2, 5, 30, 63, 64, 65, 100,
200) before any timing number below was trusted —
`tests::bit_vector_agrees_with_plain_rows_on_random_pairs` and
`tests::bit_vector_agrees_on_utf16_input` in that same file.

Re-measured (`cargo bench -p competitive-rust --bench distance --
levenshtein/verbora`, same machine, same corpus): 4 **~77 ns** · 16
**~277 ns** · 64 **~1.24 µs** · 256 **~209 µs** · 1024 **~3.68 ms**. Against
`triple_accel` (4 **66.1 ns** · 16 **232.2 ns** · 64 **1.74 µs** · 256
**38.17 µs** · 1024 **538.74 µs**): the crossover moved from n=16 to **past
n=64** — Verbora now *wins* at n=64 (**~1.4×**), where it previously lost by
**~1.7×**, and the remaining gap at n=4/16 narrowed to within ~10-20% rather
than the old ~1.5-2× regression trend continuing upward. n=256 and n=1024
are unaffected (fast path caps at 64 units, unchanged `plain_rows` scalar
DP) — the widening loss there is real and still open. Against
`stringmetrics` the picture is more mixed: `stringmetrics`'s single-`u32`-row
design (see original "Likely reason" below, still accurate) is cheaper
still at every size the fast path doesn't reach parity at, so the *shape* of
that particular gap is largely unchanged.

Original findings, still accurate for what they describe (Verbora's
*previous* scalar-only implementation, and `stringmetrics`/`editdistancek`'s
own unaffected numbers):

| | |
|---|---|
| **Capability** | Levenshtein edit distance, unit costs, ASCII-only shared corpus (`benches/data/distance-pairs.json`) |
| **Competitor** | `stringmetrics` 2.2.2 `levenshtein()`; `triple_accel` 0.4.0 `levenshtein()` (byte-level SIMD); `editdistancek` 1.0.2 `edit_distance()` (byte-level banded) |
| **Verbora result (pre-fix, scalar-only)** | Medians (`levenshtein/verbora/<n>`): 4 **40.7 ns** · 16 **526.1 ns** · 64 **10.46 µs** · 256 **195.60 µs** · 1024 **3.09 ms** |
| **Competitor result** | `stringmetrics`: 4 **26.4 ns** · 16 **173.7 ns** · 64 **3.10 µs** · 256 **61.11 µs** · 1024 **991.68 µs**. `triple_accel`: 4 **66.1 ns** · 16 **232.2 ns** · 64 **1.74 µs** · 256 **38.17 µs** · 1024 **538.74 µs**. `editdistancek`: 4 **51.5 ns** · 16 **407.2 ns** · 64 **5.29 µs** · 256 **75.94 µs** · 1024 **1.12 ms** |
| **Gap (pre-fix)** | `stringmetrics`: a clean, **one-sided loss at every size**, **1.5×** (n=4) widening to **3.0×–3.4×** (n≥16), never crossing back — still current. `triple_accel` and `editdistancek`: a genuine **crossover, not a one-sided loss** — Verbora *wins* at n=4 (**1.6×** and **1.3×** respectively) where their SIMD/banded setup cost dominates, then loses from n=16 up, widening with size to **5.7×** (`triple_accel`) and **2.8×** (`editdistancek`) at n=1024 — superseded for `triple_accel` by the crossover-at-64 result above; `editdistancek` not re-measured this pass. |
| **Likely reason** | Source-confirmed for all three. `stringmetrics::levenshtein` (`stringmetrics-2.2.2/src/algorithms/lev_impl/implementation.rs`) wraps `try_levenshtein_iter`, which keeps exactly ONE `u32`-typed row (`work_vec: Vec<u32> = (1..=b_len).collect()`) and hard-codes unit cost — no arbitrary-cost API to support, so no dispatch/UTF-16 round trip and half the cell width of the two `f64` rows Verbora ran at the time (see entry 28's memory finding for the same root cause quantified in bytes). `triple_accel::levenshtein` (`triple_accel-0.4.0/src/levenshtein.rs`) is genuinely SIMD-accelerated (AVX2/SSE4.1 bit-packed comparison, confirmed by reading `src/jewel.rs`'s lane-width dispatch) — real per-call setup cost that only pays off once the string is long enough to fill a SIMD register, exactly the observed crossover at n=16. `editdistancek::edit_distance` (`editdistancek-1.0.2/src/lib.rs`) wraps `edit_distance_bounded(s, t, max(s.len(), t.len()))`, a Myers-style banded/diagonal algorithm over `isize` buffers — fewer branches and a tighter inner loop than Verbora's generically-costed `f64` sweep, but the same asymptotic shape, hence a milder, steadily-widening gap rather than `triple_accel`'s sharper crossover. |
| **Profiling evidence** | Read `stringmetrics-2.2.2/src/algorithms/lev_impl/implementation.rs`, `triple_accel-0.4.0/src/levenshtein.rs`+`src/jewel.rs`, and `editdistancek-1.0.2/src/lib.rs` directly. Real run: `cargo bench -p competitive-rust --bench distance -- levenshtein/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/distance-levenshtein-*.json`. Correctness: `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`'s `levenshtein_agrees_across_every_new_competitor_on_ascii_pairs`. |
| **Optimization opportunity** | ~~A fixed-unit-cost fast path... A bit-parallel or banded rewrite of `levenshtein` itself...~~ **Done — see both updates above and below.** |

**Update, second pass — the multi-word block extension, closing the
remaining n=256/1024 gap.** The single-word update above explicitly left
this open ("not attempted this pass"). `plain_levenshtein` now dispatches to
a new `bit_vector_distance_blocks` function for unit-cost calls where the
shorter operand exceeds 64 units, generalising Myers' algorithm across
multiple 64-bit words following Hyyrö's 2003 block formulation — the same
paper the single-word path already cites. Derived by reading
`rapidfuzz-0.5.0`'s own `hyrroe2003_block` directly (not re-derived from
memory, given how easy bit-parallel block-carry propagation is to get
subtly wrong) and deliberately **omitting** `rapidfuzz`'s additional
Ukkonen-band early-exit layer on top of the same core: that optimisation
exists to skip blocks once a distance *threshold* rules them out, and
Verbora's `levenshtein` always wants the exact distance, never a
bounded/thresholded one, so there is nothing to band around. Still
zero `unsafe`.

Correctness before any number below was trusted: an exhaustive
differential test against the trusted scalar `plain_rows` DP at every
block-boundary length (65, 127–129, 191–193, 255–257, 500, 1000, and up to
5000 for the scanned operand), a direct cross-check against the
already-proven single-word `bit_vector_distance` at every length where both
functions' domains overlap (8..=64 — the two independently-shaped
implementations must agree with each other, not just each independently
with `plain_rows`), and UTF-16/astral-character coverage. Then an
independent adversarial-audit agent, with no visibility into the
implementation's own design reasoning, added 13 further hand-constructed
adversarial tests (exact-vs-partial last block boundaries, degenerate
single/two-character alphabets, disjoint alphabets, huge scanned operands
against boundary-length patterns, a distance-exactly-`k*64` self-checking
construction) and validated its own tests' teeth by deliberately introducing
4 distinct plausible bugs (wrong carry-out bit for non-last blocks, `mv`
substituted for `pv` in the `d0` formula, an off-by-one `last_bit`, and
`hp_carry_in`/`hn_carry_in` not reset per row) one at a time, confirming
each broke multiple tests before reverting it. No real bug was found; no
source changes resulted from the audit, only additional tests.

Re-measured (`cargo bench -p competitive-rust --bench distance -- levenshtein/`,
full Criterion defaults, same machine): Verbora medians
(`levenshtein/verbora/<n>`) 4 **39.9 ns** · 16 **226.0 ns** · 64
**953.7 ns** · 256 **11.37 µs** · 1024 **147.99 µs** — n=4/16/64 essentially
unchanged from the single-word-fast-path numbers above (as expected, this
extension only touches lengths past 64), but **n=256 is 16.4× faster than
the pre-multi-block number (186.17 µs → 11.37 µs) and n=1024 is 20.9×
faster (3.09 ms → 147.99 µs)**. Against every competitor in this entry, at
every size (`stringmetrics`/`triple_accel`/`editdistancek` medians:
4 **25.5/66.2/51.5 ns** · 16 **162.7/223.0/398.2 ns** · 64
**3.00/1.68/5.26 µs** · 256 **55.52/37.38/79.74 µs** · 1024
**921.10/517.29/1.13 ms µs/ms**): Verbora now **wins against all three at
n=64, n=256 and n=1024** (3.2×–7.6× faster, widest against `editdistancek`),
loses narrowly to `stringmetrics` at n=4 (1.6×) and n=16 (1.4×), and is
essentially tied with `triple_accel` at n=4/16 (within 1.7×, first faster
then a wash). Against `rapidfuzz`/`strsim` specifically (this entry's
sibling competitors, `site/benchmarks/competitive.md`'s Distance section):
the `rapidfuzz` gap narrowed from 90.8× to **4.3×** at 1024 characters
(rapidfuzz's own block algorithm still wins outright, plus its Ukkonen-band
layer this pass deliberately didn't replicate), and Verbora now **beats
`strsim` outright** at every size from 64 characters up, where it
previously lost by up to 4.9×.

**This closes the gap this entry originally opened, not just narrows it
further.** Every one-sided loss this entry and the single-word update
above documented at n≥64 is now a Verbora win against `stringmetrics`,
`triple_accel`, and `editdistancek` alike; the sole remaining loss in the
whole Levenshtein comparison is against `rapidfuzz` specifically, and it is
now a bounded, single-digit-multiple gap rather than the open-ended,
widening-with-length one this entry started from.

**Update, third pass (2026-08) — the last remaining losses flip: Verbora
now beats all three of this entry's competitors at every size.** The
second pass above left `stringmetrics` ahead at n=4/16 (1.4×–1.6×) and
`triple_accel` roughly tied there. This round replaced the `HashMap`-based
`Peq` pattern-match table inside both bit-vector kernels (single-word and
multi-word block) with flat/packed `BitPeq` tables — a flat `[u64; 256]`
table plus a packed distinct-rows matrix when the operands are pure `u8`,
`FxHashMap` only for genuine `u16` input — and widened the single-word
gate from 8..=64 to **1..=64**: the `HashMap`-setup cost that the
single-word update above documented as the reason for the lower bound of 8
is exactly the cost the flat table removes. Zero behavior changes,
verified by the existing differential tests against `plain_rows` plus an
independent adversarial audit with mutation testing. Re-measured medians,
Verbora vs. `stringmetrics`/`triple_accel`/`editdistancek`: n=4 **14.8**
vs. 26.0/66.3/44.0 ns · n=16 **41.9** vs. 169.7/229.1/373.2 ns · n=64
**164.7 ns** vs. 2.91/1.63/4.79 µs · n=256 **2.09** vs. 55.70/36.67/69.80
µs · n=1024 **29.07** vs. 915.47/497.94 µs/1.06 ms. **Verbora wins every
cell**: **1.8×–31.5×** faster than `stringmetrics`, **4.5×–17.5×** faster
than `triple_accel`, **3.0×–36.5×** faster than `editdistancek` — and, for
completeness across the whole Levenshtein comparison, it also now beats
`strsim` at every size (20.6/271.3 ns and 2.86/41.83/625.21 µs at
4/16/64/256/1024) and `rapidfuzz` itself at every size (see entry 1's own
closed-and-reversed update). Nothing in the plain-Levenshtein time
comparison remained a loss.

**Update, closing pass (2026-08) — the sweep stands as a result about the
kernels, but not as a current figure.** The Rust-native contract
(`docs/design/distance-contract.md`) removed the cost argument from
`levenshtein`: unit costs are the absence of an argument, weighted costs are a
separate function over a validated `LevenshteinCosts`, and the per-call
comparison of a caller's costs against `1.0` that used to select between the
bit-vector kernels and the scalar rows is gone from the dispatch. The kernels
themselves — the flat `BitPeq` tables, the 1..=64 single-word gate, the
multi-word block path — are the same code, and the text unit moved from the
UTF-16 code unit to the Unicode scalar, which is the identity on the ASCII
corpus every row here uses. So the *conclusion* (bit-parallel beats scalar DP,
by margins that widen with length) is unaffected; what is not carried across
is the per-cell timing, because a dispatch was removed from in front of it and
that removal is untimed.

⚠ **The medians above are retired pending re-measurement.** Competitor figures
are unaffected — no competitor version moved.

## 27. Restricted Damerau-Levenshtein, Hamming, and fuzzy substring search — Verbora vs. `triple_accel` (Rust, SIMD)

`triple_accel` (matrix `Selected cases` across all four of its rows —
Levenshtein is entry 26 above) was selected by `docs/COMPETITIVE_BENCHMARKS.md`
§1.8 but never wired into `Cargo.toml`/benchmarked until this round, same gap
as entry 26. Its remaining three capabilities share one root cause (genuine
SIMD acceleration — AVX2/SSE4.1, confirmed by reading `triple_accel-0.4.0/src/jewel.rs`
directly) and are reported together here rather than as three separate
entries. Correctness: `tests/distance_correctness.rs`'s
`restricted_damerau_levenshtein_agrees_with_triple_accel_rdamerau`,
`hamming_agrees_across_every_new_competitor_on_ascii_pairs`, and
`fuzzy_substring_search_runs_without_panicking_on_both_sides` (the last is a
completion/bounds check, not an output-equivalence one — see that test's own
doc comment and `manifests/competitors.json`'s `triple_accel` entry for why
`levenshtein_search`'s bounded-`k`-iterator shape is not directly comparable
to Verbora's single-best-match `SearchResult`).

| | |
|---|---|
| **Capability** | (a) Damerau-Levenshtein, restricted/OSA (`rdamerau`); (b) Hamming (`hamming`); (c) fuzzy substring search (`levenshtein_search`) |
| **Competitor** | `triple_accel` 0.4.0 |
| **Verbora result** | (a) 4 **82.7 ns** · 16 **585.7 ns** · 64 **11.13 µs** · 256 **204.01 µs** · 1024 **3.31 ms**. (b) 4 **6.7 ns** · 16 **9.1 ns** · 64 **19.2 ns** · 256 **69.2 ns** · 1024 **266.6 ns**. (c) 4 **119.0 ns** · 16 **816.4 ns** · 64 **13.05 µs** · 256 **219.20 µs** · 1024 **6.00 ms** |
| **Competitor result** | (a) 4 **78.9 ns** · 16 **272.1 ns** · 64 **2.26 µs** · 256 **52.69 µs** · 1024 **773.40 µs**. (b) 4 **3.2 ns** · 16 **4.4 ns** · 64 **3.2 ns** · 256 **5.0 ns** · 1024 **13.0 ns**. (c) 4 **122.1 ns** · 16 **452.4 ns** · 64 **2.60 µs** · 256 **69.14 µs** · 1024 **1.06 ms** |
| **Gap** | (a) one-sided loss at every size, **1.1×** (n=4) growing to **4.9×** (n=64) then settling **~4.3×** (n=1024). (b) one-sided loss at every size, and the WIDEST margin in this whole module: **2.1×** (n=4) growing to **20.6×** (n=1024) — Hamming is close to embarrassingly SIMD-parallel, so this is the cleanest possible showcase of the technique. (c) effectively tied at n=4 (**0.97×**, Verbora marginally faster), then a one-sided loss from n=16 up, reaching **5.7×** at n=1024. |
| **Likely reason** | All three read directly from `triple_accel-0.4.0/src/levenshtein.rs` and `src/hamming.rs`, both built on `src/jewel.rs`'s SIMD abstraction layer (`Avx1x32x8`/`Sse8x16x8` etc., dispatched by CPU-feature detection at call time). `rdamerau` wraps `levenshtein_simd_k_with_opts` with `RDAMERAU_COSTS`, the same bit-packed-word DP engine `levenshtein` itself uses (entry 26) with one extra cost table — Verbora's OSA path when this was measured was `restricted_rows`, a classical scalar three-row DP, by contrast. `hamming` is the starkest case: SIMD hamming distance is just a vectorized XOR-and-popcount over the whole string with no data-dependent branching at all, versus the scalar per-position comparison loop (`diffs_generic`) Verbora ran at the time — there is close to no serial work SIMD cannot parallelize here, which is exactly why this is the widest gap in the module. `levenshtein_search` dispatches to `levenshtein_search_simd`, whose internal state and iteration are both bounded by the search width (default `k = ceil(needle.len()/2)`), a structurally different, typically much smaller working set than the full backtrace matrix Verbora's search ran through at the time — see entry 29 for the same comparison on the memory axis, where the gap is far larger than on the time axis. |
| **Profiling evidence** | Read `triple_accel-0.4.0/src/levenshtein.rs`, `src/hamming.rs`, and `src/jewel.rs` directly. Real run: `cargo bench -p competitive-rust --bench distance -- damerau_levenshtein_restricted_osa/ hamming/ fuzzy_substring_search/` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/distance-*-triple_accel-*.json`. |
| **Optimization opportunity** | ~~A SIMD/bit-parallel rewrite is the same future-phase opportunity entry 1 and entry 26 already flag for plain Levenshtein — restricted Damerau-Levenshtein and Hamming would benefit from the identical technique (Hamming especially: a vectorized XOR-popcount loop is a small, self-contained, low-risk change relative to the shared multi-function DP core the Levenshtein family depends on for parity). Not attempted or measured this pass; flagged for a future, separate, dedicated phase, consistent with this file's own discipline for structural (not tuning) changes.~~ **Done for (a) restricted Damerau-Levenshtein — see the update below. (b) Hamming and (c) fuzzy substring search were not touched and remain open, real losses.** |

**Update, this round (2026-08) — (a) closed and reversed; (b) and (c)
unchanged, still real losses.** Restricted Damerau-Levenshtein now has its
own bit-parallel kernels: Hyyrö's 2003 transposition extension of Myers'
algorithm, in both a single-word and a multi-word block form (the same
two-tier pattern entry 26's plain-Levenshtein kernels follow), gated to
unit costs — non-default costs still take the scalar three-row DP, and the
implementation is still zero-`unsafe`. Parity verified by differential
tests against the scalar path plus the same independent adversarial audit
with mutation testing that covered this round's other distance kernels.
Re-measured, Verbora vs. `triple_accel`
(`damerau_levenshtein_restricted_osa/<n>` medians): n=4 **15.9 vs.
76.3 ns** · n=16 **46.2 vs. 260.4 ns** · n=64 **179.3 ns vs. 2.08 µs** ·
n=256 **2.39 vs. 50.35 µs** · n=1024 **32.47 vs. 737.06 µs** — row (a)
flips from a one-sided every-size loss to a **one-sided every-size win,
4.8× at n=4 widening to 22.7× at n=1024**. The same kernels also put
Verbora ahead of `rapidfuzz`'s OSA implementation at every size (**~1.9×**
at n=4: 15.9 vs. 30.2 ns; **1.4×** at n=1024: 32.47 vs. 45.06 µs) and
`strsim`'s `osa_distance` at every size (67.5 ns → 2.43 ms across the same
sweep). **(b) Hamming and (c) fuzzy substring search remain exactly the
losses the table above records** — Hamming is still the widest SIMD-vs-
scalar margin in the module, and fuzzy search is still bound to the full
backtrace matrix (entry 29). Both stay open.

**Update, later pass (2026-08) — API rename only; every figure above
stood at the time.** The restricted rule is now reached through its own
function, `osa` (with `osa_search` and `par_osa_batch` alongside), instead of
the `Options.restricted` flag that entry 29's final update records as removed.
The bit-parallel kernels this update measured are the same code, reached
under a different name — unlike the unrestricted-Damerau path, nothing
about the algorithm changed. `tests/distance_correctness.rs` was
updated for the rename and still passes (13 tests, `--release`).

**Update, final pass (2026-08) — all three of this entry's rows are retired,
including the two the rename update left standing.** `verbora-distance` was
rewritten to the Rust-native contract (`docs/design/distance-contract.md`),
and each row loses its premise for a different reason:

- **(a) OSA.** The bit-parallel kernels are unchanged, but the per-call cost
  comparison that used to route to them is gone — unit costs are now the
  absence of an argument (`osa(source, target) -> usize`) rather than a value
  compared against `1.0` on entry. The kernels either side of that dispatch
  are the same code and the removal is untimed, so no figure may be carried
  across it.
- **(b) Hamming.** `hamming` now returns `Option<usize>`; `INCOMPARABLE`,
  `hamming_checked` and the `ignore_case` parameter are deleted. The ASCII
  tiered kernel this entry's gap is about survives intact, but the general
  path is now one fused `chars()` walk deciding comparability and counting
  differences together — allocation-free on every input, where the measured
  code made five passes and two allocations. The wrapper is not free either:
  `Option<usize>` returns in two registers rather than one, which the contract
  flags as costing the SWAR tier a tail call, unmeasured. The 20.6× headline
  is against a function that no longer exists in that shape.
- **(c) Fuzzy substring search.** Search now reports a `&str` borrowed from
  the target plus its byte range, with no lossy-decode allocation, and
  unit-cost plain Levenshtein search runs a bit-parallel per-column kernel
  instead of the full cost-plus-parent matrix this entry's "Likely reason"
  contrasts with `triple_accel`. The structural comparison that explained the
  gap is the part that changed.

⚠ **Every Verbora figure in this entry is retired pending re-measurement.**
`triple_accel`'s own numbers are unaffected — the crate is still pinned at
0.4.0 and was not touched. Whether (b) and (c) remain losses at all is now an
open question rather than a recorded result; nothing here should be quoted
until the re-benchmark lands.

## 28. Levenshtein memory: `f64` two-row DP vs. `u32` one-row — Verbora vs. `stringmetrics` (Rust)

The memory-dimension counterpart to entry 26's Levenshtein TIME finding for
`stringmetrics` specifically, measured with the real allocator-counting
infrastructure (`benchmarks/competitive/rust-competitors/src/memory.rs`'s
`measure`, installed as `competitive-rust`'s `#[global_allocator]` — see
entry 17 for why this is a live allocator trace, not RSS or an estimate).

| | |
|---|---|
| **Capability** | Levenshtein edit distance, memory dimension |
| **Competitor** | `stringmetrics` 2.2.2, `stringmetrics::levenshtein` |
| **Verbora result** | Bytes allocated (2 allocations at every size, fully freed within the call): 80 B (n=4) · 272 B (16) · 1,040 B (64) · 4,112 B (256) · 16,400 B (1024) |
| **Competitor result** | Bytes allocated (1 allocation at every size, fully freed within the call): 16 B (4) · 64 B (16) · 256 B (64) · 1,024 B (256) · 4,096 B (1024) |
| **Gap** | A consistent **~4× more bytes for Verbora**: exactly 4.0× at n=1024/256/64, drifting slightly higher at the smallest sizes (4.25× at n=16, 5.0× at n=4 — small-allocation-overhead noise at those sizes, not a shape difference). Allocation count is 2 vs. 1 at every size. |
| **Likely reason** | Source-confirmed at the time of measurement. Verbora's `plain_rows` (`crates/verbora-distance/src/levenshtein.rs`) keeps TWO `f64`-per-cell rows (`prev`, `cur`), each sized `target.len()+1`, 8 bytes/cell — required because the caller could supply arbitrary `f64` insertion/deletion/substitution costs, and every call was routed through that path regardless of whether it did. `stringmetrics::levenshtein` (`stringmetrics-2.2.2/src/algorithms/lev_impl/implementation.rs`) wraps `try_levenshtein_iter`, which keeps exactly ONE `u32`-per-cell row (`work_vec: Vec<u32> = (1..=b_len).collect()`), 4 bytes/cell, because it hard-codes unit cost with no arbitrary-cost API at all. 2 rows × 8 bytes vs. 1 row × 4 bytes is exactly the observed 4×. |
| **Profiling evidence** | Read `crates/verbora-distance/src/levenshtein.rs`'s `plain_rows` and `stringmetrics-2.2.2/src/algorithms/lev_impl/implementation.rs`'s `try_levenshtein_iter` directly. Real run: `cargo run --release -p competitive-rust --example distance_memory` in `benchmarks/competitive/`, raw counts in `benchmarks/competitive/results/distance-memory.json` (`levenshtein` group, `verbora`/`stringmetrics` rows; one unmeasured warm-up call plus one measured call per implementation per size, per that example's own doc comment). |
| **Optimization opportunity** | ~~None flagged as a like-for-like fix: the extra row and the wider cell width are both required by the arbitrary-`f64`-cost contract, which `stringmetrics` does not offer at all (unit costs only) — matching its footprint would mean dropping weighted-cost support, a real capability regression, not a tuning win. A `u32`/fixed-point fast path for the common unit-cost case (falling back to the `f64` two-row path only when costs are non-default) is a real, narrower idea — not attempted or measured this pass.~~ **Overtaken by the split recorded in the update below: the common unit-cost case no longer visits `plain_rows` at all, so there is no fast path left to add to it.** |

**Update, later pass (2026-08) — the two-`f64`-row path is no longer what a
unit-cost `levenshtein` call runs, so this entry's byte table describes code
the benchmarked function no longer reaches.** Two changes compound. First,
entry 26's bit-parallel kernels took over unit-cost Levenshtein: a one- or
multi-word Myers state plus a packed pattern-match table, not two `f64` rows
sized to the target. Second, the Rust-native contract
(`docs/design/distance-contract.md`) removed the cost argument from
`levenshtein` altogether — weighted costs are now a separate function,
`levenshtein_weighted`, over a validated `LevenshteinCosts`, and `plain_rows`
is that function's kernel and only that function's kernel. The `stringmetrics`
side is unchanged, and the comparison it anchors is still meaningful; what
changed is which Verbora function it should be measured against.

⚠ **The (a) byte table above is retired pending re-measurement** — the
allocator probe has not been re-run against either the bit-parallel unit path
or `levenshtein_weighted`, and no figure for either exists. Whether the ~4×
memory gap survives, inverts, or disappears is open: structurally the
unit-cost path no longer allocates a pair of target-sized `f64` rows at all,
but "structurally smaller" is not a measurement and is not published as one.

## 29. Full-matrix memory: unrestricted Damerau-Levenshtein and fuzzy substring search — Verbora vs. Rust competitors

Two matrix rows sharing one root cause, reported together:
`crates/verbora-distance/src/levenshtein.rs`'s own module doc comment
already names `full_matrix` (a dense `(n+1)×(m+1)` `cost: Vec<f64>` +
`parent: Vec<(u32,u32)>`) as the shared code path for BOTH unrestricted
Damerau-Levenshtein (arbitrary-earlier-row transposition) and
search/fuzzy-substring-search (the backtrace needs the full parent matrix
to recover a match offset+substring). The fuzzy-substring-search half is
this round's mandatory new-competitor finding (`triple_accel`, entry 27's
own TIME-dimension companion); the unrestricted-Damerau half against
`strsim`/`rapidfuzz` (pre-existing competitors, wired in during the original
Fase 6 pass, memory dimension newly measured this round) is included as
directly-relevant supporting evidence for the identical code path, not a
second, unrelated claim.

| | |
|---|---|
| **Capability** | (a) Damerau-Levenshtein, unrestricted/true; (b) fuzzy substring search (`levenshtein_search`) — both routed through `full_matrix` |
| **Competitor** | (a) `strsim` 0.11.1 `damerau_levenshtein()`, `rapidfuzz` 0.5.0 `distance::damerau_levenshtein::distance` (byte-for-byte identical results to each other); (b) `triple_accel` 0.4.0 `levenshtein_search` |
| **Verbora result** | (a) bytes allocated (2 allocations, fully freed within the call): 400 (n=4) / 4,624 (16) / 67,600 (64) / 1,056,784 (256) / 16,810,000 (1024). (b) bytes allocated (3 allocations, fully freed within the call): 400 / 4,628 / 67,638 / 1,056,920 / 16,810,581 |
| **Competitor result** | (a) `strsim`=`rapidfuzz`, byte-for-byte identical: 144 / 432 / 1,584 / 6,192 / 24,624 bytes. (b) `triple_accel`: 64 / 64 / 64 / 14,912 / 59,456 bytes (2–31 allocations, non-monotonic — see Likely reason) |
| **Gap** | (a) grows from **2.8×** at n=4 to **682.7× at n=1024** — quadratic vs. near-linear growth. (b) grows from **6.2×** at n=4 to **282.7× at n=1024**, non-monotonic in between (**1057×** at n=64, where `triple_accel`'s cost is flat at 64 B across n=4..64, then jumps once the needle length crosses a SIMD-block-width threshold between n=64 and n=256). Both are large, real, one-sided Verbora losses on the memory dimension — far larger in magnitude than either capability's own TIME-dimension gap (entry 27's (a) tops out at 4.3×, its (c) at 5.7×). |
| **Likely reason** | Source-confirmed. Verbora's `full_matrix` allocates `cost: Vec<f64>` AND `parent: Vec<(u32,u32)>`, each `(n+1)×(m+1)` — 16 bytes/cell combined; at n=m=1024, `1025×1025×16 = 16,810,000`, matching (a) exactly. `strsim`'s and `rapidfuzz`'s public `damerau_levenshtein`/`distance::damerau_levenshtein::distance` both route through the SAME Zhao–Sahni-style linear-space algorithm — confirmed by reading both crates' source directly, which share identical `fr`/`r1`/`r` row-variable names (unsurprising: `strsim-rs` is maintained under the same `rapidfuzz` GitHub org as `rapidfuzz-rs`): three O(m)-sized rows, not an O(nm) matrix, hence near-linear growth. `triple_accel::levenshtein_search` dispatches to a SIMD bit-parallel search (`levenshtein_search_simd`) whose internal state scales with the BOUNDED search width (`k`, default `ceil(needle.len()/2)`) rounded to SIMD lane-block granularity, not with haystack length — a structurally smaller-by-construction working set than a full matrix, at the cost of the bounded-match/different-problem-shape trade-off entry 27 and `manifests/competitors.json`'s `triple_accel` entry already document for the TIME dimension. |
| **Profiling evidence** | Read `crates/verbora-distance/src/levenshtein.rs` (`Matrix`/`full_matrix`), `strsim-0.11.1/src/lib.rs` (`damerau_levenshtein_impl`), `rapidfuzz-0.5.0/src/distance/damerau_levenshtein.rs` (identical `fr`/`r1`/`r` linear-space rows), and `triple_accel-0.4.0/src/levenshtein.rs` (`levenshtein_search`/`levenshtein_search_simd`) directly. Real run: `cargo run --release -p competitive-rust --example distance_memory`, raw counts in `benchmarks/competitive/results/distance-memory.json` (`damerau_levenshtein_unrestricted` and `fuzzy_substring_search` groups). |
| **Optimization opportunity** | ~~Same structural note as entry 1's TIME finding for plain Levenshtein: a linear-space algorithm exists for unrestricted-Damerau-Levenshtein DISTANCE-ONLY queries (Zhao–Sahni, exactly what `strsim`/`rapidfuzz` already ship) — but Verbora's `full_matrix` is shared with SEARCH mode, which genuinely needs the full parent matrix to backtrace a substring+offset. Splitting "distance-only" (could go linear-space) from "search" (needs the matrix) into two code paths is a real, evidence-backed opportunity but touches parity-critical, shared code (`damerau_levenshtein`, `levenshtein_search`, `damerau_levenshtein_search` all currently share `full_matrix`) — flagged for a future, separate, dedicated phase, consistent with entry 1's own disposition, not implemented in Fase 6.~~ **Attempted and landed in part this round: the distance-only/search split shipped, but the "could go linear-space via Zhao–Sahni" premise turned out to be structurally wrong for Verbora's pinned recurrence — see the update below for the split, the newly-recorded structural finding, and the re-measured time numbers.** **Fully landed in a later pass: the pinned recurrence itself was replaced by the canonical Lowrance–Wagner one, and the distance path now *is* Zhao–Sahni linear-space — see the final update below.** |

**Update, this round (2026-08) — the (a) distance-only split shipped, and
it surfaced a structural fact about Verbora's unrestricted-Damerau
recurrence this file had not recorded before.** Distance-only unrestricted
Damerau-Levenshtein no longer routes through `full_matrix`: a new kernel
keeps two DP rows plus a per-symbol row-snapshot arena (cells stored as
`u16` when the combined operand length fits, widening to `u32` beyond),
and evaluates **exactly** the same pinned recurrence the full-matrix code
did — verified by differential tests against the original full-matrix
implementation plus an independent adversarial audit with mutation
testing; zero behavior changes. Search mode still genuinely needs, and
keeps, the full parent matrix for backtracing, so this entry's (b) finding
is untouched. The new kernel's allocation counts were **not** re-measured
with the allocator probe this round, so the (a) byte table above is
retained as the accurate record of the full-matrix era rather than
replaced with an unmeasured claim; structurally, the dense `(n+1)×(m+1)`
`f64` cost + `(u32,u32)` parent matrices are simply no longer allocated on
the distance path.

**The structural finding — superseded, see the final update below; retained
as the record of what this round concluded.** ~~The struck-through opportunity
above assumed
Zhao–Sahni linear-space (what `strsim`/`rapidfuzz` ship) became available
once distance was split from search. It did not — for a real, measurable
semantic reason: **Verbora's pinned unrestricted-Damerau recurrence is
deliberately not textbook Damerau-Levenshtein.** It diverges from the
textbook (Zhao–Sahni) results `strsim`/`rapidfuzz` compute on a measurable
fraction of inputs — e.g. `damerau_levenshtein("bb", "abbb")` is **1**
under Verbora's recurrence but **2** under textbook DL; the recurrence is
not even symmetric in its arguments; this round's design experiment found
divergence on roughly **11% of small random pairs**. That divergence is
inherited parity behavior, not a bug to fix — and it structurally forbids
adopting Zhao–Sahni's linear-space algorithm or the affix trimming both
competitor crates lean on, which is why the residual time gap against
`strsim` below is structural, not a tuning residue. One consequence for
this file's sibling document: `docs/COMPETITIVE_BENCHMARKS.md`'s matrix
marks `strsim`'s and `rapidfuzz`'s `damerau_levenshtein` as
`Yes`-equivalent, and this entry's own competitor row calls the three
implementations' results identical — that equivalence holds **on the
verified benchmark corpus** (the correctness tests cover the benchmarked
domain) but is **not universal**: on inputs shaped like `"bb"→"abbb"`
above, the libraries genuinely disagree, by design.~~

**Time dimension, re-measured** (previous competitive runs had Verbora up
to ~3.46× behind at the largest size): n=4 Verbora **81.2 ns** vs.
rapidfuzz 71.7 ns / strsim 55.7 ns — a real loss to both (**1.13×** and
**1.46×**); n=16 Verbora **508.2 ns wins against both** (536.4/539.4 ns);
n=64 Verbora **7.73 µs** beats rapidfuzz (8.20 µs) and is a wash with
strsim (7.69 µs); n=256 Verbora **135.52 µs** edges rapidfuzz (135.82 µs,
within noise) and trails strsim by **~1.08×** (126.05 µs); n=1024 Verbora
**2.25 ms** trails rapidfuzz by **~4%** (2.17 ms, within run noise) and
strsim by **~1.11×** (2.03 ms). A dramatically narrowed, honestly mixed
result — wins at n=16 across the board and against rapidfuzz at n=64/256,
real remaining losses at n=4 (both) and at the largest sizes (strsim
~1.1×, rapidfuzz ~4%), with the strsim residue explained by the structural
finding above rather than left as an open tuning question.

**Update, later pass (2026-08) — the byte path re-tiered on a measured
decomposition; every remaining time loss but one flips, and the one left
is quantified as a structural floor.** Profiling the snapshot kernel's
per-cell work decomposed the residual `strsim` gap precisely: the per-row
snapshot `memcpy` into the arena measured as effectively free (removing
it did not move the median), while the per-cell transposition-candidate
block — the last-occurrence table load plus the arena read sitting on the
critical path — accounted for essentially the whole gap; and at tiny
sizes the scratch-table zeroing cost more than the entire DP. The unit
kernel was accordingly split into a measured three-tier dispatcher
(`damerau_unit_dispatch` in `crates/verbora-distance/src/levenshtein.rs`):
**`damerau_unit_small`**, a table-free fixed stack matrix for operands of
at most 8 bytes each (the last-occurrence lookup is a plain `rposition`
scan over the few source bytes seen so far — no tables, no heap);
**`damerau_unit_mid`** for operands ≤ 128 bytes — the snapshot recurrence
with row 1/column 1 peeled so the steady-state loop drops its guards, the
column loop split into a no-candidate-possible phase and a candidate
phase, one packed `[u32; 256]` symbol table (half the zeroing of the
two-array form), and the `cur[c-1]` operand carried in a register; and
**`damerau_unit_large`** beyond, identical except the
`cur[c-1]`/`prev[c]` operands stay memory-carried — once the snapshot
arena outgrows L1, the register chain otherwise puts the arena load's
miss latency onto the loop-carried dependency. The UTF-16 path is
unchanged. All three tiers evaluate the pinned recurrence exactly:
differentially pinned against the `full_matrix` oracle, plus cross-tier
agreement tests on the tiers' shared domains — 91 tests green. As with
the previous update, the allocator probe was not re-run, so the (a) byte
table above remains the record of the full-matrix era (structurally, the
small tier now allocates nothing at all; the mid/large tiers two rows
plus the arena). Re-measured (full-default Criterion, quiet machine,
medians), Verbora vs. rapidfuzz / strsim: n=4 **29.5** vs. 70.6 / 55.5 ns
(wins both, 2.39× and 1.88×); n=16 **396.6** vs. 530.7 / 441.6 ns (wins
both); n=64 **4.65** vs. 7.51 / 6.95 µs (wins both, 1.49× over strsim);
n=256 **116.22** vs. 133.43 / 120.04 µs (wins both); n=1024 **1.906** vs.
2.119 ms (**1.11× faster** than rapidfuzz) / **1.866 ms** (**1.021×
behind** strsim). The n=1024 strsim residue is a statistical tie at the
measured structural floor, not a tuning gap: a probe evaluating nothing
but the bare loop-carried min-chain of the pinned recurrence costs
1.86–1.88 ms at this size on this machine, and the recurrence's measured
divergence from textbook DL — **38.6% of random small-alphabet pairs**
this pass (the earlier ~11% figure sampled unconstrained random pairs; a
small alphabet provokes the divergence far more often) — is exactly what
forbids the Zhao–Sahni candidate pruning strsim uses to get under that
floor. Final time ledger for this entry's companion dimension: Verbora
wins 4 of 5 sizes against strsim, 5 of 5 against rapidfuzz, and the last
~2% n=1024 deficit is recorded as a structural loss.

**Update, final pass (2026-08) — the premise of both updates above is gone:
the pinned recurrence was replaced, and Zhao–Sahni plus affix trimming are
now what Verbora runs.** `damerau_levenshtein` is now the canonical
unrestricted Damerau-Levenshtein (Lowrance–Wagner): symmetric, a true metric,
and byte-for-byte in agreement with `strsim::damerau_levenshtein` and
`rapidfuzz`'s `damerau_levenshtein` — `"bb"`→`"abbb"` is **2**, not 1, and
the ~11%/38.6% divergence figures recorded above describe a recurrence that
no longer exists. The restricted rule it used to share an entry point with is
now its own function, `osa` (with `osa_search` and `par_osa_batch`
alongside), selected by name rather than by the removed `Options.restricted`
flag. Three structural consequences for this entry: **(1)** the "structurally
forbids adopting Zhao–Sahni's linear-space algorithm" finding is void —
unit-cost distance now runs `damerau_zhao_sahni`, three rolling rows plus one
saved-cell row and a last-occurrence map, with a table-free stack matrix
(`damerau_unit_small`) for byte operands of at most 8 units; the
`damerau_unit_mid`/`damerau_unit_large` tiers of the previous update are
gone. **(2)** The "forbids the affix trimming both competitor crates lean on"
claim is void with it: the trim is valid precisely *because* the new
recurrence is a true metric, and all three unit-cost distance paths (plain,
OSA, unrestricted Damerau) now strip the common prefix and suffix first.
**(3)** Weighted costs are the only unrestricted-Damerau path still routed
through `full_matrix`, so this entry's (a) memory finding now applies to
weighted calls only; (b), fuzzy substring search, was untouched by this pass —
search mode needed the parent matrix then, and both Damerau search variants
still do. (Plain Levenshtein search no longer does; see the closing update
below.)
**No number in this entry has been re-measured against the new kernel** —
neither the (a) byte table nor any of the time ledgers above — so every
figure here predates the change and is retained as history rather than
replaced with an unmeasured claim. ⚠ **Every unrestricted-Damerau figure in
this entry is therefore retired and pending re-measurement**: the (a) byte
table (measured against `full_matrix`, then never re-measured across two
subsequent kernel generations), the "time dimension, re-measured" ledger,
and the three-tier ledger of the later pass, along with the 1.86–1.88 ms
"structural floor" probe and the 4-of-5/5-of-5 win counts derived from
them. None of them should be quoted until the re-benchmark lands. The
same retirement is mirrored in `docs/COMPETITIVE_BENCHMARKS.md` §1.8's
fifth-pass update, and `site/benchmarks/competitive.md` publishes no
unrestricted-Damerau timing table at all in the meantime rather than a
stale one. Entry 27's (a)/(b)/(c) figures and this entry's own (b) figures
were **not** affected by that particular change — the OSA and search paths
were not part of it. (A later change does reach them; see the closing update
below.)

**Correctness, by contrast, was re-verified and is current.** The canonical
recurrence turned a `Partial` competitor verdict back into a full `Yes`:
`docs/COMPETITIVE_BENCHMARKS.md` §1.8's `strsim` and `rapidfuzz`
unrestricted-Damerau rows are restored, on **202,000 randomized pairs with
zero divergences** across alphabets of 2/3/4/26 letters, lengths 1..=25
including unequal and empty operands, plus 2,000 binary-alphabet mutation
chains of up to eight edits. Verbora is asserted against both crates in the
same sweep, so the agreement is three-way, and the sweep additionally
asserts symmetry on every pair. It is committed rather than one-off —
`unrestricted_damerau_agrees_with_both_competitors_over_a_wide_randomized_sweep`
in `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`,
which asserts its own pair count so the figure cited here cannot drift away
from the sweep that produced it. Run it with
`cargo test --release --test distance_correctness` from
`benchmarks/competitive/rust-competitors` (that suite needs `--release`:
`eddie` 0.4.2 aborts any debug build of the binary, item 3 of the upstream
findings below).

**Update, closing pass (2026-08) — the (b) half is retired too, so nothing in
this entry is current.** The Rust-native contract
(`docs/design/distance-contract.md`) changed what search allocates and how it
answers. Three things moved under (b):

1. **Unit-cost plain Levenshtein search left the full matrix.**
   `levenshtein_search` now runs a bit-parallel per-column kernel that
   recovers cell costs from Myers' `Pv`/`Mv` words; the dense
   `(n+1)×(m+1)` cost-plus-parent matrix is reached only by the weighted
   searches and by both Damerau search variants, whose transposition parents
   depend on state cell costs alone cannot recover. `levenshtein_search` is
   the exact function (b) measures, so its 16,810,581-byte figure at n=1024
   describes a path that call no longer takes.
2. **The result borrows instead of allocating.** `SearchResult` holds a `&str`
   sliced out of the target plus its byte start, so the owned `String` the
   measured code built per call — via a lossy decode that could and did
   fabricate text absent from the target — is gone. The allocation is now the
   caller's to opt into with `.to_owned()`.
3. **Positions are bytes, counts are scalars.** The match is reported as a
   byte `Range<usize>` derived from the borrowed text, so the two cannot
   disagree; the old owned-substring-plus-unit-offset pair could.

⚠ **Every figure in this entry is now retired pending re-measurement** — the
(a) byte table (already retired above) and the (b) byte table alike.
`triple_accel`'s and `strsim`/`rapidfuzz`'s own numbers are unaffected. The
structural claim that a bounded-`k` SIMD search keeps a smaller working set
than a full matrix is still true of the *weighted* and *Damerau* searches;
whether it is still true of `levenshtein_search` is exactly the open question,
and it is not answered by inspection.

## 30. Hiragana↔katakana width conversion — Verbora vs. `unicode-jp` (Rust)

Found while wiring `normalize_ja`'s first two Rust competitors into
`benches/normalizers.rs` — this module had zero Rust competitors benchmarked
before this pass. `unicode-jp` 0.4.0 covers only 2 of Verbora's 17
`normalize_ja` conversions (`hira2kata`/`kata2hira` vs. Verbora's
`hiragana_to_katakana`/`katakana_to_hiragana`); the domain below is narrowed
to pure hiragana/pure katakana input (the Iroha pangram, いろは歌, repeated)
with neither halfwidth characters nor a small tsu before an n-row consonant,
byte-exact verified in `tests/normalizers_correctness.rs` before any timing
number here is trusted — see that file's own divergence tests for the two
real cases this narrowing excludes.

| | |
|---|---|
| **Capability** | `hiragana_to_katakana`/`katakana_to_hiragana`, repeated Iroha-pangram input, `repeats` 4–1024 |
| **Competitor** | `unicode-jp` (gemmarx) 0.4.0, `kana::hira2kata`/`kana::kata2hira` |
| **Verbora result** | Medians, `hiragana_to_katakana` (`ja_hiragana_to_katakana/verbora/<repeats>`): 4 **1.486 µs** · 16 **5.810 µs** · 64 **23.10 µs** · 256 **91.74 µs** · 1024 **377.8 µs**. `katakana_to_hiragana` (`ja_katakana_to_hiragana/verbora/<repeats>`): 4 **1.519 µs** · 16 **5.836 µs** · 64 **23.13 µs** · 256 **93.58 µs** · 1024 **361.6 µs** |
| **Competitor result** | Same input, `kana::hira2kata` (`ja_hiragana_to_katakana/unicode-jp/<repeats>`): 4 **398.9 ns** · 16 **1.348 µs** · 64 **5.102 µs** · 256 **20.24 µs** · 1024 **78.99 µs**. `kana::kata2hira` (`ja_katakana_to_hiragana/unicode-jp/<repeats>`): 4 **392.8 ns** · 16 **1.335 µs** · 64 **4.906 µs** · 256 **20.12 µs** · 1024 **78.75 µs** |
| **Gap** | Verbora **loses at every size, both directions**: `hiragana_to_katakana` **3.7×–4.8×** slower (3.7× at repeats=4, widening to 4.5×–4.8× from repeats=64 up); `katakana_to_hiragana` **3.9×–4.7×** slower, the same shape. The gap widens with size and then flattens, not a size-independent constant — see Likely reason. |
| **Likely reason** | Read directly: `hiragana_to_katakana` (`crates/verbora-normalizers/src/ja.rs`) is three sequential stages — `katakana_hf` (fold halfwidth katakana to fullwidth), `fix_fullwidth_kana` (compose standalone voiced marks / small-tsu-before-n-row), then a final `map_chars` pass that shifts U+3041..=U+3096 by `0x60` and remaps `ゝ`/`ゞ`. On pure-hiragana input the first two stages find nothing to change — `Table::translate`'s bitmap-gated `may_start` check (`crates/verbora-normalizers/src/table.rs`) rejects every hiragana character immediately, so neither stage allocates — but each is still a **full `char_indices` walk of the whole string**, so this benchmarked domain pays for three complete character-by-character passes when only the third one does real work. `kana::hira2kata`/`kata2hira` (`unicode-jp-0.4.0/src/kana.rs`) is a single `src.chars().map(shift).collect()` pass — no gate, no lookahead, just a range check and an arithmetic add per character. Three walks (two of them pure overhead on this input) against one is directly consistent with a gap in the 3×–5× range, and `Table::translate`'s own per-character cost (the `may_start` bitmap-plus-linear-scan test, run even to conclude "no") is itself more expensive than `hira2kata`'s bare range comparison, which plausibly accounts for the observed ratio sitting above a flat 3×. |
| **Profiling evidence** | Read `crates/verbora-normalizers/src/ja.rs`'s `hiragana_to_katakana`/`katakana_to_hiragana` and `crates/verbora-normalizers/src/table.rs`'s `Table::translate`/`may_start` directly, alongside `unicode-jp-0.4.0/src/kana.rs`'s `shift_code`/`hira2kata`/`kata2hira`. Real benchmark run: `cargo bench -p competitive-rust --bench normalizers -- ja_hiragana_to_katakana\|ja_katakana_to_hiragana` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/normalizers-ja_*-*.json`. Correctness (byte-exact agreement on the benchmarked domain, plus the two excluded divergences) verified in `benchmarks/competitive/rust-competitors/tests/normalizers_correctness.rs`'s `hira2kata_agrees_with_verbora_on_pure_hiragana`/`kata2hira_agrees_with_verbora_on_pure_katakana`/`hira2kata_diverges_on_small_tsu_and_halfwidth`. Memory: `cargo run --release -p competitive-rust --example memory_report`, `normalizers` section of `benchmarks/competitive/results/memory-report.json` — interestingly, allocation *count* is not the story here (Verbora: 1 allocation per call vs. `unicode-jp`: 3, both totaling the identical 36,096 bytes at repeats=256), consistent with the "extra full walks, not extra allocations" explanation above rather than an allocation-churn one. |
| **Optimization opportunity** | ~~A cheap, narrow one: check up front whether the input is pure...~~ **Implemented — see the update below.** |

**Update, later pass — implemented, real but modest.** `hiragana_to_katakana`/`katakana_to_hiragana` (`crates/verbora-normalizers/src/ja.rs`) now run a single combined pre-check, `needs_hf_or_voiced_mark_fix`, before the first two stages: one pass over `s` testing each character against *both* `HF_KATAKANA`'s and `FIX_FULLWIDTH_KANA`'s `may_start` gate (a new `pub(crate)` method on `Table`, `crates/verbora-normalizers/src/table.rs`) in the same loop. When neither gate ever admits a character, both `katakana_hf` and `fix_fullwidth_kana` are structurally guaranteed to be no-ops — `may_start` is exact for "could this character start some key" (verified by this crate's own `tables_are_sorted_and_within_the_bmp` test, which checks the gate admits exactly each table's real key-starting characters and nothing else) — so skipping both calls entirely and going straight to the final shift stage cannot change the result. This is still one `O(n)` scan, not a way to make the whole function sub-linear; the win is replacing two separate `Table::translate` walks (each with its own gate-check loop, one of them allocating a scratch buffer up front) with one combined gate-only pass.

Verified before any timing number was trusted: 16 existing tests plus two new ones (`hiragana_to_katakana_pre_check_matches_always_running_both_stages`, hand-picked cases spanning pure hiragana, halfwidth katakana, standalone voiced marks, small-tsu-before-n-row, and mixes of all four; `hiragana_to_katakana_pre_check_matches_always_running_both_stages_randomized`, 500 randomized rounds) comparing the real function against a preserved always-run-both-stages reference, all passing.

Re-measured (`cargo bench -p competitive-rust --bench normalizers -- ja_hiragana_to_katakana/verbora`, controlled A/B against the pre-change code via Criterion `--save-baseline`/`--baseline`): a real, statistically significant **4.1% speedup at repeats=1024** (`p < 0.05`); at repeats=64/256 the difference was within noise (`p > 0.05`, "no change detected") — plausibly because the fixed per-call overhead of the extra combined pre-scan only pays for itself once the string is long enough for two full `Table::translate` walks' overhead to dominate. This narrows, but does not close, the 3.7×–4.8× gap against `unicode-jp` above — `unicode-jp`'s single unconditional pass remains structurally cheaper for this domain than even a single combined gate-check plus one real shift pass; a genuinely single-pass fused implementation (folding all three stages into one character walk, rather than pre-checking then still running one `Table::translate` conditionally) would be the next step, not attempted this round.

**Update, text-shaping migration (2026-08) — both functions are deleted, and
the capability left `verbora-normalizers` entirely.**
`docs/design/text-shaping-contract.md` §3.2's "Cut: the Japanese normalizers"
records the reasoning: hiragana ↔ katakana conversion is not a Unicode
normalization at all, it is a *transliteration*, and it belongs to
`verbora-transliterators` — which today ships only kana → romaji
(`transliterate_ja`). §3.4 removes `ja::converters`' seventeen functions,
`ja/tables.rs` and the shared `table.rs` whose `Table::translate`/`may_start`
machinery this entry's whole explanation is about. `benches/normalizers.rs`'
`ja_hiragana_to_katakana` and `ja_katakana_to_hiragana` groups were deleted
with them, and the `unicode-jp` dependency went too: timing `hira2kata`/
`kata2hira` alone would measure nothing about Verbora.

⚠ **Every figure in this entry is retired, and not pending re-measurement.**
The Verbora medians in both directions, the 3.7×–4.8× and 3.9×–4.7× gaps, the
allocation counts (1 vs. 3, identical 36,096 bytes at repeats=256), and the
4.1%-at-repeats=1024 speedup the `needs_hf_or_voiced_mark_fix` pre-check
earned all describe deleted code. The follow-up this entry proposed — a
genuinely single-pass fused implementation of all three stages — is
**withdrawn**, not deferred: there are no stages left to fuse. `unicode-jp`
0.4.0's own figures are unaffected but have nothing to compare against, and
the crate is no longer pinned.

If the capability returns in `verbora-transliterators`, this is a new
comparison to select a competitor for from scratch, not a resumption of this
one — the entry is kept so that selection starts from the evidence rather than
from a blank page.

## 31. Halfwidth-to-fullwidth katakana folding — Verbora vs. `kana-converter` (Rust)

Found in the same pass as entry 30, wiring `normalize_ja`'s narrowest
competitor. `kana-converter` 0.1.2's `to_double_byte(_, KanaOnly)` is a
single-purpose halfwidth-katakana-to-fullwidth converter; narrowed to
halfwidth katakana using only the standard voiced/semi-voiced pairs both
sides recognize (no punctuation/space, which `kana-converter` folds and
Verbora's `katakana_hf` does not; no orphan mark or `ｦ`/`ﾜ` + dakuten, both
real divergences — see `tests/normalizers_correctness.rs`).

| | |
|---|---|
| **Capability** | `katakana_hf` (halfwidth katakana, valid dakuten/handakuten pairs only), `repeats` 4–1024 |
| **Competitor** | `kana-converter` 0.1.2, `kana_converter::to_double_byte(_, ConvertMode::KanaOnly)` |
| **Verbora result** | Medians (`ja_katakana_halfwidth_to_fullwidth/verbora/<repeats>`): 4 **1.434 µs** · 16 **5.907 µs** · 64 **23.75 µs** · 256 **93.53 µs** · 1024 **367.1 µs** |
| **Competitor result** | Same input (`ja_katakana_halfwidth_to_fullwidth/kana-converter/<repeats>`): 4 **1.201 µs** · 16 **4.224 µs** · 64 **16.48 µs** · 256 **63.57 µs** · 1024 **251.8 µs** |
| **Gap** | Verbora **loses at every size**, by **1.19×–1.47×** — smaller and flatter than entry 30's 3×–5× gap (this is a single-pass-vs-single-pass comparison on both sides, unlike entry 30's three-vs-one), widening slightly from 1.19× at repeats=4 to ~1.45×–1.47× from repeats=64 up. |
| **Likely reason** | Unlike entry 30, `katakana_hf` alone is already a single `Table::translate` pass (`crates/verbora-normalizers/src/ja.rs`'s `katakana_hf` is a direct call to `tables::HF_KATAKANA.translate`, no pipeline) — this is a genuine single-pass-vs-single-pass comparison, and Verbora still loses, so the cause is per-character constant factor, not extra walks. `kana_converter::to_double_byte` (`kana-converter-0.1.2/src/lib.rs`) looks up each character in one flat `HashMap<char, char>` (`HW_FW_KANA_MAP`) and, for a voiced/semi-voiced pair, adds a fixed `+1`/`+2` to the mapped codepoint — an O(1) hash lookup plus arithmetic per character. Verbora's `Table::translate` instead runs its exact bitmap-plus-linear-scan `may_start` gate, then a `binary_search_by_key` over a 26-entry two-character key table for the lookahead check on every character (`crates/verbora-normalizers/src/table.rs`) — more instructions per character even though it is a single pass, which is directly consistent with a real but modest (not several-fold) gap. Real memory counts point the same direction from the opposite side: `kana-converter` allocates **19 times** (2.4× the bytes: 100,992 vs. Verbora's 42,240 at repeats=256, with 3 of those later freed again — real churn from its `Vec<char>` push-based accumulation) against Verbora's single pre-sized buffer, yet still wins on wall-clock time — confirming the win is about cheaper per-character dispatch, not about avoiding allocation. |
| **Profiling evidence** | Read `crates/verbora-normalizers/src/ja.rs`'s `katakana_hf` and `crates/verbora-normalizers/src/table.rs`'s `Table::translate`/`may_start`/`lookup2` directly, alongside `kana-converter-0.1.2/src/lib.rs`'s `to_double_byte`/`convert_kana_char`/`check_voiced`. Real benchmark run: `cargo bench -p competitive-rust --bench normalizers -- ja_katakana_halfwidth_to_fullwidth` in `benchmarks/competitive/`, raw Criterion output in `benchmarks/competitive/results/raw/normalizers-ja_katakana_halfwidth_to_fullwidth-*.json`. Correctness (agreement on the benchmarked domain, plus the three excluded divergences: punctuation/space, orphan marks, `ｦ`/`ﾜ` + dakuten) verified in `benchmarks/competitive/rust-competitors/tests/normalizers_correctness.rs`'s `kana_converter_kana_only_agrees_with_katakana_hf_on_valid_dakuten_input`/`kana_converter_diverges_on_punctuation_and_orphan_dakuten`. Memory: `cargo run --release -p competitive-rust --example memory_report`, `normalizers` section of `benchmarks/competitive/results/memory-report.json`. |
| **Optimization opportunity** | None flagged with confidence beyond the general observation that `Table`'s exact-gate machinery (designed for `normalize_ja`'s composite, many-table pipeline, where rejecting the vast majority of tables cheaply matters far more than shaving cycles off the one table that actually matches) is not obviously the fastest shape for a single always-relevant table used in isolation, the way this benchmark uses `katakana_hf`. A `HashMap`- or match-arm-based fast path specifically for single-table standalone use would be a genuine design change to `Table`'s public contract, not a tuning pass — out of scope for a benchmarking pass per this project's own separation between measuring and modifying. |

**Update, text-shaping migration (2026-08) — unlike entry 30, the capability
survives; the function measuring it does not.** `katakana_hf` and the whole
`Table` machinery are deleted by `docs/design/text-shaping-contract.md` §3.4,
but halfwidth-to-fullwidth katakana folding is genuinely still available:
NFKC's compatibility decomposition maps halfwidth katakana to its fullwidth
form and decomposes the halfwidth voiced sound mark `U+FF9E` to combining
`U+3099`, which canonical composition then recombines, so `nfkc("ｶﾞ") == "ガ"`
— the same user-visible result. `benches/normalizers.rs`' group was therefore
**re-pointed rather than deleted**, and renamed
`ja_katakana_halfwidth_to_fullwidth` → `nfkc_halfwidth_katakana`. The
competitor is unchanged (`kana-converter` 0.1.2,
`to_double_byte(_, ConvertMode::KanaOnly)`), the narrowed domain is unchanged,
and agreement is now proved *per character* over the whole of
`U+FF66..=U+FF9D` in `tests/normalizers_correctness.rs`, plus both fixtures —
stronger evidence than the previous pass's, not weaker.

⚠ **Every Verbora figure in this entry is retired pending re-measurement, and
the replacement will not be a continuation of it.** The 1.434/5.907/23.75/
93.53/367.1 µs medians and the 1.19×–1.47× loss measured a single
purpose-built `Table::translate` pass over one 26-entry two-character key
table. `nfkc` is general UAX #15 Normalization Form KC over arbitrary Unicode:
it must consult compatibility decompositions, canonical combining classes and
composition exclusions for every scalar. This entry's central claim — "a
genuine single-pass-vs-single-pass comparison, so the cause is per-character
constant factor, not extra walks" — is exactly what stops being true, and
`benches/normalizers.rs`' `bench_nfkc_halfwidth_katakana` states the resulting
comparability limit in full: fair *for this workload*, never generalisable
into "NFKC costs X". The memory observation beside it (kana-converter
allocating 19 times / 2.4× the bytes and still winning) is retired for the
same reason: Verbora's single pre-sized buffer was `Table::translate`'s.

`kana-converter`'s own figures are unaffected; the crate is still pinned at
0.1.2. `docs/design/text-shaping-contract.md` §7 item 3 carries the general
form of the open question — whether the normalization wrapper's quick-check
`Cow::Borrowed` guarantee is worth its existence over
`unicode-normalization`'s own iterators.

## 32. Trie prefix enumeration — Verbora vs. `fast_radix_trie` (Rust, path-compressed radix)

`docs/COMPETITIVE_BENCHMARKS.md` §1.18 added `fast_radix_trie` (created
2025-10-30, actively updated) this round specifically to test whether
`verbora-trie`'s uniformly-sized, safe-Rust arena representation loses to a
path-compressed, `unsafe`-internally radix tree on any real operation. It
does, on exactly one: bulk prefix enumeration. `build`/`contains_hit`/
`contains_miss` all go the other way — see the Gap row below for all four,
not just the loss. Set-equivalence (order-blind, via `BTreeSet`) verified
once in `tests/trie_correctness.rs` before any timing number here was
trusted, per this workbench's `CORRECTNESS BEFORE PERFORMANCE` rule.

| | |
|---|---|
| **Capability** | `build` (random / prefix-heavy word sets), `contains` (hit / miss), `predictive_search` (prefix enumeration, "1char" and "all" prefixes) |
| **Competitor** | `fast_radix_trie` 1.2.0, `radix_trie::GenericRadixMap` |
| **Verbora result** | `build`: random **1.567 ms** · prefix_heavy **2.217 ms**. `contains_hit`/`contains_miss` ("words"): **1.136 ms** / **1.249 ms**. `predictive_search`: "1char" **1.145 ms** · "all" **1.453 ms** |
| **Competitor result** | `build`: random **2.415 ms** · prefix_heavy **2.995 ms**. `contains_hit`/`contains_miss`: **1.340 ms** / **1.335 ms**. `predictive_search`: "1char" **696.8 µs** · "all" **663.4 µs** |
| **Gap** | Verbora **wins build** (1.54× random, 1.35× prefix_heavy) and **wins contains** (1.18× hit, 1.07× miss — both narrow). Verbora **loses predictive_search**, and by the widest margin of the four: **1.64×** slower at "1char" (single-character prefix, the largest result set), **2.19×** slower at "all" (empty-prefix, full-corpus enumeration) — the one operation `fast_radix_trie`'s own design is specifically shaped for. |
| **Likely reason** | Read `fast_radix_trie`'s own README and source directly: nodes are dynamically sized (a node's label bytes and child pointers packed into one `unsafe`-addressed allocation sized exactly for its child count, not Verbora's fixed per-node shape) and **path-compressed** — a run of single-child nodes collapses to one node with a multi-byte label, so walking from the root to a leaf during prefix enumeration touches far fewer nodes than Verbora's one-node-per-UTF-16-unit arena walk (`crates/verbora-trie/src/lib.rs`). `contains`/`build` do not benefit from compression the same way (a single point lookup or insert pays the same asymptotic node-hop count either way, and Verbora's flat arena avoids the allocation-per-node cost `fast_radix_trie`'s dynamically-sized nodes pay during `build` instead), which is exactly why the win/loss splits along this line rather than being one-sided. `qp-trie` (already pinned, a nybble-branching radix map with no path compression) sits *between* the two on `predictive_search` (121.5 µs–130.1 µs, faster than both) — evidence that compression alone is not the whole story, but it is the specific, disclosed reason `fast_radix_trie` in particular wins where it does. |
| **Profiling evidence** | Read `fast_radix_trie`'s own README (path compression, `unsafe`-internally, `miri`-tested) and `crates/verbora-trie/src/lib.rs` directly. Real run: `cargo bench -p competitive-rust --bench trie -- build/ contains_hit/ contains_miss/ predictive_search/` in `benchmarks/competitive/`, raw Criterion `estimates.json` under this workspace's shared target directory's `criterion/{build,contains_hit,contains_miss,predictive_search}/fast_radix_trie/`. Correctness: `tests/trie_correctness.rs`'s `predictive_search_full_enumeration_matches_as_a_set`/`predictive_search_one_char_prefixes_match_as_a_set`. |
| **Optimization opportunity** | ~~Per `docs/research/fase6-benchmark-brief.md`'s own directive to weigh this: a compressed, radix-style *frozen* query representation...~~ **Done, see the update below.** |

**Update, later pass — implemented, not just recommended.** `verbora_trie::Trie::freeze(&self) -> FrozenTrie` now exists: a safe-Rust (no `unsafe` anywhere, unlike `fast_radix_trie`), path-compressed, read-only representation built once from a `Trie`, collapsing every run of single-child, non-word nodes into one edge whose label is a slice into a shared `Vec<u16>` buffer — exact, not approximate (see the type's own doc comment for why every real stopping point — root, stored word, branch — survives compression untouched). It implements `contains` and `keys_with_prefix`/`iter_keys_with_prefix`/`keys` only; `find_matches_on_path`/`find_prefix`/`find_prefix_lengths` are deliberately not extended to the frozen representation, since neither this crate's own benchmarks nor the competitive audit found a loss there. Correctness verified two ways before any timing number below was trusted: an 80-round randomized fuzzer inside `crates/verbora-trie/src/frozen.rs`'s own test module (comparing `Trie` vs. `FrozenTrie` on every prefix of every generated word plus random misses, across mixed digit/astral/case-folding input), and a fully independent adversarial audit (a second agent, no visibility into the implementation's design reasoning, that hand-traced the riskiest paths, wrote and ran its own fresh test cases — including a surrogate pair split across *two different* compressed edges, the one case explicitly flagged as untested — and validated its own tests had real teeth by deliberately introducing two bugs and confirming both were caught before reverting them). No bug found by either pass.

**Real result: a genuine, honest trade-off, not a clean win.** Re-measured (`cargo bench -p competitive-rust --bench trie`, same machine, same corpus):

| | verbora (arena) | verbora_frozen | fast_radix_trie | Frozen vs. arena | Frozen vs. fast_radix_trie |
|---|--:|--:|--:|--:|--:|
| `predictive_search/all` | 1.439 ms | 965.6 µs | 665.1 µs | **1.49× faster** | still **1.45× slower** |
| `predictive_search/1char` | 1.174 ms | 622.5 µs | 659.0 µs | **1.89× faster** | **1.06× faster** — overtakes |
| `contains_hit` | 1.142 ms | 1.882 ms | 1.253 ms | **1.65× slower** | 1.50× slower |
| `contains_miss` | 1.232 ms | 2.109 ms | 1.267 ms | **1.71× slower** | 1.67× slower |

`FrozenTrie` substantially closes the gap this entry originally reported and **overtakes `fast_radix_trie` on the more realistic query shape** (`1char`, single-letter prefixes — what a real autocomplete issues), but does **not** fully close it on full, empty-prefix enumeration of the whole corpus (`all`) — still a real, disclosed, ~1.45× loss there. `contains` — never this entry's target — genuinely **regresses** against the plain arena (1.65×–1.71×) and, as a direct consequence, now loses to `fast_radix_trie` on `contains` too, where the unfrozen `Trie` used to win (see the original Gap row above). The separate in-crate benchmark (`crates/verbora-trie/benches/trie.rs`'s own `enumeration`/`contains_hit`/`contains_miss`/`freeze` groups) shows the same shape with its own numbers, plus the one-time `freeze()` cost itself (~1.02 ms for this 20,000-word corpus, comparable to the arena's own `build` cost — a real but modest one-time charge, not paid per query).

**Why the trade-off runs this direction.** `FrozenTrie::contains` walks the same number of *branch points* a point lookup would always have crossed regardless of compression (branching is frequent early in real words), but each hop now costs a separate `units` buffer indirection plus a multi-unit slice comparison, instead of the arena's single inline `u16 == u16` check — more work per hop, not fewer hops, for a query that only ever follows one path. Full enumeration is the opposite: it visits *every* edge in the whole structure, so fewer total node-visits (fewer smallvec scans, fewer stack pushes) is a real, structural saving that the extra per-edge cost does not erase, and pays off even harder on `1char` than on `all` because a single-letter subtree has a higher density of long, otherwise-compressible chains relative to its total branch count.

**Recommendation, per this round's own scope discipline.** Use `Trie` (the arena) for point-lookup-heavy workloads — it already beats every trie competitor benchmarked here on `contains`. Call `Trie::freeze()` once, after bulk-loading, for enumeration/autocomplete-heavy workloads — it is now competitive with or ahead of `fast_radix_trie` on the realistic prefix-search shape. Keeping both representations, chosen per call site rather than replacing one with the other, is the shipped design — exactly the "multiple representations are allowed" framing `docs/research/fase6-benchmark-brief.md` itself proposed.

## 33. Trie: `fst` loses both operations it was benchmarked for — Verbora vs. `fst` (Rust, finite-state transducer)

Added alongside entry 32 per `docs/research/fase6-benchmark-brief.md`'s `FST — SPECIALIZED FROZEN
COMPETITOR` directive. Unlike `fast_radix_trie`, `fst::Set` is not a trie at
all — a finite-state transducer built once from sorted input via a streaming
builder, queried through a `Streamer` API, never mutated again. Recorded here
because the result is genuinely informative on its own (a specialized,
frozen, node-sharing representation still loses to Verbora's plain mutable
arena on both operations it was benchmarked for), not because `fst` is a
trie replacement candidate the way `fast_radix_trie` is.

| | |
|---|---|
| **Capability** | `build` (sort+dedup included in the timed closure — see `benches/trie.rs`'s own "Build asymmetry" note), `contains` (hit/miss), `predictive_search` |
| **Competitor** | `fst` 0.4.7, `fst::Set`/`fst::SetBuilder` |
| **Verbora result** | `build`: random **1.567 ms** · prefix_heavy **2.217 ms**. `contains_hit`/`contains_miss`: **1.136 ms** / **1.249 ms**. `predictive_search`: "1char" **1.145 ms** · "all" **1.453 ms** |
| **Competitor result** | `build`: random **7.036 ms** · prefix_heavy **4.639 ms**. `contains_hit`/`contains_miss`: **1.851 ms** / **1.867 ms**. `predictive_search`: "1char" **1.839 ms** · "all" **1.762 ms** |
| **Gap** | Verbora **wins every operation measured**: `build` 4.49× (random) / 2.09× (prefix_heavy) faster; `contains` 1.63× (hit) / 1.50× (miss) faster; `predictive_search` 1.61× ("1char") / 1.21× ("all") faster. The `build` gap is the largest and least uniform of the three — see Likely reason. |
| **Likely reason** | `fst::SetBuilder` requires strictly increasing input (`Err(DuplicateKey)`/`Err(OutOfOrder)` otherwise — confirmed in `fst`'s own `raw::Builder::check_last_key`), so `build_fst` sorts and deduplicates the word list **inside** the timed closure, deliberately (see `benches/trie.rs`'s own doc comment) so this real cost is not hidden by pre-sorting outside the benchmark — this is the dominant term in the `build` loss, not the FST-construction automaton itself. `contains`/`predictive_search` losses are smaller and more uniform: `Streamer`-based iteration decodes each key lazily from the transducer's shared-suffix graph rather than storing it, real per-step overhead against Verbora's flat arena that a minimal-automaton's node-sharing does not recoup at this corpus size. |
| **Profiling evidence** | Read `fst`'s own `raw::Builder::check_last_key`, `raw::Fst` traversal, and `crates/verbora-trie/src/lib.rs` directly. Real run: `cargo bench -p competitive-rust --bench trie -- build/ contains_hit/ contains_miss/ predictive_search/` in `benchmarks/competitive/`. Correctness: `tests/trie_correctness.rs` (order-blind `BTreeSet` comparison, as with every other trie competitor in this file). |
| **Optimization opportunity** | None flagged — `fst`'s value proposition is compact, mmap-able, *frozen* storage (relevant to the workspace's separate Archived Data and Memory Mapping evaluation, not exercised here — see `benches/trie.rs`'s own doc comment on why mmap mode is deliberately not benchmarked), not raw in-memory speed against a plain mutable trie. Losing here is expected and not a Verbora gap to close. |

## 34. Snowball stemmers, per language — Verbora vs. `rust-stemmers` and `snowball_stemmers_rs` (Rust)

⚠ **Every Verbora figure in this entry is retired pending re-measurement.**
Competitor figures are unaffected; no competitor version moved, and the
byte-exact agreement recorded below still holds. The retirement is not about
the `ends_with` fast path this entry's own update describes — it is about what
landed *after* it, and it lands on the diagnosis as much as on the numbers.

This entry's "Likely reason" names a **linear** suffix scan — `for s in
suffixes { ends_with(w, s) }` in `crates/verbora-stemmers/src/units.rs`'
`longest_suffix`/`first_suffix` — as the cause, and its closing paragraph
records that the competitors' real advantage, the official Snowball compiler's
`find_among`/`find_among_b` binary search with common-prefix tracking, "was not
reimplemented here" and "remains the clearest, best-evidenced follow-up".
**That follow-up has since landed.** `longest_suffix` and `first_suffix` no
longer exist. `crates/verbora-stemmers/src/among.rs` implements the same
binary search (table sorted by reversed code-unit sequence,
`common_i`/`common_j` prefix tracking, `substring_i`-style links so one search
can replace a whole guarded else-if chain), and every language in the table
below routes through it, as does English — `de`, `en`, `es`, `fr`, `it`, `nl`,
`no`, `pt`, `ru`, `sv` all import `crate::among`.

So the per-language ratios below measure an algorithm the crate no longer runs,
and the "Likely reason" they rest on has been acted upon rather than merely
recorded. ⚠ **Do not quote the ratios, and do not infer the new ones from the
direction of the change** — a re-run must produce them. Entry 24 (English vs.
`porter-stemmer`) is retired on the same ground. `docs/COMPETITIVE_BENCHMARKS.md`
§1.3 and §7 carry the same retirement from the other direction, and §1.3 also
records the two benchmarked groups that are *not* affected (`stemmer_id`,
`stemmer_ja` — neither reaches `among.rs` or `ends_with`).

Everything below is kept unchanged, because the reasoning is what the change
overturned and it is worth more than the numbers were.

`snowball_stemmers_rs` (SeekStorm, published by the original SymSpell author;
created 2026-03-09, 2 releases at pinning time — a real, disclosed maturity
caveat) added this round as a *second*, independent Snowball-family
competitor alongside the already-pinned `rust-stemmers`, per `docs/research/fase6-benchmark-brief.md`'s
"per-language comparison, no averaging across languages" directive. Byte-exact
agreement confirmed per language in `tests/stemmers_correctness.rs` before any
timing number below was trusted — 7 of 9 languages agree 100% with no
exclusions; `ru` agrees 100% *including* `ёлка` (`snowball_stemmers_rs`'s
`russian.sbl` carries the same ё→е fold Verbora's port does, `rust-stemmers`
does not — a genuine positive finding, stronger than the existing
`rust-stemmers` row); `nl` needs `Algorithm::DutchPorter` specifically, not
the crate's plainly-named `Algorithm::Dutch` (actually Kraaij–Pohlmann, a
different, non-canonical stemmer that disagrees with both Verbora and
`rust-stemmers` on most words — confirmed by reading the crate's own algorithm
list, not assumed from the name).

| Language | vs `rust-stemmers` at n=1024 | vs `snowball_stemmers_rs` at n=1024 | Verdict |
|---|---|---|---|
| de | Verbora **1.09×** faster | Verbora **1.32×** faster | Verbora wins both |
| nl | Verbora **2.93×** faster | Verbora **1.38×** slower | split — wins one, loses the other |
| es | Verbora **5.24×** slower | Verbora **7.08×** slower | Verbora loses both |
| fr | Verbora **2.46×** slower | Verbora **3.02×** slower | Verbora loses both |
| it | Verbora **2.40×** slower | Verbora **2.93×** slower | Verbora loses both |
| no | Verbora **5.46×** slower | Verbora **6.89×** slower | Verbora loses both |
| pt | Verbora **3.88×** slower | Verbora **6.96×** slower | Verbora loses both |
| ru | Verbora **8.04×** slower | Verbora **7.24×** slower | Verbora loses both |
| sv | Verbora **5.44×** slower | Verbora **6.26×** slower | Verbora loses both |

| | |
|---|---|
| **Capability** | Snowball stemming, all 9 shared canonical-Snowball languages (`porter_de`…`porter_sv` groups), sizes 4–1024 words per call |
| **Competitor** | `rust-stemmers` 1.2.0 (Snowball-to-Rust-compiler output); `snowball_stemmers_rs` 1.0.1 (same lineage, independently generated) |
| **Verbora result / Competitor result** | See the per-language table above (n=1024, the largest and most stable size); the ratio is consistent across n=4…1024 for every language, German narrowing slightly at small n, the rest widening slightly. |
| **Gap** | Verbora wins outright only on **German**. **Dutch is a genuine split**: wins vs. `rust-stemmers` (2.93×), loses vs. `snowball_stemmers_rs` (1.38×) — both real, not a wash. The other 7 languages lose to both competitors, most by 2×–8×. |
| **Likely reason** | Source-confirmed, not guessed: Verbora's suffix matching (`crates/verbora-stemmers`'s per-language `units.rs`/local helpers, e.g. `ru.rs`'s `alt_suffix`) is a **linear scan** — `for s in suffixes { ends_with(w, s) }` — a literal port of the reference reference's regex-alternation checks, run repeatedly per word (19 suffixes checked for Spanish, 43 for French, versus German's smallest table at 77 literals total and zero such calls at all — it truncates by direct index instead, which is exactly the one language where Verbora wins). Both `rust-stemmers` and `snowball_stemmers_rs` are compiled by the official Snowball-to-Rust compiler's shared runtime (`find_among`/`find_among_b`), a **binary search** over a sorted table with common-prefix tracking — the original Snowball authors' own performance-engineered design, not something either competitor wrote by hand. A secondary, smaller effect: `snowball_stemmers_rs`'s `SnowballEnv::replace_s` always allocates a fresh oversized `String` on every rule application (even pure deletions), real but outweighed by the algorithmic win above. |
| **Profiling evidence** | Read `crates/verbora-stemmers`'s per-language suffix-matching helpers and the Snowball-compiler-generated `find_among`/`find_among_b` in both competitors' own `snowball_env.rs` directly. Real run: `cargo bench -p competitive-rust --bench stemmers -- porter_de/ porter_es/ porter_fr/ porter_it/ porter_nl/ porter_no/ porter_pt/ porter_ru/ porter_sv/` in `benchmarks/competitive/`, raw Criterion `estimates.json` under `criterion/porter_<lang>/{rust-stemmers,snowball-stemmers-rs,verbora}/<n>/`. Correctness: `tests/stemmers_correctness.rs`, 4 new tests added this round covering all 9 languages plus the `Algorithm::Dutch` vs. `Algorithm::DutchPorter` distinction. |
| **Optimization opportunity** | ~~A `find_among`-style binary-search-over-sorted-suffixes rewrite...~~ **Attempted, partially landed — see the update below for what was tried, what regressed, and what actually shipped.** |

**Update, later pass — a real, measured, but partial fix, with a failed attempt documented rather than hidden.** Two rewrites of the shared suffix-matching helpers (`crates/verbora-stemmers/src/units.rs`) were implemented and benchmarked in a controlled A/B (`cargo bench -p verbora-stemmers`, Criterion `--save-baseline`/`--baseline` against the pre-change build) before either was judged:

1. **`longest_suffix`, rewritten to sort candidates by descending length then early-exit on the first match** — provably correct (a length-descending scan's first hit is, by construction, the longest one), but the `Vec::to_vec()` allocation this needs on every call **regressed** the two languages that use it most: **Spanish +484%, French +266%, Dutch +15% slower**. Reverted immediately; kept here as a documented negative result, not silently dropped.
2. **`longest_suffix`, rewritten again to skip calling `ends_with` for any candidate whose length cannot beat the running best (no allocation, no sorting, just a `usize` comparison first)** — still provably correct and genuinely helped most other languages tried alongside it, but Spanish and French **still regressed** (27%/21%) — the extra per-iteration branch itself cost more than it saved for these two tables' specific shape (their real matches tend to appear early in the table, so the skip rarely fires). Also reverted.
3. **`ends_with` itself, given a cheap last-code-unit reject before building the full comparison iterator** — no allocation, no restructuring, applies to every caller of `ends_with` (both `longest_suffix` and `first_suffix`, and any direct call). This is the one change that survived, and it is a clean, one-sided win: re-measured against the true pre-change baseline, **every one of the 9 Snowball languages plus English improved, 5%–18% faster per word**, with `fr-carry` (Carry French, a different algorithm from the Snowball one) showing no significant change either way — none regressed.

Correctness before any of these numbers were trusted: all 79 of this crate's own unit tests (including 4 new tests added directly for this change, covering the fast-path's own last-unit-mismatch/match cases), all 17 doctests, and the competitive workspace's own `tests/stemmers_correctness.rs` (14 tests, byte-exact agreement with `rust-stemmers`/`snowball_stemmers_rs` on the benchmarked word lists) all still pass — the fast path changes *when* a full string comparison runs, never *what* it returns.

**This closes part of the gap, not the whole thing — stated plainly, not oversold.** A 5–18% per-word speedup is real but small next to the 2×–8× competitor gap this entry documents; `rust-stemmers`/`snowball_stemmers_rs`'s real advantage (a genuine `find_among` binary search with common-prefix tracking, generated by the official Snowball compiler) was not reimplemented here. A full port of that algorithm remains the clearest, best-evidenced follow-up — but two independent, provably-correct attempts at a partial version of it (items 1 and 2 above) both regressed the exact languages they were meant to help, real evidence that this is harder to get right for this crate's specific table shapes than it looks on paper, and that any future attempt should budget for the same kind of controlled, per-language A/B benchmarking this pass used to catch its own two false starts before they shipped.

## 35. Spellcheck: a real architectural trade-off, not a one-sided loss — Verbora vs. `fast_symspell` (Rust, SymSpell-family deletion index)

`fast_symspell` was previously marked `No`/`No` in `docs/COMPETITIVE_BENCHMARKS.md`
§1.17 ("no repository URL — unverifiable provenance; download curve
consistent with abandonment"); re-investigated this round and the verdict
**overturned** to `Partial`/`Yes` — see `benches/spellcheck.rs`'s own "re-
investigated, not taken on faith" doc comment section for the full evidence
(readable via `cargo`'s registry-cache tarball despite no GitHub link; a
real, if low, steady 90-day download curve, not a cliff). Confirmed working
and correct on this workspace's own corpus in
`tests/spellcheck_fast_symspell_correctness.rs` before any timing number
below was trusted. Two comparisons below: `fast_symspell` against Verbora's
`Spellcheck` (its combinatorial-generation get_corrections), and separately
against `verbora-spellcheck::FuzzyIndex` (Verbora's own BK-tree extension) —
different question, different answer.

| | |
|---|---|
| **Capability** | (a) dictionary construction; (b) `get_corrections` at distance 1 and 2; (c) `FuzzyIndex::neighbors` vs. `fast_symspell`'s lookup, construction and query, at corpus sizes 100–20,000 |
| **Competitor** | `fast_symspell` 0.1.10 (`ahash` + `triple_accel`-verified fork of `symspell` 0.5.2, plus an `rkyv` zero-copy archived-load path) |
| **Verbora result** | (a) construction: 100 **12.8 µs** · 1,000 **147.9 µs** · 10,000 **1.657 ms** · 20,000 **3.598 ms**. (b) `get_corrections` d1: **~21–24 µs** flat across all four sizes; d2: 1,000 **5.968 ms** · 20,000 **5.545 ms**. (c) `FuzzyIndex` construction: 100 **36.7 µs** · 1,000 **835.5 µs** · 10,000 **26.35 ms** · 20,000 **27.51 ms**; query: 100 **601.5 µs** · 1,000 **11.53 ms** · 10,000 **94.42 ms** · 20,000 **179.51 ms** |
| **Competitor result** | (a) construction: 100 **356.8 µs** · 1,000 **3.734 ms** · 10,000 **47.52 ms** · 20,000 **122.29 ms**. `load_archived_bytes` (rkyv, pre-built): **3.12 ns** regardless of corpus size, vs. `build_from_scratch` **114.9 ms**. (b) `get_corrections`-equivalent d1: **~765–896 ns** flat; d2: 1,000 **2.155 µs** · 20,000 **3.289 µs**. (c) query: 100 **279.3 µs** · 1,000 **718.1 µs** · 10,000 **1.586 ms** · 20,000 **2.693 ms** |
| **Gap** | **Construction: Verbora wins decisively**, 25×–34× faster across all four sizes (widening with corpus size — a delete-precomputation index is inherently more expensive to build the larger the corpus and `max_distance` get). **Query at distance 1: `fast_symspell` wins**, 26×–31× faster, flat with corpus size on both sides (both are effectively O(1)-ish lookups at d1). **Query at distance 2: `fast_symspell` wins by a dramatically wider margin** — 1,686× at n=20,000, up to 2,769× at n=1,000 — Verbora's combinatorial edit generation is the one paying a real, size-independent-but-large fixed cost per call at d2 that d1 does not expose. **`FuzzyIndex` vs. `fast_symspell` query specifically**: `fast_symspell` wins, and the margin **widens sharply with corpus size** — 2.15× at n=100 up to 66.7× at n=20,000 (`FuzzyIndex`'s own construction is faster and scales more gently than `fast_symspell`'s, the reverse of the query picture — see (c) above). **Archived load**: loading a pre-built `rkyv` archive is ~36.8 million times faster than `build_from_scratch` — the dictionary's real weakness (expensive to build) essentially disappears when construction happens once and the result is persisted, a capability neither `Spellcheck` nor `FuzzyIndex` has an equivalent of today. |
| **Likely reason** | Architectural, not a tuning gap, confirmed by reading both crates' source directly. `Spellcheck::get_corrections`/`FuzzyIndex::neighbors` generate or check candidates combinatorially at query time (`corrections_over` loops `depth in 1..=distance`, and `FuzzyIndex`'s BK-tree computes a real edit distance per candidate via `verbora_distance::levenshtein`) — cheap to build (nothing precomputed), but query cost grows sharply with `max_distance` because the candidate space does. `fast_symspell`/`symspell` precompute every word's *deletion set* up to `max_distance` once, at construction time, so a query is a small number of hash lookups regardless of distance — expensive, size-dependent construction traded for near-flat query cost, the textbook SymSpell trade-off, now measured directly against both of Verbora's own spellchecking data structures rather than assumed from the algorithm's reputation. |
| **Profiling evidence** | Read `symspell-0.5.2`'s `SymSpell::lookup`/`create_dictionary_entry`/`edits_prefix` and `fast_symspell-0.1.10`'s near-verbatim fork of the same (`symspell.rs`, confirmed line-for-line) directly, alongside `crates/verbora-spellcheck/src/lib.rs`'s `corrections_over` and `src/fuzzy_index.rs`'s BK-tree query. Real run: `cargo bench -p competitive-rust --bench spellcheck -- spellcheck_new/ spellcheck_get_corrections_d1/ spellcheck_get_corrections_d2/ spellcheck_fuzzyindex_construction/ spellcheck_fuzzyindex_query/ spellcheck_fast_symspell_archived_load/` in `benchmarks/competitive/`, raw Criterion `estimates.json` under this workspace's shared target directory's `criterion/spellcheck_*/`. Correctness: `tests/spellcheck_fast_symspell_correctness.rs` (5 tests, including the domain where `FuzzyIndex` and `fast_symspell` genuinely agree on deletion-typo corrections and where the disagreement is explained). |
| **Optimization opportunity** | ~~Per `docs/research/fase6-benchmark-brief.md`'s own directive to weigh this: the evidence supports offering a second, deletion-index-backed structure...~~ **Done, see the update below.** |

**Update, later pass — implemented, not just recommended.** `verbora_spellcheck::DeletionIndex`/`DeletionIndexBuilder` now exist: a SymSpell-style deletion index built in-house with `verbora_distance` primitives, exactly as recommended (not by wrapping `fast_symspell` itself). Deletion generation operates on **UTF-16 code units**, not `char`s — matched, when it was written, to the granularity `verbora_distance::levenshtein` then verified distance at, since generating candidates at a coarser granularity than the verifier's would silently under-generate for astral (non-BMP) input; the same class of bug this crate's own `edits.rs`/`units.rs` already documents for `Spellcheck`'s edit generator. **That premise is now inverted and the alignment needs revisiting.** `verbora_distance::levenshtein` counts Unicode scalar values, not UTF-16 code units (`docs/design/distance-contract.md` §2), so the verifier is now the *coarser* of the two: a deletion index keyed on code-unit deletions generates candidates that split an astral character in half — sequences the scalar-granularity verifier will never score at distance 1. `verbora-spellcheck` itself has not been migrated (`crates/verbora-spellcheck/src/units.rs` still encodes to `Vec<u16>`), so this is recorded here as a real, open cross-crate inconsistency rather than a closed one; the ASCII and BMP domains, which is what the numbers below measure, are unaffected either way. Correctness verified before any timing below was trusted: a 3,000-word ASCII sample vs. brute force at distance 0–3 (`tests/deletion_index.rs`), a dedicated astral-character-heavy dictionary vs. the same baseline (exercising the code-unit fix directly), and agreement with `FuzzyIndex` itself on a 1,000-word sample at every distance within `DeletionIndex`'s own build-time cap. Independently re-verified by a second, adversarial audit agent with no visibility into the implementation's own design reasoning.

**Real result: another genuine, honest trade-off, not a clean win.** Re-measured (`cargo bench -p verbora-spellcheck --bench deletion_index`, `max_distance=2`, same machine): construction — `DeletionIndex` is **13×–25× slower to build than `FuzzyIndex`** at every size (977.6 µs → 407.0 ms vs. 38.7 µs → 26.97 ms, n=100→20,000) — the same shape of cost `fast_symspell` itself pays against Verbora's plain `Spellcheck` above. Query — a genuine **crossover**: `FuzzyIndex` is actually faster at the smallest size (100 words, 1.73×, where a shallow BK-tree beats a deletion index's fixed per-query overhead), but `DeletionIndex` wins from 1,000 words up, the margin **widening rapidly** — 4.9× → 35.3× → 54.3× at 20,000 — near-flat growth with corpus size (3.3× over a 200× larger corpus) against `FuzzyIndex`'s roughly 300× growth over the same range, the same widening shape `fast_symspell` showed against `FuzzyIndex` in the comparison above. `DeletionIndex` also beats brute force by a wide, widening margin throughout (1.6× → 190×). Neither structure replaces the other: `FuzzyIndex` stays the default (cheaper, more predictable, no build-time distance cap); `DeletionIndex` earns its place for large (≥1,000-word), high-query-volume, fixed-`max_distance` workloads specifically — see `docs/PERFORMANCE_MATRIX.md`'s own `DeletionIndex` entry and `docs/COMPETITIVE_BENCHMARKS.md` §1.17's updated Architectural decision note for the full numbers and reasoning.

## 36. Independently-confirmed upstream bugs, found while verifying competitor crates before trusting their numbers

`docs/research/fase6-benchmark-brief.md`'s own "Do NOT trust marketing benchmarks — reproduce locally"
rule surfaced real, reproducible defects in third-party dependencies — none
in Verbora's own code, all found by the same discipline this whole document
already applies (verify before trusting a number). Items 1–3 came out of
that round's re-verification passes over crates flagged as abandoned/stale;
item 4 (added 2026-08) came out of the phonetics-extension equivalence
audit's differential fuzzing of an actively-maintained crate. Recorded here
as disclosed findings, not filed upstream without separate confirmation.

1. **`triple_accel` 0.4.0 `rdamerau_exp` over-counts a doubled-letter
   insertion.** `rdamerau_exp("tac", "tatc")` returns **2**; the correct
   restricted-Damerau-Levenshtein distance is **1** (one insertion).
   Confirmed against `strsim::damerau_levenshtein` (returns 1) and against
   `triple_accel`'s own `levenshtein_exp` (plain Levenshtein, which can only
   be ≥ the restricted-Damerau distance — it also returns 1, so `rdamerau_exp`
   returning a number *larger* than plain Levenshtein is internally
   inconsistent, not just externally wrong). Independently reproduced in a
   standalone scratch crate outside this repository, not just inside the
   test suite. Regression-pinned in
   `tests/spellcheck_fast_symspell_correctness.rs`'s
   `triple_accel_rdamerau_exp_overcounts_on_a_reproducible_input_shape` — real
   impact: `fast_symspell` uses this exact function as its post-deletion-
   lookup distance-verification pass (see entry 35), so this bug can cause
   `fast_symspell` to silently miss or misrank a correction on an ordinary
   doubled-letter typo shape, not a synthetic corner case.
   **Update (2026-08) — the defect is wider than this item recorded: plain
   `rdamerau` carries it too.** This item, and the `Optimization
   opportunity`/correctness prose that leaned on it, treated `rdamerau` as
   the safe entry point and `rdamerau_exp` as the buggy one. A randomized
   sweep across the widened correctness corpus disproved that, and it was
   then re-confirmed on the same minimal reproducer directly:
   `triple_accel::rdamerau(b"tac", b"tatc")` returns **2** as well, against
   **1** from a from-scratch three-row OSA implementation,
   `strsim::osa_distance`, `rapidfuzz`'s `distance::osa` and Verbora's
   `osa`. (The original confirmation above cited
   `strsim::damerau_levenshtein` — the *unrestricted* function — which
   happens to answer 1 here too, but `strsim::osa_distance` is the correct
   oracle for a restricted-Damerau claim and is what the current tests use.)
   Consequence for `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`:
   `rdamerau` is asserted equal to Verbora's `osa` only on the shared ASCII
   corpus (`restricted_damerau_levenshtein_agrees_with_triple_accel_rdamerau`,
   which passes — the corpus never hits the defect) and is deliberately
   excluded from the randomized-sweep agreement, with the exclusion
   documented inline in `assert_integer_metrics_agree`. Entry 27's (a)
   timing row is unaffected and stays: both sides do the same shape of work
   on the benchmarked pairs.
2. **`fst` 0.4.7's `Levenshtein` automaton silently returns incomplete
   results for same-byte-length multi-byte UTF-8 substitutions.**
   `Set::search(&Levenshtein::new("аб", 1))` (Cyrillic) against a set
   containing `"ав"` (one substitution away) returns **nothing**, at any
   `max_distance` up to 4, even though `Set::contains` confirms both keys
   are present. Latin BMP accented substitutions (e.g. `café`/`cafe`, a
   2-byte-to-1-byte substitution) are *not* affected — only same-byte-length
   multi-byte substitutions are. Reproduces on plain, individually-
   constructed `fst::Set`s with no Verbora code involved. Matches a still-
   open upstream issue,
   [BurntSushi/fst#38](https://github.com/BurntSushi/fst/issues/38)
   ("levenshtein automata not matching Japanese Characters correctly",
   opened 2017) — a real, disclosed, already-known-upstream defect, not a
   fairness artifact of this comparison; the crate's own doc comment
   self-describes its Levenshtein automaton as "not speedy" and warns it
   "should [be] vastly improved in the future." `benches/fst_fuzzy.rs`'s
   ASCII-only corpus never exercises this — see that file's own doc comment
   for why its NARROWED_EXACT classification (not plain EXACT) is real, not
   silently dodged.
3. **`eddie` 0.4.2's internal buffer code violates a `slice::get_unchecked_mut`
   safety precondition on ordinary input, aborting any debug-profile build
   the moment `Jaro`/`JaroWinkler::similarity` is called.** Reproduced
   standalone: `eddie::Jaro::new().similarity("martha", "marhta")` — the
   textbook Wikipedia example, not an edge case — aborts
   (`unsafe precondition(s) violated: slice::get_unchecked_mut requires that
   the index is within the slice`, `eddie-0.4.2/src/utils/buffer.rs:26`)
   under a debug build on this workspace's pinned toolchain (rustc 1.97.1),
   whose standard library now runs these precondition checks by default in
   debug profile (a stable-Rust capability added years after `eddie`'s last
   publish in 2020). **Does not reproduce in `--release`** — the checks are
   compiled out, and the returned values come back numerically correct.
   Because this is a process **abort**,
   not an ordinary test failure, `cargo test`'s `--no-fail-fast` does not
   rescue other tests still pending in the same binary
   (`tests/distance_correctness.rs`) — confirmed by re-running the full
   `competitive-rust` test suite with `--no-fail-fast`: every *other* test
   binary in the workspace (one process per integration-test file) completed
   and passed cleanly, including the two other eddie-independent
   correctness suites touched this round (`fst_fuzzy_correctness`,
   `spellcheck_fast_symspell_correctness`) and `trie_correctness` (the
   confirmation that `fast_radix_trie`'s and `fst`'s concurrently-added
   benchmark/test edits merged cleanly). This is additional, sharper
   evidence for the "crate abandoned since 2020" caveat `Cargo.toml` already
   carried for `eddie` — not a new discovery that it is unmaintained, but a
   concrete, reproducible consequence of that fact against a modern
   toolchain. No code change made to work around it (the abort is in
   `eddie`'s own `unsafe` code, not reachable from this workspace); flagged
   here rather than silently tolerated.
   **Correction (2026-08) — this item understated the defect, and the
   understatement is withdrawn.** It was recorded as "technically-out-of-spec
   `unsafe` usage that happens not to corrupt anything observable on this
   platform/toolchain today, not a correctness bug in eddie's output".
   Reading `eddie-0.4.2/src/utils/buffer.rs` in full does not support that
   reading. `Buffer::store` calls `buf.clear()` — length now `0` — then writes
   through `buf.get_unchecked_mut(i)` for successive `i`, and only calls
   `buf.set_len(i)` after the loop finishes. Every write is out of bounds of
   the slice `buf` derefs to at the moment it happens; `set_len` afterwards
   cannot retroactively make a past write in-bounds. This is **undefined
   behaviour on every non-empty call**, not merely a violated debug assertion,
   and `eddie::Jaro`/`eddie::JaroWinkler::similarity` both route through it —
   `eddie::Jaro::new().similarity("a", "a")` alone is enough. "Numerically
   correct in `--release`" describes what one build of one compiler version
   happened to emit; it is not a property the program has. The practical
   consequence for this project is narrow and firm: **no `eddie` timing number
   can be trusted as evidence, because the only build that completes is the one
   with the checks compiled out.** `docs/COMPETITIVE_BENCHMARKS.md` §1.8's
   eddie rows carry the same reading.
   **Disposition (2026-08) — decided, and this item's "not decided here" is
   withdrawn.** The three options it listed were: unpin, replace with the
   `strsim`/`rapidfuzz` Jaro rows, or keep with the defect disclosed. The
   second is not available — those rows are **`Partial`**, not `Yes`: both
   crates truncate Jaro's half-transposition count with integer division where
   `docs/design/distance-contract.md` §3.4 requires the exact half, and both
   gate the Winkler boost behind `sim > 0.7` where §3.4 applies it
   unconditionally (pinned by
   `strsim_and_rapidfuzz_jaro_diverge_from_the_contract_by_truncating_transpositions`
   and `assert_jaro_family_agrees` in
   `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`).
   What was chosen instead is a split the three options did not contain:
   **`eddie` is retained for *correctness* and removed from *timing*.** It stays
   a **dev**-dependency, reached only through `eddie::slice::Jaro` /
   `eddie::slice::JaroWinkler` — the two entry points that never touch
   `Buffer::store` and reach zero `unsafe` on their whole call graph — because
   it is the only implementation in the harness computing §3.4's function.
   No timing row exists and **none may be added**: timing its published `str`
   API would time undefined behaviour, and timing the slice wrapper would hand
   it pre-decoded `Vec<char>` operands while Verbora's `jaro(&str, &str)`
   decodes scalars inside the timed region, which `AGENTS.md`
   § *Cross-Implementation Benchmark Fairness* forbids. The containment is
   machine-enforced by
   `every_reference_to_eddie_goes_through_the_sound_slice_wrapper`, which walks
   every `.rs` file in that crate and fails the suite if any other `eddie` path
   appears in code. Accordingly the ten `eddie` timing rows in
   `benchmarks/competitive/results/results.json`, their ten
   `results/raw/distance-*-eddie-*.json` copies and the ten `eddie` rows in
   `results/distance-memory.json` — all produced by a release build that
   executed UB — have been removed rather than left to be read as slow numbers.
   The record lives in `benchmarks/competitive/README.md`
   § "Resolved: `eddie` 0.4.2 is unsound, and is now contained" →
   "The decision: isolate for correctness, drop from timing", and in
   `manifests/competitors.json`'s own `eddie` entry. (The section this item
   used to point at, "Blocking defect surfaced by that repair", no longer
   exists; it was the pre-decision name of that same section.)
4. **`rphonetic` 3.0.6 panics on realistic non-ASCII input in four encoder
   families.** Found by the phonetics-extension equivalence audit (the
   104,114-input-per-encoder differential fuzz behind
   `benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs`'s
   regime-2 byte-exact claims), every case reproduced against rphonetic
   3.0.6 **release** builds — these panic outright in release, unlike eddie's
   abort above, which the release profile compiles out even though the
   underlying undefined behaviour does not go away with it:
   - `Nysiis` (strict mode, its default): the truncation
     `result[..min(len, 6)]` is a raw byte slice with no char-boundary
     check, so any input whose >6-byte code puts a multi-byte character
     across byte offset 6 panics — 4,233 of the 104,114 fuzz inputs hit it,
     i.e. a realistic non-ASCII-surname shape, not a pathological one.
   - `Caverphone1`/`Caverphone2`: same class — the final `&txt[0..6]` /
     `&txt[0..10]` byte slice on the padded rewrite result panics when a
     surviving multi-byte (non-ASCII, Unicode-lowercase) character
     straddles the cut. ASCII input can never reach it.
   - `RefinedSoundex`: cleaning keeps every `char::is_alphabetic()`
     character but then indexes `mapping[ch as usize - 65]`, so any
     alphabetic character whose uppercase form is not entirely `A`–`Z`
     (`é`, `ñ`, Cyrillic, CJK, `İ` U+0130, `ʼn` U+0149, the Kelvin sign
     U+212A) indexes out of bounds.
   - `MatchRatingApproach`: two distinct paths — `encode`'s
     `&value[0..3]` / `&value[len - 3..]` truncation panics when either
     offset falls mid-character (`"Москва"`, `"ABC日X"`), and
     `is_encoded_equals` underflows on an empty encoding: verified against
     release rphonetic, `("..", "ab")` returns `false` but `("ab", "..")`
     panics — an asymmetric partial function.

   Verbora's own seven spec-pinned encoders panic on **none** of these
   inputs: each substitutes a defined, documented output (see the
   "Divergence" section of each module's own doc comment in
   `crates/verbora-phonetics/src/`), and exactly these input shapes are
   excluded from the benchmark domain per the fairness pattern, so no
   timing number anywhere compares a panicking path. Recorded alongside,
   distinct from the bugs: four `DaitchMokotoffSoundex` behavioral quirks
   Verbora *reproduces deliberately* for byte-parity rather than treats as
   defects (non-ASCII rule keys `ą`/`ę`/`ţ`/`ț` consuming the following
   character; a before-a-vowel probe that looks one character too far;
   duplicate final codes surviving branch dedup; `ü`/`œ` missing from the
   ASCII-folding list) — documented in
   `crates/verbora-phonetics/src/daitch_mokotoff.rs`'s module doc.

## 37. Fuzzy candidate lookup: a double crossover — Verbora's `FuzzyIndex` vs. `fst`'s Levenshtein automaton (Rust)

The companion result to entry 33 (plain trie operations, where Verbora wins
across the board): the *same* `fst::Set`, this time queried through its
`Levenshtein` automaton feature against `verbora-spellcheck::FuzzyIndex`
(Verbora's own BK-tree extension) — "which stored words are within edit
distance k of this query," not "does this exact word exist." Set-equality
verified across a spread of real-corpus queries and `max_distance` values in
`tests/fst_fuzzy_correctness.rs` before any timing number below was trusted.

| | |
|---|---|
| **Capability** | (a) construction (`fst` includes its required sort+dedup, same discipline as entry 33); (b) query, `max_distance=2`, 200 queries per corpus size |
| **Competitor** | `fst` 0.4.7, `Set::search` + `automaton::Levenshtein` |
| **Verbora result** | (a) `FuzzyIndex` construction: 100 **31.6 µs** · 1,000 **778.3 µs** · 10,000 **12.30 ms** · 20,000 **25.90 ms**. (b) query: 100 **563.7 µs** · 1,000 **10.44 ms** · 10,000 **96.89 ms** · 20,000 **187.43 ms** |
| **Competitor result** | (a) `fst` construction: 100 **59.4 µs** · 1,000 **473.8 µs** · 10,000 **3.956 ms** · 20,000 **7.079 ms**. (b) query: 100 **30.59 ms** · 1,000 **68.37 ms** · 10,000 **102.26 ms** · 20,000 **128.69 ms** |
| **Gap** | **Two independent crossovers, not one-sided in either direction.** Construction: `FuzzyIndex` wins at n=100 (**1.88×**), then `fst` wins from n=1,000 up, widening (**1.64×** → **3.66×** at n=20,000). Query: the *opposite* shape — `FuzzyIndex` wins dramatically at n=100 (**54.3×**), the margin collapsing through n=1,000 (**6.55×**) to near-parity at n=10,000 (**1.06×**, `FuzzyIndex` still marginally ahead), then `fst` overtakes at n=20,000 (**1.46×**). |
| **Likely reason** | `fst::automaton::Levenshtein::new` builds a fresh DFA **per query** (confirmed by reading `fst::automaton::Levenshtein`'s constructor) — real, fixed per-call setup cost that dominates at low query volume/small corpora (explaining the large early query win for `FuzzyIndex`, which has no comparable per-query construction step, just a tree walk) but amortizes better as the automaton is reused against a larger, shared underlying `Set` structure that itself benefits from `fst`'s node-sharing as corpus size grows. Construction's crossover runs the other way because `FuzzyIndex`'s BK-tree insert cost (recursive descent keyed by exact edit distance to parent, `crates/verbora-spellcheck/src/fuzzy_index.rs`) grows less predictably with corpus size than `fst`'s streaming, sorted-input `SetBuilder`, whose asymptotic behavior is a well-understood, minimal-automaton construction. |
| **Profiling evidence** | Read `fst`'s `automaton::Levenshtein::new` and `crates/verbora-spellcheck/src/fuzzy_index.rs`'s `FuzzyIndexBuilder`/`Neighbors` directly. Real run: `cargo bench -p competitive-rust --bench fst_fuzzy` in `benchmarks/competitive/`, raw Criterion `estimates.json` under this workspace's shared target directory's `criterion/fst_fuzzy_{construction,query}/`. Correctness: `tests/fst_fuzzy_correctness.rs`'s `fst_and_fuzzy_index_agree_on_ascii_queries` and `fst_levenshtein_automaton_size_limit_is_real`. |
| **Optimization opportunity** | None flagged with confidence — this is a genuine shape difference (per-query automaton construction vs. per-query tree walk), not a tuning gap in either implementation, and the crossover point (~n=10,000–20,000 for query, ~n=100–1,000 for construction) is close enough to real corpus sizes that neither side is a strictly dominant choice; a caller choosing between `FuzzyIndex` and an `fst`-based approach should measure at their own corpus size and query volume rather than assume either wins by default. |

## 38. Character n-gram generation, trigrams — Verbora vs. `ngrammatic` (Rust): a small but consistent loss, alongside a clear bigram win

`docs/COMPETITIVE_BENCHMARKS.md` §1.2 originally recorded `NO FAIR COMPETITOR
FOUND (Rust)` for N-Grams. Re-examined: `ngrammatic`'s `Ngram`/`NgramBuilder`
(the character n-gram + frequency-count generator its `Corpus` fuzzy-search
feature is itself built on, a capability with no Verbora equivalent and
still not benchmarked) turns out to be a fair, comparable primitive against
Verbora's generic `ngrams()` engine called with `T = char`.
`tests/ngrams_correctness.rs` confirms byte-identical `(gram, count)` output
across all 20,000 words in the shared word list, at both arities benchmarked
here, before any timing number below was trusted. Three independent
full-default-Criterion runs of `cargo bench --bench ngrams` (from
`benchmarks/competitive/rust-competitors/`) were taken, reading the
**median** field from Criterion's own `estimates.json` each time (not the
mean-based confidence interval Criterion prints to the terminal by default,
which this project's own `PRIMARY METRIC` policy does not use — see
`site/benchmarks/competitive.md`'s methodology table). The direction was
consistent every time in both groups.

| | |
|---|---|
| **Capability** | Character n-grams of a word, padded with `arity - 1` spaces per side and folded into a `(gram, count)` map, over every word in the shared 20,000-word list |
| **Competitor** | `ngrammatic` 0.7.0, `NgramBuilder::new(word).arity(n).pad_full(Pad::Auto).finish()` |
| **Verbora result** | bigrams: **8.50–9.40 ms** median across 3 runs (8.498, 8.986, 9.397 ms). trigrams: **11.98–12.49 ms** median across 3 runs (11.976, 12.147, 12.494 ms) |
| **Competitor result** | bigrams: **9.78–10.07 ms** median across 3 runs (9.881, 9.775, 10.071 ms). trigrams: **11.61–11.67 ms** median across 3 runs (11.662, 11.674, 11.608 ms) |
| **Gap** | **Bigrams: Verbora wins every run**, ~1.07×–1.16× faster (1.163×, 1.088×, 1.072×). **Trigrams: Verbora loses every run**, narrowly — ngrammatic ahead by ~1.03×–1.08× (1.027×, 1.041×, 1.076×). Small margins, but the direction never reversed across all 3 independent runs in either group, so trigrams is recorded as a genuine (if marginal) loss rather than dismissed as noise, per this file's own no-cherry-picking charter. |
| **Likely reason** | Not isolated with a profiler this pass. Both sides do the same conceptual work — pad, slide a window of size `arity`, fold into a map — over the same input, so the residual is plausibly in accumulation shape: Verbora's benchmarked path builds a `Vec<Cow<[char]>>` of grams via the generic `ngrams()` engine (`crates/verbora-ngrams/src/engine.rs`) and then folds each into a `HashMap<String, usize>` with a `String` allocation per unique gram (`gram.iter().collect()`), while `ngrammatic::NgramBuilder` accumulates directly into its own `HashMap<SmolStr, usize>`, whose small-string optimization avoids a heap allocation for any gram that fits inline (bigrams and trigrams over ASCII words both do) — a plausible, source-read explanation for why the arity-3 gap (three-character-plus-padding grams, closer to `SmolStr`'s inline capacity boundary) is smaller/reversed relative to bigrams' clear win, not a confirmed, profiled one. |
| **Profiling evidence** | Read `ngrammatic-0.7.0/src/ngram.rs` (`NgramBuilder::finish`, the `HashMap<SmolStr, usize>` grams field) and `crates/verbora-ngrams/src/engine.rs` (`ngrams()`) directly. Real runs: `cargo bench --bench ngrams` from `benchmarks/competitive/rust-competitors/`, full Criterion defaults, 3 independent invocations, median read from each run's own `estimates.json`. Correctness: `tests/ngrams_correctness.rs`. |
| **Optimization opportunity** | None flagged with confidence at this margin — a ~3–8% difference at this scale does not justify a targeted change without first profiling to confirm the `SmolStr`-vs-`String` accumulation theory above; flagged for a future pass if character-level n-gram generation ever becomes a dedicated, string-input-facing Verbora API (today it is only exercised by calling the generic engine directly, per `benches/ngrams.rs`'s own doc comment). |

**Update, text-shaping migration (2026-08) — the comparison survives under the
same group names; the Verbora call it timed does not.** `verbora-ngrams` was
rewritten to `docs/design/text-shaping-contract.md` §3.3. The generic
`ngrams()` engine call this entry benchmarked —
`ngrams(&chars, arity, Some(' '), Some(' '))`, yielding `Cow<[char]>` windows
over a lazily-padded sequence — is replaced by
`Padded::new(&chars, arity, Some(&' '), Some(&' ')).ngrams()`: the arity is a
`NonZeroUsize`, and the padded sequence is materialised **once** rather than a
`Cow` window being cloned per gram. `benches/ngrams.rs`' `bigrams` and
`trigrams` groups still exist and still compare against
`ngrammatic::NgramBuilder`, unchanged at 0.7.0.

⚠ **Both groups' Verbora figures are retired pending re-measurement.** The
8.50–9.40 ms bigram and 11.98–12.49 ms trigram medians, the 1.07×–1.16× bigram
win and the 1.03×–1.08× trigram loss, and the three-run consistency argument
that made the marginal trigram loss reportable at all, all measured the
deleted engine call. This entry's "Likely reason" is the specific casualty:
it attributes the residual to accumulation shape — Verbora building a
`Vec<Cow<[char]>>` and folding each gram into a `String`, against
`ngrammatic`'s `SmolStr` small-string optimization — and the `Cow` half of
that contrast is precisely what the rewrite removed. Whether trigrams remain a
loss at all is an open question, not a recorded result.
`docs/design/text-shaping-contract.md` §7 item 7 states the direction
expectation (`Padded` should favour the new shape) and marks it unverified.

Correctness moved the other way and is worth recording separately from the
timings: `tests/ngrams_correctness.rs` now checks identical `(gram, count)`
maps over all 20,000 words at both arities **and** over inputs shorter than
`arity` — the one divergence the previous pass disclosed but could not
exercise, because the shortest word in the list is three characters.

*(Additional entries are added here as the rest of Fase 6's module-by-module
benchmarks execute — this file is not yet the complete gap inventory the
spec's Definition of Done requires.)*
