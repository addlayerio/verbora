//! Packs the bundled Brill data assets into a compact, zero-startup index.
//!
//! # Why a build script
//!
//! The reference `require()`s 4.6 MB of JSON at module load: 92,662 English lexicon
//! entries and 11,699 Dutch ones. Three shipping strategies were considered:
//!
//! | Strategy | Binary size | Startup | Notes |
//! |---|---|---|---|
//! | `include_str!` the JSON, parse at first use | +4.6 MB | ~55 ms | also needs an order-preserving map |
//! | Load the JSON from disk at runtime | +0 | ~55 ms + I/O | needs the files installed beside the binary |
//! | **Pack at build time, `include_bytes!` the index** | **+2.4 MB** | **~0** | chosen |
//!
//! The packed form is a sorted string arena plus offset tables, so a lookup is a
//! binary search directly over the bytes embedded in the executable: nothing is
//! parsed, allocated, or copied at startup, and the process pays only for the
//! entries it actually touches. It is also *smaller* than JSON, because tags
//! are interned (122 distinct tags cover all 112,502 English tag references) and
//! The JSON's punctuation and whitespace disappear.
//!
//! # Key order is part of the format
//!
//! The reference enumerates array-index-like keys first, in ascending numeric
//! order, then every other key in insertion order — and that order is observable
//! through `Lexicon.prettyPrint()`, `Object.keys`, and the sequence of
//! `addWord` calls `Corpus.buildLexicon()` makes. Entries are therefore stored
//! in the reference enumeration order, with `n_numeric` recording the length of the
//! hoisted numeric prefix so the runtime can splice newly added numeric keys
//! into it. A separate permutation table gives the sorted order used for lookup.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};

/// A JSON object deserialised into a `Vec` so that key order survives.
///
/// `serde_json::Map` is a `BTreeMap` unless the `preserve_order` feature is on,
/// and enabling that feature workspace-wide to satisfy one build script would be
/// a poor trade. `MapAccess` visits JSON entries in document order, so a
/// fifteen-line visitor gets the ordering guarantee without the dependency.
struct OrderedEntries(Vec<(String, Vec<String>)>);

impl<'de> Deserialize<'de> for OrderedEntries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = OrderedEntries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object mapping words to tag arrays")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<OrderedEntries, A::Error> {
                let mut out = Vec::with_capacity(m.size_hint().unwrap_or(1024));
                while let Some((k, v)) = m.next_entry::<String, Vec<String>>()? {
                    out.push((k, v));
                }
                Ok(OrderedEntries(out))
            }
        }
        d.deserialize_map(V)
    }
}

/// `{ "rules": [...] }`, the shape of all three bundled rule files.
#[derive(Deserialize)]
struct RuleFile {
    rules: Vec<String>,
}

/// Whether `key` is what the reference language calls an array index: the canonical decimal
/// form of an integer in `0 ..= 2^32 - 2`.
///
/// These are the keys the reference engine hoists to the front of `Object.keys`. `"01"`, `"-1"`,
/// `"1.0"` and `"4294967295"` are *not* array indices and stay in insertion
/// order — a distinction the Dutch lexicon's `"1"`…`"2000"` keys make visible.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    (n < u64::from(u32::MAX)).then_some(n as u32)
}

