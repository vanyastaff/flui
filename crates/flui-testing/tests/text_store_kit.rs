//! The conformance kit against the reference store, and against stores
//! with one fault each: the kit must pass the first and fail every other,
//! naming the case that caught the fault. A kit that cannot fail proves
//! nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point};
use flui_platform_api::text_store::{
    CommitGate, Composition, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, PointMode,
    RangeRect, Selection, TextChange, TextStore, TextStoreEdit, TextStoreError, TextStoreObserver,
    TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range, utf16,
};
use flui_testing::text_store_kit::{
    self, FixtureCapabilities, InMemoryFixture, KIT_VERSION, TextStoreFixture,
};

fn in_memory_store_conforms_to_the_kit() {
    text_store_kit::assert_conforms(&mut InMemoryFixture::new(), KIT_VERSION);
}

/// One deliberate defect a store might have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// Panics before invoking an offered grant, with an opaque aggregate payload.
    PanicsBeforeGrant,
    /// Reports its length in UTF-8 bytes.
    CountsUtf8Bytes,
    /// Ignores the commit gate it is handed and grants at once.
    GrantsInsideTransaction,
    /// Tells the observer about the platform's own edits.
    EchoesPlatformEdits,
    /// Moves a platform selection inside a grapheme to the grapheme's end.
    SnapsPlatformSelectionToGraphemes,
    /// Answers a point in the right half of a supplementary scalar with the
    /// offset between its surrogates.
    SplitsSurrogatesOnHitTest,
    /// Notifies its owner once per edit rather than once per session.
    NotifiesPerEdit,
    /// Tells the observer of an app edit at once, inside a frame
    /// transaction too.
    NotifiesInsideTransaction,
    /// Notifies its owner of every session that edited anything, the
    /// composition included.
    NotifiesForComposition,
    /// Notifies its owner from inside the session, under its lock.
    NotifiesUnderTheLock,
}

/// `InMemoryTextStore` with `fault` spliced into its sessions.
struct Faulty {
    inner: Rc<InMemoryTextStore>,
    fault: Fault,
    observer: Rc<RefCell<Option<Rc<dyn TextStoreObserver>>>>,
    edits: Rc<Cell<usize>>,
    /// Sessions that edited anything, for the faults that notify per session.
    sessions: Rc<Cell<usize>>,
    owner_hook: OwnerHook,
}

type OwnerHook = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

impl Faulty {
    fn new(text: &str, fault: Fault) -> Self {
        Self {
            inner: InMemoryTextStore::new(text),
            fault,
            observer: Rc::new(RefCell::new(None)),
            edits: Rc::new(Cell::new(0)),
            sessions: Rc::new(Cell::new(0)),
            owner_hook: Rc::new(RefCell::new(None)),
        }
    }

    /// Whether this store keeps its own count of owner notifications rather
    /// than the inner store's.
    fn counts_sessions(&self) -> bool {
        matches!(
            self.fault,
            Fault::NotifiesForComposition | Fault::NotifiesUnderTheLock
        )
    }
}

impl TextStore for Faulty {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let fault = self.fault;
        if fault == Fault::PanicsBeforeGrant {
            struct Bomb;
            impl Drop for Bomb {
                fn drop(&mut self) {
                    panic!("opaque store failure destructor");
                }
            }
            std::panic::panic_any((Bomb, Bomb));
        }
        let wrapped = match grant {
            LockGrant::Read(body) => LockGrant::read(move |session| {
                body(&ReadFault {
                    inner: session,
                    fault,
                });
            }),
            LockGrant::ReadWrite(body) => {
                let (observer, edits) = (Rc::clone(&self.observer), Rc::clone(&self.edits));
                let (sessions, hook) = (Rc::clone(&self.sessions), Rc::clone(&self.owner_hook));
                let per_session = self.counts_sessions();
                LockGrant::read_write(move |session| {
                    let before = edits.get();
                    body(&mut EditFault {
                        inner: session,
                        fault,
                        observer,
                        edits: Rc::clone(&edits),
                    });
                    if per_session && edits.get() != before {
                        sessions.set(sessions.get() + 1);
                        if fault == Fault::NotifiesUnderTheLock {
                            let hook = hook.borrow().clone();
                            if let Some(hook) = hook {
                                hook();
                            }
                        }
                    }
                })
            }
        };
        self.inner.request_lock(wrapped, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        if self.fault != Fault::GrantsInsideTransaction {
            self.inner.set_commit_gate(gate);
        }
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.observer.borrow_mut().clone_from(&observer);
        self.inner.set_observer(observer);
    }
}

