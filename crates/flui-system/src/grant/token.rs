//! A grant as bytes an application keeps: what it is, and its wire form.

use std::fmt;

/// The kind of system a [`Grant`] was made by, which is the only kind that
/// can resolve it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GrantKind {
    /// A path on a Unix system, as the file system's bytes: what a process
    /// outside a sandbox keeps, and a Flatpak's document portal path, which
    /// the portal keeps for the application across its runs.
    UnixPath,
    /// A path on Windows, as its UTF-16 units: what a desktop process
    /// keeps, packaged or not.
    WindowsPath,
    /// A bookmark Foundation made of the file's URL: security-scoped on
    /// macOS, minimal on iOS.
    AppleBookmark,
    /// An Android content URI whose read permission the application
    /// persisted (`ContentResolver.takePersistableUriPermission`).
    ContentUri,
}

impl GrantKind {
    /// The byte the kind is written as; never reused for another kind.
    const fn tag(self) -> u8 {
        match self {
            Self::UnixPath => 1,
            Self::WindowsPath => 2,
            Self::AppleBookmark => 3,
            Self::ContentUri => 4,
        }
    }

    /// The kind written as `tag`.
    const fn of_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Self::UnixPath,
            2 => Self::WindowsPath,
            3 => Self::AppleBookmark,
            4 => Self::ContentUri,
            _ => return None,
        })
    }
}

/// What made a grant: a kind of system, or the scripted double, whose grants
/// no system's backend resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Origin {
    System(GrantKind),
    Scripted,
}

/// The byte the scripted double's grants are written with, outside every
/// [`GrantKind`]'s.
const SCRIPTED_TAG: u8 = 0xF0;

impl Origin {
    const fn tag(self) -> u8 {
        match self {
            Self::System(kind) => kind.tag(),
            Self::Scripted => SCRIPTED_TAG,
        }
    }

    const fn of_tag(tag: u8) -> Option<Self> {
        if tag == SCRIPTED_TAG {
            return Some(Self::Scripted);
        }
        match GrantKind::of_tag(tag) {
            Some(kind) => Some(Self::System(kind)),
            None => None,
        }
    }
}

/// What the bytes of a grant begin with.
const MAGIC: [u8; 8] = *b"flui.grt";

/// The version of the wire form this library writes and reads.
const VERSION: u8 = 1;

/// The bytes before a grant's payload: the magic, the version, the kind.
const HEADER_LEN: usize = MAGIC.len() + 2;

/// Why bytes are not a [`Grant`] this library reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedGrant {
    /// The bytes do not begin as a grant's do: they were never one, or were
    /// cut short.
    #[error("the bytes are not a grant")]
    NotAGrant,
    /// The bytes are a grant of a wire form this library does not read.
    #[error("the grant is of version {0}, which this library does not read")]
    Version(u8),
    /// The bytes name a kind of grant this library does not know.
    #[error("the grant is of kind {0}, which this library does not know")]
    Kind(u8),
    /// The grant holds nothing, or more than [`Grant::MAX_PAYLOAD`] bytes.
    #[error("the grant's payload is empty or longer than a grant holds")]
    Payload,
}

/// Access to a file or folder the user picked that outlasts the process:
/// what an application keeps (in its settings, beside a document it
/// remembers) to reach the file again after a restart, through
/// [`Grants::resolve`](super::Grants::resolve).
///
/// It is what the system grants, in its own form: a security-scoped
/// bookmark on macOS and iOS, a content URI whose permission the
/// application persisted on Android, a path where the process may open what
/// the user can (a desktop process on Windows or Linux) or where the
/// document portal keeps a Flatpak's grant. [`to_bytes`](Self::to_bytes)
/// writes it for the application to store and
/// [`try_from_bytes`](Self::try_from_bytes) reads it back; only a system of
/// the [kind](Self::kind) that made it resolves it.
///
/// A grant is a key to the user's file, and its payload names the file: an
/// application stores it as it stores the user's other data, and its
/// [`Debug`] shows only its kind and length.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Grant {
    origin: Origin,
    payload: Box<[u8]>,
}

impl Grant {
    /// The longest payload a grant holds, in bytes: a bookmark is a few
    /// kilobytes, a path or a URI less.
    pub const MAX_PAYLOAD: usize = 64 * 1024;

    /// A grant of `kind` whose system-specific form is `payload`, as a
    /// backend makes one; `None` for an empty payload or one longer than
    /// [`MAX_PAYLOAD`](Self::MAX_PAYLOAD).
    #[must_use]
    pub fn try_new(kind: GrantKind, payload: impl Into<Box<[u8]>>) -> Option<Self> {
        Self::of(Origin::System(kind), payload.into())
    }

    fn of(origin: Origin, payload: Box<[u8]>) -> Option<Self> {
        (!payload.is_empty() && payload.len() <= Self::MAX_PAYLOAD)
            .then_some(Self { origin, payload })
    }

