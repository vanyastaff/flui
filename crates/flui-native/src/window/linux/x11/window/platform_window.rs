//! `PlatformWindow`, `HostWindow` and the host table's `Window` for [`X11Window`].

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    AdmittedCommand, ApplyError, Cursor, CursorError, CursorGrab, PlatformTextInput,
    PlatformWindow, Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand,
    WindowHandler, WindowId, WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;
use x11rb::connection::Connection as _;

use super::{X11Display, X11Redraw, X11Surface, X11Window, X11Windows};
use crate::host::{self};
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::linux::x11::{WindowRef, X11Loop};
use crate::owner::window_delivery::WindowDelivery;
use crate::text::WindowTextInput;
use crate::window::linux::x11::support::capabilities;
use crate::window::state::not_carried_out;

impl PlatformWindow for X11Window {
    fn id(&self) -> WindowId {
        self.id
    }

    /// The X server states no gesture distances; the consumer keeps its own.
    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        Ok(None)
    }

    /// The stacking of this process's windows is not read yet, so the window
    /// under a point is not known.
    fn window_at(&self, _: Point<f64>) -> flui_platform::WindowAt {
        flui_platform::WindowAt::Unknown
    }

    fn request_redraw(&self) {
        self.mark_redraw();
    }

    /// Never asked: the X server keeps no image of an obscured window
    /// (`occluded_surface`), so a frame drawn while covered is clipped away
    /// and none is owed (ADR-0389).
    fn request_frame_at(&self, _: Option<flui_platform::EventTime>) {}

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(X11Redraw {
            redraw: Arc::clone(&self.redraw),
            doorbell: self.doorbell.clone(),
            retirement: self.delivery.retirement(),
        })
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        let xid = NonZeroU32::new(self.xid).expect("BUG: the server gives no window the id 0");
        Arc::new(X11Surface {
            connection: Arc::clone(&self.connection),
            xid,
            visual: NonZeroU32::new(self.connection.screen().root_visual),
            retirement: self.delivery.retirement(),
        })
    }

    /// A revision this read publishes is announced at the owner's turn, never
    /// from the read.
    fn state(&self) -> WindowState {
        let published = self.cell.read_unannounced(&self.delivery, || self.sample());
        // A revision a command published with nobody told is told with this
        // read's, at the owner's turn, never from the read.
        let unannounced = self.unannounced.take();
        if published.changed || unannounced {
            self.delivery.state_changed_after_read();
        }
        published.state
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.get() {
            return Err(ApplyError::Closed);
        }
        // Opened under what the window honours now: an event that marked it
        // (the window manager's end) may have published other capabilities
        // since the publication the command was admitted against (ADR-0341).
        let state = self.state_now();
        let command = command.against(&state.capabilities)?;
        match command {
            WindowCommand::SetTitle(title) => {
                self.write_title(&title);
                self.record.borrow_mut().title = title;
            }
            WindowCommand::SetPosition(at) => self.place(at)?,
            WindowCommand::SetSize(size) => self.resize(size.get())?,
            WindowCommand::SetConstraints(constraints) => {
                self.record.borrow_mut().constraints = constraints;
                self.write_size_hints(None);
            }
            WindowCommand::SetResizable(resizable) => {
                self.record.borrow_mut().resizable = resizable;
                self.write_size_hints(None);
            }
            WindowCommand::SetFrame(frame) => {
                let decorated = frame.has_system_title_bar();
                self.record.borrow_mut().frame = frame;
                self.write_decorations(decorated);
            }
            WindowCommand::SetMode(mode) => self.enter_mode(mode)?,
            WindowCommand::Show => self.show(),
            WindowCommand::Hide => self.hide(),
            WindowCommand::Activate => self.activate(),
            WindowCommand::Close => {
                self.close();
                return Ok(());
            }
            WindowCommand::SetCursorGrab(CursorGrab::None) => {
                self.input.let_pointer_go(self.window_ref());
            }
            // Only a window with the keyboard focus holds the pointer: one
            // without it holds none, which its snapshot shows.
            // A hold the server refuses leaves none: the one the window held
            // was let go of first, and the snapshot says so before the
            // refusal is answered.
            WindowCommand::SetCursorGrab(grab) => {
                if state.focus.is_focused()
                    && let Err(refused) = self.input.hold_pointer(self.window_ref(), grab)
                {
                    self.refresh_state();
                    return Err(ApplyError::Platform(refused));
                }
            }
            WindowCommand::SetLevel(level) => self.set_level(level),
            WindowCommand::SetResizeIncrements(step) => {
                self.record.borrow_mut().resize_increments = step;
                self.write_size_hints(None);
            }
            WindowCommand::SetAspectRatio(ratio) => {
                self.record.borrow_mut().aspect_ratio = ratio;
                self.write_size_hints(None);
            }
            // `admit` refused the material: its capability is unavailable.
            other => return Err(not_carried_out(&other)),
        }
        if let Err(error) = self.xcb().flush() {
            return Err(ApplyError::failed(error.to_string()));
        }
        self.refresh_state();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        {
            let mut record = self.record.borrow_mut();
            if self.closed.get() || !std::mem::take(&mut record.reveal_pending) {
                return;
            }
            record.shown = true;
        }
        self.map();
        if let Err(error) = self.xcb().flush() {
            tracing::debug!(%error, "cannot map an X11 window after its first frame");
        }
        self.refresh_state();
    }

    /// The pointer and its cursor are the input feature's: it loads a system
    /// cursor from the theme, makes an image a Render cursor, and sets the
    /// cursor on the window.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        if self.closed.get() {
            return Err(Unsupported::new(UnsupportedReason::NoWindow).into());
        }
        self.input.set_cursor(self.window_ref(), cursor)
    }

    /// X11 takes no refresh-rate request.
    fn set_frame_rate_hint(&self, _hint: flui_platform::FrameRateHint) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }

    /// `x11:<XID>`: the server's id names the window to every client while
    /// it exists, so the name needs nothing held.
    fn portal_parent(
        &self,
        answer: flui_platform::capability::Resolver<flui_platform::capability::PortalParent>,
    ) {
        let parent = std::num::NonZeroU32::new(self.xid).map_or_else(
            flui_platform::capability::PortalParent::none,
            flui_platform::capability::PortalParent::x11,
        );
        // The owner retiring first leaves nothing waiting for the answer.
        let _ = answer.resolve(parent);
    }

    /// The chrome regions a press on a window without decorations is answered
    /// from (ADR-0286 §10), projected at the window's scale. X11 has no system
    /// edge gestures, so those regions are kept and never read.
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input::regions::InputRegion],
    ) -> Result<(), Unsupported> {
        if self.closed.get() {
            return Err(Unsupported::new(UnsupportedReason::NoWindow));
        }
        let scale = self.record.borrow().scale;
        // Projected with nothing borrowed: a dropped region is reported
        // through `tracing`, whose subscriber is the application's code.
        let mut projected = std::mem::take(&mut self.chrome.borrow_mut().projected);
        let _changed = projected.update(regions, scale, None);
        self.chrome.borrow_mut().projected = projected;
        Ok(())
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// Its sessions run over XDND on the loop's transfer.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::transfer::drag::PlatformDragSource>> {
        Some(Arc::clone(&self.drag).drag_source(self.window_ref()))
    }
}

