//! A backend projects window-logical input regions to device pixels once, rounding outward,
//! drops a region whose device edges or spans are not `i32`, and hands the OS a set only when
//! the projected pixels, the scale or the display changed.

use flui_foundation::geometry::{DevicePoint, DeviceRect, Rect};
use flui_platform::input_regions::{InputRegion, InputRegionKind};
use flui_platform::{DisplayKey, DisplayRegistry};

use super::{DeviceInputRegion, ProjectedInputRegions};
use crate::table_test::run_table;

const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

fn region(left: f64, top: f64, right: f64, bottom: f64) -> InputRegion {
    InputRegion::new(EXCLUSION, Rect::from_ltrb(left, top, right, bottom))
}

fn device(left: i32, top: i32, right: i32, bottom: i32) -> DeviceInputRegion {
    DeviceInputRegion::new(
        EXCLUSION,
        DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
    )
}

/// What the OS receives for `regions` projected at scale one on no display.
fn sent_at_one(regions: &[InputRegion]) -> Option<Vec<DeviceInputRegion>> {
    ProjectedInputRegions::default()
        .update(regions, 1.0, None)
        .map(<[DeviceInputRegion]>::to_vec)
}

const MAX: f64 = i32::MAX as f64;
const MIN: f64 = i32::MIN as f64;

fn device_edges_just_inside_the_i32_range_are_kept() {
    assert_eq!(
        sent_at_one(&[region(MAX - 10.0, MIN, MAX, MIN + 10.0)]),
        Some(vec![device(
            i32::MAX - 10,
            i32::MIN,
            i32::MAX,
            i32::MIN + 10
        )]),
    );
}

fn device_edges_outside_the_i32_range_drop_the_region() {
    // The right edge rounds up past `i32::MAX`; a saturating cast would keep a
    // narrower region than the one asked for.
    let past_max = region(MAX - 10.0, 0.0, MAX + 0.5, 10.0);
    let past_min = region(0.0, MIN - 1.0, 10.0, 0.0);
    let kept = region(0.0, 0.0, 4.0, 4.0);
    assert_eq!(
        sent_at_one(&[past_max, past_min, kept]),
        Some(vec![device(0, 0, 4, 4)]),
    );
    // Overflow in the device scale is not clamped from infinity either.
    let mut projected = ProjectedInputRegions::default();
    assert_eq!(
        projected.update(&[region(0.0, 0.0, f64::MAX, 4.0), kept], 2.0, None),
        Some(&[device(0, 0, 8, 8)][..]),
    );
}

fn edges_inside_the_range_whose_span_exceeds_i32_drop_the_region() {
    let wide = region(-2.0e9, 0.0, 2.0e9, 10.0);
    let tall = region(0.0, -2.0e9, 10.0, 2.0e9);
    let kept = region(0.0, 0.0, 4.0, 4.0);
    assert_eq!(
        sent_at_one(&[wide, tall, kept]),
        Some(vec![device(0, 0, 4, 4)])
    );
}

fn fractional_region_edges_round_outward() {
    // 0.2 logical px at 1.5 → [0.15, 0.45]: one whole pixel, not none.
    let mut projected = ProjectedInputRegions::default();
    assert_eq!(
        projected.update(&[region(10.1, 0.0, 10.3, 7.0)], 1.5, None),
        Some(&[device(15, 0, 16, 11)][..]),
    );
}

fn unchanged_regions_are_not_sent_again() {
    let mut projected = ProjectedInputRegions::default();
    let regions = [region(0.0, 0.0, 10.0, 10.0)];
    assert!(projected.update(&regions, 1.0, None).is_some());
    assert_eq!(projected.update(&regions, 1.0, None), None);
    // A logical change that projects to the same pixels is not a change for the OS.
    assert_eq!(
        projected.update(&[region(0.0, 0.0, 9.5, 10.0)], 1.0, None),
        None
    );
}

fn nothing_is_sent_until_there_is_something_to_exclude() {
    let mut projected = ProjectedInputRegions::default();
    assert_eq!(projected.update(&[], 1.0, None), None);
    assert!(
        projected
            .update(&[region(0.0, 0.0, 1.0, 1.0)], 1.0, None)
            .is_some()
    );
    // Clearing what the OS holds is a change.
    assert_eq!(projected.update(&[], 1.0, None), Some(&[][..]));
}

fn a_scale_or_display_change_resends_equal_pixels() {
    let mut projected = ProjectedInputRegions::default();
    let regions = [region(0.0, 0.0, 10.0, 10.0)];
    let registry = DisplayRegistry::new();
    let mint = |key| {
        registry
            .mint(&DisplayKey::Injected(key))
            .expect("display id")
    };
    let display = Some(mint(1));
    assert!(projected.update(&regions, 1.0, display).is_some());
    assert!(
        projected.update(&regions, 1.0, Some(mint(2))).is_some(),
        "another display at an equal scale"
    );
    assert!(
        projected.update(&regions, 1.0, None).is_some(),
        "Some to None"
    );
    assert!(
        projected.update(&regions, 1.0, display).is_some(),
        "None to Some"
    );
    // Same pixels for an integral region at a different scale only when the
    // logical set changed with it; the key still records the scale.
    assert!(
        projected
            .update(&[region(0.0, 0.0, 5.0, 5.0)], 2.0, display)
            .is_some()
    );
}

fn an_unusable_scale_keeps_what_the_os_holds() {
    let mut projected = ProjectedInputRegions::default();
    let regions = [region(0.0, 0.0, 10.0, 10.0)];
    assert!(projected.update(&regions, 1.0, None).is_some());
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(projected.update(&[], scale, None), None, "scale {scale}");
    }
    assert_eq!(projected.sent(), &[device(0, 0, 10, 10)]);
}

#[test]
fn input_region_projection() {
    run_table(
        "input_region_projection",
        &[
            (
                "device_edges_just_inside_the_i32_range_are_kept",
                device_edges_just_inside_the_i32_range_are_kept,
            ),
            (
                "device_edges_outside_the_i32_range_drop_the_region",
                device_edges_outside_the_i32_range_drop_the_region,
            ),
            (
                "edges_inside_the_range_whose_span_exceeds_i32_drop_the_region",
                edges_inside_the_range_whose_span_exceeds_i32_drop_the_region,
            ),
            (
                "fractional_region_edges_round_outward",
                fractional_region_edges_round_outward,
            ),
            (
                "unchanged_regions_are_not_sent_again",
                unchanged_regions_are_not_sent_again,
            ),
            (
                "nothing_is_sent_until_there_is_something_to_exclude",
                nothing_is_sent_until_there_is_something_to_exclude,
            ),
            (
                "a_scale_or_display_change_resends_equal_pixels",
                a_scale_or_display_change_resends_equal_pixels,
            ),
            (
                "an_unusable_scale_keeps_what_the_os_holds",
                an_unusable_scale_keeps_what_the_os_holds,
            ),
        ],
    );
}
