//! Every index key is reachable — enumerated, not sampled.
//!
//! The defect this suite exists to make impossible is a lookup pipeline that
//! transforms a word into a key the index is not spelled with, so that entries
//! which are unquestionably in the file report "not found". Checking a handful
//! of representative words cannot detect it: the words that survive are
//! precisely the ones a sample is made of.
//!
//! So this suite walks **every** entry of **every** index file it is given
//! through the exact pipeline [`WordNet::lookup`] uses — `index_key`, then the
//! binary search — and asserts that each one comes back. It runs against:
//!
//! * a generated dictionary whose keys enumerate the lemma alphabet
//!   `wndb(5WN)` documents, exhaustively at one and two characters and over a
//!   sub-alphabet at three, plus the shapes a binary search is most likely to
//!   step over: prefix chains, adjacent keys differing in the last byte, the
//!   first and last lines of the file, and keys far longer than the rest;
//! * the real Princeton dictionary, which is separately licensed and
//!   deliberately not vendored.
//!
//! # Why the real-dictionary tests are `#[ignore]`d
//!
//! They cannot pass without data that is not in this repository, and a test
//! that returns early when its subject is absent is *worse* than no test: it
//! reports as a pass and is counted as coverage. `#[ignore = "…"]` is the one
//! mechanism the harness reports itself — every run prints the test name
//! followed by `ignored` and the reason, and the summary line says `N ignored`
//! rather than counting it among the passes. A `println!` inside a returning
//! test says the same thing into a buffer the harness discards unless somebody
//! remembered `--nocapture`.
//!
//! So absence is reported by the harness, and inside the test absence is a
//! panic: run with `--ignored` and no dictionary, and it fails. There is no
//! path on which these tests pass without having read Princeton WordNet.
//!
//! ```text
//! WORDNET_DB_PATH=/usr/share/wordnet cargo test -p verbora-wordnet -- --ignored --nocapture
//! ```
//!
//! Counts are printed (`cargo test -p verbora-wordnet -- --nocapture`) so the
//! size of what was enumerated is visible rather than asserted in the dark.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use verbora_wordnet::{Config, IndexFile, PartOfSpeech, Storage, WordNet, index_key};

/// The lemma alphabet of `wndb(5WN)` — "lower case ASCII text of word or
/// collocation", with the punctuation WordNet 3.x lemmas actually contain —
/// listed in ascending byte order, which is the order index files are sorted in.
const ALPHABET: &str = "'-./0123456789_abcdefghijklmnopqrstuvwxyz";

/// The sub-alphabet used for the exhaustive three-character sweep, chosen to
/// span every byte-order neighbourhood of [`ALPHABET`].
const SUB_ALPHABET: &str = "'-./0159_abcemyz";

/// Every key the generated dictionary contains, sorted in ASCII byte order and
/// free of duplicates.
fn generated_keys() -> Vec<String> {
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    let sub: Vec<char> = SUB_ALPHABET.chars().collect();
    assert_eq!(alphabet.len(), 41);
    assert_eq!(sub.len(), 16);

    let mut keys: BTreeSet<String> = BTreeSet::new();

    // Exhaustive at one and two characters over the whole alphabet.
    for &a in &alphabet {
        keys.insert(a.to_string());
        for &b in &alphabet {
            keys.insert([a, b].iter().collect());
        }
    }
    // Exhaustive at three characters over the sub-alphabet.
    for &a in &sub {
        for &b in &sub {
            for &c in &sub {
                keys.insert([a, b, c].iter().collect());
            }
        }
    }
    // Prefix chains: every prefix of a long key is itself a key, which is the
    // shape a search that compares a truncated field gets wrong.
    for n in 1..=64 {
        keys.insert("a".repeat(n));
        keys.insert(format!("{}z", "q".repeat(n)));
    }
    // Realistic collocations, and lemmas exercising each punctuation character.
    for word in [
        "new_york",
        "new_york_city",
        "united_states_of_america",
        "u.s.a.",
        "x-ray",
        "9/11",
        "'tween",
        "1080",
        "b-52",
        "st._louis",
        "o'clock",
        "well-being",
        "a_a",
        "a_b_c",
    ] {
        keys.insert(word.to_owned());
    }
    // A key far longer than any other, so the forward line read has to stitch
    // chunks on the positioned-read backend.
    keys.insert("z".repeat(5000));

    keys.into_iter().collect()
}

