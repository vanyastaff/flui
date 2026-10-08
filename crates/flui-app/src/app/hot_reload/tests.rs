use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;

use super::*;

/// What a scripted hook recorded, shared with the test that installed it.
#[derive(Default)]
struct Script {
    events: VecDeque<ReloadEvent>,
    attaches: usize,
    detaches: usize,
    polls: usize,
    wake: Option<ReloadWake>,
    /// Run inside the next `poll`, standing in for a hook that re-enters the
    /// host while it is lent.
    during_poll: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<Script>>);

impl Scripted {
    fn then(&self, event: ReloadEvent) {
        self.0.lock().events.push_back(event);
    }

    fn polls(&self) -> usize {
        self.0.lock().polls
    }
}

impl DevReloadHook for Scripted {
    fn attach(&mut self, wake: ReloadWake) {
        let mut script = self.0.lock();
        script.attaches += 1;
        script.wake = Some(wake);
    }

    fn detach(&mut self) {
        let mut script = self.0.lock();
        script.detaches += 1;
        script.wake = None;
    }

    fn poll(&mut self) -> ReloadEvent {
        let (event, during) = {
            let mut script = self.0.lock();
            script.polls += 1;
            (
                script.events.pop_front().unwrap_or(ReloadEvent::Unchanged),
                script.during_poll.take(),
            )
        };
        if let Some(during) = during {
            during();
        }
        event
    }
}

/// A hook that panics in the named call, and optionally in its `Drop` too.
struct Panics {
    in_attach: bool,
    in_poll: bool,
    in_drop: bool,
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl DevReloadHook for Panics {
    fn attach(&mut self, _wake: ReloadWake) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!self.in_attach, "attach panics");
    }

    fn poll(&mut self) -> ReloadEvent {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!self.in_poll, "poll panics");
        ReloadEvent::Patched
    }
}

impl Drop for Panics {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.in_drop, "the hook's own Drop panics");
    }
}

/// A UI runtime with a mounted root whose initial build is drained, so a pending
/// build afterwards can only come from a reassemble.
fn settled_ui_runtime() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("the root mounts");
    let _ = ui_runtime.draw_frame(BoxConstraints::tight(Size::new(50.0, 50.0)));
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "precondition: the initial build is drained"
    );
    ui_runtime
}

fn frame_boundary(reload: &WorkerReload, ui_runtime: &UiRuntime) {
    ui_runtime.enter(|ui_runtime| reload.poll_and_apply(ui_runtime));
}

fn drain(ui_runtime: &UiRuntime) {
    let _ = ui_runtime.draw_frame(BoxConstraints::tight(Size::new(50.0, 50.0)));
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "the reassemble drained"
    );
}

fn no_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

fn a_patch_polled_through_the_hook_reassembles_the_ui_runtime() {
    let hook = Scripted::default();
    hook.then(ReloadEvent::Patched);
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let ui_runtime = settled_ui_runtime();

    frame_boundary(&reload, &ui_runtime);

    assert_eq!(
        hook.polls(),
        1,
        "the frame boundary polled the installed hook"
    );
    assert!(
        ui_runtime.widgets().has_pending_builds(),
        "a Patched poll must reassemble the ui_runtime: every element dirty"
    );
}

fn without_a_hook_the_frame_is_unchanged() {
    let ui_runtime = settled_ui_runtime();
    frame_boundary(&WorkerReload::from_config(&AppConfig::new()), &ui_runtime);
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "no hook, no reassemble"
    );
    assert!(
        WorkerReload::from_config(&AppConfig::new())
            .spawn_watcher(no_wake())
            .is_none(),
        "no hook, nothing to attach"
    );

    let hook = Scripted::default();
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    frame_boundary(&reload, &ui_runtime);
    assert_eq!(hook.polls(), 1);
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "an Unchanged poll reassembles nothing"
    );
}

