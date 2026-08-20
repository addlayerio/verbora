//! Packs the bundled Brill data assets into a compact, zero-startup index.
//!
//! # Why a build script
//!
//! The bundled dictionaries are 4.6 MB of JSON: 92,662 English entries and
//! 11,699 Dutch ones, of which 92,538 and 11,699 survive to be packed. Three shipping strategies were considered:
//!
//! | Strategy | Binary size | Startup |
//! |---|---|---|
//! | `include_str!` the JSON, parse at first use | +4.6 MB | one full parse |
//! | Load the JSON from disk at runtime | +0 | one full parse, plus I/O |
//! | **Pack at build time, `include_bytes!` the index** | **+2.4 MB** | **none** |
//!
//! The packed form is a byte-sorted string arena plus offset tables, so a lookup
//! is a binary search directly over bytes embedded in the executable: nothing is
//! parsed, allocated or copied at start-up, and the process pays only for the
//! entries it touches. It is also *smaller* than the JSON, because tags are
//! interned (316 distinct tags cover both languages) and the JSON's punctuation
//! disappears.
//!
//! # Source keys become tokens here
//!
//! The source dictionaries were derived from tagged corpora, and their keys are
//! written in the corpus's own escaped notation rather than as plain tokens: a
//! `/` inside a token appears as `\/`, because a bare `/` separates a token from
//! its tag, and a `*` appears as `\*`. [`decode_key`] undoes that, so the packed
//! key is the token text a caller can actually look up — `Asia/Pacific`, not
//! `Asia\/Pacific`. A key that decoding shows to be markup rather than a token
//! is dropped; see [`decode_key`] for the two shapes of that.
//!
//! # The entry contract is enforced here
//!
//! `verbora_tagger::Lexicon` accepts a key only when it is non-empty and
//! contains no Unicode `White_Space` scalar, and accepts an entry only when it
//! carries at least one tag, none of which is `*`. The decoded entries are
//! filtered against exactly that contract.
//!
//! Every entry the source had and the crate does not ship is counted, per
//! language and per reason, into a constant `data::tests` asserts — so the loss
//! is a number in a test rather than a claim in a comment.
//!
//! # Layout
//!
//! All integers little-endian. Entries are stored **sorted by key bytes**, which
//! for well-formed UTF-8 is the same order as by Unicode scalar value, so a
//! lookup is a plain binary search with no permutation table.
//!
//! ```text
//! 0   magic "LEX2"
//! 4   n_entries u32, n_tags u32
//! 12  tag_off, tag_bytes, key_off, key_bytes, val_off, val_ids   (u32 each)
//! 36  tag_off[n_tags+1]     u32  -> tag_bytes
//!     tag_bytes             u8   interned tag strings
//!     key_off[n_entries+1]  u32  -> key_bytes
//!     key_bytes             u8   keys, ascending by byte order
//!     val_off[n_entries+1]  u32  -> val_ids (in u16 units)
//!     val_ids               u16  tag indices
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};

/// A JSON object deserialised into a `Vec` so that duplicate detection and
/// ordering are the packer's decision rather than `serde_json`'s.
struct Entries(Vec<(String, Vec<String>)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Entries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object mapping words to tag arrays")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Entries, A::Error> {
                let mut out = Vec::with_capacity(m.size_hint().unwrap_or(1024));
                while let Some((k, v)) = m.next_entry::<String, Vec<String>>()? {
                    out.push((k, v));
                }
                Ok(Entries(out))
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

/// The literal contract `verbora_tagger::Tag` and `verbora_tagger::Word`
/// enforce: non-empty, and no scalar with the Unicode `White_Space` property.
fn is_valid_literal(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_whitespace)
}

/// [`is_valid_literal`], plus the restriction only a tag carries: `*` is the
/// wildcard pattern of a rule string, so it is not available as a tag.
fn is_valid_tag(s: &str) -> bool {
    is_valid_literal(s) && s != "*"
}

