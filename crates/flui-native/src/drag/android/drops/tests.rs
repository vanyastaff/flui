//! The Android drag and drop state (`drag::android::drops`, ADR-0211,
//! ADR-0247): the offers a drag over the view mints, the drops it takes and
//! what they hold, and drops that overlap. The JNI side, the content
//! resolver read and the URI permissions run on a device.

use flui_foundation::DataTransferId;
use flui_platform::DragDropEvent;
use flui_platform::data_transfer::{
    DropFeedback, RepresentationIndex, TransferActions, TransferError, TransferPayload,
};

use super::super::clip::{DroppedClip, Held};
use super::{
    ACTION_DRAG_ENDED, ACTION_DRAG_ENTERED, ACTION_DRAG_LOCATION, ACTION_DRAG_STARTED, Drops,
};

const IMAGE: RepresentationIndex = RepresentationIndex(0);

/// A drag from another app of one image enters the view and its target
/// accepts it: the offer it minted.
fn hovering_image(drops: &mut Drops) -> DataTransferId {
    let types = ["image/png".to_owned()];
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_STARTED, (0.0, 0.0), &types, false, &mut events));
    assert!(drops.drag(ACTION_DRAG_ENTERED, (0.0, 0.0), &[], false, &mut events));
    let [DragDropEvent::Entered { offer, allowed, .. }] = events.as_slice() else {
        panic!("one offer enters, not {events:?}");
    };
    assert_eq!(*allowed, TransferActions::COPY, "a foreign drag is a copy");
    let id = offer.id();
    drops.update_feedback(
        id,
        DropFeedback {
            accept: Some(TransferActions::COPY),
        },
    );
    id
}

fn image_clip(uri: &str) -> DroppedClip {
    DroppedClip {
        text: None,
        html: None,
        uris: vec![uri.to_owned()],
    }
}

fn dropped_uri(drops: &Drops, id: DataTransferId) -> String {
    match drops.held(id, IMAGE) {
        Ok(Held::Uri(held)) => held.uri,
        other => panic!("an image is read from its URI, not {other:?}"),
    }
}

/// An image dropped from another app's content provider holds its URI until
/// the target concludes the drop; until the drop the bytes are out of reach,
/// and after the conclusion the offer is stale and nothing is released twice.
fn a_content_uri_drop_holds_its_uri_until_concluded() {
    let mut drops = Drops::default();
    let id = hovering_image(&mut drops);
    assert!(
        matches!(drops.held(id, IMAGE), Err(TransferError::SourceGone)),
        "before the drop the clip's data is out of reach"
    );

    let uri = "content://dev.flui.test.provider/image/1";
    let (event, taken) = drops
        .take_drop((4.0, 8.0), false, &image_clip(uri))
        .expect("an offer hovers");
    assert!(matches!(
        event,
        DragDropEvent::Dropped { id: dropped, action: TransferActions::COPY, .. } if dropped == id
    ));
    assert_eq!(taken, Some(id));
    assert_eq!(dropped_uri(&drops, id), uri);
    assert!(matches!(
        drops.held(id, RepresentationIndex(1)),
        Err(TransferError::UnknownRepresentation { .. })
    ));

    assert!(drops.conclude(id), "a live drop releases its permissions");
    assert!(matches!(
        drops.held(id, IMAGE),
        Err(TransferError::StaleOffer(stale)) if stale == id
    ));
    assert!(
        !drops.conclude(id),
        "a concluded drop releases nothing again"
    );
}

