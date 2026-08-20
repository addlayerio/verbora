//! A saved line-start table, so [`Storage::Indexed`] need not rescan at startup.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::source::{Source, Storage, build_line_starts};

/// File magic: eight bytes, so a truncated or unrelated file is rejected
/// outright rather than misread.
const MAGIC: &[u8; 8] = b"NRSWNIX\x01";

/// Format version. Bumped on any layout change; an older or newer file is
/// refused, never reinterpreted.
const VERSION: u32 = 1;

/// The eight files a dictionary consists of, in a fixed order so that the
/// serialised bytes are reproducible.
const FILES: [&str; 8] = [
    "index.noun",
    "index.verb",
    "index.adj",
    "index.adv",
    "data.noun",
    "data.verb",
    "data.adj",
    "data.adv",
];

/// A saved table of line-start offsets for each of a dictionary's eight files.
///
/// # What it is, and what it is not
///
/// The dictionary text files remain the **only** source of truth. This sidecar
/// caches one derived fact about them — the byte offset of every line — so that
/// opening a dictionary with [`Storage::Indexed`] does not have to scan tens of
/// megabytes for newlines first. It holds no lemmas, no glosses and no synset
/// offsets, so it cannot make a lookup answer differently: the worst it can be
/// is stale, and staleness is detected rather than trusted.
///
/// Every entry records the length of the file it was built from, and opening a
/// dictionary against the sidecar refuses any entry whose file has since
/// changed size — with [`Error::Prebuilt`], naming the file and both lengths.
///
/// # Reproducibility
///
/// The builder is a pure function of the eight files: same dictionary, same
/// bytes out, on any machine. Nothing is timestamped and nothing is
/// platform-dependent — every integer is little-endian.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use verbora_wordnet::{Config, PrebuiltIndex, WordNet};
///
/// // Build once…
/// PrebuiltIndex::build("path/to/dict")?.save("wordnet.vbwnix")?;
/// // …reuse on every subsequent start.
/// let wn = WordNet::open_with(
///     "path/to/dict",
///     &Config::default().with_prebuilt("wordnet.vbwnix"),
/// )?;
/// # let _ = wn;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct PrebuiltIndex {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    /// File name only, e.g. `index.noun` — the sidecar is relocatable.
    name: String,
    /// Length of the source file when the entry was built.
    file_len: u64,
    /// Byte offset of the start of each line.
    line_starts: Box<[u32]>,
}

impl PrebuiltIndex {
    /// Scans every dictionary file in `dict_dir` and records its line starts.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a file cannot be read, or [`Error::FileTooLarge`] if
    /// one is too large for `u32` offsets.
    pub fn build(dict_dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dict_dir.as_ref();
        let mut entries = Vec::with_capacity(FILES.len());
        for name in FILES {
            let path = dir.join(name);
            let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
            entries.push(Entry {
                name: (*name).to_owned(),
                file_len: bytes.len() as u64,
                line_starts: build_line_starts(&path, &bytes)?,
            });
        }
        Ok(Self { entries })
    }

    /// Serialises to the on-disk format.
    ///
    /// Layout, all integers little-endian:
    ///
    /// ```text
    /// magic  : 8 bytes  "NRSWNIX\x01"
    /// version: u32
    /// count  : u32
    /// entries: count x {
    ///     name_len : u16
    ///     name     : name_len bytes, UTF-8
    ///     file_len : u64
    ///     lines    : u32
    ///     starts   : lines x u32
    /// }
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.byte_size());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            out.extend_from_slice(e.name.as_bytes());
            out.extend_from_slice(&e.file_len.to_le_bytes());
            out.extend_from_slice(&(e.line_starts.len() as u32).to_le_bytes());
            for s in &*e.line_starts {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        out
    }

    /// Parses the on-disk format.
    ///
    /// `path` is used only to name the file in an error.
    ///
    /// # Errors
    ///
    /// [`Error::Prebuilt`] if the magic, version or framing is wrong.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, at: 0, path };
        if r.take(8)? != MAGIC {
            return Err(Error::prebuilt(path, "not a verbora WordNet line index"));
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(Error::prebuilt(
                path,
                format!("format version {version}, expected {VERSION}; rebuild the index"),
            ));
        }
        let count = r.u32()? as usize;
        let mut entries = Vec::with_capacity(count.min(FILES.len() * 4));
        for _ in 0..count {
            let name_len = r.u16()? as usize;
            let name = std::str::from_utf8(r.take(name_len)?)
                .map_err(|e| Error::prebuilt(path, format!("entry name is not UTF-8: {e}")))?
                .to_owned();
            let file_len = r.u64()?;
            let lines = r.u32()? as usize;
            let raw = r.take(
                lines
                    .checked_mul(4)
                    .ok_or_else(|| Error::prebuilt(path, "line count overflows"))?,
            )?;
            let line_starts = raw
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            entries.push(Entry {
                name,
                file_len,
                line_starts,
            });
        }
        Ok(Self { entries })
    }

    /// Writes the index to `path`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be written.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, self.to_bytes()).map_err(|e| Error::io(path, e))
    }

    /// Reads an index from `path`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] or [`Error::Prebuilt`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        Self::from_bytes(path, &bytes)
    }

    /// Opens `path` as a resident source, reusing the stored line starts.
    ///
    /// # Errors
    ///
    /// [`Error::Prebuilt`] if the sidecar has no entry for `name` or the file
    /// has changed size since the index was built; [`Error::Io`] if it cannot
    /// be read.
    pub(crate) fn source_for(&self, name: &str, path: &Path) -> Result<Source> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| Error::prebuilt(path, format!("the index has no entry for {name}")))?;
        let actual = std::fs::metadata(path)
            .map_err(|e| Error::io(path, e))?
            .len();
        if actual != entry.file_len {
            return Err(Error::prebuilt(
                path,
                format!(
                    "the file is {actual} bytes but the index was built against {}; \
                     the dictionary files are the source of truth — rebuild the index",
                    entry.file_len
                ),
            ));
        }
        Source::open_with_line_starts(path, entry.line_starts.clone())
    }

    /// The recorded file names, in serialisation order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// Total number of recorded line starts, across all files.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.entries.iter().map(|e| e.line_starts.len()).sum()
    }

    /// The serialised size in bytes, without serialising.
    #[must_use]
    pub fn byte_size(&self) -> usize {
        16 + self
            .entries
            .iter()
            .map(|e| 2 + e.name.len() + 8 + 4 + 4 * e.line_starts.len())
            .sum::<usize>()
    }

    /// The conventional sidecar path for a dictionary directory.
    #[must_use]
    pub fn default_path(dict_dir: impl AsRef<Path>) -> PathBuf {
        dict_dir.as_ref().join("wordnet.vbwnix")
    }

    /// The storage strategy a sidecar implies.
    ///
    /// A prebuilt index is only meaningful for [`Storage::Indexed`]; this
    /// spells that out so [`Config::with_prebuilt`](crate::Config::with_prebuilt)
    /// does not have to encode it twice.
    pub(crate) const STORAGE: Storage = Storage::Indexed;
}

