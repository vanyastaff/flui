//! Single-binary consolidation of flui-painting's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `painting_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so file-relative
//! paths keep working unchanged.
//!
//! Each contract family is one table test: the modules expose plain row
//! functions and the tests below run them, naming every failing row.
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-painting's integration tests do: the crate keeps no process-global
//! font state.

#[path = "caret_contract.rs"]
mod caret_contract;

/// Carets, selection boxes, hit-testing and word boundaries read the layout
/// that measured and painted (ADR-0092 §10 step 5).
#[test]
fn caret_contract() {
    use caret_contract as cc;
    run_cases(
        "caret",
        &[
            ("caret_position", cc::caret_position),
            (
                "unbounded_breaking_uses_the_minimum_allocated_width",
                cc::unbounded_breaking_uses_the_minimum_allocated_width,
            ),
            (
                "centered_lines_use_the_tight_allocated_box",
                cc::centered_lines_use_the_tight_allocated_box,
            ),
            (
                "right_aligned_lines_use_the_tight_allocated_box",
                cc::right_aligned_lines_use_the_tight_allocated_box,
            ),
            (
                "loose_centered_lines_stay_inside_the_measured_box",
                cc::loose_centered_lines_stay_inside_the_measured_box,
            ),
            (
                "unbounded_centered_lines_align_without_wrapping",
                cc::unbounded_centered_lines_align_without_wrapping,
            ),
            (
                "rtl_start_aligns_each_line_right",
                cc::rtl_start_aligns_each_line_right,
            ),
            (
                "rtl_end_aligns_each_line_left",
                cc::rtl_end_aligns_each_line_left,
            ),
            (
                "native_rtl_lines_center_in_the_allocated_box",
                cc::native_rtl_lines_center_in_the_allocated_box,
            ),
            (
                "native_rtl_lines_align_to_the_right_edge",
                cc::native_rtl_lines_align_to_the_right_edge,
            ),
            (
                "a_last_kept_soft_line_retains_native_justification",
                cc::a_last_kept_soft_line_retains_native_justification,
            ),
            (
                "trailing_whitespace_does_not_shift_visible_alignment",
                cc::trailing_whitespace_does_not_shift_visible_alignment,
            ),
            (
                "justification_expands_soft_lines_but_not_the_final_line",
                cc::justification_expands_soft_lines_but_not_the_final_line,
            ),
            (
                "justification_leaves_hard_break_lines_unstretched",
                cc::justification_leaves_hard_break_lines_unstretched,
            ),
            (
                "alignment_change_replaces_cached_positions",
                cc::alignment_change_replaces_cached_positions,
            ),
            (
                "ellipsized_lines_align_only_the_kept_text",
                cc::ellipsized_lines_align_only_the_kept_text,
            ),
            (
                "two_space_run_word_boundary",
                cc::two_space_run_word_boundary,
            ),
            (
                "byte_offsets_snap_backward_and_clamp_at_the_text_end",
                cc::byte_offsets_snap_backward_and_clamp_at_the_text_end,
            ),
            (
                "a_combining_mark_is_one_hit_target",
                cc::a_combining_mark_is_one_hit_target,
            ),
            (
                "a_zwj_family_is_one_hit_target",
                cc::a_zwj_family_is_one_hit_target,
            ),
            (
                "rtl_paragraph_carets_run_right_to_left",
                cc::rtl_paragraph_carets_run_right_to_left,
            ),
            (
                "mixed_bidi_boxes_carry_their_run_direction",
                cc::mixed_bidi_boxes_carry_their_run_direction,
            ),
            (
                "a_trailing_newline_puts_the_caret_on_the_empty_line",
                cc::a_trailing_newline_puts_the_caret_on_the_empty_line,
            ),
            (
                "crlf_is_one_break_for_carets",
                cc::crlf_is_one_break_for_carets,
            ),
            (
                "multi_line_selection_boxes_follow_their_line",
                cc::multi_line_selection_boxes_follow_their_line,
            ),
            (
                "carets_sit_on_the_painted_glyphs",
                cc::carets_sit_on_the_painted_glyphs,
            ),
            (
                "a_soft_wrap_caret_follows_its_affinity",
                cc::a_soft_wrap_caret_follows_its_affinity,
            ),
            (
                "truncated_carets_stay_in_kept_lines",
                cc::truncated_carets_stay_in_kept_lines,
            ),
            (
                "truncated_text_without_an_ellipsis_stays_in_its_kept_line",
                cc::truncated_text_without_an_ellipsis_stays_in_its_kept_line,
            ),
            (
                "line_metrics_index_each_line",
                cc::line_metrics_index_each_line,
            ),
            (
                "a_lam_alef_ligature_is_one_glyph_and_two_caret_stops",
                cc::a_lam_alef_ligature_is_one_glyph_and_two_caret_stops,
            ),
        ],
    );
}
#[path = "support/cases.rs"]
mod cases;
#[path = "color_blend.rs"]
mod color_blend;
#[path = "color_property.rs"]
mod color_property;
#[path = "compile_fail.rs"]
mod compile_fail;
#[path = "damage_extent.rs"]
mod damage_extent;
#[path = "decoration_unit.rs"]
mod decoration_unit;
#[path = "font_registration.rs"]
mod font_registration;
#[path = "host_faces_oracle.rs"]
mod host_faces_oracle;
#[path = "parley_metrics_oracle.rs"]
mod parley_metrics_oracle;
#[path = "parley_oracle.rs"]
mod parley_oracle;
#[path = "recording.rs"]
mod recording;
#[path = "rich_text_example.rs"]
mod rich_text_example;
#[path = "text_boundaries.rs"]
mod text_boundaries;
#[path = "text_context.rs"]
mod text_context;
#[path = "text_layout_pipeline.rs"]
mod text_layout_pipeline;
#[path = "text_overflow_unit.rs"]
mod text_overflow_unit;
#[path = "text_painter_unit.rs"]
mod text_painter_unit;
#[path = "values.rs"]
mod values;

