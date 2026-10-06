//! The byte-storage capability: named values that outlive the process.
//!
//! A [`Storage`] keeps one byte value per [`StorageName`]. Reading is
//! asynchronous; publishing is accepted synchronously and written later, so a
//! frame never waits on a disk. Only bytes, versions and errors cross the
//! boundary to the thread that does the IO, which is why every future here is
//! `Send` while the documents built on it stay on the owner thread.
//!
//! Two names with the same text but different scopes are different values:
//! [`StorageName::from_static`] names data that follows the user between
//! machines, [`StorageName::machine_local`] names state that belongs to this
//! machine and this running instance (a session).

use std::future::Future;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::pin::Pin;

/// A store of named byte values that outlive the process.
///
/// Implementations answer on their own threads: the futures they return are
/// `Send` and may complete on whatever thread finished the IO. A caller on
/// the owner thread polls them inside a frame and never blocks on them.
pub trait Storage: Send + Sync + 'static {
    /// Read the value stored under `name`.
    ///
    /// A missing value is `Ok` with [`Stored::bytes`] `None` and
    /// [`StoredVersion::ABSENT`]. A value longer than `limit` bytes is refused
    /// with [`StorageError::TooLarge`] without being read whole.
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored>;

    /// Accept `bytes` as the latest value of `name` synchronously, before
    /// returning; the returned future resolves once that value is written.
    ///
    /// A previous value of the same name that was accepted but not yet
    /// started is replaced, and its future resolves to
    /// [`StorageError::Superseded`]. Under [`WriteMode::IfUnchanged`] the
    /// write is refused with [`StorageError::Conflict`] when the stored value
    /// is no longer the one the caller based its edit on.
    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: WriteMode,
    ) -> StorageFuture<StoredVersion>;
}

/// What a [`Storage`] request resolves to: a `Send` future of `T` or a
/// [`StorageError`].
pub type StorageFuture<T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'static>>;

/// Where a named value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Scope {
    /// Data that follows the user between machines.
    Roaming,
    /// State that belongs to this machine and this running instance.
    MachineLocal,
}

/// The name of a stored value: lower-case ASCII letters, digits, `_` and
/// `-`, starting with a letter or digit, at most 64 bytes, and never a name
/// Windows reserves for a device (`con`, `prn`, `aux`, `nul`, `com0`–`com9`,
/// `lpt0`–`lpt9`).
///
/// The name is checked when it is made, and both constructors are `const`, so
/// a bad name in a constant is a compile error rather than a file that cannot
/// be written on some platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StorageName {
    name: &'static str,
    scope: Scope,
}

impl StorageName {
    /// The name of data that follows the user between machines: documents,
    /// settings.
    ///
    /// ```
    /// use flui_platform_api::StorageName;
    ///
    /// const NOTES: StorageName = StorageName::from_static("notes");
    /// assert_eq!(NOTES.as_str(), "notes");
    /// ```
    ///
    /// Upper-case letters are refused:
    ///
    /// ```compile_fail,E0080
    /// use flui_platform_api::StorageName;
    ///
    /// const NOTES: StorageName = StorageName::from_static("Notes");
    /// ```
    ///
    /// A name Windows reserves for a device is refused:
    ///
    /// ```compile_fail,E0080
    /// use flui_platform_api::StorageName;
    ///
    /// const CONSOLE: StorageName = StorageName::from_static("con");
    /// ```
    ///
    /// The empty name is refused:
    ///
    /// ```compile_fail,E0080
    /// use flui_platform_api::StorageName;
    ///
    /// const EMPTY: StorageName = StorageName::from_static("");
    /// ```
    ///
    /// A name longer than 64 bytes is refused:
    ///
    /// ```compile_fail,E0080
    /// use flui_platform_api::StorageName;
    ///
    /// const LONG: StorageName = StorageName::from_static(
    ///     "a1234567890123456789012345678901234567890123456789012345678901234",
    /// );
    /// ```
    ///
    /// # Panics
    ///
    /// If `name` breaks the rules above; in a `const` that is a compile error.
    #[must_use]
    pub const fn from_static(name: &'static str) -> Self {
        Self::checked(name, Scope::Roaming)
    }

