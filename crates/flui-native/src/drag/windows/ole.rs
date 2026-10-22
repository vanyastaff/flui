//! OLE drag and drop, as source and as target (ADR-0038, ADR-0211).
//!
//! [`OleTransfer`] is the platform's one [`DataTransferSource`]. Every window
//! registers a [`WindowDropTarget`] with
//! `RegisterDragDrop`; a drag over it, from this process or another, mints an
//! offer in the transfer's table, keeps the drag's `IDataObject` for lazy
//! fetches, and answers the OS's synchronous hover queries from the feedback
//! the target last cached. The data object is released when the target
//! concludes the drop or the drag leaves.
//!
//! A window's [`Win32DragSource`] starts an OS session from the runtime's
//! request. The host calls it at its own outermost quiescent point, which is
//! still inside the window callback that decided the drag; a modal loop run
//! there would hold that window's callbacks for its whole length. So the
//! start only records the request and posts a message to the owner window:
//! `DoDragDrop` runs from that message, which the owner hands to the
//! transport's own hook on its window (`OwnerMessages`), on a fresh turn of
//! the message loop with no window callback in progress, and everything the
//! loop delivers reaches its window as a fresh callback. While it runs, the
//! main loop that waits for wake deadlines and dispatches paints is suspended
//! under OLE's, which dispatches neither while the pointer rests; a pacing
//! thread ([`ModalPacer`]) has the owner window deliver due deadlines and
//! paint every invalidated window instead, so every runtime on the thread
//! keeps animating and a second window repaints.
//!
//! Reentrancy: user code runs inside `DoDragDrop` (every window callback, and
//! the session's provider when a target fetches), so no `RefCell` borrow or
//! lock is held across a COM call or a dispatch; every value that leaves a
//! borrow (a released data object, a request's hooks) is dropped after it.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`); every `unsafe`
// block carries its own `// SAFETY:` comment.
#![expect(unsafe_code)]

mod com;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferError, TransferLimits, TransferRequest,
};
use flui_platform::drag::{DragError, OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource};
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{DRAGDROP_S_DROP, GlobalFree, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM},
    System::{
        Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL},
        DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::{
            CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_NONE, DoDragDrop, IDropTarget,
            RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
        },
    },
    UI::{
        Input::KeyboardAndMouse::GetKeyState,
        WindowsAndMessaging::{
            MSG, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW,
            QS_SENDMESSAGE, SMTO_NORMAL, SendMessageTimeoutW,
        },
    },
};
use windows_core::{ComObject, HSTRING};

use super::ole_model::{
    NativeFormat, actions_of, decode, effect_bits, offer_representations, os_loop_buttons,
};
use crate::owner::modal_pace::MODAL_FRAME_PACE_MS;
use crate::owner::panic_boundary::{contain_owner_callback, owner_callback_panicked};
use crate::owner::slot::OwnerSlot;
use crate::owner::windows::HostDrag;
use crate::owner::windows::control::{OwnerMessages, OwnerPost, OwnerWindow};
use crate::platforms::windows::platform::WindowRef;
use flui_platform::pointer::PointerKind;

pub(crate) use com::WindowDropTarget;

/// The registered name of the `text/html` clipboard format (CF_HTML).
const HTML_FORMAT_NAME: &str = "HTML Format";

/// Posted to the owner window: run the session [`OleTransfer::start`] recorded.
const START_DRAG: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 21;
/// Sent to the owner window while `DoDragDrop` runs: deliver the frames that
/// are due; answered with the milliseconds until the next request.
const MODAL_FRAME: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 22;

/// The offers minted for drags over this platform's windows.
#[derive(Default)]
struct Offers {
    table: OfferTable,
    /// The answer each live offer's target last cached.
    feedback: HashMap<DataTransferId, DropFeedback>,
}

/// One live offer's OS side: the drag's data object and the clipboard format
/// behind each representation.
struct OpenOffer {
    object: IDataObject,
    formats: Vec<NativeFormat>,
    representations: Arc<[RepresentationDescriptor]>,
}