use cases::run_cases;

#[test]
fn parley_oracle_contract() {
    run_cases(
        "parley_oracle",
        &[
            (
                "glyph_images_admit_only_complete_mask_and_color_buffers",
                parley_oracle::glyph_images_admit_only_complete_mask_and_color_buffers,
            ),
            (
                "swash_matches_the_recorded_reference",
                parley_oracle::swash_matches_the_recorded_reference,
            ),
            (
                "rasterizing_a_key_twice_draws_the_same_bitmap",
                parley_oracle::rasterizing_a_key_twice_draws_the_same_bitmap,
            ),
            (
                "registered_fonts_release_the_source_and_keep_rasterizing",
                parley_oracle::registered_fonts_release_the_source_and_keep_rasterizing,
            ),
            (
                "subpixel_split_is_total_across_the_float_domain",
                parley_oracle::subpixel_split_is_total_across_the_float_domain,
            ),
            (
                "placed_glyphs_omit_unrepresentable_coordinates",
                parley_oracle::placed_glyphs_omit_unrepresentable_coordinates,
            ),
            (
                "placed_glyphs_keep_representable_extremes_and_hinting",
                parley_oracle::placed_glyphs_keep_representable_extremes_and_hinting,
            ),
            (
                "placed_glyphs_keep_cancelling_vertical_coordinates",
                parley_oracle::placed_glyphs_keep_cancelling_vertical_coordinates,
            ),
        ],
    );
}

#[test]
fn color_contract() {
    run_cases(
        "color",
        &[
            (
                "hex_roundtrips_and_porter_duff_modes_mirror",
                color_property::hex_roundtrips_and_porter_duff_modes_mirror,
            ),
            (
                "blend_over_is_source_over_alpha_blend",
                color_blend::blend_over_is_source_over_alpha_blend,
            ),
            (
                "lerp_multi_stop_brackets_and_clamps",
                color_property::lerp_multi_stop_brackets_and_clamps,
            ),
            (
                "channels_round_to_the_nearest_step",
                color_property::channels_round_to_the_nearest_step,
            ),
        ],
    );
}

