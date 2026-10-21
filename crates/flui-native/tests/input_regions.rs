//! Input regions on the backend side (ADR-0217 §5): the headless window hands its simulated
//! OS a set only when the projection changed, and an OS that defers gestures per screen
//! edge is given the edges the exclusion regions reach.

use std::sync::Arc;

use flui_foundation::geometry::{DevicePoint, DeviceRect, Rect, Size};
use flui_native::shared::screen_edges::DeferredEdges;
use flui_native::{
    HeadlessPlatform, HostWindow, MockWindow, OwnerPlatform, Platform as _, WindowOptions,
};
use flui_platform::input_capabilities::Availability;
use flui_platform::input_regions::{DeviceInputRegion, InputRegion, InputRegionKind};
use flui_platform::{DispatchEventResult, PlatformInput};
use parking_lot::Mutex;

use crate::run_table;

const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;

fn owner() -> OwnerPlatform {
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(HeadlessPlatform::new())
        .run(Box::new(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    saved.borrow_mut().take().expect("started owner")
}

fn open(owner: &OwnerPlatform) -> Arc<dyn HostWindow> {
    owner
        .open_window(WindowOptions::default())
        .expect("open")
        .try_ready()
        .expect("ready")
}

fn mock(window: &Arc<dyn HostWindow>) -> &MockWindow {
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
    let window = open(&owner);
    let capabilities = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&capabilities);
    window.on_input(Box::new(move |input| {
        if let PlatformInput::Capabilities(value) = input {
            *seen.lock() = Some(value);
        }
        DispatchEventResult::default()
    }));
    assert_eq!(
        capabilities.lock().map(|value| value.input_regions()),
        Some(Availability::Native)
    );

    let regions = [InputRegion::new(
        EXCLUSION,
        Rect::from_ltrb(0.0, 0.0, 10.0, 20.0),
    )];
    window.set_input_regions(&regions);
    window.set_input_regions(&regions);
    mock(&window).simulate_scale_factor_change(2.0);
    window.set_input_regions(&regions);
    assert_eq!(
        mock(&window).input_region_updates(),
        vec![vec![device(0, 0, 10, 20)], vec![device(0, 0, 20, 40)]]
    );
}

fn an_exclusion_region_defers_the_edges_it_reaches() {
    let window = Size::new(100, 200);
    let edges = |regions: &[DeviceInputRegion]| DeferredEdges::of(regions, window);
    assert_eq!(edges(&[]), DeferredEdges::default());
    assert_eq!(
        edges(&[device(10, 10, 90, 190)]),
        DeferredEdges::default(),
        "clear of every edge"
    );
    assert_eq!(
        edges(&[device(0, 50, 20, 150)]),
        DeferredEdges {
            left: true,
            ..DeferredEdges::default()
        }
    );
    assert_eq!(
        edges(&[device(80, 180, 100, 200), device(0, 0, 100, 10)]),
        DeferredEdges {
            top: true,
            left: true,
            bottom: true,
            right: true,
        }
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
                "an_exclusion_region_defers_the_edges_it_reaches",
                an_exclusion_region_defers_the_edges_it_reaches,
            ),
        ],
    );
}
