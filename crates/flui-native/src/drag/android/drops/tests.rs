//! The Android drag and drop state (`drag::android::drops`, ADR-0211,
//! ADR-0247): the offers a drag over the view mints, the drops it takes and
//! where their representations are read from, the reads of URIs' bytes and
//! their places, and drops that overlap. The JNI side, the view's reads of the
//! held clip and the URI permissions run on a device.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use flui_foundation::DataTransferId;
use flui_platform::DragDropEvent;
use flui_platform::data_transfer::{
    DropFeedback, RepresentationIndex, TransferActions, TransferError, TransferFormat,
    TransferLimits, TransferPayload, TransferRequest,
};

use super::super::clip::ClipSource;
use super::{
    ACTION_DRAG_ENDED, ACTION_DRAG_ENTERED, ACTION_DRAG_LOCATION, ACTION_DRAG_STARTED, After,
    Drops, MAX_READS, MAX_WAITING, READ_TIMEOUT, Requested, UriRead, java_key,
};

const IMAGE: RepresentationIndex = RepresentationIndex(0);

/// A drag from another app offering `types` enters the view and its target
/// accepts it: the offer it minted.
fn hovering(drops: &mut Drops, types: &[&str]) -> DataTransferId {
    let types: Vec<String> = types.iter().map(|mime| (*mime).to_owned()).collect();
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

/// The hovering offer `id` drops and is taken.
fn dropped(drops: &mut Drops, id: DataTransferId) {
    assert_eq!(
        drops.hover_key(),
        java_key(id),
        "the view holds the clip under the offer's key"
    );
    let (event, taken) = drops
        .take_drop(java_key(id), (4.0, 8.0), false)
        .expect("an offer hovers");
    assert!(matches!(
        event,
        DragDropEvent::Dropped { id: dropped, action: TransferActions::COPY, .. } if dropped == id
    ));
    assert!(taken);
}

/// The byte ceiling every URI read of these tests asks for.
const MAX_BYTES: u64 = 4096;

/// A request at `now` for the bytes of the image of `id`, which waits for a
/// read.
fn wanted(
    drops: &mut Drops,
    id: DataTransferId,
    now: Instant,
    after: &mut After,
) -> TransferRequest {
    match drops.request(
        id,
        IMAGE,
        TransferLimits::default().with_max_bytes(MAX_BYTES),
        now,
        after,
    ) {
        Requested::Request(request) => request,
        other @ Requested::Clip { .. } => {
            panic!("an image is read from its URI on a thread, not {other:?}")
        }
    }
}

fn poll(request: &mut TransferRequest) -> Poll<Result<TransferPayload, TransferError>> {
    Pin::new(request).poll(&mut Context::from_waker(Waker::noop()))
}

fn answered_now(requested: Requested) -> Result<TransferPayload, TransferError> {
    let Requested::Request(mut request) = requested else {
        panic!("answered at once, not {requested:?}");
    };
    match poll(&mut request) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("answered at once"),
    }
}

/// A drop reads nothing when it is taken: its text, HTML and URI list are read
/// from the held clip when requested, under the view's key, and before the
/// drop the clip is out of reach.
fn a_drop_is_read_when_requested() {
    let mut drops = Drops::default();
    let id = hovering(&mut drops, &["text/plain", "text/html", "text/uri-list"]);
    let limits = TransferLimits::default();
    let mut after = After::default();
    assert!(
        matches!(
            answered_now(drops.request(
                id,
                RepresentationIndex(0),
                limits,
                Instant::now(),
                &mut after
            )),
            Err(TransferError::SourceGone)
        ),
        "before the drop the clip's data is out of reach"
    );
    dropped(&mut drops, id);
    let sources = [ClipSource::Text, ClipSource::Html, ClipSource::UriList];
    for (index, expected) in (0_u16..).zip(sources) {
        match drops.request(
            id,
            RepresentationIndex(index),
            limits,
            Instant::now(),
            &mut after,
        ) {
            Requested::Clip {
                key,
                source,
                descriptor,
            } => {
                assert_eq!(key, java_key(id));
                assert_eq!(source, expected);
                assert_eq!(
                    descriptor.declared_len, None,
                    "no length is known before the read"
                );
            }
            other @ Requested::Request(_) => panic!("read from the held clip, not {other:?}"),
        }
    }
    assert!(matches!(
        answered_now(drops.request(
            id,
            RepresentationIndex(3),
            limits,
            Instant::now(),
            &mut after
        )),
        Err(TransferError::UnknownRepresentation { .. })
    ));
    assert!(after.is_empty(), "nothing is read on a thread");
}

