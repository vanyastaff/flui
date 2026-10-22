//! The text services as a `TextStoreHost`, against the real TSF in a hidden
//! window, with no input method typing: which store a completion reaches,
//! and that one queued behind a TSF call is never lost.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::geometry::Size;
use flui_platform::ImeEvent;
use flui_platform::text_store::{
    CommitGate, CompositionEnd, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, OwnerCalls,
    TextStore, TextStoreError, TextStoreHost, TextStoreHostError, TextStoreObserver,
    TextStoreStatus, project_ime_event,
};
use windows::Win32::Foundation::{E_UNEXPECTED, HWND};
use windows::Win32::UI::TextServices::{
    ITextStoreACP, ITextStoreACP_Impl, ITfComposition, ITfContext, ITfDocumentMgr, ITfEditSession,
    TF_ES_READWRITE, TF_ES_SYNC, TF_POPF_ALL, TS_LF_READ, TS_LF_SYNC, TS_SS_NOHIDDENTEXT,
};
use windows_core::Interface as _;

use super::document::{TsfStore, TsfStore_Impl, ts_status};
use super::{Serving, TextServices, same_object};
use crate::platform::Platform;
use flui_platform::WindowOptions;

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

/// The case window `hwnd` with the identity of its context.
fn window_ref(hwnd: HWND) -> crate::owner::windows::WindowRef {
    let identity = crate::window::windows::proc::with_window_context(hwnd, "test", |c| c.identity)
        .expect("the case window's context");
    crate::owner::windows::WindowRef::new(hwnd, identity)
}

/// An entry into this thread's STA (nested in the platform's) for services
/// activated directly.
fn apartment() -> Rc<crate::owner::windows::com_apartment::ComApartment> {
    crate::owner::windows::com_apartment::ComApartment::enter().expect("COM apartment")
}

fn erased(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    store.clone()
}

/// A completion asked for inside a TSF call is queued with its store and
/// answered `Deferred`; when TSF has shut down by the time the call
/// returns, the composition is committed in place.
fn a_queued_completion_commits_in_place_after_a_shutdown(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    let entered = services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.shutdown();
    services.leave(entered);
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
}

/// The same when the store's document is gone by the time the call
/// returns (it was poisoned, or reopening it failed).
fn a_queued_completion_commits_in_place_without_a_document(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    let entered = services.enter();
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.close_document();
    services.leave(entered);
    assert_eq!(store.composition(), None, "committed in place");
    assert_eq!(store.text(), "abかな", "keeping the text");
    services.shutdown();
}

/// A window destroyed from inside a TSF call into a store (application code
/// closing it from a grant) does not release the document under that call:
/// the shutdown waits for the call to return, and a completion asked for
/// meanwhile still commits its store in place.
fn a_window_destroyed_inside_a_tsf_call_shuts_down_when_it_returns(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    let entered = services.enter();
    TextServices::retire_with_window(Rc::clone(&services));
    assert!(
        matches!(*services.serving.borrow(), Serving::Field(_)),
        "the document outlives the call it is under"
    );
    assert_eq!(
        services.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Deferred)
    );
    services.leave(entered);
    assert!(services.is_shut_down(), "shut down once the call returned");
    assert_eq!(
        store.composition(),
        None,
        "the completion committed in place"
    );
    assert_eq!(store.text(), "abかな");
}

