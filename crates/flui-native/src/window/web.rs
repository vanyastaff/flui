//! The web window: [`WebWindow`], the page's one window, wrapping a
//! `<canvas>`, and the window-owned DOM subscriptions it registers
//! ([`Registration`]): only the exact canvas may have a live owner.
//!
//! The page has one window and the loop keeps its callbacks
//! (`owner::web`), so the window position tracks nothing of its own.

// This module is one of the workspace's sanctioned `unsafe` FFI islands —
// `wasm-bindgen`'s JS interop requires it, and the window's `Send`/`Sync`
// claims rest on the page having one thread, which `platforms::web` enforces
// by refusing a `+atomics` build. The workspace lint `unsafe_code = "warn"` is
// opted out here, at the module boundary, rather than for the whole crate.
#![expect(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::Size;
use parking_lot::Mutex;
use wasm_bindgen::{JsCast, prelude::*};

use crate::{
    host, host_window::HostWindow, owner::OpenWindowError, owner::handlers::WindowCallbacks,
    owner::signal::OwnerSignal, owner::web::WebLoop, window::state::WindowStateCell,
};
use flui_platform::{
    CursorError, PlatformWindow, WindowAppearance, WindowBackgroundAppearance, WindowCapabilities,
    WindowCommand, WindowCommandError, WindowEvent, WindowExecutionState, WindowGeometry, WindowId,
    WindowMode, WindowOptions, WindowState,
};

use crate::owner::web::{CanvasListeners, HostDisplays};

/// Web window wrapping a `<canvas>` element
pub struct WebWindow {
    id: WindowId,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    registration: RefCell<Option<Registration>>,
    retire_frame_slot: RefCell<Option<Box<dyn FnOnce()>>>,
    drag: RefCell<Option<Box<dyn crate::owner::web::CanvasDrag>>>,
    text: RefCell<Option<Rc<crate::text::web::WebTextInput>>>,
    /// The platform's display state, which names the screen this window is on.
    displays: Arc<dyn HostDisplays>,
    /// The published window state (ADR-0237).
    snapshot: Arc<WindowStateCell>,
}

/// What the layout, focus and visibility observers need to republish the
/// window state after they changed it.
#[derive(Clone)]
pub(crate) struct StatePublisher {
    cell: Arc<WindowStateCell>,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    displays: Arc<dyn HostDisplays>,
}

impl StatePublisher {
    /// The published state, read from the page again first when something
    /// invalidated it; `on_state_changed` is raised when that produced a new
    /// revision.
    fn current(&self) -> WindowState {
        let published = self.cell.state_with(|| {
            sample_state(&self.canvas, &self.state, self.displays.current_display())
        });
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    /// The window may differ from its published state: read it again.
    pub(crate) fn publish(&self) {
        self.cell.invalidate();
        let _ = self.current();
    }
}

/// The window as the page reports it now. A canvas has no frame, no global
/// position and no limits of its own; the layout sets its size.
fn sample_state(
    canvas: &web_sys::HtmlCanvasElement,
    state: &Mutex<WebWindowState>,
    display: Option<flui_platform::DisplayId>,
) -> WindowState {
    let (title, size, focused, visible) = {
        let state = state.lock();
        (
            state.title.clone(),
            Size::new(state.width, state.height),
            state.focused,
            state.visible,
        )
    };
    // The canvas, or an element that holds it: another element of the page (a
    // video) can be fullscreen while this window is not, and an application
    // that wraps the canvas in a container makes that container fullscreen.
    let fullscreen = canvas.owner_document().is_some_and(|document| {
        document
            .fullscreen_element()
            .is_some_and(|element| element.contains(Some(canvas)))
    });
    WindowState::new(WindowGeometry::new(None, size), size)
        .with_mode(if fullscreen {
            WindowMode::Fullscreen
        } else {
            WindowMode::Normal
        })
        .with_visible(visible)
        .with_focused(focused)
        .with_title(Some(title))
        .with_resizable(false)
        .with_decorated(false)
        .with_display(display)
}

struct WebWindowState {
    title: String,
    width: f64,
    height: f64,
    scale_factor: f64,
    /// The canvas's backing store, which is the surface's physical size.
    backing: DeviceSize,
    focused: bool,
    visible: bool,
    /// What the Page Lifecycle API last said: a frozen page runs no script.
    execution: WindowExecutionState,
}

// SAFETY: WASM is single-threaded — no data races possible
unsafe impl Send for WebWindow {}
unsafe impl Sync for WebWindow {}

/// The canvas's current CSS box in CSS pixels and the device pixel ratio,
/// read from the live layout. `None` when the box is empty — a canvas that
/// is `display: none`, or not yet laid out — since an empty surface is not
/// a size to configure a swapchain for.
fn layout_size(canvas: &web_sys::HtmlCanvasElement) -> Option<(f64, f64, f64)> {
    let window = web_sys::window()?;
    let width = canvas.client_width();
    let height = canvas.client_height();
    (width > 0 && height > 0).then(|| (width as f64, height as f64, window.device_pixel_ratio()))
}

/// The page's `prefers-color-scheme: dark` query; `None` where the browser
/// cannot evaluate it, which reads as light.
pub(crate) fn dark_scheme_query() -> Option<web_sys::MediaQueryList> {
    web_sys::window()?
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
}

/// A canvas size in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DeviceSize {
    /// `logical × scale`, rounded: the device pixels a box probably covers.
    /// The browser snaps a box to device pixels from its position too, so
    /// this can be a pixel off the size it really laid out.
    fn estimate(width: f64, height: f64, scale: f64) -> Self {
        Self {
            width: (width * scale).round() as u32,
            height: (height * scale).round() as u32,
        }
    }
}

/// Size the canvas's backing store so the surface renders one texel per
/// device pixel. Skipped when unchanged: assigning `width`/`height` clears a
/// canvas even to the same value.
fn apply_backing_size(canvas: &web_sys::HtmlCanvasElement, size: DeviceSize) {
    if canvas.width() != size.width {
        canvas.set_width(size.width);
    }
    if canvas.height() != size.height {
        canvas.set_height(size.height);
    }
}

