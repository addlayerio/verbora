//! The indexed fast path behind [`Spellcheck::get_corrections`] at
//! `max_distance <= 2`: a hashed SymSpell-style deletion index plus exact
//! reconstruction of the reference's output order.
//!
//! [`Spellcheck::get_corrections`]: crate::Spellcheck::get_corrections
//!
//! # Why this exists
//!
//! The combinatorial search in `Spellcheck::corrections_over` pays for
//! *generating* the candidate stream — ~`54n + 25` strings per level, squared
//! at distance 2 — before any membership test runs. Measured on the shared
//! benchmark corpus, generation plus its dedup set alone is ~58% of a 5 ms
//! distance-2 call, while the same query resolves to a handful of stored words
//! under deletion indexing. This module removes that floor for
//! `max_distance <= 2` without changing one byte of output; `>= 3` keeps the
//! combinatorial path.
//!
//! # Parity strategy
//!
//! * **Retrieval**: depth-≤ 2 deletion sequences over UTF-16 code units,
//!   keyed by a 64-bit `FxHash` of the sequence — the deletion string itself
//!   is never materialised. A hash collision can only *add* candidates, never
//!   remove one, and every candidate is verified exactly below, so collisions
//!   cannot affect output.
//! * **Level membership**: a candidate is emitted at level 1 iff one
//!   *generator* edit (deletion / adjacent transposition / `a`–`z`
//!   substitution / `a`–`z` insertion, including the degenerate
//!   end-transposition that reproduces the word) maps query → word
//!   ([`dist1_minpos`]); at level 2 iff two sequential generator edits reach
//!   it and one does not ([`dist2_minkey`]).
//! * **In-level ordering**: descending frequency, ties by first occurrence in
//!   the generator's edit stream — reconstructed from closed forms for the
//!   *raw* (pre-dedup) stream position of each producing edit. Dedup keeps
//!   first occurrences in raw-position order, so raw positions are a valid
//!   sort key, and for finite frequencies the reference's comparator is
//!   exactly a stable descending sort.
//! * **Dispatch guards** (handled by the caller in `spellcheck.rs`):
//!   `max_distance >= 3` after the 0 → 1 falsiness mapping, or any retrieved
//!   candidate with a `NaN` (prototype-shadowed) frequency — where the exact
//!   permutation is the reference engine's TimSort on the full duplicate
//!   stream — falls back to the combinatorial path unchanged.
//!
//! Differentially verified byte-exact (ordering included) against the
//! combinatorial path: exhaustive small alphabets, randomized real-corpus
//! cases at several sizes, Cyrillic, astral/emoji, `NaN`-frequency corpora and
//! 500-character words — see the tests here and in `spellcheck.rs`.

use rustc_hash::FxHashMap;
use std::hash::Hasher;

/// Sentinel index for "skip nothing" in [`del_hash`].
const NO_SKIP: usize = usize::MAX;

/// Hashes `units` with the elements at `skip1` and `skip2` removed, without
/// materialising the deletion.
#[inline]
fn del_hash(units: &[u16], skip1: usize, skip2: usize) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    for (idx, &u) in units.iter().enumerate() {
        if idx != skip1 && idx != skip2 {
            h.write_u16(u);
        }
    }
    h.finish()
}

/// Raw-stream position bases: split position `i` starts at `base(i)` in the
/// raw (pre-dedup) edit stream. `i == 0` emits only the 26 inserts; every
/// later split emits delete, transpose, then 26 replace/insert pairs.
#[inline]
fn base(i: usize) -> u64 {
    if i == 0 { 0 } else { 26 + 54 * (i as u64 - 1) }
}

/// The alphabet index of `u`, if it is a lowercase ASCII letter.
#[inline]
fn letter_k(u: u16) -> Option<u64> {
    if (u16::from(b'a')..=u16::from(b'z')).contains(&u) {
        Some(u64::from(u) - u64::from(b'a'))
    } else {
        None
    }
}

