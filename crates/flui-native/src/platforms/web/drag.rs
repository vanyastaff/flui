//! HTML drag and drop, as target and as source (ADR-0038, ADR-0211 §11).
//!
//! [`WebTransfer`] is the platform's one [`DataTransferSource`]. A drag over
//! a window's canvas, from the page, another page or the OS, mints an offer
//! from the drag's `DataTransfer.types` and files. The browser keeps the data
//! out of reach until the drop, so a hover reads only the types (FLUI's
//! session identity rides in its type's parameters), and the drop copies
//! every announced representation out of the `DataTransfer` before the drop
//! event returns; requests are then served from memory, files read
//! asynchronously. Every answer the browser waits on is given in its event:
//! `dragenter` and `dragover` are cancelled only when the input callback
//! prevents their default and, for `dragover`, the target's cached feedback
//! accepts, with `dropEffect` naming the one accepted action.
//!
//! A window's [`WebDragSource`] starts an HTML drag. The browser starts one
//! only from its own drag gesture on a draggable element, decided at its
//! `dragstart`, and allows no data later, so the canvas is draggable while a
//! source is installed and `dragstart` asks, synchronously, whether a session
//! is waiting: one the runtime started on this press before the browser's
//! drag threshold, which then stores its representations eagerly. Otherwise
//! `dragstart` is cancelled, the press goes on as ordinary pointer input, and
//! a session the runtime starts later in that press is refused as
//! `DragError::Unavailable`, so it stays in its window.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform::DragDropEvent;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, Mime, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferError, TransferFormat,
    TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform::drag::{DragError, OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource};
use parking_lot::Mutex;
use wasm_bindgen::JsCast;

use crate::host;
use crate::owner::panic_boundary::contain_owner_callback;
use crate::shared::WindowCallbacks;
use crate::shared::html_drag_drop::{
    DomSource, FILES_TYPE, action_of, allowed_of, drop_effect, effect_allowed_of, offered,
    payload_of, payload_of_bytes, serve_stored,
};
use flui_platform::PlatformInput;

use super::events::{answer, pointer_position};
use super::platform::WebLoop;
use super::registration::Registration;

/// One announced representation, before and after the drop.
enum Held {
    /// Hovering: where it will come from.
    Announced(DomSource),
    /// Copied out at the drop.
    Payload(TransferPayload),
    /// A file of the drop, read when requested.
    File(web_sys::File),
    /// Announced but absent from the drop.
    Missing,
}

/// The offers minted for drags over this platform's canvases.
#[derive(Default)]
struct Offers {
    table: OfferTable,
    /// The answer each live offer's target last cached.
    feedback: HashMap<DataTransferId, DropFeedback>,
    /// What each live offer holds, by representation index.
    held: HashMap<DataTransferId, Vec<Held>>,
}

/// The platform's HTML drag-and-drop transport (see the module doc).
#[derive(Default)]
pub struct WebTransfer {
    offers: Mutex<Offers>,
}

impl std::fmt::Debug for WebTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransfer").finish_non_exhaustive()
    }
}

// SAFETY: the page has one thread (the module refuses `+atomics`), so the JS
// handles a held offer keeps (its files) are never reached from another; the
// trait's bounds are those every backend's transport meets.
unsafe impl Send for WebTransfer {}
// SAFETY: as for `Send`: there is no second thread to share it with.
unsafe impl Sync for WebTransfer {}

impl WebTransfer {
    fn open_offer(&self, offered: Vec<(DomSource, RepresentationDescriptor)>) -> DataTransferOffer {
        let (sources, representations): (Vec<_>, Vec<_>) = offered.into_iter().unzip();
        let representations: Arc<[RepresentationDescriptor]> = Arc::from(representations);
        let mut offers = self.offers.lock();
        let id = offers
            .table
            .mint(OfferRecord::new(Arc::clone(&representations)));
        offers.feedback.insert(id, DropFeedback::default());
        offers
            .held
            .insert(id, sources.into_iter().map(Held::Announced).collect());
        DataTransferOffer::new(id, representations)
    }

