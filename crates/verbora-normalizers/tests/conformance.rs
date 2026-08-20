//! UAX #15 conformance: the UCD's own `NormalizationTest.txt`, replayed as
//! data.
//!
//! `tests/data/NormalizationTest.txt` is taken verbatim from the Unicode
//! Character Database for the version this crate is built against (17.0.0) and
//! is redistributed under the Unicode terms of use recorded in its own header
//! (<https://www.unicode.org/terms_of_use.html>).
//!
//! This is what pins the Unicode version Verbora claims. A dependency upgrade
//! that moves any mapping fails here, loudly and by name, which is the
//! mechanism the "stamp your persisted keys" obligation in the crate
//! documentation depends on — a silent mapping change is exactly the failure
//! that corrupts an index built before the upgrade.
//!
//! Two obligations, and the second is the one a fixture set drawn from the
//! file's own rows can never discharge:
//!
//! 1. **Conformance clause 1** — the five columns of each row, cross-checked
//!    against all four forms, exactly as the file's header states them.
//! 2. **Conformance clause 2** — every code point *not* listed in Part 1 is its
//!    own NFC, NFD, NFKC and NFKD. This is where a table-driven implementation
//!    with one spurious or missing entry fails, and no row of the file reaches
//!    it.

use verbora_normalizers::{nfc, nfd, nfkc, nfkd};

/// One row of the file: the five columns, as `String`s.
struct Row {
    c: [String; 5],
    line: usize,
    part: u8,
}

/// Parses `NormalizationTest.txt`.
///
/// Data lines are five `;`-terminated columns of space-separated hex code
/// points, optionally followed by a `#` comment. `@PartN` lines start a new
/// section and carry no data.
fn parse(body: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut part = 0u8;
    for (i, raw) in body.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Part") {
            part = rest
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("line {}: bad part marker {rest:?}: {e}", i + 1));
            continue;
        }
        let cols: Vec<&str> = line.trim_end_matches(';').split(';').collect();
        assert_eq!(
            cols.len(),
            5,
            "line {}: expected five columns, got {}",
            i + 1,
            cols.len()
        );
        let mut c: [String; 5] = Default::default();
        for (slot, col) in c.iter_mut().zip(cols) {
            for hex in col.split_whitespace() {
                let cp = u32::from_str_radix(hex, 16)
                    .unwrap_or_else(|e| panic!("line {}: bad code point {hex:?}: {e}", i + 1));
                slot.push(char::from_u32(cp).unwrap_or_else(|| {
                    panic!("line {}: {hex:?} is not a Unicode scalar value", i + 1)
                }));
            }
        }
        rows.push(Row {
            c,
            line: i + 1,
            part,
        });
    }
    assert!(!rows.is_empty(), "the conformance file parsed to nothing");
    rows
}

fn rows() -> Vec<Row> {
    parse(include_str!("data/NormalizationTest.txt"))
}

/// Conformance clause 1, verbatim from the file's own header:
///
/// ```text
/// NFC:  c2 == toNFC(c1) == toNFC(c2) == toNFC(c3)
///       c4 == toNFC(c4) == toNFC(c5)
/// NFD:  c3 == toNFD(c1) == toNFD(c2) == toNFD(c3)
///       c5 == toNFD(c4) == toNFD(c5)
/// NFKC: c4 == toNFKC(c1) == toNFKC(c2) == toNFKC(c3) == toNFKC(c4) == toNFKC(c5)
/// NFKD: c5 == toNFKD(c1) == toNFKD(c2) == toNFKD(c3) == toNFKD(c4) == toNFKD(c5)
/// ```
#[test]
fn normalization_test_clause_1() {
    let rows = rows();
    assert!(
        rows.len() > 18_000,
        "expected the full file, got {} rows",
        rows.len()
    );
    for row in &rows {
        let [c1, c2, c3, c4, c5] = &row.c;
        let at = format!(
            "NormalizationTest.txt line {} (Part {})",
            row.line, row.part
        );

        for src in [c1, c2, c3] {
            assert_eq!(nfc(src), c2.as_str(), "toNFC({src:?}) — {at}");
            assert_eq!(nfd(src), c3.as_str(), "toNFD({src:?}) — {at}");
        }
        for src in [c4, c5] {
            assert_eq!(nfc(src), c4.as_str(), "toNFC({src:?}) — {at}");
            assert_eq!(nfd(src), c5.as_str(), "toNFD({src:?}) — {at}");
        }
        for src in [c1, c2, c3, c4, c5] {
            assert_eq!(nfkc(src), c4.as_str(), "toNFKC({src:?}) — {at}");
            assert_eq!(nfkd(src), c5.as_str(), "toNFKD({src:?}) — {at}");
        }
    }
}

