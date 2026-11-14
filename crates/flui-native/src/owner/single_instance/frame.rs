//! The frame a later Windows process writes to the first instance's pipe:
//! what its launch opens, its links as text and its files as the absolute
//! paths its launch resolved against its own working directory (the first
//! does not share it), in UTF-16 as Windows names them.
//!
//! On the wire a frame is its body's length (`u32`, little-endian) and the
//! body: the version, then each item as its kind, its length in UTF-16 units
//! (`u32`) and the units (little-endian); the first instance answers with one
//! byte, taken or refused. The body is read as untrusted input: its
//! length is checked before it is read, and a frame past any bound
//! [`Opened`](flui_platform::Opened) keeps, or malformed, is refused whole,
//! so its process runs as an instance of its own.

use flui_platform::{MAX_OPENED_ITEMS, MAX_OPENED_URL_LEN};

/// The longest body the first instance reads.
pub(crate) const MAX_FRAME_LEN: usize = 1 << 20;
/// The longest path a launch forwards, in UTF-16 units: the longest Windows
/// names a file by.
pub(crate) const MAX_PATH_UNITS: usize = 32_767;

/// The frame's version, its body's first byte.
const VERSION: u8 = 1;
/// An item's kind: a link.
const LINK: u8 = 0;
/// An item's kind: a file.
const FILE: u8 = 1;
/// The bytes before an item's units: its kind and its length.
const ITEM_HEADER: usize = 1 + 4;

/// What a later process hands the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Forwarded {
    /// Links as they were given.
    pub(crate) links: Vec<String>,
    /// Files as absolute paths, in UTF-16 units (a Windows path need not be
    /// valid UTF-16).
    pub(crate) files: Vec<Vec<u16>>,
}

/// Why a frame is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FrameError {
    /// The body is longer than [`MAX_FRAME_LEN`].
    #[error("the frame's {0} bytes are more than the first instance reads")]
    TooLong(u64),
    /// The body is of a version this build does not read.
    #[error("frame version {0} is not read here")]
    Version(u8),
    /// An item of a kind this build does not read.
    #[error("item kind {0} is not read here")]
    Kind(u8),
    /// The body ends inside an item, or carries bytes past its last one.
    #[error("the frame's body is cut or padded")]
    Malformed,
    /// More items than an [`Opened`](flui_platform::Opened) keeps.
    #[error("the frame carries more items than a launch opens")]
    TooManyItems,
    /// A link longer than an `Opened` keeps, or not text.
    #[error("a link is too long or not text")]
    Link,
    /// A path that is empty, too long or carries a NUL.
    #[error("a path is empty, too long or carries a NUL")]
    Path,
}

impl Forwarded {
    /// The frame of this launch, its length first; `None` when it passes a
    /// bound the first instance holds it to, and the launch then runs as an
    /// instance of its own.
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        if self.links.len() + self.files.len() > MAX_OPENED_ITEMS
            || !self
                .links
                .iter()
                .all(|link| link.len() <= MAX_OPENED_URL_LEN)
            || !self.files.iter().all(|path| path_admitted(path))
        {
            return None;
        }
        let units = self
            .links
            .iter()
            .map(|link| link.encode_utf16().count())
            .chain(self.files.iter().map(Vec::len));
        let body_len = 1 + units.map(|units| ITEM_HEADER + 2 * units).sum::<usize>();
        if body_len > MAX_FRAME_LEN {
            return None;
        }
        let mut frame = Vec::with_capacity(4 + body_len);
        frame.extend_from_slice(
            &u32::try_from(body_len)
                .expect("BUG: a body within MAX_FRAME_LEN fits u32")
                .to_le_bytes(),
        );
        frame.push(VERSION);
        for link in &self.links {
            push_item(&mut frame, LINK, link.encode_utf16());
        }
        for path in &self.files {
            push_item(&mut frame, FILE, path.iter().copied());
        }
        debug_assert_eq!(
            frame.len(),
            4 + body_len,
            "the length prefix counts the body"
        );
        Some(frame)
    }

    /// The launch `body`, a frame's body of the length [`body_len`] read,
    /// carries.
    ///
    /// # Errors
    ///
    /// The bound the body passes, or how it is malformed.
    pub(crate) fn decode(body: &[u8]) -> Result<Self, FrameError> {
        let (&version, mut rest) = body.split_first().ok_or(FrameError::Malformed)?;
        if version != VERSION {
            return Err(FrameError::Version(version));
        }
        let mut forwarded = Self::default();
        while !rest.is_empty() {
            if forwarded.links.len() + forwarded.files.len() == MAX_OPENED_ITEMS {
                return Err(FrameError::TooManyItems);
            }
            let (header, after) = rest
                .split_at_checked(ITEM_HEADER)
                .ok_or(FrameError::Malformed)?;
            let kind = header[0];
            let units = u32::from_le_bytes(
                header[1..]
                    .try_into()
                    .expect("BUG: an item header holds a u32 after its kind"),
            );
            // Checked before the units are read, by the longest item of its
            // kind: a length never sizes a copy on its own.
            let longest = match kind {
                LINK => MAX_OPENED_URL_LEN,
                FILE => MAX_PATH_UNITS,
                other => return Err(FrameError::Kind(other)),
            };
            let units = usize::try_from(units)
                .ok()
                .filter(|units| *units <= longest)
                .ok_or(if kind == LINK {
                    FrameError::Link
                } else {
                    FrameError::Path
                })?;
            let (bytes, after) = after
                .split_at_checked(2 * units)
                .ok_or(FrameError::Malformed)?;
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|unit| u16::from_le_bytes(*unit));
            if kind == LINK {
                let link = char::decode_utf16(units)
                    .collect::<Result<String, _>>()
                    .ok()
                    .filter(|link| link.len() <= MAX_OPENED_URL_LEN)
                    .ok_or(FrameError::Link)?;
                forwarded.links.push(link);
            } else {
                let path = units.collect::<Vec<u16>>();
                if !path_admitted(&path) {
                    return Err(FrameError::Path);
                }
                forwarded.files.push(path);
            }
            rest = after;
        }
        Ok(forwarded)
    }
}

