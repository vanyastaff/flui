//! Contract tables: each `#[test]` here is one capability family whose rows are the
//! scenario functions of that family. A failing row is named in the panic message, and
//! one failing row never hides the others.

use crate::common::cases::run_cases;

/// Navigator failure paths: panicking hooks and factories, mismatched result types,
/// undeliverable results, and every re-entrant push or observer callback that must not
/// deadlock the history lock.
#[test]
fn navigator_failure_containment_and_reentrancy() {
    run_cases(
        "navigator_failure_containment_and_reentrancy",
        &[
            ("navigator::a_panicking_route_hook_leaves_the_navigator_usable", crate::navigator::a_panicking_route_hook_leaves_the_navigator_usable as fn()),
            ("navigator::navigator_of_then_push_from_a_route_build_does_not_deadlock", crate::navigator::navigator_of_then_push_from_a_route_build_does_not_deadlock),
            ("navigator_public::a_factory_that_panics_after_the_result_is_erased_loses_only_the_report", crate::navigator_public::a_factory_that_panics_after_the_result_is_erased_loses_only_the_report),
            ("navigator_public::a_factory_that_pushes_re_entrantly_does_not_deadlock", crate::navigator_public::a_factory_that_pushes_re_entrantly_does_not_deadlock),
            ("navigator_public::a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock", crate::navigator_public::a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock),
            ("navigator_public::every_operation_that_cannot_deliver_a_result_reports_it", crate::navigator_public::every_operation_that_cannot_deliver_a_result_reports_it),
            ("navigator_public::delivered_route_results_remain_completed", crate::navigator_public::delivered_route_results_remain_completed),
            ("navigator_public::push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing", crate::navigator_public::push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing),
            ("navigator_public::a_route_key_carries_its_result_type_from_registration_to_delivery", crate::navigator_public::a_route_key_carries_its_result_type_from_registration_to_delivery),
            ("hero_controller::a_hero_controller_does_not_deadlock_the_observer_callback", crate::hero_controller::a_hero_controller_does_not_deadlock_the_observer_callback),
            ("hero_seam::an_observer_may_push_from_did_push_without_deadlocking", crate::hero_seam::an_observer_may_push_from_did_push_without_deadlocking),
            ("transition_route::status_listener_does_not_hold_a_lock_across_the_binding_call", crate::transition_route::status_listener_does_not_hold_a_lock_across_the_binding_call),
            ("transition_route::pop_mid_push_cancels_the_push_future_inside_the_flush_and_ends_popping", crate::transition_route::pop_mid_push_cancels_the_push_future_inside_the_flush_and_ends_popping),
            ("transition_route::dispose_releases_the_controller_slot_before_disposing_it", crate::transition_route::dispose_releases_the_controller_slot_before_disposing_it),
        ],
    );
}

/// A user callback that fails (a refused write, a panic, a stale handle, a pointer
/// cancel) is contained: reported, never propagated, and the next operation still works.
#[test]
fn callback_failure_containment() {
    run_cases(
        "callback_failure_containment",
        &[
            ("editable_text::event_cx::a_refused_write_in_on_changed_is_reported_not_panicked", crate::editable_text::event_cx::a_refused_write_in_on_changed_is_reported_not_panicked as fn()),
            ("focus::event_cx::a_refused_write_in_a_focus_edge_is_reported_not_panicked", crate::focus::event_cx::a_refused_write_in_a_focus_edge_is_reported_not_panicked),
            ("gesture_detector::event_cx::a_panicking_assistive_action_does_not_discard_the_fifo_tail", crate::gesture_detector::event_cx::a_panicking_assistive_action_does_not_discard_the_fifo_tail),
            ("listener::event_cx::a_refused_write_in_a_pointer_callback_is_reported_not_panicked", crate::listener::event_cx::a_refused_write_in_a_pointer_callback_is_reported_not_panicked),
            ("raw_button::a_refused_write_in_a_press_is_reported_not_panicked", crate::raw_button::a_refused_write_in_a_press_is_reported_not_panicked),
            ("draggable_events::a_refused_write_in_a_target_callback_is_reported_not_panicked", crate::draggable_events::a_refused_write_in_a_target_callback_is_reported_not_panicked),
            ("semantics::a_refused_write_in_an_action_handler_is_reported_not_panicked", crate::semantics::a_refused_write_in_an_action_handler_is_reported_not_panicked),
            ("semantics::an_action_invoked_outside_its_realm_is_dropped_with_a_warning", crate::semantics::an_action_invoked_outside_its_realm_is_dropped_with_a_warning),
            ("text_field_widget::raw_text_field_callbacks_write_through_the_forwarded_cx", crate::text_field_widget::raw_text_field_callbacks_write_through_the_forwarded_cx),
            ("form::a_panicking_reset_callback_does_not_disable_later_form_validation", crate::form::a_panicking_reset_callback_does_not_disable_later_form_validation),
            ("form::a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first", crate::form::a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first),
            ("gesture_detector::horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector", crate::gesture_detector::horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector),
            ("media_query_fields::a_build_that_panics_before_reading_keeps_its_dependency", crate::media_query_fields::a_build_that_panics_before_reading_keeps_its_dependency),
            ("signals::writes_and_creations_inside_build_are_refused_by_the_runtime", crate::signals::writes_and_creations_inside_build_are_refused_by_the_runtime),
            ("signals::a_stale_handle_read_in_build_is_a_typed_error_through_try_get", crate::signals::a_stale_handle_read_in_build_is_a_typed_error_through_try_get),
            ("sliver_persistent_header::a_swap_that_shrinks_max_extent_never_hands_the_delegate_an_out_of_range_pair", crate::sliver_persistent_header::a_swap_that_shrinks_max_extent_never_hands_the_delegate_an_out_of_range_pair),
        ],
    );
}

/// Signals, inherited-dependency tracking and hot reassemble: what rebuilds, and what
/// must not.
#[test]
fn reactivity_and_dependencies() {
    run_cases(
        "reactivity_and_dependencies",
        &[
            ("signals::writing_a_signal_rebuilds_exactly_its_readers", crate::signals::writing_a_signal_rebuilds_exactly_its_readers as fn()),
            ("signals::same_depth_readers_rebuild_in_child_order", crate::signals::same_depth_readers_rebuild_in_child_order),
            ("signals_legal_shapes::the_accepted_shapes_compile_and_run", crate::signals_legal_shapes::the_accepted_shapes_compile_and_run),
            ("editable_text::event_cx::typing_writes_through_on_changed_and_rebuilds_its_reader", crate::editable_text::event_cx::typing_writes_through_on_changed_and_rebuilds_its_reader),
            ("gesture_detector::event_cx::a_tap_writes_a_signal_and_rebuilds_its_reader", crate::gesture_detector::event_cx::a_tap_writes_a_signal_and_rebuilds_its_reader),
            ("gesture_detector::event_cx::repeated_assistive_taps_are_delivered_once_each_and_keep_making_progress", crate::gesture_detector::event_cx::repeated_assistive_taps_are_delivered_once_each_and_keep_making_progress),
            ("gesture_detector::event_cx::repeated_assistive_long_presses_are_delivered_once_each_and_keep_making_progress", crate::gesture_detector::event_cx::repeated_assistive_long_presses_are_delivered_once_each_and_keep_making_progress),
            ("draggable_events::a_drop_writes_through_the_targets_on_accept_before_the_draggable_completes", crate::draggable_events::a_drop_writes_through_the_targets_on_accept_before_the_draggable_completes),
            ("draggable_events::a_drag_leaving_a_target_writes_through_on_leave_and_on_move", crate::draggable_events::a_drag_leaving_a_target_writes_through_on_leave_and_on_move),
            ("semantics::an_action_handler_writes_a_signal_and_rebuilds_its_reader", crate::semantics::an_action_handler_writes_a_signal_and_rebuilds_its_reader),
            ("media_query_fields::a_size_only_change_rebuilds_size_and_whole_readers_only", crate::media_query_fields::a_size_only_change_rebuilds_size_and_whole_readers_only),
            ("directionality_dependency::a_start_aligned_column_depends_on_directionality", crate::directionality_dependency::a_start_aligned_column_depends_on_directionality),
            ("localizations::the_global_delegate_makes_an_rtl_locale_subtree_rtl", crate::localizations::the_global_delegate_makes_an_rtl_locale_subtree_rtl),
            ("hot_reload_state::perform_reassemble_rebuilds_in_place_and_preserves_state", crate::hot_reload_state::perform_reassemble_rebuilds_in_place_and_preserves_state),
            ("layout_builder::layout_builder_constraint_change_rebuilds_in_the_same_frame", crate::layout_builder::layout_builder_constraint_change_rebuilds_in_the_same_frame),
        ],
    );
}

