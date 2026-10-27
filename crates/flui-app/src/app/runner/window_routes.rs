//! The window handler the runner opens every window with (ADR-0254 §4).
//!
//! A window is opened with its handler, before the runner has a UI runtime
//! or a presentation for it: those are built from the open window, on the
//! owner turn that opened it or, for a window whose open was deferred, on a
//! later one. So the handler, [`WindowRoutes`], answers every obligation from
//! the first call, and the runner points each obligation at the presentation
//! once it exists (`route_*`). Nothing that arrives in between is lost:
//!
//! - every input, the capability snapshot first, is kept in order and
//!   answered with the deferred form of its result (its outcome is not known
//!   yet); the input route hears them all, in order, when it is set, so a
//!   press the user made while the presentation was being built is not cut
//!   off from its release;
//! - a state change, a frame request and a surface change are kept, the
//!   newest of each, and handed to their route when it is set: a state
//!   change carries no payload, a frame request is answered by one frame, and
//!   the newest surface change is the surface the window needs (a lost
//!   `Rebuild` would leave it blank);
//! - a close request keeps the window open for now and is remembered: the
//!   close route is asked once when it is set, and its `Close` closes the
//!   window;
//! - `closed` is kept, and the close route runs at once when it is set, so a
//!   window that closed while the runner was building its presentation still
//!   retires it.
//!
//! Each route is called with no borrow of the routes held, so it may set
//! another route, close the window or open one.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, PlatformWindow,
    SurfaceChange, WindowHandler, WindowId,
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

    /// A route is set and not out for a call.
    fn is_set(&self) -> bool {
        self.0.borrow().is_some()
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

/// What arrived before its route was set, handed to the route when it is.
#[derive(Default)]
struct Held {
    /// Every input, in order.
    inputs: VecDeque<PlatformInput>,
    /// A state change is owed.
    state_changed: bool,
    /// The newest frame request.
    frame: Option<FrameRequest>,
    /// The newest surface change.
    surface: Option<SurfaceChange>,
}

/// The runner's handler of one window: what each obligation does, pointed at
/// the window's presentation once it exists. See the module doc.
#[derive(Default)]
pub(crate) struct WindowRoutes {
    input: Slot<PlatformInput, DispatchEventResult>,
    state_changed: Slot<(), ()>,
    frame_requested: Slot<FrameRequest, ()>,
    close_requested: Slot<(), CloseDecision>,
    closed: RefCell<Option<Box<dyn FnOnce()>>>,
    /// `closed` arrived before its route.
    closed_heard: Cell<bool>,
    /// A close request arrived before its route: asked once it is set.
    close_pending: Cell<bool>,
    surface_changed: Slot<SurfaceChange, ()>,
    held: RefCell<Held>,
}

impl WindowRoutes {
    /// Routes that point nowhere yet.
    pub(crate) fn new() -> Rc<Self> {
        Rc::default()
    }

    /// The routes the runner opened `window` with.
    ///
    /// # Panics
    ///
    /// For a window the runner did not open with routes, or one that already
    /// let go of them: a route set there would never be called.
    pub(crate) fn of(window: &(impl PlatformWindow + ?Sized)) -> Rc<Self> {
        let id = window.id();
        let routes = APP_RUNTIME.with(|slot| slot.borrow().window_routes(id));
        // A unit test's window double was never opened and calls no handler:
        // routes set on it are never called, which is what such a test means.
        #[cfg(test)]
        let routes = routes.or_else(|| Some(Self::new()));
        routes.expect("BUG: the runner opens every window it routes with `WindowRoutes`, adopted")
    }

    /// Record that the window `window` was opened with these routes, so
    /// [`Self::of`] finds them. The runner keeps no strong reference: the
    /// window holds its handler and drops it when it closes.
    pub(crate) fn adopt(self: &Rc<Self>, window: WindowId) {
        let weak = Rc::downgrade(self);
        APP_RUNTIME.with(|slot| slot.borrow_mut().adopt_window_routes(window, weak));
    }

    /// Route the window's input; the input heard before goes first, in order.
    pub(crate) fn route_input(
        &self,
        route: impl FnMut(PlatformInput) -> DispatchEventResult + 'static,
    ) {
        self.input.set(Box::new(route));
        self.deliver_held_inputs();
    }

    /// Hand the held input to the input route, in order. Popped one at a time
    /// with no borrow held: input that arrives while the route runs joins the
    /// end and is reached in turn; a route that panics leaves the rest held
    /// for the next input to hand over.
    fn deliver_held_inputs(&self) {
        while self.input.is_set() {
            let next = self.held.borrow_mut().inputs.pop_front();
            let Some(input) = next else { break };
            let _ = self.input.call(input);
        }
    }

    /// Route the window's state changes; one owed from before is delivered.
    pub(crate) fn route_state_changed(&self, mut route: impl FnMut() + 'static) {
        self.state_changed.set(Box::new(move |()| route()));
        if std::mem::take(&mut self.held.borrow_mut().state_changed) {
            let _ = self.state_changed.call(());
        }
    }

    /// Route the window's frame requests; the newest one from before is
    /// delivered.
    pub(crate) fn route_frame_requested(&self, route: impl FnMut(FrameRequest) + 'static) {
        self.frame_requested.set(Box::new(route));
        let held = self.held.borrow_mut().frame.take();
        if let Some(request) = held {
            let _ = self.frame_requested.call(request);
        }
    }

    /// Route the window's close requests. A request that arrived before the
    /// route kept the window open and is asked now, once; granted, `close`
    /// closes the window (the window system's own request was answered
    /// already, so the close is the application's command).
    pub(crate) fn route_close_requested(
        &self,
        mut route: impl FnMut() -> CloseDecision + 'static,
        close: impl FnOnce() + 'static,
    ) {
        self.close_requested.set(Box::new(move |()| route()));
        if self.close_pending.take() && self.close_requested.call(()) == Some(CloseDecision::Close)
        {
            close();
        }
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
        let held = self.held.borrow_mut().surface.take();
        if let Some(change) = held {
            let _ = self.surface_changed.call(change);
        }
    }
}

impl WindowHandler for WindowRoutes {
    fn input(&self, event: PlatformInput) -> DispatchEventResult {
        // A set route that is not out for a call (the replay of what was held
        // takes it out) hears the input now.
        if self.held.borrow().inputs.is_empty() && self.input.is_set() {
            return self
                .input
                .call(event)
                .expect("BUG: a set input route is there for its call");
        }
        // No route yet, or input still waiting for it: kept behind that, and
        // handed over now when the route is there to take it.
        self.held.borrow_mut().inputs.push_back(event);
        self.deliver_held_inputs();
        DispatchEventResult::DEFERRED
    }

    fn state_changed(&self) {
        if self.state_changed.call(()).is_none() {
            self.held.borrow_mut().state_changed = true;
        }
    }

    fn frame_requested(&self, request: FrameRequest) -> FrameOutcome {
        if self.frame_requested.call(request).is_some() {
            return FrameOutcome::Produced;
        }
        // Answered with one frame once the route is set; nothing ran now.
        self.held.borrow_mut().frame = Some(request);
        FrameOutcome::Declined
    }

    fn close_requested(&self) -> CloseDecision {
        self.close_requested.call(()).unwrap_or_else(|| {
            // No presentation to ask yet: the window stays, and the request
            // is asked once the route is set.
            self.close_pending.set(true);
            CloseDecision::KeepOpen
        })
    }

    fn closed(&self) {
        let route = self.closed.borrow_mut().take();
        match route {
            Some(route) => route(),
            None => self.closed_heard.set(true),
        }
    }

    fn surface_changed(&self, change: SurfaceChange) {
        if self.surface_changed.call(change).is_none() {
            self.held.borrow_mut().surface = Some(change);
        }
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
