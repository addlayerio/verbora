//! Text-unit dispatch: the mechanism that keeps the scalar unit free on
//! ordinary input.
//!
//! # The unit
//!
//! **One Unicode scalar value (`char`) is one unit.** Every count this crate
//! reports is a count of scalars, so `"a😀b"` is three units and
//! `levenshtein("a😀b", "ab") == 1`. See `docs/design/distance-contract.md`
//! §2 for why: it is the unit `str::chars()` already yields, it consults no
//! Unicode character database (so results are frozen across Unicode
//! versions), and it is the only unit under which a search can report a byte
//! range that always slices the target — every scalar boundary is a byte
//! boundary, while a UTF-16 boundary can fall between the halves of a
//! surrogate pair.
//!
//! # The dispatch
//!
//! Rather than force every algorithm onto `Vec<char>` (which would allocate
//! and slow down the 99.9% case), the algorithms in this crate are written
//! once, generically over `&[T]`, and [`dispatch`] picks the narrowest
//! representation that is *provably identical* to scalars for the given
//! input:
//!
//! | Input                        | Representation | Allocates |
//! |------------------------------|----------------|-----------|
//! | both operands ASCII          | `&[u8]`        | no        |
//! | otherwise                    | `Vec<char>`    | yes       |
//!
//! ASCII is checked with [`str::is_ascii`], which is vectorised. For ASCII
//! input one byte *is* one scalar, so the fast path is not an approximation —
//! it is the same computation on a narrower type.

use rustc_hash::FxHashMap;

/// A symbol an algorithm can operate on: either an ASCII byte or a Unicode
/// scalar value.
///
/// The bound is deliberately minimal so that the algorithms stay monomorphisable
/// down to tight loops over primitives.
pub(crate) trait Unit: Copy + PartialEq + Eq {
    /// A map from unit to `usize`, used by the full-matrix
    /// unrestricted-Damerau branch to remember the last row each symbol
    /// appeared in.
    ///
    /// Bytes get a flat 256-entry array (no hashing, perfect cache behaviour);
    /// scalars fall back to a hash map.
    type Map: UnitMap<Self>;

    /// Creates an empty map.
    fn new_map() -> Self::Map;
}

/// A symbol-to-row map, abstracted so the byte path can avoid hashing entirely.
pub(crate) trait UnitMap<T> {
    /// Returns the stored row for `key`, if present.
    fn get(&self, key: T) -> Option<usize>;
    /// Stores `row` for `key`.
    fn set(&mut self, key: T, row: usize);
}

/// Flat array map for byte alphabets: a lookup is one indexed load.
#[derive(Debug)]
pub(crate) struct ByteMap {
    // `usize::MAX` is the vacant sentinel; row indices never reach it.
    rows: [usize; 256],
}

impl UnitMap<u8> for ByteMap {
    #[inline]
    fn get(&self, key: u8) -> Option<usize> {
        let v = self.rows[key as usize];
        (v != usize::MAX).then_some(v)
    }

    #[inline]
    fn set(&mut self, key: u8, row: usize) {
        self.rows[key as usize] = row;
    }
}

impl Unit for u8 {
    type Map = ByteMap;

    fn new_map() -> Self::Map {
        ByteMap {
            rows: [usize::MAX; 256],
        }
    }
}

impl UnitMap<char> for FxHashMap<char, usize> {
    #[inline]
    fn get(&self, key: char) -> Option<usize> {
        FxHashMap::get(self, &key).copied()
    }

    #[inline]
    fn set(&mut self, key: char, row: usize) {
        self.insert(key, row);
    }
}

impl Unit for char {
    type Map = FxHashMap<char, usize>;

    fn new_map() -> Self::Map {
        FxHashMap::default()
    }
}

/// Runs `f` on `a` and `b` in the narrowest representation whose element
/// sequence is exactly the operands' scalar sequence.
///
/// Both operands are promoted together: mixing a `&[u8]` view with a `&[char]`
/// view is not possible, so if either operand is non-ASCII, both are decoded.
#[inline]
pub(crate) fn dispatch<R>(a: &str, b: &str, f: impl for<'x> FnOnce(Operands<'x>) -> R) -> R {
    if a.is_ascii() && b.is_ascii() {
        f(Operands::Bytes(a.as_bytes(), b.as_bytes()))
    } else {
        let ua: Vec<char> = a.chars().collect();
        let ub: Vec<char> = b.chars().collect();
        f(Operands::Units(&ua, &ub))
    }
}

