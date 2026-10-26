//! What a canvas's listeners read of a DOM event and how they answer it, for
//! every feature that listens on the canvas (`input::web`, `drag::web`).

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
