//! Byte access to a dictionary file, and the four strategies for providing it.
//!
//! # What the reference does
//!
//! The reference reads **one byte per `fs.read` call**. `findPrevEOL` walks
//! backwards a byte at a time to the start of the enclosing line, allocating a
//! fresh 1 KiB `Buffer` on *every* step; `appendLineChar` then walks forwards a
//! byte at a time to the newline. Measured syscall counts for a single
//! operation: 1098 reads for `find('entity')` on `index.noun`, 1024 for
//! `find('node')`, 190 for `get(1740)` on `data.noun`. A `lookup('run')` opens
//! and closes 61 file descriptors and issues tens of thousands of one-byte
//! reads.
//!
//! None of that is observable in the results — only in the time. What *is*
//! observable is the sequence of byte positions the bisection probes, because
//! that sequence is what makes the search incomplete (see [`crate::index_file`]).
//! Every backend here reproduces the probe positions exactly and differs only in
//! how it fetches the bytes at them.
//!
//! # The strategies
//!
//! | [`Storage`] | Startup | Per query | Resident memory |
//! |---|---|---|---|
//! | [`Storage::Pread`] | none | a handful of positioned reads | none |
//! | [`Storage::LazyResident`] | none | in-memory after first touch | grows to the files used |
//! | [`Storage::Resident`] | reads the file | in-memory scan | the whole file |
//! | [`Storage::Indexed`] | reads the file + one `memchr` pass | in-memory, `partition_point` for line starts | the file plus 4 bytes per line |
//!
//! Measured numbers are in the crate docs and in `benches/wordnet.rs`.
//!
//! # Why there is no memory-mapped backend
//!
//! `mmap` cannot be reached from safe Rust without a dependency (`memmap2`),
//! and this workspace denies `unsafe_code` and admits no dependency outside the
//! curated `[workspace.dependencies]` list. [`Storage::LazyResident`] covers the
//! practical case `mmap` is wanted for — near-zero startup with in-memory query
//! cost once a file is touched — at the cost of paying for a whole file the
//! first time any part of it is read.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{Error, Result};

/// How a dictionary file's bytes are obtained.
///
/// All four reproduce identical results; they trade startup cost against
/// per-query cost and resident memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Storage {
    /// Positioned reads against an open descriptor. Nothing is preloaded.
    ///
    /// Closest in spirit to the reference, minus the one-byte-at-a-time
    /// pathology: a backwards scan reads 512-byte blocks and a forwards scan
    /// reads 4 KiB blocks, so a lookup costs a handful of syscalls rather than
    /// a thousand. Choose this for a short-lived process that performs one or
    /// two lookups.
    Pread,

    /// The whole file, loaded on first use and cached for the process lifetime.
    ///
    /// Startup is free and steady-state cost equals [`Storage::Resident`]; the
    /// first query against a file pays for reading it. This is the closest safe
    /// analogue of a memory map.
    LazyResident,

    /// The whole file, loaded eagerly when the dictionary is opened.
    #[default]
    Resident,

    /// [`Storage::Resident`] plus a prebuilt table of line-start offsets.
    ///
    /// `findPrevEOL` becomes a `partition_point` over `u32`s instead of a
    /// backwards byte scan. The table costs four bytes per line — about 470 KiB
    /// for `index.noun`'s 118k lines — and can be persisted with
    /// [`crate::prebuilt`] so that reopening skips the scan.
    Indexed,
}

/// A dictionary file, with whichever backing [`Storage`] was requested.
///
/// Read-only after construction and `Send + Sync`, so a single [`crate::WordNet`]
/// can serve queries from many threads concurrently.
#[derive(Debug)]
pub struct Source {
    path: PathBuf,
    /// Recorded once at open. The reference calls `fs.statSync` on *every*
    /// `find`, so a file that changed size mid-session would change the search
    /// path; that is documented as divergence **W5**.
    len: u64,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Pread(File),
    Lazy(OnceLock<Box<[u8]>>),
    Memory {
        bytes: Box<[u8]>,
        /// Byte offset of the start of each line: `0`, then one past every
        /// `\n`. Present only for [`Storage::Indexed`].
        line_starts: Option<Box<[u32]>>,
    },
}

