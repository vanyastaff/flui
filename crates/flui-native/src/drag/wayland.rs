//! Drag and drop on the native Wayland host's windows, as targets and as
//! sources (`wl_data_device` up to version 3, ADR-0312, ADR-0211).
//!
//! [`SeatDrag`] is the platform's one [`DragTransport`] and the loop's
//! `WaylandDrag`. A window tells it as it opens and closes; the drags over
//! the windows reach it through the seats' data devices
//! (`transfer::linux::wayland`), whose dispatch records them, and
//! [`SeatDrag::deliver`] tells the windows on the loop's turn:
//!
//! - **Entering.** `enter` names the surface, the position and the offer,
//!   whose types its `offer` events named first. Over a window's surface the
//!   offer is minted at once and the window hears `Entered` with the
//!   position; over a surface of no window (a window's own frame) the drag
//!   is refused, and the offer kept until the drag leaves: destroying a
//!   drag's offer early cancels its source.
//! - **Hovering.** Each `motion` is `Moved`. After each the source is
//!   answered from the feedback the window's target cached: the action
//!   (`set_actions`, version 3) and the first type it would read
//!   (`accept`), or a refusal. Unlike XDND nothing asks again, so a change of
//!   feedback is answered as it is made, from whatever thread.
//! - **Dropping.** `drop` resolves the action the compositor chose (version
//!   3; copy before it) against the cached feedback; a refusal, or no
//!   action, ends the hover and the window hears `Exited`. An accepted drop
//!   keeps the offer until the target concludes it, which `finish`es it
//!   (version 3) and destroys it.
//! - **Fetching.** A request asks the offer to write the representation's
//!   type into a pipe (`receive`), which a thread of its own reads, bounded
//!   by the request's limit and [`TRANSFER_TIMEOUT`]. At most
//!   [`MAX_FETCHES`] read at once and more wait their turn; a fetch whose
//!   offer retires answers [`TransferError::SourceGone`] at once, and keeps
//!   its place until its thread ends.
//!
//! One drag is told at a time: a drag entering on another seat ends the
//! one before it. No method dispatches with the transfer's state locked, and
//! no completer is completed under it: a window's handler, which may ask for
//! the payload or conclude the drop at once, and a request's waker run with
//! nothing of the transfer held.
//!
//! A window starts a session from a press it holds (`source`): the seat's
//! device drags a data source of its own, which serves the session's
//! representations, one session at a time.

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::os::fd::AsFd as _;
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_platform::data_transfer::{
    DataTransferOffer, DragTransport, DropFeedback, RepresentationDescriptor, RepresentationIndex,
    TransferActions, TransferCompleter, TransferError, TransferLimits, TransferPayload,
    TransferRequest,
};
use flui_platform::drag::PlatformDragSource;
use flui_platform::{DragDropEvent, PlatformInput, WindowId};
use parking_lot::Mutex;
use wayland_client::Proxy as _;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_seat::WlSeat;
use web_time::Instant;

use crate::drag::offers::{Kind, OfferTable, representations, too_large};
use crate::owner::linux::wayland::{DragWindow, LastInput, WaylandDrag};
use crate::owner::slot::OwnerCell;
use crate::transfer::linux::wayland::pipe::{self, PipeError};
use crate::transfer::linux::wayland::{DataDevices, DragListener, DragWire, OfferData};

mod source;

/// How long a fetch waits for the source to write its answer.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);

/// The most fetches reading at once, a thread each; more wait.
const MAX_FETCHES: usize = 4;

/// `wl_data_offer.set_actions` and `finish` exist from version 3.
const ACTIONS_VERSION: u32 = 3;

/// The actions `actions` names as FLUI names them: copy and move; `ask`
/// names none.
pub(crate) fn transfer_actions(actions: DndAction) -> TransferActions {
    let mut named = TransferActions::empty();
    if actions.contains(DndAction::Copy) {
        named |= TransferActions::COPY;
    }
    if actions.contains(DndAction::Move) {
        named |= TransferActions::MOVE;
    }
    named
}

/// The action `action` is on the wire; link, which Wayland has no action
/// for, and none are none.
pub(crate) fn dnd_action(action: Option<TransferActions>) -> DndAction {
    match action {
        Some(action) if action == TransferActions::COPY => DndAction::Copy,
        Some(action) if action == TransferActions::MOVE => DndAction::Move,
        _ => DndAction::empty(),
    }
}

