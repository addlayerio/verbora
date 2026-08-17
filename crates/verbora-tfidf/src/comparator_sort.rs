//! `Array.prototype.sort`, reproduced exactly — because `listTerms` sorts with
//! a comparator that is not a valid total order, and the result is observable.
//!
//! # Why this module exists
//!
//! ```text
//! return terms.sort(function (x, y) { return y.tfidf - x.tfidf })
//! ```
//!
//! For finite scores that is an ordinary stable descending sort and any stable
//! algorithm reproduces it. But `tfidf` is `NaN` whenever the term frequency is
//! not a number — a deserialized corpus with a string count, or a term named
//! after an `Object.prototype` method on a prototype-backed idf cache — and
//! `NaN - x` is `NaN`, which the specification treats as `+0`, i.e. "equal".
//! The induced relation then stops being transitive, and the output becomes a
//! property of *the reference engine's specific algorithm* rather than of "a stable sort".
//!
//! The fixtures caught it immediately. A four-term document scoring
//! `[0, -2.81, NaN, 1.405]` in `for…in` order comes out of the reference engine as
//! `[1.405, 0, -2.81, NaN]` — the reference engine's binary insertion sort finds the right home
//! for the last element by *binary* search, jumping straight over the `NaN` —
//! while Rust's stable sort, which walks left one step at a time, stops at the
//! `NaN` and leaves the array untouched.
//!
//! # Provenance
//!
//! This is a copy of `verbora-spellcheck`'s `comparator_sort`, which was validated
//! against the reference engine itself over 1,015 recorded permutations spanning every length
//! class and NaN density. It is duplicated rather than shared because that
//! module is crate-private and the two crates have no dependency relationship;
//! if a common home is ever created, both should move there.
//!
//! # What is reproduced
//!
//! The reference engine's `ArrayTimSort` (`builtins/array-sort.tq`), itself a port of Tim Peters'
//! list sort: natural run detection with in-place reversal of descending runs,
//! binary insertion sort up to `minRunLength`, a pending-run stack collapsed
//! under the corrected invariant, and galloping merges with the adaptive
//! `minGallop` threshold carried across merges — plus the short-array
//! short-circuit described at [`MIN_TIM_SORT`].
//!
//! Two details exist *only* because comparators can be inconsistent, and both
//! are load-bearing here: [`Sorter::merge_at`] returning early when a gallop
//! consumes an entire run, and the `length == 0` checks inside the merge gallop
//! loops. A port that "tidies" those away loops forever on the NaN inputs.

/// The consecutive-win count at which the reference engine switches a merge to galloping.
const MIN_GALLOP_WINS: usize = 7;

/// The length at or above which `minRunLength` starts halving.
const MIN_MERGE: usize = 64;

/// The array length below which the reference engine skips run detection entirely.
///
/// # Why this is not in the published algorithm
///
/// Tim Peters' list sort — and the `array-sort.tq` pseudocode it is written
/// from — has no such case: it always calls `CountAndMakeRun` first and then
/// extends the run to `minRunLength` with a binary insertion sort. For a
/// **consistent** comparator the two are indistinguishable, because a natural
/// run really is already sorted and re-inserting its elements cannot move them.
/// Under the reference's comparator they are *not* indistinguishable, and the
/// fixtures caught it:
///
/// ```text
/// frequencies [NaN, 1, NaN, 3]
///   run detection first   [0, 1, 2, 3]   one ascending run of 4, nothing to do
///   the reference engine  [0, 3, 1, 2]
/// ```
///
/// `NaN > x` and `x > NaN` are both false, so the comparator calls every
/// adjacent pair "ascending" and `CountAndMakeRun` swallows the whole array —
/// while a binary insertion sort still moves element 3 in front of element 1,
/// because `3 > 1` genuinely holds. The two paths therefore disagree, and the
/// recorded answer is the insertion sort's.
///
/// # How the boundary was established
///
/// By recording the reference engine's *comparator call sequence*, not just its output: an
/// instrumented comparator makes the algorithm directly observable. Against a
/// the reference transcription of the model, over ten comparators (consistent,
/// never-zero, always-equal, NaN-returning) and lengths 0–300, the model with
/// this short-circuit reproduces the reference engine's calls and its permutation exactly on
/// 45,150 randomised inputs and on all 87,381 inputs of length ≤ 8 over
/// `{NaN, 0, 1, 2}`. Moving the boundary to 7 or 9 breaks every trial at
/// length 7 or 8 respectively, so it is exactly 8.
///
/// The short-circuit applies to the **whole array**, once, and not to a short
/// trailing run inside the loop — a distinction that only shows up when a long
/// natural run leaves fewer than 8 elements behind, and which was checked
/// separately (880 crafted inputs, zero divergence).
const MIN_TIM_SORT: usize = 8;

