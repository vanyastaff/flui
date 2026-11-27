use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_view::dev_reload::{CodeVersion, LayoutEpoch, ReloadEvent, Revision};

use super::*;

/// What the host reported to a scripted hook's `applied`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Applied {
    Reassembled(CodeVersion, UiRuntimeId),
    Restarted(CodeVersion),
    RestartFailed(CodeVersion),
}

/// What a scripted hook recorded, shared with the test that installed it.
#[derive(Default)]
struct Script {
    events: VecDeque<ReloadEvent>,
    /// Answered by the next `reassemble_requested`.
    rebuild: bool,
    attaches: usize,
    detaches: usize,
    polls: usize,
    applied: Vec<Applied>,
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

    fn applied(&self) -> Vec<Applied> {
        std::mem::take(&mut self.0.lock().applied)
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

    fn reassemble_requested(&mut self) -> bool {
        std::mem::take(&mut self.0.lock().rebuild)
    }

    fn applied(&mut self, outcome: &ReloadOutcome<'_>) {
        let applied = match *outcome {
            ReloadOutcome::Reassembled {
                version, runtime, ..
            } => Applied::Reassembled(version, runtime),
            ReloadOutcome::Restarted { version, .. } => Applied::Restarted(version),
            ReloadOutcome::RestartFailed {
                version,
                ref reason,
                ..
            } => {
                assert!(
                    matches!(reason, RestartFailure::NoApplication),
                    "a host that cannot restart composes no application: {reason:?}"
                );
                Applied::RestartFailed(version)
            }
            _ => unreachable!("a test host reports no other outcome"),
        };
        self.0.lock().applied.push(applied);
    }
}

/// A hook that panics in the named call, and optionally in its `Drop` too.
#[derive(Default)]
struct Panics {
    in_attach: bool,
    in_poll: bool,
    in_applied: bool,
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
        ReloadEvent::Loaded(version(1, 1))
    }

    fn applied(&mut self, _outcome: &ReloadOutcome<'_>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!self.in_applied, "applied panics");
    }
}

impl Drop for Panics {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.in_drop, "the hook's own Drop panics");
    }
}

fn version(layout: u64, revision: u64) -> CodeVersion {
    CodeVersion::new(
        LayoutEpoch::try_new(layout).expect("a test layout is not zero"),
        Revision::try_new(revision).expect("a test revision is not zero"),
    )
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

/// A settled UI runtime the runner registered with `reload`, as each runner
/// does right after the root attaches.
fn registered(reload: &WorkerReload) -> (UiRuntime, RuntimeReload) {
    let ui_runtime = settled_ui_runtime();
    let handle = reload.register_ui_runtime(&ui_runtime);
    (ui_runtime, handle)
}

fn frame_boundary(handle: &RuntimeReload, ui_runtime: &UiRuntime) {
    ui_runtime.enter(|ui_runtime| handle.poll_and_apply(ui_runtime));
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

fn installed(hook: &Scripted) -> WorkerReload {
    WorkerReload::from_config(&AppConfig::new().dev_reload(hook.clone()))
}

fn a_version_polled_through_the_hook_reassembles_the_ui_runtime() {
    let hook = Scripted::default();
    let reload = installed(&hook);
    let (ui_runtime, handle) = registered(&reload);

    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&handle, &ui_runtime);

    assert_eq!(
        hook.polls(),
        1,
        "the frame boundary polled the installed hook"
    );
    assert!(
        ui_runtime.widgets().has_pending_builds(),
        "the first version is the one the application was composed from, and the ui_runtime \
         reassembles to it: every element dirty"
    );
}

fn without_a_hook_the_frame_is_unchanged() {
    let reload = WorkerReload::from_config(&AppConfig::new());
    let (ui_runtime, handle) = registered(&reload);
    frame_boundary(&handle, &ui_runtime);
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "no hook, no reassemble"
    );
    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "no hook, nothing to attach"
    );

    let hook = Scripted::default();
    let reload = installed(&hook);
    let (ui_runtime, handle) = registered(&reload);
    frame_boundary(&handle, &ui_runtime);
    assert_eq!(hook.polls(), 1);
    assert!(
        !ui_runtime.widgets().has_pending_builds(),
        "an Unchanged poll reassembles nothing"
    );
}

