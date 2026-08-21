//! Turns `src/syllabary.rs` into the lookup index the scanner reads.
//!
//! # Why a build script
//!
//! `AGENTS.md` § *Static and Precomputed Data* and § *Build → Freeze → Query*
//! both point the same way: invariant linguistic data should be laid out once,
//! at build time, into a form a query can read without allocating or parsing.
//! The alternative this replaced was a committed table of 2,953 lines headed
//! "DO NOT EDIT BY HAND" with no generator anywhere in the tree, which made
//! its provenance unverifiable — the exact artifact the Rust-native migration
//! exists to remove.
//!
//! So the reviewable form and the fast form are now different files. The
//! reviewable form is `src/syllabary.rs`: 200-odd `(kana, romaji)` pairs with
//! a citation on every group. The fast form is `$OUT_DIR/index.rs`, which
//! nobody reads and nobody edits, and which cannot disagree with the
//! reviewable form because it is a function of it.
//!
//! # What is derived here rather than written out
//!
//! * **The katakana half.** The Unicode Standard §18.4 encodes Hiragana and
//!   Katakana as parallel repertoires with a constant `0x60` offset across
//!   U+3041..U+3096, so every katakana entry is its hiragana entry shifted.
//!   `HIRAGANA_ONLY` and `KATAKANA_ONLY` supply the characters at each end of
//!   the two blocks that have no counterpart under that offset.
//! * **The long forms.** ALA-LC romanizes a lengthened vowel with a macron, so
//!   the long form of a romanization ending in a vowel is that romanization
//!   with its last letter replaced: `a e i o u` → `ā ē ī ō ū`. A romanization
//!   that does not end in a vowel (`・` → `" "`) has no long form.
//!
//! Not derived here, because they are decided by the *following* mora and so
//! belong to the scanner rather than to a table: the sokuon's doubled
//! consonant, and the syllabic nasal's `n` / `m` / `n'`.
//!
//! # Layout of `index.rs`
//!
//! One flat slot array covering U+3041..=U+30FF — the whole span in which any
//! key can begin — indexed by `code point - 0x3041`. A slot holds the mora for
//! the one-scalar key that is exactly that character, if any, plus a half-open
//! range into a shared array of the two-scalar keys that begin with it. So a
//! lookup is one subtraction, one bounds check, and a linear scan of at most a
//! handful of second characters; a character inside the span that begins no key
//! lands on an empty slot and is rejected without a single comparison against
//! key data.
//!
//! # What this script refuses to build
//!
//! Every check below is a `panic!` rather than a warning, because each one
//! guards an assumption the scanner makes and cannot check for itself:
//!
//! 1. a key outside U+3041..=U+30FF, which the whole-input `0xE3` gate and the
//!    slot array would both silently drop;
//! 2. a key longer than two scalars, which the two-level slot cannot express;
//! 3. a duplicate key, which would make the romanization depend on list order;
//! 4. a key that starts with the sokuon, the syllabic nasal or the prolonged
//!    sound mark, which the scanner handles before it ever consults the table;
//! 5. a romanization that is not ASCII lowercase letters, an apostrophe or a
//!    space, since the scanner reads its first and last bytes directly;
//! 6. a hiragana key whose katakana counterpart is not `0x60` above it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

#[allow(dead_code)]
mod syllabary {
    include!("src/syllabary.rs");
}

use syllabary::{HIRAGANA, HIRAGANA_ONLY, KATAKANA_ONLY, NASAL, PROLONGED_SOUND_MARK, SOKUON};

/// First code point a key may begin with: `ぁ` U+3041 `HIRAGANA LETTER SMALL A`.
const BASE: u32 = 0x3041;
/// Last code point a key may begin with: `ヿ` U+30FF `KATAKANA DIGRAPH KOTO`.
const LAST: u32 = 0x30FF;
/// One slot per code point in `BASE..=LAST`.
const SLOTS: usize = (LAST - BASE + 1) as usize;
/// Distance from a hiragana kana to its katakana counterpart (§18.4).
const KATAKANA_OFFSET: u32 = 0x60;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/syllabary.rs");

    let entries = collect();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("index.rs");
    std::fs::write(&out, render(&entries)).expect("write index.rs");
}

/// Every `(key, romaji)` pair the index will hold, deduplicated and ordered.
///
/// `BTreeMap` rather than a `Vec`: it orders the keys deterministically (so the
/// generated file is byte-stable across builds) and makes the duplicate check
/// exact rather than quadratic.
fn collect() -> BTreeMap<String, String> {
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut insert = |key: String, romaji: &str| {
        check_key(&key);
        check_romaji(romaji);
        if let Some(previous) = entries.insert(key.clone(), romaji.to_owned()) {
            panic!("duplicate key {key:?}: {previous:?} and {romaji:?}");
        }
    };

    for &(kana, romaji) in HIRAGANA {
        insert(kana.to_owned(), romaji);
        insert(to_katakana(kana), romaji);
    }
    for &(kana, romaji) in HIRAGANA_ONLY.iter().chain(KATAKANA_ONLY) {
        insert(kana.to_owned(), romaji);
    }
    entries
}