/// A drop whose key is not the hovering offer's changes nothing; one the
/// target did not accept is refused, the offer exits and retires, and nothing
/// is left for the view to hold.
fn a_drop_is_taken_as_the_offer_it_names() {
    let mut drops = Drops::default();
    let types = ["image/png".to_owned()];
    let mut events = Vec::new();
    assert_eq!(drops.hover_key(), 0, "no offer hovers");
    assert!(drops.take_drop(0, (0.0, 0.0), false).is_none());
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
    assert!(
        drops
            .take_drop(java_key(id).wrapping_add(1), (2.0, 3.0), false)
            .is_none(),
        "another key takes nothing"
    );
    assert_eq!(drops.hover_key(), java_key(id), "the offer still hovers");
    let (event, taken) = drops
        .take_drop(java_key(id), (2.0, 3.0), false)
        .expect("an offer hovers");
    assert!(matches!(event, DragDropEvent::Exited { id: exited } if exited == id));
    assert!(!taken);
    let mut after = After::default();
    assert!(matches!(
        answered_now(drops.request(id, IMAGE, TransferLimits::default(), Instant::now(), &mut after)),
        Err(TransferError::StaleOffer(stale)) if stale == id
    ));
    assert!(
        !drops.conclude(id, &mut after),
        "a refused drop has nothing to let go of"
    );
    assert!(drops.take_drop(java_key(id), (0.0, 0.0), false).is_none());
}

/// At most four URIs are read at once: a fifth request waits for a place,
/// takes the first one a read leaves, and is answered by its own read alone.
fn uri_reads_wait_for_a_place() {
    let mut drops = Drops::default();
    let id = hovering(&mut drops, &["image/png"]);
    dropped(&mut drops, id);
    let mut after = After::default();
    let mut requests: Vec<TransferRequest> = (0..=MAX_READS)
        .map(|_| wanted(&mut drops, id, Instant::now(), &mut after))
        .collect();
    let started: Vec<UriRead> = std::mem::take(&mut after.start);
    assert_eq!(started.len(), MAX_READS, "the fifth waits");
    assert!(after.gone.is_empty());
    assert!(started.iter().all(|read| read.key == java_key(id)
        && read.limits.max_bytes == MAX_BYTES
        && matches!(&read.descriptor.format, TransferFormat::Image { mime } if mime.as_str() == "image/png")));
    assert!(started.iter().all(|read| drops.begin_read(read.ticket)));

    let completer = drops
        .finish_read(started[0].ticket, Instant::now(), &mut after)
        .expect("a live read answers its request");
    completer.complete(Ok(TransferPayload::Text("first".to_owned())));
    let [next]: [UriRead; 1] = std::mem::take(&mut after.start)
        .try_into()
        .expect("the fifth starts");
    assert!(
        started.iter().all(|read| read.ticket != next.ticket),
        "each read has its own ticket"
    );
    assert!(matches!(
        poll(&mut requests[0]),
        Poll::Ready(Ok(TransferPayload::Text(text))) if text == "first"
    ));
    for request in &mut requests[1..] {
        assert!(poll(request).is_pending(), "the others are still reading");
    }
    assert!(
        drops
            .finish_read(started[0].ticket, Instant::now(), &mut after)
            .is_none(),
        "a read answers once"
    );
}

