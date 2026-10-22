//! Web window implementation wrapping a Canvas element

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::Size;
use parking_lot::Mutex;
use wasm_bindgen::JsCast;

use crate::{
    host, host_window::HostWindow, owner::OpenWindowError, owner::handlers::WindowCallbacks,
    owner::signal::OwnerSignal, owner::web::WebLoop, shared::window_state::WindowStateCell,
};
use flui_platform::{
    CursorError, PlatformWindow, WindowAppearance, WindowBackgroundAppearance, WindowCapabilities,
    WindowCommand, WindowCommandError, WindowEvent, WindowExecutionState, WindowGeometry, WindowId,
    WindowMode, WindowOptions, WindowState,
};

use super::{
    display::WebDisplays,
    registration::{CanvasClaim, Registration, effective_focus},
};

/// Web window wrapping a `<canvas>` element
pub struct WebWindow {
    id: WindowId,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    registration: RefCell<Option<Registration>>,
    retire_frame_slot: RefCell<Option<Box<dyn FnOnce()>>>,
    drag: RefCell<Option<crate::drag::web::CanvasDrag>>,
    text: RefCell<Option<Rc<crate::text::web::WebTextInput>>>,
    /// The platform's display state, which names the screen this window is on.
    displays: Arc<WebDisplays>,
    /// The published window state (ADR-0237).
    snapshot: Arc<WindowStateCell>,
}

/// What the layout, focus and visibility observers need to republish the
/// window state after they changed it.
#[derive(Clone)]
pub(super) struct StatePublisher {
    cell: Arc<WindowStateCell>,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    displays: Arc<WebDisplays>,
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
    pub(super) fn publish(&self) {
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
pub(super) fn dark_scheme_query() -> Option<web_sys::MediaQueryList> {
    web_sys::window()?
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
}

/// A canvas size in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DeviceSize {
    pub(super) width: u32,
    pub(super) height: u32,
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
    /// device pixel ratio, and `super::events` keeps both in step with the
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
    pub(super) fn new(
        id: WindowId,
        title: &str,
        width: f64,
        height: f64,
        displays: Arc<WebDisplays>,
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
    pub(super) fn publisher(&self) -> StatePublisher {
        StatePublisher {
            cell: Arc::clone(&self.snapshot),
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            displays: Arc::clone(&self.displays),
        }
    }

    /// A handle for the layout-tracking listeners `super::events` registers:
    /// re-reads the canvas's CSS box and the device pixel ratio, and, when
    /// either or the backing size changed, resizes the backing store, updates
    /// the tracked size and dispatches a resize followed by a frame request.
    /// A no-op when nothing changed (a `ResizeObserver` reports once on
    /// registration, and a `resize` event fires for scrolls on some mobile
    /// browsers).
    pub(super) fn layout_sync(&self) -> LayoutSync {
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

    pub(super) fn install(
        &self,
        input_latency: flui_platform::InputLatency,
        transfer: &Arc<crate::drag::web::WebTransfer>,
        retire_frame_slot: impl FnOnce() + 'static,
    ) -> Result<(), OpenWindowError> {
        let mut registration = self
            .registration
            .borrow_mut()
            .take()
            .expect("BUG: newly opened window owns its registration");
        let services = super::events::register_event_listeners(
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

    pub(super) fn focus_sync(&self) -> FocusSync {
        FocusSync {
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            observed: Cell::new(self.state.lock().focused),
            publisher: self.publisher(),
        }
    }

    /// The handle by which the Page Lifecycle listeners `super::events`
    /// registers set the window's execution state.
    pub(super) fn execution_sync(&self) -> ExecutionSync {
        ExecutionSync {
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
        }
    }

    pub(super) fn visibility_sync(&self, accepting: Rc<Cell<bool>>) -> VisibilitySync {
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

pub(super) struct VisibilitySync {
    pub(super) document: web_sys::Document,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    observed: Cell<bool>,
    accepting: Rc<Cell<bool>>,
    publisher: StatePublisher,
}

impl VisibilitySync {
    pub(super) fn sync(&self) {
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
pub(super) struct ExecutionSync {
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
}

impl ExecutionSync {
    /// The page is frozen (`Suspended`) or runs again (`Running`). The state
    /// lock is released before the dispatch, which runs embedder callbacks.
    pub(super) fn set(&self, next: WindowExecutionState) {
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
pub(super) struct LayoutSync {
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
    pub(super) fn observe(&self, device: Option<DeviceSize>) {
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
    pub(super) fn sync(&self) {
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
    fn open(
        owner: &WebLoop,
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
            super::events::has_gesture_events(),
        ));
        callbacks.set_input_wake(OwnerSignal::callbacks_wake(&owner.signal, &callbacks));
        let retiring = Arc::clone(&callbacks);
        let state = Arc::downgrade(&owner.state);
        let transfer = crate::drag::web::WebTransfer::of(owner);
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

    fn active(owner: &WebLoop) -> Option<WindowId> {
        owner
            .state
            .lock()
            .window_callbacks
            .as_ref()
            .map(|_| WindowId(0))
    }

    fn capabilities(_: &WebLoop) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(_: &WebLoop, _: f64) -> Size<f64> {
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
        self.drag.borrow().as_ref().map(|drag| {
            Arc::clone(&drag.source) as Arc<dyn flui_platform::drag::PlatformDragSource>
        })
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