/// A session [`OleTransfer::start`] accepted, and the mouse buttons (`MK_*`)
/// held as it was accepted, whose release drops it.
struct PendingStart {
    request: OsDragRequest,
    began: u32,
}

/// What only the owner thread touches: COM objects and the session's hooks.
#[derive(Default)]
struct OwnerDrag {
    open: RefCell<HashMap<DataTransferId, OpenOffer>>,
    /// A session [`OleTransfer::start`] accepted whose `DoDragDrop` has not run.
    pending: RefCell<Option<PendingStart>>,
    /// Whether `DoDragDrop` is running.
    running: Cell<bool>,
}

impl OwnerDrag {
    /// Tell the accepted session whose `DoDragDrop` has not run that it was
    /// cancelled: the owner window is going, so its start message never
    /// runs.
    fn cancel_pending(&self) {
        let pending = self.pending.borrow_mut().take();
        if let Some(PendingStart { request, .. }) = pending {
            cancel_unstarted(request);
        }
    }
}

impl Drop for OwnerDrag {
    fn drop(&mut self) {
        // The owner window cancels its session as it is destroyed; this is
        // the session's end when that window never was.
        self.cancel_pending();
    }
}

/// End a session that never started: it is told `Cancelled`, then its
/// provider goes. Both are user code: during an unwind neither runs, and
/// after `on_end` panics the provider is retained (ADR-0127 §2).
fn cancel_unstarted(request: OsDragRequest) {
    if std::thread::panicking() {
        std::mem::forget(request);
        return;
    }
    let OsDragRequest {
        provider, on_end, ..
    } = request;
    if owner_callback_panicked(|| on_end(OsDragEnd::Cancelled)) {
        std::mem::forget(provider);
    } else {
        contain_owner_callback(|| drop(provider));
    }
}

/// The platform's OLE drag-and-drop transport (see the module doc).
pub struct OleTransfer {
    offers: Mutex<Offers>,
    owner: OwnerSlot<Rc<OwnerDrag>>,
    /// Where a session's start is posted: the owner window, once it exists.
    owner_window: OnceLock<OwnerPost>,
}

impl std::fmt::Debug for OleTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OleTransfer").finish_non_exhaustive()
    }
}

impl OleTransfer {
    /// The transport of a platform built on the calling thread.
    pub(crate) fn new() -> Arc<Self> {
        let owner = OwnerSlot::new();
        if owner.replace(Rc::new(OwnerDrag::default())).is_err() {
            tracing::error!("drag-and-drop owner state refused; drags are unavailable");
        }
        Arc::new(Self {
            offers: Mutex::new(Offers::default()),
            owner,
            owner_window: OnceLock::new(),
        })
    }

    /// The owner-thread state; `None` off the owner thread.
    fn owner(&self) -> Option<Rc<OwnerDrag>> {
        let owner = self.owner.take()?;
        let shared = Rc::clone(&owner);
        let _ = self.owner.replace(owner);
        Some(shared)
    }

    /// The owner window is being destroyed: the session it was to start is
    /// cancelled now, on the owner thread, rather than whenever the last
    /// holder of this transport lets it go.
    fn cancel_pending(&self) {
        if let Some(owner) = self.owner() {
            owner.cancel_pending();
        }
    }

    /// Whether `DoDragDrop` is running on this thread.
    fn is_running(&self) -> bool {
        self.owner().is_some_and(|owner| owner.running.get())
    }

    /// Mint an offer for a drag entering a window, keeping its data object.
    fn open_offer(
        &self,
        object: IDataObject,
        offered: Vec<(NativeFormat, RepresentationDescriptor)>,
    ) -> Option<DataTransferOffer> {
        let owner = self.owner()?;
        let (formats, representations): (Vec<_>, Vec<_>) = offered.into_iter().unzip();
        let representations: Arc<[RepresentationDescriptor]> = Arc::from(representations);
        let id = {
            let mut offers = self.offers.lock();
            let id = offers
                .table
                .mint(OfferRecord::new(Arc::clone(&representations)));
            offers.feedback.insert(id, DropFeedback::default());
            id
        };
        let replaced = owner.open.borrow_mut().insert(
            id,
            OpenOffer {
                object,
                formats,
                representations: Arc::clone(&representations),
            },
        );
        drop(replaced);
        Some(DataTransferOffer::new(id, representations))
    }

