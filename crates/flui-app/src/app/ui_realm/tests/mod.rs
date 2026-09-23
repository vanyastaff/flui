use flui_engine::PresentDisposition;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use flui_foundation::RenderId;
use flui_semantics::{
    AccessibilityNodeId, SemanticsAction, SemanticsActionRequest, SemanticsNode, SemanticsOwner,
};
use flui_view::prelude::*;
use flui_widgets::{NavigatorCommand, NavigatorHandle, SimpleRoute, SizedBox};

use super::*;
use crate::app::raster_test_support::TestRasterBackend;

static_assertions::assert_not_impl_any!(UiRealm: Send, Sync);

fn noop_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

fn counting_wake() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let count_in_wake = Arc::clone(&count);
    (
        Arc::new(move || {
            count_in_wake.fetch_add(1, Ordering::Relaxed);
        }),
        count,
    )
}

fn test_window() -> Arc<dyn PlatformWindow> {
    crate::app::window_test_support::headless_test_window()
}

fn new_runtime(wake: Arc<dyn Fn() + Send + Sync>) -> Result<UiRealm, UiRealmError> {
    UiRealm::new(wake, test_window(), 1.0, Arc::new(AtomicBool::new(false)))
}

fn new_runtime_with_capacity(
    capacity: usize,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<UiRealm, UiRealmError> {
    UiRealm::with_capacity(
        capacity,
        wake,
        test_window(),
        1.0,
        Arc::new(AtomicBool::new(false)),
    )
}

#[test]
fn senders_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<UiCommandSender>();
}

#[test]
fn realm_entry_activates_its_global_key_registry() {
    let realm = new_runtime(noop_wake()).expect("runtime");
    let key = flui_view::GlobalKey::<()>::new();
    let element = flui_foundation::ElementId::new(17);
    realm
        .widgets()
        .with_build_owner_mut(|owner| owner.register_global_key(&key, element));

    assert_eq!(
        key.current_element(),
        None,
        "no realm is active outside enter"
    );
    realm.enter(|_| {
        assert_eq!(key.current_element(), Some(element));
    });
    assert_eq!(
        key.current_element(),
        None,
        "enter restores quiescent state"
    );
}

#[test]
fn presentation_and_widget_tree_share_the_exact_focus_owner() {
    let realm = new_runtime(noop_wake()).expect("runtime");

    let presentation_focus = realm.focus_manager();
    let widget_focus = realm
        .widgets()
        .with_build_owner(flui_view::BuildOwner::focus_manager);

    assert!(
        Rc::ptr_eq(&presentation_focus, &widget_focus),
        "keyboard dispatch and every BuildContext must address one focus tree"
    );
}

/// ADR-0074 §5.8: a cross-thread signal write is a realm command — it runs
/// against the realm's own graph on the owner thread at the next drain.
#[cfg(feature = "signals")]
#[test]
fn a_signal_write_command_reaches_the_realms_graph_at_the_next_drain() {
    let realm = new_runtime(noop_wake()).expect("runtime");
    let graph = realm
        .widgets()
        .with_build_owner(|owner| owner.reactive().clone());
    let counter = graph.signal(1u32);
    let sender = counter.detach(); // the Send form; the Signal itself is realm-affine

    realm
        .command_sender()
        .send_signal_write(Box::new(move |r: &flui_view::Reactive| {
            sender
                .attach()
                .update(r, |c| *c += 41)
                .expect("signal alive");
        }))
        .expect("send");
    assert_eq!(counter.peek(&graph, |c| *c), Ok(1), "nothing runs at send");

    let report = realm.drain_commands();

    assert_eq!(report.invoked, 1);
    assert_eq!(counter.peek(&graph, |c| *c), Ok(42));
}

#[test]
fn recreated_runtime_gets_fresh_realm_id() {
    let first = new_runtime(noop_wake()).expect("first runtime");
    let first_id = first.realm_id();
    drop(first);
    let second = new_runtime(noop_wake()).expect("second incarnation");
    assert_ne!(
        first_id,
        second.realm_id(),
        "a recreated window must never compare equal to its predecessor"
    );
}

/// Dropping a realm must free its `PipelineOwner` -- no stray strong
/// `PipelineCell` clone survives in a listener closure, a cached
/// binding, or (the hazard the type's own docs call out) a render
/// object that captured its own owning cell and closed a
/// `cell -> owner -> tree -> object -> cell` `Rc` cycle nothing frees.
///
/// Red-check: have any long-lived piece of `UiRealm` (a semantics
/// listener, the renderer binding, `WidgetsBinding`) hold an *extra*
/// `PipelineCell` clone past the realm's own lifetime and this weak
/// upgrade stops returning `None`.
#[test]
fn dropping_the_realm_frees_its_pipeline_owner() {
    let realm = UiRealm::for_test();
    let weak = realm.pipeline_for_test().downgrade_for_test();
    assert!(
        weak.upgrade().is_some(),
        "sanity: the owner is alive while the realm holds it"
    );

    drop(realm);

    assert!(
        weak.upgrade().is_none(),
        "dropping the realm must drop every strong PipelineCell clone it \
         held (presentation, widgets binding, renderer binding) -- a \
         surviving upgrade means one of them outlives the realm, or a \
         cycle is keeping the owner alive"
    );
}

#[test]
fn cross_thread_navigation_command_drains_on_owner_thread() {
    let runtime = new_runtime(noop_wake()).expect("runtime");
    let sender = runtime.command_sender();

    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    let pushed = navigator.push(test_route("/details"));
    let target = navigator.command_target();

    std::thread::spawn(move || {
        sender
            .send_navigation(NavigatorCommand::pop(target))
            .expect("inbox has room");
    })
    .join()
    .expect("sender thread did not panic");

    let report = runtime.drain_commands();
    assert_eq!(report.invoked, 1);
    assert_eq!(report.dropped_stale, 0);
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(pushed.try_take(), Some(None));
}

