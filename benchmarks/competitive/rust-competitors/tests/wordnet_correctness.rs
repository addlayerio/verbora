//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/wordnet.rs`.
//!
//! Two crates reading the same eight Princeton files should find the same
//! things in them. That is not a foregone conclusion — they parse the format
//! independently, `wordnet-db` eagerly into `HashMap`s and `verbora-wordnet`
//! on demand out of the raw bytes — and a timing row is meaningless if the two
//! sides are answering differently. So before any number from that file is
//! trusted: same lemmas found, same sense counts, same synset offsets in the
//! same order, same words inside the synsets those offsets name.
//!
//! # This suite skips rather than fails without the dictionary
//!
//! WordNet is separately licensed (Princeton University) and this repository
//! vendors none of it, exactly as `crates/verbora-wordnet` vendors none of it.
//! Every test here reports and returns when the dictionary is absent, the same
//! contract `benches/wordnet.rs` and `crates/verbora-wordnet/benches/wordnet.rs`
//! follow: a missing licence-restricted asset must never fail a suite for
//! everyone else's tests. Install it with
//! `benchmarks/competitive/scripts/fetch-models.sh wordnet-en`, or point
//! `$WORDNET_DB_PATH` at an existing `dict` directory.
//!
//! # Normalisation, and the one place the two crates legitimately differ
//!
//! Both map a caller's word onto the index spelling before searching — Verbora
//! calls it [`index_key`](verbora_wordnet::index_key) and documents it as a
//! named transform; `wordnet-db` does it in a private `normalize_lemma`. The
//! probes below are already in index form (lower case ASCII, `_` between the
//! words of a collocation), so both normalisers are the identity on them and
//! nothing about either is being asserted here. Probing with, say, `New York`
//! would be testing two normalisers against each other, which is a different
//! question from the one the benchmark asks.

use std::path::PathBuf;

use verbora_wordnet::{Config, PartOfSpeech, Storage, WordNet};
use wordnet_db::{LoadMode, WordNet as DbWordNet};
use wordnet_types::Pos;

/// The probes `benches/wordnet.rs` times, plus a handful more: the point of a
/// correctness pass is coverage, and unlike the benchmark it costs nothing to
/// widen.
///
/// Deliberately unremarkable lemmas. The ones that were once *remarkable* —
/// `run`, `cat`, `light`, `water`, `computer`, `node`, `new_york`, every one
/// of which `verbora-wordnet` could not read — live in
/// [`FORMERLY_UNREADABLE`], which asserts more about them than this list does.
const PROBES: &[&str] = &[
    "entity", "dog", "slip", "thing", "school", "tree", "child", "abandon", "zzzzz",
];

/// The lemmas whose index pointer list contains a bare `;` or `-`.
///
/// `verbora-wordnet` could not read these until it learned those two symbols;
/// see [`both_parsers_agree_on_the_formerly_unreadable_entries`].
const FORMERLY_UNREADABLE: &[&str] = &[
    "run", "cat", "light", "water", "computer", "node", "house", "hand", "new_york",
];

/// The dictionary directory, or `None` when it has not been installed.
/// Same resolution order as `benches/wordnet.rs`.
fn dict_dir() -> Option<PathBuf> {
    for var in ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            let dir = PathBuf::from(value);
            if dir.join("index.noun").is_file() {
                return Some(dir);
            }
        }
    }
    let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models");
    for release in ["wordnet-3.1", "wordnet-3.0"] {
        let fetched = models.join(release).join("dict");
        if fetched.join("index.noun").is_file() {
            return Some(fetched);
        }
    }
    None
}

