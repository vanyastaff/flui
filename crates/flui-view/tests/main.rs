//! Single-binary consolidation of flui-view's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `view_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! paths (`include_str!`, `#[path]`) and manifest-relative paths
//! (trybuild's `tests/ui/`) keep working unchanged.
//!
//! Convention: tests that WRITE process-global state (e.g. the
//! error-view builder) live in their own [[test]] target instead —
//! process isolation beats opt-in locking. See error_view_recovery.

#[path = "ancestor_finders.rs"]
mod ancestor_finders;
#[path = "build_owner_tests.rs"]
mod build_owner_tests;
#[path = "dense_reconcile_containment.rs"]
mod dense_reconcile_containment;
#[path = "dense_update_containment.rs"]
mod dense_update_containment;
#[path = "global_key.rs"]
mod global_key;
#[path = "global_key_duplication.rs"]
mod global_key_duplication;
#[path = "global_key_reparent.rs"]
mod global_key_reparent;
#[path = "inherited_dependency.rs"]
mod inherited_dependency;
#[path = "lifecycle_panic_containment.rs"]
mod lifecycle_panic_containment;
#[path = "support/lifecycle_recovery.rs"]
mod lifecycle_recovery;
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "notifications.rs"]
mod notifications;
#[path = "orphaned_render_mount.rs"]
mod orphaned_render_mount;
#[path = "production_reconcile_emits.rs"]
mod production_reconcile_emits;
#[path = "reconcile_capture.rs"]
mod reconcile_capture;
#[path = "recovered_panics.rs"]
mod recovered_panics;
#[path = "runtime_seam.rs"]
mod runtime_seam;
#[path = "signal_reads.rs"]
mod signal_reads;
#[path = "stateless_stateful_tests.rs"]
mod stateless_stateful_tests;
#[path = "trybuild_ui.rs"]
mod trybuild_ui;
#[path = "writer_source.rs"]
mod writer_source;

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
fn dense_and_production_reconcile_matrix() {
    run_table(
        "dense_and_production_reconcile_matrix",
        &[
            ("dense_reconcile_containment::dense_mount_panic_substitutes_at_exact_slot_and_preserves_topology", dense_reconcile_containment::dense_mount_panic_substitutes_at_exact_slot_and_preserves_topology as fn()),
            ("dense_reconcile_containment::repeated_dense_mount_panics_do_not_accumulate_ghosts", dense_reconcile_containment::repeated_dense_mount_panics_do_not_accumulate_ghosts as fn()),
            ("dense_update_containment::phase_one_did_update_view_panic_substitutes_at_same_slot", dense_update_containment::phase_one_did_update_view_panic_substitutes_at_same_slot as fn()),
            ("dense_update_containment::phase_five_a_shifted_suffix_update_panic_uses_final_slot", dense_update_containment::phase_five_a_shifted_suffix_update_panic_uses_final_slot as fn()),
            #[cfg(feature = "test-utils")]
            ("production_reconcile_emits::object_keys_follow_retained_allocations_through_reorder", production_reconcile_emits::object_keys_follow_retained_allocations_through_reorder as fn()),
            #[cfg(feature = "test-utils")]
            ("production_reconcile_emits::active_global_key_move_through_build_scope_updates_render_parent_links", production_reconcile_emits::active_global_key_move_through_build_scope_updates_render_parent_links as fn()),
            #[cfg(feature = "test-utils")]
            ("production_reconcile_emits::failed_dense_mount_production_reconcile_emits_only_final_slots", production_reconcile_emits::failed_dense_mount_production_reconcile_emits_only_final_slots as fn()),
        ],
    );
}

#[test]
fn global_key_contract_matrix() {
    run_table(
        "global_key_contract_matrix",
        &[
            ("global_key::global_key_state_migrates_to_new_parent_slot", global_key::global_key_state_migrates_to_new_parent_slot as fn()),
            ("global_key_duplication::a_second_parent_grafts_the_same_element_rather_than_creating_another", global_key_duplication::a_second_parent_grafts_the_same_element_rather_than_creating_another as fn()),
            ("global_key_duplication::two_parents_declaring_one_key_in_one_frame_are_reported", global_key_duplication::two_parents_declaring_one_key_in_one_frame_are_reported as fn()),
            #[cfg(feature = "test-utils")]
            ("global_key_reparent::active_to_active_reparent_emits_from_parent_and_preserves_state", global_key_reparent::active_to_active_reparent_emits_from_parent_and_preserves_state as fn()),
        ],
    );
}

