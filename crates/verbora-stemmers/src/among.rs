//! `find_among` over sorted suffix tables — the Snowball runtime's binary
//! search, ported to UTF-16 code units.
//!
//! # Why this exists
//!
//! `docs/PERFORMANCE_GAPS.md` entry 34 diagnosed, and a measured decomposition
//! confirmed, that the dominant cost in seven of the nine Snowball ports was the
//! *linear* suffix scan: every step called [`crate::units::ends_with`] on every
//! candidate in its table even after the true answer was known, and a real word
//! rejects most candidates (Spanish paid up to ~16 table scans × up to 95
//! candidates per word). Replacing those scans with the Snowball runtime's own
//! `find_among_b` — a binary search over the table sorted by *reversed*
//! code-unit sequence, with `common_i`/`common_j` prefix tracking so no unit is
//! compared twice — recovered 78% of the entire competitive gap on Spanish
//! before any other change. The port was verified byte-exact against the
//! existing linear-scan implementation over 500k+ differential cases.
//!
//! # Semantics
//!
//! [`AmongTable::longest`] returns the length of the **longest** table entry
//! that is a suffix of `w[lb..cursor]` — exactly what `longest_suffix` computes
//! over a region slice, and exactly what the reference's `/(a|bb|ccc)$/`
//! alternations compute (the earliest start at which some alternative reaches
//! `$` is the longest listed suffix). Tables whose reference semantics are
//! *first listed match* may only go through this search when their hand
//! ordering makes first and longest coincide; each such language pins that
//! property with a static ordering test ([`nested_pairs_are_longest_first`]).
//!
//! # Substring links
//!
//! Each entry carries a link to the longest *other* entry that is a proper
//! suffix of it (`-1` when none), mirroring Snowball's `substring_i`. Two uses:
//! the search itself falls back along links when a longer candidate was only
//! partially matched, and callers can *walk* the links from a full match to
//! enumerate every matching entry in strictly decreasing length — the matching
//! entries of a table are totally ordered by suffix-nesting, and every link
//! target of a match also matches. That walk is what lets one search replace a
//! whole else-if chain of region- or condition-guarded lookups
//! ([`AmongTable::longest_where`], [`UnionTable`]).
//!
//! # Cost
//!
//! Tables are built once per process (each language holds its `LazyLock`
//! bundle), from the same `&'static str` tables the linear scans used — the
//! string tables stay the single source of truth, and the build asserts the
//! invariants it relies on instead of trusting the transcription.

/// One suffix table, sorted for binary search.
///
/// `entries[k]` is `(code units, link)`: entries sorted by reversed code-unit
/// sequence, `link` the index of the longest other entry that is a proper
/// suffix of this one, or `-1`.
pub(crate) struct AmongTable {
    pub(crate) entries: Vec<(Vec<u16>, i32)>,
}

/// The longest proper suffix of `s` present in `strs`, as an index, or `-1`.
fn suffix_link(strs: &[(Vec<u16>, u8)], k: usize) -> i32 {
    let s = &strs[k].0;
    let mut link = -1i32;
    let mut best_len = 0usize;
    for (j, (cand, _)) in strs.iter().enumerate() {
        if j != k && cand.len() < s.len() && cand.len() > best_len && s.ends_with(cand) {
            best_len = cand.len();
            link = i32::try_from(j).expect("table sizes are far below i32::MAX");
        }
    }
    link
}

/// Sorts `(units, tag)` pairs by reversed code-unit sequence — the order
/// `find_among_b`'s suffix-wise binary search requires.
fn sort_reversed(strs: &mut [(Vec<u16>, u8)]) {
    strs.sort_by(|a, b| a.0.iter().rev().cmp(b.0.iter().rev()));
}