/// Bounds-checked cursor over the serialised bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    path: &'a Path,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| Error::prebuilt(self.path, "the index is truncated"))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PrebuiltIndex {
        PrebuiltIndex {
            entries: vec![
                Entry {
                    name: "index.noun".into(),
                    file_len: 27,
                    line_starts: vec![0, 9, 18].into_boxed_slice(),
                },
                Entry {
                    name: "data.noun".into(),
                    file_len: 10,
                    line_starts: vec![0].into_boxed_slice(),
                },
            ],
        }
    }

    fn here() -> &'static Path {
        Path::new("sidecar.vbwnix")
    }

    #[test]
    fn round_trips_through_bytes() {
        let a = sample();
        let bytes = a.to_bytes();
        assert_eq!(bytes.len(), a.byte_size());
        let b = PrebuiltIndex::from_bytes(here(), &bytes).unwrap();
        assert_eq!(b.names(), ["index.noun", "data.noun"]);
        assert_eq!(b.line_count(), 4);
        // Reproducible: serialising the parsed copy gives identical bytes.
        assert_eq!(b.to_bytes(), bytes);
    }

    /// Every truncation point is rejected, not just a representative one: a
    /// sidecar cut anywhere must be refused rather than half-read.
    #[test]
    fn every_truncation_of_a_valid_index_is_refused() {
        let good = sample().to_bytes();
        for cut in 0..good.len() {
            assert!(
                PrebuiltIndex::from_bytes(here(), &good[..cut]).is_err(),
                "truncated to {cut} bytes"
            );
        }
        assert!(PrebuiltIndex::from_bytes(here(), &good).is_ok());
    }

    #[test]
    fn rejects_foreign_files() {
        for foreign in [
            &b"not an index at all"[..],
            &b""[..],
            &b"NRSWNIX\x02\x01\x00\x00\x00\x00\x00\x00\x00"[..],
        ] {
            assert!(
                matches!(
                    PrebuiltIndex::from_bytes(here(), foreign),
                    Err(Error::Prebuilt { .. })
                ),
                "{foreign:?}"
            );
        }
    }

    #[test]
    fn rejects_a_future_version() {
        let mut bytes = sample().to_bytes();
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        let err = PrebuiltIndex::from_bytes(here(), &bytes).unwrap_err();
        assert!(err.to_string().contains("rebuild"), "{err}");
    }

    #[test]
    fn refuses_an_entry_whose_file_changed_size() {
        let dir = std::env::temp_dir().join(format!("verbora-wordnet-pb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.noun");
        std::fs::write(&path, b"aaa line\nbbb line\nccc line\n").unwrap();
        let pb = sample();
        assert!(pb.source_for("index.noun", &path).is_ok());

        std::fs::write(&path, b"shorter\n").unwrap();
        let err = pb.source_for("index.noun", &path).unwrap_err();
        assert!(err.to_string().contains("source of truth"), "{err}");
        assert!(pb.source_for("index.zzz", &path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_default_sidecar_path_sits_beside_the_dictionary() {
        assert_eq!(
            PrebuiltIndex::default_path("/d/dict"),
            PathBuf::from("/d/dict/wordnet.vbwnix")
        );
    }
}
