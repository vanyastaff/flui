//! The decidable half of the Android backend's drag and drop
//! (`drag::android::clip`, ADR-0348, ADR-0211): what a `DragEvent`'s clip
//! offers, how a read of the held clip answers a request, what a FLUI drag
//! puts in its `ClipData`, and how its source learns the end. The JNI side
//! runs on a device.

use std::num::NonZeroU64;
use std::sync::Arc;

use super::{
    ClipSource, DRAG_FLAG_GLOBAL, DRAG_FLAG_GLOBAL_URI_READ, TypeNames, ViewRead, clip_parts,
    delivered, incoming_allowed, offered, read_type_names, source_end,
};
use flui_foundation::{DragSessionId, UiRuntimeId};
use flui_platform::transfer::data_transfer::{
    Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat, TransferLimits,
    TransferPayload,
};
use flui_platform::transfer::drag::{DragSessionKey, OsDragEnd, ProcessNonce};

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

/// The `String[]` another application's description filled, as JNI hands
/// it over: each element read makes a local reference, which the stand-in
/// counts while it lives, and each copy is counted.
struct Elements {
    names: Vec<Option<String>>,
    live: usize,
    most_live: usize,
    asked: usize,
}

impl Elements {
    fn new(names: Vec<Option<String>>) -> Self {
        Self {
            names,
            live: 0,
            most_live: 0,
            asked: 0,
        }
    }
}

impl TypeNames for Elements {
    type Element = String;
    type Error = std::convert::Infallible;

    fn len(&mut self) -> Result<usize, Self::Error> {
        Ok(self.names.len())
    }

    fn element(&mut self, index: usize) -> Result<Option<String>, Self::Error> {
        self.asked += 1;
        let element = self.names[index].clone();
        if element.is_some() {
            self.live += 1;
            self.most_live = self.most_live.max(self.live);
        }
        Ok(element)
    }

    fn text(&mut self, element: &String) -> Result<String, Self::Error> {
        Ok(element.clone())
    }

    fn release(&mut self, _element: String) {
        self.live -= 1;
    }
}

/// A drag from another application names any number of media types, of any
/// length: its offer is read from the first `MAX_OFFERED_TYPES` alone, an
/// over-long one among them left out and counted, a `null` skipped, and one
/// element's local reference lives at a time, so the native frame stays
/// bounded however many it named.
fn a_drag_offers_no_more_types_than_every_host_reads() {
    use crate::drag::offers::{MAX_OFFERED_TYPES, MAX_TYPE_LEN};

    let long = format!("application/{}", "x".repeat(MAX_TYPE_LEN));
    let mut names: Vec<Option<String>> = vec![Some(long.clone()), None];
    names.extend((0..10_000).map(|n| Some(format!("application/x-{n}"))));
    let mut elements = Elements::new(names);
    let read = read_type_names(&mut elements).expect("an infallible array");
    assert_eq!(
        elements.asked, MAX_OFFERED_TYPES,
        "nothing past the bound is read"
    );
    assert_eq!(elements.most_live, 1, "one local reference at a time");
    assert_eq!(read.len(), MAX_OFFERED_TYPES - 2);
    assert!(!read.contains(&long), "an over-long type is left out");

    // `FluiView` copies no more across than the library reads.
    let view = crate::owner::android::java_host::java_source("dev.flui.platform.FluiView");
    for stated in [
        format!("static final int MAX_OFFERED_TYPES = {MAX_OFFERED_TYPES};"),
        format!("static final int MAX_TYPE_LENGTH = {MAX_TYPE_LEN};"),
        "Math.min(description.getMimeTypeCount(), MAX_OFFERED_TYPES)".to_owned(),
    ] {
        assert!(view.contains(&stated), "{stated:?} in FluiView");
    }

    // The model holds the same rule over what it is handed.
    let many: Vec<String> = (0..10_000).map(|n| format!("application/x-{n}")).collect();
    let offer = offered(std::iter::once(long.as_str()).chain(many.iter().map(String::as_str)));
    assert_eq!(offer.len(), MAX_OFFERED_TYPES - 1);
    assert_eq!(
        offer
            .last()
            .map(|(_, descriptor)| descriptor.format.clone()),
        Some(TransferFormat::Custom {
            mime: Mime::new(&format!("application/x-{}", MAX_OFFERED_TYPES - 2))
        })
    );
}

/// `FluiView.readDropBytes` asks the size the provider states before it
/// reads, refuses one past the request's limit with that size counted (the
/// model reads it as `TooLarge`), and only then opens the stream, which
/// still stops at the limit.
fn a_dropped_uri_is_refused_on_its_stated_size_before_it_is_read() {
    let view = crate::owner::android::java_host::java_source("dev.flui.platform.FluiView");
    let at = |needle: &str| {
        view.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} in FluiView"))
    };
    let stated = at("long declared = file.getLength();");
    let refused = at("if (declared > limit) {");
    let counted = at("read[0] = declared;");
    let opened = at("stream = file.createInputStream();");
    let stops = at("if (read[0] > limit) {");
    assert!(
        stated < refused && refused < counted && counted < opened && opened < stops,
        "in that order"
    );

    let descriptor = RepresentationDescriptor {
        format: TransferFormat::Image {
            mime: Mime::new("image/png"),
        },
        declared_len: None,
    };
    let limits = TransferLimits::default().with_max_bytes(10);
    assert!(matches!(
        delivered(
            ClipSource::UriBytes,
            &descriptor,
            ViewRead::Stopped { read: 1 << 40 },
            limits
        ),
        Err(TransferError::TooLarge { .. })
    ));
}

#[test]
fn android_drag_mapping() {
    crate::table_test::run_table(
        "android_drag_mapping",
        &[
            (
                "a_dropped_uri_is_refused_on_its_stated_size_before_it_is_read",
                a_dropped_uri_is_refused_on_its_stated_size_before_it_is_read,
            ),
            (
                "a_drag_offers_no_more_types_than_every_host_reads",
                a_drag_offers_no_more_types_than_every_host_reads,
            ),
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