impl WebWindow {
    /// Create a new WebWindow backed by a `<canvas>` element.
    ///
    /// Looks for an existing `<canvas id="flui-canvas">` in the document,
    /// or creates one and appends it to `<body>`.
    ///
    /// # Size: the canvas's CSS box is the window
    ///
    /// A browser has no window to size; it has a page with a layout. The
    /// window's logical size is therefore the canvas's CSS box, read from
    /// the live layout, and it is the page's to set: a canvas the page put
    /// in the document keeps whatever styling the page gave it (`width:
    /// 100vw; height: 100vh`, a fixed frame, a flex child — all fine), and
    /// a canvas this constructor creates is styled to fill the viewport
    /// (`display: block; width: 100vw; height: 100vh`), which is what an
    /// application with no page of its own means. The requested
    /// `width`/`height` are the fallback only for a canvas whose box is
    /// empty at construction (`display: none`, or not yet laid out) and are
    /// applied as its CSS size then; a caller that wants a fixed size
    /// otherwise sets it in CSS. The backing store follows the box at the
    /// device pixel ratio, and `platforms::web::events` keeps both in step with the
    /// layout afterwards (a `ResizeObserver` on the canvas plus the window's
    /// `resize` event for zoom), dispatching a resize to the embedder each
    /// time they change — so the first version of this backend, which
    /// pinned the canvas to the requested size in inline CSS and never
    /// dispatched a resize, no longer overrides the page.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when the browser environment lacks the
    /// pieces this needs (no global window/document/body) or the canvas
    /// element cannot be created.
    pub(crate) fn new(
        id: WindowId,
        title: &str,
        width: f64,
        height: f64,
        displays: Arc<dyn HostDisplays>,
    ) -> Result<Self, OpenWindowError> {
        fn backend(message: impl Into<String>) -> OpenWindowError {
            OpenWindowError::Backend {
                message: message.into(),
            }
        }

        let window = web_sys::window().ok_or_else(|| backend("no global window"))?;
        let document = window.document().ok_or_else(|| backend("no document"))?;

        // Find existing canvas or create a new one
        let canvas = if let Some(el) = document.get_element_by_id("flui-canvas") {
            el.dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| backend("element 'flui-canvas' is not a canvas"))?
        } else {
            let canvas = document
                .create_element("canvas")
                .map_err(|e| backend(format!("failed to create canvas: {e:?}")))?
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| backend("failed to cast to HtmlCanvasElement"))?;
            canvas.set_id("flui-canvas");
            // No page of its own: the canvas is the viewport.
            let style = canvas.style();
            let _ = style.set_property("display", "block");
            let _ = style.set_property("width", "100vw");
            let _ = style.set_property("height", "100vh");
            document
                .body()
                .ok_or_else(|| backend("no body element"))?
                .append_child(&canvas)
                .map_err(|e| backend(format!("failed to append canvas: {e:?}")))?;
            canvas
        };

        // Refuse an existing live owner before changing its sizing, focusability
        // or title. The claim compares the actual DOM object, never its label.
        let claim = CanvasClaim::acquire(&canvas)?;

        // The layout's box is the size; the requested size is the fallback
        // for a canvas that has none yet, and becomes its CSS size then —
        // including for a canvas this constructor created, whose viewport
        // styling above is then overridden by the fixed size: a page that
        // gives the canvas no box at construction (a `display: none`
        // ancestor, say) gets the requested size rather than nothing.
        let (width, height, scale_factor) = layout_size(&canvas).unwrap_or_else(|| {
            let style = canvas.style();
            let _ = style.set_property("width", &format!("{width}px"));
            let _ = style.set_property("height", &format!("{height}px"));
            (width, height, window.device_pixel_ratio())
        });
        let backing = DeviceSize::estimate(width, height, scale_factor);
        apply_backing_size(&canvas, backing);

        // Make canvas focusable for keyboard events
        canvas.set_tab_index(0);

        // Every touch on the canvas is the application's: with the default
        // `touch-action`, the browser claims a touch that moves past its
        // slop for its own panning or zooming and cancels the pointer, so a
        // swipe, a drag or a pinch dies a few pixels in. Set inline, so a
        // page's stylesheet cannot hand the gestures back to the browser.
        let _ = canvas.style().set_property("touch-action", "none");

        // Set page title
        document.set_title(title);

        let state = WebWindowState {
            title: title.to_string(),
            width,
            height,
            scale_factor,
            backing,
            focused: effective_focus(&canvas),
            visible: !document.hidden(),
            execution: WindowExecutionState::Running,
        };

        let state = Arc::new(Mutex::new(state));
        let initial = sample_state(&canvas, &state, displays.current_display());
        Ok(Self {
            id,
            registration: RefCell::new(Some(Registration::new(&canvas, claim))),
            retire_frame_slot: RefCell::new(None),
            drag: RefCell::new(None),
            text: RefCell::new(None),
            canvas,
            state,
            callbacks: Arc::new(WindowCallbacks::new()),
            displays,
            snapshot: Arc::new(WindowStateCell::new(initial)),
        })
    }

    /// The handle by which an observer republishes this window's state.
    pub(crate) fn publisher(&self) -> StatePublisher {
        StatePublisher {
            cell: Arc::clone(&self.snapshot),
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            displays: Arc::clone(&self.displays),
        }
    }

    /// A handle for the layout-tracking listeners `platforms::web::events` registers:
    /// re-reads the canvas's CSS box and the device pixel ratio, and, when
    /// either or the backing size changed, resizes the backing store, updates
    /// the tracked size and dispatches a resize followed by a frame request.
    /// A no-op when nothing changed (a `ResizeObserver` reports once on
    /// registration, and a `resize` event fires for scrolls on some mobile
    /// browsers).
    pub(crate) fn layout_sync(&self) -> LayoutSync {
        LayoutSync {
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            publisher: self.publisher(),
            observed: Cell::new(None),
        }
    }

    /// Get a reference to the underlying canvas element
    pub fn canvas(&self) -> &web_sys::HtmlCanvasElement {
        &self.canvas
    }

    /// Get window callbacks for event dispatch
    pub fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }

    pub(crate) fn install(
        &self,
        input_latency: flui_platform::InputLatency,
        transfer: &Arc<dyn crate::owner::web::HostDrag>,
        retire_frame_slot: impl FnOnce() + 'static,
    ) -> Result<(), OpenWindowError> {
        let mut registration = self
            .registration
            .borrow_mut()
            .take()
            .expect("BUG: newly opened window owns its registration");
        let services = crate::platforms::web::events::register_event_listeners(
            self,
            input_latency,
            transfer,
            &mut registration,
        )?;
        services.drag.arm();
        *self.drag.borrow_mut() = Some(services.drag);
        *self.text.borrow_mut() = Some(services.text);
        *self.registration.borrow_mut() = Some(registration);
        *self.retire_frame_slot.borrow_mut() = Some(Box::new(retire_frame_slot));
        Ok(())
    }

    pub(crate) fn focus_sync(&self) -> FocusSync {
        FocusSync {
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            observed: Cell::new(self.state.lock().focused),
            publisher: self.publisher(),
        }
    }

    /// The handle by which the Page Lifecycle listeners `platforms::web::events`
    /// registers set the window's execution state.
    pub(crate) fn execution_sync(&self) -> ExecutionSync {
        ExecutionSync {
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
        }
    }

    pub(crate) fn visibility_sync(&self, accepting: Rc<Cell<bool>>) -> VisibilitySync {
        VisibilitySync {
            document: self
                .canvas
                .owner_document()
                .expect("BUG: window construction verified the canvas document"),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            observed: Cell::new(self.state.lock().visible),
            accepting,
            publisher: self.publisher(),
        }
    }

    fn retire(&self, notify_close: bool) {
        let registration = self.registration.borrow_mut().take();
        let Some(registration) = registration else {
            return;
        };
        registration.fence();
        let drag = self.drag.borrow_mut().take();
        if let Some(drag) = drag {
            drag.retire();
        }
        let text = self.text.borrow_mut().take();
        if let Some(text) = text {
            text.retire();
        }
        let retire_frame_slot = self.retire_frame_slot.borrow_mut().take();
        if let Some(retire) = retire_frame_slot {
            retire();
        }
        drop(registration);
        struct Clear<'a>(&'a WindowCallbacks);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                self.0.clear();
            }
        }
        let _clear = Clear(&self.callbacks);
        if notify_close {
            self.callbacks.dispatch_close();
        }
    }

    fn install_callback<T>(&self, slot: &Mutex<Option<T>>, callback: T) {
        let accepting = self.registration.borrow().is_some();
        let old = if accepting {
            slot.lock().replace(callback)
        } else {
            Some(callback)
        };
        crate::owner::call_point::retire(old);
    }
}

