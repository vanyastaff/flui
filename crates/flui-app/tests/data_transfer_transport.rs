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
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferCompleter,
    TransferError, TransferFormat, TransferLimits, TransferPayload, TransferRequest,
};
use flui_scheduler::{OwnerFrame, UpdateScheduler};
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

// ============================================================================
// TransferRequest / TransferCompleter state machine
// ============================================================================

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
    let scheduler = UpdateScheduler::new();
    let owner_frame = OwnerFrame::new(&scheduler);
    let driver = owner_frame.async_driver();
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
    assert_eq!(owner_frame.poll_ready(), 1);
    assert!(outcome.lock().is_none(), "no delivery before the producer");

    source.deliver_all();
    assert_eq!(owner_frame.poll_ready(), 1, "completion woke the task");

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
