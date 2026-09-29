//! Table-driven contract matrices: one test per capability family, one row per case.

// Layout failure matrix: poisoning after bounded retries, recovery on fresh
/// invalidation, transient failures, re-entrant cycles, panic containment,
/// dirty-root preservation, cross-protocol children and invalid geometry.
#[test]
fn layout_failure_matrix() {
    let cases: &[(&str, fn())] = &[
        ("layout_poison::permanent_structural_failure_is_poisoned_after_bounded_retries", crate::layout_poison::permanent_structural_failure_is_poisoned_after_bounded_retries),
        ("layout_poison::fresh_invalidation_lifts_poison_and_layout_recovers", crate::layout_poison::fresh_invalidation_lifts_poison_and_layout_recovers),
        ("layout_poison::single_transient_failure_does_not_poison", crate::layout_poison::single_transient_failure_does_not_poison),
        ("layout_poison::a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero", crate::layout_poison::a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero),
        ("layout_cycle_guard::callback_reentry_poisons_structural_cycle", crate::layout_cycle_guard::callback_reentry_poisons_structural_cycle),
        ("layout_cycle_guard::drop_guard_clears_id_on_perform_layout_panic", crate::layout_cycle_guard::drop_guard_clears_id_on_perform_layout_panic),
        ("layout_dirty_root::descendant_err_preserves_parent_needs_layout", crate::layout_dirty_root::descendant_err_preserves_parent_needs_layout),
        ("cross_protocol_layout::cross_protocol_layout_sliver_child_on_box_child_returns_zero_and_poisons", crate::cross_protocol_layout::cross_protocol_layout_sliver_child_on_box_child_returns_zero_and_poisons),
        ("sliver_geometry_validation::sliver_leaf_layout_rejects_invalid_geometry_before_state_commit", crate::sliver_geometry_validation::sliver_leaf_layout_rejects_invalid_geometry_before_state_commit),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Frame failure matrix: paint refuses to start with layout pending, a panicking
/// effect descriptor poisons the paint phase, a failed pass keeps a real repaint.
#[test]
fn frame_failure_matrix() {
    let cases: &[(&str, fn())] = &[
        ("paint_before_layout::run_paint_refuses_to_start_with_layout_work_pending", crate::paint_before_layout::run_paint_refuses_to_start_with_layout_work_pending),
        ("effect_descriptor_poison::a_panicking_descriptor_under_a_repaint_poisons_the_frame_in_the_paint_phase", crate::effect_descriptor_poison::a_panicking_descriptor_under_a_repaint_poisons_the_frame_in_the_paint_phase),
        ("retained_boundary_layers::a_failed_pass_does_not_downgrade_a_real_repaint_to_an_update", crate::retained_boundary_layers::a_failed_pass_does_not_downgrade_a_real_repaint_to_an_update),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Lifecycle matrix: attach, dispose eviction, churn generations, structural
/// invalidation.
#[test]
fn lifecycle_matrix() {
    let cases: &[(&str, fn())] = &[
        ("attach_detach_lifecycle::insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id", crate::attach_detach_lifecycle::insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id),
        ("dispose_eviction::removing_a_subtree_evicts_its_dirty_entries", crate::dispose_eviction::removing_a_subtree_evicts_its_dirty_entries),
        ("pipeline_scenarios::repeated_churn_cycles_stay_clean_and_generations_protect_every_round", crate::pipeline_scenarios::repeated_churn_cycles_stay_clean_and_generations_protect_every_round),
        ("structural_invalidation::pure_reorder_marks_layout_only", crate::structural_invalidation::pure_reorder_marks_layout_only),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Semantics assembly matrix: clipping, exclusion, explicit child nodes, merge
/// boundaries and identity stability under reorder.
#[test]
fn semantics_assembly_matrix() {
    let cases: &[(&str, fn())] = &[
        ("semantics_assembly::a_semantics_clip_drops_a_child_that_falls_entirely_outside_it", crate::semantics_assembly::a_semantics_clip_drops_a_child_that_falls_entirely_outside_it),
        ("semantics_assembly::excludes_semantics_subtree_drops_descendant_content", crate::semantics_assembly::excludes_semantics_subtree_drops_descendant_content),
        ("semantics_assembly::explicit_child_nodes_forms_direct_contributors", crate::semantics_assembly::explicit_child_nodes_forms_direct_contributors),
        ("semantics_assembly::merge_semantics_boundary_collapses_plain_children_into_one_node", crate::semantics_assembly::merge_semantics_boundary_collapses_plain_children_into_one_node),
        ("semantics_assembly::sibling_insert_and_reorder_preserve_existing_accessibility_ids", crate::semantics_assembly::sibling_insert_and_reorder_preserve_existing_accessibility_ids),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Paint matrix: fragment snapshots, device pixel ratio, animated opacity and
/// retained boundary layers.
#[test]
fn paint_matrix() {
    let cases: &[(&str, fn())] = &[
        ("paint_fragment_snapshot::clip_rect_object_brackets_child_in_clip_layer", crate::paint_fragment_snapshot::clip_rect_object_brackets_child_in_clip_layer),
        ("paint_fragment_snapshot::inline_siblings_merge_into_one_origin_baked_picture", crate::paint_fragment_snapshot::inline_siblings_merge_into_one_origin_baked_picture),
        ("paint_fragment_snapshot::repaint_boundary_child_splits_into_rebased_offset_layer", crate::paint_fragment_snapshot::repaint_boundary_child_splits_into_rebased_offset_layer),
        ("dpr_pipeline::paint_root_carries_the_dpr_scale_and_ops_stay_logical", crate::dpr_pipeline::paint_root_carries_the_dpr_scale_and_ops_stay_logical),
        ("animation_pipeline::animated_opacity_layer_follows_and_zero_alpha_skips", crate::animation_pipeline::animated_opacity_layer_follows_and_zero_alpha_skips),
        ("retained_boundary_layers::a_retained_frame_matches_what_a_full_repaint_produces", crate::retained_boundary_layers::a_retained_frame_matches_what_a_full_repaint_produces),
        ("retained_boundary_layers::an_alpha_change_updates_the_layer_without_repainting_the_subtree", crate::retained_boundary_layers::an_alpha_change_updates_the_layer_without_repainting_the_subtree),
        ("retained_boundary_layers::the_content_of_a_clean_boundary_is_not_repainted", crate::retained_boundary_layers::the_content_of_a_clean_boundary_is_not_repainted),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Layout protocol matrix: intrinsics memoization, viewport and sliver layout.
#[test]
fn layout_protocol_matrix() {
    let cases: &[(&str, fn())] = &[
        ("intrinsics_cache::intrinsic_walk_memoizes_every_level", crate::intrinsics_cache::intrinsic_walk_memoizes_every_level),
        ("render_viewport::viewport_lays_out_forward_slivers_and_applies_content_dimensions", crate::render_viewport::viewport_lays_out_forward_slivers_and_applies_content_dimensions),
        ("render_viewport::viewport_positions_first_sliver_for_axis_and_growth_matrix", crate::render_viewport::viewport_positions_first_sliver_for_axis_and_growth_matrix),
        ("sliver_direction_matrix::sliver_direction_matrix_eight_by_three", crate::sliver_direction_matrix::sliver_direction_matrix_eight_by_three),
        ("sliver_fill_remaining::sliver_fill_remaining_with_scrollable_sizes_child_to_remaining_paint_extent", crate::sliver_fill_remaining::sliver_fill_remaining_with_scrollable_sizes_child_to_remaining_paint_extent),
        ("sliver_fixed_extent_list::sliver_fixed_extent_list_sizes_children_to_item_extent", crate::sliver_fixed_extent_list::sliver_fixed_extent_list_sizes_children_to_item_extent),
        ("sliver_grid::sliver_grid_golden_geometry", crate::sliver_grid::sliver_grid_golden_geometry),
        ("sliver_to_box_adapter::sliver_to_box_adapter_lays_out_box_child_and_commits_geometry", crate::sliver_to_box_adapter::sliver_to_box_adapter_lays_out_box_child_and_commits_geometry),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

// Hit-test matrix: box protocol hit paths, sliver hit directions, transform
/// accumulation.
#[test]
fn hit_test_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "hit_test_pipeline::flex_lays_out_and_hits_children_at_layout_offsets",
            crate::hit_test_pipeline::flex_lays_out_and_hits_children_at_layout_offsets,
        ),
        (
            "hit_test_pipeline::padding_child_hits_leaf_first_at_laid_out_offset",
            crate::hit_test_pipeline::padding_child_hits_leaf_first_at_laid_out_offset,
        ),
        (
            "hit_test_pipeline::transform_child_hits_through_inverse_matrix",
            crate::hit_test_pipeline::transform_child_hits_through_inverse_matrix,
        ),
        (
            "sliver_hit_direction_matrix::sliver_hit_direction_matrix_through_box_host",
            crate::sliver_hit_direction_matrix::sliver_hit_direction_matrix_through_box_host,
        ),
        (
            "transform_to::transform_to_accumulates_offsets_through_a_plain_chain",
            crate::transform_to::transform_to_accumulates_offsets_through_a_plain_chain,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}