    fn feedback(&self, id: DataTransferId) -> DropFeedback {
        self.offers
            .lock()
            .feedback
            .get(&id)
            .copied()
            .unwrap_or_default()
    }

    /// Copy what `id` announced out of the drop's `transfer`, while the
    /// browser still allows it.
    fn materialize(&self, id: DataTransferId, transfer: &web_sys::DataTransfer) {
        let sources: Vec<(usize, DomSource)> = {
            let offers = self.offers.lock();
            let Some(held) = offers.held.get(&id) else {
                return;
            };
            held.iter()
                .enumerate()
                .filter_map(|(index, held)| match held {
                    Held::Announced(source) => Some((index, source.clone())),
                    _ => None,
                })
                .collect()
        };
        let representations = {
            let offers = self.offers.lock();
            offers
                .table
                .get(id)
                .map(|record| Arc::clone(record.representations()))
        };
        let Some(representations) = representations else {
            return;
        };
        // DOM reads run outside the lock: a getter can reach page script.
        let files = transfer.files();
        let read: Vec<(usize, Held)> = sources
            .into_iter()
            .map(|(index, source)| {
                let held = match source {
                    DomSource::Data(dom_type) => match transfer.get_data(&dom_type) {
                        Ok(text) => Held::Payload(payload_of(&representations[index].format, text)),
                        Err(_) => Held::Missing,
                    },
                    DomSource::File(at) => files
                        .as_ref()
                        .and_then(|files| files.get(u32::try_from(at).ok()?))
                        .map_or(Held::Missing, Held::File),
                };
                (index, held)
            })
            .collect();
        let mut offers = self.offers.lock();
        if let Some(held) = offers.held.get_mut(&id) {
            for (index, value) in read {
                held[index] = value;
            }
        }
    }

    fn close_offer(&self, id: DataTransferId) {
        let removed = {
            let mut offers = self.offers.lock();
            let _ = offers.table.retire(id);
            offers.feedback.remove(&id);
            offers.held.remove(&id)
        };
        drop(removed);
    }
}

impl host::Drag<WebLoop> for WebTransfer {
    fn data_transfer(owner: &WebLoop) -> Arc<dyn DataTransferSource> {
        // HTML drag and drop; `clipboard_offer` stays `None`, since the
        // clipboard reads only its own writes.
        owner.with_state(|s| Arc::clone(&s.transfer)) as Arc<dyn DataTransferSource>
    }
}

impl DataTransferSource for WebTransfer {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        enum Found {
            Payload(TransferPayload),
            File(web_sys::File, TransferFormat),
        }
        let found = {
            let offers = self.offers.lock();
            let Some(held) = offers.held.get(&id) else {
                return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
            };
            match held.get(usize::from(representation.0)) {
                None => {
                    return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                        id,
                        index: representation,
                    }));
                }
                Some(Held::Payload(payload)) => Found::Payload(payload.clone()),
                Some(Held::File(file)) => {
                    // The format the file was announced as, image or bytes.
                    let format = offers
                        .table
                        .get(id)
                        .and_then(|record| {
                            record
                                .representations()
                                .get(usize::from(representation.0))
                                .map(|descriptor| descriptor.format.clone())
                        })
                        .unwrap_or_else(|| TransferFormat::Custom {
                            mime: Mime::new("application/octet-stream"),
                        });
                    Found::File(file.clone(), format)
                }
                // Before the drop the browser keeps the data from every
                // page; a missing one was announced but not delivered.
                Some(Held::Announced(_) | Held::Missing) => {
                    return TransferRequest::ready(Err(TransferError::SourceGone));
                }
            }
        };
        match found {
            Found::Payload(payload) => {
                let actual = payload.byte_len();
                TransferRequest::ready(if actual > limits.max_bytes {
                    Err(TransferError::TooLarge {
                        actual,
                        limit: limits.max_bytes,
                    })
                } else {
                    Ok(payload)
                })
            }
            Found::File(file, format) => {
                let size = file.size() as u64;
                if size > limits.max_bytes {
                    return TransferRequest::ready(Err(TransferError::TooLarge {
                        actual: size,
                        limit: limits.max_bytes,
                    }));
                }
                let (request, completer) = TransferRequest::channel();
                js_sys::futures::spawn_local(async move {
                    let result = js_sys::futures::JsFuture::from(file.array_buffer())
                        .await
                        .map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec())
                        .map_err(|_| TransferError::SourceGone)
                        .and_then(|bytes| {
                            if bytes.len() as u64 > limits.max_bytes {
                                Err(TransferError::TooLarge {
                                    actual: bytes.len() as u64,
                                    limit: limits.max_bytes,
                                })
                            } else {
                                Ok(payload_of_bytes(&format, bytes))
                            }
                        });
                    completer.complete(result);
                });
                request
            }
        }
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        if let Some(cached) = self.offers.lock().feedback.get_mut(&id) {
            *cached = feedback;
        }
    }

    fn conclude_drop(&self, id: DataTransferId) {
        self.close_offer(id);
    }
}

