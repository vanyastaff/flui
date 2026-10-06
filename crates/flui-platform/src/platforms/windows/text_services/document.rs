//! One field's TSF document: the `ITextStoreACP` object TSF reads and edits
//! the field's [`TextStore`] through, and the state it shares with the
//! window's [`TextServices`].
//!
//! TSF calls in on the owner thread, from inside the window's message loop
//! and from inside our own calls into TSF. Every vtable method goes through
//! [`TsfStore::com_entry`], which keeps the document and the store alive for
//! the call (clones on the stack), counts the call as a COM entry on the
//! window's text services (so host operations arriving inside it are queued)
//! and contains a panic: a panic never unwinds into TSF, it poisons the
//! document instead.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{DevicePixelRatio, DevicePoint};
use flui_platform_api::text_store::{
    Composition, LockGrant, LockOutcome, LockTiming, PointMode, Selection, TextChange, TextStore,
    TextStoreEdit, TextStoreError, TextStoreObserver, TextStoreRead, TextStoreStatus, Utf16Offset,
    Utf16Range,
};
use windows::Win32::{
    Foundation::{
        E_FAIL, E_INVALIDARG, E_NOTIMPL, E_POINTER, E_UNEXPECTED, HWND, POINT, RECT, S_OK,
    },
    Graphics::Gdi::ClientToScreen,
    System::Com::{FORMATETC, IDataObject},
    UI::{
        TextServices::{
            GXFPF_ROUND_NEAREST, ITextStoreACP_Impl, ITextStoreACPSink, ITfCompositionView,
            ITfContextOwnerCompositionSink_Impl, ITfRange, ITfRangeACP, TEXT_STORE_LOCK_FLAGS,
            TEXT_STORE_TEXT_CHANGE_FLAGS, TS_AE_END, TS_AE_START, TS_AS_LAYOUT_CHANGE,
            TS_AS_SEL_CHANGE, TS_AS_STATUS_CHANGE, TS_AS_TEXT_CHANGE, TS_ATTRVAL,
            TS_DEFAULT_SELECTION, TS_E_INVALIDPOINT, TS_E_INVALIDPOS, TS_E_NOLAYOUT, TS_E_NOLOCK,
            TS_E_SYNCHRONOUS, TS_IAS_QUERYONLY, TS_LC_CHANGE, TS_LF_READWRITE, TS_LF_SYNC,
            TS_RT_PLAIN, TS_RUNINFO, TS_S_ASYNC, TS_SELECTION_ACP, TS_SELECTIONSTYLE,
            TS_SS_NOHIDDENTEXT, TS_STATUS, TS_TEXTCHANGE,
        },
        WindowsAndMessaging::GetClientRect,
    },
};
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Ref};

use super::TextServices;
use crate::shared::text_geometry::{ScreenRect, range_rect_to_screen, screen_point_to_client};

/// The state one TSF document shares between its COM object and the
/// window's [`TextServices`].
pub(super) struct DocumentState {
    pub(super) store: Rc<dyn TextStore>,
    hwnd: HWND,
    services: Weak<TextServices>,
    sink: RefCell<Option<AdvisedSink>>,
    slot: SessionSlot,
    /// Set when the document is closed: a grant still queued for it must not
    /// reach TSF, and every later call answers `E_UNEXPECTED`.
    pub(super) closed: Cell<bool>,
    /// Set when a call into this document panicked.
    pub(super) poisoned: Cell<bool>,
    /// The last rect `GetTextExt` answered, for diagnostics.
    pub(super) last_text_ext: Cell<Option<ScreenRect>>,
}

struct AdvisedSink {
    sink: ITextStoreACPSink,
    identity: IUnknown,
    mask: u32,
}

impl DocumentState {
    pub(super) fn new(
        store: Rc<dyn TextStore>,
        hwnd: HWND,
        services: Weak<TextServices>,
    ) -> Rc<Self> {
        Rc::new(Self {
            store,
            hwnd,
            services,
            sink: RefCell::new(None),
            slot: SessionSlot::default(),
            closed: Cell::new(false),
            poisoned: Cell::new(false),
            last_text_ext: Cell::new(None),
        })
    }

    /// Forget the sink and stop listening to the store; the document is
    /// gone for TSF. The sink goes first: retiring the observer runs the
    /// store's code, which may panic.
    pub(super) fn close(&self) {
        self.closed.set(true);
        let sink = self.sink.borrow_mut().take();
        drop(sink);
        self.store.set_observer(None);
    }

