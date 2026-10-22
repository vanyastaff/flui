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
//!
//! What crosses OLE is decided in [`crossing`] with no OLE call, so every host
//! compiles and checks it; the COM side around it is Windows' alone.

// This file calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`); every `unsafe`
// block carries its own `// SAFETY:` comment. Its pure part forbids it.
#![cfg_attr(windows, expect(unsafe_code))]

#[cfg(windows)]
mod com;
#[cfg(all(windows, test))]
mod tests;

/// The decidable half of the backend (ADR-0038, ADR-0211): which clipboard
/// format carries each transfer format, how a payload is laid out in its
/// `HGLOBAL`, how `DROPEFFECT` bits map to [`TransferActions`], and when the
/// drop source ends the modal loop. Plain functions over bytes and integers,
/// so every host checks them; the COM objects around them only move bytes in
/// and out of global memory. How often that loop asks for frames is the
/// owner's (`owner::modal_pace`).
///
/// [`TransferActions`]: flui_platform::data_transfer::TransferActions
#[forbid(unsafe_code)]
mod crossing {
    use std::path::PathBuf;
    use std::sync::Arc;

    use flui_platform::data_transfer::{
        Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat,
        TransferImage, TransferPayload, TransferUri,
    };
    use flui_platform::drag::IDENTITY_ESSENCE;
    use flui_platform::pointer::PointerKind;

    /// `DROPEFFECT_COPY`.
    pub(super) const EFFECT_COPY: u32 = 1;
    /// `DROPEFFECT_MOVE`.
    pub(super) const EFFECT_MOVE: u32 = 2;
    /// `DROPEFFECT_LINK`.
    pub(super) const EFFECT_LINK: u32 = 4;

    /// The `DROPEFFECT` bits that permit `actions`.
    #[must_use]
    pub(super) fn effect_bits(actions: TransferActions) -> u32 {
        let mut bits = 0;
        if actions.contains(TransferActions::COPY) {
            bits |= EFFECT_COPY;
        }
        if actions.contains(TransferActions::MOVE) {
            bits |= EFFECT_MOVE;
        }
        if actions.contains(TransferActions::LINK) {
            bits |= EFFECT_LINK;
        }
        bits
    }

    /// The actions `DROPEFFECT` bits permit; `DROPEFFECT_SCROLL` and unknown
    /// bits are ignored.
    #[must_use]
    pub(super) fn actions_of(effect: u32) -> TransferActions {
        let mut actions = TransferActions::empty();
        if effect & EFFECT_COPY != 0 {
            actions |= TransferActions::COPY;
        }
        if effect & EFFECT_MOVE != 0 {
            actions |= TransferActions::MOVE;
        }
        if effect & EFFECT_LINK != 0 {
            actions |= TransferActions::LINK;
        }
        actions
    }

    /// The registered name browsers and Office use for PNG images.
    pub(super) const PNG_FORMAT_NAME: &str = "PNG";

