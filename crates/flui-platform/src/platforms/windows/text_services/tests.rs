//! The text services as a `TextStoreHost`, against the real TSF in a hidden
//! window, with no input method typing: which store a completion reaches,
//! and that one queued behind a TSF call is never lost.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::geometry::Size;
use flui_platform_api::ImeEvent;
use flui_platform_api::text_store::{
    CommitGate, CompositionEnd, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, OwnerCalls,
    TextStore, TextStoreError, TextStoreHost, TextStoreHostError, TextStoreObserver,
    TextStoreStatus, project_ime_event,
};
use windows::Win32::Foundation::{E_UNEXPECTED, HWND};
use windows::Win32::UI::TextServices::{
    ITextStoreACP, ITextStoreACP_Impl, ITfComposition, ITfContext, ITfDocumentMgr, ITfEditSession,
    TF_ES_READWRITE, TF_ES_SYNC, TS_LF_READ, TS_LF_SYNC, TS_SS_NOHIDDENTEXT,
};
use windows_core::Interface as _;

use super::document::{TsfStore, TsfStore_Impl, ts_status};
use super::{Serving, TextServices, same_object};
use crate::traits::{Platform, WindowOptions};

/// "ab" with "かな" composing after it, as the store sees it.
fn composing_store() -> Rc<InMemoryTextStore> {
    let store = InMemoryTextStore::new("ab");
    store.set_commit_gate(CommitGate::new());
    let applied = project_ime_event(
        &*store,
        &ImeEvent::Preedit {
            text: "かな".to_owned(),
            cursor: Some((0, 0)),
        },
    );
    assert_eq!(applied, Ok(LockOutcome::Granted), "preedit applies");
    store
}

fn erased(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    store.clone()
}

/// A completion asked for inside a TSF call is queued with its store and
/// answered `Deferred`; when TSF has shut down by the time the call
/// returns, the composition is committed in place.
fn a_queued_completion_commits_in_place_after_a_shutdown(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.shutdown();
    services.leave();
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
}

/// The same when the store's document is gone by the time the call
/// returns (it was poisoned, or reopening it failed).
fn a_queued_completion_commits_in_place_without_a_document(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.close_document();
    services.leave();
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
    services.shutdown();
}

/// A store the host does not serve, queued focus changes included, is
/// refused; so is any store once TSF has shut down.
fn a_completion_reaches_only_the_focused_store(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let (a, b) = (composing_store(), composing_store());
    services.focus_store(Some(erased(&a)));
    assert_eq!(
        services.complete_composition(&erased(&b)),
        Err(TextStoreHostError::NotFocused)
    );
    services.enter();
    services.focus_store(Some(erased(&b)));
    assert_eq!(
        services.complete_composition(&erased(&a)),
        Err(TextStoreHostError::NotFocused),
        "the queued focus change counts"
    );
    services.leave();
    assert!(b.composition().is_some() && a.composition().is_some());
    services.shutdown();
    assert_eq!(
        services.complete_composition(&erased(&b)),
        Err(TextStoreHostError::Unavailable)
    );
}

/// A panic payload that records whether it was destroyed.
struct RecordsDrop(Arc<AtomicBool>);