/// The actions the source of `offer` permits: what it named (version 3), copy
/// before it named any.
fn allowed_by(offer: &OfferData) -> TransferActions {
    offer
        .source_actions()
        .map_or(TransferActions::COPY, transfer_actions)
}

/// What a drag over the windows is over.
#[derive(Debug)]
enum Over {
    /// A surface of no window, or a window whose target concluded the drag
    /// while it hovered: the drag is refused, and its offer, if any, kept
    /// until it leaves.
    Nothing(Option<WlDataOffer>),
    /// A window, which heard `Entered` for this offer.
    Offer(DataTransferId),
}

/// The drag over the windows: one at a time.
#[derive(Debug)]
struct Hover {
    seat: WlSeat,
    over: Over,
}

/// A live offer, hovering or dropped.
#[derive(Debug)]
struct Offer {
    proxy: WlDataOffer,
    /// The window the drag is over or was dropped on.
    window: WindowId,
    /// The serial of the drag's `enter`, which an answer names.
    serial: u32,
    /// The type each representation is read as, by index.
    types: Arc<[(String, Kind)]>,
    feedback: DropFeedback,
    /// The actions the source permitted at the last event.
    allowed: TransferActions,
    /// The last position the window was told.
    position: Point<f64>,
    /// The last answer the source was sent: the action and the type
    /// accepted, `None` before the first.
    answered: Option<(Option<TransferActions>, Option<String>)>,
    /// The action the drop took, once dropped.
    dropped: Option<TransferActions>,
}

impl Offer {
    /// What the source is answered now: the action the feedback resolves,
    /// and the first type, when there is one to read.
    fn answer(&self) -> (Option<TransferActions>, Option<String>) {
        let mime = self.types.first().map(|(mime, _)| mime.clone());
        let action = self
            .feedback
            .resolve(self.allowed)
            .filter(|_| mime.is_some());
        (action, mime.filter(|_| action.is_some()))
    }

    /// Send the source the answer, unless it was sent already.
    fn send_answer(&mut self) {
        let answer = self.answer();
        if self.answered.as_ref() == Some(&answer) {
            return;
        }
        let (action, mime) = &answer;
        if self.proxy.version() >= ACTIONS_VERSION {
            let action = dnd_action(*action);
            self.proxy.set_actions(action, action);
        }
        self.proxy.accept(self.serial, mime.clone());
        self.answered = Some(answer);
    }

    /// The offer goes: `finish`ed when the drop took an action the source
    /// was told it accepts (version 3), then destroyed.
    fn end(self) {
        let accepted = self
            .answered
            .as_ref()
            .is_some_and(|(action, mime)| action.is_some() && mime.is_some());
        if self.dropped.is_some() && accepted && self.proxy.version() >= ACTIONS_VERSION {
            self.proxy.finish();
        }
        self.proxy.destroy();
    }
}

/// A request for one representation.
#[derive(Debug)]
struct Wanted {
    id: DataTransferId,
    mime: String,
    kind: Kind,
    limit: u64,
    completer: TransferCompleter,
}

/// A fetch whose thread reads the pipe.
#[derive(Debug)]
struct Fetch {
    ticket: u64,
    id: DataTransferId,
    /// `None` once its offer retired and the request was answered.
    completer: Option<TransferCompleter>,
}

/// What the transfer's lock guards.
#[derive(Default)]
struct State {
    table: OfferTable,
    windows: Vec<DragWindow>,
    hover: Option<Hover>,
    offers: HashMap<DataTransferId, Offer>,
    /// The fetches reading, at most [`MAX_FETCHES`].
    fetches: Vec<Fetch>,
    /// Requests waiting for a fetch to end.
    waiting: VecDeque<Wanted>,
    /// The last fetch's ticket.
    tickets: u64,
}

/// What a step leaves to do once the lock is let go: completers to resolve,
/// an event for a window, offers to end.
#[derive(Default)]
struct After {
    completions: Vec<(TransferCompleter, Result<TransferPayload, TransferError>)>,
    dispatch: Option<(WindowId, DragDropEvent)>,
    ended: Vec<Offer>,
    /// A request went out that another thread made: the loop sends it.
    sent: bool,
}

impl After {
    fn gone(&mut self, completer: TransferCompleter) {
        self.completions
            .push((completer, Err(TransferError::SourceGone)));
    }
}

