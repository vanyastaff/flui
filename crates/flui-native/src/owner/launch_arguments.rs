//! What a desktop launch's arguments open: the URLs and files a Windows or
//! Linux desktop hands an application on its command line (the activation
//! spec, design §4). A registered scheme or file type launches the
//! application with the link or the file as an argument (Windows' `"%1"`,
//! a desktop entry's `%U`, which gives a file as a `file:` URI). The host
//! reports what they open before `ready`, as a cold
//! [`Opened`](flui_platform::Opened) for the application as a whole.
//!
//! An argument is a URL when it starts with a scheme of two or more
//! characters and a colon (RFC 3986 §3.1; one letter is a Windows drive),
//! a file when it is a `file:` URI or the path of an existing file, and
//! neither otherwise: the application's own flags and values stay its own,
//! logged at debug and never opened.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};

/// How a `file:` URI's path names a file on the host the arguments came
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathConvention {
    /// `file:///C:/dir/name` is `C:\dir\name`, and `file://server/share/name`
    /// is `\\server\share\name`.
    Windows,
    /// `file:///dir/name` is `/dir/name`; a URI naming another host names no
    /// file here.
    Unix,
}

impl PathConvention {
    /// The convention of the host this is built for.
    pub(crate) const HOST: Self = if cfg!(windows) {
        Self::Windows
    } else {
        Self::Unix
    };
}

/// What `arguments` (without the program's own name) open, where `is_file`
/// says whether a path names an existing file and `absolute` makes a path
/// the application was given relative to its working directory absolute.
pub(crate) fn opened_by_arguments(
    arguments: impl IntoIterator<Item = OsString>,
    convention: PathConvention,
    is_file: impl Fn(&Path) -> bool,
    absolute: impl Fn(&Path) -> Option<PathBuf>,
) -> Opened {
    let mut urls = Vec::new();
    let mut files = Vec::new();
    for argument in arguments {
        match opens(&argument, convention, &is_file, &absolute) {
            Some(Opens::Url(url)) => urls.push(url),
            Some(Opens::File(path)) => files.push(OpenedFile::new(path)),
            None => tracing::debug!(
                argument = %argument.to_string_lossy(),
                "a launch argument that is neither a URL nor an existing file is left to the \
                 application"
            ),
        }
    }
    Opened::new(urls, files, OpenTarget::Application, Launch::Cold)
}

/// The process's launch arguments, without its own name: what a desktop
/// host table hands its loop.
pub(crate) fn process_arguments() -> Vec<OsString> {
    std::env::args_os().skip(1).collect()
}

/// What `arguments`, a desktop host table's launch arguments, open on this
/// host, judged against its file system.
pub(crate) fn launched_by(arguments: Vec<OsString>) -> Opened {
    opened_by_arguments(arguments, PathConvention::HOST, Path::is_file, |path| {
        std::path::absolute(path).ok()
    })
}

/// What one launch argument opens.
#[derive(Debug, PartialEq, Eq)]
enum Opens {
    Url(String),
    File(PathBuf),
}

fn opens(
    argument: &OsString,
    convention: PathConvention,
    is_file: &impl Fn(&Path) -> bool,
    absolute: &impl Fn(&Path) -> Option<PathBuf>,
) -> Option<Opens> {
    let existing = |path: &Path| {
        let path = absolute(path)?;
        is_file(&path).then_some(Opens::File(path))
    };
    let Some(text) = argument.to_str() else {
        // A path the operating system gave in bytes that are not UTF-8 is
        // still a path; a URL is always text.
        return existing(Path::new(argument));
    };
    match scheme(text) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let path = file_uri_path(text, convention)?;
            is_file(&path).then_some(Opens::File(path))
        }
        Some(_) => Some(Opens::Url(text.to_owned())),
        None => existing(Path::new(text)),
    }
}

/// The scheme `text` starts with: two or more characters, a letter and then
/// letters, digits, `+`, `-` and `.`, followed by `:`.
fn scheme(text: &str) -> Option<&str> {
    let (scheme, _) = text.split_once(':')?;
    let mut characters = scheme.chars();
    let well_formed = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (well_formed && scheme.len() >= 2).then_some(scheme)
}

/// The file a `file:` URI names under `convention`: its authority, empty or
/// `localhost` for this host, and its percent-decoded path.
fn file_uri_path(uri: &str, convention: PathConvention) -> Option<PathBuf> {
    let rest = &uri["file:".len()..];
    // A query or fragment names nothing on disk.
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = match rest.strip_prefix("//") {
        Some(authority_and_path) => match authority_and_path.find('/') {
            Some(at) => authority_and_path.split_at(at),
            None => (authority_and_path, ""),
        },
        // `file:/dir/name`, without an authority.
        None => ("", rest),
    };
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    let decoded = percent_decoded(path)?;
    match convention {
        PathConvention::Unix => {
            if !local || !decoded.starts_with(b"/") {
                return None;
            }
            unix_path(decoded)
        }
        PathConvention::Windows => {
            let decoded = String::from_utf8(decoded).ok()?;
            let windows = if local {
                // `/C:/dir/name`: the drive follows the leading slash.
                let drive = decoded.strip_prefix('/')?;
                let mut letters = drive.chars();
                let lettered = letters.next().is_some_and(|c| c.is_ascii_alphabetic())
                    && letters.next() == Some(':');
                if !lettered {
                    return None;
                }
                drive.replace('/', "\\")
            } else {
                format!("\\\\{host}{}", decoded.replace('/', "\\"))
            };
            Some(PathBuf::from(windows))
        }
    }
}