/// Keys deliberately absent from the generated dictionary, to check the search
/// does not report a neighbour as a hit.
fn absent_keys(present: &BTreeSet<String>) -> Vec<String> {
    let mut absent = Vec::new();
    let mut push = |candidate: String| {
        // The candidate must be absent *after* normalisation, since that is
        // the form the lookup will actually search for.
        if !present.contains(index_key(&candidate).as_ref()) {
            absent.push(candidate);
        }
    };
    for base in ["a", "m", "z", "aa", "zz", "new_york", "u.s.a.", "9/11"] {
        for suffix in ["!", "~", "zzz", "_q", "0q", "~1"] {
            push(format!("{base}{suffix}"));
        }
    }
    for candidate in ["", " ", "\u{FFFD}", "café", "日本語", "😀", "AAA!"] {
        push(candidate.to_owned());
    }
    absent
}

/// How many senses a generated key gets: a deterministic 1..=24, from an
/// FNV-1a hash of the key itself.
///
/// Line length has to **vary**, and vary a lot, or the generated file is not a
/// realistic index at all. A real `index.noun` line is 25 bytes for a lemma
/// with one sense and several hundred for one with thirty, and a search that
/// bisects byte positions rather than lines is only wrong when lines differ in
/// length — with uniform lines it accidentally behaves like a line search. The
/// search this crate replaced found every key of a fixed-width version of this
/// same file and missed 434 of 5,956 (7.29%) once the widths varied.
fn sense_count(key: &str) -> usize {
    let mut hash: u32 = 2_166_136_261;
    for byte in key.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    1 + (hash % 24) as usize
}

/// Writes a dictionary whose `index.noun` carries `keys`, each with
/// [`sense_count`] senses, all pointing at the single synset in `data.noun`.
///
/// The other three file pairs are minimal but well-formed, because
/// [`WordNet::open`] opens all eight.
fn build_dictionary(keys: &[String]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-enumeration-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    // `wndb(5WN)`: header lines begin with two spaces so that they sort below
    // every lemma and the binary search steps over them.
    let header = "  1 Generated for verbora-wordnet's enumeration suite\n  2 Not WordNet data: the file format only\n";
    let synset = "00000097 06 n 01 thing 0 000 | a generated synset  \n";
    assert_eq!(header.len(), 97, "the header fixes the synset offset");

    let mut index = String::from(header);
    for key in keys {
        let senses = sense_count(key);
        index.push_str(key);
        index.push_str(&format!(" n {senses} 0 {senses} 0"));
        for _ in 0..senses {
            index.push_str(" 00000097");
        }
        index.push_str("  \n");
    }
    let data = format!("{header}{synset}");

    for pos in PartOfSpeech::ALL {
        let suffix = pos.file_suffix();
        let tag = pos.tag();
        if pos == PartOfSpeech::Noun {
            std::fs::write(dir.join("index.noun"), &index).unwrap();
            std::fs::write(dir.join("data.noun"), &data).unwrap();
        } else {
            std::fs::write(
                dir.join(format!("index.{suffix}")),
                format!("{header}thing {tag} 1 0 1 0 00000097  \n"),
            )
            .unwrap();
            std::fs::write(
                dir.join(format!("data.{suffix}")),
                format!("{header}00000097 06 {tag} 01 thing 0 000 | a generated synset  \n"),
            )
            .unwrap();
        }
    }
    dir
}

/// Walks every entry of `index` through `index_key` and the binary search,
/// returning `(entries, unreachable, rewritten)`.
///
/// * `unreachable` — entries the search could not find again.
/// * `rewritten` — entries whose own lemma is not a fixed point of
///   [`index_key`], which is the shape that makes a key unreachable in the
///   first place.
fn audit(index: &IndexFile) -> (usize, Vec<String>, Vec<String>) {
    let mut entries = 0usize;
    let mut unreachable = Vec::new();
    let mut rewritten = Vec::new();

    for entry in index.entries() {
        let entry = entry.expect("every line of an index file must parse");
        entries += 1;

        if index_key(&entry.lemma) != entry.lemma {
            rewritten.push(entry.lemma.clone());
        }
        // The exact pipeline `WordNet::lookup` runs: normalise, then search.
        let found = index
            .entry(&index_key(&entry.lemma))
            .expect("the search must not fail");
        match found {
            Some(hit) if hit == entry => {}
            _ => unreachable.push(entry.lemma.clone()),
        }
    }
    (entries, unreachable, rewritten)
}

