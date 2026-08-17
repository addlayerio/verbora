//! Pluggable object persistence, ported from the reference `storage` module.
//!
//! # What is ported, and what is not
//!
//! The reference ships five plugins — Postgres, Redis, MongoDB, Memcached and
//! files. Only the **file** backend is implemented here. The other four are
//! network clients whose behaviour cannot be recorded into a golden fixture
//! without standing the servers up, so porting them would mean transcribing
//! assumptions rather than reproducing observed behaviour — exactly what this
//! project's parity methodology forbids. [`StoragePlugin`] is the seam: a caller
//! who needs Redis implements the trait against whichever client they already
//! have, and [`StorageBackend::with_plugin`] installs it.
//!
//! # Faithfully reproduced
//!
//! * The four error messages, byte for byte.
//! * `set_storage_type` assigning the type **before** validating it, so an
//!   invalid type leaves the object permanently un-reconfigurable: the second
//!   call reports "Storage type already set", and `store` then complains about
//!   the missing *client*, not the missing type.
//! * The file layout: one `<uuid>.json` per stored object, containing
//!   `JSON.stringify(object)` and nothing else.
//!
//! # Deliberately not reproduced
//!
//! The reference's constructor calls the `async` `set_storage_type` without
//! awaiting it, so a Postgres backend can still be connecting when `store` is
//! called and a constructor failure becomes an unhandled promise rejection.
//! [`StorageBackend::with_type`] is synchronous and simply keeps the error, which
//! [`StorageBackend::configuration_error`] exposes. There is no way to reproduce
//! a dangling promise in a synchronous API, and pretending otherwise would be
//! worse than saying so.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The storage kinds `setStorageType` switches on: `STORAGE_TYPES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageType {
    /// `POSTGRES` — not implemented here; see the module note.
    Postgres,
    /// `REDIS` — not implemented here.
    Redis,
    /// `MONGODB` — not implemented here.
    MongoDb,
    /// `MEMCACHED` — not implemented here.
    Memcached,
    /// `FILE` — implemented by [`FileBackend`].
    File,
}

/// The five names, in the reference's declaration order.
pub static STORAGE_TYPES: &[StorageType] = &[
    StorageType::Postgres,
    StorageType::Redis,
    StorageType::MongoDb,
    StorageType::Memcached,
    StorageType::File,
];

impl StorageType {
    /// The string the reference uses as both key and value in `STORAGE_TYPES`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "POSTGRES",
            Self::Redis => "REDIS",
            Self::MongoDb => "MONGODB",
            Self::Memcached => "MEMCACHED",
            Self::File => "FILE",
        }
    }

    /// Parses a type name, case-sensitively, as the reference's `switch` does.
    pub fn parse(name: &str) -> Option<Self> {
        STORAGE_TYPES.iter().copied().find(|t| t.as_str() == name)
    }
}

impl fmt::Display for StorageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything [`StorageBackend`] can fail with.
#[derive(Debug)]
pub enum StorageError {
    /// `new Error('No storage type specified')` — an empty type name.
    NoStorageType,
    /// `new Error('Storage type already set')`.
    AlreadySet,
    /// `new Error('Invalid storage type')` — an unrecognised name.
    InvalidStorageType,
    /// `new Error('Storage type or client not set')`.
    NotConfigured,
    /// A storage type whose plugin this crate does not ship; install one with
    /// [`StorageBackend::with_plugin`].
    Unsupported(StorageType),
    /// The underlying store failed.
    Io(std::io::Error),
    /// The stored bytes were not the JSON they claimed to be.
    Json(serde_json::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // These four are byte-for-byte the reference's messages.
            Self::NoStorageType => f.write_str("No storage type specified"),
            Self::AlreadySet => f.write_str("Storage type already set"),
            Self::InvalidStorageType => f.write_str("Invalid storage type"),
            Self::NotConfigured => f.write_str("Storage type or client not set"),
            Self::Unsupported(t) => write!(f, "No built-in plugin for storage type {t}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// A place objects can be stored and read back by key.
///
/// The contract mirrors the reference's plugins: JSON in and JSON out — the
/// backend does the encoding — and `store` returns the key it minted.
pub trait StoragePlugin: Send + Sync {
    /// Stores `object`, returning the key it can be retrieved with.
    ///
    /// # Errors
    ///
    /// Whatever the underlying store reports.
    fn store(&self, object: &serde_json::Value) -> Result<String, StorageError>;

    /// Retrieves the object stored under `key`.
    ///
    /// # Errors
    ///
    /// A missing key is an error, not `None` — the reference lets the underlying
    /// `ENOENT` or cache miss propagate.
    fn retrieve(&self, key: &str) -> Result<serde_json::Value, StorageError>;
}

/// File-backed storage: one `<uuid>.json` per object.
///
/// The reference reads its directory from `process.env.FS_PATH` **at module load
/// time**, so changing the variable later has no effect. [`FileBackend::new`]
/// takes the directory instead, which makes the same behaviour available without
/// the global; [`FileBackend::from_env`] reads `FS_PATH` for callers porting an
/// existing deployment.
#[derive(Debug, Clone)]
pub struct FileBackend {
    directory: PathBuf,
}

impl FileBackend {
    /// Creates a backend writing into `directory`.
    ///
    /// The directory is not created; the reference does not create it either,
    /// and a missing one surfaces as an I/O error on the first `store`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Creates a backend from the `FS_PATH` environment variable.
    ///
    /// # Errors
    ///
    /// [`StorageError::Io`] with [`std::io::ErrorKind::NotFound`] when `FS_PATH`
    /// is unset. The reference instead builds the path `"undefined/<key>.json"`
    /// and fails on the write, which is the same outcome reported later and less
    /// clearly.
    pub fn from_env() -> Result<Self, StorageError> {
        let path = std::env::var("FS_PATH").map_err(|_| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "FS_PATH is not set",
            ))
        })?;
        Ok(Self::new(path))
    }