#[test]
fn lifecycle_panic_containment_matrix() {
    if let Ok(kind) = std::env::var("FLUI_LIFECYCLE_RECOVERY_CHILD") {
        lifecycle_recovery::dispatch_child(&kind);
        return;
    }
    if let Ok(kind) = std::env::var("FLUI_BUILD_PAYLOAD_RECOVERY_CHILD") {
        lifecycle_panic_containment::build_payload_recovery::dispatch_child(&kind);
        return;
    }
    if let Ok(kind) = std::env::var("FLUI_OBSERVER_RECOVERY_CHILD") {
        lifecycle_panic_containment::observer_recovery::dispatch_child(&kind);
        return;
    }
    run_table(
        "lifecycle_panic_containment_matrix",
        &[
            ("lifecycle_panic_containment::child_init_state_panic_is_replaced_in_place_and_the_build_scope_continues", lifecycle_panic_containment::child_init_state_panic_is_replaced_in_place_and_the_build_scope_continues as fn()),
            ("lifecycle_panic_containment::a_dispose_panic_during_finalize_is_contained_and_the_slot_is_freed", lifecycle_panic_containment::a_dispose_panic_during_finalize_is_contained_and_the_slot_is_freed as fn()),
            ("lifecycle_panic_containment::a_dispose_panic_on_a_global_keyed_element_still_clears_the_registry", lifecycle_panic_containment::a_dispose_panic_on_a_global_keyed_element_still_clears_the_registry as fn()),
            ("lifecycle_panic_containment::a_deactivate_panic_is_contained_and_the_element_is_still_parked_inactive", lifecycle_panic_containment::a_deactivate_panic_is_contained_and_the_element_is_still_parked_inactive as fn()),
            ("recovered_panics::a_contained_build_panic_is_recorded_once_with_its_element_and_hook", recovered_panics::a_contained_build_panic_is_recorded_once_with_its_element_and_hook as fn()),
            ("lifecycle_recovery::self_cancelled_callback_failure_retains_its_capture_aggregate", lifecycle_recovery::self_cancelled_callback_failure_retains_its_capture_aggregate as fn()),
            ("lifecycle_recovery::self_cancellation_retains_competing_payload_and_capture_aggregates", lifecycle_recovery::self_cancellation_retains_competing_payload_and_capture_aggregates as fn()),
            ("lifecycle_recovery::terminal_release_keeps_the_earlier_callback_failure", lifecycle_recovery::terminal_release_keeps_the_earlier_callback_failure as fn()),
            ("lifecycle_recovery::release_retains_later_envelopes_after_first_retirement_failure", lifecycle_recovery::release_retains_later_envelopes_after_first_retirement_failure as fn()),
            ("lifecycle_recovery::subscription_retirement_during_unwind_retains_its_envelope", lifecycle_recovery::subscription_retirement_during_unwind_retains_its_envelope as fn()),
            ("lifecycle_recovery::source_retirement_during_unwind_retains_its_envelopes", lifecycle_recovery::source_retirement_during_unwind_retains_its_envelopes as fn()),
            ("lifecycle_recovery::successful_lifecycle_cancellation_retires_captures_and_keeps_fifo", lifecycle_recovery::successful_lifecycle_cancellation_retires_captures_and_keeps_fifo as fn()),
            ("build_payload_recovery::aggregate_build_payload_is_retained_before_recovery", lifecycle_panic_containment::build_payload_recovery::aggregate_build_payload_is_retained_before_recovery as fn()),
            ("build_payload_recovery::recovery_reporting_preserves_original_attribution", lifecycle_panic_containment::build_payload_recovery::recovery_reporting_preserves_original_attribution as fn()),
            ("build_payload_recovery::original_and_reporting_payloads_do_not_compete_at_retirement", lifecycle_panic_containment::build_payload_recovery::original_and_reporting_payloads_do_not_compete_at_retirement as fn()),
            ("build_payload_recovery::recovery_view_survives_reporting_with_opaque_captures", lifecycle_panic_containment::build_payload_recovery::recovery_view_survives_reporting_with_opaque_captures as fn()),
            ("build_payload_recovery::recovery_factory_failure_keeps_its_authority_after_opaque_build_failure", lifecycle_panic_containment::build_payload_recovery::recovery_factory_failure_keeps_its_authority_after_opaque_build_failure as fn()),
            ("build_payload_recovery::staged_lifecycle_attribution_survives_reporting_failure", lifecycle_panic_containment::build_payload_recovery::staged_lifecycle_attribution_survives_reporting_failure as fn()),
            ("observer_recovery::emission_retains_aggregate_payload", lifecycle_panic_containment::observer_recovery::emission_retains_aggregate_payload as fn()),
            ("observer_recovery::emission_retains_failed_capture_envelope", lifecycle_panic_containment::observer_recovery::emission_retains_failed_capture_envelope as fn()),
            ("observer_recovery::emission_retains_competing_payload_and_captures", lifecycle_panic_containment::observer_recovery::emission_retains_competing_payload_and_captures as fn()),
            ("observer_recovery::emission_contains_reporting_failure", lifecycle_panic_containment::observer_recovery::emission_contains_reporting_failure as fn()),
            ("observer_recovery::emission_contains_all_opaque_obligations", lifecycle_panic_containment::observer_recovery::emission_contains_all_opaque_obligations as fn()),
            ("observer_recovery::detach_retains_aggregate_payload", lifecycle_panic_containment::observer_recovery::detach_retains_aggregate_payload as fn()),
            ("observer_recovery::detach_retains_failed_capture_envelope", lifecycle_panic_containment::observer_recovery::detach_retains_failed_capture_envelope as fn()),
            ("observer_recovery::detach_contains_all_opaque_obligations", lifecycle_panic_containment::observer_recovery::detach_contains_all_opaque_obligations as fn()),
            ("observer_recovery::replacement_preserves_its_new_observer_after_detach_failure", lifecycle_panic_containment::observer_recovery::replacement_preserves_its_new_observer_after_detach_failure as fn()),
        ],
    );
}