impl Drop for WebWindow {
    fn drop(&mut self) {
        self.retire(false);
    }
}

pub(crate) struct FocusSync {
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    observed: Cell<bool>,
    publisher: StatePublisher,
}

impl FocusSync {
    pub(crate) fn sync(&self) {
        let focused = effective_focus(&self.canvas);
        if self.observed.replace(focused) == focused {
            return;
        }
        let state = Arc::clone(&self.state);
        let cell = Arc::clone(&self.publisher.cell);
        self.callbacks.dispatch_focus_observation(focused, move || {
            state.lock().focused = focused;
            cell.invalidate();
        });
        self.publisher.publish();
    }
}

pub(crate) struct VisibilitySync {
    pub(crate) document: web_sys::Document,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    observed: Cell<bool>,
    accepting: Rc<Cell<bool>>,
    publisher: StatePublisher,
}

impl VisibilitySync {
    pub(crate) fn sync(&self) {
        let visible = !self.document.hidden();
        // A host getter can synchronously retire this registration.
        if !self.accepting.get() || self.observed.replace(visible) == visible {
            return;
        }
        let state = Arc::clone(&self.state);
        let cell = Arc::clone(&self.publisher.cell);
        self.callbacks
            .dispatch_visibility_observation(visible, move || {
                state.lock().visible = visible;
                cell.invalidate();
            });
        self.publisher.publish();
    }
}

/// Records what the Page Lifecycle API says about the page and reports each
/// change once; see [`WebWindow::execution_sync`].
pub(crate) struct ExecutionSync {
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
}

impl ExecutionSync {
    /// The page is frozen (`Suspended`) or runs again (`Running`). The state
    /// lock is released before the dispatch, which runs embedder callbacks.
    pub(crate) fn set(&self, next: WindowExecutionState) {
        {
            let mut state = self.state.lock();
            if state.execution == next {
                return;
            }
            state.execution = next;
        }
        self.callbacks.dispatch_execution_state_change(next);
    }
}

/// See [`WebWindow::layout_sync`].
pub(crate) struct LayoutSync {
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    publisher: StatePublisher,
    /// The device-pixel size the `ResizeObserver` last reported, with the
    /// CSS box it was reported for.
    observed: Cell<Option<ObservedSize>>,
}

/// A `ResizeObserverEntry`'s `devicePixelContentBoxSize`: the device pixels
/// the browser laid the box out at, exact where the estimate can be a pixel
/// off, while it holds ([`Self::holds`]).
#[derive(Clone, Copy)]
struct ObservedSize {
    css: (f64, f64),
    device: DeviceSize,
}

