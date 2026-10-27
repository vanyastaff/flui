use std::path::PathBuf;
use std::sync::Arc;

use flui_platform::data_transfer::{
    Mime, RepresentationDescriptor, TransferFormat, TransferPayload,
};

use super::{OfferTable, PathStyle, TransferUri, format_of, payload_of_bytes};

/// A media type offers its transfer format; a type FLUI does not announce
/// (the browser's `Files`, Chromium's private types, a name that is no
/// media type) offers none.
#[test]
fn a_media_type_offers_its_transfer_format() {
    let image = |mime: &str| {
        Some(TransferFormat::Image {
            mime: Mime::new(mime),
        })
    };
    let custom = |mime: &str| {
        Some(TransferFormat::Custom {
            mime: Mime::new(mime),
        })
    };
    let rows = [
        ("text/plain", Some(TransferFormat::Text)),
        ("text/html", Some(TransferFormat::Html)),
        ("text/uri-list", Some(TransferFormat::UriList)),
        ("image/png", image("image/png")),
        ("application/pdf", custom("application/pdf")),
        (
            "application/x-flui-drag; session=7",
            custom("application/x-flui-drag; session=7"),
        ),
        ("Files", None),
        ("chromium/x-renderer-taint", None),
        ("text/", None),
        ("/plain", None),
        ("text plain", None),
        ("text/pla in", None),
    ];
    for (media_type, format) in rows {
        assert_eq!(format_of(media_type), format, "{media_type:?}");
    }
}

/// Bytes carry the payload of their format: an image the bytes of its
/// container as it was announced, any other format the raw bytes of its
/// type's essence, and a format a file never announces
/// `application/octet-stream`.
#[test]
fn bytes_carry_the_payload_of_their_format() {
    let carried = |format: &TransferFormat| match payload_of_bytes(format, b"bytes".to_vec()) {
        TransferPayload::Custom { mime, bytes } => (mime.as_str().to_owned(), bytes.to_vec()),
        other => panic!("bytes carried as {other:?}"),
    };
    let rows = [
        (
            TransferFormat::Image {
                mime: Mime::new("image/png"),
            },
            "image/png",
        ),
        (
            TransferFormat::Custom {
                mime: Mime::new("application/x-flui-drag; session=7"),
            },
            "application/x-flui-drag",
        ),
        (TransferFormat::Text, "application/octet-stream"),
        (TransferFormat::Html, "application/octet-stream"),
        (TransferFormat::UriList, "application/octet-stream"),
    ];
    for (format, mime) in rows {
        assert_eq!(
            carried(&format),
            (mime.to_owned(), b"bytes".to_vec()),
            "{format:?}"
        );
    }
}

const WINDOWS: PathStyle = PathStyle::Windows;
const POSIX: PathStyle = PathStyle::Posix;

/// The line a host writes for one path in `style`, or `None` when it leaves the
/// path out.
fn written(path: impl Into<PathBuf>, style: PathStyle) -> Option<String> {
    let TransferPayload::Custom { mime, bytes } =
        TransferUri::list_payload([TransferUri::Path(path.into())], style)
    else {
        panic!("a URI list is delivered as bytes");
    };
    assert_eq!(mime.as_str(), "text/uri-list");
    let text = std::str::from_utf8(&bytes).expect("a URI list is UTF-8");
    text.strip_suffix("\r\n").map(str::to_owned)
}

/// What a host reads one line of a served URI list as, in `style`.
fn read(line: &str, style: PathStyle) -> TransferUri {
    let served = TransferPayload::Custom {
        mime: Mime::new("text/uri-list"),
        bytes: Arc::from(format!("{line}\r\n").into_bytes()),
    };
    let mut entries = TransferUri::list_of(&served, style).expect("a URI list");
    assert_eq!(entries.len(), 1, "{line:?}");
    entries.remove(0)
}

fn path(path: &str) -> TransferUri {
    TransferUri::Path(PathBuf::from(path))
}

fn uri(uri: &str) -> TransferUri {
    TransferUri::Uri(uri.to_owned())
}

/// A host's path reaches the application as its `file:` URI in the host's
/// spelling, percent-encoded; a path no URI names is left out. Only Windows
/// spells drives, shares and verbatim paths; a POSIX name keeps a backslash.
#[test]
fn a_path_is_written_as_its_file_uri() {
    let rows: [(&str, PathStyle, Option<&str>); 13] = [
        (
            r"C:\Users\a b\x#1.txt",
            WINDOWS,
            Some("file:///C:/Users/a%20b/x%231.txt"),
        ),
        (
            r"\\server\share\é.png",
            WINDOWS,
            Some("file://server/share/%C3%A9.png"),
        ),
        (r"\\?\C:\long\a.txt", WINDOWS, Some("file:///C:/long/a.txt")),
        (
            r"\\?\UNC\server\share\a.txt",
            WINDOWS,
            Some("file://server/share/a.txt"),
        ),
        (r"\\?\Volume{1}\a.txt", WINDOWS, None),
        (r"\rooted\a.txt", WINDOWS, Some("file:///rooted/a.txt")),
        (r"relative\a.txt", WINDOWS, None),
        ("/tmp/100%.txt", POSIX, Some("file:///tmp/100%25.txt")),
        (r"/tmp/a\b.txt", POSIX, Some("file:///tmp/a%5Cb.txt")),
        (r"\\server\share\a.txt", POSIX, None),
        (r"C:\a.txt", POSIX, None),
        ("relative/a.txt", POSIX, None),
        (
            "/home/ü/ä ö.txt",
            POSIX,
            Some("file:///home/%C3%BC/%C3%A4%20%C3%B6.txt"),
        ),
    ];
    for (source, style, line) in rows {
        assert_eq!(
            written(source, style).as_deref(),
            line,
            "{source:?} in {style:?}"
        );
    }
    // Any other URI passes through, and the lines keep their order.
    let TransferPayload::Custom { bytes, .. } = TransferUri::list_payload(
        [uri("content://media/1"), path("/a"), path("relative")],
        POSIX,
    ) else {
        panic!("a URI list is delivered as bytes");
    };
    assert_eq!(&*bytes, b"content://media/1\r\nfile:///a\r\n");
}

