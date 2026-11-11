//! Public-API contracts of the compositor tree.

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
#[path = "annotations.rs"]
mod annotations;
#[path = "follower.rs"]
mod follower;
#[path = "inspect.rs"]
mod inspect;
#[path = "region.rs"]
mod region;
#[path = "scene.rs"]
mod scene;
#[path = "scene_damage.rs"]
mod scene_damage;
#[path = "texture_registry.rs"]
mod texture_registry;

/// Each row is a plain function; a failing row is named.
fn run_rows(family: &str, rows: &[(&str, fn())]) {
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{family}: failed rows {failed:?}");
}

/// A scene reports the annotations it places at a point, topmost first,
/// through offsets, transforms, followers and clips.
#[test]
fn annotation_lookup() {
    use annotations as a;
    run_rows(
        "annotation_lookup",
        &[
            ("topmost_first", a::topmost_first),
            (
                "offsets_and_transforms_map_the_point",
                a::offsets_and_transforms_map_the_point,
            ),
            (
                "clips_hide_what_they_exclude",
                a::clips_hide_what_they_exclude,
            ),
            (
                "singular_transforms_hide_their_subtree",
                a::singular_transforms_hide_their_subtree,
            ),
            (
                "perspective_hides_what_lies_behind_the_camera",
                a::perspective_hides_what_lies_behind_the_camera,
            ),
            (
                "other_types_and_regions_elsewhere_are_skipped",
                a::other_types_and_regions_elsewhere_are_skipped,
            ),
        ],
    );
}

/// A follower lands on its leader through every transform on both chains,
/// hides with its leader's clips when asked, and resolves to nothing rather
/// than to a degenerate transform.
#[test]
fn follower_resolution() {
    use follower as f;
    run_rows(
        "follower_resolution",
        &[
            (
                "linked_across_offset_branches_sums_both_chains",
                f::linked_across_offset_branches_sums_both_chains,
            ),
            (
                "a_scaled_leader_scales_the_follower",
                f::a_scaled_leader_scales_the_follower,
            ),
            (
                "a_rotated_leader_rotates_the_follower",
                f::a_rotated_leader_rotates_the_follower,
            ),
            (
                "a_follower_under_a_scaled_parent_undoes_its_scale",
                f::a_follower_under_a_scaled_parent_undoes_its_scale,
            ),
            (
                "a_leader_inside_another_follower_resolves_through_it",
                f::a_leader_inside_another_follower_resolves_through_it,
            ),
            (
                "a_singular_transform_on_a_chain_resolves_to_nothing",
                f::a_singular_transform_on_a_chain_resolves_to_nothing,
            ),
            (
                "a_non_finite_transform_on_a_chain_resolves_to_nothing",
                f::a_non_finite_transform_on_a_chain_resolves_to_nothing,
            ),
            (
                "followers_nested_past_the_limit_resolve_to_nothing",
                f::followers_nested_past_the_limit_resolve_to_nothing,
            ),
            (
                "a_cycle_through_leaders_resolves_to_nothing",
                f::a_cycle_through_leaders_resolves_to_nothing,
            ),
            (
                "hide_follows_the_anchor_through_the_leader_clip",
                f::hide_follows_the_anchor_through_the_leader_clip,
            ),
            (
                "a_clip_at_the_common_ancestor_is_not_consulted",
                f::a_clip_at_the_common_ancestor_is_not_consulted,
            ),
            (
                "a_clip_under_a_transform_tests_the_mapped_anchor",
                f::a_clip_under_a_transform_tests_the_mapped_anchor,
            ),
            (
                "a_scaled_follower_parent_keeps_the_affine_shape",
                f::a_scaled_follower_parent_keeps_the_affine_shape,
            ),
            (
                "the_clip_test_keeps_the_anchors_depth_through_the_chain",
                f::the_clip_test_keeps_the_anchors_depth_through_the_chain,
            ),
            (
                "an_unlinked_follower_takes_the_unlinked_contract",
                f::an_unlinked_follower_takes_the_unlinked_contract,
            ),
            (
                "leader_inside_the_follower_subtree_falls_back_to_unlinked",
                f::leader_inside_the_follower_subtree_falls_back_to_unlinked,
            ),
        ],
    );
}

