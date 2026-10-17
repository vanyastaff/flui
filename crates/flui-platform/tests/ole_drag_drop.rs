//! The decidable half of the Win32 OLE drag and drop backend: format mapping,
//! the bytes each clipboard format carries, `DROPEFFECT` mapping, when the
//! drop source ends the modal loop and how often that loop asks for frames.
//! The COM shell around it runs only on Windows (`platforms::windows::drag`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use flui_platform::shared::ole_drag_drop::{
    DragContinuation, MODAL_FRAME_PACE_MS, NativeFormat, actions_of, answered_action,
    continue_drag, decode, effect_bits, encode, encode_file_drop, encode_html, encode_text,
    identity_descriptor, modal_frame_wait_ms, native_format, offer_representations,
    transfer_format,
};
use flui_platform_api::data_transfer::{
    DropFeedback, Mime, TransferActions, TransferError, TransferFormat, TransferPayload,
    TransferUri,
};
use flui_platform_api::drag::IDENTITY_ESSENCE;

const COPY: TransferActions = TransferActions::COPY;
const MOVE: TransferActions = TransferActions::MOVE;
const LINK: TransferActions = TransferActions::LINK;

/// Actions and `DROPEFFECT` bits map both ways; a target's answer is one
/// action the source permits, or a refusal.
fn effects_map_both_ways_and_an_answer_is_one_permitted_action() {
    for actions in [COPY, MOVE, LINK, COPY | MOVE, COPY | MOVE | LINK] {
        assert_eq!(actions_of(effect_bits(actions)), actions);
    }
    // DROPEFFECT_SCROLL and unknown bits are no action.
    assert_eq!(actions_of(0x8000_0000 | 0x08), TransferActions::empty());
    let accept = |accept| DropFeedback { accept };
    assert_eq!(
        answered_action(accept(Some(COPY | MOVE)), MOVE | LINK),
        Some(MOVE)
    );
    assert_eq!(
        answered_action(accept(Some(COPY | MOVE)), COPY | MOVE),
        Some(COPY)
    );
    assert_eq!(answered_action(accept(Some(COPY)), MOVE), None);
    assert_eq!(answered_action(accept(None), COPY), None);
}

/// Each transfer format has one clipboard format; a foreign clipboard format
/// reads back as the transfer format it carries, and names that are not media
/// types are not announced.
fn formats_map_to_clipboard_formats_and_back() {
    let rows = [
        (TransferFormat::Text, NativeFormat::UnicodeText),
        (TransferFormat::Html, NativeFormat::Html),
        (TransferFormat::UriList, NativeFormat::FileDrop),
        (
            TransferFormat::Image {
                mime: Mime::new("image/png"),
            },
            NativeFormat::Named("PNG".into()),
        ),
        (
            TransferFormat::Image {
                mime: Mime::new("image/webp"),
            },
            NativeFormat::Named("image/webp".into()),
        ),
        (
            TransferFormat::Custom {
                mime: Mime::new("application/x-kanban-card"),
            },
            NativeFormat::Named("application/x-kanban-card".into()),
        ),
    ];
    for (format, native) in rows {
        assert_eq!(native_format(&format), native, "{format:?}");
        assert_eq!(transfer_format(&native), Some(format));
    }
    // The identity travels under its bare type; its parameters ride in the data.
    let identity = TransferFormat::Custom {
        mime: Mime::new(&format!(
            "{IDENTITY_ESSENCE};process=1;runtime=0.1;session=2"
        )),
    };
    let native = native_format(&identity);
    assert_eq!(native, NativeFormat::Named(IDENTITY_ESSENCE.into()));
    assert_eq!(
        transfer_format(&native),
        None,
        "read by identity_descriptor"
    );
    for name in [
        "Shell IDList Array",
        "FileGroupDescriptorW",
        "text/",
        "a b/c",
    ] {
        assert_eq!(
            transfer_format(&NativeFormat::Named(name.into())),
            None,
            "{name}"
        );
    }
}

