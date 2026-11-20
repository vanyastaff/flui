//! Pixels: the metric, baselines and their files, masks, stable captures and predicates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use flui_driver::pixels::metric::{self, MAX_YIQ_DELTA};
use flui_driver::pixels::{
    BaselineOptions, BaselineStatus, Baselines, Capture, CaptureSource, DeviceRect, Image,
    MASK_COLOR, Tolerance, UpdateMode,
};
use flui_driver::protocol::geometry::{LogicalPoint, LogicalRect};
use flui_driver::protocol::tree::Rect;
use flui_driver::testing::ScriptedLane;
use flui_driver::{Driver, Error, Isolation, Launch, Locator, TextMatch};

use crate::fixtures::{Counter, Probe};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

fn image(width: u32, height: u32, color: [u8; 4]) -> Image {
    Image::filled(width, height, color).expect("an image")
}

fn capture_of(image: Image) -> Capture {
    Capture::new(image, CaptureSource::Os, "scripted", 1.0)
}

#[test]
fn the_metric_measures_pixelmatchs_yiq_distance() {
    struct Case {
        name: &'static str,
        first: [u8; 4],
        second: [u8; 4],
        delta: f64,
    }
    // Each distance worked out from pixelmatch's formula by hand: blend over white by alpha,
    // then 0.5053 y² + 0.299 i² + 0.1957 q², negative when the first pixel is brighter.
    let cases = [
        Case {
            name: "the same pixel",
            first: [12, 34, 56, 255],
            second: [12, 34, 56, 255],
            delta: 0.0,
        },
        Case {
            name: "black against white: all brightness, no hue",
            first: BLACK,
            second: WHITE,
            delta: 0.5053 * 255.000_002_55_f64.powi(2),
        },
        Case {
            name: "white against black is the same distance, signed brighter-first",
            first: WHITE,
            second: BLACK,
            delta: -0.5053 * 255.000_002_55_f64.powi(2),
        },
        Case {
            name: "a transparent pixel is white",
            first: [0, 0, 0, 0],
            second: WHITE,
            delta: 0.0,
        },
        Case {
            name: "pure red against pure blue",
            first: [255, 0, 0, 255],
            second: [0, 0, 255, 255],
            delta: {
                let y = 255.0 * (0.298_895_31 - 0.114_482_23);
                let i = 255.0 * (0.595_977_99 + 0.321_801_89);
                let q = 255.0 * (0.211_470_17 - 0.311_146_94);
                -(0.5053 * y * y + 0.299 * i * i + 0.1957 * q * q)
            },
        },
    ];
    for case in cases {
        let delta = metric::color_delta(case.first, case.second);
        assert!(
            (delta - case.delta).abs() < 1e-6,
            "{}: {delta} against {}",
            case.name,
            case.delta
        );
    }
    // pixelmatch's normalizing constant bounds every distance; the largest is near it.
    let mut largest = 0.0_f64;
    for first in [
        BLACK,
        WHITE,
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
    ] {
        for second in [
            BLACK,
            WHITE,
            [255, 0, 255, 255],
            [255, 255, 0, 255],
            [0, 255, 255, 255],
        ] {
            largest = largest.max(metric::color_delta(first, second).abs());
        }
    }
    assert!(largest <= MAX_YIQ_DELTA, "{largest}");
    assert!(largest > 0.9 * MAX_YIQ_DELTA, "{largest}");
}

#[test]
fn the_threshold_lets_close_colours_pass_and_far_ones_fail() {
    let base = image(4, 4, BLACK);
    // At the default threshold 0.1 two greys pass while 0.5053 y² stays under 35215 * 0.01.
    let near = image(4, 4, [20, 20, 20, 255]);
    let far = image(4, 4, [30, 30, 30, 255]);
    let tolerance = Tolerance::default();
    let difference = metric::compare(&base, &near, &tolerance).expect("one size");
    assert_eq!(difference.differing, 0, "20 grey levels: {difference}");
    let difference = metric::compare(&base, &far, &tolerance).expect("one size");
    assert_eq!(difference.differing, 16, "30 grey levels: {difference}");
    assert!(!difference.within(&tolerance));
    assert!(difference.within(&Tolerance::default().max_diff_pixels(16)));
    assert!(difference.within(&Tolerance::default().max_diff_pixels(0).max_diff_ratio(1.0)));
    assert!(!difference.within(&Tolerance::default().max_diff_ratio(0.5)));
    let exact = metric::compare(&base, &near, &Tolerance::exact()).expect("one size");
    assert_eq!(
        exact.differing, 16,
        "threshold 0 passes nothing but equal pixels"
    );
    assert_eq!(exact.diff.pixel(0, 0), Some(metric::DIFF_COLOR));
    let refused = metric::compare(&base, &image(4, 5, BLACK), &tolerance)
        .expect_err("images of two sizes are not compared");
    assert!(
        matches!(refused, Error::InvalidArgument { .. }),
        "{refused:?}"
    );
    let refused = metric::compare(&base, &near, &Tolerance::default().threshold(1.5))
        .expect_err("a threshold past 1");
    assert!(
        matches!(refused, Error::InvalidArgument { .. }),
        "{refused:?}"
    );
}