    /// The answer the target of `id` last cached.
    fn feedback(&self, id: DataTransferId) -> DropFeedback {
        self.offers
            .lock()
            .feedback
            .get(&id)
            .copied()
            .unwrap_or_default()
    }

    /// Retire `id` and release its data object.
    fn close_offer(&self, id: DataTransferId) {
        {
            let mut offers = self.offers.lock();
            let _ = offers.table.retire(id);
            offers.feedback.remove(&id);
        }
        let released = self
            .owner()
            .and_then(|owner| owner.open.borrow_mut().remove(&id));
        // Releasing the last reference to a data object of this process runs
        // its provider's destructor: no borrow is held here.
        drop(released);
    }

    /// Accept a session of a `kind` contact and post its start to the owner
    /// window.
    fn start(&self, request: OsDragRequest, kind: Option<PointerKind>) -> Result<(), DragError> {
        let owner = self.owner().ok_or(DragError::Unavailable)?;
        if owner.running.get() || owner.pending.borrow().is_some() {
            // One OS session at a time, as the OS runs one modal loop.
            return Err(DragError::Unavailable);
        }
        // OLE's loop follows the mouse alone: a drag no mouse button holds
        // stays in its window rather than starting a session that cancels at
        // its first question (ADR-0211 §11).
        let began = os_loop_buttons(kind, mouse_buttons_held()).ok_or(DragError::Unavailable)?;
        *owner.pending.borrow_mut() = Some(PendingStart { request, began });
        if self.post_start() {
            Ok(())
        } else {
            let refused = owner.pending.borrow_mut().take();
            drop(refused);
            Err(DragError::Unavailable)
        }
    }

    fn post_start(&self) -> bool {
        self.owner_window
            .get()
            .is_some_and(|owner| owner.post(START_DRAG))
    }

    /// Run the accepted session's `DoDragDrop`, from the owner window's
    /// [`START_DRAG`] message, and tell the session how it ended.
    fn run_pending(&self, owner_window: HWND) {
        let Some(owner) = self.owner() else {
            return;
        };
        if owner.running.get() {
            return;
        }
        let Some(PendingStart { request, began }) = owner.pending.borrow_mut().take() else {
            return;
        };
        // The buttons held at the start: a release since then drops the
        // session where the pointer is, at OLE's first question.
        let source = com::DropSource::new(began);
        Self::run_session(&owner, owner_window, request, source.into());
    }

    /// Run `request`'s modal loop with `drop_source` deciding when it ends.
    fn run_session(
        owner: &OwnerDrag,
        owner_window: HWND,
        request: OsDragRequest,
        drop_source: windows::Win32::System::Ole::IDropSource,
    ) {
        let OsDragRequest {
            representations,
            allowed,
            provider,
            on_end,
            ..
        } = request;
        let data: IDataObject = com::SourceData::new(&representations, provider).into();
        owner.running.set(true);
        let running = RunningSession(owner);
        let pacer = ModalPacer::start(owner_window);
        let mut effect = DROPEFFECT_NONE;
        // SAFETY: both interfaces are live objects of this apartment, held
        // across the call; `effect` is a live local the call writes once.
        // `DoDragDrop` runs this thread's modal loop and returns on its end.
        let result = unsafe {
            DoDragDrop(
                &data,
                &drop_source,
                DROPEFFECT(effect_bits(allowed)),
                &raw mut effect,
            )
        };
        // Not running first, so a frame request the pacer is sending as it
        // stops is answered at once.
        drop(running);
        drop(pacer);
        let action = actions_of(effect.0) & allowed;
        let end = if result == DRAGDROP_S_DROP && !action.is_empty() {
            OsDragEnd::Dropped(action)
        } else {
            if result.is_err() {
                tracing::warn!(error = %windows_core::Error::from(result), "DoDragDrop failed");
            }
            OsDragEnd::Cancelled
        };
        // A target in this process may still hold the data object: the
        // provider goes with its last reference.
        drop(drop_source);
        drop(data);
        contain_owner_callback(|| on_end(end));
    }
}

