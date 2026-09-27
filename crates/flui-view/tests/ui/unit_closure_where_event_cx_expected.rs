//! ADR-0086: an event closure takes the `&mut EventCx` it is dispatched
//! with; a closure without the parameter is refused where one is expected.

use flui_view::prelude::*;

fn wire(count: Signal<u32>) {
    let _press = callback(move || {
        let _ = count;
    });
}

fn main() {}