impl ObservedSize {
    /// Whether this is still the device size of a `width`×`height` box at
    /// `scale`: the box is the one it was reported for, and `scale` can lay
    /// that box out at this size. `clientWidth`/`clientHeight` are snapped to
    /// whole CSS pixels, up to one off the size laid out, and the device box
    /// to whole device pixels, up to one off that size times `scale`.
    ///
    /// It holds across a ratio change the device-pixel observer does not
    /// report: the observer reports one that moves the device box, so one it
    /// does not report left the box as it was, which the estimate at the new
    /// ratio may not be. A size the ratio contradicts is in other pixels: the
    /// box at the old ratio, which the pixel-ratio watch replaces before the
    /// observer reports the new one, or the screen's own, which Chrome's
    /// device emulation reports while `devicePixelRatio` is the emulated
    /// ratio.
    fn holds(self, width: f64, height: f64, scale: f64) -> bool {
        let slack = scale + 1.0;
        self.css == (width, height)
            && (f64::from(self.device.width) - width * scale).abs() <= slack
            && (f64::from(self.device.height) - height * scale).abs() <= slack
    }
}

impl LayoutSync {
    /// A `ResizeObserver` reported the canvas, with its device-pixel size
    /// where the browser gives one and it can be used: record it for the
    /// current CSS box, replacing the last, and sync.
    pub(crate) fn observe(&self, device: Option<DeviceSize>) {
        let observed = device
            .zip(layout_size(&self.canvas))
            .map(|(device, (width, height, _))| ObservedSize {
                css: (width, height),
                device,
            });
        self.observed.set(observed);
        self.sync();
    }

    /// Bring the backing store and the tracked size up to the layout; see
    /// [`WebWindow::layout_sync`]. The backing store is the observed device
    /// size while it holds ([`ObservedSize::holds`]), and the estimate
    /// otherwise: a `resize` event can run before the observer reports the
    /// new box. The state lock is released before the dispatch, which runs
    /// embedder callbacks.
    pub(crate) fn sync(&self) {
        let Some((width, height, scale_factor)) = layout_size(&self.canvas) else {
            return;
        };
        let backing = self
            .observed
            .get()
            .filter(|observed| observed.holds(width, height, scale_factor))
            .map_or_else(
                || DeviceSize::estimate(width, height, scale_factor),
                |observed| observed.device,
            );
        let changed = {
            let mut state = self.state.lock();
            let changed = state.width != width
                || state.height != height
                || state.scale_factor != scale_factor
                || state.backing != backing;
            state.width = width;
            state.height = height;
            state.scale_factor = scale_factor;
            state.backing = backing;
            changed
        };
        if !changed {
            return;
        }
        apply_backing_size(&self.canvas, backing);
        self.publisher.publish();
        let physical = flui_foundation::geometry::DeviceSize::new(
            i32::try_from(backing.width).unwrap_or(i32::MAX),
            i32::try_from(backing.height).unwrap_or(i32::MAX),
        );
        if let Ok(metrics) = flui_foundation::geometry::WindowMetrics::new(physical, scale_factor) {
            self.callbacks.dispatch_resize(metrics);
        }
        self.callbacks.dispatch_request_frame();
    }
}

impl crate::host_window::CoreHostWindow for WebWindow {}

impl crate::host_window::HostWindow for WebWindow {
    /// The canvas's `EditContext` or hidden textarea, serving the focused
    /// field's store; `None` once the window closed.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
        use crate::text::{TextInput, WindowTextInput};
        self.text
            .borrow()
            .as_ref()
            .and_then(TextInput::window_text_input)
            .and_then(WindowTextInput::store_host)
    }
}

/// What every browser window honours, known before one exists
/// (`Platform::window_capabilities`).
pub(crate) const fn window_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = true;
    capabilities.enters_fullscreen = true;
    capabilities
}

impl std::fmt::Debug for WebWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebWindow")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl host::Window<WebLoop> for WebWindow {
    /// The page's one window is the loop's to track (its callbacks).
    type Shared = ();

    fn open(
        owner: &WebLoop,
        _: &Arc<()>,
        options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(title = %options.title, "Creating web window (canvas)");

        let window = Self::new(
            WindowId(0), // Single window in browser
            &options.title,
            options.size.width,
            options.size.height,
            Arc::clone(&owner.displays),
        )?;

        // Store window callbacks in state so the RAF loop can dispatch frames
        let callbacks = window.callbacks().clone();
        callbacks.set_input_capabilities(crate::shared::input_availability::web(
            crate::platforms::web::events::has_gesture_events(),
        ));
        callbacks.set_input_wake(OwnerSignal::callbacks_wake(&owner.signal, &callbacks));
        let retiring = Arc::clone(&callbacks);
        let state = Arc::downgrade(&owner.state);
        let transfer = Arc::clone(&owner.drag);
        window.install(options.input_latency, &transfer, move || {
            let removed = state.upgrade().and_then(|state| {
                let mut state = state.lock();
                if state
                    .window_callbacks
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &retiring))
                {
                    state.window_callbacks.take()
                } else {
                    None
                }
            });
            drop(removed);
        })?;
        owner.with_state(|s| {
            s.window_callbacks = Some(callbacks);
        });

        // Notify window created
        WebLoop::dispatch_window_event(&owner.state, WindowEvent::Created(WindowId(0)));

        Ok(Arc::new(window))
    }

    fn active(owner: &WebLoop, (): &()) -> Option<WindowId> {
        owner
            .state
            .lock()
            .window_callbacks
            .as_ref()
            .map(|_| WindowId(0))
    }

    fn capabilities(_: &WebLoop, (): &()) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(_: &WebLoop, (): &(), _: f64) -> Size<f64> {
        // A canvas has no title bar or borders: its outer size is its content size.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &WebLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        crate::owner::call_point::install(
            &WebLoop::handler(&owner.state, |state| &mut state.handlers.window_event),
            callback,
        );
    }
}