fn whole_text(session: &dyn TextStoreRead) -> String {
    let whole = Utf16Range::new(Utf16Offset::ZERO, session.document_len())
        .expect("zero precedes every length");
    session.text(whole).unwrap_or_default()
}

fn faulty_len(session: &dyn TextStoreRead, fault: Fault) -> Utf16Offset {
    if fault == Fault::CountsUtf8Bytes {
        Utf16Offset::new(whole_text(session).len())
    } else {
        session.document_len()
    }
}

fn faulty_index(
    session: &dyn TextStoreRead,
    fault: Fault,
    point: Point<f64>,
    mode: PointMode,
) -> Result<Utf16Offset, TextStoreError> {
    let found = session.index_at_point(point, mode)?;
    if fault != Fault::SplitsSurrogatesOnHitTest {
        return Ok(found);
    }
    let pair = Utf16Range::new(found, Utf16Offset::new(found.get() + 2)).expect("ascending");
    let Ok(text) = session.text(pair) else {
        return Ok(found);
    };
    let supplementary = text.chars().count() == 1;
    let rect = session.rect_for_range(pair)?.bounds;
    let middle = rect.origin.x + rect.size.width / 2.0;
    if supplementary && point.x > middle {
        Ok(Utf16Offset::new(found.get() + 1))
    } else {
        Ok(found)
    }
}

struct ReadFault<'a> {
    inner: &'a dyn TextStoreRead,
    fault: Fault,
}

impl TextStoreRead for ReadFault<'_> {
    fn document_len(&self) -> Utf16Offset {
        faulty_len(self.inner, self.fault)
    }
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.inner.text(range)
    }
    fn selection(&self) -> Selection {
        self.inner.selection()
    }
    fn composition(&self) -> Option<Composition> {
        self.inner.composition()
    }
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.inner.rect_for_range(range)
    }
    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        self.inner.document_bounds()
    }
    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        faulty_index(self.inner, self.fault, point, mode)
    }
}

struct EditFault<'a> {
    inner: &'a mut dyn TextStoreEdit,
    fault: Fault,
    observer: Rc<RefCell<Option<Rc<dyn TextStoreObserver>>>>,
    edits: Rc<Cell<usize>>,
}

impl EditFault<'_> {
    fn edited(&self, change: Option<TextChange>) {
        self.edits.set(self.edits.get() + 1);
        if self.fault == Fault::EchoesPlatformEdits {
            let observer = self.observer.borrow().clone();
            if let (Some(observer), Some(change)) = (observer, change) {
                observer.text_changed(change);
            }
        }
    }

    /// `offset` moved past a combining mark that follows it.
    fn snapped(&self, offset: Utf16Offset) -> Utf16Offset {
        let text = whole_text(&*self.inner);
        let Ok(byte) = utf16::byte_offset(&text, offset) else {
            return offset;
        };
        match text[byte..].chars().next() {
            Some('\u{300}'..='\u{36f}') => Utf16Offset::new(offset.get() + 1),
            _ => offset,
        }
    }
}

impl TextStoreRead for EditFault<'_> {
    fn document_len(&self) -> Utf16Offset {
        faulty_len(&*self.inner, self.fault)
    }
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.inner.text(range)
    }
    fn selection(&self) -> Selection {
        self.inner.selection()
    }
    fn composition(&self) -> Option<Composition> {
        self.inner.composition()
    }
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.inner.rect_for_range(range)
    }
    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        self.inner.document_bounds()
    }
    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        faulty_index(&*self.inner, self.fault, point, mode)
    }
}

impl TextStoreEdit for EditFault<'_> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let change = self.inner.replace(range, text)?;
        self.edited(Some(change));
        Ok(change)
    }

    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        let change = self.inner.insert_at_selection(text)?;
        self.edited(Some(change));
        Ok(change)
    }

    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        let selection = if self.fault == Fault::SnapsPlatformSelectionToGraphemes {
            Selection {
                anchor: self.snapped(selection.anchor),
                active: self.snapped(selection.active),
            }
        } else {
            selection
        };
        self.inner.set_selection(selection)?;
        self.edited(None);
        Ok(())
    }

    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        self.inner.set_composition(composition)?;
        self.edited(None);
        Ok(())
    }
}

struct FaultyFixture {
    store: Rc<Faulty>,
}

impl FaultyFixture {
    fn new(fault: Fault) -> Self {
        Self {
            store: Rc::new(Faulty::new("", fault)),
        }
    }
}

impl TextStoreFixture for FaultyFixture {
    fn store(&mut self) -> Rc<dyn TextStore> {
        let store: Rc<dyn TextStore> = self.store.clone(); // the kit drives the faulty wrapper through the erased contract.
        store
    }

