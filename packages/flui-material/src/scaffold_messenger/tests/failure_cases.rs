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

#[derive(Clone, Copy)]
enum Secondary {
    None,
    Forward,
    Fanout(usize),
}
struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("secondary payload retired");
    }
}
struct Aggregate {
    _first: Bomb,
    _second: Bomb,
}
fn secondary_payload(drops: &Arc<AtomicUsize>) -> ! {
    std::panic::panic_any(Aggregate {
        _first: Bomb(Arc::clone(drops)),
        _second: Bomb(Arc::clone(drops)),
    });
}
fn exercise(completion_fails: bool, forward_fails: bool, fanout_failures: usize) {
    exercise_with_secondary(
        completion_fails,
        forward_fails,
        fanout_failures,
        Secondary::None,
    );
}
fn exercise_with_secondary(
    completion_fails: bool,
    forward_fails: bool,
    fanout_failures: usize,
    secondary: Secondary,
) {
    let (_harness, handle) = mounted_handle();
    let armed = Arc::new(AtomicBool::new(false));
    let forward_calls = Arc::new(AtomicUsize::new(0));
    let fanout_calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let forward_drops = Arc::clone(&drops);
    let forward_armed = Arc::clone(&armed);
    let seen = Arc::clone(&forward_calls);
    let mut forward = MountedProbe::new(move || {
        if forward_armed.load(Ordering::SeqCst) {
            seen.fetch_add(1, Ordering::SeqCst);
            if matches!(secondary, Secondary::Forward) {
                secondary_payload(&forward_drops);
            }
            assert!(!forward_fails, "forward wake failed");
        }
    });
    let forward_handle = forward.handle.clone();
    handle
        .shared
        .entry_controller
        .add_status_listener(std::rc::Rc::new(move |status| {
            if status == AnimationStatus::Forward {
                forward_handle.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }
        }));
    let mut fanout = Vec::new();
    for _ in 0..2 {
        let armed = Arc::clone(&armed);
        let seen = Arc::clone(&fanout_calls);
        let fanout_drops = Arc::clone(&drops);
        fanout.push(MountedProbe::new(move || {
            if armed.load(Ordering::SeqCst) {
                let call = seen.fetch_add(1, Ordering::SeqCst);
                if matches!(secondary, Secondary::Fanout(index) if index == call) {
                    secondary_payload(&fanout_drops);
                }
                if call < fanout_failures {
                    assert!(call != 0, "first fanout wake failed");
                    panic!("second fanout wake failed");
                }
            }
        }));
    }
    // Independent owners can mint equal local ElementIds. Map keys here only
    // identify fanout slots, so use distinct slots from the Messenger owner.
    let root_id = handle
        .shared
        .rebuild
        .borrow()
        .as_ref()
        .expect("attached")
        .element_id()
        .expect("mounted");
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
    // from its dismissed-entry boundary, without an earlier dismissal wake.
    handle.shared.entry_controller.set_value(0.0);
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
    if let Some(marker) = std::env::var_os("FLUI_MESSENGER_READY") {
        std::fs::write(marker, b"ready").expect("publish completed Messenger setup");
    }
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle.shared.pop_and_advance(ReconcileOrigin::Direct);
    }))
    .expect_err("configured failure must propagate");
    let expected = if completion_fails {
        "completion failed"
    } else if forward_fails {
        "forward wake failed"
    } else {
        "first fanout wake failed"
    };
    assert_eq!(payload.downcast_ref::<&str>(), Some(&expected));
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
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "secondary aggregate must remain opaque"
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
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "later progress must not retire secondary payload"
    );
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
fn completion_then_hostile_forward() {
    bounded_child("completion_forward");
}
fn completion_then_hostile_fanout() {
    bounded_child("completion_fanout");
}
fn forward_then_hostile_fanout() {
    bounded_child("forward_fanout");
}
fn fanout_then_hostile_fanout() {
    bounded_child("fanout_fanout");
}
fn bounded_child(case: &str) {
    let marker = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!(
            "flui-messenger-{}-{case}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
    let mut child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .arg("scaffold_messenger::tests::a_panicking_completion_does_not_lock_future_queue_operations")
        .arg("--exact").arg("--nocapture")
        .env("FLUI_MESSENGER_CHILD", case).env("FLUI_MESSENGER_READY", &marker)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().expect("spawn isolated Messenger row");
    let setup_started = std::time::Instant::now();
    let mut ready_at = None;
    let result = loop {
        if ready_at.is_none() && marker.exists() {
            ready_at = Some(std::time::Instant::now());
        }
        if let Some(status) = child.try_wait().expect("child status") {
            break status.success() && ready_at.is_some();
        }
        let timed_out = ready_at.map_or_else(
            || setup_started.elapsed() > Duration::from_secs(30),
            |ready: std::time::Instant| ready.elapsed() > Duration::from_secs(5),
        );
        if timed_out {
            child.kill().expect("terminate stalled Messenger child");
            let _ = child.wait().expect("reap stalled Messenger child");
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = std::fs::remove_file(marker);
    assert!(result, "Messenger child {case} aborted, failed, or stalled");
}
pub(super) fn run() {
    if let Ok(case) = std::env::var("FLUI_MESSENGER_CHILD") {
        match case.as_str() {
            "completion_forward" => exercise_with_secondary(true, false, 0, Secondary::Forward),
            "completion_fanout" => exercise_with_secondary(true, false, 0, Secondary::Fanout(0)),
            "forward_fanout" => exercise_with_secondary(false, true, 0, Secondary::Fanout(0)),
            "fanout_fanout" => exercise_with_secondary(false, false, 1, Secondary::Fanout(1)),
            _ => panic!("unknown Messenger child"),
        }
        return;
    }
    let mut failures = Vec::new();
    for (name, row) in [
        ("completion alone", completion_alone as fn()),
        ("forward wake alone", forward_alone),
        ("fanout wake alone", fanout_alone),
        ("completion then forward wake", completion_then_forward),
        ("completion then fanout wake", completion_then_fanout),
        ("forward wake then fanout wake", forward_then_fanout),
        ("fanout wake then fanout wake", fanout_then_fanout),
        (
            "completion then hostile forward payload",
            completion_then_hostile_forward,
        ),
        (
            "completion then hostile fanout payload",
            completion_then_hostile_fanout,
        ),
        (
            "forward wake then hostile fanout payload",
            forward_then_hostile_fanout,
        ),
        (
            "fanout wake then hostile fanout payload",
            fanout_then_hostile_fanout,
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(row) {
            failures.push(name);
            flui_sdk::foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failures.is_empty(), "failed Messenger cases: {failures:?}");
}
