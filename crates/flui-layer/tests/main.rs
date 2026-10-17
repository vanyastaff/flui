//! Public-API contracts of the compositor tree.

#[path = "annotations.rs"]
mod annotations;

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