    /// The directory objects are written to.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The file a key maps to: `<directory>/<key>.json`.
    pub fn path_for(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.json"))
    }
}

impl StoragePlugin for FileBackend {
    fn store(&self, object: &serde_json::Value) -> Result<String, StorageError> {
        let key = uuid_v4();
        let data = serde_json::to_string(object)?;
        std::fs::write(self.path_for(&key), data)?;
        Ok(key)
    }

    fn retrieve(&self, key: &str) -> Result<serde_json::Value, StorageError> {
        let data = std::fs::read_to_string(self.path_for(key))?;
        Ok(serde_json::from_str(&data)?)
    }
}

/// The reference's `StorageBackend`: a storage type plus the plugin it selected.
///
/// ```
/// use verbora_util::storage::{FileBackend, StorageBackend, StorageType};
///
/// let dir = std::env::temp_dir().join("verbora-util-doctest");
/// std::fs::create_dir_all(&dir).unwrap();
///
/// let backend = StorageBackend::with_plugin(StorageType::File, FileBackend::new(&dir));
/// let key = backend.store(&serde_json::json!({ "hello": "world" })).unwrap();
/// assert_eq!(backend.retrieve(&key).unwrap()["hello"], "world");
/// ```
pub struct StorageBackend {
    storage_type: Option<String>,
    client: Option<Box<dyn StoragePlugin>>,
    configuration_error: Option<StorageError>,
}

impl fmt::Debug for StorageBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageBackend")
            .field("storage_type", &self.storage_type)
            .field("has_client", &self.client.is_some())
            .finish()
    }
}

impl Default for StorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend {
    /// Creates an unconfigured backend. `store` and `retrieve` fail until a type
    /// is set.
    pub fn new() -> Self {
        Self {
            storage_type: None,
            client: None,
            configuration_error: None,
        }
    }

    /// Creates a backend configured for `storage_type`, keeping any error.
    ///
    /// Mirrors `new StorageBackend(type)`, which calls the async
    /// `setStorageType` and never awaits it: the constructor cannot fail, and a
    /// bad type leaves the object with its `storageType` set and no client. Read
    /// [`StorageBackend::configuration_error`] to see what went wrong — the
    /// reference makes you catch an unhandled rejection instead.
    pub fn with_type(storage_type: &str) -> Self {
        let mut backend = Self::new();
        backend.configuration_error = backend.set_storage_type(storage_type).err();
        backend
    }

    /// As [`StorageBackend::with_type`], but resolving `FILE` against
    /// `file_root` instead of the `FS_PATH` environment variable.
    pub fn with_type_in(storage_type: &str, file_root: &Path) -> Self {
        let mut backend = Self::new();
        backend.configuration_error = backend.set_storage_type_in(storage_type, file_root).err();
        backend
    }

    /// Creates a backend around a caller-supplied plugin.
    ///
    /// This is the extension point the reference lacks. It is how a Redis or
    /// Postgres backend gets attached without this crate taking a dependency on
    /// a database driver.
    pub fn with_plugin(storage_type: StorageType, plugin: impl StoragePlugin + 'static) -> Self {
        Self {
            storage_type: Some(storage_type.as_str().to_owned()),
            client: Some(Box::new(plugin)),
            configuration_error: None,
        }
    }