#[test]
fn every_generated_key_is_reachable_under_every_storage_backend() {
    let keys = generated_keys();
    let present: BTreeSet<String> = keys.iter().cloned().collect();
    let dir = build_dictionary(&keys);

    println!(
        "enumeration: {} generated index keys, {} bytes of index.noun",
        keys.len(),
        std::fs::metadata(dir.join("index.noun")).unwrap().len()
    );

    for storage in [
        Storage::Resident,
        Storage::Indexed,
        Storage::LazyResident,
        Storage::Pread,
    ] {
        let wn = WordNet::open_with(&dir, &Config::new(storage)).unwrap();
        let (entries, unreachable, rewritten) = audit(wn.index_file(PartOfSpeech::Noun));

        assert_eq!(entries, keys.len(), "{storage:?}: entry count");
        assert!(
            rewritten.is_empty(),
            "{storage:?}: {} keys are not fixed points of index_key: {:?}",
            rewritten.len(),
            &rewritten[..rewritten.len().min(10)]
        );
        assert!(
            unreachable.is_empty(),
            "{storage:?}: {} of {} keys unreachable, e.g. {:?}",
            unreachable.len(),
            entries,
            &unreachable[..unreachable.len().min(10)]
        );
        println!("enumeration: {storage:?} — {entries}/{entries} keys reachable, 0 unreachable");
    }

    // And the full pipeline, not only the index layer: every key resolves to
    // the one synset the generated dictionary contains.
    let wn = WordNet::open(&dir).unwrap();
    for key in &keys {
        let senses = wn.senses(key, PartOfSpeech::Noun).unwrap();
        assert_eq!(senses.len(), sense_count(key), "{key:?}");
        assert!(senses.iter().all(|s| s.lemma() == "thing"), "{key:?}");
    }

    // No false positives: a key that is not in the file is not found.
    let absent = absent_keys(&present);
    assert!(absent.len() >= 30, "{} absent keys", absent.len());
    for key in &absent {
        assert!(
            wn.index_entry(key, PartOfSpeech::Noun).unwrap().is_none(),
            "absent key {key:?} was reported as a hit"
        );
    }
    println!(
        "enumeration: {} deliberately absent keys, 0 false hits",
        absent.len()
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Uppercase and whitespace spellings of every generated key must resolve to
/// the same entry, because that is exactly what `index_key` is for.
#[test]
fn every_generated_key_is_reachable_from_its_uppercase_and_padded_spellings() {
    let keys = generated_keys();
    let dir = build_dictionary(&keys);
    let wn = WordNet::open(&dir).unwrap();

    let mut checked = 0usize;
    for key in &keys {
        let expected = wn
            .index_entry(key, PartOfSpeech::Noun)
            .unwrap()
            .unwrap_or_else(|| panic!("{key:?} must be present"));

        let mut spellings = vec![key.to_ascii_uppercase(), format!("  {key}  ")];
        // Writing a collocation with spaces instead of underscores must reach
        // the same entry — but only where the key's underscores are interior
        // and unrepeated, since `index_key` collapses a run to one `_` and
        // trims the edges, so `a__b` and `_a` are keys no spelling produces.
        if key.contains('_') && !key.starts_with('_') && !key.ends_with('_') && !key.contains("__")
        {
            spellings.push(key.replace('_', " "));
            spellings.push(key.replace('_', "   "));
        }
        for spelling in spellings {
            let found = wn.index_entry(&spelling, PartOfSpeech::Noun).unwrap();
            assert_eq!(
                found.as_ref(),
                Some(&expected),
                "{spelling:?} should resolve to {key:?}"
            );
            checked += 1;
        }
    }
    println!("enumeration: {checked} alternative spellings all resolved");
    std::fs::remove_dir_all(&dir).ok();
}

/// Reason attached to every `#[ignore]` in this file, and the panic message
/// when one of those tests is run anyway without a dictionary.
const NEEDS_DICT: &str = "needs the separately-licensed Princeton WordNet database, which is not \
                          vendored: point $WORDNET_DB_PATH (or $VERBORA_WORDNET_DICT) at a \
                          directory holding index.noun and its seven siblings, or drop one at \
                          ./dict, then re-run with --ignored";

/// The installed dictionary, or a panic naming what is missing.
///
/// Deliberately not an `Option`: a caller that could handle `None` would handle
/// it by returning, and a test that returns early because its subject is absent
/// is counted as a pass. See this file's "Why the real-dictionary tests are
/// `#[ignore]`d".
fn real_dict_dir() -> PathBuf {
    for var in ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            let dir = PathBuf::from(value);
            if dir.join("index.noun").is_file() {
                return dir;
            }
        }
    }
    let local = Path::new("dict");
    assert!(local.join("index.noun").is_file(), "{NEEDS_DICT}");
    local.to_owned()
}