/// External textures: unique ids, newest-frame delivery, retirement and
/// listeners.
#[test]
fn texture_registry() {
    use texture_registry as t;
    run_rows(
        "texture_registry",
        &[
            ("ids_are_never_reissued", t::ids_are_never_reissued),
            (
                "the_backend_takes_the_newest_frame_once",
                t::the_backend_takes_the_newest_frame_once,
            ),
            (
                "dropping_a_registration_retires_it",
                t::dropping_a_registration_retires_it,
            ),
            (
                "listeners_run_reentrantly_until_removed",
                t::listeners_run_reentrantly_until_removed,
            ),
            (
                "a_panicking_listener_keeps_the_frame_deliverable",
                t::a_panicking_listener_keeps_the_frame_deliverable,
            ),
            (
                "a_panicking_listener_drop_still_wakes_the_retirement",
                t::a_panicking_listener_drop_still_wakes_the_retirement,
            ),
        ],
    );
}

/// A damage region holds at most eight disjoint rects, merges a ninth into
/// the cheapest pair, and escalates a non-finite rect to `Full`, whose first
/// reason survives a union (ADR-0304 §5).
#[test]
fn region_contract() {
    use region as r;
    run_rows(
        "region_contract",
        &[
            (
                "a_ninth_rect_merges_the_cheapest_pair",
                r::a_ninth_rect_merges_the_cheapest_pair,
            ),
            (
                "merged_rects_stay_pairwise_disjoint",
                r::merged_rects_stay_pairwise_disjoint,
            ),
            (
                "a_non_finite_rect_escalates_to_full",
                r::a_non_finite_rect_escalates_to_full,
            ),
            (
                "full_keeps_its_first_reason",
                r::full_keeps_its_first_reason,
            ),
        ],
    );
}

/// A patch applies all or nothing: a cycle, a stale or unknown id, a node
/// named twice or a retired node still named refuses it and leaves the
/// version as it was (ADR-0304 §3).
#[test]
fn scene_apply_contract() {
    use scene as s;
    run_rows(
        "scene_apply_contract",
        &[
            (
                "a_slot_naming_its_own_ancestor_is_refused",
                s::a_slot_naming_its_own_ancestor_is_refused,
            ),
            (
                "a_stale_generation_is_refused",
                s::a_stale_generation_is_refused,
            ),
            (
                "a_node_named_by_two_slots_is_refused",
                s::a_node_named_by_two_slots_is_refused,
            ),
            (
                "a_refused_patch_leaves_the_version_and_log_unchanged",
                s::a_refused_patch_leaves_the_version_and_log_unchanged,
            ),
            (
                "an_id_never_published_is_unknown",
                s::an_id_never_published_is_unknown,
            ),
            (
                "a_node_of_another_owner_is_refused",
                s::a_node_of_another_owner_is_refused,
            ),
            (
                "the_root_stays_and_placements_are_finite",
                s::the_root_stays_and_placements_are_finite,
            ),
            (
                "the_indexes_follow_publish_and_retire",
                s::the_indexes_follow_publish_and_retire,
            ),
            (
                "a_version_flattens_to_the_tree_it_paints",
                s::a_version_flattens_to_the_tree_it_paints,
            ),
            ("a_version_is_send_and_sync", s::a_version_is_send_and_sync),
        ],
    );
}