    /// The clipboard format one representation travels as.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub(super) enum NativeFormat {
        /// `CF_UNICODETEXT`: UTF-16 with a terminating NUL.
        UnicodeText,
        /// CF_HTML: UTF-8 markup behind its offset header.
        Html,
        /// `CF_HDROP`: a `DROPFILES` header and wide paths.
        FileDrop,
        /// A format registered by name: a MIME type, [`PNG_FORMAT_NAME`] or
        /// FLUI's identity format, whose bytes are the payload itself.
        Named(String),
    }

    /// The clipboard format `format` travels as. The session identity travels
    /// under its bare media type, [`IDENTITY_ESSENCE`]: Win32 cannot register a
    /// format per session, so the identity's parameters ride in the data.
    #[must_use]
    pub(super) fn native_format(format: &TransferFormat) -> NativeFormat {
        match format {
            TransferFormat::Text => NativeFormat::UnicodeText,
            TransferFormat::Html => NativeFormat::Html,
            TransferFormat::UriList => NativeFormat::FileDrop,
            TransferFormat::Image { mime } if mime.as_str() == "image/png" => {
                NativeFormat::Named(PNG_FORMAT_NAME.to_owned())
            }
            TransferFormat::Custom { mime } if is_identity(mime.as_str()) => {
                NativeFormat::Named(IDENTITY_ESSENCE.to_owned())
            }
            TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
                NativeFormat::Named(mime.as_str().to_owned())
            }
        }
    }

    /// Whether `mime` names FLUI's private session identity.
    fn is_identity(mime: &str) -> bool {
        mime.split(';').next() == Some(IDENTITY_ESSENCE)
    }

    /// The transfer format a foreign clipboard format reads as, or `None` for a
    /// format FLUI does not announce (`CF_TEXT` beside `CF_UNICODETEXT`, shell
    /// formats, names that are not media types) or for the identity format,
    /// which [`identity_descriptor`] reads.
    #[must_use]
    pub(super) fn transfer_format(native: &NativeFormat) -> Option<TransferFormat> {
        match native {
            NativeFormat::UnicodeText => Some(TransferFormat::Text),
            NativeFormat::Html => Some(TransferFormat::Html),
            NativeFormat::FileDrop => Some(TransferFormat::UriList),
            NativeFormat::Named(name) if name == PNG_FORMAT_NAME => Some(TransferFormat::Image {
                mime: Mime::new("image/png"),
            }),
            NativeFormat::Named(name) if is_identity(name) => None,
            NativeFormat::Named(name) => {
                let essence = name.split(';').next().unwrap_or_default();
                let (kind, subtype) = essence.split_once('/')?;
                let token = |part: &str| {
                    !part.is_empty()
                        && part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&byte)
                        })
                };
                if !token(kind) || !token(subtype) {
                    return None;
                }
                let mime = Mime::new(name);
                Some(if kind.eq_ignore_ascii_case("image") {
                    TransferFormat::Image { mime }
                } else {
                    TransferFormat::Custom { mime }
                })
            }
        }
    }

    /// The identity descriptor the identity format's bytes carry, when they are
    /// the full media type of a FLUI session identity; anything else reads as a
    /// foreign drag.
    #[must_use]
    pub(super) fn identity_descriptor(bytes: &[u8]) -> Option<RepresentationDescriptor> {
        let text = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
        is_identity(text).then(|| RepresentationDescriptor {
            format: TransferFormat::Custom {
                mime: Mime::new(text),
            },
            declared_len: Some(0),
        })
    }

    /// What a foreign data object offers, in its own order: one descriptor per
    /// transfer format (the first clipboard format reading as it wins), with the
    /// clipboard format to fetch it by, then the identity when it carries one.
    #[must_use]
    pub(super) fn offer_representations(
        formats: impl IntoIterator<Item = NativeFormat>,
        identity: Option<RepresentationDescriptor>,
    ) -> Vec<(NativeFormat, RepresentationDescriptor)> {
        let mut offered: Vec<(NativeFormat, RepresentationDescriptor)> = Vec::new();
        for native in formats {
            let Some(format) = transfer_format(&native) else {
                continue;
            };
            if offered
                .iter()
                .any(|(_, descriptor)| descriptor.format == format)
            {
                continue;
            }
            offered.push((
                native,
                RepresentationDescriptor {
                    format,
                    declared_len: None,
                },
            ));
        }
        if let Some(identity) = identity {
            offered.push((NativeFormat::Named(IDENTITY_ESSENCE.to_owned()), identity));
        }
        offered
    }

    /// The bytes `payload` is served as in `native`, or `None` when the payload
    /// does not fit the format (a URI list with no local path in `CF_HDROP`).
    #[must_use]
    pub(super) fn encode(native: &NativeFormat, payload: &TransferPayload) -> Option<Vec<u8>> {
        match (native, payload) {
            (NativeFormat::UnicodeText, TransferPayload::Text(text)) => Some(encode_text(text)),
            (NativeFormat::Html, TransferPayload::Html(markup)) => Some(encode_html(markup)),
            (NativeFormat::FileDrop, TransferPayload::UriList(uris)) => {
                let paths: Vec<&std::path::Path> = uris
                    .iter()
                    .filter_map(|uri| match uri {
                        TransferUri::Path(path) => Some(path.as_path()),
                        TransferUri::Uri(_) => None,
                    })
                    .collect();
                (!paths.is_empty()).then(|| encode_file_drop(&paths))
            }
            (NativeFormat::Named(_), TransferPayload::Custom { bytes, .. }) => Some(bytes.to_vec()),
            (NativeFormat::Named(_), TransferPayload::Image(image)) => Some(image.bytes.to_vec()),
            _ => None,
        }
    }

    /// The payload `bytes`, fetched in `native`, carry for `format`.
    ///
    /// # Errors
    ///
    /// [`TransferError::Decode`] when the bytes are not well-formed for the
    /// clipboard format.
    pub(super) fn decode(
        native: &NativeFormat,
        format: &TransferFormat,
        bytes: &[u8],
    ) -> Result<TransferPayload, TransferError> {
        let failed = || TransferError::Decode {
            format: format.clone(),
        };
        match (native, format) {
            (NativeFormat::UnicodeText, _) => decode_text(bytes).map(TransferPayload::Text),
            (NativeFormat::Html, _) => decode_html(bytes).map(TransferPayload::Html),
            (NativeFormat::FileDrop, _) => decode_file_drop(bytes).map(|paths| {
                TransferPayload::UriList(paths.into_iter().map(TransferUri::Path).collect())
            }),
            // The identity is read from the descriptor; its payload is empty, as
            // the source runtime serves it.
            (NativeFormat::Named(name), _) if is_identity(name) => Ok(TransferPayload::Custom {
                mime: Mime::new(IDENTITY_ESSENCE),
                bytes: Arc::from(Vec::new()),
            }),
            (NativeFormat::Named(_), TransferFormat::Image { mime }) => {
                Ok(TransferPayload::Image(TransferImage {
                    mime: mime.clone(),
                    bytes: Arc::from(bytes),
                }))
            }
            (NativeFormat::Named(_), TransferFormat::Custom { mime }) => {
                Ok(TransferPayload::Custom {
                    mime: Mime::new(mime.as_str().split(';').next().unwrap_or_default()),
                    bytes: Arc::from(bytes),
                })
            }
            (NativeFormat::Named(_), _) => Err(()),
        }
        .map_err(|()| failed())
    }

    /// `CF_UNICODETEXT`: UTF-16LE with a terminating NUL.
    #[must_use]
    pub(super) fn encode_text(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn utf16_units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
    }

    /// `CF_UNICODETEXT` up to its first NUL, or the whole allocation without
    /// one (another process may omit it; the allocation bounds the read).
    fn decode_text(bytes: &[u8]) -> Result<String, ()> {
        let units: Vec<u16> = utf16_units(bytes).take_while(|unit| *unit != 0).collect();
        String::from_utf16(&units).map_err(|_| ())
    }

    const HTML_PREFIX: &str = "<html><body>\r\n<!--StartFragment-->";
    const HTML_SUFFIX: &str = "<!--EndFragment-->\r\n</body></html>";

    /// CF_HTML: the offset header, then the markup as the fragment of a
    /// document.
    #[must_use]
    pub(super) fn encode_html(markup: &str) -> Vec<u8> {
        let header_len = header(0, 0, 0, 0).len();
        let start_html = header_len;
        let start_fragment = start_html + HTML_PREFIX.len();
        let end_fragment = start_fragment + markup.len();
        let end_html = end_fragment + HTML_SUFFIX.len();
        let mut out = header(start_html, end_html, start_fragment, end_fragment).into_bytes();
        out.extend_from_slice(HTML_PREFIX.as_bytes());
        out.extend_from_slice(markup.as_bytes());
        out.extend_from_slice(HTML_SUFFIX.as_bytes());
        out
    }

    fn header(
        start_html: usize,
        end_html: usize,
        start_fragment: usize,
        end_fragment: usize,
    ) -> String {
        format!(
            "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
             StartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
        )
    }

    /// The fragment of a CF_HTML document, or its whole HTML span when the
    /// fragment offsets are absent or out of range.
    fn decode_html(bytes: &[u8]) -> Result<String, ()> {
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let bytes = &bytes[..end];
        // The header is ASCII and short; the markup after it may be anything.
        let header = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        let offset = |key: &str| -> Option<usize> {
            header
                .lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
                .and_then(|value| value.trim().parse().ok())
        };
        let span = |start: Option<usize>, end: Option<usize>| match (start, end) {
            (Some(start), Some(end)) if start <= end && end <= bytes.len() => {
                Some(&bytes[start..end])
            }
            _ => None,
        };
        let chosen = span(offset("StartFragment"), offset("EndFragment"))
            .or_else(|| span(offset("StartHTML"), offset("EndHTML")))
            .ok_or(())?;
        String::from_utf8(chosen.to_vec()).map_err(|_| ())
    }

    /// The size of a `DROPFILES` header.
    const DROPFILES_LEN: usize = 20;

    /// `CF_HDROP`: a `DROPFILES` header naming wide paths, then each path with a
    /// NUL and a final NUL.
    #[must_use]
    pub(super) fn encode_file_drop(paths: &[&std::path::Path]) -> Vec<u8> {
        let mut out = Vec::with_capacity(DROPFILES_LEN);
        out.extend_from_slice(&u32::try_from(DROPFILES_LEN).unwrap_or(20).to_le_bytes());
        out.extend_from_slice(&0_i32.to_le_bytes()); // pt.x
        out.extend_from_slice(&0_i32.to_le_bytes()); // pt.y
        out.extend_from_slice(&0_i32.to_le_bytes()); // fNC
        out.extend_from_slice(&1_i32.to_le_bytes()); // fWide
        for path in paths {
            for unit in path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
            {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out.extend_from_slice(&0_u16.to_le_bytes());
        out
    }

    /// The paths of a `CF_HDROP` allocation, wide or ANSI, bounded by it.
    fn decode_file_drop(bytes: &[u8]) -> Result<Vec<PathBuf>, ()> {
        let field = |at: usize| -> Option<u32> {
            bytes
                .get(at..at + 4)
                .map(|raw| u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
        };
        let start = usize::try_from(field(0).ok_or(())?).map_err(|_| ())?;
        let wide = field(16).ok_or(())? != 0;
        let list = bytes.get(start..).ok_or(())?;
        let mut paths = Vec::new();
        if wide {
            let units: Vec<u16> = utf16_units(list).collect();
            for name in units.split(|unit| *unit == 0) {
                if name.is_empty() {
                    break;
                }
                paths.push(PathBuf::from(String::from_utf16(name).map_err(|_| ())?));
            }
        } else {
            for name in list.split(|byte| *byte == 0) {
                if name.is_empty() {
                    break;
                }
                paths.push(PathBuf::from(String::from_utf8_lossy(name).into_owned()));
            }
        }
        Ok(paths)
    }

    /// `MK_LBUTTON`, `MK_RBUTTON`, `MK_MBUTTON`, `MK_XBUTTON1` and `MK_XBUTTON2`.
    pub(super) const MOUSE_BUTTONS: u32 = 0x01 | 0x02 | 0x10 | 0x20 | 0x40;

    /// What the drop source tells the OLE loop (`IDropSource::QueryContinueDrag`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum DragContinuation {
        /// Keep dragging (`S_OK`).
        Continue,
        /// Drop on the target under the pointer (`DRAGDROP_S_DROP`).
        Drop,
        /// End without a drop (`DRAGDROP_S_CANCEL`).
        Cancel,
    }

    /// Whether a drag that began with the mouse buttons `held_at_start` goes on,
    /// given Escape and the key state `keys` (`MK_*` bits): Escape cancels;
    /// releasing the buttons that began it drops; pressing another button
    /// cancels, as Windows' own sources do. A drag no button began cannot be
    /// released, so it is cancelled rather than dropped wherever the cursor is.
    #[must_use]
    pub(super) const fn continue_drag(
        escape: bool,
        keys: u32,
        held_at_start: u32,
    ) -> DragContinuation {
        let began = held_at_start & MOUSE_BUTTONS;
        let held = keys & MOUSE_BUTTONS;
        if escape || began == 0 || held & !began != 0 {
            DragContinuation::Cancel
        } else if held & began == 0 {
            DragContinuation::Drop
        } else {
            DragContinuation::Continue
        }
    }

    /// The mouse buttons (`MK_*`) whose release ends an OS session `DoDragDrop`
    /// would run for a drag of a `kind` contact, given the buttons `held` in the
    /// thread's input state as it starts; `None` when OLE's loop cannot carry it.
    ///
    /// That loop follows the mouse alone. A pen or touch contact a window takes
    /// as `WM_POINTER` without `DefWindowProc` is never promoted to mouse input,
    /// so it sets no mouse button in the key state the loop reads; with no button
    /// held the loop could neither follow the contact nor see it lift, and the
    /// drag stays in its window instead (ADR-0211 §11).
    #[must_use]
    pub(super) fn os_loop_buttons(kind: Option<PointerKind>, held: u32) -> Option<u32> {
        if matches!(kind, Some(PointerKind::Touch | PointerKind::Pen { .. })) {
            return None;
        }
        Some(held & MOUSE_BUTTONS).filter(|buttons| *buttons != 0)
    }

    #[cfg(test)]
    mod tests;
}

// The COM side, on Windows alone.

#[cfg(windows)]
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, OnceLock},
    time::Duration,
};

