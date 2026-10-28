//! One field's TSF document: the `ITextStoreACP` object TSF reads and edits
//! the field's [`TextStore`] through, and the state it shares with the
//! window's [`TextServices`].
//!
//! TSF calls in on the owner thread, from inside the window's message loop
//! and from inside our own calls into TSF. Every vtable method that reads or
//! edits the document goes through [`TsfStore::document_method`], which keeps
//! the document and the store alive for the call (clones on the stack), counts
//! the call as a COM entry on the window's text services (so host operations
//! arriving inside it are queued) and contains a panic: a panic never unwinds
//! into TSF, it poisons the document instead. A method whose answer reads no
//! state goes through [`com_method`], the boundary alone.
//!
//! What each method answers is decided in `text::windows::acp`, free of
//! `windows` types and checked on every host; this file moves values
//! between COM's pointers and it.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{DevicePixelRatio, DevicePoint};
use flui_platform::text_store::{
    Composition, OwnerCalls, TextChange, TextInputPurpose, TextStore, TextStoreError,
    TextStoreObserver, TextStoreStatus, Utf16Range,
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

use super::{TextServices, input_scope};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::windows::WindowRef;
use crate::text::windows::acp::{
    self, AcpChange, AcpSelection, Hresult, Session, SessionSlot, acp, store_error,
};
use crate::text::windows::geometry::{ScreenRect, range_rect_to_screen, screen_point_to_client};

// The codes and flags `acp` names are the ones `windows` defines.
const _: () = {
    assert!(Hresult::S_OK.value() == S_OK.0);
    assert!(Hresult::TS_S_ASYNC.value() == TS_S_ASYNC.0);
    assert!(Hresult::TS_E_INVALIDPOS.value() == TS_E_INVALIDPOS.0);
    assert!(Hresult::TS_E_NOLOCK.value() == TS_E_NOLOCK.0);
    assert!(Hresult::TS_E_NOLAYOUT.value() == TS_E_NOLAYOUT.0);
    assert!(Hresult::TS_E_INVALIDPOINT.value() == TS_E_INVALIDPOINT.0);
    assert!(Hresult::TS_E_SYNCHRONOUS.value() == TS_E_SYNCHRONOUS.0);
    assert!(Hresult::E_UNEXPECTED.value() == E_UNEXPECTED.0);
    assert!(Hresult::E_FAIL.value() == E_FAIL.0);
    assert!(Hresult::E_INVALIDARG.value() == E_INVALIDARG.0);
    assert!(acp::TS_LF_SYNC == TS_LF_SYNC);
    assert!(acp::TS_LF_READWRITE == TS_LF_READWRITE.0);
    assert!(acp::GXFPF_ROUND_NEAREST == GXFPF_ROUND_NEAREST);
};

impl From<Hresult> for HRESULT {
    fn from(code: Hresult) -> Self {
        Self(code.value())
    }
}

impl From<Hresult> for windows_core::Error {
    fn from(code: Hresult) -> Self {
        HRESULT::from(code).into()
    }
}

/// The state one TSF document shares between its COM object and the
/// window's [`TextServices`].
pub(super) struct DocumentState {
    pub(super) store: Rc<dyn TextStore>,
    /// Whether the store was protected when the document opened: the
    /// static status TSF read then, which no notification can change.
    protected: bool,
    /// The store's purpose when the document opened; static like
    /// `protected`, because TSF reads the input scope with the document.
    purpose: TextInputPurpose,
    /// Set when TSF asked for the input scope since the last retrieval.
    scope_requested: Cell<Option<input_scope::ScopeRequest>>,
    /// The window, reached only while its handle still names it.
    window: WindowRef,
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
        window: WindowRef,
        services: Weak<TextServices>,
    ) -> Rc<Self> {
        Rc::new(Self {
            protected: store.status().protected,
            purpose: store.status().purpose,
            scope_requested: Cell::new(None),
            store,
            window,
            services,
            sink: RefCell::new(None),
            slot: SessionSlot::default(),
            closed: Cell::new(false),
            poisoned: Cell::new(false),
            last_text_ext: Cell::new(None),
        })
    }

    /// The store's status as TSF last read it: the protection and purpose it
    /// opened with, the rest current.
    fn opened_status(&self) -> TextStoreStatus {
        self.store
            .status()
            .with_protected(self.protected)
            .with_purpose(self.purpose)
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

    /// The window's scale and client origin in physical screen pixels; both
    /// read only while the window's text services are serving, which they
    /// stop doing before the window is destroyed, else the default scale at
    /// (0, 0), which the caller reports as no better than that.
    fn screen_frame(&self) -> (DevicePixelRatio, DevicePoint) {
        let Some(services) = self
            .services
            .upgrade()
            .filter(|services| !services.is_shut_down())
        else {
            return (DevicePixelRatio::default(), DevicePoint::new(0, 0));
        };
        let mut origin = POINT::default();
        // SAFETY: the window is live while its services serve; `origin` is a
        // live, writable local, and `ClientToScreen` only writes through the
        // pointer it is given.
        let _ = unsafe { ClientToScreen(self.window.hwnd(), &raw mut origin) };
        (
            DevicePixelRatio::new(services.scale()).unwrap_or_default(),
            DevicePoint::new(origin.x, origin.y),
        )
    }

    /// Run one granted lock: open the session slot, hand TSF the lock and
    /// close the slot again (on unwind too); answers what `OnLockGranted`
    /// returned. A grant for a closed document never reaches TSF.
    fn run_grant(&self, session: Session<'_>, flags: u32) -> Hresult {
        if self.closed.get() {
            tracing::debug!(target: "flui_native::tsf", "grant for a closed document dropped");
            return Hresult::E_UNEXPECTED;
        }
        let Some(sink) = self.sink_for(u32::MAX) else {
            return Hresult::E_UNEXPECTED;
        };
        self.slot.with_open(session, || {
            tracing::debug!(target: "flui_native::tsf", flags, "OnLockGranted");
            // SAFETY: a plain COM call on a sink TSF advised; no pointers.
            let granted = unsafe { sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(flags)) };
            Hresult::new(granted.map_or_else(|error| error.code(), |()| S_OK).0)
        })
    }
}