/// A pair of operands in a single, consistent representation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Operands<'a> {
    /// Both operands are ASCII; one byte is one Unicode scalar value.
    Bytes(&'a [u8], &'a [u8]),
    /// General case: explicit Unicode scalar values.
    Units(&'a [char], &'a [char]),
}

// ---------------------------------------------------------------------------
// Common-affix scans
// ---------------------------------------------------------------------------

/// Chunk width for the affix scans below, in *units*.
///
/// Wide enough that the whole-chunk compare has a real chance of lowering to
/// one `memcmp` call (16 bytes for `u8`, 64 for `char`), short enough that a
/// mismatch inside the very first chunk — the overwhelmingly common case for
/// unrelated operands — costs one such compare and nothing else. The scalar
/// tail loop then pins down the exact boundary inside the chunk that
/// differed, so the result is the true affix length, never a chunk-rounded
/// approximation.
///
/// `UNMEASURED` at `char`: whether a 64-byte chunk compare still lowers to a
/// single `memcmp` must be confirmed from the emitted code before the
/// "at least one SIMD register" rationale is restated as fact
/// (`docs/design/distance-contract.md` §7, item 7).
const AFFIX_CHUNK: usize = 16;

/// Length, in units, of the longest common prefix of `a` and `b`.
///
/// Exact, not chunk-rounded: the chunked loop only skips *whole* chunks that
/// compared equal, and the scalar loop that follows always resolves the
/// mismatching chunk unit by unit.
#[inline]
pub(crate) fn common_prefix_len<T: Unit>(a: &[T], b: &[T]) -> usize {
    let shared = a.len().min(b.len());
    let mut i = 0usize;
    while i + AFFIX_CHUNK <= shared && a[i..i + AFFIX_CHUNK] == b[i..i + AFFIX_CHUNK] {
        i += AFFIX_CHUNK;
    }
    while i < shared && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Length, in units, of the longest common suffix of `a` and `b`.
///
/// Same exactness guarantee as [`common_prefix_len`], walking inward from the
/// two operands' ends (which need not be at the same absolute index, since the
/// operands may differ in length).
#[inline]
pub(crate) fn common_suffix_len<T: Unit>(a: &[T], b: &[T]) -> usize {
    let shared = a.len().min(b.len());
    let (la, lb) = (a.len(), b.len());
    let mut i = 0usize;
    while i + AFFIX_CHUNK <= shared
        && a[la - i - AFFIX_CHUNK..la - i] == b[lb - i - AFFIX_CHUNK..lb - i]
    {
        i += AFFIX_CHUNK;
    }
    while i < shared && a[la - 1 - i] == b[lb - 1 - i] {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// Bit-parallel pattern-match tables (Peq)
// ---------------------------------------------------------------------------

/// Pattern-match-table (`Peq`) construction for the bit-parallel kernels,
/// specialised per unit type the way [`Unit::Map`] already specialises the
/// unrestricted-Damerau last-occurrence map: bytes get flat arrays (a probe
/// is one indexed load, no hashing), scalars get `FxHashMap` (the
/// fastest hasher already in this crate's dependency tree — a `std`
/// `HashMap` probe was a measured constant-factor cost in the original
/// single-word kernel).
///
/// Two table shapes: [`BitPeq::Table1`] maps a unit to one 64-bit mask
/// (single-word kernels, pattern ≤ 64 units); [`BitPeq::TableN`] maps a
/// unit to a contiguous `blocks`-length row of masks (multi-word kernels).
/// `TableN` stores only the rows of units actually present in the pattern —
/// for a pattern with `d` distinct units the table holds `d × blocks` words
/// instead of a fixed `256 × blocks` matrix, keeping the whole structure
/// cache-resident for real text; absent units resolve to `None`, which the
/// kernels map to a shared all-zero row.
pub(crate) trait BitPeq: Unit + std::hash::Hash {
    /// Single-word table: unit → one mask.
    type Table1;
    fn peq1(pattern: &[Self]) -> Self::Table1;
    fn peq1_get(table: &Self::Table1, unit: Self) -> u64;

    /// Multi-word table: unit → a `blocks`-length row of masks.
    type TableN;
    fn peqn(pattern: &[Self], blocks: usize) -> Self::TableN;
    fn peqn_row(table: &Self::TableN, unit: Self) -> Option<&[u64]>;
}

impl BitPeq for u8 {
    type Table1 = [u64; 256];

    fn peq1(pattern: &[Self]) -> Self::Table1 {
        let mut table = [0u64; 256];
        for (i, &c) in pattern.iter().enumerate() {
            table[c as usize] |= 1u64 << i;
        }
        table
    }

    #[inline]
    fn peq1_get(table: &Self::Table1, unit: Self) -> u64 {
        table[unit as usize]
    }

    // Flat 256-entry index into the packed rows; `u32::MAX` is the vacant
    // sentinel (a row start can never reach it: patterns are string-length
    // bounded).
    type TableN = ([u32; 256], Vec<u64>, usize);

    fn peqn(pattern: &[Self], blocks: usize) -> Self::TableN {
        let mut index = [u32::MAX; 256];
        let mut rows: Vec<u64> = Vec::new();
        for (i, &c) in pattern.iter().enumerate() {
            let slot = &mut index[c as usize];
            if *slot == u32::MAX {
                *slot = rows.len() as u32;
                rows.resize(rows.len() + blocks, 0);
            }
            rows[*slot as usize + i / 64] |= 1u64 << (i % 64);
        }
        (index, rows, blocks)
    }

    #[inline]
    fn peqn_row(table: &Self::TableN, unit: Self) -> Option<&[u64]> {
        let start = table.0[unit as usize];
        (start != u32::MAX).then(|| {
            let s = start as usize;
            &table.1[s..s + table.2]
        })
    }
}

impl BitPeq for char {
    type Table1 = FxHashMap<char, u64>;

    fn peq1(pattern: &[Self]) -> Self::Table1 {
        let mut table = FxHashMap::default();
        for (i, &c) in pattern.iter().enumerate() {
            *table.entry(c).or_insert(0u64) |= 1u64 << i;
        }
        table
    }

    #[inline]
    fn peq1_get(table: &Self::Table1, unit: Self) -> u64 {
        table.get(&unit).copied().unwrap_or(0)
    }

    type TableN = (FxHashMap<char, u32>, Vec<u64>, usize);

    fn peqn(pattern: &[Self], blocks: usize) -> Self::TableN {
        let mut index: FxHashMap<char, u32> = FxHashMap::default();
        let mut rows: Vec<u64> = Vec::new();
        for (i, &c) in pattern.iter().enumerate() {
            let slot = index.entry(c).or_insert_with(|| {
                let start = rows.len() as u32;
                rows.resize(rows.len() + blocks, 0);
                start
            });
            rows[*slot as usize + i / 64] |= 1u64 << (i % 64);
        }
        (index, rows, blocks)
    }

    #[inline]
    fn peqn_row(table: &Self::TableN, unit: Self) -> Option<&[u64]> {
        table.0.get(&unit).map(|&start| {
            let s = start as usize;
            &table.1[s..s + table.2]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_yields_the_scalar_sequence() {
        // The unit is the Unicode scalar value, so the promoted slices are
        // exactly `s.chars()` — one element per scalar, astral characters
        // included. Counted from the definition of UTF-8, not from the code:
        // "a😀b" is three scalars, "𝕳𝖊𝖑𝖑𝖔" is five.
        for (a, b, la, lb) in [
            ("a😀b", "ab", 3usize, 2usize),
            ("𝕳𝖊𝖑𝖑𝖔", "", 5, 0),
            ("café", "cafe", 4, 4),
            ("Москва", "Москва", 6, 6),
        ] {
            let (ga, gb) = dispatch(a, b, |ops| match ops {
                Operands::Bytes(x, y) => (x.len(), y.len()),
                Operands::Units(x, y) => {
                    assert_eq!(x, a.chars().collect::<Vec<char>>().as_slice());
                    assert_eq!(y, b.chars().collect::<Vec<char>>().as_slice());
                    (x.len(), y.len())
                }
            });
            assert_eq!((ga, gb), (la, lb), "for {a:?} / {b:?}");
        }
    }

    #[test]
    fn ascii_takes_the_byte_path() {
        // The unit change must cost the common case nothing: ASCII operands
        // still reach the `u8` kernels, whose flat `[usize; 256]` and
        // `[u64; 256]` tables are width-dependent and were left verbatim.
        // For ASCII, one byte *is* one scalar by definition, so the byte
        // arm computes the identical answer on a narrower type.
        for (a, b) in [
            ("abc", "abd"),
            ("", ""),
            ("", "abc"),
            ("abc", ""),
            ("\u{0}\u{7f}", "\u{7f}\u{0}"), // the extremes of ASCII
            (
                "the quick brown fox jumps over the lazy dog",
                "the quick brown fox",
            ),
        ] {
            let took_bytes = dispatch(a, b, |ops| matches!(ops, Operands::Bytes(..)));
            assert!(took_bytes, "{a:?} / {b:?} must take the byte path");
        }
    }

    #[test]
    fn non_ascii_promotes_both_operands() {
        // One non-ASCII operand promotes the pair, in either position: the
        // two views cannot be mixed, so a `&[u8]` operand and a `&[char]`
        // operand is not a representable state.
        for (a, b) in [
            ("abc", "café"),
            ("café", "abc"),
            ("😀", "a"),
            ("a", "😀"),
            ("", "é"),
            ("é", ""),
        ] {
            let took_units = dispatch(a, b, |ops| matches!(ops, Operands::Units(..)));
            assert!(
                took_units,
                "one non-ASCII operand must promote the pair: {a:?} / {b:?}"
            );
        }
    }

    #[test]
    fn byte_map_roundtrips() {
        let mut m = u8::new_map();
        assert_eq!(m.get(b'a'), None);
        m.set(b'a', 7);
        assert_eq!(m.get(b'a'), Some(7));
        assert_eq!(m.get(b'b'), None);
    }

    #[test]
    fn unit_map_roundtrips() {
        let mut m = char::new_map();
        assert_eq!(UnitMap::get(&m, '😀'), None);
        m.set('😀', 9);
        assert_eq!(UnitMap::get(&m, '😀'), Some(9));
        // A distinct astral scalar is a distinct key: the map is keyed on the
        // whole scalar, not on a surrogate half the two would share.
        assert_eq!(UnitMap::get(&m, '😁'), None);
    }

    // -- BitPeq table batteries ---------------------------------------------

    /// The definitionally-correct mask for `unit` in `pattern`, block `b`:
    /// bit `i % 64` of word `i / 64` set iff `pattern[i] == unit`. Computed
    /// by a naive loop with none of the packed-table machinery, so any
    /// off-by-one in slot allocation, block offsets or bit positions in the
    /// real tables disagrees with it.
    fn naive_mask<T: Copy + PartialEq>(pattern: &[T], unit: T, block: usize) -> u64 {
        let mut mask = 0u64;
        for (i, &c) in pattern.iter().enumerate() {
            if i / 64 == block && c == unit {
                mask |= 1u64 << (i % 64);
            }
        }
        mask
    }

    #[test]
    fn bitpeq_u8_tables_match_naive_masks() {
        // A pattern spanning three blocks where the same bytes recur in
        // non-adjacent blocks (so a packed row's high words are written long
        // after its slot was allocated), including the extreme byte values
        // 0 and 255 (first and last index-table entries).
        let mut pattern: Vec<u8> = Vec::new();
        for i in 0..150usize {
            pattern.push([0u8, b'a', 255, b'b', b'a'][i % 5]);
        }
        // Tail lands the last block partially filled.
        pattern.extend_from_slice(&[255, 0, b'z']);
        let blocks = pattern.len().div_ceil(64);
        assert_eq!(blocks, 3);

        let t1 = <u8 as BitPeq>::peq1(&pattern[..60]);
        for unit in [0u8, b'a', 255, b'b', b'z', b'q', 7] {
            assert_eq!(
                <u8 as BitPeq>::peq1_get(&t1, unit),
                naive_mask(&pattern[..60], unit, 0),
                "peq1 mismatch for byte {unit}"
            );
        }

        let tn = <u8 as BitPeq>::peqn(&pattern, blocks);
        for unit in [0u8, b'a', 255, b'b', b'z'] {
            let row = <u8 as BitPeq>::peqn_row(&tn, unit).expect("present unit must have a row");
            assert_eq!(row.len(), blocks);
            for (b, &word) in row.iter().enumerate() {
                assert_eq!(
                    word,
                    naive_mask(&pattern, unit, b),
                    "peqn mismatch for byte {unit}, block {b}"
                );
            }
        }
        for absent in [b'q', 1u8, 254] {
            assert!(
                <u8 as BitPeq>::peqn_row(&tn, absent).is_none(),
                "absent byte {absent} must resolve to None"
            );
        }
    }

    #[test]
    fn bitpeq_char_tables_match_naive_masks() {
        // Same battery for the FxHashMap-backed `char` tables, with unit
        // values at both extremes of the scalar range (U+0000 and
        // U+10FFFF), astral values on both sides of the BMP boundary, and
        // more than 300 distinct units so the packed rows vector grows
        // through many slot allocations.
        let mut pattern: Vec<char> = Vec::new();
        for i in 0..320u32 {
            // U+0100.. avoids the surrogate range entirely; 320 distinct
            // units spanning blocks 0..=4.
            pattern.push(char::from_u32(0x0100 + i).expect("BMP non-surrogate"));
        }
        for i in 0..70usize {
            // Recurrences of early units far from their first block, plus
            // the extremes and two astral characters.
            pattern.push(['\u{0}', '\u{105}', '\u{10FFFF}', '😀', '𝕳', '\u{111}'][i % 6]);
        }
        let blocks = pattern.len().div_ceil(64);

        let head = &pattern[..48];
        let t1 = <char as BitPeq>::peq1(head);
        for unit in [
            '\u{0}',
            '\u{100}',
            '\u{105}',
            '\u{12F}',
            '\u{130}',
            '\u{10FFFF}',
        ] {
            assert_eq!(
                <char as BitPeq>::peq1_get(&t1, unit),
                naive_mask(head, unit, 0),
                "peq1 mismatch for unit {unit:?}"
            );
        }

        let tn = <char as BitPeq>::peqn(&pattern, blocks);
        for unit in [
            '\u{0}',
            '\u{100}',
            '\u{105}',
            '\u{111}',
            '\u{23F}',
            '\u{10FFFF}',
            '😀',
            '𝕳',
        ] {
            let row = <char as BitPeq>::peqn_row(&tn, unit).expect("present unit must have a row");
            assert_eq!(row.len(), blocks);
            for (b, &word) in row.iter().enumerate() {
                assert_eq!(
                    word,
                    naive_mask(&pattern, unit, b),
                    "peqn mismatch for unit {unit:?}, block {b}"
                );
            }
        }
        assert!(<char as BitPeq>::peqn_row(&tn, 'z').is_none());
        assert!(<char as BitPeq>::peqn_row(&tn, '\u{10FFFE}').is_none());
    }

    #[test]
    fn bitpeq_packed_rows_are_disjoint_per_unit() {
        // Two units alternating across several blocks: their packed rows
        // must never alias (a slot-allocation bug that reused or overlapped
        // row storage would merge their masks). The union of all rows must
        // equal the all-positions mask, and rows must be pairwise disjoint
        // per block.
        let pattern: Vec<u8> = (0..200).map(|i| b"xy"[i % 2]).collect();
        let blocks = pattern.len().div_ceil(64);
        let tn = <u8 as BitPeq>::peqn(&pattern, blocks);
        let rx = <u8 as BitPeq>::peqn_row(&tn, b'x').unwrap().to_vec();
        let ry = <u8 as BitPeq>::peqn_row(&tn, b'y').unwrap().to_vec();
        for b in 0..blocks {
            assert_eq!(rx[b] & ry[b], 0, "rows alias in block {b}");
            let expected_union = naive_mask(&pattern, b'x', b) | naive_mask(&pattern, b'y', b);
            assert_eq!(rx[b] | ry[b], expected_union, "union wrong in block {b}");
        }
    }

    // -- Common-affix scans -------------------------------------------------

    /// The definitional scans: one unit at a time, no chunking, no length
    /// bookkeeping. Any chunk-rounding, tail-loop or reversed-index bug in
    /// the real implementations disagrees with these.
    fn naive_prefix<T: PartialEq>(a: &[T], b: &[T]) -> usize {
        let mut i = 0;
        while i < a.len().min(b.len()) && a[i] == b[i] {
            i += 1;
        }
        i
    }

    fn naive_suffix<T: PartialEq>(a: &[T], b: &[T]) -> usize {
        let mut i = 0;
        while i < a.len().min(b.len()) && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
            i += 1;
        }
        i
    }

    /// Deterministic PRNG, same shape as the sibling modules' helpers.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    #[test]
    fn affix_scans_match_the_naive_walk_at_chunk_boundaries() {
        // Every affix length from 0 through several chunk widths, against
        // both equal-length and unequal-length operands: the chunked loop
        // must hand off to the scalar tail at exactly the right unit, and
        // the suffix scan must index from each operand's own end.
        for shared in 0usize..=(AFFIX_CHUNK * 3 + 5) {
            for extra_a in [0usize, 1, 7, AFFIX_CHUNK, AFFIX_CHUNK + 3] {
                for extra_b in [0usize, 1, 5, AFFIX_CHUNK + 1] {
                    let mut a: Vec<u8> = vec![b'a'; shared];
                    let mut b: Vec<u8> = vec![b'a'; shared];
                    a.extend(std::iter::repeat_n(b'x', extra_a));
                    b.extend(std::iter::repeat_n(b'y', extra_b));
                    assert_eq!(
                        common_prefix_len(&a, &b),
                        naive_prefix(&a, &b),
                        "prefix shared={shared} extra=({extra_a},{extra_b})"
                    );
                    // Mirrored operands exercise the suffix scan over the
                    // same shapes.
                    let ra: Vec<u8> = a.iter().rev().copied().collect();
                    let rb: Vec<u8> = b.iter().rev().copied().collect();
                    assert_eq!(
                        common_suffix_len(&ra, &rb),
                        naive_suffix(&ra, &rb),
                        "suffix shared={shared} extra=({extra_a},{extra_b})"
                    );
                }
            }
        }
    }

    #[test]
    fn affix_scans_match_the_naive_walk_on_random_pairs() {
        // Randomized, both monomorphizations, tiny alphabets so shared
        // affixes arise by chance at every length rather than only when
        // planted — including the degenerate empty and single-unit cases.
        let mut rng = SplitMix64(0xAFF1_0000_5CA4_0001);
        for round in 0..4000 {
            let la = rng.next_range(120);
            let lb = rng.next_range(120);
            let alphabet: &[u8] = [&b"a"[..], b"ab", b"abc"][round % 3];
            let a: Vec<u8> = (0..la)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            let b: Vec<u8> = (0..lb)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            assert_eq!(common_prefix_len(&a, &b), naive_prefix(&a, &b));
            assert_eq!(common_suffix_len(&a, &b), naive_suffix(&a, &b));

            // Mapped into the astral plane, so the `char` monomorphization
            // is exercised on values a UTF-16 unit could never hold.
            let map = |c: u8| char::from_u32(0x1_F600 + u32::from(c)).expect("astral scalar");
            let ua: Vec<char> = a.iter().map(|&c| map(c)).collect();
            let ub: Vec<char> = b.iter().map(|&c| map(c)).collect();
            assert_eq!(common_prefix_len(&ua, &ub), naive_prefix(&ua, &ub));
            assert_eq!(common_suffix_len(&ua, &ub), naive_suffix(&ua, &ub));
        }
    }

    #[test]
    fn affix_scans_never_exceed_the_shorter_operand() {
        // A prefix of one operand: the scan must stop at the shorter
        // length, never read past it, and the two scans must be able to
        // overlap-claim the same units (callers apply the prefix cut before
        // the suffix scan, precisely so they cannot).
        let a = b"abcabcabcabcabcabcabcabc";
        let b = b"abcabcabc";
        assert_eq!(common_prefix_len(a.as_slice(), b.as_slice()), b.len());
        assert_eq!(common_suffix_len(a.as_slice(), b.as_slice()), b.len());
        assert_eq!(common_prefix_len(b"".as_slice(), a.as_slice()), 0);
        assert_eq!(common_suffix_len(b"".as_slice(), a.as_slice()), 0);
        assert_eq!(common_prefix_len(a.as_slice(), a.as_slice()), a.len());
        assert_eq!(common_suffix_len(a.as_slice(), a.as_slice()), a.len());
    }
}