/// A store the host does not serve, queued focus changes included, is
/// refused; so is any store once TSF has shut down.
fn a_completion_reaches_only_the_focused_store(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let (a, b) = (composing_store(), composing_store());
    services.focus_store(Some(erased(&a)));
    assert_eq!(
        services.complete_composition(&erased(&b)),
        Err(TextStoreHostError::NotFocused)
    );
    let entered = services.enter();
    services.focus_store(Some(erased(&b)));
    assert_eq!(
        services.complete_composition(&erased(&a)),
        Err(TextStoreHostError::NotFocused),
        "the queued focus change counts"
    );
    services.leave(entered);
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
            *metadata.level() != tracing::Level::ERROR || metadata.target() != "flui_native::tsf",
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
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
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
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
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
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
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

/// A protection change on a field TSF composes in: the composition is ended
/// in the document that owns it before the document is replaced, so the
/// store keeps no composing range that no context owns, and keeps its text.
fn a_protection_change_ends_the_composition_first(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = composing_store();
    services.focus_store(Some(erased(&store)));
    let _composition = start_tsf_composition(&services, 2);
    assert!(store.composition().is_some(), "the field composes");
    store.set_protected(true);
    assert_eq!(store.composition(), None, "the composition was ended");
    assert_eq!(store.text(), "abかな", "keeping the text");
    assert!(
        !tsf_sees_no_hidden_text(&associated_context(&services)),
        "the new document is protected"
    );
    services.shutdown();
}

/// The same when TSF refuses to end the composition (the store refuses its
/// synchronous lock): the composition is committed in place instead.
fn a_protection_change_whose_termination_is_refused_commits_in_place(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = FailingStore::new("ab");
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
    services.focus_store(Some(store.clone()));
    let _composition = start_tsf_composition(&services, 2);
    store.refuse_sync.set(true);
    store.inner.set_protected(true);
    store.refuse_sync.set(false);
    assert_eq!(store.inner.composition(), None, "committed in place");
    assert_eq!(store.inner.text(), "abかな", "keeping the text");
    assert!(
        !tsf_sees_no_hidden_text(&associated_context(&services)),
        "the new document is protected"
    );
    services.shutdown();
}

/// A `tracing` subscriber, application code, that panics on the one
/// diagnostic whose message is `.0`.
struct PanicsOnDiagnostic(&'static str);

impl tracing::Subscriber for PanicsOnDiagnostic {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(Option<String>);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = Some(format!("{value:?}"));
                }
            }
        }
        let mut message = Message(None);
        event.record(&mut message);
        assert!(message.0.as_deref() != Some(self.0), "diagnostic failure");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A field losing focus whose observer retirement panics, and whose