/// The lock-free-at-dispatch invariant this test used to probe from
/// *inside* the action handler (`Weak<RwLock<PipelineOwner>>::try_write`)
/// is unrepresentable now: `SemanticsActionHandler` is
/// `Arc<dyn Fn(..) + Send + Sync>` (a retained cross-thread seam — see
/// `flui-semantics/src/action.rs`), and `PipelineCell` is `!Send`, so no
/// handle derived from it can be captured in that closure any more. The
/// invariant itself did not disappear — it moved into production as the
/// `debug_assert!(is_free())` in `PresentationState::dispatch_semantics_
/// action` (registry: `runtime-contract.toml`'s `semantics-two-phase-borrow`
/// contract), which this test's
/// normal pass/fail already exercises (the assert would panic the test
/// if it ever fired). What remains directly assertable here — and what
/// this test still proves — is the *observable* contract: dispatch
/// defers to the owner's Idle commit point, and the counts are right.
#[test]
fn semantics_action_commits_on_the_owner_after_releasing_the_pipeline_lock() {
    let realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();
    let invoked = Arc::new(AtomicUsize::new(0));
    let invoked_in_handler = Arc::clone(&invoked);
    let render_id = RenderId::new(7);
    let target = AccessibilityNodeId::from(render_id);

    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().add_action(
        SemanticsAction::Tap,
        Arc::new(move |action, arguments| {
            assert_eq!(action, SemanticsAction::Tap);
            assert!(arguments.is_none());
            invoked_in_handler.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    pipeline.with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    let sender = realm.command_sender();
    std::thread::spawn(move || {
        sender
            .send_semantics_action(SemanticsActionRequest::new(target, SemanticsAction::Tap))
            .expect("realm inbox has room");
    })
    .join()
    .expect("platform action sender did not panic");

    assert_eq!(
        invoked.load(Ordering::SeqCst),
        0,
        "cross-thread input must wait for the owner's Idle commit point"
    );
    let report = realm.drain_commands();

    assert_eq!(report.invoked, 1);
    assert_eq!(report.dropped_stale, 0);
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
}

#[test]
fn stale_semantics_action_is_gracefully_dropped() {
    let realm = UiRealm::for_test();
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
    semantics_owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    realm
        .command_sender()
        .send_semantics_action(SemanticsActionRequest::new(
            AccessibilityNodeId::from(RenderId::new(99)),
            SemanticsAction::Tap,
        ))
        .expect("realm inbox has room");

    let report = realm.drain_commands();
    assert_eq!(report.invoked, 0);
    assert_eq!(report.dropped_stale, 1);
}

/// The full inbound platform wire (issue #684): an action request
/// arriving on the platform adapter's thread — Click, addressed by the
/// stable exported node id — crosses the AccessKit→FLUI translation,
/// marshals through the realm inbox, and invokes the node's Tap handler
/// at the owner's Idle drain. Driven through the window's OWN
/// `FakeAccessibility` and the listener `wire_platform_accessibility`
/// registered on it — never a hand-rolled sender, which would pass with
/// the production wire unplugged.
#[test]
fn platform_action_request_routes_through_the_wire_to_the_handler() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    let realm =
        UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false))).expect("realm");

    let render_id = RenderId::new(7);
    let target = AccessibilityNodeId::from(render_id);
    let invoked = Arc::new(AtomicUsize::new(0));
    let invoked_in_handler = Arc::clone(&invoked);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().add_action(
        SemanticsAction::Tap,
        Arc::new(move |action, _arguments| {
            assert_eq!(action, SemanticsAction::Tap, "Click must arrive as Tap");
            invoked_in_handler.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    // An adapter only delivers actions while an AT session is active.
    fake.set_active(true);
    fake.request_action(accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(target.as_u64()),
        data: None,
    });

    assert_eq!(
        invoked.load(Ordering::SeqCst),
        0,
        "platform input must wait for the owner's Idle commit point"
    );
    let report = realm.drain_commands();
    assert_eq!(report.invoked, 1);
    assert_eq!(report.dropped_stale, 0);
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
}

/// A typed payload crosses the whole wire: a SetValue request carrying
/// its text arrives at the node's SetText handler WITH that text. A
/// payload lost anywhere along the seam turns a screen-reader edit into
/// an argument-free no-op, which no other test would notice.
#[test]
fn platform_action_payload_reaches_the_handler_with_its_arguments() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    let realm =
        UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false))).expect("realm");

    let render_id = RenderId::new(7);
    let target = AccessibilityNodeId::from(render_id);
    let received = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().add_action(
        SemanticsAction::SetText,
        Arc::new(move |_action, arguments| {
            sink.lock().push(arguments);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    fake.set_active(true);
    fake.request_action(accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(target.as_u64()),
        data: Some(accesskit::ActionData::Value("hello".into())),
    });

    let report = realm.drain_commands();
    assert_eq!(report.invoked, 1);
    assert_eq!(
        received.lock().as_slice(),
        &[Some(flui_semantics::ActionArgs::SetText {
            text: "hello".to_string()
        })],
        "the handler must receive the platform's payload, translated"
    );
}

/// Requests the listener cannot route never reach the inbox at all: an
/// action FLUI has no counterpart for, and the zero node id no
/// published tree ever exports. Both are traced drops at the platform
/// seam — a screen reader acting on a stale snapshot is tolerated,
/// never a panic (issue #684's graceful-drop acceptance).
#[test]
fn unroutable_platform_action_requests_are_dropped_at_the_listener() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    let realm =
        UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false))).expect("realm");

    let render_id = RenderId::new(7);
    let target = AccessibilityNodeId::from(render_id);
    let invoked = Arc::new(AtomicUsize::new(0));
    let invoked_in_handler = Arc::clone(&invoked);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().add_action(
        SemanticsAction::Tap,
        Arc::new(move |_action, _arguments| {
            invoked_in_handler.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    fake.set_active(true);
    // An action with no FLUI counterpart, addressed to a live node.
    fake.request_action(accesskit::ActionRequest {
        action: accesskit::Action::Expand,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(target.as_u64()),
        data: None,
    });
    // A routable action addressed to the out-of-contract zero id.
    fake.request_action(accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(0),
        data: None,
    });

    let report = realm.drain_commands();
    assert_eq!(
        (report.invoked, report.dropped_stale),
        (0, 0),
        "neither request may even reach the inbox"
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
}

/// The activation seam end-to-end (issue #684): assistive technology
/// attaching may only flip the presentation's host flag (the listener
/// runs on the adapter's thread) — the next frame's reconcile is what
/// turns that into pipeline state, creating the semantics owner; and
/// detaching disposes it the same way.
#[test]
fn at_activation_drives_semantics_assembly_through_the_frame_reconcile() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    let realm =
        UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false))).expect("realm");
    let pipeline = realm.pipeline_for_test();
    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));

    assert!(
        !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
        "assembly is off until assistive technology attaches"
    );

    fake.set_active(true);
    assert!(
        !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
        "the listener alone must not mutate pipeline state — that is owner-thread work"
    );
    realm.enter(|_| {
        let _ = realm.draw_frame_entered(constraints);
    });
    assert!(
        pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
        "the frame reconcile turns the activation flag into pipeline state"
    );
    assert!(
        pipeline.with(|owner| owner.semantics_owner().is_some()),
        "enabling creates the semantics owner"
    );

    fake.set_active(false);
    realm.enter(|_| {
        let _ = realm.draw_frame_entered(constraints);
    });
    assert!(
        !pipeline.with(flui_rendering::PipelineOwner::semantics_enabled),
        "detaching stops assembly on the next frame"
    );
    assert!(
        pipeline.with(|owner| owner.semantics_owner().is_none()),
        "deactivation disposes the owner"
    );
}