/// A `file:` URI an application serves reads back as the path it names in the
/// host's spelling; anything else (another scheme, a malformed escape, a remote
/// host where the host has no shares, no absolute path) stays a URI.
#[test]
fn a_served_file_uri_reads_as_its_path() {
    let rows: [(&str, PathStyle, TransferUri); 14] = [
        (
            "file:///C:/Users/a%20b/x%231.txt",
            WINDOWS,
            path(r"C:\Users\a b\x#1.txt"),
        ),
        (
            "file://server/share/%C3%A9.png",
            WINDOWS,
            path(r"\\server\share\é.png"),
        ),
        ("FILE://localhost/C:/a.txt", WINDOWS, path(r"C:\a.txt")),
        ("file:///rooted/a.txt", WINDOWS, path(r"\rooted\a.txt")),
        ("file:///tmp/100%25.txt", POSIX, path("/tmp/100%.txt")),
        ("file://localhost/tmp/a.txt", POSIX, path("/tmp/a.txt")),
        ("file:///tmp/a%5Cb.txt", POSIX, path(r"/tmp/a\b.txt")),
        (
            "file://server/share/a.txt",
            POSIX,
            uri("file://server/share/a.txt"),
        ),
        ("file:///tmp/bad%zz", POSIX, uri("file:///tmp/bad%zz")),
        ("file:///tmp/sign%+1", POSIX, uri("file:///tmp/sign%+1")),
        ("file:///tmp/short%4", POSIX, uri("file:///tmp/short%4")),
        (
            "file:///tmp/not-utf8%FF",
            POSIX,
            uri("file:///tmp/not-utf8%FF"),
        ),
        ("file:relative", POSIX, uri("file:relative")),
        (
            "https://flui.dev/?q=1",
            WINDOWS,
            uri("https://flui.dev/?q=1"),
        ),
    ];
    for (line, style, entry) in rows {
        assert_eq!(read(line, style), entry, "{line:?} in {style:?}");
    }
    // Comments and blank lines are no entries; a payload of another type is
    // no list.
    let served = TransferPayload::Custom {
        mime: Mime::new("text/uri-list"),
        bytes: Arc::from(b"# a comment\r\n\r\nfile:///a\n".to_vec()),
    };
    assert_eq!(TransferUri::list_of(&served, POSIX), Some(vec![path("/a")]));
    assert_eq!(
        TransferUri::list_of(&TransferPayload::Text("file:///a".to_owned()), POSIX),
        None
    );
}

/// What a host writes, it reads back in the same spelling.
#[test]
fn a_written_path_reads_back() {
    let rows = [
        (r"D:\a\b c.txt", WINDOWS),
        (r"\\server\share\é.png", WINDOWS),
        (r"\rooted\a.txt", WINDOWS),
        ("/home/ü/ä ö.txt", POSIX),
        (r"/tmp/a\b%.txt", POSIX),
    ];
    for (source, style) in rows {
        let paths = vec![path(source)];
        let written = TransferUri::list_payload(paths.clone(), style);
        assert_eq!(
            TransferUri::list_of(&written, style),
            Some(paths),
            "{source:?} in {style:?}"
        );
    }
}

/// A path that is not Unicode has no `file:` URI a reader would take back, so
/// none is written: an unpaired surrogate on Windows, bytes that are not UTF-8
/// on Unix.
#[test]
fn a_path_that_is_not_unicode_is_left_out() {
    #[cfg(windows)]
    let (source, style) = {
        use std::os::windows::ffi::OsStringExt;
        let wide = [u16::from(b'C'), u16::from(b':'), u16::from(b'\\'), 0xD800];
        (std::ffi::OsString::from_wide(&wide), WINDOWS)
    };
    #[cfg(unix)]
    let (source, style) = {
        use std::os::unix::ffi::OsStringExt;
        (std::ffi::OsString::from_vec(b"/tmp/\xff".to_vec()), POSIX)
    };
    assert_eq!(written(source, style), None);
}

fn announcing(format: TransferFormat) -> Arc<[RepresentationDescriptor]> {
    Arc::from([RepresentationDescriptor {
        format,
        declared_len: None,
    }])
}

/// A retired offer is stale for good: its id finds nothing, retiring it again
/// does nothing, and the slot's next occupant mints a different id.
#[test]
fn a_retired_offer_never_aliases_the_next() {
    let mut table = OfferTable::default();
    let first = table.mint(announcing(TransferFormat::Text));
    assert!(table.get(first).is_some());
    assert!(table.retire(first).is_some());
    assert!(table.get(first).is_none());
    assert!(table.retire(first).is_none());
    let next = table.mint(announcing(TransferFormat::Html));
    assert_eq!(next.index(), first.index(), "the slot is reused");
    assert_ne!(next, first);
    assert!(table.get(first).is_none());
    assert_eq!(
        table.get(next).map(|offered| offered[0].format.clone()),
        Some(TransferFormat::Html)
    );
}