#[test]
#[ignore = "needs the separately-licensed Princeton WordNet database; set $WORDNET_DB_PATH and re-run with --ignored"]
fn every_entry_of_the_real_dictionary_is_reachable() {
    let dir = real_dict_dir();

    let wn = WordNet::open(&dir).unwrap();
    let mut total = 0usize;
    for pos in PartOfSpeech::ALL {
        let index = wn.index_file(pos);
        let (entries, unreachable, rewritten) = audit(index);
        total += entries;
        println!(
            "enumeration: index.{} — {entries} entries, {} unreachable, {} not fixed points",
            pos.file_suffix(),
            unreachable.len(),
            rewritten.len()
        );
        assert!(
            rewritten.is_empty(),
            "index.{}: {} lemmas are rewritten by index_key, e.g. {:?}",
            pos.file_suffix(),
            rewritten.len(),
            &rewritten[..rewritten.len().min(20)]
        );
        assert!(
            unreachable.is_empty(),
            "index.{}: {} of {entries} lemmas unreachable, e.g. {:?}",
            pos.file_suffix(),
            unreachable.len(),
            &unreachable[..unreachable.len().min(20)]
        );
    }
    println!("enumeration: {total} real index entries, all reachable");
    assert!(total > 0);
}

/// Every synset in the real dictionary must parse, and every pointer must
/// resolve to a synset that exists.
#[test]
#[ignore = "needs the separately-licensed Princeton WordNet database; set $WORDNET_DB_PATH and re-run with --ignored"]
fn every_synset_of_the_real_dictionary_parses() {
    let dir = real_dict_dir();

    let wn = WordNet::open(&dir).unwrap();
    let mut synsets = 0usize;
    let mut pointers = 0usize;
    for pos in PartOfSpeech::ALL {
        for synset in wn.data_file(pos).synsets() {
            let synset = synset.unwrap_or_else(|e| panic!("data.{}: {e}", pos.file_suffix()));
            synsets += 1;
            pointers += synset.pointers.len();
        }
    }
    println!("enumeration: {synsets} synsets parsed, {pointers} pointers");
    assert!(synsets > 0);
}

// ---------------------------------------------------------------------------
// The normalisation path, on input where it is not the identity
// ---------------------------------------------------------------------------

/// A dictionary whose lemmas are legal index keys, but whose *spellings* a user
/// would type are not.
///
/// The generated alphabet above is deliberately made of strings `index_key`
/// leaves alone, which is what makes it a fair test of reachability — and what
/// makes it no test at all of the transform itself. These keys are chosen so
/// that every probe below has to be rewritten before it can match: an ASCII
/// case fold, a whitespace run collapsed to one `_`, an edge trimmed, or all
/// three at once.
const NORMALISED_KEYS: &[&str] = &[
    "'tween",
    "a_a",
    "b-52",
    // Not lower case, and not ASCII: `index_key` folds case in ASCII only, so
    // `É` survives and this key is reachable from `CAFÉ` and from nothing else.
    "cafÉ",
    "entity",
    "new_york",
    "new_york_city",
    "st._louis",
    "u.s.a.",
    "x-ray",
];

