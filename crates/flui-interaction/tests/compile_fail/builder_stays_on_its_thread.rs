use std::{cell::Cell, rc::Rc};

use flui_interaction::{GestureArena, TapGestureRecognizer};

fn main() {
    let taps = Rc::new(Cell::new(0));
    let builder =
        TapGestureRecognizer::builder(GestureArena::new()).on_tap(move |_| taps.set(taps.get() + 1));
    std::thread::spawn(move || drop(builder));
}
