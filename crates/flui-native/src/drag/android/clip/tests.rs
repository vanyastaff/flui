//! The decidable half of the Android backend's drag and drop
//! (`drag::android::clip`, ADR-0247, ADR-0211): what a `DragEvent`'s clip
//! offers, how a read of the held clip answers a request, what a FLUI drag
//! puts in its `ClipData`, and how its source learns the end. The JNI side
//! runs on a device.

use std::num::NonZeroU64;
use std::sync::Arc;

use super::{
    ClipSource, DRAG_FLAG_GLOBAL, DRAG_FLAG_GLOBAL_URI_READ, ViewRead, clip_parts, delivered,
    incoming_allowed, offered, source_end,
};
use flui_foundation::{DragSessionId, UiRuntimeId};
use flui_platform::data_transfer::{
    Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat, TransferLimits,
    TransferPayload,
};
use flui_platform::drag::{DragSessionKey, OsDragEnd, ProcessNonce};

use crate::drag::offers::{PathStyle, TransferUri};

fn identity() -> RepresentationDescriptor {
    DragSessionKey::new(
        ProcessNonce::from_raw(NonZeroU64::new(7).expect("nonzero")),
        UiRuntimeId::new_gen(0, flui_foundation::generation::Generation::FIRST),
        DragSessionId::from_wire(NonZeroU64::MIN),
    )
    .representation()
}

fn identity_mime() -> String {
    match identity().format {
        TransferFormat::Custom { mime } => mime.as_str().to_owned(),
        other => panic!("the identity is a custom type, not {other:?}"),
    }
}

/// A clip description offers its text, HTML and URI list as themselves,
/// FLUI's identity with its parameters and no length, any other media type
/// as the bytes behind the clip's URI; an intent and a repeated type are not
/// offered.
fn a_description_offers_its_media_types() {
    let session = identity_mime();
    let types = [
        "text/plain",
        "text/html",
        "text/vnd.android.intent",
        "text/plain",
        "image/png",
        "text/uri-list",
        session.as_str(),
    ];
    let offer = offered(types);
    let sources: Vec<&ClipSource> = offer.iter().map(|(source, _)| source).collect();
    assert_eq!(
        sources,
        [
            &ClipSource::Text,
            &ClipSource::Html,
            &ClipSource::UriBytes,
            &ClipSource::UriList,
            &ClipSource::Identity,
        ]
    );
    assert_eq!(
        offer[2].1.format,
        TransferFormat::Image {
            mime: Mime::new("image/png")
        }
    );
    assert_eq!(offer[4].1.declared_len, Some(0));
    assert_eq!(
        offer[4].1.format,
        identity().format,
        "the identity survives the description, parameters and all"
    );
}

/// Each representation resolves from what the view read of the held clip:
/// text and HTML from their UTF-8, the URIs as a `text/uri-list`, a URI's
/// bytes as the format they were announced as (an image as the bytes of its
/// container, another type as raw bytes of its essence), and the session
/// identity from its descriptor alone, with nothing read.
fn a_read_of_the_held_clip_resolves_as_announced() {
    let offer = offered([
        "text/plain",
        "text/html",
        "image/png",
        "text/uri-list",
        "application/pdf; q=1",
    ]);
    let limits = TransferLimits::default();
    let resolve = |at: usize, read: ViewRead| {
        let (source, descriptor) = &offer[at];
        delivered(*source, descriptor, read, limits)
    };
    assert!(matches!(
        resolve(0, ViewRead::Bytes("héllo".as_bytes().to_vec())),
        Ok(TransferPayload::Text(text)) if text == "héllo"
    ));
    assert!(matches!(
        resolve(1, ViewRead::Bytes(b"<b>hi</b>".to_vec())),
        Ok(TransferPayload::Html(html)) if html == "<b>hi</b>"
    ));
    assert!(
        matches!(
            resolve(2, ViewRead::Bytes(vec![1, 2, 3])),
            Ok(TransferPayload::Custom { mime, bytes })
                if mime.as_str() == "image/png" && *bytes == [1, 2, 3]
        ),
        "an image offer resolves as the bytes of the image it announced"
    );
    assert!(matches!(
        resolve(3, ViewRead::Uris(vec![
            "content://media/1".to_owned(),
            "content://media/2".to_owned(),
        ])),
        Ok(TransferPayload::Custom { mime, bytes })
            if mime.as_str() == "text/uri-list"
                && *bytes == *b"content://media/1\r\ncontent://media/2\r\n"
    ));
    assert!(matches!(
        resolve(4, ViewRead::Bytes(vec![4])),
        Ok(TransferPayload::Custom { mime, bytes })
            if mime.as_str() == "application/pdf" && *bytes == [4]
    ));
    let session = identity();
    assert!(matches!(
        delivered(ClipSource::Identity, &session, ViewRead::Stopped { read: 0 }, limits),
        Ok(TransferPayload::Custom { mime, bytes })
            if TransferFormat::Custom { mime: mime.clone() } == session.format && bytes.is_empty()
    ));
}

