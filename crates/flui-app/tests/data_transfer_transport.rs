//! Gate-visible tests for the data-transfer transport (ADR-0038).
//!
//! These live in flui-app rather than next to the code because
//! flui-platform's tests are excluded from the CI test job — a transport
//! test there would be green-by-vacuity at the merge gate. flui-app is
//! CI-covered and already depends on flui-platform and flui-scheduler, so
//! the `OfferTable`, `TransferRequest`/`TransferCompleter`, and
//! mock-source-through-`AsyncDriver` evidence runs here until the exclusion
//! lifts.

use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferCompleter,
    TransferError, TransferFormat, TransferLimits, TransferPayload, TransferRequest, TransferUri,
};
use flui_scheduler::AsyncDriver;
use parking_lot::Mutex;

// ============================================================================
// Helpers
// ============================================================================

fn text_representations() -> Arc<[RepresentationDescriptor]> {
    Arc::from([RepresentationDescriptor {
        format: TransferFormat::Text,
        declared_len: None,
    }])
}

fn poll_once(
    request: &mut std::pin::Pin<&mut TransferRequest>,
) -> Poll<Result<TransferPayload, TransferError>> {
    request
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
}

// ============================================================================
// OfferTable semantics
// ============================================================================

#[test]
fn offer_table_mints_lookups_and_retires() {
    let mut table = OfferTable::new();
    assert!(table.is_empty());

    let id = table.mint(OfferRecord::new(text_representations()));
    assert_eq!(table.len(), 1);
    assert!(table.get(id).is_some(), "a live id resolves its record");

    let record = table.retire(id).expect("retiring a live offer yields it");
    assert_eq!(record.representations().len(), 1);
    assert!(table.is_empty());
    assert!(table.get(id).is_none(), "a retired id no longer resolves");
    assert!(
        table.retire(id).is_none(),
        "retire is idempotent — the second call is a stale no-op"
    );
}

#[test]
fn offer_table_bumps_the_generation_on_retire() {
    let mut table = OfferTable::new();
    let first = table.mint(OfferRecord::new(text_representations()));
    table.retire(first);

    // The freed slot is reused, so the new id shares the slot index but
    // must carry a bumped generation.
    let second = table.mint(OfferRecord::new(text_representations()));
    assert_eq!(
        first.index(),
        second.index(),
        "the slot is expected to be reused for this test to be meaningful"
    );
    assert!(
        second.generation() > first.generation(),
        "reused slot must mint under a bumped generation"
    );
    assert_ne!(first, second);

    // Cross-generation lookup fails: the stale id addresses nothing even
    // though its slot is occupied again.
    assert!(table.get(first).is_none());
    assert!(table.get(second).is_some());
}

#[test]
fn stale_offer_request_fails_with_stale_offer() {
    let source = MockSource::new();
    let offer = source.mint_text_offer("hello");
    source.retire(offer.id());

    let request = source.request(
        offer.id(),
        RepresentationIndex(0),
        TransferLimits::default(),
    );
    let mut request = pin!(request);
    let Poll::Ready(Err(TransferError::StaleOffer(stale))) = poll_once(&mut request) else {
        panic!("a retired offer must fail the generation check");
    };
    assert_eq!(stale, offer.id());
}

// ============================================================================
// TransferRequest / TransferCompleter state machine
// ============================================================================

#[test]
fn completer_resolves_the_request() {
    let (request, completer) = TransferRequest::channel();
    let mut request = pin!(request);
    assert!(poll_once(&mut request).is_pending());

    completer.complete(Ok(TransferPayload::Text("delivered".into())));

    let Poll::Ready(Ok(TransferPayload::Text(text))) = poll_once(&mut request) else {
        panic!("completed request must resolve with the payload");
    };
    assert_eq!(text, "delivered");
}