impl PlatformWindow for WebWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        // The canvas's backing store, so the surface the embedder configures
        // from this answer and the store it renders into agree.
        let backing = self.state.lock().backing;
        Size::new(
            i32::try_from(backing.width).unwrap_or(i32::MAX),
            i32::try_from(backing.height).unwrap_or(i32::MAX),
        )
    }

    fn logical_size(&self) -> Size<f64> {
        let state = self.state.lock();
        Size::new(state.width, state.height)
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    /// Served by the page's next animation frame: the platform's
    /// `requestAnimationFrame` loop dispatches a frame request on every one,
    /// and the embedder's frame skips when nothing is dirty. Never inside
    /// this call: a frame that asks for the next one (a running animation)
    /// would get it at once, at the same frame time, inside its own
    /// dispatch, in a loop that holds the page's only thread.
    fn request_redraw(&self) {}

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    fn execution_state(&self) -> WindowExecutionState {
        self.state.lock().execution
    }

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        self.publisher().current()
    }

    fn capabilities(&self) -> WindowCapabilities {
        window_capabilities()
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.registration.borrow().is_none() {
            return Err(WindowCommandError::Closed);
        }
        command.admit(&PlatformWindow::capabilities(self))?;
        match command {
            WindowCommand::SetTitle(title) => {
                self.state.lock().title.clone_from(&title);
                if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                    document.set_title(&title);
                }
            }
            WindowCommand::SetMode(WindowMode::Fullscreen) => {
                let el: &web_sys::Element = self.canvas.as_ref();
                el.request_fullscreen()
                    .map_err(|error| WindowCommandError::Backend {
                        message: format!("{error:?}"),
                    })?;
            }
            WindowCommand::SetMode(WindowMode::Normal) => {
                if let Some(document) = self.canvas.owner_document()
                    && document.fullscreen_element().is_some()
                {
                    document.exit_fullscreen();
                }
            }
            // Every other command is refused by `admit` through a capability
            // this backend leaves `false`.
            _ => return Err(WindowCommandError::Unsupported),
        }
        // The page answers a fullscreen request later, with
        // `fullscreenchange`; the title is ours to report at once.
        self.publisher().publish();
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.state.lock().focused
    }

    fn appearance(&self) -> WindowAppearance {
        if dark_scheme_query().is_some_and(|query| query.matches()) {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }

    /// The browser's screen the page is on: the one screen of a tab, or the
    /// current screen when the Window Management API lists several. The id is
    /// the snapshot's, so the host derives the period from it.
    fn display_id(&self) -> Option<flui_platform::DisplayId> {
        if self.registration.borrow().is_none() {
            return None;
        }
        self.displays.current_display()
    }

    fn close(&self) {
        self.retire(true);
    }

    fn set_background_appearance(&self, _appearance: WindowBackgroundAppearance) {
        // Not applicable for web canvas
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.drag.borrow().as_ref().map(|drag| drag.source())
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::new(crate::haptics::web::WebHaptics))
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.canvas
            .style()
            .set_property("cursor", cursor.name())
            .map_err(|error| CursorError::Backend(format!("{error:?}")))
    }

    // ==================== Callback Registration ====================

    fn on_input(
        &self,
        callback: Box<
            dyn FnMut(flui_platform::PlatformInput) -> flui_platform::DispatchEventResult,
        >,
    ) {
        if self.registration.borrow().is_some() {
            self.callbacks.set_input_callback(callback);
        } else {
            crate::owner::panic_boundary::contain_owner_callback(|| drop(callback));
        }
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_request_frame, callback);
    }
    fn on_resize(&self, callback: Box<dyn FnMut(flui_foundation::geometry::WindowMetrics) + Send>) {
        self.install_callback(&self.callbacks.on_resize, callback);
    }
    fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_state_changed, callback);
    }
    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        self.install_callback(&self.callbacks.on_close, callback);
    }
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        self.install_callback(&self.callbacks.on_should_close, callback);
    }
    fn on_safe_area_change(
        &self,
        callback: Box<dyn FnMut(flui_foundation::geometry::EdgeInsets) + Send>,
    ) {
        let accepting = self.registration.borrow().is_some();
        crate::owner::panic_boundary::contain_owner_callback(|| {
            if accepting {
                self.callbacks.set_safe_area_callback(callback);
            } else {
                drop(callback);
            }
        });
    }
    fn on_execution_state_change(
        &self,
        callback: Box<dyn FnMut(flui_platform::WindowExecutionState) + Send>,
    ) {
        let accepting = self.registration.borrow().is_some();
        crate::owner::panic_boundary::contain_owner_callback(|| {
            if accepting {
                self.callbacks.set_execution_state_callback(callback);
            } else {
                drop(callback);
            }
        });
    }
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.install_callback(&self.callbacks.on_active_status_change, callback);
    }
    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.install_callback(&self.callbacks.on_visibility_status_change, callback);
    }
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.install_callback(&self.callbacks.on_hover_status_change, callback);
    }
    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_appearance_changed, callback);
    }
    fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.install_callback(&self.callbacks.on_surface_status_change, callback);
    }

    // ==================== GPU Surface Handles ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let accepting = self.registration.borrow().is_some();
        if !accepting {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        use raw_window_handle::{RawWindowHandle, WebCanvasWindowHandle, WindowHandle};

        // WebCanvasWindowHandle expects a NonNull<c_void> pointer to the canvas object
        let obj: &wasm_bindgen::JsValue = self.canvas.as_ref();
        let ptr = std::ptr::NonNull::new(
            std::ptr::from_ref(obj)
                .cast_mut()
                .cast::<std::ffi::c_void>(),
        )
        .expect("BUG: a reference can never be null");
        let handle = WebCanvasWindowHandle::new(ptr);
        let raw = RawWindowHandle::WebCanvas(handle);
        // SAFETY: The canvas element is valid for the lifetime of this borrow
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{DisplayHandle, RawDisplayHandle, WebDisplayHandle};

        let raw = RawDisplayHandle::Web(WebDisplayHandle::new());
        // SAFETY: Web display handle is always valid
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[wasm_bindgen(inline_js = "
const ownerKey = Symbol.for('flui.canvas.input-owner');
export function claimCanvas(canvas) {
    if (canvas[ownerKey]) throw new Error('canvas already has a live FLUI input owner');
    const token = { pointerAdmissions: new Set() };
    Object.defineProperty(canvas, ownerKey, { value: token, configurable: true });
    return token;
}
export function releaseCanvas(canvas, token) {
    if (canvas[ownerKey] === token) delete canvas[ownerKey];
}
export function effectiveCanvasFocus(canvas) {
    if (!canvas.ownerDocument?.hasFocus()) return false;
    return isOwnedFocus(canvas, canvas.getRootNode().activeElement);
}
// The canvas, or the text editor its owner registered beside it.
export function isOwnedFocus(canvas, node) {
    if (node === canvas) return true;
    const editor = canvas[ownerKey]?.editor;
    return !!node && node === editor;
}
export function setCanvasEditor(canvas, token, editor) {
    if (canvas[ownerKey] === token) token.editor = editor;
}
// Observe the device-pixel content box where the browser has it (Chrome,
// Firefox); Safari rejects the box, and its content box is observed instead.
// True when the device-pixel box is.
export function observeCanvas(observer, canvas) {
    try {
        observer.observe(canvas, { box: 'device-pixel-content-box' });
        return true;
    } catch {
        observer.observe(canvas);
        return false;
    }
}
export function observeBox(observer, canvas, box) { observer.observe(canvas, { box }); }
// The last entry's device-pixel content box as [width, height]; [] when the
// entry has none, or when the canvas has any padding, a fraction of a pixel
// included: the window's size is the padding box (`clientWidth`), which the
// content box does not measure. The box sizes are logical: a vertical writing
// mode puts the width in blockSize.
export function devicePixelSize(entries) {
    const entry = entries[entries.length - 1];
    const box = entry?.devicePixelContentBoxSize?.[0];
    if (!box) return [];
    const style = globalThis.getComputedStyle?.(entry.target);
    if (!style) return [];
    const sides = [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft];
    if (sides.some(side => parseFloat(side) !== 0)) return [];
    const vertical = /^(vertical|sideways)/.test(style.writingMode ?? '');
    const width = vertical ? box.blockSize : box.inlineSize;
    const height = vertical ? box.inlineSize : box.blockSize;
    const valid = size => Number.isInteger(size) && size > 0;
    return valid(width) && valid(height) ? [width, height] : [];
}
export function disconnectObserver(observer) { observer.disconnect(); }
export function admitPointerCapture(canvas, token, id) {
    if (canvas[ownerKey] !== token) return false;
    token.pointerAdmissions.add(id);
    return true;
}
export function retirePointerCapture(token, id) { token.pointerAdmissions.delete(id); }
export function releasePointerCapture(canvas, token, id) {
    const belongsToNewOwner = () => {
        const current = canvas[ownerKey];
        return current && current !== token && current.pointerAdmissions.has(id);
    };
    if (belongsToNewOwner()) return;
    const captured = canvas.hasPointerCapture(id);
    // The host query itself can synchronously close/reopen the registration.
    if (captured && !belongsToNewOwner()) canvas.releasePointerCapture(id);
}
")]
extern "C" {
    #[wasm_bindgen(catch, js_name = claimCanvas)]
    fn claim_canvas(canvas: &web_sys::HtmlCanvasElement) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = releaseCanvas)]
    fn release_canvas(canvas: &web_sys::HtmlCanvasElement, token: &JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = effectiveCanvasFocus)]
    pub(crate) fn effective_focus(canvas: &web_sys::HtmlCanvasElement) -> bool;
    #[wasm_bindgen(js_name = isOwnedFocus)]
    pub(crate) fn is_owned_focus(canvas: &web_sys::HtmlCanvasElement, node: &JsValue) -> bool;
    #[wasm_bindgen(js_name = setCanvasEditor)]
    fn set_canvas_editor(canvas: &web_sys::HtmlCanvasElement, token: &JsValue, editor: &JsValue);
    #[wasm_bindgen(catch, js_name = observeCanvas)]
    fn observe_canvas(
        observer: &web_sys::ResizeObserver,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<bool, JsValue>;
    #[wasm_bindgen(catch, js_name = observeBox)]
    fn observe_box(
        observer: &web_sys::ResizeObserver,
        canvas: &web_sys::HtmlCanvasElement,
        r#box: &str,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = devicePixelSize)]
    fn device_pixel_size(entries: &JsValue) -> Vec<u32>;
    #[wasm_bindgen(catch, js_name = disconnectObserver)]
    fn disconnect_observer(observer: &web_sys::ResizeObserver) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = releasePointerCapture)]
    fn release_capture(
        canvas: &web_sys::HtmlCanvasElement,
        token: &JsValue,
        id: i32,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = admitPointerCapture)]
    fn admit_capture(canvas: &web_sys::HtmlCanvasElement, token: &JsValue, id: i32) -> bool;
    #[wasm_bindgen(js_name = retirePointerCapture)]
    fn retire_capture(token: &JsValue, id: i32);
}