/// The core of Snowball's `find_among_b`: the index of the longest entry that
/// is a suffix of `w[lb..cursor]`, or `-1`.
///
/// Exact u16 port of the reference runtime's search, including the
/// `first_key_inspected` re-probe of key 0 and the fallback walk along
/// substring links. `lb` is Snowball's `limit_backward`: comparisons stop at
/// it, so an entry longer than `cursor - lb` can never match — which is
/// precisely the region restriction every caller needs. Callers must pass
/// `lb <= cursor` (clamp with `min` when a stale region index may exceed the
/// current length, as the reference's `slice` clamping does implicitly).
fn search(entries: &[(Vec<u16>, i32)], w: &[u16], cursor: usize, lb: usize) -> i32 {
    debug_assert!(lb <= cursor && cursor <= w.len());
    let mut i: i32 = 0;
    let mut j: i32 = i32::try_from(entries.len()).expect("table sizes are far below i32::MAX");
    let c = cursor as i32;
    let lb = lb as i32;
    let mut common_i = 0i32;
    let mut common_j = 0i32;
    let mut first_key_inspected = false;
    loop {
        let k = i + ((j - i) >> 1);
        let mut diff: i32 = 0;
        let mut common = common_i.min(common_j);
        let ws = &entries[k as usize].0;
        for lvar in (0..ws.len() - common as usize).rev() {
            if c - common == lb {
                diff = -1;
                break;
            }
            diff = i32::from(w[(c - common - 1) as usize]) - i32::from(ws[lvar]);
            if diff != 0 {
                break;
            }
            common += 1;
        }
        if diff < 0 {
            j = k;
            common_j = common;
        } else {
            i = k;
            common_i = common;
        }
        if j - i <= 1 {
            if i > 0 || j == i || first_key_inspected {
                break;
            }
            first_key_inspected = true;
        }
    }
    loop {
        let (ws, link) = &entries[i as usize];
        if common_i >= i32::try_from(ws.len()).expect("suffix literals are short") {
            return i;
        }
        i = *link;
        if i < 0 {
            return -1;
        }
    }
}

impl AmongTable {
    /// Builds the sorted table from a reference rule table.
    pub(crate) fn build(table: &[&str]) -> Self {
        let mut strs: Vec<(Vec<u16>, u8)> = table
            .iter()
            .map(|s| (s.encode_utf16().collect(), 0))
            .collect();
        sort_reversed(&mut strs);
        let entries = (0..strs.len())
            .map(|k| (strs[k].0.clone(), suffix_link(&strs, k)))
            .collect();
        AmongTable { entries }
    }

    /// The index of the longest entry that is a suffix of `w[lb..cursor]`,
    /// or `-1`. Use when the caller needs the entry's units (e.g. to re-check
    /// it against a different string), otherwise prefer [`Self::longest`].
    #[inline]
    pub(crate) fn find(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        search(&self.entries, w, cursor, lb)
    }

    /// The unit length of the longest entry that is a suffix of
    /// `w[lb..cursor]`, or 0.
    #[inline]
    pub(crate) fn longest(&self, w: &[u16], cursor: usize, lb: usize) -> usize {
        let i = self.find(w, cursor, lb);
        if i < 0 {
            0
        } else {
            self.entries[i as usize].0.len()
        }
    }

    /// The unit length of the longest matching entry whose length also
    /// satisfies `cond`, or 0.
    ///
    /// This is the `/([ая])(ла|на|…)$/` shape: an alternation guarded by a
    /// condition on what precedes the match. Walking the substring links from
    /// the longest match enumerates every matching entry in decreasing length,
    /// so the first one passing `cond` is the longest passing one — the same
    /// entry the reference's earliest-match-start rule selects.
    #[inline]
    pub(crate) fn longest_where(
        &self,
        w: &[u16],
        cursor: usize,
        lb: usize,
        mut cond: impl FnMut(usize) -> bool,
    ) -> usize {
        let mut i = self.find(w, cursor, lb);
        while i >= 0 {
            let (units, link) = &self.entries[i as usize];
            if cond(units.len()) {
                return units.len();
            }
            i = *link;
        }
        0
    }
}

/// Several rule tables merged into one search, each entry tagged with the
/// id (position in the `build` argument) of the table it came from.
///
/// One binary search finds the longest match across *all* tables; walking the
/// substring links from it visits every matching entry in decreasing length,
/// and the caller reapplies each table's own region or condition check —
/// turning an else-if chain of per-table searches into a single search. The
/// merge is only sound when no string appears in two tables (the priority
/// between duplicates would be lost), so `build` asserts exactly that.
pub(crate) struct UnionTable {
    pub(crate) entries: Vec<(Vec<u16>, i32, u8)>,
}

impl UnionTable {
    /// Merges `tables`, tagging each entry with its table's index.
    ///
    /// # Panics
    ///
    /// When the same string appears in more than one table — the union search
    /// could then not reproduce the chain's table priority. This runs once per
    /// process at table-build time, so a bad future table edit fails loudly on
    /// first use rather than silently changing stems.
    pub(crate) fn build(tables: &[&[&str]]) -> Self {
        let mut strs: Vec<(Vec<u16>, u8)> = Vec::new();
        for (tid, t) in tables.iter().enumerate() {
            for s in t.iter() {
                strs.push((
                    s.encode_utf16().collect(),
                    u8::try_from(tid).expect("more than 256 merged tables"),
                ));
            }
        }
        sort_reversed(&mut strs);
        for pair in strs.windows(2) {
            assert_ne!(
                pair[0].0, pair[1].0,
                "duplicate suffix across merged tables"
            );
        }
        let entries = (0..strs.len())
            .map(|k| (strs[k].0.clone(), suffix_link(&strs, k), strs[k].1))
            .collect();
        UnionTable { entries }
    }