/// Assistive technology attaching must also request a self-contained
/// full republish (the adapter's state is unknown, and flushes publish
/// incrementally — the next diff would answer a fresh screen reader
/// with a fragment). The listener may only set the flag; the frame
/// reconcile consumes it on the owner thread and routes it to
/// `PipelineOwner::request_semantics_full_publish`.
#[test]
fn at_activation_requests_a_full_republish_and_the_reconcile_consumes_it() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    let realm =
        UiRealm::new(noop_wake(), window, 1.0, Arc::new(AtomicBool::new(false))).expect("realm");
    let host_flag = realm
        .presentations
        .primary()
        .semantics_host()
        .full_republish_handle();
    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));

    fake.set_active(true);
    assert!(
        host_flag.load(Ordering::Relaxed),
        "activation must record that the adapter needs a full tree"
    );

    realm.enter(|_| {
        let _ = realm.draw_frame_entered(constraints);
    });
    assert!(
        !host_flag.load(Ordering::Relaxed),
        "the frame reconcile consumes the request exactly once"
    );

    fake.set_active(false);
    realm.enter(|_| {
        let _ = realm.draw_frame_entered(constraints);
    });
    assert!(
        !host_flag.load(Ordering::Relaxed),
        "deactivation must not request a republish nobody will hear"
    );
}