/// `(spelling a user might type, the key it must normalise to)`.
///
/// Every left-hand side is asserted to be *changed* by `index_key`, so a
/// regression that made the function the identity would fail here rather than
/// quietly agreeing with every case.
const SPELLINGS: &[(&str, &str)] = &[
    // ASCII case only.
    ("ENTITY", "entity"),
    ("EnTiTy", "entity"),
    ("'TWEEN", "'tween"),
    ("B-52", "b-52"),
    // Case in ASCII, and not beyond it.
    ("CAFÉ", "cafÉ"),
    // Edges trimmed.
    ("  entity  ", "entity"),
    ("\tentity\n", "entity"),
    // One space is one `_`.
    ("new york", "new_york"),
    ("A A", "a_a"),
    // A run of whitespace is still one `_`.
    ("new   york", "new_york"),
    ("new \t york", "new_york"),
    // Whitespace is Unicode's definition, not ASCII's: a non-breaking space
    // between two words is still a word boundary.
    ("new\u{00A0}york", "new_york"),
    ("new\u{3000}york", "new_york"),
    ("new\u{2003}york", "new_york"),
    // Case, trimming, a run and a non-ASCII space, all in one string.
    ("\u{2003}NEW \t york\u{00A0}CITY \n", "new_york_city"),
    ("  U.S.A.  ", "u.s.a."),
    ("ST. LOUIS", "st._louis"),
    (" X-Ray ", "x-ray"),
];

/// Spellings whose normalised form is not a key in the dictionary, so that the
/// transform is not being credited for finding neighbours.
const ABSENT_SPELLINGS: &[&str] = &[
    "NEW YORK CITIES",
    "  entities  ",
    "café",      // lower-case `é`: a different key from `cafÉ`
    "CAFE",      // no accent at all
    "_new_york", // a leading `_` is not what trimming produces
    "new__york", // a doubled `_` is not what a run collapses to
    "X RAY",     // the stored key is hyphenated, not underscored
];

#[test]
fn index_key_normalisation_reaches_entries_no_verbatim_key_would() {
    let keys: Vec<String> = {
        let mut k: Vec<String> = NORMALISED_KEYS.iter().map(|s| (*s).to_owned()).collect();
        // Index files are sorted in the ASCII collating sequence, which for
        // `String` is exactly its `Ord`.
        k.sort();
        k
    };
    let dir = build_dictionary(&keys);

    for storage in [
        Storage::Resident,
        Storage::Indexed,
        Storage::LazyResident,
        Storage::Pread,
    ] {
        let wn = WordNet::open_with(&dir, &Config::new(storage)).unwrap();
        let index = wn.index_file(PartOfSpeech::Noun);

        for (spelling, key) in SPELLINGS {
            // The point of the whole test: this input is *not* a fixed point.
            assert_ne!(
                index_key(spelling),
                **spelling,
                "{storage:?}: {spelling:?} is unchanged by index_key, so it tests nothing"
            );
            assert_eq!(index_key(spelling), **key, "{storage:?}: {spelling:?}");
            // Idempotent: normalising a key again is a no-op.
            assert_eq!(
                index_key(&index_key(spelling)),
                **key,
                "{storage:?}: {spelling:?}"
            );

            // A *word* is normalised, so the spelling reaches the entry...
            let expected = index
                .entry(key)
                .unwrap()
                .unwrap_or_else(|| panic!("{storage:?}: {key:?} must be present"));
            let found = wn
                .index_entry(spelling, PartOfSpeech::Noun)
                .unwrap()
                .unwrap_or_else(|| panic!("{storage:?}: {spelling:?} should reach {key:?}"));
            assert_eq!(found, expected, "{storage:?}: {spelling:?}");
            assert_eq!(found.lemma, **key, "{storage:?}: {spelling:?}");

            // ...and a *key* is not, so the same string used verbatim is a
            // miss. That split is the contract, not an accident of the search.
            assert!(
                index.entry(spelling).unwrap().is_none(),
                "{storage:?}: IndexFile::entry({spelling:?}) must not normalise its argument"
            );

            // The full pipeline, not just the index layer.
            let senses = wn.senses(spelling, PartOfSpeech::Noun).unwrap();
            assert_eq!(senses.len(), sense_count(key), "{storage:?}: {spelling:?}");
            assert!(
                senses.iter().all(|s| s.lemma() == "thing"),
                "{storage:?}: {spelling:?}"
            );
        }

        for spelling in ABSENT_SPELLINGS {
            assert!(
                wn.index_entry(spelling, PartOfSpeech::Noun)
                    .unwrap()
                    .is_none(),
                "{storage:?}: {spelling:?} normalises to {:?}, which is not a stored key",
                index_key(spelling)
            );
        }
    }

    println!(
        "enumeration: {} rewritten spellings over {} keys, {} absent, on 4 backends",
        SPELLINGS.len(),
        keys.len(),
        ABSENT_SPELLINGS.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}
