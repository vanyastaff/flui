//! The capability seam as a widget meets it (ADR-0152 §4): what a UI
//! runtime answers without a provider, a provider's refusal, an answer from
//! another thread, a runtime that drops before the answer, an answer that is
//! never given, the registry's conflicts, and a handle's last clone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_interaction::{ClipboardBackend, ClipboardCapability, ClipboardHandle};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityProvider, CapabilityRegistrar, Completion, Plugin,
    ProviderContext, Resolver,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_runtime::capability::{CapabilityConflict, CapabilityRegistry, ProviderTier};
use flui_testing::{HeadlessHost, HeadlessWindow, ScriptedClipboard};
use flui_view::{
    BuildContext, IntoView, LifecycleContext, LifecycleContextExt, StatefulView, View, ViewState,
};

type Acquired = Rc<RefCell<Option<Result<ClipboardHandle, Unsupported>>>>;

/// Acquires the clipboard in `init_state` and keeps what it was answered.
#[derive(Clone)]
struct Acquirer {
    acquired: Acquired,
}

struct AcquirerState {
    acquired: Acquired,
}

impl StatefulView for Acquirer {
    type State = AcquirerState;

    fn create_state(&self) -> Self::State {
        AcquirerState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl View for Acquirer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Acquirer> for AcquirerState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.acquired
            .replace(Some(cx.capability::<ClipboardCapability>()));
    }

    fn build(&self, _view: &Acquirer, _cx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// Mount an [`Acquirer`] on `host` and return what it was answered.
fn acquire(host: &mut HeadlessHost) -> Result<ClipboardHandle, Unsupported> {
    let acquired = Acquired::default();
    host.attach(&Acquirer {
        acquired: Rc::clone(&acquired),
    })
    .expect("root attaches");
    let _ = host.pump(Duration::ZERO);
    acquired.take().expect("init_state ran")
}

fn window() -> HeadlessWindow {
    HeadlessWindow::new(10, 10)
}

/// Where a test keeps what a completion settled with.
type Settled = Rc<RefCell<Option<Result<Option<String>, Abandoned>>>>;

fn settle_into(completion: Completion<Option<String>>) -> Settled {
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
    assert_eq!(*settled.borrow(), Some(Ok(Some("copied".to_owned()))));
}

/// Answers every read from a thread of its own, as an OS callback does.
struct OtherThread;

struct OtherThreadBackend;

impl ClipboardBackend for OtherThreadBackend {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        std::thread::spawn(move || answer.resolve(Some("from the OS".to_owned())))
            .join()
            .expect("the OS thread")
            .expect("the owner is live");
    }

    fn write_text(&self, _text: String) {}
}

impl CapabilityProvider<ClipboardCapability> for OtherThread {
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        Ok(ClipboardHandle::new(
            OtherThreadBackend,
            cx.completions().clone(),
        ))
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
    assert_eq!(*settled.borrow(), Some(Ok(Some("from the OS".to_owned()))));
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
        2,
        "a late answer finds nobody, and does not panic"
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