    /// The advised sink, if its mask includes `flag`. Cloned, so no borrow
    /// is held while TSF runs.
    fn sink_for(&self, flag: u32) -> Option<ITextStoreACPSink> {
        let sink = self.sink.borrow();
        sink.as_ref()
            .filter(|advised| advised.mask & flag != 0)
            .map(|advised| advised.sink.clone())
    }

    /// The window's scale and client origin in physical screen pixels.
    fn screen_frame(&self) -> (DevicePixelRatio, DevicePoint) {
        let scale = super::super::platform::with_window_context(self.hwnd, "text_services", |c| {
            c.scale_factor.get()
        })
        .and_then(DevicePixelRatio::new)
        .unwrap_or_default();
        let mut origin = POINT::default();
        // SAFETY: `origin` is a live, writable local; `ClientToScreen` only
        // writes through the pointer it is given. A dead window leaves it at
        // (0, 0), which the caller reports as no better than that.
        let _ = unsafe { ClientToScreen(self.hwnd, &raw mut origin) };
        (scale, DevicePoint::new(origin.x, origin.y))
    }

    /// Run one granted lock: open the session slot, hand TSF the lock and
    /// close the slot again (on unwind too). A grant for a closed document
    /// never reaches TSF.
    fn run_grant(&self, session: Session<'_>, flags: u32, result: &Cell<Option<HRESULT>>) {
        if self.closed.get() {
            tracing::debug!(target: "flui_platform::tsf", "grant for a closed document dropped");
            result.set(Some(E_UNEXPECTED));
            return;
        }
        let Some(sink) = self.sink_for(u32::MAX) else {
            result.set(Some(E_UNEXPECTED));
            return;
        };
        self.slot.with_open(session, || {
            tracing::debug!(target: "flui_platform::tsf", flags, "OnLockGranted");
            // SAFETY: a plain COM call on a sink TSF advised; no pointers.
            let granted = unsafe { sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(flags)) };
            result.set(Some(granted.map_or_else(|error| error.code(), |()| S_OK)));
        });
    }
}

