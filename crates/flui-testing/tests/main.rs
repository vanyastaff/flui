//! Single-binary consolidation of flui-testing's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `flui_testing_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. The modules
//! here each drive a per-test `HeadlessBinding` (no singletons, no env vars,
//! no statics) — with one bounded exception worth naming, since it is exactly
//! the kind of thing this convention exists to catch.
//!
//! `log_capture` registers two sentinel dispatchers that live for the process.
//! They are process-wide by necessity — `tracing`'s per-callsite interest cache
//! is — but they hold no state, receive no events, and never occupy the global
//! default subscriber slot, so nothing here can observe another module's
//! capture.

#[path = "a11y_query.rs"]
mod a11y_query;
#[path = "async_driver.rs"]
mod async_driver;
#[path = "controller_restart.rs"]
mod controller_restart;
#[path = "headless_realm.rs"]
mod headless_realm;
#[path = "layout_builder_seam.rs"]
mod layout_builder_seam;
#[path = "lifecycle_panic_containment.rs"]
mod lifecycle_panic_containment;
#[path = "log_capture.rs"]
mod log_capture;
#[path = "mount_bootstrap.rs"]
mod mount_bootstrap;
#[path = "multi_presentation_clock.rs"]
mod multi_presentation_clock;
#[path = "owner_scope.rs"]
mod owner_scope;
#[path = "pointer_script_replay.rs"]
mod pointer_script_replay;
#[path = "post_frame_after_layout.rs"]
mod post_frame_after_layout;
#[path = "realm_driver.rs"]
mod realm_driver;
#[path = "self_rescheduling_local_post_frame.rs"]
mod self_rescheduling_local_post_frame;
#[path = "text_store_kit.rs"]
mod text_store_kit;

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

#[test]
fn headless_frame_driver_matrix() {
    run_table(
        "headless_frame_driver_matrix",
        &[
            ("realm_driver::logical_render_root_tracks_replacement_and_build_recovery", realm_driver::logical_render_root_tracks_replacement_and_build_recovery as fn()),
            ("a11y_query::a_button_in_the_render_tree_is_findable_by_role", a11y_query::a_button_in_the_render_tree_is_findable_by_role as fn()),
            ("mount_bootstrap::mount_root_installs_the_render_root_and_lays_it_out", mount_bootstrap::mount_root_installs_the_render_root_and_lays_it_out as fn()),
            ("mount_bootstrap::the_bound_binding_keeps_pumping_from_where_the_bootstrap_left_off", mount_bootstrap::the_bound_binding_keeps_pumping_from_where_the_bootstrap_left_off as fn()),
            ("layout_builder_seam::headless_pump_frame_runs_the_layout_builder_seam", layout_builder_seam::headless_pump_frame_runs_the_layout_builder_seam as fn()),
            ("post_frame_after_layout::post_frame_callback_runs_after_layout_in_the_same_pumped_frame", post_frame_after_layout::post_frame_callback_runs_after_layout_in_the_same_pumped_frame as fn()),
            ("self_rescheduling_local_post_frame::self_rescheduling_local_post_frame_callback_fires_exactly_once_per_pumped_frame", self_rescheduling_local_post_frame::self_rescheduling_local_post_frame_callback_fires_exactly_once_per_pumped_frame as fn()),
            ("async_driver::headless_wake_from_another_thread_is_polled_on_the_frame_thread", async_driver::headless_wake_from_another_thread_is_polled_on_the_frame_thread as fn()),
            ("controller_restart::second_run_ticks_from_its_own_start_not_a_stale_anchor", controller_restart::second_run_ticks_from_its_own_start_not_a_stale_anchor as fn()),
            ("pointer_script_replay::a_long_press_script_held_past_the_deadline_fires_it", pointer_script_replay::a_long_press_script_held_past_the_deadline_fires_it as fn()),
            ("pointer_script_replay::the_same_script_released_before_the_deadline_does_not", pointer_script_replay::the_same_script_released_before_the_deadline_does_not as fn()),
        ],
    );
}

#[test]
fn containment_and_isolation_matrix() {
    run_table(
        "containment_and_isolation_matrix",
        &[
            ("lifecycle_panic_containment::lifecycle_panic_containment_init_state_paints_exact_error_slot", lifecycle_panic_containment::lifecycle_panic_containment_init_state_paints_exact_error_slot as fn()),
            ("owner_scope::interaction_targets_are_isolated_between_headless_bindings", owner_scope::interaction_targets_are_isolated_between_headless_bindings as fn()),
            ("owner_scope::pointer_route_panic_still_runs_the_down_arena_lifecycle", owner_scope::pointer_route_panic_still_runs_the_down_arena_lifecycle as fn()),
            ("multi_presentation_clock::two_presentations_at_independent_scripted_cadences_tick_and_advance_independently", multi_presentation_clock::two_presentations_at_independent_scripted_cadences_tick_and_advance_independently as fn()),
        ],
    );
}
