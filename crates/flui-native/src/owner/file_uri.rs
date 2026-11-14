//! The file a `file:` URI names (RFC 8089): what a desktop launch's
//! arguments carry for a file (a desktop entry's `%U`) and what AppKit and
//! UIKit hand an application for a document, and an Android intent's `file:`
//! data, read the same way on every host.

use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "ios", x11_host, wayland_host, test))]
use flui_platform::{Launch, OpenTarget, Opened, OpenedFile};

/// How a `file:` URI's path names a file on the host the URI came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathConvention {
    /// `file:///C:/dir/name` is `C:\dir\name`, and `file://server/share/name`
    /// is `\\server\share\name`.
    Windows,
    /// `file:///dir/name` is `/dir/name`; a URI naming another host names no
    /// file here.
    Unix,
}

#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
impl PathConvention {
    /// The convention of the host this is built for.
    pub(crate) const HOST: Self = if cfg!(windows) {
        Self::Windows
    } else {
        Self::Unix
    };
}

/// The file `uri`, a `file:` URI, names under `convention`: its authority,
/// empty or `localhost` for this host, and its percent-decoded path; `None`
/// for a URI of another scheme, a malformed escape, or a path the convention
/// does not name a file by.
pub(crate) fn file_uri_path(uri: &str, convention: PathConvention) -> Option<PathBuf> {
    let rest = uri
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file:"))
        .map(|_| &uri[5..])?;
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
        Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
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
            // Two hex digits: `from_str_radix` alone would take `%+1` too.
            let digits = bytes
                .get(at + 1..at + 3)
                .filter(|digits| digits.iter().all(u8::is_ascii_hexdigit))?;
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

#[cfg(any(target_os = "macos", target_os = "ios", x11_host, wayland_host, test))]
/// What `urls` open for `target`, in their order: the absolute strings of
/// the `NSURL`s an Apple platform handed over, or the URIs a second Linux
/// instance forwarded. Each `file:` URL of this host (`convention`) is a
/// document, any other URL a link. A `file:` URL that names no local path
/// (another host, a malformed escape) opens nothing.
pub(crate) fn opened_by_urls(
    urls: impl IntoIterator<Item = String>,
    convention: PathConvention,
    target: OpenTarget,
    launch: Launch,
) -> Opened {
    let mut links = Vec::new();
    let mut files = Vec::new();
    for url in urls {
        let is_file = url
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"));
        if !is_file {
            links.push(url);
        } else if let Some(path) = file_uri_path(&url, convention) {
            files.push(OpenedFile::new(path));
        } else {
            tracing::debug!("a file URL the platform opened names no path on this host");
        }
    }
    Opened::new(links, files, target, launch)
}

/// The `file:` URI of `path`, an absolute Unix path: `file://` and the path
/// with every byte but an unreserved character (RFC 3986 §2.3), `/` and the
/// sub-delimiters a path keeps escaped, so [`file_uri_path`] reads `path`
/// back; `None` for a relative path.
#[cfg(any(x11_host, wayland_host, all(test, unix)))]
pub(crate) fn file_uri(path: &std::path::Path) -> Option<String> {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt as _;

    let bytes = path.as_os_str().as_bytes();
    if !bytes.starts_with(b"/") {
        return None;
    }
    let mut uri = String::with_capacity("file://".len() + bytes.len());
    uri.push_str("file://");
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    Some(uri)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{PathConvention, file_uri_path};

    /// A `file:` URI names a file of this host by its decoded path: an empty
    /// or `localhost` authority, any case of the scheme, the query and
    /// fragment dropped; another scheme, another host on Unix, a relative
    /// path and an escape that is not two hex digits name none.
    #[test]
    fn file_uris_name_the_files_of_their_host() {
        let unix = |uri: &str| file_uri_path(uri, PathConvention::Unix);
        assert_eq!(unix("file:///a/b%20c"), Some(PathBuf::from("/a/b c")));
        assert_eq!(unix("FILE://localhost/a"), Some(PathBuf::from("/a")));
        assert_eq!(unix("file:/a?x=1#top"), Some(PathBuf::from("/a")));
        assert_eq!(unix("file://other/a"), None);
        assert_eq!(unix("file:a"), None);
        assert_eq!(unix("https://example.com/a"), None);
        assert_eq!(unix("file:///a%2"), None);
        assert_eq!(unix("file:///a%zz"), None);
        assert_eq!(unix("file:///a%+1"), None, "a sign is not a hex digit");
        assert_eq!(unix("file:///a%-1"), None);

        let windows = |uri: &str| file_uri_path(uri, PathConvention::Windows);
        assert_eq!(windows("file:///C:/a/b"), Some(PathBuf::from("C:\\a\\b")));
        assert_eq!(
            windows("file://server/share/a"),
            Some(PathBuf::from("\\\\server\\share\\a"))
        );
        assert_eq!(windows("file:///a/b"), None, "no drive");

        // A path made a URI reads back as the same path, bytes included.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;

            for path in ["/a/b c/Plan #1.note", "/notes/50%?.txt", "/a/ü", "/a/[x]"] {
                let uri = super::file_uri(std::path::Path::new(path)).expect("absolute");
                assert_eq!(unix(&uri), Some(PathBuf::from(path)), "{uri}");
            }
            let bytes = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/a/\xff"));
            let uri = super::file_uri(&bytes).expect("absolute");
            assert_eq!(uri, "file:///a/%FF");
            assert_eq!(unix(&uri), Some(bytes));
            assert_eq!(super::file_uri(std::path::Path::new("a/b")), None);
        }
    }
}
