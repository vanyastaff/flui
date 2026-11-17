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
    ClipboardBackend, ClipboardCapability, ClipboardHandle, PrimaryClaim,
    PrimarySelectionCapability, PrimarySelectionHandle,
};
use flui_platform::capability::scripted::{Answer, ScenarioFailure};
use flui_platform::capability::{
    Abandoned, Capability, CapabilityProvider, CapabilityRegistrar, Completion, InvalidResultKey,
    Plugin, ProviderContext, Resolver,
};
use flui_platform::{
    ClipboardError, ClipboardExpiry, ClipboardRead, ClipboardWrite, Concealment, ExpiryClearing,
    Unsupported, UnsupportedReason,
};
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        AcquirerState {
            acquired: Rc::clone(&self.acquired),
        }
    }
}

impl<C: Capability> View for Acquirer<C> {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl<C: Capability> ViewState<Acquirer<C>> for AcquirerState<C> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquired.replace(Some(ctx.capability::<C>()));
    }

    fn build(&self, _view: &Acquirer<C>, _ctx: &dyn BuildContext) -> impl IntoView {
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
    acquire_capability_in::<ClipboardCapability>(host, window)
}

/// Mount an [`Acquirer`] of `C` as the root of `window` on `host` and return
/// what it was answered.
pub(crate) fn acquire_capability_in<C: Capability>(
    host: &mut HeadlessHost,
    window: flui_testing::HeadlessWindowId,
) -> Result<C::Handle, Unsupported> {
    let acquired = Acquired::<C>::default();
    host.attach_to(
        window,
        &Acquirer::<C> {
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

/// [`window`] on Linux, the platform with a primary selection.
fn linux_window() -> HeadlessWindow {
    window().with_platform(flui_platform::TargetPlatform::Linux)
}

/// Where a test keeps what a completion settled with.
type Settled<T = ClipboardRead> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

/// The text a read of `text` settles with.
fn answered(text: &str) -> Arc<str> {
    Arc::from(text)
}

fn settle_into<T: 'static>(completion: Completion<T>) -> Settled<T> {
    let settled = Settled::<T>::default();
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
    let written = settle_into(clipboard.write_text("copied"));
    assert_eq!(host.clipboard().text().as_deref(), Some("copied"));
    assert_eq!(*written.borrow(), None, "never inside write_text");

    let settled = settle_into(clipboard.read_text());
    assert_eq!(*settled.borrow(), None, "never inside read_text");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*written.borrow(), Some(Ok(Ok(()))));
    assert_eq!(*settled.borrow(), Some(Ok(Ok(Some(answered("copied"))))));
}

/// A write the host refuses answers its refusal to the writer, at an owner
/// turn, and leaves the clipboard as it was (ADR-0337).
pub(crate) fn a_refused_write_answers_its_refusal() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("before");
    scripted.fail_writes_with(ClipboardError::Unavailable);
    let clipboard = acquire(&mut host).expect("provided");
    let written = settle_into(clipboard.write_text("refused"));
    assert_eq!(*written.borrow(), None, "never inside write_text");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *written.borrow(),
        Some(Ok(Err(ClipboardError::Unavailable)))
    );
    assert_eq!(scripted.text().as_deref(), Some("before"));
}

/// A write is a request the clipboard's write script answers: a step
/// refuses the write it takes, and the standing answer takes the next onto
/// the clipboard (ADR-0337).
pub(crate) fn a_scripted_write_step_refuses_that_write_alone() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("before");
    scripted
        .writes()
        .answer_next(Answer::value(Err(ClipboardError::Unavailable)));
    let clipboard = acquire(&mut host).expect("provided");
    let refused = settle_into(clipboard.write_text("refused"));
    let taken = settle_into(clipboard.write_text("taken"));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *refused.borrow(),
        Some(Ok(Err(ClipboardError::Unavailable)))
    );
    assert_eq!(*taken.borrow(), Some(Ok(Ok(()))));
    assert_eq!(scripted.text().as_deref(), Some("taken"));
    assert_eq!(scripted.writes().received(), 2);
}

