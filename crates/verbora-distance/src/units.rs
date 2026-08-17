//! Text-unit dispatch: the mechanism that buys exact the reference parity without
//! paying for it on ordinary input.
//!
//! # The problem
//!
//! The reference strings are sequences of UTF-16 code units. Every positional
//! operation in the reference — `s.length`, `s[i]`, `s.slice(i, j)` — therefore
//! counts code units, and characters outside the Basic Multilingual Plane count
//! as **two**. Rust's `char` is a Unicode scalar value, so a straightforward port
//! disagrees with the reference on any input containing astral-plane characters:
//!
//! ```text
//! LevenshteinDistance("a😀b", "ab")
//!   The reference : 2   (lengths 4 and 2; two surrogate halves deleted)
//!   Rust chars : 1   (lengths 3 and 2; one character deleted)
//! ```
//!
//! That is a genuine, observable divergence, not a theoretical one — it is
//! recorded in `fixtures/distance.json`.
//!
//! # The solution
//!
//! Rather than force every algorithm onto `Vec<u16>` (which would allocate and
//! slow down the 99.9% case), the algorithms in this crate are written once,
//! generically over `&[T]`, and [`dispatch`] picks the narrowest representation
//! that is *provably identical* to UTF-16 for the given input:
//!
//! | Input                        | Representation | Allocates |
//! |------------------------------|----------------|-----------|
//! | both operands ASCII          | `&[u8]`        | no        |
//! | otherwise                    | `Vec<u16>`     | yes       |
//!
//! ASCII is checked with [`str::is_ascii`], which is vectorised. For ASCII input
//! one byte *is* one code unit, so the fast path is not an approximation — it is
//! the same computation on a narrower type.

use rustc_hash::FxHashMap;

/// A symbol an algorithm can operate on: either a byte or a UTF-16 code unit.
///
/// The bound is deliberately minimal so that the algorithms stay monomorphisable
/// down to tight loops over primitives.
pub trait Unit: Copy + PartialEq + Eq {
    /// A map from unit to `usize`, used by unrestricted Damerau–Levenshtein to
    /// remember the last row each symbol appeared in.
    ///
    /// Bytes get a flat 256-entry array (no hashing, perfect cache behaviour);
    /// code units fall back to a hash map.
    type Map: UnitMap<Self>;

    /// Creates an empty map.
    fn new_map() -> Self::Map;
}

/// A symbol-to-row map, abstracted so the byte path can avoid hashing entirely.
pub trait UnitMap<T> {
    /// Returns the stored row for `key`, if present.
    fn get(&self, key: T) -> Option<usize>;
    /// Stores `row` for `key`.
    fn set(&mut self, key: T, row: usize);
    /// Empties the map.
    fn clear(&mut self);
}

/// Flat array map for byte alphabets: a lookup is one indexed load.
#[derive(Debug)]
pub struct ByteMap {
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

    #[inline]
    fn clear(&mut self) {
        self.rows = [usize::MAX; 256];
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

impl UnitMap<u16> for FxHashMap<u16, usize> {
    #[inline]
    fn get(&self, key: u16) -> Option<usize> {
        FxHashMap::get(self, &key).copied()
    }

    #[inline]
    fn set(&mut self, key: u16, row: usize) {
        self.insert(key, row);
    }

    #[inline]
    fn clear(&mut self) {
        FxHashMap::clear(self);
    }
}

impl Unit for u16 {
    type Map = FxHashMap<u16, usize>;

    fn new_map() -> Self::Map {
        FxHashMap::default()
    }
}

/// Runs `f` on `a` and `b` in the narrowest representation that matches
/// the reference's UTF-16 semantics exactly.
///
/// Both operands are promoted together: mixing a `&[u8]` view with a `&[u16]`
/// view is not possible, so if either operand needs UTF-16, both are converted.
#[inline]
pub fn dispatch<R>(a: &str, b: &str, f: impl for<'x> FnOnce(Operands<'x>) -> R) -> R {
    if a.is_ascii() && b.is_ascii() {
        f(Operands::Bytes(a.as_bytes(), b.as_bytes()))
    } else {
        let ua: Vec<u16> = a.encode_utf16().collect();
        let ub: Vec<u16> = b.encode_utf16().collect();
        f(Operands::Units(&ua, &ub))
    }
}

/// A pair of operands in a single, consistent representation.
#[derive(Debug, Clone, Copy)]
pub enum Operands<'a> {
    /// Both operands are ASCII; one byte is one UTF-16 code unit.
    Bytes(&'a [u8], &'a [u8]),
    /// General case: explicit UTF-16 code units.
    Units(&'a [u16], &'a [u16]),
}

