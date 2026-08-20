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
//! property with a static ordering test (`nested_pairs_are_longest_first`,
//! test-only).
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

/// One entry of a built table: where its code units sit in the shared blob,
/// its substring link, and (for a merged table) which source table it came
/// from.
///
/// # Why the units are not stored inline
///
/// The first version held `Vec<(Vec<u16>, i32)>`, one heap block per suffix.
/// The binary search then paid a *dependent* load per probe — read the entry,
/// then chase its pointer before a single unit could be compared — on tables
/// of up to ~95 entries searched several times per word. Packing every
/// entry's units into one `blob` and every entry into one 12-byte record
/// makes both arrays contiguous, so a whole table is a handful of cache
/// lines and a probe is two independent loads. This is also the shape the
/// Snowball runtime itself uses (a static array of `Among` structs pointing
/// into static string data).
#[derive(Clone, Copy)]
struct Entry {
    /// Offset of the entry's first unit in the blob.
    start: u32,
    /// The entry's length in code units.
    len: u32,
    /// The longest *other* entry that is a proper suffix of this one, or `-1`.
    link: i32,
    /// The source table's index, for merged tables; `0` otherwise.
    tag: u8,
}

/// The shared storage and search behind [`AmongTable`] and [`UnionTable`].
struct Table {
    blob: Vec<u16>,
    entries: Vec<Entry>,
    /// The set of *final* code units over all entries, as the two halves of a
    /// Latin-1 bitmask. See [`Table::search`] for what it buys.
    last_lo: u128,
    last_hi: u128,
    /// Whether the [`Table::last_lo`]/[`Table::last_hi`] test may be trusted
    /// to reject; see [`Table::search`].
    filter: bool,
}

/// The longest proper suffix of `strs[k]` present in `strs`, as an index, or `-1`.
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

impl Table {
    /// Packs already-sorted `(units, tag)` pairs into the flat form.
    fn pack(strs: &[(Vec<u16>, u8)]) -> Table {
        let mut blob: Vec<u16> = Vec::with_capacity(strs.iter().map(|s| s.0.len()).sum());
        let mut entries = Vec::with_capacity(strs.len());
        for (k, (units, tag)) in strs.iter().enumerate() {
            let start = u32::try_from(blob.len()).expect("rule tables are tiny");
            blob.extend_from_slice(units);
            entries.push(Entry {
                start,
                len: u32::try_from(units.len()).expect("suffix literals are short"),
                link: suffix_link(strs, k),
                tag: *tag,
            });
        }
        // The rejection test is only sound when every entry has a final unit
        // (an empty entry matches an empty region, which the test would
        // reject) and when every such unit is inside the Latin-1 range the
        // two masks cover. Both hold for every table in this crate; a future
        // table that broke either simply turns the filter off rather than
        // changing an answer.
        let mut last_lo = 0u128;
        let mut last_hi = 0u128;
        let mut filter = true;
        for (units, _) in strs {
            match units.last() {
                Some(&c) if c < 128 => last_lo |= 1u128 << c,
                Some(&c) if c < 256 => last_hi |= 1u128 << (c - 128),
                _ => filter = false,
            }
        }
        Table {
            blob,
            entries,
            last_lo,
            last_hi,
            filter,
        }
    }

    /// The code units of entry `i`.
    #[inline]
    fn units(&self, i: usize) -> &[u16] {
        let e = self.entries[i];
        &self.blob[e.start as usize..(e.start + e.len) as usize]
    }

