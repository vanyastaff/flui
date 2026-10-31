//! The capability seam as a widget meets it (ADR-0152 §4): what a UI
//! runtime answers without a provider, a provider's refusal, an answer from
//! another thread, a runtime that drops before the answer, an answer that is
//! never given, reads that share one answer or one failure, the registry's
//! conflicts, and a handle's last clone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_interaction::{
    ClipboardBackend, ClipboardCapability, ClipboardHandle, PrimaryClaim, PrimaryClaims,
    PrimarySelectionCapability, PrimarySelectionHandle,
};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityProvider, CapabilityRegistrar, Completion, Plugin,
    ProviderContext, Resolver,
};
use flui_platform::{ClipboardError, ClipboardRead, Unsupported, UnsupportedReason};
use flui_runtime::capability::{CapabilityConflict, CapabilityRegistry, ProviderTier};
use flui_testing::{HeadlessHost, HeadlessWindow, ScriptedClipboard};
use flui_view::{
    BuildContext, IntoView, LifecycleContext, LifecycleContextExt, StatefulView, View, ViewState,
};

type Acquired<C> = Rc<RefCell<Option<Result<<C as Capability>::Handle, Unsupported>>>>;

/// Acquires capability `C` in `init_state` and keeps what it was answered.
struct Acquirer<C: Capability> {
    acquired: Acquired<C>,
}