fn one_version_reaches_every_ui_runtime_exactly_once() {
    let hook = Scripted::default();
    let config = AppConfig::new().dev_reload(hook.clone());
    // Two windows opened from clones of one configuration, as the runners do.
    let (reload_a, reload_b) = (
        WorkerReload::from_config(&config.clone()),
        WorkerReload::from_config(&config),
    );
    let ((a, handle_a), (b, handle_b)) = (registered(&reload_a), registered(&reload_b));
    assert_ne!(a.id(), b.id());

    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&handle_a, &a);
    assert!(
        a.widgets().has_pending_builds(),
        "the polling ui_runtime applies it"
    );
    assert!(
        !b.widgets().has_pending_builds(),
        "the other ui_runtime applies it at its own boundary, not before"
    );
    frame_boundary(&handle_b, &b);
    assert!(
        b.widgets().has_pending_builds(),
        "the version the first ui_runtime's poll consumed must still reach the second"
    );

    drain(&a);
    drain(&b);
    frame_boundary(&handle_a, &a);
    frame_boundary(&handle_b, &b);
    assert!(
        !a.widgets().has_pending_builds() && !b.widgets().has_pending_builds(),
        "a version is applied once per ui_runtime, not again at the next boundary"
    );
    assert_eq!(hook.polls(), 4, "every boundary polls");
}

fn a_ui_runtime_mounted_after_a_version_does_not_replay_it() {
    let hook = Scripted::default();
    let reload = installed(&hook);
    let (older, older_handle) = registered(&reload);
    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&older_handle, &older);
    assert!(older.widgets().has_pending_builds());

    let (newer, newer_handle) = registered(&reload);
    frame_boundary(&newer_handle, &newer);
    assert!(
        !newer.widgets().has_pending_builds(),
        "a ui_runtime mounted after the version was built from it already"
    );
}

fn a_ui_runtime_mounted_before_a_version_applies_it_at_its_first_boundary() {
    let hook = Scripted::default();
    let config = AppConfig::new().dev_reload(hook.clone());
    let (reload_main, reload_second) = (
        WorkerReload::from_config(&config.clone()),
        WorkerReload::from_config(&config),
    );
    let (main, main_handle) = registered(&reload_main);
    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&main_handle, &main);
    drain(&main);

    // A secondary window mounts its tree from the code loaded now...
    let (second, second_handle) = registered(&reload_second);
    // ...and the main window's next boundary takes a version before the
    // secondary window has produced a frame.
    hook.then(ReloadEvent::Loaded(version(1, 2)));
    frame_boundary(&main_handle, &main);
    assert!(main.widgets().has_pending_builds());

    frame_boundary(&second_handle, &second);
    assert!(
        second.widgets().has_pending_builds(),
        "a ui_runtime built before the version must reassemble at its first boundary"
    );
}

/// Two windows that ran version 1, and the hook's record of it cleared.
fn two_windows_on_the_first_version(
    hook: &Scripted,
) -> ((UiRuntime, RuntimeReload), (UiRuntime, RuntimeReload)) {
    let reload = installed(hook);
    let ((a, handle_a), (b, handle_b)) = (registered(&reload), registered(&reload));
    hook.then(ReloadEvent::Loaded(version(1, 1)));
    for (ui_runtime, handle) in [(&a, &handle_a), (&b, &handle_b)] {
        frame_boundary(handle, ui_runtime);
        drain(ui_runtime);
    }
    hook.applied();
    ((a, handle_a), (b, handle_b))
}

fn a_window_that_draws_late_reassembles_to_the_latest_revision() {
    let hook = Scripted::default();
    let ((a, handle_a), (b, handle_b)) = two_windows_on_the_first_version(&hook);

    hook.then(ReloadEvent::Loaded(version(1, 2)));
    frame_boundary(&handle_a, &a);
    drain(&a);
    hook.then(ReloadEvent::Loaded(version(1, 3)));
    frame_boundary(&handle_a, &a);
    drain(&a);

    frame_boundary(&handle_b, &b);
    assert!(
        b.widgets().has_pending_builds(),
        "the idle window reassembles when it draws"
    );
    assert_eq!(
        hook.applied(),
        [
            Applied::Reassembled(version(1, 2), a.id()),
            Applied::Reassembled(version(1, 3), a.id()),
            Applied::Reassembled(version(1, 3), b.id()),
        ],
        "to the latest revision, not to the one it missed"
    );
}

fn two_versions_before_a_boundary_apply_once() {
    let hook = Scripted::default();
    let ((a, handle_a), (b, handle_b)) = two_windows_on_the_first_version(&hook);

    hook.then(ReloadEvent::Loaded(version(1, 2)));
    frame_boundary(&handle_a, &a);
    hook.then(ReloadEvent::Loaded(version(1, 3)));
    frame_boundary(&handle_a, &a);

    frame_boundary(&handle_b, &b);
    drain(&b);
    frame_boundary(&handle_b, &b);
    assert!(
        !b.widgets().has_pending_builds(),
        "the second version the window missed is no second reassemble"
    );
    let at_b = hook
        .applied()
        .into_iter()
        .filter(|applied| matches!(applied, Applied::Reassembled(_, runtime) if *runtime == b.id()))
        .count();
    assert_eq!(at_b, 1, "one reassemble for both versions");
}