/// The editable-text capability: selection gestures, obscuring, IME, clipboard, keys.
#[test]
fn text_editing() {
    run_cases(
        "text_editing",
        &[
            ("editable_text::native_actions::queued_focus_and_text_reach_the_current_field_and_event_context", crate::editable_text::native_actions::queued_focus_and_text_reach_the_current_field_and_event_context),
            ("editable_text::native_actions::native_actions_follow_the_replacement_controller_and_focus_node", crate::editable_text::native_actions::native_actions_follow_the_replacement_controller_and_focus_node),
            ("editable_text::native_actions::disabled_unmounted_and_closed_fields_refuse_native_actions", crate::editable_text::native_actions::disabled_unmounted_and_closed_fields_refuse_native_actions),
            ("editable_text::native_actions::a_deferred_focus_change_preserves_the_semantic_edit_that_follows_it", crate::editable_text::native_actions::a_deferred_focus_change_preserves_the_semantic_edit_that_follows_it),
            ("editable_text::native_actions::re_adoption_before_queued_delivery_retires_the_old_field_authority", crate::editable_text::native_actions::re_adoption_before_queued_delivery_retires_the_old_field_authority),
            ("editable_text::native_actions::re_adoption_during_a_deferred_grant_refuses_the_resumed_semantic_edit", crate::editable_text::native_actions::re_adoption_during_a_deferred_grant_refuses_the_resumed_semantic_edit),
            ("editable_text::a_double_tap_selects_the_word_under_it", crate::editable_text::a_double_tap_selects_the_word_under_it as fn()),
            ("editable_text::a_drag_selects_from_its_start_to_the_pointer", crate::editable_text::a_drag_selects_from_its_start_to_the_pointer),
            ("editable_text::disabling_the_field_retires_its_selection_contact", crate::editable_text::disabling_the_field_retires_its_selection_contact),
            ("editable_text::replacing_the_controller_retires_the_old_selection_contact", crate::editable_text::replacing_the_controller_retires_the_old_selection_contact),
            ("editable_text::selection_drag_survives_a_same_controller_rebuild", crate::editable_text::selection_drag_survives_a_same_controller_rebuild),
            ("editable_text::foreign_release_preserves_the_selection_contact", crate::editable_text::foreign_release_preserves_the_selection_contact),
            ("editable_text::foreign_cancel_preserves_the_selection_contact", crate::editable_text::foreign_cancel_preserves_the_selection_contact),
            ("editable_text::insertion_keeps_the_caret_after_the_joined_combining_cluster", crate::editable_text::insertion_keeps_the_caret_after_the_joined_combining_cluster),
            ("editable_text::deleting_a_separator_keeps_the_caret_after_the_joined_flag", crate::editable_text::deleting_a_separator_keeps_the_caret_after_the_joined_flag),
            ("editable_text::text_store::rtl_scalar_rect_midpoints_resolve_to_the_source_scalar", crate::editable_text::text_store::rtl_scalar_rect_midpoints_resolve_to_the_source_scalar),
            ("editable_text::a_tap_places_the_caret_where_it_landed", crate::editable_text::a_tap_places_the_caret_where_it_landed),
            ("editable_text::a_pointer_down_and_a_paste_commit_the_composition_first", crate::editable_text::a_pointer_down_and_a_paste_commit_the_composition_first),
            ("editable_text::moving_focus_off_a_composing_field_commits_it_in_either_mount_order", crate::editable_text::moving_focus_off_a_composing_field_commits_it_in_either_mount_order),
            ("editable_text::a_press_reentered_by_its_commit_is_not_a_second_contact", crate::editable_text::a_press_reentered_by_its_commit_is_not_a_second_contact),
            ("editable_text::an_obscured_field_never_hands_its_real_text_to_the_render_object", crate::editable_text::an_obscured_field_never_hands_its_real_text_to_the_render_object),
            ("editable_text::the_editor_steps_the_graphemes_the_painter_snaps_to", crate::editable_text::the_editor_steps_the_graphemes_the_painter_snaps_to),
            ("editable_text::focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller", crate::editable_text::focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller),
            ("editable_text::text_store::store_offsets_match_controller_bytes_across_surrogates_and_graphemes", crate::editable_text::text_store::store_offsets_match_controller_bytes_across_surrogates_and_graphemes),
            ("editable_text::text_store::typing_after_a_deferred_commit_lands_after_the_commit", crate::editable_text::text_store::typing_after_a_deferred_commit_lands_after_the_commit),
            ("editable_text::text_store::on_changed_runs_after_the_lock_is_released", crate::editable_text::text_store::on_changed_runs_after_the_lock_is_released),
            ("editable_text::text_store::an_app_edit_during_a_lock_is_not_overwritten", crate::editable_text::text_store::an_app_edit_during_a_lock_is_not_overwritten),
            ("editable_text::text_store::focus_gain_and_loss_reach_the_store_host", crate::editable_text::text_store::focus_gain_and_loss_reach_the_store_host),
            ("editable_text::text_store::swapping_the_controller_during_a_grant_drops_the_session", crate::editable_text::text_store::swapping_the_controller_during_a_grant_drops_the_session),
            ("editable_text::text_store::a_panicking_on_changed_is_reported_once_and_the_field_keeps_working", crate::editable_text::text_store::a_panicking_on_changed_is_reported_once_and_the_field_keeps_working),
            ("form::a_text_form_field_validates_and_saves_the_committed_text", crate::form::a_text_form_field_validates_and_saves_the_committed_text),
            ("form::dropping_a_composing_controller_keeps_the_preedit_out_of_the_value", crate::form::dropping_a_composing_controller_keeps_the_preedit_out_of_the_value),
            ("form::setting_the_shown_text_as_the_value_ends_a_reconversion", crate::form::setting_the_shown_text_as_the_value_ends_a_reconversion),
            ("form::setting_the_shown_preedit_as_the_value_commits_it", crate::form::setting_the_shown_preedit_as_the_value_commits_it),
            ("editable_text::text_store::long_input_reveals_the_caret_and_maps_visible_pointer_positions", crate::editable_text::text_store::long_input_reveals_the_caret_and_maps_visible_pointer_positions),
            ("editable_text::text_store::editable_paint_places_long_text_under_the_viewport_clip", crate::editable_text::text_store::editable_paint_places_long_text_under_the_viewport_clip),
            ("editable_text_clipboard::copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed", crate::editable_text_clipboard::copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed),
            ("editable_text_clipboard::paste_rechecks_focus_after_committing_composition", crate::editable_text_clipboard::paste_rechecks_focus_after_committing_composition),
            ("editable_text_clipboard::copy_then_paste_round_trips_text_in_an_editable_text", crate::editable_text_clipboard::copy_then_paste_round_trips_text_in_an_editable_text),
            ("editable_text_clipboard::select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit", crate::editable_text_clipboard::select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit),
            ("editable_text_clipboard::select_all_without_a_focused_text_field_leaves_the_key_unconsumed", crate::editable_text_clipboard::select_all_without_a_focused_text_field_leaves_the_key_unconsumed),
            ("editable_text_clipboard::select_all_defers_to_an_active_composition_and_recovers_after_commit", crate::editable_text_clipboard::select_all_defers_to_an_active_composition_and_recovers_after_commit),
            ("text_field::focused_character_key_inserts_into_controller", crate::text_field::focused_character_key_inserts_into_controller),
            ("text_field::unfocused_field_does_not_receive_key_events", crate::text_field::unfocused_field_does_not_receive_key_events),
            ("text::an_enclosing_default_text_style_styles_a_bare_run", crate::text::an_enclosing_default_text_style_styles_a_bare_run),
        ],
    );
}

