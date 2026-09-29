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

/// A realm with a mounted root whose initial build is drained, so a pending
/// build afterwards can only come from a reassemble.
fn settled_realm() -> UiRealm {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("the root mounts");
    let _ = realm.draw_frame(BoxConstraints::tight(Size::new(50.0, 50.0)));
    assert!(
        !realm.widgets().has_pending_builds(),
        "precondition: the initial build is drained"
    );
    realm
}

fn frame_boundary(reload: &WorkerReload, realm: &UiRealm) {
    realm.enter(|realm| reload.poll_and_apply(realm));
}

fn drain(realm: &UiRealm) {
    let _ = realm.draw_frame(BoxConstraints::tight(Size::new(50.0, 50.0)));
    assert!(
        !realm.widgets().has_pending_builds(),
        "the reassemble drained"
    );
}

fn no_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

#[test]
fn a_patch_polled_through_the_hook_reassembles_the_realm() {
    let hook = Scripted::default();
    hook.then(ReloadEvent::Patched);
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let realm = settled_realm();

    frame_boundary(&reload, &realm);

    assert_eq!(
        hook.polls(),
        1,
        "the frame boundary polled the installed hook"
    );
    assert!(
        realm.widgets().has_pending_builds(),
        "a Patched poll must reassemble the realm: every element dirty"
    );
}

#[test]
fn without_a_hook_the_frame_is_unchanged() {
    let realm = settled_realm();
    frame_boundary(&WorkerReload::from_config(&AppConfig::new()), &realm);
    assert!(
        !realm.widgets().has_pending_builds(),
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
    frame_boundary(&reload, &realm);
    assert_eq!(hook.polls(), 1);
    assert!(
        !realm.widgets().has_pending_builds(),
        "an Unchanged poll reassembles nothing"
    );
}

#[test]
fn one_patch_reaches_every_realm_exactly_once() {
    let hook = Scripted::default();
    let config = AppConfig::new().with_dev_reload(hook.clone());
    // Two windows opened from clones of one configuration, as the runners do.
    let (reload_a, reload_b) = (
        WorkerReload::from_config(&config.clone()),
        WorkerReload::from_config(&config),
    );
    let (a, b) = (settled_realm(), settled_realm());
    assert_ne!(a.realm_id(), b.realm_id());
    frame_boundary(&reload_a, &a);
    frame_boundary(&reload_b, &b);

    hook.then(ReloadEvent::Patched);
    frame_boundary(&reload_a, &a);
    assert!(
        a.widgets().has_pending_builds(),
        "the polling realm applies it"
    );
    assert!(
        !b.widgets().has_pending_builds(),
        "the other realm applies it at its own boundary, not before"
    );
    frame_boundary(&reload_b, &b);
    assert!(
        b.widgets().has_pending_builds(),
        "the patch the first realm's poll consumed must still reach the second"
    );

    drain(&a);
    drain(&b);
    frame_boundary(&reload_a, &a);
    frame_boundary(&reload_b, &b);
    assert!(
        !a.widgets().has_pending_builds() && !b.widgets().has_pending_builds(),
        "a patch is applied once per realm, not again at the next boundary"
    );
    assert_eq!(hook.polls(), 6, "every boundary polls");
}

#[test]
fn a_realm_first_polled_after_a_patch_does_not_replay_it() {
    let hook = Scripted::default();
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(hook.clone()));
    let older = settled_realm();
    hook.then(ReloadEvent::Patched);
    frame_boundary(&reload, &older);
    assert!(older.widgets().has_pending_builds());

    let newer = settled_realm();
    frame_boundary(&reload, &newer);
    assert!(
        !newer.widgets().has_pending_builds(),
        "a realm created after the patch was built from the patched code already"
    );
}

#[test]
fn a_panicking_poll_disables_the_hook_and_the_frame_continues() {
    let (calls, drops) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let reload = WorkerReload::from_config(&AppConfig::new().with_dev_reload(Panics {
        in_attach: false,
        in_poll: true,
        in_drop: true,
        calls: Arc::clone(&calls),
        drops: Arc::clone(&drops),
    }));
    let realm = settled_realm();

    frame_boundary(&reload, &realm);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "polled once");
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the panicking hook is dropped, and its panicking Drop is contained"
    );
    assert!(
        !realm.widgets().has_pending_builds(),
        "the realm is untouched"
    );

    frame_boundary(&reload, &realm);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never polled again");
    assert_eq!(drops.load(Ordering::SeqCst), 1, "never dropped twice");
    assert!(
        reload.spawn_watcher(no_wake()).is_none(),
        "a disabled hook is not attached"
    );
    drain(&realm);
}

#[test]
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

#[test]
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
    let realm = settled_realm();

    frame_boundary(&reload, &realm);
    assert_eq!(
        hook.0.lock().detaches,
        1,
        "the owed detach ran after the poll"
    );
    assert!(guard.lock().is_none());

    frame_boundary(&reload, &realm);
    assert_eq!(hook.polls(), 2, "the hook went back into its slot");
    assert!(
        reload.spawn_watcher(no_wake()).is_some(),
        "and can be attached by the next loop"
    );
}

#[test]
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

    let realm = settled_realm();
    frame_boundary(&reload, &realm);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never polled");
    assert!(!realm.widgets().has_pending_builds());
    assert!(reload.spawn_watcher(no_wake()).is_none());
}

#[test]
fn each_reload_event_maps_to_the_realm_tier() {
    assert_eq!(reload_tier(ReloadEvent::Unchanged), None);
    assert_eq!(
        reload_tier(ReloadEvent::Patched),
        Some(ReloadTier::Reassemble)
    );
}
