//! `Platform` and `HostWindow` are sealed: an implementation outside
//! `flui-native` does not compile, and the seal says why. `Platform`'s writes
//! every item, so the seal is its only error; `HostWindow`'s cannot write the
//! items that take the owner thread's token, which it cannot name, so those
//! are missing too. `PlatformWindow` stays open, so the window below is one.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_native::storage::DataStore;
use flui_native::{
    AppHandler, HostWindow, NativeClipboard, NativePrimarySelection, OpenWindowError,
    Platform, PlatformError,
};
use flui_platform::data_transfer::DragTransport;
use flui_platform::{
    Cursor, CursorError, DisplaySnapshot, PlatformWindow,
    AppDataDir, LaunchKey, SystemPreferences,
    AdmittedCommand, ApplyError, WindowCapabilities, WindowGeometry, WindowId, WindowIdMint,
    WindowHandler, WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;

struct OutsidePlatform;

impl Platform for OutsidePlatform {
    fn run(self: Box<Self>, _: Rc<dyn AppHandler>) -> Result<(), PlatformError> {
        Ok(())
    }

    fn quit(&self) {}

    fn request_exit_policy_reevaluation(&self) {}

    fn open_window(
        &self,
        _: WindowOptions,
        _: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        Ok(Rc::new(OutsideWindow))
    }

    fn preferences(&self) -> Result<SystemPreferences, PlatformError> {
        unreachable!("the impl never compiles")
    }

    fn preferences_delivered(&self, _: &SystemPreferences) {
        unreachable!("the impl never compiles")
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        unreachable!("the impl never compiles")
    }

    fn clipboard(&self) -> Rc<dyn NativeClipboard> {
        unreachable!("the impl never compiles")
    }

    fn primary_selection(&self) -> Option<Rc<dyn NativePrimarySelection>> {
        None
    }

    fn data_transfer(&self) -> Arc<dyn DragTransport> {
        unreachable!("the impl never compiles")
    }

    fn activate(&self, _: bool) {}

    fn release_session_end(&self) {}

    fn session_end_support(&self) -> flui_platform::SessionEndSupport {
        flui_platform::SessionEndSupport::NotifyOnly
    }

    fn window_capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn window_frame_inset(&self, _: DevicePixelRatio) -> Size<f64> {
        Size::default()
    }

    fn display_connection(&self) -> Arc<dyn flui_platform::DisplayConnection> {
        unimplemented!()
    }

    fn name(&self) -> &'static str {
        "outside"
    }

    fn launch_value(&self, _: LaunchKey) -> Option<String> {
        None
    }

    fn page_origin(&self) -> Option<String> {
        None
    }

    fn data_store(&self, _: &AppDataDir) -> Option<DataStore> {
        None
    }
}

struct OutsideWindow;

impl PlatformWindow for OutsideWindow {
    fn id(&self) -> WindowId {
        WindowIdMint::new().next().expect("a fresh mint issues an id")
    }

    fn request_redraw(&self) {}

    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::EMPTY, flui_platform::WindowExtent::ZERO)
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        command.against(&WindowCapabilities::NONE)?; // This file never compiles; not a model double.
        Ok(())
    }

    fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> {
        Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform).into())
    }

    fn set_frame_rate_hint(&self, _: flui_platform::FrameRateHint) -> Result<(), flui_platform::Unsupported> { Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform)) }
    fn set_input_regions(&self, _: &[flui_platform::input_regions::InputRegion]) -> Result<(), flui_platform::Unsupported> { Err(flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform)) }
    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        Ok(None)
    }

    fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> flui_platform::WindowAt {
        flui_platform::WindowAt::Unknown
    }

    fn reveal_after_first_frame(&self) {}

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(|| {})
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(flui_platform::testing::NoNativeSurface)
    }
}

impl HostWindow for OutsideWindow {
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }

    fn occluded_surface(&self) -> flui_platform::OccludedSurface {
        flui_platform::OccludedSurface::Discarded
    }
}

fn main() {}
