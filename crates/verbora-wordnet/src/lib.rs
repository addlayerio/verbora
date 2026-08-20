//! Read the WordNet lexical database from Rust.
//!
//! WordNet groups English nouns, verbs, adjectives and adverbs into *synsets* —
//! sets of words that share one sense — and records the relations between them
//! (Fellbaum, *WordNet: An Electronic Lexical Database*, MIT Press, 1998). It is
//! distributed as eight plain-text files whose layout is specified by the
//! `wndb(5WN)` and `wninput(5WN)` manual pages that ship with the database. This
//! crate reads those files and nothing else.
//!
//! Every behaviour below is derived from that published format. Where the format
//! leaves something open — how a gloss divides into a definition and examples,
//! what a word must be turned into before it can be looked up, what happens when
//! a file is truncated — this documentation states Verbora's choice and the
//! reasoning behind it, and the test suite pins it.
//!
//! # WordNet is separately licensed
//!
//! `verbora` is MIT. **The WordNet database is not.** It is distributed under
//! Princeton University's own licence, reproduced verbatim in
//! [`LICENSE-WORDNET`](https://github.com/addlayerio/verbora/blob/main/crates/verbora-wordnet/LICENSE-WORDNET)
//! beside this crate:
//!
//! > WordNet 3.0 Copyright 2006 by Princeton University. All rights reserved.
//! > […] Permission to use, copy, modify and distribute this software and
//! > database and its documentation for any purpose and without fee or royalty
//! > is hereby granted, provided that you agree to comply with the following
//! > copyright notice and statements, including the disclaimer […]
//! > THIS SOFTWARE AND DATABASE IS PROVIDED "AS IS" AND PRINCETON UNIVERSITY
//! > MAKES NO REPRESENTATIONS OR WARRANTIES, EXPRESS OR IMPLIED.
//!
//! The licence requires the notice to accompany **all copies** of the database,
//! including modifications, and forbids using Princeton's name in advertising.
//!
//! **This crate ships no dictionary data.** It reads the files at run time from
//! a directory you supply, which keeps tens of megabytes of separately-licensed
//! content out of this repository and out of your dependency tree. Download a
//! WordNet 3.0 or 3.1 distribution from Princeton, then point [`WordNet::open`]
//! at the `dict` directory inside it, or set `WORDNET_DB_PATH` and use
//! [`WordNet::from_env`].
//!
//! # Getting started
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use verbora_wordnet::{PartOfSpeech, PointerSymbol, WordNet};
//!
//! let wn = WordNet::from_env()?;
//!
//! // Every sense of "node", in the order WordNet numbers them.
//! for (i, synset) in wn.senses("node", PartOfSpeech::Noun)?.iter().enumerate() {
//!     println!("node#n#{}: {}", i + 1, synset.gloss.definition);
//! }
//!
//! // Walk up the hypernym chain from the network sense.
//! let node = wn.sense(&"node#n#8".parse()?)?.expect("node has eight senses");
//! for parent in wn.closure(&node, PointerSymbol::Hypernym).take(5) {
//!     println!("^ {}", parent?.lemma());
//! }
//! # Ok(()) }
//! ```
//!
//! # The contract
//!
//! ## Text unit
//!
//! **The dictionary files are read as bytes, and the API hands back `&str`.**
//! That split is deliberate, and each half is chosen for its own reason:
//!
//! * **Searching is byte-wise.** `wndb(5WN)` specifies that index files are
//!   sorted so they can be binary searched, in the ASCII collating sequence.
//!   [`IndexFile::entry`] therefore compares the raw bytes of a line's first
//!   field against the raw bytes of the key. Comparing decoded scalar values
//!   would agree for every legal file — the format's alphabet is ASCII — and
//!   would silently disagree for a corrupt one, which is the case where being
//!   wrong matters.
//! * **Content is Unicode scalar values.** A gloss is handed back as `&str`,
//!   decoded from UTF-8 with invalid bytes replaced by `U+FFFD` rather than
//!   failing the whole read: one corrupt byte should cost you one character of
//!   one definition, not the record.
//! * **[`index_key`] works in scalar values for whitespace and in ASCII for
//!   case.** Whitespace is detected with Unicode's own definition, because a
//!   non-breaking space between two words is still a word boundary. Case is
//!   folded only for ASCII, because `wndb(5WN)` defines index lemmas as lower
//!   case ASCII: folding `İ` would produce a string no index contains, which is
//!   a guess dressed as a lookup.
//!
//! ## Ordering
//!
//! Everything comes out in the order the files list it.
//!
//! * [`WordNet::lookup`] consults categories in the order noun, verb,
//!   adjective, adverb.
//! * Within a category, senses come out in **sense order** — the order the
//!   index line lists its offsets, which `wndb(5WN)` defines as
//!   most-frequently-tagged first. Element `0` is sense 1.
//! * [`WordNet::pointers`] yields pointers in the order the data record writes
//!   them.
//! * [`WordNet::closure`] is breadth first, and yields each reachable synset at
//!   most once.
//!
//! ## Absence and failure
//!
//! A word with no entry is [`None`] or an empty `Vec`, never an error and never
//! a sentinel value. A file that cannot be read, or a record that does not match
//! the documented format, is an [`Error`] — including the cases a lenient reader
//! would paper over:
//!
//! | Situation | Result |
//! |---|---|
//! | a dictionary file is missing | [`Error::Io`], at open, not at the first query |
//! | an offset lies past the end of its data file | [`Error::OffsetOutOfRange`] |
//! | an offset does not point at the start of a record | [`Error::MalformedSynset`] with [`RecordError::OffsetMismatch`] |
//! | a record is read from the wrong category's file | [`Error::MalformedSynset`] with an `ss_type` complaint |
//! | a numeric field is not a number in its documented radix | [`Error::MalformedSynset`] / [`Error::MalformedIndexEntry`] |
//! | a data record has no `\|` gloss delimiter | [`RecordError::MissingGloss`] |
//! | an index line's two sense counts disagree | [`RecordError::SenseCountMismatch`] |
//! | a prebuilt sidecar no longer matches the files | [`Error::Prebuilt`] |
//!
//! No public function panics on any input: an offset, a key, a spec string or a
//! malformed file all produce a value or an `Error`.
//!
//! ## Normalisation is named, never hidden
//!
//! WordNet's index files are keyed on a specific spelling: lower case ASCII,
//! with the words of a collocation joined by `_`. Turning a word the user typed
//! into that spelling is a real transform, so it has a name — [`index_key`] —
//! and every entry point says which side of the line it is on:
//!
//! * entry points that take a **word** ([`WordNet::lookup`],
//!   [`WordNet::senses`], [`WordNet::index_entry`], [`WordNet::sense`]) apply
//!   [`index_key`] first;
//! * entry points that take a **key** ([`IndexFile::entry`]) use it verbatim.
//!
//! [`index_key`] is the identity on every string that is already a legal index
//! key, which is what makes every lemma in a dictionary reachable through it.
//! `tests/enumeration.rs` walks every entry of every index file it is given and
//! asserts that, entry by entry, rather than checking a sample.
//!
//! ## What this crate does not read
//!
//! The eight `index.*` and `data.*` files, and nothing else. A WordNet
//! distribution also ships `index.sense`, the morphological exception lists
//! (`noun.exc` and siblings), `cntlist`, and the lexicographer sources; none of
//! them is parsed here. Within a data record, the verb frame list is skipped —
//! the gloss is located by its `|` delimiter, so the frames need not be counted.
//!
//! There is no morphological reduction either: this crate looks up the lemma you
//! give it. Reducing *running* to *run* is [`verbora-stemmers`]' problem, and
//! keeping it there is what stops a lookup from quietly answering about a
//! different word.
//!
//! [`verbora-stemmers`]: https://docs.rs/verbora-stemmers
//!
//! # Choosing the right API
//!
//! ## Getting synsets out of a word
//!
//! | Call | Returns | Reads | Choose it when |
//! |---|---|---|---|
//! | [`WordNet::lookup`] | `Vec<Synset>`, all four categories | every sense | you want everything and do not know the category |
//! | [`WordNet::lookup_iter`] | lazy `Iterator<Item = Result<Synset>>` | only what you consume | you will stop early, or want to stream |
//! | [`WordNet::senses`] | `Vec<Synset>` for one category | that category's senses | you know the category — most of the time |
//! | [`WordNet::sense`] | `Option<Synset>` | one record | you have a `lemma#pos#n` reference |
//! | [`WordNet::index_entry`] | `Option<IndexEntry>` | one index line | you need the offsets or the sense count, not the synsets |
//! | [`WordNet::par_lookup_batch`] | one `Result` per word | every sense of every word | you have a large batch and the `parallel` feature |
//!
//! [`WordNet::lookup`] is the right default. The others exist for a reason you
//! can state: `lookup_iter` avoids reading senses you will not look at,
//! `senses` avoids searching three index files you know cannot match,
//! `index_entry` avoids reading any data record at all, and `par_lookup_batch`
//! spends cores instead of wall-clock on a batch large enough to amortise the
//! scheduling.
//!
//! ```text
//! do you know the part of speech?
//!  ├─ no  → will you consume every sense?
//!  │        ├─ yes → lookup
//!  │        └─ no  → lookup_iter
//!  └─ yes → do you need the synsets themselves?
//!           ├─ no            → index_entry
//!           ├─ one numbered  → sense
//!           └─ all of them   → senses
//! ```
//!
//! ## Getting one synset out of an offset
//!
//! [`WordNet::synset`] returns an owned [`Synset`]; [`WordNet::with_synset`]
//! hands a [`SynsetRef`] to a closure instead, with every string field pointing
//! into the line being parsed. The borrowed form is the primitive and the owned
//! form is one [`SynsetRef::to_synset`] on top of it, so they can never diverge.
//! Reach for `with_synset` when you are scanning many records and need only a
//! field or two out of each; reach for `synset` — the simple one — whenever the
//! record has to outlive the call, which is most of the time.
//!
//! ## Storage strategies
//!
//! [`Storage`] decides how bytes get from the file to the parser. All four
//! answer every query identically.
//!
//! | [`Storage`] | Startup | Per query | Resident memory |
//! |---|---|---|---|
//! | [`Storage::Pread`] | none | a handful of positioned reads | none |
//! | [`Storage::LazyResident`] | none | in memory once the file is touched | grows to the files used |
//! | [`Storage::Resident`] *(default)* | reads the dictionary | in memory | the whole dictionary |
//! | [`Storage::Indexed`] | + one newline scan | line starts by `partition_point` | + four bytes per line |
//!
//! Choose [`Storage::Resident`] unless you have a reason not to: it is the
//! default because a long-lived process that answers many queries wants the
//! files in memory. Choose [`Storage::Pread`] for a one-shot process that will
//! ask one or two questions and exit, [`Storage::LazyResident`] when startup
//! latency matters but steady-state does too, and [`Storage::Indexed`] when the
//! query rate is high enough that the backwards newline scan shows up — its
//! line table can be persisted with [`PrebuiltIndex`] so that reopening skips
//! the scan.
//!
//! **These are qualitative descriptions, not measurements.** The crate's
//! Criterion suite (`benches/wordnet.rs`) measures all four across startup,
//! cold, warm and batch dimensions; no figures are published here because none
//! have been taken against this implementation.
//!
//! There is no memory-mapped backend. `mmap` needs either a dependency
//! (`memmap2`) or `unsafe`, and this workspace admits neither;
//! [`Storage::LazyResident`] covers the case it is usually wanted for.
//!
//! # Concurrency
//!
//! [`WordNet`], [`IndexFile`] and [`DataFile`] are immutable after construction
//! and `Send + Sync`. Share one instance across threads and query it
//! concurrently — nothing is cached per query, nothing is locked, and results
//! never depend on what was looked up before.