    /// Selects a storage type by name and builds its plugin.
    ///
    /// # Errors
    ///
    /// * [`StorageError::NoStorageType`] for an empty name — the reference's
    ///   falsiness check, which also catches `0`, `null` and `false`.
    /// * [`StorageError::AlreadySet`] if a type is already set.
    /// * [`StorageError::InvalidStorageType`] for an unknown name. **The type is
    ///   recorded before this check**, exactly as in the reference, so the
    ///   backend is stuck afterwards.
    /// * [`StorageError::Unsupported`] for a valid type with no built-in plugin;
    ///   use [`StorageBackend::with_plugin`]. The type is likewise already
    ///   recorded.
    pub fn set_storage_type(&mut self, storage_type: &str) -> Result<(), StorageError> {
        self.select(storage_type, None)
    }

    /// As [`StorageBackend::set_storage_type`], but resolving `FILE` against
    /// `file_root` instead of the `FS_PATH` environment variable.
    ///
    /// The reference has no equivalent — it reads `process.env.FS_PATH` once, at
    /// module load, and there is no way to override it afterwards. This is the
    /// same code path with the directory passed in, which is what makes the
    /// backend usable from a test or from two configurations in one process.
    ///
    /// # Errors
    ///
    /// As [`StorageBackend::set_storage_type`].
    pub fn set_storage_type_in(
        &mut self,
        storage_type: &str,
        file_root: &Path,
    ) -> Result<(), StorageError> {
        self.select(storage_type, Some(file_root))
    }

    fn select(&mut self, storage_type: &str, file_root: Option<&Path>) -> Result<(), StorageError> {
        if storage_type.is_empty() {
            return Err(StorageError::NoStorageType);
        }
        if self.storage_type.is_some() {
            return Err(StorageError::AlreadySet);
        }
        // Assigned first, validated second. Reproducing the order is the whole
        // point: it is what makes an invalid type unrecoverable.
        self.storage_type = Some(storage_type.to_owned());
        match StorageType::parse(storage_type) {
            Some(StorageType::File) => {
                let backend = match file_root {
                    Some(root) => FileBackend::new(root),
                    None => FileBackend::from_env()?,
                };
                self.client = Some(Box::new(backend));
                Ok(())
            }
            Some(other) => Err(StorageError::Unsupported(other)),
            None => Err(StorageError::InvalidStorageType),
        }
    }

    /// The configured type name, if any.
    pub fn storage_type(&self) -> Option<&str> {
        self.storage_type.as_deref()
    }

    /// Whether a plugin is installed.
    pub fn has_client(&self) -> bool {
        self.client.is_some()
    }

    /// The error [`StorageBackend::with_type`] swallowed, if it swallowed one.
    pub fn configuration_error(&self) -> Option<&StorageError> {
        self.configuration_error.as_ref()
    }

    /// Stores an object, returning its key.
    ///
    /// # Errors
    ///
    /// [`StorageError::NotConfigured`] when either the type or the client is
    /// missing — the reference's `if (this.storageType && this.client)` — or
    /// whatever the plugin reports.
    pub fn store(&self, object: &serde_json::Value) -> Result<String, StorageError> {
        match (&self.storage_type, &self.client) {
            (Some(_), Some(client)) => client.store(object),
            _ => Err(StorageError::NotConfigured),
        }
    }

    /// Retrieves the object stored under `key`.
    ///
    /// # Errors
    ///
    /// As [`StorageBackend::store`].
    pub fn retrieve(&self, key: &str) -> Result<serde_json::Value, StorageError> {
        match (&self.storage_type, &self.client) {
            (Some(_), Some(client)) => client.retrieve(key),
            _ => Err(StorageError::NotConfigured),
        }
    }
}