    /// A grant the scripted double minted, which no system's backend
    /// resolves.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn scripted(payload: impl Into<Box<[u8]>>) -> Option<Self> {
        Self::of(Origin::Scripted, payload.into())
    }

    /// Whether the scripted double minted it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn is_scripted(&self) -> bool {
        self.origin == Origin::Scripted
    }

    /// Whether `other` was made by the same kind of system.
    pub(crate) fn same_origin(&self, other: &Self) -> bool {
        self.origin == other.origin
    }

    /// The kind of system that made it; `None` for one the scripted double
    /// minted, which no system resolves.
    #[must_use]
    pub fn kind(&self) -> Option<GrantKind> {
        match self.origin {
            Origin::System(kind) => Some(kind),
            Origin::Scripted => None,
        }
    }

    /// Its system-specific form, which only a backend of its
    /// [kind](Self::kind) reads.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The bytes an application stores: a header naming the wire form's
    /// version and the grant's kind, then the payload.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.payload.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(VERSION);
        bytes.push(self.origin.tag());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// The grant `bytes` hold, as [`to_bytes`](Self::to_bytes) wrote it.
    ///
    /// # Errors
    ///
    /// [`MalformedGrant`]: the bytes are not a grant's, are of a wire form
    /// or a kind this library does not read, or hold an empty or oversized
    /// payload.
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, MalformedGrant> {
        let (header, payload) = bytes
            .split_at_checked(HEADER_LEN)
            .ok_or(MalformedGrant::NotAGrant)?;
        let (magic, [version, kind]) = header.split_at(MAGIC.len()) else {
            return Err(MalformedGrant::NotAGrant);
        };
        if magic != MAGIC {
            return Err(MalformedGrant::NotAGrant);
        }
        if *version != VERSION {
            return Err(MalformedGrant::Version(*version));
        }
        let origin = Origin::of_tag(*kind).ok_or(MalformedGrant::Kind(*kind))?;
        // Judged by its length before a byte of it is copied.
        if payload.is_empty() || payload.len() > Self::MAX_PAYLOAD {
            return Err(MalformedGrant::Payload);
        }
        Self::of(origin, payload.into()).ok_or(MalformedGrant::Payload)
    }
}

/// A path as the grant a process that may open it keeps: its bytes on Unix,
/// its UTF-16 units on Windows.
#[cfg(any(unix, windows))]
impl Grant {
    /// The grant of `path`; `None` for a path longer than a grant holds.
    #[cfg(any(windows, target_os = "linux", test))]
    pub(crate) fn of_path(path: &std::path::Path) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            Self::try_new(GrantKind::UnixPath, path.as_os_str().as_bytes())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt as _;
            let payload = path
                .as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>();
            Self::try_new(GrantKind::WindowsPath, payload)
        }
    }

    /// The path a grant of this system's path kind names; `None` for a grant
    /// of another kind, or a Windows payload of an odd length.
    #[cfg(any(windows, target_os = "linux", test))]
    pub(crate) fn path(&self) -> Option<std::path::PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            (self.origin == Origin::System(GrantKind::UnixPath))
                .then(|| std::ffi::OsString::from_vec(self.payload.to_vec()).into())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt as _;
            let (pairs, []) = self.payload.as_chunks::<2>() else {
                return None;
            };
            if self.origin != Origin::System(GrantKind::WindowsPath) {
                return None;
            }
            let units = pairs
                .iter()
                .map(|unit| u16::from_le_bytes(*unit))
                .collect::<Vec<u16>>();
            Some(std::ffi::OsString::from_wide(&units).into())
        }
    }
}

impl fmt::Debug for Grant {
    /// The kind and the length: the payload names the user's file.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("origin", &self.origin)
            .field("len", &self.payload.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Grant, GrantKind, MalformedGrant};

    #[test]
    fn a_grant_reads_back_from_its_bytes_and_nothing_else_reads_as_one() {
        let grant = Grant::try_new(GrantKind::AppleBookmark, &b"book"[..]).expect("a payload");
        let bytes = grant.to_bytes();
        assert_eq!(Grant::try_from_bytes(&bytes), Ok(grant.clone()));
        assert_eq!(
            Grant::try_from_bytes(&bytes[..bytes.len() - 4]),
            Err(MalformedGrant::Payload),
            "a grant of no payload"
        );
        assert_eq!(
            Grant::try_from_bytes(&bytes[..5]),
            Err(MalformedGrant::NotAGrant)
        );
        assert_eq!(
            Grant::try_from_bytes(b"{\"path\":\"/a\"}"),
            Err(MalformedGrant::NotAGrant)
        );
        let mut later = bytes.clone();
        later[8] = 2;
        assert_eq!(
            Grant::try_from_bytes(&later),
            Err(MalformedGrant::Version(2))
        );
        let mut unknown = bytes;
        unknown[9] = 200;
        assert_eq!(
            Grant::try_from_bytes(&unknown),
            Err(MalformedGrant::Kind(200))
        );
        assert_eq!(Grant::try_new(GrantKind::ContentUri, Vec::new()), None);
        assert_eq!(
            Grant::try_new(GrantKind::ContentUri, vec![0; Grant::MAX_PAYLOAD + 1]),
            None
        );
        assert!(
            format!("{grant:?}").contains("len: 4"),
            "the payload stays out of a log"
        );
        assert!(!format!("{grant:?}").contains("book"));
    }

    #[test]
    fn a_scripted_grant_reads_back_and_names_no_kind_of_system() {
        let scripted = Grant::scripted(&b"12345678"[..]).expect("a payload");
        assert_eq!(scripted.kind(), None);
        let read = Grant::try_from_bytes(&scripted.to_bytes()).expect("its own bytes");
        assert!(read.is_scripted(), "it survives a restart as the double's");
        let bookmark =
            Grant::try_new(GrantKind::AppleBookmark, &b"12345678"[..]).expect("a payload");
        assert!(
            !bookmark.same_origin(&scripted),
            "same bytes, another origin"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_path_grant_names_its_path_on_the_system_that_made_it() {
        let path = std::env::temp_dir().join("Notes – é.txt");
        let grant = Grant::of_path(&path).expect("a path fits a grant");
        let read = Grant::try_from_bytes(&grant.to_bytes()).expect("its own bytes");
        assert_eq!(read.path(), Some(path));
        let foreign =
            Grant::try_new(GrantKind::ContentUri, &b"content://x"[..]).expect("a payload");
        assert_eq!(
            foreign.path(),
            None,
            "a grant of another kind names no path"
        );
    }
}