/// document's `Pop` then fails under a subscriber that panics on that
/// diagnostic: the retirement came first, so the host operation raises it,
/// and TSF is still back on the empty document.
fn a_teardown_whose_pop_diagnostic_panics_raises_the_first_failure(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = FailingStore::new("ab");
    services.focus_store(Some(store.clone()));
    let manager = match &*services.serving.borrow() {
        Serving::Field(document) => document.manager.clone(),
        Serving::Nothing | Serving::Shutdown => panic!("no document is open"),
    };
    // SAFETY: a plain COM call on the window's owner thread. The stack is
    // emptied here, so the teardown's own `Pop` fails.
    unsafe { manager.Pop(TF_POPF_ALL) }.expect("the context pops");
    store.fail_retirement.set(true);
    let raised = catch_unwind(AssertUnwindSafe(|| {
        tracing::subscriber::with_default(PanicsOnDiagnostic("Pop failed"), || {
            services.focus_store(None);
        });
    }))
    .err()
    .map(|payload| panic_text(&*payload));
    assert_eq!(
        raised.as_deref(),
        Some("observer retirement failure"),
        "the teardown's first failure, not the diagnostic's"
    );
    assert!(
        associates_the_empty_document(&services),
        "TSF is associated with the empty document again"
    );
    services.shutdown();
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    crate::owner::panic_boundary::panic_payload_message(payload).to_owned()
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
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
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
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
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

/// A TSF call whose failure escapes its own containment (the poisoning cannot
/// queue the document's release while the queue is borrowed) is answered by
/// the entry's boundary, and the entry depth it raised is restored as that
/// panic unwinds: a host operation afterwards runs at once instead of being
/// queued behind a call that will never leave. Red-check: pair `enter` and
/// `leave` by hand again.
fn a_tsf_call_failing_past_its_poisoning_leaves_no_depth(hwnd: HWND) {
    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    let store = FailingStore::new("ab");
    services.focus_store(Some(store.clone()));
    let tsf_store = document_store(&services);
    let document: &TsfStore_Impl = tsf_store
        .cast_object_ref::<TsfStore>()
        .expect("the document's own object");
    store.fail_lock.set(true);
    let queue = services.pending.borrow();
    let answer = catch_unwind(AssertUnwindSafe(|| {
        document.RequestLock(TS_LF_READ.0 | TS_LF_SYNC)
    }));
    drop(queue);
    let answer = answer.expect("no panic leaves the COM entry");
    assert_eq!(answer.map_err(|error| error.code()), Err(E_UNEXPECTED));
    assert_eq!(
        services.entry_depth.get(),
        0,
        "the entry left no depth behind"
    );
    services.focus_store(None);
    assert!(
        matches!(*services.serving.borrow(), Serving::Nothing),
        "a host operation after the call runs at once"
    );
    services.shutdown();
}

/// A document's store holds the window's `Rc` state, so it does not claim to
/// be agile: another apartment reaches it through a proxy to TSF's.
fn a_document_store_is_bound_to_its_apartment(hwnd: HWND) {
    use windows::Win32::System::Com::IAgileObject;

    let services = TextServices::activate(window_ref(hwnd), apartment()).expect("TSF activates");
    services.focus_store(Some(erased(&composing_store())));
    let agile = document_store(&services).cast::<IAgileObject>().is_ok();
    services.shutdown();
    assert!(!agile, "the document's store claims to be agile");
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

/// A real window offers its own text services as its host, on its owner
/// thread (ADR-0135 §3): the host serves a focused store through a TSF
/// document of the window, and once the window is destroyed its
/// `WM_DESTROY` has deactivated TSF, so the host the presentation still
/// holds refuses every request.
///
/// Red-checks: have `text_store_host` answer `None` (no host is offered),
/// or skip the deactivation in `WM_DESTROY` (the host still serves).
#[test]
fn a_window_offers_its_text_services_as_its_host() {
    use crate::host_window::{HostWindow as _, OwnerThreadToken};

    let platform = crate::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui window host")
                .with_size(Size::new(200.0, 80.0))
                .with_visible(false),
        )
        .expect("window");
    let win32 = window
        .as_any()
        .downcast_ref::<crate::WindowsWindow>()
        .expect("Win32 window");
    let hwnd = win32.hwnd();
    let host = win32
        .text_store_host(OwnerThreadToken::new())
        .expect("a real window offers its host");
    let services = crate::window::windows::proc::with_window_context(hwnd, "test", |context| {
        context.text_services.borrow().clone()
    })
    .flatten()
    .expect("the window holds its text services");
    assert!(
        std::ptr::addr_eq(Rc::as_ptr(&host), Rc::as_ptr(&services)),
        "the host is the window's own TSF connection"
    );

    let store = composing_store();
    host.focus_store(Some(erased(&store)));
    let document = associated_manager(&services);
    assert!(
        !same_object(&document, &services.empty),
        "a focused store gets a TSF document of its own, associated with the window"
    );
    assert_eq!(
        host.complete_composition(&erased(&store)),
        Ok(CompositionEnd::Committed),
        "TSF ends the composition it holds in the focused store (none here)"
    );

    window.close();
    assert_eq!(
        host.complete_composition(&erased(&store)),
        Err(TextStoreHostError::Unavailable),
        "the destroyed window's text services are shut down"
    );
    assert!(
        matches!(*services.serving.borrow(), Serving::Shutdown),
        "and its document released"
    );
}

/// Records which sink notifications the text services sent.
struct SinkNotifications(Arc<std::sync::Mutex<Vec<String>>>);

impl tracing::Subscriber for SinkNotifications {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        #[derive(Default)]
        struct Notification {
            message: Option<String>,
            what: Option<String>,
        }
        impl tracing::field::Visit for Notification {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "what" {
                    self.what = Some(value.to_owned());
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                match field.name() {
                    "message" => self.message = Some(format!("{value:?}")),
                    "what" => self.what = Some(format!("{value:?}").trim_matches('"').to_owned()),
                    _ => {}
                }
            }
        }
        let mut notification = Notification::default();
        event.record(&mut notification);
        if notification.message.as_deref() == Some("sink notification")
            && let Some(what) = notification.what
        {
            self.0.lock().expect("notification log").push(what);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Moving a window, ending a size-and-move loop and a DPI change tell TSF
/// the focused field's layout changed, so an input method's candidate
/// window follows the text; with no field focused nothing is sent.
#[test]
fn a_moved_window_tells_tsf_its_layout_changed() {
    use crate::host_window::{HostWindow as _, OwnerThreadToken};
    use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos,
        WM_DPICHANGED, WM_EXITSIZEMOVE,
    };

    let platform = crate::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui moved field")
                .with_size(Size::new(200.0, 80.0))
                .with_visible(false),
        )
        .expect("window");
    let win32 = window
        .as_any()
        .downcast_ref::<crate::WindowsWindow>()
        .expect("Win32 window");
    let hwnd = win32.hwnd();
    let host = win32
        .text_store_host(OwnerThreadToken::new())
        .expect("a real window offers its host");
    fn count(notifications: &std::sync::Mutex<Vec<String>>) -> usize {
        notifications
            .lock()
            .expect("notification log")
            .iter()
            .filter(|what| *what == "OnLayoutChange")
            .count()
    }
    fn move_window(hwnd: HWND, dx: i32) {
        let mut rect = RECT::default();
        // SAFETY: the case's own window on its owner thread; `rect` is a
        // live local.
        unsafe {
            GetWindowRect(hwnd, &raw mut rect).expect("window rectangle");
            SetWindowPos(
                hwnd,
                None,
                rect.left + dx,
                rect.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .expect("move the window");
        }
    }
    let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));
    let layout_changes = || count(&notifications);
    let move_by = |dx: i32| move_window(hwnd, dx);
    tracing::subscriber::with_default(SinkNotifications(Arc::clone(&notifications)), || {
        move_by(10);
        assert_eq!(layout_changes(), 0, "no field is focused");

        let store = composing_store();
        host.focus_store(Some(erased(&store)));
        notifications.lock().expect("notification log").clear();
        move_by(10);
        assert_eq!(layout_changes(), 1, "a move changes the field's layout");
        // SAFETY: the case's own window; the message carries no pointer.
        unsafe { SendMessageW(hwnd, WM_EXITSIZEMOVE, None, None) };
        assert_eq!(layout_changes(), 2, "the end of a size-and-move loop does");
        let mut rect = RECT::default();
        // SAFETY: the case's own window; `WM_DPICHANGED` carries the address
        // of the suggested rectangle, a live local here, at the DPI it has.
        unsafe {
            GetWindowRect(hwnd, &raw mut rect).expect("window rectangle");
            let dpi = (window.scale_factor() * 96.0).round() as usize;
            SendMessageW(
                hwnd,
                WM_DPICHANGED,
                Some(WPARAM((dpi << 16) | dpi)),
                Some(LPARAM(&raw const rect as isize)),
            );
        }
        assert!(layout_changes() >= 3, "a DPI change does");

        // Moved while a lock is held (a listener of an edit TSF's grant made),
        // the field reports it once the lock is released: TSF answers with a
        // synchronous lock request, which a held lock would refuse.
        let before = layout_changes();
        let during = Rc::new(Cell::new(None));
        let (seen, log, raw) = (
            Rc::clone(&during),
            Arc::clone(&notifications),
            hwnd.0 as isize,
        );
        let outcome = store.request_lock(
            LockGrant::read(move |_| {
                move_window(HWND(raw as *mut _), 10);
                seen.set(Some(count(&log)));
            }),
            LockTiming::Sync,
        );
        assert_eq!(
            outcome,
            Ok(LockOutcome::Granted),
            "the read lock is granted"
        );
        assert_eq!(during.get(), Some(before), "nothing is sent under the lock");
        assert_eq!(
            layout_changes(),
            before + 1,
            "the move is reported once the lock is released"
        );
        host.focus_store(None);
    });
    window.close();
}