/// Whatever answers a write `Ok` lands its text, in one place: a step, a
/// delayed step and a released hold, as the standing answer does; and an
/// `ApplicationOnly` answer keeps the text for this clipboard's reads.
pub(crate) fn every_ok_answer_to_a_scripted_write_lands_its_text() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("before");
    let clipboard = acquire(&mut host).expect("provided");

    scripted.writes().answer_next(Answer::value(Ok(())));
    drop(clipboard.write(ClipboardWrite::text("stepped").concealed()));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(scripted.text().as_deref(), Some("stepped"));
    assert!(
        scripted.is_concealed(),
        "the step's write is the clipboard's"
    );

    scripted
        .writes()
        .answer_next(Answer::value(Ok(())).after(Duration::from_millis(50)));
    drop(clipboard.write_text("delayed"));
    let _ = host.pump(Duration::from_millis(49));
    assert_eq!(
        scripted.text().as_deref(),
        Some("stepped"),
        "not before its answer"
    );
    let _ = host.pump(Duration::from_millis(1));
    assert_eq!(scripted.text().as_deref(), Some("delayed"));

    let hold = scripted.hold_writes();
    drop(clipboard.write_text("held"));
    assert_eq!(hold.held(), 1);
    drop(hold);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(scripted.text().as_deref(), Some("held"));

    scripted
        .writes()
        .answer_next(Answer::value(Err(ClipboardError::ApplicationOnly)));
    let kept = settle_into(clipboard.write_text("kept"));
    let read = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *kept.borrow(),
        Some(Ok(Err(ClipboardError::ApplicationOnly)))
    );
    assert_eq!(*read.borrow(), Some(Ok(Ok(Some(answered("kept"))))));
}

/// Reads and writes keep one order, as a host's clipboard serves them: a
/// read asked after a held write waits for it and answers what it wrote; a
/// read asked before it answers what was there; and writes land in the
/// order they were asked, whichever answer comes first.
pub(crate) fn the_scripted_clipboard_keeps_one_order_across_reads_and_writes() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("before");
    let clipboard = acquire(&mut host).expect("provided");

    let earlier = settle_into(clipboard.read_text());
    let hold = scripted.hold_writes();
    drop(clipboard.write_text("written"));
    let later = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*earlier.borrow(), Some(Ok(Ok(Some(answered("before"))))));
    assert_eq!(
        *later.borrow(),
        None,
        "a read after a held write waits for it"
    );
    drop(hold);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*later.borrow(), Some(Ok(Ok(Some(answered("written"))))));

    // The first write answers late, the second at once: the second lands
    // last, as it was asked last, and its completion settles only once its
    // text has landed.
    scripted
        .writes()
        .answer_next(Answer::value(Ok(())).after(Duration::from_millis(20)));
    let first = settle_into(clipboard.write_text("first"));
    let second = settle_into(clipboard.write_text("second"));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        scripted.text().as_deref(),
        Some("written"),
        "the first has not landed"
    );
    assert_eq!(
        *second.borrow(),
        None,
        "the second's completion waits for its text"
    );
    let _ = host.pump(Duration::from_millis(20));
    let _ = host.pump(Duration::ZERO);
    assert_eq!(scripted.text().as_deref(), Some("second"));
    assert_eq!(*first.borrow(), Some(Ok(Ok(()))));
    assert_eq!(*second.borrow(), Some(Ok(Ok(()))));
}

/// A test's own observer on the write script hears beside the clipboard's,
/// which keeps landing writes and letting reads through; and writes stay
/// held while either of two holds lives.
pub(crate) fn a_tests_observer_and_two_holds_leave_the_clipboard_serving() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    let heard = Rc::new(Cell::new(0_usize));
    let counted = Rc::clone(&heard);
    scripted
        .writes()
        .on_answer(move |_, _| counted.set(counted.get() + 1));
    let clipboard = acquire(&mut host).expect("provided");
    let first = scripted.hold_writes();
    let second = scripted.hold_writes();
    drop(clipboard.write_text("held"));
    let read = settle_into(clipboard.read_text());
    drop(first);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(second.held(), 1, "the other hold still holds the write");
    assert_eq!(*read.borrow(), None, "the read waits for the held write");
    drop(second);
    let _ = host.pump(Duration::ZERO);
    assert_eq!(heard.get(), 1, "the test's observer heard the write");
    assert_eq!(*read.borrow(), Some(Ok(Ok(Some(answered("held"))))));
}