    /// The name of state that belongs to this machine and this running
    /// instance, such as a session (open screen, scroll position, draft): it
    /// is kept apart from [`from_static`](Self::from_static) data, so it is
    /// not carried to another machine with it.
    ///
    /// # Panics
    ///
    /// If `name` breaks the rules on [`StorageName`]; in a `const` that is a
    /// compile error.
    #[must_use]
    pub const fn machine_local(name: &'static str) -> Self {
        Self::checked(name, Scope::MachineLocal)
    }

    /// The name's text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.name
    }

    const fn checked(name: &'static str, scope: Scope) -> Self {
        if let Err(reason) = validate(name.as_bytes()) {
            panic!("{}", reason);
        }
        Self { name, scope }
    }
}

/// The longest name, in bytes.
const MAX_NAME_LEN: usize = 64;

/// Why `name` is not a valid [`StorageName`], if it is not.
const fn validate(name: &[u8]) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("a storage name must not be empty");
    }
    if name.len() > MAX_NAME_LEN {
        return Err("a storage name must be at most 64 bytes");
    }
    if !matches!(name[0], b'a'..=b'z' | b'0'..=b'9') {
        return Err("a storage name must start with a lower-case ASCII letter or a digit");
    }
    let mut index = 1;
    while index < name.len() {
        if !matches!(name[index], b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') {
            return Err(
                "a storage name may hold only lower-case ASCII letters, digits, `_` and `-`",
            );
        }
        index += 1;
    }
    if is_reserved_device(name) {
        return Err("a storage name must not be a name Windows reserves for a device");
    }
    Ok(())
}

/// Whether `name` is a device name Windows reserves in every directory.
const fn is_reserved_device(name: &[u8]) -> bool {
    match name {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// Which stored value an edit was based on.
///
/// Equal bytes have equal versions. A version is compared only within the
/// running process: it is not a format and is never written anywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoredVersion(Option<(u64, u64)>);

impl StoredVersion {
    /// The version of a name that holds no value.
    pub const ABSENT: Self = Self(None);

    /// The version of a value holding `bytes`. A [`Storage`] reports this
    /// for what it read or wrote.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let len = u64::try_from(bytes.len()).expect("BUG: a byte length fits in u64");
        Self(Some((len, hasher.finish())))
    }
}

/// What [`Storage::read`] found under a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The value, or `None` when the name holds none.
    pub bytes: Option<Vec<u8>>,
    /// The version of [`Self::bytes`]: [`StoredVersion::ABSENT`] for none.
    pub version: StoredVersion,
}

/// How [`Storage::publish`] treats the value already stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteMode {
    /// Replace whatever is stored: the last writer wins.
    Replace,
    /// Write only while the stored value is still this version; otherwise
    /// refuse with [`StorageError::Conflict`].
    IfUnchanged(StoredVersion),
}

/// Why a [`Storage`] request failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// This platform keeps no files, or no storage was configured.
    #[error("storage is unavailable")]
    Unavailable,
    /// The value or its directory cannot be reached: no permission, a
    /// directory in the file's place, a removed medium.
    #[error("the stored value is inaccessible: {kind}")]
    Inaccessible {
        /// What the operating system reported.
        kind: std::io::ErrorKind,
    },
    /// Another process holds the value; trying again later may succeed.
    #[error("the stored value is in use by another process")]
    Busy,
    /// The disk is full.
    #[error("the disk is full")]
    Full,
    /// The value is longer than the caller's limit; it was not read whole.
    #[error("the stored value is {len} bytes, over the {limit}-byte limit")]
    TooLarge {
        /// The value's length in bytes.
        len: u64,
        /// The limit the caller set.
        limit: u64,
    },
    /// The stored value changed since the version the write was based on.
    #[error("the stored value changed since it was read")]
    Conflict,
    /// The storage directory cannot be locked, so a write based on a version
    /// cannot be made safely there.
    #[error("the storage directory does not support file locks")]
    LockUnsupported,
    /// A newer value of the same name replaced this one before it was
    /// written.
    #[error("a newer value replaced this one before it was written")]
    Superseded,
    /// The request was dropped before it completed.
    #[error("the storage request was cancelled")]
    Cancelled,
}
