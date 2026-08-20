//! Regression pins for the four defects that motivated the Rust-native
//! migration (`docs/design/distance-contract.md` §2.5).
//!
//! Each metric's own suite pins its arithmetic in general. What this file
//! pins is specifically the four answers that were *wrong before the
//! migration* — not because they are more important than the rest of the
//! contract, but because a regression on any of them would re-open a defect
//! the migration was undertaken to close, and a defect that is only implied
//! by a sweep is a defect that can be re-introduced by narrowing the sweep.
//!
//! Every test here is written so that it **fails against the pre-migration
//! code**, and each names the old answer alongside the new one. The new
//! answers come from the contract and from arithmetic shown in the comments;
//! none was read off a run.
//!
//! | Defect | Before | Under the contract |
//! |---|---|---|
//! | search fabricates text absent from the target | `substring == "\u{FFFD}"` | a borrowed slice of the target |
//! | `levenshtein("", "😀")` | `2` | `1` |
//! | `jaro("a", "a")` | `0.0` | `1.0` |
//! | `dice_coefficient("", "")` | `NaN` | `1.0` |

use verbora_distance::{
    LevenshteinCosts, damerau_levenshtein, dice_coefficient, hamming, jaro, jaro_winkler,
    levenshtein, levenshtein_search, levenshtein_search_weighted, osa,
};

// ---------------------------------------------------------------------------
// Defect 1 — search returned text that does not occur in the target
// ---------------------------------------------------------------------------

/// The search used to report its match as an owned `String` produced by
/// `String::from_utf16_lossy` over a UTF-16 span. When that span began or
/// ended between the halves of a surrogate pair, the orphaned half became
/// U+FFFD — a scalar that does not occur in the target at all, at a distance
/// the reported value did not describe.
///
/// The measured reproduction was `substitution: 0.25`, searching `"X"` in
/// `"😀ab"`: the answer was `substring: "\u{FFFD}"`. Unit costs hid it,
/// because the mid-pair alignment tied with one that avoided it.
///
/// The expected answer here is derived, not recorded. With
/// `insertion = deletion = 1.0` and `substitution = 0.25`, the substrings of
/// `"😀ab"` score, against the one-scalar source `"X"`:
///
/// ```text
///   ""     -> 1.0    (one deletion)
///   "😀"   -> 0.25   (one substitution)
///   "😀a"  -> 1.25   (substitute, then insert)
///   "😀ab" -> 2.25
///   "a"    -> 0.25
///   "ab"   -> 1.25
///   "b"    -> 0.25
/// ```
///
/// The minimum is `0.25`. The earliest scalar position at which it is
/// attained is 1 — position 0 offers only `""` at `1.0` — and at that
/// position the only substring realising it is `"😀"` starting at 0. `"😀"`
/// is four UTF-8 bytes, so the byte range is `0..4`.
#[test]
fn search_returns_a_borrowed_slice_of_the_target_not_a_replacement_character() {
    let costs = LevenshteinCosts::new(1.0, 1.0, 0.25).expect("admissible");
    let target = "😀ab";
    let got = levenshtein_search_weighted("X", target, &costs);

    assert_eq!(got.distance(), 0.25);
    assert_eq!(got.substring(), "😀");
    assert_eq!(got.range(), 0..4);
    // The two clauses the old answer violated, stated on their own.
    assert_eq!(&target[got.range()], got.substring());
    assert!(target.contains(got.substring()));
    assert!(!got.substring().contains('\u{FFFD}'));

    // Tier 3 of §2.5, at unit costs: `levenshtein_search("😀", "😁")` used to
    // report substring `"\u{FFFD}"` at distance 1, a string that occurs
    // nowhere in the target and is in fact 2 away from the source. Under the
    // contract the substrings of `"😁"` are `""` and `"😁"`, both 1 away, so
    // the minimum is 1 and the earliest end position — the empty substring
    // at column 0 — takes it.
    let got = levenshtein_search("😀", "😁");
    assert_eq!(got.distance(), 1);
    assert_eq!(got.substring(), "");
    assert_eq!(got.range(), 0..0);
    assert!("😁".contains(got.substring()));

    // Tier 1, the widest form and not an astral one: the reported offset was
    // a UTF-16 index documented as a `&str` index. "Zürich, " is
    // Z(1) ü(2) r(1) i(1) c(1) h(1) ,(1) ␠(1) = 9 UTF-8 bytes but only 8
    // UTF-16 code units, so the old answer was 8 — which is a character
    // boundary, so it sliced cleanly to the wrong text instead of panicking.
    let target = "Zürich, Berlin, Wien";
    let got = levenshtein_search("Berlin", target);
    assert_eq!(got.range(), 9..15);
    assert_eq!(&target[got.range()], "Berlin");
}

