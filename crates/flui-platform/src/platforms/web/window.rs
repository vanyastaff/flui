//! Web window implementation wrapping a Canvas element

use std::sync::Arc;

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size, device_px, px};
use parking_lot::Mutex;
use wasm_bindgen::JsCast;

use crate::{
    shared::WindowCallbacks,
    traits::{
        CursorError, DispatchEventResult, PlatformDisplay, PlatformInput, PlatformWindow,
        WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowId,
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

impl WebWindow {
    /// Create a new WebWindow backed by a `<canvas>` element.
    ///
    /// Looks for an existing `<canvas id="flui-canvas">` in the document,
    /// or creates one and appends it to `<body>`.
    pub fn new(id: WindowId, title: &str, width: f32, height: f32) -> anyhow::Result<Self> {
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no global window"))?;
        let document = window
            .document()
            .ok_or_else(|| anyhow::anyhow!("no document"))?;
        let scale_factor = window.device_pixel_ratio();

        // Find existing canvas or create a new one
        let canvas = match document.get_element_by_id("flui-canvas") {
            Some(el) => el
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| anyhow::anyhow!("element 'flui-canvas' is not a canvas"))?,
            None => {
                let canvas = document
                    .create_element("canvas")
                    .map_err(|e| anyhow::anyhow!("failed to create canvas: {e:?}"))?
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .map_err(|_| anyhow::anyhow!("failed to cast to HtmlCanvasElement"))?;
                canvas.set_id("flui-canvas");
                document
                    .body()
                    .ok_or_else(|| anyhow::anyhow!("no body element"))?
                    .append_child(&canvas)
                    .map_err(|e| anyhow::anyhow!("failed to append canvas: {e:?}"))?;
                canvas
            }
        };

        // Set physical size (sharp rendering on HiDPI)
        let phys_width = (width * scale_factor as f32) as u32;
        let phys_height = (height * scale_factor as f32) as u32;
        canvas.set_width(phys_width);
        canvas.set_height(phys_height);

        // Set CSS logical size
        let style = canvas.style();
        let _ = style.set_property("width", &format!("{width}px"));
        let _ = style.set_property("height", &format!("{height}px"));

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

    /// Get a reference to the underlying canvas element
    pub fn canvas(&self) -> &web_sys::HtmlCanvasElement {
        &self.canvas
    }

    /// Get window callbacks for event dispatch
    pub fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }

    /// Update tracked size (called from resize observer / events)
    pub fn update_size(&self, width: f32, height: f32) {
        let mut state = self.state.lock();
        state.width = width;
        state.height = height;
    }

    /// Update focus state (called from focus/blur events)
    pub fn update_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
    }
}

impl PlatformWindow for WebWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        Size::new(
            device_px((state.width * state.scale_factor as f32) as i32),
            device_px((state.height * state.scale_factor as f32) as i32),
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
        if let Some(w) = web_sys::window() {
            if let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)") {
                if mql.matches() {
                    return WindowAppearance::Dark;
                }
            }
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

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        *self.callbacks.on_input.lock() = Some(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_request_frame.lock() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        *self.callbacks.on_resize.lock() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_moved.lock() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.callbacks.on_close.lock() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        *self.callbacks.on_should_close.lock() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_active_status_change.lock() = Some(callback);
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_visibility_status_change.lock() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_appearance_changed.lock() = Some(callback);
    }

    // ==================== GPU Surface Handles ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{RawWindowHandle, WebCanvasWindowHandle, WindowHandle};

        // WebCanvasWindowHandle expects a NonNull<c_void> pointer to the canvas object
        let obj: &wasm_bindgen::JsValue = self.canvas.as_ref();
        let ptr =
            std::ptr::NonNull::new(obj as *const wasm_bindgen::JsValue as *mut std::ffi::c_void)
                .expect("canvas JsValue pointer is null");
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
}