impl Source {
    /// Opens `path` with the requested strategy.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be opened, or read when the
    /// strategy preloads it.
    pub fn open(path: &Path, storage: Storage) -> Result<Self> {
        let len = std::fs::metadata(path)
            .map_err(|e| Error::io(path, e))?
            .len();
        let kind = match storage {
            Storage::Pread => Kind::Pread(File::open(path).map_err(|e| Error::io(path, e))?),
            Storage::LazyResident => {
                // Fail fast on a missing file even though nothing is read yet:
                // the reference's silent stall is precisely what this crate
                // refuses to reproduce.
                File::open(path).map_err(|e| Error::io(path, e))?;
                Kind::Lazy(OnceLock::new())
            }
            Storage::Resident => Kind::Memory {
                bytes: read_all(path)?,
                line_starts: None,
            },
            Storage::Indexed => {
                let bytes = read_all(path)?;
                let line_starts = Some(build_line_starts(&bytes));
                Kind::Memory { bytes, line_starts }
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            len,
            kind,
        })
    }

    /// Opens `path` resident, reusing a previously built line-start table.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be read.
    pub fn open_with_line_starts(path: &Path, line_starts: Box<[u32]>) -> Result<Self> {
        let bytes = read_all(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            len: bytes.len() as u64,
            kind: Kind::Memory {
                bytes,
                line_starts: Some(line_starts),
            },
        })
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's length in bytes, as recorded when it was opened.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the file is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The line-start table, when this source carries one.
    #[must_use]
    pub fn line_starts(&self) -> Option<&[u32]> {
        match &self.kind {
            Kind::Memory { line_starts, .. } => line_starts.as_deref(),
            _ => None,
        }
    }

    /// The resident bytes, loading them if this is a lazy source.
    fn bytes(&self) -> Result<Option<&[u8]>> {
        match &self.kind {
            Kind::Memory { bytes, .. } => Ok(Some(bytes)),
            Kind::Lazy(cell) => {
                if let Some(b) = cell.get() {
                    return Ok(Some(b));
                }
                let loaded = read_all(&self.path)?;
                // A concurrent racer may have won; either copy is identical, so
                // whichever landed first is the one everyone uses.
                let _ = cell.set(loaded);
                Ok(cell.get().map(|b| &**b))
            }
            Kind::Pread(_) => Ok(None),
        }
    }

    /// `findPrevEOL`: the start of the line enclosing byte `pos`.
    ///
    /// Reproduces the reference exactly, including its special case: `pos == 0`
    /// answers `0` without looking at the byte there, and a `\n` **at** `pos`
    /// answers `pos + 1` — the start of the *next* line, not this one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NegativeProbe`] for a negative position and
    /// [`Error::Io`] if a positioned read fails.
    pub fn prev_eol(&self, pos: i64) -> Result<u64> {
        if pos < 0 {
            return Err(Error::NegativeProbe {
                path: self.path.clone(),
                position: pos,
            });
        }
        if pos == 0 {
            return Ok(0);
        }
        let pos = pos as u64;

        if let Some(starts) = self.line_starts() {
            // The answer is the greatest line start <= pos + 1: a newline sitting
            // exactly at `pos` opens the line beginning at `pos + 1`.
            let bound = pos.saturating_add(1);
            let idx = starts.partition_point(|&s| u64::from(s) <= bound);
            return Ok(u64::from(starts[idx - 1]));
        }

        if let Some(bytes) = self.bytes()? {
            let upto = (pos as usize).min(bytes.len().saturating_sub(1));
            return Ok(match memchr::memrchr(b'\n', &bytes[..=upto]) {
                Some(nl) => nl as u64 + 1,
                None => 0,
            });
        }

        // Positioned backwards scan, 512 bytes at a time. The reference does the
        // same walk one byte per syscall.
        const BACK: u64 = 512;
        let mut end = pos + 1;
        let mut buf = [0u8; BACK as usize];
        while end > 0 {
            let start = end.saturating_sub(BACK);
            let want = (end - start) as usize;
            let got = self.read_at(&mut buf[..want], start)?;
            if let Some(nl) = memchr::memrchr(b'\n', &buf[..got]) {
                return Ok(start + nl as u64 + 1);
            }
            end = start;
        }
        Ok(0)
    }