/// What `GetStatus` tells TSF about a store whose status is `status`.
///
/// A protected field's text cannot be read out, so it is not advertised as
/// free of hidden text (`TS_SS_NOHIDDENTEXT`). It stays editable: no
/// `TS_SD_READONLY`, which would refuse the input method's edits. The flag
/// is static: TSF reads it when the document is pushed and never again
/// (`OnStatusChange` carries dynamic flags only), so a document answers
/// with the protection it opened with, and a change replaces the document
/// (`TextServices::reopen`).
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

/// `change` as TSF's `TS_TEXTCHANGE`.
fn ts_change(change: AcpChange) -> TS_TEXTCHANGE {
    TS_TEXTCHANGE {
        acpStart: change.start,
        acpOldEnd: change.old_end,
        acpNewEnd: change.new_end,
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
    Ok(acp::range(start, start.saturating_add(len))?)
}

pub(super) use com_object::{TsfStore, TsfStore_Impl};

/// The boundary of a vtable method whose answer reads no document state, so
/// a poisoned or closed document answers it the same: a panic answers
/// `E_UNEXPECTED`.
fn com_method<R>(
    method: &'static str,
    body: impl FnOnce() -> windows_core::Result<R>,
) -> windows_core::Result<R> {
    os_entry(
        method,
        (),
        AfterPanic::Answer(|()| Err(E_UNEXPECTED.into())),
        body,
    )
}

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
    /// Its state is an `Rc` of the window's thread, so it is not agile: COM
    /// marshals another apartment's calls back to that thread.
    #[implement(ITextStoreACP, ITfContextOwnerCompositionSink, Agile = false)]
    pub(in super::super) struct TsfStore {
        pub(super) state: Rc<DocumentState>,
    }
}

impl TsfStore {
    pub(super) fn new(state: Rc<DocumentState>) -> Self {
        Self { state }
    }

    /// The boundary of a vtable method that reads or edits the document; see
    /// the module doc. The method's own panic poisons the document; anything
    /// else that panics here answers `E_UNEXPECTED` as well, and the services'
    /// entry depth is restored by its guard as that panic unwinds.
    fn document_method<R>(
        &self,
        method: &'static str,
        body: impl FnOnce(&DocumentState) -> windows_core::Result<R>,
    ) -> windows_core::Result<R> {
        os_entry(
            method,
            (),
            AfterPanic::Answer(|()| Err(E_UNEXPECTED.into())),
            move || self.run_document_method(method, body),
        )
    }

