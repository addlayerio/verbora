//! Byte access to a dictionary file, and the four strategies for providing it.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{Error, Result};

/// The largest dictionary file this crate will hold in memory.
///
/// Two reasons meet at the same number. [`Storage::Indexed`] stores line starts
/// as `u32`, so a file it indexes must fit in 4 GiB or an offset would be
/// truncated and silently return the wrong line. And the three resident
/// strategies each commit the whole file to memory, where the file's length is
/// an *input* — [`WordNet::open`](crate::WordNet::open) takes a caller-supplied
/// path — so it is a number to check rather than to hand to an allocator.
///
/// WordNet 3.1's largest file, `data.noun`, is about 16 MB: four thousand times
/// smaller, which is why the limit costs nothing to respect. A file past it is
/// still readable with [`Storage::Pread`], which holds one line at a time.
pub(crate) const MAX_FILE_LEN: u64 = u32::MAX as u64;

/// How much of a file's reported length [`read_all`] will reserve before it
/// starts reading.
///
/// `metadata().len()` is what the filesystem said a moment ago, not a promise
/// about what the read will yield: the file may be sparse, may be truncated
/// between the two calls, or may simply be larger than this process can hold.
/// So the reported length sizes the buffer only up to this ceiling, above which
/// the read grows the buffer as it actually fills it. Every real dictionary file
/// is orders of magnitude below the ceiling and is still allocated exactly once.
const MAX_READ_RESERVE: u64 = 64 * 1024 * 1024;

/// How a dictionary file's bytes are obtained.
///
/// All four answer every query identically; they trade startup cost against
/// per-query cost and resident memory. See the crate-level
/// "Choosing the right API" section for which to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Storage {
    /// Positioned reads against an open descriptor. Nothing is preloaded.
    ///
    /// A backwards line scan reads 512-byte blocks and a forwards line read
    /// reads 4 KiB blocks, so one lookup costs a handful of syscalls. Choose
    /// this for a short-lived process that performs one or two lookups, or when
    /// resident memory matters more than latency.
    Pread,

    /// The whole file, read on first use and cached for the process lifetime.
    ///
    /// Startup is free and steady-state cost equals [`Storage::Resident`]; the
    /// first query against a file pays for reading it. This is the closest safe
    /// analogue of a memory map — see the crate docs for why there is no `mmap`
    /// backend.
    LazyResident,

    /// The whole file, read eagerly when the dictionary is opened.
    #[default]
    Resident,

    /// [`Storage::Resident`] plus a table of line-start offsets.
    ///
    /// Locating the line enclosing a byte position becomes a `partition_point`
    /// over `u32`s instead of a backwards byte scan. The table costs four bytes
    /// per line and can be persisted with [`PrebuiltIndex`](crate::PrebuiltIndex)
    /// so that reopening skips the scan.
    Indexed,
}

/// A dictionary file, with whichever backing [`Storage`] was requested.
///
/// Read-only after construction and `Send + Sync`, so a single
/// [`WordNet`](crate::WordNet) can serve queries from many threads at once.
#[derive(Debug)]
pub(crate) struct Source {
    path: PathBuf,
    /// Recorded once, when the file is opened. Every later answer is computed
    /// against this length, so a file that changes size mid-session does not
    /// change a search that is already in flight.
    len: u64,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Pread(File),
    Lazy(OnceLock<Box<[u8]>>),
    Memory {
        bytes: Box<[u8]>,
        /// Byte offset of the start of every line: `0`, then one past every
        /// `\n`. Present only for [`Storage::Indexed`].
        line_starts: Option<Box<[u32]>>,
    },
}

/// One line of a dictionary file, as handed to [`Source::with_line`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line<'a> {
    /// The line's bytes, without its `\n` terminator and without a `\r`
    /// immediately preceding that terminator.
    pub bytes: &'a [u8],
    /// Byte offset of the next line, or the file length if this is the last.
    pub next: u64,
}