fn backend(error: JsValue) -> OpenWindowError {
    OpenWindowError::Backend {
        message: format!("DOM registration failed: {error:?}"),
    }
}

fn report(error: JsValue) {
    crate::owner::panic_boundary::contain_owner_callback(|| {
        tracing::warn!(
            ?error,
            "DOM subscription retirement failed; delivery remains fenced"
        );
    });
}

pub(crate) struct CanvasClaim {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl CanvasClaim {
    pub(crate) fn acquire(canvas: &web_sys::HtmlCanvasElement) -> Result<Self, OpenWindowError> {
        Ok(Self {
            canvas: canvas.clone(),
            token: claim_canvas(canvas).map_err(backend)?,
        })
    }

    fn capture_owner(&self) -> PointerCaptureOwner {
        PointerCaptureOwner {
            canvas: self.canvas.clone(),
            token: self.token.clone(),
        }
    }
}

/// Native capture custody belongs to a registration, not its cached buttons.
/// A retired body may still be returning from a host call after a new owner
/// has admitted the same pointer on this canvas.
#[derive(Clone)]
pub(crate) struct PointerCaptureOwner {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl PointerCaptureOwner {
    pub(crate) fn admit(&self, id: i32) -> bool {
        admit_capture(&self.canvas, &self.token, id)
    }
    pub(crate) fn forget(&self, id: i32) {
        retire_capture(&self.token, id);
    }
    pub(crate) fn release(&self, id: i32) {
        self.forget(id);
        if let Err(error) = release_capture(&self.canvas, &self.token, id) {
            report(error);
        }
    }
}

impl Drop for CanvasClaim {
    fn drop(&mut self) {
        if let Err(error) = release_canvas(&self.canvas, &self.token) {
            report(error);
        }
    }
}

pub(crate) struct Listener {
    target: web_sys::EventTarget,
    kind: &'static str,
    capture: bool,
    closure: Option<Closure<dyn Fn(web_sys::Event)>>,
    // The JS wrapper has only a Weak reference. Even a refused detach cannot
    // retain application captures or invoke the retired body.
    _body: Rc<dyn Fn(web_sys::Event)>,
}

impl Listener {
    /// A nonpassive listener for `kind` on `target`, outside a
    /// registration's installation: one the window's text input attaches to
    /// its surface while the window lives. Its body runs only while
    /// `accepting` holds; dropping it detaches it.
    pub(crate) fn install(
        target: &web_sys::EventTarget,
        kind: &'static str,
        accepting: &Rc<Cell<bool>>,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        Self::install_in_phase(target, kind, accepting, false, body)
    }