/// A foreign data object's formats are announced in its order, once per
/// transfer format, with the identity last when it carries a well-formed one.
fn an_offer_lists_each_format_once_then_the_identity() {
    let full = format!("{IDENTITY_ESSENCE};process=9;runtime=1.1;session=3");
    let identity = identity_descriptor(format!("{full}\0").as_bytes()).expect("an identity");
    let offered = offer_representations(
        [
            NativeFormat::Named("Shell IDList Array".into()),
            NativeFormat::FileDrop,
            NativeFormat::UnicodeText,
            NativeFormat::Named("text/plain".into()),
            NativeFormat::FileDrop,
            NativeFormat::Named(IDENTITY_ESSENCE.into()),
        ],
        Some(identity),
    );
    let formats: Vec<_> = offered
        .iter()
        .map(|(_, descriptor)| descriptor.format.clone())
        .collect();
    assert_eq!(
        formats,
        [
            TransferFormat::UriList,
            TransferFormat::Text,
            TransferFormat::Custom {
                mime: Mime::new("text/plain")
            },
            TransferFormat::Custom {
                mime: Mime::new(&full)
            },
        ]
    );
    assert!(identity_descriptor(b"application/x-other;session=3").is_none());
    assert!(identity_descriptor(&[0xff, 0xfe]).is_none());
}

/// What a source serves in each format reads back as the same payload.
fn payloads_round_trip_through_their_clipboard_formats() {
    let path = PathBuf::from(r"C:\Users\flui\board – ✓.txt");
    let rows = [
        (
            TransferFormat::Text,
            TransferPayload::Text("drag 🙂 me".into()),
        ),
        (
            TransferFormat::Html,
            TransferPayload::Html("<p>dragged — <b>bold</b></p>".into()),
        ),
        (
            TransferFormat::UriList,
            TransferPayload::UriList(vec![TransferUri::Path(path)]),
        ),
        (
            TransferFormat::Custom {
                mime: Mime::new("application/x-card"),
            },
            TransferPayload::Custom {
                mime: Mime::new("application/x-card"),
                bytes: Arc::from(&[0_u8, 1, 255][..]),
            },
        ),
    ];
    for (format, payload) in rows {
        let native = native_format(&format);
        let bytes = encode(&native, &payload).expect("the payload fits its format");
        let back = decode(&native, &format, &bytes).expect("decodes");
        assert_eq!(format!("{back:?}"), format!("{payload:?}"), "{format:?}");
    }
    // A URI list with no local path has nothing CF_HDROP can carry.
    let remote = TransferPayload::UriList(vec![TransferUri::Uri("https://flui.dev".into())]);
    assert!(encode(&NativeFormat::FileDrop, &remote).is_none());
    // A payload of another kind than its format is not served.
    assert!(
        encode(
            &NativeFormat::UnicodeText,
            &TransferPayload::Html("x".into())
        )
        .is_none()
    );
}

/// Foreign allocations: text with no terminator ends at the allocation,
/// CF_HTML is read between its fragment offsets (or its HTML offsets), ANSI
/// file lists are read, and malformed bytes fail to decode.
fn foreign_allocations_decode_within_their_bounds() {
    let text: Vec<u8> = "abc".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let decoded = decode(&NativeFormat::UnicodeText, &TransferFormat::Text, &text);
    assert!(matches!(decoded, Ok(TransferPayload::Text(text)) if text == "abc"));
    let mut padded = encode_text("hi");
    padded.extend_from_slice(&[0x41, 0, 0x42, 0]);
    let decoded = decode(&NativeFormat::UnicodeText, &TransferFormat::Text, &padded);
    assert!(matches!(decoded, Ok(TransferPayload::Text(text)) if text == "hi"));
    let lone_surrogate = [0x00, 0xd8, 0, 0];
    assert!(matches!(
        decode(
            &NativeFormat::UnicodeText,
            &TransferFormat::Text,
            &lone_surrogate
        ),
        Err(TransferError::Decode { .. })
    ));

    let html = encode_html("<i>x</i>");
    let header = String::from_utf8_lossy(&html[..html.len().min(105)]).into_owned();
    assert!(header.starts_with("Version:0.9\r\nStartHTML:"), "{header}");
    let without_fragment = b"Version:0.9\r\nStartHTML:0000000055\r\nEndHTML:0000000063\r\n<i>y</i>";
    let decoded = decode(&NativeFormat::Html, &TransferFormat::Html, without_fragment);
    assert!(
        matches!(&decoded, Ok(TransferPayload::Html(html)) if html == "<i>y</i>"),
        "{decoded:?}"
    );
    let out_of_range = b"Version:0.9\r\nStartFragment:0000009999\r\nEndFragment:0000099999\r\n";
    assert!(decode(&NativeFormat::Html, &TransferFormat::Html, out_of_range).is_err());

    let mut ansi = Vec::new();
    ansi.extend_from_slice(&20_u32.to_le_bytes());
    ansi.extend_from_slice(&[0; 12]);
    ansi.extend_from_slice(&0_i32.to_le_bytes());
    ansi.extend_from_slice(b"C:\\a.txt\0C:\\b.txt\0\0");
    let decoded = decode(&NativeFormat::FileDrop, &TransferFormat::UriList, &ansi);
    assert!(
        matches!(&decoded, Ok(TransferPayload::UriList(uris)) if *uris == [
            TransferUri::Path(r"C:\a.txt".into()),
            TransferUri::Path(r"C:\b.txt".into()),
        ]),
        "{decoded:?}"
    );
    let wide = encode_file_drop(&[std::path::Path::new(r"C:\c.txt")]);
    assert!(
        decode(
            &NativeFormat::FileDrop,
            &TransferFormat::UriList,
            &wide[..10]
        )
        .is_err()
    );
}