    /// `appendLineChar`: the bytes from `start` up to, but not including, the
    /// next `\n`.
    ///
    /// The returned slice borrows either the resident bytes or `scratch`,
    /// whichever backs this source, so the hot path copies nothing.
    ///
    /// Note that only `\n` terminates a line. A CRLF file leaves the `\r` in
    /// place, exactly as the reference's `buff.slice(0, buffPos)` does — which is
    /// then swallowed by the whitespace split into an extra empty token.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnterminatedLine`] when no `\n` follows `start` — the
    /// case in which the reference spins forever — or [`Error::Io`].
    pub fn line_at<'a>(&'a self, start: u64, scratch: &'a mut Vec<u8>) -> Result<&'a [u8]> {
        if let Some(bytes) = self.bytes()? {
            let s = start as usize;
            if s >= bytes.len() {
                return Err(self.unterminated(start));
            }
            return match memchr::memchr(b'\n', &bytes[s..]) {
                Some(nl) => Ok(&bytes[s..s + nl]),
                None => Err(self.unterminated(start)),
            };
        }

        const FWD: usize = 4096;
        scratch.clear();
        let mut pos = start;
        let mut chunk = [0u8; FWD];
        loop {
            let got = self.read_at(&mut chunk, pos)?;
            if got == 0 {
                return Err(self.unterminated(start));
            }
            if let Some(nl) = memchr::memchr(b'\n', &chunk[..got]) {
                scratch.extend_from_slice(&chunk[..nl]);
                return Ok(&scratch[..]);
            }
            scratch.extend_from_slice(&chunk[..got]);
            pos += got as u64;
        }
    }

    fn unterminated(&self, offset: u64) -> Error {
        Error::UnterminatedLine {
            path: self.path.clone(),
            offset,
        }
    }

    /// Fills `buf` from `offset`, returning how many bytes were available.
    ///
    /// Loops over short reads and retries `Interrupted`, so callers can treat a
    /// result below `buf.len()` as "end of file reached" rather than as a
    /// possible short read.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
        let Kind::Pread(file) = &self.kind else {
            unreachable!("read_at is only used by the Pread backend")
        };
        let mut total = 0usize;
        while total < buf.len() {
            match pread(file, &mut buf[total..], offset + total as u64) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::io(&self.path, e)),
            }
        }
        Ok(total)
    }
}

/// Reads a whole file into an exactly-sized boxed slice.
fn read_all(path: &Path) -> Result<Box<[u8]>> {
    let mut file = File::open(path).map_err(|e| Error::io(path, e))?;
    let len = file
        .metadata()
        .map_err(|e| Error::io(path, e))?
        .len()
        .try_into()
        .unwrap_or(usize::MAX);
    let mut buf = Vec::with_capacity(len);
    file.read_to_end(&mut buf).map_err(|e| Error::io(path, e))?;
    Ok(buf.into_boxed_slice())
}

/// Byte offset of the start of every line: `0`, then one past each `\n`.
///
/// Offsets are `u32` because the largest dictionary file is 15 MB; the builder
/// refuses anything larger rather than truncating.
#[must_use]
pub fn build_line_starts(bytes: &[u8]) -> Box<[u32]> {
    assert!(
        bytes.len() <= u32::MAX as usize,
        "line-start offsets are u32; this file is {} bytes",
        bytes.len()
    );
    // A rough capacity estimate from a typical dictionary line width avoids
    // most reallocation without a separate counting pass over the buffer:
    // the loop below already visits every newline once via `memchr_iter`, so
    // counting them first would scan the file twice for one result.
    let mut out = Vec::with_capacity(bytes.len() / 32 + 1);
    out.push(0u32);
    for nl in memchr::memchr_iter(b'\n', bytes) {
        let next = nl as u32 + 1;
        if (next as usize) < bytes.len() {
            out.push(next);
        }
    }
    out.into_boxed_slice()
}

