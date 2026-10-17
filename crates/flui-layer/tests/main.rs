//! Public-API contracts of the compositor tree.

#[path = "annotations.rs"]
mod annotations;
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
                "other_types_and_regions_elsewhere_are_skipped",
                a::other_types_and_regions_elsewhere_are_skipped,
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
        ],
    );
}