impl crate::host_window::CoreHostWindow for X11Window {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl HostWindow for X11Window {
    /// Without a compositing manager the server keeps no image of an obscured
    /// window, and under one a covering is not told.
    fn occluded_surface(&self) -> flui_platform::OccludedSurface {
        flui_platform::OccludedSurface::Discarded
    }

    /// The window's AT-SPI bridge, which announced the window on the bus as
    /// it opened.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text::store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::store_host)
    }
}

impl host::Window<X11Loop> for X11Window {
    type Shared = X11Windows;
    /// A window by its X id and its identity.
    type Ref = WindowRef;

    fn open(
        owner: &X11Loop,
        shared: &Arc<X11Windows>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        shared.open(owner, options, handler)
    }

    fn capabilities(_: &X11Loop, shared: &X11Windows) -> WindowCapabilities {
        capabilities(shared.support())
    }

    fn frame_inset(
        _: &X11Loop,
        shared: &X11Windows,
        scale_factor: flui_foundation::geometry::DevicePixelRatio,
    ) -> Size<f64> {
        shared.frame_inset.logical(scale_factor)
    }

    /// The connection every window of the host is made on.
    fn display_connection(
        _: &X11Loop,
        shared: &X11Windows,
    ) -> Arc<dyn flui_platform::DisplayConnection> {
        Arc::new(X11Display(Arc::clone(&shared.connection)))
    }
}