#[test]
fn dropping_the_completer_resolves_source_gone() {
    let (request, completer) = TransferRequest::channel();
    let mut request = pin!(request);
    assert!(poll_once(&mut request).is_pending());

    drop(completer);

    assert!(matches!(
        poll_once(&mut request),
        Poll::Ready(Err(TransferError::SourceGone))
    ));
}

#[test]
fn dropping_the_request_is_observable_as_cancellation() {
    let (request, completer) = TransferRequest::channel();
    assert!(!completer.is_cancelled());

    drop(request);

    assert!(
        completer.is_cancelled(),
        "the producer must observe consumer-side cancellation"
    );
    // Completing after cancellation is a silent no-op (must not panic).
    completer.complete(Ok(TransferPayload::Text("too late".into())));
}

#[test]
fn ready_request_resolves_immediately() {
    let request = TransferRequest::ready(Ok(TransferPayload::UriList(vec![TransferUri::Path(
        PathBuf::from("/tmp/ready.txt"),
    )])));
    let mut request = pin!(request);
    let Poll::Ready(Ok(TransferPayload::UriList(uris))) = poll_once(&mut request) else {
        panic!("ready request must resolve on the first poll");
    };
    assert_eq!(uris.len(), 1);
}

#[test]
fn completion_wakes_the_stored_waker() {
    // A real Waker (the AsyncDriver's) must fire on complete(); the noop
    // waker above cannot show that, so drive one task through the driver.
    let driver = AsyncDriver::new();
    let frames = Arc::new(AtomicUsize::new(0));
    let frames_for_hook = Arc::clone(&frames);
    driver.set_request_frame(move || {
        frames_for_hook.fetch_add(1, Ordering::Relaxed);
    });

    let (request, completer) = TransferRequest::channel();
    let outcome: Arc<Mutex<Option<Result<TransferPayload, TransferError>>>> =
        Arc::new(Mutex::new(None));
    let outcome_for_task = Arc::clone(&outcome);
    let _token = driver.spawn_local(Box::pin(async move {
        *outcome_for_task.lock() = Some(request.await);
    }));

    assert_eq!(driver.poll_ready(), 1, "first poll parks the request");
    assert!(outcome.lock().is_none());
    let frames_before = frames.load(Ordering::Relaxed);

    completer.complete(Ok(TransferPayload::Text("woken".into())));
    assert!(
        frames.load(Ordering::Relaxed) > frames_before,
        "completion must wake the driver's frame-request hook"
    );

    assert_eq!(driver.poll_ready(), 1, "the wake re-arms exactly this task");
    let delivered = outcome.lock().take().expect("task observed the payload");
    assert!(matches!(delivered, Ok(TransferPayload::Text(text)) if text == "woken"));
}

// ============================================================================
// Mock source: the seven stages over a real AsyncDriver
// ============================================================================

/// A `DataTransferSource` with the same shape a native backend has: one
/// `OfferTable`, lazy delivery through parked completers, and a feedback
/// cache — enough to drive every transport stage from a test.
struct MockSource {
    state: Mutex<MockState>,
}

struct MockState {
    table: OfferTable,
    /// Text payload per live offer, delivered when the test releases it.
    payloads: Vec<(DataTransferId, String)>,
    /// Deliveries parked until `deliver_all` (the "async backend" half).
    parked: Vec<(DataTransferId, TransferCompleter, TransferLimits)>,
    /// Latest stage-2 feedback per offer, as a backend would cache it.
    feedback: Vec<(DataTransferId, DropFeedback)>,
    /// Offers retired through stage-6 conclusion.
    concluded: Vec<DataTransferId>,
}

impl MockSource {
    fn new() -> Self {
        Self {
            state: Mutex::new(MockState {
                table: OfferTable::new(),
                payloads: Vec::new(),
                parked: Vec::new(),
                feedback: Vec::new(),
                concluded: Vec::new(),
            }),
        }
    }