impl Drop for RecordsDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// An in-memory store whose observer retirement (`set_observer(None)`) or
/// lock request panics once, when told: application code a document's
/// teardown and a TSF call reach. It can also refuse synchronous locks, and
/// run a hook when TSF reads its status.
struct FailingStore {
    inner: Rc<InMemoryTextStore>,
    fail_retirement: Cell<bool>,
    /// Set: the retirement panics with a [`RecordsDrop`] payload instead.
    retirement_payload: RefCell<Option<Arc<AtomicBool>>>,
    fail_lock: Cell<bool>,
    refuse_sync: Cell<bool>,
    on_status: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl FailingStore {
    fn new(text: &str) -> Rc<Self> {
        let inner = InMemoryTextStore::new(text);
        inner.set_commit_gate(CommitGate::new());
        Rc::new(Self {
            inner,
            fail_retirement: Cell::new(false),
            retirement_payload: RefCell::new(None),
            fail_lock: Cell::new(false),
            refuse_sync: Cell::new(false),
            on_status: RefCell::new(None),
        })
    }
}

impl TextStore for FailingStore {
    fn status(&self) -> TextStoreStatus {
        let hook = self.on_status.borrow_mut().take();
        if let Some(hook) = hook {
            hook();
        }
        self.inner.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        assert!(!self.fail_lock.take(), "lock request failure");
        if timing == LockTiming::Sync && self.refuse_sync.get() {
            return Err(TextStoreError::SyncLockUnavailable);
        }
        self.inner.request_lock(grant, timing)
    }
    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }
    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        let retiring = observer.is_none();
        self.inner.set_observer(observer);
        if retiring && let Some(dropped) = self.retirement_payload.borrow_mut().take() {
            std::panic::panic_any(RecordsDrop(dropped));
        }
        assert!(
            !(retiring && self.fail_retirement.take()),
            "observer retirement failure"
        );
    }
}

/// A `tracing` subscriber, application code, that panics on every error
/// the text services log.
struct PanicsOnTsfErrors;

impl tracing::Subscriber for PanicsOnTsfErrors {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        assert!(
            *metadata.level() != tracing::Level::ERROR || metadata.target() != "flui_platform::tsf",
            "diagnostic failure"
        );
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A field losing focus inside a TSF call (queued, applied when the call
/// returns) whose observer retirement panics, under a subscriber that panics
/// on the error the teardown logs: nothing unwinds out of the COM entry, the
/// teardown's payload is retained rather than destroyed, and TSF is back on
/// the empty document.
fn a_queued_unfocus_whose_teardown_and_diagnostic_panic_stays_in_the_com_entry(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = FailingStore::new("ab");
    services.focus_store(Some(store.clone()));
    let dropped = Arc::new(AtomicBool::new(false));
    *store.retirement_payload.borrow_mut() = Some(Arc::clone(&dropped));
    let unfocus = Rc::clone(&services);
    *store.on_status.borrow_mut() = Some(Box::new(move || unfocus.focus_store(None)));
    let tsf_store = document_store(&services);
    let document: &TsfStore_Impl = tsf_store
        .cast_object_ref::<TsfStore>()
        .expect("the document's own object");
    let answer = catch_unwind(AssertUnwindSafe(|| {
        tracing::subscriber::with_default(PanicsOnTsfErrors, || document.GetStatus())
    }));
    assert!(answer.is_ok(), "no panic leaves the COM entry");
    assert!(
        !dropped.load(Ordering::SeqCst),
        "the teardown's payload is retained, not destroyed"
    );
    assert!(
        matches!(*services.serving.borrow(), Serving::Nothing),
        "the queued unfocus ran"
    );
    assert!(
        associates_the_empty_document(&services),
        "TSF is associated with the empty document again"
    );
    services.shutdown();
}

use edit_session::StartsComposition;

/// The `#[implement]` expansion, in a module of its own so the lints its
/// generated code trips are expected here and nowhere else.
#[expect(
    clippy::ref_as_ptr,
    clippy::inline_always,
    raw_borrows_via_references,
    reason = "code generated by windows-core's #[implement]"
)]
mod edit_session {
    use std::cell::RefCell;

    use windows::Win32::UI::TextServices::{
        ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
        ITfContextComposition, ITfEditSession, ITfEditSession_Impl,
    };
    use windows_core::Ref;
    use windows_core::{Interface as _, implement};

    /// A TSF edit session that starts a composition over the last `length`
    /// characters of its context, as a text service does when it composes.
    #[implement(ITfEditSession)]
    pub(in super::super) struct StartsComposition {
        pub(in super::super) context: ITfContext,
        pub(in super::super) length: i32,
        pub(in super::super) started: RefCell<Option<ITfComposition>>,
    }