/// The drop source ends the loop with a drop when the buttons that began the
/// drag are released, and cancels on Escape, on another button, or when no
/// button began it.
fn the_drop_source_drops_on_release_and_cancels_otherwise() {
    const LEFT: u32 = 0x01;
    const RIGHT: u32 = 0x02;
    const CONTROL: u32 = 0x08;
    let rows = [
        (false, LEFT, LEFT, DragContinuation::Continue),
        (false, LEFT | CONTROL, LEFT, DragContinuation::Continue),
        (false, CONTROL, LEFT, DragContinuation::Drop),
        (false, 0, RIGHT, DragContinuation::Drop),
        (true, LEFT, LEFT, DragContinuation::Cancel),
        (false, LEFT | RIGHT, LEFT, DragContinuation::Cancel),
        (false, 0, 0, DragContinuation::Cancel),
        (false, LEFT, 0, DragContinuation::Cancel),
    ];
    for (escape, keys, began, expected) in rows {
        assert_eq!(
            continue_drag(escape, keys, began),
            expected,
            "escape {escape} keys {keys:#x} began {began:#x}"
        );
    }
}

/// The modal loop asks for frames by the next deadline, at least every
/// frame and never in a busy loop.
fn the_modal_loop_waits_for_the_next_deadline_within_a_frame() {
    let now = web_time::Instant::now();
    let rows = [
        (None, MODAL_FRAME_PACE_MS),
        (Some(now), 1),
        (Some(now + Duration::from_micros(200)), 1),
        (Some(now + Duration::from_micros(4_200)), 5),
        (Some(now + Duration::from_secs(3)), MODAL_FRAME_PACE_MS),
    ];
    for (deadline, wait) in rows {
        assert_eq!(modal_frame_wait_ms(deadline, now), wait, "{deadline:?}");
    }
    let past = now
        .checked_sub(Duration::from_millis(5))
        .expect("past instant");
    assert_eq!(modal_frame_wait_ms(Some(past), now), 1);
}

#[test]
fn ole_drag_drop_contracts() {
    super::run_table(
        "ole_drag_drop",
        &[
            (
                "effects_map_both_ways_and_an_answer_is_one_permitted_action",
                effects_map_both_ways_and_an_answer_is_one_permitted_action,
            ),
            (
                "formats_map_to_clipboard_formats_and_back",
                formats_map_to_clipboard_formats_and_back,
            ),
            (
                "an_offer_lists_each_format_once_then_the_identity",
                an_offer_lists_each_format_once_then_the_identity,
            ),
            (
                "payloads_round_trip_through_their_clipboard_formats",
                payloads_round_trip_through_their_clipboard_formats,
            ),
            (
                "foreign_allocations_decode_within_their_bounds",
                foreign_allocations_decode_within_their_bounds,
            ),
            (
                "the_drop_source_drops_on_release_and_cancels_otherwise",
                the_drop_source_drops_on_release_and_cancels_otherwise,
            ),
            (
                "the_modal_loop_waits_for_the_next_deadline_within_a_frame",
                the_modal_loop_waits_for_the_next_deadline_within_a_frame,
            ),
        ],
    );
}