#[test]
fn color_lerp_is_premultiplied_oklab() {
    run_cases(
        "color_lerp",
        &[
            (
                "black_to_white_midpoint_is_oklab_mid_grey",
                color_property::black_to_white_midpoint_is_oklab_mid_grey,
            ),
            (
                "lerp_to_transparent_keeps_the_hue",
                color_property::lerp_to_transparent_keeps_the_hue,
            ),
            (
                "lerp_endpoints_are_exact",
                color_property::lerp_endpoints_are_exact,
            ),
            (
                "lerp_matches_the_exact_oklab_evaluation",
                color_property::lerp_matches_the_exact_oklab_evaluation,
            ),
            (
                "lerp_outside_the_segment_saturates_and_nan_keeps_begin",
                color_property::lerp_outside_the_segment_saturates_and_nan_keeps_begin,
            ),
        ],
    );
}

#[test]
fn value_contract() {
    run_cases(
        "value",
        &[
            (
                "radial_overshoot_refuses_coincident_nonzero_circles",
                values::radial_overshoot_refuses_coincident_nonzero_circles,
            ),
            (
                "gradient_geometry_rejects_invalid_inputs_before_equal_shortcuts",
                values::gradient_geometry_rejects_invalid_inputs_before_equal_shortcuts,
            ),
            (
                "gradient_geometry_checks_intermediate_and_output_overflow",
                values::gradient_geometry_checks_intermediate_and_output_overflow,
            ),
            (
                "gradient_packing_preserves_extrapolated_geometry",
                values::gradient_packing_preserves_extrapolated_geometry,
            ),
            (
                "decoration_gradient_falls_back_after_bounds_scaling",
                values::decoration_gradient_falls_back_after_bounds_scaling,
            ),
            (
                "decoration_gradient_centers_fall_back_after_bounds_scaling",
                values::decoration_gradient_centers_fall_back_after_bounds_scaling,
            ),
            (
                "decoration_silhouette_and_terminal_fallback",
                values::decoration_silhouette_and_terminal_fallback,
            ),
            (
                "decoration_endpoint_ramp_preserves_stop_limit",
                values::decoration_endpoint_ramp_preserves_stop_limit,
            ),
            (
                "decoration_linear_overshoot_resolves_in_small_box",
                values::decoration_linear_overshoot_resolves_in_small_box,
            ),
            (
                "gradient_domains_keep_zero_radii_and_signed_angles",
                values::gradient_domains_keep_zero_radii_and_signed_angles,
            ),
            (
                "gradient_geometry_preserves_overshoot",
                values::gradient_geometry_preserves_overshoot,
            ),
            (
                "decoration_gradient_geometry_preserves_overshoot",
                values::decoration_gradient_geometry_preserves_overshoot,
            ),
            (
                "negative_linear_stops_are_rejected",
                values::negative_linear_stops_are_rejected,
            ),
            (
                "radial_stops_above_one_are_rejected",
                values::radial_stops_above_one_are_rejected,
            ),
            (
                "extreme_negative_sweep_stops_are_rejected",
                values::extreme_negative_sweep_stops_are_rejected,
            ),
            (
                "extreme_positive_linear_stops_are_rejected",
                values::extreme_positive_linear_stops_are_rejected,
            ),
            (
                "linear_nan_stops_are_rejected",
                values::linear_nan_stops_are_rejected,
            ),
            (
                "radial_infinite_stops_are_rejected",
                values::radial_infinite_stops_are_rejected,
            ),
            (
                "sweep_descending_stops_are_rejected",
                values::sweep_descending_stops_are_rejected,
            ),
            (
                "empty_equal_gradients_are_rejected",
                values::empty_equal_gradients_are_rejected,
            ),
            (
                "mismatched_equal_gradient_stops_are_rejected",
                values::mismatched_equal_gradient_stops_are_rejected,
            ),
            (
                "nan_gradient_interpolation_is_rejected",
                values::nan_gradient_interpolation_is_rejected,
            ),
            (
                "linear_interpolation_keeps_hard_transitions",
                values::linear_interpolation_keeps_hard_transitions,
            ),
            (
                "radial_interpolation_keeps_hard_transitions",
                values::radial_interpolation_keeps_hard_transitions,
            ),
            (
                "sweep_interpolation_keeps_hard_transitions",
                values::sweep_interpolation_keeps_hard_transitions,
            ),
            (
                "font_weight_from_css_breaks_ties_like_css",
                values::font_weight_from_css_breaks_ties_like_css,
            ),
            (
                "a_one_sided_focal_point_lerps_to_the_other_center",
                values::a_one_sided_focal_point_lerps_to_the_other_center,
            ),
            (
                "an_arc_joins_an_open_contour_and_starts_a_closed_one_fresh",
                values::an_arc_joins_an_open_contour_and_starts_a_closed_one_fresh,
            ),
            #[cfg(feature = "serde")]
            (
                "deserialized_paths_keep_geometry_authoritative",
                values::deserialized_paths_keep_geometry_authoritative,
            ),
            #[cfg(feature = "serde")]
            (
                "serialized_factory_paths_preserve_their_shapes",
                values::serialized_factory_paths_preserve_their_shapes,
            ),
            #[cfg(feature = "serde")]
            (
                "deserialized_images_validate_rgba_dimensions_and_data",
                values::deserialized_images_validate_rgba_dimensions_and_data,
            ),
        ],
    );
}

