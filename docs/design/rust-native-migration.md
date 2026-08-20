# Rust-native migration — plan and standing findings

**Status:** in progress. `verbora-distance` is complete; twelve crates remain.

Verbora is specified from Rust. Its behaviour is defined by an explicit
contract plus the tests that pin it — never by another implementation. This
document is the working record for the migration that makes that true
everywhere: what has already been settled, what each crate still owes, and
the findings that must not be rediscovered.

## The rule

A crate is Rust-native when its **code, tests and documentation** all agree
that Verbora's contract is the source of truth. Three things must hold:

1. **Behaviour** is derived from a published standard or from an explicit
   Verbora specification — not from another implementation's output.
2. **Tests** pin that contract. Expected values come from the standard or
   from an independently written reference, never from recording what the
   current code emits.
3. **Documentation** explains what Verbora does, not where it came from.
   Rustdoc states API contracts; the VitePress site is the product
   documentation.

Competitors appear in exactly one place: competitive benchmarks, named with
version, methodology, measured result and comparability limits. A competitor
never defines correctness.

## Settled: unrestricted Damerau–Levenshtein

The pattern this migration follows, already executed end to end.

`damerau_levenshtein` used to evaluate a recurrence that updated its
last-occurrence map *inside* the column loop, letting a cell claim a
transposition against its own row. Consequences, all measured:

- **Not symmetric** — `d("bb","abbb") = 1` but `d("abbb","bb") = 2`. A
  distance that depends on argument order is a defect, not a quirk.
- Diverged from every other implementation on ~38.6% of random
  small-alphabet pairs.
- Blocked Zhao–Sahni and common-affix trimming, costing a measured 2490×
  against the fastest competitor on near-identical pairs.

It now implements canonical Lowrance–Wagner (Zhao–Sahni linear space), with
affix trimming enabled. Verified against an in-tree reference transcribed
from the published recurrence, plus 202,000 randomized pairs cross-checked
externally — zero divergences. Symmetry and the triangle inequality are
pinned by property tests.

`osa` / `osa_search` became first-class functions. `Options.restricted` is
gone: the algorithm lives in the function name.

**The lesson to reuse:** redefine behaviour, replace fixtures with values
computed from the standard, *then* rewrite documentation. Renaming prose
without changing behaviour would have left the defect in place.

## Standing findings — do not re-litigate

Measured floors. Each was investigated, quantified, and is a real constraint
rather than unfinished work.

| Finding | Evidence |
|---|---|
| **Hamming vs SIMD competitors** | Safe-Rust SWAR reaches ~1.2–1.6× of an AVX2 implementation at n ≥ 64. Closing it needs `unsafe`, which `unsafe_code = "deny"` forbids. Already 12.7× faster than before the campaign. |
| **Naive-Bayes training** | Verbora stems every token because stemmed tokens are observable in its public state; that alone exceeds a competitor's entire training run at small corpus sizes. |
| **`predictive_search` enumeration** | Partly a benchmark-shape artifact: Verbora's row returns owned `String`s while a competitor's counts borrowed keys. The borrowed-keys API is the like-for-like comparison. |

Upstream defects found while verifying competitors, reproduced
independently and pinned by test:

- **`triple_accel::rdamerau`** over-counts insertions next to a repeated
  character — `"tac"` → `"tatc"` returns 2, true restricted-Damerau distance
  is 1. Confirmed against a from-scratch OSA reference, `strsim` and
  `rapidfuzz`. Both `rdamerau` and `rdamerau_exp` carry it, so the earlier
  belief that `rdamerau` was the safe entry point is wrong.
- **`rphonetic` 3.0.6** panics on realistic non-ASCII input in Nysiis
  (strict), Caverphone 1/2, RefinedSoundex and MatchRatingApproach. Verbora
  panics on none of them.
- A **HuggingFace tokenizer** benchmark segfaulted once under a full-suite
  run and proved non-reproducible in isolation across 544 measurements —
  consistent with memory pressure reaching its C dependency through FFI.
  Benchmarks are therefore run per target so one crash cannot abort a
  campaign.