#[cfg(windows)]
use flui_foundation::DataTransferId;
#[cfg(windows)]
use flui_platform::{
    data_transfer::{
        DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
        RepresentationDescriptor, RepresentationIndex, TransferError, TransferLimits,
        TransferRequest,
    },
    drag::{DragError, OsDragEnd, OsDragRequest, OsDragStart, PlatformDragSource},
    pointer::PointerKind,
};
#[cfg(windows)]
use parking_lot::Mutex;
#[cfg(windows)]
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
#[cfg(windows)]
use windows_core::{ComObject, HSTRING};

#[cfg(windows)]
use crate::{
    input::WindowInput,
    owner::modal_pace::MODAL_FRAME_PACE_MS,
    owner::panic_boundary::{contain_owner_callback, owner_callback_panicked},
    owner::slot::OwnerSlot,
    owner::windows::WindowRef,
    owner::windows::control::{OwnerMessages, OwnerPost, OwnerWindow},
    owner::windows::{CreatedWindow, HostDrag, WindowMessages},
};
#[cfg(windows)]
use crossing::{
    NativeFormat, actions_of, decode, effect_bits, offer_representations, os_loop_buttons,
};

#[cfg(windows)]
pub(crate) use com::WindowDropTarget;

/// The registered name of the `text/html` clipboard format (CF_HTML).
#[cfg(windows)]
const HTML_FORMAT_NAME: &str = "HTML Format";