impl<C: Capability> Clone for Acquirer<C> {
    fn clone(&self) -> Self {
        Self {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

struct AcquirerState<C: Capability> {
    acquired: Acquired<C>,
}

impl<C: Capability> StatefulView for Acquirer<C> {
    type State = AcquirerState<C>;

    fn create_state(&self) -> Self::State {
        AcquirerState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl<C: Capability> View for Acquirer<C> {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl<C: Capability> ViewState<Acquirer<C>> for AcquirerState<C> {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.acquired.replace(Some(cx.capability::<C>()));
    }

    fn build(&self, _view: &Acquirer<C>, _cx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// Mount an [`Acquirer`] of the clipboard on `host` and return what it was
/// answered.
fn acquire(host: &mut HeadlessHost) -> Result<ClipboardHandle, Unsupported> {
    acquire_capability::<ClipboardCapability>(host)
}

/// Mount an [`Acquirer`] of `C` on `host` and return what it was answered.
pub(crate) fn acquire_capability<C: Capability>(
    host: &mut HeadlessHost,
) -> Result<C::Handle, Unsupported> {
    let acquired = Acquired::<C>::default();
    host.attach(&Acquirer::<C> {
        acquired: Rc::clone(&acquired),
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    acquired.take().expect("init_state ran")
}

/// Mount an [`Acquirer`] of the clipboard as the root of `window` on `host`
/// and return what it was answered.
fn acquire_in(
    host: &mut HeadlessHost,
    window: flui_testing::HeadlessWindowId,
) -> Result<ClipboardHandle, Unsupported> {
    let acquired = Acquired::<ClipboardCapability>::default();
    host.attach_to(
        window,
        &Acquirer::<ClipboardCapability> {
            acquired: Rc::clone(&acquired),
        },
    )
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    acquired.take().expect("init_state ran")
}

fn window() -> HeadlessWindow {
    HeadlessWindow::new(10, 10)
}

/// Where a test keeps what a completion settled with.
type Settled = Rc<RefCell<Option<Result<ClipboardRead, Abandoned>>>>;

/// The text a read of `text` settles with.
fn answered(text: &str) -> Arc<str> {
    Arc::from(text)
}

fn settle_into(completion: Completion<ClipboardRead>) -> Settled {
    let settled = Settled::default();
    let sink = Rc::clone(&settled);
    completion.on_complete(move |outcome| {
        sink.replace(Some(outcome));
    });
    settled
}

pub(crate) fn a_runtime_with_no_provider_answers_not_registered() {
    let mut host = HeadlessHost::with_capabilities(window(), CapabilityRegistry::empty());
    let refusal = acquire(&mut host).expect_err("nothing provides the clipboard");
    assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
    assert_eq!(refusal.capability(), Some(ClipboardCapability::NAME));
}

pub(crate) fn a_provider_refusal_reaches_the_widget_unchanged() {
    let mut host = HeadlessHost::new(window());
    let refusal = Unsupported::of::<ClipboardCapability>(UnsupportedReason::NotOnThisPlatform);
    host.clipboard().refuse(refusal);
    assert_eq!(
        acquire(&mut host).expect_err("the provider refuses"),
        refusal
    );
}

pub(crate) fn copy_and_paste_round_trip_through_the_headless_clipboard() {
    let mut host = HeadlessHost::new(window());
    let clipboard = acquire(&mut host).expect("the headless host provides a clipboard");
    clipboard.write_text("copied");
    assert_eq!(host.clipboard().text().as_deref(), Some("copied"));

    let settled = settle_into(clipboard.read_text());
    assert_eq!(*settled.borrow(), None, "never inside read_text");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*settled.borrow(), Some(Ok(Ok(Some(answered("copied"))))));
}

/// Answers every read from a thread of its own, as an OS callback does.
struct OtherThread;

struct OtherThreadBackend;

impl ClipboardBackend for OtherThreadBackend {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        std::thread::spawn(move || answer.resolve(Ok(Some(Arc::from("from the OS")))))
            .join()
            .expect("the OS thread")
            .expect("the owner is live");
    }

    fn write_text(&self, _text: String) {}
}

impl CapabilityProvider<ClipboardCapability> for OtherThread {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        Ok(ClipboardHandle::new(OtherThreadBackend, cx))
    }
}

fn registry_of(register: impl FnOnce(&mut CapabilityRegistrar)) -> CapabilityRegistry {
    let mut registrar = CapabilityRegistrar::new();
    register(&mut registrar);
    CapabilityRegistry::builder()
        .built_ins(registrar)
        .build()
        .expect("one provider")
}

pub(crate) fn an_answer_from_another_thread_settles_at_an_owner_turn() {
    let mut host = HeadlessHost::with_capabilities(
        window(),
        registry_of(|registrar| registrar.provide::<ClipboardCapability>(OtherThread)),
    );
    let clipboard = acquire(&mut host).expect("provided");
    let settled = settle_into(clipboard.read_text());
    assert_eq!(
        *settled.borrow(),
        None,
        "answered, but not settled before the owner's turn"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *settled.borrow(),
        Some(Ok(Ok(Some(answered("from the OS")))))
    );
}

/// Records that it was dropped, to tell "dropped unrun" from "kept".
struct DropFlag(Rc<Cell<bool>>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

pub(crate) fn a_read_pending_when_its_runtime_drops_ends_as_owner_retired() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.hold_reads();
    let clipboard = acquire(&mut host).expect("provided");

    let mut polled = clipboard.read_text();
    let waiting = clipboard.read_text();
    let (ran, dropped) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(false)));
    let (flag, ran_in) = (DropFlag(Rc::clone(&dropped)), Rc::clone(&ran));
    waiting.on_complete(move |_| {
        let _flag = &flag;
        ran_in.set(true);
    });
    assert_eq!(polled.poll(), None, "held");

    drop(host);
    assert!(
        dropped.get(),
        "the waiting closure is dropped with the runtime"
    );
    assert!(!ran.get(), "and never runs");
    assert_eq!(polled.poll(), Some(Err(Abandoned::OwnerRetired)));
    assert_eq!(
        scripted.answer_held_reads(),
        1,
        "the two reads share one host read, whose late answer finds nobody and does not panic"
    );
    assert_eq!(polled.poll(), None, "a completion ends once");
}

pub(crate) fn a_read_whose_answer_is_dropped_ends_as_provider_dropped() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    let settled = settle_into(clipboard.read_text());
    assert_eq!(scripted.drop_held_reads(), 1);
    assert_eq!(*settled.borrow(), None, "ends at the owner's turn");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*settled.borrow(), Some(Err(Abandoned::ProviderDropped)));
}

/// The text a settled read holds, which the test compares by allocation.
fn settled_text(settled: &Settled) -> Arc<str> {
    match settled.borrow().clone() {
        Some(Ok(Ok(Some(text)))) => text,
        other => panic!("the read settles with text, not {other:?}"),
    }
}

pub(crate) fn reads_asked_while_one_is_in_flight_share_its_answer() {
    // Two widgets' handles in two windows of one runtime: one read.
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("shared");
    scripted.hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    // A widget in another window of the same runtime acquires its own handle.
    let second_window = host.open_window(window());
    let other = acquire_in(&mut host, second_window).expect("provided");
    let readers = [
        settle_into(clipboard.read_text()),
        settle_into(other.read_text()),
        settle_into(clipboard.read_text()),
    ];
    assert_eq!(scripted.reads_asked(), 1, "one host read for the burst");
    assert_eq!(scripted.answer_held_reads(), 1);
    let _ = host.pump(Duration::ZERO);
    let first = settled_text(&readers[0]);
    assert_eq!(&*first, "shared");
    for reader in &readers[1..] {
        assert!(
            Arc::ptr_eq(&first, &settled_text(reader)),
            "every reader holds the same copy of the text"
        );
    }

    let later = settle_into(clipboard.read_text());
    assert_eq!(
        scripted.reads_asked(),
        2,
        "a read after the answer asks again"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*later.borrow(), Some(Ok(Ok(Some(answered("shared"))))));
}

pub(crate) fn each_runtime_asks_for_its_own_read() {
    let scripted = ScriptedClipboard::new();
    scripted.set_text("per runtime");
    scripted.hold_reads();
    let host_of = || {
        HeadlessHost::with_capabilities(
            window(),
            registry_of(|registrar| scripted.register(registrar)),
        )
    };
    let (mut first, mut second) = (host_of(), host_of());
    let readers = [
        settle_into(acquire(&mut first).expect("provided").read_text()),
        settle_into(acquire(&mut second).expect("provided").read_text()),
    ];
    assert_eq!(
        scripted.reads_asked(),
        2,
        "one provider, a read per runtime"
    );
    drop(second);
    assert_eq!(scripted.answer_held_reads(), 2);
    let _ = first.pump(Duration::ZERO);
    assert_eq!(
        *readers[0].borrow(),
        Some(Ok(Ok(Some(answered("per runtime")))))
    );
    assert_eq!(
        *readers[1].borrow(),
        None,
        "the retired runtime's reader is dropped unrun with it, and strands nobody else"
    );
}

pub(crate) fn a_read_after_a_write_never_joins_the_read_before_it() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("before");
    let clipboard = acquire(&mut host).expect("provided");
    // The host answers at once, but the answer settles at the owner's turn:
    // the earlier read is still in flight when the write lands.
    let before = settle_into(clipboard.read_text());
    clipboard.write_text("after");
    let after = settle_into(clipboard.read_text());
    assert_eq!(scripted.reads_asked(), 2, "the write ends the sharing");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*before.borrow(), Some(Ok(Ok(Some(answered("before"))))));
    assert_eq!(*after.borrow(), Some(Ok(Ok(Some(answered("after"))))));
}

