//! String distance and similarity metrics for Rust.
//!
//! String distance and similarity across four algorithm families: the
//! Levenshtein family (Levenshtein, unrestricted Damerau–Levenshtein and
//! optimal string alignment, each in a unit-cost and a weighted form),
//! Hamming, Jaro and Jaro–Winkler, and Dice.
//!
//! ```
//! use verbora_distance::{levenshtein, jaro_winkler, dice_coefficient, hamming};
//!
//! assert_eq!(levenshtein("kitten", "sitting"), 3);
//! assert_eq!(dice_coefficient("abc", "abc"), 1.0);
//! assert_eq!(hamming("karolin", "kathrin"), Some(3));
//! assert_eq!(jaro_winkler("abc", "abc"), 1.0);
//! ```
//!
//! # Conventions differ between metrics
//!
//! The metrics deliberately do not share a single direction convention, and this crate does
//! not "fix" that, since doing so would change every caller's results:
//!
//! | Metric | Range | Direction |
//! |--------|-------|-----------|
//! | [`fn@levenshtein`], [`fn@damerau_levenshtein`], [`fn@osa`] | `0..` (`usize`) | distance — lower is closer |
//! | [`fn@levenshtein_weighted`] and its two siblings | `0.0..` | distance — lower is closer |
//! | [`fn@hamming`] | `Option<usize>` | distance — lower is closer; `None` means the two operands hold a different number of Unicode scalars, so the distance is undefined |
//! | [`fn@jaro`], [`fn@jaro_winkler`] | `0..=1` | similarity — higher is closer |
//! | [`dice_coefficient`] | `0..=1` | similarity — higher is closer |
//!
//! Direction is recorded by the name of the function a caller calls, not by a
//! trait: code that ranks candidates picks `min` for a distance and `max` for
//! a similarity at the call site, where the metric is already chosen.
//!
//! # What the similarities guarantee
//!
//! [`dice_coefficient`], [`jaro`] and [`fn@jaro_winkler`] are **similarities**
//! — higher is closer — and for every input, including empty and single-unit
//! operands, each returns a value that is
//!
//! 1. **total** — a finite `f64`, never `NaN`, never infinite;
//! 2. **in range** — within `0.0..=1.0` inclusive;
//! 3. **an identity** — `f(x, x) == 1.0` bit-exactly, so `score == 1.0` is a
//!    sound test to write. For [`jaro`] and [`fn@jaro_winkler`] it holds *only*
//!    for equal operands; for [`dice_coefficient`] it does not imply equality,
//!    since bigrams form a set — see *Identity is not injective* on that
//!    function;
//! 4. **symmetric** — `f(a, b)` and `f(b, a)` are bit-identical.
//!
//! Guarantee 1 is why each of them decides its degenerate cases *before* the
//! division that would otherwise evaluate `0 / 0`, rather than after it. `NaN`
//! is the worst possible sentinel: it is not orderable, so `max_by` over a
//! candidate list gives order-dependent results and a single `NaN` poisons a
//! ranking with no visible failure.
//!
//! The degenerate cases follow from one sentence — identical inputs score
//! `1.0`, inputs sharing no feature score `0.0` — and all three agree on them:
//!
//! | Input | [`dice_coefficient`] | [`jaro`] | [`fn@jaro_winkler`] |
//! |---|---|---|---|
//! | `("", "")` | `1.0` | `1.0` | `1.0` |
//! | `("", "a")` / `("a", "")` | `0.0` | `0.0` | `0.0` |
//! | `("a", "a")` | `1.0` | `1.0` | `1.0` |
//! | `("a", "b")` | `0.0` | `0.0` | `0.0` |
//!
//! The distances answer their own degenerate cases on their own terms:
//! [`fn@hamming`] returns `None` when the operands' scalar counts differ
//! rather than carving a sentinel out of the numeric range, while the
//! Levenshtein family measures an empty operand against the other one's whole
//! length — as a count of edits in the unit-cost tier, and as the price of
//! that many insertions or deletions in the weighted tier.
//!
//! # Purity, and what that means for `#[must_use]`
//!
//! Every function and method this crate publishes is pure: it reads its
//! arguments, mutates nothing, and the value it returns is the entire reason
//! to call it. Dropping that value is therefore always a mistake and never a
//! side effect somebody wanted — so **every one of them carries
//! `#[must_use]`**, with no per-function judgement about which results are
//! "important enough".
//!
//! There is exactly one exception, and it is mechanical rather than a
//! judgement: [`LevenshteinCosts::new`], [`OsaCosts::new`] and
//! [`DamerauCosts::new`] return [`Result`], which is itself `#[must_use]`, so
//! the diagnostic already fires and repeating the attribute would be
//! redundant — `clippy::double_must_use` rejects it. The rule to apply when
//! adding a function here is therefore: annotate it, unless its return type
//! is already `#[must_use]`.
//!
//! # Unicode
//!
//! **One Unicode scalar value (`char`) is one unit.** Every count these
//! metrics report is a count of scalars, so `"a😀b"` is three units and this
//! crate ships no length function — `s.chars().count()` is that function.
//! Positions are the one exception: [`SearchResult::range`] is a **byte**
//! range, because a byte range is what indexes a `&str`, and every scalar
//! boundary is a byte boundary. An internal dispatch keeps the choice free on
//! ordinary input: for ASCII operands one byte is one scalar exactly, so the
//! fast path is the same computation on a narrower type.
//!
//! **No metric here rewrites its inputs** — none folds case, trims, collapses
//! whitespace or normalises — so none consults a Unicode character database,
//! and this crate has no dependency that does. Results are therefore frozen
//! across Unicode versions, which matters for anything that persists
//! distances or distance-derived keys. Caseless or whitespace-insensitive
//! matching is the caller's, applied once at ingestion rather than re-applied
//! against every candidate.
//!
//! # The Levenshtein family
//!
//! Three algorithms, each in a unit-cost and a weighted tier, and each of
//! those in a "distance" and a "search" flavour:
//!
//! | Algorithm | Unit cost | Weighted | Transposition rule |
//! |-----------|-----------|----------|--------------------|
//! | Levenshtein | [`fn@levenshtein`] / [`levenshtein_search`] | [`levenshtein_weighted`] / [`levenshtein_search_weighted`] | none — a swap costs two edits |
//! | unrestricted Damerau–Levenshtein (Lowrance–Wagner) | [`damerau_levenshtein`] / [`damerau_levenshtein_search`] | [`damerau_levenshtein_weighted`] / [`damerau_levenshtein_search_weighted`] | two characters adjacent *in the source* may be swapped, and the substring between them may itself be edited |
//! | optimal string alignment (restricted Damerau) | [`osa`] / [`osa_search`] | [`osa_weighted`] / [`osa_search_weighted`] | a swap is a single operation, but no substring may be edited between two swaps |
//!
//! **The algorithm is chosen by the function you call, never by a flag.** The
//! two Damerau families answer genuinely different questions — `"CA"` →
//! `"ABC"` is 2 under [`damerau_levenshtein`] and 3 under [`osa`] — so they
//! are separate entry points rather than one function with a mode switch.
//!
//! Both Damerau variants are **symmetric**: `d(a, b) == d(b, a)` under unit
//! costs. Unrestricted Damerau–Levenshtein is additionally a true metric (it
//! satisfies the triangle inequality); OSA is not, since it forbids editing
//! between two swaps. Each is pinned by differential tests against a
//! from-scratch reference implementation transcribed from the published
//! recurrence, plus property tests for symmetry and the triangle inequality.
//!
//! The search flavour reports *where* in a larger text the best match sits;
//! see [`SearchResult`] for what it guarantees and for the tie-breaking that
//! fixes which of several equally-cheap spans is returned.
//!
//! # Choosing the Right API
//!
//! Three choices arise in this crate, and none of them is a choice between a
//! fast API and a correct one.
//!
//! ## Unit costs or weighted costs
//!
//! Each of the three Levenshtein-family algorithms is published twice: a
//! **unit-cost** function that takes no cost argument and returns `usize`, and
//! a **weighted** function that takes a validated cost set and returns `f64`.
//!
//! ### The conceptual difference
//!
//! Unit cost is not a cost *value* here; it is the absence of an argument.
//! There is no way to spell "unit costs" as a value, because the unit metric
//! is a different function: a different kernel (bit-parallel rather than a
//! scalar dynamic program), a different complexity class, and a different
//! result type. That is the same lesson [`osa`] already applies to the
//! *algorithm* — it is a function name rather than a `restricted: bool` — now
//! applied to the cost set.
//!
//! The boundary is **unit versus weighted**, never integer versus float:
//! `substitution: 2.0` is exactly as slow as `substitution: 0.5`, because the
//! bit-parallel kernels have no notion of a weighted operation at all.
//!
//! ### Comparison
//!
//! | | [`fn@levenshtein`], [`damerau_levenshtein`], [`osa`] | [`levenshtein_weighted`], [`damerau_levenshtein_weighted`], [`osa_weighted`] |
//! |---|---|---|
//! | Question answered | how many edits separate the two strings | the minimum total cost of an edit script under caller-assigned prices |
//! | Cost argument | none | [`LevenshteinCosts`] / [`DamerauCosts`] / [`OsaCosts`], each built through a `new` returning [`Result`] |
//! | Returns | `usize` — exact, [`Ord`], [`Hash`](core::hash::Hash), no rounding, usable as a map key | `f64` |
//! | Kernel | bit-parallel: Myers (1999), Hyyrö (2003), Zhao–Sahni (2020) | scalar dynamic program (one row, three rows, or a full matrix) |
//! | Common-affix trimming | yes — a pair differing in one interior position collapses to `O(1)` kernel work | no; the reduction's proof assumes a unit-cost matrix |
//! | [`PreparedPattern`] | yes | no — the prepared table is a bit-parallel structure |
//! | Allocation | none for ASCII operands up to 64 units; packed match-table rows past that | one `Vec<f64>` row (plain), three (OSA), a full `(n+1) × (m+1)` matrix (unrestricted Damerau) |
//! | Can the caller get it wrong? | no — there is no argument to get wrong | an inadmissible cost set is rejected by the constructor, before any metric sees it |
//!
//! ### Examples
//!
//! ```
//! use verbora_distance::{LevenshteinCosts, levenshtein, levenshtein_weighted};
//!
//! // Unit cost: no argument, exact integer answer.
//! assert_eq!(levenshtein("kitten", "sitting"), 3);
//!
//! // Weighted: a deletion is priced at three insertions.
//! let costs = LevenshteinCosts::new(1.0, 3.0, 1.0)?;
//! assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);
//!
//! // The two tiers agree where they overlap.
//! let unit = LevenshteinCosts::new(1.0, 1.0, 1.0)?;
//! assert_eq!(
//!     levenshtein("kitten", "sitting") as f64,
//!     levenshtein_weighted("kitten", "sitting", &unit)
//! );
//! # Ok::<(), verbora_distance::CostError>(())
//! ```
//!
//! ### Decision tree
//!
//! ```text
//! Do the edit operations have genuinely different prices?
//! ├── no  → the unit form: levenshtein / damerau_levenshtein / osa
//! │        (the right choice for the large majority of programs)
//! └── yes → does a transposition count as one operation?
//!           ├── no                       → levenshtein_weighted
//!           ├── yes, and a swapped pair
//!           │   may be edited again      → damerau_levenshtein_weighted
//!           └── yes, but a swapped pair
//!               is then frozen           → osa_weighted
//!
//! Do you need *where* in a larger text the match sits, rather than the
//! distance between two whole strings?
//! └── yes → the matching *_search function, same two tiers.
//! ```
//!
//! ### Benchmarks
//!
//! `UNMEASURED`: no timing of the split between the two tiers is published,
//! and none may be quoted until a full-precision run exists. The structural
//! difference that is not in doubt: the weighted tier evaluates `Θ(n·m)`
//! scalar cells, the unit tier `Θ(n·⌈m/64⌉)` machine words over a pair whose
//! common prefix and suffix have already been trimmed away.
//!
//! ### Recommendation
//!
//! Reach for the unit form unless you can name the prices. It is faster, it
//! cannot fail to construct, and its `usize` result is exact — so it can key
//! a `HashMap`, seed a BK-tree edge, or be compared with `==` without any of
//! the questions a floating-point distance raises. The weighted form exists
//! because it answers a question the unit form cannot, not because it is a
//! more general spelling of the same thing.
//!
//! Moving a caller from one to the other changes the return type, which is a
//! mechanical but pervasive edit. That cost is deliberate, and it is pinned
//! that the two agree: `levenshtein(a, b) as f64 ==
//! levenshtein_weighted(a, b, &LevenshteinCosts::new(1.0, 1.0, 1.0)?)`.
//!
//! ## One comparison, or one pattern against many
//!
//! ### The conceptual difference
//!
//! Every metric here is a free function over two strings, which is the right
//! shape for one comparison and the wrong shape for screening a query against
//! a candidate set: the Levenshtein family's bit-parallel kernels rebuild the
//! same pattern-match table on every call, so a fixed query pays for its own
//! length once per candidate. [`PreparedPattern`] builds that table once and
//! answers [`levenshtein`](PreparedPattern::levenshtein) and
//! [`osa`](PreparedPattern::osa) from it.
//!
//! The two are not two algorithms. They are the same kernels reached two ways,
//! and they return the same `usize` for the same operands — the type exists
//! only to move the table build *out* of the loop. So the question is never
//! "which is more accurate" or even "which is faster", but **how many
//! comparisons share one operand**. A pattern compared once has nothing to
//! hoist: the table would be built, used once and dropped, which is exactly
//! what [`fn@levenshtein`] already does, minus a `String` clone. A pattern
//! compared against a candidate set amortizes that build over the whole set.
//! The crossover is structural, not tuned — the build is `O(pattern)` and
//! happens once instead of `N` times.
//!
//! Two things fall outside the choice entirely, because there is no prepared
//! form to choose: **weighted costs** and **unrestricted
//! Damerau–Levenshtein**. Neither absence is a missing feature; see
//! [`PreparedPattern`] for why each is structural.
//!
//! ### Comparison
//!
//! | | [`fn@levenshtein`] / [`osa`] | [`PreparedPattern`] |
//! |---|---|---|
//! | Question answered | the distance between *these two* strings | the distance from *this fixed* string to each of many |
//! | Metrics | all six: three algorithms × unit and weighted | two: unit-cost Levenshtein and OSA |
//! | Cost paid once | none | one table build plus one owned copy of the pattern |
//! | Cost per comparison | kernel, plus a table build for patterns over 4 units | kernel only |
//! | State to manage | none | one value to keep alive per fixed pattern |
//! | Allocation | none for ASCII operands up to 64 units | ~2 KB inline for an ASCII pattern up to 64 units; heap past that or for non-ASCII |
//! | Concurrency | pure functions | `&self`, never mutated — share one across threads by plain `&` |
//! | Answers | — | identical, by construction: every case the table cannot serve calls the per-call function verbatim |
//!
//! ### Examples
//!
//! ```
//! use verbora_distance::PreparedPattern;
//! use verbora_distance::levenshtein;
//!
//! let candidates = ["Jonathon", "Nathan", "Johnson", "Jon"];
//!
//! // One pair: the free function. Nothing to amortize.
//! let one = levenshtein("Jonathan", "Jonathon");
//!
//! // One query, many candidates: prepare it once, then screen.
//! let query = PreparedPattern::new("Jonathan");
//! let within_two: Vec<&str> = candidates
//!     .into_iter()
//!     .filter(|c| query.levenshtein(c) <= 2)
//!     .collect();
//!
//! assert_eq!(one, 1);
//! assert_eq!(within_two, ["Jonathon"]);
//!
//! // And the two agree everywhere, so a rewrite in either direction is safe.
//! for c in candidates {
//!     assert_eq!(query.levenshtein(c), levenshtein(query.pattern(), c));
//! }
//! ```
//!
//! ### Decision tree
//!
//! ```text
//! Do you need weighted costs, or unrestricted Damerau–Levenshtein?
//! ├── yes → the per-call function. There is no prepared form of either,
//! │         and the reason is structural rather than a missing feature.
//! └── no  → is one operand fixed across many comparisons?
//!           ├── no  → the per-call function
//!           │         (the right choice for the large majority of programs:
//!           │          one pair, one call, no state to hold)
//!           └── yes → how many candidates does it face?
//!                     ├── one or two → the per-call function; the build
//!                     │                would not pay for itself
//!                     └── a set      → PreparedPattern::new once, then
//!                                      one call per candidate
//! ```
//!
//! Two refinements once you are on the prepared branch, neither of which can
//! make an answer wrong — both only decide whether the table gets used:
//!
//! * **An ASCII pattern cannot serve a non-ASCII target**, so a candidate set
//!   of mixed script gets the per-call path for its non-ASCII members. A
//!   non-ASCII *pattern* has no such limit; its table is built over scalars
//!   and serves every target.
//! * **Candidates that are near-duplicates of the pattern** (shared prefixes
//!   and suffixes — `"Alexander"` versus `"Alexandre"`) are better served by
//!   the per-call path's common-affix trim than by any table, and the query
//!   path routes them there itself. Preparing still costs the build; it just
//!   buys less.
//!
//! ### Benchmarks
//!
//! `UNMEASURED`: no timing of the two shapes against each other is published,
//! and none may be quoted until a full-precision run exists. What is not in
//! doubt is the structural difference, which is a count and not a measurement:
//! screening `N` candidates builds `N` tables through the per-call functions
//! and one through a prepared pattern.
//!
//! ### Recommendation
//!
//! Reach for [`fn@levenshtein`] and [`osa`] first. They are the whole API for
//! one comparison, they cover all three algorithms and both cost tiers, and
//! they hold no state — a program that compares pairs should never grow a
//! [`PreparedPattern`] to do it.
//!
//! Reach for [`PreparedPattern`] when the shape of the *workload* is
//! one-against-many and you can name the fixed operand: a spellchecker's
//! query term, a dictionary headword, a name being screened against a
//! sanctions list. Build it where that operand is decided, hold it for as
//! long as the operand lives, and share it across threads by `&`. Because
//! every fallback is literally the call you would otherwise have written,
//! adopting it is never a correctness risk and never the slower choice for
//! the workload it is meant for — but it is state, and state you do not need
//! is not free to a reader.
//!
//! ### Prepared state is not a scratch buffer
//!
//! Two different things are easy to conflate, and only one of them is offered
//! here:
//!
//! * **Prepared immutable state** — memory derived from *one* operand, valid
//!   for every query against it, and never written during a query. The
//!   pattern-match table is exactly that, which is why [`PreparedPattern`] can
//!   be `&self`, shared across threads, and reused without any reset step.
//! * **A scratch buffer** — mutable working memory a caller lends the
//!   algorithm for the duration of one call, so the allocator is not asked
//!   again on the next one. That is the dynamic-programming working set, it
//!   depends on *both* operands, and **no API in this crate takes one**.
//!   Queries still build their own, per call.
//!
//! ## Jaro or Jaro–Winkler
//!
//! ### The conceptual difference
//!
//! [`jaro`] scores agreement between two strings as a whole.
//! [`fn@jaro_winkler`] is that score pulled toward `1.0` in proportion to how
//! much of the *beginning* the two strings share — Winkler's empirical
//! observation from census record linkage that human transcription errors
//! cluster toward the end of a field, so an agreeing prefix is stronger
//! evidence of a match than an agreeing suffix.
//!
//! That makes the choice a question about your data, not about speed or
//! precision. Both functions run the same matching pass over the same
//! operands; the boost is one multiply-add on top.
//!
//! ### Comparison
//!
//! | | [`jaro`] | [`fn@jaro_winkler`] |
//! |---|---|---|
//! | Question answered | how much do these two strings agree | …weighted toward agreement at the start |
//! | Definition | Jaro (1989) | Winkler (1990) over Jaro (1989) |
//! | Range | `0.0..=1.0` | `0.0..=1.0` (affine interpolation between `sim_j` and `1.0` with weight `l · p <= 0.4`, so no clamp is applied or needed) |
//! | Extra cost over [`jaro`] | — | one common-prefix scan bounded at 4 units, plus one multiply-add |
//! | Ordering | a plain agreement ranking | re-ranks prefix-sharing pairs above equally-agreeing pairs that differ early |
//! | Symmetric | yes | yes |
//!
//! ### Examples
//!
//! ```
//! use verbora_distance::{jaro, jaro_winkler};
//!
//! // Same Jaro score, different Jaro–Winkler score: the boost is what
//! // separates a shared prefix from a shared suffix.
//! let head = jaro("abcde", "abcdz"); // differs at the end
//! let tail = jaro("abcde", "zbcde"); // differs at the start
//! assert_eq!(head, tail);
//! assert!(jaro_winkler("abcde", "abcdz") > jaro_winkler("abcde", "zbcde"));
//!
//! // With no shared prefix the boost term is zero and the two agree exactly.
//! assert_eq!(jaro_winkler("abcd", "zbcd"), jaro("abcd", "zbcd"));
//! ```
//!
//! ### Decision tree
//!
//! ```text
//! Are your strings fields where errors cluster away from the start —
//! personal names, place names, titles, product names?
//! ├── yes → jaro_winkler   (what Winkler designed it for)
//! └── no  → is the start of the string more meaningful than the rest?
//!           ├── yes → jaro_winkler
//!           └── no  → jaro   (identifiers, codes, hashes, reversed keys:
//!                             an agreeing prefix carries no extra evidence,
//!                             so do not pay for a boost you did not want)
//! ```
//!
//! ### Benchmarks
//!
//! `UNMEASURED`: no timing of the boost against plain [`jaro`] is published,
//! and none may be quoted until a full-precision run exists. Structurally the
//! boost is the prefix scan and the multiply-add in the table above, on top of
//! a matching pass the two share exactly.
//!
//! ### Recommendation
//!
//! [`fn@jaro_winkler`] is the usual choice for fuzzy *name* matching, which is
//! the workload both papers were written for. Reach for plain [`jaro`] when a
//! shared prefix is not evidence — and note that whichever you pick, neither
//! is a distance: the derived `1 − score` violates the triangle inequality,
//! so neither can seed a BK-tree. Use [`fn@levenshtein`] or
//! [`damerau_levenshtein`] where a true metric is required.
//!
//! # Batch computation (feature = `parallel`)
//!
//! Every metric above is a pure, stateless free function, so scoring many
//! independent pairs is embarrassingly parallel with zero coordination cost.
//! With the `parallel` feature enabled, [`par_levenshtein_batch`],
//! [`par_damerau_levenshtein_batch`], [`par_osa_batch`],
//! [`par_jaro_winkler_batch`], [`par_dice_coefficient_batch`] and
//! [`par_hamming_batch`] fan a batch of pairs out across a `rayon` thread
//! pool. Each is exactly
//! `pairs.par_iter().map(<the sequential function>).collect()` — see the
//! individual function docs for cost trade-offs and when a plain sequential
//! loop is the better choice (usually: for small batches or short strings).

mod dice;
mod hamming;
mod jaro_winkler;
mod levenshtein;
mod prepared;
mod units;

pub use dice::dice_coefficient;
#[cfg(feature = "parallel")]
pub use dice::par_dice_coefficient_batch;
pub use hamming::hamming;
#[cfg(feature = "parallel")]
pub use hamming::par_hamming_batch;
#[cfg(feature = "parallel")]
pub use jaro_winkler::par_jaro_winkler_batch;
pub use jaro_winkler::{jaro, jaro_winkler};
pub use levenshtein::{
    CostError, DamerauCosts, LevenshteinCosts, Operation, OsaCosts, SearchResult,
    damerau_levenshtein, damerau_levenshtein_search, damerau_levenshtein_search_weighted,
    damerau_levenshtein_weighted, levenshtein, levenshtein_search, levenshtein_search_weighted,
    levenshtein_weighted, osa, osa_search, osa_search_weighted, osa_weighted,
};
#[cfg(feature = "parallel")]
pub use levenshtein::{par_damerau_levenshtein_batch, par_levenshtein_batch, par_osa_batch};
pub use prepared::PreparedPattern;
