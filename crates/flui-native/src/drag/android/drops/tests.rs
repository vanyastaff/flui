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
use parking_lot::Mutex;

use super::super::clip::ClipSource;
use super::{
    ACTION_DRAG_ENDED, ACTION_DRAG_ENTERED, ACTION_DRAG_LOCATION, ACTION_DRAG_STARTED, After,
    CANCEL_RETRY, Drops, MAX_READS, MAX_WAITING, READ_TIMEOUT, Requested, Spawn, UriRead, java_key,
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
/// thread ends, while a request waiting for that place times out where it
/// waits. One that never entered its Java read is sent no cancellation and
/// finds nothing to read. The watch looks again at the next deadline, ends
/// when no request is left to time, and the next request starts another.
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
    assert!(
        std::mem::take(&mut after.cancel_worker),
        "its Java read is unblocked by the cancel worker"
    );
    assert_eq!(drops.next_cancel(), Some(running[0].ticket));
    assert_eq!(drops.next_cancel(), None, "the worker ends");
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }

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
    assert_eq!(
        after.timed_out.len(),
        MAX_READS,
        "the queued request times out where it waits, with the reads ahead of it"
    );
    assert!(!after.cancel_worker, "no thread entered those Java reads");
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }
    for request in rest.iter_mut().chain(std::iter::once(&mut queued)) {
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
    assert!(after.is_empty(), "nothing waits for its place");
    let mut fresh = wanted(&mut drops, id, late, &mut after);
    let [next]: [UriRead; 1] = std::mem::take(&mut after.start)
        .try_into()
        .expect("its place goes to the next request once its thread ends");
    assert!(after.watch, "the next request starts another watch");
    assert_eq!(drops.expire(late, &mut after), Some(late + READ_TIMEOUT));
    assert!(drops.begin_read(next.ticket));
    assert!(poll(&mut fresh).is_pending());
}

/// Two reads that entered their Java reads, `MAX_READS` placed in all: the
/// handle the tests keep of them.
fn entered_reads(
    drops: &mut Drops,
    start: Instant,
    after: &mut After,
) -> (Vec<TransferRequest>, Vec<UriRead>) {
    let id = hovering(drops, &["image/png"]);
    dropped(drops, id);
    let requests: Vec<TransferRequest> = (0..MAX_READS)
        .map(|at| {
            let at = u32::try_from(at).expect("BUG: four reads");
            wanted(drops, id, start + Duration::from_secs(at.into()), after)
        })
        .collect();
    let running = std::mem::take(&mut after.start);
    after.watch = false;
    for read in &running {
        assert!(drops.begin_read(read.ticket));
    }
    (requests, running)
}

/// A cancellation goes to the cancel worker's queue, never to the thread
/// that timed the read out or concluded its drop: while the worker is stuck
/// in one, later deadlines still pass and queue theirs behind it, one worker
/// at a time. A read whose thread ended takes its cancellation out of the
/// queue, so the queue holds at most one per place.
fn a_stalled_cancel_delays_no_deadline() {
    let mut drops = Drops::default();
    let start = Instant::now();
    let mut after = After::default();
    let (mut requests, running) = entered_reads(&mut drops, start, &mut after);

    drops.expire(start + READ_TIMEOUT, &mut after);
    assert!(
        std::mem::take(&mut after.cancel_worker),
        "the worker starts"
    );
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }
    assert_eq!(
        drops.next_cancel(),
        Some(running[0].ticket),
        "and is stuck in it"
    );

    let later = start + READ_TIMEOUT + Duration::from_secs(2);
    drops.expire(later, &mut after);
    assert!(!after.cancel_worker, "one worker at a time");
    assert_eq!(after.timed_out.len(), 2, "the later deadlines still pass");
    for completer in std::mem::take(&mut after.timed_out) {
        completer.complete(Err(TransferError::TimedOut));
    }
    for request in &mut requests[..3] {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::TimedOut))
        ));
    }

    drops.finish_read(running[1].ticket, later, &mut after);
    assert_eq!(
        drops.next_cancel(),
        Some(running[2].ticket),
        "an ended read is not cancelled"
    );
    assert_eq!(drops.next_cancel(), None);
    drops.expire(start + READ_TIMEOUT + Duration::from_secs(3), &mut after);
    assert!(after.cancel_worker, "the next stop starts the worker again");
}