pub(crate) fn a_failed_read_reaches_every_reader_that_joined_it() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.fail_reads_with(ClipboardError::Timeout);
    scripted.hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    let readers = [
        settle_into(clipboard.read_text()),
        settle_into(clipboard.read_text()),
    ];
    assert_eq!(scripted.answer_held_reads(), 1);
    let _ = host.pump(Duration::ZERO);
    for reader in &readers {
        assert_eq!(
            *reader.borrow(),
            Some(Ok(Err(ClipboardError::Timeout))),
            "a failure is no empty clipboard"
        );
    }

    scripted.stop_failing_reads();
    let settled = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *settled.borrow(),
        Some(Ok(Ok(None))),
        "an empty clipboard reads as none"
    );
}

pub(crate) fn an_abandoned_read_abandons_its_joiners_and_the_next_read_asks_again() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("after");
    scripted.hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    let readers = [
        settle_into(clipboard.read_text()),
        settle_into(clipboard.read_text()),
    ];
    assert_eq!(scripted.drop_held_reads(), 1);
    let _ = host.pump(Duration::ZERO);
    for reader in &readers {
        assert_eq!(*reader.borrow(), Some(Err(Abandoned::ProviderDropped)));
    }

    scripted.answer_held_reads();
    let next = settle_into(clipboard.read_text());
    assert_eq!(scripted.reads_asked(), 2, "the abandoned read is over");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*next.borrow(), Some(Ok(Ok(Some(answered("after"))))));
}

