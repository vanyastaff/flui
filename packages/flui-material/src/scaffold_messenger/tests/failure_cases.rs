//! Distinct mounted owner inboxes expose each fanout delivery failure. They are
//! a private fault seam, not supported cross-UI runtime Messenger registration.
use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone, StatefulView)]
struct RebuildProbe {
    handle: Rc<RefCell<Option<RebuildHandle>>>,
    builds: Rc<Cell<usize>>,
}
struct RebuildProbeState {
    handle: Rc<RefCell<Option<RebuildHandle>>>,
}
impl StatefulView for RebuildProbe {
    type State = RebuildProbeState;
    fn create_state(&self) -> Self::State {
        RebuildProbeState {
            handle: Rc::clone(&self.handle),
        }
    }
}
impl ViewState<RebuildProbe> for RebuildProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.handle.borrow_mut() = Some(ctx.rebuild_handle());
    }
    fn build(&self, view: &RebuildProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        view.builds.set(view.builds.get() + 1);
        flui_sdk::widgets::SizedBox::shrink()
    }
}
struct MountedProbe {
    binding: flui_testing::HeadlessBinding,
    handle: RebuildHandle,
    builds: Rc<Cell<usize>>,
}
impl MountedProbe {
    fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let handle = Rc::new(RefCell::new(None));
        let builds = Rc::new(Cell::new(0));
        let view = RebuildProbe {
            handle: Rc::clone(&handle),
            builds: Rc::clone(&builds),
        };
        let mut owners = flui_testing::MountOwners::fresh();
        owners.build_owner.set_on_build_scheduled(wake);
        let mut binding = flui_testing::HeadlessBinding::new();
        binding.mount_root(&view, owners, flui_testing::MountOptions::tight(32.0, 32.0));
        binding.pump_frame(Duration::from_millis(16));
        let handle = handle
            .borrow()
            .clone()
            .expect("lifecycle acquired mounted rebuild");
        Self {
            binding,
            handle,
            builds,
        }
    }
    fn pump_and_check(&mut self) {
        let before = self.builds.get();
        self.binding.pump_frame(Duration::from_millis(16));
        assert!(
            self.builds.get() > before,
            "accepted wake must rebuild its actual mounted element"
        );
    }
}