/// The session a granted lock opened, as the slot holds it.
pub(super) enum Session<'a> {
    Read(&'a dyn TextStoreRead),
    Write(&'a mut dyn TextStoreEdit),
}

/// A session with its lifetime erased, valid only while
/// [`SessionSlot::with_open`]'s frame is on the stack.
#[derive(Clone, Copy)]
enum ErasedSession {
    Read(*const (dyn TextStoreRead + 'static)),
    Write(*mut (dyn TextStoreEdit + 'static)),
}

/// Where a granted lock's session waits for the TSF calls made under it.
///
/// TSF reads and edits by calling the text store's methods from inside
/// `OnLockGranted`; those calls reach the session through this slot. It is
/// open only while [`Self::with_open`] runs, and at most one method borrows
/// the session at a time (`in_use`): a method reached reentrantly while
/// another holds it answers `E_UNEXPECTED` instead of taking a second
/// `&mut`.
#[derive(Default)]
struct SessionSlot {
    session: Cell<Option<ErasedSession>>,
    in_use: Cell<bool>,
}

/// Restores the slot's previous value when `with_open`'s frame ends,
/// unwinding included.
struct SlotRestore<'a> {
    slot: &'a SessionSlot,
    previous: Option<ErasedSession>,
}

impl Drop for SlotRestore<'_> {
    fn drop(&mut self) {
        self.slot.session.set(self.previous);
    }
}

/// Clears the slot's `in_use` flag when a method's borrow ends.
struct InUse<'a>(&'a Cell<bool>);

impl Drop for InUse<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl SessionSlot {
    /// Run `f` with `session` reachable through the slot.
    #[expect(
        clippy::transmute_ptr_to_ptr,
        reason = "only the trait object's lifetime bound changes, which a cast cannot do"
    )]
    fn with_open<R>(&self, session: Session<'_>, f: impl FnOnce() -> R) -> R {
        let erased = match session {
            // SAFETY: only the trait object's lifetime bound changes (the
            // pointer's layout is the same). This frame keeps the borrow the
            // pointer came from alive and unused until `SlotRestore` closes
            // the slot again, before this function returns or unwinds, so
            // the pointer never outlives the session it points to.
            Session::Read(read) => ErasedSession::Read(unsafe {
                std::mem::transmute::<
                    *const (dyn TextStoreRead + '_),
                    *const (dyn TextStoreRead + 'static),
                >(read)
            }),
            // SAFETY: as above, for the read-write session.
            Session::Write(write) => ErasedSession::Write(unsafe {
                std::mem::transmute::<
                    *mut (dyn TextStoreEdit + '_),
                    *mut (dyn TextStoreEdit + 'static),
                >(write)
            }),
        };
        let _restore = SlotRestore {
            slot: self,
            previous: self.session.replace(Some(erased)),
        };
        f()
    }

    fn borrow(&self) -> windows_core::Result<(ErasedSession, InUse<'_>)> {
        let session = self.session.get().ok_or(TS_E_NOLOCK)?;
        if self.in_use.replace(true) {
            tracing::warn!(target: "flui_platform::tsf", "nested session borrow refused");
            return Err(E_UNEXPECTED.into());
        }
        Ok((session, InUse(&self.in_use)))
    }

    /// Run `f` on the open session, read or read-write.
    fn read<R>(
        &self,
        f: impl FnOnce(&dyn TextStoreRead) -> windows_core::Result<R>,
    ) -> windows_core::Result<R> {
        let (session, _in_use) = self.borrow()?;
        match session {
            // SAFETY: the slot holds a pointer only while `with_open`'s frame
            // keeps its session alive (see there); `in_use` makes this the
            // only borrow of it; the slot is owner-thread only (`Cell`).
            ErasedSession::Read(read) => f(unsafe { &*read }),
            // SAFETY: as above.
            ErasedSession::Write(write) => f(unsafe { &*write }),
        }
    }

    /// Run `f` on the open session; a read-only session answers
    /// `TS_E_NOLOCK`.
    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn TextStoreEdit) -> windows_core::Result<R>,
    ) -> windows_core::Result<R> {
        let (session, _in_use) = self.borrow()?;
        match session {
            ErasedSession::Read(_) => Err(TS_E_NOLOCK.into()),
            // SAFETY: as in `read`; `in_use` rules out a second `&mut`.
            ErasedSession::Write(write) => f(unsafe { &mut *write }),
        }
    }
}

/// A store error as the HRESULT TSF expects.
fn store_error(error: TextStoreError) -> windows_core::Error {
    match error {
        TextStoreError::SyncLockUnavailable => TS_E_SYNCHRONOUS,
        TextStoreError::Offset(_) => TS_E_INVALIDPOS,
        TextStoreError::NoLayout => TS_E_NOLAYOUT,
        TextStoreError::PointOutside => TS_E_INVALIDPOINT,
        TextStoreError::Detached => E_UNEXPECTED,
        _ => E_FAIL,
    }
    .into()
}

/// What `GetStatus` tells TSF about a store whose status is `status`, read
/// again whenever the store reports a status change (`OnStatusChange`).
///
/// A protected field's text cannot be read out, so it is not advertised as
/// free of hidden text (`TS_SS_NOHIDDENTEXT`). It stays editable: no
/// `TS_SD_READONLY`, which would refuse the input method's edits.
pub(super) fn ts_status(status: TextStoreStatus) -> TS_STATUS {
    TS_STATUS {
        dwDynamicFlags: 0,
        dwStaticFlags: if status.protected {
            0
        } else {
            TS_SS_NOHIDDENTEXT
        },
    }
}

fn offset(acp: i32) -> windows_core::Result<Utf16Offset> {
    usize::try_from(acp)
        .map(Utf16Offset::new)
        .map_err(|_| TS_E_INVALIDPOS.into())
}

fn acp(offset: Utf16Offset) -> i32 {
    i32::try_from(offset.get()).unwrap_or(i32::MAX)
}

fn range(start: i32, end: i32) -> windows_core::Result<Utf16Range> {
    Utf16Range::new(offset(start)?, offset(end)?).ok_or_else(|| TS_E_INVALIDPOS.into())
}

fn text_change(change: TextChange) -> TS_TEXTCHANGE {
    TS_TEXTCHANGE {
        acpStart: acp(change.start),
        acpOldEnd: acp(change.old_end),
        acpNewEnd: acp(change.new_end),
    }
}

/// Write `value` through an out-pointer TSF passed, if it passed one.
fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: non-null was just checked; TSF owns the pointee for the
        // length of the call that handed it to us (the `ITextStoreACP`
        // out-parameter contract).
        unsafe { out.write(value) };
    }
}

/// The ACP span a TSF range covers.
fn range_extent(range: &ITfRange) -> windows_core::Result<Utf16Range> {
    let range: ITfRangeACP = range.cast()?;
    let (mut start, mut len) = (0, 0);
    // SAFETY: both out-pointers are live, writable locals.
    unsafe { range.GetExtent(&raw mut start, &raw mut len) }?;
    self::range(start, start.saturating_add(len))
}

