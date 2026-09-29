use std::sync::atomic::{AtomicBool, AtomicUsize};

use flui_view::prelude::*;
use flui_widgets::{NavigatorCommand, NavigatorHandle, SimpleRoute, SizedBox};

use super::*;
use crate::testing::ScriptedSink;

static_assertions::assert_not_impl_any!(UiRealm: Send, Sync);

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

/// A headless window as a runner hands it to a realm: the window and the
/// accessibility bridge its backend fixed, read once (what `flui-app`'s
/// `runner::presentation_window` does), so the headless backend's bridge is
/// wired exactly as a production window's would be.
fn test_window() -> crate::presentation::PresentationWindow {
    let host = flui_platform::headless_platform()
        .open_window(flui_platform::traits::WindowOptions::default())
        .expect("headless platform should create a test window");
    let accessibility = host.accessibility();
    crate::presentation::PresentationWindow::new(host, accessibility)
}

fn new_runtime(wake: Arc<dyn Fn() + Send + Sync>) -> Result<UiRealm, UiRealmError> {
    UiRealm::new(
        wake,
        test_window(),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
    )
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
        crate::presentation::test_clipboard(),
    )
}

#[test]
fn full_inbox_retries_outstanding_wake_debt_before_rejecting() {
    let fail_next_wake = Arc::new(AtomicBool::new(true));
    let fail_next_wake_in_callback = Arc::clone(&fail_next_wake);
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_count_in_callback = Arc::clone(&wake_count);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_count_in_callback.fetch_add(1, Ordering::Relaxed);
        assert!(
            !fail_next_wake_in_callback.swap(false, Ordering::Relaxed),
            "initial command wake probe"
        );
    });
    let runtime = new_runtime_with_capacity(1, wake).expect("runtime with one command slot");
    let sender = runtime.command_sender();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    let filler = || NavigatorCommand::maybe_pop(navigator.command_target());

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sender.send_navigation(filler())
    }));
    assert!(
        first.is_err(),
        "the accepted command's first wake must fail"
    );

    let overflow = sender
        .send_navigation(filler())
        .expect_err("the accepted command still fills the inbox");
    assert!(matches!(
        overflow,
        CommandSendError::ChannelFull { capacity: 1, .. }
    ));
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        2,
        "the full path must retry the first command's wake debt"
    );

    assert_eq!(runtime.drain_commands().invoked, 1);
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        2,
        "successful debt delivery is acknowledged exactly once"
    );
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
    BoxConstraints::tight(Size::new(200.0, 200.0))
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
// `UiRealm::pump`, the frame transaction (ADR-0083 §1): its phases, their
// order, and the frame clock it publishes; and `pump_background`.
// ========================================================================
mod pump_transaction;

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
// Async-task disposition audit (ADR-0043 §5) — a dropped presentation's
// in-flight AsyncDriver task must not reach a live sibling
// ========================================================================
mod async_completion_isolation;

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

// ========================================================================
// A GlobalKey read inside a presentation's own frame returns instead of
// re-entering that presentation's frame lock.
// ========================================================================
mod global_key_lookup_during_frame;

// ========================================================================
// Cross-thread signal writes run against the graph that minted the slot,
// in whichever presentation owns it (ADR-0085 §1).
// ========================================================================
mod signal_write_routing;