/// The number of UTF-16 code units in `s` — the reference's `String#length`.
///
/// Uses the ASCII fast path where possible; otherwise counts without allocating
/// by adding one extra unit per astral-plane character.
#[inline]
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    s.chars().map(|c| c.len_utf16()).sum()
}

// ---------------------------------------------------------------------------
// Bit-parallel pattern-match tables (Peq)
// ---------------------------------------------------------------------------

/// Pattern-match-table (`Peq`) construction for the bit-parallel kernels,
/// specialised per unit type the way [`Unit::Map`] already specialises the
/// unrestricted-Damerau last-occurrence map: bytes get flat arrays (a probe
/// is one indexed load, no hashing), UTF-16 units get `FxHashMap` (the
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

impl BitPeq for u16 {
    type Table1 = FxHashMap<u16, u64>;

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

    type TableN = (FxHashMap<u16, u32>, Vec<u64>, usize);

    fn peqn(pattern: &[Self], blocks: usize) -> Self::TableN {
        let mut index: FxHashMap<u16, u32> = FxHashMap::default();
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
    fn utf16_len_matches_utf16_count() {
        for s in ["", "abc", "café", "Москва", "😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"] {
            assert_eq!(utf16_len(s), s.encode_utf16().count(), "for {s:?}");
        }
    }

    #[test]
    fn ascii_takes_the_byte_path() {
        let took_bytes = dispatch("abc", "abd", |ops| matches!(ops, Operands::Bytes(..)));
        assert!(took_bytes);
    }

    #[test]
    fn non_ascii_promotes_both_operands() {
        let took_units = dispatch("abc", "café", |ops| matches!(ops, Operands::Units(..)));
        assert!(took_units, "one non-ASCII operand must promote the pair");
    }

    #[test]
    fn byte_map_roundtrips() {
        let mut m = u8::new_map();
        assert_eq!(m.get(b'a'), None);
        m.set(b'a', 7);
        assert_eq!(m.get(b'a'), Some(7));
        m.clear();
        assert_eq!(m.get(b'a'), None);
    }

    #[test]
    fn unit_map_roundtrips() {
        let mut m = u16::new_map();
        assert_eq!(UnitMap::get(&m, 0x1234), None);
        m.set(0x1234, 9);
        assert_eq!(UnitMap::get(&m, 0x1234), Some(9));
        m.clear();
        assert_eq!(UnitMap::get(&m, 0x1234), None);
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
    fn bitpeq_u16_tables_match_naive_masks() {
        // Same battery for the FxHashMap-backed u16 tables, with unit
        // values at both extremes (0 and 0xFFFF), surrogate-range values
        // (astral halves), and more than 300 distinct units so the packed
        // rows vector grows through many slot allocations.
        let mut pattern: Vec<u16> = Vec::new();
        for i in 0..320usize {
            pattern.push(i as u16); // 320 distinct units, blocks 0..=4
        }
        for i in 0..70usize {
            // Recurrences of early units far from their first block, plus
            // extremes and surrogate halves.
            pattern.push([0u16, 5, 0xFFFF, 0xD83D, 0xDE00, 17][i % 6]);
        }
        let blocks = pattern.len().div_ceil(64);

        let head = &pattern[..48];
        let t1 = <u16 as BitPeq>::peq1(head);
        for unit in [0u16, 5, 17, 47, 48, 0xFFFF] {
            assert_eq!(
                <u16 as BitPeq>::peq1_get(&t1, unit),
                naive_mask(head, unit, 0),
                "peq1 mismatch for unit {unit}"
            );
        }

        let tn = <u16 as BitPeq>::peqn(&pattern, blocks);
        for unit in [0u16, 5, 17, 100, 319, 0xFFFF, 0xD83D, 0xDE00] {
            let row = <u16 as BitPeq>::peqn_row(&tn, unit).expect("present unit must have a row");
            assert_eq!(row.len(), blocks);
            for (b, &word) in row.iter().enumerate() {
                assert_eq!(
                    word,
                    naive_mask(&pattern, unit, b),
                    "peqn mismatch for unit {unit}, block {b}"
                );
            }
        }
        assert!(<u16 as BitPeq>::peqn_row(&tn, 999).is_none());
        assert!(<u16 as BitPeq>::peqn_row(&tn, 0xFFFE).is_none());
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
}