pub(super) use com_object::{TsfStore, TsfStore_Impl};

/// The `#[implement]` expansion, in a module of its own so the lints its
/// generated code trips are expected here and nowhere else.
#[expect(
    clippy::ref_as_ptr,
    clippy::inline_always,
    raw_borrows_via_references,
    reason = "code generated by windows-core's #[implement]"
)]
mod com_object {
    use std::rc::Rc;

    use windows::Win32::UI::TextServices::{ITextStoreACP, ITfContextOwnerCompositionSink};
    use windows_core::implement;

    use super::DocumentState;

    /// The `ITextStoreACP` object TSF holds for one document.
    #[implement(ITextStoreACP, ITfContextOwnerCompositionSink)]
    pub(in super::super) struct TsfStore {
        pub(super) state: Rc<DocumentState>,
    }
}

impl TsfStore {
    pub(super) fn new(state: Rc<DocumentState>) -> Self {
        Self { state }
    }

    /// The boundary every vtable method crosses; see the module doc.
    fn com_entry<R>(
        &self,
        method: &'static str,
        body: impl FnOnce(&DocumentState) -> windows_core::Result<R>,
    ) -> windows_core::Result<R> {
        let state = Rc::clone(&self.state);
        let _store = Rc::clone(&state.store);
        if state.poisoned.get() || state.closed.get() {
            return Err(E_UNEXPECTED.into());
        }
        let services = state.services.upgrade();
        if let Some(services) = &services {
            services.enter();
        }
        let result = match catch_unwind(AssertUnwindSafe(|| body(&state))) {
            Ok(result) => result,
            Err(payload) => {
                state.poisoned.set(true);
                let message = crate::shared::panic_boundary::panic_payload_message(&*payload);
                tracing::error!(
                    target: "flui_platform::tsf",
                    method,
                    panic = message,
                    "a TSF call into the text store panicked; the document is poisoned"
                );
                // A payload whose `Drop` panics must not unwind into TSF.
                std::mem::forget(payload);
                if let Some(services) = &services {
                    services.document_poisoned(&state);
                }
                Err(E_UNEXPECTED.into())
            }
        };
        if let Some(services) = &services {
            services.leave();
        }
        result
    }
}

impl ITextStoreACP_Impl for TsfStore_Impl {
    fn AdviseSink(
        &self,
        riid: *const GUID,
        punk: Ref<'_, IUnknown>,
        dwmask: u32,
    ) -> windows_core::Result<()> {
        self.com_entry("AdviseSink", |state| {
            // SAFETY: TSF passes a valid IID pointer; null was checked.
            if riid.is_null() || unsafe { *riid } != ITextStoreACPSink::IID {
                return Err(E_INVALIDARG.into());
            }
            let identity: IUnknown = punk.ok()?.cast()?;
            let mut advised = state.sink.borrow_mut();
            if let Some(existing) = advised.as_mut() {
                if existing.identity == identity {
                    existing.mask = dwmask;
                    return Ok(());
                }
                return Err(windows::Win32::System::Ole::CONNECT_E_ADVISELIMIT.into());
            }
            *advised = Some(AdvisedSink {
                sink: identity.cast()?,
                identity,
                mask: dwmask,
            });
            drop(advised);
            let observer: Rc<dyn TextStoreObserver> = Rc::new(SinkObserver {
                state: Rc::downgrade(&self.state),
            });
            state.store.set_observer(Some(observer));
            tracing::debug!(target: "flui_platform::tsf", dwmask, "AdviseSink");
            Ok(())
        })
    }

    fn UnadviseSink(&self, punk: Ref<'_, IUnknown>) -> windows_core::Result<()> {
        self.com_entry("UnadviseSink", |state| {
            let identity: IUnknown = punk.ok()?.cast()?;
            let removed = {
                let mut advised = state.sink.borrow_mut();
                if advised.as_ref().is_some_and(|a| a.identity == identity) {
                    advised.take()
                } else {
                    None
                }
            };
            if removed.is_none() {
                return Err(windows::Win32::System::Ole::CONNECT_E_NOCONNECTION.into());
            }
            state.store.set_observer(None);
            Ok(())
        })
    }