/// Posted to the owner window: run the session [`OleTransfer::start`] recorded.
#[cfg(windows)]
const START_DRAG: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 21;
/// Sent to the owner window while `DoDragDrop` runs: deliver the frames that
/// are due; answered with the milliseconds until the next request.
#[cfg(windows)]
const MODAL_FRAME: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 22;

/// The offers minted for drags over this platform's windows.
#[derive(Default)]
#[cfg(windows)]
struct Offers {
    table: OfferTable,
    /// The answer each live offer's target last cached.
    feedback: HashMap<DataTransferId, DropFeedback>,
}

/// One live offer's OS side: the drag's data object and the clipboard format
/// behind each representation.
#[cfg(windows)]
struct OpenOffer {
    object: IDataObject,
    formats: Vec<NativeFormat>,
    representations: Arc<[RepresentationDescriptor]>,
}

/// A session [`OleTransfer::start`] accepted, and the mouse buttons (`MK_*`)
/// held as it was accepted, whose release drops it.
#[cfg(windows)]
struct PendingStart {
    request: OsDragRequest,
    began: u32,
}

/// What only the owner thread touches: COM objects and the session's hooks.
#[derive(Default)]
#[cfg(windows)]
struct OwnerDrag {
    open: RefCell<HashMap<DataTransferId, OpenOffer>>,
    /// A session [`OleTransfer::start`] accepted whose `DoDragDrop` has not run.
    pending: RefCell<Option<PendingStart>>,
    /// Whether `DoDragDrop` is running.
    running: Cell<bool>,
}