/// The length of the body a frame's first four bytes announce.
///
/// # Errors
///
/// [`FrameError::TooLong`] past [`MAX_FRAME_LEN`], before anything of the
/// body is read.
pub(crate) fn body_len(prefix: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_le_bytes(prefix);
    usize::try_from(len)
        .ok()
        .filter(|len| *len <= MAX_FRAME_LEN)
        .ok_or(FrameError::TooLong(u64::from(len)))
}

/// Whether `path` is one a launch forwards: not empty, no longer than
/// [`MAX_PATH_UNITS`], with no NUL.
fn path_admitted(path: &[u16]) -> bool {
    !path.is_empty() && path.len() <= MAX_PATH_UNITS && !path.contains(&0)
}

/// Append an item of `kind` whose units are `units`.
fn push_item(frame: &mut Vec<u8>, kind: u8, units: impl Iterator<Item = u16> + Clone) {
    let count =
        u32::try_from(units.clone().count()).expect("BUG: an item within MAX_FRAME_LEN fits u32");
    frame.push(kind);
    frame.extend_from_slice(&count.to_le_bytes());
    frame.extend(units.flat_map(u16::to_le_bytes));
}

#[cfg(windows)]
impl Forwarded {
    /// What `launch`, this process's own cold launch, forwards: its links,
    /// then its files, whose paths the launch made absolute.
    pub(crate) fn of_launch(launch: flui_platform::Opened) -> Self {
        use std::os::windows::ffi::OsStrExt as _;

        Self {
            files: launch
                .files
                .iter()
                .map(|file| file.path.as_os_str().encode_wide().collect())
                .collect(),
            links: launch.urls,
        }
    }

    /// What a forwarded launch opens in this instance: its links and files,
    /// as reaching the running application as a whole.
    pub(crate) fn opened(&self) -> flui_platform::Opened {
        use std::os::windows::ffi::OsStringExt as _;

        use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};

        Opened::new(
            self.links.clone(),
            self.files
                .iter()
                .map(|path| {
                    OpenedFile::new(std::path::PathBuf::from(std::ffi::OsString::from_wide(
                        path,
                    )))
                })
                .collect::<Vec<OpenedFile>>(),
            OpenTarget::Application,
            Launch::Running,
        )
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{MAX_OPENED_ITEMS, MAX_OPENED_URL_LEN};

    use super::{Forwarded, FrameError, MAX_FRAME_LEN, MAX_PATH_UNITS, body_len};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// The body of `forwarded`'s frame, after its length.
    fn body(forwarded: &Forwarded) -> Vec<u8> {
        let frame = forwarded.encode().expect("a frame within its bounds");
        let len = body_len(frame[..4].try_into().expect("a length prefix")).expect("a length");
        assert_eq!(len, frame.len() - 4, "the prefix counts the body");
        frame[4..].to_vec()
    }