/// A dark square on white whose edge pixels are a softer grey in one image and a harder one in
/// the other: anti-aliasing.
fn soft_edge(edge: u8) -> Image {
    let mut image = image(9, 9, WHITE);
    image.fill(DeviceRect::new(3, 3, 3, 3), BLACK);
    for (x, y) in [(2, 4), (6, 4), (4, 2), (4, 6)] {
        image.set_pixel(x, y, [edge, edge, edge, 255]);
    }
    image
}

#[test]
fn anti_aliased_pixels_count_unless_the_tolerance_excuses_them() {
    let one = soft_edge(120);
    let other = soft_edge(200);
    let counted = metric::compare(&one, &other, &Tolerance::default()).expect("one size");
    assert_eq!(counted.differing, 4, "{counted}");
    let excused = metric::compare(
        &one,
        &other,
        &Tolerance::default().ignore_antialiasing(true),
    )
    .expect("one size");
    assert_eq!(
        (excused.differing, excused.antialiased),
        (0, 4),
        "{excused}"
    );
    assert_eq!(excused.diff.pixel(2, 4), Some(metric::ANTIALIASED_COLOR));
    // A pixel changed in the middle of a flat area is no anti-aliasing.
    let mut spot = image(9, 9, WHITE);
    spot.set_pixel(1, 1, BLACK);
    let flat = metric::compare(
        &image(9, 9, WHITE),
        &spot,
        &Tolerance::default().ignore_antialiasing(true),
    )
    .expect("one size");
    assert_eq!((flat.differing, flat.antialiased), (1, 0), "{flat}");
}

#[test]
fn a_logical_region_covers_every_device_pixel_it_touches() {
    let rect =
        DeviceRect::of_logical(LogicalRect::new(1.25, 0.5, 2.0, 1.0), 2.0).expect("a region");
    assert_eq!(rect, DeviceRect::new(2, 1, 5, 2));
    let left_of = DeviceRect::of_logical(LogicalRect::new(-3.0, 0.0, 4.0, 1.0), 1.0).expect("cut");
    assert_eq!(left_of, DeviceRect::new(0, 0, 1, 1));
    for (rect, scale) in [
        (LogicalRect::new(f64::NAN, 0.0, 1.0, 1.0), 1.0),
        (LogicalRect::new(0.0, 0.0, -1.0, 1.0), 1.0),
        (LogicalRect::new(0.0, 0.0, 1.0, 1.0), 0.0),
        (LogicalRect::new(0.0, 0.0, 1e300, 1.0), 1.0),
    ] {
        let refused = DeviceRect::of_logical(rect, scale).expect_err("no device pixels");
        assert!(
            matches!(refused, Error::InvalidArgument { .. }),
            "{refused:?}"
        );
    }
    assert!(
        Image::try_new(2, 2, vec![0; 15]).is_err(),
        "a short buffer is refused"
    );
    let mut crop = image(4, 4, WHITE);
    crop.set_pixel(3, 3, BLACK);
    let corner = crop.crop(DeviceRect::new(2, 2, 2, 2)).expect("inside");
    assert_eq!(corner.pixel(1, 1), Some(BLACK));
    assert!(
        crop.crop(DeviceRect::new(3, 3, 2, 2)).is_err(),
        "a region past the image"
    );
}