/// One row: its name, and the case run against the shared window.
type Row = (&'static str, fn(HWND));

#[test]
fn the_text_services_answer_a_completion_for_its_store() {
    let platform = crate::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui text services")
                .with_size(Size::new(200.0, 80.0))
                .with_visible(false),
        )
        .expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<crate::WindowsWindow>()
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
            "window destroyed inside a TSF call",
            a_window_destroyed_inside_a_tsf_call_shuts_down_when_it_returns,
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
            "TSF call failing past its poisoning",
            a_tsf_call_failing_past_its_poisoning_leaves_no_depth,
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
        (
            "protection change while composing",
            a_protection_change_ends_the_composition_first,
        ),
        (
            "protection change while composing, termination refused",
            a_protection_change_whose_termination_is_refused_commits_in_place,
        ),
        (
            "teardown with a panicking retirement and Pop diagnostic",
            a_teardown_whose_pop_diagnostic_panics_raises_the_first_failure,
        ),
        (
            "document store bound to its apartment",
            a_document_store_is_bound_to_its_apartment,
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

/// A subscriber that panics on the one event whose message is `.0`.
struct PanicsOn(&'static str);

impl tracing::Subscriber for PanicsOn {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(&'static str, bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.1 |= field.name() == "message" && format!("{value:?}") == self.0;
            }
        }
        let mut message = Message(self.0, false);
        event.record(&mut message);
        assert!(!message.1, "injected subscriber panic");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Whether this owner thread still has a top-level window titled `title`.
fn owner_thread_window_titled(title: &str) -> bool {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW};
    use windows_core::BOOL;

    struct Search {
        title: Vec<u16>,
        buffer: Vec<u16>,
        found: bool,
    }
    unsafe extern "system" fn visit(hwnd: HWND, state: LPARAM) -> BOOL {
        // SAFETY: enumeration is synchronous on the caller; state points to
        // its live exclusive Search, and the buffer is writable for the call.
        let search = unsafe { &mut *(state.0 as *mut Search) };
        let length = unsafe { GetWindowTextW(hwnd, &mut search.buffer) } as usize;
        search.found |= search.buffer[..length] == search.title;
        BOOL(1)
    }
    let title: Vec<u16> = title.encode_utf16().collect();
    let mut search = Search {
        buffer: vec![0; title.len() + 2],
        title,
        found: false,
    };
    // SAFETY: only the calling thread's windows are visited; Search remains
    // live until enumeration finishes and the callback never unwinds.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            LPARAM((&raw mut search) as isize),
        )
    }
    .expect("enumerate this owner thread's windows");
    search.found
}