pub(crate) fn a_dropped_handle_ends_its_subscription() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    let clipboard = acquire(&mut host).expect("provided");
    assert_eq!(scripted.live_handles(), 1);
    let clone = clipboard.clone();
    drop(clipboard);
    assert_eq!(scripted.live_handles(), 1, "a clone keeps it");
    drop(clone);
    assert_eq!(scripted.live_handles(), 0, "the last clone ends it");
}

/// Provides the clipboard: two of them conflict.
struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        ScriptedClipboard::new().register(registrar);
    }
}

/// Declares another capability under the clipboard's name.
enum Impostor {}

impl Capability for Impostor {
    type Handle = ();
    const NAME: &'static str = ClipboardCapability::NAME;
}

struct ImpostorPlugin;

impl Plugin for ImpostorPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<Impostor>(|_: &ProviderContext<'_>| Ok(()));
    }
}

pub(crate) fn conflicting_registrations_fail_when_the_registry_is_built() {
    let built_in = || registry_registrar(|registrar| ScriptedClipboard::new().register(registrar));
    let two_plugins = CapabilityRegistry::builder()
        .built_ins(built_in())
        .plugin(&ClipboardPlugin)
        .plugin(&ClipboardPlugin)
        .build()
        .expect_err("two plugins for one capability");
    assert_eq!(
        two_plugins,
        CapabilityConflict::Duplicate {
            name: ClipboardCapability::NAME,
            tier: ProviderTier::Plugin,
        },
        "a conflict even where the built-in answers in their place"
    );

    let shared_name = CapabilityRegistry::builder()
        .built_ins(built_in())
        .plugin(&ImpostorPlugin)
        .build()
        .expect_err("two capabilities named alike");
    assert_eq!(
        shared_name,
        CapabilityConflict::SharedName {
            name: ClipboardCapability::NAME,
        }
    );
}

fn registry_registrar(register: impl FnOnce(&mut CapabilityRegistrar)) -> CapabilityRegistrar {
    let mut registrar = CapabilityRegistrar::new();
    register(&mut registrar);
    registrar
}

pub(crate) fn an_override_answers_in_place_of_the_built_in() {
    let built_in = ScriptedClipboard::new();
    let chosen = ScriptedClipboard::new();
    let registry = CapabilityRegistry::builder()
        .built_ins(registry_registrar(|registrar| built_in.register(registrar)))
        .overrides(registry_registrar(|registrar| chosen.register(registrar)))
        .plugin(&ClipboardPlugin)
        .build()
        .expect("one provider per tier");
    let mut host = HeadlessHost::with_capabilities(window(), registry);
    acquire(&mut host)
        .expect("provided")
        .write_text("overridden");
    assert_eq!(chosen.text().as_deref(), Some("overridden"));
    assert_eq!(built_in.text(), None);
}

/// Panics when dropped, as a capture with a faulty `Drop` does.
struct PanicOnDrop;

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("capture dropped at teardown");
    }
}

/// Counts its wakes.
struct CountingWaker(std::sync::atomic::AtomicUsize);

impl std::task::Wake for CountingWaker {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(crate) fn a_capture_panicking_at_teardown_reaches_the_runtime_owner() {
    use std::task::{Context, Poll, Waker};

