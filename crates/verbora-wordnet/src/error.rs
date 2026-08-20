//! Errors.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::synset::SynsetOffset;

/// The result type used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Something went wrong locating, reading or parsing the dictionary.
///
/// Every variant names the file it concerns, and the two record variants also
/// name the exact byte position of the record that failed, so a malformed
/// dictionary can be inspected with `dd`/`sed` without guesswork.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A dictionary file could not be opened or read.
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying operating-system error.
        source: std::io::Error,
    },

    /// [`WordNet::from_env`](crate::WordNet::from_env) found no dictionary.
    DictionaryNotFound {
        /// Every candidate directory that was checked, in the order checked.
        tried: Vec<PathBuf>,
    },

    /// A line of an `index.*` file did not match the documented format.
    MalformedIndexEntry {
        /// The index file being read.
        path: PathBuf,
        /// Byte offset at which the offending line begins.
        line_start: u64,
        /// What specifically was wrong.
        kind: RecordError,
    },

    /// A record of a `data.*` file did not match the documented format.
    MalformedSynset {
        /// The data file being read.
        path: PathBuf,
        /// The offset the record was read from.
        offset: SynsetOffset,
        /// What specifically was wrong.
        kind: RecordError,
    },

    /// A synset offset lies at or beyond the end of its data file.
    OffsetOutOfRange {
        /// The data file being read.
        path: PathBuf,
        /// The offset that was asked for.
        offset: SynsetOffset,
        /// The file's length in bytes, as recorded when it was opened.
        file_len: u64,
    },

    /// A file is too large for [`Storage::Indexed`](crate::Storage::Indexed)'s
    /// `u32` line-start table.
    FileTooLarge {
        /// The file in question.
        path: PathBuf,
        /// Its length in bytes.
        len: u64,
        /// The largest length that can be indexed.
        limit: u64,
    },

    /// A prebuilt index sidecar could not be used.
    ///
    /// The dictionary text files are always the source of truth; a sidecar that
    /// no longer describes them is refused rather than trusted.
    Prebuilt {
        /// The sidecar, or the dictionary file whose entry was rejected.
        path: PathBuf,
        /// Why it was refused.
        reason: String,
    },
}

/// What was wrong with one dictionary record.
///
/// Field names are the ones the WordNet database format documentation
/// (`wndb(5WN)`) uses, so a message can be matched against the specification
/// directly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordError {
    /// The record ended before a required field.
    MissingField {
        /// The field's name in `wndb(5WN)`.
        field: &'static str,
    },
    /// A field was present but not a legal value for that field.
    InvalidField {
        /// The field's name in `wndb(5WN)`.
        field: &'static str,
        /// The text that was found there.
        value: String,
    },
    /// A `data.*` record contained no `|` gloss delimiter.
    MissingGloss,
    /// An index line's `synset_cnt` and its redundant `sense_cnt` copy disagree.
    ///
    /// `wndb(5WN)` states that `sense_cnt` is "the same as `synset_cnt`",
    /// retained only for backward compatibility. A file where they differ is
    /// malformed, and guessing which one to believe would silently drop or
    /// invent senses.
    SenseCountMismatch {
        /// The first count on the line.
        synset_cnt: u32,
        /// The redundant copy that should have equalled it.
        sense_cnt: u32,
    },
    /// A record read at offset *X* declares its own offset to be *Y*.
    ///
    /// Every `data.*` record begins with its own byte offset, so this check
    /// catches an offset that does not point at the start of a record — which
    /// would otherwise parse into a plausible-looking synset assembled from the
    /// middle of a real one.
    OffsetMismatch {
        /// The offset the record was read from.
        requested: SynsetOffset,
        /// The offset the record declares.
        found: SynsetOffset,
    },
}

impl Error {
    /// Wraps an I/O error with the path that produced it.
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Wraps a record error with the index file and line that produced it.
    pub(crate) fn index(path: &Path, line_start: u64, kind: RecordError) -> Self {
        Self::MalformedIndexEntry {
            path: path.to_path_buf(),
            line_start,
            kind,
        }
    }