/// Sorts `work` with `compare`, exactly as the reference engine's `Array.prototype.sort` would.
///
/// `compare` returns the reference comparator's number. Only the predicate
/// `result < 0` is ever consulted, which is precisely what the reference engine does, so an
/// inconsistent comparator yields the same permutation here as there.
pub(crate) fn sort_by<T: Copy, C: FnMut(&T, &T) -> i32>(work: &mut [T], compare: C) {
    if work.len() < 2 {
        return;
    }
    Sorter {
        work,
        temp: Vec::new(),
        min_gallop: MIN_GALLOP_WINS,
        pending: Vec::new(),
        cmp: compare,
    }
    .run();
}

/// A run on the pending stack: `(base, length)`.
type Run = (usize, usize);

struct Sorter<'a, T, C> {
    work: &'a mut [T],
    /// Scratch holding the smaller side of a merge; reused across merges,
    /// exactly as the reference engine's `tempArray` is.
    temp: Vec<T>,
    /// Adaptive galloping threshold. Persists across merges within one sort,
    /// which is why it lives here rather than in a merge local.
    min_gallop: usize,
    pending: Vec<Run>,
    cmp: C,
}

/// Which of the reference's two `goto` labels a merge loop jumped to.
enum MergeExit {
    /// The remainder of the run held in `temp` still has to be copied back.
    Succeed,
    /// Exactly one element of the run left in place remains; it belongs at the
    /// far end of the merged range.
    CopyOther,
}

