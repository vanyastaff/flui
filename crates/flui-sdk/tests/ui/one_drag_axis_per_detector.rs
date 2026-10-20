//! A detector's drag families cannot compete: its one drag slot picks its
//! axis when it is made, and there is no per-family callback beside it to
//! configure a second drag on the same detector.
use flui_sdk::widgets::{DragSlot, GestureDetector};

fn main() {
    let _ = DragSlot::vertical().horizontal();
    let _ = GestureDetector::new()
        .drag(DragSlot::pan())
        .on_vertical_drag_update(|_| {});
}
