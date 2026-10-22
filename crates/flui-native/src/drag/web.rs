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
//!
//! How `DataTransfer` types and effect names map to transfer formats and
//! actions is decided in [`mapping`] with no DOM call, so every host compiles
//! and checks it; the DOM side around it is the browser's alone.

// The page has one thread, so the transport and the drag source claim
// `Send`/`Sync` by hand: the workspace lint `unsafe_code = "warn"` is opted out
// here, at the module boundary, rather than for the whole crate (see `lib.rs`).
// Its pure part forbids it.
#![cfg_attr(target_arch = "wasm32", expect(unsafe_code))]

/// The decidable half of the backend (ADR-0038, ADR-0211 §11): how
/// `DataTransfer` types and effect names map to transfer formats and actions,
/// and what string a payload is stored as. Plain functions, so every host
/// checks them; the DOM side around them moves the values in and out of the
/// page.
///
/// The browser carries every representation as a string keyed by its
/// (lowercased) type, plus files. FLUI's session identity travels under its
/// full media type, parameters included, so a hover reads it from `types`
/// while the data itself is still out of reach.
#[forbid(unsafe_code)]
mod mapping {
    use flui_platform::data_transfer::{
        DropFeedback, Mime, RepresentationDescriptor, RepresentationIndex, TransferActions,
        TransferFormat, TransferPayload, TransferUri,
    };
    use flui_platform::drag::OsDragProvider;

    use crate::drag::offers::{format_of, payload_of_bytes};

    /// The actions an `effectAllowed` value permits. `uninitialized` (a drag
    /// from outside the page) and unknown values permit every action, as the
    /// browser does for them.
    #[must_use]
    pub(super) fn allowed_of(effect_allowed: &str) -> TransferActions {
        match effect_allowed {
            "none" => TransferActions::empty(),
            "copy" => TransferActions::COPY,
            "move" => TransferActions::MOVE,
            "link" => TransferActions::LINK,
            "copyMove" => TransferActions::COPY | TransferActions::MOVE,
            "copyLink" => TransferActions::COPY | TransferActions::LINK,
            "linkMove" => TransferActions::LINK | TransferActions::MOVE,
            _ => TransferActions::all(),
        }
    }

