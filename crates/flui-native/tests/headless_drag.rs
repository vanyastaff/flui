//! The headless backend's OS drag session (ADR-0211): the action a drop
//! delivers, a test that drives the double out of order, and how the double
//! holds the provider a FLUI drag serves its representations from.

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use flui_foundation::geometry::Point;
use flui_foundation::{DataTransferId, DragSessionId, IdMint, UiRuntimeId};
use flui_native::traits::{DispatchEventResult, PlatformInput};
use flui_native::{
    HeadlessDragAndDrop, HeadlessPlatform, HostWindow, Platform as _, WindowOptions,
};
use flui_platform::DragDropEvent;
use flui_platform::data_transfer::{
    DataTransferSource, DropFeedback, RepresentationDescriptor, RepresentationIndex,
    TransferActions, TransferError, TransferFormat, TransferLimits, TransferPayload,
    TransferRequest,
};
use flui_platform::drag::{
    DragError, DragSessionKey, OsDragProvider, OsDragRequest, OsDragStart, ProcessNonce,
};

const COPY: TransferActions = TransferActions::COPY;
const MOVE: TransferActions = TransferActions::MOVE;
const LINK: TransferActions = TransferActions::LINK;

/// The OS side of a drag, a window under it whose target answers `accept`,
/// and what that window saw.
struct Desktop {
    dnd: HeadlessDragAndDrop,
    transfer: Arc<dyn DataTransferSource>,
    window: Arc<dyn HostWindow>,
    seen: Rc<RefCell<Vec<DragDropEvent>>>,
    _owner: flui_native::OwnerPlatform,
}

fn desktop(accept: Option<TransferActions>) -> Desktop {
    let platform = HeadlessPlatform::new().with_os_drag(OsDragStart::OnLeavingWindow);
    let dnd = platform.drag_and_drop();
    let transfer = platform.data_transfer();
    let saved = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&saved);
    Box::new(platform)
        .run(Box::new(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner: flui_native::OwnerPlatform = saved.borrow_mut().take().expect("started owner");
    let window = owner
        .open_window(WindowOptions::default())
        .expect("open")
        .try_ready()
        .expect("ready");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (target, answering) = (Rc::clone(&seen), Arc::clone(&transfer));
    window.on_input(Box::new(move |input| {
        if let PlatformInput::DragDrop(event) = input {
            let hovered = match &event {
                DragDropEvent::Entered { offer, .. } => Some(offer.id()),
                DragDropEvent::Moved { id, .. } => Some(*id),
                _ => None,
            };
            if let Some(id) = hovered {
                answering.update_drop_feedback(id, DropFeedback { accept });
            }
            target.borrow_mut().push(event);
        }
        DispatchEventResult::default()
    }));
    Desktop {
        dnd,
        transfer,
        window,
        seen,
        _owner: owner,
    }
}

impl Desktop {
    fn hover(&self) {
        self.dnd
            .hover(self.window.id(), Point::new(10.0, 10.0))
            .expect("a session runs");
    }

    /// The offer the window entered last.
    fn offer(&self) -> DataTransferId {
        self.seen
            .borrow()
            .iter()
            .rev()
            .find_map(|event| match event {
                DragDropEvent::Entered { offer, .. } => Some(offer.id()),
                _ => None,
            })
            .expect("the window was entered")
    }

    fn request(&self, id: DataTransferId) -> Result<TransferPayload, TransferError> {
        resolved(
            self.transfer
                .request(id, RepresentationIndex(0), TransferLimits::default()),
        )
    }

    /// Start a FLUI drag from the window, serving its one text representation
    /// from `provider`.
    fn start(&self, provider: OsDragProvider) {
        let source = self
            .window
            .drag_source()
            .expect("the window has a drag source");
        let session = IdMint::<DragSessionId>::new().next().expect("fresh mint");
        source
            .start(OsDragRequest {
                key: DragSessionKey::new(
                    ProcessNonce::from_raw(NonZeroU64::MIN),
                    UiRuntimeId::new(1),
                    session,
                ),
                pointer: None,
                representations: vec![RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                }],
                allowed: COPY,
                provider,
                on_end: Box::new(|_| {}),
            })
            .expect("the session starts");
    }
}

fn resolved(request: TransferRequest) -> Result<TransferPayload, TransferError> {
    let mut request = std::pin::pin!(request);
    match std::future::Future::poll(request.as_mut(), &mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("the double answers at once"),
    }
}

fn text(text: &str) -> Vec<(TransferFormat, TransferPayload)> {
    vec![(TransferFormat::Text, TransferPayload::Text(text.to_owned()))]
}