    /// The index of the longest entry that is a suffix of `w[lb..cursor]`, or
    /// `-1`. The caller walks `entries[i].1` links from here, applying each
    /// entry's table-specific check via its tag.
    #[inline]
    pub(crate) fn find_longest_index(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        // The search never reads the tag, so a plain transmute-free reborrow is
        // not possible; instead the same algorithm is instantiated over the
        // tagged entry type.
        let mut i: i32 = 0;
        let mut j: i32 =
            i32::try_from(self.entries.len()).expect("table sizes are far below i32::MAX");
        let c = cursor as i32;
        let lb = lb as i32;
        let mut common_i = 0i32;
        let mut common_j = 0i32;
        let mut first_key_inspected = false;
        debug_assert!(lb <= c && cursor <= w.len());
        loop {
            let k = i + ((j - i) >> 1);
            let mut diff: i32 = 0;
            let mut common = common_i.min(common_j);
            let ws = &self.entries[k as usize].0;
            for lvar in (0..ws.len() - common as usize).rev() {
                if c - common == lb {
                    diff = -1;
                    break;
                }
                diff = i32::from(w[(c - common - 1) as usize]) - i32::from(ws[lvar]);
                if diff != 0 {
                    break;
                }
                common += 1;
            }
            if diff < 0 {
                j = k;
                common_j = common;
            } else {
                i = k;
                common_i = common;
            }
            if j - i <= 1 {
                if i > 0 || j == i || first_key_inspected {
                    break;
                }
                first_key_inspected = true;
            }
        }
        loop {
            let (ws, link, _) = &self.entries[i as usize];
            if common_i >= i32::try_from(ws.len()).expect("suffix literals are short") {
                return i;
            }
            i = *link;
            if i < 0 {
                return -1;
            }
        }
    }
}

/// Asserts that, whenever one entry of `table` is a proper suffix of another,
/// the longer one is listed first.
///
/// Italian's `endsinArr` and Portuguese's `replaceSuffixInRegion` stop at the
/// **first** listed match; routing them through the longest-match search above
/// is only byte-exact because their tables are hand-ordered so that first and
/// longest coincide. This check pins that property so a future table edit
/// cannot silently break the equivalence — call it from a static test in each
/// first-match language.
#[cfg(test)]
pub(crate) fn nested_pairs_are_longest_first(name: &str, table: &[&str]) {
    let units: Vec<Vec<u16>> = table.iter().map(|s| s.encode_utf16().collect()).collect();
    for (i, a) in units.iter().enumerate() {
        for b in units.iter().skip(i + 1) {
            assert!(
                !(a.len() < b.len() && b.ends_with(a)),
                "{name}: entry {:?} is listed before {:?}, of which it is a proper \
                 suffix — first-match would fire the shorter entry, longest-match \
                 the longer one",
                String::from_utf16_lossy(a),
                String::from_utf16_lossy(b),
            );
        }
    }
}

/// A UTF-16 working buffer that lives on the stack for words up to 64 units.
///
/// The Spanish measurement attributed ~17 ns/word of the remaining floor to
/// the working `Vec<u16>` allocation; virtually every real word fits in 64
/// units, so the inline arm removes that allocation entirely. Every rewrite
/// rule in the algorithms that use this is net-shrinking — a `push` only ever
/// follows a larger `truncate` — so the capacity check happens once, at fill
/// time, and `push` never needs to spill.
pub(crate) enum Buf {
    /// Up to 64 units, inline. The second field is the live length.
    Inline([u16; 64], usize),
    /// The spill arm for longer words.
    Heap(Vec<u16>),
}

impl Buf {
    /// Encodes `word` as UTF-16, inline when it fits.
    pub(crate) fn fill(word: &str) -> Buf {
        let mut a = [0u16; 64];
        let mut n = 0usize;
        for unit in word.encode_utf16() {
            if n == 64 {
                let mut v: Vec<u16> = Vec::with_capacity(word.len());
                v.extend_from_slice(&a);
                v.extend(word.encode_utf16().skip(64));
                return Buf::Heap(v);
            }
            a[n] = unit;
            n += 1;
        }
        Buf::Inline(a, n)
    }