/// Concluding a drop answers its reads `SourceGone` at once, the running and
/// the waiting; a running read keeps its place until its thread ends, and
/// what it reads late answers nothing. Another drop's reads go on.
fn a_concluded_drop_answers_its_reads() {
    let mut drops = Drops::default();
    let first = hovering(&mut drops, &["image/png"]);
    dropped(&mut drops, first);
    let mut events = Vec::new();
    assert!(drops.drag(ACTION_DRAG_ENDED, (0.0, 0.0), &[], false, &mut events));
    assert!(
        events.is_empty(),
        "a taken drop does not exit at the drag's end"
    );
    let second = hovering(&mut drops, &["image/png"]);
    assert_ne!(first, second, "each drag mints its own offer");
    dropped(&mut drops, second);

    let mut after = After::default();
    let mut ours: Vec<TransferRequest> = (0..=MAX_READS)
        .map(|_| wanted(&mut drops, first, Instant::now(), &mut after))
        .collect();
    let mut theirs = wanted(&mut drops, second, Instant::now(), &mut after);
    let running = std::mem::take(&mut after.start);
    assert_eq!(running.len(), MAX_READS);

    assert!(
        drops.conclude(first, &mut after),
        "a taken drop lets go of its clip"
    );
    assert!(
        after.start.is_empty(),
        "the running reads keep their places"
    );
    assert_eq!(
        after.gone.len(),
        MAX_READS + 1,
        "the running and the waiting"
    );
    for completer in std::mem::take(&mut after.gone) {
        completer.complete(Err(TransferError::SourceGone));
    }
    for request in &mut ours {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::SourceGone))
        ));
    }
    assert!(
        running.iter().all(|read| !drops.begin_read(read.ticket)),
        "nothing waits for them"
    );
    assert!(poll(&mut theirs).is_pending());

    assert!(
        drops
            .finish_read(running[0].ticket, Instant::now(), &mut after)
            .is_none(),
        "a late read answers nothing"
    );
    let [next]: [UriRead; 1] = std::mem::take(&mut after.start)
        .try_into()
        .expect("the other drop's read starts");
    assert_eq!(next.key, java_key(second));
    drops
        .finish_read(next.ticket, Instant::now(), &mut after)
        .expect("its request waits")
        .complete(Ok(TransferPayload::Text("second".to_owned())));
    assert!(matches!(poll(&mut theirs), Poll::Ready(Ok(_))));

    assert!(
        !drops.conclude(first, &mut after),
        "a concluded drop lets go of nothing again"
    );
    assert!(matches!(
        answered_now(drops.request(first, IMAGE, TransferLimits::default(), Instant::now(), &mut after)),
        Err(TransferError::StaleOffer(stale)) if stale == first
    ));
    assert!(drops.conclude(second, &mut after));
}

/// A request dropped while its read waits is let go of when a place frees,
/// and one dropped while it reads is no longer wanted.
fn a_dropped_request_gives_up_its_read() {
    let mut drops = Drops::default();
    let id = hovering(&mut drops, &["image/png"]);
    dropped(&mut drops, id);
    let mut after = After::default();
    let mut requests: Vec<TransferRequest> = (0..MAX_READS)
        .map(|_| wanted(&mut drops, id, Instant::now(), &mut after))
        .collect();
    let running = std::mem::take(&mut after.start);
    drop(wanted(&mut drops, id, Instant::now(), &mut after));
    let kept = wanted(&mut drops, id, Instant::now(), &mut after);
    drop(requests.pop());
    assert!(
        !drops.begin_read(running[MAX_READS - 1].ticket),
        "its request is gone"
    );

    assert!(
        drops
            .finish_read(running[0].ticket, Instant::now(), &mut after)
            .is_some()
    );
    assert_eq!(after.gone.len(), 1, "the dropped request is let go of");
    let [next]: [UriRead; 1] = std::mem::take(&mut after.start)
        .try_into()
        .expect("the kept one starts");
    assert!(drops.begin_read(next.ticket));
    drop(kept);
}

/// While every place is held by a read that blocks, at most `MAX_WAITING`
/// reads wait and the next request is refused `Busy`; a request dropped while
/// it waits gives its entry back at the next request, so asking and dropping
/// without end never fills the queue.
fn waiting_reads_are_bounded() {
    let mut drops = Drops::default();
    let id = hovering(&mut drops, &["image/png"]);
    dropped(&mut drops, id);
    let mut after = After::default();
    let _running: Vec<TransferRequest> = (0..MAX_READS)
        .map(|_| wanted(&mut drops, id, Instant::now(), &mut after))
        .collect();
    assert_eq!(after.start.len(), MAX_READS, "every place is held");

    for _ in 0..4 * MAX_WAITING {
        drop(wanted(&mut drops, id, Instant::now(), &mut after));
    }
    let mut waiting: Vec<TransferRequest> = (0..MAX_WAITING)
        .map(|_| wanted(&mut drops, id, Instant::now(), &mut after))
        .collect();
    assert_eq!(
        after.gone.len(),
        4 * MAX_WAITING,
        "each dropped request gave its entry back at the next request"
    );
    assert!(waiting.iter_mut().all(|request| poll(request).is_pending()));
    let mut refused = wanted(&mut drops, id, Instant::now(), &mut after);
    assert!(
        matches!(poll(&mut refused), Poll::Ready(Err(TransferError::Busy))),
        "past the bound the request is refused"
    );
    assert_eq!(after.start.len(), MAX_READS, "nothing else started");
}

