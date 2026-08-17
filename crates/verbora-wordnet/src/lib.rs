//! Read the WordNet lexical database exactly the way the reference does.
//!
//! This is parity-verified against the reference `wordnet` module — `wordnet`, `index_file`,
//! `data_file` and `wordnet_file` — with observable behaviour preserved
//! including the parts that are wrong. Two of those matter enough to state up
//! front:
//!
//! * **The index search is incomplete.** It bisects byte positions rather than
//!   lines, and reports "not found" for 944 lemmas that are unquestionably in the
//!   database — among them the adverbs `awful`, `safely`, `such`, `bitter` and
//!   `firm`, and the entire head of `index.verb` (`aah`, `abandon`, `abase`,
//!   `abate`). A correct search is a *different program*, and the reference's own
//!   specs assert results that depend on this one.
//! * **Results come back in reverse.** Every traversal in the reference drains
//!   its work list with `pop()`, so parts of speech are visited adv → adj → verb
//!   → noun, and within each, the index line's offsets last to first.
//!
//! Both are reproduced deliberately. Both are covered by `fixtures/wordnet.json`,
//! recorded from the real library.
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
//! The licence requires that the notice accompany **all copies** of the database,
//! including modifications, and forbids using Princeton's name in advertising.
//!
//! **This crate ships no dictionary data.** It reads the files at run time from a
//! directory you supply, which keeps 34 MB of separately-licensed content out of
//! this repository and out of your dependency tree. Obtain it with:
//!
//! ```text
//! npm install wordnet-db          # 3.1 database, ~10 MB packed
//! export WORDNET_DB_PATH="$PWD/node_modules/wordnet-db/dict"
//! ```
//!
//! then use [`WordNet::from_env`], or pass the path to [`WordNet::open`].
//!
//! # Getting started
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use verbora_wordnet::{WordNet, pointer};
//!
//! let wn = WordNet::open("node_modules/wordnet-db/dict")?;
//!
//! for synset in wn.lookup("node")? {
//!     println!("{:?}  {}", synset.lemma, synset.def);
//! }
//!
//! // Walk up the hypernym chain from "node" (the network sense).
//! let node = wn.get(3_832_647.0, "n")?;
//! for parent in wn.closure(&node, pointer::HYPERNYM).take(5) {
//!     println!("^ {}", parent?.def);
//! }
//! # Ok(()) }
//! ```
//!
//! # Mapping the callback API onto Rust
//!
//! Every reference entry point takes a callback and returns `undefined`. Within
//! one operation, though, its I/O is strictly sequential — each `fs` callback
//! issues the next read — so a synchronous port produces the same results in the
//! same order. Callbacks therefore become return values; see
//! [the table in the `wordnet` module](crate::wordnet).
//!
//! Iterator forms are the primitives, and the collecting forms are built on
//! them: [`WordNet::lookup_iter`] before [`WordNet::lookup`],
//! [`WordNet::pointers`] and [`WordNet::closure`] for relation traversal,
//! [`IndexFile::probes`] for the bisection itself, and
//! [`DataFile::with_record`] for a zero-copy view of a synset.
//!
//! # Choosing a storage strategy
//!
//! The reference reads **one byte per `fs.read` call**: 1098 syscalls for a
//! single `find('entity')` on `index.noun`, and 61 open/close pairs for
//! `lookup('run')`. This crate reproduces the probe *positions* exactly — they
//! are observable, through the false misses — and nothing else about how the
//! bytes arrive. Four strategies are offered; see [`Storage`] and the numbers in
//! `benches/wordnet.rs`.
//!
//! | [`Storage`] | Startup | Per query | Resident |
//! |---|---|---|---|
//! | [`Storage::Pread`] | none | a handful of syscalls | none |
//! | [`Storage::LazyResident`] | none | in-memory once touched | grows to files used |
//! | [`Storage::Resident`] *(default)* | reads 28 MB | in-memory | 28 MB |
//! | [`Storage::Indexed`] | + one `memchr` pass | `partition_point` line starts | + ~600 KB |
//!
//! [`Storage::Indexed`] can load its line-start tables from a [`PrebuiltIndex`]
//! sidecar instead of scanning. The dictionary text files remain the source of
//! truth: the sidecar stores only derived line offsets, records the length of
//! each file it was built from, and is refused if that no longer matches.
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
//! never depend on what was looked up before. (The reference is the same in
//! substance: it opens and closes a descriptor per operation and caches nothing.)
//!
//! # Deliberate divergences
//!
//! Everything below is a case in which the reference does not *return* anything —
//! it hangs, or throws from inside an `fs` callback where no caller can catch it.
//! Reproducing those literally would make the crate unusable, so each becomes an
//! [`Error`]. Each is exercised by the fixture suites, which record the reference
//! outcome so the mapping cannot drift silently.
//!
//! | | Reference | Here |
//! |---|---|---|
//! | **W1** | unopenable file: `console.log('Unable to open %s')`, callback never fires | [`Error::Io`], at open rather than at first query |
//! | **W2** | offset past EOF: `appendLineChar` recurses forever, buffer doubling until the process dies | [`Error::UnterminatedLine`] |
//! | **W3** | record with no `'| '`: async `TypeError … reading 'split'` | [`Error::MissingGloss`] |
//! | **W4** | absurd `wCnt`: loops that many times, pushing `undefined` until out of memory | [`Error::CountTooLarge`] beyond [`numfmt::MAX_COUNT`] |
//! | **W5** | `fs.statSync` on every `find`, so a file changing size mid-session changes the search path | length recorded once, at open |
//! | **W6** | a descriptor opened and closed per operation (61 for `lookup('run')`) | one handle, or none, for the process lifetime |
//! | **W7** | negative probe position: `fs.read` throws `ERR_OUT_OF_RANGE`, nothing is delivered | [`Error::NegativeProbe`]; verified unreachable for all 147,580 keys the fixture exercises |
//!
//! Two API-shape divergences, both forced by types rather than chosen:
//!
//! * `WordNet#lookup(123, cb)` throws `word.toLowerCase is not a function`; here
//!   the argument is `&str` and the error is a compile error instead.
//! * `getSynonyms` dispatches on `arguments` and *truthiness*, so a falsy `pos`
//!   silently promotes the callback into the `pos` slot and a falsy
//!   `synsetOffset` promotes the record object into the offset slot. Rust has no
//!   `arguments` object; the two intended call shapes are
//!   [`WordNet::get_synonyms`] and [`WordNet::get_synonyms_of`], and the reference's
//!   four failure modes are recorded in the fixture with the error each one maps to.
//!
//! # Extensions
//!
//! [`WordNet::find_sense`] and [`WordNet::query_sense`] have **no reference
//! counterpart** — the reference exports nine methods and neither is among them.
//! They are built on the parity surface and documented in [`sense`].

