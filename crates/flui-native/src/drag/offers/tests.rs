use std::path::PathBuf;
use std::sync::Arc;

use flui_platform::data_transfer::{
    Mime, RepresentationDescriptor, TransferFormat, TransferPayload,
};

use super::{OfferTable, TransferUri, format_of, payload_of_bytes};

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

/// The text a URI-list payload carries.
fn list_text(payload: &TransferPayload) -> &str {
    let TransferPayload::Custom { mime, bytes } = payload else {
        panic!("a URI list is delivered as bytes, got {payload:?}");
    };
    assert_eq!(mime.as_str(), "text/uri-list");
    std::str::from_utf8(bytes).expect("a URI list is UTF-8")
}

/// A host's paths and URIs reach the application as `text/uri-list`: a path as
/// its `file:` URI, percent-encoded, one CRLF-terminated line each; a relative
/// path, which no URI names, is left out.
#[test]
fn a_uri_list_is_delivered_as_file_uris() {
    let payload = TransferUri::list_payload([
        TransferUri::Path(PathBuf::from(r"C:\Users\a b\x#1.txt")),
        TransferUri::Path(PathBuf::from(r"\\server\share\é.png")),
        TransferUri::Path(PathBuf::from("/tmp/100%.txt")),
        TransferUri::Path(PathBuf::from("relative/a.txt")),
        TransferUri::Uri("content://media/1".to_owned()),
    ]);
    assert_eq!(
        list_text(&payload),
        "file:///C:/Users/a%20b/x%231.txt\r\n\
         file://server/share/%C3%A9.png\r\n\
         file:///tmp/100%25.txt\r\n\
         content://media/1\r\n"
    );
}

/// A URI list an application serves reads back as the host's entries: a
/// `file:` URI of a local path as that path, any other URI as itself, with
/// comments and blank lines skipped; a payload of another type is no list.
#[test]
fn a_served_uri_list_reads_as_paths_and_uris() {
    let served = TransferPayload::Custom {
        mime: Mime::new("text/uri-list"),
        bytes: Arc::from(
            b"# a comment\r\n\
              file:///C:/Users/a%20b/x%231.txt\r\n\
              \r\n\
              FILE://localhost/tmp/100%25.txt\n\
              file://server/share/%C3%A9.png\r\n\
              file:///tmp/bad%zz\r\n\
              https://flui.dev/?q=1\r\n"
                .to_vec(),
        ),
    };
    assert_eq!(
        TransferUri::list_of(&served),
        Some(vec![
            TransferUri::Path(PathBuf::from(r"C:\Users\a b\x#1.txt")),
            TransferUri::Path(PathBuf::from("/tmp/100%.txt")),
            TransferUri::Path(PathBuf::from(r"\\server\share\é.png")),
            TransferUri::Uri("file:///tmp/bad%zz".to_owned()),
            TransferUri::Uri("https://flui.dev/?q=1".to_owned()),
        ])
    );
    assert_eq!(
        TransferUri::list_of(&TransferPayload::Text("file:///tmp/a".to_owned())),
        None
    );
    // What a host writes, it reads back.
    let paths = vec![
        TransferUri::Path(PathBuf::from(r"D:\a\b c.txt")),
        TransferUri::Path(PathBuf::from("/home/ü/ä ö.txt")),
    ];
    assert_eq!(
        TransferUri::list_of(&TransferUri::list_payload(paths.clone())),
        Some(paths)
    );
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