/// The `TextStore` conformance kit (ADR-0090) over both editable-text configurations.
#[test]
fn text_store_kit_conformance() {
    run_cases(
        "text_store_kit_conformance",
        &[
            (
                "text_store_kit::editable_text_conforms_to_kit_v1",
                crate::text_store_kit::editable_text_conforms_to_kit_v1 as fn(),
            ),
            (
                "text_store_kit::obscured_editable_text_conforms_to_kit_v1",
                crate::text_store_kit::obscured_editable_text_conforms_to_kit_v1,
            ),
        ],
    );
}

/// Pointer routing and gesture recognition, including the back-swipe release matrix.
#[test]
fn pointer_and_gesture_recognition() {
    run_cases(
        "pointer_and_gesture_recognition",
        &[
            ("pointer_vocabulary::viewer_repeated_native_start_retires_the_previous_generation", crate::pointer_vocabulary::viewer_repeated_native_start_retires_the_previous_generation as fn()),
            ("pointer_vocabulary::viewer_native_owner_survives_descendant_enable_during_rebuild", crate::pointer_vocabulary::viewer_native_owner_survives_descendant_enable_during_rebuild),
            ("pointer_vocabulary::viewer_native_rotation_preserves_the_scene_pivot", crate::pointer_vocabulary::viewer_native_rotation_preserves_the_scene_pivot),
            ("pointer_vocabulary::viewer_rotation_refuses_an_unfittable_quad_then_recovers", crate::pointer_vocabulary::viewer_rotation_refuses_an_unfittable_quad_then_recovers),
            ("pointer_vocabulary::viewer_extreme_finite_pan_preserves_the_boundary_result", crate::pointer_vocabulary::viewer_extreme_finite_pan_preserves_the_boundary_result),
            ("pointer_vocabulary::viewer_focal_fling_advances_then_stops_on_new_input", crate::pointer_vocabulary::viewer_focal_fling_advances_then_stops_on_new_input),
            ("pointer_vocabulary::viewer_reports_scale_velocity_separately_from_focal_velocity", crate::pointer_vocabulary::viewer_reports_scale_velocity_separately_from_focal_velocity),
            ("listener::presentation_resampling_uses_the_owner_frame_clock", crate::listener::presentation_resampling_uses_the_owner_frame_clock as fn()),
            ("gesture_detector::exclusive_drag_callbacks_have_one_arena_winner", crate::gesture_detector::exclusive_drag_callbacks_have_one_arena_winner),
            ("gesture_detector::a_detector_in_a_composed_scope_preserves_double_tap_timing", crate::gesture_detector::a_detector_in_a_composed_scope_preserves_double_tap_timing),
            ("pointer_vocabulary::viewer_native_pan_moves_the_scene_under_the_focal_point", crate::pointer_vocabulary::viewer_native_pan_moves_the_scene_under_the_focal_point as fn()),
            ("pointer_vocabulary::viewer_native_session_reports_one_start_and_one_terminal", crate::pointer_vocabulary::viewer_native_session_reports_one_start_and_one_terminal),
            ("pointer_vocabulary::viewer_pan_transitions_to_pinch_without_contact_count_jumps", crate::pointer_vocabulary::viewer_pan_transitions_to_pinch_without_contact_count_jumps),
            ("pointer_vocabulary::scroll_claim_preserves_owned_source_units_and_phase", crate::pointer_vocabulary::scroll_claim_preserves_owned_source_units_and_phase as fn()),
            ("pointer_vocabulary::pointer_delivery_preserves_source_and_sample_families", crate::pointer_vocabulary::pointer_delivery_preserves_source_and_sample_families),
            ("pointer_vocabulary::page_scroll_resolves_against_the_actual_viewport", crate::pointer_vocabulary::page_scroll_resolves_against_the_actual_viewport),
            ("pointer_vocabulary::viewer_page_zoom_resolves_against_the_actual_viewport", crate::pointer_vocabulary::viewer_page_zoom_resolves_against_the_actual_viewport),
            ("pointer_vocabulary::viewer_cumulative_zoom_survives_rebuild_and_resets", crate::pointer_vocabulary::viewer_cumulative_zoom_survives_rebuild_and_resets),
            ("pointer_vocabulary::viewer_unstarted_pinch_updates_remain_independent_steps", crate::pointer_vocabulary::viewer_unstarted_pinch_updates_remain_independent_steps),
            ("pointer_vocabulary::viewer_extreme_zoom_reports_the_finite_applied_change", crate::pointer_vocabulary::viewer_extreme_zoom_reports_the_finite_applied_change),
            ("pointer_vocabulary::viewer_page_overflow_and_empty_viewport_recover", crate::pointer_vocabulary::viewer_page_overflow_and_empty_viewport_recover),
            ("gesture_detector::clearing_pan_callbacks_mid_drag_still_finishes_the_drag", crate::gesture_detector::clearing_pan_callbacks_mid_drag_still_finishes_the_drag as fn()),
            ("gesture_detector::mounted_drag_policy_replaces_targets_before_cancellation_and_recovers", crate::gesture_detector::mounted_drag_policy_replaces_targets_before_cancellation_and_recovers),
            ("gesture_detector::scoped_settings_control_touch_recognition_thresholds", crate::gesture_detector::scoped_settings_control_touch_recognition_thresholds),
            ("gesture_detector::scoped_settings_control_gesture_deadlines", crate::gesture_detector::scoped_settings_control_gesture_deadlines),
            ("gesture_detector::scoped_estimator_controls_delivered_drag_velocity", crate::gesture_detector::scoped_estimator_controls_delivered_drag_velocity),
            ("gesture_detector::unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival", crate::gesture_detector::unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival as fn()),
            ("gesture_detector::viewer_reports_cancelled_then_completed_interactions", crate::gesture_detector::viewer_reports_cancelled_then_completed_interactions as fn()),
            ("gesture_detector::gesture_detector_fires_on_tap_for_a_down_up_on_the_child", crate::gesture_detector::gesture_detector_fires_on_tap_for_a_down_up_on_the_child as fn()),
            ("gesture_detector::gesture_detector_recognizes_a_pan_and_suppresses_the_tap", crate::gesture_detector::gesture_detector_recognizes_a_pan_and_suppresses_the_tap),
            ("gesture_detector_advanced::double_tap_combined_with_tap_fires_double_tap_once_and_tap_never", crate::gesture_detector_advanced::double_tap_combined_with_tap_fires_double_tap_once_and_tap_never),
            ("gesture_detector_advanced::long_press_fires_when_held_past_the_deadline", crate::gesture_detector_advanced::long_press_fires_when_held_past_the_deadline),
            ("absorb_pointer::absorbing_true_blocks_the_tap_from_reaching_a_child_gesture_detector", crate::absorb_pointer::absorbing_true_blocks_the_tap_from_reaching_a_child_gesture_detector),
            ("listener::listener_routes_down_and_up_to_their_own_callbacks", crate::listener::listener_routes_down_and_up_to_their_own_callbacks),
            ("listener::listener_capture_retains_one_target_and_drop_delivers_loss", crate::listener::listener_capture_retains_one_target_and_drop_delivers_loss),
            ("listener::listener_unmount_preserves_one_captured_contact_terminal", crate::listener::listener_unmount_preserves_one_captured_contact_terminal),
            ("listener::listener_admission_keeps_terminal_delivery_and_weak_ownership", crate::listener::listener_admission_keeps_terminal_delivery_and_weak_ownership),
            ("listener::listener_raw_observer_panic_still_delivers_the_recognizer_event", crate::listener::listener_raw_observer_panic_still_delivers_the_recognizer_event),
            ("listener::custom_recognizer_competes_through_a_listener", crate::listener::custom_recognizer_competes_through_a_listener),
            ("draggable_events::unmounting_a_target_releases_its_slot", crate::draggable_events::unmounting_a_target_releases_its_slot),
            ("back_gesture::release_matrix_fling_and_slow_release", crate::back_gesture::release_matrix_fling_and_slow_release),
            ("page_route::back_gesture_edge_drag_normalizes_against_the_routes_real_width_not_the_hit_strip", crate::page_route::back_gesture_edge_drag_normalizes_against_the_routes_real_width_not_the_hit_strip),
        ],
    );
}

