//! `data.{noun,verb,adj,adv}`: the synsets themselves, addressed by byte offset.

use std::path::{Path, PathBuf};

use crate::error::{Error, RecordError, Result};
use crate::pos::PartOfSpeech;
use crate::source::{Source, Storage};
use crate::synset::{Synset, SynsetOffset, SynsetRef, parse_synset};

/// One `data.*` file.
///
/// Read-only after construction and `Send + Sync`: share one instance across
/// threads and query it concurrently.
#[derive(Debug)]
pub struct DataFile {
    path: PathBuf,
    pos: PartOfSpeech,
    source: Source,
}

impl DataFile {
    /// Opens one data file directly.
    ///
    /// `pos` must match the file. Every record's own `ss_type` is checked
    /// against it, so reading `data.adv` offsets out of `data.noun` is an error
    /// rather than a plausible-looking wrong answer.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be opened or read;
    /// [`Error::FileTooLarge`] for any storage but [`Storage::Pread`] on a file
    /// of 4 GiB or more.
    pub fn open(path: impl AsRef<Path>, pos: PartOfSpeech, storage: Storage) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self {
            path: path.to_path_buf(),
            pos,
            source: Source::open(path, storage)?,
        })
    }

    pub(crate) fn from_source(path: PathBuf, pos: PartOfSpeech, source: Source) -> Self {
        Self { path, pos, source }
    }

    /// The file this was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The category this file holds. Adjective satellites live in the adjective
    /// file, so this answers [`PartOfSpeech::Adjective`] for both.
    #[must_use]
    pub fn part_of_speech(&self) -> PartOfSpeech {
        self.pos
    }

    /// The file's size in bytes, as recorded when it was opened.
    #[must_use]
    pub fn len_bytes(&self) -> u64 {
        self.source.len()
    }

    /// Reads the synset at `offset` and hands a borrowed view to `f`.
    ///
    /// This is the zero-copy primitive: every string field of the
    /// [`SynsetRef`] points into the line being parsed, so nothing is copied
    /// for a caller that only needs to look. [`DataFile::synset`] is this
    /// function plus one [`SynsetRef::to_synset`].
    ///
    /// Three things are validated, all of them from the record itself rather
    /// than from a caller's promise:
    ///
    /// * `offset` must lie within the file;
    /// * the record must declare `offset` as its own `synset_offset`, which is
    ///   what makes a mid-record offset an error instead of a well-formed
    ///   synset assembled out of the middle of a real one;
    /// * the record's `ss_type` must belong to this file's category.
    ///
    /// # Errors
    ///
    /// [`Error::OffsetOutOfRange`], [`Error::MalformedSynset`] or
    /// [`Error::Io`].
    pub fn with_synset<R>(
        &self,
        offset: SynsetOffset,
        f: impl FnOnce(&SynsetRef<'_>) -> R,
    ) -> Result<R> {
        let at = u64::from(offset);
        if at >= self.source.len() {
            return Err(Error::OffsetOutOfRange {
                path: self.path.clone(),
                offset,
                file_len: self.source.len(),
            });
        }
        let mut scratch = Vec::new();
        let outcome = self.source.with_line(at, &mut scratch, |line| {
            // A data file is ASCII by the format's own definition; decoding
            // lossily means a corrupt byte becomes a replacement character in
            // one field instead of failing the whole read.
            let text = String::from_utf8_lossy(line.bytes);
            let record = parse_synset(&text)?;
            if record.offset != offset {
                return Err(RecordError::OffsetMismatch {
                    requested: offset,
                    found: record.offset,
                });
            }
            if record.part_of_speech() != self.pos {
                return Err(RecordError::InvalidField {
                    field: "ss_type",
                    value: record.synset_type.tag().to_owned(),
                });
            }
            Ok(f(&record))
        })?;
        match outcome {
            Some(Ok(value)) => Ok(value),
            Some(Err(kind)) => Err(Error::synset(&self.path, offset, kind)),
            None => Err(Error::OffsetOutOfRange {
                path: self.path.clone(),
                offset,
                file_len: self.source.len(),
            }),
        }
    }

    /// The synset at `offset`, owned.
    ///
    /// # Errors
    ///
    /// As [`DataFile::with_synset`].
    pub fn synset(&self, offset: SynsetOffset) -> Result<Synset> {
        self.with_synset(offset, |record| record.to_synset())
    }

    /// Every synset in the file, in file order, skipping the copyright header.
    ///
    /// A full sequential scan, for building derived structures and for auditing
    /// a dictionary. Use [`DataFile::synset`] to read one record.
    #[must_use]
    pub fn synsets(&self) -> Synsets<'_> {
        Synsets {
            file: self,
            at: 0,
            scratch: Vec::new(),
            done: false,
        }
    }
}

