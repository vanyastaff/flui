//! Public-API contracts of the compositor tree.

#[path = "annotations.rs"]
mod annotations;
#[path = "follower.rs"]
mod follower;
#[path = "inspect.rs"]
mod inspect;
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