/// A panic where the watch runs user code (a waker's, as `run` stands in for
/// here) does not end the watch: the later deadline still passes, the watch
/// ends with nothing left to time, its flag is clear for the next read, and
/// the panic is raised after.
fn a_panic_in_the_watch_misses_no_deadline() {
    let drops = std::sync::Arc::new(parking_lot::Mutex::new(Drops::default()));
    let start = Instant::now();
    let mut after = After::default();
    let (mut requests, _running) = entered_reads(&mut drops.lock(), start, &mut after);
    let watched = std::sync::Arc::clone(&drops);
    let ended = std::thread::spawn(move || {
        let clock = std::cell::Cell::new(start);
        let turns = std::cell::Cell::new(0_u32);
        super::watch(
            &watched,
            || clock.get(),
            |next| clock.set(next),
            |mut after| {
                for completer in std::mem::take(&mut after.timed_out) {
                    completer.complete(Err(TransferError::TimedOut));
                }
                turns.set(turns.get() + 1);
                assert!(turns.get() != 2, "a waker panics");
            },
        );
    })
    .join();
    assert!(ended.is_err(), "the panic is raised once the watch ends");
    for request in &mut requests {
        assert!(
            matches!(poll(request), Poll::Ready(Err(TransferError::TimedOut))),
            "every deadline passed, those after the panic too"
        );
    }
    let id = hovering(&mut drops.lock(), &["image/png"]);
    dropped(&mut drops.lock(), id);
    let mut fresh = After::default();
    let reading = drops.lock().reading.keys().copied().collect::<Vec<_>>();
    for ticket in reading {
        drops.lock().finish_read(ticket, start, &mut fresh);
    }
    let _request = wanted(&mut drops.lock(), id, start, &mut fresh);
    assert!(fresh.watch, "the watch's flag was cleared");
}

/// A panic in one cancellation does not stop the rest, and the worker's flag
/// is clear when it ends.
fn a_panic_in_one_cancel_unblocks_the_rest() {
    let drops = parking_lot::Mutex::new(Drops::default());
    let start = Instant::now();
    let mut after = After::default();
    let (_requests, running) = entered_reads(&mut drops.lock(), start, &mut after);
    drops.lock().expire(start + READ_TIMEOUT * 2, &mut after);
    assert!(after.cancel_worker);
    let cancelled = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        let ended = scope
            .spawn(|| {
                super::cancel_all(&drops, |ticket| {
                    cancelled.lock().expect("BUG: not poisoned").push(ticket);
                    assert!(ticket != running[0].ticket, "the first cancel panics");
                });
            })
            .join();
        assert!(ended.is_err(), "the panic is raised once the worker ends");
    });
    let mut cancelled = cancelled.into_inner().expect("BUG: not poisoned");
    cancelled.sort_unstable();
    let mut expected: Vec<_> = running.iter().map(|read| read.ticket).collect();
    expected.sort_unstable();
    assert_eq!(cancelled, expected, "every read was unblocked");
    let mut again = After::default();
    let id = hovering(&mut drops.lock(), &["image/png"]);
    dropped(&mut drops.lock(), id);
    for read in &running {
        drops.lock().finish_read(read.ticket, start, &mut again);
    }
    let _request = wanted(&mut drops.lock(), id, start, &mut again);
    for read in std::mem::take(&mut again.start) {
        assert!(drops.lock().begin_read(read.ticket));
    }
    drops.lock().expire(start + READ_TIMEOUT * 3, &mut again);
    assert!(again.cancel_worker, "the worker's flag was cleared");
}

/// A thread that cannot start, for [`super::settle`].
fn no_thread() -> std::io::Result<()> {
    Err(std::io::Error::other("no thread"))
}