/// Focus attachment and traversal, the actions chain and keyboard shortcuts.
#[test]
fn focus_actions_and_shortcuts() {
    crate::shortcuts::tab_tests::run_policy_child_if_requested();
    run_cases(
        "focus_actions_and_shortcuts",
        &[
            ("focus::traversal_groups_order_blocks_without_creating_focus_scopes", crate::focus::traversal_groups_order_blocks_without_creating_focus_scopes as fn()),
            ("focus::nested_scope_edges_visit_the_containing_group_and_reuse_policy_order", crate::focus::nested_scope_edges_visit_the_containing_group_and_reuse_policy_order as fn()),
            ("focus::typed_focus_overrides_fall_back_after_target_invalidation", crate::focus::typed_focus_overrides_fall_back_after_target_invalidation as fn()),
            ("focus::arrow_traversal_prefers_the_beam_and_respects_group_edges", crate::focus::arrow_traversal_prefers_the_beam_and_respects_group_edges as fn()),
            ("focus::widget_scope_edge_configuration_reaches_the_tab_path", crate::focus::widget_scope_edge_configuration_reaches_the_tab_path as fn()),
            ("focus::tab_groups_vertically_overlapping_widgets_into_one_reading_row", crate::focus::tab_groups_vertically_overlapping_widgets_into_one_reading_row as fn()),
            ("focus::tab_reads_an_rtl_scope_from_its_inherited_directionality", crate::focus::tab_reads_an_rtl_scope_from_its_inherited_directionality as fn()),
            ("focus::a_tall_widget_cannot_bridge_disjoint_reading_rows", crate::focus::a_tall_widget_cannot_bridge_disjoint_reading_rows as fn()),
            ("focus::a_directionality_update_changes_tab_order_without_replacing_focus_nodes", crate::focus::a_directionality_update_changes_tab_order_without_replacing_focus_nodes as fn()),
            ("focus::spatial_tab_preserves_geometric_ties_and_row_boundaries", crate::focus::spatial_tab_preserves_geometric_ties_and_row_boundaries as fn()),
            ("focus::platform_focus_requests_the_mounted_node_and_rejects_disabled_focus", crate::focus::platform_focus_requests_the_mounted_node_and_rejects_disabled_focus as fn()),
            ("focus::an_adopted_external_node_ignores_the_old_elements_focus_action", crate::focus::an_adopted_external_node_ignores_the_old_elements_focus_action as fn()),
            ("actions::the_nearest_enabled_action_wins_and_receives_the_payload", crate::actions::the_nearest_enabled_action_wins_and_receives_the_payload as fn()),
            ("actions::callback_action_writes_through_the_key_events_cx", crate::actions::callback_action_writes_through_the_key_events_cx),
            ("actions::a_refused_write_in_a_callback_action_is_reported_not_panicked", crate::actions::a_refused_write_in_a_callback_action_is_reported_not_panicked),
            ("shortcuts::activation_tests::enter_space_and_select_activate_the_focused_control", crate::shortcuts::activation_tests::enter_space_and_select_activate_the_focused_control),
            ("shortcuts::activator_tests::a_shift_produced_character_matches_when_shift_is_ignored", crate::shortcuts::activator_tests::a_shift_produced_character_matches_when_shift_is_ignored),
            ("shortcuts::intent_tests::a_shortcut_dispatches_its_intent_through_the_actions_chain", crate::shortcuts::intent_tests::a_shortcut_dispatches_its_intent_through_the_actions_chain),
            ("shortcuts::tab_tests::tab_and_shift_tab_move_the_focus_through_the_actions_chain", crate::shortcuts::tab_tests::tab_and_shift_tab_move_the_focus_through_the_actions_chain),
            ("shortcuts::tab_tests::tab_traversal_preserves_failure_before_policy_and_candidate_retirement", crate::shortcuts::tab_tests::tab_traversal_preserves_failure_before_policy_and_candidate_retirement),
            ("shortcuts::event_cx_tests::callback_shortcut_writes_a_signal_and_rebuilds_its_reader", crate::shortcuts::event_cx_tests::callback_shortcut_writes_a_signal_and_rebuilds_its_reader),
            ("shortcuts::event_cx_tests::a_refused_write_in_a_callback_shortcut_is_reported_not_panicked", crate::shortcuts::event_cx_tests::a_refused_write_in_a_callback_shortcut_is_reported_not_panicked),
            ("shortcuts::event_cx_tests::a_shortcut_action_writes_through_the_key_events_cx", crate::shortcuts::event_cx_tests::a_shortcut_action_writes_through_the_key_events_cx),
            ("shortcuts::event_cx_tests::a_refused_write_in_a_shortcut_action_is_reported_not_panicked", crate::shortcuts::event_cx_tests::a_refused_write_in_a_shortcut_action_is_reported_not_panicked),
            ("shortcuts::event_cx_tests::a_let_bound_action_closure_compiles_through_callback_ref", crate::shortcuts::event_cx_tests::a_let_bound_action_closure_compiles_through_callback_ref),
            ("focus::a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases", crate::focus::a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases),
            ("focus::tab_traversal_follows_geometry_not_attach_order", crate::focus::tab_traversal_follows_geometry_not_attach_order),
            ("focus::event_cx::replacing_a_focused_node_delivers_a_writable_loss_and_new_gain", crate::focus::event_cx::replacing_a_focused_node_delivers_a_writable_loss_and_new_gain),
        ],
    );
}