    /// Told when TSF ends the composition; nothing to do.
    #[implement(ITfCompositionSink)]
    struct IgnoresTermination;

    impl ITfCompositionSink_Impl for IgnoresTermination_Impl {
        fn OnCompositionTerminated(
            &self,
            _: u32,
            _: Ref<'_, ITfComposition>,
        ) -> windows_core::Result<()> {
            Ok(())
        }
    }

    impl ITfEditSession_Impl for StartsComposition_Impl {
        fn DoEditSession(&self, cookie: u32) -> windows_core::Result<()> {
            let compositions: ITfContextComposition = self.context.cast()?;
            // SAFETY: plain COM calls inside the edit session TSF granted.
            let started = unsafe {
                let range = self.context.GetEnd(cookie)?;
                let mut moved = 0;
                range.ShiftStart(cookie, -self.length, &raw mut moved, std::ptr::null())?;
                let sink: ITfCompositionSink = IgnoresTermination.into();
                compositions.StartComposition(cookie, &range, &sink)?
            };
            *self.started.borrow_mut() = Some(started);
            Ok(())
        }
    }
}

/// Start a TSF composition over the last `length` characters of the focused
/// document, under the window's own client id, so ending it needs a lock on
/// the store.
fn start_tsf_composition(services: &TextServices, length: i32) -> ITfComposition {
    let context = match &*services.serving.borrow() {
        Serving::Field(document) => document.context.clone(),
        Serving::Nothing | Serving::Shutdown => panic!("no document is open"),
    };
    let session = StartsComposition {
        context: context.clone(),
        length,
        started: RefCell::new(None),
    };
    let started = windows_core::ComObject::new(session);
    let as_session: ITfEditSession = started.to_interface();
    // SAFETY: a plain COM call on the window's owner thread.
    let answer = unsafe {
        context.RequestEditSession(
            services.client_id,
            &as_session,
            TF_ES_SYNC | TF_ES_READWRITE,
        )
    };
    assert_eq!(
        answer.map(|session| session.0),
        Ok(0),
        "TSF ran the edit session"
    );
    started
        .started
        .borrow_mut()
        .take()
        .expect("the edit session started a composition")
}

/// A completion TSF refuses replaces the document; that teardown's observer
/// retirement panics, and the in-place commit that recovers the composition
/// then settles owner code that panics and parks in the owner's gate. The
/// teardown came first, so the owner's containment reports it, not the
/// recovery's failure.
fn a_refused_completion_reports_its_teardown_before_its_recovery(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = FailingStore::new("ab");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    assert_eq!(
        project_ime_event(
            &*store.inner,
            &ImeEvent::Preedit {
                text: "かな".to_owned(),
                cursor: Some((0, 0)),
            },
        ),
        Ok(LockOutcome::Granted),
        "preedit applies"
    );
    let erased: Rc<dyn TextStore> = store.clone();
    services.focus_store(Some(Rc::clone(&erased)));
    // TSF composes "かな", the store's composition.
    let _composition = start_tsf_composition(&services, 2);
    store.refuse_sync.set(true);
    store.fail_retirement.set(true);
    store
        .inner
        .set_owner_listener(Some(Rc::new(|| panic!("recovery settle failure"))));
    let mut calls = OwnerCalls::new();
    let answer = calls.run_parking(&gate, || services.complete_composition(&erased));
    store.inner.set_owner_listener(None);
    store.refuse_sync.set(false);
    assert_eq!(answer, None, "the completion raised a failure");
    let first = calls.into_failure().map(|payload| panic_text(&*payload));
    assert_eq!(
        first.as_deref(),
        Some("observer retirement failure"),
        "the teardown's failure is the first"
    );
    assert_eq!(store.inner.composition(), None, "committed in place");
    assert_eq!(store.inner.text(), "abかな", "keeping the text");
    assert!(
        gate.take_failure().is_none(),
        "nothing is left for a later turn"
    );
    services.shutdown();
}

