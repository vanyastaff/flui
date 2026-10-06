//! `HeadlessBinding::pump_frame` runs the shared async-driver step, and a
//! realm owns the tasks its widgets spawn.
//!
//! `flui-app` carries the mirror-image test for `UiRealm::draw_frame`. Both
//! poll the realm's owner-local tasks in the frame's mid-frame slot; if either
//! stopped, exactly one of the two would fail — which is the
//! headless↔production divergence this pair exists to catch.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_scheduler::{AsyncDriver, TaskToken};
use flui_testing::HeadlessBinding;
use flui_testing::widgets::{LaidOut, lay_out, loose};
use flui_view::prelude::*;
use parking_lot::Mutex;

/// A future the test can complete from outside, exposing its waker.
struct Signal {
    done: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Waker>>>,
    polls: Arc<AtomicUsize>,
}

impl std::future::Future for Signal {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        self.polls.fetch_add(1, Ordering::Relaxed);
        if self.done.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            let _prev = self.waker.lock().replace(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// A wake from a worker thread is picked up by the next frame, on the frame
/// thread.
pub(crate) fn headless_wake_from_another_thread_is_polled_on_the_frame_thread() {
    let mut binding = HeadlessBinding::new();
    let done = Arc::new(AtomicBool::new(false));
    let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
    let polls = Arc::new(AtomicUsize::new(0));
    let polled_on = Arc::new(Mutex::new(Vec::new()));
    let polled_on_for_task = Arc::clone(&polled_on);

    let signal = Signal {
        done: Arc::clone(&done),
        waker: Arc::clone(&waker),
        polls: Arc::clone(&polls),
    };
    let _token = binding.spawn_local(Box::pin(async move {
        polled_on_for_task.lock().push(std::thread::current().id());
        signal.await;
    }));

    binding.pump_frame(Duration::from_millis(16));
    let waker = waker.lock().clone().expect("waker stored");
    done.store(true, Ordering::Release);

    let worker_id = std::thread::spawn(move || {
        waker.wake_by_ref();
        std::thread::current().id()
    })
    .join()
    .expect("worker");

    assert_eq!(polls.load(Ordering::Relaxed), 1, "no poll off-thread");

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 2);

    let main_id = std::thread::current().id();
    let threads = polled_on.lock().clone();
    assert!(threads.iter().all(|id| *id == main_id));
    assert_ne!(worker_id, main_id);
}

// ---------------------------------------------------------------------------
// Owner-local tasks: the realm owns them, widget handles only reach them
// ---------------------------------------------------------------------------

/// An owner-thread value whose destructor records the thread it ran on.
struct Capture {
    drops: Rc<RefCell<Vec<std::thread::ThreadId>>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.drops.borrow_mut().push(std::thread::current().id());
    }
}

/// A future a worker thread completes: `Pending` until `done`, keeping the
/// last waker it was handed where the worker can reach it.
#[derive(Clone, Default)]
struct WorkerGate {
    done: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl WorkerGate {
    fn wait(&self) -> impl std::future::Future<Output = ()> + use<> {
        let gate = self.clone();
        std::future::poll_fn(move |cx| {
            if gate.done.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                let _previous = gate.waker.lock().replace(cx.waker().clone());
                Poll::Pending
            }
        })
    }

    /// Complete from a worker thread, as an IO completion would.
    fn complete_from_a_worker(&self) {
        let gate = self.clone();
        std::thread::spawn(move || {
            gate.done.store(true, Ordering::Release);
            let waker = gate.waker.lock().take();
            if let Some(waker) = waker {
                waker.wake();
            }
        })
        .join()
        .expect("the worker completes without panicking");
    }
}

/// A future holding `Rc` state runs on the binding's driver, and a worker
/// thread's wake completes it on the next frame.
pub(crate) fn owner_local_future_completes_after_a_worker_wake() {
    let mut binding = HeadlessBinding::new();
    let gate = WorkerGate::default();
    let completed = Rc::new(Cell::new(false));
    let seen = Rc::clone(&completed);
    let wait = gate.wait();
    let _token = binding.spawn_local(Box::pin(async move {
        wait.await;
        seen.set(true);
    }));

    binding.pump_frame(Duration::from_millis(16));
    assert!(!completed.get(), "the gate is still closed");

    gate.complete_from_a_worker();
    binding.pump_frame(Duration::from_millis(16));
    assert!(
        completed.get(),
        "the worker's wake is polled by the next frame, on the owner thread"
    );
}

/// What a mounted [`TaskOwner`] hands back to the test, and what outlives
/// its realm.
#[derive(Clone, Default)]
struct Parked {
    /// Every task token the widget spawned: held here, not in the widget's
    /// state, so unmounting the widget does not cancel the task and the
    /// realm's own teardown is what has to retire it.
    tokens: Rc<RefCell<Vec<TaskToken>>>,
    /// The widget's driver handle, kept past the realm.
    driver: Rc<RefCell<Option<AsyncDriver>>>,
}

/// Spawns one owner-local task in `init_state` that waits on `gate`
/// holding a [`Capture`]; its state's destructor panics when `state_drop_panics`.
#[derive(Clone, StatefulView)]
struct TaskOwner {
    gate: WorkerGate,
    drops: Rc<RefCell<Vec<std::thread::ThreadId>>>,
    parked: Parked,
    state_drop_panics: bool,
}