/// A read the view stopped past the limit is too large, with the length it
/// counted; one with nothing to read (the clip did not carry it, the drop was
/// let go of, the provider failed) is gone; a read within the view's count but
/// past the limit once delivered is still too large; text that is not UTF-8
/// does not decode.
fn a_read_of_the_held_clip_is_held_to_its_limits() {
    let limit = |max_bytes| TransferLimits::default().with_max_bytes(max_bytes);
    let text = RepresentationDescriptor {
        format: TransferFormat::Text,
        declared_len: None,
    };
    let list = RepresentationDescriptor {
        format: TransferFormat::UriList,
        declared_len: None,
    };
    assert!(
        matches!(
            delivered(ClipSource::Text, &text, ViewRead::Bytes(b"ab".to_vec()), limit(2)),
            Ok(TransferPayload::Text(text)) if text == "ab"
        ),
        "a read of exactly the limit is admitted"
    );
    let over = delivered(
        ClipSource::UriBytes,
        &text,
        ViewRead::Stopped { read: 16_384 },
        limit(10),
    );
    assert!(
        matches!(
            over,
            Err(TransferError::TooLarge {
                actual: 16_384,
                limit: 10
            })
        ),
        "a read stopped past the limit is too large, not gone: {over:?}"
    );
    for read in [0, 3] {
        assert!(matches!(
            delivered(
                ClipSource::Text,
                &text,
                ViewRead::Stopped { read },
                limit(10)
            ),
            Err(TransferError::SourceGone)
        ));
    }
    assert!(
        matches!(
            delivered(
                ClipSource::Html,
                &text,
                ViewRead::Bytes(vec![0; 11]),
                limit(10)
            ),
            Err(TransferError::TooLarge {
                actual: 11,
                limit: 10
            })
        ),
        "what the view delivers is held to the limit again"
    );
    assert!(
        matches!(
            delivered(
                ClipSource::UriList,
                &list,
                ViewRead::Uris(vec!["content://a/1".to_owned()]),
                limit(14),
            ),
            Err(TransferError::TooLarge {
                actual: 15,
                limit: 14
            })
        ),
        "the list is held to the limit with its line ends"
    );
    assert!(matches!(
        delivered(
            ClipSource::Text,
            &text,
            ViewRead::Bytes(vec![0xff]),
            limit(10)
        ),
        Err(TransferError::Decode {
            format: TransferFormat::Text
        })
    ));
    assert!(
        matches!(
            delivered(
                ClipSource::UriList,
                &list,
                ViewRead::Uris(Vec::new()),
                limit(10)
            ),
            Err(TransferError::SourceGone)
        ),
        "a clip with no URI carries no list"
    );
}

/// Android has no drop effects: a drag from another app is a copy, and one
/// this process started permits what its source advertised.
fn a_foreign_drag_is_a_copy_and_ours_permits_what_it_advertised() {
    assert_eq!(incoming_allowed(None), TransferActions::COPY);
    assert_eq!(
        incoming_allowed(Some(TransferActions::MOVE | TransferActions::COPY)),
        TransferActions::MOVE | TransferActions::COPY
    );
}