- **`eddie` 0.4.2 is unsound.** `utils/buffer.rs`'s `Buffer::store` calls
  `buf.clear()`, then writes through `buf.get_unchecked_mut(i)` for
  `i = 0, 1, 2, …`, and calls `set_len` only afterwards — every write indexes
  a slice whose length is still `0`. Undefined behaviour on the first
  character of every call, not on a rare input. Debug builds abort with a
  non-unwinding panic (`SIGABRT`); release builds only compile the check out.
  Enumerated one process per probe: UB on all of `Levenshtein`,
  `DamerauLevenshtein`, `Jaro` and `JaroWinkler` at the `str` level, plus
  slice-level `Levenshtein`; sound on `str` `Hamming` and on slice-level
  `DamerauLevenshtein`, `Hamming`, `Jaro` and `JaroWinkler`. `0.4.2` is the
  latest published version (2020-01-18), so there is nothing to upgrade to.
  **Contained rather than dropped:** the slice-level Jaro pair reaches zero
  `unsafe`, and `eddie`'s `str` Jaro is literally `buffer.store(s.chars())`
  plus the slice call — so wrapping the slice API computes the same function
  soundly. It is retained *only* as a correctness oracle, in
  `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs`'s
  `eddie_slice` module, with a test that walks every source file in that crate
  and fails if any other `eddie` path reappears in code. No `eddie` timing or
  memory row exists any more, and none may be added: a timing row must call
  the published API as published, and that API is UB.
- **`linfa-bayes` 0.8.1 ships a `dbg!` in published code.**
  `src/multinomial_nb.rs:78` calls `dbg!(&model.class_info.get(&class));` inside
  the training loop, printing the whole per-class model to stderr on every fit.
  One `bayes_train` target emitted **3.6 million lines and 1.5 GB** of it, which
  is how it was noticed: the log exceeded GitHub's 100 MB per-file limit and
  blocked a push. The consequence for measurement is the point — `dbg!` formats
  an `ndarray` and writes it, so that group's `linfa_bayes` timings measure
  stderr I/O rather than Naive Bayes training. **No `linfa_bayes` training
  figure may be published as a like-for-like comparison** until the row is
  either re-measured against a patched build or retired with this reason
  recorded. A win against a competitor crippled by its own debug output is not
  a win, and `AGENTS.md` § Cross-Implementation Benchmark Fairness forbids
  presenting one.

- **`strsim` 0.11.1 and `rapidfuzz` 0.5.0 do not compute Verbora's Jaro.**
  Found while deciding whether `eddie` could simply be dropped. Two
  independent divergences from `docs/design/distance-contract.md` §3.4: they
  truncate the half-transposition count with integer division where §3.4
  requires the exact half, and they gate the Winkler boost behind `sim > 0.7`
  where §3.4 applies it unconditionally. The first alone disagrees with
  Verbora on **23,428 of 82,000** random pairs (28.6%), from operand length 6
  upward — including the `"<n>-near"` shape the benchmark actually times at
  n=64. Fixture: `jaro("abccba","abbaca")` is `0.788…` for Verbora and
  `eddie`, `0.822…` for the other two. Neither side is defective and the
  timing rows still measure comparable work, but `docs/COMPETITIVE_BENCHMARKS.md`
  §1.8 marks all four rows `Yes` for algorithmic equivalence, which is wrong
  and must be amended before any of those numbers is published as
  like-for-like. `eddie` is the only Jaro implementation in the harness that
  matches Verbora, which is why it was kept. Pinned as fixtures in
  `tests/distance_correctness.rs`.

### The transform-then-lookup defect class

The finding from this migration most likely to be rediscovered, because it is
a **shape** rather than a bug: *an index built by one derivation and consulted
by another.*

Wherever a table is keyed by the output of some transform — lowercasing, a
diacritic fold, NFKC, a tokenizer's word walk, a stemmer, a prelude that marks
vowels — and the probe that reaches that table has been through a *different*
transform, entries stop being reachable. Nothing throws. The table keeps its
entries and `contains` keeps answering `true` for them; they simply never match
anything again. The feature quietly gets smaller, and every number around it
stays arithmetically valid.

`verbora-inflectors`' own `every_lexical_entry_is_reachable` (`src/engine.rs`)
names the count: **seven times**. Two of those seven landed *inside the fix for
one of the other five*. Read seven as a floor, not a total — the shape recurs
faster than any tally of it stays current, and the eighth is described below
the table.