    /// Wraps a record error with the data file and offset that produced it.
    pub(crate) fn synset(path: &Path, offset: SynsetOffset, kind: RecordError) -> Self {
        Self::MalformedSynset {
            path: path.to_path_buf(),
            offset,
            kind,
        }
    }

    /// Builds a sidecar rejection.
    pub(crate) fn prebuilt(path: &Path, reason: impl Into<String>) -> Self {
        Self::Prebuilt {
            path: path.to_path_buf(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::DictionaryNotFound { tried } => {
                write!(f, "no WordNet dictionary found (tried: ")?;
                for (i, p) in tried.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", p.display())?;
                }
                f.write_str(
                    "); the database is separately licensed and not shipped with this crate, \
                     so point $WORDNET_DB_PATH at a directory holding index.noun and its \
                     seven siblings",
                )
            }
            Self::MalformedIndexEntry {
                path,
                line_start,
                kind,
            } => write!(
                f,
                "{}: malformed index entry at byte {line_start}: {kind}",
                path.display()
            ),
            Self::MalformedSynset { path, offset, kind } => write!(
                f,
                "{}: malformed synset at byte {offset}: {kind}",
                path.display()
            ),
            Self::OffsetOutOfRange {
                path,
                offset,
                file_len,
            } => write!(
                f,
                "{}: synset offset {offset} is at or past the end of the file ({file_len} bytes)",
                path.display()
            ),
            Self::FileTooLarge { path, len, limit } => write!(
                f,
                "{}: {len} bytes exceeds the {limit}-byte limit for an indexed file; \
                 use Storage::Resident instead",
                path.display()
            ),
            Self::Prebuilt { path, reason } => {
                write!(f, "{}: unusable prebuilt index: {reason}", path.display())
            }
        }
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { field } => write!(f, "field {field} is missing"),
            Self::InvalidField { field, value } => {
                write!(f, "field {field} is not valid: {value:?}")
            }
            Self::MissingGloss => f.write_str("no '|' gloss delimiter"),
            Self::SenseCountMismatch {
                synset_cnt,
                sense_cnt,
            } => write!(
                f,
                "synset_cnt is {synset_cnt} but its redundant sense_cnt copy is {sense_cnt}"
            ),
            Self::OffsetMismatch { requested, found } => write!(
                f,
                "read from byte {requested} but the record declares offset {found}; \
                 the offset does not point at the start of a synset"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl std::error::Error for RecordError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
        assert_send_sync::<RecordError>();
    }

    #[test]
    fn messages_name_the_file_and_the_position() {
        let e = Error::synset(
            Path::new("/d/data.noun"),
            SynsetOffset::new(1740),
            RecordError::MissingGloss,
        );
        let s = e.to_string();
        assert!(s.contains("/d/data.noun"), "{s}");
        // Offsets print in the eight-digit form the files themselves use.
        assert!(s.contains("00001740"), "{s}");
        assert!(s.contains("gloss"), "{s}");
    }

    #[test]
    fn a_missing_dictionary_names_every_candidate_and_how_to_get_one() {
        let e = Error::DictionaryNotFound {
            tried: vec![PathBuf::from("/a/dict"), PathBuf::from("dict")],
        };
        let s = e.to_string();
        assert!(s.contains("/a/dict"), "{s}");
        assert!(s.contains("dict"), "{s}");
        assert!(s.contains("WORDNET_DB_PATH"), "{s}");
        assert!(s.contains("separately licensed"), "{s}");
        // An empty candidate list still produces a well-formed sentence.
        assert!(
            Error::DictionaryNotFound { tried: Vec::new() }
                .to_string()
                .contains("no WordNet dictionary found")
        );
    }

    #[test]
    fn a_field_error_names_the_field_from_the_format_documentation() {
        let e = RecordError::InvalidField {
            field: "w_cnt",
            value: "zz".to_owned(),
        };
        assert_eq!(e.to_string(), "field w_cnt is not valid: \"zz\"");
    }
}
