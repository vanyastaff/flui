//! Contract tables: each `#[test]` here is one capability family whose rows are the
//! scenario functions of that family. A failing row is named in the panic message, and
//! one failing row never hides the others.

use crate::common::cases::run_cases;

/// Input regions: what a window hands the OS for `SystemGestureExclusion`, mapped from
/// layout through transforms and clips, and re-sent only when its projection changed.
#[test]
fn input_regions() {
    run_cases(
        "input_regions",
        &[
            ("system_gesture_exclusion::input_regions_reach_the_os_only_when_changed", crate::system_gesture_exclusion::input_regions_reach_the_os_only_when_changed as fn()),
            ("system_gesture_exclusion::a_transformed_region_reports_its_bounding_box", crate::system_gesture_exclusion::a_transformed_region_reports_its_bounding_box),
            ("system_gesture_exclusion::a_clipped_region_reports_the_intersection", crate::system_gesture_exclusion::a_clipped_region_reports_the_intersection),
            ("system_gesture_exclusion::a_region_beyond_the_window_is_clamped_to_it", crate::system_gesture_exclusion::a_region_beyond_the_window_is_clamped_to_it),
            ("system_gesture_exclusion::a_region_in_a_scrolled_list_item_reports_the_scrolled_rect", crate::system_gesture_exclusion::a_region_in_a_scrolled_list_item_reports_the_scrolled_rect),
            ("system_gesture_exclusion::a_scale_change_resends_unchanged_logical_regions", crate::system_gesture_exclusion::a_scale_change_resends_unchanged_logical_regions),
            ("system_gesture_exclusion::a_display_change_at_equal_scale_resends_regions", crate::system_gesture_exclusion::a_display_change_at_equal_scale_resends_regions),
            ("system_gesture_exclusion::a_display_appearing_or_vanishing_resends_regions", crate::system_gesture_exclusion::a_display_appearing_or_vanishing_resends_regions),
            ("system_gesture_exclusion::fractional_region_edges_round_outward", crate::system_gesture_exclusion::fractional_region_edges_round_outward),
            ("system_gesture_exclusion::a_degenerate_transform_drops_the_region_with_a_diagnostic", crate::system_gesture_exclusion::a_degenerate_transform_drops_the_region_with_a_diagnostic),
            ("system_gesture_exclusion::a_region_crossing_the_perspective_horizon_is_dropped", crate::system_gesture_exclusion::a_region_crossing_the_perspective_horizon_is_dropped),
        ],
    );
}

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
            ("navigator_public::explicit_finish_commits_an_authored_page_and_retires_the_old_route", crate::navigator_public::explicit_finish_commits_an_authored_page_and_retires_the_old_route),
            ("navigator_public::push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing", crate::navigator_public::push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing),
            ("navigator_public::a_route_key_carries_its_result_type_from_registration_to_delivery", crate::navigator_public::a_route_key_carries_its_result_type_from_registration_to_delivery),
            ("hero_controller::a_hero_controller_does_not_deadlock_the_observer_callback", crate::hero_controller::a_hero_controller_does_not_deadlock_the_observer_callback),
            ("hero_controller::replacing_a_hero_navigator_retires_routes_after_attachment", crate::hero_controller::replacing_a_hero_navigator_retires_routes_after_attachment),
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
            ("gesture_detector::event_cx::a_panicking_assistive_action_leaves_the_next_one_deliverable", crate::gesture_detector::event_cx::a_panicking_assistive_action_leaves_the_next_one_deliverable),
            ("listener::event_cx::a_refused_write_in_a_pointer_callback_is_reported_not_panicked", crate::listener::event_cx::a_refused_write_in_a_pointer_callback_is_reported_not_panicked),
            ("raw_button::a_refused_write_in_a_press_is_reported_not_panicked", crate::raw_button::a_refused_write_in_a_press_is_reported_not_panicked),
            ("semantics::a_refused_write_in_an_action_handler_is_reported_not_panicked", crate::semantics::a_refused_write_in_an_action_handler_is_reported_not_panicked),
            ("semantics::an_action_invoked_outside_its_ui_runtime_is_dropped_with_a_warning", crate::semantics::an_action_invoked_outside_its_ui_runtime_is_dropped_with_a_warning),
            ("text_field_widget::raw_text_field_callbacks_write_through_the_forwarded_cx", crate::text_field_widget::raw_text_field_callbacks_write_through_the_forwarded_cx),
            ("form::a_panicking_reset_callback_does_not_disable_later_form_validation", crate::form::a_panicking_reset_callback_does_not_disable_later_form_validation),
            ("form::a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first", crate::form::a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first),
            ("gesture_detector::horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector", crate::gesture_detector::horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector),
            ("gesture_detector::a_tap_cancel_only_follows_a_tap_down", crate::gesture_detector::a_tap_cancel_only_follows_a_tap_down),
            ("gesture_detector::a_rotated_axial_drag_claims_motion_along_its_own_axis", crate::gesture_detector::a_rotated_axial_drag_claims_motion_along_its_own_axis),
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
            ("gesture_detector::event_cx::repeated_assistive_taps_run_once_each_in_their_dispatch", crate::gesture_detector::event_cx::repeated_assistive_taps_run_once_each_in_their_dispatch),
            ("gesture_detector::event_cx::repeated_assistive_long_presses_run_once_each_in_their_dispatch", crate::gesture_detector::event_cx::repeated_assistive_long_presses_run_once_each_in_their_dispatch),
            ("semantics::an_action_handler_writes_a_signal_and_rebuilds_its_reader", crate::semantics::an_action_handler_writes_a_signal_and_rebuilds_its_reader),
            ("media_query_fields::a_size_only_change_rebuilds_size_and_whole_readers_only", crate::media_query_fields::a_size_only_change_rebuilds_size_and_whole_readers_only),
            ("media_query_fields::safe_area_reserves_the_home_indicator_only_while_the_keyboard_is_down", crate::media_query_fields::safe_area_reserves_the_home_indicator_only_while_the_keyboard_is_down),
            ("media_query_fields::removing_the_bottom_view_inset_shrinks_view_padding_by_what_it_covered", crate::media_query_fields::removing_the_bottom_view_inset_shrinks_view_padding_by_what_it_covered),
            ("media_query_fields::padding_is_derived_from_view_padding_and_view_insets", crate::media_query_fields::padding_is_derived_from_view_padding_and_view_insets),
            ("media_query_fields::consuming_and_overriding_padding_updates_its_sources", crate::media_query_fields::consuming_and_overriding_padding_updates_its_sources),
            ("media_query_fields::a_text_scale_change_relayouts_a_preserved_text_subtree",crate::media_query_fields::a_text_scale_change_relayouts_a_preserved_text_subtree),
            ("directionality_dependency::a_start_aligned_column_depends_on_directionality", crate::directionality_dependency::a_start_aligned_column_depends_on_directionality),
            ("localizations::the_global_delegate_makes_an_rtl_locale_subtree_rtl", crate::localizations::the_global_delegate_makes_an_rtl_locale_subtree_rtl),
            ("hot_reload_state::perform_reassemble_rebuilds_in_place_and_preserves_state", crate::hot_reload_state::perform_reassemble_rebuilds_in_place_and_preserves_state),
            ("layout_builder::layout_builder_constraint_change_rebuilds_in_the_same_frame", crate::layout_builder::layout_builder_constraint_change_rebuilds_in_the_same_frame),
        ],
    );
}

/// Drag and drop on the sessions (ADR-0211): `Draggable` and `DropTarget` within the
/// window and out of it, their keyboard and assistive paths, and what a drag survives.
#[test]
fn drag_and_drop() {
    run_cases(
        "drag_and_drop",
        &[
            ("drag_and_drop::a_drag_drops_on_the_target_that_accepted_its_last_move", crate::drag_and_drop::a_drag_drops_on_the_target_that_accepted_its_last_move as fn()),
            ("drag_and_drop::a_drag_leaving_the_window_escalates_to_the_os_when_it_may_reach_it", crate::drag_and_drop::a_drag_leaving_the_window_escalates_to_the_os_when_it_may_reach_it),
            ("drag_and_drop::escape_cancels_a_drag_in_its_window", crate::drag_and_drop::escape_cancels_a_drag_in_its_window),
            ("drag_and_drop::cut_and_paste_by_keyboard_drop_like_a_drag", crate::drag_and_drop::cut_and_paste_by_keyboard_drop_like_a_drag),
            ("drag_and_drop::a_target_removed_mid_drag_takes_no_drop", crate::drag_and_drop::a_target_removed_mid_drag_takes_no_drop),
            ("drag_and_drop::unmounting_the_source_mid_drag_cancels_it", crate::drag_and_drop::unmounting_the_source_mid_drag_cancels_it),
            ("drag_and_drop::a_panicking_drop_callback_ends_the_drag_cancelled_and_the_next_drops", crate::drag_and_drop::a_panicking_drop_callback_ends_the_drag_cancelled_and_the_next_drops),
            ("drag_and_drop::a_paste_drops_at_the_targets_reading_start_corner", crate::drag_and_drop::a_paste_drops_at_the_targets_reading_start_corner),
            ("drag_and_drop::the_cursor_shows_what_a_drop_would_do", crate::drag_and_drop::the_cursor_shows_what_a_drop_would_do),
            ("drag_and_drop::spring_loading_fires_once_per_hover", crate::drag_and_drop::spring_loading_fires_once_per_hover),
            ("drag_and_drop::a_long_press_trigger_lifts_after_the_hold", crate::drag_and_drop::a_long_press_trigger_lifts_after_the_hold),
            ("drag_and_drop::a_drag_reports_to_the_callbacks_it_started_with", crate::drag_and_drop::a_drag_reports_to_the_callbacks_it_started_with),
            ("drag_and_drop::the_feedback_keeps_its_anchor_in_an_inset_overlay", crate::drag_and_drop::the_feedback_keeps_its_anchor_in_an_inset_overlay),
            ("drag_and_drop::autoscroll_finds_the_target_scrolled_under_a_resting_pointer", crate::drag_and_drop::autoscroll_finds_the_target_scrolled_under_a_resting_pointer),
            ("drag_and_drop::unmounting_a_target_releases_its_receiver", crate::drag_and_drop::unmounting_a_target_releases_its_receiver),
            ("drag_and_drop::a_dragged_row_scrolled_out_of_a_lazy_list_keeps_its_drag", crate::drag_and_drop::a_dragged_row_scrolled_out_of_a_lazy_list_keeps_its_drag),
            ("drag_and_drop::the_alternatives_perform_the_transfer_the_drag_would", crate::drag_and_drop::the_alternatives_perform_the_transfer_the_drag_would),
            ("drag_and_drop::draggable_reads_admission_profiles_and_retires_authored_owners", crate::drag_and_drop::draggable_reads_admission_profiles_and_retires_authored_owners),
        ],
    );
}

