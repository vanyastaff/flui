//! Input regions on the backend side (ADR-0217 §5): the headless window hands its simulated
//! OS a set only when the projection changed.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{DevicePoint, DeviceRect, Rect};
use flui_native::{HeadlessPlatform, HostWindow, MockWindow, OwnerPlatform, Platform as _};
use flui_platform::Availability;
use flui_platform::WindowOptions;
use flui_platform::input_regions::{DeviceInputRegion, InputRegion, InputRegionKind};
use flui_platform::{DispatchEventResult, PlatformInput};
use parking_lot::Mutex;

use crate::run_table;

const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

fn owner() -> OwnerPlatform {
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(HeadlessPlatform::new())
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

fn mock(window: &Rc<dyn HostWindow>) -> &MockWindow {
    window.as_any().downcast_ref::<MockWindow>().expect("mock")
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
        capabilities.lock().map(|value| value.input_regions()),
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

#[test]
fn input_regions() {
    run_table(
        "input_regions",
        &[(
            "headless_reports_input_regions_and_sends_only_changes",
            headless_reports_input_regions_and_sends_only_changes,
        )],
    );
}