    /// [`Self::install`] in the capture phase when `capture` holds, which
    /// sees the events of descendants that do not bubble (`scroll`).
    pub(crate) fn install_in_phase(
        target: &web_sys::EventTarget,
        kind: &'static str,
        accepting: &Rc<Cell<bool>>,
        capture: bool,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        Self::install_body(target, kind, accepting, capture, Rc::new(body))
    }

    /// [`Self::install_in_phase`] for a body handed over behind its `Rc`, of
    /// which the listener keeps the only strong reference.
    fn install_body(
        target: &web_sys::EventTarget,
        kind: &'static str,
        accepting: &Rc<Cell<bool>>,
        capture: bool,
        body: Rc<dyn Fn(web_sys::Event)>,
    ) -> Result<Self, OpenWindowError> {
        debug_assert_eq!(
            Rc::strong_count(&body),
            1,
            "BUG: a listener keeps the only strong reference to its body"
        );
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(accepting);
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |event| {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body(event);
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        options.set_capture(capture);
        // Own the token before calling the host: a replaced add method might
        // install a listener and then throw. Dropping it still attempts removal.
        let listener = Self {
            target: target.clone(),
            kind,
            capture,
            closure: Some(closure),
            _body: body,
        };
        target
            .add_event_listener_with_callback_and_add_event_listener_options(
                kind,
                listener
                    .closure
                    .as_ref()
                    .expect("BUG: live listener owns closure")
                    .as_ref()
                    .unchecked_ref(),
                &options,
            )
            .map_err(backend)?;
        Ok(listener)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let Some(closure) = self.closure.take() else {
            return;
        };
        if let Err(error) = self.target.remove_event_listener_with_callback_and_bool(
            self.kind,
            closure.as_ref().unchecked_ref(),
            self.capture,
        ) {
            // A host-replaced removal method can leave JS referring to this
            // function. Transfer the weak-only wrapper to wasm-bindgen's GC
            // lifetime instead of invalidating a still-referenced trampoline.
            let _ = closure.into_js_value();
            report(error);
        }
    }
}

struct ResizeSubscription {
    observer: web_sys::ResizeObserver,
    closure: Option<Closure<dyn Fn(JsValue)>>,
    _body: Rc<dyn Fn(Option<DeviceSize>)>,
}

impl Drop for ResizeSubscription {
    fn drop(&mut self) {
        if let Err(error) = disconnect_observer(&self.observer) {
            if let Some(closure) = self.closure.take() {
                let _ = closure.into_js_value();
            }
            report(error);
        }
    }
}

/// A registration-owned `setTimeout` slot: one pending deadline at a time,
/// cleared when re-armed and when the registration retires. Its body runs
/// only while the registration accepts, through a weak reference like a
/// listener's.
pub(crate) struct DeadlineTimer {
    closure: Closure<dyn Fn()>,
    pending: Rc<Cell<Option<i32>>>,
    _body: Rc<dyn Fn()>,
}

impl DeadlineTimer {
    /// Run the body once `delay` has passed, replacing a pending run.
    pub(crate) fn arm(&self, delay: std::time::Duration) {
        let Some(window) = web_sys::window() else {
            return;
        };
        if let Some(id) = self.pending.take() {
            window.clear_timeout_with_handle(id);
        }
        let millis = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
        match window.set_timeout_with_callback_and_timeout_and_arguments_0(
            self.closure.as_ref().unchecked_ref(),
            millis,
        ) {
            Ok(id) => self.pending.set(Some(id)),
            Err(error) => report(error),
        }
    }