#[test]
fn element_lifecycle_and_dependency_matrix() {
    run_table(
        "element_lifecycle_and_dependency_matrix",
        &[
            ("lifecycle_tests::test_stateful_element_multiple_deactivate_activate_cycles", lifecycle_tests::test_stateful_element_multiple_deactivate_activate_cycles as fn()),
            ("stateless_stateful_tests::test_stateful_element_update_calls_did_update_view", stateless_stateful_tests::test_stateful_element_update_calls_did_update_view as fn()),
            ("stateless_stateful_tests::stateful_activate_and_deactivate_require_completed_init_state", stateless_stateful_tests::stateful_activate_and_deactivate_require_completed_init_state as fn()),
            ("notifications::dispatch_notification_calls_handler_and_stops_on_true", notifications::dispatch_notification_calls_handler_and_stops_on_true as fn()),
            ("ancestor_finders::find_ancestor_view_returns_nearest_match", ancestor_finders::find_ancestor_view_returns_nearest_match as fn()),
            ("inherited_dependency::inherited_update_notifies_dependents", inherited_dependency::inherited_update_notifies_dependents as fn()),
            ("inherited_dependency::unmounted_dependent_is_removed_from_provider_before_next_notification", inherited_dependency::unmounted_dependent_is_removed_from_provider_before_next_notification as fn()),
            ("build_owner_tests::build_owners_have_isolated_focus_managers", build_owner_tests::build_owners_have_isolated_focus_managers as fn()),
            ("build_owner_tests::test_build_scope_processes_in_depth_order", build_owner_tests::test_build_scope_processes_in_depth_order as fn()),
        ],
    );
}

#[test]
fn signal_read_and_write_matrix() {
    if signal_reads::run_released_loan_child() {
        return;
    }
    run_table(
        "signal_read_and_write_matrix",
        &[
            (
                "signal_reads::released_update_reports_ordinary_retirement_failure",
                signal_reads::released_update_reports_ordinary_retirement_failure as fn(),
            ),
            (
                "signal_reads::released_read_reports_ordinary_retirement_failure",
                signal_reads::released_read_reports_ordinary_retirement_failure as fn(),
            ),
            (
                "signal_reads::released_update_retains_aggregate_before_resuming_failure",
                signal_reads::released_update_retains_aggregate_before_resuming_failure as fn(),
            ),
            (
                "signal_reads::released_read_retains_aggregate_before_resuming_failure",
                signal_reads::released_read_retains_aggregate_before_resuming_failure as fn(),
            ),
            (
                "signal_reads::released_update_retains_nested_release_obligations",
                signal_reads::released_update_retains_nested_release_obligations as fn(),
            ),
            (
                "signal_reads::released_read_retains_nested_release_obligations",
                signal_reads::released_read_retains_nested_release_obligations as fn(),
            ),
            (
                "signal_reads::a_read_in_build_subscribes_through_the_production_context",
                signal_reads::a_read_in_build_subscribes_through_the_production_context as fn(),
            ),
            (
                "signal_reads::a_partially_committed_panicking_update_rebuilds_its_mounted_reader",
                signal_reads::a_partially_committed_panicking_update_rebuilds_its_mounted_reader
                    as fn(),
            ),
            (
                "writer_source::writer_source_from_init_state_writes_and_rebuilds_the_reader",
                writer_source::writer_source_from_init_state_writes_and_rebuilds_the_reader
                    as fn(),
            ),
        ],
    );
}
