//! R19: a handle a UI runtime handed out refuses once the runtime is
//! dropped, typed and without a panic, and holding it does not keep the
//! runtime's tree alive. One row per handle type.

use super::*;

/// The handles a mounted state acquires, kept past the runtime that made them.
struct Acquired {
    rebuild: flui_view::RebuildHandle,
    writer: flui_view::WriterSource,
    signal: flui_view::Signal<u32>,
}

/// A root that hands its handles out in `init_state` and records the drop of
/// its state, the tree's own value.
#[derive(Clone)]
struct HandleSource {
    acquired: Rc<std::cell::RefCell<Option<Acquired>>>,
    released: Rc<Cell<bool>>,
}

struct HandleSourceState(HandleSource);

impl Drop for HandleSourceState {
    fn drop(&mut self) {
        self.0.released.set(true);
    }
}

impl StatefulView for HandleSource {
    type State = HandleSourceState;

    fn create_state(&self) -> Self::State {
        HandleSourceState(self.clone())
    }
}

impl ViewState<HandleSource> for HandleSourceState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.0.acquired.borrow_mut().replace(Acquired {
            rebuild: ctx.rebuild_handle(),
            writer: ctx.writer_source(),
            signal: ctx.signal(7),
        });
    }

    fn build(&self, _view: &HandleSource, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
}

impl View for HandleSource {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A runtime with [`HandleSource`] mounted and framed once, its handles and
/// the flag its state's drop sets.
fn mounted() -> (UiRuntime, Acquired, Rc<Cell<bool>>) {
    let source = HandleSource {
        acquired: Rc::new(std::cell::RefCell::new(None)),
        released: Rc::new(Cell::new(false)),
    };
    let mut ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&source)
        .expect("the root mounts");
    let mut clock = flui_foundation::ManualClock::new();
    let mut sink = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Presented);
    assert!(ui_runtime.pump(&mut clock, &mut sink).presented());
    let acquired = source
        .acquired
        .borrow_mut()
        .take()
        .expect("init_state acquired the handles");
    (ui_runtime, acquired, source.released)
}

/// Drops the runtime while `held` is alive and asserts the tree went with it.
fn drop_holding<H>(ui_runtime: UiRuntime, released: &Cell<bool>, held: &H) {
    let _ = held;
    drop(ui_runtime);
    assert!(
        released.get(),
        "a held handle does not keep the runtime's tree alive"
    );
}

/// A `RebuildHandle` goes inactive, and scheduling through it does nothing.
pub(crate) fn rebuild_handle() {
    let (ui_runtime, acquired, released) = mounted();
    assert!(acquired.rebuild.is_active());
    drop_holding(ui_runtime, &released, &acquired.rebuild);
    assert!(!acquired.rebuild.is_active());
    acquired.rebuild.schedule(RebuildReason::StateChange);
}

/// A `UiCommandSender` refuses every command with `OwnerGone`, handing the
/// rejected command back.
pub(crate) fn command_sender() {
    let (ui_runtime, _acquired, released) = mounted();
    let sender = ui_runtime.command_sender();
    drop_holding(ui_runtime, &released, &sender);
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    assert!(matches!(
        sender.send_navigation(NavigatorCommand::maybe_pop(navigator.command_target())),
        Err(CommandSendError::OwnerGone { .. })
    ));
    sender.request_redraw();
}

/// A cross-thread signal write sent after the drop is refused with
/// `OwnerGone`; its closure is returned unrun.
pub(crate) fn signal_write() {
    let (ui_runtime, acquired, released) = mounted();
    let sender = ui_runtime.command_sender();
    drop_holding(ui_runtime, &released, &sender);
    let ran = Arc::new(AtomicBool::new(false));
    let applied = Arc::clone(&ran);
    assert!(matches!(
        sender.send_signal_write(acquired.signal.detach(), move |_, _| {
            applied.store(true, Ordering::SeqCst);
        }),
        Err(CommandSendError::OwnerGone { .. })
    ));
    assert!(!ran.load(Ordering::SeqCst));
}

/// A write through a `WriterSource` is refused with `OwnerClosed`.
pub(crate) fn writer_source() {
    let (ui_runtime, acquired, released) = mounted();
    drop_holding(ui_runtime, &released, &acquired.writer);
    let written = acquired.writer.write(|cx| acquired.signal.set(cx, 8));
    assert_eq!(written, Err(flui_view::SignalError::OwnerClosed));
}

/// A `PostFrameHandle` refuses a callback with `Closed`; the callback is
/// never run.
pub(crate) fn post_frame_handle() {
    let (ui_runtime, _acquired, released) = mounted();
    let handle = ui_runtime.owner_frame.post_frame_handle();
    drop_holding(ui_runtime, &released, &handle);
    assert_eq!(
        handle.schedule(|_| unreachable!("a refused callback never runs")),
        Err(flui_scheduler::PostFrameScheduleError::Closed)
    );
}