    fn reset(&mut self, text: &str) {
        self.store = Rc::new(Faulty::new(text, self.store.fault));
    }

    fn app_replace_all(&mut self, text: &str) {
        let inner = &self.store.inner;
        let whole = Utf16Range::new(Utf16Offset::ZERO, utf16::utf16_len(&inner.text()))
            .expect("zero precedes every length");
        if self.store.fault != Fault::NotifiesInsideTransaction {
            inner.app_replace(whole, text);
            return;
        }
        // Edit with the inner store's observer unset, then tell the
        // platform's observer at once, whatever the gate says.
        let observer = self.store.observer.borrow().clone();
        inner.set_observer(None);
        inner.app_replace(whole, text);
        inner.set_observer(observer.clone());
        if let Some(observer) = observer {
            observer.text_changed(TextChange {
                start: Utf16Offset::ZERO,
                old_end: whole.end(),
                new_end: utf16::utf16_len(text),
            });
            observer.selection_changed();
        }
    }

    fn pump(&mut self) {
        let _ = self.store.run_deferred_grants();
    }

    fn owner_notifications(&self) -> usize {
        if self.store.fault == Fault::NotifiesPerEdit {
            self.store.edits.get()
        } else if self.store.counts_sessions() {
            self.store.sessions.get()
        } else {
            self.store.inner.owner_notifications()
        }
    }

    fn set_owner_hook(&mut self, hook: Option<Rc<dyn Fn()>>) {
        if self.store.counts_sessions() {
            *self.store.owner_hook.borrow_mut() = hook;
        } else {
            self.store.inner.set_owner_listener(hook);
        }
    }

    fn capabilities(&self) -> FixtureCapabilities {
        FixtureCapabilities::new().with_geometry(true)
    }
}

#[track_caller]
fn assert_kit_catches(fault: Fault, case: &str) {
    let failures = text_store_kit::run(&mut FaultyFixture::new(fault), KIT_VERSION);
    assert!(
        failures.iter().any(|failure| failure.case == case),
        "{fault:?} should fail `{case}`; the kit reported {failures:#?}"
    );
}

fn kit_fails_a_store_that_grants_inside_a_transaction() {
    assert_kit_catches(
        Fault::GrantsInsideTransaction,
        "sync_request_inside_a_transaction_is_refused",
    );
    assert_kit_catches(
        Fault::GrantsInsideTransaction,
        "async_request_inside_a_transaction_waits_for_the_next_anchor",
    );
}

fn kit_fails_a_store_that_notifies_inside_a_transaction() {
    assert_kit_catches(
        Fault::NotifiesInsideTransaction,
        "app_edits_inside_a_transaction_reach_the_observer_after_it",
    );
}

fn kit_fails_a_store_that_notifies_its_owner_of_a_composition() {
    assert_kit_catches(
        Fault::NotifiesForComposition,
        "composition_only_sessions_do_not_notify_the_owner",
    );
}

fn kit_fails_a_store_that_notifies_its_owner_under_the_lock() {
    assert_kit_catches(
        Fault::NotifiesUnderTheLock,
        "owner_notification_runs_after_release",
    );
}

/// A kit version names the cases a downstream suite certified against: the
/// version 2 cases fail a store that predates them, and a suite pinned to
/// version 1 still passes it.
fn a_pinned_kit_version_does_not_grow() {
    let failures = text_store_kit::run(&mut FaultyFixture::new(Fault::NotifiesForComposition), 1);
    assert!(failures.is_empty(), "kit v1 grew: {failures:#?}");
}

fn a_store_failure_before_the_grant_is_reported_and_next_grant_progresses() {
    let mut fixture = FaultyFixture::new(Fault::PanicsBeforeGrant);
    let case = text_store_kit::cases()
        .iter()
        .find(|case| case.name == "a_panicking_grant_releases_the_lock")
        .expect("documented public kit case");
    let failure = case
        .run(&mut fixture)
        .expect_err("store failure did not invoke the offered grant");
    assert_eq!(failure.case, case.name);
    assert_eq!(failure.message, "panicked: a non-string panic payload");
    // Change only the consumer fixture's injected fault; an ordinary idle
    // grant does not exercise its unrelated transaction-grant defect.
    Rc::get_mut(&mut fixture.store)
        .expect("case released its store clones")
        .fault = Fault::GrantsInsideTransaction;
    let ran = Rc::new(Cell::new(false));
    let next = Rc::clone(&ran);
    assert_eq!(
        fixture
            .store()
            .request_lock(LockGrant::read(move |_| next.set(true)), LockTiming::Sync),
        Ok(LockOutcome::Granted)
    );
    assert!(ran.get());
}