    /// A launch's links and files reach the first instance as they left,
    /// a path that is not valid UTF-16 among them; a launch that opens
    /// nothing is a frame too, and brings the first forward.
    #[test]
    fn a_launch_crosses_the_pipe_as_it_left() {
        let lone_surrogate = {
            let mut path = wide(r"C:\notes\");
            path.push(0xD800);
            path
        };
        for forwarded in [
            Forwarded::default(),
            Forwarded {
                links: vec![
                    "notes:///note/3".to_owned(),
                    "https://example.com/ü".to_owned(),
                ],
                files: vec![wide(r"C:\Users\me\Документы\a.note"), lone_surrogate],
            },
            Forwarded {
                links: vec!["x".repeat(MAX_OPENED_URL_LEN)],
                files: vec![vec![u16::from(b'a'); MAX_PATH_UNITS]],
            },
        ] {
            assert_eq!(Forwarded::decode(&body(&forwarded)), Ok(forwarded.clone()));
        }
    }

    /// A launch past a bound the first holds it to is not sent: its process
    /// runs as an instance of its own.
    #[test]
    fn a_launch_past_its_bounds_is_not_sent() {
        let too_many = Forwarded {
            links: vec!["a:b".to_owned(); MAX_OPENED_ITEMS + 1],
            files: Vec::new(),
        };
        let long_link = Forwarded {
            links: vec!["x".repeat(MAX_OPENED_URL_LEN + 1)],
            files: Vec::new(),
        };
        let long_path = Forwarded {
            links: Vec::new(),
            files: vec![vec![u16::from(b'a'); MAX_PATH_UNITS + 1]],
        };
        let empty_path = Forwarded {
            links: Vec::new(),
            files: vec![Vec::new()],
        };
        let past_the_frame = Forwarded {
            links: Vec::new(),
            files: vec![vec![u16::from(b'a'); MAX_PATH_UNITS]; 17],
        };
        for (case, forwarded) in [
            ("too many items", too_many),
            ("a long link", long_link),
            ("a long path", long_path),
            ("an empty path", empty_path),
            ("past the frame", past_the_frame),
        ] {
            assert_eq!(forwarded.encode(), None, "{case}");
        }
    }

    /// A body is read as untrusted input: its length is refused before the
    /// body is read, and a body malformed or past a bound is refused whole.
    #[test]
    fn a_malformed_frame_is_refused_whole() {
        let max = u32::try_from(MAX_FRAME_LEN).expect("the bound fits u32");
        assert_eq!(body_len(max.to_le_bytes()), Ok(MAX_FRAME_LEN));
        assert_eq!(
            body_len((max + 1).to_le_bytes()),
            Err(FrameError::TooLong(u64::from(max) + 1))
        );
        assert_eq!(
            body_len(u32::MAX.to_le_bytes()),
            Err(FrameError::TooLong(u64::from(u32::MAX)))
        );

        let item = |kind: u8, units: u32, payload: &[u16]| {
            let mut bytes = vec![kind];
            bytes.extend_from_slice(&units.to_le_bytes());
            bytes.extend(payload.iter().flat_map(|unit| unit.to_le_bytes()));
            bytes
        };
        let frame = |items: &[Vec<u8>]| {
            let mut bytes = vec![1];
            bytes.extend(items.concat());
            bytes
        };
        let link = item(0, 3, &wide("a:b"));
        let mut padded = frame(std::slice::from_ref(&link));
        padded.push(0);
        let mut cut = frame(std::slice::from_ref(&link));
        cut.pop();
        let cases: [(&str, Vec<u8>, FrameError); 11] = [
            ("empty", Vec::new(), FrameError::Malformed),
            ("another version", vec![2], FrameError::Version(2)),
            (
                "another kind",
                frame(&[item(7, 0, &[])]),
                FrameError::Kind(7),
            ),
            ("cut", cut, FrameError::Malformed),
            ("padded", padded, FrameError::Malformed),
            ("a header cut", vec![1, 0, 3], FrameError::Malformed),
            (
                "a link's length past its bound",
                frame(&[item(0, u32::MAX, &[])]),
                FrameError::Link,
            ),
            (
                "a link that is not text",
                frame(&[item(0, 1, &[0xDC00])]),
                FrameError::Link,
            ),
            (
                "a path with a NUL",
                frame(&[item(1, 2, &[u16::from(b'a'), 0])]),
                FrameError::Path,
            ),
            ("an empty path", frame(&[item(1, 0, &[])]), FrameError::Path),
            (
                "too many items",
                frame(&vec![link.clone(); MAX_OPENED_ITEMS + 1]),
                FrameError::TooManyItems,
            ),
        ];
        for (case, body, refused) in cases {
            assert_eq!(Forwarded::decode(&body), Err(refused), "{case}");
        }
        assert_eq!(
            Forwarded::decode(&frame(&vec![link; MAX_OPENED_ITEMS])).map(|f| f.links.len()),
            Ok(MAX_OPENED_ITEMS),
            "as many items as a launch opens"
        );
    }
}