// ---------------------------------------------------------------------------
// Defect 2 — an astral scalar counted as two units
// ---------------------------------------------------------------------------

/// `levenshtein("", "😀")` was `2`, one per UTF-16 code unit. One scalar is
/// one unit (§2.1), so inserting one emoji is one edit.
///
/// The whole of §2.5's astral table is asserted with it: every row is a case
/// where the old unit gave an answer the definition forbids, most starkly
/// `hamming("😀", "𝕳") == 2` — a distance of 2 between two one-character
/// strings, which exceeds the operand length.
#[test]
fn one_astral_scalar_is_one_unit() {
    // Was 2.
    assert_eq!(levenshtein("", "😀"), 1);
    assert_eq!(levenshtein("😀", ""), 1);
    // Was 2.0: deleting one emoji is one edit, not one per surrogate.
    assert_eq!(levenshtein("a😀b", "ab"), 1);
    // Was 2.0: an adjacent swap of two whole scalars is one transposition.
    assert_eq!(osa("😀😁", "😁😀"), 1);
    assert_eq!(damerau_levenshtein("😀😁", "😁😀"), 1);
    // Was 2: three UTF-16 units against two, and a distance exceeding both.
    assert_eq!(hamming("😀", "𝕳"), Some(1));
    // Was Some(2): one scalar against two is not a Hamming-comparable pair.
    assert_eq!(hamming("😀", "ab"), None);
    // Was 0.6666…, identical to `jaro("北京", "南京")` — two emoji sharing a
    // high surrogate were indistinguishable from two CJK words sharing a
    // real character. They now share nothing.
    assert_eq!(jaro("😀", "😁"), 0.0);
    // Was 0.5: `"😀😁"` has one bigram and `"😀"` has none, so
    // `2 · 0 / (1 + 0)` is 0.
    assert_eq!(dice_coefficient("😀😁", "😀"), 0.0);
    // Basic Multilingual Plane results are unchanged, which is the other
    // half of the claim: only astral input moves.
    assert_eq!(levenshtein("北京", "南京"), 1);
    assert_eq!(levenshtein("café", "cafe"), 1);
}

// ---------------------------------------------------------------------------
// Defect 3 — Jaro contradicted its own identity axiom
// ---------------------------------------------------------------------------