/// A drop takes one action, as the OS reports one effect: what the target
/// accepts among what the source permits, copy before move before link.
fn a_drop_takes_one_action_the_source_permits() {
    for (allowed, accept, taken) in [
        (COPY | MOVE, Some(COPY | MOVE), Some(COPY)),
        (MOVE | LINK, Some(COPY | MOVE | LINK), Some(MOVE)),
        (COPY | LINK, Some(LINK), Some(LINK)),
        (COPY, Some(MOVE), None),
        (COPY | MOVE, None, None),
    ] {
        let desktop = desktop(accept);
        desktop
            .dnd
            .start_foreign(text("dropped"), allowed)
            .expect("no session runs yet");
        desktop.hover();
        assert_eq!(
            desktop.dnd.drop_here(),
            Ok(taken),
            "{accept:?} within {allowed:?}"
        );
        let delivered = desktop.seen.borrow().last().map(|event| match event {
            DragDropEvent::Dropped { action, .. } => Some(*action),
            _ => None,
        });
        assert_eq!(delivered, Some(taken), "the window is told the same action");
        assert!(!desktop.dnd.is_active());
    }
}

/// A test that drives the double out of order gets an error, never a panic,
/// and the session already running is untouched.
fn driving_the_double_out_of_order_is_an_error() {
    let desktop = desktop(Some(COPY));
    assert_eq!(
        desktop.dnd.hover(desktop.window.id(), Point::new(1.0, 1.0)),
        Err(DragError::SessionClosed)
    );
    assert_eq!(desktop.dnd.drop_here(), Err(DragError::SessionClosed));
    desktop
        .dnd
        .start_foreign(text("first"), COPY)
        .expect("no session runs yet");
    assert_eq!(
        desktop.dnd.start_foreign(text("second"), MOVE),
        Err(DragError::Unavailable),
        "the OS runs one session at a time"
    );
    assert_eq!(
        desktop.dnd.advertised(),
        Some(COPY),
        "the first session runs on"
    );
    assert_eq!(
        desktop.dnd.drop_here(),
        Ok(None),
        "a release over no window cancels the drag"
    );
    assert!(!desktop.dnd.is_active());
}

/// A provider that panics fails the request it was serving, as a native
/// source fails it, and still serves the next one.
fn a_provider_that_panics_fails_its_request_and_serves_the_next() {
    let desktop = desktop(Some(COPY));
    let mut calls = 0;
    desktop.start(Box::new(move |_| {
        calls += 1;
        assert!(calls > 1, "the provider's first call fails");
        Ok(TransferPayload::Text("served".to_owned()))
    }));
    desktop.hover();
    let offer = desktop.offer();
    let failed = desktop.request(offer);
    assert!(
        matches!(failed, Err(TransferError::SourceGone)),
        "the failing call fails its request: {failed:?}"
    );
    let served = desktop.request(offer);
    assert!(
        matches!(&served, Ok(TransferPayload::Text(text)) if text == "served"),
        "the provider serves on: {served:?}"
    );
}

/// The target concluding a drop releases the provider's last owner: its
/// captures retire with no lock of the double held, so they may call it.
fn a_concluded_drop_retires_its_provider_outside_the_double() {
    struct Reentrant {
        dnd: HeadlessDragAndDrop,
        seen: std::sync::mpsc::Sender<usize>,
    }
    impl Drop for Reentrant {
        fn drop(&mut self) {
            let _ = self.seen.send(self.dnd.open_offers());
        }
    }
    let (sender, open_at_retirement) = std::sync::mpsc::channel();
    // A deadlock would hang this thread, not the table.
    std::thread::spawn(move || {
        let desktop = desktop(Some(COPY));
        let capture = Reentrant {
            dnd: desktop.dnd.clone(),
            seen: sender,
        };
        desktop.start(Box::new(move |_| {
            let _ = &capture;
            Ok(TransferPayload::Text("served".to_owned()))
        }));
        desktop.hover();
        let offer = desktop.offer();
        assert_eq!(desktop.dnd.drop_here(), Ok(Some(COPY)));
        assert_eq!(
            desktop.dnd.open_offers(),
            1,
            "open until the target concludes"
        );
        desktop.transfer.conclude_drop(offer);
    });
    assert_eq!(
        open_at_retirement.recv_timeout(Duration::from_secs(20)),
        Ok(0),
        "the provider's captures retire once the drop concluded, outside the double's lock"
    );
}

#[test]
fn headless_drag_contracts() {
    crate::run_table(
        "headless_drag_contracts",
        &[
            (
                "a_drop_takes_one_action_the_source_permits",
                a_drop_takes_one_action_the_source_permits,
            ),
            (
                "driving_the_double_out_of_order_is_an_error",
                driving_the_double_out_of_order_is_an_error,
            ),
            (
                "a_provider_that_panics_fails_its_request_and_serves_the_next",
                a_provider_that_panics_fails_its_request_and_serves_the_next,
            ),
            (
                "a_concluded_drop_retires_its_provider_outside_the_double",
                a_concluded_drop_retires_its_provider_outside_the_double,
            ),
        ],
    );
}