    /// The core of Snowball's `find_among_b`: the index of the longest entry
    /// that is a suffix of `w[lb..cursor]`, or `-1`.
    ///
    /// Exact u16 port of the reference runtime's search, including the
    /// `first_key_inspected` re-probe of key 0 and the fallback walk along
    /// substring links. `lb` is Snowball's `limit_backward`: comparisons stop
    /// at it, so an entry longer than `cursor - lb` can never match — which is
    /// precisely the region restriction every caller needs. Callers must pass
    /// `lb <= cursor` (clamp with `min` when a stale region index may exceed
    /// the current length, as the reference's `slice` clamping does
    /// implicitly).
    ///
    /// # The rejection test
    ///
    /// An entry is a suffix of `w[lb..cursor]` only if its own last unit is
    /// `w[cursor - 1]`, so a word whose final unit ends no entry at all
    /// cannot match anything and the binary search below can be skipped
    /// outright. That is the common case by a wide margin — a step's table
    /// covers a handful of terminal letters (Norwegian's `consonant_pair` is
    /// reached only by a final `t`, its `other_suffix` only by `g`, `s` or
    /// `v`) while a real word ends in whatever it ends in — and the test is
    /// two shifts against a mask pair held in the same cache line as the
    /// entry count. Measured on Norwegian: the four searches `stem` performs
    /// per word fell from 35.0 µs to 12.6 µs per 1024 bench words.
    ///
    /// The test is exact rather than heuristic: it only ever skips a search
    /// whose answer is provably `-1`, and it is disabled at build time for
    /// any table where that proof does not hold (see [`Table::pack`]).
    ///
    /// # What was tried and rejected
    ///
    /// Entries sharing a final unit are also *contiguous* (the sort key is
    /// the reversed sequence, so the final unit is the primary key), so the
    /// mask can be made to select that run — its rank is the number of set
    /// bits below `w[cursor - 1]` — and the binary search handed the run
    /// rather than the whole table. That is sound, link walks included (a
    /// proper suffix ends in the same unit, so no link leaves the run), and
    /// it measured *slower*: 44.0 against 38.5 µs per 1024 Norwegian bench
    /// words. At these table sizes the saved probes do not pay for the
    /// second lookup array's cache line and the rank arithmetic. The plain
    /// rejection below keeps all of the win and none of that cost.
    #[inline]
    fn search(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        debug_assert!(lb <= cursor && cursor <= w.len());
        if self.filter {
            // Every entry is at least one unit long, so an empty region
            // matches nothing.
            if cursor == lb {
                return -1;
            }
            if !crate::units::in_set(w[cursor - 1], self.last_lo, self.last_hi) {
                return -1;
            }
        }
        self.search_sorted(w, cursor, lb)
    }

    /// The binary search proper, reached once the cheap rejection above has
    /// failed to rule every entry out.
    fn search_sorted(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        let mut i: i32 = 0;
        let mut j: i32 =
            i32::try_from(self.entries.len()).expect("table sizes are far below i32::MAX");
        let c = cursor as i32;
        let lb = lb as i32;
        let mut common_i = 0i32;
        let mut common_j = 0i32;
        let mut first_key_inspected = false;
        loop {
            let k = i + ((j - i) >> 1);
            let mut diff: i32 = 0;
            let mut common = common_i.min(common_j);
            let ws = self.units(k as usize);
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
            let e = self.entries[i as usize];
            if common_i >= e.len as i32 {
                return i;
            }
            i = e.link;
            if i < 0 {
                return -1;
            }
        }
    }
}

/// One suffix table, sorted for binary search.
///
/// Entries are sorted by reversed code-unit sequence and carry the substring
/// link the search and the caller-side link walks need.
pub(crate) struct AmongTable(Table);

impl AmongTable {
    /// Builds the sorted table from a rule table.
    pub(crate) fn build(table: &[&str]) -> Self {
        let mut strs: Vec<(Vec<u16>, u8)> = table
            .iter()
            .map(|s| (s.encode_utf16().collect(), 0))
            .collect();
        sort_reversed(&mut strs);
        AmongTable(Table::pack(&strs))
    }

    /// The index of the longest entry that is a suffix of `w[lb..cursor]`,
    /// or `-1`. Use when the caller needs the entry's units (e.g. to re-check
    /// it against a different string), otherwise prefer [`Self::longest`].
    #[inline]
    pub(crate) fn find(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        self.0.search(w, cursor, lb)
    }

    /// The unit length of entry `i`, as returned by [`Self::find`].
    #[inline]
    pub(crate) fn len_at(&self, i: i32) -> usize {
        self.0.entries[i as usize].len as usize
    }

    /// The unit length of the longest entry that is a suffix of
    /// `w[lb..cursor]`, or 0.
    #[inline]
    pub(crate) fn longest(&self, w: &[u16], cursor: usize, lb: usize) -> usize {
        let i = self.find(w, cursor, lb);
        if i < 0 { 0 } else { self.len_at(i) }
    }