impl State {
    /// Retire `id`: its fetches and waiting requests answer
    /// [`TransferError::SourceGone`], and the offer is handed back.
    fn retire(&mut self, id: DataTransferId, after: &mut After) -> Option<Offer> {
        let _ = self.table.retire(id);
        for fetch in self.fetches.iter_mut().filter(|fetch| fetch.id == id) {
            if let Some(completer) = fetch.completer.take() {
                after.gone(completer);
            }
        }
        let (stale, kept): (VecDeque<Wanted>, VecDeque<Wanted>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|wanted| wanted.id == id);
        self.waiting = kept;
        for wanted in stale {
            after.gone(wanted.completer);
        }
        self.offers.remove(&id)
    }

    /// End the drag over the windows: an offer not dropped retires, the
    /// window it was over hears `Exited`, and its proxy goes.
    fn end_hover(&mut self, after: &mut After) {
        let Some(hover) = self.hover.take() else {
            return;
        };
        match hover.over {
            Over::Nothing(offer) => {
                if let Some(offer) = offer {
                    offer.destroy();
                }
            }
            Over::Offer(id) => {
                if let Some(offer) = self.retire(id, after) {
                    if self.windows.iter().any(|window| window.id == offer.window) {
                        after.dispatch = Some((offer.window, DragDropEvent::Exited { id }));
                    }
                    after.ended.push(offer);
                }
            }
        }
    }

    /// The window `surface` is.
    fn window_of(
        &self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
    ) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|window| window.surface == *surface)
            .map(|window| window.id)
    }

    /// The live offer the drag is over, when it hovers over a window.
    fn hovering(&mut self, seat: &WlSeat) -> Option<(DataTransferId, &mut Offer)> {
        let hover = self.hover.as_ref().filter(|hover| hover.seat == *seat)?;
        let Over::Offer(id) = hover.over else {
            return None;
        };
        Some((id, self.offers.get_mut(&id)?))
    }
}

/// What the drag and its fetch threads share.
struct Shared {
    /// Wakes the loop, which sends what another thread asked of an offer.
    wake: Box<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    /// What the seats' devices recorded, delivered on the loop's turn.
    wires: OwnerCell<VecDeque<(WlSeat, DragWire)>>,
    /// The seats' data devices, which start a session's drag.
    devices: DataDevices,
    /// The seats' pointer grabs, which a session starts under.
    last_input: LastInput,
    /// The session the windows started, and what its source heard.
    sources: OwnerCell<source::Sources>,
    /// The writes of a session's data under way.
    writers: pipe::Writers,
}

/// The Wayland host's drag and drop: its windows as drag targets and as the
/// sources of drag sessions. See the module docs.
pub(crate) struct SeatDrag {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for SeatDrag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeatDrag").finish_non_exhaustive()
    }
}

impl DragListener for Shared {
    fn dragged(&self, seat: &WlSeat, wire: DragWire) {
        self.wires.with(|wires| {
            // Movements between two turns tell the last alone.
            if let (DragWire::Motion { .. }, Some((held, DragWire::Motion { .. }))) =
                (&wire, wires.back())
                && held == seat
            {
                wires.pop_back();
            }
            wires.push_back((seat.clone(), wire));
        });
    }
}