/// The editable-text capability: selection gestures, obscuring, IME, clipboard, keys.
#[test]
fn text_editing() {
    run_cases(
        "text_editing",
        &[
            ("keyboard_modifiers::explicit_altgraph_character_reaches_text_instead_of_a_command", crate::keyboard_modifiers::explicit_altgraph_character_reaches_text_instead_of_a_command),
            ("keyboard_modifiers::ctrl_alt_fallback_depends_on_whether_a_text_field_is_focused", crate::keyboard_modifiers::ctrl_alt_fallback_depends_on_whether_a_text_field_is_focused),
            ("keyboard_modifiers::locks_release_repeat_and_next_operation_preserve_existing_behavior", crate::keyboard_modifiers::locks_release_repeat_and_next_operation_preserve_existing_behavior),
            ("keyboard_modifiers::altgraph_word_aliases_preserve_character_movement", crate::keyboard_modifiers::altgraph_word_aliases_preserve_character_movement),
            ("keyboard_modifiers::source_composition_preserves_editing_and_submit", crate::keyboard_modifiers::source_composition_preserves_editing_and_submit),
            ("editable_text::inherited_text_sizing_updates_editable_glyphs_and_caret", crate::editable_text::inherited_text_sizing_updates_editable_glyphs_and_caret),
            ("editable_text::native_actions::queued_focus_and_text_reach_the_current_field_and_event_context", crate::editable_text::native_actions::queued_focus_and_text_reach_the_current_field_and_event_context),
            ("editable_text::native_actions::native_actions_follow_the_replacement_controller_and_focus_node", crate::editable_text::native_actions::native_actions_follow_the_replacement_controller_and_focus_node),
            ("editable_text::native_actions::disabled_unmounted_and_closed_fields_refuse_native_actions", crate::editable_text::native_actions::disabled_unmounted_and_closed_fields_refuse_native_actions),
            ("editable_text::native_actions::a_deferred_focus_change_preserves_the_semantic_edit_that_follows_it", crate::editable_text::native_actions::a_deferred_focus_change_preserves_the_semantic_edit_that_follows_it),
            ("editable_text::native_actions::re_adoption_before_queued_delivery_retires_the_old_field_authority", crate::editable_text::native_actions::re_adoption_before_queued_delivery_retires_the_old_field_authority),
            ("editable_text::native_actions::re_adoption_during_a_deferred_grant_refuses_the_resumed_semantic_edit", crate::editable_text::native_actions::re_adoption_during_a_deferred_grant_refuses_the_resumed_semantic_edit),
            ("editable_text::native_actions::a_screen_reader_moves_by_word_where_the_caret_steps", crate::editable_text::native_actions::a_screen_reader_moves_by_word_where_the_caret_steps),
            ("editable_text::native_actions::a_screen_reader_types_into_an_empty_field_and_selects_in_it", crate::editable_text::native_actions::a_screen_reader_types_into_an_empty_field_and_selects_in_it),
            ("editable_text::native_actions::a_password_field_publishes_only_its_mask", crate::editable_text::native_actions::a_password_field_publishes_only_its_mask),
            ("editable_text::native_actions::a_long_cluster_and_a_long_line_read_whole", crate::editable_text::native_actions::a_long_cluster_and_a_long_line_read_whole),
            ("editable_text::a_double_tap_selects_the_word_under_it", crate::editable_text::a_double_tap_selects_the_word_under_it as fn()),
            ("editable_text::a_drag_selects_from_its_start_to_the_pointer", crate::editable_text::a_drag_selects_from_its_start_to_the_pointer),
            ("editable_text::disabling_the_field_retires_its_selection_contact", crate::editable_text::disabling_the_field_retires_its_selection_contact),
            ("editable_text::replacing_the_controller_retires_the_old_selection_contact", crate::editable_text::replacing_the_controller_retires_the_old_selection_contact),
            ("editable_text::selection_drag_survives_a_same_controller_rebuild", crate::editable_text::selection_drag_survives_a_same_controller_rebuild),
            ("editable_text::a_field_highlights_its_selection_with_the_inherited_style", crate::editable_text::a_field_highlights_its_selection_with_the_inherited_style),
            ("editable_text::foreign_release_preserves_the_selection_contact", crate::editable_text::foreign_release_preserves_the_selection_contact),
            ("editable_text::foreign_cancel_preserves_the_selection_contact", crate::editable_text::foreign_cancel_preserves_the_selection_contact),
            ("editable_text::insertion_keeps_the_caret_after_the_joined_combining_cluster", crate::editable_text::insertion_keeps_the_caret_after_the_joined_combining_cluster),
            ("editable_text::deleting_a_separator_keeps_the_caret_after_the_joined_flag", crate::editable_text::deleting_a_separator_keeps_the_caret_after_the_joined_flag),
            ("editable_text::text_store::rtl_scalar_rect_midpoints_resolve_to_the_source_scalar", crate::editable_text::text_store::rtl_scalar_rect_midpoints_resolve_to_the_source_scalar),
            ("editable_text::the_store_reports_the_purpose_the_field_was_built_with", crate::editable_text::the_store_reports_the_purpose_the_field_was_built_with),
            ("editable_text::a_tap_places_the_caret_where_it_landed", crate::editable_text::a_tap_places_the_caret_where_it_landed),
            ("editable_text::a_field_measures_and_hit_tests_the_current_text_scale", crate::editable_text::a_field_measures_and_hit_tests_the_current_text_scale),
            ("editable_text::a_pointer_down_and_a_paste_commit_the_composition_first", crate::editable_text::a_pointer_down_and_a_paste_commit_the_composition_first),
            ("editable_text::moving_focus_off_a_composing_field_commits_it_in_either_mount_order", crate::editable_text::moving_focus_off_a_composing_field_commits_it_in_either_mount_order),
            ("editable_text::a_press_reentered_by_its_commit_is_not_a_second_contact", crate::editable_text::a_press_reentered_by_its_commit_is_not_a_second_contact),
            ("editable_text::an_obscured_field_never_hands_its_real_text_to_the_render_object", crate::editable_text::an_obscured_field_never_hands_its_real_text_to_the_render_object),
            ("editable_text::the_editor_steps_the_graphemes_the_painter_snaps_to", crate::editable_text::the_editor_steps_the_graphemes_the_painter_snaps_to),
            ("editable_text::focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller", crate::editable_text::focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller),
            ("editable_text::text_store::store_offsets_match_controller_bytes_across_surrogates_and_graphemes", crate::editable_text::text_store::store_offsets_match_controller_bytes_across_surrogates_and_graphemes),
            ("editable_text::text_store::typing_after_a_deferred_commit_lands_after_the_commit", crate::editable_text::text_store::typing_after_a_deferred_commit_lands_after_the_commit),
            ("editable_text::text_store::on_changed_runs_after_the_lock_is_released", crate::editable_text::text_store::on_changed_runs_after_the_lock_is_released),
            ("editable_text::text_store::the_owner_reads_the_controller_from_its_callbacks", crate::editable_text::text_store::the_owner_reads_the_controller_from_its_callbacks),
            ("editable_text::text_store::an_app_edit_during_a_lock_is_not_overwritten", crate::editable_text::text_store::an_app_edit_during_a_lock_is_not_overwritten),
            ("editable_text::text_store::a_no_layout_answer_is_followed_by_a_layout_notification", crate::editable_text::text_store::a_no_layout_answer_is_followed_by_a_layout_notification),
            ("editable_text::text_store::focus_gain_and_loss_reach_the_store_host", crate::editable_text::text_store::focus_gain_and_loss_reach_the_store_host),
            ("editable_text::text_store::a_conversion_queued_for_an_unmounted_field_is_dropped", crate::editable_text::text_store::a_conversion_queued_for_an_unmounted_field_is_dropped),
            ("editable_text::text_store::unmounting_a_composing_field_commits_the_preedit_once", crate::editable_text::text_store::unmounting_a_composing_field_commits_the_preedit_once),
            ("editable_text::text_store::swapping_the_controller_during_a_grant_drops_the_session", crate::editable_text::text_store::swapping_the_controller_during_a_grant_drops_the_session),
            ("editable_text::text_store::a_panicking_on_changed_is_reported_once_and_the_field_keeps_working", crate::editable_text::text_store::a_panicking_on_changed_is_reported_once_and_the_field_keeps_working),
            ("form::a_text_form_field_validates_and_saves_the_committed_text", crate::form::a_text_form_field_validates_and_saves_the_committed_text),
            ("form::dropping_a_composing_controller_keeps_the_preedit_out_of_the_value", crate::form::dropping_a_composing_controller_keeps_the_preedit_out_of_the_value),
            ("form::setting_the_shown_text_as_the_value_ends_a_reconversion", crate::form::setting_the_shown_text_as_the_value_ends_a_reconversion),
            ("form::setting_the_shown_preedit_as_the_value_commits_it", crate::form::setting_the_shown_preedit_as_the_value_commits_it),
            ("form::a_reset_mid_composition_removes_the_preedit", crate::form::a_reset_mid_composition_removes_the_preedit),
            ("editable_text::text_store::long_input_reveals_the_caret_and_maps_visible_pointer_positions", crate::editable_text::text_store::long_input_reveals_the_caret_and_maps_visible_pointer_positions),
            ("editable_text::text_store::editable_paint_places_long_text_under_the_viewport_clip", crate::editable_text::text_store::editable_paint_places_long_text_under_the_viewport_clip),
            ("editable_text_clipboard::copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed", crate::editable_text_clipboard::copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed),
            ("editable_text_clipboard::paste_rechecks_focus_after_committing_composition", crate::editable_text_clipboard::paste_rechecks_focus_after_committing_composition),
            ("editable_text_clipboard::copy_then_paste_round_trips_text_in_an_editable_text", crate::editable_text_clipboard::copy_then_paste_round_trips_text_in_an_editable_text),
            ("editable_text_clipboard::a_field_selection_is_offered_lazily_as_the_primary_selection", crate::editable_text_clipboard::a_field_selection_is_offered_lazily_as_the_primary_selection),
            ("editable_text_clipboard::a_middle_press_pastes_the_primary_selection_where_it_lands", crate::editable_text_clipboard::a_middle_press_pastes_the_primary_selection_where_it_lands),
            ("editable_text_clipboard::a_selection_held_before_mounting_is_offered", crate::editable_text_clipboard::a_selection_held_before_mounting_is_offered),
            ("editable_text_clipboard::a_panicking_listener_keeps_the_middle_press_paste", crate::editable_text_clipboard::a_panicking_listener_keeps_the_middle_press_paste),
            ("editable_text_clipboard::a_selection_changed_while_the_field_claims_stands", crate::editable_text_clipboard::a_selection_changed_while_the_field_claims_stands),
            ("editable_text_clipboard::an_unobscuring_rebuild_claims_for_the_new_controller", crate::editable_text_clipboard::an_unobscuring_rebuild_claims_for_the_new_controller),
            ("editable_text_clipboard::the_harness_releases_the_button_it_pressed", crate::editable_text_clipboard::the_harness_releases_the_button_it_pressed),
            ("editable_text_clipboard::select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit", crate::editable_text_clipboard::select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit),
            ("editable_text_clipboard::select_all_without_a_focused_text_field_leaves_the_key_unconsumed", crate::editable_text_clipboard::select_all_without_a_focused_text_field_leaves_the_key_unconsumed),
            ("editable_text_clipboard::select_all_defers_to_an_active_composition_and_recovers_after_commit", crate::editable_text_clipboard::select_all_defers_to_an_active_composition_and_recovers_after_commit),
            ("text_field::focused_character_key_inserts_into_controller", crate::text_field::focused_character_key_inserts_into_controller),
            ("text_field_widget::a_press_on_a_field_takes_no_focus_back_once_focus_moves_on", crate::text_field_widget::a_press_on_a_field_takes_no_focus_back_once_focus_moves_on),
            ("text_field::unfocused_field_does_not_receive_key_events", crate::text_field::unfocused_field_does_not_receive_key_events),
            ("text::an_enclosing_default_text_style_styles_a_bare_run", crate::text::an_enclosing_default_text_style_styles_a_bare_run),
            ("text::the_system_text_scale_grows_each_span_once", crate::text::the_system_text_scale_grows_each_span_once),
            ("text::media_text_scaling_changes_the_laid_out_text", crate::text::media_text_scaling_changes_the_laid_out_text),
            ("text::icon_text_sizing_keeps_box_and_glyph_together", crate::text::icon_text_sizing_keeps_box_and_glyph_together),
            ("text::icon_labels_reach_the_assembled_accessibility_tree", crate::text::icon_labels_reach_the_assembled_accessibility_tree),
            ("text::a_tap_on_a_link_reaches_on_link_at_the_current_text_scale", crate::text::a_tap_on_a_link_reaches_on_link_at_the_current_text_scale),
            ("text::a_link_shows_a_pointing_hand_and_a_span_its_own_cursor", crate::text::a_link_shows_a_pointing_hand_and_a_span_its_own_cursor),
            ("text::a_handled_link_is_a_link_node_assistive_technology_clicks", crate::text::a_handled_link_is_a_link_node_assistive_technology_clicks),
            ("text::tab_stops_on_each_link_and_enter_opens_it", crate::text::tab_stops_on_each_link_and_enter_opens_it),
            ("text::a_focused_link_rings_each_line_of_its_glyphs", crate::text::a_focused_link_rings_each_line_of_its_glyphs),
            ("text::assistive_technology_focuses_a_link", crate::text::assistive_technology_focuses_a_link),
            ("text::tab_to_a_link_below_the_view_scrolls_it_into_view", crate::text::tab_to_a_link_below_the_view_scrolls_it_into_view),
            ("text::a_shadowed_link_takes_no_stop_and_a_textless_label_is_still_read", crate::text::a_shadowed_link_takes_no_stop_and_a_textless_label_is_still_read),
            ("text::select_opens_a_link_and_space_activates_nothing_around_it", crate::text::select_opens_a_link_and_space_activates_nothing_around_it),
            ("text::a_link_the_layout_cut_takes_no_focus", crate::text::a_link_the_layout_cut_takes_no_focus),
            ("text::links_keep_their_focus_and_presses_when_links_before_them_change", crate::text::links_keep_their_focus_and_presses_when_links_before_them_change),
            ("text::unmounting_a_linked_paragraph_releases_its_links", crate::text::unmounting_a_linked_paragraph_releases_its_links),
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
            ("pointer_vocabulary::viewer_touch_focal_inertia_uses_the_admitted_profile", crate::pointer_vocabulary::viewer_touch_focal_inertia_uses_the_admitted_profile as fn()),
            ("pointer_vocabulary::viewer_native_focal_inertia_uses_the_admitted_profile", crate::pointer_vocabulary::viewer_native_focal_inertia_uses_the_admitted_profile),
            ("pointer_vocabulary::viewer_focal_fling_rebuild_preserves_or_retires_geometry", crate::pointer_vocabulary::viewer_focal_fling_rebuild_preserves_or_retires_geometry as fn()),
            ("pointer_vocabulary::viewer_repeated_native_start_retires_the_previous_generation", crate::pointer_vocabulary::viewer_repeated_native_start_retires_the_previous_generation as fn()),
            ("pointer_vocabulary::viewer_native_owner_survives_descendant_enable_during_rebuild", crate::pointer_vocabulary::viewer_native_owner_survives_descendant_enable_during_rebuild),
            ("pointer_vocabulary::viewer_native_rotation_preserves_the_scene_pivot", crate::pointer_vocabulary::viewer_native_rotation_preserves_the_scene_pivot),
            ("pointer_vocabulary::viewer_rotation_refuses_an_unfittable_quad_then_recovers", crate::pointer_vocabulary::viewer_rotation_refuses_an_unfittable_quad_then_recovers),
            ("pointer_vocabulary::viewer_extreme_finite_pan_preserves_the_boundary_result", crate::pointer_vocabulary::viewer_extreme_finite_pan_preserves_the_boundary_result),
            ("pointer_vocabulary::viewer_focal_fling_advances_then_stops_on_new_input", crate::pointer_vocabulary::viewer_focal_fling_advances_then_stops_on_new_input),
            ("pointer_vocabulary::viewer_reports_scale_velocity_separately_from_focal_velocity", crate::pointer_vocabulary::viewer_reports_scale_velocity_separately_from_focal_velocity),
            ("touchpad_pan::a_touchpad_pan_drives_a_pan_only_gesture_detector", crate::touchpad_pan::a_touchpad_pan_drives_a_pan_only_gesture_detector as fn()),
            ("touchpad_pan::a_scrollable_inside_a_horizontal_drag_wins_a_vertical_pan", crate::touchpad_pan::a_scrollable_inside_a_horizontal_drag_wins_a_vertical_pan),
            ("touchpad_pan::a_horizontal_pan_beside_a_vertical_list_goes_to_the_drag", crate::touchpad_pan::a_horizontal_pan_beside_a_vertical_list_goes_to_the_drag),
            ("touchpad_pan::a_list_with_nowhere_to_go_leaves_the_pan_to_the_drag", crate::touchpad_pan::a_list_with_nowhere_to_go_leaves_the_pan_to_the_drag),
            ("touchpad_pan::a_plain_list_scrolls_with_a_touchpad_pan_as_before", crate::touchpad_pan::a_plain_list_scrolls_with_a_touchpad_pan_as_before),
            ("listener::presentation_resampling_uses_the_owner_frame_clock", crate::listener::presentation_resampling_uses_the_owner_frame_clock as fn()),
            ("gesture_detector::pan_claim_region_keeps_the_pan_from_an_enclosing_drag", crate::gesture_detector::pan_claim_region_keeps_the_pan_from_an_enclosing_drag),
            ("gesture_detector::recognizers_follow_a_changed_arena_scope", crate::gesture_detector::recognizers_follow_a_changed_arena_scope),
            ("gesture_detector::a_retired_recognizer_panicking_on_cancel_leaves_the_kept_ones_updated", crate::gesture_detector::a_retired_recognizer_panicking_on_cancel_leaves_the_kept_ones_updated),
            ("gesture_detector::drag_and_transform_slots_carry_their_locks", crate::gesture_detector::drag_and_transform_slots_carry_their_locks),
            ("gesture_detector::a_composition_past_the_delay_ceiling_is_reported", crate::gesture_detector::a_composition_past_the_delay_ceiling_is_reported),
            ("gesture_detector::drag_and_transform_slots_split_one_contact_from_two", crate::gesture_detector::drag_and_transform_slots_split_one_contact_from_two),
            ("gesture_detector::binding_admits_distinct_drag_sequences_when_pointer_ids_are_reused", crate::gesture_detector::binding_admits_distinct_drag_sequences_when_pointer_ids_are_reused),
            ("gesture_detector::drag_motion_retains_root_units_under_scale_and_rotation", crate::gesture_detector::drag_motion_retains_root_units_under_scale_and_rotation),
            ("gesture_detector::a_smart_zoom_reaches_only_the_innermost_detector_under_it", crate::gesture_detector::a_smart_zoom_reaches_only_the_innermost_detector_under_it),
            ("gesture_detector::a_detector_in_a_composed_scope_preserves_double_tap_timing", crate::gesture_detector::a_detector_in_a_composed_scope_preserves_double_tap_timing),
            ("pointer_vocabulary::viewer_native_pan_moves_the_scene_under_the_focal_point", crate::pointer_vocabulary::viewer_native_pan_moves_the_scene_under_the_focal_point as fn()),
            ("pointer_vocabulary::viewer_keeps_a_pinch_whose_focal_point_leaves_it", crate::pointer_vocabulary::viewer_keeps_a_pinch_whose_focal_point_leaves_it as fn()),
            ("pointer_vocabulary::viewer_native_session_reports_one_start_and_one_terminal", crate::pointer_vocabulary::viewer_native_session_reports_one_start_and_one_terminal),
            ("pointer_vocabulary::viewer_pan_transitions_to_pinch_without_contact_count_jumps", crate::pointer_vocabulary::viewer_pan_transitions_to_pinch_without_contact_count_jumps),
            ("pointer_vocabulary::scroll_claim_preserves_owned_source_units_and_phase", crate::pointer_vocabulary::scroll_claim_preserves_owned_source_units_and_phase as fn()),
            ("pointer_vocabulary::pointer_delivery_preserves_source_and_sample_families", crate::pointer_vocabulary::pointer_delivery_preserves_source_and_sample_families),
            ("pointer_vocabulary::page_scroll_resolves_against_the_actual_viewport", crate::pointer_vocabulary::page_scroll_resolves_against_the_actual_viewport),
            ("pointer_vocabulary::viewer_raw_detents_zoom_without_stealing_plain_scrolls", crate::pointer_vocabulary::viewer_raw_detents_zoom_without_stealing_plain_scrolls),
            ("pointer_vocabulary::viewer_page_zoom_resolves_against_the_actual_viewport", crate::pointer_vocabulary::viewer_page_zoom_resolves_against_the_actual_viewport),
            ("pointer_vocabulary::viewer_cumulative_zoom_survives_rebuild_and_resets", crate::pointer_vocabulary::viewer_cumulative_zoom_survives_rebuild_and_resets),
            ("pointer_vocabulary::viewer_unstarted_pinch_updates_remain_independent_steps", crate::pointer_vocabulary::viewer_unstarted_pinch_updates_remain_independent_steps),
            ("pointer_vocabulary::viewer_extreme_zoom_reports_the_finite_applied_change", crate::pointer_vocabulary::viewer_extreme_zoom_reports_the_finite_applied_change),
            ("pointer_vocabulary::viewer_page_overflow_and_empty_viewport_recover", crate::pointer_vocabulary::viewer_page_overflow_and_empty_viewport_recover),
            ("interactive_viewer::viewer_keys_pan_and_zoom_from_the_focused_row", crate::interactive_viewer::viewer_keys_pan_and_zoom_from_the_focused_row),
            ("interactive_viewer::keys_typed_in_a_field_inside_the_viewer_stay_in_the_field", crate::interactive_viewer::keys_typed_in_a_field_inside_the_viewer_stay_in_the_field),
            ("interactive_viewer::viewer_assistive_actions_zoom_and_pan", crate::interactive_viewer::viewer_assistive_actions_zoom_and_pan),
            ("interactive_viewer::viewer_double_tap_zooms_about_the_tap_and_back_out_at_the_maximum", crate::interactive_viewer::viewer_double_tap_zooms_about_the_tap_and_back_out_at_the_maximum),
            ("interactive_viewer::viewer_degenerate_geometry_and_bounds_publish_finite_transforms", crate::interactive_viewer::viewer_degenerate_geometry_and_bounds_publish_finite_transforms),
            ("interactive_viewer::viewer_rebuild_from_its_update_callback_keeps_the_gesture", crate::interactive_viewer::viewer_rebuild_from_its_update_callback_keeps_the_gesture),
            ("interactive_viewer::viewer_pinch_past_the_scale_bounds_stretches_and_snaps_back", crate::interactive_viewer::viewer_pinch_past_the_scale_bounds_stretches_and_snaps_back),
            ("interactive_viewer::viewer_touch_twist_rotates_only_when_rotation_is_enabled", crate::interactive_viewer::viewer_touch_twist_rotates_only_when_rotation_is_enabled),
            ("interactive_viewer::viewer_contact_gives_it_the_keys", crate::interactive_viewer::viewer_contact_gives_it_the_keys),
            ("interactive_viewer::viewer_key_sequence_ends_when_focus_moves_before_the_release", crate::interactive_viewer::viewer_key_sequence_ends_when_focus_moves_before_the_release),
            ("gesture_detector::clearing_pan_callbacks_mid_drag_still_finishes_the_drag", crate::gesture_detector::clearing_pan_callbacks_mid_drag_still_finishes_the_drag as fn()),
            ("gesture_detector::mounted_drag_policy_replaces_targets_before_cancellation_and_recovers", crate::gesture_detector::mounted_drag_policy_replaces_targets_before_cancellation_and_recovers),
            ("gesture_detector::authored_settings_replace_active_owners_and_preserve_equal_profiles", crate::gesture_detector::authored_settings_replace_active_owners_and_preserve_equal_profiles),
            ("gesture_detector::authored_settings_retire_tap_candidates_and_deadlines", crate::gesture_detector::authored_settings_retire_tap_candidates_and_deadlines),
            ("gesture_detector::authored_settings_retire_native_scale_session_before_fresh_admission", crate::gesture_detector::authored_settings_retire_native_scale_session_before_fresh_admission),
            ("gesture_detector::mounted_native_begin_retains_estimator_before_first_claim", crate::gesture_detector::mounted_native_begin_retains_estimator_before_first_claim),
            ("gesture_detector::mounted_native_begin_refused_by_touch_cannot_claim_after_touch_terminal", crate::gesture_detector::mounted_native_begin_refused_by_touch_cannot_claim_after_touch_terminal),
            ("gesture_detector::nested_native_scale_loser_recovers_touch_after_winner_terminal", crate::gesture_detector::nested_native_scale_loser_recovers_touch_after_winner_terminal),
            ("gesture_detector::scoped_settings_control_touch_recognition_thresholds", crate::gesture_detector::scoped_settings_control_touch_recognition_thresholds),
            ("gesture_detector::scoped_settings_control_gesture_deadlines", crate::gesture_detector::scoped_settings_control_gesture_deadlines),
            ("gesture_detector::scoped_estimator_controls_delivered_drag_velocity", crate::gesture_detector::scoped_estimator_controls_delivered_drag_velocity),
            ("gesture_detector::unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival", crate::gesture_detector::unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival as fn()),
            ("gesture_detector::viewer_reports_cancelled_then_completed_interactions", crate::gesture_detector::viewer_reports_cancelled_then_completed_interactions as fn()),
            ("gesture_detector::gesture_detector_fires_on_tap_for_a_down_up_on_the_child", crate::gesture_detector::gesture_detector_fires_on_tap_for_a_down_up_on_the_child as fn()),
            ("gesture_detector::toggling_on_tap_keeps_the_child_state", crate::gesture_detector::toggling_on_tap_keeps_the_child_state),
            ("gesture_detector::gesture_detector_recognizes_a_pan_and_suppresses_the_tap", crate::gesture_detector::gesture_detector_recognizes_a_pan_and_suppresses_the_tap),
            ("gesture_detector::a_vertical_drag_slot_claims_only_vertical_motion", crate::gesture_detector::a_vertical_drag_slot_claims_only_vertical_motion),
            ("gesture_detector::two_contacts_released_together_fire_on_multi_tap", crate::gesture_detector::two_contacts_released_together_fire_on_multi_tap),
            ("gesture_detector::a_pressure_contact_starts_peaks_and_ends_a_force_press", crate::gesture_detector::a_pressure_contact_starts_peaks_and_ends_a_force_press),
            ("gesture_detector::scoped_position_smoothing_reaches_drag_slot_updates", crate::gesture_detector::scoped_position_smoothing_reaches_drag_slot_updates),
            ("gesture_detector_advanced::double_tap_combined_with_tap_fires_double_tap_once_and_tap_never", crate::gesture_detector_advanced::double_tap_combined_with_tap_fires_double_tap_once_and_tap_never),
            ("gesture_detector_advanced::long_press_fires_when_held_past_the_deadline", crate::gesture_detector_advanced::long_press_fires_when_held_past_the_deadline),
            ("absorb_pointer::absorbing_true_blocks_the_tap_from_reaching_a_child_gesture_detector", crate::absorb_pointer::absorbing_true_blocks_the_tap_from_reaching_a_child_gesture_detector),
            ("hit_slop::overlapping_slops_go_to_the_nearest_edge_then_the_topmost", crate::hit_slop::overlapping_slops_go_to_the_nearest_edge_then_the_topmost),
            ("hit_slop::a_visual_hit_beats_a_neighbours_slop", crate::hit_slop::a_visual_hit_beats_a_neighbours_slop),
            ("hit_slop::slop_does_not_leave_its_composited_layer", crate::hit_slop::slop_does_not_leave_its_composited_layer),
            ("hit_slop::slop_does_not_cross_an_overlay_entry", crate::hit_slop::slop_does_not_cross_an_overlay_entry),
            ("hit_slop::content_shape_rejects_the_corners_of_a_circle", crate::hit_slop::content_shape_rejects_the_corners_of_a_circle),
            ("hit_slop::min_target_defaults_to_the_settings_preset", crate::hit_slop::min_target_defaults_to_the_settings_preset),
            ("hit_slop::min_target_follows_a_live_settings_update", crate::hit_slop::min_target_follows_a_live_settings_update),
            ("hit_slop::slop_takes_the_gap_between_items_in_their_own_layers", crate::hit_slop::slop_takes_the_gap_between_items_in_their_own_layers),
            ("hit_slop::a_modal_barrier_blocks_slop_on_both_sides", crate::hit_slop::a_modal_barrier_blocks_slop_on_both_sides),
            ("listener::listener_routes_down_and_up_to_their_own_callbacks", crate::listener::listener_routes_down_and_up_to_their_own_callbacks),
            ("listener::a_press_on_a_list_row_reaches_the_listeners_above_the_list", crate::listener::a_press_on_a_list_row_reaches_the_listeners_above_the_list),
            ("listener::listener_capture_retains_one_target_and_drop_delivers_loss", crate::listener::listener_capture_retains_one_target_and_drop_delivers_loss),
            ("listener::listener_unmount_preserves_one_captured_contact_terminal", crate::listener::listener_unmount_preserves_one_captured_contact_terminal),
            ("listener::listener_admission_keeps_terminal_delivery_and_weak_ownership", crate::listener::listener_admission_keeps_terminal_delivery_and_weak_ownership),
            ("listener::listener_raw_observer_panic_still_delivers_the_recognizer_event", crate::listener::listener_raw_observer_panic_still_delivers_the_recognizer_event),
            ("listener::a_revision_reaches_listeners_and_never_recognizers", crate::listener::a_revision_reaches_listeners_and_never_recognizers),
            ("listener::custom_recognizer_competes_through_a_listener", crate::listener::custom_recognizer_competes_through_a_listener),
            ("back_gesture::release_matrix_fling_and_slow_release", crate::back_gesture::release_matrix_fling_and_slow_release),
            ("page_route::back_gesture_edge_drag_normalizes_against_the_routes_real_width_not_the_hit_strip", crate::page_route::back_gesture_edge_drag_normalizes_against_the_routes_real_width_not_the_hit_strip),
        ],
    );
}

