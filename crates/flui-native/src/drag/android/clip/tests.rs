//! The decidable half of the Android backend's drag and drop
//! (`drag::android::clip`, ADR-0247, ADR-0211): what a `DragEvent`'s clip
//! offers, what a bounded request reads, what a FLUI drag puts in its
//! `ClipData`, and how its source learns the end. The JNI side runs on a
//! device.

use std::num::NonZeroU64;
use std::sync::Arc;

use super::{
    ClipSource, DRAG_FLAG_GLOBAL, DRAG_FLAG_GLOBAL_URI_READ, clip_parts, drop_bytes,
    incoming_allowed, offered, source_end,
};
use flui_foundation::{DragSessionId, UiRuntimeId};
use flui_platform::data_transfer::{
    Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat, TransferLimits,
    TransferPayload,
};
use flui_platform::drag::{DragSessionKey, OsDragEnd, ProcessNonce};

use crate::drag::offers::{PathStyle, TransferUri};

fn limits(max_bytes: u64) -> TransferLimits {
    TransferLimits::default().with_max_bytes(max_bytes)
}

fn identity() -> RepresentationDescriptor {
    DragSessionKey::new(
        ProcessNonce::from_raw(NonZeroU64::new(7).expect("nonzero")),
        UiRuntimeId::new_gen(0, std::num::NonZeroU32::MIN),
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

/// The common intake policy bounds source entries and name lengths before
/// Android maps formats, including an endless stream of duplicate names.
fn a_description_is_bounded_before_format_mapping() {
    let unique: Vec<String> = (0..101)
        .map(|index| format!("application/x-{index}"))
        .collect();
    let mut reads = 0;
    let offer = offered(unique.iter().map(String::as_str).inspect(|_| reads += 1));
    assert_eq!(reads, 100, "the 101st source descriptor is not read");
    assert_eq!(offer.len(), 100);
    assert!(
        matches!(&offer[99].1.format, TransferFormat::Custom { mime } if mime.as_str() == "application/x-99")
    );

    let mime_of_len = |len| {
        const PREFIX: &str = "application/x-";
        assert!(len >= PREFIX.len());
        format!("{PREFIX}{}", "a".repeat(len - PREFIX.len()))
    };
    let longest = mime_of_len(255);
    let overlong = mime_of_len(256);
    let bounded = offered([longest.as_str(), overlong.as_str()]);
    assert_eq!(bounded.len(), 1, "only the 255-byte name is admitted");
    assert!(
        matches!(&bounded[0].1.format, TransferFormat::Custom { mime } if mime.as_str() == longest)
    );
    assert!(offered([overlong.as_str()]).is_empty());

    let mut endless_reads = 0;
    let endless = offered(std::iter::repeat("text/plain").inspect(|_| {
        endless_reads += 1;
        assert!(endless_reads <= 100, "an endless input is not bounded");
    }));
    assert_eq!(endless_reads, 100);
    assert_eq!(endless.len(), 1, "duplicates leave one representation");
    assert_eq!(endless[0].0, ClipSource::Text);
}

/// Each request reads only its own representation from the saved clip and
/// decodes it as announced; the identity needs no read at all.
fn a_request_reads_only_its_representation() {
    let offer = offered([
        "text/plain",
        "text/html",
        "image/png",
        "text/uri-list",
        "application/pdf; q=1",
        &identity_mime(),
    ]);
    let read = |source: ClipSource, limits: TransferLimits| {
        assert_eq!(limits.max_bytes, 1024);
        let bytes: &[u8] = match source {
            ClipSource::Text => "A😀é".as_bytes(),
            ClipSource::Html => b"<b>hello</b>",
            ClipSource::UriBytes => &[1, 2, 3],
            ClipSource::UriList => b"content://media/1\r\ncontent://media/2\r\n",
            ClipSource::Identity => panic!("the identity is already in its descriptor"),
        };
        Ok(bytes.to_vec())
    };
    assert!(matches!(
        offer[0].0.request(&offer[0].1, limits(1024), read),
        Ok(TransferPayload::Text(text)) if text == "A😀é"
    ));
    assert!(matches!(
        offer[1].0.request(&offer[1].1, limits(1024), read),
        Ok(TransferPayload::Html(html)) if html == "<b>hello</b>"
    ));
    assert!(matches!(
        offer[2].0.request(&offer[2].1, limits(1024), read),
        Ok(TransferPayload::Custom { mime, bytes })
            if mime.as_str() == "image/png" && *bytes == [1, 2, 3]
    ));
    assert!(matches!(
        offer[3].0.request(&offer[3].1, limits(1024), read),
        Ok(TransferPayload::Custom { mime, bytes })
            if mime.as_str() == "text/uri-list"
                && *bytes == b"content://media/1\r\ncontent://media/2\r\n"[..]
    ));
    assert!(matches!(
        offer[4].0.request(&offer[4].1, limits(1024), read),
        Ok(TransferPayload::Custom { mime, bytes })
            if mime.as_str() == "application/pdf" && *bytes == [1, 2, 3]
    ));
    let identity_reads = std::cell::Cell::new(0);
    assert!(matches!(
        offer[5].0.request(&offer[5].1, limits(0), |_, _| {
            identity_reads.set(identity_reads.get() + 1);
            Err(TransferError::SourceGone)
        }),
        Ok(TransferPayload::Custom { bytes, .. }) if bytes.is_empty()
    ));
    assert_eq!(identity_reads.get(), 0);
}

/// A missing representation is gone, and a read stopped past its byte limit is
/// too large rather than missing.
fn a_missing_or_bounded_request_reports_how_it_ended() {
    let (source, descriptor) = &offered(["text/plain"])[0];
    assert!(matches!(
        source.request(descriptor, limits(10), |_, limits| {
            drop_bytes(None, 0, limits)
        }),
        Err(TransferError::SourceGone)
    ));
    assert!(matches!(drop_bytes(Some(vec![1, 2]), 2, limits(4)), Ok(bytes) if bytes == [1, 2]));
    let over = drop_bytes(None, 11, limits(10));
    assert!(
        matches!(
            over,
            Err(TransferError::TooLarge {
                actual: 11,
                limit: 10
            })
        ),
        "a read stopped past the limit is too large, not gone: {over:?}"
    );
    assert!(matches!(
        drop_bytes(None, 3, limits(10)),
        Err(TransferError::SourceGone)
    ));
}

/// The limits count exact UTF-8 bytes, not UTF-16 units, and their boundary is
/// inclusive even when supplementary and multibyte scalars cross it.
fn a_request_enforces_the_exact_utf8_byte_limit() {
    let (source, descriptor) = &offered(["text/plain"])[0];
    let text = "A😀é";
    assert_eq!(text.len(), 7);
    assert!(matches!(
        source.request(descriptor, limits(7), |_, _| {
            Ok(text.as_bytes().to_vec())
        }),
        Ok(TransferPayload::Text(found)) if found == text
    ));
    assert!(matches!(
        source.request(descriptor, limits(6), |_, _| {
            Ok(text.as_bytes().to_vec())
        }),
        Err(TransferError::TooLarge {
            actual: 7,
            limit: 6
        })
    ));
    assert!(matches!(
        source.request(descriptor, limits(6), |_, limits| {
            drop_bytes(None, 7, limits)
        }),
        Err(TransferError::TooLarge {
            actual: 7,
            limit: 6
        })
    ));
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
                "a_description_is_bounded_before_format_mapping",
                a_description_is_bounded_before_format_mapping,
            ),
            (
                "a_request_reads_only_its_representation",
                a_request_reads_only_its_representation,
            ),
            (
                "a_missing_or_bounded_request_reports_how_it_ended",
                a_missing_or_bounded_request_reports_how_it_ended,
            ),
            (
                "a_request_enforces_the_exact_utf8_byte_limit",
                a_request_enforces_the_exact_utf8_byte_limit,
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