/// Clears the running flag however the session ends.
struct RunningSession<'a>(&'a OwnerDrag);

impl Drop for RunningSession<'_> {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}

/// Asks the owner window for frames while `DoDragDrop` runs.
///
/// The OLE loop waits only for input and for messages sent from other
/// threads: while the pointer rests it dispatches no posted message, timer or
/// paint, so neither a timer nor a posted wake on this thread would ever be
/// seen. A pacing thread sends [`MODAL_FRAME`] instead, which the loop's wait
/// does dispatch; the owner window delivers what is due and answers with how
/// many milliseconds to wait before the next one.
struct ModalPacer {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ModalPacer {
    fn start(owner_window: HWND) -> Option<Self> {
        let address = owner_window.0 as isize;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let spawned = std::thread::Builder::new()
            .name("flui-drag-frames".into())
            .spawn(move || {
                let mut wait = MODAL_FRAME_PACE_MS;
                loop {
                    std::thread::park_timeout(Duration::from_millis(u64::from(wait)));
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let mut answer = 0_usize;
                    // SAFETY: a message with no pointer payload, sent to the
                    // owner window, which outlives the session this thread
                    // paces; `answer` is a live local the call writes. The
                    // timeout bounds the wait if the owner thread is busy.
                    let sent = unsafe {
                        SendMessageTimeoutW(
                            HWND(address as *mut _),
                            MODAL_FRAME,
                            WPARAM(0),
                            LPARAM(0),
                            SMTO_NORMAL,
                            100,
                            Some(&raw mut answer),
                        )
                    };
                    wait = u32::try_from(answer)
                        .ok()
                        .filter(|millis| sent.0 != 0 && (1..=MODAL_FRAME_PACE_MS).contains(millis))
                        .unwrap_or(MODAL_FRAME_PACE_MS);
                }
            });
        match spawned {
            Ok(thread) => Some(Self {
                stop,
                thread: Some(thread),
            }),
            Err(error) => {
                tracing::warn!(%error, "no frames while the drag rests: the pacing thread did not start");
                None
            }
        }
    }
}

impl Drop for ModalPacer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(thread) = self.thread.take() else {
            return;
        };
        thread.thread().unpark();
        // A frame request already being sent waits for this thread: keep
        // answering sent messages until the pacer has gone.
        while !thread.is_finished() {
            let mut msg = MSG::default();
            // SAFETY: `PM_QS_SENDMESSAGE` with `PM_NOREMOVE` only dispatches
            // messages other threads sent this one; `msg` is a live local.
            // The wait takes no handles and lasts at most a millisecond.
            unsafe {
                let _ = PeekMessageW(&raw mut msg, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
                MsgWaitForMultipleObjects(None, false, 1, QS_SENDMESSAGE);
            }
        }
        let _ = thread.join();
    }
}

/// Drag and drop's part of the owner window: a session runs from its
/// [`START_DRAG`], its modal loop's frames come from its [`MODAL_FRAME`], and a
/// session that has not started is cancelled as it is destroyed.
impl OwnerMessages for OleTransfer {
    fn attach(&self, owner: OwnerPost) {
        let _ = self.owner_window.set(owner);
    }

    fn message(&self, owner: &OwnerWindow, message: u32) -> Option<LRESULT> {
        match message {
            START_DRAG => {
                self.run_pending(owner.hwnd());
                Some(LRESULT(0))
            }
            MODAL_FRAME => {
                if !self.is_running() {
                    return Some(LRESULT(0));
                }
                Some(LRESULT(
                    owner.deliver_modal_frames().map_or(0, |wait| wait as isize),
                ))
            }
            _ => None,
        }
    }

