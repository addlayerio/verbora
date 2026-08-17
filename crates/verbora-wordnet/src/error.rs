//! Errors, and what the reference implementation does instead.
//!
//! The reference never reports a WordNet failure. `WordNetFile#open` logs
//! `Unable to open %s` and returns **without invoking its callback**, so the
//! whole chain stalls and the caller waits forever; a data offset past the end of
//! the file makes `appendLineChar` recurse indefinitely; and a malformed record
//! throws from inside an `fs` callback, where no `try`/`catch` at the call site
//! can see it.
//!
//! Every variant here therefore corresponds to a case the reference either never
//! answers or answers by crashing the process. Returning an error instead is the
//! crate's one deliberate, blanket divergence — see the crate-level docs.

use std::fmt;
use std::path::{Path, PathBuf};

/// The result type used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Something went wrong reading or parsing the dictionary.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A dictionary file could not be opened or read.
    ///
    /// Reference behaviour: `console.log('Unable to open %s')`, then the
    /// callback is never invoked.
    Io {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying operating-system error.
        source: std::io::Error,
    },

    /// A line starting at `offset` ran to the end of the file without a `\n`.
    ///
    /// Reference behaviour: `appendLineChar` recurses forever — `fs.read`
    /// returns `count = 0`, the zero-filled buffer never yields byte `0x0A`, and
    /// the buffer doubles until the process dies.
    UnterminatedLine {
        /// The file being read.
        path: PathBuf,
        /// The byte offset the read started from.
        offset: u64,
    },

    /// A data record contained no `'| '` gloss separator.
    ///
    /// Reference behaviour: `data[1]` is `undefined`, and `data[1].split('; ')`
    /// throws `TypeError: Cannot read properties of undefined (reading 'split')`
    /// asynchronously, from inside the `fs` callback.
    MissingGloss {
        /// The file being read.
        path: PathBuf,
        /// The byte offset the record was read from.
        offset: u64,
    },

    /// A synset offset was not a usable file position.
    ///
    /// Reference behaviour: `fs.read` throws `The "position" argument must be of
    /// type bigint or integer`, again from inside the callback. Reachable
    /// through `getSynonyms({ synsetOffset: 0, … })`, where the falsy `0` makes
    /// The reference substitute the record object itself for the offset.
    InvalidOffset {
        /// The file being read.
        path: PathBuf,
        /// The offset as parsed; `NaN` when the token was absent or unparsable.
        offset: f64,
    },

    /// A part-of-speech tag outside `n`, `v`, `a`, `s`, `r`.
    ///
    /// Reference behaviour: `getDataFile` has no `default` clause, returns
    /// `undefined`, and the caller throws
    /// `TypeError: Cannot read properties of undefined (reading 'get')`
    /// synchronously.
    UnknownPos(String),

    /// The index bisection probed a negative byte position.
    ///
    /// Reference behaviour: `fs.read` with a position below `-1` throws
    /// `ERR_OUT_OF_RANGE`, so nothing is ever delivered. Verified unreachable
    /// for all 147,580 keys the fixture exercises across all eleven dictionary
    /// files, but the arithmetic does not forbid it, so it is reported rather
    /// than silently clamped.
    NegativeProbe {
        /// The file being searched.
        path: PathBuf,
        /// The probe position that went negative.
        position: i64,
    },

    /// A parsed count would have required an absurd allocation.
    ///
    /// Reference behaviour: the reference loops that many times regardless — for a
    /// mid-record offset whose `tokens[3]` parses as a large hexadecimal number,
    /// that means pushing hundreds of millions of `undefined`s until the process
    /// runs out of memory. See [`crate::numfmt::MAX_COUNT`].
    CountTooLarge {
        /// Which field produced the count (`"wCnt"` or `"p_cnt"`).
        field: &'static str,
        /// The parsed value.
        value: f64,
    },

    /// A prebuilt index sidecar could not be used.
    ///
    /// Has no reference counterpart: the prebuilt index is an optimisation this
    /// crate adds, and the dictionary text files remain the source of truth.
    Prebuilt(String),
}

impl Error {
    /// Wraps an I/O error with the path that produced it.
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::UnterminatedLine { path, offset } => write!(
                f,
                "{}: no newline after byte {offset} (the reference loops forever here)",
                path.display()
            ),
            Self::MissingGloss { path, offset } => write!(
                f,
                "{}: record at byte {offset} has no '| ' gloss separator \
                 (the reference throws TypeError reading 'split')",
                path.display()
            ),
            Self::InvalidOffset { path, offset } => {
                write!(f, "{}: {offset} is not a byte offset", path.display())
            }
            Self::UnknownPos(tag) => write!(
                f,
                "unknown part-of-speech tag {tag:?}; expected one of n, v, a, s, r"
            ),
            Self::NegativeProbe { path, position } => write!(
                f,
                "{}: index bisection probed negative position {position}",
                path.display()
            ),
            Self::CountTooLarge { field, value } => {
                write!(f, "{field} = {value} exceeds the supported maximum")
            }
            Self::Prebuilt(msg) => write!(f, "prebuilt index unusable: {msg}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn messages_name_the_reference_behaviour() {
        let e = Error::MissingGloss {
            path: PathBuf::from("/d/data.noun"),
            offset: 0,
        };
        assert!(e.to_string().contains("'| '"));
        assert!(
            Error::UnknownPos("x".into())
                .to_string()
                .contains("n, v, a, s, r")
        );
    }
}