/// What a canvas's drag source is doing.
#[derive(Default)]
enum Outgoing {
    /// Nothing started.
    #[default]
    Idle,
    /// The runtime started a session on this press; the browser's
    /// `dragstart` has not come yet.
    Waiting(OsDragRequest),
    /// The browser's drag runs.
    Running {
        allowed: TransferActions,
        on_end: flui_platform::drag::OsDragEndHook,
    },
}

/// A canvas's HTML drag source (see the module doc).
pub(super) struct WebDragSource {
    outgoing: RefCell<Outgoing>,
    /// A mouse or pen button is down on the canvas.
    pressed: Cell<bool>,
    /// The browser's `dragstart` of this press was cancelled: no HTML drag
    /// can start before the next press.
    refused: Cell<bool>,
    /// Whether the window still accepts sessions.
    open: Cell<bool>,
}

// SAFETY: the page has one thread (the module refuses `+atomics`); the request
// a waiting session holds (its provider and hook) is reached only from the
// owner thread there is.
unsafe impl Send for WebDragSource {}
// SAFETY: as for `Send`.
unsafe impl Sync for WebDragSource {}

impl WebDragSource {
    fn new() -> Self {
        Self {
            outgoing: RefCell::new(Outgoing::Idle),
            pressed: Cell::new(false),
            refused: Cell::new(false),
            open: Cell::new(true),
        }
    }

    /// End whatever session waits or runs, told it was cancelled.
    pub(super) fn retire(&self) {
        self.open.set(false);
        self.end(OsDragEnd::Cancelled);
    }

    /// End the waiting or running session with `end`, outside the borrow.
    fn end(&self, end: OsDragEnd) {
        let outgoing = self.outgoing.replace(Outgoing::Idle);
        match outgoing {
            Outgoing::Idle => {}
            Outgoing::Waiting(request) => {
                let OsDragRequest {
                    provider, on_end, ..
                } = request;
                contain_owner_callback(|| drop(provider));
                contain_owner_callback(|| on_end(OsDragEnd::Cancelled));
            }
            Outgoing::Running { on_end, .. } => {
                contain_owner_callback(|| on_end(end));
            }
        }
    }

    /// The browser's `dragstart`: run the waiting session as an HTML drag,
    /// storing every representation in `transfer`, or refuse the drag.
    /// Returns whether it started.
    fn begin(&self, transfer: Option<&web_sys::DataTransfer>) -> bool {
        let waiting = std::mem::take(&mut *self.outgoing.borrow_mut());
        let Outgoing::Waiting(request) = waiting else {
            *self.outgoing.borrow_mut() = waiting;
            self.refused.set(true);
            return false;
        };
        let OsDragRequest {
            representations,
            allowed,
            mut provider,
            on_end,
            ..
        } = request;
        let Some(transfer) = transfer else {
            contain_owner_callback(|| drop(provider));
            contain_owner_callback(|| on_end(OsDragEnd::Cancelled));
            self.refused.set(true);
            return false;
        };
        let served = serve_stored(&representations, &mut provider);
        contain_owner_callback(|| drop(provider));
        // A provider that panicked ends its session here, before the drag
        // is refused: no later `dragend` would reach it.
        let Ok(served) = served else {
            contain_owner_callback(|| on_end(OsDragEnd::Cancelled));
            self.refused.set(true);
            return false;
        };
        for (dom_type, text) in served {
            if let Err(error) = transfer.set_data(&dom_type, &text) {
                tracing::debug!(?error, dom_type, "the browser refused a representation");
            }
        }
        transfer.set_effect_allowed(effect_allowed_of(allowed));
        *self.outgoing.borrow_mut() = Outgoing::Running { allowed, on_end };
        true
    }
}