/// A counter making two keys minted in the same nanosecond distinct.
static UUID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates a version-4-shaped UUID.
///
/// # Not cryptographically random
///
/// The workspace takes no `uuid` or `getrandom` dependency, so the 122 free bits
/// come from hashing a per-process [`RandomState`] seed (which std itself seeds
/// from the OS), the wall clock and a monotonic counter. That is ample for
/// "distinct filenames in a directory", which is all the reference uses the key
/// for, and it is **not** suitable as a security token. Say so at any call site
/// that might be tempted.
fn uuid_v4() -> String {
    let state = RandomState::new();
    let n = UUID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);

    let mut hi_hasher = state.build_hasher();
    hi_hasher.write_u64(n);
    hi_hasher.write_u64(nanos);
    let hi = hi_hasher.finish();

    let mut lo_hasher = state.build_hasher();
    lo_hasher.write_u64(hi);
    lo_hasher.write_u64(n.rotate_left(32));
    let lo = lo_hasher.finish();

    // Version 4 in the high nibble of octet 6, variant 10 in octet 8.
    let hi = (hi & 0xffff_ffff_ffff_0fff) | 0x0000_0000_0000_4000;
    let lo = (lo & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (hi >> 32) as u32,
        ((hi >> 16) & 0xffff) as u16,
        (hi & 0xffff) as u16,
        ((lo >> 48) & 0xffff) as u16,
        lo & 0x0000_ffff_ffff_ffff,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("verbora-util-storage-{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn error_messages_match_the_reference() {
        assert_eq!(
            StorageError::NoStorageType.to_string(),
            "No storage type specified"
        );
        assert_eq!(
            StorageError::AlreadySet.to_string(),
            "Storage type already set"
        );
        assert_eq!(
            StorageError::InvalidStorageType.to_string(),
            "Invalid storage type"
        );
        assert_eq!(
            StorageError::NotConfigured.to_string(),
            "Storage type or client not set"
        );
    }

    #[test]
    fn unconfigured_backend_refuses_everything() {
        let b = StorageBackend::new();
        assert!(b.storage_type().is_none());
        assert!(!b.has_client());
        assert!(matches!(
            b.store(&json!({"a": 1})),
            Err(StorageError::NotConfigured)
        ));
        assert!(matches!(b.retrieve("k"), Err(StorageError::NotConfigured)));
    }

    #[test]
    fn an_invalid_type_is_recorded_before_it_is_rejected() {
        // A naive port would validate first and leave `storage_type` unset,
        // which would make the object recoverable. It is not.
        let mut b = StorageBackend::new();
        assert!(matches!(
            b.set_storage_type("BOGUS"),
            Err(StorageError::InvalidStorageType)
        ));
        assert_eq!(b.storage_type(), Some("BOGUS"));
        assert!(matches!(
            b.set_storage_type("FILE"),
            Err(StorageError::AlreadySet)
        ));
        assert!(matches!(
            b.store(&json!(1)),
            Err(StorageError::NotConfigured)
        ));
    }

    #[test]
    fn empty_type_name_is_the_falsy_case() {
        let mut b = StorageBackend::new();
        assert!(matches!(
            b.set_storage_type(""),
            Err(StorageError::NoStorageType)
        ));
        // Nothing was recorded, so the backend is still usable.
        assert!(b.storage_type().is_none());
    }

    #[test]
    fn storage_type_names() {
        assert_eq!(StorageType::MongoDb.as_str(), "MONGODB");
        assert_eq!(StorageType::parse("FILE"), Some(StorageType::File));
        assert_eq!(StorageType::parse("file"), None);
        assert_eq!(STORAGE_TYPES.len(), 5);
    }

    #[test]
    fn file_round_trip() {
        let dir = temp_dir("round-trip");
        let backend = StorageBackend::with_plugin(StorageType::File, FileBackend::new(&dir));
        for value in [
            json!(null),
            json!(true),
            json!(0),
            json!(-17.5),
            json!(""),
            json!("café Москва 日本語 😀"),
            json!([]),
            json!([1, [2, [3]]]),
            json!({}),
            json!({"a": 1, "b": "two", "c": [3]}),
        ] {
            let key = backend.store(&value).unwrap();
            assert_eq!(key.len(), 36);
            assert_eq!(backend.retrieve(&key).unwrap(), value);
        }
    }

    #[test]
    fn on_disk_bytes_are_json_stringify() {
        let dir = temp_dir("bytes");
        let plugin = FileBackend::new(&dir);
        let key = plugin.store(&json!({"a": 1, "b": [true, null]})).unwrap();
        let raw = std::fs::read_to_string(plugin.path_for(&key)).unwrap();
        assert_eq!(raw, r#"{"a":1,"b":[true,null]}"#);
    }

    #[test]
    fn missing_and_corrupt_keys() {
        let dir = temp_dir("missing");
        let plugin = FileBackend::new(&dir);
        match plugin.retrieve("00000000-0000-4000-8000-000000000000") {
            Err(StorageError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("expected NotFound, got {other:?}"),
        }

        let key = "11111111-1111-4111-8111-111111111111";
        std::fs::write(plugin.path_for(key), "not json").unwrap();
        assert!(matches!(plugin.retrieve(key), Err(StorageError::Json(_))));
    }

    #[test]
    fn keys_are_distinct_and_v4_shaped() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let key = uuid_v4();
            assert_eq!(key.len(), 36);
            let parts: Vec<&str> = key.split('-').collect();
            assert_eq!(
                parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
                vec![8, 4, 4, 4, 12]
            );
            assert!(key.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
            assert!(key.as_bytes()[14] == b'4', "version nibble: {key}");
            assert!(
                matches!(key.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
                "variant nibble: {key}"
            );
            assert!(seen.insert(key), "duplicate key");
        }
    }
}