/// A concealed write reaches the host marked, pastes like any other, and an
/// ordinary write after it is unmarked again; the handle answers what the
/// host does with one (ADR-0152 §6).
pub(crate) fn a_concealed_write_reaches_the_host_marked() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    let clipboard = acquire(&mut host).expect("provided");
    assert_eq!(clipboard.concealment(), Concealment::Honored);
    scripted.answer_concealment(Concealment::HintOnly);
    assert_eq!(clipboard.concealment(), Concealment::HintOnly);

    drop(clipboard.write(ClipboardWrite::text("hunter2").concealed()));
    assert_eq!(scripted.text().as_deref(), Some("hunter2"));
    assert!(scripted.is_concealed(), "the write reaches the host marked");
    let settled = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*settled.borrow(), Some(Ok(Ok(Some(answered("hunter2"))))));

    drop(clipboard.write_text("plain"));
    assert!(!scripted.is_concealed(), "an ordinary write is unmarked");

    // An expiring write is a concealed one, and the handle answers what the
    // host does with its time (ADR-0361).
    assert_eq!(clipboard.expiry_clearing(), ExpiryClearing::AtTimeAndLock);
    scripted.answer_expiry_clearing(ExpiryClearing::Unavailable);
    assert_eq!(clipboard.expiry_clearing(), ExpiryClearing::Unavailable);
    let thirty = ClipboardExpiry::try_new(Duration::from_secs(30)).expect("a valid expiry");
    drop(clipboard.write(ClipboardWrite::text("token").expires_after(thirty)));
    assert!(scripted.is_concealed(), "an expiring write is concealed");
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

    fn write(
        &self,
        _write: flui_platform::ClipboardWrite,
        answer: Resolver<flui_platform::ClipboardWritten>,
    ) {
        std::thread::spawn(move || answer.resolve(Ok(())))
            .join()
            .expect("the OS thread")
            .expect("the owner is live");
    }

    fn concealment(&self) -> flui_platform::Concealment {
        flui_platform::Concealment::Unavailable
    }

    fn expiry_clearing(&self) -> ExpiryClearing {
        ExpiryClearing::Unavailable
    }
}

impl CapabilityProvider<ClipboardCapability> for OtherThread {
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        Ok(ClipboardHandle::new(OtherThreadBackend, ctx))
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

/// A read still pending when its runtime ends, whether the host drops it or
/// its last window closes, which retires the runtime on a real host, ends as
/// `OwnerRetired`: its waiting closure is dropped unrun, and the late answer
/// runs nothing.
///
/// The last-window row fails against a host that only stops its last
/// presentation: the runtime lives on, and the late answer runs the closure.
pub(crate) fn a_read_pending_when_its_runtime_drops_ends_as_owner_retired() {
    for last_window_closes in [false, true] {
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

        let kept = if last_window_closes {
            let primary = host.primary_window();
            assert!(host.request_close(primary, flui_view::CloseReason::User));
            assert!(host.is_retired(), "the last close retires the runtime");
            Some(host)
        } else {
            drop(host);
            None
        };
        assert!(
            dropped.get(),
            "the waiting closure is dropped with the runtime (last window closes: \
             {last_window_closes})"
        );
        assert!(!ran.get(), "and never runs");
        assert_eq!(polled.poll(), Some(Err(Abandoned::OwnerRetired)));
        assert_eq!(
            scripted.answer_held_reads(),
            1,
            "the two reads share one host read, whose late answer finds nobody and does not panic"
        );
        if let Some(mut host) = kept {
            let _ = host.pump(Duration::ZERO);
        }
        assert!(!ran.get(), "a pump after the late answer runs nothing");
        assert_eq!(polled.poll(), None, "a completion ends once");
    }
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
    drop(clipboard.write_text("after"));
    let after = settle_into(clipboard.read_text());
    assert_eq!(scripted.reads_asked(), 2, "the write ends the sharing");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*before.borrow(), Some(Ok(Ok(Some(answered("before"))))));
    assert_eq!(*after.borrow(), Some(Ok(Ok(Some(answered("after"))))));
}