impl SeatDrag {
    /// The drag on the owner thread, hearing the drags through `devices`,
    /// starting sessions under the grabs `last_input` holds, and calling
    /// `wake` to have the loop send what another thread asked.
    pub(crate) fn new(
        devices: &DataDevices,
        last_input: LastInput,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared {
            wake,
            state: Mutex::new(State::default()),
            wires: OwnerCell::new(),
            devices: devices.clone(),
            last_input,
            sources: OwnerCell::new(),
            writers: pipe::Writers::new(source::MAX_WRITERS),
        });
        let listener: Weak<Shared> = Arc::downgrade(&shared);
        devices.listen_drags(listener);
        Arc::new(Self { shared })
    }

    /// Do what a step left, with no lock held; a panic of the window's
    /// handler is handed back once the step's completions are made.
    fn try_finish(&self, after: After) -> std::thread::Result<()> {
        for offer in after.ended {
            offer.end();
        }
        if after.sent {
            (self.shared.wake)();
        }
        // A request accepted before the handler failed is still answered.
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            if let Some((window, event)) = after.dispatch {
                let delivery = self
                    .shared
                    .state
                    .lock()
                    .windows
                    .iter()
                    .find(|held| held.id == window)
                    .map(|held| Arc::clone(&held.delivery));
                if let Some(delivery) = delivery {
                    delivery.dispatch_input(PlatformInput::DragDrop(event));
                }
            }
        });
        complete_all(&mut failure, after.completions);
        failure.into_payload().map_or(Ok(()), Err)
    }

    fn finish(&self, after: After) {
        if let Err(failure) = self.try_finish(after) {
            std::panic::resume_unwind(failure);
        }
    }

    /// Answer the source of the drag over `id` from what the feedback is now,
    /// unless the drag left it or dropped.
    fn answer(&self, id: DataTransferId) {
        let mut state = self.shared.state.lock();
        let hovering = state
            .hover
            .as_ref()
            .is_some_and(|hover| matches!(hover.over, Over::Offer(over) if over == id));
        if let Some(offer) = state.offers.get_mut(&id).filter(|_| hovering) {
            offer.send_answer();
        }
    }

    /// One recorded wire of `seat`'s device.
    fn wire(&self, seat: WlSeat, wire: DragWire) {
        match wire {
            DragWire::Enter {
                serial,
                surface,
                x,
                y,
                offer,
            } => self.enter(seat, serial, &surface, Point::new(x, y), offer),
            DragWire::Motion { x, y } => self.motion(&seat, Point::new(x, y)),
            DragWire::Leave => self.leave(&seat),
            DragWire::Drop => self.drop_on(&seat),
        }
    }

    /// A drag entered `surface` at `position`; one that never left is left
    /// first.
    fn enter(
        &self,
        seat: WlSeat,
        serial: u32,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        position: Point<f64>,
        offer: Option<WlDataOffer>,
    ) {
        let mut after = After::default();
        let entered = {
            let mut state = self.shared.state.lock();
            state.end_hover(&mut after);
            let window = state.window_of(surface);
            let data = offer.as_ref().and_then(|offer| offer.data::<OfferData>());
            if let (Some(window), Some(proxy), Some(data)) = (window, offer.clone(), data) {
                let names = data.types(<[String]>::to_vec).unwrap_or_default();
                let named: Vec<(usize, &str)> = names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (index, name.as_str()))
                    .collect();
                let types: Arc<[(String, Kind)]> = representations(&named)
                    .into_iter()
                    .map(|(index, kind)| (names[index].clone(), kind))
                    .collect();
                let described: Arc<[RepresentationDescriptor]> = types
                    .iter()
                    .map(|(_, kind)| RepresentationDescriptor {
                        format: kind.format(),
                        declared_len: None,
                    })
                    .collect();
                let allowed = allowed_by(data);
                let id = state.table.mint(Arc::clone(&described));
                state.offers.insert(
                    id,
                    Offer {
                        proxy,
                        window,
                        serial,
                        types,
                        feedback: DropFeedback::default(),
                        allowed,
                        position,
                        answered: None,
                        dropped: None,
                    },
                );
                state.hover = Some(Hover {
                    seat,
                    over: Over::Offer(id),
                });
                after.dispatch = Some((
                    window,
                    DragDropEvent::Entered {
                        offer: DataTransferOffer::new(id, described),
                        allowed,
                        position: Some(position),
                    },
                ));
                Some(id)
            } else {
                state.hover = Some(Hover {
                    seat,
                    over: Over::Nothing(offer),
                });
                None
            }
        };
        self.answer_after(entered, after);
    }

    /// The pointer moved to `position` over the surface the drag entered.
    fn motion(&self, seat: &WlSeat, position: Point<f64>) {
        let mut after = After::default();
        let moved = {
            let mut state = self.shared.state.lock();
            state.hovering(seat).map(|(id, offer)| {
                if let Some(data) = offer.proxy.data::<OfferData>() {
                    offer.allowed = allowed_by(data);
                }
                offer.position = position;
                after.dispatch = Some((
                    offer.window,
                    DragDropEvent::Moved {
                        id,
                        allowed: offer.allowed,
                        position,
                    },
                ));
                id
            })
        };
        self.answer_after(moved, after);
    }

    /// Tell the window what `after` holds, then answer the source of `id`
    /// from the feedback cached after; a handler that panicked gave none,
    /// and the source still hears what is cached before the panic goes on.
    fn answer_after(&self, id: Option<DataTransferId>, after: After) {
        let dispatched = self.try_finish(after);
        if let Some(id) = id {
            self.answer(id);
        }
        if let Err(failure) = dispatched {
            std::panic::resume_unwind(failure);
        }
    }

    /// The drag left the surface it entered, or its source gave up.
    fn leave(&self, seat: &WlSeat) {
        let mut after = After::default();
        {
            let mut state = self.shared.state.lock();
            if state
                .hover
                .as_ref()
                .is_some_and(|hover| hover.seat == *seat)
            {
                state.end_hover(&mut after);
            }
        }
        self.finish(after);
    }

    /// The user dropped: the window hears `Dropped` with the action the
    /// compositor chose, when the cached feedback accepts it, or `Exited`.
    fn drop_on(&self, seat: &WlSeat) {
        let mut after = After::default();
        let dropped = {
            let mut state = self.shared.state.lock();
            let accepted = state.hovering(seat).and_then(|(id, offer)| {
                let (action, _) = offer.answer();
                let action = action?;
                let chosen = if offer.proxy.version() >= ACTIONS_VERSION {
                    // The compositor's choice among what the source permits
                    // and the window was told it accepts.
                    let chosen = offer.proxy.data::<OfferData>()?.action()?;
                    transfer_actions(chosen)
                } else {
                    action
                };
                (chosen == action
                    && offer
                        .answered
                        .as_ref()
                        .is_some_and(|(_, mime)| mime.is_some()))
                .then_some((id, action))
            });
            if let Some((id, action)) = accepted {
                state.hover = None;
                let offer = state
                    .offers
                    .get_mut(&id)
                    .expect("BUG: a hovering offer is live");
                offer.dropped = Some(action);
                after.dispatch = Some((
                    offer.window,
                    DragDropEvent::Dropped {
                        id,
                        action,
                        position: Some(offer.position),
                    },
                ));
                Some(id)
            } else {
                if state
                    .hover
                    .as_ref()
                    .is_some_and(|hover| hover.seat == *seat)
                {
                    state.end_hover(&mut after);
                }
                None
            }
        };
        let dispatched = self.try_finish(after);
        if let (Err(_), Some(id)) = (&dispatched, dropped) {
            // A handler that panicked as it heard the drop will not conclude
            // it: the source hears it not taken.
            self.abandon_drop(id);
        }
        if let Err(failure) = dispatched {
            std::panic::resume_unwind(failure);
        }
    }

    /// Retire the dropped offer `id` the window will not conclude, and
    /// destroy it unfinished; an offer the window concluded already is gone.
    /// A failure on the way is discarded under containment: no caller resumes it.
    fn abandon_drop(&self, id: DataTransferId) {
        let mut after = After::default();
        {
            let mut state = self.shared.state.lock();
            if state
                .offers
                .get(&id)
                .is_some_and(|offer| offer.dropped.is_some())
                && let Some(mut offer) = state.retire(id, &mut after)
            {
                offer.dropped = None;
                after.ended.push(offer);
            }
        }
        if let Err(failure) = self.try_finish(after) {
            discard_payload(failure);
        }
    }

    /// Start the waiting requests the fetches have room for.
    fn start_waiting(&self, state: &mut State, after: &mut After) {
        while state.fetches.len() < MAX_FETCHES {
            let Some(wanted) = state.waiting.pop_front() else {
                return;
            };
            if wanted.completer.is_cancelled() {
                after.gone(wanted.completer);
                continue;
            }
            let Some(offer) = state.offers.get(&wanted.id) else {
                after.gone(wanted.completer);
                continue;
            };
            let (reader, writer) = match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
                Ok(ends) => ends,
                Err(error) => {
                    tracing::warn!(%error, "no pipe for a drop's transfer");
                    after.gone(wanted.completer);
                    continue;
                }
            };
            // The type as the source offered it.
            offer.proxy.receive(wanted.mime.clone(), writer.as_fd());
            // The source holds the write end once the request is sent.
            drop(writer);
            after.sent = true;
            state.tickets = state.tickets.wrapping_add(1);
            let ticket = state.tickets;
            let shared = Arc::downgrade(&self.shared);
            let (kind, limit) = (wanted.kind, wanted.limit);
            let spawned = std::thread::Builder::new()
                .name("flui-wayland-drop".to_owned())
                .spawn(move || {
                    let bytes = pipe::read(
                        reader,
                        Instant::now() + TRANSFER_TIMEOUT,
                        usize::try_from(limit).unwrap_or(usize::MAX),
                    );
                    fetched(&shared, ticket, outcome(&kind, limit, bytes));
                });
            match spawned {
                Ok(_) => state.fetches.push(Fetch {
                    ticket,
                    id: wanted.id,
                    completer: Some(wanted.completer),
                }),
                Err(error) => {
                    tracing::warn!(%error, "no thread for a drop's transfer");
                    after.gone(wanted.completer);
                }
            }
        }
    }
}

