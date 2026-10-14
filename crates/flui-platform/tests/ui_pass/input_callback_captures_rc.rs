//! A window's input callback runs on its owner thread only, so it may capture
//! owner-local state such as an `Rc` (ADR-0082 §4).

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::traits::{DispatchEventResult, PlatformInput, PlatformWindow};

fn register(window: &dyn PlatformWindow) {
    let seen = Rc::new(Cell::new(0_u32));
    window.on_input(Box::new(move |_input: PlatformInput| {
        seen.set(seen.get() + 1);
        DispatchEventResult::default()
    }));
}

fn main() {
    let _ = register;
}
