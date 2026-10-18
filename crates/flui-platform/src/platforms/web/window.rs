//! Web window implementation wrapping a Canvas element

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use parking_lot::Mutex;
use wasm_bindgen::JsCast;

use crate::{
    shared::WindowCallbacks,
    traits::{
        CursorError, OpenWindowError, PlatformDisplay, PlatformWindow, WindowAppearance,
        WindowBackgroundAppearance, WindowBounds, WindowId,
    },
};

use super::{
    display::WebDisplay,
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
    drag: RefCell<Option<super::drag::CanvasDrag>>,
    text: RefCell<Option<Rc<super::text_input::WebTextInput>>>,
}

struct WebWindowState {
    title: String,
    width: f64,
    height: f64,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    fullscreen: bool,
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
    pub fn new(
        id: WindowId,
        title: &str,
        width: f64,
        height: f64,
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
            focused: effective_focus(&canvas),
            visible: !document.hidden(),
            fullscreen: false,
        };

        Ok(Self {
            id,
            registration: RefCell::new(Some(Registration::new(&canvas, claim))),
            retire_frame_slot: RefCell::new(None),
            drag: RefCell::new(None),
            text: RefCell::new(None),
            canvas,
            state: Arc::new(Mutex::new(state)),
            callbacks: Arc::new(WindowCallbacks::new()),
        })
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
        transfer: &Arc<super::drag::WebTransfer>,
        retire_frame_slot: impl FnOnce() + 'static,
    ) -> Result<(), OpenWindowError> {
        let mut registration = self
            .registration
            .borrow_mut()
            .take()
            .expect("BUG: newly opened window owns its registration");
        let services = super::events::register_event_listeners(self, transfer, &mut registration)?;
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
}

impl FocusSync {
    pub(super) fn sync(&self) {
        let focused = effective_focus(&self.canvas);
        if self.observed.replace(focused) == focused {
            return;
        }
        let state = Arc::clone(&self.state);
        self.callbacks
            .dispatch_focus_observation(focused, move || state.lock().focused = focused);
    }
}

pub(super) struct VisibilitySync {
    pub(super) document: web_sys::Document,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    observed: Cell<bool>,
    accepting: Rc<Cell<bool>>,
}

impl VisibilitySync {
    pub(super) fn sync(&self) {
        let visible = !self.document.hidden();
        // A host getter can synchronously retire this registration.
        if !self.accepting.get() || self.observed.replace(visible) == visible {
            return;
        }
        let state = Arc::clone(&self.state);
        self.callbacks
            .dispatch_visibility_observation(visible, move || state.lock().visible = visible);
    }
}

/// See [`WebWindow::layout_sync`].
pub(super) struct LayoutSync {
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
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
        self.callbacks
            .dispatch_resize(Size::new(width, height), scale_factor);
        self.callbacks.dispatch_request_frame();
    }
}

impl crate::traits::HostWindow for WebWindow {
    /// The canvas's `EditContext` or hidden textarea, serving the focused
    /// field's store; `None` once the window closed.
    fn text_store_host(
        &self,
        _owner: crate::traits::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform_api::text_store::TextStoreHost>> {
        self.text
            .borrow()
            .as_ref()
            .map(|text| Rc::clone(text) as Rc<dyn flui_platform_api::text_store::TextStoreHost>)
    }
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

    fn bounds(&self) -> Bounds<f64> {
        let state = self.state.lock();
        Bounds::new(Point::new(0.0, 0.0), Size::new(state.width, state.height))
    }

    fn content_size(&self) -> Size<f64> {
        self.logical_size()
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.state.lock();
        let bounds = Bounds::new(Point::default(), Size::new(state.width, state.height));
        if state.fullscreen {
            WindowBounds::Fullscreen(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.state.lock().fullscreen
    }

    fn is_active(&self) -> bool {
        self.state.lock().focused
    }

    fn appearance(&self) -> WindowAppearance {
        if let Some(w) = web_sys::window()
            && let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)")
            && mql.matches()
        {
            return WindowAppearance::Dark;
        }
        WindowAppearance::Light
    }

    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(WebDisplay::from_browser()))
    }

    fn get_title(&self) -> String {
        self.state.lock().title.clone()
    }

    fn set_title(&self, title: &str) {
        self.state.lock().title = title.to_string();
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            document.set_title(title);
        }
    }

    fn toggle_fullscreen(&self) {
        let is_fullscreen = self.state.lock().fullscreen;
        if is_fullscreen {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                document.exit_fullscreen();
            }
        } else {
            let el: &web_sys::Element = self.canvas.as_ref();
            let _ = el.request_fullscreen();
        }
        self.state.lock().fullscreen = !is_fullscreen;
    }

    fn close(&self) {
        self.retire(true);
    }

    fn set_background_appearance(&self, _appearance: WindowBackgroundAppearance) {
        // Not applicable for web canvas
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform_api::drag::PlatformDragSource>> {
        self.drag.borrow().as_ref().map(|drag| {
            Arc::clone(&drag.source) as Arc<dyn flui_platform_api::drag::PlatformDragSource>
        })
    }

    fn haptics(&self) -> Option<Arc<dyn crate::traits::PlatformHaptics>> {
        Some(Arc::new(super::haptics::WebHaptics))
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
        callback: Box<dyn FnMut(crate::traits::PlatformInput) -> crate::DispatchEventResult>,
    ) {
        if self.registration.borrow().is_some() {
            self.callbacks.set_input_callback(callback);
        } else {
            crate::shared::panic_boundary::contain_owner_callback(|| drop(callback));
        }
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_request_frame, callback);
    }
    fn on_resize(&self, callback: Box<dyn FnMut(Size<f64>, f64) + Send>) {
        self.install_callback(&self.callbacks.on_resize, callback);
    }
    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        self.install_callback(&self.callbacks.on_moved, callback);
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