    /// The `effectAllowed` value that permits `actions`; `"none"` for none.
    #[must_use]
    pub(super) fn effect_allowed_of(actions: TransferActions) -> &'static str {
        let (copy, moves, link) = (
            actions.contains(TransferActions::COPY),
            actions.contains(TransferActions::MOVE),
            actions.contains(TransferActions::LINK),
        );
        match (copy, moves, link) {
            (true, true, true) => "all",
            (true, true, false) => "copyMove",
            (true, false, true) => "copyLink",
            (false, true, true) => "linkMove",
            (true, false, false) => "copy",
            (false, true, false) => "move",
            (false, false, true) => "link",
            (false, false, false) => "none",
        }
    }

    /// The `dropEffect` a target answers with: the one action its cached
    /// feedback accepts among those the drag permits (copy, then move, then
    /// link), or `"none"`, which refuses the drop.
    #[must_use]
    pub(super) fn drop_effect(feedback: DropFeedback, allowed: TransferActions) -> &'static str {
        match feedback.resolve(allowed) {
            Some(action) if action == TransferActions::COPY => "copy",
            Some(action) if action == TransferActions::MOVE => "move",
            Some(action) if action == TransferActions::LINK => "link",
            _ => "none",
        }
    }

    /// The action a `dropEffect` names; empty for `"none"` and unknown values.
    #[must_use]
    pub(super) fn action_of(drop_effect: &str) -> TransferActions {
        match drop_effect {
            "copy" => TransferActions::COPY,
            "move" => TransferActions::MOVE,
            "link" => TransferActions::LINK,
            _ => TransferActions::empty(),
        }
    }

    /// Where one announced representation comes from at the drop.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum DomSource {
        /// `getData` of this type.
        Data(String),
        /// The drag's file at this index of `DataTransfer.files`.
        File(usize),
    }

    /// The representations a hovering drag offers, in the browser's order: one
    /// per string type FLUI announces (the identity with its parameters, its
    /// declared length zero), then one per file, by its media type
    /// (`application/octet-stream` when the browser has none).
    #[must_use]
    pub(super) fn offered<'a>(
        types: impl IntoIterator<Item = &'a str>,
        file_types: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(DomSource, RepresentationDescriptor)> {
        let mut offered: Vec<(DomSource, RepresentationDescriptor)> = Vec::new();
        for dom_type in types {
            let Some(format) = format_of(dom_type) else {
                continue;
            };
            if offered.iter().any(|(_, known)| known.format == format) {
                continue;
            }
            let declared_len = format.is_drag_identity().then_some(0);
            offered.push((
                DomSource::Data(dom_type.to_owned()),
                RepresentationDescriptor {
                    format,
                    declared_len,
                },
            ));
        }
        for (index, file_type) in file_types.into_iter().enumerate() {
            let mime = if file_type.is_empty() {
                "application/octet-stream"
            } else {
                file_type
            };
            let format = if mime.starts_with("image/") {
                TransferFormat::Image {
                    mime: Mime::new(mime),
                }
            } else {
                TransferFormat::Custom {
                    mime: Mime::new(mime),
                }
            };
            offered.push((
                DomSource::File(index),
                RepresentationDescriptor {
                    format,
                    declared_len: None,
                },
            ));
        }
        offered
    }

    /// The payload a `getData` string carries for `format`.
    #[must_use]
    pub(super) fn payload_of(format: &TransferFormat, text: String) -> TransferPayload {
        match format {
            TransferFormat::Text => TransferPayload::Text(text),
            TransferFormat::Html => TransferPayload::Html(text),
            // RFC 2483: one URI per CRLF-separated line, `#` lines are comments.
            TransferFormat::UriList => TransferPayload::UriList(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(|line| TransferUri::Uri(line.to_owned()))
                    .collect(),
            ),
            TransferFormat::Image { .. } | TransferFormat::Custom { .. } => {
                payload_of_bytes(format, text.into_bytes())
            }
        }
    }

    /// A drag's provider panicked while it served a representation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct ProviderPanicked;

    /// What a source stores at `dragstart`: the `DataTransfer` type and string of
    /// each of `representations` that `provider` serves and the browser can
    /// carry, in order. A representation the provider refuses is left out.
    ///
    /// # Errors
    ///
    /// [`ProviderPanicked`] when the provider panics: the panic is contained and
    /// reported here, and nothing after it is asked for. The caller still owns
    /// the session and ends it.
    pub(super) fn serve_stored(
        representations: &[RepresentationDescriptor],
        provider: &mut OsDragProvider,
    ) -> Result<Vec<(String, String)>, ProviderPanicked> {
        let mut out = Vec::new();
        for (index, descriptor) in representations.iter().enumerate() {
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                provider(RepresentationIndex(index))
            }));
            match served {
                Ok(Ok(payload)) => out.extend(stored(descriptor, &payload)),
                Ok(Err(error)) => {
                    tracing::debug!(%error, index, "a representation could not be served");
                }
                Err(payload) => {
                    // A payload whose destructor panics must not unwind from here.
                    std::mem::forget(payload);
                    crate::owner::panic_boundary::contain_owner_callback(|| {
                        tracing::error!("a drag's provider panicked");
                    });
                    return Err(ProviderPanicked);
                }
            }
        }
        Ok(out)
    }

    /// The `DataTransfer` type and string a source stores `payload` of
    /// `descriptor` as at `dragstart`, or `None` when the browser cannot carry
    /// it (bytes that are not UTF-8: `setData` takes strings).
    #[must_use]
    pub(super) fn stored(
        descriptor: &RepresentationDescriptor,
        payload: &TransferPayload,
    ) -> Option<(String, String)> {
        let dom_type = match &descriptor.format {
            TransferFormat::Text => "text/plain".to_owned(),
            TransferFormat::Html => "text/html".to_owned(),
            TransferFormat::UriList => "text/uri-list".to_owned(),
            TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
                mime.as_str().to_owned()
            }
        };
        let text = match payload {
            TransferPayload::Text(text) | TransferPayload::Html(text) => text.clone(),
            TransferPayload::UriList(uris) => uris
                .iter()
                .map(|uri| match uri {
                    TransferUri::Uri(uri) => uri.clone(),
                    TransferUri::Path(path) => format!("file://{}", path.display()),
                })
                .collect::<Vec<_>>()
                .join("\r\n"),
            TransferPayload::Custom { bytes, .. } => std::str::from_utf8(bytes).ok()?.to_owned(),
            TransferPayload::Image(image) => std::str::from_utf8(&image.bytes).ok()?.to_owned(),
        };
        Some((dom_type, text))
    }

    #[cfg(test)]
    mod tests;
}

// The DOM side, in the browser alone.

#[cfg(target_arch = "wasm32")]
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

#[cfg(target_arch = "wasm32")]
use flui_foundation::{DataTransferId, geometry::Point};
#[cfg(target_arch = "wasm32")]
use flui_platform::{
    DragDropEvent, PlatformInput,
    data_transfer::{
        DataTransferOffer, DataTransferSource, DropFeedback, Mime, OfferRecord, OfferTable,
        RepresentationDescriptor, RepresentationIndex, TransferActions, TransferError,
        TransferFormat, TransferLimits, TransferPayload, TransferRequest,
    },
    drag::{DragError, OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource},
};
#[cfg(target_arch = "wasm32")]
use parking_lot::Mutex;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