/// A FLUI drag carries its text and HTML in one item and each content URI in
/// its own; a `file:` URI and bytes stay out, and the identity travels as a
/// media type of the description alone.
fn a_flui_drag_puts_what_a_clip_can_carry_in_its_clip() {
    let representations = [
        RepresentationDescriptor {
            format: TransferFormat::Text,
            declared_len: None,
        },
        RepresentationDescriptor {
            format: TransferFormat::Html,
            declared_len: None,
        },
        RepresentationDescriptor {
            format: TransferFormat::UriList,
            declared_len: None,
        },
        RepresentationDescriptor {
            format: TransferFormat::Image {
                mime: Mime::new("image/png"),
            },
            declared_len: None,
        },
        identity(),
    ];
    let payloads = [
        Some(TransferPayload::Text("hi".to_owned())),
        Some(TransferPayload::Html("<b>hi</b>".to_owned())),
        Some(TransferUri::list_payload(
            [
                TransferUri::Uri("content://doc/1".to_owned()),
                TransferUri::Uri("file:///sdcard/a.txt".to_owned()),
                TransferUri::Path("/sdcard/b.txt".into()),
            ],
            PathStyle::Posix,
        )),
        Some(TransferPayload::Custom {
            mime: Mime::new("image/png"),
            bytes: Arc::from(vec![0_u8; 4]),
        }),
        None,
    ];
    let parts = clip_parts(&representations, &payloads);
    assert_eq!(parts.text.as_deref(), Some("hi"));
    assert_eq!(parts.html.as_deref(), Some("<b>hi</b>"));
    assert_eq!(parts.uris, ["content://doc/1"]);
    assert_eq!(
        parts.mime_types,
        [
            "text/plain".to_owned(),
            "text/html".to_owned(),
            "text/uri-list".to_owned(),
            identity_mime(),
        ]
    );
    assert_eq!(parts.flags(), DRAG_FLAG_GLOBAL | DRAG_FLAG_GLOBAL_URI_READ);
    let text_only = clip_parts(&representations[..1], &payloads[..1]);
    assert_eq!(text_only.flags(), DRAG_FLAG_GLOBAL, "no URIs to grant");
}

/// The source hears a refused drag as cancelled, a drop on its own target as
/// the action that target took, and another app's drop as a copy when it may
/// be one.
fn the_source_learns_how_its_drag_ended() {
    let copy_move = TransferActions::COPY | TransferActions::MOVE;
    assert_eq!(source_end(false, None, copy_move), OsDragEnd::Cancelled);
    assert_eq!(
        source_end(true, Some(TransferActions::MOVE), copy_move),
        OsDragEnd::Dropped(TransferActions::MOVE)
    );
    assert_eq!(
        source_end(true, None, copy_move),
        OsDragEnd::Dropped(TransferActions::COPY)
    );
    assert_eq!(
        source_end(true, None, TransferActions::MOVE),
        OsDragEnd::Dropped(TransferActions::MOVE)
    );
}

#[test]
fn android_drag_mapping() {
    crate::table_test::run_table(
        "android_drag_mapping",
        &[
            (
                "a_description_offers_its_media_types",
                a_description_offers_its_media_types,
            ),
            (
                "a_read_of_the_held_clip_resolves_as_announced",
                a_read_of_the_held_clip_resolves_as_announced,
            ),
            (
                "a_read_of_the_held_clip_is_held_to_its_limits",
                a_read_of_the_held_clip_is_held_to_its_limits,
            ),
            (
                "a_foreign_drag_is_a_copy_and_ours_permits_what_it_advertised",
                a_foreign_drag_is_a_copy_and_ours_permits_what_it_advertised,
            ),
            (
                "a_flui_drag_puts_what_a_clip_can_carry_in_its_clip",
                a_flui_drag_puts_what_a_clip_can_carry_in_its_clip,
            ),
            (
                "the_source_learns_how_its_drag_ended",
                the_source_learns_how_its_drag_ended,
            ),
        ],
    );
}