#[test]
fn the_predicates_check_one_capture_without_a_baseline() {
    let mut drawn = image(10, 10, WHITE);
    drawn.fill(DeviceRect::new(0, 0, 5, 10), [250, 10, 10, 255]);
    let drawn = Capture::new(drawn, CaptureSource::Os, "scripted", 2.0);
    drawn
        .expect_color_at(LogicalPoint::new(1.0, 1.0), [255, 0, 0, 255], 10)
        .expect("red within 10");
    let wrong = drawn
        .expect_color_at(LogicalPoint::new(1.0, 1.0), [255, 0, 0, 255], 5)
        .expect_err("not within 5");
    assert!(matches!(wrong, Error::Expectation { .. }), "{wrong:?}");
    let past = drawn
        .expect_color_at(LogicalPoint::new(5.0, 1.0), WHITE, 0)
        .expect_err("past the capture");
    assert!(matches!(past, Error::OutsideTarget { .. }), "{past:?}");
    drawn
        .expect_uniform(Some(LogicalRect::new(2.5, 0.0, 2.5, 5.0)), 0)
        .expect("the right half is white");
    assert!(
        drawn.expect_uniform(None, 0).is_err(),
        "red and white are not one colour"
    );

    let mut later = drawn.image().clone();
    later.set_pixel(9, 9, BLACK);
    let later = Capture::new(later, CaptureSource::Os, "scripted", 2.0);
    let left = Some(LogicalRect::new(0.0, 0.0, 2.5, 5.0));
    let corner = Some(LogicalRect::new(4.0, 4.0, 1.0, 1.0));
    let exact = Tolerance::exact();
    later
        .expect_same_as(&drawn, left, &exact)
        .expect("the left half is unchanged");
    later
        .expect_differs_from(&drawn, corner, &exact)
        .expect("the corner changed");
    assert!(later.expect_same_as(&drawn, None, &exact).is_err());
    assert!(later.expect_differs_from(&drawn, left, &exact).is_err());
}

#[test]
fn an_update_is_refused_in_ci_and_a_misspelled_one_everywhere() {
    for (setting, ci, mode) in [
        (None, None, Some(UpdateMode::None)),
        (Some(""), Some("true"), Some(UpdateMode::None)),
        (Some("none"), Some("1"), Some(UpdateMode::None)),
        (Some("missing"), None, Some(UpdateMode::Missing)),
        (Some("changed"), Some(""), Some(UpdateMode::Changed)),
        (Some("all"), Some("false"), Some(UpdateMode::All)),
        (Some("all"), Some("0"), Some(UpdateMode::All)),
        (Some("missing"), Some("true"), None),
        (Some("all"), Some("1"), None),
        (Some("everything"), None, None),
    ] {
        let read = UpdateMode::from_setting(setting, ci);
        assert_eq!(
            read.as_ref().ok(),
            mode.as_ref(),
            "{setting:?} with CI={ci:?}: {read:?}"
        );
    }
}

#[test]
fn a_missing_baseline_fails_and_leaves_the_capture_as_new() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let baselines = Baselines::new(dir.path());
    let capture = capture_of(image(6, 4, [10, 120, 200, 255]));
    let comparison = baselines
        .compare("home", &capture, &[], &Tolerance::default())
        .expect("a comparison");
    let BaselineStatus::Missing { new } = &comparison.status else {
        panic!("a missing baseline: {comparison}");
    };
    assert!(!comparison.passed());
    assert!(
        !comparison.baseline.exists(),
        "nothing is written as the baseline"
    );
    assert_eq!(
        new,
        &dir.path()
            .join("home")
            .join(format!("os-scripted-{}.new.png", std::env::consts::OS))
    );
    let written = flui_driver::pixels::read_png(new).expect("the new capture reads back");
    assert_eq!(&written, capture.image());
    let refused = comparison
        .into_result()
        .expect_err("a missing baseline is no pass");
    assert!(matches!(refused, Error::Expectation { .. }), "{refused:?}");
}

