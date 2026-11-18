//! What a canvas's listeners read of a DOM event and of the page it reaches
//! (the pointer lock), and how they answer it, for every feature that listens
//! on the canvas (`input::web`, `drag::web`, `window::web`).

use flui_foundation::geometry::Point;
use flui_platform::DispatchEventResult;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// CSSOM View defines these offsets as doubles. web-sys exposes the older
// integer signature, which discards subpixel positions before Rust receives
// them. Bind the actual DOM getters with their floating-point result type.
#[wasm_bindgen]
extern "C" {
    type PreciseMouseEvent;
    #[wasm_bindgen(method, getter, js_name = offsetX)]
    fn offset_x_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = offsetY)]
    fn offset_y_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = clientX)]
    fn client_x_f64(this: &PreciseMouseEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = clientY)]
    fn client_y_f64(this: &PreciseMouseEvent) -> f64;
}

/// The event's position on its target's padding edge, with its subpixels.
pub(crate) fn pointer_position(event: &web_sys::MouseEvent) -> Point {
    let event: &PreciseMouseEvent = event.unchecked_ref();
    Point::new(event.offset_x_f64(), event.offset_y_f64())
}

/// The event's position in the viewport, with its subpixels.
pub(crate) fn client_position(event: &web_sys::MouseEvent) -> Point {
    let event: &PreciseMouseEvent = event.unchecked_ref();
    Point::new(event.client_x_f64(), event.client_y_f64())
}

/// Whether the page's pointer lock is `canvas`'s: the browser grants it to one
/// element at a time and names it on the document. What the window's
/// snapshot reports as its grab, and what makes the input read a mouse event
/// as the locked pointer's motion.
pub(crate) fn holds_pointer_lock(canvas: &web_sys::HtmlCanvasElement) -> bool {
    let canvas: &web_sys::Element = canvas.as_ref();
    canvas
        .owner_document()
        .and_then(|document| document.pointer_lock_element())
        .is_some_and(|locked| locked == *canvas)
}

/// Apply a dispatch result to the DOM event it answers, synchronously, while
/// the browser still waits on it: `propagate: false` stops it reaching the
/// canvas's ancestors (same-target listeners still run), and
/// `default_prevented` cancels its default where it is cancelable.
pub(crate) fn answer(event: &web_sys::Event, result: DispatchEventResult) {
    if !result.propagate {
        event.stop_propagation();
    }
    if result.default_prevented {
        event.prevent_default();
    }
}