/// Answer every request a step accepted, each in its own containment: a waker
/// that panics leaves the rest answered. `failure` keeps the first panic, a
/// handler's included, and a later one is discarded under containment
/// (ADR-0281).
fn complete_all(
    failure: &mut FirstPanic,
    completions: Vec<(TransferCompleter, Result<TransferPayload, TransferError>)>,
) {
    for (completer, result) in completions {
        failure.contain(move || completer.complete(result));
    }
}

/// What a fetch of `kind` read comes to, held to `limit`: measured before
/// it is made, which can take more than the bytes read.
fn outcome(
    kind: &Kind,
    limit: u64,
    bytes: Result<Vec<u8>, PipeError>,
) -> Result<TransferPayload, TransferError> {
    match bytes {
        Ok(bytes) => {
            too_large(kind.decoded_len(&bytes), limit).map_or_else(|| Ok(kind.payload(bytes)), Err)
        }
        Err(PipeError::TooLarge { read }) => {
            Err(too_large(read, limit).unwrap_or(TransferError::SourceGone))
        }
        Err(PipeError::Timeout) => Err(TransferError::TimedOut),
        Err(PipeError::Failed) => Err(TransferError::SourceGone),
    }
}

/// The fetch `ticket` ended with `outcome`, on its thread: its request is
/// answered unless its offer retired first, and its place goes to the next.
fn fetched(shared: &Weak<Shared>, ticket: u64, outcome: Result<TransferPayload, TransferError>) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    let drag = SeatDrag { shared };
    let mut after = After::default();
    {
        let mut state = drag.shared.state.lock();
        let Some(at) = state
            .fetches
            .iter()
            .position(|fetch| fetch.ticket == ticket)
        else {
            return;
        };
        let fetch = state.fetches.swap_remove(at);
        if let Some(completer) = fetch.completer {
            after.completions.push((completer, outcome));
        }
        drag.start_waiting(&mut state, &mut after);
    }
    // No window hears anything here; a waker's panic stays on this thread.
    if let Err(failure) = drag.try_finish(after) {
        discard_payload(failure);
    }
}