/// A read past `READ_TIMEOUT` is answered `TimedOut` and, once inside its
/// Java read, is asked to stop exactly once; it keeps its place until its
/// thread ends, and only then does a waiting read start. One that never
/// entered its Java read is sent no cancellation and finds nothing to read.
/// The watch looks again at the next deadline, ends when no read is left to
/// time, and the next read to start starts another.
fn a_read_past_its_deadline_times_out() {
    let mut drops = Drops::default();
    let id = hovering(&mut drops, &["image/png"]);
    dropped(&mut drops, id);
    let start = Instant::now();
    let later = start + Duration::from_secs(2);
    let mut after = After::default();
    let mut first = wanted(&mut drops, id, start, &mut after);
    assert!(
        std::mem::take(&mut after.watch),
        "the first read starts the watch"
    );
    let mut rest: Vec<TransferRequest> = (1..MAX_READS)
        .map(|_| wanted(&mut drops, id, later, &mut after))
        .collect();
    let mut queued = wanted(&mut drops, id, later, &mut after);
    assert!(!after.watch, "one watch at a time");
    let running = std::mem::take(&mut after.start);
    assert_eq!(running.len(), MAX_READS);
    assert!(
        drops.begin_read(running[0].ticket),
        "the first enters its Java read"
    );

    assert_eq!(
        drops.expire(start + Duration::from_secs(4), &mut after),
        Some(start + READ_TIMEOUT),
        "nothing is due yet"
    );
    assert!(after.is_empty());

    let due = start + READ_TIMEOUT;
    assert_eq!(drops.expire(due, &mut after), Some(later + READ_TIMEOUT));
    assert_eq!(
        after.cancel,
        [running[0].ticket],
        "its Java read is unblocked"
    );
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }
    after.cancel.clear();
    assert!(matches!(
        poll(&mut first),
        Poll::Ready(Err(TransferError::TimedOut))
    ));
    assert!(
        after.is_empty(),
        "the queued read waits: the place is still held"
    );
    assert_eq!(drops.expire(due, &mut after), Some(later + READ_TIMEOUT));
    assert!(after.is_empty(), "a read times out once");

    assert_eq!(
        drops.expire(later + READ_TIMEOUT, &mut after),
        None,
        "the watch ends"
    );
    assert_eq!(after.timed_out.len(), MAX_READS - 1);
    assert!(
        after.cancel.is_empty(),
        "no thread entered those Java reads"
    );
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }
    for request in &mut rest {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::TimedOut))
        ));
    }
    assert!(
        !drops.begin_read(running[1].ticket),
        "a timed-out read reads nothing"
    );

    let late = later + READ_TIMEOUT + Duration::from_secs(1);
    assert!(
        drops
            .finish_read(running[0].ticket, late, &mut after)
            .is_none(),
        "what it read late answers nothing"
    );
    let [next]: [UriRead; 1] = std::mem::take(&mut after.start)
        .try_into()
        .expect("its place goes to the queued read once its thread ends");
    assert!(after.watch, "the next read starts another watch");
    assert_eq!(drops.expire(late, &mut after), Some(late + READ_TIMEOUT));
    assert!(drops.begin_read(next.ticket));
    assert!(poll(&mut queued).is_pending());
}

/// FLUI's own drag dropped on its own target reports the action the target
/// took, once.
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
        .take_drop(java_key(id), (0.0, 0.0), true)
        .expect("an offer hovers");
    assert_eq!(drops.take_local_drop(), Some(TransferActions::MOVE));
    assert_eq!(drops.take_local_drop(), None, "taken once");
}

#[test]
fn android_drop_state() {
    crate::table_test::run_table(
        "android_drop_state",
        &[
            (
                "a_drop_is_read_when_requested",
                a_drop_is_read_when_requested,
            ),
            (
                "a_drop_is_taken_as_the_offer_it_names",
                a_drop_is_taken_as_the_offer_it_names,
            ),
            ("uri_reads_wait_for_a_place", uri_reads_wait_for_a_place),
            ("waiting_reads_are_bounded", waiting_reads_are_bounded),
            (
                "a_read_past_its_deadline_times_out",
                a_read_past_its_deadline_times_out,
            ),
            (
                "a_concluded_drop_answers_its_reads",
                a_concluded_drop_answers_its_reads,
            ),
            (
                "a_dropped_request_gives_up_its_read",
                a_dropped_request_gives_up_its_read,
            ),
            (
                "our_own_drop_reports_its_action",
                our_own_drop_reports_its_action,
            ),
        ],
    );
}