/// A focused field rebuilt under its key keeps the focus; one that is really
/// removed hands it, in the frame that removed it and without any input, to
/// the next field in order (else the first) in one edge that never passes
/// through `None` while a field remains.
///
/// Red-check: drop the settling step from the runtime's frame — the removed
/// field stays primary until the next input, and the removal rows fail.
#[test]
fn focus_survives_rebuild_and_moves_on_removal() {
    run_cases(
        "focus_survives_rebuild_and_moves_on_removal",
        &[
            (
                "focus::rebuild_with_the_same_keys_keeps_focus",
                crate::focus::rebuild_with_the_same_keys_keeps_focus as fn(),
            ),
            (
                "focus::removal_moves_focus_to_the_next_field",
                crate::focus::removal_moves_focus_to_the_next_field,
            ),
            (
                "focus::removing_the_last_field_moves_focus_to_the_first",
                crate::focus::removing_the_last_field_moves_focus_to_the_first,
            ),
            (
                "focus::reload_of_every_key_publishes_one_edge",
                crate::focus::reload_of_every_key_publishes_one_edge,
            ),
            (
                "focus::removing_every_field_releases_focus",
                crate::focus::removing_every_field_releases_focus,
            ),
            (
                "focus::the_successor_rebuilds_in_the_frame_that_moved_focus",
                crate::focus::the_successor_rebuilds_in_the_frame_that_moved_focus,
            ),
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
            ("keyboard_modifiers::altgraph_preserves_glyph_shortcuts_without_claiming_ctrl_alt_commands", crate::keyboard_modifiers::altgraph_preserves_glyph_shortcuts_without_claiming_ctrl_alt_commands),
            ("keyboard_modifiers::a_composing_key_does_not_fire_shortcuts", crate::keyboard_modifiers::a_composing_key_does_not_fire_shortcuts),
            ("keyboard_modifiers::both_shortcut_boundaries_preserve_meta_shift_and_full_glyphs", crate::keyboard_modifiers::both_shortcut_boundaries_preserve_meta_shift_and_full_glyphs),
            ("keyboard_modifiers::source_composition_leaves_traversal_and_activation_unclaimed", crate::keyboard_modifiers::source_composition_leaves_traversal_and_activation_unclaimed),
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
            ("focus::a_focus_keeps_its_subtree_when_it_stops_being_focusable", crate::focus::a_focus_keeps_its_subtree_when_it_stops_being_focusable as fn()),
            ("focus::the_first_focus_gain_does_not_rebuild_the_app_under_a_drag", crate::focus::the_first_focus_gain_does_not_rebuild_the_app_under_a_drag as fn()),
            ("focus::a_failing_frame_request_still_delivers_every_focus_edge", crate::focus::a_failing_frame_request_still_delivers_every_focus_edge as fn()),
            ("focus::a_refused_node_replacement_is_made_by_the_next_rebuild", crate::focus::a_refused_node_replacement_is_made_by_the_next_rebuild as fn()),
            ("actions::the_nearest_enabled_action_wins_and_receives_the_payload", crate::actions::the_nearest_enabled_action_wins_and_receives_the_payload as fn()),
            ("actions::callback_action_writes_through_the_key_events_cx", crate::actions::callback_action_writes_through_the_key_events_cx),
            ("actions::a_refused_write_in_a_callback_action_is_reported_not_panicked", crate::actions::a_refused_write_in_a_callback_action_is_reported_not_panicked),
            ("shortcuts::activation_tests::enter_space_and_select_activate_the_focused_control", crate::shortcuts::activation_tests::enter_space_and_select_activate_the_focused_control),
            ("shortcuts::activator_tests::a_shift_produced_character_matches_when_shift_is_ignored", crate::shortcuts::activator_tests::a_shift_produced_character_matches_when_shift_is_ignored),
            ("shortcuts::activator_tests::a_command_chord_on_a_non_latin_layout_matches_by_its_physical_key", crate::shortcuts::activator_tests::a_command_chord_on_a_non_latin_layout_matches_by_its_physical_key),
            ("shortcuts::intent_tests::a_shortcut_dispatches_its_intent_through_the_actions_chain", crate::shortcuts::intent_tests::a_shortcut_dispatches_its_intent_through_the_actions_chain),
            ("shortcuts::intent_tests::a_multiscalar_character_shortcut_reaches_the_focused_action", crate::shortcuts::intent_tests::a_multiscalar_character_shortcut_reaches_the_focused_action),
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

/// Keyboard focus through the default bindings: activation matches a click once per
/// press, the navigation keys reach their intents, the focus ring follows the input
/// modality, a retired node's request is inert, and a failing callback leaves the
/// keyboard working.
#[test]
fn keyboard_focus_contracts() {
    run_cases(
        "keyboard_focus_contracts",
        &[
            (
                "keyboard_focus::activation::keyboard_activation_matches_click",
                crate::keyboard_focus::activation::keyboard_activation_matches_click as fn(),
            ),
            (
                "keyboard_focus::list::list_keyboard_navigation",
                crate::keyboard_focus::list::list_keyboard_navigation,
            ),
            (
                "keyboard_focus::list::sliver_keyboard_navigation",
                crate::keyboard_focus::list::sliver_keyboard_navigation,
            ),
            (
                "keyboard_focus::list::grid_keyboard_navigation",
                crate::keyboard_focus::list::grid_keyboard_navigation,
            ),
            (
                "keyboard_focus::list::focused_row_is_pinned_while_scrolled_away",
                crate::keyboard_focus::list::focused_row_is_pinned_while_scrolled_away,
            ),
            (
                "keyboard_focus::list::an_empty_list_is_a_tab_stop_with_its_empty_label",
                crate::keyboard_focus::list::an_empty_list_is_a_tab_stop_with_its_empty_label,
            ),
            (
                "keyboard_focus::list::the_list_node_carries_the_list_name",
                crate::keyboard_focus::list::the_list_node_carries_the_list_name,
            ),
            (
                "keyboard_focus::activation::navigation_keys_reach_back_and_dismiss_once",
                crate::keyboard_focus::activation::navigation_keys_reach_back_and_dismiss_once,
            ),
            (
                "keyboard_focus::activation::keys_typed_in_a_field_stay_in_the_field",
                crate::keyboard_focus::activation::keys_typed_in_a_field_stay_in_the_field,
            ),
            (
                "keyboard_focus::activation::a_space_released_after_tab_activates_nothing",
                crate::keyboard_focus::activation::a_space_released_after_tab_activates_nothing,
            ),
            (
                "keyboard_focus::activation::keyboard_continues_after_callback_panic",
                crate::keyboard_focus::activation::keyboard_continues_after_callback_panic,
            ),
            (
                "keyboard_focus::modality::focus_indicator_follows_input_modality",
                crate::keyboard_focus::modality::focus_indicator_follows_input_modality,
            ),
            (
                "keyboard_focus::requests::stale_focus_request_is_inert",
                crate::keyboard_focus::requests::stale_focus_request_is_inert,
            ),
            (
                "keyboard_focus::routes::first_tab_enters_the_current_route",
                crate::keyboard_focus::routes::first_tab_enters_the_current_route,
            ),
            (
                "keyboard_focus::routes::push_moves_focus_into_the_new_route",
                crate::keyboard_focus::routes::push_moves_focus_into_the_new_route,
            ),
            (
                "keyboard_focus::routes::late_autofocus_does_not_override_the_restored_focus",
                crate::keyboard_focus::routes::late_autofocus_does_not_override_the_restored_focus,
            ),
            (
                "keyboard_focus::routes::pop_restores_the_opening_element",
                crate::keyboard_focus::routes::pop_restores_the_opening_element,
            ),
            (
                "keyboard_focus::routes::a_panicking_restored_focus_listener_still_closes_the_route",
                crate::keyboard_focus::routes::a_panicking_restored_focus_listener_still_closes_the_route,
            ),
            (
                "keyboard_focus::routes::a_panicking_observer_keeps_the_removal_failure_first",
                crate::keyboard_focus::routes::a_panicking_observer_keeps_the_removal_failure_first,
            ),
            (
                "keyboard_focus::routes::a_covered_nested_navigator_does_not_take_the_restored_focus",
                crate::keyboard_focus::routes::a_covered_nested_navigator_does_not_take_the_restored_focus,
            ),
            (
                "keyboard_focus::routes::keys_during_transition_reach_only_the_top_route",
                crate::keyboard_focus::routes::keys_during_transition_reach_only_the_top_route,
            ),
            (
                "keyboard_focus::routes::back_keys_pop_only_the_current_page",
                crate::keyboard_focus::routes::back_keys_pop_only_the_current_page,
            ),
            (
                "keyboard_focus::routes::back_keys_pop_the_enclosing_navigator_at_a_nested_root",
                crate::keyboard_focus::routes::back_keys_pop_the_enclosing_navigator_at_a_nested_root,
            ),
            (
                "keyboard_focus::routes::back_keys_leave_a_page_with_nothing_focusable",
                crate::keyboard_focus::routes::back_keys_leave_a_page_with_nothing_focusable,
            ),
            (
                "raw_button::raw_button_activates_from_the_keyboard",
                crate::raw_button::raw_button_activates_from_the_keyboard,
            ),
        ],
    );
}

/// Static text inside a `SelectionArea` publishes runs and is selected by assistive
/// technology as one document; text outside one publishes none (ADR-0214).
#[test]
fn selection_area_text_runs() {
    run_cases(
        "selection_area_text_runs",
        &[
            ("selection_area::selection_area_text_publishes_runs", crate::selection_area::selection_area_text_publishes_runs as fn()),
            ("selection_area::selection_area_accepts_assistive_selection", crate::selection_area::selection_area_accepts_assistive_selection),
            ("selection_area::selection_area_offers_its_selection_as_the_primary_selection", crate::selection_area::selection_area_offers_its_selection_as_the_primary_selection),
            ("selection_area::the_primary_selection_snaps_to_a_paragraph_rebuilt_since", crate::selection_area::the_primary_selection_snaps_to_a_paragraph_rebuilt_since),
            ("selection_area::selection_area_refuses_stale_or_foreign_positions", crate::selection_area::selection_area_refuses_stale_or_foreign_positions),
            ("selection_area::selectable_text_under_a_merging_ancestor_keeps_its_runs", crate::selection_area::selectable_text_under_a_merging_ancestor_keeps_its_runs),
            ("selection_area::static_text_outside_selection_area_publishes_no_runs", crate::selection_area::static_text_outside_selection_area_publishes_no_runs),
            ("selection_area::nested_merger_paragraph_keeps_its_run_nodes", crate::selection_area::nested_merger_paragraph_keeps_its_run_nodes),
            ("selection_area::a_control_inside_a_selection_area_under_an_outer_merger_keeps_its_node", crate::selection_area::a_control_inside_a_selection_area_under_an_outer_merger_keeps_its_node),
            ("selection_area::a_document_selection_snaps_into_its_texts_or_is_withheld", crate::selection_area::a_document_selection_snaps_into_its_texts_or_is_withheld),
            ("selection_area::links_in_a_selection_area_keep_their_own_nodes", crate::selection_area::links_in_a_selection_area_keep_their_own_nodes),
            ("selection_area::links_in_selectable_text_hold_their_own_runs", crate::selection_area::links_in_selectable_text_hold_their_own_runs),
            ("selection_area::a_wrapped_link_holds_a_run_on_each_line", crate::selection_area::a_wrapped_link_holds_a_run_on_each_line),
            ("selection_area::a_clipped_link_leaves_its_runs_with_the_text", crate::selection_area::a_clipped_link_leaves_its_runs_with_the_text),
            ("selection_area::a_link_inside_a_cluster_leaves_the_runs_with_the_text", crate::selection_area::a_link_inside_a_cluster_leaves_the_runs_with_the_text),
            ("selection_area::linked_text_under_a_merging_ancestor_keeps_its_runs", crate::selection_area::linked_text_under_a_merging_ancestor_keeps_its_runs),
            ("selection_area::linked_text_too_large_to_place_withholds_its_runs", crate::selection_area::linked_text_too_large_to_place_withholds_its_runs),
            ("selection_area::an_assistive_end_inside_a_long_grapheme_snaps_to_its_start", crate::selection_area::an_assistive_end_inside_a_long_grapheme_snaps_to_its_start),
            ("selection_area::offstage_text_is_left_out_of_the_selection", crate::selection_area::offstage_text_is_left_out_of_the_selection),
            ("selection_area::a_panicking_wake_still_reports_the_committed_selection", crate::selection_area::a_panicking_wake_still_reports_the_committed_selection),
            ("selection_area::a_document_reads_its_texts_where_they_are_laid_out", crate::selection_area::a_document_reads_its_texts_where_they_are_laid_out),
            ("selection_area::blocked_text_refuses_assistive_selection", crate::selection_area::blocked_text_refuses_assistive_selection),
            ("selection_area::a_silenced_text_is_left_out_of_the_selection", crate::selection_area::a_silenced_text_is_left_out_of_the_selection),
            ("selection_area::span_overrides_read_their_override_and_hold_their_runs", crate::selection_area::span_overrides_read_their_override_and_hold_their_runs),
            ("selection_area::the_highlight_colour_follows_the_area_style", crate::selection_area::the_highlight_colour_follows_the_area_style),
            ("selection_area::a_selection_highlights_the_parts_it_covers", crate::selection_area::a_selection_highlights_the_parts_it_covers),
            ("selection_area::an_edited_text_keeps_its_highlight_current", crate::selection_area::an_edited_text_keeps_its_highlight_current),
            ("selection_area::sort_keys_order_the_reported_text", crate::selection_area::sort_keys_order_the_reported_text),
            ("selection_area::a_line_limit_bounds_what_a_paragraph_reports", crate::selection_area::a_line_limit_bounds_what_a_paragraph_reports),
        ],
    );
}

/// A `SelectionController` reads and moves an area's selection, refuses ends the area
/// does not present, and every change reaches the listener with what made it.
#[test]
fn selection_area_controller() {
    run_cases(
        "selection_area_controller",
        &[
            ("selection_area_controller::a_controller_selects_reads_and_clears", crate::selection_area_controller::a_controller_selects_reads_and_clears as fn()),
            ("selection_area_controller::a_controller_selection_leaves_the_focus_where_it_is", crate::selection_area_controller::a_controller_selection_leaves_the_focus_where_it_is),
            ("selection_area_controller::a_controller_refuses_ends_the_area_does_not_present", crate::selection_area_controller::a_controller_refuses_ends_the_area_does_not_present),
            ("selection_area_controller::the_listener_hears_what_changed_the_selection", crate::selection_area_controller::the_listener_hears_what_changed_the_selection),
            ("selection_area_controller::a_panicking_listener_still_lets_the_controller_notify", crate::selection_area_controller::a_panicking_listener_still_lets_the_controller_notify),
            ("selection_area_controller::a_controller_refuses_a_change_during_a_build", crate::selection_area_controller::a_controller_refuses_a_change_during_a_build),
        ],
    );
}

/// `EditableText` by touch: a tap's caret handle, a long press's word and
/// its handles, dragging a handle, and what takes the handles away.
#[test]
fn editable_text_handles() {
    run_cases(
        "editable_text_handles",
        &[
            ("editable_text_handles::a_tap_shows_a_caret_handle_that_a_drag_moves", crate::editable_text_handles::a_tap_shows_a_caret_handle_that_a_drag_moves as fn()),
            ("editable_text_handles::a_caret_handle_follows_the_field_or_its_default", crate::editable_text_handles::a_caret_handle_follows_the_field_or_its_default),
            ("editable_text_handles::a_caret_handle_hides_after_its_timeout", crate::editable_text_handles::a_caret_handle_hides_after_its_timeout),
            ("editable_text_handles::a_long_press_selects_a_word_whose_handles_move_its_ends", crate::editable_text_handles::a_long_press_selects_a_word_whose_handles_move_its_ends),
            ("editable_text_handles::a_key_a_mouse_or_the_focus_leaving_hides_the_handles", crate::editable_text_handles::a_key_a_mouse_or_the_focus_leaving_hides_the_handles),
            ("editable_text_handles::a_long_press_in_an_obscured_field_selects_all_of_it", crate::editable_text_handles::a_long_press_in_an_obscured_field_selects_all_of_it),
            ("editable_text_handles::an_edit_that_keeps_the_offsets_still_hides_the_handles", crate::editable_text_handles::an_edit_that_keeps_the_offsets_still_hides_the_handles),
            ("editable_text_handles::a_finger_resting_on_the_caret_handle_keeps_it", crate::editable_text_handles::a_finger_resting_on_the_caret_handle_keeps_it),
            ("editable_text_handles::the_focus_leaving_mid_press_cancels_the_long_press", crate::editable_text_handles::the_focus_leaving_mid_press_cancels_the_long_press),
            ("editable_text_handles::a_new_caret_handle_timeout_applies_to_the_shown_handle", crate::editable_text_handles::a_new_caret_handle_timeout_applies_to_the_shown_handle),
            ("editable_text_handles::the_focus_leaving_before_a_tap_lifts_shows_no_handles", crate::editable_text_handles::the_focus_leaving_before_a_tap_lifts_shows_no_handles),
            ("editable_text_handles::a_panic_on_the_last_drag_move_still_ends_the_drag", crate::editable_text_handles::a_panic_on_the_last_drag_move_still_ends_the_drag),
            ("editable_text_handles::an_edit_during_a_long_press_ends_it", crate::editable_text_handles::an_edit_during_a_long_press_ends_it),
            ("editable_text_handles::shown_handles_follow_a_new_line_height", crate::editable_text_handles::shown_handles_follow_a_new_line_height),
        ],
    );
}

/// A `SelectionArea` selects by touch: a long press shows a handle under each end, a drag
/// of a handle moves its end, and a tap, Esc or the mouse hides them.
#[test]
fn selection_area_handles() {
    run_cases(
        "selection_area_handles",
        &[
            ("selection_area_handles::a_long_press_selects_a_word_and_shows_a_handle_under_each_end", crate::selection_area_handles::a_long_press_selects_a_word_and_shows_a_handle_under_each_end as fn()),
            ("selection_area_handles::dragging_a_handle_moves_its_end", crate::selection_area_handles::dragging_a_handle_moves_its_end),
            ("selection_area_handles::a_tap_esc_or_the_mouse_hides_the_handles", crate::selection_area_handles::a_tap_esc_or_the_mouse_hides_the_handles),
            ("selection_area_handles::handles_come_from_the_area_then_the_default_and_need_an_overlay", crate::selection_area_handles::handles_come_from_the_area_then_the_default_and_need_an_overlay),
            ("selection_area_handles::an_end_at_a_text_edge_drags_from_where_its_handle_is", crate::selection_area_handles::an_end_at_a_text_edge_drags_from_where_its_handle_is),
            ("selection_area_handles::a_failing_wake_while_the_handles_follow_still_commits", crate::selection_area_handles::a_failing_wake_while_the_handles_follow_still_commits),
            ("selection_area_handles::a_failing_primary_release_still_takes_the_handles_away", crate::selection_area_handles::a_failing_primary_release_still_takes_the_handles_away),
            ("selection_area_handles::shown_handles_rebuild_with_a_new_direction", crate::selection_area_handles::shown_handles_rebuild_with_a_new_direction),
            ("selection_area_handles::a_handle_released_past_its_last_move_ends_there", crate::selection_area_handles::a_handle_released_past_its_last_move_ends_there),
            ("selection_area_handles::shown_handles_follow_their_text_laid_out_anew", crate::selection_area_handles::shown_handles_follow_their_text_laid_out_anew),
            ("selection_area_handles::an_end_inside_a_new_cluster_sits_where_the_highlight_ends", crate::selection_area_handles::an_end_inside_a_new_cluster_sits_where_the_highlight_ends),
            ("selection_area_handles::shown_handles_follow_an_inherited_style", crate::selection_area_handles::shown_handles_follow_an_inherited_style),
            ("selection_area_handles::losing_the_focus_hides_the_handles", crate::selection_area_handles::losing_the_focus_hides_the_handles),
            ("selection_area_handles::a_failing_wake_as_the_handles_appear_still_shows_them", crate::selection_area_handles::a_failing_wake_as_the_handles_appear_still_shows_them),
            ("selection_area_handles::a_listener_taking_the_focus_away_leaves_no_handles", crate::selection_area_handles::a_listener_taking_the_focus_away_leaves_no_handles),
            ("selection_area_handles::failing_wakes_as_the_focus_leaves_are_reported_not_raised", crate::selection_area_handles::failing_wakes_as_the_focus_leaves_are_reported_not_raised),
            ("selection_area_handles::letting_go_of_one_handle_keeps_the_other_dragging", crate::selection_area_handles::letting_go_of_one_handle_keeps_the_other_dragging),
            ("selection_area_handles::shown_handles_rebuild_with_new_handles", crate::selection_area_handles::shown_handles_rebuild_with_new_handles),
            ("selection_area_handles::a_panicking_listener_still_shows_the_handles", crate::selection_area_handles::a_panicking_listener_still_shows_the_handles),
            ("selection_area_handles::a_handle_scrolled_out_of_view_takes_no_touch", crate::selection_area_handles::a_handle_scrolled_out_of_view_takes_no_touch),
            ("selection_area_handles::shown_handles_follow_a_relayout_without_a_build", crate::selection_area_handles::shown_handles_follow_a_relayout_without_a_build),
            ("selection_area_handles::a_controller_selection_hides_the_handles", crate::selection_area_handles::a_controller_selection_hides_the_handles),
            ("selection_area_handles::a_listener_selecting_elsewhere_leaves_no_handles", crate::selection_area_handles::a_listener_selecting_elsewhere_leaves_no_handles),
            ("selection_area_handles::a_long_press_extends_by_words_as_the_touch_moves", crate::selection_area_handles::a_long_press_extends_by_words_as_the_touch_moves),
            ("selection_area_handles::losing_the_focus_ends_a_long_press_extension", crate::selection_area_handles::losing_the_focus_ends_a_long_press_extension),
        ],
    );
}

/// A `SelectionArea` selects with a mouse and the keyboard, copies, clears and dims its
/// highlight on a blur, reading its texts where they are laid out.
#[test]
fn selection_area_input() {
    run_cases(
        "selection_area_input",
        &[
            ("selection_area_input::a_drag_selects_across_texts_and_a_click_clears", crate::selection_area_input::a_drag_selects_across_texts_and_a_click_clears as fn()),
            ("selection_area_input::multi_clicks_select_a_word_or_a_text_and_shift_click_extends", crate::selection_area_input::multi_clicks_select_a_word_or_a_text_and_shift_click_extends),
            ("selection_area_input::keys_extend_select_everything_copy_and_clear", crate::selection_area_input::keys_extend_select_everything_copy_and_clear),
            ("selection_area_input::a_drag_without_semantics_reads_where_the_texts_are_laid_out", crate::selection_area_input::a_drag_without_semantics_reads_where_the_texts_are_laid_out),
            ("selection_area_input::a_blur_dims_the_highlight_and_keeps_the_selection", crate::selection_area_input::a_blur_dims_the_highlight_and_keeps_the_selection),
            ("selection_area_input::a_removed_text_hands_its_end_to_the_text_beside_it", crate::selection_area_input::a_removed_text_hands_its_end_to_the_text_beside_it),
            ("selection_area_input::a_drag_ends_where_it_is_released", crate::selection_area_input::a_drag_ends_where_it_is_released),
            ("selection_area_input::a_click_targets_the_keys_which_cross_at_the_shown_end", crate::selection_area_input::a_click_targets_the_keys_which_cross_at_the_shown_end),
            ("selection_area_input::a_panicking_blur_listener_keeps_the_clicked_selection", crate::selection_area_input::a_panicking_blur_listener_keeps_the_clicked_selection),
            ("selection_area_input::line_keys_keep_their_column_and_their_line", crate::selection_area_input::line_keys_keep_their_column_and_their_line),
            ("selection_area_input::the_nearest_text_is_nearest_on_screen", crate::selection_area_input::the_nearest_text_is_nearest_on_screen),
            ("selection_area_input::esc_clears_then_reaches_the_enclosing_dismiss", crate::selection_area_input::esc_clears_then_reaches_the_enclosing_dismiss),
            ("selection_area_input::a_panicking_listener_keeps_what_the_keys_remember", crate::selection_area_input::a_panicking_listener_keeps_what_the_keys_remember),
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
            ("collection_semantics::a_lazy_list_item_reads_its_position_and_its_containers_size", crate::collection_semantics::a_lazy_list_item_reads_its_position_and_its_containers_size as fn()),
            ("collection_semantics::a_virtualized_list_publishes_its_full_size_and_true_positions", crate::collection_semantics::a_virtualized_list_publishes_its_full_size_and_true_positions),
            ("collection_semantics::an_unknown_length_publishes_no_size", crate::collection_semantics::an_unknown_length_publishes_no_size),
            ("collection_semantics::a_lazy_sliver_names_its_collection", crate::collection_semantics::a_lazy_sliver_names_its_collection),
            ("collection_semantics::separators_are_not_members_of_the_set", crate::collection_semantics::separators_are_not_members_of_the_set),
            ("collection_semantics::an_unknown_set_inside_a_known_one_withholds_the_outer_size", crate::collection_semantics::an_unknown_set_inside_a_known_one_withholds_the_outer_size),
            ("collection_semantics::a_known_set_inside_a_known_one_reads_its_own_size", crate::collection_semantics::a_known_set_inside_a_known_one_reads_its_own_size),
            ("collection_semantics::slivers_composed_into_one_set_continue_its_positions", crate::collection_semantics::slivers_composed_into_one_set_continue_its_positions),
            ("collection_semantics::two_sets_in_one_scroll_view_do_not_share_a_container", crate::collection_semantics::two_sets_in_one_scroll_view_do_not_share_a_container),
            ("collection_semantics::an_explicitly_indexed_row_keeps_its_own_position", crate::collection_semantics::an_explicitly_indexed_row_keeps_its_own_position),
            ("collection_semantics::a_table_publishes_its_shape_cells_and_sort", crate::collection_semantics::a_table_publishes_its_shape_cells_and_sort),
            ("collection_semantics::a_table_of_unknown_length_publishes_no_row_count", crate::collection_semantics::a_table_of_unknown_length_publishes_no_row_count),
            ("collection_semantics::a_tree_publishes_its_levels_and_selection_rule", crate::collection_semantics::a_tree_publishes_its_levels_and_selection_rule),
            ("collection_semantics::a_hand_built_list_declares_its_size_and_positions", crate::collection_semantics::a_hand_built_list_declares_its_size_and_positions),
            ("collection_semantics::a_position_beyond_its_set_is_not_announced", crate::collection_semantics::a_position_beyond_its_set_is_not_announced),
            ("collection_semantics::a_declared_unknown_set_inside_a_known_one_withholds_the_outer_size", crate::collection_semantics::a_declared_unknown_set_inside_a_known_one_withholds_the_outer_size),
            ("collection_semantics::a_window_with_no_member_resident_keeps_the_sets_size", crate::collection_semantics::a_window_with_no_member_resident_keeps_the_sets_size),
            ("collection_semantics::a_row_publishes_its_own_index", crate::collection_semantics::a_row_publishes_its_own_index),
            ("collection_semantics::a_row_of_several_controls_is_one_member", crate::collection_semantics::a_row_of_several_controls_is_one_member),
            ("collection_semantics::a_lone_member_sits_below_its_container", crate::collection_semantics::a_lone_member_sits_below_its_container),
            ("collection_semantics::an_explicit_position_in_a_descendant_suppresses_the_stamp", crate::collection_semantics::an_explicit_position_in_a_descendant_suppresses_the_stamp),
            ("collection_semantics::an_unpublishable_position_withholds_no_total", crate::collection_semantics::an_unpublishable_position_withholds_no_total),
            ("collection_semantics::a_set_declared_by_size_keeps_its_lone_member", crate::collection_semantics::a_set_declared_by_size_keeps_its_lone_member),
            ("collection_semantics::a_shapeless_table_bounds_its_own_cells", crate::collection_semantics::a_shapeless_table_bounds_its_own_cells),
            ("collection_semantics::an_empty_unknown_set_keeps_the_outer_size", crate::collection_semantics::an_empty_unknown_set_keeps_the_outer_size),
            ("collection_semantics::a_cell_beyond_its_table_is_not_announced", crate::collection_semantics::a_cell_beyond_its_table_is_not_announced),
            ("collection_semantics::a_size_the_adapters_cannot_convert_counts_as_unknown", crate::collection_semantics::a_size_the_adapters_cannot_convert_counts_as_unknown),
            ("semantics::sliver_fade_keeps_geometry_and_retargets_paint_and_semantics", crate::semantics::sliver_fade_keeps_geometry_and_retargets_paint_and_semantics),
            ("semantics::sliver_annotations_update_and_exclude_without_adapting_geometry", crate::semantics::sliver_annotations_update_and_exclude_without_adapting_geometry),
            ("semantics::lazy_collection_totals_reach_the_real_consumer_after_incremental_updates", crate::semantics::lazy_collection_totals_reach_the_real_consumer_after_incremental_updates),
            ("semantics::collection_consumer_preserves_unknown_and_explicit_nested_sets", crate::semantics::collection_consumer_preserves_unknown_and_explicit_nested_sets),
            ("semantics::collection_consumer_uses_member_totals_for_separated_and_composed_delegates", crate::semantics::collection_consumer_uses_member_totals_for_separated_and_composed_delegates),
            ("semantics::rich_text_accessible_labels_follow_span_overrides_and_replacement", crate::semantics::rich_text_accessible_labels_follow_span_overrides_and_replacement),
            ("semantics::a_semantics_barrier_crosses_proxies_but_stops_at_its_boundary", crate::semantics::a_semantics_barrier_crosses_proxies_but_stops_at_its_boundary),
            ("semantics::semantics_sort_keys_reorder_published_siblings_after_local_updates", crate::semantics::semantics_sort_keys_reorder_published_siblings_after_local_updates),
            ("semantics::flow_repaint_and_delegate_replacement_update_accessible_children", crate::semantics::flow_repaint_and_delegate_replacement_update_accessible_children),
            ("semantics::custom_painter_regions_follow_semantics_only_updates_and_layout_size", crate::semantics::custom_painter_regions_follow_semantics_only_updates_and_layout_size),
            ("semantics::a_fields_runs_keep_their_identities_until_their_text_changes", crate::semantics::a_fields_runs_keep_their_identities_until_their_text_changes),
            ("semantics::retained_visibility_hides_child_semantics_by_default", crate::semantics::retained_visibility_hides_child_semantics_by_default as fn()),
            ("semantics::retained_visibility_updates_semantics_without_changing_layout", crate::semantics::retained_visibility_updates_semantics_without_changing_layout as fn()),
            ("semantics::queued_directional_actions_and_numeric_values_reach_the_frame_producer", crate::semantics::queued_directional_actions_and_numeric_values_reach_the_frame_producer as fn()),
            ("semantics::numeric_range_admission_and_owner_payload_validation", crate::semantics::numeric_range_admission_and_owner_payload_validation as fn()),
            ("semantics::a_set_value_request_without_its_text_is_refused_rather_than_emptied", crate::semantics::a_set_value_request_without_its_text_is_refused_rather_than_emptied as fn()),
            ("semantics::a_tap_handler_round_trips_from_a_platform_click_to_the_callback", crate::semantics::a_tap_handler_round_trips_from_a_platform_click_to_the_callback),
            ("semantics::the_assistive_cursor_entering_and_leaving_a_subtree_reaches_its_handler", crate::semantics::the_assistive_cursor_entering_and_leaving_a_subtree_reaches_its_handler),
            ("semantics::controls_publish_their_identifier_on_the_node_they_form", crate::semantics::controls_publish_their_identifier_on_the_node_they_form),
            ("semantics::authored_properties_and_relations_reach_the_published_node", crate::semantics::authored_properties_and_relations_reach_the_published_node),
            ("semantics::a_diagnostic_is_reported_outside_the_pipeline_checkout", crate::semantics::a_diagnostic_is_reported_outside_the_pipeline_checkout),
            ("semantics::merged_custom_actions_each_reach_their_own_handler", crate::semantics::merged_custom_actions_each_reach_their_own_handler),
            ("semantics::a_repeated_custom_action_label_replaces_the_action", crate::semantics::a_repeated_custom_action_label_replaces_the_action),
            ("semantics::a_gated_custom_action_follows_its_gate_without_a_rebuild", crate::semantics::a_gated_custom_action_follows_its_gate_without_a_rebuild),
            ("semantics::assistive_scroll_actions_move_a_scrollable", crate::semantics::assistive_scroll_actions_move_a_scrollable),
            ("semantics::a_horizontal_item_scroll_moves_one_character", crate::semantics::a_horizontal_item_scroll_moves_one_character),
            ("semantics::merge_semantics_collapses_its_descendants_in_the_a11y_tree", crate::semantics::merge_semantics_collapses_its_descendants_in_the_a11y_tree),
            ("semantics::published_bounds_are_physical_and_follow_the_scale_factor", crate::semantics::published_bounds_are_physical_and_follow_the_scale_factor),
            ("semantics::animated_slide_publishes_bounds_at_its_current_sample", crate::semantics::animated_slide_publishes_bounds_at_its_current_sample),
            ("semantics::animated_scale_and_rotation_publish_clipped_bounds", crate::semantics::animated_scale_and_rotation_publish_clipped_bounds),
            ("semantics::unrepresentable_projected_bounds_are_withdrawn_and_recover", crate::semantics::unrepresentable_projected_bounds_are_withdrawn_and_recover),
            ("semantics::a_covered_retained_form_stays_absent_after_a_late_controller_update", crate::semantics::a_covered_retained_form_stays_absent_after_a_late_controller_update),
            ("semantics::rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one", crate::semantics::rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one),
            ("semantics::unmounting_a_node_releases_its_action_table", crate::semantics::unmounting_a_node_releases_its_action_table),
            ("semantics::a_detached_mount_advertises_no_actions", crate::semantics::a_detached_mount_advertises_no_actions),
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
            ("scroll::replacing_pending_scroll_motion_delivers_and_recovers_after_user_failure", crate::scroll::replacing_pending_scroll_motion_delivers_and_recovers_after_user_failure),
            ("scroll::retiring_scroll_curves_preserves_newer_reentrant_commands", crate::scroll::retiring_scroll_curves_preserves_newer_reentrant_commands),
            ("scroll::stopping_scroll_motion_preserves_commands_admitted_by_activity_observers", crate::scroll::stopping_scroll_motion_preserves_commands_admitted_by_activity_observers),
            ("scroll::scrollable_accessibility_ranges_follow_the_actual_axis", crate::scroll::scrollable_accessibility_ranges_follow_the_actual_axis),
            ("scroll::scroll_offsets_reach_platforms_from_the_top_or_left_edge", crate::scroll::scroll_offsets_reach_platforms_from_the_top_or_left_edge),
            ("scroll::hover_holds_while_a_touch_scroll_is_latched", crate::scroll::hover_holds_while_a_touch_scroll_is_latched),
            ("scroll::constrained_sliver_updates_layout_without_replacing_the_child", crate::scroll::constrained_sliver_updates_layout_without_replacing_the_child),
            ("scroll::cross_axis_sliver_tracks_reconcile_weights_and_publish_child_semantics", crate::scroll::cross_axis_sliver_tracks_reconcile_weights_and_publish_child_semantics),
            ("scroll::terminal_scroll_motion_uses_the_admitted_fling_profile", crate::scroll::terminal_scroll_motion_uses_the_admitted_fling_profile as fn()),
            ("scroll::nested_scroll_sequence_keeps_its_first_consumptive_target", crate::scroll::nested_scroll_sequence_keeps_its_first_consumptive_target as fn()),
            ("scroll::scroll_latch_survives_focal_motion_and_releases_on_cancel", crate::scroll::scroll_latch_survives_focal_motion_and_releases_on_cancel),
            ("scroll::phase_less_scroll_latch_expires_on_owner_clock_inactivity", crate::scroll::phase_less_scroll_latch_expires_on_owner_clock_inactivity),
            ("scroll::scroll_latches_are_source_local_and_device_removal_releases", crate::scroll::scroll_latches_are_source_local_and_device_removal_releases),
            ("scroll::a_remaining_touch_continues_scroll_without_an_intermediate_fling", crate::scroll::a_remaining_touch_continues_scroll_without_an_intermediate_fling as fn()),
            ("scroll::notched_wheel_accumulates_distance_and_eases_out_in_150ms", crate::scroll::notched_wheel_accumulates_distance_and_eases_out_in_150ms),
            ("scroll::a_click_during_wheel_easing_taps_and_a_press_on_a_fling_catches_it", crate::scroll::a_click_during_wheel_easing_taps_and_a_press_on_a_fling_catches_it),
            ("scroll::resting_fingers_catch_a_fling_and_leave_wheel_easing", crate::scroll::resting_fingers_catch_a_fling_and_leave_wheel_easing),
            ("scroll::precise_and_unknown_wheels_interrupt_synthetic_motion_once", crate::scroll::precise_and_unknown_wheels_interrupt_synthetic_motion_once),
            ("scroll::replacing_or_unmounting_a_scrollable_retires_its_notched_motion", crate::scroll::replacing_or_unmounting_a_scrollable_retires_its_notched_motion),
            ("scroll::dragging_interrupts_notched_motion_and_windows_progress_independently", crate::scroll::dragging_interrupts_notched_motion_and_windows_progress_independently),
            ("scroll::scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded", crate::scroll::scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded as fn()),
            ("scroll::dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick", crate::scroll::dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick),
            ("scroll::cancelling_an_in_range_scroll_ends_activity_without_coasting", crate::scroll::cancelling_an_in_range_scroll_ends_activity_without_coasting as fn()),
            ("scroll::replacing_vsync_releases_old_seats_and_drives_fresh_contacts", crate::scroll::replacing_vsync_releases_old_seats_and_drives_fresh_contacts),
            ("scroll::controller_mutation_during_build_is_refused", crate::scroll::controller_mutation_during_build_is_refused),
            ("scroll::replacing_a_scroll_position_cancels_its_contact_and_recovers", crate::scroll::replacing_a_scroll_position_cancels_its_contact_and_recovers),
            ("scroll::a_repeated_frame_does_not_cancel_viewer_inertia", crate::scroll::a_repeated_frame_does_not_cancel_viewer_inertia),
            ("scroll::cancelling_bouncing_overscroll_settles_without_release_velocity", crate::scroll::cancelling_bouncing_overscroll_settles_without_release_velocity as fn()),
            ("scroll::bouncing_lower_edge_preserves_outward_direction", crate::scroll::bouncing_lower_edge_preserves_outward_direction as fn()),
            ("scroll::bouncing_upper_edge_preserves_outward_direction", crate::scroll::bouncing_upper_edge_preserves_outward_direction as fn()),
            ("scroll::bouncing_stationary_input_preserves_overscroll", crate::scroll::bouncing_stationary_input_preserves_overscroll as fn()),
            ("scroll::bouncing_inward_motion_and_crossing_respect_the_new_edge", crate::scroll::bouncing_inward_motion_and_crossing_respect_the_new_edge as fn()),
            ("scroll::bouncing_resistance_stiffens_toward_the_viewport_length", crate::scroll::bouncing_resistance_stiffens_toward_the_viewport_length as fn()),
            ("scroll::unordered_extents_are_refused_and_bound_nothing", crate::scroll::unordered_extents_are_refused_and_bound_nothing as fn()),
            ("scroll::clamping_release_past_an_edge_returns_to_it", crate::scroll::clamping_release_past_an_edge_returns_to_it as fn()),
            ("scroll::a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook", crate::scroll::a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook as fn()),
            ("scroll::a_same_position_scrollable_rebuild_preserves_motion", crate::scroll::a_same_position_scrollable_rebuild_preserves_motion as fn()),
            ("scroll::retiring_one_scrollable_preserves_a_later_owners_jump_hook", crate::scroll::retiring_one_scrollable_preserves_a_later_owners_jump_hook as fn()),
            (
                "scroll::shift_wheel_scrolls_the_horizontal_axis",
                crate::scroll::shift_wheel_scrolls_the_horizontal_axis as fn(),
            ),
            (
                "scroll::wheel_policy_resolves_authored_axes_and_provider_replacement",
                crate::scroll::wheel_policy_resolves_authored_axes_and_provider_replacement as fn(),
            ),
            (
                "scroll::wheel_distances_refuse_overflow_and_the_next_packet_recovers",
                crate::scroll::wheel_distances_refuse_overflow_and_the_next_packet_recovers as fn(),
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
                "scroll::a_command_queued_before_mount_is_serviced_outside_build",
                crate::scroll::a_command_queued_before_mount_is_serviced_outside_build,
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
                "scroll::scrollable_retarget_preserves_the_position_velocity",
                crate::scroll::scrollable_retarget_preserves_the_position_velocity,
            ),
            (
                "scroll::scrollable_retarget_at_the_current_position_brakes_continuously",
                crate::scroll::scrollable_retarget_at_the_current_position_brakes_continuously,
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
                "scroll::a_fresh_physics_each_build_keeps_the_fling_and_its_bounce",
                crate::scroll::a_fresh_physics_each_build_keeps_the_fling_and_its_bounce,
            ),
            (
                "scroll::clamping_fling_into_the_edge_rests_on_it",
                crate::scroll::clamping_fling_into_the_edge_rests_on_it,
            ),
            (
                "scroll::a_fling_rests_on_the_end_the_measured_rows_give",
                crate::scroll::a_fling_rests_on_the_end_the_measured_rows_give,
            ),
            (
                "scroll::a_snap_rests_on_its_target_when_the_end_clamps_the_offset",
                crate::scroll::a_snap_rests_on_its_target_when_the_end_clamps_the_offset,
            ),
            (
                "scroll::a_rebuilt_run_keeps_the_mode_its_release_started_in",
                crate::scroll::a_rebuilt_run_keeps_the_mode_its_release_started_in,
            ),
            (
                "scroll::a_rebuilt_run_keeps_the_physics_its_release_started_with",
                crate::scroll::a_rebuilt_run_keeps_the_physics_its_release_started_with,
            ),
            (
                "scroll::a_rebuild_taken_over_by_its_physics_stops_the_obsolete_run",
                crate::scroll::a_rebuild_taken_over_by_its_physics_stops_the_obsolete_run,
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
            ("lazy_list::a_pinned_row_stays_announced_until_its_pin_is_released", crate::lazy_list::a_pinned_row_stays_announced_until_its_pin_is_released as fn()),
            ("lazy_list::lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame", crate::lazy_list::lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame),
            ("lazy_list::lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state", crate::lazy_list::lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state),
            ("lazy_list::lazy_list_view_builder_pathological_extents_defer_instead_of_panicking", crate::lazy_list::lazy_list_view_builder_pathological_extents_defer_instead_of_panicking),
            ("lazy_list::lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band", crate::lazy_list::lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band),
            ("lazy_list::replacing_nested_lazy_hosts_disposes_every_resident_once", crate::lazy_list::replacing_nested_lazy_hosts_disposes_every_resident_once),
            ("lazy_grid::lazy_grid_view_builder_places_tiles_at_oracle_positions", crate::lazy_grid::lazy_grid_view_builder_places_tiles_at_oracle_positions),
            ("sliver_persistent_header::a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends", crate::sliver_persistent_header::a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends),
            ("sliver_persistent_header::an_active_floating_snap_migrates_and_unmounts", crate::sliver_persistent_header::an_active_floating_snap_migrates_and_unmounts),
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
            ("back_gesture::mounted_back_swipe_reads_retained_admission_settings", crate::back_gesture::mounted_back_swipe_reads_retained_admission_settings),
            ("back_gesture::replacing_authored_back_swipe_policy_cancels_the_outgoing_contact", crate::back_gesture::replacing_authored_back_swipe_policy_cancels_the_outgoing_contact),
            ("back_gesture::mounted_back_swipe_settle_uses_the_admitted_fling_bound", crate::back_gesture::mounted_back_swipe_settle_uses_the_admitted_fling_bound),
            ("navigator::local_history::an_entry_pops_before_the_route_and_observers_stay_silent", crate::navigator::local_history::an_entry_pops_before_the_route_and_observers_stay_silent as fn()),
            ("navigator::local_history::local_history_removal_writes_after_the_flush", crate::navigator::local_history::local_history_removal_writes_after_the_flush),
            ("navigator::local_history::a_panicking_removal_does_not_lose_the_next", crate::navigator::local_history::a_panicking_removal_does_not_lose_the_next),
            ("navigator::local_history::an_unmounted_navigators_owed_removal_is_severed", crate::navigator::local_history::an_unmounted_navigators_owed_removal_is_severed),
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
            ("page_route::page_route_paints_its_separate_entrance_and_exit_durations", crate::page_route::page_route_paints_its_separate_entrance_and_exit_durations),
            ("page_route::secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping", crate::page_route::secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping),
            ("page_transitions::a_page_route_adopts_the_ambient_builder_and_its_duration", crate::page_transitions::a_page_route_adopts_the_ambient_builder_and_its_duration),
            ("page_transitions::a_route_with_no_hook_in_scope_keeps_the_jump_cut", crate::page_transitions::a_route_with_no_hook_in_scope_keeps_the_jump_cut),
            ("page_transitions::an_explicit_transition_or_duration_wins_over_the_hook", crate::page_transitions::an_explicit_transition_or_duration_wins_over_the_hook),
            ("page_transitions::each_route_kind_adopts_the_nearest_builder_of_its_kind", crate::page_transitions::each_route_kind_adopts_the_nearest_builder_of_its_kind),
            ("page_transitions::a_replaced_hook_does_not_swap_a_running_transition", crate::page_transitions::a_replaced_hook_does_not_swap_a_running_transition),
            ("page_transitions::an_adopted_transition_settles_on_the_first_reduced_motion_frame", crate::page_transitions::an_adopted_transition_settles_on_the_first_reduced_motion_frame),
            ("page_transitions::a_route_pushed_before_mount_adopts_the_hook_at_mount", crate::page_transitions::a_route_pushed_before_mount_adopts_the_hook_at_mount),
            ("page_transitions::a_route_pushed_while_durations_are_read_adopts_the_new_hooks", crate::page_transitions::a_route_pushed_while_durations_are_read_adopts_the_new_hooks),
            ("transition_route::push_transition_parks_the_entry_in_pushing_until_the_controller_completes", crate::transition_route::push_transition_parks_the_entry_in_pushing_until_the_controller_completes),
            ("transition_route::scope_replacement_moves_an_existing_route_without_restarting_it", crate::transition_route::scope_replacement_moves_an_existing_route_without_restarting_it),
            ("transition_route::hopping_route_dropped_without_dispose_frees_proxy", crate::transition_route::hopping_route_dropped_without_dispose_frees_proxy),
            ("transition_route::replacement_retains_near_zero_secondary_progress_until_crossing", crate::transition_route::replacement_retains_near_zero_secondary_progress_until_crossing),
        ],
    );
}

/// Navigation torn down mid-way: a window closing during a transition and a hero
/// flight, an observer removed while its flight is airborne, and the last
/// navigator handle dropped after a push. Each releases what it owned.
#[test]
fn navigation_teardown() {
    run_cases(
        "navigation_teardown",
        &[
            ("navigator::closing_mid_transition_releases_the_route_overlay_and_hero", crate::navigator::closing_mid_transition_releases_the_route_overlay_and_hero as fn()),
            ("hero_flight::removing_the_observer_mid_flight_ends_the_flight", crate::hero_flight::removing_the_observer_mid_flight_ends_the_flight),
            ("modal_route::dropping_the_last_navigator_handle_after_push_clears_the_modal_registry", crate::modal_route::dropping_the_last_navigator_handle_after_push_clears_the_modal_registry),
        ],
    );
}

/// The `Router`, `#[derive(Routable)]` round trip and the `WidgetsApp` shell.
#[test]
fn router_and_widgets_app() {
    run_cases(
        "router_and_widgets_app",
        &[
            ("widgets_app::locale_override_removal_uses_the_nearest_current_preferences", crate::widgets_app::locale_override_removal_uses_the_nearest_current_preferences as fn()),
            ("router::popup_routes_are_admitted_and_leave_the_location_alone", crate::router::popup_routes_are_admitted_and_leave_the_location_alone as fn()),
            ("router::a_named_page_is_announced_when_pushed_and_when_uncovered", crate::router::a_named_page_is_announced_when_pushed_and_when_uncovered),
            ("router::router_never_pops_its_last_page", crate::router::router_never_pops_its_last_page),
            ("router::router_opens_at_a_location_with_its_back_stack", crate::router::router_opens_at_a_location_with_its_back_stack),
            ("router::router_push_commits_before_a_reentrant_observer_pop", crate::router::router_push_commits_before_a_reentrant_observer_pop),
            ("router::router_replace_commits_before_a_reentrant_observer_pop", crate::router::router_replace_commits_before_a_reentrant_observer_pop),
            ("router::router_go_commits_before_a_reentrant_observer_pop", crate::router::router_go_commits_before_a_reentrant_observer_pop),
            ("router::a_deep_link_adds_its_lower_page_in_the_frame_its_top_settles", crate::router::a_deep_link_adds_its_lower_page_in_the_frame_its_top_settles),
            ("router::router_go_recomputes_its_prefix_after_popup_observer_navigation", crate::router::router_go_recomputes_its_prefix_after_popup_observer_navigation),
            ("router::router_push_preserves_its_commit_after_an_observer_panic", crate::router::router_push_preserves_its_commit_after_an_observer_panic),
            ("router::router_go_preserves_its_commit_after_an_observer_panic", crate::router::router_go_preserves_its_commit_after_an_observer_panic),
            ("router::removing_a_router_value_retires_it_after_releasing_the_stack_borrow", crate::router::removing_a_router_value_retires_it_after_releasing_the_stack_borrow),
            ("router::empty_stack_is_refused", crate::router::empty_stack_is_refused),
            ("router::from_stack_restores_back_order", crate::router::from_stack_restores_back_order),
            ("router::stack_reads_every_committed_edit", crate::router::stack_reads_every_committed_edit),
            ("router::stack_listeners_hear_each_committed_change_once", crate::router::stack_listeners_hear_each_committed_change_once),
            ("router::a_stack_listener_may_navigate", crate::router::a_stack_listener_may_navigate),
            ("router::an_unmounted_router_lets_go_of_its_listeners", crate::router::an_unmounted_router_lets_go_of_its_listeners),
            ("router::a_frame_hook_panic_while_queueing_leaves_later_changes_told", crate::router::a_frame_hook_panic_while_queueing_leaves_later_changes_told),
            ("router::from_stack_opens_without_a_transition", crate::router::from_stack_opens_without_a_transition),
            ("router::stack_leaves_out_popups", crate::router::stack_leaves_out_popups),
            ("router::debug_prints_the_whole_stack", crate::router::debug_prints_the_whole_stack),
            ("router::a_route_clone_may_drive_its_router", crate::router::a_route_clone_may_drive_its_router),
            ("router::a_route_eq_may_drive_its_router_during_go", crate::router::a_route_eq_may_drive_its_router_during_go),
            ("widgets_app::builder_only_app_receives_no_routing_and_supplies_the_subtree", crate::widgets_app::builder_only_app_receives_no_routing_and_supplies_the_subtree),
            ("widgets_app::home_is_seeded_once_as_the_root_route", crate::widgets_app::home_is_seeded_once_as_the_root_route),
            ("widgets_app::observers_attach_at_mount_and_see_the_home_route", crate::widgets_app::observers_attach_at_mount_and_see_the_home_route),
            ("widgets_app_router::a_pushed_route_focuses_its_first_control_in_every_mount", crate::widgets_app_router::a_pushed_route_focuses_its_first_control_in_every_mount),
            ("widgets_app_router::switching_widgets_app_from_home_to_router_releases_the_navigator", crate::widgets_app_router::switching_widgets_app_from_home_to_router_releases_the_navigator),
            ("widgets_app_router::widgets_app_router_navigates_by_handle_and_the_url_follows", crate::widgets_app_router::widgets_app_router_navigates_by_handle_and_the_url_follows),
        ],
    );
}

/// An adopted page-transition builder may reach its navigator; each row runs
/// in a child process, because the failure mode is a deadlock.
#[test]
fn page_transition_builders_reach_their_navigator() {
    crate::page_transitions::an_adopted_builder_may_query_its_navigator();
}

/// Retiring an ambient hook whose builders panic in `Drop` keeps the first
/// panic; the row runs in a child process because the failure mode is an abort.
#[test]
fn page_transition_hooks_retire_panicking_builders() {
    crate::page_transitions::retiring_hooks_whose_builders_both_panic_does_not_abort();
}

/// Hero flights: state survival, curves, diversion by a pop, gesture release.
#[test]
fn hero_flights() {
    run_cases(
        "hero_flights",
        &[
            ("hero::changing_a_mounted_hero_tag_moves_its_registration", crate::hero::changing_a_mounted_hero_tag_moves_its_registration as fn()),
            ("hero::registry_key_callbacks_can_read_their_registry", crate::hero::registry_key_callbacks_can_read_their_registry),
            ("hero::retagging_to_a_duplicate_preserves_the_existing_winner", crate::hero::retagging_to_a_duplicate_preserves_the_existing_winner as fn()),
            ("hero::replacing_a_hero_scope_moves_the_existing_hero", crate::hero::replacing_a_hero_scope_moves_the_existing_hero),
            ("hero::reparenting_a_hero_moves_registration_without_recreating_it", crate::hero::reparenting_a_hero_moves_registration_without_recreating_it),
            ("hero::reparenting_an_unscoped_hero_adopts_its_first_route", crate::hero::reparenting_an_unscoped_hero_adopts_its_first_route),
            ("hero::reparenting_refreshes_a_build_time_miss_but_prunes_an_unread_one", crate::hero::reparenting_refreshes_a_build_time_miss_but_prunes_an_unread_one),
            ("hero::a_retained_hero_handle_does_not_keep_its_presentation_alive", crate::hero::a_retained_hero_handle_does_not_keep_its_presentation_alive as fn()),
            ("hero::unmount_releases_hero_configuration_despite_a_retained_handle", crate::hero::unmount_releases_hero_configuration_despite_a_retained_handle),
            ("hero::a_hero_child_keeps_its_state_across_a_flight_without_a_global_key", crate::hero::a_hero_child_keeps_its_state_across_a_flight_without_a_global_key as fn()),
            ("hero_flight::a_push_eases_on_the_destination_hero_curve", crate::hero_flight::a_push_eases_on_the_destination_hero_curve),
            ("hero_flight::a_push_flight_interrupted_by_a_pop_diverts_in_place", crate::hero_flight::a_push_flight_interrupted_by_a_pop_diverts_in_place),
            ("hero_flight::a_nonlinear_hero_pop_retraces_the_airborne_push", crate::hero_flight::a_nonlinear_hero_pop_retraces_the_airborne_push),
            ("hero_flight::cancelling_a_hero_mapping_does_not_refreeze_the_previous_page", crate::hero_flight::cancelling_a_hero_mapping_does_not_refreeze_the_previous_page),
            ("hero_flight::a_shrinking_flight_with_overshoot_keeps_a_non_negative_size", crate::hero_flight::a_shrinking_flight_with_overshoot_keeps_a_non_negative_size),
            ("hero_flight::hero_geometry_samples_preserve_the_last_finite_shuttle", crate::hero_flight::hero_geometry_samples_preserve_the_last_finite_shuttle),
            ("hero_flight::hero_geometry_reentry_preserves_the_newer_authority", crate::hero_flight::hero_geometry_reentry_preserves_the_newer_authority),
            ("hero_flight::disappearing_hero_destination_keeps_a_finite_fade", crate::hero_flight::disappearing_hero_destination_keeps_a_finite_fade),
            ("hero_gesture::complete_release_pops_to_the_destination_route_and_the_flight_lands", crate::hero_gesture::complete_release_pops_to_the_destination_route_and_the_flight_lands),
            ("hero_gesture::an_excluding_gesture_restores_pending_programmatic_placeholders", crate::hero_gesture::an_excluding_gesture_restores_pending_programmatic_placeholders),
            ("hero_public::a_hero_push_flight_runs_and_settles", crate::hero_public::a_hero_push_flight_runs_and_settles),
            ("hero_waypoint_tracks::authored_cubic_hero_geometry_is_painted_and_retraced", crate::hero_waypoint_tracks::authored_cubic_hero_geometry_is_painted_and_retraced),
            ("hero_waypoint_tracks::authored_piecewise_hero_geometry_is_painted_and_retraced", crate::hero_waypoint_tracks::authored_piecewise_hero_geometry_is_painted_and_retraced),
            ("hero_waypoint_tracks::authored_spring_hero_geometry_is_painted_and_retraced", crate::hero_waypoint_tracks::authored_spring_hero_geometry_is_painted_and_retraced),
            ("hero_public::replacing_the_hero_observer_inside_its_builder_cancels_the_flight", crate::hero_public::replacing_the_hero_observer_inside_its_builder_cancels_the_flight),
            ("hero_public::replacing_the_hero_observer_cancels_queued_measurement", crate::hero_public::replacing_the_hero_observer_cancels_queued_measurement),
            ("hero_public::a_failed_hero_builder_restores_its_child_and_allows_a_fresh_flight", crate::hero_public::a_failed_hero_builder_restores_its_child_and_allows_a_fresh_flight),
            ("hero_public::hero_builder_cancellation_and_failure_account_for_the_matched_tail", crate::hero_public::hero_builder_cancellation_and_failure_account_for_the_matched_tail),
        ],
    );
}

/// Explicit and implicit animation widgets and `Visibility`.
#[test]
fn animation_and_visibility() {
    run_cases(
        "animation_and_visibility",
        &[
            ("frame_partitions::mounted_motion_reaches_equal_scenes_across_frame_partitions", crate::frame_partitions::mounted_motion_reaches_equal_scenes_across_frame_partitions as fn()),
            ("dismissible::dismissal_release_uses_its_captured_fling_profile", crate::dismissible::dismissal_release_uses_its_captured_fling_profile as fn()),
            ("dismissible::vertical_dismissal_release_uses_its_captured_fling_profile", crate::dismissible::vertical_dismissal_release_uses_its_captured_fling_profile),
            ("dismissible::cancelling_a_fully_slid_card_restores_it_without_dismissal", crate::dismissible::cancelling_a_fully_slid_card_restores_it_without_dismissal as fn()),
            ("dismissible::a_cancelled_horizontal_dismiss_restores_the_card", crate::dismissible::a_cancelled_horizontal_dismiss_restores_the_card as fn()),
            ("dismissible::a_cancelled_vertical_dismiss_restores_the_card", crate::dismissible::a_cancelled_vertical_dismiss_restores_the_card as fn()),
            ("dismissible::a_vertical_dismissible_leaves_a_horizontal_drag_to_its_ancestor", crate::dismissible::a_vertical_dismissible_leaves_a_horizontal_drag_to_its_ancestor),
            ("dismissible::a_dismissible_release_keeps_finger_speed_on_any_width", crate::dismissible::a_dismissible_release_keeps_finger_speed_on_any_width as fn()),
            ("dismissible::dismissible_slides_without_rebuilding_per_frame", crate::dismissible::dismissible_slides_without_rebuilding_per_frame as fn()),
            ("dismissible::dismissal_updates_follow_owner_lifetime", crate::dismissible::dismissal_updates_follow_owner_lifetime as fn()),
            ("dismissible::a_dismissible_collapses_its_laid_out_size", crate::dismissible::a_dismissible_collapses_its_laid_out_size as fn()),
            ("dismissible::resize_delivery_keeps_accepted_ticks_before_completion", crate::dismissible::resize_delivery_keeps_accepted_ticks_before_completion as fn()),
            ("dismissible::a_dismissible_goes_out_only_where_its_direction_and_threshold_allow", crate::dismissible::a_dismissible_goes_out_only_where_its_direction_and_threshold_allow),
            ("dismissible::a_direction_contains_the_sides_it_allows", crate::dismissible::a_direction_contains_the_sides_it_allows),
            ("dismissible::a_row_scrolled_out_of_a_lazy_list_mid_slide_keeps_its_slide", crate::dismissible::a_row_scrolled_out_of_a_lazy_list_mid_slide_keeps_its_slide),
            ("animated_size::animated_size_interpolates_to_a_new_child_size_over_frames", crate::animated_size::animated_size_interpolates_to_a_new_child_size_over_frames as fn()),
            ("animated_size::an_active_size_run_migrates_between_registries", crate::animated_size::an_active_size_run_migrates_between_registries),
            ("animated_size::animated_size_defaults_to_a_critically_damped_spring", crate::animated_size::animated_size_defaults_to_a_critically_damped_spring),
            ("animated_size::animated_size_completion_writes_a_signal_after_build", crate::animated_size::animated_size_completion_writes_a_signal_after_build),
            ("animated_size::size_completion_keeps_owner_event_order_and_lifetime", crate::animated_size::size_completion_keeps_owner_event_order_and_lifetime),
            ("animated_size::size_retarget_preserves_the_laid_out_velocity", crate::animated_size::size_retarget_preserves_the_laid_out_velocity),
            ("animated_size::an_underdamped_spring_overshoots_the_new_size_and_settles", crate::animated_size::an_underdamped_spring_overshoots_the_new_size_and_settles),
            ("animated_size::a_slow_spring_lands_on_the_new_size_under_reduced_motion", crate::animated_size::a_slow_spring_lands_on_the_new_size_under_reduced_motion),
            ("animated_size::a_motion_change_mid_resize_keeps_the_size_shown", crate::animated_size::a_motion_change_mid_resize_keeps_the_size_shown),
            ("animated_size::a_spring_finishing_before_layout_completes_once", crate::animated_size::a_spring_finishing_before_layout_completes_once),
            ("animated_size::a_long_spring_resize_lands_without_a_jump", crate::animated_size::a_long_spring_resize_lands_without_a_jump),
            ("animated_size::a_curve_resize_follows_the_motion_segment", crate::animated_size::a_curve_resize_follows_the_motion_segment),
            ("animated_size::a_size_and_motion_change_together_complete_one_resize", crate::animated_size::a_size_and_motion_change_together_complete_one_resize),
            ("implicit_animations::switching_entries_migrate_their_incoming_and_outgoing_runs", crate::implicit_animations::switching_entries_migrate_their_incoming_and_outgoing_runs),
            ("implicit_animations::switching_entries_finish_migration_before_a_wake_failure", crate::implicit_animations::switching_entries_finish_migration_before_a_wake_failure),
            ("implicit_animations::outgoing_switcher_retirement_preserves_independent_captures", crate::implicit_animations::outgoing_switcher_retirement_preserves_independent_captures),
            ("scroll::an_active_fling_migrates_between_registries", crate::scroll::an_active_fling_migrates_between_registries),
            ("scroll::a_notched_wheel_run_migrates_with_its_accepted_destination", crate::scroll::a_notched_wheel_run_migrates_with_its_accepted_destination),
            ("dismissible::a_returning_dismissible_migrates_between_registries", crate::dismissible::a_returning_dismissible_migrates_between_registries),
            ("dismissible::a_collapsing_dismissible_migrates_between_registries", crate::dismissible::a_collapsing_dismissible_migrates_between_registries),
            ("dismissible::a_collapsing_dismissible_survives_a_migration_wake_failure", crate::dismissible::a_collapsing_dismissible_survives_a_migration_wake_failure),
            ("implicit_animations::animated_container_interpolates_size_over_frames", crate::implicit_animations::animated_container_interpolates_size_over_frames),
            ("motion::effects_timeline_places_each_entry", crate::motion::effects_timeline_places_each_entry),
            ("motion::transition_merge_keeps_the_right_operand_per_slot", crate::motion::transition_merge_keeps_the_right_operand_per_slot),
            ("motion::a_scale_effect_keeps_its_alignment_fixed", crate::motion::a_scale_effect_keeps_its_alignment_fixed),
            ("motion::on_mount_plays_the_timeline_in_order", crate::motion::on_mount_plays_the_timeline_in_order),
            ("motion::on_change_replays_only_for_a_new_value", crate::motion::on_change_replays_only_for_a_new_value),
            ("motion::target_reverses_mid_flight_without_a_jump", crate::motion::target_reverses_mid_flight_without_a_jump),
            ("motion::staggered_effects_start_one_step_apart", crate::motion::staggered_effects_start_one_step_apart),
            ("motion::a_controller_trigger_follows_its_animation", crate::motion::a_controller_trigger_follows_its_animation),
            ("motion::reduced_motion_paints_the_end_on_the_first_frame", crate::motion::reduced_motion_paints_the_end_on_the_first_frame),
            ("motion::extreme_scales_compose_without_overflow", crate::motion::extreme_scales_compose_without_overflow),
            ("motion::adopting_a_controller_stops_the_owned_run", crate::motion::adopting_a_controller_stops_the_owned_run),
            ("motion::a_panicking_controller_release_still_hands_progress_back", crate::motion::a_panicking_controller_release_still_hands_progress_back),
            ("motion::a_switcher_plays_its_transition_on_both_sides", crate::motion::a_switcher_plays_its_transition_on_both_sides),
            ("motion::switching_back_mid_exit_reverses_from_current_progress", crate::motion::switching_back_mid_exit_reverses_from_current_progress),
            ("motion::reduced_motion_switches_at_once", crate::motion::reduced_motion_switches_at_once),
            ("motion::skeletons_share_one_band", crate::motion::skeletons_share_one_band),
            ("motion::a_shimmer_sweeps_once_per_default_period", crate::motion::a_shimmer_sweeps_once_per_default_period),
            ("motion::a_still_shimmer_paints_skeletons_flat", crate::motion::a_still_shimmer_paints_skeletons_flat),
            ("motion::a_shimmer_announces_one_loading_region", crate::motion::a_shimmer_announces_one_loading_region),
            ("motion::a_shimmer_effect_masks_the_child_mid_sweep", crate::motion::a_shimmer_effect_masks_the_child_mid_sweep),
            ("motion::a_non_finite_effect_leaves_its_siblings_applied", crate::motion::a_non_finite_effect_leaves_its_siblings_applied),
            ("motion::an_empty_transition_drives_no_frames", crate::motion::an_empty_transition_drives_no_frames),
            ("motion::a_subnormal_alignment_is_not_treated_as_centre", crate::motion::a_subnormal_alignment_is_not_treated_as_centre),
            ("motion::replacing_a_running_transition_with_none_stops_frames", crate::motion::replacing_a_running_transition_with_none_stops_frames),
            ("motion::a_scaled_alignment_offset_survives_intermediate_overflow", crate::motion::a_scaled_alignment_offset_survives_intermediate_overflow),
            ("motion::a_side_switch_at_an_unchanged_value_repaints", crate::motion::a_side_switch_at_an_unchanged_value_repaints),
            ("particles::configurations_are_checked_when_built", crate::particles::configurations_are_checked_when_built),
            ("particles::a_seeded_burst_draws_one_atlas_the_same_each_run", crate::particles::a_seeded_burst_draws_one_atlas_the_same_each_run),
            ("particles::the_clock_stops_when_empty_and_a_burst_restarts_it", crate::particles::the_clock_stops_when_empty_and_a_burst_restarts_it),
            ("particles::a_burst_is_cut_to_the_free_capacity", crate::particles::a_burst_is_cut_to_the_free_capacity),
            ("particles::a_muted_ticker_refuses_bursts", crate::particles::a_muted_ticker_refuses_bursts),
            ("particles::non_finite_particles_never_reach_the_atlas", crate::particles::non_finite_particles_never_reach_the_atlas),
            ("particles::a_paused_ticker_catches_up_in_fixed_steps", crate::particles::a_paused_ticker_catches_up_in_fixed_steps),
            ("particles::presets_never_flash", crate::particles::presets_never_flash),
            ("particles::a_controller_drives_one_emitter", crate::particles::a_controller_drives_one_emitter),
            ("particles::reduced_motion_emits_nothing", crate::particles::reduced_motion_emits_nothing),
            ("particles::particles_are_left_out_of_semantics", crate::particles::particles_are_left_out_of_semantics),
            ("animated_icon::icon_data_is_checked", crate::animated_icon::icon_data_is_checked),
            ("animated_icon::the_frame_follows_the_progress", crate::animated_icon::the_frame_follows_the_progress),
            ("animated_icon::theme_and_direction_shape_the_icon", crate::animated_icon::theme_and_direction_shape_the_icon),
            ("animated_icon::reduced_motion_shows_the_target_frame", crate::animated_icon::reduced_motion_shows_the_target_frame),
            ("animated_icon::the_label_lives_on_the_icon", crate::animated_icon::the_label_lives_on_the_icon),
            ("symbol_effect::each_effect_peaks_where_its_timing_says", crate::symbol_effect::each_effect_peaks_where_its_timing_says),
            ("symbol_effect::a_one_shot_effect_plays_per_change_and_repeat", crate::symbol_effect::a_one_shot_effect_plays_per_change_and_repeat),
            ("symbol_effect::a_repeating_effect_finishes_its_cycle_when_turned_off", crate::symbol_effect::a_repeating_effect_finishes_its_cycle_when_turned_off),
            ("symbol_effect::a_replace_cross_fades_and_only_the_new_label_is_heard", crate::symbol_effect::a_replace_cross_fades_and_only_the_new_label_is_heard),
            ("symbol_effect::a_replace_sizes_to_the_new_icon_only", crate::symbol_effect::a_replace_sizes_to_the_new_icon_only),
            ("symbol_effect::transitions_play_when_first_applied", crate::symbol_effect::transitions_play_when_first_applied),
            ("symbol_effect::reduced_motion_keeps_the_icon_still_or_at_its_end", crate::symbol_effect::reduced_motion_keeps_the_icon_still_or_at_its_end),
            ("symbol_effect::a_speed_is_positive_and_finite_and_scales_the_timing", crate::symbol_effect::a_speed_is_positive_and_finite_and_scales_the_timing),
            ("symbol_effect::a_path_effect_draws_the_paths_on_and_off", crate::symbol_effect::a_path_effect_draws_the_paths_on_and_off),
            ("symbol_effect::a_new_speed_retimes_a_running_repeat", crate::symbol_effect::a_new_speed_retimes_a_running_repeat),
            ("animated_list::a_removed_item_plays_its_exit_and_then_drops", crate::animated_list::a_removed_item_plays_its_exit_and_then_drops),
            ("animated_list::a_key_returning_mid_exit_reclaims_its_element", crate::animated_list::a_key_returning_mid_exit_reclaims_its_element),
            ("animated_list::swapped_items_slide_between_their_places", crate::animated_list::swapped_items_slide_between_their_places),
            ("animated_list::a_duplicate_key_panics_naming_it", crate::animated_list::a_duplicate_key_panics_naming_it),
            ("animated_list::an_item_entering_the_band_takes_its_place_without_sliding", crate::animated_list::an_item_entering_the_band_takes_its_place_without_sliding),
            ("animated_list::a_resized_item_moves_its_followers_at_once", crate::animated_list::a_resized_item_moves_its_followers_at_once),
            ("animated_list::reduced_motion_lands_every_change_on_the_next_frame", crate::animated_list::reduced_motion_lands_every_change_on_the_next_frame),
            ("animated_list::grid_tiles_slide_between_cells", crate::animated_list::grid_tiles_slide_between_cells),
            ("animated_list::a_staggered_list_mounts_its_items_one_after_another", crate::animated_list::a_staggered_list_mounts_its_items_one_after_another),
            ("animated_list::a_panicking_item_drop_leaves_the_list_working", crate::animated_list::a_panicking_item_drop_leaves_the_list_working),
            ("implicit_animations::swapping_the_scope_registry_preserves_an_implicit_run", crate::implicit_animations::swapping_the_scope_registry_preserves_an_implicit_run),
            ("implicit_animations::reparenting_across_ticker_mode_moves_the_registration", crate::implicit_animations::reparenting_across_ticker_mode_moves_the_registration),
            ("implicit_animations::a_detached_ticker_mode_lands_an_implicit_run", crate::implicit_animations::a_detached_ticker_mode_lands_an_implicit_run),
            ("implicit_animations::animated_opacity_retargets_from_the_current_value_midflight", crate::implicit_animations::animated_opacity_retargets_from_the_current_value_midflight),
            ("implicit_animations::opacity_retarget_with_a_new_curve_keeps_the_displayed_sample", crate::implicit_animations::opacity_retarget_with_a_new_curve_keeps_the_displayed_sample),
            ("implicit_animations::opacity_retarget_preserves_the_painted_velocity", crate::implicit_animations::opacity_retarget_preserves_the_painted_velocity),
            ("implicit_animations::padding_retarget_preserves_the_laid_out_velocity", crate::implicit_animations::padding_retarget_preserves_the_laid_out_velocity),
            ("implicit_animations::padding_retarget_with_a_new_curve_keeps_the_displayed_sample", crate::implicit_animations::padding_retarget_with_a_new_curve_keeps_the_displayed_sample),
            ("implicit_animations::container_retarget_with_a_new_curve_keeps_the_displayed_sample", crate::implicit_animations::container_retarget_with_a_new_curve_keeps_the_displayed_sample),
            ("implicit_animations::container_retarget_preserves_the_laid_out_size_velocity", crate::implicit_animations::container_retarget_preserves_the_laid_out_size_velocity),
            ("implicit_animations::container_color_retarget_preserves_painted_alpha_progress", crate::implicit_animations::container_color_retarget_preserves_painted_alpha_progress),
            ("implicit_animations::container_property_motion_settles_and_unmounts_independently", crate::implicit_animations::container_property_motion_settles_and_unmounts_independently),
            ("implicit_animations::container_refused_property_motion_preserves_the_admitted_goals", crate::implicit_animations::container_refused_property_motion_preserves_the_admitted_goals),
            ("implicit_animations::container_refused_transform_motion_preserves_the_admitted_matrix", crate::implicit_animations::container_refused_transform_motion_preserves_the_admitted_matrix),
            ("implicit_animations::an_absent_container_transform_owns_no_frame_registration", crate::implicit_animations::an_absent_container_transform_owns_no_frame_registration),
            ("implicit_animations::container_registry_migration_finishes_before_a_wake_failure", crate::implicit_animations::container_registry_migration_finishes_before_a_wake_failure),
            ("implicit_animations::align_registry_migration_finishes_before_a_wake_failure", crate::implicit_animations::align_registry_migration_finishes_before_a_wake_failure),
            ("implicit_animations::align_retarget_with_a_new_curve_keeps_the_displayed_sample", crate::implicit_animations::align_retarget_with_a_new_curve_keeps_the_displayed_sample),
            ("implicit_animations::align_retarget_preserves_the_laid_out_velocity", crate::implicit_animations::align_retarget_preserves_the_laid_out_velocity),
            ("implicit_animations::align_changes_leave_an_unchanged_factor_on_its_original_deadline", crate::implicit_animations::align_changes_leave_an_unchanged_factor_on_its_original_deadline),
            ("implicit_animations::align_spring_settles_and_optional_factors_snap_independently", crate::implicit_animations::align_spring_settles_and_optional_factors_snap_independently),
            ("implicit_animations::invalid_align_targets_preserve_all_running_layout_properties", crate::implicit_animations::invalid_align_targets_preserve_all_running_layout_properties),
            ("implicit_animations::rotation_retarget_with_a_new_curve_keeps_the_displayed_sample", crate::implicit_animations::rotation_retarget_with_a_new_curve_keeps_the_displayed_sample),
            ("implicit_animations::rotation_retarget_preserves_the_painted_velocity", crate::implicit_animations::rotation_retarget_preserves_the_painted_velocity),
            ("implicit_animations::changing_only_the_curve_keeps_the_existing_deadline", crate::implicit_animations::changing_only_the_curve_keeps_the_existing_deadline),
            ("implicit_animations::changing_only_the_duration_preserves_motion_and_sets_a_new_deadline", crate::implicit_animations::changing_only_the_duration_preserves_motion_and_sets_a_new_deadline),
            ("implicit_animations::overshooting_padding_stays_non_negative", crate::implicit_animations::overshooting_padding_stays_non_negative),
            ("implicit_animations::overshooting_margin_stays_non_negative", crate::implicit_animations::overshooting_margin_stays_non_negative),
            ("implicit_animations::overshooting_size_stays_non_negative", crate::implicit_animations::overshooting_size_stays_non_negative),
            ("implicit_animations::animated_container_animates_its_transform", crate::implicit_animations::animated_container_animates_its_transform),
            ("implicit_animations::non_finite_container_targets_preserve_the_last_admitted_layout", crate::implicit_animations::non_finite_container_targets_preserve_the_last_admitted_layout),
            ("implicit_animations::animated_container_reanchors_unchanged_properties_on_restart", crate::implicit_animations::animated_container_reanchors_unchanged_properties_on_restart),
            ("implicit_animations::animated_container_keeps_an_unchanged_collapsed_transform_on_restart", crate::implicit_animations::animated_container_keeps_an_unchanged_collapsed_transform_on_restart),
            ("implicit_animations::animated_rotation_takes_the_shorter_arc", crate::implicit_animations::animated_rotation_takes_the_shorter_arc),
            ("implicit_animations::animated_rotation_takes_the_numeric_arc", crate::implicit_animations::animated_rotation_takes_the_numeric_arc),
            ("implicit_animations::animated_rotation_retargets_on_a_path_change", crate::implicit_animations::animated_rotation_retargets_on_a_path_change),
            ("implicit_animations::motion_replaces_the_configured_motion", crate::implicit_animations::motion_replaces_the_configured_motion),
            ("implicit_animations::implicit_widgets_land_on_their_target_under_reduced_motion", crate::implicit_animations::implicit_widgets_land_on_their_target_under_reduced_motion),
            ("implicit_animations::single_value_implicit_widgets_show_the_half_way_value", crate::implicit_animations::single_value_implicit_widgets_show_the_half_way_value),
            ("implicit_animations::positioned_edges_and_size_move_half_way", crate::implicit_animations::positioned_edges_and_size_move_half_way),
            ("implicit_animations::cross_fade_fades_resizes_and_routes_pointers_to_the_shown_child", crate::implicit_animations::cross_fade_fades_resizes_and_routes_pointers_to_the_shown_child),
            ("implicit_animations::default_text_style_grows_between_sizes_and_restarts_from_the_shown_one", crate::implicit_animations::default_text_style_grows_between_sizes_and_restarts_from_the_shown_one),
            ("implicit_animations::translation_fractions_interpolate_finitely", crate::implicit_animations::translation_fractions_interpolate_finitely),
            ("implicit_animations::animated_lerp_keeps_overshoot_and_applies_motion_changes", crate::implicit_animations::animated_lerp_keeps_overshoot_and_applies_motion_changes),
            ("implicit_animations::animated_lerp_retires_each_anchor_on_unmount", crate::implicit_animations::animated_lerp_retires_each_anchor_on_unmount),
            ("implicit_animations::default_text_style_keeps_a_traded_line_height_shapeable", crate::implicit_animations::default_text_style_keeps_a_traded_line_height_shapeable),
            ("implicit_animations::text_style_overshoot_stays_shapeable", crate::implicit_animations::text_style_overshoot_stays_shapeable),
            ("implicit_animations::cross_fade_announces_only_the_shown_child", crate::implicit_animations::cross_fade_announces_only_the_shown_child),
            ("implicit_animations::cross_fade_focuses_only_the_shown_child", crate::implicit_animations::cross_fade_focuses_only_the_shown_child),
            ("implicit_animations::cross_fade_keeps_its_selection_when_a_retarget_is_refused", crate::implicit_animations::cross_fade_keeps_its_selection_when_a_retarget_is_refused),
            ("implicit_animations::new_implicit_widgets_keep_velocity_across_a_spring_retarget", crate::implicit_animations::new_implicit_widgets_keep_velocity_across_a_spring_retarget),
            ("implicit_animations::an_invalid_first_placement_is_refused_as_a_whole", crate::implicit_animations::an_invalid_first_placement_is_refused_as_a_whole),
            ("binding_animation::registered_controller_advances_fade_opacity_frame_to_frame", crate::binding_animation::registered_controller_advances_fade_opacity_frame_to_frame),
            ("binding_animation::fade_source_replacement_retries_after_status_failure", crate::binding_animation::fade_source_replacement_retries_after_status_failure),
            ("visibility::hidden_without_maintain_state_shows_the_default_replacement", crate::visibility::hidden_without_maintain_state_shows_the_default_replacement),
            ("visibility::maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes", crate::visibility::maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes),
        ],
    );
}

/// Explicit transitions: a tick moves the transform, the layout or the painted
/// decoration on the same frame without rebuilding any element.
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
            ("size", crate::transitions::size_ticks_without_rebuilding),
            ("align", crate::transitions::align_ticks_without_rebuilding),
            (
                "positioned",
                crate::transitions::positioned_ticks_without_rebuilding,
            ),
            (
                "decorated_box",
                crate::transitions::decorated_box_ticks_without_rebuilding,
            ),
            (
                "reduced_motion",
                crate::transitions::reduced_motion_lands_on_the_end_value,
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
                "a_moving_slide_preserves_selected_pointer_coordinates",
                crate::transitions::a_moving_slide_preserves_selected_pointer_coordinates,
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
            ("stack_positioned::an_indexed_stack_is_heard_only_through_its_shown_child", crate::stack_positioned::an_indexed_stack_is_heard_only_through_its_shown_child),
            ("wrap::wrap_three_boxes_form_two_runs_when_width_is_narrow", crate::wrap::wrap_three_boxes_form_two_runs_when_width_is_narrow),
            ("wrap::wrap_starts_its_runs_at_the_reading_edge", crate::wrap::wrap_starts_its_runs_at_the_reading_edge),
            ("table::table_mounts_render_table_and_lays_out_a_grid_row_major", crate::table::table_mounts_render_table_and_lays_out_a_grid_row_major),
            ("flex_parent_data::two_expandeds_split_main_axis_by_flex_factor", crate::flex_parent_data::two_expandeds_split_main_axis_by_flex_factor),
            ("flex_parent_data::vertical_direction_updates_mounted_flex_children", crate::flex_parent_data::vertical_direction_updates_mounted_flex_children),
            ("composition::nested_padding_accumulates_insets_through_levels", crate::composition::nested_padding_accumulates_insets_through_levels),
            ("custom_multi_child_layout::custom_multi_child_layout_mounts_render_object_and_positions_layout_id_children", crate::custom_multi_child_layout::custom_multi_child_layout_mounts_render_object_and_positions_layout_id_children),
            ("child_type_swap::a_replaced_root_render_object_is_laid_out_in_the_frame_it_is_mounted", crate::child_type_swap::a_replaced_root_render_object_is_laid_out_in_the_frame_it_is_mounted),
            ("unchanged_child::a_child_handed_back_unchanged_is_built_only_when_something_it_reads_changes", crate::unchanged_child::a_child_handed_back_unchanged_is_built_only_when_something_it_reads_changes),
            ("unchanged_child::a_lazily_mounted_item_handed_back_unchanged_is_not_built_again", crate::unchanged_child::a_lazily_mounted_item_handed_back_unchanged_is_not_built_again),
            ("component_child_ordering::component_child_keeps_slot_order_before_a_render_sibling", crate::component_child_ordering::component_child_keeps_slot_order_before_a_render_sibling),
            ("component_child_ordering::a_lazy_list_item_keeps_its_containers_children_in_slot_order", crate::component_child_ordering::a_lazy_list_item_keeps_its_containers_children_in_slot_order),
            ("parent_data_ancestry::expanded_under_stack_panics_at_attach_with_ancestry_diagnostic", crate::parent_data_ancestry::expanded_under_stack_panics_at_attach_with_ancestry_diagnostic),
            ("layer_inspection::the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list", crate::layer_inspection::the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list),
            ("layer_inspection::a_superellipse_clip_composites_and_updates_its_radius", crate::layer_inspection::a_superellipse_clip_composites_and_updates_its_radius),
            ("layer_inspection::backdrop_filters_in_one_group_share_a_stable_identity", crate::layer_inspection::backdrop_filters_in_one_group_share_a_stable_identity),
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
        ("keyboard_modifiers::lock_states_do_not_disable_slider_navigation", crate::keyboard_modifiers::lock_states_do_not_disable_slider_navigation),
        ("keyboard_modifiers::slider_glyph_and_composition_policy", crate::keyboard_modifiers::slider_glyph_and_composition_policy),
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
        ("catalog_disclosure::disclosure_header_controls_its_labelled_panel", crate::catalog_disclosure::disclosure_header_controls_its_labelled_panel as fn()),
        ("catalog_disclosure::disclosure_focus_reentry_cannot_activate_after_focus_manager_close", crate::catalog_disclosure::disclosure_focus_reentry_cannot_activate_after_focus_manager_close as fn()),
        ("catalog_disclosure::disclosure_retained_expand_after_unmount_cannot_reach_a_new_control", crate::catalog_disclosure::disclosure_retained_expand_after_unmount_cannot_reach_a_new_control as fn()),
        ("catalog_disclosure::disclosure_owns_constructor_clone_and_retirement_failure_tails", crate::catalog_disclosure::disclosure_owns_constructor_clone_and_retirement_failure_tails as fn()),
    ]);
}

/// A framework relay registered on a controller's `Listenable` runs inside the
/// notifying delivery's recovery scope, so it sees a failure an earlier
/// listener raised; that failure still propagates once the delivery ends.
#[test]
fn relay_failure_custody() {
    run_cases(
        "relay_failure_custody",
        &[
            (
                "listenable_relays::text_editing_controller_relays_keep_failure_custody",
                crate::listenable_relays::text_editing_controller_relays_keep_failure_custody
                    as fn(),
            ),
            (
                "listenable_relays::widget_states_controller_relays_keep_failure_custody",
                crate::listenable_relays::widget_states_controller_relays_keep_failure_custody,
            ),
            (
                "listenable_relays::transformation_controller_relays_keep_failure_custody",
                crate::listenable_relays::transformation_controller_relays_keep_failure_custody,
            ),
            (
                "listenable_relays::scroll_position_relays_keep_failure_custody",
                crate::listenable_relays::scroll_position_relays_keep_failure_custody,
            ),
        ],
    );
}

/// The nested scroll channel (ADR-0209): one offset primitive shared along an owner-local
/// chain — latch per axis, remainder rule, fling hand-off under the releasing sequence,
/// preemption, the participant contract, snapping — each row scripted for touch and mouse;
/// with the fling hand-off and reveal witnesses ADR-0169 left.
#[test]
fn nested_scroll_channel() {
    run_cases(
        "nested_scroll_channel",
        &[
            ("nested_scroll::list_in_pager_shares_deltas_by_axis", crate::nested_scroll::list_in_pager_shares_deltas_by_axis as fn()),
            ("nested_scroll::steps_carry_the_focal_point_and_the_device_pixel_ratio", crate::nested_scroll::steps_carry_the_focal_point_and_the_device_pixel_ratio),
            ("nested_scroll::each_participant_is_offered_the_step_in_its_own_plane", crate::nested_scroll::each_participant_is_offered_the_step_in_its_own_plane),
            ("nested_scroll::edge_at_start_latches_ancestor_on_every_preset", crate::nested_scroll::edge_at_start_latches_ancestor_on_every_preset),
            ("nested_scroll::mid_drag_transfer_relatches_only_at_edge_within_the_sequence", crate::nested_scroll::mid_drag_transfer_relatches_only_at_edge_within_the_sequence),
            ("nested_scroll::channel_latch_ends_exactly_when_binding_mints_a_new_sequence", crate::nested_scroll::channel_latch_ends_exactly_when_binding_mints_a_new_sequence),
            ("nested_scroll::an_idle_wheel_sequence_ends_at_its_latch_deadline", crate::nested_scroll::an_idle_wheel_sequence_ends_at_its_latch_deadline),
            ("nested_scroll::fling_handoff_inherits_sequence_across_three_levels", crate::nested_scroll::fling_handoff_inherits_sequence_across_three_levels),
            ("nested_scroll::momentum_never_offers_post_fling", crate::nested_scroll::momentum_never_offers_post_fling),
            ("nested_scroll::new_touch_preempts_handed_off_fling", crate::nested_scroll::new_touch_preempts_handed_off_fling),
            ("nested_scroll::new_wheel_or_assistive_scroll_preempts_handed_off_fling", crate::nested_scroll::new_wheel_or_assistive_scroll_preempts_handed_off_fling),
            ("nested_scroll::user_input_preempts_programmatic_scroll", crate::nested_scroll::user_input_preempts_programmatic_scroll),
            ("nested_scroll::a_participant_declining_programmatic_scrolls_hears_none_of_them", crate::nested_scroll::a_participant_declining_programmatic_scrolls_hears_none_of_them),
            ("nested_scroll::anchor_correction_bypasses_the_channel", crate::nested_scroll::anchor_correction_bypasses_the_channel),
            ("nested_scroll::participant_result_is_clamped", crate::nested_scroll::participant_result_is_clamped),
            ("nested_scroll::a_step_that_is_not_finite_is_dropped_at_the_channel", crate::nested_scroll::a_step_that_is_not_finite_is_dropped_at_the_channel),
            ("nested_scroll::a_participant_cannot_dispatch_input_into_its_own_chain", crate::nested_scroll::a_participant_cannot_dispatch_input_into_its_own_chain),
            ("nested_scroll::requests::a_jump_from_a_participant_lands_when_the_pass_returns", crate::nested_scroll::requests::a_jump_from_a_participant_lands_when_the_pass_returns),
            ("nested_scroll::requests::a_jump_from_a_position_listener_never_nests_inside_a_step", crate::nested_scroll::requests::a_jump_from_a_position_listener_never_nests_inside_a_step),
            ("nested_scroll::requests::requests_made_in_one_pass_keep_the_latest", crate::nested_scroll::requests::requests_made_in_one_pass_keep_the_latest),
            ("nested_scroll::requests::a_landing_jump_preempts_the_run_and_keeps_live_input", crate::nested_scroll::requests::a_landing_jump_preempts_the_run_and_keeps_live_input),
            ("nested_scroll::requests::a_jump_asked_before_a_participant_panics_still_lands", crate::nested_scroll::requests::a_jump_asked_before_a_participant_panics_still_lands),
            ("nested_scroll::requests::a_jump_feedback_loop_is_bounded_and_keeps_its_target", crate::nested_scroll::requests::a_jump_feedback_loop_is_bounded_and_keeps_its_target),
            ("nested_scroll::requests::a_jump_pending_across_an_unmount_writes_the_position", crate::nested_scroll::requests::a_jump_pending_across_an_unmount_writes_the_position),
            ("nested_scroll::caught::a_remainder_catches_a_springing_ancestor_and_moves_it_once", crate::nested_scroll::caught::a_remainder_catches_a_springing_ancestor_and_moves_it_once),
            ("nested_scroll::caught::an_outward_remainder_leaves_the_spring_running", crate::nested_scroll::caught::an_outward_remainder_leaves_the_spring_running),
            ("nested_scroll::caught::a_new_contact_holds_a_springing_ancestor_until_the_inner_drag_takes_it", crate::nested_scroll::caught::a_new_contact_holds_a_springing_ancestor_until_the_inner_drag_takes_it),
            ("nested_scroll::caught::a_caught_ancestor_settles_at_release_with_the_latched_velocity", crate::nested_scroll::caught::a_caught_ancestor_settles_at_release_with_the_latched_velocity),
            ("nested_scroll::caught::a_caught_ancestor_settles_when_the_sequence_ends_without_release", crate::nested_scroll::caught::a_caught_ancestor_settles_when_the_sequence_ends_without_release),
            ("nested_scroll::caught::a_caught_ancestor_touched_directly_follows_its_own_contact", crate::nested_scroll::caught::a_caught_ancestor_touched_directly_follows_its_own_contact),
            ("nested_scroll::caught::a_caught_ancestor_retired_mid_drag_is_settled_by_its_successor", crate::nested_scroll::caught::a_caught_ancestor_retired_mid_drag_is_settled_by_its_successor),
            ("nested_scroll::a_fresh_ancestor_physics_mid_drag_keeps_taking_the_remainder", crate::nested_scroll::a_fresh_ancestor_physics_mid_drag_keeps_taking_the_remainder),
            ("nested_scroll::a_new_chain_above_keeps_the_leaf_run", crate::nested_scroll::a_new_chain_above_keeps_the_leaf_run),
            ("nested_scroll::participant_panic_consumes_all_and_next_sequence_recovers", crate::nested_scroll::participant_panic_consumes_all_and_next_sequence_recovers),
            ("nested_scroll::a_contact_that_ends_without_release_names_why", crate::nested_scroll::a_contact_that_ends_without_release_names_why),
            ("nested_scroll::a_release_taken_over_by_reentrant_code_still_ends_its_sequence", crate::nested_scroll::a_release_taken_over_by_reentrant_code_still_ends_its_sequence),
            ("nested_scroll::contained_scrollable_keeps_its_remainder", crate::nested_scroll::contained_scrollable_keeps_its_remainder),
            ("nested_scroll::snap_policy_targets", crate::nested_scroll::snap_policy_targets),
            ("interactive_viewer::photo_in_pager_hands_pan_remainder_to_pager", crate::interactive_viewer::photo_in_pager_hands_pan_remainder_to_pager),
            ("interactive_viewer::a_turned_photo_at_its_edge_frees_the_axis_it_cannot_pan", crate::interactive_viewer::a_turned_photo_at_its_edge_frees_the_axis_it_cannot_pan),
            ("interactive_viewer::photo_in_rtl_pager_turns_toward_the_reading_end", crate::interactive_viewer::photo_in_rtl_pager_turns_toward_the_reading_end),
            ("interactive_viewer::viewer_inertia_is_a_side_effect_of_the_releasing_sequence", crate::interactive_viewer::viewer_inertia_is_a_side_effect_of_the_releasing_sequence),
            ("interactive_viewer::viewer_release_failure_still_ends_the_sequence", crate::interactive_viewer::viewer_release_failure_still_ends_the_sequence),
            ("interactive_viewer::plain_wheel_pans_the_viewer_then_leaves_its_edge_to_the_list", crate::interactive_viewer::plain_wheel_pans_the_viewer_then_leaves_its_edge_to_the_list),
            ("interactive_viewer::viewer_pan_past_its_boundary_stretches_and_snaps_back", crate::interactive_viewer::viewer_pan_past_its_boundary_stretches_and_snaps_back),
            ("interactive_viewer::viewer_fling_into_its_boundary_overshoots_and_settles", crate::interactive_viewer::viewer_fling_into_its_boundary_overshoots_and_settles),
            ("refresh_indicator::pull_past_threshold_refreshes_once", crate::refresh_indicator::pull_past_threshold_refreshes_once),
            ("refresh_indicator::pull_released_short_settles_back", crate::refresh_indicator::pull_released_short_settles_back),
            ("refresh_indicator::a_refresh_style_rebuilds_its_indicator_as_the_pull_grows", crate::refresh_indicator::a_refresh_style_rebuilds_its_indicator_as_the_pull_grows),
            ("refresh_indicator::pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss", crate::refresh_indicator::pull_to_refresh_ignores_wheel_and_momentum_and_resets_on_loss),
            ("refresh_indicator::refresh_during_refresh_is_refused", crate::refresh_indicator::refresh_during_refresh_is_refused),
            ("refresh_indicator::refresh_completion_arrives_on_the_next_frame", crate::refresh_indicator::refresh_completion_arrives_on_the_next_frame),
            ("refresh_indicator::unmount_mid_refresh_cancels_it", crate::refresh_indicator::unmount_mid_refresh_cancels_it),
            ("refresh_indicator::horizontal_pull_refreshes_from_the_left_edge", crate::refresh_indicator::horizontal_pull_refreshes_from_the_left_edge),
            ("refresh_indicator::horizontal_pull_refreshes_from_the_reading_start_under_rtl", crate::refresh_indicator::horizontal_pull_refreshes_from_the_reading_start_under_rtl),
            ("refresh_indicator::a_replaced_configuration_takes_over_the_refresh", crate::refresh_indicator::a_replaced_configuration_takes_over_the_refresh),
            ("refresh_indicator::a_first_command_without_the_refresh_intent_is_reported", crate::refresh_indicator::a_first_command_without_the_refresh_intent_is_reported),
            ("anchored_drag::anchored_drag_settles_on_anchors_by_release_and_fling", crate::anchored_drag::anchored_drag_settles_on_anchors_by_release_and_fling),
            ("anchored_drag::anchored_drag_reads_offsets_toward_the_reading_end", crate::anchored_drag::anchored_drag_reads_offsets_toward_the_reading_end),
            ("anchored_drag::anchored_drag_refuses_degenerate_anchor_sets", crate::anchored_drag::anchored_drag_refuses_degenerate_anchor_sets),
            ("anchored_drag::anchored_drag_follows_anchors_replaced_mid_drag", crate::anchored_drag::anchored_drag_follows_anchors_replaced_mid_drag),
            ("anchored_drag::anchored_drag_unmount_mid_drag_rests_on_the_nearest_anchor", crate::anchored_drag::anchored_drag_unmount_mid_drag_rests_on_the_nearest_anchor),
            ("anchored_drag::anchored_drag_assistive_and_key_steps_move_between_anchors", crate::anchored_drag::anchored_drag_assistive_and_key_steps_move_between_anchors),
            ("anchored_drag::anchored_drag_plays_a_haptic_on_each_new_anchor", crate::anchored_drag::anchored_drag_plays_a_haptic_on_each_new_anchor),
            ("anchored_drag::anchored_drag_is_a_bounded_leaf_and_a_participant", crate::anchored_drag::anchored_drag_is_a_bounded_leaf_and_a_participant),
            ("anchored_drag::anchored_drag_is_no_drop_autoscroll_target", crate::anchored_drag::anchored_drag_is_no_drop_autoscroll_target),
            ("anchored_drag::anchored_drag_is_a_tab_stop_and_a_named_slider", crate::anchored_drag::anchored_drag_is_a_tab_stop_and_a_named_slider),
            ("anchored_drag::anchored_drag_offset_listenable_follows_every_move", crate::anchored_drag::anchored_drag_offset_listenable_follows_every_move),
            ("swipe_actions::swipe_actions_reveal_settle_and_perform_a_tapped_action", crate::swipe_actions::swipe_actions_reveal_settle_and_perform_a_tapped_action),
            ("swipe_actions::swipe_actions_full_swipe_performs_the_first_action_on_release", crate::swipe_actions::swipe_actions_full_swipe_performs_the_first_action_on_release),
            ("swipe_actions::swipe_actions_mirror_right_to_left", crate::swipe_actions::swipe_actions_mirror_right_to_left),
            ("swipe_actions::swipe_rows_in_a_vertical_list_share_contacts_by_axis", crate::swipe_actions::swipe_rows_in_a_vertical_list_share_contacts_by_axis),
            ("swipe_actions::swipe_actions_leave_the_system_edges_to_the_os", crate::swipe_actions::swipe_actions_leave_the_system_edges_to_the_os),
            ("swipe_actions::swipe_actions_unmounted_mid_swipe_perform_nothing", crate::swipe_actions::swipe_actions_unmounted_mid_swipe_perform_nothing),
            ("swipe_actions::a_panicking_swipe_action_leaves_the_row_closed", crate::swipe_actions::a_panicking_swipe_action_leaves_the_row_closed),
            ("swipe_actions::a_cancelled_swipe_returns_where_it_began", crate::swipe_actions::a_cancelled_swipe_returns_where_it_began),
            ("swipe_actions::swipe_actions_refuse_rows_without_distinct_actions", crate::swipe_actions::swipe_actions_refuse_rows_without_distinct_actions),
            ("collapsing_header::collapsing_header_collapses_before_or_after_the_content", crate::collapsing_header::collapsing_header_collapses_before_or_after_the_content),
            ("collapsing_header::collapsing_header_expands_first_or_last", crate::collapsing_header::collapsing_header_expands_first_or_last),
            ("collapsing_header::collapsing_header_inherits_the_fling_across_its_collapse", crate::collapsing_header::collapsing_header_inherits_the_fling_across_its_collapse),
            ("collapsing_header::collapsing_header_snaps_to_an_extent", crate::collapsing_header::collapsing_header_snaps_to_an_extent),
            ("collapsing_header::collapsing_header_rests_within_half_a_device_pixel", crate::collapsing_header::collapsing_header_rests_within_half_a_device_pixel),
            ("collapsing_header::collapsing_header_reveals_focused_row_from_tab_and_assistive_request", crate::collapsing_header::collapsing_header_reveals_focused_row_from_tab_and_assistive_request),
            ("collapsing_header::collapsing_header_is_a_banner_assistive_scrolls_expand_and_collapse", crate::collapsing_header::collapsing_header_is_a_banner_assistive_scrolls_expand_and_collapse),
            ("collapsing_header::collapsing_header_with_equal_extents_takes_no_scroll", crate::collapsing_header::collapsing_header_with_equal_extents_takes_no_scroll),
            ("nested_scroll::scroll_channel_data_is_send", crate::nested_scroll::scroll_channel_data_is_send),
            ("nested_scroll::reveal_and_assistive_scroll_go_through_the_chain", crate::nested_scroll::reveal_and_assistive_scroll_go_through_the_chain),
            ("nested_scroll::a_nested_reveal_moves_each_ancestor_in_a_sequence_of_its_own", crate::nested_scroll::a_nested_reveal_moves_each_ancestor_in_a_sequence_of_its_own),
            ("nested_scroll::connection_defaults_take_nothing", crate::nested_scroll::connection_defaults_take_nothing),
            ("nested_scroll::fling_residual_survives_a_participant_taking_part_of_it", crate::nested_scroll::fling_residual_survives_a_participant_taking_part_of_it),
            ("nested_scroll::a_run_short_of_the_edge_of_a_long_list_hands_nothing_on", crate::nested_scroll::a_run_short_of_the_edge_of_a_long_list_hands_nothing_on),
            ("nested_scroll::shared_connection_hears_a_sequence_once", crate::nested_scroll::shared_connection_hears_a_sequence_once),
            ("nested_scroll::keyboard_scrolls_through_the_channel_as_its_own_sequence", crate::nested_scroll::keyboard_scrolls_through_the_channel_as_its_own_sequence),
            ("nested_scroll::a_held_scroll_key_ends_its_sequence_when_its_release_goes_elsewhere", crate::nested_scroll::a_held_scroll_key_ends_its_sequence_when_its_release_goes_elsewhere),
            ("nested_scroll::drag_autoscroll_dispatches_side_effect_deltas", crate::nested_scroll::drag_autoscroll_dispatches_side_effect_deltas),
            ("nested_scroll::drag_autoscroll_from_past_an_edge_moves_by_its_step", crate::nested_scroll::drag_autoscroll_from_past_an_edge_moves_by_its_step),
            ("nested_scroll::a_remainder_moves_an_overscrolled_ancestor_only_toward_its_range", crate::nested_scroll::a_remainder_moves_an_overscrolled_ancestor_only_toward_its_range),
            ("nested_scroll::keyboard_bubbles_to_the_enclosing_scrollable_and_follows_the_axis", crate::nested_scroll::keyboard_bubbles_to_the_enclosing_scrollable_and_follows_the_axis),
            ("nested_scroll::dragging_a_scrollable_gives_it_the_scroll_keys", crate::nested_scroll::dragging_a_scrollable_gives_it_the_scroll_keys),
            ("scroll::nested_fling_hands_remaining_velocity_to_matching_parent_axes", crate::scroll::nested_fling_hands_remaining_velocity_to_matching_parent_axes),
            ("scroll::nested_fling_same_controller_rebuild_preserves_accepted_handoff", crate::scroll::nested_fling_same_controller_rebuild_preserves_accepted_handoff),
            ("scroll::nested_fling_parent_boundary_policy_receives_presentation_pixel_ratio", crate::scroll::nested_fling_parent_boundary_policy_receives_presentation_pixel_ratio),
            ("scroll::nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy", crate::scroll::nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy),
            ("scroll::replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers", crate::scroll::replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers),
            ("scroll::nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress", crate::scroll::nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress),
            ("scroll::nested_fling_equal_edge_jump_cancels_old_handoff_and_next_gesture_recovers", crate::scroll::nested_fling_equal_edge_jump_cancels_old_handoff_and_next_gesture_recovers),
            ("scroll::nested_fling_custom_physics_failure_and_retirement_preserve_first_and_recover", crate::scroll::nested_fling_custom_physics_failure_and_retirement_preserve_first_and_recover),
            ("scroll::nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer", crate::scroll::nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer),
            ("scroll::nested_fling_bouncing_parent_at_extent_absorbs_before_grandparent", crate::scroll::nested_fling_bouncing_parent_at_extent_absorbs_before_grandparent),
            ("scroll::show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse", crate::scroll::show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse),
            ("scroll::show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry", crate::scroll::show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry),
            ("scroll::show_on_screen_failure_continues_live_ancestors_and_fresh_requests_recover", crate::scroll::show_on_screen_failure_continues_live_ancestors_and_fresh_requests_recover),
            ("scroll::show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers", crate::scroll::show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers),
            ("scroll::show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion", crate::scroll::show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion),
        ],
    );
}

/// Every gesture of a behavioural widget reached by assistive technology,
/// the keyboard and one pointer (ADR-0216): the input-path matrix rows.
#[test]
fn gesture_alternatives() {
    run_cases(
        "gesture_alternatives",
        &[
            (
                "gesture_alternatives::dismissible_with_a_visible_control",
                crate::gesture_alternatives::dismissible_with_a_visible_control as fn(),
            ),
            (
                "gesture_alternatives::dismissible_with_a_context_menu",
                crate::gesture_alternatives::dismissible_with_a_context_menu,
            ),
            (
                "gesture_alternatives::swipe_actions_with_visible_controls",
                crate::gesture_alternatives::swipe_actions_with_visible_controls,
            ),
            (
                "gesture_alternatives::swipe_actions_with_a_presented_menu",
                crate::gesture_alternatives::swipe_actions_with_a_presented_menu,
            ),
            (
                "gesture_alternatives::swipe_actions_with_the_plain_context_menu",
                crate::gesture_alternatives::swipe_actions_with_the_plain_context_menu,
            ),
            (
                "gesture_alternatives::interactive_viewer_with_zoom_controls",
                crate::gesture_alternatives::interactive_viewer_with_zoom_controls,
            ),
            (
                "gesture_alternatives::interactive_viewer_with_pan_controls",
                crate::gesture_alternatives::interactive_viewer_with_pan_controls,
            ),
            (
                "gesture_alternatives::page_view_with_page_controls",
                crate::gesture_alternatives::page_view_with_page_controls,
            ),
            (
                "gesture_alternatives::anchored_drag_with_anchor_controls",
                crate::gesture_alternatives::anchored_drag_with_anchor_controls,
            ),
            (
                "gesture_alternatives::page_view_with_buttons_beside_it",
                crate::gesture_alternatives::page_view_with_buttons_beside_it,
            ),
            (
                "gesture_alternatives::a_companion_link_follows_its_registrations",
                crate::gesture_alternatives::a_companion_link_follows_its_registrations,
            ),
            (
                "gesture_alternatives::a_companion_command_must_perform_an_intent_its_link_carries",
                crate::gesture_alternatives::a_companion_command_must_perform_an_intent_its_link_carries,
            ),
            (
                "gesture_alternatives::unfulfilled_companion_alternatives_are_reported",
                crate::gesture_alternatives::unfulfilled_companion_alternatives_are_reported,
            ),
            (
                "gesture_alternatives::every_preset_opens_the_menu_by_its_pointer_trigger",
                crate::gesture_alternatives::every_preset_opens_the_menu_by_its_pointer_trigger,
            ),
            (
                "gesture_alternatives::a_press_off_the_revealed_control_frees_the_trailing_edge",
                crate::gesture_alternatives::a_press_off_the_revealed_control_frees_the_trailing_edge,
            ),
            (
                "gesture_alternatives::the_menu_long_press_waits_for_the_system_setting",
                crate::gesture_alternatives::the_menu_long_press_waits_for_the_system_setting,
            ),
            (
                "gesture_alternatives::the_revealed_menu_control_follows_focus_and_reading_direction",
                crate::gesture_alternatives::the_revealed_menu_control_follows_focus_and_reading_direction,
            ),
            (
                "gesture_alternatives::labels_name_one_action_each",
                crate::gesture_alternatives::labels_name_one_action_each,
            ),
            (
                "gesture_alternatives::reserved_keys_are_not_bound",
                crate::gesture_alternatives::reserved_keys_are_not_bound,
            ),
            (
                "gesture_alternatives::controls_follow_the_reading_direction",
                crate::gesture_alternatives::controls_follow_the_reading_direction,
            ),
            (
                "gesture_alternatives::the_revealed_menu_control_names_its_target",
                crate::gesture_alternatives::the_revealed_menu_control_names_its_target,
            ),
            (
                "gesture_alternatives::the_revealed_menu_control_draws_its_glyph_on_its_own_surface",
                crate::gesture_alternatives::the_revealed_menu_control_draws_its_glyph_on_its_own_surface,
            ),
            (
                "gesture_alternatives::an_unanswered_intent_and_a_reserved_key_are_reported",
                crate::gesture_alternatives::an_unanswered_intent_and_a_reserved_key_are_reported,
            ),
            (
                "refresh_indicator::refresh_indicator_with_a_visible_control",
                crate::refresh_indicator::refresh_indicator_with_a_visible_control,
            ),
            (
                "drag_and_drop::draggable_and_drop_target_by_every_path",
                crate::drag_and_drop::draggable_and_drop_target_by_every_path,
            ),
            (
                "context_menu_area::context_menu_area",
                crate::context_menu_area::context_menu_area,
            ),
            (
                "context_menu_area::context_menu_area_right_to_left",
                crate::context_menu_area::context_menu_area_right_to_left,
            ),
        ],
    );
}

/// The theme-free context menu: where each trigger opens it, the keys inside
/// it, what closes it and what assistive technology reads.
#[test]
fn context_menu_area() {
    run_cases(
        "context_menu_area",
        &[
            (
                "context_menu_area::the_menu_opens_where_its_trigger_points",
                crate::context_menu_area::the_menu_opens_where_its_trigger_points as fn(),
            ),
            (
                "context_menu_area::arrow_keys_home_and_end_move_between_items",
                crate::context_menu_area::arrow_keys_home_and_end_move_between_items,
            ),
            (
                "context_menu_area::escape_and_an_outside_tap_close_the_menu_and_restore_focus",
                crate::context_menu_area::escape_and_an_outside_tap_close_the_menu_and_restore_focus,
            ),
            (
                "context_menu_area::the_menu_closes_when_its_area_unmounts",
                crate::context_menu_area::the_menu_closes_when_its_area_unmounts,
            ),
            (
                "context_menu_area::a_missing_navigator_is_reported",
                crate::context_menu_area::a_missing_navigator_is_reported,
            ),
            (
                "context_menu_area::items_read_in_the_area_direction",
                crate::context_menu_area::items_read_in_the_area_direction,
            ),
            (
                "context_menu_area::the_menu_labels_contrast_with_its_surface_under_any_text_colour",
                crate::context_menu_area::the_menu_labels_contrast_with_its_surface_under_any_text_colour,
            ),
            (
                "context_menu_area::a_panicking_item_leaves_no_menu_open",
                crate::context_menu_area::a_panicking_item_leaves_no_menu_open,
            ),
            (
                "context_menu_area::a_held_press_moved_past_the_slop_closes_the_menu_and_drags",
                crate::context_menu_area::a_held_press_moved_past_the_slop_closes_the_menu_and_drags,
            ),
            (
                "context_menu_area::assistive_technology_reads_a_menu_of_menu_items",
                crate::context_menu_area::assistive_technology_reads_a_menu_of_menu_items,
            ),
        ],
    );
}