/// Iterator returned by [`DataFile::synsets`].
#[derive(Debug)]
pub struct Synsets<'a> {
    file: &'a DataFile,
    at: u64,
    scratch: Vec<u8>,
    done: bool,
}

impl Iterator for Synsets<'_> {
    type Item = Result<Synset>;

    fn next(&mut self) -> Option<Result<Synset>> {
        while !self.done {
            let start = self.at;
            let mut scratch = std::mem::take(&mut self.scratch);
            let read = self.file.source.with_line(start, &mut scratch, |line| {
                (
                    line.bytes.starts_with(b"  "),
                    String::from_utf8_lossy(line.bytes).into_owned(),
                    line.next,
                )
            });
            self.scratch = scratch;
            match read {
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Ok(None) => {
                    self.done = true;
                    return None;
                }
                Ok(Some((header, text, next))) => {
                    self.at = next;
                    if header || text.is_empty() {
                        continue;
                    }
                    // `start` fits a `u32` whenever the file does; a file too
                    // large for that cannot hold a `synset_offset` addressing
                    // this record in the first place.
                    let offset = SynsetOffset::new(u32::try_from(start).unwrap_or(u32::MAX));
                    return Some(
                        parse_synset(&text)
                            .map(|r| r.to_synset())
                            .map_err(|kind| Error::synset(&self.file.path, offset, kind)),
                    );
                }
            }
        }
        None
    }
}

