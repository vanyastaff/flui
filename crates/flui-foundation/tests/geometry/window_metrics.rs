//! `WindowMetrics`: a drawable area is at least a pixel in each extent at a finite, positive
//! ratio, and the device size a platform reports comes back unchanged.

use flui_foundation::geometry::{
    DevicePixelRatio, DeviceSize, InvalidMetrics, Size, WindowMetrics, device_extent,
};

fn physical_size_survives_a_fractional_scale() {
    // 115 / 1.75 = 65.714…; truncating the logical size back to pixels gave 114.
    let metrics = WindowMetrics::try_new(DeviceSize::new(115, 201), 1.75).expect("valid metrics");
    assert_eq!(metrics.physical(), DeviceSize::new(115, 201));
    let rebuilt = WindowMetrics::from_logical(metrics.logical(), 1.75).expect("valid metrics");
    assert_eq!(rebuilt.physical(), DeviceSize::new(115, 201));
}

fn logical_size_is_the_device_size_over_the_ratio() {
    let metrics = WindowMetrics::try_new(DeviceSize::new(300, 150), 1.5).expect("valid metrics");
    assert_eq!(metrics.logical(), Size::new(200.0, 100.0));
    assert!((metrics.scale().get() - 1.5).abs() < f64::EPSILON);
}

fn from_logical_rounds_each_extent_to_the_nearest_pixel() {
    let metrics = WindowMetrics::from_logical(Size::new(100.4, 100.6), 1.0).expect("valid metrics");
    assert_eq!(metrics.physical(), DeviceSize::new(100, 101));
}

type Row = (
    &'static str,
    Result<WindowMetrics, InvalidMetrics>,
    fn(&InvalidMetrics) -> bool,
);

fn a_report_that_is_not_a_drawable_area_names_its_cause() {
    let size = DeviceSize::new;
    let rows: [Row; 13] = [
        (
            "zero width",
            WindowMetrics::try_new(size(0, 10), 1.0),
            |e| {
                matches!(
                    e,
                    InvalidMetrics::EmptyExtent {
                        width: 0,
                        height: 10
                    }
                )
            },
        ),
        (
            "zero height",
            WindowMetrics::try_new(size(10, 0), 1.0),
            |e| matches!(e, InvalidMetrics::EmptyExtent { .. }),
        ),
        (
            "negative width",
            WindowMetrics::try_new(size(-1, 10), 1.0),
            |e| matches!(e, InvalidMetrics::EmptyExtent { width: -1, .. }),
        ),
        (
            "zero scale",
            WindowMetrics::try_new(size(10, 10), 0.0),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "negative scale",
            WindowMetrics::try_new(size(10, 10), -2.0),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "nan scale",
            WindowMetrics::try_new(size(10, 10), f64::NAN),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "nan logical",
            WindowMetrics::from_logical(Size::new(f64::NAN, 10.0), 1.0),
            |e| matches!(e, InvalidMetrics::NonFiniteSize { .. }),
        ),
        (
            "infinite logical",
            WindowMetrics::from_logical(Size::new(10.0, f64::INFINITY), 1.0),
            |e| matches!(e, InvalidMetrics::NonFiniteSize { .. }),
        ),
        (
            "logical below a pixel",
            WindowMetrics::from_logical(Size::new(0.4, 10.0), 1.0),
            |e| matches!(e, InvalidMetrics::EmptyExtent { width: 0, .. }),
        ),
        (
            "logical beyond device pixels",
            WindowMetrics::from_logical(Size::new(3.0e9, 10.0), 1.0),
            |e| matches!(e, InvalidMetrics::Overflow { .. }),
        ),
        (
            "a scale below the smallest a screen has",
            WindowMetrics::try_new(size(1, 1), f64::from_bits(1)),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "a scale past the largest a screen has",
            WindowMetrics::try_new(size(1, 1), 1.0e6),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "scale beyond device pixels",
            WindowMetrics::from_logical(Size::new(1.0e9, 10.0), 32.0),
            |e| matches!(e, InvalidMetrics::Overflow { .. }),
        ),
    ];
    for (what, metrics, cause) in rows {
        let error = metrics.expect_err(what);
        assert!(cause(&error), "{what}: {error:?}");
    }
}

fn a_ratio_is_held_to_the_range_a_screen_can_have() {
    let (min, max) = (DevicePixelRatio::MIN.get(), DevicePixelRatio::MAX.get());
    // (case, ratio, taken)
    for (case, ratio, taken) in [
        ("at the minimum", min, true),
        ("just inside the minimum", min * 1.001, true),
        ("just outside the minimum", min * 0.999, false),
        ("a subnormal", 1e-310, false),
        ("the base ratio", 1.0, true),
        ("just inside the maximum", max * 0.999, true),
        ("at the maximum", max, true),
        ("just outside the maximum", max * 1.001, false),
        ("far past it", 1e300, false),
        ("zero", 0.0, false),
        ("negative", -1.0, false),
        ("not a number", f64::NAN, false),
        ("infinite", f64::INFINITY, false),
    ] {
        assert_eq!(
            DevicePixelRatio::try_new(ratio).is_some(),
            taken,
            "{case}: {ratio}"
        );
    }
    // Over any ratio in the range, a logical size of a whole surface is finite.
    assert!(
        DevicePixelRatio::MIN
            .to_logical(f64::from(i32::MAX))
            .is_finite()
    );
}

fn a_surface_side_is_snapped_to_whole_device_pixels() {
    let ratio = |ratio: f64| DevicePixelRatio::try_new(ratio).expect("a ratio in range");
    // (case, logical, ratio, device)
    for (case, logical, scale, device) in [
        ("whole", 800.0, 1.0, Some(800)),
        // 601.35: the nearest pixel, not the next one up.
        ("a fraction below the half", 400.9, 1.5, Some(601)),
        ("a half rounds up", 0.5, 5.0, Some(3)),
        ("a sliver is a pixel", 0.01, 1.0, Some(1)),
        ("past u32 saturates", 1e12, 32.0, Some(u32::MAX)),
        ("no size", 0.0, 1.0, None),
        ("negative", -1.0, 1.0, None),
        ("not a number", f64::NAN, 1.0, None),
        ("infinite", f64::INFINITY, 1.0, None),
    ] {
        assert_eq!(device_extent(logical, ratio(scale)), device, "{case}");
    }
}

#[test]
fn window_metrics_contract() {
    crate::run_table(&[
        (
            "a surface side is snapped to whole device pixels",
            a_surface_side_is_snapped_to_whole_device_pixels,
        ),
        (
            "a ratio is held to the range a screen can have",
            a_ratio_is_held_to_the_range_a_screen_can_have,
        ),
        (
            "physical size survives a fractional scale",
            physical_size_survives_a_fractional_scale,
        ),
        (
            "logical size is the device size over the ratio",
            logical_size_is_the_device_size_over_the_ratio,
        ),
        (
            "from_logical rounds each extent to the nearest pixel",
            from_logical_rounds_each_extent_to_the_nearest_pixel,
        ),
        (
            "a report that is not a drawable area names its cause",
            a_report_that_is_not_a_drawable_area_names_its_cause,
        ),
    ]);
}