#[test]
fn update_modes_write_baselines_and_a_written_one_still_fails() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let first = capture_of(image(6, 4, WHITE));
    let mut moved = image(6, 4, WHITE);
    moved.fill(DeviceRect::new(0, 0, 3, 4), BLACK);
    let second = capture_of(moved);
    let tolerance = Tolerance::default();

    let created = Baselines::new(dir.path())
        .update(UpdateMode::Missing)
        .compare("panel", &first, &[], &tolerance)
        .expect("a comparison");
    assert_eq!(created.status, BaselineStatus::Written { replaced: false });
    assert!(!created.passed(), "a run that only created baselines fails");
    assert!(created.baseline.exists());

    let matched = Baselines::new(dir.path())
        .compare("panel", &first, &[], &tolerance)
        .expect("a comparison");
    assert_eq!(matched.status, BaselineStatus::Matched { differing: 0 });

    // `missing` leaves a differing baseline alone.
    let kept = Baselines::new(dir.path())
        .update(UpdateMode::Missing)
        .compare("panel", &second, &[], &tolerance)
        .expect("a comparison");
    let BaselineStatus::Differs {
        differing,
        new,
        diff,
        ..
    } = &kept.status
    else {
        panic!("a differing capture: {kept}");
    };
    assert_eq!(*differing, 12);
    assert!(
        new.exists() && diff.exists(),
        "the capture and the diff are left for review"
    );
    let marked = flui_driver::pixels::read_png(diff).expect("the diff reads back");
    assert_eq!(marked.pixel(0, 0), Some(metric::DIFF_COLOR));

    let rewritten = Baselines::new(dir.path())
        .update(UpdateMode::Changed)
        .compare("panel", &second, &[], &tolerance)
        .expect("a comparison");
    assert_eq!(rewritten.status, BaselineStatus::Written { replaced: true });
    assert!(
        !new.exists() && !diff.exists(),
        "a written baseline clears what was left"
    );

    let again = Baselines::new(dir.path())
        .compare("panel", &second, &[], &tolerance)
        .expect("a comparison");
    assert!(again.passed(), "{again}");
    let all = Baselines::new(dir.path())
        .update(UpdateMode::All)
        .compare("panel", &second, &[], &tolerance)
        .expect("a comparison");
    assert_eq!(all.status, BaselineStatus::Written { replaced: true });

    let resized = Baselines::new(dir.path())
        .compare("panel", &capture_of(image(7, 4, WHITE)), &[], &tolerance)
        .expect("a comparison");
    assert!(
        matches!(
            resized.status,
            BaselineStatus::SizeDiffers {
                expected: (6, 4),
                actual: (7, 4),
                ..
            }
        ),
        "{resized}"
    );
    assert!(
        Baselines::new(dir.path())
            .path("../escape", &flui_driver::pixels::BaselineKey::of(&first))
            .is_err()
    );
}

#[test]
fn a_baseline_past_its_bounds_or_not_a_png_is_refused_before_it_is_decoded() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let baselines = Baselines::new(dir.path());
    let capture = capture_of(image(2, 2, WHITE));
    let path = baselines
        .path("broken", &flui_driver::pixels::BaselineKey::of(&capture))
        .expect("a path");
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
    std::fs::write(&path, b"not a png").expect("the file");
    let refused = baselines
        .compare("broken", &capture, &[], &Tolerance::default())
        .expect_err("not a PNG");
    assert!(matches!(refused, Error::Unreadable { .. }), "{refused:?}");

    // A PNG whose header declares a surface past the decode bound: refused from its header,
    // before any pixel buffer is made.
    std::fs::write(&path, png_declaring(20_000, 20_000)).expect("the file");
    let refused = baselines
        .compare("broken", &capture, &[], &Tolerance::default())
        .expect_err("past the decode bound");
    let Error::Unreadable { message, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert!(message.contains("20000 x 20000"), "{message}");

    let file = std::fs::File::create(&path).expect("the file");
    file.set_len(flui_driver::pixels::MAX_PNG_FILE_BYTES + 1)
        .expect("a long file");
    let refused = baselines
        .compare("broken", &capture, &[], &Tolerance::default())
        .expect_err("past the file bound");
    assert!(matches!(refused, Error::Unreadable { .. }), "{refused:?}");
}