impl std::iter::FusedIterator for Synsets<'_> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Two synsets and a two-line copyright header, in the `data.*` format.
    /// Offsets are the real byte positions of each record in this text.
    fn temp_data(name: &str) -> (PathBuf, [SynsetOffset; 2]) {
        let header = "  1 Copyright notice line one\n  2 Copyright notice line two\n";
        let first = "00000060 06 n 01 alpha 0 001 @ 00000143 n 0000 | the first letter; \"as in alpha\"  \n";
        let second = "00000143 06 n 02 beta 0 second 1 000 | the second letter  \n";
        let body = format!("{header}{first}{second}");
        assert_eq!(header.len(), 60);
        assert_eq!(header.len() + first.len(), 143);

        let dir = std::env::temp_dir().join(format!(
            "verbora-wordnet-datafile-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.noun");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        (path, [SynsetOffset::new(60), SynsetOffset::new(143)])
    }

    #[test]
    fn reads_a_record_at_its_own_offset_under_every_backend() {
        let (path, offsets) = temp_data("read");
        for storage in [
            Storage::Pread,
            Storage::LazyResident,
            Storage::Resident,
            Storage::Indexed,
        ] {
            let f = DataFile::open(&path, PartOfSpeech::Noun, storage).unwrap();
            let alpha = f.synset(offsets[0]).unwrap();
            assert_eq!(alpha.lemma(), "alpha", "{storage:?}");
            assert_eq!(alpha.gloss.definition, "the first letter", "{storage:?}");
            assert_eq!(alpha.gloss.examples, ["as in alpha"], "{storage:?}");
            assert_eq!(alpha.pointers.len(), 1, "{storage:?}");

            let beta = f.synset(offsets[1]).unwrap();
            assert_eq!(beta.words.len(), 2, "{storage:?}");
            assert_eq!(beta.words[1].lemma, "second", "{storage:?}");
            assert_eq!(beta.words[1].lex_id, 1, "{storage:?}");

            // The borrowed and owned reads agree.
            let borrowed_lemma = f.with_synset(offsets[0], |r| r.lemma().to_owned()).unwrap();
            assert_eq!(borrowed_lemma, "alpha", "{storage:?}");
        }
    }

    #[test]
    fn an_offset_that_is_not_a_record_start_is_an_error() {
        let (path, offsets) = temp_data("midrecord");
        let f = DataFile::open(&path, PartOfSpeech::Noun, Storage::Resident).unwrap();
        // One byte into the first record: the leading zero of the offset field
        // is skipped, so the record still parses but declares a different
        // offset from the one it was read at.
        let off_by_one = SynsetOffset::new(offsets[0].get() + 1);
        let err = f.synset(off_by_one).unwrap_err();
        assert!(
            matches!(
                err,
                Error::MalformedSynset {
                    kind: RecordError::OffsetMismatch {
                        requested,
                        found,
                    },
                    ..
                } if requested == off_by_one && found == offsets[0]
            ),
            "{err}"
        );

        // Further in, the field boundaries no longer line up at all, and the
        // record is refused on the first field that cannot be read.
        for skip in [2u32, 5, 10, 20, 40] {
            let mid = SynsetOffset::new(offsets[0].get() + skip);
            let err = f.synset(mid).unwrap_err();
            assert!(
                matches!(err, Error::MalformedSynset { .. }),
                "skip {skip}: {err}"
            );
        }
    }

    #[test]
    fn an_offset_inside_the_copyright_header_is_an_error() {
        let (path, _) = temp_data("header");
        let f = DataFile::open(&path, PartOfSpeech::Noun, Storage::Resident).unwrap();
        assert!(f.synset(SynsetOffset::new(0)).is_err());
    }

    #[test]
    fn an_offset_past_the_end_is_an_error_not_a_hang() {
        let (path, _) = temp_data("past");
        for storage in [
            Storage::Pread,
            Storage::LazyResident,
            Storage::Resident,
            Storage::Indexed,
        ] {
            let f = DataFile::open(&path, PartOfSpeech::Noun, storage).unwrap();
            let err = f.synset(SynsetOffset::new(u32::MAX)).unwrap_err();
            assert!(
                matches!(err, Error::OffsetOutOfRange { .. }),
                "{storage:?}: {err}"
            );
        }
    }

    #[test]
    fn a_record_from_the_wrong_category_is_refused() {
        let (path, offsets) = temp_data("wrongpos");
        let f = DataFile::open(&path, PartOfSpeech::Verb, Storage::Resident).unwrap();
        let err = f.synset(offsets[0]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::MalformedSynset {
                    kind: RecordError::InvalidField {
                        field: "ss_type",
                        ..
                    },
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn scanning_the_file_skips_the_header_and_finds_every_record() {
        let (path, offsets) = temp_data("scan");
        for storage in [
            Storage::Pread,
            Storage::LazyResident,
            Storage::Resident,
            Storage::Indexed,
        ] {
            let f = DataFile::open(&path, PartOfSpeech::Noun, storage).unwrap();
            let all: Vec<Synset> = f.synsets().collect::<Result<_>>().unwrap();
            assert_eq!(all.len(), 2, "{storage:?}");
            assert_eq!(all[0].offset, offsets[0], "{storage:?}");
            assert_eq!(all[1].offset, offsets[1], "{storage:?}");
            assert_eq!(all[0].lemma(), "alpha", "{storage:?}");
            assert_eq!(all[1].lemma(), "beta", "{storage:?}");
        }
    }

    #[test]
    fn files_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DataFile>();
    }
}