impl Source {
    /// Opens `path` with the requested strategy.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be opened, or read when the strategy
    /// preloads it; [`Error::FileTooLarge`] if any strategy that holds the file
    /// in memory — every one but [`Storage::Pread`] — is given a file of 4 GiB
    /// or more.
    pub(crate) fn open(path: &Path, storage: Storage) -> Result<Self> {
        let len = std::fs::metadata(path)
            .map_err(|e| Error::io(path, e))?
            .len();
        let kind = match storage {
            Storage::Pread => Kind::Pread(File::open(path).map_err(|e| Error::io(path, e))?),
            Storage::LazyResident => {
                // Fail now on a missing or unreadable file even though nothing
                // is read yet: a dictionary that reports success at open and
                // fails at the first query is harder to diagnose, not easier.
                File::open(path).map_err(|e| Error::io(path, e))?;
                // A file too large to ever become resident is the same kind of
                // failure, known at the same moment, so it is reported at the
                // same moment — `read_all` would refuse it at the first query
                // regardless.
                if len > MAX_FILE_LEN {
                    return Err(Error::FileTooLarge {
                        path: path.to_path_buf(),
                        len,
                        limit: MAX_FILE_LEN,
                    });
                }
                Kind::Lazy(OnceLock::new())
            }
            Storage::Resident => Kind::Memory {
                bytes: read_all(path)?,
                line_starts: None,
            },
            Storage::Indexed => {
                let bytes = read_all(path)?;
                let line_starts = Some(build_line_starts(path, &bytes)?);
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
    /// [`Error::Io`] if the file cannot be read.
    pub(crate) fn open_with_line_starts(path: &Path, line_starts: Box<[u32]>) -> Result<Self> {
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

    /// The file's length in bytes, as recorded when it was opened.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// The line-start table, when this source carries one.
    fn line_starts(&self) -> Option<&[u32]> {
        match &self.kind {
            Kind::Memory { line_starts, .. } => line_starts.as_deref(),
            Kind::Lazy(_) | Kind::Pread(_) => None,
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
                // whichever landed first is the one every thread then uses.
                let _ = cell.set(loaded);
                Ok(cell.get().map(|b| &**b))
            }
            Kind::Pread(_) => Ok(None),
        }
    }

    /// The byte offset at which the line **containing** `pos` begins.
    ///
    /// A `\n` at `pos` belongs to the line it terminates, so `line_start` of a
    /// newline is the start of that same line, not of the next one. A `pos` past
    /// the last byte is treated as the last byte, so the answer is always the
    /// start of a line that exists.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a positioned read fails.
    pub(crate) fn line_start(&self, pos: u64) -> Result<u64> {
        let pos = pos.min(self.len.saturating_sub(1));
        if pos == 0 {
            return Ok(0);
        }

        if let Some(starts) = self.line_starts() {
            // The greatest recorded start that is <= pos. A table this crate
            // built always begins with 0, so the partition point is at least 1;
            // a table loaded from a sidecar is caller-supplied data, so the
            // subtraction is checked rather than assumed.
            let idx = starts.partition_point(|&s| u64::from(s) <= pos);
            return Ok(idx
                .checked_sub(1)
                .and_then(|i| starts.get(i))
                .map_or(0, |&s| u64::from(s)));
        }

        if let Some(bytes) = self.bytes()? {
            let upto = (pos as usize).min(bytes.len());
            return Ok(match memchr::memrchr(b'\n', &bytes[..upto]) {
                Some(nl) => nl as u64 + 1,
                None => 0,
            });
        }

        // Positioned backwards scan, 512 bytes at a time.
        const BACK: u64 = 512;
        let mut end = pos;
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

    /// Hands the line beginning at `start` to `f`, or answers `None` when
    /// `start` is at or past the end of the file.
    ///
    /// A line runs to the next `\n` or to the end of the file, whichever comes
    /// first; a final line without a trailing newline is still a line. One `\r`
    /// immediately before the terminator is dropped, so a dictionary saved with
    /// CRLF endings parses identically to one saved with LF.
    ///
    /// `scratch` is a caller-owned buffer the positioned-read backend fills;
    /// resident backends ignore it and hand out a subslice of the file, so the
    /// hot path copies nothing.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a positioned read fails.
    pub(crate) fn with_line<R>(
        &self,
        start: u64,
        scratch: &mut Vec<u8>,
        f: impl FnOnce(Line<'_>) -> R,
    ) -> Result<Option<R>> {
        if start >= self.len {
            return Ok(None);
        }

        if let Some(bytes) = self.bytes()? {
            let s = (start as usize).min(bytes.len());
            let (end, next) = match memchr::memchr(b'\n', &bytes[s..]) {
                Some(nl) => (s + nl, (s + nl + 1) as u64),
                // A final line without a newline still ends the file, and
                // `next` is past `start`, so a forward scan terminates.
                None => (bytes.len(), (bytes.len() as u64).max(start + 1)),
            };
            return Ok(Some(f(Line {
                bytes: strip_cr(&bytes[s..end]),
                next,
            })));
        }

        const FWD: usize = 4096;
        scratch.clear();
        let mut pos = start;
        let mut chunk = [0u8; FWD];
        let next = loop {
            let got = self.read_at(&mut chunk, pos)?;
            if got == 0 {
                // The file shrank under us. Report the line as ending here, and
                // never at `start`: a caller scanning forward must always be
                // able to make progress.
                break pos.max(start.saturating_add(1));
            }
            if let Some(nl) = memchr::memchr(b'\n', &chunk[..got]) {
                scratch.extend_from_slice(&chunk[..nl]);
                break pos + nl as u64 + 1;
            }
            scratch.extend_from_slice(&chunk[..got]);
            pos += got as u64;
        };
        Ok(Some(f(Line {
            bytes: strip_cr(scratch),
            next,
        })))
    }

    /// Fills `buf` from `offset`, returning how many bytes were available.
    ///
    /// Loops over short reads and retries `Interrupted`, so callers can treat a
    /// result below `buf.len()` as "end of file reached" rather than as a
    /// possible short read.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> Result<usize> {
        let Kind::Pread(file) = &self.kind else {
            // The two resident kinds are served entirely by `bytes()`; every
            // call site checks that first. This is an internal invariant, not
            // an input the caller can violate.
            unreachable!("read_at is reachable only from the Pread backend")
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

/// Drops one `\r` immediately preceding the (already removed) `\n`.
fn strip_cr(line: &[u8]) -> &[u8] {
    match line.split_last() {
        Some((b'\r', rest)) => rest,
        _ => line,
    }
}

/// Reads a whole file into an exactly-sized boxed slice.
///
/// # Errors
///
/// [`Error::Io`] if the file cannot be opened or read;
/// [`Error::FileTooLarge`] if it is longer than [`MAX_FILE_LEN`], either as
/// reported before the read or as observed during it.
fn read_all(path: &Path) -> Result<Box<[u8]>> {
    let mut file = File::open(path).map_err(|e| Error::io(path, e))?;
    let len = file.metadata().map_err(|e| Error::io(path, e))?.len();
    let too_large = |len: u64| Error::FileTooLarge {
        path: path.to_path_buf(),
        len,
        limit: MAX_FILE_LEN,
    };
    if len > MAX_FILE_LEN {
        return Err(too_large(len));
    }

    // Capped, because `len` is a report about the file rather than a promise
    // about the read — see `MAX_READ_RESERVE`. Below the cap it is exact, so
    // every real dictionary file is still one allocation. Fallible, and the
    // failure is ignored, because a reservation is an optimisation: without it
    // the read below grows the buffer as it fills.
    let reserve = usize::try_from(len.min(MAX_READ_RESERVE)).unwrap_or(0);
    let mut buf = Vec::new();
    let _ = buf.try_reserve_exact(reserve);

    // One byte past the limit, so a file that grew between the metadata call
    // and the read is refused on what was actually read rather than on what was
    // promised. `read_to_end` grows the buffer itself past `reserve`.
    let read = file
        .by_ref()
        .take(MAX_FILE_LEN + 1)
        .read_to_end(&mut buf)
        .map_err(|e| Error::io(path, e))?;
    if read as u64 > MAX_FILE_LEN {
        return Err(too_large(read as u64));
    }
    Ok(buf.into_boxed_slice())
}

/// Byte offset of the start of every line: `0`, then one past each `\n`.
///
/// # Errors
///
/// [`Error::FileTooLarge`] when `bytes` is 4 GiB or larger, which `u32` offsets
/// cannot address.
pub(crate) fn build_line_starts(path: &Path, bytes: &[u8]) -> Result<Box<[u32]>> {
    if bytes.len() as u64 > MAX_FILE_LEN {
        return Err(Error::FileTooLarge {
            path: path.to_path_buf(),
            len: bytes.len() as u64,
            limit: MAX_FILE_LEN,
        });
    }
    // A capacity estimate from a typical dictionary line width avoids most
    // reallocation without a separate counting pass: the loop below already
    // visits every newline once, so counting first would scan the file twice.
    let mut out = Vec::with_capacity(bytes.len() / 32 + 1);
    out.push(0u32);
    for nl in memchr::memchr_iter(b'\n', bytes) {
        let next = nl as u32 + 1;
        if (next as usize) < bytes.len() {
            out.push(next);
        }
    }
    Ok(out.into_boxed_slice())
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

    /// `line_start` is defined arithmetically: the answer for byte `p` is
    /// `1 + max{i < p : bytes[i] == b'\n'}`, or 0 when there is no such `i`.
    /// The expectations below are that formula evaluated by hand over `BODY`,
    /// whose newlines sit at bytes 8, 17 and 26.
    #[test]
    fn every_backend_agrees_on_line_starts() {
        let path = temp_file("agree", BODY);
        for (storage, src) in all_storages(&path) {
            assert_eq!(src.len(), BODY.len() as u64, "{storage:?}");
            assert_eq!(src.line_start(0).unwrap(), 0, "{storage:?}");
            assert_eq!(src.line_start(3).unwrap(), 0, "{storage:?}");
            // The newline at byte 8 terminates the line that starts at 0.
            assert_eq!(src.line_start(8).unwrap(), 0, "{storage:?}");
            assert_eq!(src.line_start(9).unwrap(), 9, "{storage:?}");
            assert_eq!(src.line_start(17).unwrap(), 9, "{storage:?}");
            assert_eq!(src.line_start(18).unwrap(), 18, "{storage:?}");
            assert_eq!(src.line_start(20).unwrap(), 18, "{storage:?}");
            assert_eq!(src.line_start(26).unwrap(), 18, "{storage:?}");
            // Past the last byte clamps to it, so the answer is a real line.
            assert_eq!(src.line_start(27).unwrap(), 18, "{storage:?}");
            assert_eq!(src.line_start(9_999).unwrap(), 18, "{storage:?}");
        }
    }

    fn line(src: &Source, start: u64) -> Option<(Vec<u8>, u64)> {
        let mut scratch = Vec::new();
        src.with_line(start, &mut scratch, |l| (l.bytes.to_vec(), l.next))
            .unwrap()
    }

    #[test]
    fn every_backend_agrees_on_lines() {
        let path = temp_file("lines", BODY);
        for (storage, src) in all_storages(&path) {
            assert_eq!(
                line(&src, 0),
                Some((b"aaa line".to_vec(), 9)),
                "{storage:?}"
            );
            assert_eq!(
                line(&src, 9),
                Some((b"bbb line".to_vec(), 18)),
                "{storage:?}"
            );
            assert_eq!(line(&src, 13), Some((b"line".to_vec(), 18)), "{storage:?}");
            assert_eq!(line(&src, 27), None, "{storage:?} at EOF");
            assert_eq!(line(&src, 99), None, "{storage:?} past EOF");
        }
    }

    #[test]
    fn a_final_line_without_a_newline_is_still_a_line() {
        let path = temp_file("noeol", b"aaa\nbbb");
        for (storage, src) in all_storages(&path) {
            assert_eq!(line(&src, 4), Some((b"bbb".to_vec(), 7)), "{storage:?}");
            assert_eq!(line(&src, 7), None, "{storage:?}");
        }
    }

    #[test]
    fn crlf_endings_lose_only_the_carriage_return() {
        let path = temp_file("crlf", b"aaa\r\nbbb\r\n");
        for (storage, src) in all_storages(&path) {
            assert_eq!(line(&src, 0), Some((b"aaa".to_vec(), 5)), "{storage:?}");
            assert_eq!(line(&src, 5), Some((b"bbb".to_vec(), 10)), "{storage:?}");
            // A `\r` that is not immediately before the terminator survives.
            assert_eq!(src.line_start(4).unwrap(), 0, "{storage:?}");
        }
    }

    #[test]
    fn a_long_line_survives_the_chunked_forward_scan() {
        // Longer than the 4 KiB forward chunk, so the Pread path has to stitch.
        let long = format!("{}\n", "x".repeat(10_000));
        let path = temp_file("long", long.as_bytes());
        for (storage, src) in all_storages(&path) {
            assert_eq!(line(&src, 0).unwrap().0.len(), 10_000, "{storage:?}");
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
            assert_eq!(src.line_start(3000).unwrap(), 2001, "{storage:?}");
            assert_eq!(src.line_start(1500).unwrap(), 0, "{storage:?}");
        }
    }

    #[test]
    fn an_empty_file_has_no_lines() {
        let path = temp_file("empty", b"");
        for (storage, src) in all_storages(&path) {
            assert_eq!(src.len(), 0, "{storage:?}");
            assert_eq!(src.line_start(0).unwrap(), 0, "{storage:?}");
            assert_eq!(line(&src, 0), None, "{storage:?}");
        }
    }

    #[test]
    fn line_starts_table_matches_the_arithmetic_definition() {
        let p = Path::new("x");
        assert_eq!(&*build_line_starts(p, BODY).unwrap(), &[0, 9, 18]);
        assert_eq!(&*build_line_starts(p, b"").unwrap(), &[0]);
        assert_eq!(&*build_line_starts(p, b"\n").unwrap(), &[0]);
        assert_eq!(&*build_line_starts(p, b"\n\n").unwrap(), &[0, 1]);
        assert_eq!(&*build_line_starts(p, b"abc").unwrap(), &[0]);
    }

    #[test]
    fn sources_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Source>();
    }

    /// A dictionary file's length is an *input*: [`WordNet::open`] takes a
    /// caller-supplied path, so `metadata().len()` is a number the caller
    /// chose. A strategy that hands it straight to an allocator therefore
    /// aborts the process on a file it should have refused with an error.
    ///
    /// A sparse file makes the case cheaply — no blocks are written, only the
    /// recorded length changes.
    #[test]
    fn an_oversized_file_is_an_error_for_every_resident_backend() {
        const HUGE: u64 = 1 << 40; // 1 TiB
        let path = temp_file("huge", b"aaa line\n");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(HUGE)
            .ok();
        if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(HUGE) {
            eprintln!(
                "SKIPPED an_oversized_file_is_an_error_for_every_resident_backend: this \
                 filesystem would not report a {HUGE}-byte sparse file, so the input this \
                 test is about cannot be constructed here."
            );
            return;
        }

        for s in [Storage::Resident, Storage::LazyResident, Storage::Indexed] {
            let opened = Source::open(&path, s);
            assert!(
                matches!(
                    opened,
                    Err(Error::FileTooLarge {
                        len: HUGE,
                        limit: MAX_FILE_LEN,
                        ..
                    })
                ),
                "{s:?} accepted a {HUGE}-byte file"
            );
        }

        // `Pread` reads a line at a time and is bounded by nothing but the
        // file, so it is the strategy that still works at this size.
        let pread = Source::open(&path, Storage::Pread).unwrap();
        assert_eq!(pread.len(), HUGE);
        assert_eq!(line(&pread, 0), Some((b"aaa line".to_vec(), 9)));

        std::fs::remove_file(&path).ok();
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