#[cfg(windows)]
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

#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
pub struct OleTransfer {
    offers: Mutex<Offers>,
    owner: OwnerSlot<Rc<OwnerDrag>>,
    /// Where a session's start is posted: the owner window, once it exists.
    owner_window: OnceLock<OwnerPost>,
}

#[cfg(windows)]
impl std::fmt::Debug for OleTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OleTransfer").finish_non_exhaustive()
    }
}

#[cfg(windows)]
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
#[cfg(windows)]
struct RunningSession<'a>(&'a OwnerDrag);

#[cfg(windows)]
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
#[cfg(windows)]
struct ModalPacer {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
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

#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
impl HostDrag for OleTransfer {
    fn drag_source(self: Arc<Self>, window: WindowRef) -> Arc<dyn PlatformDragSource> {
        Win32DragSource::new(self, window)
    }

    fn register_target(self: Arc<Self>, window: WindowRef) {
        register_target(window, self);
    }
}

#[cfg(windows)]
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
#[cfg(windows)]
struct Win32DragSource {
    transfer: Arc<OleTransfer>,
    /// The window, reached only on its owner thread and only while its
    /// handle still names it.
    window: WindowRef,
}

#[cfg(windows)]
impl Win32DragSource {
    fn new(transfer: Arc<OleTransfer>, window: WindowRef) -> Arc<Self> {
        Arc::new(Self { transfer, window })
    }