impl PlatformDragSource for WebDragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::shared::input_availability::WEB_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let startable = self.open.get()
            && self.pressed.get()
            && !self.refused.get()
            && matches!(*self.outgoing.borrow(), Outgoing::Idle);
        if !startable {
            // The request is dropped here, outside any borrow: the session
            // stays in its window.
            let OsDragRequest {
                provider, on_end, ..
            } = request;
            contain_owner_callback(|| drop(provider));
            drop(on_end);
            return Err(DragError::Unavailable);
        }
        *self.outgoing.borrow_mut() = Outgoing::Waiting(request);
        Ok(())
    }
}

/// The canvas's `draggable` as the page left it. Read through `Reflect`
/// rather than the typed getter, which throws on a canvas-like object whose
/// property is missing or not a boolean; such a canvas counts as not draggable.
fn page_draggable(canvas: &web_sys::HtmlCanvasElement) -> bool {
    js_sys::Reflect::get(canvas, &wasm_bindgen::JsValue::from_str("draggable"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// The source and target of one canvas, from registration to retirement.
pub(super) struct CanvasDrag {
    pub(super) source: Arc<WebDragSource>,
    canvas: web_sys::HtmlCanvasElement,
    was_draggable: bool,
}

impl CanvasDrag {
    /// Make the canvas draggable, once its whole registration succeeded.
    pub(super) fn arm(&self) {
        self.canvas.set_draggable(true);
    }

    /// Put the canvas's draggable attribute back as the page had it.
    pub(super) fn retire(&self) {
        self.source.retire();
        self.canvas.set_draggable(self.was_draggable);
    }
}

/// The offer hovering a canvas and the actions its drag permits.
type Hover = Rc<Cell<Option<(DataTransferId, TransferActions)>>>;

fn transfer_of(event: &web_sys::Event) -> Option<web_sys::DataTransfer> {
    event
        .dyn_ref::<web_sys::DragEvent>()
        .and_then(web_sys::DragEvent::data_transfer)
}

fn drag_position(event: &web_sys::Event) -> Point {
    pointer_position(event.unchecked_ref())
}

/// What a hovering drag offers.
fn offered_by(transfer: &web_sys::DataTransfer) -> Vec<(DomSource, RepresentationDescriptor)> {
    let types: Vec<String> = transfer
        .types()
        .iter()
        .filter_map(|value| value.as_string())
        .filter(|dom_type| dom_type != FILES_TYPE)
        .collect();
    let items = transfer.items();
    let file_types: Vec<String> = (0..items.length())
        .filter_map(|index| items.get(index))
        .filter(|item| item.kind() == "file")
        .map(|item| item.type_())
        .collect();
    offered(
        types.iter().map(String::as_str),
        file_types.iter().map(String::as_str),
    )
}

/// Register the canvas's drop target and drag source listeners.
pub(super) fn register(
    canvas: &web_sys::HtmlCanvasElement,
    transfer: &Arc<WebTransfer>,
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut Registration,
) -> Result<CanvasDrag, crate::OpenWindowError> {
    let hover: Hover = Rc::default();
    let source = Arc::new(WebDragSource::new());

    // The target.
    for kind in ["dragenter", "dragover"] {
        let (transfer, callbacks, hover) = (
            Arc::clone(transfer),
            Arc::clone(callbacks),
            Rc::clone(&hover),
        );
        registration.listen(canvas.as_ref(), kind, move |event| {
            let Some(data) = transfer_of(&event) else {
                return;
            };
            let allowed = allowed_of(&data.effect_allowed());
            let position = drag_position(&event);
            let (input, id) = if let Some((id, _)) = hover.get() {
                (
                    DragDropEvent::Moved {
                        id,
                        allowed,
                        position,
                    },
                    id,
                )
            } else {
                let offer = transfer.open_offer(offered_by(&data));
                let id = offer.id();
                (
                    DragDropEvent::Entered {
                        offer,
                        allowed,
                        position: Some(position),
                    },
                    id,
                )
            };
            hover.set(Some((id, allowed)));
            let result = callbacks.dispatch_input(PlatformInput::DragDrop(input));
            let effect = drop_effect(transfer.feedback(id), allowed);
            data.set_drop_effect(effect);
            // Cancelling `dragenter` makes the canvas the drag's target;
            // cancelling `dragover` accepts the drop with `dropEffect`.
            let accepts = event.type_() == "dragenter" || effect != "none";
            answer(
                &event,
                flui_platform::DispatchEventResult::resolved(
                    result.propagate,
                    result.default_prevented && accepts,
                ),
            );
        })?;
    }
    {
        let (transfer, callbacks, hover) = (
            Arc::clone(transfer),
            Arc::clone(callbacks),
            Rc::clone(&hover),
        );
        registration.listen(canvas.as_ref(), "dragleave", move |event| {
            let Some((id, _)) = hover.take() else {
                return;
            };
            let result =
                callbacks.dispatch_input(PlatformInput::DragDrop(DragDropEvent::Exited { id }));
            transfer.close_offer(id);
            answer(&event, result);
        })?;
    }
    {
        let (transfer, callbacks, hover) = (
            Arc::clone(transfer),
            Arc::clone(callbacks),
            Rc::clone(&hover),
        );
        registration.listen(canvas.as_ref(), "drop", move |event| {
            let Some((id, allowed)) = hover.take() else {
                return;
            };
            let action = action_of(drop_effect(transfer.feedback(id), allowed));
            if action.is_empty() {
                let result =
                    callbacks.dispatch_input(PlatformInput::DragDrop(DragDropEvent::Exited { id }));
                transfer.close_offer(id);
                answer(&event, result);
                return;
            }
            if let Some(data) = transfer_of(&event) {
                transfer.materialize(id, &data);
            }
            let result =
                callbacks.dispatch_input(PlatformInput::DragDrop(DragDropEvent::Dropped {
                    id,
                    action,
                    position: Some(drag_position(&event)),
                }));
            answer(&event, result);
        })?;
    }

    // The source.
    {
        let source = Arc::clone(&source);
        registration.listen(canvas.as_ref(), "pointerdown", move |event| {
            let pointer: &web_sys::PointerEvent = event.unchecked_ref();
            if pointer.pointer_type() != "touch" {
                source.pressed.set(true);
                source.refused.set(false);
            }
        })?;
    }
    for kind in ["pointerup", "pointercancel"] {
        let source = Arc::clone(&source);
        registration.listen(canvas.as_ref(), kind, move |_| {
            source.pressed.set(false);
            // A session no `dragstart` took by the release never runs.
            if matches!(*source.outgoing.borrow(), Outgoing::Waiting(_)) {
                source.end(OsDragEnd::Cancelled);
            }
        })?;
    }
    {
        let source = Arc::clone(&source);
        registration.listen(canvas.as_ref(), "dragstart", move |event| {
            if !source.begin(transfer_of(&event).as_ref()) {
                event.prevent_default();
            }
        })?;
    }
    {
        let source = Arc::clone(&source);
        registration.listen(canvas.as_ref(), "dragend", move |event| {
            let effect = transfer_of(&event).map(|data| data.drop_effect());
            let allowed = match &*source.outgoing.borrow() {
                Outgoing::Running { allowed, .. } => *allowed,
                _ => return,
            };
            let action = effect
                .as_deref()
                .map_or(TransferActions::empty(), action_of)
                & allowed;
            source.end(if action.is_empty() {
                OsDragEnd::Cancelled
            } else {
                OsDragEnd::Dropped(action)
            });
        })?;
    }
    Ok(CanvasDrag {
        source,
        canvas: canvas.clone(),
        was_draggable: page_draggable(canvas),
    })
}