| # | The two derivations that disagreed | What it cost | Pinned by |
|---|---|---|---|
| 1 | Swedish/Norwegian `prepare` folded diacritics over the **whole document**, rewriting `för` to `for`, before `is_stop_word` ever saw the token — while the stop-word list is spelled with `å ä ö`. | **124 entries** silently stopped being stop words: 116 of Swedish's 428, 8 of Norwegian's 129. `is_stop_word` still answered `true` for every one of them. | `verbora-stemmers/src/data/audit.rs`, `sv.rs`, `no.rs` |
| 2 | Stop-word entries carrying a character the **word walk** can never put in a token, so no document text can produce them. | Dutch `"je "`, German `"ei,"` — dead entries that a filtering test cannot see, because German also lists `"ei"`, so the token *did* get filtered and the list looked fine. English `"$"` and `"_"` are the third shape: not misspellings of anything, just strings no tokenizer emits. | `verbora-core/src/stopwords/tests.rs` |
| 3 | Indonesian reduplication is one lexeme spelled with `U+002D`; untailored UAX #29 **breaks** there. | All **335 hyphenated roots** of 29,932 and **22 hyphenated stop words** of 809 were unreachable through `tokenize_and_stem`, and `stem_plural` was dead in that path entirely. Fixed by `HYPHEN_JOINS_LETTERS`, not by editing the data. | `verbora-stemmers/src/id.rs` |
| 4 | Suffix **rule tables** versus the per-language prelude (lowercasing, `ß` expansion, Italian acute-to-grave, `I`/`U`/`Y` marking, nasal detour, `ё` fold) that rewrites a token before the table is searched. | Spanish listed `"  aseis"` with two leading spaces, so the `-aseis` imperfect-subjunctive rule never fired for any input and `hablaseis` came back unstemmed. Italian listed `"Yamo"`, whose capital `Y` the Italian prelude cannot produce — it lowercases first. | `verbora-stemmers/src/data/table_audit.rs` |
| 5 | Sentiment **vocabulary keys** versus `WordTokenizer`, which is free to cut a key into pieces the analyzer then looks up one at a time. | **14,273 keys** stopped being reachable. Several did not merely fall to zero, they **inverted**: `non-approved` (−2) scored **+1** as `non` + `approved`. | `verbora-sentiment/tests/reachability.rs` |
| 6 | **Inside the fix for #5.** The stemmed table filed each entry by segmenting the stemmer's output a *second* time; the scoring loop probed with the stemmer's output verbatim. Two spellings of the same sentence, written twice as two pieces of code. | `ofendre's` — Porter-stemmed to `ofendre'` — was filed under `ofendre`, where it answered for the unrelated `ofendre` key it had **displaced**, while the probe `ofendre'` reached nothing. **228 keys** across 14 vocabularies × 16 stemmers scored some other entry's polarity or none. | `verbora-sentiment/tests/key_derivation.rs` |
| 7 | **Inside the fix for #5, again.** `Vocabulary::get` derived a lookup form from its argument before probing an index whose keys already *are* lookup forms — a form derived from a form, the one thing that module's own documentation forbids. | With `PorterStemmerFr` on English senticon, `get("ne'")` re-segmented to `ne`: a *different* entry of the same table with the **opposite sign** (+0.25 where the key is −0.25). Another **352 forms** across sixteen stemmers and thirteen tables missed entirely. The scoring loop was never affected — it probes verbatim — so every existing enumeration passed. | `verbora-sentiment/tests/stemmed_lookup.rs` |

**And once more, one layer up — which is why seven is a floor.** A classifier's
feature keys are *stems*, and `Classifier::restore` rebuilt with the English
Porter default whatever stemmer had trained the model, because a saved model
recorded nothing about which one it was. A French classifier, saved and
restored, kept a feature table of French stems and probed it with English ones:
`chantait` stems to `chant` under `PorterStemmerFr` and to `chantait` under
English Porter, so the restored classifier missed a feature its own vocabulary
held. Every number in the model stayed valid while the answer changed. It is
now `StampError::Incompatible` — the artifact stamp carries
`stemmer_fingerprint`, and `restore` demands the fingerprint of whatever
stemmer it is about to rebuild with. See
`verbora-classifiers/tests/stemmer_stamp.rs`.

**Not one of these was ever caught by a failing test.** The suite was green
through all of them, and in each case for the same reason, stated by the tests
that now exist:

- sv/no: *"Both survived a green suite, because the tests that covered stop
  words sampled a handful of ASCII words. Sampling cannot find this class: the
  entries that die are exactly the ones a spot check does not name."*
- sentiment #5: *"every test still passed, because the handful of keys the
  tests named happened to be single words."*
- sentiment #6: *"It survived a test that checked three `(vocabulary, stemmer)`
  pairs, because all three happened to contain no key whose stem carries a
  character the tokenizer strips."*
- sentiment #7: the defect was in the *public lookup* only, never in the
  scoring loop, so *"the existing enumerations — which score through the
  analyzer — passed."*
- tfidf: *"an entry silently filters nothing and nobody notices, because the
  suite stays green."*

Coverage tooling cannot see it either: every line of the transform and every
line of the lookup runs, and the assertions that run over them are true. What
is missing is not a line, it is a *pair*.

#### How to find it in a crate that has not been audited yet

1. **Name the two derivations, explicitly.** For every table, index, memo,
   saved artifact or `match` arm keyed by a string, write down the function
   that produces the *keys* and the function that produces the *probes*. If
   they are not literally the same call, the crate has a candidate. The
   grep-shaped heuristic: any `contains` / `get` / `binary_search` /
   `FxHashMap` lookup whose argument passed through a lowercaser, normalizer,
   fold, tokenizer, stemmer or prelude on its way in.
2. **Enumerate the table; never sample it.** This is the whole lesson. Walk
   *every* entry of *every* table through the *pipeline that would have to
   produce it*, and assert what it does. Spot checks are structurally blind
   here: the entries that die are precisely the ones nobody thought to name.
3. **Compute the expected answer in the test, from the written contract** —
   not by asking the crate. `verbora-sentiment`'s tests re-implement the lookup
   form and the last-wins collision rule locally so that a change to the
   implementation cannot move the target. A fixture recorded from current
   output would have pinned the defect, not the contract.
4. **Give "unreachable" a predicate, and make every exemption a named,
   pinned class.** `verbora-stemmers/src/data/audit.rs` is the reference:
   an entry is reachable when `prepare(e) == e` *and* `e` is exactly one whole
   token of the word walk; anything unreachable must fall into `NO_WORD` or
   `PHRASE`, each *checked* per entry rather than asserted, and the exempt sets
   are pinned by exact equality per language — so an entry cannot quietly join
   them, and making an exempt entry reachable fails the test too.
5. **When the deriving code cannot be re-run at lookup time, fingerprint it
   and refuse.** A saved model, a prebuilt index, a serialized corpus: the
   derivation is not present to be compared, so record its identity in the
   artifact and reject a mismatch loudly. `ArtifactStamp` is the worked
   example.

The four reference files to read before auditing a new crate:
`verbora-stemmers/src/data/audit.rs` (stop words vs. two transforms),
`verbora-stemmers/src/data/table_audit.rs` (rule tables vs. a prelude),
`verbora-sentiment/tests/reachability.rs` and `tests/key_derivation.rs`
(keys vs. a tokenizer, then vs. a stemmer), and
`verbora-classifiers/tests/stemmer_stamp.rs` (a derivation that outlives its
process). `verbora-inflectors/src/engine.rs`,
`verbora-tfidf/tests/contract.rs`, `verbora-util/src/abbreviations.rs` and
`verbora-transliterators/src/romanize.rs` each now carry the same guard for
their own tables, two of them finding nothing — which is the point: the
enumeration is cheap and it is the only thing that can prove the absence.

## Per-crate scope

Each crate owes the same audit: public API, rustdoc, tests, fixtures,
benchmarks and data. For each, identify behaviour that exists only for
external compatibility, define the Verbora contract (inputs and outputs,
Unicode, errors, empty values, ordering, determinism, floating point,
serialization, limits), redesign the API if needed, implement, replace
parity fixtures with specification or property tests, rewrite rustdoc,
update the site page, and record incompatible changes as Verbora's own
release note.

