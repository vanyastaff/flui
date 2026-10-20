//! `WindowMetrics`: a drawable area is at least a pixel in each extent at a finite, positive
//! ratio, and the device size a platform reports comes back unchanged.

use flui_foundation::geometry::{DeviceSize, InvalidMetrics, Size, WindowMetrics};

fn physical_size_survives_a_fractional_scale() {
    // 115 / 1.75 = 65.714…; truncating the logical size back to pixels gave 114.
    let metrics = WindowMetrics::new(DeviceSize::new(115, 201), 1.75).expect("valid metrics");
    assert_eq!(metrics.physical(), DeviceSize::new(115, 201));
    let rebuilt = WindowMetrics::from_logical(metrics.logical(), 1.75).expect("valid metrics");
    assert_eq!(rebuilt.physical(), DeviceSize::new(115, 201));
}

fn logical_size_is_the_device_size_over_the_ratio() {
    let metrics = WindowMetrics::new(DeviceSize::new(300, 150), 1.5).expect("valid metrics");
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
    let rows: [Row; 11] = [
        ("zero width", WindowMetrics::new(size(0, 10), 1.0), |e| {
            matches!(
                e,
                InvalidMetrics::EmptyExtent {
                    width: 0,
                    height: 10
                }
            )
        }),
        ("zero height", WindowMetrics::new(size(10, 0), 1.0), |e| {
            matches!(e, InvalidMetrics::EmptyExtent { .. })
        }),
        (
            "negative width",
            WindowMetrics::new(size(-1, 10), 1.0),
            |e| matches!(e, InvalidMetrics::EmptyExtent { width: -1, .. }),
        ),
        ("zero scale", WindowMetrics::new(size(10, 10), 0.0), |e| {
            matches!(e, InvalidMetrics::InvalidScale(_))
        }),
        (
            "negative scale",
            WindowMetrics::new(size(10, 10), -2.0),
            |e| matches!(e, InvalidMetrics::InvalidScale(_)),
        ),
        (
            "nan scale",
            WindowMetrics::new(size(10, 10), f64::NAN),
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
            "scale beyond device pixels",
            WindowMetrics::from_logical(Size::new(1.0e6, 10.0), 1.0e6),
            |e| matches!(e, InvalidMetrics::Overflow { .. }),
        ),
    ];
    for (what, metrics, cause) in rows {
        let error = metrics.expect_err(what);
        assert!(cause(&error), "{what}: {error:?}");
    }
}

#[test]
fn window_metrics_contract() {
    crate::run_table(&[
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
