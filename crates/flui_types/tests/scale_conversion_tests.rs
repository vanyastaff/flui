//! Integration tests for type-safe scale conversions in geometry types

use flui_types::geometry::{
    device_px, px, DevicePixels, Offset, Pixels, Point, Rect, ScaleFactor, Size,
};

#[test]
fn test_point_scale_with() {
    let logical = Point::new(px(100.0), px(200.0));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let device = logical.scale_with(scale);

    assert_eq!(device.x.get(), 200);
    assert_eq!(device.y.get(), 400);
}

#[test]
fn test_point_unscale() {
    let device = Point::new(device_px(200), device_px(400));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let logical = device.unscale(scale);

    assert_eq!(logical.x, px(100.0));
    assert_eq!(logical.y, px(200.0));
}

#[test]
fn test_size_scale_with() {
    let logical = Size::new(px(100.0), px(200.0));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let device = logical.scale_with(scale);

    assert_eq!(device.width.get(), 200);
    assert_eq!(device.height.get(), 400);
}

#[test]
fn test_size_unscale() {
    let device = Size::new(device_px(200), device_px(400));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let logical = device.unscale(scale);

    assert_eq!(logical.width, px(100.0));
    assert_eq!(logical.height, px(200.0));
}

#[test]
fn test_rect_scale_with() {
    let logical = Rect::from_points(
        Point::new(px(10.0), px(20.0)),
        Point::new(px(110.0), px(220.0)),
    );
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let device = logical.scale_with(scale);

    assert_eq!(device.min.x.get(), 20);
    assert_eq!(device.min.y.get(), 40);
    assert_eq!(device.max.x.get(), 220);
    assert_eq!(device.max.y.get(), 440);
}

#[test]
fn test_rect_unscale() {
    let device = Rect::from_points(
        Point::new(device_px(20), device_px(40)),
        Point::new(device_px(220), device_px(440)),
    );
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let logical = device.unscale(scale);

    assert_eq!(logical.min.x, px(10.0));
    assert_eq!(logical.min.y, px(20.0));
    assert_eq!(logical.max.x, px(110.0));
    assert_eq!(logical.max.y, px(220.0));
}

#[test]
fn test_offset_scale_with() {
    let logical = Offset::new(px(100.0), px(200.0));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let device = logical.scale_with(scale);

    assert_eq!(device.dx.get(), 200);
    assert_eq!(device.dy.get(), 400);
}

#[test]
fn test_offset_unscale() {
    let device = Offset::new(device_px(200), device_px(400));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
    let logical = device.unscale(scale);

    assert_eq!(logical.dx, px(100.0));
    assert_eq!(logical.dy, px(200.0));
}

#[test]
fn test_roundtrip_conversions() {
    // Test that scale -> unscale gives back reasonably close values
    // Note: DevicePixels uses i32 internally, so rounding occurs during conversion
    let original_point = Point::new(px(100.0), px(200.0));
    let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);

    let device = original_point.scale_with(scale);
    let roundtrip = device.unscale(scale);

    // With integer device pixels and rounding, we should get exact match for whole numbers
    assert_eq!(roundtrip.x, px(100.0));
    assert_eq!(roundtrip.y, px(200.0));

    // Test with fractional values - allow rounding error
    let fractional = Point::new(px(123.456), px(789.012));
    let device_frac = fractional.scale_with(scale);
    let roundtrip_frac = device_frac.unscale(scale);

    // DevicePixels rounds, so we expect ~0.5 pixel error maximum
    assert!((roundtrip_frac.x.get() - fractional.x.get()).abs() < 0.5);
    assert!((roundtrip_frac.y.get() - fractional.y.get()).abs() < 0.5);
}