fn a_version_reported_twice_changes_nothing() {
    let hook = Scripted::default();
    let ((a, handle_a), _b) = two_windows_on_the_first_version(&hook);

    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&handle_a, &a);
    assert!(
        !a.widgets().has_pending_builds(),
        "the version the window runs is reported again"
    );
    assert_eq!(hook.applied(), [], "and nothing is reported back");
}

fn a_changed_layout_reports_a_failed_restart_and_nothing_reassembles() {
    let hook = Scripted::default();
    let ((a, handle_a), (b, handle_b)) = two_windows_on_the_first_version(&hook);

    hook.then(ReloadEvent::Loaded(version(2, 2)));
    frame_boundary(&handle_a, &a);
    frame_boundary(&handle_b, &b);
    assert!(
        !a.widgets().has_pending_builds() && !b.widgets().has_pending_builds(),
        "a version of another layout never reassembles"
    );
    assert_eq!(
        hook.applied(),
        [Applied::RestartFailed(version(2, 2))],
        "this host cannot restart yet, and says so once"
    );
    hook.then(ReloadEvent::Loaded(version(2, 2)));
    frame_boundary(&handle_a, &a);
    assert_eq!(
        hook.applied(),
        [],
        "a failed restart is not tried again for the same version"
    );

    hook.then(ReloadEvent::Loaded(version(1, 3)));
    frame_boundary(&handle_a, &a);
    assert!(
        !a.widgets().has_pending_builds(),
        "after a failed restart no application runs the composed layout"
    );
    assert_eq!(hook.applied(), [Applied::RestartFailed(version(1, 3))]);
}

fn each_reassembled_runtime_and_each_restart_is_reported_once() {
    let hook = Scripted::default();
    let reload = installed(&hook);
    let ((a, handle_a), (b, handle_b)) = (registered(&reload), registered(&reload));
    hook.then(ReloadEvent::Loaded(version(1, 1)));
    for _ in 0..2 {
        frame_boundary(&handle_a, &a);
        frame_boundary(&handle_b, &b);
    }
    hook.then(ReloadEvent::Loaded(version(2, 2)));
    for _ in 0..2 {
        frame_boundary(&handle_a, &a);
        frame_boundary(&handle_b, &b);
    }
    assert_eq!(
        hook.applied(),
        [
            Applied::Reassembled(version(1, 1), a.id()),
            Applied::Reassembled(version(1, 1), b.id()),
            Applied::RestartFailed(version(2, 2)),
        ]
    );
    assert!(
        !hook.applied().contains(&Applied::Restarted(version(2, 2))),
        "a restart this host did not do is not reported as one"
    );
}

fn a_rebuild_the_loaded_code_asks_for_reassembles_without_a_report() {
    let hook = Scripted::default();
    let ((a, handle_a), (b, handle_b)) = two_windows_on_the_first_version(&hook);

    hook.0.lock().rebuild = true;
    frame_boundary(&handle_a, &a);
    assert!(
        a.widgets().has_pending_builds(),
        "the polling window rebuilds"
    );
    frame_boundary(&handle_b, &b);
    assert!(
        b.widgets().has_pending_builds(),
        "and the other one at its boundary"
    );
    drain(&a);
    frame_boundary(&handle_a, &a);
    assert!(!a.widgets().has_pending_builds(), "once");
    assert_eq!(hook.applied(), [], "no version was delivered");
}

fn a_retired_ui_runtime_leaves_the_record() {
    let hook = Scripted::default();
    let reload = installed(&hook);
    let (ui_runtime, handle) = registered(&reload);
    let record_holds = |id| {
        reload
            .0
            .as_ref()
            .expect("the hook is installed")
            .0
            .lock()
            .record
            .holds(id)
    };
    assert!(record_holds(ui_runtime.id()));
    drop(handle);
    assert!(!record_holds(ui_runtime.id()), "the window closed");
}

fn a_window_kept_across_loops_reassembles_to_the_next_loops_first_version() {
    let hook = Scripted::default();
    let reload = installed(&hook);
    let (ui_runtime, handle) = registered(&reload);
    let first_loop = reload.spawn_watcher(no_wake()).expect("attaches");
    hook.then(ReloadEvent::Loaded(version(1, 1)));
    frame_boundary(&handle, &ui_runtime);
    drain(&ui_runtime);
    drop(first_loop);

    let _second_loop = reload.spawn_watcher(no_wake()).expect("attaches again");
    hook.then(ReloadEvent::Loaded(version(1, 2)));
    frame_boundary(&handle, &ui_runtime);
    assert!(
        ui_runtime.widgets().has_pending_builds(),
        "the window was built from code older than the new loop's first version"
    );
}