/// `jaro("a", "a")` was `0.0`: the match window `floor(max/2) - 1` evaluated
/// to `-1` for one-unit operands and pruned the single candidate pair at
/// displacement 0, so `m` was 0 and the `m == 0` clause fired. Meanwhile
/// `jaro_winkler("a", "a")` was `1.0`, via an equality short-circuit at the
/// top of that function — so the two contradicted each other about their own
/// identity element.
///
/// Clamping the window at 0 (§3.4) fixes the class, and the short-circuit is
/// deleted rather than kept: `jaro_winkler` reaches `1.0` through the
/// formula, `sim_j + l · p · (1 - sim_j)` with `sim_j == 1.0`, so the boost
/// term is exactly `l · p · 0.0 == 0.0`.
#[test]
fn jaro_identity_holds_for_single_unit_operands() {
    // Was 0.0. `(1/1 + 1/1 + (1 - 0)/1) / 3` is `3.0 / 3.0`, exactly 1.0 in
    // IEEE-754.
    assert_eq!(jaro("a", "a").to_bits(), 1.0f64.to_bits());
    assert_eq!(jaro_winkler("a", "a").to_bits(), 1.0f64.to_bits());
    // The two agree now, which is the property that was broken.
    assert_eq!(jaro("a", "a").to_bits(), jaro_winkler("a", "a").to_bits());
    // The clamp does not manufacture matches: distinct one-unit operands
    // still have `m == 0`.
    assert_eq!(jaro("a", "b"), 0.0);
    assert_eq!(jaro_winkler("a", "b"), 0.0);
    // Under the scalar unit an astral character is one unit, so it lands in
    // exactly this class — and would have regressed from 1.0 to 0.0 without
    // the clamp, since it used to be two UTF-16 units.
    assert_eq!(jaro("😀", "😀").to_bits(), 1.0f64.to_bits());
    assert_eq!(jaro_winkler("😀", "😀").to_bits(), 1.0f64.to_bits());
    // Operands of two units or more never entered the broken branch, so
    // `floor(max/2) - 1 >= 0` there and nothing moved.
    assert_eq!(jaro("ab", "ab").to_bits(), 1.0f64.to_bits());
}

// ---------------------------------------------------------------------------
// Defect 4 — Dice returned NaN
// ---------------------------------------------------------------------------

/// `dice_coefficient("", "")` was `NaN`: neither operand has a bigram, so the
/// implementation evaluated `2 · 0 / (0 + 0)`. `NaN` is the worst possible
/// sentinel — it is not orderable, so `max_by` over a candidate list gives
/// order-dependent results and one poisoned score corrupts a ranking with no
/// visible failure.
///
/// The `|A| + |B| == 0` branch is now taken *before* the division (§3.5):
/// identical operands score `1.0`, everything else `0.0`. The defect was
/// wider than the empty pair — the deleted `sanitize` step trimmed and
/// collapsed whitespace first, so several whitespace-only pairs reached the
/// same division.
#[test]
fn dice_is_total_where_it_used_to_return_nan() {
    for (a, b, want) in [
        // Was NaN. Two empty strings are *identical*, not disjoint.
        ("", "", 1.0f64),
        // All NaN before, via `sanitize` stripping them to empty first.
        (" ", "\t", 0.0),
        ("\u{FEFF}", " ", 0.0),
        ("  ", "\n\n", 0.0),
        ("\u{0085}", "", 0.0),
        // One-scalar operands: no bigram either, and the same branch.
        ("a", "a", 1.0),
        ("a", "b", 0.0),
        ("", "a", 0.0),
        ("😀", "😀", 1.0),
    ] {
        let got = dice_coefficient(a, b);
        assert!(got.is_finite(), "dice({a:?}, {b:?}) = {got} is not finite");
        assert!(
            (0.0..=1.0).contains(&got),
            "dice({a:?}, {b:?}) = {got} is outside [0, 1]"
        );
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "dice({a:?}, {b:?}) = {got}, expected {want}"
        );
    }

    // The property `NaN` destroyed, restated as the reason the branch exists:
    // a ranking over these scores is order-independent. With a `NaN` in the
    // list, `max_by` would return whichever element the comparison order
    // happened to leave standing.
    let corpus = ["", " ", "\t", "a", "ab", "😀"];
    let mut scores: Vec<f64> = corpus.iter().map(|c| dice_coefficient("", c)).collect();
    assert!(scores.iter().all(|s| s.is_finite()));
    scores.sort_by(f64::total_cmp);
    assert_eq!(scores.first().copied(), Some(0.0));
    assert_eq!(scores.last().copied(), Some(1.0));
}