/// Teardown withdraws from the platform bridge: after the presentation
/// closes, an activation flip delivered by the (longer-lived) adapter
/// must not reach the dead presentation's enablement flag — `close()`
/// replaced the listener with a no-op. If the withdrawal is removed,
/// the original listener (still held by the fake) stores `true` into
/// the flag this test kept a handle on, and the assertion fails.
#[test]
fn closing_the_presentation_withdraws_from_the_platform_bridge() {
    let fake = Arc::new(flui_platform::FakeAccessibility::new());
    let window =
        crate::app::presentation::test_platform_window_with_accessibility(Arc::clone(&fake));
    // Hold the window strong across the drop: `close()`'s withdrawal
    // reaches the bridge through a `Weak` window upgrade, exactly as a
    // production runner (which owns the window) would still succeed.
    let realm = UiRealm::new(
        noop_wake(),
        Arc::clone(&window),
        1.0,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("realm");
    let flag = realm
        .presentations
        .primary()
        .semantics_host()
        .platform_semantics_enabled_handle();

    drop(realm);

    fake.set_active(true);
    assert!(
        !flag.load(Ordering::Relaxed),
        "a closed presentation's enablement flag must never flip again"
    );
}

/// Distinct from `stale_semantics_action_is_gracefully_dropped` above:
/// that test forges a stale *node* id against a live presentation; this
/// one forges a stale *presentation* stamp against a request whose node
/// would otherwise resolve — proving the stamp check runs first and
/// never lets the request reach `dispatch_semantics_action` (no
/// pipeline borrow at all).
///
/// If reverted: remove the `presentation_id` comparison from
/// `drain_commands` and this fails (`invoked == 1` instead of
/// `dropped_stale == 1`).
#[test]
fn semantics_action_with_stale_presentation_stamp_is_dropped() {
    let realm = UiRealm::for_test();
    let render_id = RenderId::new(1);
    let invoked = Arc::new(AtomicUsize::new(0));
    let invoked_in_handler = Arc::clone(&invoked);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    // The action handler is real and would succeed: this test's stamp
    // check must be what drops the request, not an unrelated resolution
    // failure (e.g. a node with no registered action at all, which
    // would drop for the wrong reason and pass even with the stamp
    // check deleted).
    node.config_mut().add_action(
        SemanticsAction::Tap,
        Arc::new(move |_action, _arguments| {
            invoked_in_handler.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    let live = realm.presentation_id();
    let forged = PresentationId::new_gen(
        live.index(),
        NonZeroU32::new(live.generation().get() + 1).expect("nonzero"),
    );
    realm
        .command_sender()
        .send(UiCommand::SemanticsAction {
            presentation_id: forged,
            request: SemanticsActionRequest::new(
                AccessibilityNodeId::from(render_id),
                SemanticsAction::Tap,
            ),
        })
        .expect("realm inbox has room");

    let report = realm.drain_commands();
    assert_eq!(report.invoked, 0);
    assert_eq!(report.dropped_stale, 1);
    assert_eq!(
        invoked.load(Ordering::SeqCst),
        0,
        "a stale-stamped action must never reach the handler at all"
    );
}

/// Issue #555's addressed-routing slice: `drain_commands`'s `SemanticsAction` arm checks
/// FOREST MEMBERSHIP (`self.presentations.get(presentation_id)`), not
/// equality against `self.presentations.primary().id()` — a live
/// NON-primary presentation's own stamped action must actually invoke,
/// not be dropped as stale just because it is not the primary.
///
/// If reverted (the pre-addressed-routing primary-equality check restored):
/// this fails — `report.invoked` would be `0` and `dropped_stale` would
/// be `1`, because B is never the primary in this fixture.
#[test]
fn semantics_action_addressed_to_a_live_non_primary_presentation_is_invoked() {
    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();

    let render_id = RenderId::new(1);
    let invoked = Arc::new(AtomicUsize::new(0));
    let invoked_in_handler = Arc::clone(&invoked);
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    node.config_mut().add_action(
        SemanticsAction::Tap,
        Arc::new(move |_action, _arguments| {
            invoked_in_handler.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut semantics_owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = semantics_owner.insert(node);
    semantics_owner.set_root(Some(root));
    realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .pipeline()
        .with_mut(|owner| owner.set_semantics_owner(Some(semantics_owner)));

    realm
        .command_sender()
        .send(UiCommand::SemanticsAction {
            presentation_id: b_id,
            request: SemanticsActionRequest::new(
                AccessibilityNodeId::from(render_id),
                SemanticsAction::Tap,
            ),
        })
        .expect("realm inbox has room");

    let report = realm.drain_commands();
    assert_eq!(
        report.invoked, 1,
        "B's own stamped action must be invoked, not dropped"
    );
    assert_eq!(report.dropped_stale, 0);
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
}

/// Issue #555's addressed-routing slice: an action stamped for a presentation that this
/// realm no longer hosts — closed since the request was enqueued —
/// drops traced, exactly like a forged/never-existed generation does.
///
/// If reverted the same way as the test above (primary-equality
/// restored, or the forest-membership check deleted outright): this
/// still passes vacuously for the WRONG reason (A, the closed
/// presentation, was never the primary either) — the companion test
/// above is what actually pins the membership check; this one pins the
/// closed-presentation half of the same contract.
#[test]
fn semantics_action_with_stale_presentation_generation_drops_traced() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let _b_id = realm.install_second_presentation_for_test();

    realm
        .command_sender()
        .send(UiCommand::SemanticsAction {
            presentation_id: a_id,
            request: SemanticsActionRequest::new(
                AccessibilityNodeId::from(RenderId::new(1)),
                SemanticsAction::Tap,
            ),
        })
        .expect("realm inbox has room");

    // Close A (the sender's stamped target) while B survives -- A's
    // generation is now gone from the forest entirely.
    realm.close_presentation_entered(a_id);

    let report = realm.drain_commands();
    assert_eq!(
        report.invoked, 0,
        "a request stamped for a presentation this realm no longer hosts must never invoke"
    );
    assert_eq!(report.dropped_stale, 1);
}

#[test]
fn dead_navigation_target_is_dropped_at_commit() {
    let runtime = new_runtime(noop_wake()).expect("runtime");
    let sender = runtime.command_sender();
    let target = {
        let navigator = NavigatorHandle::new();
        navigator.command_target()
    };

    sender
        .send_navigation(NavigatorCommand::maybe_pop(target))
        .expect("inbox has room");

    let report = runtime.drain_commands();
    assert_eq!(report.invoked, 0);
    assert_eq!(report.dropped_stale, 1);
}

#[test]
fn inbox_reports_backpressure_at_capacity() {
    let runtime = new_runtime_with_capacity(2, noop_wake()).expect("runtime with tiny inbox");
    let sender = runtime.command_sender();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    let filler = || NavigatorCommand::maybe_pop(navigator.command_target());

    sender.send_navigation(filler()).expect("first fits");
    sender.send_navigation(filler()).expect("second fits");
    let overflow = sender
        .send_navigation(filler())
        .expect_err("third command is rejected");
    assert!(matches!(
        overflow,
        CommandSendError::ChannelFull { capacity: 2, .. }
    ));
    // Draining frees the inbox again.
    let _ = runtime.drain_commands();
    sender.send_navigation(filler()).expect("room after drain");
}

#[test]
fn dropped_runtime_yields_owner_gone() {
    let runtime = new_runtime(noop_wake()).expect("runtime");
    let sender = runtime.command_sender();
    drop(runtime);
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    assert!(matches!(
        sender.send_navigation(NavigatorCommand::maybe_pop(navigator.command_target())),
        Err(CommandSendError::OwnerGone { .. })
    ));
}

#[test]
fn channel_full_retry_preserves_the_rejected_payload() {
    let runtime = new_runtime_with_capacity(1, noop_wake()).expect("runtime");
    let sender = runtime.command_sender();
    let filler_navigator = NavigatorHandle::new();
    filler_navigator.seed_initial(test_route("/"));
    sender
        .send_navigation(NavigatorCommand::maybe_pop(
            filler_navigator.command_target(),
        ))
        .expect("fills inbox");

    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    let pushed = navigator.push(test_route("/details"));
    let rejected = sender
        .send_navigation(NavigatorCommand::pop(navigator.command_target()))
        .expect_err("inbox full")
        .into_rejected();

    let _ = runtime.drain_commands();
    sender.send(rejected).expect("retry fits");
    let _ = runtime.drain_commands();
    assert_eq!(navigator.route_ids().len(), 1);
    assert_eq!(pushed.try_take(), Some(None));
}

#[test]
fn redraw_requests_coalesce_to_one_flag_and_one_wake() {
    let (wake, wake_count) = counting_wake();
    let runtime = new_runtime(wake).expect("runtime");
    let sender = runtime.command_sender();

    sender.request_redraw();
    sender.request_redraw();
    sender.request_redraw();
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        1,
        "a burst of redraw requests pays exactly one wake"
    );
    assert!(runtime.take_redraw_request(), "flag observed once");
    assert!(!runtime.take_redraw_request(), "reading clears the flag");

    sender.request_redraw();
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        2,
        "after the owner consumes the flag, the next request wakes again"
    );
}

#[test]
fn every_send_wakes_the_owner() {
    let (wake, wake_count) = counting_wake();
    let runtime = new_runtime(wake).expect("runtime");
    let sender = runtime.command_sender();

    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    sender
        .send_navigation(NavigatorCommand::maybe_pop(navigator.command_target()))
        .expect("inbox has room");
    sender
        .send_navigation(NavigatorCommand::maybe_pop(navigator.command_target()))
        .expect("inbox has room");

    assert_eq!(wake_count.load(Ordering::Relaxed), 2);
    let _ = runtime.drain_commands();
}

fn test_route(name: &'static str) -> SimpleRoute<i32> {
    SimpleRoute::new(move |_ctx| SizedBox::new(1.0, 1.0).into_view().boxed()).named(name)
}

// ========================================================================
// Realm coexistence — the criterion-1/2 evidence for singleton retirement.
// Every process-global graph `UiRealm` used to front (the transitional
// at-most-one-instance construction guard, `AppBinding`, the `UpdateScheduler`
// singleton) is gone: these tests prove what that actually buys, rather
// than asserting the absence of code that no longer exists.
// ========================================================================

fn coexistence_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(200.0), px(200.0)))
}

/// Two realms constructed through the PRODUCTION path (`UiRealm::new`,
/// via `new_runtime` — not the `for_test` bypass), on ONE thread,
/// simultaneously live.
///
/// Red at the old shape: resurrecting the deleted at-most-one claim
/// flag's swap-and-check inside `with_capacity` (an atomic swap
/// returning early with a now-deleted typed "already exists" error
/// variant) turns the second `new_runtime` call below into a typed
/// error instead of a live realm — this test would fail immediately.
/// What made that guard load-bearing at the old HEAD is also gone:
/// `UiRealm::construct` resolves its own
/// `RealmServices` (a fresh `UpdateScheduler` strong root) instead of reaching
/// a process-global one, so two realms on one thread no longer alias
/// anything to guard against.
/// An input-driven redraw request must WAKE the platform loop, not just
/// set flags: the loop parks between events, and flags nobody pokes it
/// about are inert — a live drag froze the screen for exactly this
/// reason (125 pointer moves produced 2 frames; the pointer-routing
/// path requested a redraw every time, flag-only). The wake is what
/// converts the request into a `RedrawRequested` and therefore a frame.
#[test]
fn a_redraw_request_fires_the_platform_wake() {
    let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let wake_count_in_closure = Arc::clone(&wake_count);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_count_in_closure.fetch_add(1, Ordering::Relaxed);
    });
    let realm = UiRealm::new(
        wake,
        crate::app::presentation::test_platform_window(None),
        1.0,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("test realm construction");

    realm.request_redraw();

    assert!(
        wake_count.load(Ordering::Relaxed) >= 1,
        "request_redraw must fire the platform wake — flag-only requests \
         leave a parked event loop asleep and the screen frozen"
    );
}