    /// Clear the pending run, if any.
    pub(crate) fn cancel(&self) {
        if let (Some(id), Some(window)) = (self.pending.take(), web_sys::window()) {
            window.clear_timeout_with_handle(id);
        }
    }
}

impl Drop for DeadlineTimer {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) struct Registration {
    pub(crate) accepting: Rc<Cell<bool>>,
    pub(crate) pointers: Rc<RefCell<HashMap<i32, u16>>>,
    canvas: web_sys::HtmlCanvasElement,
    listeners: Vec<Listener>,
    observers: Vec<ResizeSubscription>,
    timers: Vec<Rc<DeadlineTimer>>,
    claim: CanvasClaim,
}

/// The canvas window's listeners, which its input and the drag transport add
/// theirs to.
impl CanvasListeners for Registration {
    fn listen(
        &mut self,
        target: &web_sys::EventTarget,
        kind: &'static str,
        body: Rc<dyn Fn(web_sys::Event)>,
    ) -> Result<(), OpenWindowError> {
        // A refused add drops its token at once, which attempts removal; the
        // installed prefix is removed when the registration rolls back.
        let listener = Listener::install_body(target, kind, &self.accepting, false, body)?;
        self.listeners.push(listener);
        Ok(())
    }
}

impl Registration {
    pub(crate) fn new(canvas: &web_sys::HtmlCanvasElement, claim: CanvasClaim) -> Self {
        Self {
            accepting: Rc::new(Cell::new(true)),
            pointers: Rc::default(),
            canvas: canvas.clone(),
            listeners: Vec::new(),
            observers: Vec::new(),
            timers: Vec::new(),
            claim,
        }
    }

    /// A deadline timer whose body runs only while this registration
    /// accepts; retirement clears it.
    pub(crate) fn timer(&mut self, body: impl Fn() + 'static) -> Rc<DeadlineTimer> {
        let body: Rc<dyn Fn()> = Rc::new(body);
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(&self.accepting);
        let pending: Rc<Cell<Option<i32>>> = Rc::default();
        let fired = Rc::clone(&pending);
        let closure = Closure::<dyn Fn()>::new(move || {
            fired.set(None);
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body();
            }
        });
        let timer = Rc::new(DeadlineTimer {
            closure,
            pending,
            _body: body,
        });
        self.timers.push(Rc::clone(&timer));
        timer
    }

    /// Observe the canvas's size; `body` gets each report's device-pixel
    /// size where the browser gives one and it can be used. Each observer
    /// reports only a change of its own box, so three watch the canvas: the
    /// device-pixel content box where the browser has it (Chrome, Firefox),
    /// else the CSS content box (Safari); the CSS content box beside a
    /// device-pixel one, as padding inside a `border-box` canvas or a reflow
    /// can move it while the rounded device box stays; and the border box,
    /// as padding around a `content-box` canvas grows the window's padding
    /// box (`clientWidth`) and neither content box. Every entry carries the
    /// device-pixel size where the browser has it, so any report replaces the
    /// last. A browser without `ResizeObserver` is left to the window's
    /// `resize` subscription.
    pub(crate) fn observe(
        &mut self,
        body: impl Fn(Option<DeviceSize>) + 'static,
    ) -> Result<(), OpenWindowError> {
        let body: Rc<dyn Fn(Option<DeviceSize>)> = Rc::new(body);
        let Some(device) = self.subscribe(&body) else {
            return Ok(());
        };
        let css_boxes: &[&str] = if observe_canvas(&device, &self.canvas).map_err(backend)? {
            &["content-box", "border-box"]
        } else {
            &["border-box"]
        };
        for &css_box in css_boxes {
            if let Some(observer) = self.subscribe(&body) {
                observe_box(&observer, &self.canvas, css_box).map_err(backend)?;
            }
        }
        Ok(())
    }

    /// A `ResizeObserver` that calls `body` with each report's device-pixel
    /// size while this registration accepts; `None` in a browser without one.
    fn subscribe(
        &mut self,
        body: &Rc<dyn Fn(Option<DeviceSize>)>,
    ) -> Option<web_sys::ResizeObserver> {
        let weak = Rc::downgrade(body);
        let accepting = Rc::clone(&self.accepting);
        let closure = Closure::<dyn Fn(JsValue)>::new(move |entries: JsValue| {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body(match device_pixel_size(&entries)[..] {
                    [width, height] => Some(DeviceSize { width, height }),
                    _ => None,
                });
            }
        });
        let observer = match web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref()) {
            Ok(observer) => observer,
            Err(error) => {
                crate::owner::panic_boundary::contain_owner_callback(|| {
                    tracing::warn!(
                        ?error,
                        "ResizeObserver unavailable; using window resize subscription"
                    );
                });
                return None;
            }
        };
        self.observers.push(ResizeSubscription {
            observer: observer.clone(),
            closure: Some(closure),
            _body: Rc::clone(body),
        });
        Some(observer)
    }

    pub(crate) fn fence(&self) {
        self.accepting.set(false);
    }

    pub(crate) fn capture_owner(&self) -> PointerCaptureOwner {
        self.claim.capture_owner()
    }

    /// The canvas's claim, as the text input names its editor on it.
    pub(crate) fn editor_slot(&self) -> EditorSlot {
        EditorSlot {
            canvas: self.claim.canvas.clone(),
            token: self.claim.token.clone(),
        }
    }
}

/// Where a canvas's owner names the text editor beside it, so the canvas's
/// focus area includes it.
#[derive(Clone)]
pub(crate) struct EditorSlot {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl EditorSlot {
    /// `editor` is now part of the canvas's focus area; `None` when none is.
    pub(crate) fn set(&self, editor: Option<&web_sys::HtmlElement>) {
        let editor = editor.map_or(JsValue::NULL, |editor| JsValue::from(editor.clone()));
        set_canvas_editor(&self.canvas, &self.token, &editor);
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.fence();
        let pointers = std::mem::take(&mut *self.pointers.borrow_mut());
        let owner = self.capture_owner();
        for &id in pointers.keys() {
            owner.forget(id);
        }
        // Admission ends before release, which can synchronously dispatch loss.
        for id in pointers.into_keys() {
            crate::owner::panic_boundary::contain_owner_callback(|| owner.release(id));
        }
        let observers = std::mem::take(&mut self.observers);
        let listeners = std::mem::take(&mut self.listeners);
        for timer in std::mem::take(&mut self.timers) {
            timer.cancel();
        }
        drop(observers);
        for listener in listeners {
            drop(listener);
        }
    }
}
