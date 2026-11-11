//! Input regions on the backend side (ADR-0217 §5): the headless window hands its simulated
//! OS a set only when the projection changed.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{DevicePoint, DeviceRect, Rect};
use flui_native::{HeadlessHostWindow, HeadlessPlatform, HostWindow, OwnerPlatform, Platform as _};
use flui_platform::Availability;
use flui_platform::WindowOptions;
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::input_regions::{DeviceInputRegion, InputRegion, InputRegionKind, ResizeEdge};
use flui_platform::{DispatchEventResult, PlatformInput};
use parking_lot::Mutex;

use crate::run_table;

const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

fn owner() -> OwnerPlatform {
    started(HeadlessPlatform::new())
}

/// A headless owner whose simulated OS honours the region kinds `availability` names.
fn owner_honouring(availability: InputCapabilities) -> OwnerPlatform {
    started(HeadlessPlatform::new().with_input_availability(availability))
}

fn started(platform: HeadlessPlatform) -> OwnerPlatform {
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(platform)
        .run(crate::app::on_ready(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    saved.borrow_mut().take().expect("started owner")
}

fn open(
    owner: &OwnerPlatform,
    handler: &std::rc::Rc<flui_testing::window_handler::RecordingWindowHandler>,
) -> Rc<dyn HostWindow> {
    owner
        .open_window(WindowOptions::default(), handler.clone())
        .expect("open")
        .try_ready()
        .expect("ready")
}

fn mock(window: &Rc<dyn HostWindow>) -> &HeadlessHostWindow {
    window
        .as_any()
        .downcast_ref::<HeadlessHostWindow>()
        .expect("mock")
}

fn device(left: i32, top: i32, right: i32, bottom: i32) -> DeviceInputRegion {
    DeviceInputRegion::new(
        EXCLUSION,
        DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
    )
}

fn headless_reports_input_regions_and_sends_only_changes() {
    let owner = owner();
    let handler = flui_testing::window_handler::RecordingWindowHandler::new();
    let capabilities = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&capabilities);
    handler.react_to_input(move |input| {
        if let PlatformInput::Capabilities(value) = input {
            *seen.lock() = Some(value);
        }
        DispatchEventResult::default()
    });
    let window = open(&owner, &handler);
    assert_eq!(
        capabilities
            .lock()
            .map(|value| value.input_region(InputRegionKind::SystemGestureExclusion)),
        Some(Availability::Native)
    );

    let regions = [InputRegion::new(
        EXCLUSION,
        Rect::from_ltrb(0.0, 0.0, 10.0, 20.0),
    )];
    let send = || {
        window
            .set_input_regions(&regions)
            .expect("the headless window takes input regions");
    };
    send();
    send();
    mock(&window).simulate_scale_factor_change(2.0);
    send();
    assert_eq!(
        mock(&window).input_region_updates(),
        vec![vec![device(0, 0, 10, 20)], vec![device(0, 0, 20, 40)]]
    );
}

/// A window whose OS honours move areas only hands it the move area and the content cut
/// out of it, and drops the gesture exclusion it cannot honour.
fn headless_drops_the_kinds_its_os_does_not_honour() {
    let owner = owner_honouring(
        InputCapabilities::builder()
            .window_move(Availability::Native)
            .build(),
    );
    let handler = flui_testing::window_handler::RecordingWindowHandler::new();
    let window = open(&owner, &handler);
    let rect = Rect::from_ltrb(0.0, 0.0, 10.0, 20.0);
    window
        .set_input_regions(&[
            InputRegion::new(EXCLUSION, rect),
            InputRegion::new(InputRegionKind::WindowMove, rect),
            InputRegion::new(InputRegionKind::WindowContent, rect),
            InputRegion::new(InputRegionKind::WindowResize(ResizeEdge::Top), rect),
        ])
        .expect("a window that honours move areas takes input regions");
    let pixels = DeviceRect::from_min_max(DevicePoint::new(0, 0), DevicePoint::new(10, 20));
    assert_eq!(
        mock(&window).input_region_updates(),
        vec![vec![
            DeviceInputRegion::new(InputRegionKind::WindowMove, pixels),
            DeviceInputRegion::new(InputRegionKind::WindowContent, pixels),
        ]]
    );
}

/// A window whose OS honours no kind of region refuses the set for good.
fn headless_without_any_kind_refuses_input_regions() {
    let owner = owner_honouring(InputCapabilities::UNAVAILABLE);
    let handler = flui_testing::window_handler::RecordingWindowHandler::new();
    let window = open(&owner, &handler);
    let refused = window
        .set_input_regions(&[InputRegion::new(
            InputRegionKind::WindowMove,
            Rect::from_ltrb(0.0, 0.0, 10.0, 20.0),
        )])
        .expect_err("no kind of region is honoured");
    assert!(!refused.is_retryable(), "{refused}");
    assert_eq!(
        mock(&window).input_region_updates(),
        Vec::<Vec<DeviceInputRegion>>::new()
    );
}

#[test]
fn input_regions() {
    run_table(
        "input_regions",
        &[
            (
                "headless_reports_input_regions_and_sends_only_changes",
                headless_reports_input_regions_and_sends_only_changes,
            ),
            (
                "headless_drops_the_kinds_its_os_does_not_honour",
                headless_drops_the_kinds_its_os_does_not_honour,
            ),
            (
                "headless_without_any_kind_refuses_input_regions",
                headless_without_any_kind_refuses_input_regions,
            ),
        ],
    );
}