/// The realm installs a live root `MediaQuery`: the user's subtree can
/// read it from the very first build, and a platform appearance change
/// republishes the new brightness through the same root on the next
/// frame — the wire nothing used to consume.
#[test]
fn the_root_media_query_republishes_a_brightness_change() {
    use std::cell::Cell;

    #[derive(Clone)]
    struct BrightnessProbe {
        seen: std::rc::Rc<Cell<Option<flui_types::platform::Brightness>>>,
    }
    impl flui_view::View for BrightnessProbe {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }
    impl flui_view::StatelessView for BrightnessProbe {
        fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
            self.seen
                .set(Some(flui_widgets::MediaQuery::of(ctx).platform_brightness));
            SizedBox::new(10.0, 10.0)
        }
    }

    let seen = std::rc::Rc::new(Cell::new(None));
    let realm = new_runtime(noop_wake()).expect("realm claims cleanly");
    realm
        .attach_root_widget_with_size(
            &BrightnessProbe {
                seen: std::rc::Rc::clone(&seen),
            },
            10.0,
            10.0,
        )
        .expect("mounts under the root MediaQuery");
    let _ = realm.draw_frame(coexistence_constraints());
    assert_eq!(
        seen.get(),
        Some(flui_types::platform::Brightness::Light),
        "the first build reads the installed root MediaQuery (default light)"
    );

    // The appearance arm's write side: mutate the shared source and pump.
    seen.set(None);
    realm.media_query().update(|data| {
        data.platform_brightness = flui_types::platform::Brightness::Dark;
    });
    assert!(
        realm.presentations.primary().widgets().has_pending_builds(),
        "the appearance write's externally-scheduled rebuild must count as \
         dirty work — a frame gate blind to the external inbox skips the \
         very frame the schedule woke"
    );
    let _ = realm.draw_frame(coexistence_constraints());
    assert_eq!(
        seen.get(),
        Some(flui_types::platform::Brightness::Dark),
        "an appearance change republishes through the root on the next frame"
    );
}

/// A realm's fresh hit test reads its own live render tree.
///
/// Every layer here is separately capable of being wired to nothing: the
/// lane declares the capability but cannot perform one, the probe can but
/// is inert until installed, and the handle is realm-scoped but useless if
/// it addresses a different presentation's tree. So the oracle is the
/// ANSWER — a position over the mounted content hits, one past it does not
/// — rather than the handle merely being obtainable, which every unwired
/// arrangement above also satisfies.
///
/// The content is wrapped in an OPAQUE `Listener` deliberately. A bare
/// `SizedBox` is not hit-testable (`forward_hit_test` returns false with no
/// child, matching `RenderProxyBox`), and `Listener`'s own default
/// `DeferToChild` inherits that miss — both correct, and both enough to
/// make this pass against a probe wired to nothing.
#[test]
fn a_realms_fresh_hit_test_reads_its_own_live_tree() {
    use flui_types::{Offset, Pixels};

    let realm = new_runtime(noop_wake()).expect("realm claims cleanly");
    realm
        .attach_root_widget_with_size(
            &flui_widgets::Listener::new()
                .behavior(flui_interaction::HitTestBehavior::Opaque)
                .child(SizedBox::new(40.0, 20.0)),
            40.0,
            20.0,
        )
        .expect("mounts");
    // Tight 200x200 root constraints, so the mounted content fills the
    // view rather than staying at its 40x20 request.
    let _ = realm.draw_frame(coexistence_constraints());

    let handle = realm
        .presentations
        .primary()
        .widgets()
        .with_build_owner(|owner| owner.hit_test_handle().cloned())
        .expect("a presentation installs its own hit-test handle at assembly");

    let probe_at = |x: f32, y: f32| {
        realm
            .interaction_lane
            .enter(|| handle.hit_test_at(Offset::new(Pixels(x), Pixels(y))))
    };

    let inside = probe_at(5.0, 5.0).expect("realm active");
    assert!(
        !inside.is_empty(),
        "a position over the mounted content must hit it; an empty path \
         means the probe is reading some tree other than the one this \
         presentation's frame just laid out, or none at all"
    );

    let outside = probe_at(500.0, 500.0).expect("realm active");
    assert!(
        outside.is_empty(),
        "a position past the 200x200 view must hit nothing — if this also \
         reports hits, the probe is answering from position-independent \
         state rather than testing the position it was given"
    );

    // Realm-scoped, not process-global: another realm refuses the handle.
    let other = new_runtime(noop_wake()).expect("second realm");
    assert_eq!(
        other
            .interaction_lane
            .enter(|| handle.hit_test_at(Offset::new(Pixels(5.0), Pixels(5.0))))
            .unwrap_err(),
        flui_interaction::InteractionDispatchError::WrongRealm
    );
}

/// Two presentations in ONE realm read their own trees.
///
/// The realm's interaction lane is shared by every presentation it hosts,
/// but each has its own `PipelineOwner`. A probe held once per realm would
/// answer the second presentation with the first one's tree — and would
/// report the tree gone once the FIRST presentation closed, while the
/// second was still live. Hence the handle is minted per presentation, at
/// assembly, pairing the realm's ticket with that presentation's pipeline.
#[test]
fn two_presentations_in_one_realm_do_not_share_a_hit_test_tree() {
    use flui_types::{Offset, Pixels};

    let mut realm = UiRealm::for_test();
    let second_id = realm.install_second_presentation_for_test();
    let first_id = realm.presentation_id();
    assert_ne!(first_id, second_id);

    let handle_for = |id: PresentationId| {
        realm
            .presentations
            .get(id)
            .expect("installed")
            .widgets()
            .with_build_owner(|owner| owner.hit_test_handle().cloned())
            .expect("each presentation installs its own handle")
    };

    // Content in the FIRST presentation only. Its tree hits; the second
    // presentation's tree is empty and must say so.
    realm
        .attach_root_widget_with_size(
            &flui_widgets::Listener::new()
                .behavior(flui_interaction::HitTestBehavior::Opaque)
                .child(SizedBox::new(40.0, 20.0)),
            40.0,
            20.0,
        )
        .expect("mounts into the primary presentation");
    let _ = realm.draw_frame(coexistence_constraints());

    let at_origin = Offset::new(Pixels(5.0), Pixels(5.0));
    let (from_first, from_second) = realm.interaction_lane.enter(|| {
        (
            handle_for(first_id).hit_test_at(at_origin),
            handle_for(second_id).hit_test_at(at_origin),
        )
    });

    assert!(
        !from_first.expect("first answers").is_empty(),
        "the presentation that owns the mounted content must hit it"
    );
    assert!(
        from_second.expect("second answers").is_empty(),
        "the second presentation has no content — reporting a hit here \
         means it is reading the first presentation's tree"
    );
}

/// A closed presentation refuses hit tests even while its tree is held.
///
/// Under `SharedRealm` the realm outlives any one presentation, so the
/// realm ticket alone cannot say "closed". Neither can the pipeline's
/// allocation: `BuildContext::pipeline_owner()` hands out a STRONG
/// `PipelineCell`, so a widget that stored one keeps the tree alive past
/// the close — and a probe reading liveness from the allocation would go
/// on answering from a detached tree with nothing to catch it.
///
/// Hence the presentation owns a token, and this test holds the tree the
/// way such a widget would.
#[test]
fn a_closed_presentations_hit_test_refuses_while_its_tree_is_still_held() {
    use flui_interaction::InteractionDispatchError;
    use flui_types::{Offset, Pixels};

    let mut realm = UiRealm::for_test();
    let second_id = realm.install_second_presentation_for_test();
    let closing_id = realm.presentation_id();

    let handle = realm
        .presentations
        .get(closing_id)
        .expect("installed")
        .widgets()
        .with_build_owner(|owner| owner.hit_test_handle().cloned())
        .expect("assembly installs a handle");

    // Stand in for a widget that stored `pipeline_owner()`: a strong
    // reference outliving the presentation it came from.
    let retained_tree = realm
        .presentations
        .get(closing_id)
        .expect("installed")
        .pipeline()
        .clone();

    let at = Offset::new(Pixels(5.0), Pixels(5.0));
    assert!(
        realm
            .interaction_lane
            .enter(|| handle.hit_test_at(at))
            .is_ok(),
        "precondition: the handle answers while its presentation is open"
    );

    assert!(realm.close_presentation_entered(closing_id));
    assert!(
        realm.presentations.get(second_id).is_some(),
        "the sibling presentation keeps the realm -- and its ticket -- alive, \
         which is what makes the realm check unable to catch this"
    );

    assert_eq!(
        realm
            .interaction_lane
            .enter(|| handle.hit_test_at(at))
            .unwrap_err(),
        InteractionDispatchError::OwnerGone,
        "a closed presentation must report itself gone even though its \
         tree is still allocated and its realm is still live"
    );
    drop(retained_tree);
}