#[cfg(unix)]
fn pread(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn pread(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

#[cfg(not(any(unix, windows)))]
fn pread(_file: &File, _buf: &mut [u8], _offset: u64) -> std::io::Result<usize> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Storage::Pread needs positioned reads; use Storage::Resident instead",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, body: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "verbora-wordnet-source-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(body).unwrap();
        path
    }

    const BODY: &[u8] = b"aaa line\nbbb line\nccc line\n";

    fn all_storages(path: &Path) -> Vec<(Storage, Source)> {
        [
            Storage::Pread,
            Storage::LazyResident,
            Storage::Resident,
            Storage::Indexed,
        ]
        .into_iter()
        .map(|s| (s, Source::open(path, s).unwrap()))
        .collect()
    }

    #[test]
    fn every_backend_agrees_on_line_starts() {
        let path = temp_file("agree", BODY);
        for (storage, src) in all_storages(&path) {
            assert_eq!(src.len(), BODY.len() as u64, "{storage:?}");
            // pos 0 short-circuits, a newline at pos opens the next line.
            assert_eq!(src.prev_eol(0).unwrap(), 0, "{storage:?}");
            assert_eq!(src.prev_eol(3).unwrap(), 0, "{storage:?}");
            assert_eq!(src.prev_eol(8).unwrap(), 9, "{storage:?} newline at 8");
            assert_eq!(src.prev_eol(9).unwrap(), 9, "{storage:?}");
            assert_eq!(src.prev_eol(20).unwrap(), 18, "{storage:?}");
        }
    }

    #[test]
    fn every_backend_agrees_on_lines() {
        let path = temp_file("lines", BODY);
        for (storage, src) in all_storages(&path) {
            let mut scratch = Vec::new();
            assert_eq!(
                src.line_at(0, &mut scratch).unwrap(),
                b"aaa line",
                "{storage:?}"
            );
            let mut scratch = Vec::new();
            assert_eq!(
                src.line_at(9, &mut scratch).unwrap(),
                b"bbb line",
                "{storage:?}"
            );
            let mut scratch = Vec::new();
            assert_eq!(
                src.line_at(13, &mut scratch).unwrap(),
                b"line",
                "{storage:?}"
            );
        }
    }

    #[test]
    fn a_line_without_a_newline_is_an_error_not_a_hang() {
        let path = temp_file("noeol", b"aaa\nbbb");
        for (storage, src) in all_storages(&path) {
            let mut scratch = Vec::new();
            let err = src.line_at(4, &mut scratch).unwrap_err();
            assert!(
                matches!(err, Error::UnterminatedLine { .. }),
                "{storage:?}: {err}"
            );
        }
    }

    #[test]
    fn a_long_line_survives_the_chunked_forward_scan() {
        // Longer than the 4 KiB forward chunk, so the Pread path has to stitch.
        let long = format!("{}\n", "x".repeat(10_000));
        let path = temp_file("long", long.as_bytes());
        for (storage, src) in all_storages(&path) {
            let mut scratch = Vec::new();
            assert_eq!(
                src.line_at(0, &mut scratch).unwrap().len(),
                10_000,
                "{storage:?}"
            );
        }
    }

    #[test]
    fn a_long_line_survives_the_chunked_backward_scan() {
        let mut body = vec![b'y'; 2000];
        body.push(b'\n');
        body.extend(std::iter::repeat_n(b'z', 2000));
        body.push(b'\n');
        let path = temp_file("longback", &body);
        for (storage, src) in all_storages(&path) {
            assert_eq!(src.prev_eol(3000).unwrap(), 2001, "{storage:?}");
            assert_eq!(src.prev_eol(1500).unwrap(), 0, "{storage:?}");
        }
    }

    #[test]
    fn negative_probes_are_reported() {
        let path = temp_file("neg", BODY);
        let src = Source::open(&path, Storage::Resident).unwrap();
        assert!(matches!(
            src.prev_eol(-1).unwrap_err(),
            Error::NegativeProbe { .. }
        ));
    }

    #[test]
    fn line_starts_table_matches_a_manual_scan() {
        assert_eq!(&*build_line_starts(BODY), &[0, 9, 18]);
        assert_eq!(&*build_line_starts(b""), &[0]);
        assert_eq!(&*build_line_starts(b"\n"), &[0]);
        assert_eq!(&*build_line_starts(b"\n\n"), &[0, 1]);
        assert_eq!(&*build_line_starts(b"abc"), &[0]);
    }

    #[test]
    fn sources_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Source>();
    }

    #[test]
    fn a_missing_file_is_an_error_for_every_backend() {
        let missing = Path::new("/no/such/dir/index.noun");
        for s in [
            Storage::Pread,
            Storage::LazyResident,
            Storage::Resident,
            Storage::Indexed,
        ] {
            assert!(
                matches!(Source::open(missing, s), Err(Error::Io { .. })),
                "{s:?}"
            );
        }
    }
}