impl WaylandDrag for SeatDrag {
    fn attach(&self, window: DragWindow) {
        self.shared.state.lock().windows.push(window);
    }

    fn detach(&self, id: WindowId) {
        let mut after = After::default();
        let removed = {
            let mut state = self.shared.state.lock();
            let Some(at) = state.windows.iter().position(|window| window.id == id) else {
                return;
            };
            let removed = state.windows.remove(at);
            // The drag over it ends with nothing to tell, its offer kept
            // until the drag leaves; a drop it took stays until its target
            // concludes it.
            let over = state.hover.as_ref().and_then(|hover| match hover.over {
                Over::Offer(over) => Some(over),
                Over::Nothing(_) => None,
            });
            if let Some(over) = over
                && state
                    .offers
                    .get(&over)
                    .is_some_and(|offer| offer.window == id)
                && let Some(offer) = state.retire(over, &mut after)
                && let Some(hover) = state.hover.as_mut()
            {
                hover.over = Over::Nothing(Some(offer.proxy));
            }
            removed
        };
        drop(removed);
        self.window_closed(id);
        self.finish(after);
    }

    fn deliver(&self) {
        while let Some((seat, wire)) = self.shared.wires.with(VecDeque::pop_front).flatten() {
            self.wire(seat, wire);
        }
        self.deliver_sources();
    }

    fn source(self: Arc<Self>, window: WindowId) -> Arc<dyn PlatformDragSource> {
        Arc::new(source::WindowDragSource { drag: self, window })
    }

    fn release(&self) {
        let mut after = After::default();
        let windows = {
            let mut state = self.shared.state.lock();
            if let Some(hover) = state.hover.take()
                && let Over::Nothing(Some(offer)) = hover.over
            {
                offer.destroy();
            }
            let ids: Vec<DataTransferId> = state.offers.keys().copied().collect();
            for id in ids {
                if let Some(mut offer) = state.retire(id, &mut after) {
                    offer.dropped = None;
                    after.ended.push(offer);
                }
            }
            std::mem::take(&mut state.windows)
        };
        self.shared.wires.with(VecDeque::clear);
        drop(windows);
        self.release_session();
        if let Err(failure) = self.try_finish(after) {
            discard_payload(failure);
        }
    }
}