/// Decodes one source key out of the escaped notation the source corpora write
/// their tokens in.
///
/// A tagged corpus separates a token from its tag with `/` and marks a null
/// element with `*`, so a token whose own text contains either character writes
/// it escaped: `Asia\/Pacific` is the token `Asia/Pacific`, and `M\*A\*S\*H`
/// is `M*A*S*H`. Decoding is the inverse, and nothing else is escaped.
///
/// `None` means the key is not a token at all but a piece of the corpus's own
/// markup, and there are two shapes of that:
///
/// * a `\` with nothing escapable after it. That is what the left half of an
///   `A\/B` token looks like once something has truncated it at the `/`, and
///   the tags on it belong to the whole compound (`Asia\` carries `JJ`, the tag
///   of `Asia\/Pacific`), not to the fragment.
/// * a bare `/` with text on both sides — the corpus's own word/tag separator,
///   left in the key together with the tag it introduced: `me/PRP`, `na/TO`,
///   `W/NNP.R.G.`. Those tags contradict the entry's own tag list, so neither
///   half of such an entry can be trusted. A lone `/` is not a separator (a
///   separator has a word before it and a tag after it); it is the punctuation
///   token, and it is kept.
fn decode_key(key: &str) -> Option<String> {
    let mut out = String::with_capacity(key.len());
    let mut chars = key.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some(&(_, escaped @ ('/' | '*'))) => {
                    out.push(escaped);
                    chars.next();
                }
                _ => return None,
            },
            '/' if at > 0 && at + 1 < key.len() => return None,
            _ => out.push(c),
        }
    }
    Some(out)
}

/// What packing one dictionary produced, and everything the source lost on the
/// way, so the crate can assert the counts instead of trusting them.
struct Prepared {
    entries: Vec<(String, Vec<String>)>,
    /// Source keys that were corpus markup rather than tokens.
    not_tokens: Vec<String>,
    /// Source keys that decoded onto a key another entry already held.
    merged: Vec<String>,
    /// Source keys the lexicon entry contract rejected.
    rejected: Vec<String>,
}

/// Decodes, de-duplicates and filters one dictionary's source entries.
///
/// When two source keys decode onto the same token the entry that needed no
/// decoding wins, and the decoded one is dropped rather than merged. Merging
/// would have to concatenate two tag lists whose relative frequencies are not
/// recorded anywhere, and the order of a lexicon entry *is* its frequency
/// ranking — `Lexicon::primary_tag` reads the first tag. Inventing that order is
/// worse than keeping the entry that was already spelled as its own token.
fn prepare(entries: Vec<(String, Vec<String>)>) -> Prepared {
    let mut decoded = Vec::with_capacity(entries.len());
    let mut not_tokens = Vec::new();
    for (key, tags) in entries {
        match decode_key(&key) {
            Some(token) => decoded.push((token, key, tags)),
            None => not_tokens.push(key),
        }
    }
    // Sort so that within one decoded key the entry that needed no decoding
    // comes first, and the rest in a source order that does not depend on the
    // JSON reader.
    decoded.sort_by(|a, b| {
        a.0.as_bytes()
            .cmp(b.0.as_bytes())
            .then_with(|| (a.0 != a.1).cmp(&(b.0 != b.1)))
            .then_with(|| a.1.as_bytes().cmp(b.1.as_bytes()))
    });

    let mut kept: Vec<(String, Vec<String>)> = Vec::with_capacity(decoded.len());
    let mut merged = Vec::new();
    let mut rejected = Vec::new();
    for (token, source, tags) in decoded {
        if kept.last().is_some_and(|(k, _)| *k == token) {
            merged.push(source);
        } else if is_valid_literal(&token)
            && !tags.is_empty()
            && tags.iter().all(|t| is_valid_tag(t))
        {
            kept.push((token, tags));
        } else {
            rejected.push(source);
        }
    }
    Prepared {
        entries: kept,
        not_tokens,
        merged,
        rejected,
    }
}