#[test]
fn recording_contract() {
    run_cases(
        "recording",
        &[
            (
                "save_restore_tracks_the_save_count",
                recording::save_restore_tracks_the_save_count,
            ),
            (
                "shear_factors_map_the_named_axes_in_recorded_commands",
                recording::shear_factors_map_the_named_axes_in_recorded_commands,
            ),
            #[cfg(debug_assertions)]
            (
                "finish_panics_in_debug_on_unrestored_save",
                recording::finish_panics_in_debug_on_unrestored_save,
            ),
            (
                "isolated_append_preserves_bounds_and_scopes_the_run",
                recording::isolated_append_preserves_bounds_and_scopes_the_run,
            ),
            (
                "interning_shares_arc_for_identical_paints",
                recording::interning_shares_arc_for_identical_paints,
            ),
            (
                "a_finished_display_list_is_sendable_to_another_thread",
                recording::a_finished_display_list_is_sendable_to_another_thread,
            ),
        ],
    );
}

#[test]
fn damage_extent_contract() {
    run_cases(
        "damage_extent",
        &[
            (
                "a_color_fill_makes_the_extent_unbounded",
                damage_extent::a_color_fill_makes_the_extent_unbounded,
            ),
            (
                "paragraph_extent_covers_every_rasterized_glyph",
                damage_extent::paragraph_extent_covers_every_rasterized_glyph,
            ),
            (
                "stroke_and_shadow_extents_cover_their_outsets",
                damage_extent::stroke_and_shadow_extents_cover_their_outsets,
            ),
            (
                "shadow_extent_spreads_by_the_largest_scale_on_both_axes",
                damage_extent::shadow_extent_spreads_by_the_largest_scale_on_both_axes,
            ),
            (
                "fill_style_lines_and_points_reach_their_stroke_width",
                damage_extent::fill_style_lines_and_points_reach_their_stroke_width,
            ),
            (
                "atlas_extent_covers_the_sprite_destination",
                damage_extent::atlas_extent_covers_the_sprite_destination,
            ),
            (
                "a_bounded_save_layer_extent_is_its_mapped_bounds",
                damage_extent::a_bounded_save_layer_extent_is_its_mapped_bounds,
            ),
            (
                "transparent_source_and_transparent_black_predicates",
                damage_extent::transparent_source_and_transparent_black_predicates,
            ),
        ],
    );
}

