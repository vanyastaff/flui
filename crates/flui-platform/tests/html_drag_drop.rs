//! The decidable half of the web backend's HTML drag and drop
//! (`shared::html_drag_drop`): effect names, what a hovering drag offers by
//! its `DataTransfer` types and files, and how payloads travel as strings.
//! The DOM side runs in Chrome (`tests/web_browser.rs`).

use std::cell::RefCell;
use std::num::{NonZeroU32, NonZeroU64};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::{DataTransferId, DragSessionId, UiRuntimeId};
use flui_platform::shared::html_drag_drop::{
    DomSource, ProviderPanicked, action_of, allowed_of, drop_effect, effect_allowed_of, format_of,
    offered, payload_of, payload_of_bytes, serve_stored, stored,
};
use flui_platform_api::data_transfer::{
    DataTransferOffer, DropFeedback, Mime, RepresentationDescriptor, RepresentationIndex,
    TransferActions, TransferError, TransferFormat, TransferPayload, TransferUri,
};
use flui_platform_api::drag::{DragSessionKey, OsDragProvider, ProcessNonce};

fn effect_names_round_trip_the_actions() {
    for actions in [
        TransferActions::empty(),
        TransferActions::COPY,
        TransferActions::MOVE,
        TransferActions::LINK,
        TransferActions::COPY | TransferActions::MOVE,
        TransferActions::COPY | TransferActions::LINK,
        TransferActions::LINK | TransferActions::MOVE,
        TransferActions::all(),
    ] {
        assert_eq!(
            allowed_of(effect_allowed_of(actions)),
            actions,
            "{actions:?}"
        );
    }
    // A drag from outside the page permits everything.
    assert_eq!(allowed_of("uninitialized"), TransferActions::all());
    assert_eq!(action_of("copy"), TransferActions::COPY);
    assert_eq!(action_of("none"), TransferActions::empty());
}

fn the_drop_effect_names_one_accepted_action_the_drag_permits() {
    let accept = |actions| DropFeedback {
        accept: Some(actions),
    };
    let both = TransferActions::COPY | TransferActions::MOVE;
    assert_eq!(drop_effect(accept(TransferActions::MOVE), both), "move");
    assert_eq!(drop_effect(accept(both), both), "copy", "one action");
    assert_eq!(
        drop_effect(accept(TransferActions::LINK), both),
        "none",
        "not permitted"
    );
    assert_eq!(
        drop_effect(DropFeedback::default(), both),
        "none",
        "declined"
    );
}

fn a_hover_offers_the_types_flui_announces_then_the_files() {
    let key = DragSessionKey::new(
        ProcessNonce::from_raw(NonZeroU64::new(7).expect("nonzero")),
        UiRuntimeId::new_gen(2, NonZeroU32::MIN),
        DragSessionId::from_wire(NonZeroU64::new(9).expect("nonzero")),
    );
    let identity = key.representation();
    let TransferFormat::Custom { mime } = &identity.format else {
        panic!("the identity is a custom type");
    };
    let identity_type = mime.as_str().to_owned();
    let types = [
        "text/plain",
        "chromium/x-drag-id",
        "text/uri-list",
        identity_type.as_str(),
        "not a media type",
        "application/x-app",
        "text/plain",
    ];
    let offer = offered(types, ["image/png", ""]);
    let sources: Vec<&DomSource> = offer.iter().map(|(source, _)| source).collect();
    assert_eq!(
        sources,
        [
            &DomSource::Data("text/plain".to_owned()),
            &DomSource::Data("text/uri-list".to_owned()),
            &DomSource::Data(identity_type.clone()),
            &DomSource::Data("application/x-app".to_owned()),
            &DomSource::File(0),
            &DomSource::File(1),
        ]
    );
    let formats: Vec<&TransferFormat> = offer.iter().map(|(_, known)| &known.format).collect();
    assert_eq!(formats[0], &TransferFormat::Text);
    assert_eq!(formats[1], &TransferFormat::UriList);
    assert_eq!(
        formats[4],
        &TransferFormat::Image {
            mime: Mime::new("image/png")
        }
    );
    assert_eq!(
        formats[5],
        &TransferFormat::Custom {
            mime: Mime::new("application/octet-stream")
        }
    );
    assert_eq!(offer[2].1.declared_len, Some(0), "the identity is empty");
    // The identity a hover reads names the session.
    let representations: Arc<[RepresentationDescriptor]> =
        offer.into_iter().map(|(_, known)| known).collect();
    let offer =
        DataTransferOffer::new(DataTransferId::new_gen(0, NonZeroU32::MIN), representations);
    assert_eq!(DragSessionKey::from_offer(&offer), Some(key));
    assert_eq!(format_of("Files"), None);
}