/// Widget semantics reach the platform and platform actions reach widgets, with the
/// documented drop set and the exhaustive routing list pinned.
#[test]
fn semantics_translation_and_routing() {
    run_cases(
        "semantics_translation_and_routing",
        &[
            ("semantics::retained_visibility_hides_child_semantics_by_default", crate::semantics::retained_visibility_hides_child_semantics_by_default as fn()),
            ("semantics::retained_visibility_updates_semantics_without_changing_layout", crate::semantics::retained_visibility_updates_semantics_without_changing_layout as fn()),
            ("semantics::queued_directional_actions_and_numeric_values_reach_the_frame_producer", crate::semantics::queued_directional_actions_and_numeric_values_reach_the_frame_producer as fn()),
            ("semantics::numeric_range_admission_and_owner_payload_validation", crate::semantics::numeric_range_admission_and_owner_payload_validation as fn()),
            ("semantics::a_set_text_request_without_a_payload_is_dropped_rather_than_emptied", crate::semantics::a_set_text_request_without_a_payload_is_dropped_rather_than_emptied as fn()),
            ("semantics::a_tap_handler_round_trips_from_a_platform_click_to_the_callback", crate::semantics::a_tap_handler_round_trips_from_a_platform_click_to_the_callback),
            ("semantics::assistive_scroll_actions_move_a_scrollable", crate::semantics::assistive_scroll_actions_move_a_scrollable),
            ("semantics::merge_semantics_collapses_its_descendants_in_the_a11y_tree", crate::semantics::merge_semantics_collapses_its_descendants_in_the_a11y_tree),
            ("semantics::published_bounds_are_physical_and_follow_the_scale_factor", crate::semantics::published_bounds_are_physical_and_follow_the_scale_factor),
            ("semantics::a_covered_retained_form_stays_absent_after_a_late_controller_update", crate::semantics::a_covered_retained_form_stays_absent_after_a_late_controller_update),
            ("semantics::rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one", crate::semantics::rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one),
            ("semantics::unmounting_a_node_releases_its_action_table", crate::semantics::unmounting_a_node_releases_its_action_table),
            ("semantics::a_detached_mount_advertises_no_actions", crate::semantics::a_detached_mount_advertises_no_actions),
            ("semantics::the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set", crate::semantics::the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set),
            ("semantics::the_exhaustive_routing_list_agrees_with_the_translation_table", crate::semantics::the_exhaustive_routing_list_agrees_with_the_translation_table),
            ("raw_button::raw_button_press_is_reachable_through_a_platform_click", crate::raw_button::raw_button_press_is_reachable_through_a_platform_click),
        ],
    );
}

/// Scrollable gesture, fling, bounce, wheel-nesting and programmatic-jump behavior.
#[test]
fn scroll_physics_and_activity() {
    run_cases(
        "scroll_physics_and_activity",
        &[
            ("scroll::nested_fling_hands_remaining_velocity_to_matching_parent_axes", crate::scroll::nested_fling_hands_remaining_velocity_to_matching_parent_axes as fn()),
            ("scroll::nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy", crate::scroll::nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy),
            ("scroll::replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers", crate::scroll::replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers),
            ("scroll::nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress", crate::scroll::nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress),
            ("scroll::nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer", crate::scroll::nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer),
            ("scroll::show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse", crate::scroll::show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse),
            ("scroll::show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry", crate::scroll::show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry),
            ("scroll::nested_scroll_sequence_keeps_its_first_consumptive_target", crate::scroll::nested_scroll_sequence_keeps_its_first_consumptive_target as fn()),
            ("scroll::scroll_latch_survives_focal_motion_and_releases_on_cancel", crate::scroll::scroll_latch_survives_focal_motion_and_releases_on_cancel),
            ("scroll::phase_less_scroll_latch_expires_on_owner_clock_inactivity", crate::scroll::phase_less_scroll_latch_expires_on_owner_clock_inactivity),
            ("scroll::scroll_latches_are_source_local_and_device_removal_releases", crate::scroll::scroll_latches_are_source_local_and_device_removal_releases),
            ("scroll::a_remaining_touch_continues_scroll_without_an_intermediate_fling", crate::scroll::a_remaining_touch_continues_scroll_without_an_intermediate_fling as fn()),
            ("scroll::notched_wheel_accumulates_distance_and_eases_out_in_150ms", crate::scroll::notched_wheel_accumulates_distance_and_eases_out_in_150ms),
            ("scroll::precise_and_unknown_wheels_interrupt_synthetic_motion_once", crate::scroll::precise_and_unknown_wheels_interrupt_synthetic_motion_once),
            ("scroll::replacing_or_unmounting_a_scrollable_retires_its_notched_motion", crate::scroll::replacing_or_unmounting_a_scrollable_retires_its_notched_motion),
            ("scroll::dragging_interrupts_notched_motion_and_windows_progress_independently", crate::scroll::dragging_interrupts_notched_motion_and_windows_progress_independently),
            ("scroll::scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded", crate::scroll::scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded as fn()),
            ("scroll::dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick", crate::scroll::dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick),
            ("scroll::cancelling_an_in_range_scroll_ends_activity_without_coasting", crate::scroll::cancelling_an_in_range_scroll_ends_activity_without_coasting as fn()),
            ("scroll::refresh_indicator_drag_scrolls_without_rebuilding", crate::scroll::refresh_indicator_drag_scrolls_without_rebuilding),
            ("scroll::refresh_indicator_rebuilds_only_on_a_phase_change", crate::scroll::refresh_indicator_rebuilds_only_on_a_phase_change),
            ("scroll::cancelling_bouncing_overscroll_settles_without_release_velocity", crate::scroll::cancelling_bouncing_overscroll_settles_without_release_velocity as fn()),
            ("scroll::cancelling_a_threshold_refresh_pull_does_not_refresh", crate::scroll::cancelling_a_threshold_refresh_pull_does_not_refresh as fn()),
            ("scroll::bouncing_lower_edge_preserves_outward_direction", crate::scroll::bouncing_lower_edge_preserves_outward_direction as fn()),
            ("scroll::bouncing_upper_edge_preserves_outward_direction", crate::scroll::bouncing_upper_edge_preserves_outward_direction as fn()),
            ("scroll::bouncing_stationary_input_preserves_overscroll", crate::scroll::bouncing_stationary_input_preserves_overscroll as fn()),
            ("scroll::bouncing_inward_motion_and_crossing_respect_the_new_edge", crate::scroll::bouncing_inward_motion_and_crossing_respect_the_new_edge as fn()),
            ("scroll::a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook", crate::scroll::a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook as fn()),
            ("scroll::a_same_position_scrollable_rebuild_preserves_motion", crate::scroll::a_same_position_scrollable_rebuild_preserves_motion as fn()),
            ("scroll::retiring_one_scrollable_preserves_a_later_owners_jump_hook", crate::scroll::retiring_one_scrollable_preserves_a_later_owners_jump_hook as fn()),
            ("scroll::a_fast_gesture_while_refreshing_does_not_start_a_fling", crate::scroll::a_fast_gesture_while_refreshing_does_not_start_a_fling as fn()),
            ("scroll::incremental_pulls_refresh_once_and_finish_allows_the_next_gesture", crate::scroll::incremental_pulls_refresh_once_and_finish_allows_the_next_gesture as fn()),
            ("scroll::reversing_a_pull_consumes_it_before_scrolling_content", crate::scroll::reversing_a_pull_consumes_it_before_scrolling_content as fn()),
            ("scroll::a_refresh_controller_swap_retires_the_old_fling_and_drives_the_new_position", crate::scroll::a_refresh_controller_swap_retires_the_old_fling_and_drives_the_new_position as fn()),
            ("scroll::rebuilding_refresh_content_with_the_same_position_preserves_its_fling", crate::scroll::rebuilding_refresh_content_with_the_same_position_preserves_its_fling as fn()),
            (
                "scroll::shift_wheel_scrolls_the_horizontal_axis",
                crate::scroll::shift_wheel_scrolls_the_horizontal_axis as fn(),
            ),
            (
                "scroll::a_wheel_tick_over_nested_scrollables_moves_only_the_inner",
                crate::scroll::a_wheel_tick_over_nested_scrollables_moves_only_the_inner as fn(),
            ),
            (
                "scroll::bouncing_physics_fling_springs_back_after_overscroll",
                crate::scroll::bouncing_physics_fling_springs_back_after_overscroll,
            ),
            (
                "scroll::scroll_activity_tracks_the_whole_gesture_lifecycle",
                crate::scroll::scroll_activity_tracks_the_whole_gesture_lifecycle,
            ),
            (
                "scroll::scrollable_drag_up_increases_scroll_offset",
                crate::scroll::scrollable_drag_up_increases_scroll_offset,
            ),
            (
                "scroll::scrollable_fling_advances_offset_past_release",
                crate::scroll::scrollable_fling_advances_offset_past_release,
            ),
            (
                "scroll::scrollable_jump_to_during_animate_to_cancels_it_synchronously",
                crate::scroll::scrollable_jump_to_during_animate_to_cancels_it_synchronously,
            ),
            (
                "scroll::scroll_fling_rest_scales_with_device_pixel_ratio",
                crate::scroll::scroll_fling_rest_scales_with_device_pixel_ratio,
            ),
            ("scroll::inverted_extents_do_not_fling", crate::scroll::inverted_extents_do_not_fling),
            (
                "scroll::bouncing_fling_into_the_edge_overscrolls_and_returns",
                crate::scroll::bouncing_fling_into_the_edge_overscrolls_and_returns,
            ),
            (
                "scroll::scroll_fling_rest_scales_with_device_pixel_ratio",
                crate::scroll::scroll_fling_rest_scales_with_device_pixel_ratio,
            ),
            ("scroll::inverted_extents_do_not_fling", crate::scroll::inverted_extents_do_not_fling),
            (
                "scroll::bouncing_fling_into_the_edge_overscrolls_and_returns",
                crate::scroll::bouncing_fling_into_the_edge_overscrolls_and_returns,
            ),
        ],
    );
}