mod data_file;
mod error;
mod index_file;
mod parse;
mod pointer;
mod pos;
mod prebuilt;
mod sense;
mod source;
mod synset;
mod wordnet;

pub use data_file::{DataFile, Synsets};
pub use error::{Error, RecordError, Result};
pub use index_file::{Entries, IndexEntry, IndexFile, index_key};
pub use pointer::{Pointer, PointerScope, PointerSymbol};
pub use pos::{PartOfSpeech, SynsetType};
pub use prebuilt::PrebuiltIndex;
pub use sense::{ParseSenseError, Sense, SenseNumber};
pub use source::Storage;
pub use synset::{
    Gloss, GlossRef, Synset, SynsetOffset, SynsetRef, SyntacticMarker, Word, WordRef,
};
pub use wordnet::{Closure, Config, LookupIter, Pointers, WordNet};

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built dictionary in the WordNet text format, so the crate's own
    /// tests never need the separately-licensed database. It is the *format*,
    /// not the content: three lemmas and two synsets.
    ///
    /// Byte offsets below are the real positions of each record in `data`, and
    /// the index lines quote them in the eight-digit form the format requires.
    fn tiny_dict(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "verbora-wordnet-lib-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let header = "  1 Copyright notice, two leading spaces, as the format requires\n";
        let index = format!(
            "{header}\
             aaa n 1 0 1 0 00000065  \n\
             bbb n 2 1 @ 2 0 00000065 00000148  \n\
             ccc n 1 0 1 0 00000148  \n"
        );
        let alpha = "00000065 06 n 01 alpha 0 001 @ 00000148 n 0000 | the first letter; \"as in alpha\"  \n";
        let beta = "00000148 06 n 02 beta 0 second 1 000 | the second letter  \n";
        let data = format!("{header}{alpha}{beta}");
        assert_eq!(header.len(), 65, "the header fixes the first record offset");
        assert_eq!(header.len() + alpha.len(), 148);

        for pos in PartOfSpeech::ALL {
            let suffix = pos.file_suffix();
            let tagged = index.replace(" n ", &format!(" {} ", pos.tag()));
            let tagged_data = data.replace(" 06 n ", &format!(" 06 {} ", pos.tag()));
            std::fs::write(dir.join(format!("index.{suffix}")), &tagged).unwrap();
            std::fs::write(dir.join(format!("data.{suffix}")), &tagged_data).unwrap();
        }
        dir
    }

    fn offsets() -> (SynsetOffset, SynsetOffset) {
        (SynsetOffset::new(65), SynsetOffset::new(148))
    }

    #[test]
    fn end_to_end_against_a_hand_built_dictionary() {
        let dir = tiny_dict("e2e");
        let wn = WordNet::open(&dir).unwrap();
        let (alpha_at, beta_at) = offsets();

        let alpha = wn.synset(alpha_at, PartOfSpeech::Noun).unwrap();
        assert_eq!(alpha.lemma(), "alpha");
        assert_eq!(alpha.gloss.definition, "the first letter");
        assert_eq!(alpha.gloss.examples, ["as in alpha"]);
        assert_eq!(alpha.pointers.len(), 1);
        assert_eq!(alpha.synset_type, SynsetType::Noun);

        let beta = wn.synset(beta_at, PartOfSpeech::Noun).unwrap();
        assert_eq!(beta.words.len(), 2);
        assert_eq!(
            beta.words
                .iter()
                .map(|w| w.lemma.as_str())
                .collect::<Vec<_>>(),
            ["beta", "second"]
        );

        // One pointer hop from alpha reaches beta.
        let hop: Vec<Synset> = wn.pointers(&alpha).collect::<Result<_>>().unwrap();
        assert_eq!(hop.len(), 1);
        assert_eq!(hop[0].lemma(), "beta");
        assert_eq!(wn.related(&alpha, PointerSymbol::Hypernym).count(), 1);
        assert_eq!(wn.related(&alpha, PointerSymbol::Hyponym).count(), 0);

        // Closure terminates and does not yield the starting synset.
        let chain: Vec<Synset> = wn
            .closure(&alpha, PointerSymbol::Hypernym)
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].lemma(), "beta");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lookup_walks_the_four_categories_in_order_and_senses_in_index_order() {
        let dir = tiny_dict("order");
        let wn = WordNet::open(&dir).unwrap();
        let (alpha_at, beta_at) = offsets();

        // "bbb" lists 63 then 144 on its index line, so sense 1 is alpha.
        let senses = wn.senses("bbb", PartOfSpeech::Noun).unwrap();
        assert_eq!(
            senses.iter().map(|s| s.offset).collect::<Vec<_>>(),
            [alpha_at, beta_at]
        );

        // Every category carries the same three lemmas, so `lookup` yields the
        // noun senses first, then verb, then adjective, then adverb.
        let all = wn.lookup("bbb").unwrap();
        assert_eq!(all.len(), 8);
        assert_eq!(
            all.iter().map(Synset::part_of_speech).collect::<Vec<_>>(),
            [
                PartOfSpeech::Noun,
                PartOfSpeech::Noun,
                PartOfSpeech::Verb,
                PartOfSpeech::Verb,
                PartOfSpeech::Adjective,
                PartOfSpeech::Adjective,
                PartOfSpeech::Adverb,
                PartOfSpeech::Adverb,
            ]
        );

        // The lazy form agrees with the eager one, item for item.
        let lazy: Vec<Synset> = wn.lookup_iter("bbb").collect::<Result<_>>().unwrap();
        assert_eq!(lazy, all);
        // And stopping early really does stop.
        assert_eq!(wn.lookup_iter("bbb").take(3).count(), 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sense_addressing_counts_forwards_from_one() {
        let dir = tiny_dict("sense");
        let wn = WordNet::open(&dir).unwrap();
        let (alpha_at, beta_at) = offsets();

        let first: Sense = "bbb#n#1".parse().unwrap();
        let second: Sense = "bbb#n#2".parse().unwrap();
        assert_eq!(wn.sense(&first).unwrap().unwrap().offset, alpha_at);
        assert_eq!(wn.sense(&second).unwrap().unwrap().offset, beta_at);
        // Past the end is absence, not an error.
        assert!(wn.sense(&"bbb#n#3".parse().unwrap()).unwrap().is_none());
        assert!(wn.sense(&"zzz#n#1".parse().unwrap()).unwrap().is_none());
        // A satellite spec routes to the adjective files.
        assert!(wn.sense(&"bbb#s#1".parse().unwrap()).unwrap().is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A sidecar is caller-supplied data, so it must not be able to make the
    /// search misbehave. One with empty line tables degrades the bisection into
    /// a forward scan — slower, still terminating, still correct — rather than
    /// underflowing an index or looping without narrowing its window.
    #[test]
    fn a_sidecar_with_empty_line_tables_degrades_but_does_not_break() {
        let dir = tiny_dict("badsidecar");
        let files = [
            "index.noun",
            "index.verb",
            "index.adj",
            "index.adv",
            "data.noun",
            "data.verb",
            "data.adj",
            "data.adv",
        ];

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NRSWNIX\x01");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for name in files {
            let len = std::fs::metadata(dir.join(name)).unwrap().len();
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&len.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes()); // no line starts at all
        }
        let sidecar = dir.join("empty.vbwnix");
        std::fs::write(&sidecar, &bytes).unwrap();

        let scanned = WordNet::open_with(&dir, &Config::new(Storage::Indexed)).unwrap();
        let degraded =
            WordNet::open_with(&dir, &Config::default().with_prebuilt(&sidecar)).unwrap();
        for word in ["aaa", "bbb", "ccc", "zzz", "", "AAA"] {
            assert_eq!(
                degraded.lookup(word).unwrap(),
                scanned.lookup(word).unwrap(),
                "{word:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_storage_backend_gives_identical_answers() {
        let dir = tiny_dict("storage");
        let reference = {
            let wn = WordNet::open_with(&dir, &Config::new(Storage::Resident)).unwrap();
            (
                wn.lookup("ccc").unwrap(),
                wn.synset(offsets().0, PartOfSpeech::Noun).unwrap(),
                wn.lookup("zzz").unwrap(),
            )
        };
        for storage in [Storage::Pread, Storage::LazyResident, Storage::Indexed] {
            let wn = WordNet::open_with(&dir, &Config::new(storage)).unwrap();
            assert_eq!(wn.lookup("ccc").unwrap(), reference.0, "{storage:?}");
            assert_eq!(
                wn.synset(offsets().0, PartOfSpeech::Noun).unwrap(),
                reference.1,
                "{storage:?}"
            );
            assert_eq!(wn.lookup("zzz").unwrap(), reference.2, "{storage:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_prebuilt_sidecar_reproduces_the_scanned_index() {
        let dir = tiny_dict("prebuilt");
        let sidecar = dir.join("wordnet.vbwnix");
        PrebuiltIndex::build(&dir).unwrap().save(&sidecar).unwrap();
        // Byte-for-byte reproducible: rebuilding gives the same file.
        let again = PrebuiltIndex::build(&dir).unwrap().to_bytes();
        assert_eq!(std::fs::read(&sidecar).unwrap(), again);

        let scanned = WordNet::open_with(&dir, &Config::new(Storage::Indexed)).unwrap();
        let loaded = WordNet::open_with(&dir, &Config::default().with_prebuilt(&sidecar)).unwrap();
        for word in ["aaa", "bbb", "ccc", "zzz", "", "  aaa  ", "AAA"] {
            assert_eq!(
                scanned.lookup(word).unwrap(),
                loaded.lookup(word).unwrap(),
                "{word:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn words_are_converted_to_index_keys_before_the_search() {
        let dir = tiny_dict("keys");
        let wn = WordNet::open(&dir).unwrap();
        let expected = wn.lookup("aaa").unwrap();
        assert!(!expected.is_empty());
        for spelling in ["AAA", "  aaa  ", "Aaa", "\taaa\n"] {
            assert_eq!(wn.lookup(spelling).unwrap(), expected, "{spelling:?}");
        }
        // The key-level API does not transform, so the same spellings miss.
        let index = wn.index_file(PartOfSpeech::Noun);
        assert!(index.entry("aaa").unwrap().is_some());
        for spelling in ["AAA", "  aaa  ", ""] {
            assert!(index.entry(spelling).unwrap().is_none(), "{spelling:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_words_and_empty_input_are_absence_not_errors() {
        let dir = tiny_dict("absence");
        let wn = WordNet::open(&dir).unwrap();
        for word in ["", " ", "\t", "zzz", "café", "日本語", "😀", "a b c"] {
            assert!(wn.lookup(word).unwrap().is_empty(), "{word:?}");
            assert!(
                wn.index_entry(word, PartOfSpeech::Noun).unwrap().is_none(),
                "{word:?}"
            );
            assert!(
                wn.senses(word, PartOfSpeech::Verb).unwrap().is_empty(),
                "{word:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Cycle safety, and the reason the visited set is keyed on
    /// `(category, offset)` rather than on the offset alone: the same byte
    /// position names a different synset in each data file.
    #[test]
    fn closure_terminates_on_a_cycle_and_keeps_the_four_files_apart() {
        let dir = std::env::temp_dir().join(format!(
            "verbora-wordnet-lib-cycle-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let header = "  1 not WordNet data, only its format\n";
        let off_a = header.len() as u32;
        // `aaa` points twice at the *same* offset, once into `data.noun` and
        // once into `data.verb`. The offset field is eight digits wide either
        // way, so the record's length does not depend on the value.
        let rec_a = |target: u32| {
            format!(
                "{off_a:08} 06 n 01 aaa 0 002 @ {target:08} n 0000 @ {target:08} v 0000 | aaa def  \n"
            )
        };
        let off_b = off_a + rec_a(0).len() as u32;
        // `bbb` points back at `aaa`: a two-node cycle.
        let rec_b = format!("{off_b:08} 06 n 01 bbb 0 001 @ {off_a:08} n 0000 | bbb def  \n");
        // A header padded so that `data.verb`'s only record sits at `off_b`.
        let verb_header = format!("  {}\n", "p".repeat(off_b as usize - 3));
        assert_eq!(verb_header.len(), off_b as usize);
        let rec_v = format!("{off_b:08} 06 v 01 vvv 0 000 | vvv def  \n");

        std::fs::write(
            dir.join("data.noun"),
            format!("{header}{}{rec_b}", rec_a(off_b)),
        )
        .unwrap();
        std::fs::write(dir.join("data.verb"), format!("{verb_header}{rec_v}")).unwrap();
        for pos in PartOfSpeech::ALL {
            let suffix = pos.file_suffix();
            std::fs::write(dir.join(format!("index.{suffix}")), header).unwrap();
            if !matches!(pos, PartOfSpeech::Noun | PartOfSpeech::Verb) {
                std::fs::write(dir.join(format!("data.{suffix}")), header).unwrap();
            }
        }

        let wn = WordNet::open(&dir).unwrap();
        let a = wn
            .synset(SynsetOffset::new(off_a), PartOfSpeech::Noun)
            .unwrap();
        assert_eq!(a.lemma(), "aaa");

        let reached: Vec<Synset> = wn
            .closure(&a, PointerSymbol::Hypernym)
            .collect::<Result<_>>()
            .unwrap();
        // Both targets share offset `off_b`; only the file tells them apart.
        let mut lemmas: Vec<&str> = reached.iter().map(Synset::lemma).collect();
        lemmas.sort_unstable();
        assert_eq!(lemmas, ["bbb", "vvv"]);
        assert!(reached.iter().all(|s| s.offset == SynsetOffset::new(off_b)));
        // The cycle back to the starting synset is not re-yielded.
        assert!(reached.iter().all(|s| s.lemma() != "aaa"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn queries_run_concurrently_on_a_shared_dictionary() {
        let dir = tiny_dict("threads");
        let wn = std::sync::Arc::new(WordNet::open(&dir).unwrap());
        let expected = wn.lookup("ccc").unwrap();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let wn = std::sync::Arc::clone(&wn);
                let expected = expected.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        assert_eq!(wn.lookup("ccc").unwrap(), expected);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_dictionary_fails_at_open_rather_than_at_the_first_query() {
        assert!(matches!(
            WordNet::open("/no/such/dir"),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn a_truncated_dictionary_is_reported_not_guessed_at() {
        let dir = tiny_dict("truncated");
        // Chop the last data record in half.
        let path = dir.join("data.noun");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(180);
        std::fs::write(&path, &bytes).unwrap();

        let wn = WordNet::open(&dir).unwrap();
        // The surviving first record still reads.
        assert_eq!(
            wn.synset(offsets().0, PartOfSpeech::Noun).unwrap().lemma(),
            "alpha"
        );
        // The half record is refused rather than delivered as a partial synset.
        let err = wn.synset(offsets().1, PartOfSpeech::Noun).unwrap_err();
        assert!(matches!(err, Error::MalformedSynset { .. }), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