#[test]
fn decoration_contract() {
    run_cases(
        "decoration",
        &[
            (
                "decoration_unit::circular_hit_testing_preserves_finite_distance_ranges",
                decoration_unit::circular_hit_testing_preserves_finite_distance_ranges as fn(),
            ),
            (
                "hidden_uniform_rectangle_borders_do_not_paint",
                decoration_unit::hidden_uniform_rectangle_borders_do_not_paint,
            ),
            (
                "hidden_uniform_circle_borders_do_not_paint",
                decoration_unit::hidden_uniform_circle_borders_do_not_paint,
            ),
            (
                "hidden_table_borders_do_not_paint",
                decoration_unit::hidden_table_borders_do_not_paint,
            ),
            (
                "hidden_edges_do_not_shorten_visible_neighboring_edges",
                decoration_unit::hidden_edges_do_not_shorten_visible_neighboring_edges,
            ),
            (
                "paint_order_is_shadow_background_border",
                decoration_unit::paint_order_is_shadow_background_border,
            ),
            (
                "hit_test_respects_rounded_corners",
                decoration_unit::hit_test_respects_rounded_corners,
            ),
            (
                "circle_uniform_border_is_a_stroked_circle_not_a_drrect",
                decoration_unit::circle_uniform_border_is_a_stroked_circle_not_a_drrect,
            ),
        ],
    );
}

#[test]
fn text_contract() {
    run_cases(
        "text",
        &[
            ("exact_profiles_reach_every_measurement_and_painted_run", text_layout_pipeline::exact_profiles_reach_every_measurement_and_painted_run),
            ("exact_frontier_is_complete_and_does_not_publish_stale_geometry", text_layout_pipeline::exact_frontier_is_complete_and_does_not_publish_stale_geometry),
            ("captured_answers_reach_retained_clones_without_reshaping_old_geometry", text_layout_pipeline::captured_answers_reach_retained_clones_without_reshaping_old_geometry),
            ("captured_live_geometry_rejects_conflicts_after_warm_eviction", text_layout_pipeline::captured_live_geometry_rejects_conflicts_after_warm_eviction),
            ("captured_cohorts_converge_beyond_warm_capacity_and_retire_history", text_layout_pipeline::captured_cohorts_converge_beyond_warm_capacity_and_retire_history),
            ("captured_cohorts_do_not_pin_other_attempts_history", text_layout_pipeline::captured_cohorts_do_not_pin_other_attempts_history),
            ("captured_attempts_retain_nested_sources_across_frontiers", text_layout_pipeline::captured_attempts_retain_nested_sources_across_frontiers),
            ("fixed_and_linear_attempts_retain_multiple_nested_sources", text_layout_pipeline::fixed_and_linear_attempts_retain_multiple_nested_sources),
            ("captured_debug_releases_infrastructure_before_formatter_reentry", text_layout_pipeline::captured_debug_releases_infrastructure_before_formatter_reentry),
            ("captured_cache_hits_pin_the_current_attempt_before_font_invalidation", text_layout_pipeline::captured_cache_hits_pin_the_current_attempt_before_font_invalidation),
            (
                "changing_to_an_invalid_scale_cannot_reuse_successful_geometry",
                text_layout_pipeline::changing_to_an_invalid_scale_cannot_reuse_successful_geometry,
            ),
            (
                "authored_font_size_overflow_is_an_ordinary_error",
                text_layout_pipeline::authored_font_size_overflow_is_an_ordinary_error,
            ),
            (
                "scaled_font_size_overflow_is_an_ordinary_error",
                text_layout_pipeline::scaled_font_size_overflow_is_an_ordinary_error,
            ),
            (
                "font_size_narrowing_to_zero_is_an_ordinary_error",
                text_layout_pipeline::font_size_narrowing_to_zero_is_an_ordinary_error,
            ),
            ("root_word_spacing_reaches_measurement_paint_and_carets", text_painter_unit::root_word_spacing_reaches_measurement_paint_and_carets),
            ("span_word_spacing_reaches_measurement_paint_and_carets", text_painter_unit::span_word_spacing_reaches_measurement_paint_and_carets),
            ("font_features_change_the_measured_and_painted_glyphs", text_painter_unit::font_features_change_the_measured_and_painted_glyphs),
            ("invalid_font_features_do_not_replace_valid_settings", text_painter_unit::invalid_font_features_do_not_replace_valid_settings),
            ("font_variations_select_the_painted_run_instance", text_painter_unit::font_variations_select_the_painted_run_instance),
            ("text_weight_adjustment_shapes_once_and_restores_authored_weights", text_painter_unit::text_weight_adjustment_shapes_once_and_restores_authored_weights),
            ("invalid_font_variations_do_not_replace_valid_settings", text_painter_unit::invalid_font_variations_do_not_replace_valid_settings),
            (
                "styled_text_pipeline",
                text_layout_pipeline::full_pipeline_with_styled_text,
            ),
            (
                "wide_ellipsis_floors_min_intrinsic_width",
                text_painter_unit::wide_ellipsis_floors_min_intrinsic_width,
            ),
            (
                "a_rich_span_ellipsis_floors_min_intrinsic_width",
                text_painter_unit::a_rich_span_ellipsis_floors_min_intrinsic_width,
            ),
            (
                "an_empty_paragraph_measures_a_line_of_its_style",
                text_painter_unit::an_empty_paragraph_measures_a_line_of_its_style,
            ),
            (
                "truncated_paragraph_paints_what_it_measured",
                text_overflow_unit::a_truncated_paragraph_paints_exactly_the_lines_it_measured,
            ),
            (
                "root_recolor_keeps_the_shaped_buffer",
                text_overflow_unit::root_recolor_keeps_the_shaped_buffer_and_span_recolor_reshapes_once,
            ),
            (
                "bidirectional_text_lays_out",
                rich_text_example::example_bidirectional_text,
            ),
        ],
    );
}