/// `MAX_READS + 1` requests for the image of a fresh drop: the requests, and
/// what placing them left.
fn more_than_a_place_each(drops: &Mutex<Drops>, now: Instant) -> (Vec<TransferRequest>, After) {
    let mut after = After::default();
    let id = hovering(&mut drops.lock(), &["image/png"]);
    dropped(&mut drops.lock(), id);
    let requests = (0..=MAX_READS)
        .map(|_| wanted(&mut drops.lock(), id, now, &mut after))
        .collect();
    (requests, after)
}

/// With no watch no read runs without a deadline: when the watch cannot
/// start, the reads placed are refused `SourceGone` before a thread starts
/// for any of them, the places they give back go to the waiting read, which
/// is refused the same, and the next read tries for a watch again.
fn a_watch_that_cannot_start_refuses_its_reads() {
    let drops = Mutex::new(Drops::default());
    let now = Instant::now();
    let (mut requests, after) = more_than_a_place_each(&drops, now);
    let mut watches = 0_usize;
    let mut reads = 0_usize;
    super::settle(
        &drops,
        after,
        || now,
        |job| match job {
            Spawn::Watch => {
                watches += 1;
                no_thread()
            }
            Spawn::Read(_) => {
                reads += 1;
                Ok(())
            }
            Spawn::CancelWorker => Ok(()),
        },
    );
    assert_eq!(watches, 1, "the waiting read is refused with them");
    assert_eq!(reads, 0, "no read starts without a deadline");
    for request in &mut requests {
        assert!(
            matches!(poll(request), Poll::Ready(Err(TransferError::SourceGone))),
            "every request is answered"
        );
    }
    assert!(drops.lock().reading.is_empty(), "every place is given back");

    let mut again = After::default();
    let id = hovering(&mut drops.lock(), &["image/png"]);
    dropped(&mut drops.lock(), id);
    let _request = wanted(&mut drops.lock(), id, now, &mut again);
    assert!(again.watch, "the next read tries for a watch");
}

/// A read whose thread cannot start is refused `SourceGone` and gives its
/// place to the next, which is refused the same, under one watch.
fn a_read_that_cannot_start_is_refused() {
    let drops = Mutex::new(Drops::default());
    let now = Instant::now();
    let (mut requests, after) = more_than_a_place_each(&drops, now);
    let mut watches = 0_usize;
    let mut reads = Vec::new();
    super::settle(
        &drops,
        after,
        || now,
        |job| match job {
            Spawn::Watch => {
                watches += 1;
                Ok(())
            }
            Spawn::Read(read) => {
                reads.push(read.ticket);
                no_thread()
            }
            Spawn::CancelWorker => Ok(()),
        },
    );
    assert_eq!(watches, 1);
    reads.dedup();
    assert_eq!(reads.len(), MAX_READS + 1, "the waiting read was tried too");
    for request in &mut requests {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::SourceGone))
        ));
    }
    assert!(drops.lock().reading.is_empty(), "every place is given back");
}

/// A cancel worker that cannot start leaves its reads blocked only until the
/// watch tries again: a watch starts though no deadline is left to time, and
/// its turn asks for the worker again and looks again `CANCEL_RETRY` later.
fn a_cancel_worker_that_cannot_start_is_tried_again() {
    let drops = Mutex::new(Drops::default());
    let start = Instant::now();
    let mut after = After::default();
    let (mut requests, running) = entered_reads(&mut drops.lock(), start, &mut after);
    let late = start + READ_TIMEOUT * 2;
    assert_eq!(
        drops.lock().expire(late, &mut after),
        None,
        "every read is late"
    );
    let mut watches = 0_usize;
    super::settle(
        &drops,
        after,
        || late,
        |job| match job {
            Spawn::CancelWorker => no_thread(),
            Spawn::Watch => {
                watches += 1;
                Ok(())
            }
            Spawn::Read(_) => panic!("no read is placed"),
        },
    );
    for request in &mut requests {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::TimedOut))
        ));
    }
    assert_eq!(watches, 1, "a watch starts to try the worker again");

    let mut retry = After::default();
    assert_eq!(
        drops.lock().expire(late, &mut retry),
        Some(late + CANCEL_RETRY),
        "and stays while the reads are blocked"
    );
    assert!(retry.cancel_worker, "its turn asks for the worker again");
    let mut cancelled: Vec<_> = std::iter::from_fn(|| drops.lock().next_cancel()).collect();
    cancelled.sort_unstable();
    let mut expected: Vec<_> = running.iter().map(|read| read.ticket).collect();
    expected.sort_unstable();
    assert_eq!(cancelled, expected, "every blocked read is unblocked");
}