/// The document manager TSF associates with the window, read by swapping
/// the empty one in and the found one back.
fn associated_manager(services: &TextServices) -> ITfDocumentMgr {
    // SAFETY: plain COM calls on the window's owner thread.
    unsafe {
        let manager = services
            .thread_manager
            .AssociateFocus(services.hwnd, Some(&services.empty))
            .expect("a manager is associated");
        let _ = services
            .thread_manager
            .AssociateFocus(services.hwnd, Some(&manager));
        manager
    }
}

/// The context on top of the manager TSF associates with the window.
fn associated_context(services: &TextServices) -> ITfContext {
    // SAFETY: a plain COM call on the window's owner thread.
    unsafe { associated_manager(services).GetTop() }.expect("a context is pushed")
}

/// Whether TSF reports `context`'s document as free of hidden text.
fn tsf_sees_no_hidden_text(context: &ITfContext) -> bool {
    // SAFETY: a plain COM call on the window's owner thread.
    let status = unsafe { context.GetStatus() }.expect("status");
    status.dwStaticFlags & TS_SS_NOHIDDENTEXT != 0
}

/// TSF reads static status flags (`TS_SS_NOHIDDENTEXT`) once per document:
/// `OnStatusChange` carries only dynamic ones. A protection change on the
/// focused field reaches TSF as a new context, whose status TSF reports.
fn a_protection_change_reaches_tsf_as_a_new_context(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = InMemoryTextStore::new("secret");
    store.set_commit_gate(CommitGate::new());
    services.focus_store(Some(erased(&store)));
    let plain = associated_context(&services);
    assert!(tsf_sees_no_hidden_text(&plain), "a plain field");
    store.set_protected(true);
    let hidden = associated_context(&services);
    assert!(!same_object(&plain, &hidden), "a new context is pushed");
    assert!(!tsf_sees_no_hidden_text(&hidden), "a protected field");
    store.set_protected(false);
    let shown = associated_context(&services);
    assert!(!same_object(&hidden, &shown), "and again when it is lifted");
    assert!(tsf_sees_no_hidden_text(&shown), "a plain field again");
    services.shutdown();
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    crate::shared::panic_boundary::panic_payload_message(payload).to_owned()
}

/// The `ITextStoreACP` TSF holds for the focused document.
fn document_store(services: &TextServices) -> ITextStoreACP {
    match &*services.serving.borrow() {
        Serving::Field(document) => document.tsf_store.clone(),
        Serving::Nothing | Serving::Shutdown => panic!("no document is open"),
    }
}

/// Whether TSF associates the window with the empty document manager.
fn associates_the_empty_document(services: &TextServices) -> bool {
    // SAFETY: plain COM calls on the window's owner thread. Associating
    // the empty manager again changes nothing when it is already associated.
    let previous = unsafe {
        services
            .thread_manager
            .AssociateFocus(services.hwnd, Some(&services.empty))
    };
    previous.is_ok_and(|previous| same_object(&previous, &services.empty))
}

/// A field losing focus whose observer retirement panics: the document is
/// still taken away from TSF, and the host operation raises the failure
/// once that is done.
fn an_unfocus_whose_observer_retirement_panics_releases_the_document(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = FailingStore::new("ab");
    services.focus_store(Some(store.clone()));
    store.fail_retirement.set(true);
    let raised = catch_unwind(AssertUnwindSafe(|| services.focus_store(None)))
        .err()
        .map(|payload| panic_text(&*payload));
    assert_eq!(
        raised.as_deref(),
        Some("observer retirement failure"),
        "the host operation raises the failure"
    );
    assert!(
        associates_the_empty_document(&services),
        "TSF is associated with the empty document again"
    );
    services.focus_store(Some(store));
    assert!(
        matches!(*services.serving.borrow(), Serving::Field(_)),
        "the next focus opens a document"
    );
    services.shutdown();
}