fn kit_reports_pre_grant_failure_without_destroying_its_payload() {
    isolated_kit_payload_child("lock");
}

fn kit_retains_opaque_failure_payloads_and_continues() {
    if std::env::var_os("FLUI_TEXT_KIT_PAYLOAD_CHILD").is_some() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Bomb(Arc<AtomicUsize>);
        impl Drop for Bomb {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("opaque payload destructor");
            }
        }
        struct Fixture {
            inner: InMemoryFixture,
            remaining: usize,
            drops: Arc<AtomicUsize>,
        }
        impl TextStoreFixture for Fixture {
            fn store(&mut self) -> Rc<dyn TextStore> {
                self.inner.store()
            }
            fn reset(&mut self, text: &str) {
                if self.remaining > 0 {
                    self.remaining -= 1;
                    std::panic::panic_any((
                        Bomb(Arc::clone(&self.drops)),
                        Bomb(Arc::clone(&self.drops)),
                    ));
                }
                self.inner.reset(text);
            }
            fn app_replace_all(&mut self, text: &str) {
                self.inner.app_replace_all(text);
            }
            fn pump(&mut self) {
                self.inner.pump();
            }
            fn owner_notifications(&self) -> usize {
                self.inner.owner_notifications()
            }
            fn set_owner_hook(&mut self, hook: Option<Rc<dyn Fn()>>) {
                self.inner.set_owner_hook(hook);
            }
            fn capabilities(&self) -> FixtureCapabilities {
                self.inner.capabilities()
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let mut fixture = Fixture {
            inner: InMemoryFixture::new(),
            remaining: 2,
            drops: Arc::clone(&drops),
        };
        let failures = text_store_kit::run(&mut fixture, KIT_VERSION);
        assert_eq!(
            failures.len(),
            2,
            "only the two reset failures are reported"
        );
        assert_eq!(failures[0].case, "length_counts_utf16_units");
        assert_eq!(failures[1].case, "text_reads_utf16_ranges");
        for failure in failures {
            assert_eq!(failure.message, "panicked: a non-string panic payload");
        }
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        text_store_kit::assert_conforms(&mut fixture, KIT_VERSION);
        return;
    }
    isolated_kit_payload_child("reset");
}

fn isolated_kit_payload_child(kind: &str) {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "text_store_kit::text_store_kit_matrix",
            "--nocapture",
        ])
        .env("FLUI_TEXT_KIT_PAYLOAD_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kit consumer child");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().expect("child status").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap child");
            panic!("kit failure containment blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("child output");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "kit opaque payload containment failed: {output:?}"
    );
}

#[test]
fn text_store_kit_matrix() {
    if let Ok(kind) = std::env::var("FLUI_TEXT_KIT_PAYLOAD_CHILD") {
        match kind.as_str() {
            "reset" => kit_retains_opaque_failure_payloads_and_continues(),
            "lock" => a_store_failure_before_the_grant_is_reported_and_next_grant_progresses(),
            _ => panic!("unknown kit child"),
        }
        return;
    }
    crate::run_table(
        "text_store_kit_matrix",
        &[
            (
                "kit_reports_pre_grant_failure_without_destroying_its_payload",
                kit_reports_pre_grant_failure_without_destroying_its_payload as fn(),
            ),
            (
                "kit_retains_opaque_failure_payloads_and_continues",
                kit_retains_opaque_failure_payloads_and_continues as fn(),
            ),
            (
                "in_memory_store_conforms_to_the_kit",
                in_memory_store_conforms_to_the_kit as fn(),
            ),
            (
                "kit_fails_a_store_that_grants_inside_a_transaction",
                kit_fails_a_store_that_grants_inside_a_transaction as fn(),
            ),
            (
                "kit_fails_a_store_that_notifies_inside_a_transaction",
                kit_fails_a_store_that_notifies_inside_a_transaction as fn(),
            ),
            (
                "kit_fails_a_store_that_notifies_its_owner_of_a_composition",
                kit_fails_a_store_that_notifies_its_owner_of_a_composition as fn(),
            ),
            (
                "kit_fails_a_store_that_notifies_its_owner_under_the_lock",
                kit_fails_a_store_that_notifies_its_owner_under_the_lock as fn(),
            ),
            (
                "a_pinned_kit_version_does_not_grow",
                a_pinned_kit_version_does_not_grow as fn(),
            ),
        ],
    );
}
