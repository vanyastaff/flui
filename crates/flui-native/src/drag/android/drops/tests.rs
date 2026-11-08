//! The Android drag and drop state (`drag::android::drops`, ADR-0211,
//! ADR-0247): the offers a drag over the view mints, the drops it takes,
//! their lazy sources and one-time conclusions, and drops that overlap. The
//! JNI side, clip reads and URI permissions run on a device.

use flui_foundation::DataTransferId;
use flui_platform::DragDropEvent;
use flui_platform::data_transfer::{
    DropFeedback, RepresentationIndex, TransferActions, TransferError, TransferLimits,
    TransferPayload,
};

use super::super::clip::ClipSource;
use super::{
    ACTION_DRAG_ENDED, ACTION_DRAG_ENTERED, ACTION_DRAG_LOCATION, ACTION_DRAG_STARTED, Drops,
};

const IMAGE: RepresentationIndex = RepresentationIndex(0);

fn image_types() -> [String; 1] {
    ["image/png".to_owned()]
}

fn key(id: DataTransferId) -> i64 {
    id.as_u64().cast_signed()
}

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

fn take_image(
    drops: &mut Drops,
    id: DataTransferId,
) -> Option<(DragDropEvent, Option<DataTransferId>)> {
    drops.take_drop(key(id), (0.0, 0.0), false, &image_types())
}

fn dropped_source(drops: &Drops, id: DataTransferId) -> ClipSource {
    match drops.request(id, IMAGE) {
        Ok((ClipSource::UriBytes, _)) => ClipSource::UriBytes,
        other => panic!("an image is read from its URI, not {other:?}"),
    }
}

/// An image drop retains only its keyed lazy source until conclusion; until
/// the drop its data is out of reach, and a repeated conclusion releases no
/// Java clip or permission grant twice.
fn a_content_uri_drop_releases_its_saved_clip_once() {
    let mut drops = Drops::default();
    let id = hovering_image(&mut drops);
    assert_eq!(drops.hover_key(), key(id));
    assert!(
        matches!(drops.request(id, IMAGE), Err(TransferError::SourceGone)),
        "before the drop the clip's data is out of reach"
    );

    let (event, taken) = take_image(&mut drops, id).expect("an offer hovers");
    assert!(matches!(
        event,
        DragDropEvent::Dropped { id: dropped, action: TransferActions::COPY, .. } if dropped == id
    ));
    assert_eq!(taken, Some(id));
    assert_eq!(dropped_source(&drops, id), ClipSource::UriBytes);
    assert!(matches!(
        drops.request(id, RepresentationIndex(1)),
        Err(TransferError::UnknownRepresentation { .. })
    ));

    assert!(drops.conclude(id), "a taken drop releases its saved clip");
    assert!(matches!(
        drops.request(id, IMAGE),
        Err(TransferError::StaleOffer(stale)) if stale == id
    ));
    assert!(
        !drops.conclude(id),
        "a concluded drop releases nothing again"
    );
}

/// A second drag drops while the first drop is still being read: each keeps
/// its own keyed source, and concluding either retires its offer alone.
fn overlapping_drops_conclude_independently() {
    let mut drops = Drops::default();
    let first = hovering_image(&mut drops);
    take_image(&mut drops, first).expect("the first drop");
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENDED, (0.0, 0.0), &[], false, &mut events));
    assert!(
        events.is_empty(),
        "a taken drop does not exit at the drag's end"
    );

    let second = hovering_image(&mut drops);
    assert_ne!(first, second, "each drag mints its own offer");
    take_image(&mut drops, second).expect("the second drop");

    assert_eq!(dropped_source(&drops, first), ClipSource::UriBytes);
    assert_eq!(dropped_source(&drops, second), ClipSource::UriBytes);

    assert!(drops.conclude(second));
    assert_eq!(
        dropped_source(&drops, first),
        ClipSource::UriBytes,
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
        .take_drop(key(id), (2.0, 3.0), false, &image_types())
        .expect("an offer hovers");
    assert!(matches!(event, DragDropEvent::Exited { id: exited } if exited == id));
    assert_eq!(taken, None);
    assert!(matches!(
        drops.request(id, IMAGE),
        Err(TransferError::StaleOffer(stale)) if stale == id
    ));
    assert!(!drops.conclude(id), "a refused drop has no clip to release");
    assert!(
        take_image(&mut drops, id).is_none(),
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
    drops
        .take_drop(key(id), (0.0, 0.0), true, &types)
        .expect("an offer hovers");
    let (source, descriptor) = drops
        .request(id, RepresentationIndex(0))
        .expect("the dropped text");
    let limits = TransferLimits::default().with_max_bytes(5);
    assert!(matches!(
        source.request(&descriptor, limits, |_, _| Ok(b"hello".to_vec())),
        Ok(TransferPayload::Text(text)) if text == "hello"
    ));
    assert_eq!(drops.take_local_drop(), Some(TransferActions::MOVE));
    assert_eq!(drops.take_local_drop(), None, "taken once");
}

/// Taking a large multi-item clip performs no item read or payload allocation.
/// Only the requested representation is materialized, once, afterwards.
fn a_large_multi_item_drop_is_not_materialized() {
    let mut drops = Drops::default();
    let types = ["text/plain".to_owned()];
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENTERED, (0.0, 0.0), &types, false, &mut events));
    let [DragDropEvent::Entered { offer, .. }] = events.as_slice() else {
        panic!("one text offer enters, not {events:?}");
    };
    let id = offer.id();
    drops.update_feedback(
        id,
        DropFeedback {
            accept: Some(TransferActions::COPY),
        },
    );
    let reads = std::cell::Cell::new(0);
    drops
        .take_drop(key(id), (0.0, 0.0), false, &types)
        .expect("an offer hovers");
    assert_eq!(reads.get(), 0, "the drop did not touch the clip's items");

    let (source, descriptor) = drops
        .request(id, RepresentationIndex(0))
        .expect("the dropped text");
    let limits = TransferLimits::default().with_max_bytes(200_000);
    let payload = source.request(&descriptor, limits, |_, _| {
        reads.set(reads.get() + 1);
        let mut text = String::with_capacity(100_000);
        for _ in 0..10_000 {
            text.push_str("0123456789");
        }
        Ok(text.into_bytes())
    });
    assert_eq!(reads.get(), 1);
    assert!(matches!(payload, Ok(TransferPayload::Text(text)) if text.len() == 100_000));
}

#[test]
fn android_drop_state() {
    crate::table_test::run_table(
        "android_drop_state",
        &[
            (
                "a_content_uri_drop_releases_its_saved_clip_once",
                a_content_uri_drop_releases_its_saved_clip_once,
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
            (
                "a_large_multi_item_drop_is_not_materialized",
                a_large_multi_item_drop_is_not_materialized,
            ),
        ],
    );
}