#[test]
fn two_realms_coexist_same_thread() {
    use std::cell::Cell;

    let realm_a = new_runtime(noop_wake()).expect("realm A claims cleanly");
    let realm_b =
        new_runtime(noop_wake()).expect("realm B claims cleanly ALONGSIDE realm A, same thread");

    // Disjoint identities.
    assert_ne!(
        realm_a.realm_id(),
        realm_b.realm_id(),
        "two live realms must never compare equal"
    );

    // Disjoint focus tree and disjoint PipelineOwner (the container a
    // SemanticsOwner is set on, `pipeline.write().set_semantics_owner`)
    // -- neither realm's focus dispatch nor its semantics state can be
    // the other's Rc/Arc.
    assert!(
        !Rc::ptr_eq(&realm_a.focus_manager(), &realm_b.focus_manager()),
        "two realms must never share one focus tree"
    );
    assert!(
        !realm_a
            .pipeline_for_test()
            .ptr_eq(&realm_b.pipeline_for_test()),
        "two realms must never share one PipelineOwner (and therefore never one SemanticsOwner)"
    );

    // Independent widget mounts: A gets a root; B stays unattached, so
    // A's mount must not leak into B's own WidgetsBinding.
    realm_a
        .attach_root_widget_with_size(&SizedBox::new(50.0, 50.0), 50.0, 50.0)
        .expect("realm A mounts its own root");
    assert!(
        realm_a.draw_frame(coexistence_constraints()).is_some(),
        "realm A produced a scene from its own mounted root"
    );
    assert!(
        realm_b.draw_frame(coexistence_constraints()).is_none(),
        "realm B has no root attached; realm A's mount must not leak into it"
    );

    // Independent scheduler phases: drive realm A's frame transaction
    // and observe realm B's scheduler from inside it, mid-frame.
    let phase_a_mid_frame = Cell::new(None);
    let phase_b_mid_frame = Cell::new(None);
    realm_a.scheduler().drive_frame_with_lane(
        flui_scheduler::Instant::now(),
        flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
        || {
            phase_a_mid_frame.set(Some(realm_a.scheduler().phase()));
            phase_b_mid_frame.set(Some(realm_b.scheduler().phase()));
            let _ = realm_a.draw_frame(coexistence_constraints());
        },
        realm_a.local_post_frame_lane(),
    );
    assert_ne!(
        phase_a_mid_frame.get(),
        Some(SchedulerPhase::Idle),
        "realm A's own scheduler must be mid-frame while its pipeline runs"
    );
    assert_eq!(
        phase_b_mid_frame.get(),
        Some(SchedulerPhase::Idle),
        "realm B's scheduler must stay Idle while realm A alone is driving a frame"
    );

    // Independent gesture arenas: a route registered on A must never
    // fire for input dispatched into B.
    use flui_interaction::PointerId;
    use flui_interaction::events::{PointerType, make_down_event_for_id};
    use flui_interaction::routing::PointerRouteHandler;
    use flui_types::geometry::Pixels;

    let pointer = PointerId::new(9002).expect("nonzero pointer id");
    let position = flui_types::Offset::new(Pixels(10.0), Pixels(10.0));
    let fired = Rc::new(Cell::new(0));
    let fired_by_route = Rc::clone(&fired);
    let handler: PointerRouteHandler = Rc::new(move |_| {
        fired_by_route.set(fired_by_route.get() + 1);
    });
    realm_a
        .gestures()
        .pointer_router()
        .add_route(pointer, handler);

    let down_b = make_down_event_for_id(pointer, position, PointerType::Touch);
    realm_b.enter(|realm| {
        realm.handle_input_entered(PlatformInput::Pointer(down_b));
    });
    assert_eq!(
        fired.get(),
        0,
        "a route registered on realm A must not fire for realm B's input"
    );
    assert_eq!(realm_b.gestures().active_pointer_count(), 1);
    assert_eq!(realm_a.gestures().active_pointer_count(), 0);
}

/// Realm A on the test's own thread, realm B constructed, driven, and
/// dropped entirely on a SECOND thread — proving there is no shared
/// mutable state to race on, not merely that construction succeeds.
/// `UiRealm` stays `!Send` throughout: `realm_b` never crosses the
/// thread boundary, only its `Send + Sync` wake counter and plain
/// `RealmId` do, via the join return value.
///
/// A barrier released BEFORE either side's frame work only proves both
/// threads STARTED around the same time — the OS scheduler is free to
/// run one side's whole `draw_frame` to completion before the other
/// even resumes, so the frame TRANSACTIONS themselves might never
/// actually overlap. The rendezvous below fixes that by sitting INSIDE
/// each realm's own `drive_frame` pipeline closure — which
/// `UpdateScheduler::handle_draw_frame` guarantees runs during
/// `SchedulerPhase::PersistentCallbacks` (see
/// `the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline`)
/// — so neither closure can proceed past the rendezvous until BOTH
/// realms are provably mid-transaction at the same instant.
/// `std::sync::Barrier` has no timeout, so a regression that stops one
/// side from ever reaching its frame closure would hang the test
/// forever instead of failing it; `rendezvous_or_timeout` below fails
/// loudly on a bounded deadline instead of deadlocking.
#[test]
fn two_realms_two_threads_no_shared_state() {
    let parties_arrived = std::sync::atomic::AtomicUsize::new(0);
    let rendezvous_or_timeout = |label: &'static str| {
        parties_arrived.fetch_add(1, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while parties_arrived.load(Ordering::SeqCst) < 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "{label}: rendezvous timed out -- the other realm's frame \
                 transaction never became concurrently mid-flight"
            );
            std::thread::yield_now();
        }
    };

    let (wake_a, wakes_a) = counting_wake();
    let realm_a = new_runtime(wake_a).expect("realm A claims cleanly on this thread");
    let sender_a = realm_a.command_sender();

    let (wakes_b, realm_b_id) = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let (wake_b, wakes_b) = counting_wake();
            let realm_b = new_runtime(wake_b)
                .expect("realm B claims cleanly on its OWN thread, concurrently with A");
            let sender_b = realm_b.command_sender();

            sender_b.request_redraw();
            let _ = realm_b.drain_commands();
            realm_b.scheduler().drive_frame_with_lane(
                flui_scheduler::Instant::now(),
                flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                || {
                    // Mid-PersistentCallbacks rendezvous: cannot return
                    // until realm A's own closure below has ALSO
                    // reached this point.
                    rendezvous_or_timeout("realm B");
                    let _ = realm_b.draw_frame(coexistence_constraints());
                },
                realm_b.local_post_frame_lane(),
            );
            (wakes_b.load(Ordering::Relaxed), realm_b.realm_id())
        });

        sender_a.request_redraw();
        let _ = realm_a.drain_commands();
        realm_a.scheduler().drive_frame_with_lane(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {
                rendezvous_or_timeout("realm A");
                let _ = realm_a.draw_frame(coexistence_constraints());
            },
            realm_a.local_post_frame_lane(),
        );

        handle.join().expect("realm B's thread did not panic")
    });

    assert_eq!(
        wakes_a.load(Ordering::Relaxed),
        1,
        "realm A's wake counter reflects only its own request"
    );
    assert_eq!(
        wakes_b, 1,
        "realm B's wake counter reflects only its own request, made on its own thread"
    );
    assert_ne!(realm_a.realm_id(), realm_b_id);
}