/// The path of `bytes`, a Unix path: the bytes themselves on a host whose
/// paths are bytes, only text elsewhere.
#[cfg_attr(
    unix,
    expect(
        clippy::unnecessary_wraps,
        reason = "a host whose paths are not bytes refuses a path that is not text"
    )
)]
fn unix_path(bytes: Vec<u8>) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        Some(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes).ok().map(PathBuf::from)
    }
}

/// `text` with each `%XX` replaced by its byte, or `None` when an escape is
/// malformed.
fn percent_decoded(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let digits = bytes.get(at + 1..at + 3)?;
            let hex = std::str::from_utf8(digits).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use flui_platform::{Launch, OpenTarget, OpenedFile};

    use super::{PathConvention, opened_by_arguments};

    /// The launch arguments of a desktop open the URLs they give and the
    /// files that exist, by path or by `file:` URI, for the application as
    /// a whole on a cold launch; a flag, a value, a missing file, a `file:`
    /// URI of another host on Unix and a drive letter are not URLs.
    #[test]
    fn launch_arguments_open_their_urls_and_existing_files() {
        let existing = [
            "/home/ana/notes/a b.note",
            "/home/ana/notes/plan.note",
            "/work/tool/relative.note",
            "C:\\Users\\Ana\\a b.note",
            "\\\\server\\share\\shared.note",
        ];
        let is_file = |path: &Path| existing.iter().any(|file| Path::new(file) == path);
        let absolute = |path: &Path| {
            Some(
                if path.is_absolute() || path.to_string_lossy().contains(':') {
                    path.to_path_buf()
                } else {
                    Path::new("/work/tool").join(path)
                },
            )
        };
        let arguments =
            |items: &[&str]| -> Vec<OsString> { items.iter().map(OsString::from).collect() };

        let unix = opened_by_arguments(
            arguments(&[
                "notes:///note/3",
                "https://notes.example.com/note/4?ref=mail",
                "file:///home/ana/notes/a%20b.note",
                "file://localhost/home/ana/notes/plan.note",
                "file://other-host/home/ana/notes/plan.note",
                "file:///home/ana/notes/missing.note",
                "relative.note",
                "--verbose",
                "--log=debug",
                "missing.note",
                "file:///home/ana/notes/bad%zz.note",
            ]),
            PathConvention::Unix,
            is_file,
            absolute,
        );
        assert_eq!(
            unix.urls,
            [
                "notes:///note/3",
                "https://notes.example.com/note/4?ref=mail"
            ]
        );
        assert_eq!(
            unix.files,
            [
                OpenedFile::new("/home/ana/notes/a b.note"),
                OpenedFile::new("/home/ana/notes/plan.note"),
                OpenedFile::new("/work/tool/relative.note"),
            ]
        );
        assert_eq!(unix.target, OpenTarget::Application);
        assert_eq!(unix.launch, Launch::Cold);

        let windows = opened_by_arguments(
            arguments(&[
                "notes:///note/3",
                "C:\\Users\\Ana\\a b.note",
                "file:///C:/Users/Ana/a%20b.note",
                "file://server/share/shared.note",
                "file:///Users/Ana/no-drive.note",
                "/flag",
            ]),
            PathConvention::Windows,
            is_file,
            absolute,
        );
        assert_eq!(windows.urls, ["notes:///note/3"]);
        assert_eq!(
            windows.files,
            [
                OpenedFile::new("C:\\Users\\Ana\\a b.note"),
                OpenedFile::new("C:\\Users\\Ana\\a b.note"),
                OpenedFile::new("\\\\server\\share\\shared.note"),
            ]
        );

        let nothing = opened_by_arguments(
            arguments(&["--help"]),
            PathConvention::Unix,
            is_file,
            absolute,
        );
        assert!(nothing.is_empty());
    }

    /// A path in bytes that are not UTF-8 is still a path.
    #[cfg(unix)]
    #[test]
    fn a_launch_path_need_not_be_text() {
        use std::os::unix::ffi::OsStringExt as _;
        let path = OsString::from_vec(b"/tmp/caf\xe9.note".to_vec());
        let expected = PathBuf::from(path.clone());
        let opened = opened_by_arguments(
            [path],
            PathConvention::Unix,
            |candidate| candidate == expected,
            |path| Some(path.to_path_buf()),
        );
        assert_eq!(opened.files, [OpenedFile::new(expected)]);
        assert_eq!(opened.urls, Vec::<String>::new());
    }
}