1. **`verbora-distance`** — **done.** Specified in
   `docs/design/distance-contract.md`. One Unicode scalar is one unit; counts
   are in scalars and positions in bytes; unit cost is the absence of an
   argument; no sentinel, no `NaN`, no panic, no metric rewrites its input.
   The crate root is the entire public surface. Four defects it fixed are
   recorded above. Lessons that generalise to the remaining twelve:
   - **`cargo check --workspace` does not build tests or benches.** Two real
     breakages passed it cleanly. Use `--all-targets`.
   - **A test can pass while the claim it checks is false.** The length-lemma
     test used only exactly-representable costs, so it never rounded — and the
     published bound was wrong by one ulp for four years of arithmetic it never
     exercised. Choose fixtures that can reach the failure.
   - **Making a module private un-publishes its `//!` docs.** Privacy is right;
     losing 781 lines of contract from docs.rs is not. Relocate user-facing
     prose onto public items first.
   - **Site snippets are executable.** `site/check-snippets.py` compiles and
     runs 217 of them, so a stale example is a build failure — but prose and
     tables asserting deleted behaviour are caught by nothing at all, and are
     the larger risk.
2. **`verbora-tokenizers`, `verbora-normalizers`, `verbora-ngrams`** —
   replace inherited regex, ordering and tokenization behaviour with
   explicit Unicode and language rules.
3. **`verbora-inflectors`, `verbora-stemmers`, `verbora-transliterators`** —
   ground rules in linguistic standards or published algorithms; drop tables
   whose only justification is an external implementation.
4. **`verbora-phonetics`** — every algorithm cites its publication. Do not
   reproduce another runtime's defects. One divergence is currently
   undocumented and must be either specified or removed: Metaphone emits
   `KSH` where the canonical algorithm emits `X` for `ch` (measured on 41 of
   649 corpus names). `PhoneticIndex` stays as a Verbora extension.
5. **Remaining crates** — `verbora-spellcheck`, `-trie`, `-tfidf`,
   `-wordnet`, `-tagger`, `-classifiers`, `-sentiment`, `-util`,
   `-language`, `-analyzers`.

## Performance baseline

Principle: measure before and after; introduce no gratuitous regression.

A full competitive measurement over the doubled case grid exists and serves
as the "before" baseline. Two lessons from producing it:

- **Doubling the grid paid off.** Of 97 size series, 56 had ratios that vary
  with size, including full crossovers that a coarser grid hid entirely. New
  input *shapes* (near-identical pairs, boundary density) exposed gaps that
  no additional size would have.
- **Uniform maximum precision did not.** 77% of cells have an unambiguous
  verdict (beyond 1.5× either way) that does not need Criterion's full
  sample count. A screening pass followed by full precision only on cells
  within 1.5× reaches the same conclusions in roughly a third of the time.
  Numbers that get published always come from the full-precision pass.

**Measurement happens once, after the code is settled — not per change.**
The migration deliberately breaks behaviour across many crates, so any
measurement taken mid-flight is invalidated by the next step. A full campaign
costs hours and a change costs minutes; benchmarking per change would spend
more time measuring than migrating. Benchmarks are therefore never launched
on the agent's own initiative: after a change lands, state that its
performance is unmeasured and ask whether to measure now or batch it. See
`CLAUDE.md` for the full rule.

Run benchmarks one target at a time. Never run them concurrently with builds
or other CPU-heavy work.

`scripts/competitive-benchmarks.sh` now enforces the first half of that rather
than merely stating it. It used to invoke `cargo bench --release` once over the
whole competitive crate under `set -euo pipefail`, so a single target that
failed to compile, panicked or crashed took the subshell down, took the script
down with `set -e`, and skipped both Verbora's own benches and the
structured-result collection — a run that had already measured fourteen targets
correctly produced nothing, for any of them. Each target is now invoked on its
own (`--bench <name>`, which isolates *compilation* too), each exit status is
captured instead of propagated, each target gets its own log under
`benchmarks/competitive/results/logs/`, and the run ends with a per-target
verdict — succeeded, skipped and why, failed and with which status — before
exiting non-zero if anything failed. A partial campaign reports as partial; it
is never silently a success and never discarded wholesale.

## Verified defects driving the `verbora-distance` contract

Reproduced from the current code before the contract was accepted, so the
design rests on measured behaviour rather than on assertion. Each must be
pinned by a test that fails against today's implementation.