/// A second drag drops while the first drop is still being read: each holds
/// its own URI, and concluding either retires its offer alone.
fn overlapping_drops_conclude_independently() {
    let mut drops = Drops::default();
    let first = hovering_image(&mut drops);
    let first_uri = "content://dev.flui.test.provider/image/1";
    drops
        .take_drop((0.0, 0.0), false, &image_clip(first_uri))
        .expect("the first drop");
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENDED, (0.0, 0.0), &[], false, &mut events));
    assert!(
        events.is_empty(),
        "a taken drop does not exit at the drag's end"
    );

    let second = hovering_image(&mut drops);
    assert_ne!(first, second, "each drag mints its own offer");
    let second_uri = "content://dev.flui.test.provider/image/2";
    drops
        .take_drop((0.0, 0.0), false, &image_clip(second_uri))
        .expect("the second drop");

    assert_eq!(dropped_uri(&drops, first), first_uri);
    assert_eq!(dropped_uri(&drops, second), second_uri);

    assert!(drops.conclude(second));
    assert_eq!(
        dropped_uri(&drops, first),
        first_uri,
        "the earlier drop is still readable"
    );
    assert!(drops.conclude(first));
    assert!(!drops.conclude(second));
}

/// A drop the target did not accept is refused: the offer exits and retires,
/// and the next drag is unaffected.
fn a_refused_drop_exits_and_holds_nothing() {
    let mut drops = Drops::default();
    let types = ["image/png".to_owned()];
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENTERED, (0.0, 0.0), &types, false, &mut events));
    assert!(drops.drag(ACTION_DRAG_LOCATION, (2.0, 3.0), &[], false, &mut events));
    let id = match events.as_slice() {
        [
            DragDropEvent::Entered { offer, .. },
            DragDropEvent::Moved { id, .. },
        ] => {
            assert_eq!(offer.id(), *id);
            *id
        }
        other => panic!("an offer enters and moves, not {other:?}"),
    };
    let (event, taken) = drops
        .take_drop((2.0, 3.0), false, &image_clip("content://x/1"))
        .expect("an offer hovers");
    assert!(matches!(event, DragDropEvent::Exited { id: exited } if exited == id));
    assert_eq!(taken, None);
    assert!(matches!(
        drops.held(id, IMAGE),
        Err(TransferError::StaleOffer(stale)) if stale == id
    ));
    assert!(
        drops
            .take_drop((0.0, 0.0), false, &image_clip("content://x/1"))
            .is_none(),
        "nothing hovers any more"
    );
}

/// FLUI's own drag dropped on its own target reports the action the target
/// took; a foreign drop while FLUI's drag runs does not.
fn our_own_drop_reports_its_action() {
    let mut drops = Drops::default();
    drops.set_own(Some(TransferActions::COPY | TransferActions::MOVE));
    let types = ["text/plain".to_owned()];
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENTERED, (0.0, 0.0), &types, true, &mut events));
    let [DragDropEvent::Entered { offer, allowed, .. }] = events.as_slice() else {
        panic!("one offer enters, not {events:?}");
    };
    assert_eq!(*allowed, TransferActions::COPY | TransferActions::MOVE);
    let id = offer.id();
    drops.update_feedback(
        id,
        DropFeedback {
            accept: Some(TransferActions::MOVE),
        },
    );
    let clip = DroppedClip {
        text: Some("hello".to_owned()),
        html: None,
        uris: Vec::new(),
    };
    drops
        .take_drop((0.0, 0.0), true, &clip)
        .expect("an offer hovers");
    assert!(matches!(
        drops.held(id, RepresentationIndex(0)),
        Ok(Held::Payload(TransferPayload::Text(text))) if text == "hello"
    ));
    assert_eq!(drops.take_local_drop(), Some(TransferActions::MOVE));
    assert_eq!(drops.take_local_drop(), None, "taken once");
}

#[test]
fn android_drop_state() {
    crate::table_test::run_table(
        "android_drop_state",
        &[
            (
                "a_content_uri_drop_holds_its_uri_until_concluded",
                a_content_uri_drop_holds_its_uri_until_concluded,
            ),
            (
                "overlapping_drops_conclude_independently",
                overlapping_drops_conclude_independently,
            ),
            (
                "a_refused_drop_exits_and_holds_nothing",
                a_refused_drop_exits_and_holds_nothing,
            ),
            (
                "our_own_drop_reports_its_action",
                our_own_drop_reports_its_action,
            ),
        ],
    );
}