    /// The unit length of the longest matching entry whose length also
    /// satisfies `cond`, or 0.
    ///
    /// This is the `/([ая])(ла|на|…)$/` shape: an alternation guarded by a
    /// condition on what precedes the match. Walking the substring links from
    /// the longest match enumerates every matching entry in decreasing length,
    /// so the first one passing `cond` is the longest passing one — which is
    /// the entry an anchored alternation's earliest match start selects.
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
            let e = self.0.entries[i as usize];
            if cond(e.len as usize) {
                return e.len as usize;
            }
            i = e.link;
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
pub(crate) struct UnionTable(Table);

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
        assert!(
            strs.iter().all(|s| s.0.len() < 32),
            "an entry of 32 units or more cannot be represented in \
             `length_masks`'s per-table bitmask"
        );
        UnionTable(Table::pack(&strs))
    }

    /// The index of the longest entry that is a suffix of `w[lb..cursor]`, or
    /// `-1`. The caller walks [`Self::entry`] from here, applying each entry's
    /// table-specific check via its tag.
    #[inline]
    pub(crate) fn find_longest_index(&self, w: &[u16], cursor: usize, lb: usize) -> i32 {
        self.0.search(w, cursor, lb)
    }

    /// Entry `i` as `(unit length, substring link, source table id)` — the
    /// three fields a link walk reads, in one load.
    #[inline]
    pub(crate) fn entry(&self, i: i32) -> (usize, i32, usize) {
        let e = self.0.entries[i as usize];
        (e.len as usize, e.link, e.tag as usize)
    }

    /// Records, per source table, which entry *lengths* are suffixes of
    /// `w[..cursor]`, as a bitmask over lengths (bit `n` set ⇔ some entry of
    /// that table with `n` units matches).
    ///
    /// # Why a mask rather than one best length
    ///
    /// A rule chain does not ask one question per table. French's step 1 asks
    /// up to three of the same table — "does it match anywhere?", "does it
    /// match inside R2?", "inside R1?" — and each is the same match set
    /// filtered by a different *length* bound, because that is all the
    /// region limit `lb` does (an entry matches within `lb` exactly when its
    /// length is at most `cursor - lb`). One unrestricted search therefore
    /// answers every region question the chain can pose, provided the whole
    /// match set is kept rather than just its maximum — which is what this
    /// mask is. [`longest_at_most`] then reads off each answer in a couple of
    /// instructions, replacing what were 25 separate binary searches per word
    /// with one.
    ///
    /// `masks` must have one slot per merged table and start zeroed.
    pub(crate) fn length_masks(&self, w: &[u16], cursor: usize, masks: &mut [u32]) {
        let mut i = self.find_longest_index(w, cursor, 0);
        while i >= 0 {
            let (n, link, tid) = self.entry(i);
            masks[tid] |= 1u32 << n;
            i = link;
        }
    }
}

