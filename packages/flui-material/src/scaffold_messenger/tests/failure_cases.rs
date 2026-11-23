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
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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
/// A probe on its own bare owner, built by hand: its wake hook is the
/// owner's `on_build_scheduled`, the seam a fanout delivery reaches.
struct MountedProbe {
    owner: flui_view::__runtime::BuildOwner,
    tree: flui_view::__runtime::ElementTree,
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
        let mut owner = flui_view::__runtime::BuildOwner::new();
        owner.set_on_build_scheduled(wake);
        let mut tree = flui_view::__runtime::ElementTree::new();
        let root = tree.mount_root(&view.boxed(), &mut owner.element_owner_mut());
        owner.schedule_build_for(root, 0, flui_sdk::view::RebuildReason::InitialMount);
        owner.build_scope(&mut tree);
        let handle = handle
            .borrow()
            .clone()
            .expect("lifecycle acquired mounted rebuild");
        Self {
            owner,
            tree,
            handle,
            builds,
        }
    }
    fn run_builds(&mut self) {
        self.owner.build_scope(&mut self.tree);
    }
    fn run_builds_and_check(&mut self) {
        let before = self.builds.get();
        self.run_builds();
        assert!(
            self.builds.get() > before,
            "accepted wake must rebuild its actual mounted element"
        );
    }
}

fn exercise(completion_fails: bool, forward_fails: bool, fanout_failures: usize) {
    let (_harness, handle, _driver) = mounted_handle();
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
        .listen_status(std::rc::Rc::new(move |status| {
            if status == AnimationStatus::Forward {
                forward_handle.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }
        }))
        .forget();
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
    // Independent owners can mint equal local ElementIds. The ids here only
    // identify fanout slots, so take two distinct slots from one owner.
    let root_id = fanout[1].tree.iter().next().expect("a mounted slot");
    handle
        .shared
        .scaffolds
        .borrow_mut()
        .push((root_id, fanout[0].handle.clone()));
    let second_id = fanout[1]
        .tree
        .iter()
        .find(|id| *id != root_id)
        .expect("distinct mounted slot");
    handle
        .shared
        .scaffolds
        .borrow_mut()
        .push((second_id, fanout[1].handle.clone()));
    let completions = Rc::new(Cell::new(0));
    let seen = Rc::clone(&completions);
    let arm = Arc::clone(&armed);
    handle
        .show_snack_bar(snack_bar("first"))
        .on_closed(move |_ctx, reason| {
            assert_eq!(reason, SnackBarClosedReason::Remove);
            seen.set(seen.get() + 1);
            arm.store(true, Ordering::SeqCst);
            assert!(!completion_fails, "completion failed");
        });
    let next = Rc::new(Cell::new(None));
    let seen = Rc::clone(&next);
    handle
        .show_snack_bar(snack_bar("next"))
        .on_closed(move |_ctx, reason| seen.set(Some(reason)));
    // Empty the independent inboxes after setup. Start the production advance
    // from its dismissed-entry boundary, without an earlier dismissal wake: the
    // status listener stays quiet while the setup settles the controller.
    {
        let _quiet = handle.shared.enter(Drain::Reconciling);
        handle
            .shared
            .entry_controller
            .set_value(0.0)
            .expect("no build is running");
    }
    forward.run_builds();
    for probe in &mut fanout {
        probe.run_builds();
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
    forward.run_builds_and_check();
    for probe in &mut fanout {
        probe.run_builds_and_check();
    }
    handle.remove_current_snack_bar();
    assert_eq!(next.get(), Some(SnackBarClosedReason::Remove));
    assert!(handle.shared.queue.borrow().is_empty());
    for probe in &mut fanout {
        probe.run_builds_and_check();
    }
    let fresh = Rc::new(Cell::new(false));
    let seen = Rc::clone(&fresh);
    handle
        .show_snack_bar(snack_bar("fresh"))
        .on_closed(move |_ctx, _reason| seen.set(true));
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
/// A display timer counts for one entry: a second settle of the same
/// entrance keeps the one task, removing the entry cancels it, and its old
/// deadline cannot close the next entry, which times out on its own.
fn a_display_timer_counts_for_one_entry() {
    let (mut harness, handle, _driver) = mounted_handle();
    let driver = handle
        .shared
        .clock
        .as_ref()
        .expect("a mounted messenger has its UI runtime's driver")
        .driver
        .clone();
    let closed = Rc::new(RefCell::new(Vec::new()));
    for label in ["first", "second"] {
        let seen = Rc::clone(&closed);
        handle
            .show_snack_bar(snack_bar(label).duration(Duration::from_millis(800)))
            .on_closed(move |_ctx, reason| seen.borrow_mut().push((label, reason)));
    }
    let tasks_before = driver.pending_task_count();
    // The entrance settles outside any queue operation, as a tick settles
    // it: the status listener starts the first entry's timer.
    handle
        .shared
        .entry_controller
        .set_value(1.0)
        .expect("no build is running");
    assert_eq!(driver.pending_task_count(), tasks_before + 1);
    assert!(
        !handle.shared.start_display_timer(),
        "a second settle of the same entrance starts no second timer"
    );
    assert_eq!(driver.pending_task_count(), tasks_before + 1);
    harness.pump_for(Duration::from_millis(400));
    assert!(closed.borrow().is_empty());

    handle.remove_current_snack_bar();
    assert_eq!(
        &*closed.borrow(),
        &[("first", SnackBarClosedReason::Remove)]
    );
    assert_eq!(
        driver.pending_task_count(),
        tasks_before,
        "removing the entry cancels its timer's task"
    );
    handle
        .shared
        .entry_controller
        .set_value(1.0)
        .expect("no build is running");
    assert_eq!(driver.pending_task_count(), tasks_before + 1);
    // Past the first entry's deadline, short of the second's.
    harness.pump_for(Duration::from_millis(500));
    assert_eq!(
        handle.shared.entry_controller.status(),
        AnimationStatus::ForwardSettled,
        "the removed entry's deadline cannot close the next entry"
    );
    harness.pump_for(Duration::from_millis(400));
    assert_eq!(
        handle.shared.entry_controller.status(),
        AnimationStatus::Reverse,
        "the second entry's own deadline hides it"
    );
    assert_eq!(driver.pending_task_count(), tasks_before);
    handle
        .shared
        .entry_controller
        .set_value(0.0)
        .expect("no build is running");
    // The exit settled as a tick settles it, but outside a frame: the frame
    // its completion runs after is asked for, as a ticked frame would run.
    harness
        .post_frame_handle()
        .schedule_frame()
        .expect("the messenger's presentation is alive");
    harness.tick();
    assert_eq!(
        &*closed.borrow(),
        &[
            ("first", SnackBarClosedReason::Remove),
            ("second", SnackBarClosedReason::Timeout)
        ]
    );
    assert!(handle.shared.queue.borrow().is_empty());
}

pub(super) fn run() {
    let mut failures = Vec::new();
    for (name, row) in [
        (
            "a display timer counts for one entry",
            a_display_timer_counts_for_one_entry as fn(),
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
            flui_sdk::foundation::panic::discard_payload(payload);
        }
    }
    assert!(failures.is_empty(), "failed Messenger cases: {failures:?}");
}
