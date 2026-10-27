//! The window handler the runner opens every window with (ADR-0254 §4).
//!
//! A window is opened with its handler, before the runner has a UI runtime
//! or a presentation for it: those are built from the open window. So the
//! handler, [`WindowRoutes`], answers every obligation from the first call,
//! and the runner points each obligation at the presentation once it exists
//! (`route_*`). Until then:
//!
//! - an input is not taken, and the platform's default runs, except the
//!   capability snapshot, which is kept and handed to the input route as its
//!   first input, so the presentation still hears the window's capabilities
//!   before anything else;
//! - a state change, a frame request and a surface change are left: the
//!   presentation reads the window's snapshot when it is built, and asks for
//!   its first frame itself;
//! - a close request closes the window: nothing can veto it yet;
//! - `closed` is kept, and the close route runs at once when it is set, so a
//!   window that closed while the runner was building its presentation still
//!   retires it.
//!
//! Each route is called with no borrow of the routes held, so it may set
//! another route, close the window or open one.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameRequest, PlatformInput, PlatformWindow, SurfaceChange,
    WindowHandler, WindowId,
};

use super::host::APP_RUNTIME;

type Route<A, R> = Box<dyn FnMut(A) -> R>;

/// One route: taken out for its call, put back unless a new one was set
/// meanwhile.
struct Slot<A, R>(RefCell<Option<Route<A, R>>>);

impl<A, R> Default for Slot<A, R> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}

impl<A, R> Slot<A, R> {
    fn set(&self, route: Route<A, R>) {
        let replaced = self.0.replace(Some(route));
        drop(replaced);
    }

    /// Call the route with `argument`, with no borrow of the slot held;
    /// `None` while none is set.
    ///
    /// The route goes back afterwards, a panicking one included, so the next
    /// call still reaches the presentation; a route set while it ran wins, and
    /// the one that ran is dropped then, or kept out of the failure when it
    /// panicked. The panic resumes to the window, which contains it.
    fn call(&self, argument: A) -> Option<R> {
        let mut route = self.0.borrow_mut().take()?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| route(argument)));
        let displaced = {
            let mut slot = self.0.borrow_mut();
            if slot.is_none() {
                *slot = Some(route);
                None
            } else {
                Some(route)
            }
        };
        match outcome {
            Ok(result) => {
                drop(displaced);
                Some(result)
            }
            Err(payload) => {
                std::mem::forget(displaced);
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// The runner's handler of one window: what each obligation does, pointed at
/// the window's presentation once it exists. See the module doc.
#[derive(Default)]
pub(crate) struct WindowRoutes {
    input: Slot<PlatformInput, DispatchEventResult>,
    /// The capabilities heard while no input route was set: handed to the
    /// route as its first input.
    capabilities: Cell<Option<InputCapabilities>>,
    state_changed: Slot<(), ()>,
    frame_requested: Slot<FrameRequest, ()>,
    close_requested: Slot<(), CloseDecision>,
    closed: RefCell<Option<Box<dyn FnOnce()>>>,
    /// `closed` arrived before its route.
    closed_heard: Cell<bool>,
    surface_changed: Slot<SurfaceChange, ()>,
}

impl WindowRoutes {
    /// Routes that point nowhere yet.
    pub(crate) fn new() -> Rc<Self> {
        Rc::default()
    }

    /// The routes of `window`: the ones the runner opened it with, or, for a
    /// window it did not open (a test double), routes nothing calls.
    pub(crate) fn of(window: &(impl PlatformWindow + ?Sized)) -> Rc<Self> {
        let id = window.id();
        APP_RUNTIME
            .with(|slot| slot.borrow().window_routes(id))
            .unwrap_or_default()
    }

    /// Record that the window `window` was opened with these routes, so
    /// [`Self::of`] finds them. The runner keeps no strong reference: the
    /// window holds its handler and drops it when it closes.
    pub(crate) fn adopt(self: &Rc<Self>, window: WindowId) {
        let weak = Rc::downgrade(self);
        APP_RUNTIME.with(|slot| slot.borrow_mut().adopt_window_routes(window, weak));
    }

    /// Route the window's input; the capabilities heard before go first.
    pub(crate) fn route_input(
        &self,
        route: impl FnMut(PlatformInput) -> DispatchEventResult + 'static,
    ) {
        self.input.set(Box::new(route));
        if let Some(capabilities) = self.capabilities.take() {
            let _ = self.input.call(PlatformInput::Capabilities(capabilities));
        }
    }

    /// Route the window's state changes.
    pub(crate) fn route_state_changed(&self, mut route: impl FnMut() + 'static) {
        self.state_changed.set(Box::new(move |()| route()));
    }

    /// Route the window's frame requests.
    pub(crate) fn route_frame_requested(&self, route: impl FnMut(FrameRequest) + 'static) {
        self.frame_requested.set(Box::new(route));
    }

    /// Route the window's close requests.
    pub(crate) fn route_close_requested(&self, mut route: impl FnMut() -> CloseDecision + 'static) {
        self.close_requested.set(Box::new(move |()| route()));
    }

    /// Run `route` when the window has closed, in place of the route set
    /// before; at once when it closed already.
    pub(crate) fn route_closed(&self, route: impl FnOnce() + 'static) {
        if self.closed_heard.get() {
            route();
        } else {
            let replaced = self.closed.replace(Some(Box::new(route)));
            drop(replaced);
        }
    }

    /// Route the window's surface changes.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32", test)),
        expect(
            dead_code,
            reason = "only the Android, iOS and web hosts release a window's surface"
        )
    )]
    pub(crate) fn route_surface_changed(&self, route: impl FnMut(SurfaceChange) + 'static) {
        self.surface_changed.set(Box::new(route));
    }
}