    /// Stage 1: announce an offer whose text payload the mock will deliver.
    fn mint_text_offer(&self, text: &str) -> DataTransferOffer {
        let representations = text_representations();
        let mut state = self.state.lock();
        let id = state
            .table
            .mint(OfferRecord::new(Arc::clone(&representations)));
        state.payloads.push((id, text.to_string()));
        DataTransferOffer::new(id, representations)
    }

    /// Producer half: resolve every parked delivery from "the backend".
    fn deliver_all(&self) {
        let (parked, payloads) = {
            let mut state = self.state.lock();
            let parked = std::mem::take(&mut state.parked);
            (parked, state.payloads.clone())
        };
        // Completions run outside the lock, like a real backend thread.
        for (id, completer, limits) in parked {
            let payload = payloads
                .iter()
                .find(|(payload_id, _)| *payload_id == id)
                .map(|(_, text)| TransferPayload::Text(text.clone()));
            let result = match payload {
                Some(payload) if payload.byte_len() > limits.max_bytes => {
                    Err(TransferError::TooLarge {
                        actual: payload.byte_len(),
                        limit: limits.max_bytes,
                    })
                }
                Some(payload) => Ok(payload),
                None => Err(TransferError::SourceGone),
            };
            completer.complete(result);
        }
    }

    fn retire(&self, id: DataTransferId) {
        self.state.lock().table.retire(id);
    }

    fn cached_feedback(&self, id: DataTransferId) -> Option<DropFeedback> {
        self.state
            .lock()
            .feedback
            .iter()
            .rev()
            .find(|(feedback_id, _)| *feedback_id == id)
            .map(|(_, feedback)| *feedback)
    }

    fn concluded(&self) -> Vec<DataTransferId> {
        self.state.lock().concluded.clone()
    }

    fn parked_count(&self) -> usize {
        self.state.lock().parked.len()
    }

    fn parked_completer_cancelled(&self, id: DataTransferId) -> Option<bool> {
        self.state
            .lock()
            .parked
            .iter()
            .find(|(parked_id, _, _)| *parked_id == id)
            .map(|(_, completer, _)| completer.is_cancelled())
    }
}

impl DataTransferSource for MockSource {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let mut state = self.state.lock();
        let Some(record) = state.table.get(id) else {
            return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
        };
        if usize::from(representation.0) >= record.representations().len() {
            return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                id,
                index: representation,
            }));
        }
        let (request, completer) = TransferRequest::channel();
        state.parked.push((id, completer, limits));
        request
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        let mut state = self.state.lock();
        if state.table.get(id).is_none() {
            return; // stale ids are a no-op per the trait contract
        }
        state.feedback.push((id, feedback));
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let mut state = self.state.lock();
        if state.table.retire(id).is_some() {
            state.concluded.push(id);
        }
    }
}