fn one_patch_reaches_every_ui_runtime_exactly_once() {
    let hook = Scripted::default();
    let config = AppConfig::new().with_dev_reload(hook.clone());
    // Two windows opened from clones of one configuration, as the runners do.
    let (reload_a, reload_b) = (
        WorkerReload::from_config(&config.clone()),
        WorkerReload::from_config(&config),
    );
    let (a, b) = (settled_ui_runtime(), settled_ui_runtime());
    assert_ne!(a.id(), b.id());
    frame_boundary(&reload_a, &a);
    frame_boundary(&reload_b, &b);

    hook.then(ReloadEvent::Patched);
    frame_boundary(&reload_a, &a);
    assert!(
        a.widgets().has_pending_builds(),
        "the polling ui_runtime applies it"
    );
    assert!(
        !b.widgets().has_pending_builds(),
        "the other ui_runtime applies it at its own boundary, not before"
    );
    frame_boundary(&reload_b, &b);
    assert!(
        b.widgets().has_pending_builds(),
        "the patch the first ui_runtime's poll consumed must still reach the second"
    );

    drain(&a);
    drain(&b);
    frame_boundary(&reload_a, &a);
    frame_boundary(&reload_b, &b);
    assert!(
        !a.widgets().has_pending_builds() && !b.widgets().has_pending_builds(),
        "a patch is applied once per ui_runtime, not again at the next boundary"
    );
    assert_eq!(hook.polls(), 6, "every boundary polls");
}

fn a_ui_runtime_first_polled_after_a_patch_does_not_replay_it() {
    let hook = Scripted::default();
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let older = settled_ui_runtime();
    hook.then(ReloadEvent::Patched);
    frame_boundary(&reload, &older);
    assert!(older.widgets().has_pending_builds());

    let newer = settled_ui_runtime();
    frame_boundary(&reload, &newer);
    assert!(
        !newer.widgets().has_pending_builds(),
        "a ui_runtime created after the patch was built from the patched code already"
    );
}

fn a_ui_runtime_mounted_before_a_patch_applies_it_at_its_first_boundary() {
    let hook = Scripted::default();
    let config = AppConfig::new().with_dev_reload(hook.clone());
    let (reload_main, reload_second) = (
        WorkerReload::from_config(&config.clone()),
        WorkerReload::from_config(&config),
    );
    let main = settled_ui_runtime();
    reload_main.register_ui_runtime(&main);
    frame_boundary(&reload_main, &main);

    // A secondary window mounts its tree from the code loaded now...
    let second = settled_ui_runtime();
    reload_second.register_ui_runtime(&second);
    // ...and the main window's next boundary takes a patch before the
    // secondary window has produced a frame.
    hook.then(ReloadEvent::Patched);
    frame_boundary(&reload_main, &main);
    assert!(main.widgets().has_pending_builds());

    frame_boundary(&reload_second, &second);
    assert!(
        second.widgets().has_pending_builds(),
        "a ui_runtime built before the patch must reassemble at its first boundary"
    );
}

fn a_panicking_poll_disables_the_hook_and_the_frame_continues() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(Panics {
        in_attach: false,
        in_poll: true,
        in_drop: true,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
    }));
    let ui_runtime = settled_ui_runtime();

    frame_boundary(&reload, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "polled once");
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the panicking hook is dropped, and its panicking Drop is contained"
    );
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "the ui_runtime is untouched"
    );

    frame_boundary(&reload, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never polled again");
    assert_eq!(drops.load(Ordering::SeqCst), 1, "never dropped twice");
    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "a disabled hook is not attached"
    );
    drain(&ui_runtime);
}

fn attach_and_detach_pair_once_per_loop_and_a_second_attach_is_refused() {
    let hook = Scripted::default();
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let guard = reload
        .spawn_watcher(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("the installed hook attaches");
    assert_eq!(hook.0.lock().attaches, 1);

    let wake = hook.0.lock().wake.clone().expect("the hook kept its wake");
    wake.wake();
    assert_eq!(wakes.load(Ordering::SeqCst), 1, "the wake reaches the host");

    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "a second attach while the first loop runs is refused"
    );
    assert_eq!(hook.0.lock().attaches, 1, "the hook saw one attach");
    assert_eq!(hook.0.lock().detaches, 0);

    drop(guard);
    assert_eq!(hook.0.lock().detaches, 1, "dropping the guard detaches");

    let next_loop = reload
        .spawn_watcher(no_wake())
        .expect("a later loop attaches");
    assert_eq!(hook.0.lock().attaches, 2);
    drop(next_loop);
    assert_eq!(hook.0.lock().detaches, 2);
}