#[test]
fn orphan_detection_ignores_a_foreign_owner_but_still_finds_its_own_window() {
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
        TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
    };
    use windows_core::{HSTRING, w};

    const TITLE: &str = "flui orphan oracle foreign owner";
    struct HiddenWindow(HWND);
    impl HiddenWindow {
        fn new() -> Self {
            let title = HSTRING::from(TITLE);
            // SAFETY: the system STATIC class owns this hidden top-level
            // window; its title stays live through creation. Only this owner
            // thread accesses the wrapper and destroys its HWND.
            Self(
                unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("STATIC"),
                        &title,
                        WINDOW_STYLE(0),
                        0,
                        0,
                        100,
                        50,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                .expect("hidden oracle window"),
            )
        }
    }
    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns the live HWND on its creating thread.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }
    struct ForeignOwner {
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for ForeignOwner {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let (ready, waiting) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _window = HiddenWindow::new();
        ready.send(()).expect("test waits for foreign owner");
        let mut message = MSG::default();
        while !stop_worker.load(Ordering::Acquire) {
            // SAFETY: this owner pumps only its own thread queue; message is
            // writable for PeekMessage and initialized before dispatch.
            unsafe {
                while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let _foreign = ForeignOwner {
        stop,
        worker: Some(worker),
    };
    waiting.recv().expect("foreign owner has a live window");
    assert!(
        !owner_thread_window_titled(TITLE),
        "foreign window is not our orphan"
    );
    let own = HiddenWindow::new();
    assert!(
        owner_thread_window_titled(TITLE),
        "our live window is detected"
    );
    drop(own);
    assert!(
        !owner_thread_window_titled(TITLE),
        "our window is gone while foreign owner remains"
    );
}

/// A subscriber that panics on the activation's own diagnostic neither
/// orphans the half-built window nor fails the open: the diagnostic is
/// contained, and the window comes up with its text services.
///
/// Red-check: activate before the wrapper owns the window and log outside
/// containment (the panic unwinds out of `open_window` and leaves the HWND
/// behind).
#[test]
fn a_panicking_activation_diagnostic_orphans_no_window() {
    const TITLE: &str = "flui panicking TSF diagnostic";
    let platform = crate::WindowsPlatform::new().expect("platform");
    let opened = tracing::subscriber::with_default(PanicsOn("ITfThreadMgr activated"), || {
        catch_unwind(AssertUnwindSafe(|| {
            platform.open_window(
                WindowOptions::default()
                    .with_title(TITLE)
                    .with_size(Size::new(200.0, 80.0))
                    .with_visible(false),
            )
        }))
    });
    match opened {
        Ok(Ok(window)) => {
            let hwnd = window
                .as_any()
                .downcast_ref::<crate::WindowsWindow>()
                .expect("Win32 window")
                .hwnd();
            assert!(
                crate::window::windows::proc::with_window_context(hwnd, "test", |context| {
                    context.text_services.borrow().is_some()
                })
                .unwrap_or(false),
                "the window's text services are active"
            );
            window.close();
        }
        Ok(Err(error)) => panic!("the open failed: {error}"),
        Err(_) => assert!(
            !owner_thread_window_titled(TITLE),
            "the panic left an orphaned native window behind"
        ),
    }
    assert!(
        !owner_thread_window_titled(TITLE),
        "no native window is left"
    );
}

/// A window destroyed while its text services activate (by a message the
/// activation dispatched) fails the open with a typed error instead of
/// returning a wrapper around the dead handle.
///
/// Red-check: return the wrapper whatever the activation left.
#[test]
fn a_window_destroyed_during_activation_fails_the_open() {
    fn destroy(hwnd: HWND) {
        // SAFETY: the owner thread, on the window being built.
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd) };
    }
    let platform = crate::WindowsPlatform::new().expect("platform");
    super::super::BEFORE_TSF_ACTIVATION.set(Some(destroy));
    let opened = platform.open_window(
        WindowOptions::default()
            .with_title("flui destroyed during activation")
            .with_size(Size::new(200.0, 80.0))
            .with_visible(false),
    );
    super::super::BEFORE_TSF_ACTIVATION.set(None);
    assert!(
        matches!(opened, Err(crate::owner::OpenWindowError::Backend { .. })),
        "the open fails with a typed error, got {:?}",
        opened.map(|_| "a window")
    );
}

/// A window that outlives its platform retires its text services inside the
/// thread's COM apartment: the services keep the apartment entered, so the
/// platform's drop does not leave COM under them, and the apartment ends
/// once the window is gone.
///
/// Red-check: have `WindowsLoop`'s drop leave COM itself (the services
/// then shut down outside any apartment).
#[test]
fn a_window_dropped_after_its_platform_retires_tsf_inside_com() {
    use std::sync::mpsc;
    use windows::Win32::System::Com::{
        APTTYPE, APTTYPEQUALIFIER, COINIT_MULTITHREADED, CoGetApartmentType, CoInitializeEx,
        CoUninitialize,
    };

    use super::SHUTDOWN_IN_APARTMENT;
    use crate::owner::windows::com_apartment::thread_in_apartment;

    struct HeldMta {
        release: mpsc::Sender<()>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for HeldMta {
        fn drop(&mut self) {
            let _ = self.release.send(());
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    assert!(!thread_in_apartment(), "the test thread starts outside STA");
    let platform = crate::WindowsPlatform::new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("flui outlives its platform")
                .with_size(Size::new(200.0, 80.0))
                .with_visible(false),
        )
        .expect("window");
    let hwnd = window
        .as_any()
        .downcast_ref::<crate::WindowsWindow>()
        .expect("Win32 window")
        .hwnd();
    #[expect(
        clippy::used_underscore_binding,
        reason = "Observe the drop-only apartment hold without extending its lifetime."
    )]
    let apartment = crate::window::windows::proc::with_window_context(hwnd, "test", |context| {
        context
            .text_services
            .borrow()
            .as_ref()
            .map(|services| Rc::downgrade(&services._apartment))
    })
    .flatten()
    .expect("the window's text services are active");

    // A second COM client may keep the process MTA alive while this
    // window releases its own STA. Hold it until the assertion, including
    // panic cleanup, so the apartment query has a deterministic context.
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        // SAFETY: no pointer arguments; success is balanced on this thread.
        let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let _ = ready_tx.send(initialized.0);
        if initialized.is_ok() {
            let _ = released.recv();
            // SAFETY: matches this worker's successful initialization.
            unsafe { CoUninitialize() };
        }
    });
    let _mta = HeldMta {
        release,
        worker: Some(worker),
    };
    assert!(ready_rx.recv().expect("MTA worker initialized") >= 0);

    SHUTDOWN_IN_APARTMENT.set(None);
    drop(platform);
    window.close();
    drop(window);
    assert!(
        apartment.upgrade().is_none(),
        "the window released the last owned apartment lease"
    );
    assert_eq!(
        SHUTDOWN_IN_APARTMENT.get(),
        Some(true),
        "the window's text services shut down inside the COM apartment"
    );
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    // SAFETY: both out-pointers refer to live writable locals.
    let observed = unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) };
    assert!(
        !thread_in_apartment(),
        "the apartment ends with its last holder; observed={observed:?}, kind={kind:?}, qualifier={qualifier:?}"
    );
}