pub(crate) fn scripted_reads_answer_in_order_before_the_clipboards_text() {
    let mut host = HeadlessHost::new(window());
    let scripted = host.clipboard();
    scripted.set_text("copied");
    scripted
        .reads()
        .answer_next(Answer::value(Err(ClipboardError::Denied)));
    scripted
        .reads()
        .answer_next(Answer::value(Ok(Some(answered("late")))).after(Duration::from_millis(100)));
    let clipboard = acquire(&mut host).expect("provided");

    let denied = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*denied.borrow(), Some(Ok(Err(ClipboardError::Denied))));

    let late = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::from_millis(99));
    assert_eq!(*late.borrow(), None, "the second step answers at 100 ms");
    let _ = host.pump(Duration::from_millis(1));
    assert_eq!(*late.borrow(), Some(Ok(Ok(Some(answered("late"))))));

    let text = settle_into(clipboard.read_text());
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        *text.borrow(),
        Some(Ok(Ok(Some(answered("copied"))))),
        "the steps used up, the clipboard's text answers"
    );
    host.scenario().verify().expect("every step was used");
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

/// Declares a capability whose name could not key an activity result.
enum Unkeyed {}

impl Capability for Unkeyed {
    type Handle = ();
    const NAME: &'static str = "two words";
}

struct UnkeyedPlugin;

