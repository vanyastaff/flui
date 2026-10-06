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

pub(super) fn new_runtime(wake: Arc<dyn Fn() + Send + Sync>) -> Result<UiRealm, UiRealmError> {
    UiRealm::new(
        wake,
        test_window(),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
        None,
        &flui_painting::FontCollection::new(),
        flui_scheduler::ClockSource::Platform,
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
        None,
        &flui_painting::FontCollection::new(),
        flui_scheduler::ClockSource::Platform,
    )
}

pub(crate) fn full_inbox_retries_outstanding_wake_debt_before_rejecting() {
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
pub(crate) fn two_realms_two_threads_no_shared_state() {
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

/// Across two coexisting realms, dropping realm A must leave realm B's
/// wake counter and inbox completely untouched, and A's own senders must
/// turn `CommandSendError::OwnerGone` rather than silently reaching B.
pub(crate) fn dropping_realm_a_cannot_wake_realm_b() {
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
// `SemanticsAgent`: an agent's wire read of, and actions on, a
// presentation's committed semantics tree through the owner inbox
// (ADR-0095 §3).
// ========================================================================
mod agent_semantics;

// ========================================================================
// Frame pipeline, first-frame deferral, and Vsync — migrated from the
// retired `AppBinding`'s own test module (`binding.rs`, deleted alongside
// it). These are the frame-loop parity oracle: `draw_frame_entered`'s
// internal ordering (vsync tick before build, the async-driver
// mid-frame slot, the pipeline-failure retry path) and the first-frame
// deferral gate moved to `UiRealm` verbatim; only the receiver syntax
// changed (`binding.draw_frame(&realm, c)` -> `realm.draw_frame(c)`),
// never the assertions themselves.
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
#[cfg(feature = "hot-reload")]
mod hot_reload_recovery;
mod signal_write_routing;

#[test]
fn wake_debt_and_signal_write_matrix() {
    crate::table_test::run_table(
        "wake_debt_and_signal_write_matrix",
        &[
            ("signal_write_routing::a_write_to_a_secondary_presentations_signal_rebuilds_its_reader", signal_write_routing::a_write_to_a_secondary_presentations_signal_rebuilds_its_reader as fn()),
            ("signal_write_routing::stale_signal_command_disposal_panic_rearms_its_fifo_tail", signal_write_routing::stale_signal_command_disposal_panic_rearms_its_fifo_tail as fn()),
            ("signal_write_routing::a_failed_signal_write_rearm_retries_at_the_next_owner_boundary", signal_write_routing::a_failed_signal_write_rearm_retries_at_the_next_owner_boundary as fn()),
            ("signal_write_routing::an_older_overlapping_wake_cannot_clear_newer_failed_delivery_debt", signal_write_routing::an_older_overlapping_wake_cannot_clear_newer_failed_delivery_debt as fn()),
            ("redraw_wake_routing::a_cross_thread_frame_request_reaches_the_realms_platform_wake", redraw_wake_routing::a_cross_thread_frame_request_reaches_the_realms_platform_wake as fn()),
            ("addressed_input_routing::panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake", addressed_input_routing::panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake as fn()),
            #[cfg(feature = "hot-reload")]
            ("hot_reload_recovery::failed_reload_wake_rearms_the_accepted_tail", hot_reload_recovery::failed_reload_wake_rearms_the_accepted_tail as fn()),
            #[cfg(feature = "hot-reload")]
            ("hot_reload_recovery::competing_reload_wakes_preserve_the_first_failure_and_retry", hot_reload_recovery::competing_reload_wakes_preserve_the_first_failure_and_retry as fn()),
            #[cfg(feature = "hot-reload")]
            ("hot_reload_recovery::reassemble_failure_keeps_priority_over_a_failed_rearm", hot_reload_recovery::reassemble_failure_keeps_priority_over_a_failed_rearm as fn()),
            #[cfg(feature = "hot-reload")]
            ("hot_reload_recovery::reassemble_failure_rearms_the_accepted_tail", hot_reload_recovery::reassemble_failure_rearms_the_accepted_tail as fn()),
            ("full_inbox_retries_outstanding_wake_debt_before_rejecting", full_inbox_retries_outstanding_wake_debt_before_rejecting as fn()),
        ],
    );
}

#[test]
fn realm_and_presentation_isolation_matrix() {
    if let Ok(kind) = std::env::var("FLUI_PRESENTATION_CLOSE_CHILD") {
        closing_one_presentation_is_invisible_to_siblings::run_presentation_close_child(&kind);
        return;
    }
    crate::table_test::run_table(
        "realm_and_presentation_isolation_matrix",
        &[
            ("exhausted_incarnations_never_alias_previous_realms", crate::realm_services::exhausted_incarnations_never_alias_previous_realms as fn()),
            ("addressed_input_routing::input_stamped_for_b_never_reaches_as_arena", addressed_input_routing::input_stamped_for_b_never_reaches_as_arena as fn()),
            ("async_completion_isolation::async_completion_after_presentation_teardown_fails_closed_no_sibling_reach", async_completion_isolation::async_completion_after_presentation_teardown_fails_closed_no_sibling_reach as fn()),
            ("closing_one_presentation_is_invisible_to_siblings::closing_presentation_a_leaves_sibling_layer_tree_identical", closing_one_presentation_is_invisible_to_siblings::closing_presentation_a_leaves_sibling_layer_tree_identical as fn()),
            ("closing_one_presentation_is_invisible_to_siblings::presentation_close_retirement_failures_preserve_focus_ime_and_siblings", closing_one_presentation_is_invisible_to_siblings::presentation_close_retirement_failures_preserve_focus_ime_and_siblings as fn()),
            ("global_key_lookup_during_frame::state_read_across_presentations_during_a_segment_resolves", global_key_lookup_during_frame::state_read_across_presentations_during_a_segment_resolves as fn()),
            ("presentation_forest_isolation::sibling_presentations_flush_independently", presentation_forest_isolation::sibling_presentations_flush_independently as fn()),
            ("two_realms_two_threads_no_shared_state", two_realms_two_threads_no_shared_state as fn()),
            ("dropping_realm_a_cannot_wake_realm_b", dropping_realm_a_cannot_wake_realm_b as fn()),
        ],
    );
}

#[test]
fn frame_pacing_and_pump_matrix() {
    crate::table_test::run_table(
        "frame_pacing_and_pump_matrix",
        &[
            ("frame_clock_segment_gate::segment_runs_iff_woken_or_has_pending_work_over_the_full_table", frame_clock_segment_gate::segment_runs_iff_woken_or_has_pending_work_over_the_full_table as fn()),
            ("frame_clock_segment_gate::n_ticks_under_backpressure_wake_the_platform_exactly_once_then_rearm", frame_clock_segment_gate::n_ticks_under_backpressure_wake_the_platform_exactly_once_then_rearm as fn()),
            ("frame_clock_segment_gate::occlude_then_dirty_then_unocclude_wakes_exactly_once_and_produces_exactly_once", frame_clock_segment_gate::occlude_then_dirty_then_unocclude_wakes_exactly_once_and_produces_exactly_once as fn()),
            ("frame_clock_segment_gate::surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame", frame_clock_segment_gate::surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame as fn()),
            ("frame_pipeline_and_vsync::attach_root_widget_bootstraps_shared_render_tree", frame_pipeline_and_vsync::attach_root_widget_bootstraps_shared_render_tree as fn()),
            ("frame_pipeline_and_vsync::the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline", frame_pipeline_and_vsync::the_production_frame_polls_the_realms_async_driver_once_before_the_pipeline as fn()),
            ("frame_pipeline_and_vsync::surface_lost_keeps_needs_redraw_armed_for_a_retry", frame_pipeline_and_vsync::surface_lost_keeps_needs_redraw_armed_for_a_retry as fn()),
            ("pump_transaction::pump_post_frame_callback_observes_this_frames_committed_layout", pump_transaction::pump_post_frame_callback_observes_this_frames_committed_layout as fn()),
            ("presentation_text_input::a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns", presentation_text_input::a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns as fn()),
        ],
    );
}

#[test]
fn frame_failure_containment_matrix() {
    if let Ok(kind) = std::env::var("FLUI_OPAQUE_FRAME_CHILD") {
        frame_failure_containment::run_opaque_frame_child(&kind);
        return;
    }
    crate::table_test::run_table(
        "frame_failure_containment_matrix",
        &[
            ("frame_failure_containment::a_failed_handler_envelope_is_retained_through_realm_teardown", frame_failure_containment::a_failed_handler_envelope_is_retained_through_realm_teardown as fn()),
            ("frame_failure_containment::competing_opaque_frame_failures_keep_one_report_and_retry", frame_failure_containment::competing_opaque_frame_failures_keep_one_report_and_retry as fn()),
            ("frame_failure_containment::opaque_diagnostics_cannot_suppress_the_frame_handler", frame_failure_containment::opaque_diagnostics_cannot_suppress_the_frame_handler as fn()),
            ("frame_failure_containment::an_opaque_handler_failure_preserves_frame_recovery", frame_failure_containment::an_opaque_handler_failure_preserves_frame_recovery as fn()),
            ("frame_failure_containment::a_segment_opaque_payload_is_retained_before_recovery", frame_failure_containment::a_segment_opaque_payload_is_retained_before_recovery as fn()),
            ("frame_failure_containment::an_escaped_segment_panic_is_contained_to_its_own_presentation_and_the_sibling_still_frames", frame_failure_containment::an_escaped_segment_panic_is_contained_to_its_own_presentation_and_the_sibling_still_frames as fn()),
            ("frame_failure_containment::consecutive_failures_count_up_and_reset_on_a_clean_segment", frame_failure_containment::consecutive_failures_count_up_and_reset_on_a_clean_segment as fn()),
            ("frame_failure_containment::a_panicking_handler_during_a_pipeline_report_is_delivered_once_not_re_reported", frame_failure_containment::a_panicking_handler_during_a_pipeline_report_is_delivered_once_not_re_reported as fn()),
            ("super::frame_failure_phase_tests::every_segment_phase_survives_unwind_and_retries_to_scene", super::frame_failure_phase_tests::every_segment_phase_survives_unwind_and_retries_to_scene as fn()),
            ("super::frame_failure_recovery_tests::real_build_recovery_is_reported_once_in_the_same_attempt", super::frame_failure_recovery_tests::real_build_recovery_is_reported_once_in_the_same_attempt as fn()),
            ("super::frame_failure_recovery_tests::two_real_recoveries_from_one_attempt_are_delivered_in_production_queue_order", super::frame_failure_recovery_tests::two_real_recoveries_from_one_attempt_are_delivered_in_production_queue_order as fn()),
            ("super::frame_failure_recovery_tests::panicking_handler_does_not_escape_or_duplicate_recovery", super::frame_failure_recovery_tests::panicking_handler_does_not_escape_or_duplicate_recovery as fn()),
            ("super::frame_commit_state_tests::errored_frame_is_uncommitted", super::frame_commit_state_tests::errored_frame_is_uncommitted as fn()),
            ("super::frame_commit_state_tests::deferred_painted_frame_waits_for_the_later_present_to_commit", super::frame_commit_state_tests::deferred_painted_frame_waits_for_the_later_present_to_commit as fn()),
            ("super::frame_commit_state_tests::the_withheld_retry_is_bounded_and_then_parks", super::frame_commit_state_tests::the_withheld_retry_is_bounded_and_then_parks as fn()),
        ],
    );
}

// ========================================================================
// Each realm owns one `TextContext` over the app's `FontCollection`
// (ADR-0092 §3): built at construction, dropped with the realm, none per
// presentation.
// ========================================================================
mod text_context;

// ========================================================================
// A face registered on the app's collection re-lays out every realm's text
// at its next frame (ADR-0092 §10 step 3b).
// ========================================================================
mod font_registration;