    let mut host = HeadlessHost::new(window());
    host.clipboard().hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    let capture = PanicOnDrop;
    clipboard.read_text().on_complete(move |_| {
        let _ = &capture;
    });
    let woken = std::sync::Arc::new(CountingWaker(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(std::sync::Arc::clone(&woken));
    let mut later = std::pin::pin!(clipboard.read_text().into_future());
    assert_eq!(
        later.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    );

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(host)))
        .expect_err("the capture's panic reaches whoever drops the runtime");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"capture dropped at teardown")
    );
    assert_eq!(
        woken.0.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the read after the panicking one is still woken"
    );
    assert_eq!(
        later.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(Abandoned::OwnerRetired))
    );
}

/// A provider whose own `Drop` panics, wrapping the scripted clipboard.
struct ExplodingProvider(ScriptedClipboard);

impl Drop for ExplodingProvider {
    fn drop(&mut self) {
        panic!("provider dropped at teardown");
    }
}

impl CapabilityProvider<ClipboardCapability> for ExplodingProvider {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        self.0.provide(cx)
    }
}

pub(crate) fn a_provider_after_a_teardown_failure_is_kept_not_dropped() {
    let scripted = ScriptedClipboard::new();
    scripted.hold_reads();
    let provider = ExplodingProvider(scripted);
    let mut host = HeadlessHost::with_capabilities(
        window(),
        registry_of(move |registrar| registrar.provide::<ClipboardCapability>(provider)),
    );
    let clipboard = acquire(&mut host).expect("provided");
    let capture = PanicOnDrop;
    clipboard.read_text().on_complete(move |_| {
        let _ = &capture;
    });
    drop(clipboard);

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(host)))
        .expect_err("the first teardown failure reaches whoever drops the runtime");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"capture dropped at teardown"),
        "the completion's failure comes first; the provider is kept, not dropped into a second panic"
    );
}

/// A claim is asked for its text only when the selection is read, with no
/// borrow of the selection held; a newer claim, or another application
/// selecting, makes it stale, and a stale claim's drop leaves the selection
/// alone.
pub(crate) fn a_primary_read_after_a_release_never_joins_the_read_before_it() {
    let mut host = HeadlessHost::new(window());
    let selection = acquire_capability::<PrimarySelectionCapability>(&mut host)
        .expect("the headless host provides a primary selection");
    let claim = selection.claim(|| Some("withdrawn".to_owned()));
    // Answered at once, settled at the owner's turn: still in flight.
    let before = settle_into(selection.read_text());
    drop(claim);
    let after = settle_into(selection.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*before.borrow(), Some(Ok(Ok(Some(answered("withdrawn"))))));
    assert_eq!(
        *after.borrow(),
        Some(Ok(Ok(None))),
        "the released text is not read again"
    );
}

pub(crate) fn a_primary_claim_is_asked_lazily_and_released_only_while_current() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.primary_selection();
    let selection = acquire_capability::<PrimarySelectionCapability>(&mut host)
        .expect("the headless host provides a primary selection");
    let asked = Rc::new(Cell::new(0));
    let first = selection.claim({
        let asked = Rc::clone(&asked);
        let scripted = scripted.clone();
        move || {
            asked.set(asked.get() + 1);
            // Re-enters the selection, as a widget reading its own state may.
            assert!(scripted.is_claimed());
            Some("first".to_owned())
        }
    });
    assert_eq!(asked.get(), 0, "a claim copies nothing");
    assert!(first.is_current());

    let settled = settle_into(selection.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*settled.borrow(), Some(Ok(Ok(Some(answered("first"))))));
    assert_eq!(asked.get(), 1, "a read asks the claim once");

    let second = selection.claim(|| Some("second".to_owned()));
    assert!(!first.is_current(), "the newer claim holds the selection");
    drop(first);
    assert!(second.is_current(), "a stale claim releases nothing");
    assert_eq!(scripted.text().as_deref(), Some("second"));

    scripted.select("elsewhere");
    assert!(!second.is_current(), "another application selected");
    drop(second);
    assert_eq!(scripted.text().as_deref(), Some("elsewhere"));

    let third = selection.claim(|| Some("third".to_owned()));
    drop(third);
    assert_eq!(scripted.text(), None, "a current claim's drop releases it");
}