    fn RequestLock(&self, dwlockflags: u32) -> windows_core::Result<HRESULT> {
        self.com_entry("RequestLock", |state| {
            let timing = if dwlockflags & TS_LF_SYNC == 0 {
                LockTiming::Async
            } else {
                LockTiming::Sync
            };
            let read_write =
                dwlockflags & TS_LF_READWRITE.0 == TS_LF_READWRITE.0;
            let flags = dwlockflags & !TS_LF_SYNC;
            let session_result = Rc::new(Cell::new(None));
            let (grant_state, grant_result) = (Rc::clone(&self.state), Rc::clone(&session_result));
            let grant = if read_write {
                LockGrant::read_write(move |session| {
                    grant_state.run_grant(Session::Write(session), flags, &grant_result);
                })
            } else {
                LockGrant::read(move |session| {
                    grant_state.run_grant(Session::Read(session), flags, &grant_result);
                })
            };
            let outcome = state.store.request_lock(grant, timing);
            let answer = match outcome {
                Ok(LockOutcome::Granted) => session_result.get().unwrap_or(S_OK),
                Ok(LockOutcome::Deferred) => TS_S_ASYNC,
                Err(TextStoreError::SyncLockUnavailable) => TS_E_SYNCHRONOUS,
                Err(error) => {
                    tracing::debug!(target: "flui_platform::tsf", dwlockflags, ?error, "RequestLock refused");
                    return Err(store_error(error));
                }
            };
            tracing::debug!(
                target: "flui_platform::tsf",
                dwlockflags,
                ?outcome,
                session = ?answer,
                "RequestLock"
            );
            Ok(answer)
        })
    }

    fn GetStatus(&self) -> windows_core::Result<TS_STATUS> {
        self.com_entry("GetStatus", |state| Ok(ts_status(state.store.status())))
    }

    fn QueryInsert(
        &self,
        acpteststart: i32,
        acptestend: i32,
        _cch: u32,
        pacpresultstart: *mut i32,
        pacpresultend: *mut i32,
    ) -> windows_core::Result<()> {
        self.com_entry("QueryInsert", |_| {
            range(acpteststart, acptestend)?;
            put(pacpresultstart, acpteststart);
            put(pacpresultend, acptestend);
            Ok(())
        })
    }

    fn GetSelection(
        &self,
        ulindex: u32,
        ulcount: u32,
        pselection: *mut TS_SELECTION_ACP,
        pcfetched: *mut u32,
    ) -> windows_core::Result<()> {
        self.com_entry("GetSelection", |state| {
            if ulindex != 0 && ulindex != TS_DEFAULT_SELECTION {
                return Err(E_INVALIDARG.into());
            }
            if pselection.is_null() || ulcount == 0 {
                put(pcfetched, 0);
                return Ok(());
            }
            let selection = state.slot.read(|session| Ok(session.selection()))?;
            let span = selection.range();
            put(
                pselection,
                TS_SELECTION_ACP {
                    acpStart: acp(span.start()),
                    acpEnd: acp(span.end()),
                    style: TS_SELECTIONSTYLE {
                        ase: if selection.active < selection.anchor {
                            TS_AE_START
                        } else {
                            TS_AE_END
                        },
                        fInterimChar: false.into(),
                    },
                },
            );
            put(pcfetched, 1);
            Ok(())
        })
    }

    fn SetSelection(
        &self,
        ulcount: u32,
        pselection: *const TS_SELECTION_ACP,
    ) -> windows_core::Result<()> {
        self.com_entry("SetSelection", |state| {
            if ulcount != 1 || pselection.is_null() {
                return Err(E_INVALIDARG.into());
            }
            // SAFETY: non-null was checked; TSF passes one selection.
            let wanted = unsafe { *pselection };
            let (start, end) = (offset(wanted.acpStart)?, offset(wanted.acpEnd)?);
            let selection = if wanted.style.ase == TS_AE_START {
                Selection {
                    anchor: end,
                    active: start,
                }
            } else {
                Selection {
                    anchor: start,
                    active: end,
                }
            };
            state
                .slot
                .write(|session| session.set_selection(selection).map_err(store_error))
        })
    }