/// With no thread to be had at all, every request is still answered and
/// settling ends: the late reads time out and, once their threads end, the
/// next requests are refused.
fn with_no_thread_every_request_is_answered() {
    let drops = Mutex::new(Drops::default());
    let start = Instant::now();
    let mut after = After::default();
    let (mut requests, running) = entered_reads(&mut drops.lock(), start, &mut after);
    let late = start + READ_TIMEOUT * 2;
    drops.lock().expire(late, &mut after);
    super::settle(&drops, after, || late, |_| no_thread());
    for request in &mut requests {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::TimedOut))
        ));
    }
    let mut ended = After::default();
    for read in &running {
        assert!(
            drops
                .lock()
                .finish_read(read.ticket, late, &mut ended)
                .is_none()
        );
    }
    assert!(ended.is_empty());
    let (mut more, after) = more_than_a_place_each(&drops, late);
    super::settle(&drops, after, || late, |_| no_thread());
    for request in &mut more {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::SourceGone))
        ));
    }
}

/// A request waiting for a place times out within the deadline it was
/// accepted with, though the four reads ahead of it block for good, and the
/// watch stays for it after theirs have passed.
fn a_waiting_request_times_out_behind_blocked_reads() {
    let drops = Mutex::new(Drops::default());
    let start = Instant::now();
    let mut after = After::default();
    let (mut running, _reads) = entered_reads(&mut drops.lock(), start, &mut after);
    let id = hovering(&mut drops.lock(), &["image/png"]);
    dropped(&mut drops.lock(), id);
    let asked = start + Duration::from_secs(4);
    let mut fifth = wanted(&mut drops.lock(), id, asked, &mut after);
    assert!(after.start.is_empty(), "it waits for a place");
    let clock = std::cell::Cell::new(asked);
    let answered = std::cell::RefCell::new(None);
    super::watch(
        &drops,
        || clock.get(),
        |next| clock.set(next),
        |mut after| {
            for completer in std::mem::take(&mut after.timed_out) {
                completer.complete(Err(TransferError::TimedOut));
            }
            let mut answered = answered.borrow_mut();
            if answered.is_none()
                && let Poll::Ready(result) = poll(&mut fifth)
            {
                *answered = Some((result, clock.get()));
            }
        },
    );
    let answered = answered.into_inner();
    assert!(
        matches!(
            answered,
            Some((Err(TransferError::TimedOut), at)) if at == asked + READ_TIMEOUT
        ),
        "it times out at the deadline it was accepted with, not {answered:?}"
    );
    for request in &mut running {
        assert!(matches!(
            poll(request),
            Poll::Ready(Err(TransferError::TimedOut))
        ));
    }
    assert!(drops.lock().waiting.is_empty(), "and it is let go of");
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
                "a_stalled_cancel_delays_no_deadline",
                a_stalled_cancel_delays_no_deadline,
            ),
            (
                "a_panic_in_the_watch_misses_no_deadline",
                a_panic_in_the_watch_misses_no_deadline,
            ),
            (
                "a_panic_in_one_cancel_unblocks_the_rest",
                a_panic_in_one_cancel_unblocks_the_rest,
            ),
            (
                "a_watch_that_cannot_start_refuses_its_reads",
                a_watch_that_cannot_start_refuses_its_reads,
            ),
            (
                "a_read_that_cannot_start_is_refused",
                a_read_that_cannot_start_is_refused,
            ),
            (
                "a_cancel_worker_that_cannot_start_is_tried_again",
                a_cancel_worker_that_cannot_start_is_tried_again,
            ),
            (
                "with_no_thread_every_request_is_answered",
                with_no_thread_every_request_is_answered,
            ),
            (
                "a_waiting_request_times_out_behind_blocked_reads",
                a_waiting_request_times_out_behind_blocked_reads,
            ),
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