    fn destroyed(&self) {
        self.cancel_pending();
    }
}

/// What a window asks of the transport: its drag source, and its drop target.
impl HostDrag for OleTransfer {
    fn drag_source(self: Arc<Self>, window: WindowRef) -> Arc<dyn PlatformDragSource> {
        Win32DragSource::new(self, window)
    }

    fn register_target(self: Arc<Self>, window: WindowRef) {
        register_target(window, self);
    }
}

impl DataTransferSource for OleTransfer {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        // The data object lives on the owner thread.
        let Some(owner) = self.owner() else {
            return TransferRequest::ready(Err(TransferError::SourceGone));
        };
        let found = {
            let open = owner.open.borrow();
            open.get(&id).map(|offer| {
                let index = usize::from(representation.0);
                (
                    offer.object.clone(),
                    offer.formats.get(index).cloned(),
                    offer.representations.get(index).cloned(),
                )
            })
        };
        let Some((object, native, descriptor)) = found else {
            return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
        };
        let (Some(native), Some(descriptor)) = (native, descriptor) else {
            return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                id,
                index: representation,
            }));
        };
        if let Some(declared) = descriptor.declared_len
            && declared > limits.max_bytes
        {
            return TransferRequest::ready(Err(TransferError::TooLarge {
                actual: declared,
                limit: limits.max_bytes,
            }));
        }
        // No borrow is held: a data object of this process serves the fetch
        // from its provider, which is user code.
        let result = fetch(&object, &native, limits.max_bytes)
            .and_then(|bytes| decode(&native, &descriptor.format, &bytes))
            .and_then(|payload| {
                let actual = payload.byte_len();
                if actual > limits.max_bytes {
                    Err(TransferError::TooLarge {
                        actual,
                        limit: limits.max_bytes,
                    })
                } else {
                    Ok(payload)
                }
            });
        drop(object);
        TransferRequest::ready(result)
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

/// The drag source a Win32 window offers: the platform's one transport.
struct Win32DragSource {
    transfer: Arc<OleTransfer>,
    /// The window, reached only on its owner thread and only while its
    /// handle still names it.
    window: WindowRef,
}

impl Win32DragSource {
    fn new(transfer: Arc<OleTransfer>, window: WindowRef) -> Arc<Self> {
        Arc::new(Self { transfer, window })
    }

    /// The kind of the contact whose press began `pointer`'s drag, as the
    /// window's pointer registry read it.
    fn contact_kind(&self, pointer: Option<flui_platform::PointerId>) -> Option<PointerKind> {
        let pointer = pointer?;
        self.window
            .with_context("drag source", |context| {
                let registry = context.pointer_registry.try_borrow().ok()?;
                registry.origin(pointer).map(|origin| origin.kind)
            })
            .flatten()
    }
}

impl PlatformDragSource for Win32DragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::shared::input_availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let kind = self.contact_kind(request.pointer);
        self.transfer.start(request, kind)
    }
}

/// The hook on FLUI's window class that registers each window it creates as
/// an OLE drop target, from then on taking drags from this process or another
/// (ADR-0211).
pub(crate) struct DropTargetRegistration;

impl crate::owner::windows::WindowMessages for DropTargetRegistration {
    fn created(&self, window: &crate::owner::windows::CreatedWindow) {
        let this = WindowRef::new(window.hwnd, window.identity);
        if let Some(transfer) =
            this.with_context("drop target", |context| Arc::clone(&context.drag))
        {
            transfer.register_target(this);
        }
    }
}