struct TaskOwnerState {
    view: TaskOwner,
}

impl StatefulView for TaskOwner {
    type State = TaskOwnerState;

    fn create_state(&self) -> Self::State {
        TaskOwnerState { view: self.clone() }
    }
}

impl ViewState<TaskOwner> for TaskOwnerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let driver = ctx
            .async_driver()
            .expect("a realm's presentation installs its async driver");
        let capture = Capture {
            drops: Rc::clone(&self.view.drops),
        };
        let wait = self.view.gate.wait();
        let token = driver.spawn_local(Box::pin(async move {
            let _capture = capture;
            wait.await;
        }));
        self.view.parked.tokens.borrow_mut().push(token);
        *self.view.parked.driver.borrow_mut() = Some(driver);
    }

    fn build(&self, _view: &TaskOwner, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

impl Drop for TaskOwnerState {
    fn drop(&mut self) {
        assert!(!self.view.state_drop_panics, "state drop probe");
    }
}

fn mount_task_owner(state_drop_panics: bool) -> (LaidOut, TaskOwner) {
    let view = TaskOwner {
        gate: WorkerGate::default(),
        drops: Rc::new(RefCell::new(Vec::new())),
        parked: Parked::default(),
        state_drop_panics,
    };
    let laid = lay_out(view.clone(), loose(100.0));
    assert_eq!(view.parked.tokens.borrow().len(), 1, "one task spawned");
    assert!(view.drops.borrow().is_empty(), "the task is pending");
    (laid, view)
}

/// Dropping the realm retires its pending tasks on the owner thread, and a
/// completion arriving afterwards from a worker finds nothing to deliver to.
fn late_completion_after_an_ordinary_teardown() {
    let (laid, view) = mount_task_owner(false);
    drop(laid);
    view.gate.complete_from_a_worker();
    assert_eq!(
        *view.drops.borrow(),
        [std::thread::current().id()],
        "the task's capture is dropped once, by the realm, on the owner thread"
    );
    view.parked.tokens.borrow_mut().clear();
    assert_eq!(view.drops.borrow().len(), 1, "a dead token retires nothing");
}

/// The same when a widget state's destructor panics during the teardown: the
/// realm still retires its tasks on the owner thread before raising the
/// first failure.
fn late_completion_after_a_teardown_that_panics() {
    let (laid, view) = mount_task_owner(true);
    let teardown = catch_unwind(AssertUnwindSafe(move || drop(laid)));
    let payload = teardown.expect_err("the destructor panic reaches the realm's owner");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("state drop probe"),
        "the first failure stays authoritative"
    );
    view.gate.complete_from_a_worker();
    assert_eq!(
        *view.drops.borrow(),
        [std::thread::current().id()],
        "the task's capture is dropped once, on the owner thread, before the panic resumes"
    );
}

pub(crate) fn late_completion_after_realm_drop_drops_captures_on_the_owner() {
    late_completion_after_an_ordinary_teardown();
    late_completion_after_a_teardown_that_panics();
}

/// A driver handle a widget leaked past its realm keeps no task alive, and
/// spawning through it retires the future at once instead of queueing it
/// somewhere no frame will ever poll.
pub(crate) fn a_leaked_async_driver_holds_no_task_after_the_realm() {
    let (laid, view) = mount_task_owner(false);
    let leaked = view
        .parked
        .driver
        .borrow()
        .clone()
        .expect("the widget kept its driver");
    drop(laid);
    assert_eq!(
        *view.drops.borrow(),
        [std::thread::current().id()],
        "the realm's teardown retired the task the leaked handle could reach"
    );
    assert_eq!(leaked.pending_task_count(), 0);

    let late_drops = Rc::new(RefCell::new(Vec::new()));
    let capture = Capture {
        drops: Rc::clone(&late_drops),
    };
    let token = leaked.spawn_local(Box::pin(async move {
        let _capture = capture;
    }));
    assert!(token.is_cancelled(), "a dead driver admits no task");
    assert_eq!(
        *late_drops.borrow(),
        [std::thread::current().id()],
        "the refused future is dropped at once, on the caller's thread"
    );

    // Nor does it keep the realm's task store: a hook installed through it
    // lands nowhere and is dropped at once.
    let hook_drops = Arc::new(AtomicUsize::new(0));
    let hook_capture = HookCapture(Arc::clone(&hook_drops));
    leaked.set_request_frame(move || {
        let _capture = &hook_capture;
    });
    assert_eq!(
        hook_drops.load(Ordering::SeqCst),
        1,
        "a handle that outlived its realm retains nothing"
    );
}

/// A `Send` hook capture that counts its drops.
struct HookCapture(Arc<AtomicUsize>);

impl Drop for HookCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn owner_local_task_matrix() {
    crate::run_table(
        "owner_local_task_matrix",
        &[
            (
                "owner_local_future_completes_after_a_worker_wake",
                owner_local_future_completes_after_a_worker_wake as fn(),
            ),
            (
                "late_completion_after_realm_drop_drops_captures_on_the_owner",
                late_completion_after_realm_drop_drops_captures_on_the_owner as fn(),
            ),
            (
                "a_leaked_async_driver_holds_no_task_after_the_realm",
                a_leaked_async_driver_holds_no_task_after_the_realm as fn(),
            ),
        ],
    );
}