/// Lazy list and grid builders and the persistent header: the band, the pass budget,
/// keyed state and pathological extents.
#[test]
fn lazy_slivers() {
    run_cases(
        "lazy_slivers",
        &[
            ("lazy_list::lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame", crate::lazy_list::lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame as fn()),
            ("lazy_list::lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state", crate::lazy_list::lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state),
            ("lazy_list::lazy_list_view_builder_pathological_extents_defer_instead_of_panicking", crate::lazy_list::lazy_list_view_builder_pathological_extents_defer_instead_of_panicking),
            ("lazy_list::lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band", crate::lazy_list::lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band),
            ("lazy_grid::lazy_grid_view_builder_places_tiles_at_oracle_positions", crate::lazy_grid::lazy_grid_view_builder_places_tiles_at_oracle_positions),
            ("sliver_persistent_header::a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends", crate::sliver_persistent_header::a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends),
        ],
    );
}

/// Navigator push/pop/veto/local-history and the overlay it drives.
#[test]
fn navigator_and_overlay() {
    run_cases(
        "navigator_and_overlay",
        &[
            ("back_gesture::cancelling_a_back_swipe_past_halfway_keeps_the_route", crate::back_gesture::cancelling_a_back_swipe_past_halfway_keeps_the_route as fn()),
            ("navigator::local_history::an_entry_pops_before_the_route_and_observers_stay_silent", crate::navigator::local_history::an_entry_pops_before_the_route_and_observers_stay_silent as fn()),
            ("navigator::navigator_pop_removes_top_route_and_completes_result", crate::navigator::navigator_pop_removes_top_route_and_completes_result),
            ("navigator::navigator_push_builds_new_route_and_rearranges_overlay", crate::navigator::navigator_push_builds_new_route_and_rearranges_overlay),
            ("navigator::pop_scope_vetoes_maybe_pop_but_not_programmatic_pop", crate::navigator::pop_scope_vetoes_maybe_pop_but_not_programmatic_pop),
            ("overlay::overlay_opaque_top_entry_drops_lower_entries_entirely", crate::overlay::overlay_opaque_top_entry_drops_lower_entries_entirely),
            ("overlay::overlay_rearrange_reorders_and_preserves_entry_state", crate::overlay::overlay_rearrange_reorders_and_preserves_entry_state),
        ],
    );
}

/// Modal, page and transition routes: occlusion, barriers, secondary animation, parking.
#[test]
fn route_transitions() {
    run_cases(
        "route_transitions",
        &[
            ("modal_route::modal_barrier_absorbs_pointers_and_a_dismissible_one_adds_a_gesture_detector", crate::modal_route::modal_barrier_absorbs_pointers_and_a_dismissible_one_adds_a_gesture_detector as fn()),
            ("modal_route::modal_opaque_route_occludes_the_route_below_once_its_transition_completes", crate::modal_route::modal_opaque_route_occludes_the_route_below_once_its_transition_completes),
            ("page_route::page_route_occludes_the_route_below_once_its_transition_completes", crate::page_route::page_route_occludes_the_route_below_once_its_transition_completes),
            ("page_route::secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping", crate::page_route::secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping),
            ("transition_route::push_transition_parks_the_entry_in_pushing_until_the_controller_completes", crate::transition_route::push_transition_parks_the_entry_in_pushing_until_the_controller_completes),
            ("transition_route::hopping_route_dropped_without_dispose_frees_proxy", crate::transition_route::hopping_route_dropped_without_dispose_frees_proxy),
        ],
    );
}