#[cfg(target_arch = "wasm32")]
use crate::{
    drag::offers::payload_of_bytes,
    owner::handlers::WindowCallbacks,
    owner::panic_boundary::contain_owner_callback,
    owner::web::{CanvasDrag, CanvasListeners, HostDrag},
    platforms::web::events::{answer, pointer_position},
};
#[cfg(target_arch = "wasm32")]
use mapping::{
    DomSource, action_of, allowed_of, drop_effect, effect_allowed_of, offered, payload_of,
    serve_stored,
};

/// The `DataTransfer` type of `Files`, which names the files of a drag, not
/// a string representation.
#[cfg(target_arch = "wasm32")]
const FILES_TYPE: &str = "Files";

/// One announced representation, before and after the drop.
#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
struct Offers {
    table: OfferTable,
    /// The answer each live offer's target last cached.
    feedback: HashMap<DataTransferId, DropFeedback>,
    /// What each live offer holds, by representation index.
    held: HashMap<DataTransferId, Vec<Held>>,
}

/// The platform's HTML drag-and-drop transport (see the module doc).
#[derive(Default)]
#[cfg(target_arch = "wasm32")]
pub struct WebTransfer {
    offers: Mutex<Offers>,
}

#[cfg(target_arch = "wasm32")]
impl std::fmt::Debug for WebTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransfer").finish_non_exhaustive()
    }
}

// SAFETY: the page has one thread (the module refuses `+atomics`), so the JS
// handles a held offer keeps (its files) are never reached from another; the
// trait's bounds are those every backend's transport meets.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for WebTransfer {}
// SAFETY: as for `Send`: there is no second thread to share it with.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for WebTransfer {}

#[cfg(target_arch = "wasm32")]
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

/// What the window asks of the transport: its canvas's drop target and drag
/// source.
#[cfg(target_arch = "wasm32")]
impl HostDrag for WebTransfer {
    fn register(
        self: Arc<Self>,
        canvas: &web_sys::HtmlCanvasElement,
        callbacks: &Arc<WindowCallbacks>,
        registration: &mut dyn CanvasListeners,
    ) -> Result<Box<dyn CanvasDrag>, crate::OpenWindowError> {
        Ok(Box::new(register(canvas, &self, callbacks, registration)?))
    }
}

// HTML drag and drop; `clipboard_offer` stays `None`, since the clipboard
// reads only its own writes.
#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
struct WebDragSource {
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
#[cfg(target_arch = "wasm32")]
unsafe impl Send for WebDragSource {}
// SAFETY: as for `Send`.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for WebDragSource {}

#[cfg(target_arch = "wasm32")]
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
    fn retire(&self) {
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

#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
fn page_draggable(canvas: &web_sys::HtmlCanvasElement) -> bool {
    js_sys::Reflect::get(canvas, &wasm_bindgen::JsValue::from_str("draggable"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// The source and target of one canvas, from registration to retirement.
#[cfg(target_arch = "wasm32")]
struct HtmlCanvasDrag {
    source: Arc<WebDragSource>,
    canvas: web_sys::HtmlCanvasElement,
    was_draggable: bool,
}

#[cfg(target_arch = "wasm32")]
impl CanvasDrag for HtmlCanvasDrag {
    fn source(&self) -> Arc<dyn PlatformDragSource> {
        Arc::clone(&self.source) as Arc<dyn PlatformDragSource>
    }

    fn arm(&self) {
        self.canvas.set_draggable(true);
    }

    fn retire(&self) {
        self.source.retire();
        self.canvas.set_draggable(self.was_draggable);
    }
}

/// The offer hovering a canvas and the actions its drag permits.
#[cfg(target_arch = "wasm32")]
type Hover = Rc<Cell<Option<(DataTransferId, TransferActions)>>>;

#[cfg(target_arch = "wasm32")]
fn transfer_of(event: &web_sys::Event) -> Option<web_sys::DataTransfer> {
    event
        .dyn_ref::<web_sys::DragEvent>()
        .and_then(web_sys::DragEvent::data_transfer)
}

#[cfg(target_arch = "wasm32")]
fn drag_position(event: &web_sys::Event) -> Point {
    pointer_position(event.unchecked_ref())
}

/// What a hovering drag offers.
#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
fn register(
    canvas: &web_sys::HtmlCanvasElement,
    transfer: &Arc<WebTransfer>,
    callbacks: &Arc<WindowCallbacks>,
    registration: &mut dyn CanvasListeners,
) -> Result<HtmlCanvasDrag, crate::OpenWindowError> {
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
    Ok(HtmlCanvasDrag {
        source,
        canvas: canvas.clone(),
        was_draggable: page_draggable(canvas),
    })
}