/// Extends `dropped_runtime_yields_owner_gone`'s single-realm shape
/// (`UiRealmError`/`CommandSendError::OwnerGone`) across two coexisting
/// realms: dropping realm A must leave realm B's wake counter and inbox
/// completely untouched, and A's own senders must turn `OwnerGone`
/// rather than silently reaching B.
#[test]
fn dropping_realm_a_cannot_wake_realm_b() {
    // Realm A's own wake counter has nothing left to assert once A is
    // dropped below (its `wake` closure can never fire again); only
    // realm B's counter is the interesting observable here.
    let (wake_a, _wakes_a) = counting_wake();
    let (wake_b, wakes_b) = counting_wake();
    let realm_a = new_runtime(wake_a).expect("realm A");
    let realm_b = new_runtime(wake_b).expect("realm B, alongside realm A");

    let sender_a = realm_a.command_sender();
    let realm_b_id_before = realm_b.realm_id();

    drop(realm_a);

    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    assert!(
        matches!(
            sender_a.send_navigation(NavigatorCommand::maybe_pop(navigator.command_target())),
            Err(CommandSendError::OwnerGone { .. })
        ),
        "a sender into the dropped realm A must turn OwnerGone"
    );
    assert_eq!(
        wakes_b.load(Ordering::Relaxed),
        0,
        "dropping realm A must not wake realm B"
    );
    assert_eq!(
        realm_b.realm_id(),
        realm_b_id_before,
        "realm B is unaffected by realm A's drop"
    );

    // realm B's own inbox still drains normally — dropping a SIBLING
    // realm leaves it fully live, not merely non-crashed.
    let sender_b = realm_b.command_sender();
    let navigator_b = NavigatorHandle::new();
    navigator_b.seed_initial(test_route("/"));
    sender_b
        .send_navigation(NavigatorCommand::maybe_pop(navigator_b.command_target()))
        .expect("realm B's inbox has room");
    let report = realm_b.drain_commands();
    assert_eq!(report.invoked, 1);
    assert_eq!(report.dropped_stale, 0);

    assert_eq!(
        wakes_b.load(Ordering::Relaxed),
        1,
        "only realm B's own command may wake realm B"
    );
    assert_eq!(
        realm_b.gestures().active_pointer_count(),
        0,
        "...nor its gesture arena"
    );
}

/// The sharpest widget-state isolation probe (per ADR-0027 §8: GlobalKey
/// is realm-scoped, not process-global). Within ONE realm, mounting a
/// second element under an already-registered `GlobalKey` value panics
/// eagerly (`register_global_key_with_collision_check`,
/// `element_tree.rs`) — that is FLUI's documented divergence from
/// Flutter's end-of-frame duplicate detection. Mounting the SAME key
/// VALUE as a root in two DIFFERENT realms must succeed in both: each
/// realm's `WidgetsBinding` owns its own `ElementOwner`/registry, and the
/// collision check is scoped to the currently-active one, never to the
/// key's numeric id process-wide.
///
/// Both realms go through the PRODUCTION constructor (`UiRealm::new`,
/// via `new_runtime`), matching every other coexistence proof in this
/// module — not the `for_test` bypass, so this test exercises the exact
/// path a real embedder would.
#[test]
fn cross_realm_duplicate_global_key_mounts_succeed_in_both() {
    #[derive(Clone)]
    struct GlobalKeyedRootView {
        key: flui_view::GlobalKey<Self>,
    }

    impl flui_view::RenderView for GlobalKeyedRootView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.set_size(
                Some(flui_types::Pixels::ZERO),
                Some(flui_types::Pixels::ZERO),
            )
        }
    }

    impl flui_view::View for GlobalKeyedRootView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    let shared_key = flui_view::GlobalKey::<GlobalKeyedRootView>::new();
    let realm_a = new_runtime(noop_wake()).expect("realm A claims cleanly");
    let realm_b = new_runtime(noop_wake()).expect("realm B claims cleanly ALONGSIDE realm A");

    realm_a
        .attach_root_widget_with_size(
            &GlobalKeyedRootView {
                key: shared_key.clone(),
            },
            10.0,
            10.0,
        )
        .expect("realm A mounts the keyed root");
    let _ = realm_a.draw_frame(coexistence_constraints());

    // Realm B wraps the SAME keyed view one layer deeper (a `SizedBox`
    // ancestor realm A's mount doesn't have) -- deliberately, so the two
    // trees are NOT structurally identical. A bare identical-shape mount
    // in both realms would legitimately allocate the same local slab
    // index in each realm's independent `ElementTree`, which would make
    // an `ElementId` equality/inequality assertion below pure
    // coincidence either way, discriminating nothing. With the shapes
    // different, a numeric match WOULD be suspicious.
    realm_b
        .attach_root_widget_with_size(
            &SizedBox::new(10.0, 10.0).child(GlobalKeyedRootView {
                key: shared_key.clone(),
            }),
            10.0,
            10.0,
        )
        .expect(
            "realm B mounts the SAME GlobalKey value without a duplicate-attachment \
             panic -- the eager collision check is scoped to one realm's own \
             ElementOwner, never process-wide",
        );
    let _ = realm_b.draw_frame(coexistence_constraints());

    let element_in_a = realm_a.enter(|_| shared_key.current_element());
    let element_in_b = realm_b.enter(|_| shared_key.current_element());

    assert!(element_in_a.is_some(), "realm A resolves its own mount");
    assert!(element_in_b.is_some(), "realm B resolves its own mount");
    assert_ne!(
        element_in_a, element_in_b,
        "each realm mounted its OWN distinct element under the same key value -- \
         structurally guaranteed distinct here (B's tree has one extra ancestor \
         element A's doesn't), not a coincidence of matching slab layouts"
    );

    // The distinctness above is a snapshot; the deeper isolation proof
    // is behavioral: tear realm A down entirely and confirm realm B's
    // registration is completely unaffected -- if the registries were
    // secretly shared, dropping A's `ElementOwner` would clear or
    // corrupt B's entry too.
    drop(realm_a);
    let element_in_b_after_a_dropped = realm_b.enter(|_| shared_key.current_element());
    assert_eq!(
        element_in_b_after_a_dropped, element_in_b,
        "realm B's registration under the shared key value must survive \
         realm A's teardown completely untouched"
    );
}