/// The `Router`, `#[derive(Routable)]` round trip and the `WidgetsApp` shell.
#[test]
fn router_and_widgets_app() {
    run_cases(
        "router_and_widgets_app",
        &[
            ("router::popup_routes_are_admitted_and_leave_the_location_alone", crate::router::popup_routes_are_admitted_and_leave_the_location_alone as fn()),
            ("router::router_never_pops_its_last_page", crate::router::router_never_pops_its_last_page),
            ("router::router_opens_at_a_location_with_its_back_stack", crate::router::router_opens_at_a_location_with_its_back_stack),
            ("router::router_push_commits_before_a_reentrant_observer_pop", crate::router::router_push_commits_before_a_reentrant_observer_pop),
            ("router::router_replace_commits_before_a_reentrant_observer_pop", crate::router::router_replace_commits_before_a_reentrant_observer_pop),
            ("router::router_go_commits_before_a_reentrant_observer_pop", crate::router::router_go_commits_before_a_reentrant_observer_pop),
            ("router::router_go_recomputes_its_prefix_after_popup_observer_navigation", crate::router::router_go_recomputes_its_prefix_after_popup_observer_navigation),
            ("router::router_push_preserves_its_commit_after_an_observer_panic", crate::router::router_push_preserves_its_commit_after_an_observer_panic),
            ("router::router_go_preserves_its_commit_after_an_observer_panic", crate::router::router_go_preserves_its_commit_after_an_observer_panic),
            ("router::removing_a_router_value_retires_it_after_releasing_the_stack_borrow", crate::router::removing_a_router_value_retires_it_after_releasing_the_stack_borrow),
            ("router::empty_stack_is_refused", crate::router::empty_stack_is_refused),
            ("widgets_app::builder_only_app_receives_no_routing_and_supplies_the_subtree", crate::widgets_app::builder_only_app_receives_no_routing_and_supplies_the_subtree),
            ("widgets_app::home_is_seeded_once_as_the_root_route", crate::widgets_app::home_is_seeded_once_as_the_root_route),
            ("widgets_app::observers_attach_at_mount_and_see_the_home_route", crate::widgets_app::observers_attach_at_mount_and_see_the_home_route),
            ("widgets_app_router::a_pushed_route_focuses_its_first_control_in_every_mount", crate::widgets_app_router::a_pushed_route_focuses_its_first_control_in_every_mount),
            ("widgets_app_router::switching_widgets_app_from_home_to_router_releases_the_navigator", crate::widgets_app_router::switching_widgets_app_from_home_to_router_releases_the_navigator),
            ("widgets_app_router::widgets_app_router_navigates_by_handle_and_the_url_follows", crate::widgets_app_router::widgets_app_router_navigates_by_handle_and_the_url_follows),
        ],
    );
}

/// A router reopened on a saved stack keeps its Back order. Joins
/// `router_and_widgets_app` once `Router::from_stack` places the whole stack.
#[test]
#[ignore = "contract: Router::from_stack places every saved route"]
fn from_stack_restores_back_order() {
    crate::router::from_stack_restores_back_order();
}

/// `RouterHandle::stack` reads the whole stack. Joins
/// `router_and_widgets_app` once it does.
#[test]
#[ignore = "contract: RouterHandle::stack reads every page on the stack"]
fn stack_reads_every_committed_edit() {
    crate::router::stack_reads_every_committed_edit();
}

/// Hero flights: state survival, curves, diversion by a pop, gesture release.
#[test]
fn hero_flights() {
    run_cases(
        "hero_flights",
        &[
            ("hero::a_hero_child_keeps_its_state_across_a_flight_without_a_global_key", crate::hero::a_hero_child_keeps_its_state_across_a_flight_without_a_global_key as fn()),
            ("hero_flight::a_push_eases_on_the_destination_hero_curve", crate::hero_flight::a_push_eases_on_the_destination_hero_curve),
            ("hero_flight::a_push_flight_interrupted_by_a_pop_diverts_in_place", crate::hero_flight::a_push_flight_interrupted_by_a_pop_diverts_in_place),
            ("hero_flight::a_shrinking_flight_with_overshoot_keeps_a_non_negative_size", crate::hero_flight::a_shrinking_flight_with_overshoot_keeps_a_non_negative_size),
            ("hero_gesture::complete_release_pops_to_the_destination_route_and_the_flight_lands", crate::hero_gesture::complete_release_pops_to_the_destination_route_and_the_flight_lands),
            ("hero_public::a_hero_push_flight_runs_and_settles", crate::hero_public::a_hero_push_flight_runs_and_settles),
        ],
    );
}

/// Explicit and implicit animation widgets and `Visibility`.
#[test]
fn animation_and_visibility() {
    run_cases(
        "animation_and_visibility",
        &[
            ("dismissible::cancelling_a_fully_slid_card_restores_it_without_dismissal", crate::dismissible::cancelling_a_fully_slid_card_restores_it_without_dismissal as fn()),
            ("dismissible::a_cancelled_horizontal_dismiss_restores_the_card", crate::dismissible::a_cancelled_horizontal_dismiss_restores_the_card as fn()),
            ("dismissible::a_cancelled_vertical_dismiss_restores_the_card", crate::dismissible::a_cancelled_vertical_dismiss_restores_the_card as fn()),
            ("dismissible::a_dismissible_release_keeps_finger_speed_on_any_width", crate::dismissible::a_dismissible_release_keeps_finger_speed_on_any_width as fn()),
            ("animated_size::animated_size_interpolates_to_a_new_child_size_over_frames", crate::animated_size::animated_size_interpolates_to_a_new_child_size_over_frames as fn()),
            ("implicit_animations::animated_container_interpolates_size_over_frames", crate::implicit_animations::animated_container_interpolates_size_over_frames),
            ("implicit_animations::animated_opacity_retargets_from_the_current_value_midflight", crate::implicit_animations::animated_opacity_retargets_from_the_current_value_midflight),
            ("implicit_animations::overshooting_padding_stays_non_negative", crate::implicit_animations::overshooting_padding_stays_non_negative),
            ("implicit_animations::overshooting_margin_stays_non_negative", crate::implicit_animations::overshooting_margin_stays_non_negative),
            ("implicit_animations::overshooting_size_stays_non_negative", crate::implicit_animations::overshooting_size_stays_non_negative),
            ("implicit_animations::animated_container_animates_its_transform", crate::implicit_animations::animated_container_animates_its_transform),
            ("implicit_animations::nan_size_passes_through_like_container", crate::implicit_animations::nan_size_passes_through_like_container),
            ("implicit_animations::animated_container_reanchors_unchanged_properties_on_restart", crate::implicit_animations::animated_container_reanchors_unchanged_properties_on_restart),
            ("implicit_animations::animated_container_keeps_an_unchanged_collapsed_transform_on_restart", crate::implicit_animations::animated_container_keeps_an_unchanged_collapsed_transform_on_restart),
            ("implicit_animations::animated_rotation_takes_the_shorter_arc", crate::implicit_animations::animated_rotation_takes_the_shorter_arc),
            ("implicit_animations::animated_rotation_takes_the_numeric_arc", crate::implicit_animations::animated_rotation_takes_the_numeric_arc),
            ("implicit_animations::animated_rotation_retargets_on_a_path_change", crate::implicit_animations::animated_rotation_retargets_on_a_path_change),
            ("binding_animation::registered_controller_advances_fade_opacity_frame_to_frame", crate::binding_animation::registered_controller_advances_fade_opacity_frame_to_frame),
            ("visibility::hidden_without_maintain_state_shows_the_default_replacement", crate::visibility::hidden_without_maintain_state_shows_the_default_replacement),
            ("visibility::maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes", crate::visibility::maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes),
        ],
    );
}