    fn GetText(
        &self,
        acpstart: i32,
        acpend: i32,
        pchplain: PWSTR,
        cchplainreq: u32,
        pcchplainret: *mut u32,
        prgruninfo: *mut TS_RUNINFO,
        cruninforeq: u32,
        pcruninforet: *mut u32,
        pacpnext: *mut i32,
    ) -> windows_core::Result<()> {
        self.com_entry("GetText", |state| {
            let units = state.slot.read(|session| {
                let end = if acpend == -1 {
                    acp(session.document_len())
                } else {
                    acpend
                };
                let text = session.text(range(acpstart, end)?).map_err(store_error)?;
                Ok(text.encode_utf16().collect::<Vec<u16>>())
            })?;
            let wanted = usize::try_from(cchplainreq).unwrap_or(usize::MAX);
            let copied = if pchplain.is_null() {
                0
            } else {
                units.len().min(wanted)
            };
            if copied > 0 {
                // SAFETY: TSF's buffer holds `cchplainreq` units and
                // `copied` is at most that; the source is our own vector.
                unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), pchplain.0, copied) };
            }
            let copied_u32 = u32::try_from(copied).unwrap_or(u32::MAX);
            put(pcchplainret, copied_u32);
            if cruninforeq > 0 && copied > 0 {
                put(
                    prgruninfo,
                    TS_RUNINFO {
                        uCount: copied_u32,
                        r#type: TS_RT_PLAIN,
                    },
                );
                put(pcruninforet, 1);
            } else {
                put(pcruninforet, 0);
            }
            put(
                pacpnext,
                acpstart.saturating_add(i32::try_from(copied).unwrap_or(i32::MAX)),
            );
            Ok(())
        })
    }

    fn SetText(
        &self,
        _dwflags: u32,
        acpstart: i32,
        acpend: i32,
        pchtext: &PCWSTR,
        cch: u32,
    ) -> windows_core::Result<TS_TEXTCHANGE> {
        self.com_entry("SetText", |state| {
            let text = wide_text(pchtext, cch)?;
            let span = range(acpstart, acpend)?;
            tracing::debug!(target: "flui_platform::tsf", acpstart, acpend, text, "SetText");
            state
                .slot
                .write(|session| session.replace(span, &text).map_err(store_error))
                .map(text_change)
        })
    }

    fn GetFormattedText(&self, _acpstart: i32, _acpend: i32) -> windows_core::Result<IDataObject> {
        Err(E_NOTIMPL.into())
    }

    fn GetEmbedded(
        &self,
        _acppos: i32,
        _rguidservice: *const GUID,
        _riid: *const GUID,
    ) -> windows_core::Result<IUnknown> {
        Err(E_NOTIMPL.into())
    }

    fn QueryInsertEmbedded(
        &self,
        _pguidservice: *const GUID,
        _pformatetc: *const FORMATETC,
    ) -> windows_core::Result<BOOL> {
        Ok(false.into())
    }

    fn InsertEmbedded(
        &self,
        _dwflags: u32,
        _acpstart: i32,
        _acpend: i32,
        _pdataobject: Ref<'_, IDataObject>,
    ) -> windows_core::Result<TS_TEXTCHANGE> {
        Err(E_NOTIMPL.into())
    }

    fn InsertTextAtSelection(
        &self,
        dwflags: u32,
        pchtext: &PCWSTR,
        cch: u32,
        pacpstart: *mut i32,
        pacpend: *mut i32,
        pchange: *mut TS_TEXTCHANGE,
    ) -> windows_core::Result<()> {
        self.com_entry("InsertTextAtSelection", |state| {
            if dwflags & TS_IAS_QUERYONLY != 0 {
                let span = state.slot.read(|session| Ok(session.selection().range()))?;
                put(pacpstart, acp(span.start()));
                put(pacpend, acp(span.end()));
                return Ok(());
            }
            let text = wide_text(pchtext, cch)?;
            tracing::debug!(target: "flui_platform::tsf", text, "InsertTextAtSelection");
            let change = state
                .slot
                .write(|session| session.insert_at_selection(&text).map_err(store_error))?;
            put(pacpstart, acp(change.start));
            put(pacpend, acp(change.new_end));
            put(pchange, text_change(change));
            Ok(())
        })
    }

    fn InsertEmbeddedAtSelection(
        &self,
        _dwflags: u32,
        _pdataobject: Ref<'_, IDataObject>,
        _pacpstart: *mut i32,
        _pacpend: *mut i32,
        _pchange: *mut TS_TEXTCHANGE,
    ) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn RequestSupportedAttrs(
        &self,
        _dwflags: u32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn RequestAttrsAtPosition(
        &self,
        _acppos: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn RequestAttrsTransitioningAtPosition(
        &self,
        _acppos: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> windows_core::Result<()> {
        Ok(())
    }

    fn FindNextAttrTransition(
        &self,
        _acpstart: i32,
        acphalt: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
        pacpnext: *mut i32,
        pffound: *mut BOOL,
        plfoundoffset: *mut i32,
    ) -> windows_core::Result<()> {
        put(pacpnext, acphalt);
        put(pffound, false.into());
        put(plfoundoffset, 0);
        Ok(())
    }

    fn RetrieveRequestedAttrs(
        &self,
        _ulcount: u32,
        _paattrvals: *mut TS_ATTRVAL,
        pcfetched: *mut u32,
    ) -> windows_core::Result<()> {
        put(pcfetched, 0);
        Ok(())
    }

    fn GetEndACP(&self) -> windows_core::Result<i32> {
        self.com_entry("GetEndACP", |state| {
            state.slot.read(|session| Ok(acp(session.document_len())))
        })
    }

    fn GetActiveView(&self) -> windows_core::Result<u32> {
        Ok(0)
    }

    fn GetACPFromPoint(
        &self,
        _vcview: u32,
        ptscreen: *const POINT,
        dwflags: u32,
    ) -> windows_core::Result<i32> {
        self.com_entry("GetACPFromPoint", |state| {
            if ptscreen.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: non-null was checked; TSF passes one point.
            let screen = unsafe { *ptscreen };
            let (scale, origin) = state.screen_frame();
            let logical =
                screen_point_to_client(DevicePoint::new(screen.x, screen.y), origin, scale);
            let mode = if dwflags & GXFPF_ROUND_NEAREST == 0 {
                PointMode::Exact
            } else {
                PointMode::Nearest
            };
            state.slot.read(|session| {
                session
                    .index_at_point(logical, mode)
                    .map(acp)
                    .map_err(store_error)
            })
        })
    }

    fn GetTextExt(
        &self,
        _vcview: u32,
        acpstart: i32,
        acpend: i32,
        prc: *mut RECT,
        pfclipped: *mut BOOL,
    ) -> windows_core::Result<()> {
        self.com_entry("GetTextExt", |state| {
            let span = range(acpstart, acpend)?;
            let rect = state.slot.read(|session| {
                session.rect_for_range(span).map_err(store_error)
            });
            let rect = match rect {
                Ok(rect) => rect,
                Err(error) => {
                    tracing::debug!(
                        target: "flui_platform::tsf",
                        acpstart,
                        acpend,
                        error = ?error.code(),
                        "GetTextExt"
                    );
                    return Err(error);
                }
            };
            let (scale, origin) = state.screen_frame();
            let screen = range_rect_to_screen(rect.bounds, origin, scale).map_err(|error| {
                tracing::debug!(target: "flui_platform::tsf", %error, "GetTextExt has no screen rect");
                windows_core::Error::from(TS_E_NOLAYOUT)
            })?;
            state.last_text_ext.set(Some(screen));
            tracing::debug!(target: "flui_platform::tsf", acpstart, acpend, ?screen, "GetTextExt");
            put(
                prc,
                RECT {
                    left: screen.left,
                    top: screen.top,
                    right: screen.right,
                    bottom: screen.bottom,
                },
            );
            put(pfclipped, rect.clipped.into());
            Ok(())
        })
    }

    fn GetScreenExt(&self, _vcview: u32) -> windows_core::Result<RECT> {
        self.com_entry("GetScreenExt", |state| {
            let (scale, origin) = state.screen_frame();
            let bounds = state
                .slot
                .read(|session| session.document_bounds().map_err(store_error));
            if let Ok(bounds) = bounds
                && let Ok(screen) = range_rect_to_screen(bounds, origin, scale)
            {
                return Ok(RECT {
                    left: screen.left,
                    top: screen.top,
                    right: screen.right,
                    bottom: screen.bottom,
                });
            }
            // Outside a lock, or before a layout: the client area.
            let mut client = RECT::default();
            // SAFETY: `client` is a live, writable local.
            unsafe { GetClientRect(state.hwnd, &raw mut client) }?;
            Ok(RECT {
                left: client.left + origin.x,
                top: client.top + origin.y,
                right: client.right + origin.x,
                bottom: client.bottom + origin.y,
            })
        })
    }

    fn GetWnd(&self, _vcview: u32) -> windows_core::Result<HWND> {
        self.com_entry("GetWnd", |state| Ok(state.hwnd))
    }
}

impl ITfContextOwnerCompositionSink_Impl for TsfStore_Impl {
    fn OnStartComposition(
        &self,
        pcomposition: Ref<'_, ITfCompositionView>,
    ) -> windows_core::Result<BOOL> {
        self.com_entry("OnStartComposition", |state| {
            // SAFETY: a plain COM call on the view TSF passed.
            let range = unsafe { pcomposition.ok()?.GetRange() }?;
            let span = range_extent(&range)?;
            tracing::debug!(target: "flui_platform::tsf", ?span, "OnStartComposition");
            set_composition(state, Some(span))?;
            Ok(true.into())
        })
    }

    fn OnUpdateComposition(
        &self,
        pcomposition: Ref<'_, ITfCompositionView>,
        prangenew: Ref<'_, ITfRange>,
    ) -> windows_core::Result<()> {
        self.com_entry("OnUpdateComposition", |state| {
            let range = match prangenew.as_ref() {
                Some(range) => range.clone(),
                // SAFETY: a plain COM call on the view TSF passed.
                None => unsafe { pcomposition.ok()?.GetRange() }?,
            };
            let span = range_extent(&range)?;
            tracing::debug!(target: "flui_platform::tsf", ?span, "OnUpdateComposition");
            set_composition(state, Some(span))
        })
    }

    fn OnEndComposition(
        &self,
        _pcomposition: Ref<'_, ITfCompositionView>,
    ) -> windows_core::Result<()> {
        self.com_entry("OnEndComposition", |state| {
            tracing::debug!(target: "flui_platform::tsf", "OnEndComposition");
            set_composition(state, None)
        })
    }
}

/// Apply a composition change TSF reported, which it does from inside a
/// read-write session; outside one there is nothing to apply it to.
fn set_composition(state: &DocumentState, span: Option<Utf16Range>) -> windows_core::Result<()> {
    let composition = span.map(|range| Composition {
        range,
        hides_caret: false,
    });
    state
        .slot
        .write(|session| session.set_composition(composition).map_err(store_error))
        .inspect_err(|error| {
            tracing::warn!(
                target: "flui_platform::tsf",
                error = ?error.code(),
                "composition change outside a read-write session"
            );
        })
}

/// `cch` UTF-16 units at `text` as a `String`; unpaired surrogates are an
/// invalid argument, not a lossy replacement.
fn wide_text(text: &PCWSTR, cch: u32) -> windows_core::Result<String> {
    let len = usize::try_from(cch).map_err(|_| E_INVALIDARG)?;
    if len == 0 {
        return Ok(String::new());
    }
    if text.is_null() {
        return Err(E_INVALIDARG.into());
    }
    // SAFETY: TSF passes `cch` readable units at a non-null `text`.
    let units = unsafe { std::slice::from_raw_parts(text.0, len) };
    String::from_utf16(units).map_err(|_| E_INVALIDARG.into())
}

/// The store's notifications forwarded to the advised `ITextStoreACPSink`.
struct SinkObserver {
    state: Weak<DocumentState>,
}

impl SinkObserver {
    fn notify(
        &self,
        flag: u32,
        what: &'static str,
        call: impl FnOnce(&ITextStoreACPSink) -> windows_core::Result<()>,
    ) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        if state.closed.get() {
            return;
        }
        let Some(sink) = state.sink_for(flag) else {
            return;
        };
        tracing::debug!(target: "flui_platform::tsf", what, "sink notification");
        if let Err(error) = call(&sink) {
            tracing::debug!(target: "flui_platform::tsf", what, ?error, "sink notification failed");
        }
    }
}

impl TextStoreObserver for SinkObserver {
    fn text_changed(&self, change: TextChange) {
        let change = text_change(change);
        // SAFETY: plain COM calls on the advised sink; `change` is a live
        // local for the call's length.
        self.notify(TS_AS_TEXT_CHANGE, "OnTextChange", |sink| unsafe {
            sink.OnTextChange(TEXT_STORE_TEXT_CHANGE_FLAGS(0), &raw const change)
        });
    }

    fn selection_changed(&self) {
        // SAFETY: a plain COM call on the advised sink.
        self.notify(TS_AS_SEL_CHANGE, "OnSelectionChange", |sink| unsafe {
            sink.OnSelectionChange()
        });
    }

    fn layout_changed(&self) {
        // SAFETY: a plain COM call on the advised sink.
        self.notify(TS_AS_LAYOUT_CHANGE, "OnLayoutChange", |sink| unsafe {
            sink.OnLayoutChange(TS_LC_CHANGE, 0)
        });
    }

    fn status_changed(&self) {
        // SAFETY: a plain COM call on the advised sink.
        self.notify(TS_AS_STATUS_CHANGE, "OnStatusChange", |sink| unsafe {
            sink.OnStatusChange(0)
        });
    }
}
