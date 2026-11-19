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

/// The byte the scripted double's grants are written with, outside every
/// [`GrantKind`]'s, the kind it stands in for following it. Only a build
/// with the double reads it: elsewhere it is a kind this library does not
/// know ([`MalformedGrant::Kind`]).
#[cfg(any(test, feature = "testing"))]
const SCRIPTED_TAG: u8 = 0xF0;

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
    kind: GrantKind,
    /// The scripted double minted it, standing in for a system of `kind`:
    /// no system's backend resolves it.
    #[cfg(any(test, feature = "testing"))]
    scripted: bool,
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
        let payload = payload.into();
        (!payload.is_empty() && payload.len() <= Self::MAX_PAYLOAD).then_some(Self {
            kind,
            #[cfg(any(test, feature = "testing"))]
            scripted: false,
            payload,
        })
    }

    /// A grant the scripted double minted in place of one of `kind`, which
    /// no system's backend resolves.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn scripted(kind: GrantKind, payload: impl Into<Box<[u8]>>) -> Option<Self> {
        Self::try_new(kind, payload).map(|grant| Self {
            scripted: true,
            ..grant
        })
    }

    /// Whether the scripted double minted it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn is_scripted(&self) -> bool {
        self.scripted
    }

    /// Whether a system of `kind` made it: what that system's backend takes,
    /// and never a grant the scripted double minted.
    #[cfg_attr(
        not(any(
            windows,
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "android"
        )),
        expect(dead_code, reason = "a target with no grant backend takes none")
    )]
    pub(crate) fn made_by(&self, kind: GrantKind) -> bool {
        #[cfg(any(test, feature = "testing"))]
        if self.scripted {
            return false;
        }
        self.kind == kind
    }

    /// Whether `other` was made by the same kind of system, or both by the
    /// scripted double.
    pub(crate) fn same_origin(&self, other: &Self) -> bool {
        #[cfg(any(test, feature = "testing"))]
        if self.scripted != other.scripted {
            return false;
        }
        self.kind == other.kind
    }

    /// The kind of system that made it, the only kind that resolves it. In
    /// a build with the scripted double (`test`, the `testing` feature), a
    /// grant the double minted names the kind it stands in for, which no
    /// system's backend resolves.
    #[must_use]
    pub fn kind(&self) -> GrantKind {
        self.kind
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
        let mut bytes = Vec::with_capacity(HEADER_LEN + 1 + self.payload.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.push(VERSION);
        #[cfg(any(test, feature = "testing"))]
        if self.scripted {
            bytes.push(SCRIPTED_TAG);
        }
        bytes.push(self.kind.tag());
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
        #[cfg(any(test, feature = "testing"))]
        if *kind == SCRIPTED_TAG {
            let (stands_for, payload) = payload.split_first().ok_or(MalformedGrant::NotAGrant)?;
            let stands_for =
                GrantKind::of_tag(*stands_for).ok_or(MalformedGrant::Kind(*stands_for))?;
            check_length(payload)?;
            return Self::scripted(stands_for, payload).ok_or(MalformedGrant::Payload);
        }
        let kind = GrantKind::of_tag(*kind).ok_or(MalformedGrant::Kind(*kind))?;
        check_length(payload)?;
        Self::try_new(kind, payload).ok_or(MalformedGrant::Payload)
    }
}

/// A payload judged by its length before a byte of it is copied.
fn check_length(payload: &[u8]) -> Result<(), MalformedGrant> {
    if payload.is_empty() || payload.len() > Grant::MAX_PAYLOAD {
        Err(MalformedGrant::Payload)
    } else {
        Ok(())
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
            self.made_by(GrantKind::UnixPath)
                .then(|| std::ffi::OsString::from_vec(self.payload.to_vec()).into())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt as _;
            let (pairs, []) = self.payload.as_chunks::<2>() else {
                return None;
            };
            if !self.made_by(GrantKind::WindowsPath) {
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
        let mut debug = f.debug_struct("Grant");
        debug.field("kind", &self.kind);
        #[cfg(any(test, feature = "testing"))]
        debug.field("scripted", &self.scripted);
        debug.field("len", &self.payload.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Grant, GrantKind, MalformedGrant, SCRIPTED_TAG};

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
    fn a_scripted_grant_reads_back_as_the_doubles_and_no_system_takes_it() {
        let scripted =
            Grant::scripted(GrantKind::AppleBookmark, &b"12345678"[..]).expect("a payload");
        assert_eq!(
            scripted.kind(),
            GrantKind::AppleBookmark,
            "the kind it stands in for"
        );
        assert!(
            !scripted.made_by(GrantKind::AppleBookmark),
            "no system's backend takes it"
        );
        let read = Grant::try_from_bytes(&scripted.to_bytes()).expect("its own bytes");
        assert!(read.is_scripted(), "it survives a restart as the double's");
        assert_eq!(read, scripted);
        let bookmark =
            Grant::try_new(GrantKind::AppleBookmark, &b"12345678"[..]).expect("a payload");
        assert!(
            !bookmark.same_origin(&scripted),
            "same kind and bytes, another origin"
        );
        assert!(bookmark.made_by(GrantKind::AppleBookmark));
        // A build without the double decodes the tag as a kind, and none is it.
        assert_eq!(GrantKind::of_tag(SCRIPTED_TAG), None);
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