fn payloads_travel_as_the_strings_the_browser_stores() {
    let uris = payload_of(
        &TransferFormat::UriList,
        "# comment\r\nhttps://a.example/\r\n\r\nfile:///tmp/x\r\n".to_owned(),
    );
    let TransferPayload::UriList(list) = &uris else {
        panic!("a uri list");
    };
    assert_eq!(
        list,
        &[
            TransferUri::Uri("https://a.example/".to_owned()),
            TransferUri::Uri("file:///tmp/x".to_owned())
        ]
    );
    let text = RepresentationDescriptor {
        format: TransferFormat::Text,
        declared_len: None,
    };
    assert_eq!(
        stored(&text, &TransferPayload::Text("hello".to_owned())),
        Some(("text/plain".to_owned(), "hello".to_owned()))
    );
    let custom = RepresentationDescriptor {
        format: TransferFormat::Custom {
            mime: Mime::new("application/x-app"),
        },
        declared_len: None,
    };
    let bytes = |raw: &[u8]| TransferPayload::Custom {
        mime: Mime::new("application/x-app"),
        bytes: Arc::from(raw),
    };
    assert_eq!(
        stored(&custom, &bytes(b"{}")),
        Some(("application/x-app".to_owned(), "{}".to_owned()))
    );
    assert_eq!(stored(&custom, &bytes(&[0xff, 0xfe])), None, "not a string");
    let uri_list = RepresentationDescriptor {
        format: TransferFormat::UriList,
        declared_len: None,
    };
    assert_eq!(
        stored(&uri_list, &uris).map(|(_, text)| text),
        Some("https://a.example/\r\nfile:///tmp/x".to_owned())
    );
}

/// A dropped file, and an image a page stored as a string, resolve as the
/// format they were announced as: an image for `image/*`, bytes otherwise.
fn dropped_images_resolve_as_the_image_they_announced() {
    let offer = offered(["image/svg+xml"], ["image/png", "application/pdf"]);
    let resolve =
        |index: usize, bytes: &[u8]| payload_of_bytes(&offer[index].1.format, bytes.to_vec());
    let TransferPayload::Image(png) = resolve(1, b"\x89PNG") else {
        panic!("a png file is an image");
    };
    assert_eq!(
        (png.mime.as_str(), &*png.bytes),
        ("image/png", &b"\x89PNG"[..])
    );
    let TransferPayload::Custom { mime, bytes } = resolve(2, b"%PDF") else {
        panic!("a pdf file is bytes");
    };
    assert_eq!((mime.as_str(), &*bytes), ("application/pdf", &b"%PDF"[..]));
    let TransferPayload::Image(svg) = payload_of(&offer[0].1.format, "<svg/>".to_owned()) else {
        panic!("an image string is an image");
    };
    assert_eq!(
        (svg.mime.as_str(), &*svg.bytes),
        ("image/svg+xml", &b"<svg/>"[..])
    );
}

/// `dragstart` stores what the provider serves; a refusal leaves one
/// representation out, a panic is contained and stops the serving, so the
/// source can end its session.
fn a_provider_panic_at_dragstart_is_contained_and_reported() {
    let text = RepresentationDescriptor {
        format: TransferFormat::Text,
        declared_len: None,
    };
    let html = RepresentationDescriptor {
        format: TransferFormat::Html,
        declared_len: None,
    };
    let representations = [text.clone(), html, text];
    let asked = Rc::new(RefCell::new(Vec::new()));
    let serving = |panics: bool| -> OsDragProvider {
        let asked = Rc::clone(&asked);
        Box::new(move |RepresentationIndex(index)| {
            asked.borrow_mut().push(index);
            match index {
                0 => Ok(TransferPayload::Text("dragged".to_owned())),
                1 if panics => panic!("a lazy payload panicked"),
                1 => Err(TransferError::SourceGone),
                _ => Ok(TransferPayload::Text("again".to_owned())),
            }
        })
    };
    assert_eq!(
        serve_stored(&representations, &mut serving(false)),
        Ok(vec![
            ("text/plain".to_owned(), "dragged".to_owned()),
            ("text/plain".to_owned(), "again".to_owned()),
        ]),
        "a refused representation is left out"
    );
    asked.borrow_mut().clear();
    assert_eq!(
        serve_stored(&representations, &mut serving(true)),
        Err(ProviderPanicked)
    );
    assert_eq!(*asked.borrow(), [0, 1], "nothing is asked after the panic");
}

#[test]
fn html_drag_drop_table() {
    super::run_table(
        "html_drag_drop_table",
        &[
            (
                "effect_names_round_trip_the_actions",
                effect_names_round_trip_the_actions,
            ),
            (
                "the_drop_effect_names_one_accepted_action_the_drag_permits",
                the_drop_effect_names_one_accepted_action_the_drag_permits,
            ),
            (
                "a_hover_offers_the_types_flui_announces_then_the_files",
                a_hover_offers_the_types_flui_announces_then_the_files,
            ),
            (
                "payloads_travel_as_the_strings_the_browser_stores",
                payloads_travel_as_the_strings_the_browser_stores,
            ),
            (
                "dropped_images_resolve_as_the_image_they_announced",
                dropped_images_resolve_as_the_image_they_announced,
            ),
            (
                "a_provider_panic_at_dragstart_is_contained_and_reported",
                a_provider_panic_at_dragstart_is_contained_and_reported,
            ),
        ],
    );
}