/// Both readers over the same directory, or `None` with a printed notice.
///
/// `LoadMode::Owned` rather than the crate's default `Mmap`: it is the
/// like-for-like partner of Verbora's `Storage::Resident` (both read the files
/// into owned heap buffers, no OS mapping), and the two modes are documented
/// to parse identically, so a correctness pass has no reason to prefer the one
/// that maps.
fn open_both() -> Option<(WordNet, DbWordNet)> {
    let Some(dict) = dict_dir() else {
        eprintln!(
            "wordnet_correctness skipped: no WordNet dictionary found.\n\
             It is separately licensed (Princeton University) and not vendored.\n\
             Fetch it with: benchmarks/competitive/scripts/fetch-models.sh wordnet-en\n\
             or point $WORDNET_DB_PATH at a directory holding `index.noun`."
        );
        return None;
    };
    let verbora = WordNet::open_with(&dict, &Config::new(Storage::Resident))
        .expect("verbora-wordnet opens the dictionary");
    let db =
        DbWordNet::load_with_mode(&dict, LoadMode::Owned).expect("wordnet-db loads the dictionary");
    Some((verbora, db))
}

/// The offsets of every noun sense of `lemma`, in sense order, as plain
/// integers — the one representation both crates can be reduced to without
/// either being flattered.
fn verbora_offsets(wn: &WordNet, lemma: &str) -> Vec<u32> {
    wn.index_entry(lemma, PartOfSpeech::Noun)
        .expect("a well-formed index file")
        .map(|entry| entry.synset_offsets.iter().map(|o| o.get()).collect())
        .unwrap_or_default()
}

fn db_offsets(db: &DbWordNet, lemma: &str) -> Vec<u32> {
    db.synsets_for_lemma(Pos::Noun, lemma)
        .iter()
        .map(|id| id.offset)
        .collect()
}

/// The headline agreement: same senses, same offsets, same order.
///
/// Order matters and is asserted rather than sorted away. `wndb(5WN)` defines
/// an index line's offsets as most-frequently-tagged first, and both crates
/// document that they preserve file order, so a `Vec` comparison is the honest
/// one — sorting first would hide a real disagreement about sense numbering.
#[test]
fn the_two_readers_find_the_same_noun_senses() {
    let Some((wn, db)) = open_both() else { return };
    for lemma in PROBES {
        assert_eq!(
            verbora_offsets(&wn, lemma),
            db_offsets(&db, lemma),
            "{lemma:?}: the two readers disagree about which noun synsets the \
             index lists, or in what order"
        );
    }
}

/// A miss is a miss on both sides — not an error, not an empty-but-present
/// entry, on either.
#[test]
fn a_lemma_in_neither_index_is_absent_from_both() {
    let Some((wn, db)) = open_both() else { return };
    for lemma in ["zzzzz", "qqqqqqqq", "notawordatall"] {
        assert!(
            wn.index_entry(lemma, PartOfSpeech::Noun)
                .expect("a miss is not an error")
                .is_none(),
            "{lemma:?} unexpectedly found by verbora-wordnet"
        );
        assert!(
            !db.lemma_exists(Pos::Noun, lemma),
            "{lemma:?} unexpectedly found by wordnet-db"
        );
        assert!(db.synsets_for_lemma(Pos::Noun, lemma).is_empty());
    }
}

/// The records behind the offsets, not just the offsets: for every sense of
/// every probe, the two readers must have parsed the same set of words out of
/// the same data line.
///
/// Compared as sets, unlike the offsets above: `wndb(5WN)` fixes the word order
/// within a record, but the two crates spell a word differently — Verbora
/// keeps the lexicographer's capitalisation and splits a trailing syntactic
/// marker off into its own field, `wordnet-db` hands back the raw text. Lower
/// casing and sorting compares what both genuinely claim to have read, which
/// is *which words are in the synset*, without asserting a formatting
/// convention neither crate promises the other.
#[test]
fn the_two_readers_parse_the_same_words_out_of_each_synset() {
    let Some((wn, db)) = open_both() else { return };
    let mut compared = 0usize;
    for lemma in PROBES {
        let senses = wn
            .senses(lemma, PartOfSpeech::Noun)
            .expect("a well-formed dictionary");
        let ids = db.synsets_for_lemma(Pos::Noun, lemma);
        assert_eq!(senses.len(), ids.len(), "{lemma:?}: sense counts differ");

        for (synset, id) in senses.iter().zip(ids) {
            let theirs = db
                .get_synset(*id)
                .unwrap_or_else(|| panic!("{lemma:?}: wordnet-db has no record at {id:?}"));

            let mut ours: Vec<String> = synset
                .words
                .iter()
                .map(|w| w.lemma.to_lowercase())
                .collect();
            let mut them: Vec<String> =
                theirs.words.iter().map(|w| w.text.to_lowercase()).collect();
            ours.sort();
            them.sort();
            assert_eq!(
                ours, them,
                "{lemma:?} at offset {}: the two readers parsed different words \
                 out of the same record",
                id.offset
            );
            compared += 1;
        }
    }
    assert!(
        compared > 0,
        "no synset was compared — the probe list found nothing, which means \
         this test proved nothing"
    );
}