/// A painter measures on Parley through the context it is lent, and its cache
/// answers only for the fonts that measured it (ADR-0092 §10 steps 3a and 4a);
/// UI runtimes' contexts over one collection shape in parallel and share its faces
/// (§2–§3).
#[test]
fn text_context_contract() {
    use text_painter_unit::parley_measurement as pm;
    run_cases(
        "text_context",
        &[
            (
                "aligned_extreme_text_keeps_finite_paint_bounds",
                text_context::aligned_extreme_text_keeps_finite_paint_bounds,
            ),
            (
                "direct_shape_zero_size_is_an_ordinary_error",
                text_context::direct_shape_zero_size_is_an_ordinary_error,
            ),
            (
                "direct_shape_negative_size_is_an_ordinary_error",
                text_context::direct_shape_negative_size_is_an_ordinary_error,
            ),
            (
                "direct_shape_nonfinite_size_is_an_ordinary_error",
                text_context::direct_shape_nonfinite_size_is_an_ordinary_error,
            ),
            (
                "direct_shape_finite_size_cannot_publish_nonfinite_metrics",
                text_context::direct_shape_finite_size_cannot_publish_nonfinite_metrics,
            ),
            (
                "measurement_follows_the_context_it_is_given",
                pm::measurement_follows_the_context_it_is_given,
            ),
            (
                "intrinsic_widths_follow_the_context_they_are_asked_through",
                pm::intrinsic_widths_follow_the_context_they_are_asked_through,
            ),
            (
                "a_registration_on_the_collection_invalidates_the_painter_cache",
                pm::a_registration_on_the_collection_invalidates_the_painter_cache,
            ),
            (
                "collection_handles_are_shared_and_counted",
                text_context::collection_handles_are_shared_and_counted,
            ),
            (
                "two_ui_runtimes_shape_in_parallel",
                text_context::two_ui_runtimes_shape_in_parallel,
            ),
            (
                "a_face_registered_after_the_fork_shapes_in_every_ui_runtime",
                text_context::a_face_registered_after_the_fork_shapes_in_every_ui_runtime,
            ),
        ],
    );
}