/// Content versions never repeat across owners or lineages, and a spent
/// sequence renews the lineage instead of wrapping (ADR-0304 §2, §10).
#[test]
fn content_version_contract() {
    use scene as s;
    run_rows(
        "content_version_contract",
        &[
            (
                "a_replaced_owner_never_reports_unchanged",
                s::a_replaced_owner_never_reports_unchanged,
            ),
            (
                "an_exhausted_mint_renews_the_lineage_and_publishes_full",
                s::an_exhausted_mint_renews_the_lineage_and_publishes_full,
            ),
        ],
    );
}

/// An apply copies table nodes in proportion to its patch and the table's
/// height, not to the scene, and rewrites only the depths and indexes the
/// patch changes (ADR-0304 §1).
#[test]
fn scene_table_cost() {
    use scene as s;
    run_rows(
        "scene_table_cost",
        &[
            (
                "an_apply_copies_the_patch_not_the_scene",
                s::an_apply_copies_the_patch_not_the_scene,
            ),
            (
                "a_republished_parent_costs_its_change_not_its_slots",
                s::a_republished_parent_costs_its_change_not_its_slots,
            ),
            (
                "a_repaint_with_the_same_links_leaves_the_indexes",
                s::a_repaint_with_the_same_links_leaves_the_indexes,
            ),
            (
                "wrapping_a_subtree_rewrites_its_depths",
                s::wrapping_a_subtree_rewrites_its_depths,
            ),
        ],
    );
}

/// A patch damages the footprint of its operations under the version before
/// and after it, resolves followers after them and closes over what filters
/// read (ADR-0304 §4, §6).
#[test]
fn scene_patch_damage() {
    use scene_damage as d;
    run_rows(
        "scene_patch_damage",
        &[
            (
                "a_place_damages_the_subtree_under_both_placements",
                d::a_place_damages_the_subtree_under_both_placements,
            ),
            (
                "an_effect_chain_change_damages_every_slot_under_it",
                d::an_effect_chain_change_damages_every_slot_under_it,
            ),
            (
                "a_follower_resolves_after_its_leaders_boundary_moves_parents",
                d::a_follower_resolves_after_its_leaders_boundary_moves_parents,
            ),
            (
                "a_patch_that_changes_no_pixel_damages_nothing",
                d::a_patch_that_changes_no_pixel_damages_nothing,
            ),
            (
                "damage_under_a_backdrop_takes_what_it_reads",
                d::damage_under_a_backdrop_takes_what_it_reads,
            ),
            (
                "an_unattached_leader_never_shadows_the_attached_one",
                d::an_unattached_leader_never_shadows_the_attached_one,
            ),
            (
                "a_magnifier_damages_its_sampling_fringe",
                d::a_magnifier_damages_its_sampling_fringe,
            ),
            (
                "a_follower_at_the_nesting_limit_damages_what_it_draws",
                d::a_follower_at_the_nesting_limit_damages_what_it_draws,
            ),
            (
                "nested_filter_kernels_add_along_the_chain",
                d::nested_filter_kernels_add_along_the_chain,
            ),
            (
                "a_picture_effect_damages_its_damage_extent",
                d::a_picture_effect_damages_its_damage_extent,
            ),
            (
                "nested_followers_resolve_below_their_common_ancestor",
                d::nested_followers_resolve_below_their_common_ancestor,
            ),
            (
                "sibling_followers_pair_in_one_pass_over_their_links",
                d::sibling_followers_pair_in_one_pass_over_their_links,
            ),
            (
                "a_clip_on_the_leaders_branch_that_hides_its_follower_damages_it",
                d::a_clip_on_the_leaders_branch_that_hides_its_follower_damages_it,
            ),
            (
                "a_filter_under_a_shrinking_ancestor_damages_its_device_kernel",
                d::a_filter_under_a_shrinking_ancestor_damages_its_device_kernel,
            ),
        ],
    );
}

/// The patch damage covers the full diff on 10 000 random cases and misses
/// it once a patch drops one operation's footprint (ADR-0304 §11).
#[test]
fn patch_damage_covers_diff_damage() {
    scene_damage::patch_damage_covers_diff_damage();
}