impl Plugin for UnkeyedPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<Unkeyed>(|_: &ProviderContext<'_>| Ok(()));
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

    let unkeyed = CapabilityRegistry::builder()
        .plugin(&UnkeyedPlugin)
        .build()
        .expect_err("a name that cannot key a result");
    assert_eq!(
        unkeyed,
        CapabilityConflict::InvalidName {
            name: "two words",
            reason: InvalidResultKey::NotPrintable,
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
    drop(
        acquire(&mut host)
            .expect("provided")
            .write_text("overridden"),
    );
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

pub(crate) fn a_capture_panicking_at_teardown_reaches_the_runtime_owner() {
    use std::task::{Context, Poll};

    let mut host = HeadlessHost::new(window());
    let scenario = host.scenario().clone();
    host.clipboard().hold_reads();
    let clipboard = acquire(&mut host).expect("provided");
    let capture = PanicOnDrop;
    clipboard.read_text().on_complete(move |_| {
        let _ = &capture;
    });
    let woken = flui_foundation::testing::CountingWaker::new().shared();
    let waker = woken.waker();
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
        woken.count(),
        1,
        "the read after the panicking one is still woken"
    );
    assert_eq!(
        later.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Err(Abandoned::OwnerRetired))
    );
    let owed = scenario.verify().expect_err("the read was held to the end");
    assert!(
        matches!(owed.failures(), [ScenarioFailure::Unanswered { .. }]),
        "{owed}"
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
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<ClipboardHandle, Unsupported> {
        self.0.provide(ctx)
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
    let mut host = HeadlessHost::new(linux_window());
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
    let mut host = HeadlessHost::new(linux_window());
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

/// A runtime whose registry has no primary selection refuses it as not
/// registered, and the host's built-ins have one only where the window's
/// platform does, as a runner's: on Linux and nowhere else, the
/// platform-neutral window included.
///
/// The platform rows fail against a host that registers the primary selection
/// whatever the platform: a field under a Windows convention would paste it
/// on a middle press.
pub(crate) fn a_runtime_without_a_primary_selection_answers_not_registered() {
    use flui_platform::TargetPlatform::{Android, Linux, MacOS, Unknown, Windows, iOS};

    let mut host = HeadlessHost::with_capabilities(window(), CapabilityRegistry::empty());
    let refusal = acquire_capability::<PrimarySelectionCapability>(&mut host)
        .expect_err("nothing provides the primary selection");
    assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
    assert_eq!(refusal.capability(), Some(PrimarySelectionCapability::NAME));

    for (platform, provided) in [
        (Linux, true),
        (Windows, false),
        (MacOS, false),
        (Android, false),
        (iOS, false),
        (Unknown, false),
    ] {
        let mut host = HeadlessHost::new(window().with_platform(platform));
        let acquired = acquire_capability::<PrimarySelectionCapability>(&mut host);
        match acquired {
            Ok(_) => assert!(provided, "{platform:?} has no primary selection"),
            Err(refusal) => {
                assert!(!provided, "{platform:?} provides a primary selection");
                assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
            }
        }
    }
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
    let mut host = HeadlessHost::new(linux_window());
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

/// Whether a provider was offered an Activity when it was asked.
enum ActivitySeen {}

impl Capability for ActivitySeen {
    type Handle = bool;
    const NAME: &'static str = "test-activity-seen";
}

/// An Activity that refuses everything: only whether one is offered counts.
struct EndedActivity;

impl flui_platform::capability::ActivityBridge for EndedActivity {
    fn bind(&self, _mint: flui_platform::capability::ResultMint) {}

    fn launch_for_result(
        &self,
        _completions: &flui_platform::capability::CompletionSource,
        _capability: &flui_platform::capability::CapabilityId,
        _key: flui_platform::capability::RequestKey,
        _intent: &mut dyn FnMut(flui_platform::capability::IntentFrame) -> bool,
    ) -> Result<flui_platform::capability::ResultRequest, flui_platform::capability::ResultRefused>
    {
        Err(flui_platform::capability::ResultRefused::ActivityEnded)
    }

    fn reattach(
        &self,
        _completions: &flui_platform::capability::CompletionSource,
        _capability: &flui_platform::capability::CapabilityId,
        _key: flui_platform::capability::RequestKey,
    ) -> Result<flui_platform::capability::ResultRequest, flui_platform::capability::ResultRefused>
    {
        Err(flui_platform::capability::ResultRefused::ActivityEnded)
    }

    fn take_data(
        &self,
        _capability: &flui_platform::capability::CapabilityId,
        _data: flui_platform::capability::ResultData,
        _f: &mut dyn FnMut(flui_platform::capability::JniFrame),
    ) -> Result<(), flui_platform::capability::DataRefused> {
        Err(flui_platform::capability::DataRefused::NotHeld)
    }

    fn lend(&self, _f: &mut dyn FnMut(flui_platform::capability::JniFrame)) -> bool {
        false
    }
}

/// A provider sees the Activity the composition root offers through the
/// registry's host bridge at each acquisition: none before the host installs
/// one, then the one installed, and none again once it is taken back.
pub(crate) fn a_provider_sees_the_activity_the_host_offers() {
    let host_bridge = flui_platform::capability::HostBridge::new();
    let mut registrar = CapabilityRegistrar::new();
    registrar.provide::<ActivitySeen>(|ctx: &ProviderContext<'_>| Ok(ctx.activity().is_some()));
    let registry = CapabilityRegistry::builder()
        .built_ins(registrar)
        .host(host_bridge.clone())
        .build()
        .expect("one provider");
    let seen = |registry: &CapabilityRegistry| {
        let mut host = HeadlessHost::with_capabilities(window(), registry.clone());
        acquire_capability::<ActivitySeen>(&mut host).expect("provided")
    };

    assert!(!seen(&registry), "no Activity before the host offers one");
    let activity: Rc<dyn flui_platform::capability::ActivityBridge> = Rc::new(EndedActivity);
    assert!(host_bridge.install_activity(Some(activity)).is_none());
    assert!(seen(&registry), "the Activity the host offers");
    assert!(host_bridge.install_activity(None).is_some());
    assert!(!seen(&registry), "none once the host takes it back");
}