/// Serialises one dictionary into the packed index format.
fn pack(entries: &[(String, Vec<String>)]) -> Vec<u8> {
    // ---- the reference enumeration order -------------------------------------
    let mut numeric: BTreeMap<u32, usize> = BTreeMap::new();
    let mut plain: Vec<usize> = Vec::new();
    for (i, (k, _)) in entries.iter().enumerate() {
        match array_index(k) {
            Some(n) => {
                numeric.insert(n, i);
            }
            None => plain.push(i),
        }
    }
    let order: Vec<usize> = numeric.values().copied().chain(plain).collect();
    let n_numeric = numeric.len();
    let n = order.len();
    assert_eq!(n, entries.len(), "duplicate keys in source JSON");

    // ---- Tag interning -----------------------------------------------------
    let mut tag_ids: BTreeMap<&str, u16> = BTreeMap::new();
    let mut tags: Vec<&str> = Vec::new();
    for i in &order {
        for t in &entries[*i].1 {
            if !tag_ids.contains_key(t.as_str()) {
                tag_ids.insert(
                    t.as_str(),
                    u16::try_from(tags.len()).expect("<= 65536 tags"),
                );
                tags.push(t);
            }
        }
    }

    // ---- Section payloads --------------------------------------------------
    let mut tag_off = Vec::with_capacity(tags.len() + 1);
    let mut tag_bytes: Vec<u8> = Vec::new();
    for t in &tags {
        tag_off.push(u32::try_from(tag_bytes.len()).unwrap());
        tag_bytes.extend_from_slice(t.as_bytes());
    }
    tag_off.push(u32::try_from(tag_bytes.len()).unwrap());

    let mut key_off = Vec::with_capacity(n + 1);
    let mut key_bytes: Vec<u8> = Vec::new();
    let mut val_off = Vec::with_capacity(n + 1);
    let mut val_ids: Vec<u16> = Vec::new();
    for i in &order {
        key_off.push(u32::try_from(key_bytes.len()).unwrap());
        key_bytes.extend_from_slice(entries[*i].0.as_bytes());
        val_off.push(u32::try_from(val_ids.len()).unwrap());
        for t in &entries[*i].1 {
            val_ids.push(tag_ids[t.as_str()]);
        }
    }
    key_off.push(u32::try_from(key_bytes.len()).unwrap());
    val_off.push(u32::try_from(val_ids.len()).unwrap());

    // Permutation giving byte-order-sorted keys, for binary search.
    let mut sorted: Vec<u32> = (0..u32::try_from(n).unwrap()).collect();
    sorted.sort_by(|a, b| {
        entries[order[*a as usize]]
            .0
            .as_bytes()
            .cmp(entries[order[*b as usize]].0.as_bytes())
    });

    // ---- Assemble ----------------------------------------------------------
    const HEADER: usize = 44;
    let mut off = HEADER;
    let mut take = |len: usize| {
        let at = off;
        off += len;
        u32::try_from(at).unwrap()
    };
    let off_tag_off = take(tag_off.len() * 4);
    let off_tag_bytes = take(tag_bytes.len());
    let off_key_off = take(key_off.len() * 4);
    let off_key_bytes = take(key_bytes.len());
    let off_val_off = take(val_off.len() * 4);
    let off_val_ids = take(val_ids.len() * 2);
    let off_sorted = take(sorted.len() * 4);

    let mut out = Vec::with_capacity(off);
    out.extend_from_slice(&u32::from_le_bytes(*b"LEX1").to_le_bytes());
    for v in [
        u32::try_from(n).unwrap(),
        u32::try_from(tags.len()).unwrap(),
        u32::try_from(n_numeric).unwrap(),
        off_tag_off,
        off_tag_bytes,
        off_key_off,
        off_key_bytes,
        off_val_off,
        off_val_ids,
        off_sorted,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(out.len(), HEADER);
    for v in &tag_off {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&tag_bytes);
    for v in &key_off {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&key_bytes);
    for v in &val_off {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &val_ids {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &sorted {
        out.extend_from_slice(&v.to_le_bytes());
    }
    debug_assert_eq!(out.len(), off);
    out
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets this"));
    let data = root.join("data");

    for rel in [
        "English/lexicon_from_reference.json",
        "English/tr_from_reference.json",
        "English/tr_from_brill_paper.json",
        "Dutch/brill_Lexicon.json",
        "Dutch/brill_CONTEXTRULES.json",
    ] {
        println!("cargo:rerun-if-changed=data/{rel}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    for (src, dst) in [
        ("English/lexicon_from_reference.json", "english.lex"),
        ("Dutch/brill_Lexicon.json", "dutch.lex"),
    ] {
        let entries: OrderedEntries = serde_json::from_str(&read(&data.join(src)))
            .unwrap_or_else(|e| panic!("cannot parse {src}: {e}"));
        std::fs::write(out.join(dst), pack(&entries.0)).expect("write packed lexicon");
    }

    let mut rules = String::from("// @generated by build.rs — do not edit.\n");
    for (src, name, doc) in [
        (
            "English/tr_from_reference.json",
            "ENGLISH_RULES",
            "The 18 English transformation rules (`data/English/tr_from_reference.json`).",
        ),
        (
            "Dutch/brill_CONTEXTRULES.json",
            "DUTCH_RULES",
            "The 285 Dutch transformation rules (`data/Dutch/brill_CONTEXTRULES.json`).",
        ),
        (
            "English/tr_from_brill_paper.json",
            "BRILL_PAPER_RULES",
            "The 10 rules from Brill's paper (`data/English/tr_from_brill_paper.json`).",
        ),
    ] {
        let f: RuleFile = serde_json::from_str(&read(&data.join(src)))
            .unwrap_or_else(|e| panic!("cannot parse {src}: {e}"));
        writeln!(rules, "/// {doc}").unwrap();
        writeln!(rules, "pub static {name}: &[&str] = &[").unwrap();
        for r in &f.rules {
            writeln!(rules, "    {r:?},").unwrap();
        }
        writeln!(rules, "];").unwrap();
    }
    std::fs::write(out.join("rules.rs"), rules).expect("write rules");
}
