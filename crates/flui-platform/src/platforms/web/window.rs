//! Web window implementation wrapping a Canvas element

use std::sync::Arc;

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size, device_px, px};
use parking_lot::Mutex;
use wasm_bindgen::JsCast;

use crate::{
    shared::WindowCallbacks,
    traits::{
        CursorError, OpenWindowError, PlatformDisplay, PlatformWindow, WindowAppearance,
        WindowBackgroundAppearance, WindowBounds, WindowId,
    },
};

use super::display::WebDisplay;

/// Web window wrapping a `<canvas>` element
pub struct WebWindow {
    id: WindowId,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
}

struct WebWindowState {
    title: String,
    width: f32,
    height: f32,
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
fn layout_size(canvas: &web_sys::HtmlCanvasElement) -> Option<(f32, f32, f64)> {
    let window = web_sys::window()?;
    let width = canvas.client_width();
    let height = canvas.client_height();
    (width > 0 && height > 0).then(|| (width as f32, height as f32, window.device_pixel_ratio()))
}

/// Set the canvas's backing store to `logical × scale`, rounded, so the
/// surface renders one texel per device pixel. Skipped when unchanged:
/// assigning `width`/`height` clears a canvas even to the same value.
fn apply_backing_size(canvas: &web_sys::HtmlCanvasElement, width: f32, height: f32, scale: f64) {
    let phys_width = (f64::from(width) * scale).round() as u32;
    let phys_height = (f64::from(height) * scale).round() as u32;
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
        width: f32,
        height: f32,
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
            focused: true,
            visible: true,
            fullscreen: false,
        };

        Ok(Self {
            id,
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
            .dispatch_resize(Size::new(px(width), px(height)), scale_factor as f32);
        self.callbacks.dispatch_request_frame();
    }
}

impl WebWindow {
    /// Update focus state (called from focus/blur events)
    // Unused until the web backend subscribes to focus/blur on the canvas.
    #[expect(dead_code)]
    pub fn update_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
    }
}

impl crate::traits::HostWindow for WebWindow {}

impl PlatformWindow for WebWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        // Rounded, exactly as `apply_backing_size` sizes the canvas's
        // backing store, so the surface the embedder configures from this
        // answer and the store it renders into agree at fractional device
        // pixel ratios (981 CSS px at 1.5 is 1472, not 1471).
        let state = self.state.lock();
        Size::new(
            device_px((f64::from(state.width) * state.scale_factor).round() as i32),
            device_px((f64::from(state.height) * state.scale_factor).round() as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        Size::new(px(state.width), px(state.height))
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        self.callbacks.dispatch_request_frame();
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let state = self.state.lock();
        Bounds::new(
            Point::new(px(0.0), px(0.0)),
            Size::new(px(state.width), px(state.height)),
        )
    }

    fn content_size(&self) -> Size<Pixels> {
        self.logical_size()
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.state.lock();
        let bounds = Bounds::new(
            Point::default(),
            Size::new(px(state.width), px(state.height)),
        );
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
        self.callbacks.dispatch_close();
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

    crate::shared::impl_window_callback_setters!(callbacks);

    // ==================== GPU Surface Handles ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
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