impl DragTransport for SeatDrag {
    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let mut after = After::default();
        let request = {
            let mut state = self.shared.state.lock();
            let Some(offer) = state.offers.get(&id) else {
                return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
            };
            let Some((mime, kind)) = offer.types.get(usize::from(representation.0)).cloned() else {
                return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                    id,
                    index: representation,
                }));
            };
            let (request, completer) = TransferRequest::pending();
            state.waiting.push_back(Wanted {
                id,
                mime,
                kind,
                limit: limits.max_bytes,
                completer,
            });
            self.start_waiting(&mut state, &mut after);
            request
        };
        self.finish(after);
        request
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        let changed = {
            let mut state = self.shared.state.lock();
            state
                .offers
                .get_mut(&id)
                .filter(|offer| offer.dropped.is_none())
                .map(|offer| offer.feedback = feedback)
                .is_some()
        };
        if changed {
            // Nothing asks again: the source hears the change now.
            self.answer(id);
            (self.shared.wake)();
        }
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let mut after = After::default();
        {
            let mut state = self.shared.state.lock();
            if let Some(offer) = state.retire(id, &mut after) {
                match state.hover.as_mut() {
                    // Concluded while it hovers: refused from now on, its
                    // offer kept until the drag leaves.
                    Some(hover) if matches!(hover.over, Over::Offer(over) if over == id) => {
                        hover.over = Over::Nothing(Some(offer.proxy));
                    }
                    _ => {
                        after.ended.push(offer);
                        after.sent = true;
                    }
                }
            }
        }
        self.finish(after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copy and move cross both ways; ask and link name no action of the
    /// other side.
    #[test]
    fn actions_map_both_ways() {
        assert_eq!(
            transfer_actions(DndAction::Copy | DndAction::Move | DndAction::Ask),
            TransferActions::COPY | TransferActions::MOVE
        );
        assert_eq!(transfer_actions(DndAction::Ask), TransferActions::empty());
        for action in [TransferActions::COPY, TransferActions::MOVE] {
            assert_eq!(transfer_actions(dnd_action(Some(action))), action);
        }
        assert_eq!(dnd_action(Some(TransferActions::LINK)), DndAction::empty());
        assert_eq!(dnd_action(None), DndAction::empty());
    }

    /// A panic payload that counts its drops.
    struct Counted {
        first: bool,
        drops: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Requests whose wakers panic leave the rest answered. The first panic is
    /// kept and a later one is destroyed under containment, not leaked.
    #[test]
    fn a_waker_that_panics_leaves_the_rest_answered_and_a_later_panic_destroyed() {
        use std::future::Future as _;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::task::{Context, Poll, Waker};

        let drops = Arc::new(AtomicUsize::new(0));
        let failing = |first| {
            let drops = Arc::clone(&drops);
            // The payload is a [`Counted`], so the test sees which panic was kept.
            flui_foundation::testing::CountingWaker::calling(move || {
                std::panic::panic_any(Counted {
                    first,
                    drops: Arc::clone(&drops),
                })
            })
            .shared()
            .waker()
        };
        let wakers = [failing(true), Waker::noop().clone(), failing(false)];
        let mut requests = Vec::new();
        let mut completions = Vec::new();
        for waker in &wakers {
            let (mut request, completer) = TransferRequest::pending();
            assert!(
                std::pin::Pin::new(&mut request)
                    .poll(&mut Context::from_waker(waker))
                    .is_pending()
            );
            requests.push(request);
            completions.push((completer, Err(TransferError::SourceGone)));
        }
        drop(wakers);

        let mut failure = FirstPanic::new();
        complete_all(&mut failure, completions);

        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "the later panic's payload was destroyed"
        );
        let kept = failure.into_payload().expect("the first panic is kept");
        assert!(
            kept.downcast_ref::<Counted>()
                .is_some_and(|payload| payload.first),
            "the kept panic is the first"
        );
        discard_payload(kept);
        for request in &mut requests {
            assert!(
                matches!(
                    std::pin::Pin::new(request).poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Err(TransferError::SourceGone))
                ),
                "every request is answered"
            );
        }
    }
}