fn exercise(completion_fails: bool, forward_fails: bool, fanout_failures: usize) {
    let (_harness, handle) = mounted_handle();
    let armed = Arc::new(AtomicBool::new(false));
    let forward_calls = Arc::new(AtomicUsize::new(0));
    let fanout_calls = Arc::new(AtomicUsize::new(0));
    let forward_armed = Arc::clone(&armed);
    let seen = Arc::clone(&forward_calls);
    let mut forward = MountedProbe::new(move || {
        if forward_armed.load(Ordering::SeqCst) {
            seen.fetch_add(1, Ordering::SeqCst);
            assert!(!forward_fails, "forward wake failed");
        }
    });
    let forward_handle = forward.handle.clone();
    handle
        .shared
        .entry_controller
        .subscribe_status(std::rc::Rc::new(move |status| {
            if status == AnimationStatus::Forward {
                forward_handle.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }
        }))
        .detach();
    let mut fanout = Vec::new();
    for _ in 0..2 {
        let armed = Arc::clone(&armed);
        let seen = Arc::clone(&fanout_calls);
        fanout.push(MountedProbe::new(move || {
            if armed.load(Ordering::SeqCst) {
                let call = seen.fetch_add(1, Ordering::SeqCst);
                if call < fanout_failures {
                    assert!(call != 0, "first fanout wake failed");
                    panic!("second fanout wake failed");
                }
            }
        }));
    }
    // Independent owners can mint equal local ElementIds. Map keys here only
    // identify fanout slots, so take two distinct slots from one owner.
    let root_id = fanout[1]
        .binding
        .tree_mut()
        .iter()
        .next()
        .expect("a mounted slot");
    handle
        .shared
        .scaffolds
        .borrow_mut()
        .insert(root_id, fanout[0].handle.clone());
    let second_id = fanout[1]
        .binding
        .tree_mut()
        .iter()
        .find(|id| *id != root_id)
        .expect("distinct mounted slot");
    handle
        .shared
        .scaffolds
        .borrow_mut()
        .insert(second_id, fanout[1].handle.clone());
    let completions = Rc::new(Cell::new(0));
    let seen = Rc::clone(&completions);
    let arm = Arc::clone(&armed);
    handle
        .show_snack_bar(snack_bar("first"))
        .on_closed(move |_cx, reason| {
            assert_eq!(reason, SnackBarClosedReason::Remove);
            seen.set(seen.get() + 1);
            arm.store(true, Ordering::SeqCst);
            assert!(!completion_fails, "completion failed");
        });
    let next = Rc::new(Cell::new(None));
    let seen = Rc::clone(&next);
    handle
        .show_snack_bar(snack_bar("next"))
        .on_closed(move |_cx, reason| seen.set(Some(reason)));
    // Empty the independent inboxes after setup. Start the production advance
    // from its dismissed-entry boundary, without an earlier dismissal wake: the
    // status listener stays quiet while the setup settles the controller.
    {
        let _quiet = handle.shared.enter_reconcile();
        handle
            .shared
            .entry_controller
            .set_value(0.0)
            .expect("no build is running");
    }
    forward.binding.pump_frame(Duration::from_millis(16));
    for probe in &mut fanout {
        probe.binding.pump_frame(Duration::from_millis(16));
    }
    let front = handle
        .shared
        .queue
        .borrow()
        .front()
        .cloned()
        .expect("first queued");
    front.set_reason(SnackBarClosedReason::Remove);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle.shared.pop_and_advance(ReconcileOrigin::Direct);
    }));
    // A failing wake is reported where it happens and stays owed; only the
    // completion's own failure reaches the advance's caller.
    if completion_fails {
        let payload = outcome.expect_err("the completion failure propagates");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"completion failed"));
    } else {
        assert!(
            outcome.is_ok(),
            "a failing wake does not unwind the advance"
        );
    }
    assert_eq!(completions.get(), 1);
    assert_eq!(
        forward_calls.load(Ordering::SeqCst),
        1,
        "next entrance delivery attempted"
    );
    assert_eq!(
        fanout_calls.load(Ordering::SeqCst),
        2,
        "every scaffold delivery attempted after a failure"
    );
    armed.store(false, Ordering::SeqCst);
    forward.pump_and_check();
    for probe in &mut fanout {
        probe.pump_and_check();
    }
    handle.remove_current_snack_bar();
    assert_eq!(next.get(), Some(SnackBarClosedReason::Remove));
    assert!(handle.shared.queue.borrow().is_empty());
    for probe in &mut fanout {
        probe.pump_and_check();
    }
    let fresh = Rc::new(Cell::new(false));
    let seen = Rc::clone(&fresh);
    handle
        .show_snack_bar(snack_bar("fresh"))
        .on_closed(move |_cx, _reason| seen.set(true));
    handle.remove_current_snack_bar();
    assert!(fresh.get(), "fresh queue operation remains live");
}
fn completion_alone() {
    exercise(true, false, 0);
}
fn forward_alone() {
    exercise(false, true, 0);
}
fn fanout_alone() {
    exercise(false, false, 1);
}
fn completion_then_forward() {
    exercise(true, true, 0);
}
fn completion_then_fanout() {
    exercise(true, false, 1);
}
fn forward_then_fanout() {
    exercise(false, true, 1);
}
fn fanout_then_fanout() {
    exercise(false, false, 2);
}
fn a_second_display_timer_retires_the_first() {
    use flui_sdk::animation::{Animation as _, MotionClock};

    struct RetiredCapture(Rc<Cell<usize>>);
    impl Drop for RetiredCapture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let (mut harness, handle) = mounted_handle();
    let closed = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&closed);
    handle
        .show_snack_bar(snack_bar("replacement").duration(Duration::from_millis(800)))
        .on_closed(move |_cx, reason| seen.borrow_mut().push(reason));
    handle
        .shared
        .entry_controller
        .set_value(1.0)
        .expect("no build is running");
    harness.tick();
    let registry = handle
        .shared
        .vsync
        .borrow()
        .clone()
        .expect("mounted registry");
    let first = handle
        .shared
        .duration_controller
        .borrow()
        .as_ref()
        .expect("first display timer")
        .controller()
        .clone();
    let first_run = first.forward().expect("first timer remains live");
    let retired_callbacks = Rc::new(Cell::new(0));
    first
        .subscribe_status(Rc::new({
            let capture = RetiredCapture(Rc::clone(&retired_callbacks));
            move |_| {
                let _ = &capture;
            }
        }))
        .detach();
    let seats = registry.len();
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(400)));
    assert!((first.value() - 0.5).abs() < 1e-9);
    let canceled = Rc::new(Cell::new(0));
    let seats_at_cancel = Rc::new(Cell::new(None));
    first_run.when_complete_or_cancel({
        let canceled = Rc::clone(&canceled);
        let seats_at_cancel = Rc::clone(&seats_at_cancel);
        let registry = registry.clone();
        let tick = clock.frame(Duration::from_millis(400));
        move |outcome| {
            assert!(outcome.is_err());
            seats_at_cancel.set(Some(registry.len()));
            registry.tick_all(&tick);
            canceled.set(canceled.get() + 1);
        }
    });
    // The public queue cancels before starting another entry. This private
    // mounted seam delivers a second ForwardSettled without that cancellation.
    handle
        .shared
        .handle_entry_status(AnimationStatus::ForwardSettled, ReconcileOrigin::Direct);
    assert!(
        first_run.is_canceled(),
        "replacement cancels the old accepted run"
    );
    assert!(
        first.forward().is_err(),
        "a saved observer cannot revive its retired owner"
    );
    assert!(!first.is_animating());
    assert_eq!(
        retired_callbacks.get(),
        1,
        "retirement releases status captures while an observer survives"
    );
    assert_eq!(
        canceled.get(),
        1,
        "cancellation can reenter the same registry once"
    );
    assert_eq!(
        seats_at_cancel.get(),
        Some(seats),
        "the old seat is withdrawn before cancellation"
    );
    assert_eq!(
        registry.len(),
        seats,
        "replacement retains one display registration"
    );
    registry.tick_all(&clock.frame(Duration::from_millis(400)));
    registry.tick_all(&clock.frame(Duration::from_millis(800)));
    harness.tick();
    assert!(
        closed.borrow().is_empty(),
        "the retired timer's old deadline cannot close the entry"
    );
    registry.tick_all(&clock.frame(Duration::from_millis(1200)));
    harness.tick();
    registry.tick_all(&clock.frame(Duration::from_millis(1200)));
    registry.tick_all(&clock.frame(Duration::from_millis(1450)));
    harness.tick();
    assert_eq!(&*closed.borrow(), &[SnackBarClosedReason::Timeout]);
    registry.tick_all(&clock.frame(Duration::from_secs(3)));
    harness.tick();
    assert_eq!(
        &*closed.borrow(),
        &[SnackBarClosedReason::Timeout],
        "no retired completion replay"
    );
    assert_eq!(canceled.get(), 1);
    drop(harness);
    assert!(
        registry.is_empty(),
        "unmount withdraws the remaining mounted owners"
    );
}

pub(super) fn run() {
    let mut failures = Vec::new();
    for (name, row) in [
        (
            "replacement retires the first display timer",
            a_second_display_timer_retires_the_first as fn(),
        ),
        ("completion alone", completion_alone as fn()),
        ("forward wake alone", forward_alone),
        ("fanout wake alone", fanout_alone),
        ("completion then forward wake", completion_then_forward),
        ("completion then fanout wake", completion_then_fanout),
        ("forward wake then fanout wake", forward_then_fanout),
        ("fanout wake then fanout wake", fanout_then_fanout),
    ] {
        if let Err(payload) = std::panic::catch_unwind(row) {
            failures.push(name);
            drop(payload);
        }
    }
    assert!(failures.is_empty(), "failed Messenger cases: {failures:?}");
}