impl WindowHandler for WindowRoutes {
    fn input(&self, event: PlatformInput) -> DispatchEventResult {
        let capabilities = match &event {
            PlatformInput::Capabilities(capabilities) => Some(*capabilities),
            _ => None,
        };
        if let Some(result) = self.input.call(event) {
            return result;
        }
        // No route yet: the capabilities are kept for the route to come.
        if let Some(capabilities) = capabilities {
            self.capabilities.set(Some(capabilities));
        }
        DispatchEventResult::default()
    }

    fn state_changed(&self) {
        let _ = self.state_changed.call(());
    }

    fn frame_requested(&self, request: FrameRequest) {
        let _ = self.frame_requested.call(request);
    }

    fn close_requested(&self) -> CloseDecision {
        self.close_requested
            .call(())
            .unwrap_or(CloseDecision::Close)
    }

    fn closed(&self) {
        let route = self.closed.borrow_mut().take();
        match route {
            Some(route) => route(),
            None => self.closed_heard.set(true),
        }
    }

    fn surface_changed(&self, change: SurfaceChange) {
        let _ = self.surface_changed.call(change);
    }
}

/// The routes the runner opened windows with, by window: weak, so a window
/// that closed and dropped its handler leaves nothing behind.
#[derive(Default)]
pub(crate) struct RouteBook(Vec<(WindowId, Weak<WindowRoutes>)>);

impl RouteBook {
    pub(crate) fn find(&self, window: WindowId) -> Option<Rc<WindowRoutes>> {
        self.0
            .iter()
            .find(|(id, _)| *id == window)
            .and_then(|(_, routes)| routes.upgrade())
    }

    pub(crate) fn adopt(&mut self, window: WindowId, routes: Weak<WindowRoutes>) {
        self.0
            .retain(|(id, routes)| *id != window && routes.strong_count() > 0);
        self.0.push((window, routes));
    }
}

/// Opening a window with fresh routes, adopted, as the runner opens its
/// windows: for tests that drive a window the runner installs.
#[cfg(test)]
pub(crate) trait OpenRouted {
    /// What opening answers.
    type Opened;

    /// Open a window with `options` and fresh routes.
    fn open_routed(
        &self,
        options: flui_platform::WindowOptions,
    ) -> Result<Self::Opened, flui_native::OpenWindowError>;
}

#[cfg(test)]
impl<P: flui_native::Platform + ?Sized> OpenRouted for P {
    type Opened = std::sync::Arc<dyn flui_native::HostWindow>;

    fn open_routed(
        &self,
        options: flui_platform::WindowOptions,
    ) -> Result<Self::Opened, flui_native::OpenWindowError> {
        let routes = WindowRoutes::new();
        let window = self.open_window(options, routes.clone())?;
        routes.adopt(window.id());
        Ok(window)
    }
}

/// [`OpenRouted`] for the owner capability, whose open may be deferred: a
/// window that opens at once has its routes adopted.
#[cfg(test)]
pub(crate) trait OwnerOpenRouted {
    /// Open a window with `options` and fresh routes.
    fn open_routed(
        &self,
        options: flui_platform::WindowOptions,
    ) -> Result<flui_native::WindowOpen, flui_native::OpenWindowError>;
}

#[cfg(test)]
impl OwnerOpenRouted for flui_native::OwnerPlatform {
    fn open_routed(
        &self,
        options: flui_platform::WindowOptions,
    ) -> Result<flui_native::WindowOpen, flui_native::OpenWindowError> {
        let routes = WindowRoutes::new();
        let opened = self.open_window(options, routes.clone())?;
        if let flui_native::WindowOpen::Ready(window) = &opened {
            routes.adopt(window.id());
        }
        Ok(opened)
    }
}

#[cfg(test)]
mod tests;
