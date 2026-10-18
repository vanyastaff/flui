//! The decidable half of the Android backend's drag and drop
//! (`shared::android_drag`, ADR-0244, ADR-0211): what a `DragEvent`'s clip
//! offers, what a drop holds and answers, what a FLUI drag puts in its
//! `ClipData`, and how its source learns the end. The JNI side runs on a
//! device (`cargo xtask device android-input`).

use std::num::NonZeroU64;
use std::sync::Arc;

use flui_foundation::{DragSessionId, UiRuntimeId};
use flui_platform::shared::android_drag::{
    ClipSource, DRAG_FLAG_GLOBAL, DRAG_FLAG_GLOBAL_URI_READ, DroppedClip, Held, clip_parts,
    drop_action, held, incoming_allowed, offered, source_end,
};
use flui_platform_api::data_transfer::{
    DropFeedback, Mime, RepresentationDescriptor, TransferActions, TransferFormat, TransferPayload,
    TransferUri,
};
use flui_platform_api::drag::{DragSessionKey, OsDragEnd, ProcessNonce};

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

/// The drop holds the clip's text, HTML and URIs in memory, and the bytes of
/// a typed representation as its first URI, read when requested; what the
/// clip did not carry is missing.
fn a_drop_holds_what_the_clip_carried() {
    let offer = offered(["text/plain", "text/html", "image/png", "text/uri-list"]);
    let clip = DroppedClip {
        text: Some("hello".to_owned()),
        html: None,
        uris: vec![
            "content://media/1".to_owned(),
            "content://media/2".to_owned(),
        ],
    };
    let holds: Vec<Held> = offer
        .iter()
        .map(|(source, descriptor)| held(source, descriptor, &clip))
        .collect();
    assert!(matches!(&holds[0], Held::Payload(TransferPayload::Text(text)) if text == "hello"));
    assert!(matches!(holds[1], Held::Missing), "no HTML");
    assert!(
        matches!(&holds[2], Held::Uri { uri, mime } if uri == "content://media/1" && mime == "image/png")
    );
    assert!(matches!(
        &holds[3],
        Held::Payload(TransferPayload::UriList(uris)) if uris.len() == 2
            && matches!(&uris[1], TransferUri::Uri(uri) if uri == "content://media/2")
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

/// The drop's answer is the target's cached feedback, narrowed to what the
/// drag permits; none refuses it.
fn the_drop_takes_the_action_the_target_cached() {
    let accept = |actions| DropFeedback {
        accept: Some(actions),
    };
    assert_eq!(
        drop_action(accept(TransferActions::COPY), TransferActions::COPY),
        Some(TransferActions::COPY)
    );
    assert_eq!(
        drop_action(accept(TransferActions::MOVE), TransferActions::COPY),
        None,
        "a move a copy-only drag does not permit"
    );
    assert_eq!(
        drop_action(DropFeedback::default(), TransferActions::COPY),
        None
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
        Some(TransferPayload::UriList(vec![
            TransferUri::Uri("content://doc/1".to_owned()),
            TransferUri::Uri("file:///sdcard/a.txt".to_owned()),
            TransferUri::Path("/sdcard/b.txt".into()),
        ])),
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
    super::run_table(
        "android_drag_mapping",
        &[
            (
                "a_description_offers_its_media_types",
                a_description_offers_its_media_types,
            ),
            (
                "a_drop_holds_what_the_clip_carried",
                a_drop_holds_what_the_clip_carried,
            ),
            (
                "a_foreign_drag_is_a_copy_and_ours_permits_what_it_advertised",
                a_foreign_drag_is_a_copy_and_ours_permits_what_it_advertised,
            ),
            (
                "the_drop_takes_the_action_the_target_cached",
                the_drop_takes_the_action_the_target_cached,
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