/// All seven stages, end to end, with delivery polled by a real
/// `AsyncDriver` — the exact contour a widget-facing consumer will use.
#[test]
fn mock_source_drives_all_seven_stages_through_the_async_driver() {
    let source = Arc::new(MockSource::new());
    let driver = AsyncDriver::new();
    let frames = Arc::new(AtomicUsize::new(0));
    let frames_for_hook = Arc::clone(&frames);
    driver.set_request_frame(move || {
        frames_for_hook.fetch_add(1, Ordering::Relaxed);
    });

    // Stage 1 — offer: metadata only, no payload.
    let offer = source.mint_text_offer("pasted text");
    assert_eq!(offer.representations().len(), 1);

    // Stage 2 — negotiation: the consumer picks a representation and (drag
    // facade) replies with feedback the source caches.
    let index = offer
        .find(&TransferFormat::Text)
        .expect("the announced Text representation is findable");
    source.update_drop_feedback(
        offer.id(),
        DropFeedback {
            accept: Some(TransferActions::COPY),
        },
    );
    assert_eq!(
        source.cached_feedback(offer.id()),
        Some(DropFeedback {
            accept: Some(TransferActions::COPY),
        })
    );

    // Stage 3 — request: returns immediately with a pollable value.
    let request = source.request(offer.id(), index, TransferLimits::default());

    // Stage 4 — async delivery: the request rides a BoxedTask on the house
    // driver; nothing resolves until the producer completes.
    let outcome: Arc<Mutex<Option<Result<TransferPayload, TransferError>>>> =
        Arc::new(Mutex::new(None));
    let outcome_for_task = Arc::clone(&outcome);
    let token = driver.spawn_local(Box::pin(async move {
        *outcome_for_task.lock() = Some(request.await);
    }));
    assert_eq!(driver.poll_ready(), 1);
    assert!(outcome.lock().is_none(), "no delivery before the producer");

    source.deliver_all();
    assert_eq!(driver.poll_ready(), 1, "completion woke the task");

    // Stage 5 — decoding: the payload arrives typed.
    let delivered = outcome.lock().take().expect("delivery observed");
    let Ok(TransferPayload::Text(text)) = delivered else {
        panic!("expected the typed Text payload, got {delivered:?}");
    };
    assert_eq!(text, "pasted text");

    // Stage 6 — drop action/conclusion: the consumer is done; the source
    // releases the offer.
    source.conclude_drop(offer.id());
    assert_eq!(source.concluded(), vec![offer.id()]);

    // Stage 7 — completion: the offer is now stale for everyone.
    let stale = source.request(offer.id(), index, TransferLimits::default());
    let mut stale = pin!(stale);
    assert!(matches!(
        poll_once(&mut stale),
        Poll::Ready(Err(TransferError::StaleOffer(_)))
    ));
    drop(token);
}

/// Stage-7 cancellation composes through the driver: dropping the
/// `TaskToken` drops the task's future, which drops the `TransferRequest`,
/// which the producer observes via `TransferCompleter::is_cancelled`.
#[test]
fn dropping_the_task_token_cancels_the_in_flight_delivery() {
    let source = Arc::new(MockSource::new());
    let driver = AsyncDriver::new();

    let offer = source.mint_text_offer("never delivered");
    let request = source.request(
        offer.id(),
        RepresentationIndex(0),
        TransferLimits::default(),
    );

    let outcome: Arc<Mutex<Option<Result<TransferPayload, TransferError>>>> =
        Arc::new(Mutex::new(None));
    let outcome_for_task = Arc::clone(&outcome);
    let token = driver.spawn_local(Box::pin(async move {
        *outcome_for_task.lock() = Some(request.await);
    }));
    assert_eq!(driver.poll_ready(), 1);
    assert_eq!(
        source.parked_completer_cancelled(offer.id()),
        Some(false),
        "delivery is live while the token is held"
    );

    drop(token);

    assert_eq!(
        source.parked_completer_cancelled(offer.id()),
        Some(true),
        "TaskToken drop → future drop → TransferRequest drop → cancelled"
    );

    // Producer-side completion after cancellation is a silent no-op.
    source.deliver_all();
    assert_eq!(source.parked_count(), 0);
    assert!(
        outcome.lock().is_none(),
        "nothing is delivered after cancel"
    );
    assert_eq!(
        driver.poll_ready(),
        0,
        "the cancelled task never runs again"
    );
}

/// Byte limits are enforced at delivery against actual accumulated size.
#[test]
fn limits_are_enforced_at_delivery() {
    let source = Arc::new(MockSource::new());
    let offer = source.mint_text_offer("this payload is longer than four bytes");

    let request = source.request(
        offer.id(),
        RepresentationIndex(0),
        TransferLimits::default().with_max_bytes(4),
    );
    let mut request = pin!(request);
    assert!(poll_once(&mut request).is_pending());

    source.deliver_all();

    assert!(matches!(
        poll_once(&mut request),
        Poll::Ready(Err(TransferError::TooLarge { limit: 4, .. }))
    ));
}