    fn run_document_method<R>(
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
        let entered = services.as_deref().map(TextServices::enter);
        let result = match catch_unwind(AssertUnwindSafe(|| body(&state))) {
            Ok(result) => result,
            Err(payload) => {
                state.poisoned.set(true);
                // The log runs a user-installed subscriber: contained, so
                // nothing it raises unwinds into TSF (a failure it raises is
                // retained with the scope).
                let mut diagnostics = OwnerCalls::new();
                diagnostics.run(|| {
                    tracing::error!(
                        target: "flui_native::tsf",
                        method,
                        panic = crate::owner::panic_boundary::panic_payload_message(&*payload),
                        "a TSF call into the text store panicked; the document is poisoned"
                    );
                });
                drop(diagnostics);
                // A payload whose `Drop` panics must not unwind into TSF.
                std::mem::forget(payload);
                if let Some(services) = &services {
                    services.document_poisoned(&state);
                }
                Err(E_UNEXPECTED.into())
            }
        };
        if let (Some(services), Some(entered)) = (services.as_deref(), entered) {
            services.leave(entered);
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
        TsfStore::document_method(self, "AdviseSink", |state| {
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
            tracing::debug!(target: "flui_native::tsf", dwmask, "AdviseSink");
            Ok(())
        })
    }

    fn UnadviseSink(&self, punk: Ref<'_, IUnknown>) -> windows_core::Result<()> {
        TsfStore::document_method(self, "UnadviseSink", |state| {
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
        TsfStore::document_method(self, "RequestLock", |state| {
            let grant_state = Rc::clone(&self.state);
            let answer = acp::request_lock(&*state.store, dwlockflags, move |session, flags| {
                grant_state.run_grant(session, flags)
            });
            tracing::debug!(
                target: "flui_native::tsf",
                dwlockflags,
                ?answer,
                "RequestLock"
            );
            Ok(HRESULT::from(answer?))
        })
    }

    fn GetStatus(&self) -> windows_core::Result<TS_STATUS> {
        TsfStore::document_method(self, "GetStatus", |state| {
            Ok(ts_status(state.opened_status()))
        })
    }

    fn QueryInsert(
        &self,
        acpteststart: i32,
        acptestend: i32,
        _cch: u32,
        pacpresultstart: *mut i32,
        pacpresultend: *mut i32,
    ) -> windows_core::Result<()> {
        TsfStore::document_method(self, "QueryInsert", |_| {
            acp::range(acpteststart, acptestend)?;
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
        TsfStore::document_method(self, "GetSelection", |state| {
            if ulindex != 0 && ulindex != TS_DEFAULT_SELECTION {
                return Err(E_INVALIDARG.into());
            }
            if pselection.is_null() || ulcount == 0 {
                put(pcfetched, 0);
                return Ok(());
            }
            let selection = state
                .slot
                .read(|session| Ok::<_, Hresult>(acp::selection(session)))?;
            put(
                pselection,
                TS_SELECTION_ACP {
                    acpStart: selection.start,
                    acpEnd: selection.end,
                    style: TS_SELECTIONSTYLE {
                        ase: if selection.active_at_start {
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
        TsfStore::document_method(self, "SetSelection", |state| {
            if ulcount != 1 || pselection.is_null() {
                return Err(E_INVALIDARG.into());
            }
            // SAFETY: non-null was checked; TSF passes one selection.
            let wanted = unsafe { *pselection };
            let wanted = AcpSelection {
                start: wanted.acpStart,
                end: wanted.acpEnd,
                active_at_start: wanted.style.ase == TS_AE_START,
            };
            acp::offset(wanted.start)?;
            acp::offset(wanted.end)?;
            Ok(state
                .slot
                .write(|session| acp::set_selection(session, wanted))?)
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
        TsfStore::document_method(self, "GetText", |state| {
            // Only what the caller's buffer can hold is read and encoded.
            let room = if pchplain.is_null() { 0 } else { cchplainreq };
            let units = state
                .slot
                .read(|session| acp::text(session, acpstart, acpend, room))?;
            let copied = units.len().min(usize::try_from(room).unwrap_or(usize::MAX));
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
        TsfStore::document_method(self, "SetText", |state| {
            let text = wide_text(pchtext, cch)?;
            acp::range(acpstart, acpend)?;
            tracing::debug!(target: "flui_native::tsf", acpstart, acpend, text, "SetText");
            let change = state
                .slot
                .write(|session| acp::replace(session, acpstart, acpend, &text))?;
            Ok(ts_change(change))
        })
    }

    fn GetFormattedText(&self, _acpstart: i32, _acpend: i32) -> windows_core::Result<IDataObject> {
        com_method("GetFormattedText", || Err(E_NOTIMPL.into()))
    }

    fn GetEmbedded(
        &self,
        _acppos: i32,
        _rguidservice: *const GUID,
        _riid: *const GUID,
    ) -> windows_core::Result<IUnknown> {
        com_method("GetEmbedded", || Err(E_NOTIMPL.into()))
    }

    fn QueryInsertEmbedded(
        &self,
        _pguidservice: *const GUID,
        _pformatetc: *const FORMATETC,
    ) -> windows_core::Result<BOOL> {
        com_method("QueryInsertEmbedded", || Ok(false.into()))
    }

    fn InsertEmbedded(
        &self,
        _dwflags: u32,
        _acpstart: i32,
        _acpend: i32,
        _pdataobject: Ref<'_, IDataObject>,
    ) -> windows_core::Result<TS_TEXTCHANGE> {
        com_method("InsertEmbedded", || Err(E_NOTIMPL.into()))
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
        TsfStore::document_method(self, "InsertTextAtSelection", |state| {
            if dwflags & TS_IAS_QUERYONLY != 0 {
                let selection = state
                    .slot
                    .read(|session| Ok::<_, Hresult>(acp::selection(session)))?;
                put(pacpstart, selection.start);
                put(pacpend, selection.end);
                return Ok(());
            }
            let text = wide_text(pchtext, cch)?;
            tracing::debug!(target: "flui_native::tsf", text, "InsertTextAtSelection");
            let change = state.slot.write(|session| {
                session
                    .insert_at_selection(&text)
                    .map(acp::text_change)
                    .map_err(store_error)
            })?;
            put(pacpstart, change.start);
            put(pacpend, change.new_end);
            put(pchange, ts_change(change));
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
        com_method("InsertEmbeddedAtSelection", || Err(E_NOTIMPL.into()))
    }

    fn RequestSupportedAttrs(
        &self,
        dwflags: u32,
        cfilterattrs: u32,
        pafilterattrs: *const GUID,
    ) -> windows_core::Result<()> {
        TsfStore::document_method(self, "RequestSupportedAttrs", |state| {
            // SAFETY: TSF passes `cfilterattrs` GUIDs at `pafilterattrs`.
            let request = unsafe {
                input_scope::ScopeRequest::from_request(dwflags, cfilterattrs, pafilterattrs)
            };
            state.scope_requested.set(request);
            Ok(())
        })
    }

    fn RequestAttrsAtPosition(
        &self,
        _acppos: i32,
        cfilterattrs: u32,
        pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> windows_core::Result<()> {
        // The scope is one for the whole document, so it holds at any position.
        TsfStore::document_method(self, "RequestAttrsAtPosition", |state| {
            // SAFETY: TSF passes `cfilterattrs` GUIDs at `pafilterattrs`.
            let request =
                unsafe { input_scope::ScopeRequest::at_position(cfilterattrs, pafilterattrs) };
            state.scope_requested.set(request);
            Ok(())
        })
    }

    fn RequestAttrsTransitioningAtPosition(
        &self,
        _acppos: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> windows_core::Result<()> {
        com_method("RequestAttrsTransitioningAtPosition", || Ok(()))
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
        com_method("FindNextAttrTransition", || {
            put(pacpnext, acphalt);
            put(pffound, false.into());
            put(plfoundoffset, 0);
            Ok(())
        })
    }

    fn RetrieveRequestedAttrs(
        &self,
        ulcount: u32,
        paattrvals: *mut TS_ATTRVAL,
        pcfetched: *mut u32,
    ) -> windows_core::Result<()> {
        TsfStore::document_method(self, "RetrieveRequestedAttrs", |state| {
            let scope = input_scope::scope_for(state.opened_status());
            let requested = state.scope_requested.take();
            match (scope, requested) {
                (Some(scope), Some(request)) if ulcount > 0 && !paattrvals.is_null() => {
                    // SAFETY: TSF passes room for `ulcount` (at least one)
                    // values, uninitialised, so the write must not drop.
                    unsafe { paattrvals.write(input_scope::scope_attribute(scope, request)) };
                    put(pcfetched, 1);
                }
                _ => put(pcfetched, 0),
            }
            Ok(())
        })
    }

    fn GetEndACP(&self) -> windows_core::Result<i32> {
        TsfStore::document_method(self, "GetEndACP", |state| {
            Ok(state
                .slot
                .read(|session| Ok::<_, Hresult>(acp(session.document_len())))?)
        })
    }

    fn GetActiveView(&self) -> windows_core::Result<u32> {
        com_method("GetActiveView", || Ok(0))
    }

    fn GetACPFromPoint(
        &self,
        _vcview: u32,
        ptscreen: *const POINT,
        dwflags: u32,
    ) -> windows_core::Result<i32> {
        TsfStore::document_method(self, "GetACPFromPoint", |state| {
            if ptscreen.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: non-null was checked; TSF passes one point.
            let screen = unsafe { *ptscreen };
            let (scale, origin) = state.screen_frame();
            let logical =
                screen_point_to_client(DevicePoint::new(screen.x, screen.y), origin, scale)
                    .ok_or_else(|| store_error(TextStoreError::PointOutside))?;
            let mode = acp::point_mode(dwflags);
            Ok(state.slot.read(|session| {
                session
                    .index_at_point(logical, mode)
                    .map(acp)
                    .map_err(store_error)
            })?)
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
        TsfStore::document_method(self, "GetTextExt", |state| {
            let span = acp::range(acpstart, acpend)?;
            let rect = state
                .slot
                .read(|session| session.rect_for_range(span).map_err(store_error));
            let rect = match rect {
                Ok(rect) => rect,
                Err(error) => {
                    tracing::debug!(
                        target: "flui_native::tsf",
                        acpstart,
                        acpend,
                        ?error,
                        "GetTextExt"
                    );
                    return Err(error.into());
                }
            };
            let (scale, origin) = state.screen_frame();
            let screen = range_rect_to_screen(rect.bounds, origin, scale).map_err(|error| {
                tracing::debug!(target: "flui_native::tsf", %error, "GetTextExt has no screen rect");
                windows_core::Error::from(TS_E_NOLAYOUT)
            })?;
            state.last_text_ext.set(Some(screen));
            tracing::debug!(target: "flui_native::tsf", acpstart, acpend, ?screen, "GetTextExt");
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
        TsfStore::document_method(self, "GetScreenExt", |state| {
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
            unsafe { GetClientRect(state.window.hwnd(), &raw mut client) }?;
            Ok(RECT {
                left: client.left + origin.x,
                top: client.top + origin.y,
                right: client.right + origin.x,
                bottom: client.bottom + origin.y,
            })
        })
    }

    fn GetWnd(&self, _vcview: u32) -> windows_core::Result<HWND> {
        TsfStore::document_method(self, "GetWnd", |state| Ok(state.window.hwnd()))
    }
}

impl ITfContextOwnerCompositionSink_Impl for TsfStore_Impl {
    fn OnStartComposition(
        &self,
        pcomposition: Ref<'_, ITfCompositionView>,
    ) -> windows_core::Result<BOOL> {
        TsfStore::document_method(self, "OnStartComposition", |state| {
            // SAFETY: a plain COM call on the view TSF passed.
            let range = unsafe { pcomposition.ok()?.GetRange() }?;
            let span = range_extent(&range)?;
            tracing::debug!(target: "flui_native::tsf", ?span, "OnStartComposition");
            set_composition(state, Some(span))?;
            Ok(true.into())
        })
    }

    fn OnUpdateComposition(
        &self,
        pcomposition: Ref<'_, ITfCompositionView>,
        prangenew: Ref<'_, ITfRange>,
    ) -> windows_core::Result<()> {
        TsfStore::document_method(self, "OnUpdateComposition", |state| {
            let range = match prangenew.as_ref() {
                Some(range) => range.clone(),
                // SAFETY: a plain COM call on the view TSF passed.
                None => unsafe { pcomposition.ok()?.GetRange() }?,
            };
            let span = range_extent(&range)?;
            tracing::debug!(target: "flui_native::tsf", ?span, "OnUpdateComposition");
            set_composition(state, Some(span))
        })
    }

    fn OnEndComposition(
        &self,
        _pcomposition: Ref<'_, ITfCompositionView>,
    ) -> windows_core::Result<()> {
        TsfStore::document_method(self, "OnEndComposition", |state| {
            tracing::debug!(target: "flui_native::tsf", "OnEndComposition");
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
    Ok(state
        .slot
        .write(|session| session.set_composition(composition).map_err(store_error))
        .inspect_err(|error| {
            tracing::warn!(
                target: "flui_native::tsf",
                ?error,
                "composition change outside a read-write session"
            );
        })?)
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
    Ok(acp::wide_text(units)?)
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
        tracing::debug!(target: "flui_native::tsf", what, "sink notification");
        if let Err(error) = call(&sink) {
            tracing::debug!(target: "flui_native::tsf", what, ?error, "sink notification failed");
        }
    }
}

impl TextStoreObserver for SinkObserver {
    fn text_changed(&self, change: TextChange) {
        let change = ts_change(acp::text_change(change));
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
        // A protection change is a static one, which TSF reads only from a
        // new document.
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let now = state.store.status();
        if state.closed.get() || (now.protected == state.protected && now.purpose == state.purpose)
        {
            return;
        }
        if let Some(services) = state.services.upgrade() {
            services.reopen(&state);
        }
    }
}