/// The four `Storage` strategies answer identically, so `benches/wordnet.rs`
/// is comparing four ways of getting the same bytes rather than four different
/// answers.
///
/// `crates/verbora-wordnet` asserts this for itself; it is re-asserted here
/// because this harness is what publishes the four rows side by side, and a
/// number is only comparable across rows that agree.
#[test]
fn every_verbora_storage_strategy_agrees_with_wordnet_db() {
    let Some(dict) = dict_dir() else {
        eprintln!("wordnet_correctness skipped: no WordNet dictionary found.");
        return;
    };
    let db = DbWordNet::load_with_mode(&dict, LoadMode::Owned).expect("wordnet-db loads");
    for storage in [
        Storage::Pread,
        Storage::LazyResident,
        Storage::Resident,
        Storage::Indexed,
    ] {
        let wn = WordNet::open_with(&dict, &Config::new(storage)).expect("verbora-wordnet opens");
        for lemma in PROBES {
            assert_eq!(
                verbora_offsets(&wn, lemma),
                db_offsets(&db, lemma),
                "{storage:?} disagrees with wordnet-db on {lemma:?}"
            );
        }
    }
}

/// `wordnet-db`'s two load modes are documented as a portability choice, not a
/// semantic one. Pinned, because `benches/wordnet.rs` reports them as separate
/// rows and a reader is entitled to assume the difference is only in how the
/// bytes arrived.
#[test]
fn wordnet_db_load_modes_agree_with_each_other() {
    let Some(dict) = dict_dir() else {
        eprintln!("wordnet_correctness skipped: no WordNet dictionary found.");
        return;
    };
    let mapped = DbWordNet::load_with_mode(&dict, LoadMode::Mmap).expect("mmap load");
    let owned = DbWordNet::load_with_mode(&dict, LoadMode::Owned).expect("owned load");
    assert_eq!(mapped.synset_count(), owned.synset_count());
    assert_eq!(mapped.lemma_count(), owned.lemma_count());
    for lemma in PROBES {
        assert_eq!(
            db_offsets(&mapped, lemma),
            db_offsets(&owned, lemma),
            "{lemma:?}: wordnet-db's Mmap and Owned modes disagree"
        );
    }
}

/// The previously-affected lemmas, now read by both parsers.
///
/// These were unreadable by `verbora-wordnet` until it learned the bare `;`
/// and `-`: 13,606 of WordNet 3.1's 155,467 index entries carried one. The
/// list is kept as its own constant, and asserted against `wordnet-db` rather
/// than folded silently into [`PROBES`], because agreement between two
/// independent parsers on exactly the entries that used to disagree is the
/// strongest evidence the fix is real that this crate can produce.
#[test]
fn both_parsers_agree_on_the_formerly_unreadable_entries() {
    let Some((wn, db)) = open_both() else { return };

    for lemma in FORMERLY_UNREADABLE {
        let theirs = db.synsets_for_lemma(Pos::Noun, lemma);
        assert!(
            !theirs.is_empty(),
            "{lemma:?} is not a noun in this dictionary release; the fixture \
             needs updating before it can pin anything"
        );
        let ours = wn
            .index_entry(lemma, PartOfSpeech::Noun)
            .unwrap_or_else(|e| panic!("{lemma:?} must parse, got {e:?}"))
            .unwrap_or_else(|| panic!("{lemma:?} must be present as a noun"));
        assert_eq!(
            ours.sense_count(),
            theirs.len(),
            "{lemma:?}: sense counts disagree between the two parsers"
        );
    }
}