| Defect | Reproduction |
|---|---|
| **Search can return a substring absent from the target.** The result is built by `String::from_utf16_lossy` over a slice of the UTF-16 buffer (`levenshtein.rs:1930`). When the optimal window starts or ends between the halves of a surrogate pair, the lone surrogate becomes `U+FFFD` — text the caller never supplied and the target does not contain. Both documented guarantees break at once. | 75 of 240 probes across all three `*_search` functions with `substitution_cost: 0.25`. `levenshtein_search("X", "😀ab")` returns `substring: "\u{FFFD}"`, `offset: 0`. Unit costs hide it only because the mid-pair alignment always ties with one that avoids it — tie-breaking luck, not a guarantee. |
| **`jaro` violates identity for single-unit inputs.** `jaro("a","a") == 0.0`. The matching window is `floor(max/2) - 1`, which is negative at length 1 and collapses to zero matches. | Direct call. `jaro("aa","aa") == 1.0`, so only length 1 is affected — and `jaro_winkler("a","a") == 1.0`, so the two functions openly disagree about the same pair. |
| **`dice_coefficient("", "")` returns `NaN`.** A `0/0` with no guard. `NaN` poisons comparisons and sorts silently rather than failing. | Direct call. |
| **An astral scalar costs two edits.** `levenshtein("", "😀") == 2` while `levenshtein("", "a") == 1`. Inserting one character is one edit under any reading a caller would recognise. | Direct call. This is the user-visible consequence of the inherited UTF-16 unit. |

The window-clamp interaction matters for step ordering: under the scalar unit
`"😀"` has length 1, so it would land on exactly the `jaro` identity bug above.
Changing the unit without fixing the window first would silently regress
`jaro("😀","😀")` from `1.0` to `0.0`.

## Remaining migration debt

Nothing is presently tracked here — the stemmer, phonetics and classifiers
rows this table used to carry are all closed now.

`verbora-stemmers` no longer indexes by UTF-16 code unit anywhere — every
stemmer measures in Unicode scalar values (see `units.rs`). `verbora-phonetics`
was re-verified rather than merely recounted: only **12** references to Apache
Commons Codec remain in the whole crate (10 in code and doc comments, 2 inside
test modules), not the 162 previously logged here, and every one of the twelve
is a legitimate standards citation or a candid note about a diffed oracle used
only during development — none asserts a value transcribed from a Java test
class. The violation this row used to flag no longer exists in the crate.

`verbora-classifiers`' `OrderedMap` no longer reproduces another runtime's
own-property enumeration order. A key takes the next free slot the first time
it is inserted and keeps it — integer-like or not — so adding a token to a
trained classifier appends rather than reshuffling: no previously learned
feature index can be shifted, and therefore silently corrupted, by vocabulary
growth (`src/ordmap.rs`). The one remaining way a fitted model's slots move is
`Classifier::remove_document`, which deletes matched tokens outright and
closes the resulting gap; that is a documented, deliberate behaviour, not
migration debt.

## Pending documentation corrections

Known-stale claims, verified but deliberately not yet fixed: both are about
surfaces this migration is still changing, so correcting them now would only
make them stale again. Fix them in the documentation pass that follows the
`verbora-distance` implementation.

| Page | Claim | Verified fact |
|---|---|---|
| `site/features/distance.md:18` | "150 unit tests and 9 doctests" | 161 unit tests, 28 integration (`tests/parallel.rs`, `--all-features`), 9 doctests. The panic, backtrack and case-folding tests landed after the count was written. |
| `site/performance/allocation.md:47` | The three `*_search` functions allocate a full cost matrix **and** a parent matrix, plus a `String` | True only off the fast path. Unit-cost plain `levenshtein_search` on non-empty operands takes `search_bits` (`levenshtein.rs:1955`) — per-column bit-vector deltas, no parent matrix. Both Damerau variants and every weighted cost set do use the full matrix. The same wrong claim appears in the "Choosing the right API" table on `site/features/distance.md`. |

## Verification

```
cargo test --workspace --all-features      # debug and release
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Plus the crate's own tests, relevant before/after benchmarks when a hot path
changes, and `npm run check` in `site/` when public documentation changes.

Residual external references are found with:

```
rg -n -i --glob '!graft/**' \
  '(the reference|reference implementation|reference engine|literal parity|PARITY_VERIFIED|lib/natural|natural npm)' .
```

Every match must be classifiable as: a competitive benchmark, a legitimate
standard or publication, or pending removal.

`graft/` is out of scope and must not be touched.