fn a_detach_that_arrives_during_a_hook_call_runs_when_the_call_returns() {
    let hook = Scripted::default();
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let guard = Arc::new(Mutex::new(Some(
        reload.spawn_watcher(no_wake()).expect("attaches"),
    )));
    let dropped_inside = Arc::clone(&guard);
    hook.0.lock().during_poll = Some(Box::new(move || {
        let guard = dropped_inside.lock().take();
        drop(guard);
    }));
    let ui_runtime = settled_ui_runtime();

    frame_boundary(&reload, &ui_runtime);
    assert_eq!(
        hook.0.lock().detaches,
        1,
        "the owed detach ran after the poll"
    );
    assert!(guard.lock().is_none());

    frame_boundary(&reload, &ui_runtime);
    assert_eq!(hook.polls(), 2, "the hook went back into its slot");
    assert!(
        reload.spawn_watcher(no_wake()).is_some(),
        "and can be attached by the next loop"
    );
}

fn a_panicking_attach_leaves_the_loop_without_reload() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(Panics {
        in_attach: true,
        in_poll: false,
        in_drop: false,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
    }));

    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "the panic is contained"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1, "the hook is gone");

    let ui_runtime = settled_ui_runtime();
    frame_boundary(&reload, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never polled");
    assert!(!ui_runtime.widgets().has_pending_builds());
    assert!(reload.spawn_watcher(no_wake()).is_none());
}

fn each_reload_event_maps_to_the_ui_runtime_tier() {
    assert_eq!(reload_tier(ReloadEvent::Unchanged), None);
    assert_eq!(
        reload_tier(ReloadEvent::Patched),
        Some(ReloadTier::Reassemble)
    );
}

#[test]
fn dev_reload_hook_matrix() {
    crate::table_test::run_table(
        "dev_reload_hook_matrix",
        &[
            (
                "a_patch_polled_through_the_hook_reassembles_the_ui_runtime",
                a_patch_polled_through_the_hook_reassembles_the_ui_runtime as fn(),
            ),
            (
                "without_a_hook_the_frame_is_unchanged",
                without_a_hook_the_frame_is_unchanged as fn(),
            ),
            (
                "one_patch_reaches_every_ui_runtime_exactly_once",
                one_patch_reaches_every_ui_runtime_exactly_once as fn(),
            ),
            (
                "a_ui_runtime_first_polled_after_a_patch_does_not_replay_it",
                a_ui_runtime_first_polled_after_a_patch_does_not_replay_it as fn(),
            ),
            (
                "a_ui_runtime_mounted_before_a_patch_applies_it_at_its_first_boundary",
                a_ui_runtime_mounted_before_a_patch_applies_it_at_its_first_boundary as fn(),
            ),
            (
                "a_panicking_poll_disables_the_hook_and_the_frame_continues",
                a_panicking_poll_disables_the_hook_and_the_frame_continues as fn(),
            ),
            (
                "attach_and_detach_pair_once_per_loop_and_a_second_attach_is_refused",
                attach_and_detach_pair_once_per_loop_and_a_second_attach_is_refused as fn(),
            ),
            (
                "a_detach_that_arrives_during_a_hook_call_runs_when_the_call_returns",
                a_detach_that_arrives_during_a_hook_call_runs_when_the_call_returns as fn(),
            ),
            (
                "a_panicking_attach_leaves_the_loop_without_reload",
                a_panicking_attach_leaves_the_loop_without_reload as fn(),
            ),
            (
                "each_reload_event_maps_to_the_ui_runtime_tier",
                each_reload_event_maps_to_the_ui_runtime_tier as fn(),
            ),
        ],
    );
}