/// Conformance clause 2, verbatim from the file's own header:
///
/// > For every code point X assigned in this version of Unicode that is not
/// > specifically listed in Part 1, the following invariants must be true for
/// > all conformant implementations:
/// >
/// > `X == toNFC(X) == toNFD(X) == toNFKC(X) == toNFKD(X)`
///
/// Swept over every Unicode scalar value, not only the assigned ones: an
/// unassigned code point has no decomposition either, so the identity holds
/// there too and the wider sweep is the stronger claim.
///
/// This is the obligation a fixture set drawn from the file's rows cannot
/// reach. Every row of Part 1 says what *does* change; only this says that
/// nothing else does.
#[test]
fn normalization_test_clause_2() {
    let listed: std::collections::HashSet<char> = rows()
        .iter()
        .filter(|r| r.part == 1)
        .map(|r| {
            let mut it = r.c[0].chars();
            let c = it.next().expect("a Part 1 source column is one code point");
            assert!(
                it.next().is_none(),
                "Part 1 line {}: source column is not a single code point",
                r.line
            );
            c
        })
        .collect();
    assert!(
        listed.len() > 15_000,
        "expected Part 1 to list thousands of code points, got {}",
        listed.len()
    );

    let mut checked = 0u32;
    for cp in 0..=0x10_FFFFu32 {
        let Some(c) = char::from_u32(cp) else {
            continue; // surrogate code point; not a scalar value
        };
        if listed.contains(&c) {
            continue;
        }
        let s = c.to_string();
        assert_eq!(nfc(&s), s, "toNFC(U+{cp:04X})");
        assert_eq!(nfd(&s), s, "toNFD(U+{cp:04X})");
        assert_eq!(nfkc(&s), s, "toNFKC(U+{cp:04X})");
        assert_eq!(nfkd(&s), s, "toNFKD(U+{cp:04X})");
        checked += 1;
    }
    assert!(checked > 1_000_000, "swept only {checked} scalars");
}

/// The `Cow` iff-guarantee, checked against the conformance corpus rather than
/// against hand-picked inputs — every column of every row is a string that is
/// in exactly one, two or none of the four forms, which is the spread a
/// hand-written fixture set does not produce.
#[test]
fn cow_borrows_exactly_when_nothing_changed() {
    for row in &rows() {
        for src in &row.c {
            for (name, got) in [
                ("nfc", nfc(src)),
                ("nfd", nfd(src)),
                ("nfkc", nfkc(src)),
                ("nfkd", nfkd(src)),
            ] {
                assert_eq!(
                    matches!(got, std::borrow::Cow::Borrowed(_)),
                    got.as_ref() == src.as_str(),
                    "{name}({src:?}) — NormalizationTest.txt line {}",
                    row.line
                );
            }
        }
    }
}

/// The tripwire for a dependency bump: `tests/data/NormalizationTest.txt` is
/// version-specific, so a version change without a matching data refresh means
/// the suite above is silently checking the wrong standard.
#[test]
fn unicode_version_matches_the_conformance_data() {
    assert_eq!(
        verbora_normalizers::unicode_version(),
        (17, 0, 0),
        "tests/data/NormalizationTest.txt is the 17.0.0 file; refresh it from \
         https://www.unicode.org/Public/<version>/ucd/ and update the version \
         recorded in the crate documentation"
    );
}