    /// The kind of the contact whose press began `pointer`'s drag, as the
    /// window's pointer registry read it.
    fn contact_kind(&self, pointer: Option<flui_platform::PointerId>) -> Option<PointerKind> {
        let pointer = pointer?;
        let input = self.window.attached::<WindowInput>()?;
        let registry = input.registry.try_borrow().ok()?;
        registry.origin(pointer).map(|origin| origin.kind)
    }
}

#[cfg(windows)]
impl PlatformDragSource for Win32DragSource {
    fn start_timing(&self) -> OsDragStart {
        crate::input::availability::DESKTOP_DRAG_START
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let kind = self.contact_kind(request.pointer);
        self.transfer.start(request, kind)
    }
}

/// The hook on FLUI's window class that registers each window it creates as
/// an OLE drop target, from then on taking drags from this process or another
/// (ADR-0211). The window's context holds the loop's transport; the host lists
/// the hook after the text services and before the touchpad, so a window
/// activates TSF, then takes drags, then gets its viewport.
#[cfg(windows)]
pub(crate) struct DropTargetRegistration;

#[cfg(windows)]
impl WindowMessages for DropTargetRegistration {
    fn created(&self, window: &CreatedWindow) {
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
#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
fn mouse_buttons_held() -> u32 {
    crate::input::keys::MOUSE_BUTTONS
        .iter()
        .filter(|(key, _, _)| {
            // SAFETY: a query of this thread's key state by a constant code.
            let state = unsafe { GetKeyState(i32::from(*key)) };
            state < 0
        })
        .fold(0, |held, (_, bit, _)| held | bit)
}

/// The clipboard format `native` travels as; `None` when registration failed.
#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
const IDENTITY_MAX_BYTES: u64 = 4096;

/// The most entries read from a foreign data object's format enumerator:
/// far more than any source offers.
#[cfg(windows)]
const MAX_ENUMERATED_FORMATS: usize = 256;

/// What a foreign data object offers: its formats in its order, and the
/// session identity when it carries FLUI's.
#[cfg(windows)]
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
        .and_then(|bytes| crossing::identity_descriptor(&bytes));
    offer_representations(formats, identity)
}

/// Fetch `native` from `object` as bytes, at most `max_bytes` of them.
#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
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