    /// The live units.
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u16] {
        match self {
            Buf::Inline(a, n) => &a[..*n],
            Buf::Heap(v) => v,
        }
    }

    /// The live units, mutably.
    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u16] {
        match self {
            Buf::Inline(a, n) => &mut a[..*n],
            Buf::Heap(v) => v,
        }
    }

    /// The live length in units.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Buf::Inline(_, n) => *n,
            Buf::Heap(v) => v.len(),
        }
    }

    /// Shortens the buffer to `keep` units; longer values are a no-op.
    #[inline]
    pub(crate) fn truncate(&mut self, keep: usize) {
        match self {
            Buf::Inline(_, n) => {
                if keep < *n {
                    *n = keep;
                }
            }
            Buf::Heap(v) => v.truncate(keep),
        }
    }

    /// Appends one unit. Callers only push after a larger truncate (every
    /// rewrite rule is net-shrinking), so the inline arm cannot overflow.
    #[inline]
    pub(crate) fn push(&mut self, unit: u16) {
        match self {
            Buf::Inline(a, n) => {
                a[*n] = unit;
                *n += 1;
            }
            Buf::Heap(v) => v.push(unit),
        }
    }

    /// Appends the code units of `s`, under the same net-shrinking contract.
    #[inline]
    pub(crate) fn push_str(&mut self, s: &str) {
        for unit in s.encode_utf16() {
            self.push(unit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{longest_suffix, slen, units};

    /// A miniature xorshift so the table fuzz below is deterministic without
    /// adding a dev-dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn longest_agrees_with_the_linear_scan() {
        let table = &[
            "a", "e", "ede", "ande", "ende", "ane", "ene", "hetene", "en", "heten", "ar", "er",
            "heter", "as", "es", "edes", "endes", "enes", "hetenes", "ens", "hetens", "ers", "ets",
            "et", "het", "ast", "ía", "ción", "ución",
        ];
        let among = AmongTable::build(table);
        let mut rng = Rng(0x1234_5678_9ABC_DEF0);
        let alphabet = ['a', 'e', 'h', 'n', 'r', 's', 't', 'd', 'í', 'c', 'ó'];
        for _ in 0..20_000 {
            let mut word = String::new();
            for _ in 0..rng.below(12) {
                word.push(alphabet[rng.below(alphabet.len())]);
            }
            let w = units(&word);
            let lb = rng.below(w.len() + 1);
            let want = longest_suffix(&w[lb..], table).map_or(0, slen);
            assert_eq!(
                among.longest(&w, w.len(), lb),
                want,
                "word {word:?} lb {lb}"
            );
        }
    }

    #[test]
    fn links_enumerate_matches_in_decreasing_length() {
        let among = AmongTable::build(&["s", "es", "res", "tres", "x"]);
        let w = units("tres");
        let mut lens = Vec::new();
        let mut i = among.find(&w, w.len(), 0);
        while i >= 0 {
            let (u, link) = &among.entries[i as usize];
            lens.push(u.len());
            i = *link;
        }
        assert_eq!(lens, [4, 3, 2, 1]);
    }

    #[test]
    fn longest_where_takes_the_longest_passing_entry() {
        let among = AmongTable::build(&["s", "es", "res"]);
        let w = units("tres");
        assert_eq!(among.longest_where(&w, w.len(), 0, |_| true), 3);
        assert_eq!(among.longest_where(&w, w.len(), 0, |n| n < 3), 2);
        assert_eq!(among.longest_where(&w, w.len(), 0, |_| false), 0);
    }

    #[test]
    #[should_panic(expected = "duplicate suffix across merged tables")]
    fn union_build_rejects_duplicates() {
        let _ = UnionTable::build(&[&["ar", "er"], &["er", "ir"]]);
    }

    #[test]
    fn union_walk_recovers_per_table_longest() {
        let union = UnionTable::build(&[&["ando", "ar"], &["o", "do"]]);
        let w = units("cantando");
        let mut best = [0usize; 2];
        let mut i = union.find_longest_index(&w, w.len(), 0);
        while i >= 0 {
            let (u, link, tid) = &union.entries[i as usize];
            let tid = *tid as usize;
            if best[tid] == 0 {
                best[tid] = u.len();
            }
            i = *link;
        }
        assert_eq!(best, [4, 2], "longest per table: 'ando' and 'do'");
    }

    #[test]
    fn buf_spills_to_the_heap_past_64_units() {
        let short = "palabra";
        let b = Buf::fill(short);
        assert!(matches!(b, Buf::Inline(..)));
        assert_eq!(b.as_slice(), units(short).as_slice());

        let long = "x".repeat(100);
        let mut b = Buf::fill(&long);
        assert!(matches!(b, Buf::Heap(..)));
        assert_eq!(b.len(), 100);
        b.truncate(70);
        b.push_str("er");
        assert_eq!(b.len(), 72);
    }

    #[test]
    fn empty_region_never_matches() {
        let among = AmongTable::build(&["a", "ab"]);
        let w = units("ab");
        assert_eq!(among.longest(&w, 2, 2), 0);
        assert_eq!(among.longest(&w, 0, 0), 0);
    }
}