/// The katakana spelling of a hiragana key, per the constant §18.4 offset.
fn to_katakana(hiragana: &str) -> String {
    hiragana
        .chars()
        .map(|c| {
            let shifted = c as u32 + KATAKANA_OFFSET;
            assert!(
                (0x30A1..=0x30F6).contains(&shifted),
                "{c:?} (U+{:04X}) has no katakana counterpart at +0x{KATAKANA_OFFSET:X}",
                c as u32
            );
            char::from_u32(shifted).expect("katakana code point")
        })
        .collect()
}

/// Rejects a key the scanner could not reach or could not represent.
fn check_key(key: &str) {
    let chars: Vec<char> = key.chars().collect();
    assert!(
        (1..=2).contains(&chars.len()),
        "key {key:?} is {} scalars; the index holds one or two",
        chars.len()
    );
    let first = chars[0];
    assert!(
        (BASE..=LAST).contains(&(first as u32)),
        "key {key:?} begins with U+{:04X}, outside U+{BASE:04X}..=U+{LAST:04X}",
        first as u32
    );
    assert!(
        !SOKUON.contains(&first) && !NASAL.contains(&first) && first != PROLONGED_SOUND_MARK,
        "key {key:?} begins with a mark the scanner resolves before consulting the table"
    );
}

/// Rejects a romanization whose bytes the scanner could not inspect.
fn check_romaji(romaji: &str) {
    assert!(!romaji.is_empty(), "empty romanization");
    assert!(
        romaji
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b' ' || b == b'\''),
        "romanization {romaji:?} is not ASCII lowercase, space or apostrophe"
    );
}

/// The romanization with its final vowel lengthened, if it ends in a vowel.
///
/// ALA-LC writes a long vowel with a macron. The five replacements are
/// U+0101 `ā`, U+0113 `ē`, U+012B `ī`, U+014D `ō` and U+016B `ū`.
fn lengthen(romaji: &str) -> Option<String> {
    let macron = match romaji.as_bytes().last()? {
        b'a' => 'ā',
        b'e' => 'ē',
        b'i' => 'ī',
        b'o' => 'ō',
        b'u' => 'ū',
        _ => return None,
    };
    let mut long = String::with_capacity(romaji.len() + 1);
    long.push_str(&romaji[..romaji.len() - 1]);
    long.push(macron);
    Some(long)
}

/// Renders the slot array and the shared two-scalar array.
fn render(entries: &BTreeMap<String, String>) -> String {
    // Group by first character, keeping the two-scalar keys of one character
    // contiguous so a slot can name them as a range.
    let mut ones: BTreeMap<char, &str> = BTreeMap::new();
    let mut twos: BTreeMap<char, Vec<(char, &str)>> = BTreeMap::new();
    for (key, romaji) in entries {
        let mut chars = key.chars();
        let first = chars.next().expect("non-empty key");
        match chars.next() {
            None => {
                ones.insert(first, romaji);
            }
            Some(second) => twos.entry(first).or_default().push((second, romaji)),
        }
    }

    let mut two_rows = String::new();
    let mut ranges: BTreeMap<char, (usize, usize)> = BTreeMap::new();
    let mut cursor = 0usize;
    for (&first, rows) in &twos {
        let start = cursor;
        for &(second, romaji) in rows {
            let _ = writeln!(two_rows, "    ({:?}, {}),", second, mora(romaji));
            cursor += 1;
        }
        ranges.insert(first, (start, cursor));
    }

    let mut slots = String::new();
    for i in 0..SLOTS {
        let c = char::from_u32(BASE + i as u32).expect("kana code point");
        let one = match ones.get(&c) {
            Some(romaji) => format!("Some({})", mora(romaji)),
            None => "None".to_owned(),
        };
        let (lo, hi) = ranges.get(&c).copied().unwrap_or((0, 0));
        let _ = writeln!(slots, "    Slot {{ one: {one}, two: ({lo}, {hi}) }},");
    }

    format!(
        "// @generated by build.rs from src/syllabary.rs. Do not edit; edit the syllabary.\n\
         use crate::scan::{{Mora, Slot}};\n\
         \n\
         /// Code point that [`SLOTS`]`[0]` describes.\n\
         pub(crate) const SLOT_BASE: u32 = 0x{BASE:04X};\n\
         \n\
         /// One slot per code point in U+{BASE:04X}..=U+{LAST:04X}.\n\
         pub(crate) static SLOTS: [Slot; {SLOTS}] = [\n{slots}];\n\
         \n\
         /// Second scalars of the two-scalar keys, grouped by first scalar.\n\
         pub(crate) static TWO: [(char, Mora); {cursor}] = [\n{two_rows}];\n"
    )
}

/// Renders one `Mora` literal.
fn mora(romaji: &str) -> String {
    match lengthen(romaji) {
        Some(long) => format!("Mora {{ short: {romaji:?}, long: Some({long:?}) }}"),
        None => format!("Mora {{ short: {romaji:?}, long: None }}"),
    }
}