/// Slide, scale and rotation transitions: a tick moves the transform on the
/// same frame without rebuilding any element.
#[test]
fn transitions_tick_without_rebuilding() {
    run_cases(
        "transitions_tick_without_rebuilding",
        &[
            (
                "slide",
                crate::transitions::slide_ticks_without_rebuilding as fn(),
            ),
            (
                "slide_rtl",
                crate::transitions::slide_rtl_mirrors_dx_without_rebuilding,
            ),
            ("scale", crate::transitions::scale_ticks_without_rebuilding),
            (
                "rotation",
                crate::transitions::rotation_ticks_without_rebuilding,
            ),
            (
                "shared_controller",
                crate::transitions::shared_controller_moves_both_transitions_on_one_tick,
            ),
            (
                "zero_dt_huge_dt_reverse_mid_run",
                crate::transitions::virtual_time_ticks_follow_the_controller_value,
            ),
        ],
    );
}

/// Where a transformed child is hit, and how the transition
/// owns, swaps and releases its animation.
#[test]
fn transitions_follow_the_painted_transform() {
    run_cases(
        "transitions_follow_the_painted_transform",
        &[
            (
                "transitions_hit_test_follows_the_painted_transform",
                crate::transitions::hit_test_follows_the_painted_transform as fn(),
            ),
            (
                "transitions_swap_their_animation_in_place",
                crate::transitions::swapping_the_animation_keeps_the_render_object,
            ),
            (
                "transition_unmounted_during_a_tick_marks_nothing",
                crate::transitions::unmounted_transition_leaves_its_sibling_ticking,
            ),
            (
                "transitions_release_the_animation_after_unmount",
                crate::transitions::the_animation_is_released_after_unmount,
            ),
        ],
    );
}

/// Widgets backed by render objects: each row mounts the widget and checks layout,
/// parent data or the composited layer tree.
#[test]
fn layout_and_render_object_wiring() {
    run_cases(
        "layout_and_render_object_wiring",
        &[
            ("stack_positioned::positioned_places_child_at_explicit_edges", crate::stack_positioned::positioned_places_child_at_explicit_edges as fn()),
            ("wrap::wrap_three_boxes_form_two_runs_when_width_is_narrow", crate::wrap::wrap_three_boxes_form_two_runs_when_width_is_narrow),
            ("table::table_mounts_render_table_and_lays_out_a_grid_row_major", crate::table::table_mounts_render_table_and_lays_out_a_grid_row_major),
            ("flex_parent_data::two_expandeds_split_main_axis_by_flex_factor", crate::flex_parent_data::two_expandeds_split_main_axis_by_flex_factor),
            ("composition::nested_padding_accumulates_insets_through_levels", crate::composition::nested_padding_accumulates_insets_through_levels),
            ("custom_multi_child_layout::custom_multi_child_layout_mounts_render_object_and_positions_layout_id_children", crate::custom_multi_child_layout::custom_multi_child_layout_mounts_render_object_and_positions_layout_id_children),
            ("child_type_swap::a_replaced_root_render_object_is_laid_out_in_the_frame_it_is_mounted", crate::child_type_swap::a_replaced_root_render_object_is_laid_out_in_the_frame_it_is_mounted),
            ("component_child_ordering::component_child_keeps_slot_order_before_a_render_sibling", crate::component_child_ordering::component_child_keeps_slot_order_before_a_render_sibling),
            ("parent_data_ancestry::expanded_under_stack_panics_at_attach_with_ancestry_diagnostic", crate::parent_data_ancestry::expanded_under_stack_panics_at_attach_with_ancestry_diagnostic),
            ("layer_inspection::the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list", crate::layer_inspection::the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list),
        ],
    );
}

/// `Form` validation and reset, and the future/stream builders.
#[test]
fn forms_and_async_builders() {
    run_cases(
        "forms_and_async_builders",
        &[
            ("form::reset_restores_initial_values_and_clears_errors_and_interaction", crate::form::reset_restores_initial_values_and_clears_errors_and_interaction as fn()),
            ("form::validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it", crate::form::validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it),
            ("future_builder::future_builder_pending_then_error", crate::future_builder::future_builder_pending_then_error),
            ("future_builder::future_builder_accepts_an_owner_local_future", crate::future_builder::future_builder_accepts_an_owner_local_future),
            ("stream_builder::stream_builder_data_error_data_then_done", crate::stream_builder::stream_builder_data_error_data_then_done),
        ],
    );
}

#[test]
fn controlled_slider_input_and_geometry() {
    run_cases("controlled_slider_input_and_geometry", &[
        ("catalog_slider::slider_uses_allocated_fractional_bounds_for_paint_and_pointer_mapping", crate::catalog_slider::slider_uses_allocated_fractional_bounds_for_paint_and_pointer_mapping as fn()),
        ("catalog_slider::slider_drag_proposals_do_not_commit_without_parent_update", crate::catalog_slider::slider_drag_proposals_do_not_commit_without_parent_update as fn()),
        ("catalog_slider::slider_ambient_rtl_and_explicit_override_mirror_input_and_thumb", crate::catalog_slider::slider_ambient_rtl_and_explicit_override_mirror_input_and_thumb as fn()),
        ("catalog_slider::slider_extreme_range_interpolation_and_step_remain_finite", crate::catalog_slider::slider_extreme_range_interpolation_and_step_remain_finite as fn()),
        ("catalog_slider::slider_degenerate_geometry_or_span_is_inert", crate::catalog_slider::slider_degenerate_geometry_or_span_is_inert as fn()),
        ("catalog_slider::slider_focus_keys_and_semantic_actions_share_controlled_proposals", crate::catalog_slider::slider_focus_keys_and_semantic_actions_share_controlled_proposals as fn()),
        ("catalog_slider::slider_disable_callback_replacement_and_unmount_retire_old_actions", crate::catalog_slider::slider_disable_callback_replacement_and_unmount_retire_old_actions as fn()),
        ("catalog_slider::slider_focus_manager_close_during_pointer_focus_rejects_proposal", crate::catalog_slider::slider_focus_manager_close_during_pointer_focus_rejects_proposal as fn()),
    ]);
}

#[test]
fn controlled_disclosure_state_and_geometry() {
    run_cases("controlled_disclosure_state_and_geometry", &[
        ("catalog_disclosure::disclosure_proposes_controlled_changes_and_retains_real_header_focus", crate::catalog_disclosure::disclosure_proposes_controlled_changes_and_retains_real_header_focus as fn()),
        ("catalog_disclosure::disclosure_disabled_and_replaced_handlers_do_not_run_old_proposals", crate::catalog_disclosure::disclosure_disabled_and_replaced_handlers_do_not_run_old_proposals as fn()),
        ("catalog_disclosure::disclosure_indicator_obeys_constraints_and_explicit_reading_direction", crate::catalog_disclosure::disclosure_indicator_obeys_constraints_and_explicit_reading_direction as fn()),
        ("catalog_disclosure::disclosure_focus_reentry_cannot_activate_after_focus_manager_close", crate::catalog_disclosure::disclosure_focus_reentry_cannot_activate_after_focus_manager_close as fn()),
        ("catalog_disclosure::disclosure_retained_expand_after_unmount_cannot_reach_a_new_control", crate::catalog_disclosure::disclosure_retained_expand_after_unmount_cannot_reach_a_new_control as fn()),
        ("catalog_disclosure::disclosure_owns_constructor_clone_and_retirement_failure_tails", crate::catalog_disclosure::disclosure_owns_constructor_clone_and_retirement_failure_tails as fn()),
    ]);
}