pub(crate) fn a_runtime_without_a_primary_selection_answers_not_registered() {
    let mut host = HeadlessHost::with_capabilities(window(), CapabilityRegistry::empty());
    let refusal = acquire_capability::<PrimarySelectionCapability>(&mut host)
        .expect_err("nothing provides the primary selection");
    assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
    assert_eq!(refusal.capability(), Some(PrimarySelectionCapability::NAME));
}

/// A capture that, as it drops, claims the selection anew and then panics.
struct ClaimsThenPanics {
    selection: PrimarySelectionHandle,
    newer: Rc<RefCell<Option<PrimaryClaim>>>,
}

impl Drop for ClaimsThenPanics {
    fn drop(&mut self) {
        self.newer
            .replace(Some(self.selection.claim(|| Some("newer".to_owned()))));
        panic!("capture dropped at teardown");
    }
}

/// When the claim a new one replaces panics as its captures drop, the new
/// claim, whose guard never reached its caller, is withdrawn; a claim that
/// panicking `Drop` made in turn stays current.
pub(crate) fn a_claim_whose_predecessor_panics_as_it_drops_is_withdrawn() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.primary_selection();
    let selection = acquire_capability::<PrimarySelectionCapability>(&mut host)
        .expect("the headless host provides a primary selection");

    let first = selection.claim({
        let capture = PanicOnDrop;
        move || {
            let _ = &capture;
            Some("first".to_owned())
        }
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        selection.claim(|| Some("second".to_owned()))
    }))
    .expect_err("the replaced capture's panic reaches the claimer");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"capture dropped at teardown")
    );
    assert!(!first.is_current());
    assert!(
        !scripted.is_claimed(),
        "the claim whose guard never reached its caller is withdrawn"
    );
    drop(first);

    let newer = Rc::new(RefCell::new(None));
    let first = selection.claim({
        let capture = ClaimsThenPanics {
            selection: selection.clone(),
            newer: Rc::clone(&newer),
        };
        move || {
            let _ = &capture;
            Some("first".to_owned())
        }
    });
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        selection.claim(|| Some("second".to_owned()))
    }))
    .expect_err("the replaced capture's panic reaches the claimer");
    assert_eq!(
        scripted.text().as_deref(),
        Some("newer"),
        "withdrawing the failed claim leaves the one its predecessor made"
    );
    assert!(
        newer
            .borrow()
            .as_ref()
            .is_some_and(PrimaryClaim::is_current),
        "the claim made while the predecessor dropped holds the selection"
    );
    drop(first);
}

/// The environment variable naming the case a child process of
/// `primary_claims_keep_their_captures_while_unwinding` runs.
pub(crate) const PRIMARY_UNWIND_CHILD: &str = "FLUI_PRIMARY_UNWIND_CHILD";

/// One case of a claim whose captures panic as they drop, met while
/// another panic unwinds: dropping them then would abort the process, so
/// the claim is withdrawn and its captures are kept (ADR-0127). The parent
/// runs each in a child process, where an abort is a failed exit.
pub(crate) fn a_claim_unwound_keeps_its_captures(case: &str) {
    let held = || {
        let capture = PanicOnDrop;
        move || {
            let _ = &capture;
            Some("held".to_owned())
        }
    };
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match case {
        // A widget's claim guard drops in the unwind.
        "claim" => {
            let mut host = HeadlessHost::new(window());
            let selection = acquire_capability::<PrimarySelectionCapability>(&mut host)
                .expect("the headless host provides a primary selection");
            let _claim = selection.claim(held());
            panic!("the first failure");
        }
        // A backend's claims drop in the unwind with a claim still current.
        "claims" => {
            let mut claims = PrimaryClaims::new();
            let (_current, _) = claims.claim(Rc::new(held()));
            panic!("the first failure");
        }
        _ => unreachable!("no case {case}"),
    }))
    .expect_err("the case panics");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"the first failure"));
}