impl<T: Copy, C: FnMut(&T, &T) -> i32> Sorter<'_, T, C> {
    #[inline]
    fn lt(&mut self, a: &T, b: &T) -> bool {
        (self.cmp)(a, b) < 0
    }

    fn run(&mut self) {
        let length = self.work.len();

        // Short arrays never reach TimSort proper; see `MIN_TIM_SORT` for why
        // that is observable rather than an optimisation detail.
        if length < MIN_TIM_SORT {
            self.binary_insertion_sort(0, 1, length);
            return;
        }

        let mut remaining = length;
        let mut low = 0usize;
        let min_run_length = compute_min_run_length(remaining);

        while remaining != 0 {
            let mut current = self.count_and_make_run(low, low + remaining);
            if current < min_run_length {
                let forced = min_run_length.min(remaining);
                self.binary_insertion_sort(low, low + current, low + forced);
                current = forced;
            }
            self.pending.push((low, current));
            self.merge_collapse();
            low += current;
            remaining -= current;
        }
        self.merge_force_collapse();
    }

    /// Finds — and, if descending, reverses in place — the natural run at `low`.
    ///
    /// A port that skips the descending case still sorts, but it hands the merge
    /// stack a different set of runs, and with an inconsistent comparator a
    /// different set of runs means a different permutation.
    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run_length = 2usize;

        let element_low = self.work[low];
        let element_low_pred = self.work[low - 1];
        // "Descending" is in comparator terms: the second element sorts first.
        let is_descending = self.lt(&element_low, &element_low_pred);

        let mut previous = element_low;
        for idx in (low + 1)..high {
            let current = self.work[idx];
            let less = self.lt(&current, &previous);
            if is_descending {
                if !less {
                    break;
                }
            } else if less {
                break;
            }
            previous = current;
            run_length += 1;
        }

        if is_descending {
            self.work[low_arg..low_arg + run_length].reverse();
        }
        run_length
    }

    /// Extends the sorted prefix `[low, start)` to cover `[low, high)`.
    fn binary_insertion_sort(&mut self, low: usize, start_arg: usize, high: usize) {
        let mut start = if low == start_arg {
            start_arg + 1
        } else {
            start_arg
        };
        while start < high {
            let mut left = low;
            let mut right = start;
            let pivot = self.work[right];
            while left < right {
                let mid = left + ((right - left) >> 1);
                let m = self.work[mid];
                // Only `pivot < work[mid]` sends the pivot left. A tie — and,
                // under the reference comparator, *every* non-greater pair —
                // sends it right, which is what makes the sort stable.
                if self.lt(&pivot, &m) {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.work.copy_within(left..start, left + 1);
            self.work[left] = pivot;
            start += 1;
        }
    }

    fn run_len(&self, n: usize) -> usize {
        self.pending[n].1
    }

    /// `true` iff `len(n-2) > len(n-1) + len(n)`; vacuously true for `n < 2`.
    fn invariant_established(&self, n: usize) -> bool {
        if n < 2 {
            return true;
        }
        self.run_len(n - 2) > self.run_len(n - 1) + self.run_len(n)
    }

    fn merge_collapse(&mut self) {
        while self.pending.len() > 1 {
            let mut n = self.pending.len() - 2;
            if !self.invariant_established(n + 1) || !self.invariant_established(n) {
                if n > 0 && self.run_len(n - 1) < self.run_len(n + 1) {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.run_len(n) <= self.run_len(n + 1) {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    fn merge_force_collapse(&mut self) {
        while self.pending.len() > 1 {
            let mut n = self.pending.len() - 2;
            if n > 0 && self.run_len(n - 1) < self.run_len(n + 1) {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    fn merge_at(&mut self, i: usize) {
        let (mut base_a, mut length_a) = self.pending[i];
        let (base_b, length_b) = self.pending[i + 1];

        self.pending[i] = (base_a, length_a + length_b);
        // `merge_at` is only ever called with `i` at the top two positions, so
        // closing the gap means moving at most one entry down.
        if i + 3 == self.pending.len() {
            self.pending[i + 1] = self.pending[i + 2];
        }
        self.pending.pop();

        // The prefix of A that already precedes everything in B needs no merging.
        let key_right = self.work[base_b];
        let k = self.gallop_right_work(&key_right, base_a, length_a, 0);
        base_a += k;
        length_a -= k;
        // Unreachable with a consistent comparator; reachable here.
        if length_a == 0 {
            return;
        }

        // Symmetrically, the suffix of B that follows all of A can stay put.
        let key_left = self.work[base_a + length_a - 1];
        let length_b = self.gallop_left_work(&key_left, base_b, length_b, length_b - 1);
        if length_b == 0 {
            return;
        }

        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    // --- gallop wrappers ---------------------------------------------------
    //
    // Both merge directions gallop over both the work array and `temp`, so each
    // combination gets a thin wrapper. Splitting the comparator out of `self`
    // this way is what keeps the field borrows disjoint.

    fn gallop_left_work(&mut self, key: &T, base: usize, length: usize, hint: usize) -> usize {
        gallop_left(&mut self.cmp, self.work, key, base, length, hint)
    }

    fn gallop_right_work(&mut self, key: &T, base: usize, length: usize, hint: usize) -> usize {
        gallop_right(&mut self.cmp, self.work, key, base, length, hint)
    }

    fn gallop_left_temp(&mut self, key: &T, base: usize, length: usize, hint: usize) -> usize {
        gallop_left(&mut self.cmp, &self.temp, key, base, length, hint)
    }

    fn gallop_right_temp(&mut self, key: &T, base: usize, length: usize, hint: usize) -> usize {
        gallop_right(&mut self.cmp, &self.temp, key, base, length, hint)
    }

    /// Merges when the left run is the shorter one: A is copied out to `temp`
    /// and the result is written forwards from `base_a`.
    fn merge_low(
        &mut self,
        base_a: usize,
        length_a_arg: usize,
        base_b: usize,
        length_b_arg: usize,
    ) {
        let mut length_a = length_a_arg;
        let mut length_b = length_b_arg;

        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_a..base_a + length_a]);

        let mut dest = base_a;
        let mut cursor_temp = 0usize;
        let mut cursor_b = base_b;

        let first = self.work[cursor_b];
        self.work[dest] = first;
        dest += 1;
        cursor_b += 1;
        length_b -= 1;

        let exit = if length_b == 0 {
            MergeExit::Succeed
        } else if length_a == 1 {
            MergeExit::CopyOther
        } else {
            self.merge_low_loop(
                &mut dest,
                &mut cursor_temp,
                &mut cursor_b,
                &mut length_a,
                &mut length_b,
            )
        };

        match exit {
            MergeExit::Succeed => {
                self.work[dest..dest + length_a]
                    .copy_from_slice(&self.temp[cursor_temp..cursor_temp + length_a]);
            }
            MergeExit::CopyOther => {
                // One element of A is left, and it belongs after all of B.
                self.work.copy_within(cursor_b..cursor_b + length_b, dest);
                self.work[dest + length_b] = self.temp[cursor_temp];
            }
        }
    }

    fn merge_low_loop(
        &mut self,
        dest: &mut usize,
        cursor_temp: &mut usize,
        cursor_b: &mut usize,
        length_a: &mut usize,
        length_b: &mut usize,
    ) -> MergeExit {
        let mut min_gallop = self.min_gallop;
        loop {
            // Phase 1: element-at-a-time, until one run wins `min_gallop` times
            // consecutively.
            let mut wins_a = 0usize;
            let mut wins_b = 0usize;
            loop {
                let b = self.work[*cursor_b];
                let a = self.temp[*cursor_temp];
                if self.lt(&b, &a) {
                    self.work[*dest] = b;
                    *dest += 1;
                    *cursor_b += 1;
                    *length_b -= 1;
                    wins_b += 1;
                    wins_a = 0;
                    if *length_b == 0 {
                        return MergeExit::Succeed;
                    }
                    if wins_b >= min_gallop {
                        break;
                    }
                } else {
                    self.work[*dest] = a;
                    *dest += 1;
                    *cursor_temp += 1;
                    *length_a -= 1;
                    wins_a += 1;
                    wins_b = 0;
                    if *length_a == 1 {
                        return MergeExit::CopyOther;
                    }
                    if wins_a >= min_gallop {
                        break;
                    }
                }
            }

            // Phase 2: gallop for as long as it keeps paying off.
            min_gallop += 1;
            let mut first_pass = true;
            while first_pass || wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS {
                first_pass = false;
                min_gallop = min_gallop.saturating_sub(1).max(1);
                self.min_gallop = min_gallop;

                let key = self.work[*cursor_b];
                wins_a = self.gallop_right_temp(&key, *cursor_temp, *length_a, 0);
                if wins_a != 0 {
                    let src = *cursor_temp;
                    self.work[*dest..*dest + wins_a].copy_from_slice(&self.temp[src..src + wins_a]);
                    *dest += wins_a;
                    *cursor_temp += wins_a;
                    *length_a -= wins_a;
                    if *length_a == 1 {
                        return MergeExit::CopyOther;
                    }
                    // Only reachable with an inconsistent comparator — which is
                    // exactly the case this module exists for.
                    if *length_a == 0 {
                        return MergeExit::Succeed;
                    }
                }
                let b = self.work[*cursor_b];
                self.work[*dest] = b;
                *dest += 1;
                *cursor_b += 1;
                *length_b -= 1;
                if *length_b == 0 {
                    return MergeExit::Succeed;
                }

                let key = self.temp[*cursor_temp];
                wins_b = self.gallop_left_work(&key, *cursor_b, *length_b, 0);
                if wins_b != 0 {
                    self.work.copy_within(*cursor_b..*cursor_b + wins_b, *dest);
                    *dest += wins_b;
                    *cursor_b += wins_b;
                    *length_b -= wins_b;
                    if *length_b == 0 {
                        return MergeExit::Succeed;
                    }
                }
                self.work[*dest] = self.temp[*cursor_temp];
                *dest += 1;
                *cursor_temp += 1;
                *length_a -= 1;
                if *length_a == 1 {
                    return MergeExit::CopyOther;
                }
            }
            min_gallop += 1;
            self.min_gallop = min_gallop;
        }
    }

    /// Merges when the right run is the shorter one: B is copied out to `temp`
    /// and the result is written *backwards* from the end of B.
    fn merge_high(
        &mut self,
        base_a: usize,
        length_a_arg: usize,
        base_b: usize,
        length_b_arg: usize,
    ) {
        let mut length_a = length_a_arg;
        let mut length_b = length_b_arg;

        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_b..base_b + length_b]);

        // The cursors run downwards and the last write can land on index 0,
        // after which a cursor legitimately becomes -1: hence `isize`.
        let mut dest = (base_b + length_b - 1) as isize;
        let mut cursor_a = (base_a + length_a - 1) as isize;
        let mut cursor_temp = (length_b - 1) as isize;

        let last = self.work[cursor_a as usize];
        self.work[dest as usize] = last;
        dest -= 1;
        cursor_a -= 1;
        length_a -= 1;

        let exit = if length_a == 0 {
            MergeExit::Succeed
        } else if length_b == 1 {
            MergeExit::CopyOther
        } else {
            self.merge_high_loop(
                &mut dest,
                &mut cursor_a,
                &mut cursor_temp,
                &mut length_a,
                &mut length_b,
                base_a,
            )
        };

        match exit {
            MergeExit::Succeed => {
                let start = (dest + 1) as usize - length_b;
                self.work[start..start + length_b].copy_from_slice(&self.temp[..length_b]);
            }
            MergeExit::CopyOther => {
                // One element of B is left, and it belongs before all of A.
                let src = (cursor_a + 1) as usize - length_a;
                let dst = (dest + 1) as usize - length_a;
                self.work.copy_within(src..src + length_a, dst);
                self.work[dst - 1] = self.temp[cursor_temp as usize];
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_high_loop(
        &mut self,
        dest: &mut isize,
        cursor_a: &mut isize,
        cursor_temp: &mut isize,
        length_a: &mut usize,
        length_b: &mut usize,
        base_a: usize,
    ) -> MergeExit {
        let mut min_gallop = self.min_gallop;
        loop {
            let mut wins_a = 0usize;
            let mut wins_b = 0usize;
            loop {
                let b = self.temp[*cursor_temp as usize];
                let a = self.work[*cursor_a as usize];
                if self.lt(&b, &a) {
                    self.work[*dest as usize] = a;
                    *dest -= 1;
                    *cursor_a -= 1;
                    *length_a -= 1;
                    wins_a += 1;
                    wins_b = 0;
                    if *length_a == 0 {
                        return MergeExit::Succeed;
                    }
                    if wins_a >= min_gallop {
                        break;
                    }
                } else {
                    self.work[*dest as usize] = b;
                    *dest -= 1;
                    *cursor_temp -= 1;
                    *length_b -= 1;
                    wins_b += 1;
                    wins_a = 0;
                    if *length_b == 1 {
                        return MergeExit::CopyOther;
                    }
                    if wins_b >= min_gallop {
                        break;
                    }
                }
            }

            min_gallop += 1;
            let mut first_pass = true;
            while first_pass || wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS {
                first_pass = false;
                min_gallop = min_gallop.saturating_sub(1).max(1);
                self.min_gallop = min_gallop;

                let key = self.temp[*cursor_temp as usize];
                let k = self.gallop_right_work(&key, base_a, *length_a, *length_a - 1);
                wins_a = *length_a - k;
                if wins_a != 0 {
                    *dest -= wins_a as isize;
                    *cursor_a -= wins_a as isize;
                    let src = (*cursor_a + 1) as usize;
                    let dst = (*dest + 1) as usize;
                    self.work.copy_within(src..src + wins_a, dst);
                    *length_a -= wins_a;
                    if *length_a == 0 {
                        return MergeExit::Succeed;
                    }
                }
                self.work[*dest as usize] = self.temp[*cursor_temp as usize];
                *dest -= 1;
                *cursor_temp -= 1;
                *length_b -= 1;
                if *length_b == 1 {
                    return MergeExit::CopyOther;
                }

                let key = self.work[*cursor_a as usize];
                let k = self.gallop_left_temp(&key, 0, *length_b, *length_b - 1);
                wins_b = *length_b - k;
                if wins_b != 0 {
                    *dest -= wins_b as isize;
                    *cursor_temp -= wins_b as isize;
                    let src = (*cursor_temp + 1) as usize;
                    let dst = (*dest + 1) as usize;
                    self.work[dst..dst + wins_b].copy_from_slice(&self.temp[src..src + wins_b]);
                    *length_b -= wins_b;
                    if *length_b == 1 {
                        return MergeExit::CopyOther;
                    }
                    if *length_b == 0 {
                        return MergeExit::Succeed;
                    }
                }
                let a = self.work[*cursor_a as usize];
                self.work[*dest as usize] = a;
                *dest -= 1;
                *cursor_a -= 1;
                *length_a -= 1;
                if *length_a == 0 {
                    return MergeExit::Succeed;
                }
            }
            min_gallop += 1;
            self.min_gallop = min_gallop;
        }
    }
}

/// `minRunLength`: the top six bits of `n`, rounded up if any lower bit was set.
fn compute_min_run_length(n_arg: usize) -> usize {
    let mut n = n_arg;
    let mut r = 0usize;
    while n >= MIN_MERGE {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

// The two gallops carry a genuinely signed intermediate: when the leftward
// search runs off the front, `lastOfs` is `hint - (hint + 1) == -1`, and the
// `++lastOfs` that follows brings it back to 0. The reference's C and Torque
// both rely on that, so the offsets here are `isize` rather than `usize`.

/// Leftmost index in `array[base..base + length]` at which `key` could be
/// inserted while keeping the range sorted — i.e. *before* any equal elements.
fn gallop_left<T: Copy, C: FnMut(&T, &T) -> i32>(
    cmp: &mut C,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint_arg: usize,
) -> usize {
    let length = length as isize;
    let hint = hint_arg as isize;
    let at = |i: isize| base + i as usize;

    let mut last_ofs: isize = 0;
    let mut ofs: isize = 1;

    if cmp(&array[at(hint)], key) < 0 {
        // array[hint] < key: search rightwards in doubling steps.
        let max_ofs = length - hint;
        while ofs < max_ofs {
            if cmp(&array[at(hint + ofs)], key) < 0 {
                last_ofs = ofs;
                ofs = ofs.saturating_mul(2).saturating_add(1);
            } else {
                break;
            }
        }
        if ofs > max_ofs {
            ofs = max_ofs;
        }
        last_ofs += hint;
        ofs += hint;
    } else {
        // key <= array[hint]: search leftwards.
        let max_ofs = hint + 1;
        while ofs < max_ofs {
            if cmp(&array[at(hint - ofs)], key) < 0 {
                break;
            }
            last_ofs = ofs;
            ofs = ofs.saturating_mul(2).saturating_add(1);
        }
        if ofs > max_ofs {
            ofs = max_ofs;
        }
        let tmp = last_ofs;
        last_ofs = hint - ofs;
        ofs = hint - tmp;
    }

    last_ofs += 1;
    while last_ofs < ofs {
        let m = last_ofs + ((ofs - last_ofs) >> 1);
        if cmp(&array[at(m)], key) < 0 {
            last_ofs = m + 1;
        } else {
            ofs = m;
        }
    }
    ofs as usize
}

/// Rightmost such index: `key` is placed *after* any equal elements.
fn gallop_right<T: Copy, C: FnMut(&T, &T) -> i32>(
    cmp: &mut C,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint_arg: usize,
) -> usize {
    let length = length as isize;
    let hint = hint_arg as isize;
    let at = |i: isize| base + i as usize;

    let mut last_ofs: isize = 0;
    let mut ofs: isize = 1;

    if cmp(key, &array[at(hint)]) < 0 {
        // key < array[hint]: search leftwards.
        let max_ofs = hint + 1;
        while ofs < max_ofs {
            if cmp(key, &array[at(hint - ofs)]) < 0 {
                last_ofs = ofs;
                ofs = ofs.saturating_mul(2).saturating_add(1);
            } else {
                break;
            }
        }
        if ofs > max_ofs {
            ofs = max_ofs;
        }
        let tmp = last_ofs;
        last_ofs = hint - ofs;
        ofs = hint - tmp;
    } else {
        // array[hint] <= key: search rightwards.
        let max_ofs = length - hint;
        while ofs < max_ofs {
            if cmp(key, &array[at(hint + ofs)]) < 0 {
                break;
            }
            last_ofs = ofs;
            ofs = ofs.saturating_mul(2).saturating_add(1);
        }
        if ofs > max_ofs {
            ofs = max_ofs;
        }
        last_ofs += hint;
        ofs += hint;
    }

    last_ofs += 1;
    while last_ofs < ofs {
        let m = last_ofs + ((ofs - last_ofs) >> 1);
        if cmp(key, &array[at(m)]) < 0 {
            ofs = m;
        } else {
            last_ofs = m + 1;
        }
    }
    ofs as usize
}

#[cfg(test)]
mod tests {
    use super::sort_by;

    /// `listTerms`' comparator, reduced to the only predicate the reference engine consults.
    fn value_cmp(a: &(u32, f64), b: &(u32, f64)) -> i32 {
        if b.1 - a.1 < 0.0 { -1 } else { 1 }
    }

    fn order(scores: &[f64]) -> Vec<u32> {
        let mut v: Vec<(u32, f64)> = scores
            .iter()
            .enumerate()
            .map(|(i, s)| (u32::try_from(i).unwrap(), *s))
            .collect();
        sort_by(&mut v, value_cmp);
        v.into_iter().map(|(i, _)| i).collect()
    }

    #[test]
    fn empty_and_single() {
        assert!(order(&[]).is_empty());
        assert_eq!(order(&[5.0]), [0]);
    }

    #[test]
    fn sorts_descending_and_stably() {
        assert_eq!(order(&[3.0, 2.0, 3.0, 1.0, 3.0]), [0, 2, 4, 1, 3]);
    }

    #[test]
    fn equal_scores_keep_the_for_in_order() {
        let scores = vec![7.0; 200];
        let want: Vec<u32> = (0..200).collect();
        assert_eq!(order(&scores), want);
    }

    /// The exact case that made Rust's own stable sort diverge: the recorded
    /// answer for `{x: 0, y: -2, z: 'three', w: 1}` is `[w, x, y, z]`.
    #[test]
    fn a_nan_score_does_not_block_a_later_element() {
        assert_eq!(
            order(&[
                0.0,
                -2.810_930_216_216_328_8,
                f64::NAN,
                1.405_465_108_108_164_4
            ]),
            [3, 0, 1, 2]
        );
    }

    #[test]
    fn long_inputs_with_nan_terminate() {
        // The merge paths that only an inconsistent comparator can reach.
        let scores: Vec<f64> = (0..500)
            .map(|i| {
                if i % 3 == 0 {
                    f64::NAN
                } else {
                    f64::from(i % 11)
                }
            })
            .collect();
        assert_eq!(order(&scores).len(), 500);
    }
}