/// Register the drop target of `window`, created on this thread, and keep it
/// in the window's context. A window whose registration fails takes no drags.
fn register_target(window: WindowRef, transfer: Arc<OleTransfer>) {
    let hwnd = window.hwnd();
    let target = ComObject::new(WindowDropTarget::new(window, transfer));
    let interface: IDropTarget = target.to_interface();
    // SAFETY: `hwnd` is the live window this thread just created, in the
    // OLE apartment the platform entered on it; OLE keeps its own reference
    // to `interface` until `RevokeDragDrop`.
    if let Err(error) = unsafe { RegisterDragDrop(hwnd, &interface) } {
        tracing::warn!(%error, "the window takes no drags: RegisterDragDrop failed");
        return;
    }
    let kept = window.with_context("drop target", |context| {
        context.drop_target.borrow_mut().replace(target)
    });
    drop(kept);
}

/// Revoke the window's drop target as it is destroyed: a drag over it is
/// left first, while its callbacks still exist.
pub(crate) fn revoke_target(hwnd: HWND, slot: &RefCell<Option<ComObject<WindowDropTarget>>>) {
    let target = slot.borrow_mut().take();
    let Some(target) = target else {
        return;
    };
    // SAFETY: `hwnd` is the live window being destroyed, on its thread; the
    // registration being revoked is the one `register_target` made.
    if let Err(error) = unsafe { RevokeDragDrop(hwnd) } {
        tracing::debug!(%error, "RevokeDragDrop failed");
    }
    target.leave();
}

/// The `MK_*` bits of the mouse buttons held now, in this thread's input
/// state: the buttons whose release drops the session about to start.
fn mouse_buttons_held() -> u32 {
    crate::platforms::windows::events::MOUSE_BUTTONS
        .iter()
        .filter(|(key, _, _)| {
            // SAFETY: a query of this thread's key state by a constant code.
            let state = unsafe { GetKeyState(i32::from(key.0)) };
            state < 0
        })
        .fold(0, |held, (_, bit, _)| held | bit)
}

/// The clipboard format `native` travels as; `None` when registration failed.
fn clipboard_format(native: &NativeFormat) -> Option<u16> {
    let registered = |name: &str| {
        let name = HSTRING::from(name);
        // SAFETY: `name` is a live NUL-terminated string for the call.
        u16::try_from(unsafe { RegisterClipboardFormatW(&name) })
            .ok()
            .filter(|format| *format != 0)
    };
    match native {
        NativeFormat::UnicodeText => Some(CF_UNICODETEXT.0),
        NativeFormat::FileDrop => Some(CF_HDROP.0),
        NativeFormat::Html => registered(HTML_FORMAT_NAME),
        NativeFormat::Named(name) => registered(name),
    }
}

/// The FLUI format a clipboard format of a foreign data object is, if any.
fn native_of(format: u16) -> Option<NativeFormat> {
    match format {
        format if format == CF_UNICODETEXT.0 => Some(NativeFormat::UnicodeText),
        format if format == CF_HDROP.0 => Some(NativeFormat::FileDrop),
        // Registered formats live in 0xC000..=0xFFFF; predefined ones FLUI
        // does not announce.
        format if format >= 0xC000 => {
            let mut name = [0_u16; 256];
            // SAFETY: the call writes at most `name.len()` units into the
            // live buffer and returns how many.
            let written = unsafe { GetClipboardFormatNameW(u32::from(format), &mut name) };
            let written = usize::try_from(written).ok().filter(|count| *count > 0)?;
            let name = String::from_utf16(&name[..written]).ok()?;
            Some(if name == HTML_FORMAT_NAME {
                NativeFormat::Html
            } else {
                NativeFormat::Named(name)
            })
        }
        _ => None,
    }
}