/// The largest `n` with bit `n` set in `mask` and `n <= avail`, or 0.
///
/// Companion to [`UnionTable::length_masks`]: `avail` is `cursor - lb`, the
/// number of units the region leaves available, so this is "the longest entry
/// of that table that matches inside the region". Length 0 is never a table
/// entry, so `0` doubles as "no match", exactly like the search's own
/// `longest`.
#[inline]
pub(crate) fn longest_at_most(mask: u32, avail: usize) -> usize {
    let keep = if avail >= 31 {
        mask
    } else {
        mask & ((2u32 << avail) - 1)
    };
    if keep == 0 {
        0
    } else {
        (31 - keep.leading_zeros()) as usize
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

/// How many code units a [`Buf`] holds without allocating.
///
/// Measured, not guessed: the inline array is zero-initialised on every
/// fill, so its size is a per-word memset. Real words in every language this
/// crate stems average eight to eleven units, and the arm exists to cover
/// ordinary words, not adversarial ones — 32 keeps essentially all of them
/// inline while halving that memset against the 64 this started at. Anything
/// longer spills to the heap and is correct, just not free.
pub(crate) const INLINE: usize = 32;

/// A UTF-16 working buffer that lives on the stack for words up to
/// [`INLINE`] units.
///
/// The Spanish measurement attributed ~17 ns/word of the remaining floor to
/// the working `Vec<u16>` allocation, and every language here pays it once
/// per word; the inline arm removes that allocation entirely. Cloning one is
/// a fixed 64-byte stack copy, which is what makes the "did this step change
/// the word?" snapshots the French and English algorithms need affordable
/// without a heap allocation.

#[derive(Clone)]
pub(crate) enum Buf {
    /// Up to [`INLINE`] units, inline. The second field is the live length.
    Inline([u16; INLINE], usize),
    /// The spill arm for longer words.
    Heap(Vec<u16>),
}

impl Buf {
    /// Encodes `word` as UTF-16, inline when it fits.
    #[inline]
    pub(crate) fn fill(word: &str) -> Buf {
        let mut a = [0u16; INLINE];
        let mut n = 0usize;
        for unit in word.encode_utf16() {
            if n == INLINE {
                let mut v: Vec<u16> = Vec::with_capacity(word.len());
                v.extend_from_slice(&a);
                v.extend(word.encode_utf16().skip(INLINE));
                return Buf::Heap(v);
            }
            a[n] = unit;
            n += 1;
        }
        Buf::Inline(a, n)
    }

    /// `units(&word.to_lowercase())`, without the intermediate `String`.
    ///
    /// Nine of the twelve Snowball ports open with `toLowerCase()`, and the
    /// straightforward port allocated twice before the algorithm had run a
    /// single rule: once for the lowered `String`, once for the `Vec<u16>`
    /// it was then re-encoded into. Both are gone here — the lowered units
    /// land straight in the inline array.
    ///
    /// # The ASCII arm
    ///
    /// `str::to_lowercase` on ASCII is exactly `u8::to_ascii_lowercase` per
    /// byte (the sigma rule cannot apply, and no ASCII character expands),
    /// so the common case is a flat, vectorizable byte loop with no
    /// character decoding at all. The general arm below reproduces
    /// [`crate::units::for_each_lowercase_unit`]'s mapping inline rather
    /// than through the callback, because the callback's captured cursor
    /// stopped the loop from keeping its index in a register.
    ///
    /// Lowercasing can *lengthen* a string (`İ` becomes two characters), so
    /// unlike [`Self::fill`] the general arm spills on the growth condition
    /// rather than on the input's own unit count.
    #[inline]
    pub(crate) fn fill_lowercase(word: &str) -> Buf {
        let bytes = word.as_bytes();
        if bytes.len() <= INLINE && word.is_ascii() {
            let mut a = [0u16; INLINE];
            for (slot, &b) in a.iter_mut().zip(bytes) {
                *slot = u16::from(b.to_ascii_lowercase());
            }
            return Buf::Inline(a, bytes.len());
        }
        let mut a = [0u16; INLINE];
        let mut n = 0usize;
        for c in word.chars() {
            // The overwhelming majority of characters are one code unit in
            // and one out, so that case writes straight into the array; the
            // two that are not — a character outside the mask ranges, and
            // the one context-sensitive mapping — take the cold arms.
            if (c as u32) < 0x1_0000
                && let Some(l) = crate::units::fast_lower(c as u16)
            {
                if n == INLINE {
                    return Self::lowercase_on_the_heap(word);
                }
                a[n] = l;
                n += 1;
                continue;
            }
            if c == '\u{03A3}' {
                // The one context-sensitive mapping; see
                // `crate::units::for_each_lowercase_unit`. Checking it here
                // rather than with a `contains` pre-scan keeps the common
                // case to a single pass over the word.
                return Buf::fill(&word.to_lowercase());
            }
            for l in c.to_lowercase() {
                let mut b = [0u16; 2];
                for unit in l.encode_utf16(&mut b) {
                    if n == INLINE {
                        return Self::lowercase_on_the_heap(word);
                    }
                    a[n] = *unit;
                    n += 1;
                }
            }
        }
        Buf::Inline(a, n)
    }

    /// [`Self::fill_lowercase`], plus whether `word` was *already* ASCII
    /// lowercase — the condition under which the stemmed result is a slice
    /// of `word` itself rather than a new `String`.
    ///
    /// # Why the flag is a separate pass
    ///
    /// Folding the two tests into [`Self::fill_lowercase`]'s widening loop
    /// is the obvious way to answer them for free, and it measured *slower*
    /// — 38.0 against 36.7 µs per 1024 Norwegian bench words. The widening
    /// loop and the predicate below both vectorize on their own; carrying
    /// two loop-carried booleans through the widening loop stops it doing
    /// so, and a second vectorized pass over eight bytes is cheaper than a
    /// scalar first one. The apparently redundant scan is therefore
    /// deliberate.
    #[inline]
    pub(crate) fn fill_lowercase_tracked(word: &str) -> (Buf, bool) {
        let ascii_lower = !word
            .as_bytes()
            .iter()
            .any(|&c| c >= 0x80 || c.is_ascii_uppercase());
        (Buf::fill_lowercase(word), ascii_lower)
    }

    /// The spill arm of [`Self::fill_lowercase`]: rare enough to be worth
    /// restarting from the beginning rather than carrying a branch for it
    /// through the inline loop.
    #[cold]
    fn lowercase_on_the_heap(word: &str) -> Buf {
        let mut v: Vec<u16> = Vec::with_capacity(word.len() + 8);
        crate::units::for_each_lowercase_unit(word, |unit| v.push(unit));
        Buf::Heap(v)
    }

    /// Copies `w` into a fresh buffer, on the stack when it fits.
    ///
    /// The English stemmer needs the token as it was *before* a step while
    /// rewriting it in place — the measure guards read the original — and
    /// this is that snapshot without a heap allocation.
    pub(crate) fn from_slice(w: &[u16]) -> Buf {
        if w.len() <= INLINE {
            let mut a = [0u16; INLINE];
            a[..w.len()].copy_from_slice(w);
            Buf::Inline(a, w.len())
        } else {
            Buf::Heap(w.to_vec())
        }
    }

    /// Decodes the live units back to a `String`.
    #[inline]
    pub(crate) fn into_text(self) -> String {
        crate::units::text(self.as_slice())
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

    /// Appends one unit, spilling to the heap if the inline array is full.
    ///
    /// Every rewrite rule in these algorithms is net-shrinking — a push only
    /// ever follows a larger truncate — so the spill branch is unreachable
    /// for them and predicts perfectly. It exists so that a future rule which
    /// is *not* net-shrinking degrades to an allocation instead of to an
    /// out-of-bounds panic.
    #[inline]
    pub(crate) fn push(&mut self, unit: u16) {
        match self {
            Buf::Inline(a, n) => {
                if *n == a.len() {
                    let mut v = a.to_vec();
                    v.push(unit);
                    *self = Buf::Heap(v);
                } else {
                    a[*n] = unit;
                    *n += 1;
                }
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
            lens.push(among.len_at(i));
            i = among.0.entries[i as usize].link;
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
            let (len, link, tid) = union.entry(i);
            if best[tid] == 0 {
                best[tid] = len;
            }
            i = link;
        }
        assert_eq!(best, [4, 2], "longest per table: 'ando' and 'do'");
    }

    #[test]
    fn buf_spills_to_the_heap_past_the_inline_capacity() {
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
        // A push that overflows the inline arm spills rather than panicking.
        let mut b = Buf::fill(&"y".repeat(INLINE));
        assert!(matches!(b, Buf::Inline(..)));
        b.push(0x7A);
        assert!(matches!(b, Buf::Heap(..)));
        assert_eq!(b.len(), INLINE + 1);
    }

    #[test]
    fn fill_lowercase_matches_the_two_step_form() {
        for word in [
            "",
            "Palabra",
            "ÁRBOL",
            "ВАЖНАЯ",
            "ΟΔΟΣ",
            "İstanbul",
            "😀A",
            "x".repeat(INLINE).as_str(),
            "X".repeat(INLINE + 1).as_str(),
            // As many units of input as fit inline, each lowercasing to two
            // — the growth the inline arm must spill on even though the
            // input itself fits.
            "İ".repeat(INLINE).as_str(),
        ] {
            let want = units(&word.to_lowercase());
            assert_eq!(
                Buf::fill_lowercase(word).as_slice(),
                want.as_slice(),
                "{word:?}"
            );
        }
        assert!(matches!(Buf::fill_lowercase("Corto"), Buf::Inline(..)));
        assert!(matches!(
            Buf::fill_lowercase(&"X".repeat(INLINE + 1)),
            Buf::Heap(..)
        ));
    }

    #[test]
    fn empty_region_never_matches() {
        let among = AmongTable::build(&["a", "ab"]);
        let w = units("ab");
        assert_eq!(among.longest(&w, 2, 2), 0);
        assert_eq!(among.longest(&w, 0, 0), 0);
    }
}