fn a_panicking_applied_retires_the_hook() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().dev_reload(Panics {
        in_applied: true,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
        ..Panics::default()
    }));
    let (ui_runtime, handle) = registered(&reload);

    frame_boundary(&handle, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "polled, then told");
    assert_eq!(drops.load(Ordering::SeqCst), 1, "the hook is retired");
    assert!(
        ui_runtime.widgets().has_pending_builds(),
        "the reassemble the report was about stands"
    );
    drain(&ui_runtime);

    frame_boundary(&handle, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "never called again");
}

fn a_panicking_poll_disables_the_hook_and_the_frame_continues() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().dev_reload(Panics {
        in_poll: true,
        in_drop: true,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
        ..Panics::default()
    }));
    let (ui_runtime, handle) = registered(&reload);

    frame_boundary(&handle, &ui_runtime);
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

    frame_boundary(&handle, &ui_runtime);
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
    let reload = WorkerReload::from_config(&AppConfig::new().dev_reload(hook.clone()));
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
    let reload = WorkerReload::from_config(&AppConfig::new().dev_reload(hook.clone()));
    let guard = Arc::new(Mutex::new(Some(
        reload.spawn_watcher(no_wake()).expect("attaches"),
    )));
    let dropped_inside = Arc::clone(&guard);
    hook.0.lock().during_poll = Some(Box::new(move || {
        let guard = dropped_inside.lock().take();
        drop(guard);
    }));
    let (ui_runtime, handle) = registered(&reload);

    frame_boundary(&handle, &ui_runtime);
    assert_eq!(
        hook.0.lock().detaches,
        1,
        "the owed detach ran after the poll"
    );
    assert!(guard.lock().is_none());

    frame_boundary(&handle, &ui_runtime);
    assert_eq!(hook.polls(), 2, "the hook went back into its slot");
    assert!(
        reload.spawn_watcher(no_wake()).is_some(),
        "and can be attached by the next loop"
    );
}

fn a_panicking_attach_leaves_the_loop_without_reload() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().dev_reload(Panics {
        in_attach: true,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
        ..Panics::default()
    }));

    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "the panic is contained"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1, "the hook is gone");

    let (ui_runtime, handle) = registered(&reload);
    frame_boundary(&handle, &ui_runtime);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never polled");
    assert!(!ui_runtime.widgets().has_pending_builds());
    assert!(reload.spawn_watcher(no_wake()).is_none());
}

#[test]
fn dev_reload_hook_matrix() {
    crate::table_test::run_table(
        "dev_reload_hook_matrix",
        &[
            (
                "a_version_polled_through_the_hook_reassembles_the_ui_runtime",
                a_version_polled_through_the_hook_reassembles_the_ui_runtime as fn(),
            ),
            (
                "without_a_hook_the_frame_is_unchanged",
                without_a_hook_the_frame_is_unchanged as fn(),
            ),
            (
                "one_version_reaches_every_ui_runtime_exactly_once",
                one_version_reaches_every_ui_runtime_exactly_once as fn(),
            ),
            (
                "a_ui_runtime_mounted_after_a_version_does_not_replay_it",
                a_ui_runtime_mounted_after_a_version_does_not_replay_it as fn(),
            ),
            (
                "a_ui_runtime_mounted_before_a_version_applies_it_at_its_first_boundary",
                a_ui_runtime_mounted_before_a_version_applies_it_at_its_first_boundary as fn(),
            ),
            (
                "a_window_that_draws_late_reassembles_to_the_latest_revision",
                a_window_that_draws_late_reassembles_to_the_latest_revision as fn(),
            ),
            (
                "two_versions_before_a_boundary_apply_once",
                two_versions_before_a_boundary_apply_once as fn(),
            ),
            (
                "a_version_reported_twice_changes_nothing",
                a_version_reported_twice_changes_nothing as fn(),
            ),
            (
                "a_changed_layout_reports_a_failed_restart_and_nothing_reassembles",
                a_changed_layout_reports_a_failed_restart_and_nothing_reassembles as fn(),
            ),
            (
                "each_reassembled_runtime_and_each_restart_is_reported_once",
                each_reassembled_runtime_and_each_restart_is_reported_once as fn(),
            ),
            (
                "a_rebuild_the_loaded_code_asks_for_reassembles_without_a_report",
                a_rebuild_the_loaded_code_asks_for_reassembles_without_a_report as fn(),
            ),
            (
                "a_retired_ui_runtime_leaves_the_record",
                a_retired_ui_runtime_leaves_the_record as fn(),
            ),
            (
                "a_window_kept_across_loops_reassembles_to_the_next_loops_first_version",
                a_window_kept_across_loops_reassembles_to_the_next_loops_first_version as fn(),
            ),
            (
                "a_panicking_applied_retires_the_hook",
                a_panicking_applied_retires_the_hook as fn(),
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
        ],
    );
}