pub mod data_file;
pub mod error;
pub mod index_file;
pub mod numfmt;
pub mod pointer;
pub mod prebuilt;
pub mod sense;
pub mod source;
pub mod whitespace;
pub mod wordnet;

pub use data_file::{DataFile, DataRecord, DataRecordRef, Pointer, PointerRef};
pub use error::{Error, Result};
pub use index_file::{Find, IndexFile, IndexHit, IndexRecord, Probe, Probes};
pub use prebuilt::PrebuiltIndex;
pub use sense::{ParseSenseError, Sense};
pub use source::{Source, Storage};
pub use wordnet::{Closure, Config, FilePair, LookupIter, Pointers, Pos, WordNet};

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built dictionary, so the crate's own tests never need the 34 MB
    /// database. It is not WordNet data — the format, not the content.
    fn tiny_dict() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "verbora-wordnet-lib-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Offsets are real byte positions: the "beta" record starts at byte 83.
        let index = "aaa n 1 0 1 0 00000000  \nbbb n 2 1 @ 2 0 00000000 00000083  \nccc n 1 0 1 0 00000083  \n";
        let data = "00000000 06 n 01 alpha 0 001 @ 00000083 n 0000 | the first letter; \"as in alpha\"  \n00000083 06 n 02 beta 0 second 1 000 | the second letter  \n";
        for pos in ["noun", "verb", "adj", "adv"] {
            std::fs::write(dir.join(format!("index.{pos}")), index).unwrap();
            std::fs::write(dir.join(format!("data.{pos}")), data).unwrap();
        }
        dir
    }

    #[test]
    fn end_to_end_against_a_hand_built_dictionary() {
        let dir = tiny_dict();
        let wn = WordNet::open(&dir).unwrap();

        let alpha = wn.get(0.0, "n").unwrap();
        assert_eq!(alpha.lemma.as_deref(), Some("alpha"));
        assert_eq!(alpha.def, "the first letter");
        assert_eq!(alpha.exp, ["as in alpha"]);
        assert_eq!(alpha.ptrs.len(), 1);

        let beta = wn.get(83.0, "n").unwrap();
        assert_eq!(beta.w_cnt, 2.0);
        assert_eq!(
            beta.synonyms,
            [Some("beta".to_owned()), Some("second".to_owned())]
        );

        // One pointer hop from alpha reaches beta.
        let syns = wn.get_synonyms_of(&alpha).unwrap();
        assert_eq!(syns.len(), 1);
        assert_eq!(syns[0].lemma.as_deref(), Some("beta"));

        // Relation iterators agree with the eager form.
        let via_iter: Vec<_> = wn
            .pointers(&alpha)
            .map(|r| r.unwrap().lemma)
            .collect::<Vec<_>>();
        assert_eq!(via_iter, [Some("beta".to_owned())]);
        assert_eq!(wn.relation(&alpha, pointer::HYPERNYM).count(), 1);
        assert_eq!(wn.relation(&alpha, pointer::HYPONYM).count(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_storage_backend_gives_identical_answers() {
        let dir = tiny_dict();
        let reference = {
            let wn = WordNet::open_with(&dir, &Config::new(Storage::Resident)).unwrap();
            (wn.lookup("ccc").unwrap(), wn.get(0.0, "n").unwrap())
        };
        for storage in [Storage::Pread, Storage::LazyResident, Storage::Indexed] {
            let wn = WordNet::open_with(&dir, &Config::new(storage)).unwrap();
            assert_eq!(wn.lookup("ccc").unwrap(), reference.0, "{storage:?}");
            assert_eq!(wn.get(0.0, "n").unwrap(), reference.1, "{storage:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_prebuilt_sidecar_reproduces_the_scanned_index() {
        let dir = tiny_dict();
        let sidecar = dir.join("wordnet.nrsidx");
        PrebuiltIndex::build(&dir).unwrap().save(&sidecar).unwrap();
        // Byte-for-byte reproducible: rebuilding gives the same file.
        let again = PrebuiltIndex::build(&dir).unwrap().to_bytes();
        assert_eq!(std::fs::read(&sidecar).unwrap(), again);

        let scanned = WordNet::open_with(&dir, &Config::new(Storage::Indexed)).unwrap();
        let loaded = WordNet::open_with(&dir, &Config::default().with_prebuilt(&sidecar)).unwrap();
        for word in ["aaa", "bbb", "ccc", "zzz", ""] {
            assert_eq!(scanned.lookup(word).unwrap(), loaded.lookup(word).unwrap());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_pos_tags_are_an_error_not_a_panic() {
        let dir = tiny_dict();
        let wn = WordNet::open(&dir).unwrap();
        assert!(matches!(wn.get(0.0, "x"), Err(Error::UnknownPos(_))));
        assert!(matches!(wn.get(0.0, "N"), Err(Error::UnknownPos(_))));
        assert!(wn.data_file("noun").is_none());
        assert!(wn.data_file("s").is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn queries_run_concurrently_on_a_shared_dictionary() {
        let dir = tiny_dict();
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
    fn a_missing_dictionary_fails_at_open_rather_than_stalling() {
        assert!(matches!(
            WordNet::open("/no/such/dir"),
            Err(Error::Io { .. })
        ));
    }
}