/// Length of the longest common prefix of `a` and `b`.
#[inline]
fn lcp(a: &[u16], b: &[u16]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Length of the longest common suffix of `a` and `b`.
#[inline]
fn lcs(a: &[u16], b: &[u16]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Minimum raw position at which one generator edit of `q` produces `w`, or
/// `None` if `w` is not one generator edit away.
pub(crate) fn dist1_minpos(q: &[u16], w: &[u16]) -> Option<u64> {
    let n = q.len();
    let m = w.len();
    if m + 1 == n {
        // Deletion at 0-based j: q[..j] == w[..j] && q[j+1..] == w[j..].
        let p = lcp(q, w);
        let s = lcs(q, w);
        let jmin = (n - 1).saturating_sub(s);
        if jmin <= p {
            return Some(base(jmin + 1)); // D(jmin + 1)
        }
        None
    } else if m == n {
        if n == 0 {
            return None; // edits("") is only the 26 inserts.
        }
        if q == w {
            // Earliest of: equal-pair transposition, same-letter substitution,
            // degenerate end transposition.
            for i in 1..n {
                if q[i - 1] == q[i] {
                    return Some(base(i) + 1); // T(i)
                }
                if let Some(k) = letter_k(q[i - 1]) {
                    return Some(base(i) + 2 + 2 * k); // S(i, q[i-1])
                }
            }
            // i == n: the degenerate transpose beats S(n, .).
            return Some(base(n) + 1);
        }
        let p = lcp(q, w);
        let s = lcs(q, w);
        if p + s >= n - 1 {
            // Exactly one mismatching position, at p: substitution.
            let k = letter_k(w[p])?;
            return Some(base(p + 1) + 2 + 2 * k); // S(p+1, w[p])
        }
        if p + s == n - 2 && q[p] == w[p + 1] && q[p + 1] == w[p] {
            return Some(base(p + 1) + 1); // T(p+1)
        }
        None
    } else if m == n + 1 {
        // Insertion before 0-based j: q[..j] == w[..j] && q[j..] == w[j+1..].
        let p = lcp(q, w);
        let s = lcs(q, w);
        let jmin = n.saturating_sub(s);
        if jmin <= p {
            let k = letter_k(w[jmin])?;
            return Some(if jmin == 0 { k } else { base(jmin) + 3 + 2 * k }); // I(jmin, w[jmin])
        }
        None
    } else {
        None
    }
}

/// Minimum lexicographic `(raw position of op1 in edits(q), raw position of
/// op2 in edits(op1(q)))` over all two-edit decompositions reaching `w`.
///
/// Call only when [`dist1_minpos`]`(q, w)` is `None`; targets one edit away
/// are emitted at level 1 and never ranked at level 2. `s` is a scratch
/// buffer, reused across calls.
pub(crate) fn dist2_minkey(q: &[u16], w: &[u16], s: &mut Vec<u16>) -> Option<(u64, u64)> {
    let n = q.len();
    let m = w.len();
    if n.abs_diff(m) > 2 {
        return None;
    }
    // The one cancellation case that is not one edit away: the empty query
    // reaching itself (insert a letter, delete it again). op1 = I(0,'a') at
    // raw position 0; op2 deletes the sole unit at raw position base(1).
    if n == 0 && m == 0 {
        return Some((0, base(1)));
    }

    s.clear();
    s.reserve(n + 1);
    // i = 0: inserts only, raw position k. The inserted letter must survive
    // into w (an undone insert reproduces q, one edit away at most), so it is
    // one of w's first two units.
    {
        let mut ks: [u64; 2] = [u64::MAX; 2];
        let mut nk = 0usize;
        for wi in w.iter().take(2) {
            if let Some(k) = letter_k(*wi) {
                if !ks[..nk].contains(&k) {
                    ks[nk] = k;
                    nk += 1;
                }
            }
        }
        ks[..nk].sort_unstable();
        for &k in &ks[..nk] {
            s.clear();
            s.push(u16::from(b'a') + k as u16);
            s.extend_from_slice(q);
            if let Some(p2) = dist1_minpos(s, w) {
                return Some((k, p2));
            }
        }
    }
    for i in 1..=n {
        // D(i)
        s.clear();
        s.extend_from_slice(&q[..i - 1]);
        s.extend_from_slice(&q[i..]);
        if let Some(p2) = dist1_minpos(s, w) {
            return Some((base(i), p2));
        }
        // T(i), non-degenerate, non-identity.
        if i < n && q[i - 1] != q[i] {
            s.clear();
            s.extend_from_slice(&q[..i - 1]);
            s.push(q[i]);
            s.push(q[i - 1]);
            s.extend_from_slice(&q[i + 1..]);
            if let Some(p2) = dist1_minpos(s, w) {
                return Some((base(i) + 1, p2));
            }
        }
        // Substitutions / insertions at split i. The introduced letter must
        // survive into w: for S(i, c) (changes q[i-1]) it ends at one of
        // w[i-2..=i]; for I(i, c) (unit lands at index i) at one of
        // w[i-1..=i+1]. Try k ascending, S before I at each k.
        // Union of the S window {w[i-2..=i]} and I window {w[i-1..=i+1]}:
        // at most 4 distinct letters, gathered and sorted in place.
        let mut ks: [(u64, bool, bool); 4] = [(u64::MAX, false, false); 4];
        let mut nk = 0usize;
        {
            let push_k = |k: u64, is_s: bool, arr: &mut [(u64, bool, bool); 4], nk: &mut usize| {
                for slot in arr[..*nk].iter_mut() {
                    if slot.0 == k {
                        if is_s {
                            slot.1 = true;
                        } else {
                            slot.2 = true;
                        }
                        return;
                    }
                }
                arr[*nk] = (k, is_s, !is_s);
                *nk += 1;
            };
            for wi in i.saturating_sub(2)..=i {
                if let Some(&u) = w.get(wi) {
                    if let Some(k) = letter_k(u) {
                        push_k(k, true, &mut ks, &mut nk);
                    }
                }
            }
            for wi in i.saturating_sub(1)..=(i + 1) {
                if let Some(&u) = w.get(wi) {
                    if let Some(k) = letter_k(u) {
                        push_k(k, false, &mut ks, &mut nk);
                    }
                }
            }
        }
        ks[..nk].sort_unstable();
        for &(k, try_s, try_i) in &ks[..nk] {
            let c = u16::from(b'a') + k as u16;
            if try_s && q[i - 1] != c {
                s.clear();
                s.extend_from_slice(&q[..i - 1]);
                s.push(c);
                s.extend_from_slice(&q[i..]);
                if let Some(p2) = dist1_minpos(s, w) {
                    return Some((base(i) + 2 + 2 * k, p2));
                }
            }
            if try_i {
                s.clear();
                s.extend_from_slice(&q[..i]);
                s.push(c);
                s.extend_from_slice(&q[i..]);
                if let Some(p2) = dist1_minpos(s, w) {
                    return Some((base(i) + 3 + 2 * k, p2));
                }
            }
        }
    }
    None
}

/// The lazily built retrieval structure: every word's depth-≤ 2 deletion
/// sequences, hashed, mapping back to entry indices.
///
/// Indices align with `Spellcheck::entries`; the frequency and the word text
/// stay there, so this holds only what retrieval and verification need.
#[derive(Debug)]
pub(crate) struct QueryIndex {
    /// Deletion-sequence hash → indices of the entries that share it.
    del: FxHashMap<u64, Vec<u32>>,
    /// Each entry's word as UTF-16 code units, aligned with the entry list.
    words_units: Vec<Box<[u16]>>,
}

impl QueryIndex {
    /// Builds the index over `words`, one per distinct entry, in entry order.
    pub(crate) fn new<'a, I>(words: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let words_units: Vec<Box<[u16]>> = words
            .into_iter()
            .map(|w| w.encode_utf16().collect::<Vec<u16>>().into_boxed_slice())
            .collect();

        let mut del: FxHashMap<u64, Vec<u32>> = FxHashMap::default();
        for (idx, wu) in words_units.iter().enumerate() {
            let idx = u32::try_from(idx).expect("word list exceeds u32 entries");
            let n = wu.len();
            // Distinct deletion sequences of one word can collide on the same
            // hash (identical letters, or a genuine hash collision); the tail
            // check keeps each key's vector free of adjacent repeats of this
            // word without a per-key set — one word only ever appends to a
            // key's vector while it is that vector's most recent writer.
            let mut push = |h: u64| {
                let v: &mut Vec<u32> = del.entry(h).or_default();
                if v.last() != Some(&idx) {
                    v.push(idx);
                }
            };
            push(del_hash(wu, NO_SKIP, NO_SKIP));
            for i in 0..n {
                push(del_hash(wu, i, NO_SKIP));
            }
            for i in 0..n {
                for j in i + 1..n {
                    push(del_hash(wu, i, j));
                }
            }
        }

        Self { del, words_units }
    }

    /// Collects into `out` the (sorted, deduplicated) indices of every entry
    /// sharing a depth-≤ `depth` deletion sequence with `query`.
    pub(crate) fn candidates(&self, query: &[u16], depth: u32, out: &mut Vec<u32>) {
        out.clear();
        let n = query.len();
        let look = |h: u64, out: &mut Vec<u32>| {
            if let Some(v) = self.del.get(&h) {
                out.extend_from_slice(v);
            }
        };
        look(del_hash(query, NO_SKIP, NO_SKIP), out);
        for i in 0..n {
            look(del_hash(query, i, NO_SKIP), out);
        }
        if depth >= 2 {
            for i in 0..n {
                for j in i + 1..n {
                    look(del_hash(query, i, j), out);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// The UTF-16 units of entry `index`'s word.
    pub(crate) fn word_units(&self, index: u32) -> &[u16] {
        &self.words_units[index as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The raw, pre-dedup edit stream of `q`, generated directly from the
    /// reference's rules: for each split `i` in `0..=len` — delete (skipped at
    /// `i == 0`), transpose (skipped at `i == 0`, degenerate at `i == len`),
    /// then for each letter `a`–`z` a replace (skipped at `i == 0`) and an
    /// insert. This is the ground truth the closed forms in [`base`],
    /// [`dist1_minpos`] and [`dist2_minkey`] reconstruct positions of.
    fn raw_edits(q: &[u16]) -> Vec<Vec<u16>> {
        let n = q.len();
        let mut out: Vec<Vec<u16>> = Vec::with_capacity(26 + 54 * n);
        for i in 0..=n {
            if i > 0 {
                // Delete: q[..i-1] + q[i..]
                let mut v = q[..i - 1].to_vec();
                v.extend_from_slice(&q[i..]);
                out.push(v);
                // Transpose: q[..i-1] + q[i..min(i+1,n)] + q[i-1..i] + q[min(i+1,n)..]
                let mut v = q[..i - 1].to_vec();
                v.extend_from_slice(&q[i..(i + 1).min(n)]);
                v.extend_from_slice(&q[i - 1..i]);
                v.extend_from_slice(&q[(i + 1).min(n)..]);
                out.push(v);
            }
            for c in b'a'..=b'z' {
                let c = u16::from(c);
                if i > 0 {
                    // Replace: q[..i-1] + c + q[i..]
                    let mut v = q[..i - 1].to_vec();
                    v.push(c);
                    v.extend_from_slice(&q[i..]);
                    out.push(v);
                }
                // Insert: q[..i] + c + q[i..]
                let mut v = q[..i].to_vec();
                v.push(c);
                v.extend_from_slice(&q[i..]);
                out.push(v);
            }
        }
        out
    }

    /// All strings over `alphabet` of length `<= max_len`, as UTF-16 units.
    fn strings(alphabet: &[u16], max_len: usize) -> Vec<Vec<u16>> {
        let mut out = vec![Vec::new()];
        let mut level = vec![Vec::new()];
        for _ in 0..max_len {
            let mut next = Vec::new();
            for s in &level {
                for &c in alphabet {
                    let mut t = s.clone();
                    t.push(c);
                    out.push(t.clone());
                    next.push(t);
                }
            }
            level = next;
        }
        out
    }

    fn u(word: &str) -> Vec<u16> {
        word.encode_utf16().collect()
    }

    #[test]
    fn base_matches_the_raw_stream_layout() {
        // Every split past 0 contributes 54 raw candidates after the 26
        // initial inserts, so the stream length pins the block layout down.
        for q in [u(""), u("a"), u("ab"), u("abc"), u("café")] {
            assert_eq!(raw_edits(&q).len(), 26 + 54 * q.len());
        }
        assert_eq!(base(0), 0);
        assert_eq!(base(1), 26);
        assert_eq!(base(2), 80);
        assert_eq!(base(3), 134);
    }

    #[test]
    fn del_hash_skips_exactly_the_named_positions() {
        let full = u("abcde");
        let hash_of = |units: &[u16]| {
            let mut h = rustc_hash::FxHasher::default();
            for &x in units {
                h.write_u16(x);
            }
            h.finish()
        };
        assert_eq!(del_hash(&full, NO_SKIP, NO_SKIP), hash_of(&full));
        assert_eq!(del_hash(&full, 0, NO_SKIP), hash_of(&u("bcde")));
        assert_eq!(del_hash(&full, 4, NO_SKIP), hash_of(&u("abcd")));
        assert_eq!(del_hash(&full, 1, 3), hash_of(&u("ace")));
        assert_eq!(del_hash(&full, 0, 4), hash_of(&u("bcd")));
        assert_eq!(del_hash(&u(""), NO_SKIP, NO_SKIP), hash_of(&[]));
    }

    /// `dist1_minpos` against the raw stream, brute-forced: for every query
    /// over small alphabets (letters only, and letters mixed with non-alphabet
    /// units), the closed form must return exactly the first raw position
    /// producing `w`, and `None` for every unreachable `w`.
    #[test]
    fn dist1_minpos_matches_raw_stream_brute_force() {
        let alphabets: [&[u16]; 2] = [
            &[u16::from(b'a'), u16::from(b'b'), u16::from(b'c')],
            &[u16::from(b'a'), u16::from(b'Q'), u16::from(b'1')],
        ];
        for alphabet in alphabets {
            for q in strings(alphabet, 4) {
                let raw = raw_edits(&q);
                let mut first: FxHashMap<&[u16], u64> = FxHashMap::default();
                for (pos, e) in raw.iter().enumerate() {
                    first.entry(e.as_slice()).or_insert(pos as u64);
                }
                // Every reachable string, plus unreachable probes over the
                // same alphabet (never longer than one insert can reach).
                for w in raw.iter().map(Vec::as_slice) {
                    assert_eq!(
                        dist1_minpos(&q, w),
                        first.get(w).copied(),
                        "q={q:?} w={w:?}"
                    );
                }
                for w in strings(alphabet, q.len() + 1) {
                    assert_eq!(
                        dist1_minpos(&q, &w),
                        first.get(w.as_slice()).copied(),
                        "q={q:?} w={w:?}"
                    );
                }
            }
        }
    }

    /// `dist2_minkey` against the raw stream, brute-forced: enumerating op1 in
    /// raw order and op2 in raw order, the first `(p1, p2)` reaching `w` is
    /// the expected key. Only pairs where `dist1_minpos` is `None` are
    /// checked — that is the caller's contract.
    #[test]
    fn dist2_minkey_matches_raw_stream_brute_force() {
        let alphabets: [&[u16]; 2] = [
            &[u16::from(b'a'), u16::from(b'b'), u16::from(b'c')],
            &[u16::from(b'a'), u16::from(b'Q'), u16::from(b'1')],
        ];
        let mut scratch: Vec<u16> = Vec::new();
        for alphabet in alphabets {
            for q in strings(alphabet, 3) {
                let raw = raw_edits(&q);
                let mut first1: FxHashMap<&[u16], u64> = FxHashMap::default();
                for (pos, e) in raw.iter().enumerate() {
                    first1.entry(e.as_slice()).or_insert(pos as u64);
                }
                let mut first2: FxHashMap<Vec<u16>, (u64, u64)> = FxHashMap::default();
                for (p1, s) in raw.iter().enumerate() {
                    for (p2, t) in raw_edits(s).into_iter().enumerate() {
                        first2.entry(t).or_insert((p1 as u64, p2 as u64));
                    }
                }
                // Probes: every enumerable string over the alphabet within
                // reach, plus a deterministic sample of the full two-edit
                // closure (which contains letters outside the alphabet).
                let mut probes: Vec<Vec<u16>> = strings(alphabet, q.len() + 2);
                let mut closure: Vec<&Vec<u16>> = first2.keys().collect();
                closure.sort_unstable();
                probes.extend(closure.iter().step_by(7).map(|w| (*w).clone()));
                for w in &probes {
                    if dist1_minpos(&q, w).is_some() {
                        continue;
                    }
                    assert_eq!(
                        dist2_minkey(&q, w, &mut scratch),
                        first2.get(w).copied(),
                        "q={q:?} w={w:?}"
                    );
                }
            }
        }
    }

    /// The one cancellation case: the empty query reaches the empty word in
    /// exactly two edits (insert a letter, delete it again), never one.
    #[test]
    fn empty_query_reaches_empty_word_at_level_two() {
        let mut scratch = Vec::new();
        assert_eq!(dist1_minpos(&[], &[]), None);
        assert_eq!(dist2_minkey(&[], &[], &mut scratch), Some((0, base(1))));
    }

    /// Retrieval is a superset check: every word within generator distance 2
    /// of the query must come back as a candidate (verification prunes the
    /// rest), including words reached only through transposition.
    #[test]
    fn candidates_cover_the_two_edit_neighbourhood() {
        let corpus = ["cat", "cta", "act", "ct", "c", "cats", "scat", "dog", ""];
        let index = QueryIndex::new(corpus.iter().copied());
        let mut out = Vec::new();
        let mut scratch = Vec::new();
        for query in ["cat", "ca", "cta", "catz", "", "c"] {
            let qu = u(query);
            index.candidates(&qu, 2, &mut out);
            for (i, w) in corpus.iter().enumerate() {
                let wu = u(w);
                let reachable = dist1_minpos(&qu, &wu).is_some()
                    || dist2_minkey(&qu, &wu, &mut scratch).is_some();
                if reachable {
                    assert!(
                        out.contains(&(i as u32)),
                        "query {query:?} must retrieve {w:?}"
                    );
                }
            }
        }
    }
}