/// Serialises one dictionary into the packed index format.
fn pack(mut entries: Vec<(String, Vec<String>)>) -> Vec<u8> {
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for pair in entries.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "duplicate key in source JSON");
    }
    let n = entries.len();

    // ---- Tag interning, in first-seen order over the sorted entries ---------
    let mut tag_ids: BTreeMap<&str, u16> = BTreeMap::new();
    let mut tags: Vec<&str> = Vec::new();
    for (_, ts) in &entries {
        for t in ts {
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
    for (key, ts) in &entries {
        key_off.push(u32::try_from(key_bytes.len()).unwrap());
        key_bytes.extend_from_slice(key.as_bytes());
        val_off.push(u32::try_from(val_ids.len()).unwrap());
        for t in ts {
            val_ids.push(tag_ids[t.as_str()]);
        }
    }
    key_off.push(u32::try_from(key_bytes.len()).unwrap());
    val_off.push(u32::try_from(val_ids.len()).unwrap());

    // ---- Assemble ----------------------------------------------------------
    const HEADER: usize = 36;
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

    let mut out = Vec::with_capacity(off);
    out.extend_from_slice(b"LEX2");
    for v in [
        u32::try_from(n).unwrap(),
        u32::try_from(tags.len()).unwrap(),
        off_tag_off,
        off_tag_bytes,
        off_key_off,
        off_key_bytes,
        off_val_off,
        off_val_ids,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(out.len(), HEADER);
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
    assert_eq!(out.len(), off);
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

    let mut generated = String::from("// @generated by build.rs — do not edit.\n");

    for (src, dst, prefix) in [
        (
            "English/lexicon_from_reference.json",
            "english.lex",
            "ENGLISH",
        ),
        ("Dutch/brill_Lexicon.json", "dutch.lex", "DUTCH"),
    ] {
        let entries: Entries = serde_json::from_str(&read(&data.join(src)))
            .unwrap_or_else(|e| panic!("cannot parse {src}: {e}"));
        let source_count = entries.0.len();
        let prepared = prepare(entries.0);
        let dropped = prepared.not_tokens.len() + prepared.merged.len() + prepared.rejected.len();
        if dropped > 0 {
            println!(
                "cargo:warning={src}: packed {} of {source_count} entries \
                 ({} corpus markup, {} merged by decoding, {} rejected by the entry contract)",
                prepared.entries.len(),
                prepared.not_tokens.len(),
                prepared.merged.len(),
                prepared.rejected.len(),
            );
        }
        for (name, doc, value) in [
            (
                "SOURCE_ENTRIES",
                "Entries in the source JSON, before anything was dropped.",
                source_count,
            ),
            (
                "KEYS_NOT_TOKENS",
                "Source keys that were corpus markup rather than tokens.",
                prepared.not_tokens.len(),
            ),
            (
                "KEYS_MERGED",
                "Source keys that decoded onto a key another entry already held.",
                prepared.merged.len(),
            ),
            (
                "ENTRIES_REJECTED",
                "Source entries that the lexicon entry contract rejected.",
                prepared.rejected.len(),
            ),
        ] {
            writeln!(generated, "/// {doc} (`{src}`)").unwrap();
            writeln!(generated, "#[allow(dead_code)] // read by `data::tests`.").unwrap();
            writeln!(
                generated,
                "pub(crate) const {prefix}_{name}: usize = {value};"
            )
            .unwrap();
        }
        std::fs::write(out.join(dst), pack(prepared.entries)).expect("write packed lexicon");
    }

    for (src, name, doc) in [
        (
            "English/tr_from_reference.json",
            "ENGLISH_RULES",
            "The 18 bundled English transformation rules (`data/English/tr_from_reference.json`).",
        ),
        (
            "Dutch/brill_CONTEXTRULES.json",
            "DUTCH_RULES",
            "The 274 bundled Dutch transformation rules (`data/Dutch/brill_CONTEXTRULES.json`).",
        ),
        (
            "English/tr_from_brill_paper.json",
            "BRILL_PAPER_RULES",
            "The 10 rules published in Brill (1992), Table 1 (`data/English/tr_from_brill_paper.json`).",
        ),
    ] {
        let f: RuleFile = serde_json::from_str(&read(&data.join(src)))
            .unwrap_or_else(|e| panic!("cannot parse {src}: {e}"));
        writeln!(generated, "/// {doc}").unwrap();
        writeln!(generated, "pub(crate) static {name}: &[&str] = &[").unwrap();
        for r in &f.rules {
            writeln!(generated, "    {r:?},").unwrap();
        }
        writeln!(generated, "];").unwrap();
    }
    std::fs::write(out.join("generated.rs"), generated).expect("write generated.rs");
}