/// A PNG whose header declares a `width` by `height` RGBA image and whose data is one empty
/// chunk: what a decoder reads before it would allocate the pixels.
fn png_declaring(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(
            &u32::try_from(data.len())
                .expect("a short chunk")
                .to_be_bytes(),
        );
        let mut typed = kind.to_vec();
        typed.extend_from_slice(data);
        out.extend_from_slice(&typed);
        out.extend_from_slice(&crc32(&typed).to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 6 (RGBA), deflate, adaptive filtering, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(
        &mut out,
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The counter's window, read without a lane, and where its count's text lies on the surface.
fn counter_with_clock() -> (Driver, Rect) {
    let mut driver = Driver::launch(Launch::headless(|| Counter {
        probe: Probe::default(),
    }))
    .expect("the headless app starts");
    let clock = driver
        .main_window()
        .expect("a window")
        .locate(Locator::default().name_matching(TextMatch::prefix("Count:")))
        .matches()
        .expect("the app answers")
        .matches
        .first()
        .and_then(|found| found.node.surface_rect)
        .expect("the count's bounds");
    (driver, clock)
}

/// A screen that paints the window white with the count's text a new shade at every capture, as
/// a clock that ticks between captures would.
fn ticking_screen(
    clock: Rect,
) -> impl FnMut(&flui_driver::protocol::session::Placement) -> flui_driver::Result<Image> + Send {
    let tick = Arc::new(AtomicU8::new(0));
    move |placement| {
        let width = (placement.size.width * placement.scale).round() as u32;
        let height = (placement.size.height * placement.scale).round() as u32;
        let mut screen = Image::filled(width, height, WHITE)?;
        let shade = tick.fetch_add(40, Ordering::SeqCst);
        screen.fill(
            DeviceRect::new(clock.x as u32, clock.y as u32, clock.width, clock.height),
            [shade, 0, 0, 255],
        );
        Ok(screen)
    }
}

#[test]
fn a_mask_hides_a_changing_clock() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (driver, clock) = counter_with_clock();
    let (lane, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = driver
        .with_lane(lane.screen(ticking_screen(clock)))
        .expect("the lane starts");
    let mut window = driver.main_window().expect("a window");
    let masked = BaselineOptions::new(CaptureSource::Os)
        .mask(Locator::default().name_matching(TextMatch::prefix("Count:")))
        .timeout(Duration::from_secs(1));

    let missing = window
        .expect_baseline(&Baselines::new(dir.path()), "counter", &masked)
        .expect_err("no baseline yet");
    assert!(matches!(missing, Error::Expectation { .. }), "{missing:?}");
    let new = dir
        .path()
        .join("counter")
        .join(format!("os-scripted-{}.new.png", std::env::consts::OS));
    let left = flui_driver::pixels::read_png(&new).expect("the capture is left as new");
    assert_eq!(
        left.pixel(clock.x as u32, clock.y as u32),
        Some(MASK_COLOR),
        "the clock is masked"
    );

    let created = window
        .expect_baseline(
            &Baselines::new(dir.path()).update(UpdateMode::Missing),
            "counter",
            &masked,
        )
        .expect_err("a written baseline is no pass");
    assert!(matches!(created, Error::Expectation { .. }), "{created:?}");
    let matched = window
        .expect_baseline(&Baselines::new(dir.path()), "counter", &masked)
        .expect("the clock moved, and the mask hides it");
    assert!(matched.passed(), "{matched}");

    // Unmasked, the clock that moves on every capture never settles.
    let unmasked = BaselineOptions::new(CaptureSource::Os).timeout(Duration::from_secs(1));
    let never = window
        .expect_baseline(&Baselines::new(dir.path()), "counter", &unmasked)
        .expect_err("a moving clock is never stable");
    assert!(matches!(never, Error::Timeout { .. }), "{never:?}");
    let mask_of_nothing =
        BaselineOptions::new(CaptureSource::Os).mask(Locator::named("nothing like it"));
    let refused = window
        .expect_baseline(&Baselines::new(dir.path()), "counter", &mask_of_nothing)
        .expect_err("a mask that matches nothing");
    assert!(matches!(refused, Error::NotFound { .. }), "{refused:?}");
}

#[test]
fn a_window_stable_for_n_frames_passes_and_a_moving_one_names_the_frame() {
    let (driver, clock) = counter_with_clock();
    let (still, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = driver
        .with_lane(
            still.screen(|placement: &flui_driver::protocol::session::Placement| {
                Image::filled(
                    placement.size.width as u32,
                    placement.size.height as u32,
                    WHITE,
                )
            }),
        )
        .expect("the lane starts");
    let capture = driver
        .main_window()
        .expect("a window")
        .expect_stable_for(CaptureSource::Os, 3)
        .expect("a still window");
    assert_eq!(capture.source(), CaptureSource::Os);
    assert_eq!(capture.backend(), "scripted");

    let (driver, _) = counter_with_clock();
    let (moving, _) = ScriptedLane::new(Isolation::InProcess);
    let mut driver = driver
        .with_lane(moving.screen(ticking_screen(clock)))
        .expect("the lane starts");
    let changed = driver
        .main_window()
        .expect("a window")
        .expect_stable_for(CaptureSource::Os, 3)
        .expect_err("a ticking clock");
    let Error::Expectation { actual, .. } = &changed else {
        panic!("{changed:?}");
    };
    assert!(actual.contains("after 1"), "{actual}");
}

#[test]
fn the_apps_own_sources_are_named_as_not_supported_until_the_app_serves_them() {
    let (mut driver, _) = counter_with_clock();
    let mut window = driver.main_window().expect("a window");
    for source in [CaptureSource::Presented, CaptureSource::Rendered] {
        let refused = window
            .capture(source)
            .expect_err("the headless app captures nothing");
        assert!(refused.is_cannot_verify(), "{refused:?}");
        assert_eq!(
            refused.missing_capability().as_deref(),
            Some(source_method(source))
        );
    }
    let refused = window.capture(CaptureSource::Os).expect_err("no lane");
    assert_eq!(refused.missing_capability().as_deref(), Some("os_capture"));
}

fn source_method(source: CaptureSource) -> &'static str {
    match source {
        CaptureSource::Presented => "capture.presented",
        _ => "capture.rendered",
    }
}
