//! The text services as a `TextStoreHost`, against the real TSF in a hidden
//! window, with no input method typing: which store a completion reaches,
//! and that one queued behind a TSF call is never lost.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_platform_api::ImeEvent;
use flui_platform_api::text_store::{
    CommitGate, CompositionEnd, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore,
    TextStoreError, TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreStatus,
    project_ime_event,
};
use windows::Win32::Foundation::{E_UNEXPECTED, HWND};
use windows::Win32::UI::TextServices::{
    ITextStoreACP, ITextStoreACP_Impl, TS_LF_READ, TS_LF_SYNC, TS_SS_NOHIDDENTEXT,
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

/// An in-memory store whose observer retirement (`set_observer(None)`) or
/// lock request panics once, when told: application code a document's
/// teardown and a TSF call reach.
struct FailingStore {
    inner: Rc<InMemoryTextStore>,
    fail_retirement: Cell<bool>,
    fail_lock: Cell<bool>,
}

impl FailingStore {
    fn new(text: &str) -> Rc<Self> {
        let inner = InMemoryTextStore::new(text);
        inner.set_commit_gate(CommitGate::new());
        Rc::new(Self {
            inner,
            fail_retirement: Cell::new(false),
            fail_lock: Cell::new(false),
        })
    }
}

impl TextStore for FailingStore {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        assert!(!self.fail_lock.take(), "lock request failure");
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
        assert!(
            !(retiring && self.fail_retirement.take()),
            "observer retirement failure"
        );
    }
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

/// TSF reads a field's status when it opens the document and again on
/// `OnStatusChange`: a protected field's text is hidden from it.
fn the_status_follows_the_focused_store(hwnd: HWND) {
    let services = TextServices::activate(hwnd).expect("TSF activates");
    let store = InMemoryTextStore::new("secret");
    store.set_commit_gate(CommitGate::new());
    store.set_protected(true);
    services.focus_store(Some(erased(&store)));
    let tsf_store = document_store(&services);
    // SAFETY: a plain COM call on the document's own object.
    let hidden = unsafe { tsf_store.GetStatus() }.expect("status");
    assert_eq!(
        hidden.dwStaticFlags & TS_SS_NOHIDDENTEXT,
        0,
        "a protected field holds hidden text"
    );
    store.set_protected(false);
    // SAFETY: as above.
    let plain = unsafe { tsf_store.GetStatus() }.expect("status");
    assert_eq!(
        plain.dwStaticFlags & TS_SS_NOHIDDENTEXT,
        TS_SS_NOHIDDENTEXT,
        "an unprotected one does not"
    );
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
        ("status", the_status_follows_the_focused_store),
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