/// A TSF call that poisons its document, whose observer retirement then
/// panics while the poisoned document is dropped as the call returns: no
/// panic leaves the COM entry, and the document is still released.
fn a_poisoned_document_whose_observer_retirement_panics_returns_to_tsf(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = FailingStore::new("ab");
    services.focus_store(Some(store.clone()));
    let tsf_store = document_store(&services);
    let document: &TsfStore_Impl = tsf_store
        .cast_object_ref::<TsfStore>()
        .expect("the document's own object");
    store.fail_lock.set(true);
    store.fail_retirement.set(true);
    let answer = catch_unwind(AssertUnwindSafe(|| {
        document.RequestLock(TS_LF_READ.0 | TS_LF_SYNC)
    }));
    let answer = answer.expect("no panic leaves the COM entry");
    assert_eq!(answer.map_err(|error| error.code()), Err(E_UNEXPECTED));
    assert!(
        matches!(*services.serving.borrow(), Serving::Nothing),
        "the poisoned document was dropped"
    );
    assert!(
        associates_the_empty_document(&services),
        "TSF is associated with the empty document again"
    );
    let next = catch_unwind(AssertUnwindSafe(|| {
        services.focus_store(Some(erased(&composing_store())));
    }));
    assert!(next.is_ok(), "the failure is not raised a second time");
    services.shutdown();
}

/// What `GetStatus` answers for each store status: hidden text exactly when
/// the field is protected, and an editable document either way.
#[test]
fn the_document_status_follows_the_store_status() {
    let rows = [
        (TextStoreStatus::EDITABLE_SINGLE_LINE, TS_SS_NOHIDDENTEXT),
        (
            TextStoreStatus::EDITABLE_SINGLE_LINE.with_protected(true),
            0,
        ),
    ];
    for (status, static_flags) in rows {
        let answer = ts_status(status);
        assert_eq!(answer.dwStaticFlags, static_flags, "{status:?}");
        assert_eq!(answer.dwDynamicFlags, 0, "{status:?} is editable");
    }
}

/// One row: its name, and the case run against the shared window.
type Row = (&'static str, fn(HWND));

#[test]
fn the_text_services_answer_a_completion_for_its_store() {
    let platform = super::super::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(WindowOptions {
            title: "flui text services".into(),
            size: Size::new(200.0, 80.0),
            visible: false,
            ..Default::default()
        })
        .expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<super::super::WindowsWindow>()
        .expect("Win32 window")
        .hwnd();
    let cases: &[Row] = &[
        (
            "shut down",
            a_queued_completion_commits_in_place_after_a_shutdown,
        ),
        (
            "no document",
            a_queued_completion_commits_in_place_without_a_document,
        ),
        (
            "focused store only",
            a_completion_reaches_only_the_focused_store,
        ),
        (
            "unfocus with a panicking observer retirement",
            an_unfocus_whose_observer_retirement_panics_releases_the_document,
        ),
        (
            "poisoned document with a panicking observer retirement",
            a_poisoned_document_whose_observer_retirement_panics_returns_to_tsf,
        ),
        (
            "queued unfocus with a panicking teardown and diagnostic",
            a_queued_unfocus_whose_teardown_and_diagnostic_panic_stays_in_the_com_entry,
        ),
        (
            "refused completion with a panicking teardown and recovery",
            a_refused_completion_reports_its_teardown_before_its_recovery,
        ),
        (
            "protection change",
            a_protection_change_reaches_tsf_as_a_new_context,
        ),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if catch_unwind(AssertUnwindSafe(|| case(hwnd))).is_err() {
            failed.push(name);
        }
    }
    window.close();
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}
