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
    shared::{WindowCallbacks, window_state::WindowStateCell},
    traits::{
        CursorError, OpenWindowError, PlatformWindow, WindowAppearance, WindowBackgroundAppearance,
        WindowCapabilities, WindowCommand, WindowCommandError, WindowGeometry, WindowId,
        WindowMode, WindowState,
    },
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
    display: Option<flui_platform_api::DisplayId>,
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
    let fullscreen = canvas
        .owner_document()
        .is_some_and(|document| document.fullscreen_element().is_some());
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
    focused: bool,
    visible: bool,
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

/// Set the canvas's backing store to `logical × scale`, rounded, so the
/// surface renders one texel per device pixel. Skipped when unchanged:
/// assigning `width`/`height` clears a canvas even to the same value.
fn apply_backing_size(canvas: &web_sys::HtmlCanvasElement, width: f64, height: f64, scale: f64) {
    let phys_width = (width * scale).round() as u32;
    let phys_height = (height * scale).round() as u32;
    if canvas.width() != phys_width {
        canvas.set_width(phys_width);
    }
    if canvas.height() != phys_height {
        canvas.set_height(phys_height);
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
        apply_backing_size(&canvas, width, height, scale_factor);

        // Make canvas focusable for keyboard events
        canvas.set_tab_index(0);

        // Set page title
        document.set_title(title);

        let state = WebWindowState {
            title: title.to_string(),
            width,
            height,
            scale_factor,
            focused: effective_focus(&canvas),
            visible: !document.hidden(),
        };

        let state = Arc::new(Mutex::new(state));
        let initial = sample_state(&canvas, &state, displays.current_display());
        Ok(Self {
            id,
            registration: RefCell::new(Some(Registration::new(&canvas, claim))),
            retire_frame_slot: RefCell::new(None),
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
    /// either changed, resizes the backing store, updates the tracked size
    /// and dispatches a resize followed by a frame request. A no-op when
    /// nothing changed (a `ResizeObserver` reports once on registration,
    /// and a `resize` event fires for scrolls on some mobile browsers).
    pub(super) fn layout_sync(&self) -> LayoutSync {
        LayoutSync {
            canvas: self.canvas.clone(),
            state: Arc::clone(&self.state),
            callbacks: Arc::clone(&self.callbacks),
            publisher: self.publisher(),
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
        retire_frame_slot: impl FnOnce() + 'static,
    ) -> Result<(), OpenWindowError> {
        let mut registration = self
            .registration
            .borrow_mut()
            .take()
            .expect("BUG: newly opened window owns its registration");
        super::events::register_event_listeners(self, &mut registration)?;
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
        crate::shared::panic_boundary::contain_owner_callback(|| drop(old));
    }
}

impl Drop for WebWindow {
    fn drop(&mut self) {
        self.retire(false);
    }
}

pub(super) struct FocusSync {
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    observed: Cell<bool>,
    publisher: StatePublisher,
}

impl FocusSync {
    pub(super) fn sync(&self) {
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

/// See [`WebWindow::layout_sync`].
pub(super) struct LayoutSync {
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    publisher: StatePublisher,
}

impl LayoutSync {
    /// Bring the backing store and the tracked size up to the layout; see
    /// [`WebWindow::layout_sync`]. The state lock is released before the
    /// dispatch, which runs embedder callbacks.
    pub(super) fn sync(&self) {
        let Some((width, height, scale_factor)) = layout_size(&self.canvas) else {
            return;
        };
        let changed = {
            let mut state = self.state.lock();
            let changed = state.width != width
                || state.height != height
                || state.scale_factor != scale_factor;
            state.width = width;
            state.height = height;
            state.scale_factor = scale_factor;
            changed
        };
        if !changed {
            return;
        }
        apply_backing_size(&self.canvas, width, height, scale_factor);
        self.publisher.publish();
        self.callbacks
            .dispatch_resize(Size::new(width, height), scale_factor);
        self.callbacks.dispatch_request_frame();
    }
}

impl crate::traits::HostWindow for WebWindow {}

/// What every browser window honours, known before one exists
/// (`Platform::window_capabilities`).
pub(crate) const fn window_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = true;
    capabilities.enters_fullscreen = true;
    capabilities
}

impl PlatformWindow for WebWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        // Rounded, exactly as `apply_backing_size` sizes the canvas's
        // backing store, so the surface the embedder configures from this
        // answer and the store it renders into agree at fractional device
        // pixel ratios (981 CSS px at 1.5 is 1472, not 1471).
        let state = self.state.lock();
        Size::new(
            (state.width * state.scale_factor).round() as i32,
            (state.height * state.scale_factor).round() as i32,
        )
    }

    fn logical_size(&self) -> Size<f64> {
        let state = self.state.lock();
        Size::new(state.width, state.height)
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        let accepting = self.registration.borrow().is_some();
        if accepting {
            self.callbacks.dispatch_request_frame();
        }
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
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
    fn display_id(&self) -> Option<flui_platform_api::DisplayId> {
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

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.canvas
            .style()
            .set_property("cursor", cursor.name())
            .map_err(|error| CursorError::Backend(format!("{error:?}")))
    }

    // ==================== Callback Registration ====================

    fn on_input(
        &self,
        callback: Box<dyn FnMut(crate::traits::PlatformInput) -> crate::DispatchEventResult + Send>,
    ) {
        self.install_callback(&self.callbacks.on_input, callback);
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_request_frame, callback);
    }
    fn on_resize(&self, callback: Box<dyn FnMut(Size<f64>, f64) + Send>) {
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
        crate::shared::panic_boundary::contain_owner_callback(|| {
            if accepting {
                self.callbacks.set_safe_area_callback(callback);
            } else {
                drop(callback);
            }
        });
    }
    fn on_execution_state_change(
        &self,
        callback: Box<dyn FnMut(crate::WindowExecutionState) + Send>,
    ) {
        let accepting = self.registration.borrow().is_some();
        crate::shared::panic_boundary::contain_owner_callback(|| {
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