/// The `FORMATETC` asking for `format` in global memory.
const fn global_format(format: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// The most a session identity's media type may take: far more than one
/// FLUI writes, and a bound on what a foreign object can make this read.
const IDENTITY_MAX_BYTES: u64 = 4096;

/// The most entries read from a foreign data object's format enumerator:
/// far more than any source offers.
const MAX_ENUMERATED_FORMATS: usize = 256;

/// What a foreign data object offers: its formats in its order, and the
/// session identity when it carries FLUI's.
fn describe(object: &IDataObject) -> Vec<(NativeFormat, RepresentationDescriptor)> {
    let mut formats = Vec::new();
    // SAFETY: `object` is a live data object; the enumerator it returns is
    // owned here.
    if let Ok(list) =
        unsafe { object.EnumFormatEtc(windows::Win32::System::Com::DATADIR_GET.0 as u32) }
    {
        // The other process's enumerator decides when it ends; a bound keeps
        // one that never does from holding `DragEnter`, and its source, here.
        for _ in 0..MAX_ENUMERATED_FORMATS {
            let mut entry = [FORMATETC::default()];
            let mut fetched = 0;
            // SAFETY: `entry` has room for the one entry asked for and
            // `fetched` is a live local; both outlive the call.
            let next = unsafe { list.Next(&mut entry, Some(&raw mut fetched)) };
            if next.is_err() || fetched == 0 {
                break;
            }
            let [entry] = entry;
            if !entry.ptd.is_null() {
                // SAFETY: an enumerated target device is the caller's to free.
                unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(entry.ptd.cast())) };
            }
            if entry.tymed & (TYMED_HGLOBAL.0 as u32) == 0 {
                continue;
            }
            if let Some(native) = native_of(entry.cfFormat)
                && !formats.contains(&native)
            {
                formats.push(native);
            }
        }
    }
    let identity_format = NativeFormat::Named(flui_platform::drag::IDENTITY_ESSENCE.to_owned());
    let identity = formats
        .contains(&identity_format)
        .then(|| fetch(object, &identity_format, IDENTITY_MAX_BYTES).ok())
        .flatten()
        .and_then(|bytes| crate::drag::windows::ole_model::identity_descriptor(&bytes));
    offer_representations(formats, identity)
}

/// Fetch `native` from `object` as bytes, at most `max_bytes` of them.
fn fetch(
    object: &IDataObject,
    native: &NativeFormat,
    max_bytes: u64,
) -> Result<Vec<u8>, TransferError> {
    let format = clipboard_format(native).ok_or(TransferError::SourceGone)?;
    let request = global_format(format);
    // SAFETY: `request` is a live, fully initialized `FORMATETC`; the medium
    // returned is released exactly once below, after its memory is copied.
    let mut medium: STGMEDIUM =
        unsafe { object.GetData(&raw const request) }.map_err(|_| TransferError::SourceGone)?;
    let bytes = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
        // SAFETY: a `TYMED_HGLOBAL` medium's union holds its global handle.
        read_global(unsafe { medium.u.hGlobal }, max_bytes)
    } else {
        Err(TransferError::SourceGone)
    };
    // SAFETY: `medium` is the one `GetData` filled; released once.
    unsafe { ReleaseStgMedium(&raw mut medium) };
    bytes
}

/// Copy a global allocation's bytes, refusing one larger than `max_bytes`.
fn read_global(global: HGLOBAL, max_bytes: u64) -> Result<Vec<u8>, TransferError> {
    // SAFETY: `global` is the live handle of a medium the caller holds;
    // `GlobalSize` bounds the copy, which reads only while the lock holds.
    unsafe {
        let size = GlobalSize(global);
        if size as u64 > max_bytes {
            return Err(TransferError::TooLarge {
                actual: size as u64,
                limit: max_bytes,
            });
        }
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            return Err(TransferError::SourceGone);
        }
        let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(global);
        Ok(bytes)
    }
}

/// A movable global allocation holding `bytes`, owned by the caller.
fn write_global(bytes: &[u8]) -> windows_core::Result<HGLOBAL> {
    // SAFETY: the allocation is at least `bytes.len()` long, written only
    // while locked, and freed here if it cannot be locked.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
        let pointer = GlobalLock(global);
        if pointer.is_null() {
            let error = windows_core::Error::from_thread();
            let _ = GlobalFree(Some(global));
            return Err(error);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(global);
        Ok(global)
    }
}
