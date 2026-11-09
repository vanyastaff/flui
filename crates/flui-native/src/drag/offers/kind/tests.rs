use std::sync::Arc;

use flui_platform::data_transfer::{Mime, TransferError, TransferFormat, TransferPayload};

use super::super::too_large;
use super::{Kind, representations};
use crate::clipboard::text::TextForm;

/// The platform's transfer values compare by what they print: they hold no
/// equality of their own.
macro_rules! assert_same {
    ($actual:expr, $expected:expr $(, $($context:tt)*)?) => {{
        let (actual, expected) = (format!("{:?}", $actual), format!("{:?}", $expected));
        assert_eq!(actual, expected $(, $($context)*)?)
    }};
}

/// Text is offered once, in the form a reader prefers, where the first text
/// form stood; names that are no media type are left out.
#[test]
fn types_offer_each_format_once() {
    let types = [
        (1, "TARGETS"),
        (2, "text/plain"),
        (3, "text/uri-list"),
        (4, "UTF8_STRING"),
        (5, "text/html"),
        (6, "COMPOUND_TEXT"),
        (7, "image/png"),
        (8, "text/plain;charset=utf-8"),
        (9, "text/x-moz-url"),
    ];
    let offered = representations(&types);
    assert_eq!(
        offered,
        vec![
            (8, Kind::Text(TextForm::PlainUtf8)),
            (3, Kind::Bytes(TransferFormat::UriList)),
            (5, Kind::Html),
            (
                7,
                Kind::Bytes(TransferFormat::Image {
                    mime: Mime::new("image/png")
                })
            ),
            (
                9,
                Kind::Bytes(TransferFormat::Custom {
                    mime: Mime::new("text/x-moz-url")
                })
            ),
        ]
    );
    assert_eq!(representations(&[(1, "TARGETS")]), Vec::new());
}

/// Each kind's bytes make its payload: text decoded, HTML in UTF-8 or
/// behind a UTF-16 mark, a URI list as its bytes without a trailing NUL;
/// each measured exactly before it is made.
#[test]
fn bytes_make_their_payload() {
    let table: [(&str, Kind, Vec<u8>, TransferPayload); 7] = [
        (
            "ill-formed utf-8 text",
            Kind::Text(TextForm::Utf8String),
            vec![b'a', 0xFF, 0xFE, b'b'],
            TransferPayload::Text("a\u{FFFD}\u{FFFD}b".to_owned()),
        ),
        (
            "utf-16le html with a lone surrogate and a trailing NUL",
            Kind::Html,
            vec![0xFF, 0xFE, b'x', 0, 0x00, 0xD8, 0, 0],
            TransferPayload::Html("x\u{FFFD}".to_owned()),
        ),
        (
            "latin-1 text",
            Kind::Text(TextForm::Latin1),
            vec![b'a', 0xE9],
            TransferPayload::Text("aé".to_owned()),
        ),
        (
            "utf-8 html with a trailing NUL",
            Kind::Html,
            b"<b>x</b>\0".to_vec(),
            TransferPayload::Html("<b>x</b>".to_owned()),
        ),
        (
            "utf-16le html",
            Kind::Html,
            vec![0xFF, 0xFE, b'<', 0, b'p', 0, b'>', 0],
            TransferPayload::Html("<p>".to_owned()),
        ),
        (
            "utf-16be html",
            Kind::Html,
            vec![0xFE, 0xFF, 0, b'<', 0, b'p', 0, b'>'],
            TransferPayload::Html("<p>".to_owned()),
        ),
        (
            "a uri list ended by a NUL",
            Kind::Bytes(TransferFormat::UriList),
            b"file:///a\r\n\0".to_vec(),
            TransferPayload::Custom {
                mime: Mime::new("text/uri-list"),
                bytes: Arc::from(&b"file:///a\r\n"[..]),
            },
        ),
    ];
    for (case, kind, bytes, expected) in table {
        let len = u64::try_from(kind.decoded_len(&bytes)).expect("a small payload");
        assert_eq!(len, expected.byte_len(), "{case}: measured as made");
        assert_same!(kind.payload(bytes), expected, "{case}");
    }
}

/// A payload past the limit is refused, one at it is not.
#[test]
fn a_payload_past_its_limit_is_too_large() {
    assert_same!(too_large(10, 10), None::<TransferError>);
    assert_same!(
        too_large(11, 10),
        Some(TransferError::TooLarge {
            actual: 11,
            limit: 10
        })
    );
}