/// A hot-reload command mutates the exact presentation owned by this
/// realm and arms the realm's own redraw request. There is no process
/// singleton left for it to resolve instead: `UiRealm::for_test`
/// constructs a fully independent realm (its own pipeline, its own
/// `needs_redraw` flag), so this test's realm is structurally the only
/// thing the command can reach.
#[cfg(feature = "hot-reload")]
#[test]
fn hot_reload_command_applies_to_the_owned_presentation() {
    let realm = UiRealm::for_test();

    realm
        .command_sender()
        .request_hot_reload(flui_hot_reload::HotReloadTier::HotReload)
        .expect("inbox has room");

    let report = realm.drain_commands();

    assert_eq!(
        report.invoked, 1,
        "the hot-reload command must be applied, not dropped as stale"
    );
    // `drain_commands`'s `HotReload` arm arms the coalesced
    // `redraw_pending` flag (`take_redraw_request`) directly on this
    // exact realm -- the point being tested: it never resolves a
    // process-wide instance, only the realm the sender was vended from.
    assert!(realm.take_redraw_request());
}

/// Full restart is owned by the process supervisor, so the presentation
/// records the command as handled without arming a UI redraw.
#[cfg(feature = "hot-reload")]
#[test]
fn full_restart_command_does_not_arm_a_presentation_redraw() {
    let runtime = new_runtime(noop_wake()).expect("runtime");

    runtime
        .command_sender()
        .request_hot_reload(flui_hot_reload::HotReloadTier::FullRestart)
        .expect("inbox has room");

    let report = runtime.drain_commands();
    assert_eq!(report.invoked, 1);
    assert_eq!(report.dropped_stale, 0);
    assert!(!runtime.take_redraw_request());
}

// ========================================================================
// Frame pipeline, first-frame deferral, and Vsync — migrated from the
// retired `AppBinding`'s own test module (`binding.rs`, deleted alongside
// it). These are the frame-loop parity oracle: `draw_frame_entered`'s
// internal ordering (vsync tick before build, the async-driver
// mid-frame slot, the pipeline-failure retry path) and the first-frame
// deferral gate moved to `UiRealm` verbatim; only the receiver syntax
// changed (`binding.draw_frame(&realm, c)` -> `realm.draw_frame(c)`),
// never the assertions themselves.
//
// NOT migrated in this change (tracked as deferred, not silently
// dropped): the gesture-arena/pointer-dispatch tests
// (`shell_installed_arena_resolves_nested_tap_detectors_to_one_winner`,
// `root_gesture_scope_arbitrates_overlapping_detectors_once`,
// `realm_input_dispatch_keeps_gesture_state_isolated`,
// `pointer_input_boundary_drains_a_lone_deferred_winner`,
// `long_press_fires_at_its_deadline_with_no_further_input`,
// `resampled_contact_motion_keeps_the_frame_wake_gate_open`), the two
// scheduler-wake-hook-stealing tests (re-homed to `runner.rs` against
// the `install_platform_realm`-based once-per-thread seam),
// `frames_reenable_redirties_root_so_next_frame_paints_not_idle`
// (re-homed to `runner.rs`, same reason), the IME/text-input module,
// and the haptics/clipboard/performance-overlay modules (re-homed to
// `presentation.rs`/`runtime.rs`, whose state now owns them).
mod frame_pipeline_and_vsync;

// ========================================================================
// Presentation-owned text input — migrated from the retired
// `AppBinding`'s own test module (`binding.rs`, deleted alongside it).
// End-to-end against a headless `FakeTextInput`, including realm-routed
// IME dispatch through `handle_input_entered`.
// ========================================================================
mod presentation_text_input;

// ========================================================================
// Presentation forest — isolation suite (ADR-0043 §1)
//
// Production can install multiple presentations, while attaching widget
// content to a secondary remains a test-only seam until issue #559 adds
// per-presentation frame submission. These tests exercise the composite
// `GlobalKey` registry and hot-reload fan-out against a genuine N=2
// forest.
// ========================================================================
mod presentation_forest_isolation;

// ========================================================================
// Addressed input/keyboard routing (issue #555's addressed-routing slice hop-2) — every
// input/IME delivery lands on exactly the stamped presentation, except
// keyboard, which routes through FocusCoordinator's active presentation
// instead (ADR-0043 §4).
// ========================================================================
mod addressed_input_routing;

// ========================================================================
// Per-presentation redraw wake (issue #555's addressed-routing slice) — a redraw request that
// dirties one presentation's own pipeline must poke ITS OWN native
// window only, never a sibling's.
// ========================================================================
mod redraw_wake_routing;

// ========================================================================
// Presentation DPR seeding — a non-primary presentation's pipeline must
// be seeded with ITS OWN window's scale factor before construction, the
// same DPR-before-first-frame ordering the primary path already gets.
// ========================================================================
mod presentation_dpr_seeding;

// ========================================================================
// Async-task disposition audit (ADR-0043 §5) — a dropped presentation's
// in-flight AsyncDriver task must not reach a live sibling
// ========================================================================
mod async_completion_isolation;

// ========================================================================
// Dispose-during-teardown isolation (ADR-0043 §5 step 3)
// ========================================================================
mod dispose_teardown_isolation;

// ========================================================================
// Closing one presentation must be structurally invisible to a
// surviving sibling (ADR-0043's end-state invariant)
// ========================================================================
mod closing_one_presentation_is_invisible_to_siblings;

/// This module's own red-exploits: the segment gate's poll-decision
/// equivalence table, the close-mid-animation probe, and the
/// zero-produce (demand-driven idle) invariant. Distinct from
/// `frame_pipeline_and_vsync` (which pins the PRE-EXISTING deferral/vsync
/// behavior unedited) — these are the NEW clock-level guarantees this
/// slice adds.
// ========================================================================
// Presentation-frame transaction boundary (ADR-0048):
// a frame failure — a structured pipeline error or a panic that escaped
// every inner recovery layer — is contained to the one presentation
// whose frame it was. Siblings keep framing, the process survives, the
// last presented frame is retained (no zero/blank scene is submitted in
// its place), and the failure surfaces through the typed
// `FrameFailureReport` route instead of a silent skip.
// ========================================================================
mod frame_failure_containment;

mod frame_clock_segment_gate;
